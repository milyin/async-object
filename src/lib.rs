use futures::{Future, Stream};
use std::{
    any::{Any, TypeId},
    collections::{HashMap, VecDeque},
    fmt::Debug,
    marker::PhantomData,
    pin::Pin,
    sync::{Arc, RwLock, Weak},
    task::{Context, Poll, Waker},
};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Object not exists")]
    ObjectNotExists,
}

struct Subscribers {
    subscribers: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl Subscribers {
    pub fn new() -> Self {
        Self {
            subscribers: HashMap::new(),
        }
    }
    pub fn subscribe<EVT: Send + Sync + Clone + 'static>(
        &mut self,
        event_queue: Weak<RwLock<EventQueue<EVT>>>,
    ) {
        let event_subscribers = self
            .subscribers
            .entry(TypeId::of::<EVT>())
            .or_insert(Box::new(EventSubscribers::<EVT>::new()) as Box<dyn Any + Send + Sync>);
        let event_subscibers = event_subscribers
            .downcast_mut::<EventSubscribers<EVT>>()
            .unwrap();
        event_subscibers.subscribe(event_queue)
    }
    pub fn send_event<EVT: Send + Sync + Clone + 'static>(&mut self, event: EVT) {
        if let Some(event_subscribers) = self.subscribers.get_mut(&TypeId::of::<EVT>()) {
            let event_subscribers = event_subscribers
                .downcast_mut::<EventSubscribers<EVT>>()
                .unwrap();
            event_subscribers.send_event(event)
        }
    }
}
pub struct EventSubscribers<EVT: Send + Sync + Clone> {
    event_subscribers: Vec<Weak<RwLock<EventQueue<EVT>>>>,
}

impl<EVT: Send + Sync + Clone> Drop for EventSubscribers<EVT> {
    fn drop(&mut self) {
        self.event_subscribers.iter().for_each(|w| {
            w.upgrade().map(|w| w.write().ok().map(|mut w| w.detach()));
        })
    }
}

impl<EVT: Send + Sync + Clone> EventSubscribers<EVT> {
    pub fn new() -> Self {
        Self {
            event_subscribers: Vec::new(),
        }
    }
    pub fn count(&self) -> usize {
        self.event_subscribers
            .iter()
            .filter(|w| w.strong_count() > 0)
            .count()
    }
    fn subscribe(&mut self, event_queue: Weak<RwLock<EventQueue<EVT>>>) {
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
    pub fn send_event(&mut self, event: EVT) {
        self.event_subscribers
            .iter()
            .filter_map(|event_queue| event_queue.upgrade())
            .for_each(|event_queue| {
                event_queue.write().unwrap().send_event(event.clone());
            });
    }
}
pub struct EventQueue<EVT: Send + Sync> {
    detached: bool,
    waker: Option<Waker>,
    events: VecDeque<Box<EVT>>,
}

impl<EVT: Send + Sync> EventQueue<EVT> {
    fn new() -> Self {
        Self {
            detached: false,
            waker: None,
            events: VecDeque::new(),
        }
    }
    fn detach(&mut self) {
        self.detached = true;
        self.waker.take().map(|w| w.wake());
    }
    fn send_event(&mut self, event: EVT) {
        self.events.push_back(Box::new(event));
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
    fn get_event(&mut self) -> Option<EVT> {
        self.events.pop_front().map(|e| *e)
    }
}

struct EventStream<EVT: Send + Sync + Clone + 'static> {
    event_queue: Arc<RwLock<EventQueue<EVT>>>,
}

impl<EVT: Send + Sync + Clone + 'static> EventStream<EVT> {
    pub fn new<T>(handle: Handle<T>) -> Self {
        let event_queue = Arc::new(RwLock::new(EventQueue::new()));
        let weak_event_queue = Arc::downgrade(&mut (event_queue.clone()));
        handle.subscribe(weak_event_queue);
        Self { event_queue }
    }
    fn poll_next(self: &mut Self, cx: &mut Context<'_>) -> Poll<Option<EVT>> {
        let mut event_queue = self.event_queue.write().unwrap();
        if event_queue.detached {
            if let Some(evt) = event_queue.get_event() {
                Poll::Ready(Some(evt))
            } else {
                Poll::Ready(None)
            }
        } else {
            if let Some(evt) = event_queue.get_event() {
                Poll::Ready(Some(evt))
            } else {
                event_queue.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

impl<EVT: Send + Sync + Clone + 'static> Stream for EventStream<EVT> {
    type Item = EVT;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<EVT>> {
        self.get_mut().poll_next(cx)
    }
}
enum Either<F, FMut> {
    F(F),
    Fmut(FMut),
}

struct HandleCall<T: 'static, R, F, FMut>
where
    F: FnOnce(&T) -> R,
    FMut: FnOnce(&mut T) -> R,
{
    handle: Handle<T>,
    func: Option<Either<Box<F>, Box<FMut>>>,
    _phantom: PhantomData<Box<(T, R)>>,
}

fn new_handle_call<T, R, F: FnOnce(&T) -> R>(
    handle: Handle<T>,
    f: F,
) -> HandleCall<T, R, F, fn(&mut T) -> R> {
    HandleCall::new(handle, f)
}
fn new_handle_call_mut<T, R, FMut: FnOnce(&mut T) -> R>(
    handle: Handle<T>,
    f: FMut,
) -> HandleCall<T, R, fn(&T) -> R, FMut> {
    HandleCall::new_mut(handle, f)
}

impl<T: 'static, R, F, FMut> HandleCall<T, R, F, FMut>
where
    F: FnOnce(&T) -> R,
    FMut: FnOnce(&mut T) -> R,
{
    fn new(handle: Handle<T>, func: F) -> Self {
        Self {
            handle,
            func: Some(Either::F(Box::new(func))),
            _phantom: PhantomData,
        }
    }
    fn new_mut(handle: Handle<T>, func: FMut) -> Self {
        Self {
            handle,
            func: Some(Either::Fmut(Box::new(func))),
            _phantom: PhantomData,
        }
    }
    fn poll(&mut self, cx: &Context) -> Poll<Result<R, Error>> {
        if let Some(object) = self.handle.object.upgrade() {
            self.handle.add_call_waker(cx.waker().clone());
            let res = match self.func.take().unwrap() {
                Either::F(func) => {
                    if let Ok(object) = object.try_read() {
                        let r = func(&*object);
                        Poll::Ready(Ok(r))
                    } else {
                        self.func = Some(Either::F(func));
                        Poll::Pending
                    }
                }
                Either::Fmut(func_mut) => {
                    if let Ok(mut object) = object.try_write() {
                        let r = func_mut(&mut *object);
                        Poll::Ready(Ok(r))
                    } else {
                        self.func = Some(Either::Fmut(func_mut));
                        Poll::Pending
                    }
                }
            };
            if res.is_ready() {
                self.handle.wake_calls();
            }
            res
        } else {
            Poll::Ready(Err(Error::ObjectNotExists))
        }
    }
}

impl<T: Any, R, F: FnOnce(&T) -> R, FMut: FnOnce(&mut T) -> R> Future
    for HandleCall<T, R, F, FMut>
{
    type Output = Result<R, Error>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.get_mut().poll(cx)
    }
}

pub struct Handle<T: 'static> {
    object: Weak<RwLock<T>>,
    subscribers: Weak<RwLock<Subscribers>>,
    call_wakers: Weak<RwLock<Vec<Waker>>>,
}

impl<T: 'static> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Self {
            object: self.object.clone(),
            subscribers: self.subscribers.clone(),
            call_wakers: self.call_wakers.clone(),
        }
    }
}

