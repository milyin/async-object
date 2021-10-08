# async-object
This library allows to avoid borrow checker restrictions by accessing objects by handles and interacting between them using message passing.

Library provides a pair of wrappers for normal Rust structure S: Handle\<S\> and Keeper\<S\>. Keeper takes ownership of structure. Handle provides methods for accessing the structure in async context with option to fail when structure is destroyed.

Additionally handle provides methods for broadcasting and subcribing to custom events. Events are broadcasted through asyncronous streams.
