use std::process::exit;

use async_object::CArc;
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
    let deep_thought = CArc::new(DeepThought::default());
    let pool = ThreadPool::builder().pool_size(3).create().unwrap();

    //
    // Many observers wants to know what Deep Thought is thinking about at this moment
    //
    for n in 1..10 {
        let deep_thought = deep_thought.clone();
        pool.spawn_ok(async move {
            loop {
                // println!(
                //     "{}th observer : He it thinking on {} !",
                //     n,
                //     deep_thought.async_call(|v| v.status()).await // deep_thought.call(|v| v.status())
                // );
                deep_thought
                    .async_call(|v| {
                        println!("{}th observer : He it thinking on {} !", n, v.status())
                    })
                    .await
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
                if let Some(answer) = deep_thought.async_call_mut(|v| v.think()).await {
                    // if let Some(answer) = deep_thought.call_mut(|v| v.think()) {
                    println!("Deep Thought: The Answer Is {}", answer);
                    exit(0)
                } else {
                    println!("Deep Thought is thinking...")
                }
            }
        }
    });
    std::thread::sleep(core::time::Duration::from_secs(100));
    println!("Answer not found!");
}
