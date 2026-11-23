//! A folder of its own per test, for a board and its server, removed when the test passes and kept
//! when it fails; the folders kept by runs since ended pruned to the newest; and a stand-in program
//! a case writes. A repository of its own, its board in its git dir, is [`crate::own_board`]'s.

use crate::children::Child as _;
pub use crate::own_board::signal;
use std::path::{Path, PathBuf};
use symphony_board::server::Home;
use symphony_board::sys::Spawning as _;

pub struct Scratch {
    pub dir: PathBuf,
}

/// How many folders of tests that failed in runs since ended are kept, the newest.
const KEPT: usize = 5;

/// The folders [`Scratch`] kept under `root` for tests that failed (`server-PID-NAME`) in runs that
/// have since ended, all but the newest `keep` removed. A run still going keeps every one of its
/// own; nothing else under `root` is touched.
pub fn prune_kept(root: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let ended = |name: &str| {
        name.strip_prefix("server-")
            .and_then(|rest| rest.split_once('-'))
            .and_then(|(pid, _)| pid.parse::<i64>().ok())
            .is_some_and(|pid| !symphony_board::sys::pid_alive(pid))
    };
    let mut kept: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|entry| entry.file_name().to_str().is_some_and(ended))
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            meta.is_dir()
                .then_some((meta.modified().ok()?, entry.path()))
        })
        .collect();
    kept.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    for (_, path) in kept.into_iter().skip(keep) {
        let _ = std::fs::remove_dir_all(path);
    }
}

impl Scratch {
    /// A folder of its own in the test target's folder, which is no scratch place
    /// ([`symphony_board::server::scratch`]): a board there outlives its starter unless the case
    /// says it is supervised.
    pub fn new(name: &str) -> Scratch {
        static PRUNED: std::sync::Once = std::sync::Once::new();
        PRUNED.call_once(|| prune_kept(Path::new(env!("CARGO_TARGET_TMPDIR")), KEPT));
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("server-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch { dir }
    }

    pub fn home(&self) -> Home {
        Home::new(&self.dir)
    }
}

impl Drop for Scratch {
    /// A test that failed keeps its folder, its server's log in it.
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("{} is kept", self.dir.display());
        } else {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// Installs `body` behind the prepared shell image and checks that it exits with no arguments,
/// before any startup patience begins. The argument guard keeps that check from starting a server
/// or counting a start. The script's path.
pub fn script(path: &Path, body: &str) -> PathBuf {
    crate::fixture_script::write(
        path,
        &format!("#!/bin/sh\n[ $# -gt 0 ] || exit 0\n{body}\n"),
        false,
    );
    let status = std::process::Command::of(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status_locked()
        .unwrap();
    assert!(
        status.success(),
        "{} with no arguments: {status}",
        path.display()
    );
    path.to_path_buf()
}

/// Only the folders of runs that have ended are pruned, the newest kept; a running process's, and
/// what is not a kept folder, stay.
#[test]
fn kept_folders_of_ended_runs_are_pruned_to_the_newest() {
    use std::time::Duration;
    use symphony_board::sys::Command;
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("prune-kept-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let ended = || {
        let mut child = Command::of("/usr/bin/true").spawn().unwrap();
        child.wait().unwrap();
        child.id()
    };
    let ours = std::process::id();
    let names = [
        format!("server-{}-oldest", ended()),
        format!("server-{}-older", ended()),
        format!("server-{ours}-running"),
        format!("server-{}-newest", ended()),
        "server-runs".to_string(),
    ];
    // Each folder a second newer than the one before, set rather than waited for.
    let made = std::time::SystemTime::now() - Duration::from_secs(60);
    for (at, name) in names.iter().enumerate() {
        std::fs::create_dir_all(root.join(name)).unwrap();
        let seconds = Duration::from_secs(u64::try_from(at).unwrap());
        std::fs::File::open(root.join(name))
            .unwrap()
            .set_modified(made + seconds)
            .unwrap();
    }
    let file = format!("server-{}-file", ended());
    std::fs::write(root.join(&file), "").unwrap();
    prune_kept(&root, 1);
    let mut left: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    left.sort();
    let mut expected = vec![names[2].clone(), names[3].clone(), names[4].clone(), file];
    expected.sort();
    assert_eq!(left, expected);
    let _ = std::fs::remove_dir_all(&root);
}
