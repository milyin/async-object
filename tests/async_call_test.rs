use async_object::{CArc, EArc, EventStream, WCArc};
use async_std::future::timeout;
use async_std::task::sleep;
use futures::stream::select;
use futures::{executor::LocalPool, task::LocalSpawnExt, StreamExt};
use std::time::Duration;
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

#[test]
fn test_send_event() {
    let mut pool = LocalPool::new();
    let earc = EArc::new();
    let mut numbers = EventStream::<usize>::new(&earc);
    pool.spawner()
        .spawn_local(async move {
            earc.send_event(1 as usize).await;
            earc.send_event(2 as usize).await;
            earc.send_event(3 as usize).await;
            // last copy of earc is dropped here, make sure of it
            let wearc = earc.downgrade();
            drop(earc);
            assert!(wearc.upgrade().is_none());
        })
        .unwrap();
    pool.spawner()
        .spawn_local({
            async move {
                timeout(Duration::from_secs(5), async move {
                    let n1 = numbers.next().await.unwrap();
                    assert!(*n1.as_ref() == 1);
                    drop(n1);
                    let n2 = numbers.next().await.unwrap();
                    assert!(*n2.as_ref() == 2);
                    let no_n3 = timeout(Duration::from_millis(1), numbers.next()).await;
                    assert!(no_n3.is_err()); // Sending next event blocked by not dropped previous one
                    drop(n2);
                    let n3 = numbers.next().await.unwrap();
                    assert!(*n3.as_ref() == 3);
                    drop(n3);
                    // earc is dropped, stream returns None
                    assert!(numbers.next().await.is_none());
                })
                .await
                .unwrap()
            }
        })
        .unwrap();
    pool.run();
}

#[test]
fn test_send_dependent_event() {
    let mut pool = LocalPool::new();
    {
        let source = EArc::new();
        let evens = EArc::new();
        let odds = EArc::new();

        // Send source events - sequence of numbers
        pool.spawner()
            .spawn_local({
                let source = source.clone();
                async move {
                    for n in 0usize..10 {
                        source.send_event(n).await;
                    }
                }
            })
            .unwrap();

        // Read events from source and resend only even ones
        pool.spawner()
            .spawn_local({
                let evens = evens.clone();
                let mut src = EventStream::<usize>::new(&source);
                async move {
                    while let Some(en) = src.next().await {
                        let n = *en.as_ref();
                        // Release source event and skip forward other task to provoke disorder if dependent events does't work
                        // Comment 'send_dependent_event', uncomment 'send_event' and 'drop_en' to make test fail
                        // TODO: Make this fail part of test
                        // drop(en);
                        sleep(Duration::from_millis(1)).await;
                        if n % 2 == 0 {
                            // evens.send_event(n).await;
                            evens.send_derived_event(n, en).await;
                        }
                    }
                }
            })
            .unwrap();

        // Read events from source and resend only odd ones
        pool.spawner()
            .spawn_local({
                let odds = odds.clone();
                let mut src = EventStream::<usize>::new(&source);
                async move {
                    while let Some(en) = src.next().await {
                        let n = *en.as_ref();
                        // drop(en); -- see comments above
                        if n % 2 != 0 {
                            // odds.send_event(n).await;
                            odds.send_derived_event(n, en).await;
                        }
                    }
                }
            })
            .unwrap();

        pool.spawner()
            .spawn_local({
                let evens = EventStream::<usize>::new(&evens);
                let odds = EventStream::<usize>::new(&odds);
                let mut ns = select(evens, odds);
                async move {
                    timeout(Duration::from_secs(5), async move {
                        let mut expect = 0;
                        while let Some(en) = ns.next().await {
                            let n = *en.as_ref();
                            assert!(n == expect);
                            expect += 1;
                        }
                    })
                    .await
                    .unwrap()
                }
            })
            .unwrap();
    }
    pool.run();
}
