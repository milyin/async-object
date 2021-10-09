use async_object::{self, Keeper, Tag};
use futures::{executor::LocalPool, task::LocalSpawnExt};
use std::{cell::RefCell, rc::Rc};

struct Counter {
    value: usize,
}

impl Counter {
    fn new() -> Self {
        Self { value: 0 }
    }
    fn inc(&mut self) {
        self.value += 1;
    }
    fn value(&self) -> usize {
        self.value
    }
}

struct HCounter(Tag<Counter>);

impl HCounter {
    async fn inc(&self) -> async_object::Result<()> {
        self.0
            .async_call_mut(|counter: &mut Counter| counter.inc())
            .await
    }
    async fn value(&self) -> async_object::Result<usize> {
        self.0.async_call(|counter: &Counter| counter.value()).await
    }
}

struct KCounter(Keeper<Counter>);

impl KCounter {
    fn new() -> Self {
        KCounter(Keeper::new(Counter::new()))
    }
    fn handle(&self) -> HCounter {
        HCounter(self.0.tag())
    }
}

#[test]
fn test_handle_call() {
    let test_value = Rc::new(RefCell::new(None));
    let test_value_r = test_value.clone();

    let kvalue = KCounter::new();
    let hvalue = kvalue.handle();

    let future = async move {
        let v = hvalue.value().await.unwrap();
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

    let kvalue = KCounter::new();
    let hvalue = kvalue.handle();

    let future = async move {
        hvalue.inc().await.unwrap();
        let v = hvalue.value().await.unwrap();
        *(test_value.borrow_mut()) = Some(v);
    };
    let mut pool = LocalPool::new();
    pool.spawner().spawn_local(future).unwrap();
    pool.run_until_stalled();
    assert!(test_value_r.borrow().is_some());
    assert!(test_value_r.borrow().unwrap() == 1);
}
