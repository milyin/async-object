use async_object;
use async_object_derive::AsyncObject;

#[derive(AsyncObject)]
struct TestImpl {
    foo: usize,
}

#[test]
fn derive_test() {}
