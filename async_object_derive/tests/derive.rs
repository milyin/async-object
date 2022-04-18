use async_object_derive::{async_object_decl, async_object_impl, async_object_with_events_decl};
use futures::StreamExt;
use futures::{executor::LocalPool, task::SpawnExt};

#[async_object_decl(Test, WTest)]
struct TestImpl {
    foo: usize,
    this: Option<WTest>,
}

impl TestImpl {
    pub fn new() -> Self {
        Self { foo: 0, this: None }
    }
    pub fn new_cyclic(this: WTest) -> Self {
        Self {
            foo: 0,
            this: Some(this),
        }
    }
}

#[async_object_impl(Test, WTest)]
impl TestImpl {
    pub fn get(&self) -> Result<usize, ()> {
        Ok(self.foo)
    }
    fn put(&mut self, foo: usize) {
        self.foo = foo;
    }
    fn is_cyclic(&self) -> bool {
        self.this.is_some()
    }
}

impl Test {
    pub fn new() -> Self {
        Test::create(TestImpl::new())
    }
    pub fn new_cyclic() -> Self {
        Test::create_cyclic(|v| TestImpl::new_cyclic(v.clone()))
    }
}

#[test]
fn async_object_decl_test() {
    let mut test = Test::new();
    let test_cyclic = Test::new_cyclic();
    let mut wtest = test.downgrade();
    test.put(42);
    assert!(test.get() == Ok(42));
    assert!(wtest.upgrade().is_some());
    assert!(wtest.put(43) == Some(()));
    assert!(wtest.get() == Ok(Some(43)));
    assert!(test_cyclic.is_cyclic());
    assert!(!test.is_cyclic());
}

#[async_object_with_events_decl(pub Test2, pub WTest2)]
struct Test2Impl {
    foo: usize,
    this: Option<WTest2>,
}

impl Test2Impl {
    pub fn new() -> Self {
        Self { foo: 0, this: None }
    }
    pub fn new_cyclic(this: WTest2) -> Self {
        Self {
            foo: 0,
            this: Some(this),
        }
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
    fn is_cyclic(&self) -> bool {
        self.this.is_some()
    }
}

impl Test2 {
    pub fn new() -> Self {
        Test2::create(Test2Impl::new())
    }
    pub fn new_cyclic() -> Self {
        Test2::create_cyclic(|v| Test2Impl::new_cyclic(v.clone()))
    }
}

#[test]
fn async_object_with_events_decl_test() {
    let mut test = Test2::new();
    let test_cyclic = Test::new_cyclic();
    let mut wtest = test.downgrade();
    test.test_mut(42);
    assert!(test.test() == 42);
    assert!(wtest.upgrade().is_some());
    assert!(wtest.test_mut(43) == Some(()));
    assert!(wtest.test() == Some(43));
    assert!(test_cyclic.is_cyclic());
    assert!(!test.is_cyclic());

    let mut pool = LocalPool::new();
    pool.spawner()
        .spawn({
            let test = test.clone();
            async move {
                test.send_event(44usize).await;
            }
        })
        .unwrap();

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
    pool.run_until_stalled();
    dbg!(test.test());
    assert!(test.test() == 45);
}
