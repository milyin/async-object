use futures::{Future, Stream};
use std::{
    any::Any,
    collections::VecDeque,
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
    pub fn send_event<EVT: Any + Clone + 'static>(&mut self, event: EVT) {
        self.subscribers
            .iter()
            .filter_map(|event_queue| event_queue.upgrade())
            .for_each(|event_queue| event_queue.write().unwrap().send_event(event.clone()));
    }
}

struct EventQueue {
    waker: Option<Waker>,
    events: VecDeque<Box<dyn Any + 'static>>,
}

impl EventQueue {
    fn new() -> Self {
        Self {
            waker: None,
            events: VecDeque::new(),
        }
    }
    fn send_event<EVT: Any + 'static>(&mut self, event: EVT) {
        self.events.push_back(Box::new(event));
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
    fn get_event<EVT: Any>(&mut self) -> Option<EVT> {
        self.events.pop_front().map(|evt| *evt.downcast().unwrap())
    }
}

struct EventSource<EVT: Any> {
    object: Weak<RwLock<dyn Any + 'static>>,
    event_queue: Arc<RwLock<EventQueue>>,
    _phantom: PhantomData<Box<EVT>>,
}

impl<EVT: Any> EventSource<EVT> {
    fn new(handle: Handle) -> Self {
        let event_queue = Arc::new(RwLock::new(EventQueue::new()));
        let weak_event_queue = Arc::downgrade(&mut (event_queue.clone()));
        handle.subscribe(weak_event_queue);
        Self {
            object: handle.object.clone(),
            event_queue,
            _phantom: PhantomData,
        }
    }
    fn poll_next(self: &mut Self, cx: &mut Context<'_>) -> Poll<Option<EVT>> {
        if let Ok(mut event_queue) = self.event_queue.try_write() {
            event_queue.waker = Some(cx.waker().clone());
            if let Some(evt) = event_queue.get_event::<EVT>() {
                Poll::Ready(Some(evt))
            } else if self.object.strong_count() == 0 {
                Poll::Ready(None)
            } else {
                Poll::Pending
            }
        } else {
            Poll::Pending
        }
    }
}

impl<EVT: Any> Stream for EventSource<EVT> {
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
    handle: Handle,
    func: Option<Either<Box<F>, Box<FMut>>>,
    _phantom: PhantomData<Box<(T, R)>>,
}

fn new_handle_call<T: Any, R, F: FnOnce(&T) -> R>(
    handle: Handle,
    f: F,
) -> HandleCall<T, R, F, fn(&mut T) -> R> {
    HandleCall::new(handle, f)
}

fn new_handle_call_mut<T: Any, R, FMut: FnOnce(&mut T) -> R>(
    handle: Handle,
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
    fn new(handle: Handle, func: F) -> Self {
        Self {
            handle,
            func: Some(Either::F(Box::new(func))),
            _phantom: PhantomData,
        }
    }
    fn new_mut(handle: Handle, func: FMut) -> Self {
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
                        let object = object.downcast_ref::<T>().expect("wrong type");
                        let r = func(object);
                        drop(object);
                        self.handle.wake_calls();
                        Poll::Ready(Ok(r))
                    } else {
                        self.handle.add_call_waker(cx.waker().clone());
                        Poll::Pending
                    }
                }
                Either::Fmut(func_mut) => {
                    if let Ok(mut object) = object.try_write() {
                        let object = object.downcast_mut::<T>().expect("wrong type");
                        let r = func_mut(object);
                        drop(object);
                        self.handle.wake_calls();
                        Poll::Ready(Ok(r))
                    } else {
                        self.handle.add_call_waker(cx.waker().clone());
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

#[derive(Clone)]
pub struct Handle {
    object: Weak<RwLock<dyn Any + 'static>>,
    subscribers: Weak<RwLock<EventSubscribers>>,
    call_wakers: Weak<RwLock<Vec<Waker>>>,
}

impl Handle {
    pub fn new(
        object: Weak<RwLock<dyn Any + 'static>>,
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

    pub fn call<T: Any, R, F: FnOnce(&T) -> R>(
        &self,
        f: F,
    ) -> impl Future<Output = Result<R, Error>> {
        new_handle_call(self.clone(), f)
    }
    pub fn call_mut<T: Any, R, F: FnOnce(&mut T) -> R>(
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
    pub fn get_event<EVT: Any>(&self) -> impl Stream<Item = EVT> {
        EventSource::new(self.clone())
    }
    pub fn send_event<EVT: Any + Clone + 'static>(&self, event: EVT) {
        if let Some(subscribers) = self.subscribers.upgrade() {
            subscribers.write().unwrap().send_event(event)
        }
    }
}

pub struct HandleSupport<T: Any + 'static> {
    subscribers: Arc<RwLock<EventSubscribers>>,
    call_wakers: Arc<RwLock<Vec<Waker>>>,
    object: Weak<RwLock<T>>,
}

impl<T: Any + 'static> HandleSupport<T> {
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(RwLock::new(EventSubscribers::new())),
            call_wakers: Arc::new(RwLock::new(Vec::new())),
            object: Weak::new(),
        }
    }
    pub fn set_object(&mut self, object: &Arc<RwLock<T>>) {
        self.object = Arc::downgrade(object)
    }
    pub fn handle(&self) -> Handle {
        let object = self.object.clone() as Weak<RwLock<dyn Any>>;
        let subscribers = Arc::downgrade(&self.subscribers);
        let call_wakers = Arc::downgrade(&self.call_wakers);
        Handle::new(object, subscribers, call_wakers)
    }
}
