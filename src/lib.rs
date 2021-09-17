use futures::{
    executor::{LocalPool, LocalSpawner},
    Future,
};
use std::{
    any::{Any, TypeId},
    cell::{RefCell, RefMut},
    collections::{HashMap, VecDeque},
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
    #[error("Wrong object type requested")]
    ObjectWrongType,
}

#[derive(Clone)]
pub struct Handle {
    entry_pos: usize,
    object: Weak<RefCell<Box<dyn Any + 'static>>>,
    wakers: Weak<RefCell<HandleWakers>>,
    spawner: LocalSpawner,
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

impl Drop for HandleWakers {
    fn drop(&mut self) {
        (&mut self.wakers)
            .into_iter()
            .filter_map(|w| w.take())
            .for_each(|w| w.wake());
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
    pub fn call<T: Any, R, F: FnOnce(&T) -> R>(
        &self,
        f: F,
    ) -> impl Future<Output = Result<R, Error>> {
        HandleCall::new(self.clone(), f)
    }
    pub fn spawner(&self) -> LocalSpawner {
        self.spawner.clone()
    }
}

struct Entry {
    object: Rc<RefCell<Box<dyn Any + 'static>>>,
    wakers: Rc<RefCell<HandleWakers>>,
}

impl Entry {
    fn new(object: impl Any + 'static) -> Self {
        let object: Box<dyn Any + 'static> = Box::new(object);
        let object = Rc::new(RefCell::new(object));
        let wakers = Rc::new(RefCell::new(HandleWakers::new()));
        Self { object, wakers }
    }
    fn make_handle(&self, entry_pos: usize, spawner: LocalSpawner) -> Handle {
        Handle {
            entry_pos,
            object: Rc::downgrade(&self.object),
            wakers: Rc::downgrade(&self.wakers),
            spawner,
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
            let handle = entry.make_handle(pos, self.spawner());
            self.entries[pos] = Some(entry);
            handle
        } else {
            let pos = self.entries.len();
            let handle = entry.make_handle(pos, self.spawner());
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

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
