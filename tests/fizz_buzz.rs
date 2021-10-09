use std::sync::{mpsc::channel, Arc, RwLock};

use async_object::{Refefence, Keeper};
use futures::{executor::ThreadPool, join, task::SpawnExt, Stream, StreamExt};

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
struct HSink(Refefence<Sink>);

impl HSink {
    async fn set_value(&self, pos: usize, value: FizzBuzz) -> Option<()> {
        self.0.call_mut(|sink| sink.set_value(pos, value)).await
    }
}

struct KSink(Keeper<Sink>);

impl KSink {
    pub fn new() -> Self {
        Self(Keeper::new(Sink::new()))
    }
    pub fn handle(&self) -> HSink {
        HSink(self.0.reference())
    }
}

impl AsRef<Arc<RwLock<Sink>>> for KSink {
    fn as_ref(&self) -> &Arc<RwLock<Sink>> {
        self.0.as_ref()
    }
}

struct Generator;

#[derive(Clone)]
struct HGenerator(Refefence<Generator>);
impl HGenerator {
    fn values(&self) -> impl Stream<Item = usize> {
        self.0.receive_events()
    }
    fn send_value(&self, value: usize) {
        self.0.send_event(value)
    }
}

struct KGenerator(Keeper<Generator>);
impl KGenerator {
    pub fn new() -> Self {
        Self(Keeper::new(Generator {}))
    }
    pub fn handle(&self) -> HGenerator {
        HGenerator(self.0.reference())
    }
}

#[test]
fn fizz_buzz_threadpool() {
    let pool = ThreadPool::builder() /*.pool_size(8)*/
        .create()
        .unwrap();
    let ksink = KSink::new();
    let hsink = ksink.handle();
    {
        let kgenerator = KGenerator::new();
        let hgenerator = kgenerator.handle();
        let task_nums = {
            let mut values = hgenerator.values();
            let hsink = hsink.clone();
            pool.spawn_with_handle(async move {
                while let Some(n) = values.next().await {
                    hsink.set_value(n, FizzBuzz::Number).await.unwrap();
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
                        hsink.set_value(n, FizzBuzz::Fizz).await.unwrap();
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
                        hsink.set_value(n, FizzBuzz::Buzz).await.unwrap();
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
                        hsink.set_value(n, FizzBuzz::FizzBuzz).await.unwrap();
                    }
                }
            })
            .unwrap()
        };

        pool.spawn_ok(async move {
            for n in 1..100 {
                hsink.set_value(n, FizzBuzz::Expected).await.unwrap();
                hgenerator.send_value(n);
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
    assert!(ksink.as_ref().read().unwrap().validate());
}
