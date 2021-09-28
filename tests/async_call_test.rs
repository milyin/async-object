use futures::{
    executor::{LocalPool, LocalSpawner},
    task::LocalSpawnExt,
};
use loopa::{self, EventSubscribers, Handle};
use std::{
    any::Any,
    cell::RefCell,
    rc::Rc,
    sync::{Arc, RwLock, Weak},
    task::Waker,
};

#[derive(Clone)]
enum CounterEvent {
    Incremented,
}
struct Counter {
    value: usize,
    subscribers: Arc<RwLock<EventSubscribers>>,
    call_wakers: Arc<RwLock<Vec<Waker>>>,
    object: Weak<RwLock<Counter>>,
}

impl Counter {
    pub fn new() -> Arc<RwLock<Self>> {
        let this = Self {
            value: 0,
            subscribers: Arc::new(RwLock::new(EventSubscribers::new())),
            call_wakers: Arc::new(RwLock::new(Vec::new())),
            object: Weak::new(),
        };
        let pthis = Arc::new(RwLock::new(this));
        let weak_pthis = Arc::downgrade(&pthis);
        pthis.write().unwrap().object = weak_pthis;
        pthis
    }
    pub fn handle(&self) -> HCounter {
        let object = self.object.clone() as Weak<RwLock<dyn Any>>;
        let subscribers = Arc::downgrade(&self.subscribers);
        let call_wakers = Arc::downgrade(&self.call_wakers);
        HCounter(Handle::new(object, subscribers, call_wakers))
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
    let hcounter = counter.read().unwrap().handle();
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
