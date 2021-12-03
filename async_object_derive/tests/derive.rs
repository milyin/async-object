use async_object_derive::async_object;

#[async_object(Test)]
struct TestImpl {
    foo: usize,
}

#[test]
fn derive_test() {
    let _ = TestImpl { foo: 0 };
    let _ = Test;
}