impl<T: 'static> Handle<T> {
    fn new(
        object: Weak<RwLock<T>>,
        subscribers: Weak<RwLock<Subscribers>>,
        call_wakers: Weak<RwLock<Vec<Waker>>>,
    ) -> Self {
        Self {
            object,
            subscribers,
            call_wakers,
        }
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

    pub fn call<R, F: FnOnce(&T) -> R>(&self, f: F) -> impl Future<Output = Result<R, Error>> {
        new_handle_call(self.clone(), f)
    }
    pub fn call_mut<R, F: FnOnce(&mut T) -> R>(
        &self,
        f: F,
    ) -> impl Future<Output = Result<R, Error>> {
        new_handle_call_mut(self.clone(), f)
    }
    fn subscribe<EVT: Send + Sync + Clone + 'static>(
        &self,
        event_queue: Weak<RwLock<EventQueue<EVT>>>,
    ) {
        if let Some(subscribers) = self.subscribers.upgrade() {
            subscribers.write().unwrap().subscribe(event_queue)
        }
    }
    pub fn receive_events<EVT: Send + Sync + Clone + 'static>(&self) -> impl Stream<Item = EVT> {
        EventStream::new(self.clone())
    }
    pub fn send_event<EVT: Send + Sync + Clone + 'static>(&self, event: EVT) {
        if let Some(subscribers) = self.subscribers.upgrade() {
            subscribers.write().unwrap().send_event(event)
        }
    }
}

pub struct Keeper<T> {
    subscribers: Arc<RwLock<Subscribers>>,
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

impl<T> Keeper<T> {
    pub fn new(object: T) -> Self {
        Self {
            subscribers: Arc::new(RwLock::new(Subscribers::new())),
            call_wakers: Arc::new(RwLock::new(Vec::new())),
            object: Arc::new(RwLock::new(object)),
        }
    }
    pub fn handle(&self) -> Handle<T> {
        let object = Arc::downgrade(&self.object);
        let subscribers = Arc::downgrade(&self.subscribers);
        let call_wakers = Arc::downgrade(&self.call_wakers);
        Handle::new(object, subscribers, call_wakers)
    }
}

impl<T> AsRef<Arc<RwLock<T>>> for Keeper<T> {
    fn as_ref(&self) -> &Arc<RwLock<T>> {
        &self.object
    }
}
