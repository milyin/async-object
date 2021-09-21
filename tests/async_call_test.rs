use loopa::{self, Handle, Pool};
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

#[derive(Clone)]
struct HCounter(Handle);

impl HCounter {
    pub fn new(pool: &mut Pool) -> Self {
        HCounter(pool.register_object(Counter::new()))
    }
    pub async fn inc(&self) -> Result<(), loopa::Error> {
        self.0.call_mut(|counter: &mut Counter| counter.inc()).await
    }
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
    let future = async move {
        let v = hcounter.value().await?;
        *(value.borrow_mut()) = Some(v);
        Ok(())
    };
    loopa::spawn::<loopa::Error, _>(pool.spawner(), future);
    pool.run_until_stalled();
    assert!(value_r.borrow().is_some())
}

#[test]
fn handle_call_mut() {
    let value = Rc::new(RefCell::new(0));
    let value_r = value.clone();
    let mut pool = Pool::new();
    let hcounter = HCounter::new(&mut pool);
    let future = async move {
        hcounter.inc().await?;
        let v = hcounter.value().await?;
        *(value.borrow_mut()) = v;
        Ok(())
    };
    loopa::spawn::<loopa::Error, _>(pool.spawner(), future);
    pool.run_until_stalled();
    assert!(*value_r.borrow() == 1)
}
