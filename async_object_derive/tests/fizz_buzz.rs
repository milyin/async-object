use std::sync::mpsc::channel;

use async_object::EventStream;
use async_object_derive::{async_object_decl, async_object_impl, async_object_with_events_decl};
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

#[async_object_decl(Sink, WSink)]
struct SinkImpl {
    values: Vec<Option<FizzBuzz>>,
}

impl SinkImpl {
    fn new() -> Self {
        Self { values: Vec::new() }
    }
}

impl Sink {
    fn new() -> Self {
        Sink::create(SinkImpl::new())
    }
}

#[async_object_impl(Sink, WSink)]
impl SinkImpl {
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

#[async_object_with_events_decl(Generator, WGenerator)]
struct GeneratorImpl;

impl Generator {
    pub fn new() -> Self {
        Generator::create(GeneratorImpl)
    }
    fn values(&self) -> EventStream<usize> {
        self.create_event_stream()
    }
    async fn send_value(&mut self, value: usize) {
        self.send_event(value).await
    }
}

async fn fizz_buzz_test(pool: impl Spawn + Clone, sink: Sink) {
    let mut generator = Generator::new();
    let task_nums = {
        let mut sink = sink.clone();
        let mut values = generator.values();
        pool.spawn_with_handle(async move {
            while let Some(n) = values.next().await {
                let n = *n.as_ref();
                sink.async_set_value(n, FizzBuzz::Number).await
            }
        })
        .unwrap()
    };
    let task_fizz = {
        let mut sink = sink.clone();
        let mut values = generator.values();
        pool.spawn_with_handle(async move {
            while let Some(n) = values.next().await {
                let n = *n.as_ref();
                if n % 3 == 0 {
                    sink.async_set_value(n, FizzBuzz::Fizz).await
                }
            }
        })
        .unwrap()
    };
    let task_buzz = {
        let mut tsink = sink.clone();
        let mut values = generator.values();
        pool.spawn_with_handle(async move {
            while let Some(n) = values.next().await {
                let n = *n.as_ref();
                if n % 5 == 0 {
                    tsink.async_set_value(n, FizzBuzz::Buzz).await
                }
            }
        })
        .unwrap()
    };
    let task_fizzbuzz = {
        let mut tsink = sink.clone();
        let mut values = generator.values();
        pool.spawn_with_handle(async move {
            while let Some(n) = values.next().await {
                let n = *n.as_ref();
                if n % 5 == 0 && n % 3 == 0 {
                    tsink.async_set_value(n, FizzBuzz::FizzBuzz).await
                }
            }
        })
        .unwrap()
    };

    pool.spawn({
        let mut sink = sink.clone();
        async move {
            for n in 1..100 {
                sink.async_set_value(n, FizzBuzz::Expected).await;
                generator.send_value(n).await;
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
