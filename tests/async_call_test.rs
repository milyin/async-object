use async_object::{self, Keeper, Tag};
use futures::{executor::LocalPool, task::LocalSpawnExt};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, RwLock},
};

#[derive(Clone)]
struct CounterShared {
    value: usize,
}

struct Counter {
    value: usize,
    shared: Arc<RwLock<CounterShared>>,
}

impl Counter {
    fn new() -> Self {
        Self {
            value: 0,
            shared: Arc::new(RwLock::new(CounterShared { value: 0 })),
        }
    }
    fn inc(&mut self) {
        self.value += 1;
        self.shared.write().unwrap().value = self.value;
    }
    fn value(&self) -> usize {
        self.value
    }
}

struct HCounter(Tag<Counter, CounterShared>);

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

struct KCounter(Keeper<Counter, CounterShared>);

impl KCounter {
    fn new() -> Self {
        let counter = Counter::new();
        let shared = counter.shared.clone();
        Self(Keeper::new_with_shared(counter, shared))
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
    let test_value_shared = Rc::new(RefCell::new(None));
    let test_value_shared_r = test_value_shared.clone();

    let kvalue = KCounter::new();
    let hvalue = kvalue.handle();

    let future = async move {
        hvalue.inc().await.unwrap();
        let v = hvalue.value().await.unwrap();
        *(test_value.borrow_mut()) = Some(v);
        *(test_value_shared.borrow_mut()) = Some(hvalue.0.shared().unwrap());
    };
    let mut pool = LocalPool::new();
    pool.spawner().spawn_local(future).unwrap();
    pool.run_until_stalled();
    assert!(test_value_r.borrow().is_some());
    assert!(test_value_r.borrow().unwrap() == 1);
    assert!(test_value_shared_r.borrow().as_ref().unwrap().value == 1);
}
