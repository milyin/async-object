//! # Async Object
//!
//! This crate provides reference-counting wrappers and support for event publishing/subscription
//! for using objects in multithread asynchronous environment.
//!
//! The main purpose of the library is to provide foundation for my experimental GUI library
//! [WAG](https://github.com/milyin/wag), but it's abstract enough to be used anywhere else.
//!
//! The library primitives (CArc, EArc, etc) ususally are not supposed to be used directly. Macros for generation
//! the wrapper structures should be employed instead.
//!
//! See documentation for [async_object_derive](https://docs.rs/async_object_derive/0.1.0/async_object_derive/) library for usage sample
//!
mod carc;
mod earc;

pub use carc::CArc;
pub use carc::WCArc;
pub use earc::EArc;
pub use earc::Event;
pub use earc::EventStream;
pub use earc::WEArc;
