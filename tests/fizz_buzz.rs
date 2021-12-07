use std::sync::mpsc::channel;

use async_object::{CArc, EArc, EventStream};
use futures::{
    executor::{LocalPool, ThreadPool},
    join,
    task::{LocalSpawnExt, Spawn, SpawnExt},
    StreamExt,
};

#[derive(Copy, Clone, PartialEq, PartialOrd, Debug)]
enum FizzBuzz {
    Expected,
    Number,
    Fizz,
    Buzz,
    FizzBuzz,
}

struct SinkImpl {
    values: Vec<Option<FizzBuzz>>,
}

impl SinkImpl {
    fn new() -> Self {
        Self { values: Vec::new() }
    }
    fn set_value(&mut self, pos: usize, value: FizzBuzz) {
        if pos >= self.values.len() {
            self.values.resize(pos, None);
            self.values.push(Some(value))
        } else if let Some(prev) = self.values[pos] {
            if value > prev {
                self.values[pos] = Some(value)
            }
        } else {
            self.values[pos] = Some(value)
        }
    }
    fn validate(&self) -> bool {
        for (n, res) in self.values.iter().enumerate() {
            if let Some(res) = res {
                let expected = match (n % 5 == 0, n % 3 == 0) {
                    (true, true) => FizzBuzz::FizzBuzz,
                    (true, false) => FizzBuzz::Buzz,
                    (false, true) => FizzBuzz::Fizz,
                    (false, false) => FizzBuzz::Number,
                };
                if *res != expected {
                    dbg!(n, *res);
                    return false;
                }
            }
        }
        true
    }
}

#[derive(Clone)]
struct Sink(CArc<SinkImpl>);

impl Sink {
    pub fn new() -> Self {
        Sink(CArc::new(SinkImpl::new()))
    }
    async fn set_value(&self, pos: usize, value: FizzBuzz) -> () {
        self.0
            .async_call_mut(|sink| sink.set_value(pos, value))
            .await
    }
    pub fn validate(&self) -> bool {
        self.0.call(|sink| sink.validate())
    }
}

#[derive(Clone)]
struct Generator(EArc);
impl Generator {
    pub fn new(pool: impl Spawn) -> Self {
        Generator(EArc::new(pool).unwrap())
    }
    fn values(&self) -> EventStream<usize> {
        EventStream::new(&self.0)
    }
    fn send_value(&mut self, value: usize) {
        self.0.send_event(value)
    }
}

async fn fizz_buzz_test(pool: impl Spawn + Clone, sink: Sink) {
    let mut generator = Generator::new(pool.clone());
    let task_nums = {
        let sink = sink.clone();
        let mut values = generator.values();
        pool.spawn_with_handle(async move {
            while let Some(n) = values.next().await {
                let n = *n.as_ref();
                sink.set_value(n, FizzBuzz::Number).await
            }
        })
        .unwrap()
    };
    let task_fizz = {
        let sink = sink.clone();
        let mut values = generator.values();
        pool.spawn_with_handle(async move {
            while let Some(n) = values.next().await {
                let n = *n.as_ref();
                if n % 3 == 0 {
                    sink.set_value(n, FizzBuzz::Fizz).await
                }
            }
        })
        .unwrap()
    };
    let task_buzz = {
        let tsink = sink.clone();
        let mut values = generator.values();
        pool.spawn_with_handle(async move {
            while let Some(n) = values.next().await {
                let n = *n.as_ref();
                if n % 5 == 0 {
                    tsink.set_value(n, FizzBuzz::Buzz).await
                }
            }
        })
        .unwrap()
    };
    let task_fizzbuzz = {
        let tsink = sink.clone();
        let mut values = generator.values();
        pool.spawn_with_handle(async move {
            while let Some(n) = values.next().await {
                let n = *n.as_ref();
                if n % 5 == 0 && n % 3 == 0 {
                    tsink.set_value(n, FizzBuzz::FizzBuzz).await
                }
            }
        })
        .unwrap()
    };

    pool.spawn({
        let sink = sink.clone();
        async move {
            for n in 1..100 {
                sink.set_value(n, FizzBuzz::Expected).await;
                generator.send_value(n);
            }
        }
    })
    .unwrap();

    pool.spawn_with_handle(async move {
        join!(task_nums, task_fizz, task_buzz, task_fizzbuzz);
    })
    .unwrap()
    .await
}

#[test]
fn fizz_buzz_threadpool_async_call() {
    let pool = ThreadPool::builder() /*.pool_size(8)*/
        .create()
        .unwrap();

    let sink = Sink::new();
    let handle = fizz_buzz_test(pool.clone(), sink.clone());

    let (tx, rx) = channel();
    pool.spawn_ok(async move {
        join!(handle);
        tx.send(()).unwrap();
    });
    let _ = rx.recv().unwrap();
    assert!(sink.validate());
}

#[test]
fn fizz_buzz_locapool_sync_call() {
    let mut pool = LocalPool::new();
    let spawner = pool.spawner();
    let sink = Sink::new();
    spawner
        .spawn_local(fizz_buzz_test(spawner.clone(), sink.clone()))
        .unwrap();
    pool.run();
    assert!(sink.validate());
}
