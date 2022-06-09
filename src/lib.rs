//! # Async Object
//!
//! This crate provides reference-counting wrappers and support for event publishing/subscription
//! for using objects in multithread asynchronous environment.
//!
//! The main purpose of the library is to provide foundation for my experimental GUI library
//! [WAG](https://github.com/milyin/wag), but it's abstract enough to be used anywhere else.
//!
//! Library implements [CArc<T>](CArc) wrapper containing```Arc<RwLock<T>``` inside.
//! ```CArc``` provides methods for accessing ```T``` both in synchronous and asychronous environment.
//! Sync methods [CArc::call] blocks calling thread, async ones [CArc::async_call] releases task if wrapper object
//! is locked, allowing other async tasks to continue.
//!
//! Example:
//!
//! ```
//! use async_object::CArc;
//! let obj = CArc::new(42 as usize);
//! obj.call_mut(|v| *v += 1 );
//! ```
//!
//! Library also provides event queue [EArc] which allows to subscribe to async stream of events.
//!
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
