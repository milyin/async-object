# Async Object Derive

This crate provides macros for automatically generating wrappers to make wrapped objects work in asynchronous anvironment
and exchange messages through asynchronous streams. The macros are based on primitives from 
[async_object](https://docs.rs/async_object/0.1.1/async_object/) library. The purpose of library was to be basement for 
developing native Windows GUI library [WAG](https://github.com/milyin/wag). Currently the ```WAG``` library was
switched to direct usage of ```async_object``` primitives, so this library may be considered obsolete at this moment.

# Example

This code makes wrappers Background and WBackground for BackgroundImpl object. Internally they are just
Arc\<Rwlock\<BackgroundImpl\>\> and Weak\<Rwlock\<BackgroundImpl\>\> plus tooling for access the Rwlock without blocking
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
    pub fn press(&mut self) {
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
