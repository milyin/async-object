use std::{cell::RefCell, rc::Rc};

use futures::{executor::LocalSpawner, task::LocalSpawnExt};
use loopa::{self, Handle, Pool};

struct Counter {
    value: usize,
}

impl Counter {
    pub fn new() -> Self {
        Self { value: 0 }
    }
    pub fn inc(&mut self) {
        self.value += 1;
    }
    pub fn value(&self) -> usize {
        self.value
    }
}

struct HCounter(Handle);

impl HCounter {
    pub fn new(pool: &mut Pool) -> Self {
        HCounter(pool.register_object(Counter::new()))
    }
    pub fn spawner(&self) -> LocalSpawner {
        self.0.spawner()
    }
    // pub async fn inc(&self) -> Result<(), loopa::Error> {
    //     self.0.call(|counter: &mut Counter| counter.inc()).await
    // }
    pub async fn value(&self) -> Result<usize, loopa::Error> {
        self.0.call(|counter: &Counter| counter.value()).await
    }
}

#[test]
fn handle_call() {
    let value = Rc::new(RefCell::new(None));
    let value_r = value.clone();
    let mut pool = Pool::new();
    let hcounter = HCounter::new(&mut pool);
    hcounter
        .spawner()
        .spawn_local(async move { *(value.borrow_mut()) = Some(hcounter.value().await) })
        .unwrap();
    pool.run_until_stalled();
    assert!(value_r.borrow().is_some())
}
