//! Pure and owned receipt controls.
use super::records::{fields, object};
use super::*;
// Existing initializer tests moved only to keep recipe.rs below600 lines.
use super::super::receipt_output;
fn warm(
    cell: &std::sync::OnceLock<super::super::Receipt>,
    attempt: impl FnOnce() -> std::io::Result<std::process::Output>,
) -> &super::super::Receipt {
    super::super::initialize_warm(cell, || (attempt(), None))
}
use std::sync::OnceLock;
#[test]
fn receipt_failed_initializers_leave_the_cell_empty_for_a_later_caller() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    for result in [
        Err(std::io::Error::other("SECRET")),
        Ok(receipt_output(1, b"failure\n", b"SECRET")),
    ] {
        let cell = OnceLock::new();
        let failed = catch_unwind(AssertUnwindSafe(|| warm(&cell, || result)));
        assert!(failed.is_err(), "failed warm must panic");
        assert!(cell.get().is_none(), "failed result must not be published");
        let receipt = warm(&cell, || Ok(receipt_output(0, b"malformed\n", b"")));
        assert!(receipt.status.as_ref().unwrap().success(), "{receipt:?}");
        assert!(receipt.selection.is_err(), "{receipt:?}");
        assert!(receipt.file.is_none(), "{receipt:?}");
        assert!(std::ptr::eq(
            receipt,
            warm(&cell, || panic!("initialized cell ran again"))
        ));
    }
}

