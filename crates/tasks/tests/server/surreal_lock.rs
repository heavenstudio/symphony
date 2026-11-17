//! `tools/surreal.sh`'s lock, which a fetch of the pinned SurrealDB holds: of callers at once one
//! fetches and the others wait and find its binary, and a fetch killed part way frees the lock for
//! the next once nothing of it runs, which removes the scratch it left and fetches again. Each case
//! sources the script on a folder of its own under the test's temporary one, never the binary's
//! real place, at the boundary its header names: a stand-in `fetch` notes each call, waits for a
//! line the test writes on a FIFO, then copies a tarball made here, and `pinned` is that tarball's
//! SHA-256.

use crate::children::Child as _;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Write as _;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use symphony_board::server::SURREAL_VERSION;
use symphony_board::sys;

/// The build the cases ask for, which no real tarball is named for.
const BUILD: &str = "test-build";

/// The stand-in fetch and pin, defined over the script's own once it is sourced; then `ensure`, as
/// the script's `main` runs it, on the folder `$2`.
const CALLER: &str = r#"source "$1" || exit 3
fetch() {
    printf '%s\n' "$1" >> "$FETCH_CALLS"
    read -r _ < "$FETCH_GATE"
    cp "$FETCH_TARBALL" "$2"
}
pinned() { printf '%s\n' "$FETCH_PIN"; }
ensure "$2" "$3"
"#;

/// The binary the made tarball holds.
const SURREAL: &str = "#!/bin/sh\necho surreal\n";

/// A folder of its own for one case, the script's folder in it, and the gate its fetches wait on.
struct Fetching {
    dir: PathBuf,
    folder: PathBuf,
    tarball: PathBuf,
    pin: String,
    calls: PathBuf,
    gate_path: PathBuf,
    /// The gate held open both ways for the case's whole life, so a fetch's open of it never
    /// waits and a line written before its read is kept for it; its close, as the case ends,
    /// lets any fetch still waiting go.
    gate: File,
    /// Each caller's group, ended whole when a case fails part way.
    groups: Vec<u32>,
}

/// A caller of the script's `ensure`, started in a process group of its own.
struct Caller {
    child: Child,
    out: PathBuf,
    err: PathBuf,
}

impl Caller {
    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn err(&self) -> String {
        fs::read_to_string(&self.err).unwrap()
    }

    /// Its end, within the backstop of a wait: its status, stdout and stderr.
    fn ended(mut self) -> (ExitStatus, String, String) {
        let status = test_time::until("the caller ends", test_time::FLOOR, || {
            self.child.try_wait().unwrap()
        })
        .unwrap_or_else(|stall| panic!("{stall}: {}", self.err()));
        (status, fs::read_to_string(&self.out).unwrap(), self.err())
    }
}

