//! A real subject panic must retain its entry count and original payload.
use super::{Caller, FailingWriter, launch, private_fixture};
use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind, panic_any};

#[test]
fn a_failed_diagnostic_writer_does_not_catch_a_real_subject_panic() {
    struct SubjectPanic;

    let entered = Cell::new(0);
    let capture = private_fixture("input-report-real-panic", Caller::Warm);
    let mut writer = FailingWriter { attempts: 0 };
    let result = catch_unwind(AssertUnwindSafe(|| {
        launch(Some(capture), &mut writer, |capture| -> () {
            // This also arms the failed Pending write before subject entry.
            assert!(capture.is_none());
            entered.set(entered.get() + 1);
            panic_any(SubjectPanic);
        })
    }));
    assert_eq!(writer.attempts, 1);
    assert_eq!(entered.get(), 1);
    let Err(payload) = result else {
        panic!("production launch returned after a real subject panic");
    };
    assert!(
        payload.downcast_ref::<SubjectPanic>().is_some(),
        "an unrelated diagnostic panic cannot satisfy this control"
    );
}
