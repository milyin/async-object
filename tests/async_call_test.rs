use async_object::{CArc, WCArc};
use futures::{executor::LocalPool, task::LocalSpawnExt};
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

struct Counter(CArc<CounterImpl>);
struct WCounter(WCArc<CounterImpl>);

impl Counter {
    fn new() -> Self {
        Counter(CArc::new(CounterImpl::new()))
    }
    async fn inc(&self) {
        self.0
            .async_call_mut(|counter: &mut CounterImpl| counter.inc())
            .await
    }
    async fn internal_value(&self) -> usize {
        self.0
            .async_call(|counter: &CounterImpl| counter.internal_value())
            .await
    }
    fn downgrade(&self) -> WCounter {
        WCounter(self.0.downgrade())
    }
    fn id(&self) -> usize {
        self.0.id()
    }
}

impl WCounter {
    fn upgrade(&self) -> Option<Counter> {
        self.0.upgrade().map(|v| Counter(v))
    }
    fn id(&self) -> Option<usize> {
        self.0.id()
    }
}

#[test]
fn test_handle_call() {
    let mut pool = LocalPool::new();
    let test_value = Rc::new(RefCell::new(None));
    let test_value_r = test_value.clone();

    let counter = Counter::new();

    let future = async move {
        let v = counter.internal_value().await;
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

    let counter = Counter::new();

    let future = async move {
        counter.inc().await;
        let v = counter.internal_value().await;
        *(test_value.borrow_mut()) = Some(v);
    };
    pool.spawner().spawn_local(future).unwrap();
    pool.run_until_stalled();
    assert!(test_value_r.borrow().is_some());
    assert!(test_value_r.borrow().unwrap() == 1);
}

#[test]
fn test_id() {
    let a = Counter::new();
    let b = Counter::new();
    let wa = a.downgrade();
    let wb = b.downgrade();
    let ua = wa.upgrade().unwrap();
    let ub = wb.upgrade().unwrap();
    assert!(a.id() != b.id());
    assert!(a.id() == wa.id().unwrap());
    assert!(b.id() == wb.id().unwrap());
    assert!(a.id() == ua.id());
    assert!(b.id() == ub.id());
    drop(a);
    drop(b);
    drop(ua);
    drop(ub);
    assert!(wa.id() == None);
    assert!(wb.id() == None);
}
