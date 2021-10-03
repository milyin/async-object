use futures::{Future, Stream};
use std::{
    any::Any,
    collections::VecDeque,
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

pub struct EventSubscribers {
    subscribers: Vec<Weak<RwLock<EventQueue>>>,
}

impl Drop for EventSubscribers {
    fn drop(&mut self) {
        self.subscribers.iter().for_each(|w| {
            w.upgrade().map(|w| w.write().ok().map(|mut w| w.detach()));
        })
    }
}

impl EventSubscribers {
    pub fn new() -> Self {
        Self {
            subscribers: Vec::new(),
        }
    }
    fn subscribe(&mut self, event_queue: Weak<RwLock<EventQueue>>) {
        if let Some(empty) = self.subscribers.iter_mut().find(|w| w.strong_count() == 0) {
            *empty = event_queue;
        } else {
            self.subscribers.push(event_queue);
        };
    }
    pub fn send_event<EVT: Any + Send + Sync + Clone + 'static>(&mut self, event: EVT) {
        self.subscribers
            .iter()
            .filter_map(|event_queue| event_queue.upgrade())
            .for_each(|event_queue| {
                event_queue.write().unwrap().send_event(event.clone());
            });
    }
}

impl Debug for EventSubscribers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.subscribers.iter().map(|v| v.upgrade()))
            .finish()
    }
}

pub struct EventQueue {
    detached: bool,
    waker: Option<Waker>,
    events: VecDeque<Box<dyn Any + Send + Sync + 'static>>,
}

impl EventQueue {
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
    fn send_event<EVT: Any + Send + Sync + 'static>(&mut self, event: EVT) {
        self.events.push_back(Box::new(event));
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
    fn get_event<EVT: Any>(&mut self) -> Option<EVT> {
        self.events.pop_front().map(|evt| *evt.downcast().unwrap())
    }
}

impl Debug for EventQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventQueue")
            .field("waker", &self.waker)
            .field("events", &self.events.len())
            .finish()
    }
}

struct EventSource<EVT: Any + Send + Sync> {
    event_queue: Arc<RwLock<EventQueue>>,
    _phantom: PhantomData<Box<EVT>>,
}

impl<EVT: Any + Send + Sync> EventSource<EVT> {
    fn new<T>(handle: Handle<T>) -> Self {
        let event_queue = Arc::new(RwLock::new(EventQueue::new()));
        let weak_event_queue = Arc::downgrade(&mut (event_queue.clone()));
        handle.subscribe(weak_event_queue);
        Self {
            event_queue,
            _phantom: PhantomData,
        }
    }
    fn poll_next(self: &mut Self, cx: &mut Context<'_>) -> Poll<Option<EVT>> {
        let mut event_queue = self.event_queue.write().unwrap();
        if event_queue.detached {
            if let Some(evt) = event_queue.get_event::<EVT>() {
                Poll::Ready(Some(evt))
            } else {
                Poll::Ready(None)
            }
        } else {
            event_queue.waker = Some(cx.waker().clone());
            if let Some(evt) = event_queue.get_event::<EVT>() {
                Poll::Ready(Some(evt))
            } else {
                Poll::Pending
            }
        }
    }
}

impl<EVT: Any + Send + Sync> Stream for EventSource<EVT> {
    type Item = EVT;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<EVT>> {
        self.get_mut().poll_next(cx)
    }
}
enum Either<F, FMut> {
    F(F),
    Fmut(FMut),
}

struct HandleCall<T, R, F, FMut>
where
    T: Any,
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

impl<T, R, F, FMut> HandleCall<T, R, F, FMut>
where
    T: Any,
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
            match self.func.take().unwrap() {
                Either::F(func) => {
                    if let Ok(object) = object.try_read() {
                        let r = func(&*object);
                        drop(object);
                        self.handle.wake_calls();
                        Poll::Ready(Ok(r))
                    } else {
                        self.handle.add_call_waker(cx.waker().clone());
                        self.func = Some(Either::F(func));
                        Poll::Pending
                    }
                }
                Either::Fmut(func_mut) => {
                    if let Ok(mut object) = object.try_write() {
                        let r = func_mut(&mut *object);
                        drop(object);
                        self.handle.wake_calls();
                        Poll::Ready(Ok(r))
                    } else {
                        self.handle.add_call_waker(cx.waker().clone());
                        self.func = Some(Either::Fmut(func_mut));
                        Poll::Pending
                    }
                }
            }
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
    subscribers: Weak<RwLock<EventSubscribers>>,
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
    pub fn new(
        object: Weak<RwLock<T>>,
        subscribers: Weak<RwLock<EventSubscribers>>,
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
    fn subscribe(&self, event_queue: Weak<RwLock<EventQueue>>) {
        if let Some(subscribers) = self.subscribers.upgrade() {
            subscribers.write().unwrap().subscribe(event_queue)
        }
    }
    pub fn get_event<EVT: Any + Send + Sync>(&self) -> impl Stream<Item = EVT> {
        EventSource::new(self.clone())
    }
}

#[derive(Debug)]
pub struct HandleSupport<T: 'static> {
    subscribers: Arc<RwLock<EventSubscribers>>,
    call_wakers: Arc<RwLock<Vec<Waker>>>,
    object: Weak<RwLock<T>>,
}

impl<T: 'static> Drop for HandleSupport<T> {
    fn drop(&mut self) {
        self.call_wakers
            .write()
            .unwrap()
            .drain(..)
            .for_each(|w| w.wake());
    }
}

impl<T: 'static> HandleSupport<T> {
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(RwLock::new(EventSubscribers::new())),
            call_wakers: Arc::new(RwLock::new(Vec::new())),
            object: Weak::new(),
        }
    }
    pub fn init(&mut self, object: &Arc<RwLock<T>>) -> Handle<T> {
        self.object = Arc::downgrade(object);
        self.handle()
    }
    pub fn handle(&self) -> Handle<T> {
        let object = self.object.clone() as Weak<RwLock<T>>;
        let subscribers = Arc::downgrade(&self.subscribers);
        let call_wakers = Arc::downgrade(&self.call_wakers);
        Handle::new(object, subscribers, call_wakers)
    }
    pub fn send_event<EVT: Any + Send + Sync + Clone + 'static>(&self, event: EVT) {
        self.subscribers.write().unwrap().send_event(event)
    }
}
