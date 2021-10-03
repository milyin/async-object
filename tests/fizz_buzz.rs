use std::sync::{mpsc::channel, Arc, RwLock};

use futures::{executor::ThreadPool, join, task::SpawnExt, Stream, StreamExt};
use loopa::{Handle, HandleSupport};

#[derive(Copy, Clone, PartialEq, PartialOrd, Debug)]
enum FizzBuzz {
    Number,
    Fizz,
    Buzz,
    FizzBuzz,
}

struct Sink {
    values: Vec<Option<FizzBuzz>>,
    hsupport: HandleSupport<Self>,
}

impl Sink {
    pub fn new() -> (Arc<RwLock<Self>>, HSink) {
        let this = Arc::new(RwLock::new(Self {
            values: Vec::new(),
            hsupport: HandleSupport::new(),
        }));
        let handle = HSink(this.write().unwrap().hsupport.init(&this));
        (this, handle)
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
        dbg!(pos, value);
    }
}

#[derive(Clone)]
struct HSink(Handle<Sink>);

impl HSink {
    pub async fn set_value(&self, pos: usize, value: FizzBuzz) -> Result<(), loopa::Error> {
        self.0
            .call_mut(|sink: &mut Sink| sink.set_value(pos, value))
            .await
    }
}

#[derive(Debug)]
struct Generator {
    hsupport: HandleSupport<Self>,
}

impl Generator {
    pub fn new() -> (Arc<RwLock<Self>>, HGenerator) {
        let this = Arc::new(RwLock::new(Self {
            hsupport: HandleSupport::new(),
        }));
        let handle = HGenerator(this.write().unwrap().hsupport.init(&this));
        (this, handle)
    }
    pub fn send_value(&mut self, value: usize) {
        self.hsupport.send_event::<usize>(value)
    }
}

#[derive(Clone)]
struct HGenerator(Handle<Generator>);

impl HGenerator {
    pub fn values(&self) -> impl Stream<Item = usize> {
        self.0.get_event::<usize>()
    }
    pub async fn send_value(&self, value: usize) -> Result<(), loopa::Error> {
        self.0
            .call_mut(|generator: &mut Generator| generator.send_value(value))
            .await
    }
}

#[test]
fn fizz_buzz_threadpool() {
    let pool = ThreadPool::builder() /*.pool_size(8)*/
        .create()
        .unwrap();
    let (_sink, hsink) = Sink::new();
    {
        let (generator, hgenerator) = Generator::new();
        let task_nums = {
            let mut values = hgenerator.values();
            let hsink = hsink.clone();
            pool.spawn_with_handle(async move {
                dbg!("Start nums");
                while let Some(n) = values.next().await {
                    hsink.set_value(n, FizzBuzz::Number).await.unwrap();
                }
                dbg!("End nums");
            })
            .unwrap()
        };
        let task_fizz = {
            let hsink = hsink.clone();
            let mut values = hgenerator.values();
            pool.spawn_with_handle(async move {
                dbg!("Start fizz");
                while let Some(n) = values.next().await {
                    if n % 3 == 0 {
                        hsink.set_value(n, FizzBuzz::Fizz).await.unwrap();
                    }
                }
                dbg!("End fizz");
            })
            .unwrap()
        };
        let task_buzz = {
            let hsink = hsink.clone();
            let mut values = hgenerator.values();
            pool.spawn_with_handle(async move {
                dbg!("Start buzz");
                while let Some(n) = values.next().await {
                    if n % 5 == 0 {
                        hsink.set_value(n, FizzBuzz::Buzz).await.unwrap();
                    }
                }
                dbg!("End buzz");
            })
            .unwrap()
        };
        let task_fizzbuzz = {
            let hsink = hsink.clone();
            let mut values = hgenerator.values();
            pool.spawn_with_handle(async move {
                dbg!("Start fizzbuzz");
                while let Some(n) = values.next().await {
                    if n % 5 == 0 && n % 3 == 0 {
                        hsink.set_value(n, FizzBuzz::FizzBuzz).await.unwrap();
                    }
                }
                dbg!("End fizzbuzz");
            })
            .unwrap()
        };

        pool.spawn_ok(async move {
            for n in 1..100 {
                dbg!(n);
                hgenerator.send_value(n).await.unwrap();
            }
            drop(generator);
        });

        let (tx, rx) = channel();
        pool.spawn_ok(async move {
            join!(task_nums, task_fizz, task_buzz, task_fizzbuzz);
            tx.send(()).unwrap();
        });
        let _ = rx.recv().unwrap();
    }
}
