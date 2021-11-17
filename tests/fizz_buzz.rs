use std::sync::mpsc::channel;

use async_object::{EventStream, Keeper, Tag};
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
                    return false;
                }
            }
        }
        true
    }
}

#[derive(Clone)]
struct TSink(Tag<SinkImpl>);

impl TSink {
    async fn set_value(&self, pos: usize, value: FizzBuzz) -> async_object::Result<()> {
        self.0
            .async_call_mut(|sink| sink.set_value(pos, value))
            .await
    }
}

struct Sink(Keeper<SinkImpl>);

impl Sink {
    pub fn new() -> Self {
        Self(Keeper::new(SinkImpl::new()))
    }
    pub fn tag(&self) -> TSink {
        TSink(self.0.tag())
    }
    pub fn validate(&self) -> bool {
        self.0.get().validate()
    }
}

struct GeneratorImpl;

#[derive(Clone)]
struct TGenerator(Tag<GeneratorImpl>);
impl TGenerator {
    fn values(&self) -> EventStream<usize> {
        EventStream::new(self.0.clone())
    }
}

struct Generator(Keeper<GeneratorImpl>);
impl Generator {
    pub fn new() -> Self {
        Self(Keeper::new(GeneratorImpl {}))
    }
    pub fn tag(&self) -> TGenerator {
        TGenerator(self.0.tag())
    }
    fn send_value(&mut self, value: usize) {
        self.0.send_event(value)
    }
}

async fn fizz_buzz_test(pool: impl Spawn, tsink: TSink) {
    let mut generator = Generator::new();
    let tgenerator = generator.tag();
    let task_nums = {
        let mut values = tgenerator.values();
        let tsink = tsink.clone();
        pool.spawn_with_handle(async move {
            while let Some(n) = values.next().await {
                tsink.set_value(n, FizzBuzz::Number).await.unwrap();
            }
        })
        .unwrap()
    };
    let task_fizz = {
        let tsink = tsink.clone();
        let mut values = tgenerator.values();
        pool.spawn_with_handle(async move {
            while let Some(n) = values.next().await {
                if n % 3 == 0 {
                    tsink.set_value(n, FizzBuzz::Fizz).await.unwrap();
                }
            }
        })
        .unwrap()
    };
    let task_buzz = {
        let tsink = tsink.clone();
        let mut values = tgenerator.values();
        pool.spawn_with_handle(async move {
            while let Some(n) = values.next().await {
                if n % 5 == 0 {
                    tsink.set_value(n, FizzBuzz::Buzz).await.unwrap();
                }
            }
        })
        .unwrap()
    };
    let task_fizzbuzz = {
        let tsink = tsink.clone();
        let mut values = tgenerator.values();
        pool.spawn_with_handle(async move {
            while let Some(n) = values.next().await {
                if n % 5 == 0 && n % 3 == 0 {
                    tsink.set_value(n, FizzBuzz::FizzBuzz).await.unwrap();
                }
            }
        })
        .unwrap()
    };

    pool.spawn({
        let tsink = tsink.clone();
        async move {
            for n in 1..100 {
                tsink.set_value(n, FizzBuzz::Expected).await.unwrap();
                generator.send_value(n);
            }
            drop(generator);
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
    let handle = fizz_buzz_test(pool.clone(), sink.tag());

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
        .spawn_local(fizz_buzz_test(spawner.clone(), sink.tag()))
        .unwrap();
    pool.run();
    assert!(sink.validate());
}
