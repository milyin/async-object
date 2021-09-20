use futures::{
    executor::{LocalPool, LocalSpawner},
    Future,
};
use std::{
    any::Any,
    cell::RefCell,
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
    waker_pos: Option<usize>,
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
            waker_pos: None,
            _phantom: PhantomData,
        }
    }
    fn new_mut(handle: Handle, func: FMut) -> Self {
        Self {
            handle,
            func: Some(Either::Fmut(Box::new(func))),
            waker_pos: None,
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
