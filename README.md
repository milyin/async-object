# async-object
This library allows to avoid borrow checker restrictions by accessing objects by handles and interacting between them using message passing.

Library proviese pair of wrappers for normal Rust struct S: Handle<S> and Keeper<S>. Keeper takes ownership of structure. Handle provides exactly the same methods as structure, 
but in async context. Also these methods can fail when structure is destroyed.

Additionally handle provides methods for broadcasting and subcribing to custom events. Events are broadcasted through as asyncronous streams.
