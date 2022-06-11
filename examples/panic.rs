use std::{process::exit, sync::Arc};

use async_std::sync::RwLock;
use futures::executor::ThreadPool;

#[derive(Default)]
struct DeepThought {
    answer: usize,
}

impl DeepThought {
    fn status(&self) -> usize {
        return self.answer;
    }
    fn think(&mut self) -> Option<usize> {
        std::thread::sleep(core::time::Duration::from_millis(100));
        self.answer += 1;
        if self.answer == 42 {
            return Some(self.answer);
        } else {
            return None;
        }
    }
}

fn main() {
    let deep_thought = Arc::new(RwLock::new(DeepThought::default()));
    let pool = ThreadPool::builder().pool_size(4).create().unwrap();
    //
    // Many observers wants to know what Deep Thought is thinking about at this moment
    //
    for i in 1..3 {
        let deep_thought = deep_thought.clone();
        pool.spawn_ok(async move {
            loop {
                let reader = deep_thought.read().await;
                std::thread::sleep(core::time::Duration::from_millis(10));
                let n = reader.status();
                drop(reader);
                println!("{}th observer : He it thinking on {} !", i, n,);
            }
        })
    }

    //
    // Deep Thought is thinking on Answer
    //
    pool.spawn_ok({
        let deep_thought = deep_thought.clone();
        async move {
            loop {
                let mut writer = deep_thought.write().await;
                if let Some(answer) = writer.think() {
                    println!("Deep Thought: The Answer Is {}", answer);
                    exit(0)
                } else {
                    println!("Deep Thought is thinking...")
                }
            }
        }
    });
    std::thread::sleep(core::time::Duration::from_secs(10));
    println!("Answer not found!");
}
