# Async Object

This crate provides reference-counting wrappers and support for event publishing/subscription
for using objects in multithread asynchronous environment.

The main purpose of the crate is to provide foundation for my experimental GUI library
[WAG](https://github.com/milyin/wag), but it's abstract enough to be used anywhere else.

See more detailed documentation at docs.rs: [async_object](https://docs.rs/async_object/0.1.1/async_object/) 

Library implements ```CArc<T>``` wrapper containing ```Arc<RwLock<T>``` inside.
```CArc``` provides methods for accessing ```T``` both in synchronous and asynchronous ways.

## Example:

```
use async_object::CArc;
let obj = CArc::new(42 as usize);
obj.call_mut(|v| *v += 1 );
```

Library also provides event queue object ```EArc``` which allows to send events and subscribe to async streams of them.
