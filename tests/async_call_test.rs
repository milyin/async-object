use futures::{
    executor::{LocalPool, LocalSpawner},
    task::LocalSpawnExt,
};
use loopa::{self, EventSubscribers, Handle};
use std::{
    any::Any,
    cell::RefCell,
    rc::{Rc, Weak},
};

#[derive(Clone)]
enum CounterEvent {
    Incremented,
}
struct Counter {
    value: usize,
    subscribers: Rc<RefCell<EventSubscribers>>,
    object: Weak<RefCell<Counter>>,
}

impl Counter {
    pub fn new() -> Rc<RefCell<Self>> {
        let this = Self {
            value: 0,
            subscribers: Rc::new(RefCell::new(EventSubscribers::new())),
            object: Weak::new(),
        };
        let pthis = Rc::new(RefCell::new(this));
        let weak_pthis = Rc::downgrade(&pthis);
        pthis.borrow_mut().object = weak_pthis;
        pthis
    }
    pub fn handle(&self) -> HCounter {
        let object = self.object.clone() as Weak<RefCell<dyn Any>>;
        let subscribers = Rc::downgrade(&self.subscribers);
        HCounter(Handle::new(object, subscribers))
    }
    fn inc(&mut self) {
        self.value += 1;
    }
    fn value(&self) -> usize {
        self.value
    }
}

#[derive(Clone)]
struct HCounter(Handle);

impl HCounter {
    pub async fn inc(&self) -> Result<(), loopa::Error> {
        self.0.call_mut(|counter: &mut Counter| counter.inc()).await
    }
    pub async fn value(&self) -> Result<usize, loopa::Error> {
        self.0.call(|counter: &Counter| counter.value()).await
    }
}

#[test]
fn test_handle_call() {
    let value = Rc::new(RefCell::new(None));
    let value_r = value.clone();
    let counter = Counter::new();
    let hcounter = counter.borrow().handle();
    let future = async move {
        let v = hcounter.value().await.unwrap();
        *(value.borrow_mut()) = Some(v);
    };
    let mut pool = LocalPool::new();
    pool.spawner().spawn_local(future).unwrap();
    pool.run_until_stalled();
    assert!(value_r.borrow().is_some())
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
//     loopa::spawn::<loopa::Error, _>(pool.spawner(), future);
//     pool.run_until_stalled();
//     assert!(*value_r.borrow() == 1)
// }

// fn test_send_receive_event() {}
