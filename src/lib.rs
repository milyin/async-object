use futures::{executor::LocalPool, Future};
use std::{
    any::{Any, TypeId},
    borrow::Borrow,
    cell::RefCell,
    collections::{HashMap, VecDeque},
    marker::PhantomData,
    ops::DerefMut,
    pin::Pin,
    rc::{Rc, Weak},
    sync::WaitTimeoutResult,
    task::{Context, Poll, Waker},
};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Object not exists")]
    ObjectNotExists,
    #[error("Wrong object type requested")]
    ObjectWrongType,
}

#[derive(Clone)]
struct Handle {
    entry_pos: usize,
    object: Weak<RefCell<dyn Any + 'static>>,
    wakers: Weak<RefCell<HandleWakers>>,
}

struct HandleWakers {
    wakers: Vec<Option<Waker>>,
}

impl HandleWakers {
    fn new() -> Self {
        Self { wakers: Vec::new() }
    }

    fn set_waker(&mut self, pos: &mut Option<usize>, waker: Waker) {
        if let Some(pos) = pos {
            self.wakers[*pos] = Some(waker)
        } else if let Some(free_pos) = self.wakers.iter().position(|w| w.is_none()) {
            *pos = Some(free_pos);
            self.wakers[free_pos] = Some(waker)
        } else {
            *pos = Some(self.wakers.len());
            self.wakers.push(Some(waker))
        }
    }

    fn wake(&mut self) {
        if let Some(waker) = (&mut self.wakers).into_iter().find_map(|w| w.take()) {
            waker.wake()
        }
    }
}

struct HandleCall<T: Any, R, F: FnOnce(&T) -> R> {
    handle: Handle,
    func: Option<Box<F>>,
    waker_pos: Option<usize>,
    _phantom: PhantomData<Box<(T, R)>>,
}

impl<T: Any, R, F: FnOnce(&T) -> R> Future for HandleCall<T, R, F> {
    type Output = Result<R, Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let res = if let Some(object) = this.handle.object.upgrade() {
            if let Ok(object) = object.try_borrow() {
                if let Some(object) = object.downcast_ref::<T>() {
                    let func = this.func.take().unwrap();
                    Poll::Ready(Ok(func(object)))
                } else {
                    Poll::Ready(Err(Error::ObjectWrongType))
                }
            } else {
                let wakers = this.handle.wakers.upgrade().unwrap();
                let mut wakers = wakers.try_borrow_mut().unwrap();
                wakers.set_waker(&mut this.waker_pos, cx.waker().clone());
                Poll::Pending
            }
        } else {
            Poll::Ready(Err(Error::ObjectNotExists))
        };
        if res.is_ready() {
            let wakers = this.handle.wakers.upgrade().unwrap();
            let mut wakers = wakers.try_borrow_mut().unwrap();
            wakers.wake();
        }
        res
    }
}

impl<T: Any, R, F: FnOnce(&T) -> R> HandleCall<T, R, F> {
    fn new(handle: Handle, func: F) -> Self {
        Self {
            handle,
            func: Some(Box::new(func)),
            waker_pos: None,
            _phantom: PhantomData,
        }
    }
}

impl Handle {
    fn call<T: Any, R, F: FnOnce(&T) -> R>(&self, f: F) -> impl Future<Output = Result<R, Error>> {
        HandleCall::new(self.clone(), f)
    }
}

struct Entry {
    object: Rc<RefCell<Box<dyn Any + 'static>>>,
    wakers: Rc<RefCell<Vec<Waker>>>,
}

struct Pool {
    local_pool: LocalPool,
    entries: Vec<Option<Entry>>,
}

/*
struct Entry {
    object: Box<dyn Any + 'static>,
    events: HashMap<TypeId, VecDeque<Box<dyn Any + 'static>>>,
    wakers: HashMap<TypeId, Vec<Waker>>,
}

impl Entry {
    fn new<T: Any>(object: T) -> Self {
        Self {
            object: Box::new(object),
            events: HashMap::new(),
            wakers: HashMap::new(),
        }
    }
}

pub struct Pool {
    local_pool: LocalPool,
    entries: Vec<Option<Rc<RefCell<Entry>>>>,
}

pub struct ExpectEvent<T: Any> {
    object_id: usize,
    _phantom_event: PhantomData<T>,
}

impl<T: Any> ExpectEvent<T> {
    pub fn new(object_id: usize) -> Self {
        Self {
            object_id,
            _phantom_event: PhantomData,
        }
    }
}

// impl<T: Any> Future for ExpectEvent<T> {
//     type Output = T;

//     // fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {}
// }

struct ObjectId(usize);

impl Pool {
    pub fn new() -> Self {
        let local_pool = LocalPool::new();
        let entries = Vec::new();
        Self {
            local_pool,
            entries,
        }
    }

    pub fn add_object<T: Any>(&mut self, object: T) -> ObjectId {
        let entry = Some(Rc::new(RefCell::new(Entry::new(object))));
        if let Some((object_id, entry_cell)) =
            self.entries.iter_mut().enumerate().find(|v| v.1.is_none())
        {
            assert!(entry_cell.is_none());
            let object_id = ObjectId(object_id);
            *entry_cell = entry;
            object_id
        } else {
            let object_id = ObjectId(self.entries.len());
            self.entries.push(entry);
            object_id
        }
    }

    pub fn get_object<T: Any>(&mut self, object_id: ObjectId) -> Option<&mut T> {
        if let Some(Some(ref mut entry)) = self.entries.get(object_id.0) {
            entry.get_mut().object.downcast_mut::<T>()
        } else {
            None
        }
    }

    pub fn send_event(&mut self, object_id: usize, event: impl Any) {
        if let Some(mut wakers) = self.wakers.remove(&(object_id, event.type_id())) {
            wakers.drain(..).for_each(|w| w.wake());
        }
        self.events
            .entry((object_id, event.type_id()))
            .or_insert_with(|| VecDeque::new())
            .push_front(Box::new(event));
    }
    pub fn receive_event<T: Any>(&mut self, object_id: usize) -> Option<T> {
        let event_id = TypeId::of::<T>();
        let event = match self.events.get_mut(&(object_id, event_id)) {
            Some(events) => events.pop_back(),
            None => None,
        };
        if let Some(event) = event {
            if let Ok(t) = event.downcast::<T>() {
                Some(*t)
            } else {
                panic!("unexpected event type")
            }
        } else {
            None
        }
    }
    pub async fn expect_event<T: Any>(&mut self, object_id: usize) -> T {}
}
*/
#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
