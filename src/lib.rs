use futures::{
    task::{Spawn, SpawnError, SpawnExt},
    Future, Stream, StreamExt,
};
use std::{
    any::{Any, TypeId},
    collections::{HashMap, VecDeque},
    marker::PhantomData,
    pin::Pin,
    sync::{Arc, RwLock, Weak},
    task::{Context, Poll, Waker},
};

struct EventQueue<EVT: Send + Sync> {
    detached: bool,
    waker: Option<Waker>,
    events: VecDeque<EVT>,
}

impl<EVT: Send + Sync> EventQueue<EVT> {
    fn new() -> Self {
        Self {
            detached: false,
            waker: None,
            events: VecDeque::new(),
        }
    }
    fn is_detached(&self) -> bool {
        self.detached
    }
    fn detach(&mut self) {
        self.detached = true;
        self.wake();
    }
    fn wake(&mut self) {
        self.waker.take().map(|w| w.wake());
    }
    fn set_waker(&mut self, waker: Waker) {
        self.waker = Some(waker)
    }
    fn send_event(&mut self, event: EVT) {
        self.events.push_back(event);
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
    fn get_event(&mut self) -> Option<EVT> {
        self.events.pop_front()
    }
}

pub struct EventBox {
    event_id: TypeId,
    event: Box<dyn Any + Send + Sync>,
    waker: Option<Waker>,
}

impl Drop for EventBox {
    fn drop(&mut self) {
        if let Some(waker) = self.waker.take() {
            waker.wake()
        }
    }
}

impl EventBox {
    fn new(event_id: TypeId, event: Box<dyn Any + Send + Sync>, waker: Waker) -> Self {
        Self {
            event_id,
            event,
            waker: Some(waker),
        }
    }
    pub fn get_event_id(&self) -> TypeId {
        self.event_id
    }
    pub fn get_event<EVT: 'static + Send + Sync>(&self) -> Option<&EVT> {
        self.event.downcast_ref()
    }
}

type EventBoxQueue = EventQueue<Arc<EventBox>>;
type PendingQueue = EventQueue<(TypeId, Box<dyn Any + Send + Sync>)>;

struct Keeper<T> {
    subscribers: Arc<RwLock<Subscribers>>,
    pending_queue: Arc<RwLock<PendingQueue>>,
    pending_event: Weak<RwLock<EventBox>>,
    call_wakers: Arc<RwLock<Vec<Waker>>>,
    object: Arc<RwLock<T>>,
}

impl<T> Drop for Keeper<T> {
    fn drop(&mut self) {
        self.call_wakers
            .write()
            .unwrap()
            .drain(..)
            .for_each(|w| w.wake());
    }
}

fn drain_wakers(wakers: &Arc<RwLock<Vec<Waker>>>) {
    wakers.write().unwrap().drain(..).for_each(|w| w.wake());
}

