//! Real filesystem boundaries of the private wire fixture reader. All data and reports are owned
//! scratch outputs; process audit context is supplied only to an isolated copy of this harness.

use super::fixture;
use repository_read::{HEADER, REPORT_ENV, ROOT_ENV, Reader};
use std::fmt::Write as _;
use std::fs;
use std::io::ErrorKind;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "symphony-wire-reader-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => {
                    return Self(
                        path.canonicalize()
                            .expect("the owned scratch directory exists"),
                    );
                }
                Err(error) => assert_eq!(
                    error.kind(),
                    ErrorKind::AlreadyExists,
                    "{}: {error}",
                    path.display()
                ),
            }
        }
    }

    fn join(&self, path: &str) -> PathBuf {
        self.0.join(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn payload(folder: &Path, text: &str) {
    fs::create_dir_all(folder).expect("the owned fixture folder is created");
    fs::write(folder.join("payload.json"), text).expect("the fixture payload is written");
}

fn report_line(operation: &str, path: &Path) -> String {
    let mut encoded = String::new();
    for byte in path.as_os_str().as_bytes() {
        write!(encoded, "{byte:02x}").expect("formatting into a String succeeds");
    }
    format!("{operation}\t{encoded}\n")
}

#[test]
fn the_explicit_fixture_folder_wins_over_same_named_files_and_unrelated_ancestors() {
    let tmp = Scratch::new();
    let hostile = tmp.join("ancestor");
    payload(&hostile.join("fixtures"), "ancestor");
    payload(
        &hostile.join("symphony/crates/symphony-api/fixtures"),
        "containing project",
    );
    let first = hostile.join("one/fixtures");
    let second = hostile.join("two/fixtures");
    payload(&first, "first");
    payload(&second, "second");
    let reader = Reader::new(hostile, None).unwrap();
    assert_eq!(
        fixture::read(&reader, &first, "payload.json").unwrap(),
        "first"
    );
    assert_eq!(
        fixture::read(&reader, &second, "payload.json").unwrap(),
        "second"
    );
}

#[test]
fn failed_reads_are_reported_but_a_missing_report_is_never_created() {
    let tmp = Scratch::new();
    let fixtures = tmp.join("fixtures");
    payload(&fixtures, "readable");
    let report = tmp.join("reads");
    fs::write(&report, HEADER).unwrap();
    let reader = Reader::new(tmp.0.clone(), Some(report.clone())).unwrap();
    let missing = fixtures.join("missing.json");
    assert_eq!(
        fixture::read(&reader, &fixtures, "missing.json")
            .unwrap_err()
            .kind(),
        ErrorKind::NotFound
    );
    assert!(
        fs::read_to_string(&report)
            .unwrap()
            .starts_with(&format!("{HEADER}{}", report_line("file", &missing)))
    );

    let absent = tmp.join("absent-report");
    let reader = Reader::new(tmp.0.clone(), Some(absent.clone())).unwrap();
    assert_eq!(
        fixture::read(&reader, &fixtures, "payload.json")
            .unwrap_err()
            .kind(),
        ErrorKind::NotFound
    );
    assert!(
        !absent.exists(),
        "readers may append only to an existing report"
    );
}

#[test]
fn link_then_parent_is_reported_in_physical_order_without_normalizing_the_subject() {
    let tmp = Scratch::new();
    let fixtures = tmp.join("fixtures");
    let elsewhere = tmp.join("elsewhere");
    payload(&fixtures, "lexical parent");
    payload(&elsewhere, "physical parent");
    fs::create_dir(elsewhere.join("sub")).unwrap();
    symlink("../elsewhere/sub", fixtures.join("link")).unwrap();
    let report = tmp.join("reads");
    fs::write(&report, HEADER).unwrap();
    let reader = Reader::new(tmp.0.clone(), Some(report.clone())).unwrap();
    assert_eq!(
        fixture::read(&reader, &fixtures.join("link/.."), "payload.json").unwrap(),
        "physical parent"
    );
    let recorded = fs::read_to_string(report).unwrap();
    for (operation, path) in [
        ("file", fixtures.join("link/../payload.json")),
        ("link", fixtures.join("link")),
        ("target", fixtures.join("../elsewhere/sub")),
        ("resolved", elsewhere.join("payload.json")),
    ] {
        assert!(
            recorded.contains(&report_line(operation, &path)),
            "{operation}: {recorded}"
        );
    }
}

#[test]
fn process_audit_context_is_authoritative_only_with_an_active_report() {
    const MODE: &str = "SYMPHONY_WIRE_READER_CHILD";
    const DEFAULT: &str = "SYMPHONY_WIRE_READER_DEFAULT";
    if let Some(mode) = std::env::var_os(MODE) {
        let root = PathBuf::from(std::env::var_os(DEFAULT).unwrap());
        let reader = Reader::process(root);
        match mode.to_str().unwrap() {
            "no-root" => assert_eq!(
                reader
                    .err()
                    .expect("an active report requires its root")
                    .to_string(),
                "read audit report has no source root"
            ),
            "relative-root" => assert_eq!(
                reader
                    .err()
                    .expect("an audit root must be absolute")
                    .to_string(),
                "repository reader needs an absolute source root"
            ),
            mode => {
                let expected = match mode {
                    "active" => "audit root",
                    "inactive" => "default root",
                    other => panic!("unknown isolated case: {other}"),
                };
                assert_eq!(
                    fixture::read(&reader.unwrap(), Path::new("fixtures"), "payload.json").unwrap(),
                    expected
                );
            }
        }
        return;
    }

    let tmp = Scratch::new();
    let default = tmp.join("default");
    let audit = tmp.join("audit");
    let cwd = tmp.join("unrelated-cwd");
    payload(&default.join("fixtures"), "default root");
    payload(&audit.join("fixtures"), "audit root");
    payload(&cwd.join("fixtures"), "cwd decoy");
    let report = tmp.join("reads");
    // Each case: the variable naming the report, and the one naming the root with its value.
    let audited = audit.as_path();
    let cases = [
        ("active", Some(REPORT_ENV), Some((ROOT_ENV, audited))),
        ("no-root", Some(REPORT_ENV), None),
        (
            "relative-root",
            Some(REPORT_ENV),
            Some((ROOT_ENV, Path::new("relative"))),
        ),
        ("inactive", None, Some((ROOT_ENV, audited))),
    ];
    for (mode, report_named, root_named) in cases {
        fs::write(&report, HEADER).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .args([
                "--exact",
                "reader_tests::process_audit_context_is_authoritative_only_with_an_active_report",
                "--nocapture",
            ])
            .current_dir(&cwd)
            .env(MODE, mode)
            .env(DEFAULT, &default)
            .env_remove(ROOT_ENV)
            .env_remove(REPORT_ENV);
        if let Some(name) = report_named {
            child.env(name, &report);
        }
        if let Some((name, root)) = root_named {
            child.env(name, root);
        }
        let result = child.output().unwrap();
        assert!(
            result.status.success(),
            "{mode}: {}\n{}\n{}",
            result.status,
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        let recorded = fs::read_to_string(&report).unwrap();
        if mode == "active" {
            assert!(recorded.contains(&report_line("file", &audit.join("fixtures/payload.json"))));
        } else {
            assert_eq!(recorded, HEADER, "{mode} must not append subject reads");
        }
    }
}
