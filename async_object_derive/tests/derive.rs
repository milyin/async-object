use async_object_derive::{async_object_decl, async_object_impl, async_object_with_events_decl};
use futures::StreamExt;
use futures::{executor::LocalPool, task::SpawnExt};

#[async_object_decl(Test, WTest)]
struct TestImpl {
    foo: usize,
}

impl TestImpl {
    pub fn new() -> Self {
        Self { foo: 0 }
    }
}

#[async_object_impl(Test, WTest)]
impl TestImpl {
    fn test(&self) -> usize {
        self.foo
    }
    fn test_mut(&mut self, foo: usize) {
        self.foo = foo;
    }
}

#[test]
fn async_object_decl_test() {
    let mut test = Test::create(TestImpl::new());
    let mut wtest = test.downgrade();
    test.test_mut(42);
    assert!(test.test() == 42);
    assert!(wtest.upgrade().is_some());
    assert!(wtest.test_mut(43) == Some(()));
    assert!(wtest.test().unwrap() == 43);
}

#[async_object_with_events_decl(pub Test2, pub WTest2)]
struct Test2Impl {
    foo: usize,
}

impl Test2Impl {
    pub fn new() -> Self {
        Self { foo: 0 }
    }
}

#[async_object_impl(Test2, WTest2)]
impl Test2Impl {
    fn test(&self) -> usize {
        self.foo
    }
    fn test_mut(&mut self, foo: usize) {
        self.foo = foo;
    }
}

#[test]
fn async_object_with_events_decl_test() {
    let mut pool = LocalPool::new();
    let mut test = Test2::create(Test2Impl::new(), pool.spawner()).unwrap();
    let mut wtest = test.downgrade();
    test.test_mut(42);
    assert!(test.test() == 42);
    assert!(wtest.upgrade().is_some());
    assert!(wtest.test_mut(43) == Some(()));
    assert!(wtest.test().unwrap() == 43);
    pool.spawner()
        .spawn({
            let mut test = test.clone();
            let mut stream = test.create_event_stream::<usize>();
            async move {
                let v = stream.next().await;
                assert!(*v.unwrap().as_ref() == 44);
                test.test_mut(45);
            }
        })
        .unwrap();
    test.send_event(44usize);
    pool.run_until_stalled();
    assert!(test.test() == 45);
}
