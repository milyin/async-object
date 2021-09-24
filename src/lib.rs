use futures::{
    executor::{LocalPool, LocalSpawner},
    task::LocalSpawnExt,
    Future, Stream,
};
use std::{
    any::Any,
    cell::RefCell,
    collections::VecDeque,
    fmt,
    marker::PhantomData,
    pin::Pin,
    rc::{Rc, Weak},
    task::{Context, Poll, Waker},
};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Object not exists")]
    ObjectNotExists,
}

#[derive(Clone)]
pub struct Handle {
    entry_pos: usize,
    object: Weak<RefCell<Box<dyn Any + 'static>>>,
    subscribers: Weak<RefCell<EventSubscribers>>,
}

struct EventSubscribers {
    subscribers: Vec<Weak<RefCell<EventQueue>>>,
}

impl EventSubscribers {
    fn new() -> Self {
        Self {
            subscribers: Vec::new(),
        }
    }
    fn subscribe(&mut self, event_queue: Weak<RefCell<EventQueue>>) {
        if let Some(empty) = self.subscribers.iter_mut().find(|w| w.strong_count() == 0) {
            *empty = event_queue;
        } else {
            self.subscribers.push(event_queue);
        };
    }
    fn send_event<EVT: Any + Clone + 'static>(&mut self, event: EVT) {
        self.subscribers
            .iter()
            .filter_map(|event_queue| event_queue.upgrade())
            .for_each(|event_queue| event_queue.borrow_mut().send_event(event.clone()));
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
    object: Weak<RefCell<Box<dyn Any + 'static>>>,
    event_queue: Rc<RefCell<EventQueue>>,
    _phantom: PhantomData<Box<EVT>>,
}

impl<EVT: Any> EventSource<EVT> {
    fn new(handle: Handle) -> Self {
        let event_queue = Rc::new(RefCell::new(EventQueue::new()));
        let weak_event_queue = Rc::downgrade(&mut (event_queue.clone()));
        handle.subscribe(weak_event_queue);
        Self {
            object: handle.object.clone(),
            event_queue,
            _phantom: PhantomData,
        }
    }
    fn poll_next(self: &mut Self, cx: &mut Context<'_>) -> Poll<Option<EVT>> {
        let mut event_queue = self.event_queue.borrow_mut();
        event_queue.waker = Some(cx.waker().clone());
        if let Some(evt) = event_queue.get_event::<EVT>() {
            Poll::Ready(Some(evt))
        } else if self.object.strong_count() == 0 {
            Poll::Ready(None)
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
    fn poll(&mut self) -> Poll<Result<R, Error>> {
        let object = if let Some(object) = self.handle.object.upgrade() {
            object
        } else {
            return Poll::Ready(Err(Error::ObjectNotExists));
        };
        let result = match self.func.take().unwrap() {
            Either::F(func) => {
                let object = object.borrow();
                let object = object.downcast_ref::<T>().expect("wrong type");
                func(object)
            }
            Either::Fmut(func_mut) => {
                let mut object = object.borrow_mut();
                let object = object.downcast_mut::<T>().expect("wrong type");
                func_mut(object)
            }
        };
        Poll::Ready(Ok(result))
    }
}

impl<T: Any, R, F: FnOnce(&T) -> R, FMut: FnOnce(&mut T) -> R> Future
    for HandleCall<T, R, F, FMut>
{
    type Output = Result<R, Error>;
    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.get_mut().poll()
    }
}

impl Handle {
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
    fn subscribe(&self, event_queue: Weak<RefCell<EventQueue>>) {
        if let Some(subscribers) = self.subscribers.upgrade() {
            subscribers.borrow_mut().subscribe(event_queue)
        }
    }
    pub fn get_event<EVT: Any>(&self) -> impl Stream<Item = EVT> {
        EventSource::new(self.clone())
    }
    pub fn send_event<EVT: Any>(&self) {}
}

struct Entry {
    object: Rc<RefCell<Box<dyn Any + 'static>>>,
    subscribers: Rc<RefCell<EventSubscribers>>,
}

impl Entry {
    fn new(object: impl Any + 'static) -> Self {
        let object: Box<dyn Any + 'static> = Box::new(object);
        let object = Rc::new(RefCell::new(object));
        let subscribers = Rc::new(RefCell::new(EventSubscribers::new()));

        Self {
            object,
            subscribers,
        }
    }
    fn make_handle(&self, entry_pos: usize) -> Handle {
        Handle {
            entry_pos,
            object: Rc::downgrade(&self.object),
            subscribers: Rc::downgrade(&self.subscribers),
        }
    }
}

pub struct Pool {
    local_pool: LocalPool,
    entries: Vec<Option<Entry>>,
}

impl Pool {
    pub fn new() -> Self {
        let local_pool = LocalPool::new();
        let entries = Vec::new();
        Self {
            local_pool,
            entries,
        }
    }
    pub fn register_object(&mut self, object: impl Any + 'static) -> Handle {
        let entry = Entry::new(object);
        if let Some(pos) = self.entries.iter().position(|e| e.is_none()) {
            let handle = entry.make_handle(pos);
            self.entries[pos] = Some(entry);
            handle
        } else {
            let pos = self.entries.len();
            let handle = entry.make_handle(pos);
            self.entries.push(Some(entry));
            handle
        }
    }

    pub fn spawner(&self) -> LocalSpawner {
        self.local_pool.spawner()
    }

    pub fn drop_object(&mut self, handle: Handle) {
        self.entries[handle.entry_pos] = None;
    }

    pub fn run_until_stalled(&mut self) {
        self.local_pool.run_until_stalled()
    }
}

pub fn spawn<E: fmt::Debug, FUTURE: Future<Output = Result<(), E>> + 'static>(
    spawner: LocalSpawner,
    f: FUTURE,
) {
    let future = async move { f.await.expect("spawned task failed") };
    spawner.spawn_local(future).expect("spawner failed");
}
