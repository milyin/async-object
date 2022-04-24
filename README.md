# Async Object

This crate provides reference-counting wrappers with event support for using objects in asynchronous environment

The main purpose of the library is to provide foundation for my experimental GUI library
[WAG](https://github.com/milyin/wag), but it's abstract enough to be used anywhere else.

# Example

This code makes wrappers Background and WBackground for BackgroundImpl object. Internally they are just
Arc<Rwlock<BackgroundImpl>> and Weak<Rwlock<BackgroundImpl>> plus tooling for access the Rwlock without blocking
asyncronous job.

 ```
#[async_object_decl(pub Background, pub WBackground)]
struct BackgroundImpl {
    color: Color
}

#[async_object_impl(Background,  WBackground)]
impl BackgroundImpl {
    pub fn set_color(&mut this, color: Color) {
        this.color = color
    }
    pub fn get_color(&this) -> Color {
        this.color
    }
}

impl Background {
    pub fn new() -> Background {
        Background::create(BackgroundImpl { color: Color::White })
    }
}
```

The structures Background and WBackground will have these automatically generated proxy methods:

```
impl Background {
    pub fn set_color(...);
    pub fn get_golor() -> Color;
    async pub fn async_set_color(...);
    async pub fn async_get_color(...) -> Color
}

impl WBackground {
    pub fn set_color(...) -> Option<()>;
    pub fn get_golor() -> Option<Color>;
    async pub fn async_set_color(...) -> Option<()>;
    async pub fn async_get_color(...) -> Option<Color>
}
 ```
Note Option return type for weak wrapper WBackground. If all instance of Background are destroyed,
the BackgroundImpl is dropped too. Remaining WBackground instances starts to return None for all method calls.

The difference between normal and async proxy methods is in their way to mutex access. The synchronous methods are just
waiting for mutex unlock. The async methods tries to access the mutex and if it's locked puts asynchronous task to sleep
until mutex is released, allowing other tasks to continue on this worker.
 
There is also event pub/sub support. For example:
```
enum ButtonEvent { Press, Release }

#[async_object_with_events_decl(pub Button, pub WButton)]
struct ButtonImpl {
}

#[async_object_impl(Button, WButton)]
impl ButtonImpl {
    async pub fn async_press(&mut self) {
        self.send_event(ButtonEvent::Press).await
    }
    async pub fn press(&mut self) {
        let _ = self.send_event(ButtonEvent::Press)
    }
    pub fn events(&self) -> EventStream<ButtonEvent> {
        self.create_event_stream()
    }
}
```

Below is the code which changes background color when button is pressed

```
let pool = ThreadPool::builder().create().unwrap();
let button = Button::new();
let background = Background::new();

pool.spawn({
    let events = button.events();
    async move {
        // As soon as button is destroyed stream returns None
        while let Some(event) = events.next().await {
            // event has type Event<ButtonEvent>
            match *event.as_ref() {
                ButtonEvent::Pressed => background.set_color(Color::Red).await,
                ButtonEvent::Released => background.set_color(Color::White).await,
            }
        }
    }
});

```

It's important to emphasize the role of Event wrapper (note that in code above stream provides Event<ButtonEvent> instances)
and asyncness of send_event method. The future returned by send_event is allowed to continue only when all subscribers got their instances of
Event<ButtonEvent> *and* these instances are dropped. This allows to pause before sending next event until the moment when previous event is fully processed
by subscribers.
  
If user is not interested in result of sending the event the returned future may just be dropped as in 'press' method above. Event itself will be sent anyway.
