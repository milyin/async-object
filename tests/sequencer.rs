use async_object::EventSequencer;
use futures::{executor::ThreadPool, StreamExt};

#[test]
fn sequencer_test() {
    let pool = ThreadPool::new().unwrap();
    let mut sequencer = EventSequencer::new();
    let mut tsequencer = sequencer.tag();
    pool.spawn_ok(async move {
        while let Some(event) = sequencer.next().await {
            if let Some(value) = event.get_event::<usize>() {
                dbg!(&value);
                println!("{}", &value);
            }
        }
    });
    for n in 1usize..100 {
        let _ = tsequencer.send_blocking_event(n);
    }
    let _ = tsequencer.close();
}
