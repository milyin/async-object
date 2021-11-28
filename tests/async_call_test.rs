use async_object::{self, run, Tag};
use futures::{
    executor::LocalPool,
    task::{LocalSpawnExt, Spawn},
};
use std::{cell::RefCell, rc::Rc};

struct CounterImpl {
    internal_value: usize,
}

impl CounterImpl {
    fn new() -> Self {
        Self { internal_value: 0 }
    }
    fn inc(&mut self) {
        self.internal_value += 1;
    }
    fn internal_value(&self) -> usize {
        self.internal_value
    }
}

struct Counter(Tag<CounterImpl>);

impl Counter {
    fn new(pool: impl Spawn) -> Self {
        Counter(run(pool, CounterImpl::new()).unwrap())
    }
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
}

#[test]
fn test_handle_call() {
    let mut pool = LocalPool::new();
    let test_value = Rc::new(RefCell::new(None));
    let test_value_r = test_value.clone();

    let counter = Counter::new(pool.spawner());

    let future = async move {
        let v = counter.internal_value().await.unwrap();
        *(test_value.borrow_mut()) = Some(v);
    };
    pool.spawner().spawn_local(future).unwrap();
    pool.run_until_stalled();
    assert!(test_value_r.borrow().is_some())
}

#[test]
fn test_handle_call_mut() {
    let mut pool = LocalPool::new();
    let test_value = Rc::new(RefCell::new(None));
    let test_value_r = test_value.clone();

    let counter = Counter::new(pool.spawner());

    let future = async move {
        counter.inc().await.unwrap();
        let v = counter.internal_value().await.unwrap();
        *(test_value.borrow_mut()) = Some(v);
    };
    pool.spawner().spawn_local(future).unwrap();
    pool.run_until_stalled();
    assert!(test_value_r.borrow().is_some());
    assert!(test_value_r.borrow().unwrap() == 1);
}
