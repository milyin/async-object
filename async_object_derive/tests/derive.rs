use async_object_derive::async_object;

#[async_object(Test)]
struct TestImpl {}
#[async_object(pub Test2)]
struct Test2Impl {}

#[test]
fn derive_test() {
    let test = TestImpl {};
    let _ = Test::create(test);
    let test2 = Test2Impl {};
    let _ = Test2::create(test2);
}
