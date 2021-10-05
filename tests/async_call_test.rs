use async_object::{self, Handle, Keeper};
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

struct HCounter(Handle<Counter>);

impl HCounter {
    async fn inc(&self) -> Result<(), async_object::Error> {
        self.0.call_mut(|counter: &mut Counter| counter.inc()).await
    }
    async fn value(&self) -> Result<usize, async_object::Error> {
        self.0.call(|counter: &Counter| counter.value()).await
    }
}

struct KCounter(Keeper<Counter>);

impl KCounter {
    fn new() -> Self {
        KCounter(Keeper::new(Counter::new()))
    }
    fn handle(&self) -> HCounter {
        HCounter(self.0.handle())
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

// #[test]
// fn test_handle_call_mut() {
//     let value = Rc::new(RefCell::new(0));
//     let value_r = value.clone();
//     let mut pool = Pool::new();
//     let hcounter = HCounter::new(&mut pool);
//     let future = async move {
//         hcounter.inc().await?;
//         let v = hcounter.value().await?;
//         *(value.borrow_mut()) = v;
//         Ok(())
//     };
//     async_object::spawn::<async_object::Error, _>(pool.spawner(), future);
//     pool.run_until_stalled();
//     assert!(*value_r.borrow() == 1)
// }

// fn test_send_receive_event() {}
