//! `tasks database`: the board's PostgreSQL said, held stopped with every board command refused
//! naming the hold, and released; and `tasks-move --restore`, a snapshot of it put back over it. On
//! the board of a fixture repository of its own outside this one (no `TASKS_SERVER`), its project
//! file naming the machine's PostgreSQL, its server started by its first command and stopped when
//! the case ends.

use crate::common::NOW;
use crate::{project_file, repository, run, scratch};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use symphony_board::cli::Env;
use symphony_board::server::{Home, Postgres};
use symphony_board::sys::pid_alive;

/// The folder of the machine's PostgreSQL programs, as the project file names them.
fn programs() -> PathBuf {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    symphony_board::server::postgres::discover_programs(&workspace).unwrap()
}

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
        std::fs::write(root.join("symphony.toml"), project_file("alpha", "")).unwrap();
        let machine = dir.join("machine");
        std::fs::create_dir_all(&machine).unwrap();
        Alpha { root, machine }
    }

    fn env(&self) -> Env {
        let vars: BTreeMap<String, String> = [
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
        Home::in_common(&self.root.join(".git"))
    }

    /// The board's PostgreSQL, its programs the project file's.
    fn pg(&self) -> Postgres {
        self.home().postgresql(&programs())
    }

    /// `tasks ARGS` here: its exit code, stdout and stderr.
    fn run(&self, args: &[&str]) -> (i32, String, String) {
        let ran = run(&self.env(), args, "");
        (ran.code, ran.out, ran.err)
    }
}

impl Drop for Alpha {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.root.join(".git/symphony/tasks/move.json"));
        let home = self.home();
        if let Some(hold) = home.held() {
            let _ = home.release(&hold.by, None);
        }
        let _ = self.pg().stop();
    }
}

fn data(root: &Path) -> String {
    root.join(".git/symphony/tasks/postgresql")
        .display()
        .to_string()
}

