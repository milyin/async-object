use async_object_derive::{async_object_decl, async_object_impl};

#[async_object_decl(Test, WTest)]
struct TestImpl {}
#[async_object_decl(pub Test2, pub WTest2)]
struct Test2Impl {}

#[async_object_impl(Test, WTest)]
impl TestImpl {
    fn test(&self, foo: usize) -> bool {
        true
    }
    fn test_mut(&mut self, foo: usize) -> bool {
        true
    }
}

#[test]
fn derive_test() {
    let test = TestImpl {};
    let mut test = Test::create(test);
    let test2 = Test2Impl {};
    let test2 = Test2::create(test2);
    test.test(42);
    test.test_mut(42);
}