#[test]
fn receipt_successful_initializer_publishes_only_safe_observed_metadata() {
    let root = crate::scratch("receipt-success");
    let bin = root.join("sf-cargo/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let path = bin.join("tasks-0123456789abcdef");
    // Owned unit-test data only; never an executable, launcher input or operational cache.
    std::fs::write(&path, b"data").unwrap();
    let cell = OnceLock::new();
    let receipt = warm(&cell, || {
        Ok(receipt_output(
            0,
            format!("{}\n", path.display()).as_bytes(),
            b"SECRET",
        ))
    });
    assert_eq!(receipt.selection, Ok((path, "0123456789abcdef".into())));
    assert!(receipt.file.as_ref().unwrap().regular, "{receipt:?}");
    assert!(!format!("{receipt:?}").contains("SECRET"), "{receipt:?}");
    assert!(std::ptr::eq(receipt, cell.get().unwrap()));
}

#[test]
fn input_frames_are_exact_and_do_not_accept_partial_or_duplicate_records() {
    let raw = b"one\0two\0";
    assert_eq!(
        fields::<2>(raw).unwrap(),
        [b"one".as_slice(), b"two".as_slice()]
    );
    for bad in [
        b"one\0two".as_slice(),
        b"one\0two\0extra\0",
        b"one\0two\0one\0two\0",
    ] {
        assert!(fields::<2>(bad).is_err(), "{bad:?}");
    }
    assert!(!object(b"0123456789abcdef"));
    assert!(object(b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
}

fn fixture_hash(op: &str, phase: usize, id: &str) -> Vec<u8> {
    // Constructed protocol data only. No actual file/object hash is executed or asserted.
    format!(
        "IR1\0H\0{op}\0{phase}\0{id}\0/project/\0/project/a.rs\0{}\0{}\0END\0",
        "a".repeat(40),
        "a".repeat(40)
    )
    .into_bytes()
}

fn fixture_final(op: &str, id: &str) -> Vec<u8> {
    format!(
        "IR1\0F\0{op}\0{id}\0aaaaaaaaaaaaaaaa\0/public/sf-cargo/bin/tasks-aaaaaaaaaaaaaaaa\0END\0"
    )
    .into_bytes()
}

#[test]
fn input_hash_and_final_fields_bind_the_operation_phase_child_and_public_key() {
    let hash = fixture_hash("build", 0, "123-0");
    assert_eq!(
        hash_record(&hash, "build", 0, "123-0").unwrap(),
        "aaaaaaaaaaaaaaaa"
    );
    for (op, phase, id) in [
        ("where", 0, "123-0"),
        ("build", 1, "123-0"),
        ("build", 0, "124-0"),
    ] {
        assert!(hash_record(&hash, op, phase, id).is_err());
    }
    let final_row = fixture_final("build", "123-0");
    assert_eq!(
        final_record(&final_row, "build", "123-0").unwrap().0,
        "aaaaaaaaaaaaaaaa"
    );
    let wrong = String::from_utf8(final_row)
        .unwrap()
        .replace("tasks-aaaaaaaaaaaaaaaa", "tasks-bbbbbbbbbbbbbbbb");
    assert!(final_record(wrong.as_bytes(), "build", "123-0").is_err());
}

pub(super) fn private_fixture(label: &str, caller: Caller) -> Capture {
    let artifacts = crate::scratch(label).join("artifacts");
    fs::create_dir(&artifacts).unwrap();
    let dir = artifacts.join("recipe-inputs-123-0");
    DirBuilder::new().mode(0o700).create(&dir).unwrap();
    let m = fs::symlink_metadata(&dir).unwrap();
    let capture = Capture {
        dir,
        id: "123-0".into(),
        caller,
        uid: m.uid(),
        device: m.dev(),
        inode: m.ino(),
    };
    private_write(
        &capture.dir.join("owner"),
        format!("123-0 {caller:?}\n").as_bytes(),
    );
    capture
}

pub(super) fn private_write(path: &Path, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
}

#[test]
fn input_owned_files_have_exact_byte_limits_and_reject_links_or_nonregular_data() {
    let c = private_fixture("input-byte-limit", Caller::Warm);
    let record = c.dir.join("build-0");
    private_write(&record, &vec![b'x'; BYTES]);
    let mut bytes = Vec::with_capacity(BYTES + 1);
    assert!(c.file("build-0", &mut bytes).unwrap());
    assert_eq!(bytes.len(), BYTES);
    let mut append = OpenOptions::new().append(true).open(&record).unwrap();
    append.write_all(b"x").unwrap();
    assert!(c.file("build-0", &mut bytes).is_err());
    assert!(!c.file("missing", &mut bytes).unwrap());
    std::os::unix::fs::symlink(&record, c.dir.join("build-final")).unwrap();
    assert!(c.file("build-final", &mut bytes).is_err());
    DirBuilder::new()
        .mode(0o700)
        .create(c.dir.join("where-0"))
        .unwrap();
    assert!(c.file("where-0", &mut bytes).is_err());
}

#[test]
fn input_complete_records_remain_observations_and_missing_or_conflicting_evidence_is_unknown() {
    let c = private_fixture("input-phase-binding", Caller::Warm);
    private_write(&c.dir.join("build.claim"), b"");
    private_write(&c.dir.join("build-0"), &fixture_hash("build", 0, &c.id));
    private_write(&c.dir.join("build-final"), &fixture_final("build", &c.id));
    assert!(
        c.validate(Some((
            Path::new("/public/sf-cargo/bin/tasks-aaaaaaaaaaaaaaaa"),
            "aaaaaaaaaaaaaaaa"
        )))
        .is_ok()
    );
    assert!(
        c.validate(Some((
            Path::new("/public/sf-cargo/bin/tasks-aaaaaaaaaaaaaaaa"),
            "bbbbbbbbbbbbbbbb"
        )))
        .is_err()
    );
    assert!(c.validate(None).is_err());
    private_write(&c.dir.join("build-2"), &fixture_hash("build", 2, &c.id));
    assert!(
        c.validate(Some((
            Path::new("/public/sf-cargo/bin/tasks-aaaaaaaaaaaaaaaa"),
            "aaaaaaaaaaaaaaaa"
        )))
        .is_err(),
        "phase gap"
    );
    private_write(&c.dir.join("invalid"), b"");
    assert!(
        c.validate(Some((
            Path::new("/public/sf-cargo/bin/tasks-aaaaaaaaaaaaaaaa"),
            "aaaaaaaaaaaaaaaa"
        )))
        .is_err(),
        "explicit failure"
    );
}

pub(super) struct FailingWriter {
    pub(super) attempts: usize,
}
impl Write for FailingWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        self.attempts += 1;
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "private writer refused",
        ))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn actual_reporting_paths_keep_one_subject_result_and_mark_writer_failure_unknown() {
    use std::cell::Cell;
    let calls = Cell::new(0);
    let mut before = FailingWriter { attempts: 0 };
    let c = private_fixture("input-report-before", Caller::Warm);
    let (output, capture) = launch(Some(c), &mut before, |capture| {
        assert!(
            capture.is_none(),
            "failed Pending report must not activate capture"
        );
        calls.set(calls.get() + 1);
        super::super::receipt_output(7, b"owned stdout", b"owned stderr")
    });
    assert_eq!(calls.get(), 1);
    assert_eq!(before.attempts, 1);
    assert!(capture.is_none());
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(output.stdout, b"owned stdout");
    assert_eq!(output.stderr, b"owned stderr");
    assert_eq!(finish(capture, None).state, "unknown");

    let c = private_fixture("input-report-after", Caller::Warm);
    private_write(&c.dir.join("build.claim"), b"");
    private_write(&c.dir.join("build-0"), &fixture_hash("build", 0, &c.id));
    private_write(&c.dir.join("build-final"), &fixture_final("build", &c.id));
    let expected = Some((
        Path::new("/public/sf-cargo/bin/tasks-aaaaaaaaaaaaaaaa"),
        "aaaaaaaaaaaaaaaa",
    ));
    assert_eq!(
        c.validate(expected).unwrap().state,
        "captured",
        "arm the downgrade"
    );
    let mut after = FailingWriter { attempts: 0 };
    let (output, evidence) = report_result(output, Some(c), expected, &mut after);
    assert_eq!(after.attempts, 1);
    assert_eq!(evidence.state, "unknown");
    assert_eq!(evidence.reason, Some("report unavailable"));
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(output.stdout, b"owned stdout");
    assert_eq!(output.stderr, b"owned stderr");
}