impl<T> Keeper<T> {
    fn new(object: T) -> Self {
        Self {
            subscribers: Arc::new(RwLock::new(Subscribers::new())),
            pending_queue: Arc::new(RwLock::new(EventQueue::new())),
            pending_event: Weak::new(),
            call_wakers: Arc::new(RwLock::new(Vec::new())),
            object: Arc::new(RwLock::new(object)),
        }
    }
    fn tag(&self) -> Tag<T> {
        let object = Arc::downgrade(&self.object);
        let pending_queue = Arc::downgrade(&self.pending_queue);
        let subscribers = Arc::downgrade(&self.subscribers);
        let call_wakers = Arc::downgrade(&self.call_wakers);
        Tag::new(object, pending_queue, subscribers, call_wakers)
    }
    fn poll_next(self: &mut Self, cx: &mut Context<'_>) -> Poll<Option<Arc<EventBox>>> {
        if let Some(pending_event) = self.pending_event.upgrade() {
            let mut pending_event = pending_event.write().unwrap();
            pending_event.waker = Some(cx.waker().clone());
            return Poll::Pending;
        }
        let event = self.pending_queue.write().unwrap().get_event();
        if let Some((event_id, event)) = event {
            let waker = cx.waker().clone();
            Poll::Ready(Some(Arc::new(EventBox::new(event_id, event, waker))))
        } else {
            let weak_count = Arc::weak_count(&self.pending_queue);
            if weak_count == 0 {
                Poll::Ready(None)
            } else {
                self.pending_queue
                    .write()
                    .unwrap()
                    .set_waker(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

impl<T> AsRef<Arc<RwLock<T>>> for Keeper<T> {
    fn as_ref(&self) -> &Arc<RwLock<T>> {
        &self.object
    }
}

impl<T> Stream for Keeper<T> {
    type Item = Arc<EventBox>;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Arc<EventBox>>> {
        self.get_mut().poll_next(cx)
    }
}

pub fn run<T: Send + Sync + 'static>(pool: impl Spawn, object: T) -> Result<Tag<T>, SpawnError> {
    let mut object = Keeper::new(object);
    let tag = object.tag();
    pool.spawn(async move {
        while let Some(event) = object.next().await {
            object.subscribers.write().unwrap().send_event(event);
        }
    })?;
    Ok(tag)
}

struct Subscribers {
    subscribers: HashMap<TypeId, EventSubscribers>,
}

impl Subscribers {
    pub fn new() -> Self {
        Self {
            subscribers: HashMap::new(),
        }
    }
    pub fn subscribe(&mut self, event_id: TypeId, event_queue: Weak<RwLock<EventBoxQueue>>) {
        let event_subscribers = self
            .subscribers
            .entry(event_id)
            .or_insert(EventSubscribers::new());
        event_subscribers.subscribe(event_queue)
    }
    pub fn send_event(&mut self, event: Arc<EventBox>) {
        if let Some(event_subscribers) = self.subscribers.get_mut(&event.get_event_id()) {
            event_subscribers.send_event(event)
        }
    }
}
struct EventSubscribers {
    event_subscribers: Vec<Weak<RwLock<EventBoxQueue>>>,
}

impl Drop for EventSubscribers {
    fn drop(&mut self) {
        self.event_subscribers.iter().for_each(|w| {
            w.upgrade().map(|w| w.write().ok().map(|mut w| w.detach()));
        })
    }
}

impl EventSubscribers {
    fn new() -> Self {
        Self {
            event_subscribers: Vec::new(),
        }
    }
    #[allow(dead_code)]
    fn count(&self) -> usize {
        self.event_subscribers
            .iter()
            .filter(|w| w.strong_count() > 0)
            .count()
    }
    fn subscribe(&mut self, event_queue: Weak<RwLock<EventBoxQueue>>) {
        if let Some(empty) = self
            .event_subscribers
            .iter_mut()
            .find(|w| w.strong_count() == 0)
        {
            *empty = event_queue;
        } else {
            self.event_subscribers.push(event_queue);
        };
    }
    fn send_event(&mut self, event: Arc<EventBox>) {
        self.event_subscribers
            .iter()
            .filter_map(|event_queue| event_queue.upgrade())
            .for_each(|event_queue| {
                event_queue.write().unwrap().send_event(event.clone());
            });
    }
}

pub struct Event<EVT: 'static + Send + Sync> {
    event_box: Arc<EventBox>,
    _phantom: PhantomData<EVT>,
}

impl<EVT: 'static + Send + Sync> Event<EVT> {
    pub fn new(event_box: Arc<EventBox>) -> Self {
        Self {
            event_box,
            _phantom: PhantomData,
        }
    }
}

impl<EVT: 'static + Send + Sync> AsRef<EVT> for Event<EVT> {
    fn as_ref(&self) -> &EVT {
        &self.event_box.get_event().unwrap()
    }
}

pub struct EventStream<EVT: Send + Sync + 'static> {
    event_queue: Arc<RwLock<EventBoxQueue>>,
    _phantom: PhantomData<EVT>,
}

