#[test]
fn succeeds() {}

#[test]
fn fails() {
    panic!("fixture failure");
}

#[test]
fn timeout() {
    std::thread::park();
}
