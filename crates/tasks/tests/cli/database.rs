//! `tasks database`: the board's database said, held stopped with every board command refused
//! naming the hold, and released, on the board of a fixture repository of its own outside this one
//! (no `TASKS_SERVER`), its server started by its first command and stopped when the case ends.

use crate::common::{NOW, surreal};
use crate::{repository, run, scratch};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use symphony_board::cli::Env;
use symphony_board::server::Home;
use symphony_board::sys::pid_alive;

/// A fixture project, `alpha`, its machine folder its own: its board's hold released and its server
/// stopped when the case ends, however it ends.
struct Alpha {
    root: PathBuf,
    machine: PathBuf,
}

impl Alpha {
    fn new(name: &str) -> Alpha {
        let dir = scratch(name);
        let root = repository(&dir.join("alpha"));
        std::fs::write(root.join("symphony.toml"), "[project]\nid = \"alpha\"\n").unwrap();
        let machine = dir.join("machine");
        std::fs::create_dir_all(&machine).unwrap();
        Alpha { root, machine }
    }

    fn env(&self) -> Env {
        let vars: BTreeMap<String, String> = [
            ("SF_SURREAL", surreal().display().to_string()),
            ("SYMPHONY_MACHINE_DIR", self.machine.display().to_string()),
            ("TASKS_NOW", NOW.to_string()),
            ("TASKS_SWEEP", "off".to_string()),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect();
        Env {
            vars,
            cwd: self.root.clone(),
        }
    }

    fn home(&self) -> Home {
        Home::in_common(&self.root.join(".git"), Some(&surreal()))
    }

    /// `tasks ARGS` here: its exit code, stdout and stderr.
    fn run(&self, args: &[&str]) -> (i32, String, String) {
        let ran = run(&self.env(), args, "");
        (ran.code, ran.out, ran.err)
    }
}

impl Drop for Alpha {
    fn drop(&mut self) {
        let home = self.home();
        if let Some(hold) = home.held() {
            let _ = home.release(&hold.by, None);
        }
        let _ = home.stop();
    }
}

fn data(root: &Path) -> String {
    root.join(".git/sf-tasks/data").display().to_string()
}

/// A board whose server is not running is said so, and held and released without one ever
/// starting.
#[test]
fn a_board_with_no_server_is_held_and_released_and_none_starts() {
    let alpha = Alpha::new("database-idle");
    let home = alpha.home();
    let data = data(&alpha.root);
    let (code, out, _) = alpha.run(&["database"]);
    assert_eq!(
        (code, out),
        (
            0,
            format!(
                "the board's database at {data} is not held. No server runs on its data: the \
                 board's next command starts one\n"
            )
        )
    );
    let (code, out, err) = alpha.run(&["database", "hold", "a clone", "--as", "russet-lynx"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.ends_with(&format!(
            ": a clone. No server ran on its data. Nothing serves {data} now, and nothing starts \
             a server on it until `just tasks database release --as russet-lynx`\n"
        )),
        "{out}"
    );
    let (code, _, err) = alpha.run(&["database", "release", "--as", "russet-lynx"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(home.recorded(), None, "nothing started the board");
    assert_eq!(home.serving(), None);
}

#[test]
fn a_held_board_refuses_every_command_naming_the_hold_until_its_release() {
    let alpha = Alpha::new("database-hold");
    let home = alpha.home();
    let (code, handle, err) = alpha.run(&["register", "--kind", "worker"]);
    assert_eq!(code, 0, "{err}");
    let running = home
        .recorded()
        .expect("the first command started the board");
    let data = data(&alpha.root);

    let (code, out, _) = alpha.run(&["database"]);
    assert_eq!(
        (code, out),
        (
            0,
            format!(
                "the board's database at {data} is not held. Its server is pid {} at {}\n",
                running.pid, running.addr
            )
        )
    );

    let (code, _, err) = alpha.run(&["database", "hold", "a snapshot"]);
    assert_eq!(code, 2);
    assert!(err.starts_with("✖ no session: pass --as HANDLE"), "{err}");

    let (code, out, err) = alpha.run(&["database", "hold", "a snapshot", "--as", "russet-lynx"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with("✔ the board's database is held by russet-lynx since "),
        "{out}"
    );
    assert!(
        out.ends_with(&format!(
            ": a snapshot. Stopped pid {} at {} cleanly. Nothing serves {data} now, and nothing \
             starts a server on it until `just tasks database release --as russet-lynx`\n",
            running.pid, running.addr
        )),
        "{out}"
    );
    assert!(!pid_alive(running.pid));
    let hold = home.held().unwrap();

    // Every command is refused, naming the hold, and starts nothing: one that writes, one that
    // reads, and the session's own; `tasks api`, and the copy's dump of the board.
    let held = format!(
        "the board's database is {hold}; nothing starts it until its holder releases it (`just \
         tasks database release`, run by russet-lynx, or by another handle naming it with \
         `--of`): wait for that, or ask russet-lynx"
    );
    let refused = format!("✖ {held}\n");
    for args in [
        &["list"][..],
        &["show", "sim/jump"],
        &["create", "sim/jump", "Jump", "--as", handle.trim()],
    ] {
        let (code, out, err) = alpha.run(args);
        assert_eq!(
            (code, out.as_str(), err.as_str()),
            (2, "", refused.as_str()),
            "{args:?}"
        );
    }
    let api = run(&alpha.env(), &["api"], r#"{"method": "sessions"}"#);
    let answer: serde_json::Value = serde_json::from_str(&api.out).unwrap();
    assert_eq!(answer, serde_json::json!({"error": held}), "{}", api.err);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = symphony_board::copy_main(
        &["--board".to_string(), "--out".to_string()],
        &alpha.env(),
        &mut &b""[..],
        &mut out,
        &mut err,
    );
    assert_eq!((code, String::from_utf8(out).unwrap()), (2, String::new()));
    assert_eq!(
        String::from_utf8(err).unwrap(),
        format!("❌ tasks-copy: {held}\n")
    );
    assert_eq!(home.recorded(), None, "a refused command started the board");
    assert_eq!(home.serving(), None);

    let (code, out, _) = alpha.run(&["database"]);
    assert_eq!(code, 0);
    assert_eq!(
        out,
        format!(
            "the board's database at {data} is {hold}; nothing starts it until its holder \
             releases it. No server runs on its data\n"
        )
    );
    let (code, _, err) = alpha.run(&["database", "hold", "a swap", "--as", "vivid-swift"]);
    assert_eq!((code, err), (1, refused.clone()));
    // Nor does another handle release it, unless it names the holder.
    let (code, out, err) = alpha.run(&["database", "release", "--as", "vivid-swift"]);
    assert_eq!((code, out.as_str()), (1, ""));
    assert_eq!(
        err,
        format!(
            "✖ the board's database is {hold}; only russet-lynx releases it, or another handle \
             naming it (`--of russet-lynx`), so that no hold is released by mistake in the \
             middle of work on the data\n"
        )
    );
    assert_eq!(home.held(), Some(hold.clone()));

    let (code, out, err) = alpha.run(&["database", "release", "--as", "russet-lynx"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with("✔ released the hold russet-lynx set at "),
        "{out}"
    );
    assert!(
        out.ends_with(
            " (a snapshot). Nothing was started: the board's next command starts the database\n"
        ),
        "{out}"
    );
    assert_eq!(home.recorded(), None, "the release started the board");
    let (code, out, _) = alpha.run(&["database", "release", "--as", "russet-lynx"]);
    assert_eq!(
        (code, out.as_str()),
        (
            0,
            "✔ the board's database was not held: nothing to release\n"
        )
    );
    let (code, _, err) = alpha.run(&["list"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        home.recorded().is_some(),
        "the next command started the board"
    );
    let (code, out, _) = alpha.run(&["help", "database"]);
    assert_eq!(code, 0);
    assert!(
        out.starts_with(concat!(
            "usage: just tasks database [-h] [hold WHY | release [--of HOLDER]]\n",
            "                           [--as ACTOR]\n"
        )),
        "{out}"
    );
}

/// The snapshots that hold a log (one taken before a replay among them) are listed with the bytes
/// of their logs, the owner's to remove, the oldest first, and so is a clone cut short; one whose
/// log is empty, pruned by count, is not.
#[test]
fn the_snapshots_that_hold_a_log_are_listed_for_the_owner() {
    let alpha = Alpha::new("database-kept");
    let dir = alpha.root.join(".git/sf-tasks");
    let snapshots = dir.join("snapshots");
    let kept = snapshots.join("2026-11-12T07-45-00Z");
    std::fs::create_dir_all(kept.join("wal")).unwrap();
    std::fs::write(
        kept.join("wal/00000000000000000017.wal"),
        vec![0u8; 3 << 20],
    )
    .unwrap();
    let later = snapshots.join("2026-11-12T08-00-00Z");
    std::fs::create_dir_all(later.join("wal")).unwrap();
    std::fs::write(
        later.join("wal/00000000000000000018.wal"),
        vec![0u8; 1 << 20],
    )
    .unwrap();
    std::fs::create_dir_all(snapshots.join("2026-11-12T09-00-00Z/wal")).unwrap();
    let cut = snapshots.join(".taking");
    std::fs::create_dir_all(&cut).unwrap();
    let (code, out, err) = alpha.run(&["database"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        out,
        format!(
            "the board's database at {} is not held. No server runs on its data: the board's \
             next command starts one. Snapshots that hold a log (taken after an unclean end, \
             before a replay, or of a store that went back), each kept until the owner removes it \
             once the board reads as it should: {} (its log 3.0 MiB), {} (its log 1.0 MiB). Cut \
             short in the middle of its clone or copy, not whole, removed by the next snapshot: \
             {}\n",
            data(&alpha.root),
            kept.display(),
            later.display(),
            cut.display()
        )
    );
    assert_eq!(alpha.home().recorded(), None, "saying it started the board");
}

/// The Symphony's read of a board whose starters' lock another holds (a start, a replay of
/// minutes) is refused once its bound passes, naming the holder, rather than hold the thread that
/// serves its pages; it starts no server. A holder that writes no note is said as one.
#[test]
fn the_symphonys_read_waits_on_anothers_start_at_most_its_bound() {
    let alpha = Alpha::new("database-read-wait");
    let dir = alpha.root.join(".git/sf-tasks");
    std::fs::create_dir_all(&dir).unwrap();
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("lock"))
        .unwrap();
    lock.lock().unwrap();
    let began = std::time::Instant::now();
    let refused = symphony_board::cli::board(&alpha.env())
        .err()
        .expect("the lock is held");
    let waited = began.elapsed();
    assert!(
        waited >= std::time::Duration::from_secs(5) && waited < std::time::Duration::from_secs(30),
        "{waited:?}"
    );
    assert_eq!(
        refused,
        format!(
            "the board's starters' lock ({}) is held by a starter that says nothing of what it \
             does (a build without the lock's note); this read waits at most 5 s for it, and the \
             next one tries again",
            dir.join("lock").display()
        )
    );
    lock.unlock().unwrap();
    assert_eq!(
        alpha.home().recorded(),
        None,
        "the refused read started the board"
    );
}

/// Another handle releases a hold by naming its holder, which it says on stderr and the history
/// records.
#[test]
fn another_handle_releases_a_hold_only_by_naming_its_holder() {
    let alpha = Alpha::new("database-release-of");
    let home = alpha.home();
    let (code, _, err) = alpha.run(&["database", "hold", "a swap", "--as", "dunlin-probe"]);
    assert_eq!(code, 0, "{err}");
    let (code, _, err) = alpha.run(&[
        "database",
        "release",
        "--of",
        "someone",
        "--as",
        "vivid-swift",
    ]);
    assert_eq!(code, 1);
    assert!(
        err.contains("; only dunlin-probe releases it, or another handle naming it"),
        "{err}"
    );
    assert!(home.held().is_some());
    let (code, out, err) = alpha.run(&[
        "database",
        "release",
        "--of",
        "dunlin-probe",
        "--as",
        "vivid-swift",
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with("✔ released the hold dunlin-probe set at "),
        "{out}"
    );
    assert!(
        err.starts_with("· vivid-swift released the hold dunlin-probe set at ")
            && err.ends_with(" (a swap), as sf-tasks/hold.log records\n"),
        "{err}"
    );
    assert_eq!(home.held(), None);
    let history = std::fs::read_to_string(alpha.root.join(".git/sf-tasks/hold.log")).unwrap();
    let lines: Vec<serde_json::Value> = history
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 2, "{history}");
    assert_eq!(
        (&lines[1]["did"], &lines[1]["by"], &lines[1]["of"]),
        (
            &serde_json::json!("release"),
            &serde_json::json!("vivid-swift"),
            &serde_json::json!("dunlin-probe")
        )
    );
}