impl<EVT: Send + Sync + 'static> EventStream<EVT> {
    pub fn new<T>(tag: Tag<T>) -> Self {
        let event_queue = Arc::new(RwLock::new(EventBoxQueue::new()));
        let weak_event_queue = Arc::downgrade(&mut (event_queue.clone()));
        tag.subscribe(TypeId::of::<EVT>().into(), weak_event_queue);
        Self {
            event_queue,
            _phantom: PhantomData,
        }
    }
    fn poll_next(self: &mut Self, cx: &mut Context<'_>) -> Poll<Option<Event<EVT>>> {
        let mut event_queue = self.event_queue.write().unwrap();
        if event_queue.is_detached() {
            Poll::Ready(event_queue.get_event().map(|v| Event::new(v)))
        } else {
            if let Some(evt) = event_queue.get_event() {
                Poll::Ready(Some(Event::new(evt)))
            } else {
                event_queue.set_waker(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

impl<EVT: Send + Sync + Unpin + 'static> Stream for EventStream<EVT> {
    type Item = Event<EVT>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().poll_next(cx)
    }
}
enum Either<F, FMut> {
    F(F),
    Fmut(FMut),
}

struct AsyncCall<T: 'static, R, F, FMut>
where
    F: FnOnce(&T) -> R,
    FMut: FnOnce(&mut T) -> R,
{
    tag: Tag<T>,
    func: Option<Either<Box<F>, Box<FMut>>>,
    _phantom: PhantomData<Box<(T, R)>>,
}

fn new_async_call<T, R, F: FnOnce(&T) -> R>(
    tag: Tag<T>,
    f: F,
) -> AsyncCall<T, R, F, fn(&mut T) -> R> {
    AsyncCall::new(tag, f)
}

fn new_async_call_mut<T, R, FMut: FnOnce(&mut T) -> R>(
    tag: Tag<T>,
    f: FMut,
) -> AsyncCall<T, R, fn(&T) -> R, FMut> {
    AsyncCall::new_mut(tag, f)
}

impl<T: 'static, R, F, FMut> AsyncCall<T, R, F, FMut>
where
    F: FnOnce(&T) -> R,
    FMut: FnOnce(&mut T) -> R,
{
    fn new(tag: Tag<T>, func: F) -> Self {
        Self {
            tag,
            func: Some(Either::F(Box::new(func))),
            _phantom: PhantomData,
        }
    }
    fn new_mut(tag: Tag<T>, func: FMut) -> Self {
        Self {
            tag,
            func: Some(Either::Fmut(Box::new(func))),
            _phantom: PhantomData,
        }
    }
    fn poll(&mut self, cx: &Context) -> Poll<Option<R>> {
        if let Some(object) = self.tag.object.upgrade() {
            self.tag.add_call_waker(cx.waker().clone());
            let res = match self.func.take().unwrap() {
                Either::F(func) => {
                    if let Ok(object) = object.try_read() {
                        let r = func(&*object);
                        Poll::Ready(Some(r))
                    } else {
                        self.func = Some(Either::F(func));
                        Poll::Pending
                    }
                }
                Either::Fmut(func_mut) => {
                    if let Ok(mut object) = object.try_write() {
                        let r = func_mut(&mut *object);
                        Poll::Ready(Some(r))
                    } else {
                        self.func = Some(Either::Fmut(func_mut));
                        Poll::Pending
                    }
                }
            };
            if res.is_ready() {
                self.tag.wake_calls();
            }
            res
        } else {
            Poll::Ready(None)
        }
    }
}

impl<T: Any, R, F: FnOnce(&T) -> R, FMut: FnOnce(&mut T) -> R> Future for AsyncCall<T, R, F, FMut> {
    type Output = Option<R>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.get_mut().poll(cx)
    }
}

pub struct Tag<T: 'static> {
    object: Weak<RwLock<T>>,
    pending_queue: Weak<RwLock<PendingQueue>>,
    subscribers: Weak<RwLock<Subscribers>>,
    call_wakers: Weak<RwLock<Vec<Waker>>>,
}

