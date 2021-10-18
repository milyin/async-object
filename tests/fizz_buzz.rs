use std::sync::{mpsc::channel, Arc, RwLock, RwLockReadGuard};

use async_object::{EventStream, Keeper, Tag};
use futures::{
    executor::{LocalPool, ThreadPool},
    join,
    task::SpawnExt,
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

struct Sink {
    values: Vec<Option<FizzBuzz>>,
}

impl Sink {
    pub fn new() -> Self {
        Self { values: Vec::new() }
    }
    pub fn set_value(&mut self, pos: usize, value: FizzBuzz) {
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
    pub fn validate(&self) -> bool {
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
struct HSink(Tag<Sink>);

impl HSink {
    async fn async_set_value(&self, pos: usize, value: FizzBuzz) -> async_object::Result<()> {
        self.0
            .async_call_mut(|sink| sink.set_value(pos, value))
            .await
    }
    fn set_value(&self, pos: usize, value: FizzBuzz) -> async_object::Result<()> {
        self.0.call_mut(|sink| sink.set_value(pos, value))
    }
}

#[derive(Clone)]
struct KSink(Keeper<Sink>);

impl KSink {
    pub fn new() -> Self {
        Self(Keeper::new(Sink::new()))
    }
    pub fn tag(&self) -> HSink {
        HSink(self.0.tag())
    }
    pub fn get(&self) -> RwLockReadGuard<'_, Sink> {
        self.0.get()
    }
}

impl AsRef<Arc<RwLock<Sink>>> for KSink {
    fn as_ref(&self) -> &Arc<RwLock<Sink>> {
        self.0.as_ref()
    }
}

struct Generator;

#[derive(Clone)]
struct HGenerator(Tag<Generator>);
impl HGenerator {
    fn values(&self) -> EventStream<usize> {
        EventStream::new(self.0.clone())
    }
}

struct KGenerator(Keeper<Generator>);
impl KGenerator {
    pub fn new() -> Self {
        Self(Keeper::new(Generator {}))
    }
    pub fn handle(&self) -> HGenerator {
        HGenerator(self.0.tag())
    }
    fn send_value(&self, value: usize) {
        self.0.send_event(value)
    }
}

#[test]
fn fizz_buzz_threadpool_async_call() {
    let pool = ThreadPool::builder() /*.pool_size(8)*/
        .create()
        .unwrap();
    let ksink = KSink::new();
    let hsink = ksink.tag();
    {
        let kgenerator = KGenerator::new();
        let hgenerator = kgenerator.handle();
        let task_nums = {
            let mut values = hgenerator.values();
            let hsink = hsink.clone();
            pool.spawn_with_handle(async move {
                while let Some(n) = values.next().await {
                    hsink.async_set_value(n, FizzBuzz::Number).await.unwrap();
                }
            })
            .unwrap()
        };
        let task_fizz = {
            let hsink = hsink.clone();
            let mut values = hgenerator.values();
            pool.spawn_with_handle(async move {
                while let Some(n) = values.next().await {
                    if n % 3 == 0 {
                        hsink.async_set_value(n, FizzBuzz::Fizz).await.unwrap();
                    }
                }
            })
            .unwrap()
        };
        let task_buzz = {
            let hsink = hsink.clone();
            let mut values = hgenerator.values();
            pool.spawn_with_handle(async move {
                while let Some(n) = values.next().await {
                    if n % 5 == 0 {
                        hsink.async_set_value(n, FizzBuzz::Buzz).await.unwrap();
                    }
                }
            })
            .unwrap()
        };
        let task_fizzbuzz = {
            let hsink = hsink.clone();
            let mut values = hgenerator.values();
            pool.spawn_with_handle(async move {
                while let Some(n) = values.next().await {
                    if n % 5 == 0 && n % 3 == 0 {
                        hsink.async_set_value(n, FizzBuzz::FizzBuzz).await.unwrap();
                    }
                }
            })
            .unwrap()
        };

        pool.spawn_ok(async move {
            for n in 1..100 {
                hsink.async_set_value(n, FizzBuzz::Expected).await.unwrap();
                kgenerator.send_value(n);
            }
            drop(kgenerator);
        });

        let (tx, rx) = channel();
        pool.spawn_ok(async move {
            join!(task_nums, task_fizz, task_buzz, task_fizzbuzz);
            tx.send(()).unwrap();
        });
        let _ = rx.recv().unwrap();
    }
    assert!(ksink.get().validate());
}

#[test]
fn fizz_buzz_localpool_sync_call() {
    let mut pool = LocalPool::new();
    let spawner = pool.spawner();
    let ksink = KSink::new();
    let hsink = ksink.tag();
    {
        let kgenerator = KGenerator::new();
        let hgenerator = kgenerator.handle();
        {
            let mut values = hgenerator.values();
            let hsink = hsink.clone();
            spawner
                .spawn(async move {
                    while let Some(n) = values.next().await {
                        hsink.set_value(n, FizzBuzz::Number).unwrap();
                    }
                })
                .unwrap()
        };
        {
            let hsink = hsink.clone();
            let mut values = hgenerator.values();
            spawner
                .spawn(async move {
                    while let Some(n) = values.next().await {
                        if n % 3 == 0 {
                            hsink.set_value(n, FizzBuzz::Fizz).unwrap();
                        }
                    }
                })
                .unwrap()
        };
        {
            let hsink = hsink.clone();
            let mut values = hgenerator.values();
            spawner
                .spawn(async move {
                    while let Some(n) = values.next().await {
                        if n % 5 == 0 {
                            hsink.set_value(n, FizzBuzz::Buzz).unwrap();
                        }
                    }
                })
                .unwrap()
        };
        {
            let hsink = hsink.clone();
            let mut values = hgenerator.values();
            spawner
                .spawn(async move {
                    while let Some(n) = values.next().await {
                        if n % 5 == 0 && n % 3 == 0 {
                            hsink.async_set_value(n, FizzBuzz::FizzBuzz).await.unwrap();
                        }
                    }
                })
                .unwrap()
        };

        spawner
            .spawn(async move {
                for n in 1..100 {
                    hsink.async_set_value(n, FizzBuzz::Expected).await.unwrap();
                    kgenerator.send_value(n);
                }
                drop(kgenerator);
            })
            .unwrap();

        pool.run();
    }
    assert!(ksink.get().validate());
}
