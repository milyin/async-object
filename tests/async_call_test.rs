use async_object::{self, Keeper, Tag};
use futures::{executor::LocalPool, task::LocalSpawnExt};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, RwLock},
};

#[derive(Clone)]
struct CounterShared {
    shared_value: usize,
}

struct CounterImpl {
    internal_value: usize,
    shared: Arc<RwLock<CounterShared>>,
}

impl CounterImpl {
    fn new() -> Self {
        Self {
            internal_value: 0,
            shared: Arc::new(RwLock::new(CounterShared { shared_value: 0 })),
        }
    }
    fn inc(&mut self) {
        self.internal_value += 1;
        self.shared.write().unwrap().shared_value = self.internal_value;
    }
    fn internal_value(&self) -> usize {
        self.internal_value
    }
}

struct TCounter(Tag<CounterImpl, CounterShared>);

impl TCounter {
    async fn inc(&self) -> Option<()> {
        self.0
            .async_write(|counter: &mut CounterImpl| counter.inc())
            .await
    }
    async fn internal_value(&self) -> Option<usize> {
        self.0
            .async_read(|counter: &CounterImpl| counter.internal_value())
            .await
    }
    fn shared_value(&self) -> Option<usize> {
        self.0.clone_shared().map(|v| v.shared_value)
    }
}

struct Counter(Keeper<CounterImpl, CounterShared>);

impl Counter {
    fn new() -> Self {
        let counter = CounterImpl::new();
        let shared = counter.shared.clone();
        Self(Keeper::new_with_shared(counter, shared))
    }
    fn tag(&self) -> TCounter {
        TCounter(self.0.tag())
    }
}

#[test]
fn test_handle_call() {
    let test_value = Rc::new(RefCell::new(None));
    let test_value_r = test_value.clone();

    let counter = Counter::new();
    let tcounter = counter.tag();

    let future = async move {
        let v = tcounter.internal_value().await.unwrap();
        *(test_value.borrow_mut()) = Some(v);
    };
    let mut pool = LocalPool::new();
    pool.spawner().spawn_local(future).unwrap();
    pool.run_until_stalled();
    assert!(test_value_r.borrow().is_some())
}

#[test]
fn test_handle_call_mut() {
    let test_value = Rc::new(RefCell::new(None));
    let test_value_r = test_value.clone();
    let test_value_shared = Rc::new(RefCell::new(None));
    let test_value_shared_r = test_value_shared.clone();

    let counter = Counter::new();
    let tcounter = counter.tag();

    let future = async move {
        tcounter.inc().await.unwrap();
        let v = tcounter.internal_value().await.unwrap();
        *(test_value.borrow_mut()) = Some(v);
        *(test_value_shared.borrow_mut()) = Some(tcounter.shared_value().unwrap());
    };
    let mut pool = LocalPool::new();
    pool.spawner().spawn_local(future).unwrap();
    pool.run_until_stalled();
    assert!(test_value_r.borrow().is_some());
    assert!(test_value_r.borrow().unwrap() == 1);
    assert!(test_value_shared_r.borrow().is_some());
    assert!(test_value_shared_r.borrow().unwrap() == 1);
}
