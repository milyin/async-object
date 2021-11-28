# async-object
This library provides framework for intraction of multiple objects in asynchronous environment wihout strict borrow-checker imposed lifetime restictions. Each object is holded by it's own asynchronous event loop and accessed by clonable wrapper **Tag\<S\>**. Objects can call each other methods and send/receive events.
