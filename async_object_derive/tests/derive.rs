use async_object_derive::async_object;

#[async_object(Test)]
struct TestImpl {
    foo: usize,
}
#[async_object(pub Test2)]
struct Test2Impl {
    foo: usize,
}

#[test]
fn derive_test() {
    let _ = TestImpl { foo: 0 };
    let _ = Test;
    let _ = Test2Impl { foo: 0 };
    let _ = Test2;
}
