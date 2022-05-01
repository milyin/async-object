use std::{
    marker::PhantomData,
    pin::Pin,
    sync::{Arc, RwLock, Weak},
    task::{Context, Poll, Waker},
};

use futures::Future;

/// Reference-counting pointer based on Arc<RwLock<T>> with methods for access wrapped data from asynchronous code
pub struct CArc<T: 'static> {
    object: Arc<RwLock<T>>,
    call_wakers: Arc<RwLock<Vec<Waker>>>,
}

/// Non-owning variant of ['CArc']
pub struct WCArc<T: 'static> {
    object: Weak<RwLock<T>>,
    call_wakers: Weak<RwLock<Vec<Waker>>>,
}

impl<T: 'static> CArc<T> {
    pub fn new(object: T) -> Self {
        Self {
            object: Arc::new(RwLock::new(object)),
            call_wakers: Arc::new(RwLock::new(Vec::new())),
        }
    }
    pub fn new_cyclic<F>(data_fn: F) -> Self
    where
        F: FnOnce(WCArc<T>) -> T,
    {
        let call_wakers = Arc::new(RwLock::new(Vec::new()));
        let object = Arc::new_cyclic(|v| {
            let wcarc = WCArc::<T> {
                object: v.clone(),
                call_wakers: Arc::downgrade(&call_wakers),
            };
            RwLock::new(data_fn(wcarc))
        });
        Self {
            object,
            call_wakers,
        }
    }
    pub fn downgrade(&self) -> WCArc<T> {
        WCArc {
            object: Arc::downgrade(&self.object),
            call_wakers: Arc::downgrade(&self.call_wakers),
        }
    }
    pub fn id(&self) -> usize {
        Arc::as_ptr(&self.object) as usize
    }
}

impl<T: 'static> Clone for CArc<T> {
    fn clone(&self) -> Self {
        Self {
            object: self.object.clone(),
            call_wakers: self.call_wakers.clone(),
        }
    }
}

impl<T: 'static> Clone for WCArc<T> {
    fn clone(&self) -> Self {
        Self {
            object: self.object.clone(),
            call_wakers: self.call_wakers.clone(),
        }
    }
}

impl<T: 'static> PartialEq for CArc<T> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.object, &other.object)
    }
}

impl<T: 'static> PartialEq for WCArc<T> {
    fn eq(&self, other: &Self) -> bool {
        self.object.ptr_eq(&other.object)
    }
}

impl<T: 'static> Default for WCArc<T> {
    fn default() -> Self {
        Self {
            object: Default::default(),
            call_wakers: Default::default(),
        }
    }
}

impl<T: 'static> CArc<T> {
    fn add_call_waker(&self, waker: Waker) {
        self.call_wakers.write().unwrap().push(waker);
    }
    fn wake_calls(&self) {
        drain_wakers(&self.call_wakers)
    }
    pub fn async_call<R, F: FnOnce(&T) -> R>(&self, f: F) -> impl Future<Output = R> {
        new_async_call(self.clone(), f)
    }
    pub fn async_call_mut<R, F: FnOnce(&mut T) -> R>(&self, f: F) -> impl Future<Output = R> {
        new_async_call_mut(self.clone(), f)
    }
    pub fn call<R, F: FnOnce(&T) -> R>(&self, f: F) -> R {
        let r = f(&*self.object.read().unwrap());
        drain_wakers(&self.call_wakers);
        r
    }
    pub fn call_mut<R, F: FnOnce(&mut T) -> R>(&self, f: F) -> R {
        let r = f(&mut *self.object.write().unwrap());
        drain_wakers(&self.call_wakers);
        r
    }
}

impl<T: 'static> WCArc<T> {
    pub fn upgrade(&self) -> Option<CArc<T>> {
        if let (Some(object), Some(call_wakers)) =
            (self.object.upgrade(), self.call_wakers.upgrade())
        {
            Some(CArc {
                object,
                call_wakers,
            })
        } else {
            None
        }
    }
    pub fn id(&self) -> Option<usize> {
        if self.object.strong_count() == 0 {
            None
        } else {
            Some(Weak::as_ptr(&self.object) as usize)
        }
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
    carc: CArc<T>,
    func: Option<Either<Box<F>, Box<FMut>>>,
    _phantom: PhantomData<Box<(T, R)>>,
}

fn new_async_call<T, R, F: FnOnce(&T) -> R>(
    carc: CArc<T>,
    f: F,
) -> AsyncCall<T, R, F, fn(&mut T) -> R> {
    AsyncCall::new(carc, f)
}

fn new_async_call_mut<T, R, FMut: FnOnce(&mut T) -> R>(
    carc: CArc<T>,
    f: FMut,
) -> AsyncCall<T, R, fn(&T) -> R, FMut> {
    AsyncCall::new_mut(carc, f)
}

impl<T: 'static, R, F, FMut> AsyncCall<T, R, F, FMut>
where
    F: FnOnce(&T) -> R,
    FMut: FnOnce(&mut T) -> R,
{
    fn new(carc: CArc<T>, func: F) -> Self {
        Self {
            carc,
            func: Some(Either::F(Box::new(func))),
            _phantom: PhantomData,
        }
    }
    fn new_mut(carc: CArc<T>, func: FMut) -> Self {
        Self {
            carc,
            func: Some(Either::Fmut(Box::new(func))),
            _phantom: PhantomData,
        }
    }
    fn poll(&mut self, cx: &Context) -> Poll<R> {
        self.carc.add_call_waker(cx.waker().clone());
        let res = match self.func.take().unwrap() {
            Either::F(func) => {
                if let Ok(object) = self.carc.object.try_read() {
                    let r = func(&*object);
                    Poll::Ready(r)
                } else {
                    self.func = Some(Either::F(func));
                    Poll::Pending
                }
            }
            Either::Fmut(func_mut) => {
                if let Ok(mut object) = self.carc.object.try_write() {
                    let r = func_mut(&mut *object);
                    Poll::Ready(r)
                } else {
                    self.func = Some(Either::Fmut(func_mut));
                    Poll::Pending
                }
            }
        };
        if res.is_ready() {
            self.carc.wake_calls();
        }
        res
    }
}

impl<T: 'static, R, F: FnOnce(&T) -> R, FMut: FnOnce(&mut T) -> R> Future
    for AsyncCall<T, R, F, FMut>
{
    type Output = R;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.get_mut().poll(cx)
    }
}

fn drain_wakers(wakers: &Arc<RwLock<Vec<Waker>>>) {
    wakers.write().unwrap().drain(..).for_each(|w| w.wake());
}