impl<T> Default for Tag<T> {
    fn default() -> Self {
        Self {
            object: Default::default(),
            pending_queue: Default::default(),
            subscribers: Default::default(),
            call_wakers: Default::default(),
        }
    }
}

impl<T: 'static> Clone for Tag<T> {
    fn clone(&self) -> Self {
        Self {
            object: self.object.clone(),
            pending_queue: self.pending_queue.clone(),
            subscribers: self.subscribers.clone(),
            call_wakers: self.call_wakers.clone(),
        }
    }
}

impl<T: 'static> PartialEq for Tag<T> {
    fn eq(&self, other: &Self) -> bool {
        self.object.ptr_eq(&other.object)
    }
}

impl<T> Drop for Tag<T> {
    fn drop(&mut self) {
        if let Some(pending_queue) = self.pending_queue.upgrade() {
            self.pending_queue = Weak::new(); // drop weak reference before waking keeper's poll_next
            pending_queue.write().unwrap().wake();
        }
    }
}

impl<T: 'static> Tag<T> {
    fn new(
        object: Weak<RwLock<T>>,
        pending_queue: Weak<RwLock<PendingQueue>>,
        subscribers: Weak<RwLock<Subscribers>>,
        call_wakers: Weak<RwLock<Vec<Waker>>>,
    ) -> Self {
        Self {
            object,
            pending_queue,
            subscribers,
            call_wakers,
        }
    }
    pub fn is_valid(&self) -> bool {
        self.object.strong_count() > 0
    }
    fn add_call_waker(&self, waker: Waker) {
        let wakers = self.call_wakers.upgrade().unwrap();
        let mut wakers = wakers.write().unwrap();
        wakers.push(waker);
    }
    fn wake_calls(&self) {
        let wakers = self.call_wakers.upgrade().unwrap();
        let mut wakers = wakers.write().unwrap();
        wakers.drain(..).for_each(|w| w.wake());
    }

    pub fn async_read<R, F: FnOnce(&T) -> R>(&self, f: F) -> impl Future<Output = Option<R>> {
        new_async_call(self.clone(), f)
    }
    pub fn async_write<R, F: FnOnce(&mut T) -> R>(&self, f: F) -> impl Future<Output = Option<R>> {
        new_async_call_mut(self.clone(), f)
    }
    pub fn read<R, F: FnOnce(&T) -> R>(&self, f: F) -> Option<R> {
        if let (Some(object), Some(wakers)) = (self.object.upgrade(), self.call_wakers.upgrade()) {
            let v = f(&*object.read().unwrap());
            drain_wakers(&wakers);
            Some(v)
        } else {
            None
        }
    }
    pub fn write<R, F: FnOnce(&mut T) -> R>(&self, f: F) -> Option<R> {
        if let (Some(object), Some(wakers)) = (self.object.upgrade(), self.call_wakers.upgrade()) {
            let v = f(&mut *object.write().unwrap());
            drain_wakers(&wakers);
            Some(v)
        } else {
            None
        }
    }
    fn subscribe(&self, event_id: TypeId, event_queue: Weak<RwLock<EventBoxQueue>>) {
        if let Some(subscribers) = self.subscribers.upgrade() {
            subscribers
                .write()
                .unwrap()
                .subscribe(event_id, event_queue)
        }
    }
    pub fn send_event<EVT: Send + Sync + 'static>(&self, event: EVT) {
        if let Some(pending_queue) = self.pending_queue.upgrade() {
            pending_queue
                .write()
                .unwrap()
                .send_event((TypeId::of::<EVT>(), Box::new(event)))
        }
    }
}