impl Fetching {
    fn new(name: &str) -> Fetching {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("server-{}-surreal-lock-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("made")).unwrap();
        fs::write(dir.join("made/surreal"), SURREAL).unwrap();
        let tarball = dir.join("made.tgz");
        let made = Command::of("tar")
            .arg("-czf")
            .arg(&tarball)
            .arg("-C")
            .arg(dir.join("made"))
            .arg("surreal")
            .status()
            .unwrap();
        assert!(made.success());
        let pin = Sha256::digest(fs::read(&tarball).unwrap())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let gate_path = dir.join("gate");
        let fifo = Command::of("mkfifo").arg(&gate_path).status().unwrap();
        assert!(fifo.success());
        let gate = File::options()
            .read(true)
            .write(true)
            .open(&gate_path)
            .unwrap();
        Fetching {
            folder: dir.join("common/symphony/surreal"),
            calls: dir.join("calls"),
            dir,
            tarball,
            pin,
            gate_path,
            gate,
            groups: Vec::new(),
        }
    }

    /// The script's `ensure` started for the folder, as `name`, its output in files of that name.
    fn caller(&mut self, name: &str) -> Caller {
        let (out, err) = (
            self.dir.join(format!("{name}.out")),
            self.dir.join(format!("{name}.err")),
        );
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/surreal.sh");
        let child = Command::of("bash")
            .args(["-c", CALLER, name])
            .arg(script)
            .arg(&self.folder)
            .arg(BUILD)
            .env("FETCH_CALLS", &self.calls)
            .env("FETCH_GATE", &self.gate_path)
            .env("FETCH_TARBALL", &self.tarball)
            .env("FETCH_PIN", &self.pin)
            .stdin(Stdio::null())
            .stdout(File::create(&out).unwrap())
            .stderr(File::create(&err).unwrap())
            .process_group(0)
            .spawn()
            .unwrap();
        self.groups.push(child.id());
        Caller { child, out, err }
    }

    /// How many fetches have begun.
    fn calls(&self) -> usize {
        fs::read_to_string(&self.calls).map_or(0, |text| text.lines().count())
    }

    /// Until `count` fetches have begun.
    fn until_calls(&self, count: usize) {
        test_time::until_true(format!("{count} fetches begun"), test_time::FLOOR, || {
            self.calls() >= count
        })
        .unwrap_or_else(|stall| panic!("{stall}"));
        assert_eq!(self.calls(), count, "{}", self.folder.display());
    }

    /// One fetch waiting at the gate goes on.
    fn release(&mut self) {
        self.gate.write_all(b"go\n").unwrap();
    }

    /// The folder's entries by name, each scratch folder a fetch made among them.
    fn entries(&self) -> BTreeSet<String> {
        fs::read_dir(&self.folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect()
    }

    /// The one scratch folder a fetch made.
    fn scratch(&self) -> String {
        let scratch: Vec<String> = self
            .entries()
            .into_iter()
            .filter(|name| name.starts_with("fetch."))
            .collect();
        assert_eq!(scratch.len(), 1, "{scratch:?}");
        scratch[0].clone()
    }

    fn binary(&self) -> PathBuf {
        self.folder.join(SURREAL_VERSION).join("surreal")
    }

    /// That `caller` found or installed the binary, and that the folder keeps only its lock and
    /// the checked binary.
    fn installed(&self, caller: Caller, name: &str) -> String {
        let (status, out, err) = caller.ended();
        assert!(status.success(), "{name}: {status}: {err}");
        assert_eq!(
            out,
            format!("{}\n", self.binary().display()),
            "{name}: {err}"
        );
        assert_eq!(fs::read_to_string(self.binary()).unwrap(), SURREAL);
        assert_eq!(
            self.entries(),
            BTreeSet::from(["lock".to_string(), SURREAL_VERSION.to_string()]),
            "{name}"
        );
        assert_eq!(
            fs::read_dir(self.binary().parent().unwrap())
                .unwrap()
                .count(),
            1
        );
        err
    }

    /// Until nothing of `group` runs.
    fn until_gone(group: u32) {
        test_time::until_true("the killed caller's group ends", test_time::FLOOR, || {
            !sys::group_alive(group)
        })
        .unwrap_or_else(|stall| panic!("{stall}"));
    }
}

impl Drop for Fetching {
    /// A case that failed ends each caller's group and keeps its folder; one that passed left
    /// nothing running, and its folder goes.
    fn drop(&mut self) {
        if std::thread::panicking() {
            for group in &self.groups {
                if sys::group_alive(*group) {
                    sys::end_group(*group, true);
                }
            }
            eprintln!("{} is kept", self.dir.display());
        } else {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
}

#[test]
fn of_two_callers_at_once_one_fetches_and_the_other_waits_for_its_binary() {
    let mut f = Fetching::new("two");
    let first = f.caller("first");
    f.until_calls(1);
    let second = f.caller("second");
    let pid = first.pid();
    test_time::until_true("the second caller waits", test_time::FLOOR, || {
        second.err().contains("waiting for the SurrealDB folder")
    })
    .unwrap_or_else(|stall| panic!("{stall}: {}", second.err()));
    assert!(
        second.err().contains(&format!(
            "surreal: waiting for the SurrealDB folder {}, held by {pid}\n",
            f.folder.display()
        )),
        "{}",
        second.err()
    );
    assert_eq!(f.calls(), 1, "the second waits rather than fetching");
    f.release();
    let first = f.installed(first, "first");
    assert!(first.contains("surreal: fetching SurrealDB"), "{first}");
    let second = f.installed(second, "second");
    assert!(!second.contains("fetching"), "{second}");
    assert_eq!(f.calls(), 1, "one download for the two");
}

#[test]
fn a_caller_killed_with_its_fetch_frees_the_lock_and_the_next_removes_what_it_left() {
    let mut f = Fetching::new("killed-whole");
    let mut first = f.caller("first");
    f.until_calls(1);
    let left = f.scratch();
    assert!(sys::end_group(first.pid(), true));
    first.child.wait().unwrap();
    Fetching::until_gone(first.pid());
    let next = f.caller("next");
    f.until_calls(2);
    assert!(
        !next.err().contains("waiting for"),
        "a killed caller left no lock: {}",
        next.err()
    );
    assert_ne!(f.scratch(), left, "what the killed fetch left is gone");
    f.release();
    let next = f.installed(next, "next");
    assert!(next.contains("surreal: fetching SurrealDB"), "{next}");
}

/// A caller killed alone leaves its fetch running, which holds the lock until it ends: the next
/// waits for it, then removes what it wrote and fetches again.
#[test]
fn a_caller_killed_alone_holds_the_lock_until_its_fetch_ends() {
    let mut f = Fetching::new("killed-alone");
    let mut first = f.caller("first");
    f.until_calls(1);
    let left = f.scratch();
    assert!(sys::terminate(i64::from(first.pid()), true));
    first.child.wait().unwrap();
    assert!(sys::group_alive(first.pid()), "its fetch runs on");
    let next = f.caller("next");
    test_time::until_true("the next caller waits", test_time::FLOOR, || {
        next.err().contains("waiting for the SurrealDB folder")
    })
    .unwrap_or_else(|stall| panic!("{stall}: {}", next.err()));
    assert_eq!(f.calls(), 1);
    // The killed caller's fetch goes on and ends, writing its tarball; then the next fetches.
    f.release();
    f.until_calls(2);
    Fetching::until_gone(first.pid());
    assert!(
        fs::read_dir(&f.folder).unwrap().all(|entry| entry
            .unwrap()
            .file_name()
            .into_string()
            .unwrap()
            != left),
        "what the killed fetch wrote is gone"
    );
    f.release();
    f.installed(next, "next");
}
