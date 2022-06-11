//! # Async Object
//!
//! This crate provides reference-counting wrappers and support for event publishing/subscription
//! for using objects in multithread asynchronous environment.
//!
//! The main purpose of the library is to provide foundation for my experimental GUI library
//! [WAG](https://github.com/milyin/wag), but it's abstract enough to be used anywhere else.
//!
//! Library implements [CArc<T>](CArc) wrapper containing ```Arc<std::sync::RwLock<T>``` inside.
//! ```CArc``` provides methods for accessing ```T``` both in synchronous and asynchronous ways.
//! Sync methods [CArc::call] blocks calling thread, async ones [CArc::async_call] releases task if wrapper object
//! is locked, allowing other async tasks to continue.
//!
//! ```
//! use async_object::CArc;
//! let obj = CArc::new(42 as usize);
//! obj.call_mut(|v| *v += 1 );
//! ```
//!
//! Library also provides event queue object [EArc] which allows to send events and subscribe to async streams of them.
//!
//! ## Usage example
//! ```
//! #[derive(Clone, Default)]
//! struct DeepThought {
//!     answer: usize,
//! }
//!
//! impl DeepThought {
//!     fn status(&self) -> usize {
//!         return self.answer;
//!     }
//!     fn think(&mut self) -> Option<usize> {
//!         std::thread:sleep(time::Duration::from_mills(1000));
//!         self.answer += 1;
//!         if self.answer == 42 {
//!             return Some(self.answer)
//!         } else {
//!             return None;
//!         }
//!     }
//! }
//!
//! let deep_thought = CArc::new(DeepThought::default());
//! let pool = ThreadPool::builder().create().unwrap();
//! pool.spawn_ok(async move {
//!     loop {    
//!         if let Some(answer) = match deep_thought.async_call_mut(|v| v.think()).await {
//!             println("The Answer Is {}", answer)   
//!         }
//!     }
//!})
//!
//! ```
//!
//!
//! # Event ordering
//!
//! Each subcriber (subscriber = asyncrhonous task reading event stream) uses it's own instance of event stream. Streams are fed by 'send_event'
//! and when event is sent each subscriber may pick it at different moments. I.e. when events A and B are sent, one subscriber may handle
//! both while another subscriber haven't handled any. That means that B is handled by subscriber 1 earlier than A is handled by subscriber 2.
//! Sometimes this is not desirable.
//!
//! This problem is solved by reference-counting Event wrapper for actual events. All suscribers revceives clone of same Event. The asynchronous
//! send_event method is blocked hold until all these clones are dropped. This allows to pause before sending next event until the moment when
//! previous event is fully processed by subscribers.
//!
//! If it's enough to just send event and forget about it, post_event method may be used.
//!
//! Event subscribers may fire other events. For example we may have mouse click handler which sends button press events if click
//! occurs on the button. It may be important to guarantee that button click events are not handled in order different than mouse clicks order.
//!
//! For example consider two buttons A and B, both subscribed to mouse events C, each in it's own task. Click event C1 causes button A send press
//! event P1, click C2 causes button B send press event P2. It's guaranteed that send_enent(C2) occures only when all instances of C1 are destroyed.
//! So it's guaranteed that P2 is *sent* after P1 (because P2 is reaction to C2, which appears in stream only after all subscribers processed C1).
//!
//! But there is still no guarantee that P2 is *handled* after P1. They are sent from independent streams and it's easy to imagine situation when
//! some subscriber for A button is frozen and handles his istance of P1 event long after the moment when B subsciber already handled P2.
//!
//! This may be inappropriate. For example: user presses "Apply" button and then "Close" button in the dialog. "Close" button is handled earler,
//! than "Apply". It's subscriber destroys the whole dialog. "Apply"'s subcriber have nothing to do. User's data is lost.
//!
//! To avoid this the concept of "source" event is added. The functions send_event and post_event have the additional optional parameter -
//! event which caused the sent one. Reference to this 'source' event is saved inside Event wrapper of new event and therefore send_event which
//! sent this source event is blocked until all derived events are dropped. So C2 click in example above is sent only when all instances of P1
//! are destroyed. So click on "Close" button is sent only after "Apply" press button event is handled.  
//!

mod carc;
mod earc;

pub use carc::CArc;
pub use carc::WCArc;
pub use earc::EArc;
pub use earc::Event;
pub use earc::EventBox;
pub use earc::EventStream;
pub use earc::WEArc;