/// A board whose server is not running is said so, and held and released without one ever
/// starting: a board not made yet stays one.
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
            format!("the board's PostgreSQL at {data} is not held; no server runs on its data\n")
        )
    );
    let (code, out, err) = alpha.run(&["database", "hold", "a clone", "--as", "russet-lynx"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with("✔ the board's PostgreSQL is held by russet-lynx since "),
        "{out}"
    );
    assert!(
        out.ends_with(&format!(": a clone. Nothing serves {data} now\n")),
        "{out}"
    );
    let (code, _, err) = alpha.run(&["database", "release", "--as", "russet-lynx"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        alpha.pg().current().unwrap(),
        None,
        "nothing started the board"
    );
    assert!(home.unmade().unwrap(), "nothing made the board");
}

#[test]
fn a_held_board_refuses_every_command_naming_the_hold_until_its_release() {
    let alpha = Alpha::new("database-hold");
    let home = alpha.home();
    let (code, handle, err) = alpha.run(&["register", "--kind", "worker"]);
    assert_eq!(code, 0, "{err}");
    let running = alpha
        .pg()
        .current()
        .unwrap()
        .expect("the first command started the board");
    let data = data(&alpha.root);

    let (code, out, _) = alpha.run(&["database"]);
    assert_eq!(
        (code, out),
        (
            0,
            format!(
                "the board's PostgreSQL at {data} is not held; pid {} on {}\n",
                running.pid,
                running.socket.display()
            )
        )
    );

    let (code, _, err) = alpha.run(&["database", "hold", "a snapshot"]);
    assert_eq!(code, 2);
    assert!(err.starts_with("✖ no session: pass --as HANDLE"), "{err}");

    let (code, out, err) = alpha.run(&["database", "hold", "a snapshot", "--as", "russet-lynx"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with("✔ the board's PostgreSQL is held by russet-lynx since "),
        "{out}"
    );
    assert!(
        out.ends_with(&format!(": a snapshot. Nothing serves {data} now\n")),
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
    assert_eq!(
        alpha.pg().current().unwrap(),
        None,
        "a refused command started the board"
    );

    let (code, out, _) = alpha.run(&["database"]);
    assert_eq!(code, 0);
    assert_eq!(
        out,
        format!("the board's PostgreSQL at {data} is {hold}; no server runs on its data\n")
    );
    let (code, _, err) = alpha.run(&["database", "hold", "a swap", "--as", "vivid-swift"]);
    assert_eq!((code, err), (2, refused.clone()));
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
    assert_eq!(
        alpha.pg().current().unwrap(),
        None,
        "the release started the board"
    );
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
        alpha.pg().current().unwrap().is_some(),
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

/// The Symphony's read of a board whose starters' lock another holds (a start, a snapshot, a
/// restore) is refused once its bound passes, naming the holder, rather than hold the thread that
/// serves its pages; it starts no server. A holder that writes no note is said as one.
#[test]
fn the_symphonys_read_waits_on_anothers_start_at_most_its_bound() {
    let alpha = Alpha::new("database-read-wait");
    // A board made, its server stopped: the read would start it, and first takes the lock. One
    // not made yet the read refuses before it, as nothing would start.
    let (code, _, err) = alpha.run(&["list"]);
    assert_eq!(code, 0, "{err}");
    alpha.pg().stop().unwrap();
    let dir = alpha.root.join(".git/symphony/tasks");
    std::fs::create_dir_all(&dir).unwrap();
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("lock"))
        .unwrap();
    lock.lock().unwrap();
    // Held until the read is refused, or for the backstop of the 30 s the read was once bounded by,
    // when it is let go: a read that waited on the holder for as long as it held on would then go on
    // to the board, not be refused.
    let (refusal, heard) = std::sync::mpsc::channel::<()>();
    let holder = std::thread::spawn(move || {
        let _ = heard.recv_timeout(test_time::backstop(std::time::Duration::from_secs(30)));
        lock.unlock().unwrap();
    });
    let began = std::time::Instant::now();
    let refused = symphony_board::cli::board(&alpha.env())
        .err()
        .expect("the lock is held");
    let waited = began.elapsed();
    let _ = refusal.send(());
    holder.join().unwrap();
    assert!(waited >= std::time::Duration::from_secs(5), "{waited:?}");
    assert_eq!(
        refused,
        format!(
            "the board's starters' lock ({}) is held by a starter that says nothing of what it \
             does (a build without the lock's note); this read waits at most 5 s for it, and the \
             next one tries again",
            dir.join("lock").display()
        )
    );
    assert_eq!(
        alpha.pg().current().unwrap(),
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
            && err.ends_with(" (a swap), as symphony/tasks/hold.log records\n"),
        "{err}"
    );
    assert_eq!(home.held(), None);
    let history = std::fs::read_to_string(alpha.root.join(".git/symphony/tasks/hold.log")).unwrap();
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

/// `tasks-move ARGS` in `env`: its code, what it said and what it refused.
fn moving(env: &symphony_board::cli::Env, args: &[&str]) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(|word| (*word).to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = symphony_board::move_main(&args, env, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

/// A snapshot of the board's PostgreSQL is put back over it by `tasks-move --restore`, by its
/// name, under the caller's hold or one of its own, and the board's own command takes neither
/// `move` nor `restore`.
#[test]
fn a_snapshot_is_restored_over_the_board_by_the_command() {
    use symphony_board::NewTask;
    let alpha = Alpha::new("database-restore");
    let create = |slug: &str| {
        symphony_board::cli::connect(&alpha.env())
            .unwrap()
            .create(&NewTask::new(slug, "A task", "owner"))
            .unwrap();
    };
    create("sim/one");

    let env = alpha.env();
    // The board's own command takes neither word: the restore is a program of its own.
    for word in ["move", "restore"] {
        let (code, _, err) = alpha.run(&["database", word, "postgresql", "--as", "mover"]);
        assert_eq!(code, 2, "{word}");
        assert!(err.contains("usage: just tasks database"), "{err}");
    }
    let (code, _, err) = moving(&env, &["--restore", "never-taken"]);
    assert_eq!(code, 2);
    assert!(err.contains("no session: pass --as HANDLE"), "{err}");

    // A snapshot of it, a task made after, and the snapshot put back by its name.
    assert_eq!(
        alpha
            .run(&["database", "hold", "a snapshot", "--as", "mover"])
            .0,
        0
    );
    let (code, _, err) = alpha.run(&["snapshot"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(alpha.run(&["database", "release", "--as", "mover"]).0, 0);
    create("sim/two");
    let snapshots = alpha.pg().snapshots().unwrap();
    let name = snapshots[0]
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let (code, _, err) = moving(&env, &["--restore", "never-taken", "--as", "mover"]);
    assert_eq!(code, 2);
    assert!(
        err.contains("never-taken is no snapshot of this board's PostgreSQL"),
        "{err}"
    );
    let (code, out, err) = moving(&env, &["--restore", &name, "--as", "mover"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.contains("✔ the board's PostgreSQL is the snapshot"),
        "{out}"
    );
    assert!(out.contains("The cluster it replaced is kept in"), "{out}");
    assert!(
        out.contains("`just tasks recovery rebind` binds it again"),
        "{out}"
    );
    assert_eq!(
        alpha.home().held(),
        None,
        "the restore's own hold is released"
    );
    let (_, out, _) = alpha.run(&["list"]);
    assert!(out.contains("sim/one") && !out.contains("sim/two"), "{out}");

    // Again under the caller's own hold, which the restore leaves standing.
    assert_eq!(
        alpha
            .run(&["database", "hold", "a restore", "--as", "mover"])
            .0,
        0
    );
    let (code, _, err) = moving(&env, &["--restore", &name, "--as", "mover"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        alpha.home().held().map(|hold| hold.why),
        Some("a restore".to_string()),
        "the caller's hold stands"
    );
    assert_eq!(alpha.run(&["database", "release", "--as", "mover"]).0, 0);
    let (_, out, _) = alpha.run(&["list"]);
    assert!(out.contains("sim/one"), "{out}");
}

/// Under the marker the board recipes set, a board outside a scratch place is not moved or
/// restored, and nothing of it is touched.
#[test]
fn under_the_recipes_marker_a_board_outside_a_scratch_place_is_not_moved_or_restored() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("database-unasked-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let alpha = Alpha {
        root: repository(&dir.join("alpha")),
        machine: dir.join("machine"),
    };
    std::fs::create_dir_all(&alpha.machine).unwrap();
    std::fs::write(
        alpha.root.join("symphony.toml"),
        "[project]\nid = \"alpha\"\n",
    )
    .unwrap();
    let mut env = alpha.env();
    env.vars.insert("SYMPHONY_UNASKED".into(), "1".into());
    for words in [
        &["postgresql", "--as", "mover"][..],
        &["--restore", "2026-01-01T00-00-00Z", "--as", "mover"],
    ] {
        let (code, out, err) = moving(&env, words);
        assert_eq!((code, out.as_str()), (2, ""), "{words:?}");
        assert!(err.contains("SYMPHONY_UNASKED is set"), "{err}");
        assert!(err.contains("is outside a scratch place"), "{err}");
        assert!(err.contains("Nothing was touched"), "{err}");
    }
    let folder = alpha.root.join(".git/symphony/tasks");
    assert!(!folder.join("hold").exists() && !folder.join("moving").exists());
    drop(alpha);
    std::fs::remove_dir_all(&dir).unwrap();
}
