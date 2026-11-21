//! `tasks database`: the board's database said, held stopped with every board command refused
//! naming the hold, and released, on the board of a fixture repository of its own outside this one
//! (no `TASKS_SERVER`), its server started by its first command and stopped when the case ends.

use crate::common::{NOW, surreal};
use crate::{repository, run, scratch};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use symphony_board::cli::Env;
use symphony_board::server::{Home, Running};
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
            ("SYMPHONY_SURREAL", surreal().display().to_string()),
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
    root.join(".git/symphony/tasks/data").display().to_string()
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

/// A server the private replay's record names, its starter gone, is said as the replay's, never
/// the board's; the board's next command stops it cleanly, as it answers, and starts the board's
/// own, which `tasks database` then says.
#[test]
fn a_replays_server_its_starter_left_running_is_said_as_such_and_the_next_command_replaces_it() {
    let alpha = Alpha::new("database-left-replay");
    let home = alpha.home();
    let (code, _, err) = alpha.run(&["list"]);
    assert_eq!(code, 0, "{err}");
    let left = home
        .recorded()
        .expect("the first command started the board");
    // As a replay whose starter ended leaves it: named by the replay's record, by no address file.
    let dir = alpha.root.join(".git/symphony/tasks");
    std::fs::write(
        dir.join("replay.json"),
        serde_json::json!({
            "by": 1, "at": 0.0, "logged": 3 << 20, "memtable": 6 << 20, "server": left,
        })
        .to_string(),
    )
    .unwrap();
    std::fs::remove_file(dir.join("server.json")).unwrap();
    let data = data(&alpha.root);
    let (code, out, _) = alpha.run(&["database"]);
    assert_eq!(
        (code, out),
        (
            0,
            format!(
                "the board's database at {data} is not held. Its private replay's server is pid {} \
                 at {}, left running when its starter ended: never the board's server, it is \
                 stopped cleanly by the board's next command once it answers (never before), \
                 which then starts the board's own\n",
                left.pid, left.addr
            )
        )
    );

    let (code, _, err) = alpha.run(&["list"]);
    assert_eq!(code, 0, "{err}");
    let log = std::fs::read_to_string(dir.join("server.log")).unwrap();
    let stopped = |which: &str, besides: &str, running: &Running| {
        format!(
            "· the private replay's record stands, and {which} (pid {}, {}){besides} answers: \
             while the record stands every server on the data is taken for the replay's, so this \
             one is stopped cleanly, never adopted as the board's server\n",
            running.pid, running.addr
        )
    };
    assert!(
        log.contains(&stopped(
            "the server it names",
            ", left running when its starter ended,",
            &left
        )),
        "{log}"
    );
    assert!(!pid_alive(left.pid), "the replay's server still runs");
    assert!(!dir.join("replay.json").exists());
    let running = home.recorded().expect("the board's own server");
    assert_ne!(running.pid, left.pid);
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

    // A record that does not read, naming none, while the board's own server runs: that server is
    // said as one the record does not name, taken for the replay's, and replaced by the next.
    std::fs::write(dir.join("replay.json"), "{not json").unwrap();
    let (code, out, _) = alpha.run(&["database"]);
    assert_eq!(
        (code, out),
        (
            0,
            format!(
                "the board's database at {data} is not held. Its server is pid {} at {}, which \
                 the private replay's record (symphony/tasks/replay.json) does not name: while \
                 that record stands every server on the data is taken for the replay's, never the \
                 board's, so the board's next command stops it cleanly once it answers (never \
                 before), then starts the board's own\n",
                running.pid, running.addr
            )
        )
    );
    let (code, _, err) = alpha.run(&["list"]);
    assert_eq!(code, 0, "{err}");
    let log = std::fs::read_to_string(dir.join("server.log")).unwrap();
    assert!(
        log.contains(&stopped(
            "the server the address file names",
            ", which the record does not name,",
            &running
        )),
        "{log}"
    );
    assert!(!dir.join("replay.json").exists());
    assert_ne!(home.recorded().map(|r| r.pid), Some(running.pid));
}

/// While whether the private replay's server still runs cannot be told (here the pid its record
/// names runs, and no server is on the data), `tasks database` says so, why, and what the owner
/// does, never that the next command starts a server: that command is refused, starting none.
#[test]
fn a_replay_whose_server_cannot_be_told_is_said_as_such_and_refuses_the_next_command() {
    let alpha = Alpha::new("database-untold");
    let home = alpha.home();
    let dir = alpha.root.join(".git/symphony/tasks");
    std::fs::create_dir_all(&dir).unwrap();
    // This test's own process: it runs, and serves no data.
    let pid = std::process::id();
    std::fs::write(
        dir.join("replay.json"),
        serde_json::json!({
            "by": 1, "at": 0.0, "logged": 3 << 20, "memtable": 6 << 20,
            "server": {"addr": "127.0.0.1:1", "pid": pid, "version": "3.0.0"},
        })
        .to_string(),
    )
    .unwrap();
    let data = data(&alpha.root);
    let record = dir.join("replay.json");
    let why = format!(
        "the server it names, pid {pid}, runs, and neither it nor any other process is a server \
         on this board's data as `ps` lists them: it may be between programs as it starts"
    );
    let rule = format!(
        "While its record ({}) stands, every server on the data is the replay's: none is adopted \
         as the board's, none is started beside it, and the record is removed only once the \
         server it names has ended and `ps` lists no server on the data. The next command asks \
         again, and settles the record once `ps` can tell. Should it never, remove the record by \
         hand once `ps -ww -axo pid,command` lists no `surreal start` on surrealkv://{data} \
         (docs/TASKS.md, Stuck servers)",
        record.display()
    );
    let (code, out, _) = alpha.run(&["database"]);
    assert_eq!(
        (code, out),
        (
            0,
            format!(
                "the board's database at {data} is not held. No server is found on its data. The \
                 private replay's record stands, and whether its server still runs on the data \
                 cannot be told: {why}, so the board's next command is refused while it cannot. \
                 {rule}\n"
            )
        )
    );
    let (code, _, err) = alpha.run(&["list"]);
    assert_ne!(code, 0);
    assert!(
        err.contains(&format!(
            "whether the private replay's server still runs on this board's data cannot be told: \
             {why}. {rule}"
        )),
        "{err}"
    );
    assert!(record.exists(), "the record went");
    assert_eq!(home.recorded(), None, "a server started beside it");
}

/// The snapshots that hold a log (one taken before a replay among them) are listed with the bytes
/// of their logs, the owner's to remove, the oldest first, and so is a clone cut short; one whose
/// log is empty, pruned by count, is not.
#[test]
fn the_snapshots_that_hold_a_log_are_listed_for_the_owner() {
    let alpha = Alpha::new("database-kept");
    let dir = alpha.root.join(".git/symphony/tasks");
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

/// The folder of the machine's PostgreSQL programs, as the project file names them.
fn programs() -> PathBuf {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    symphony_board::server::postgres::discover_programs(&workspace).unwrap()
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

/// `alpha`'s PostgreSQL stopped when the case ends, whichever engine its board is on then.
struct Stopped<'a>(&'a Alpha);

impl Drop for Stopped<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.0.root.join(".git/symphony/tasks/move.json"));
        let home = self.0.home();
        if let Some(hold) = home.held() {
            let _ = home.release(&hold.by, None);
        }
        let _ = home.postgresql(&programs()).stop();
    }
}

/// The board's store is moved to PostgreSQL and back by `tasks-move`, a snapshot of its
/// PostgreSQL put back over it, and the board's own command takes neither word.
#[test]
fn a_board_is_moved_between_engines_and_a_snapshot_restored_over_it_by_the_command() {
    use symphony_board::NewTask;
    use symphony_board::server::Engine;
    let alpha = Alpha::new("database-move");
    let _stopped = Stopped(&alpha);
    std::fs::write(
        alpha.root.join("symphony.toml"),
        format!(
            "[project]\nid = \"alpha\"\n[board]\npostgresql = {:?}\n",
            programs().display().to_string()
        ),
    )
    .unwrap();
    let create = |slug: &str| {
        symphony_board::cli::connect(&alpha.env())
            .unwrap()
            .create(&NewTask::new(slug, "A task", "owner"))
            .unwrap();
    };
    create("sim/one");

    let env = alpha.env();
    // The board's own command takes neither word: the move is a program of its own.
    for word in ["move", "restore"] {
        let (code, _, err) = alpha.run(&["database", word, "postgresql", "--as", "mover"]);
        assert_eq!(code, 2, "{word}");
        assert!(err.contains("usage: just tasks database"), "{err}");
    }
    let (code, _, err) = moving(&env, &["postgresql"]);
    assert_eq!(code, 2);
    assert!(err.contains("no session: pass --as HANDLE"), "{err}");
    assert_eq!(alpha.home().backend().unwrap(), Engine::SurrealDb);

    let (code, out, err) = moving(&env, &["postgresql", "--as", "mover"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("✔ the board is on PostgreSQL"), "{out}");
    assert!(out.contains("rows compared with what was read"), "{out}");
    assert!(out.contains("Its SurrealDB store is kept in"), "{out}");
    assert!(
        out.contains("`just tasks recovery rebind` binds it again"),
        "{out}"
    );
    assert!(out.contains("The hold is released"), "{out}");
    assert_eq!(alpha.home().backend().unwrap(), Engine::PostgreSql);
    let (code, out, err) = alpha.run(&["list"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("sim/one"), "{out}");
    let (code, _, err) = moving(&env, &["postgresql", "--as", "mover"]);
    assert_eq!(code, 2);
    assert!(
        err.contains("is on PostgreSQL already: nothing was moved"),
        "{err}"
    );

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
    let snapshots = alpha.home().postgresql(&programs()).snapshots().unwrap();
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
    let (_, out, _) = alpha.run(&["list"]);
    assert!(out.contains("sim/one") && !out.contains("sim/two"), "{out}");

    // And back, under the caller's own hold, which the move leaves standing.
    assert_eq!(
        alpha
            .run(&["database", "hold", "a move", "--as", "mover"])
            .0,
        0
    );
    let (code, out, err) = moving(&env, &["surrealdb", "--as", "mover"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("✔ the board is on SurrealDB"), "{out}");
    assert!(
        out.contains("The board is still held, by the hold set before the move"),
        "{out}"
    );
    assert_eq!(alpha.run(&["database", "release", "--as", "mover"]).0, 0);
    let (_, out, _) = alpha.run(&["list"]);
    assert!(out.contains("sim/one"), "{out}");
}

/// A release as its install lays it in `machine` (`releases/NAME/bin/`, `current` naming it),
/// holding this build's `tasks` and `tasks-move`: each a link to the file Cargo built, which
/// has run before, where a copy would be a new file macOS assesses under the case's clock.
fn released(machine: &Path) {
    let bin = machine.join("releases/release-2026-11-21/bin");
    std::fs::create_dir_all(&bin).unwrap();
    for (name, built) in [
        ("tasks", env!("CARGO_BIN_EXE_tasks")),
        ("tasks-move", env!("CARGO_BIN_EXE_tasks-move")),
    ] {
        std::os::unix::fs::symlink(built, bin.join(name)).unwrap();
    }
    std::os::unix::fs::symlink("releases/release-2026-11-21", machine.join("current")).unwrap();
}

/// The release's `name` in `alpha`'s machine folder, found as a recipe finds it past a
/// checkout's links, run with `args` in the project: its code, what it said and what it refused.
fn installed(alpha: &Alpha, name: &str, args: &[&str]) -> (i32, String, String) {
    assert!(
        symphony_board::builds::link(&alpha.root, name).is_none(),
        "the project has no build of its own"
    );
    let beyond = symphony_board::builds::Beyond {
        machine: Some(alpha.machine.clone()),
        path: None,
    };
    let binary = beyond
        .find(name)
        .unwrap_or_else(|| panic!("the release holds no {name}"));
    assert!(binary.starts_with(alpha.machine.join("current/bin")));
    let mut vars = alpha.env().vars;
    vars.extend(std::env::var("PATH").map(|path| ("PATH".to_string(), path)));
    // A locale, as a shell has one: macOS's PostgreSQL does not start in an environment
    // naming none ("postmaster became multithreaded during startup").
    vars.insert("LC_ALL".to_string(), "C".to_string());
    let done = symphony_board::sys::output(
        symphony_board::sys::command_with(&binary, &vars)
            .args(args)
            .current_dir(&alpha.root),
    )
    .unwrap();
    (
        done.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&done.stdout).into_owned(),
        String::from_utf8_lossy(&done.stderr).into_owned(),
    )
}

/// A project with no build of its own, on the release the machine runs, moves its seeded board
/// to PostgreSQL and back with what the release installed alone: its `tasks-move` beside its
/// `tasks`, each run by its path under `current`.
#[test]
fn a_project_on_the_installed_release_moves_its_board_with_the_releases_own_program() {
    use symphony_board::server::Engine;
    let alpha = Alpha::new("database-move-released");
    let _stopped = Stopped(&alpha);
    released(&alpha.machine);
    std::fs::write(
        alpha.root.join("symphony.toml"),
        format!(
            "[project]\nid = \"alpha\"\n[board]\npostgresql = {:?}\n",
            programs().display().to_string()
        ),
    )
    .unwrap();
    let listed = |engine: Engine| {
        assert_eq!(alpha.home().backend().unwrap(), engine);
        let (code, out, err) = installed(&alpha, "tasks", &["list"]);
        assert_eq!(code, 0, "{err}");
        assert!(out.contains("sim/one") && out.contains("sim/two"), "{out}");
    };
    let (code, out, err) = installed(&alpha, "tasks", &["register", "--kind", "orchestrator"]);
    assert_eq!(code, 0, "{err}");
    let handle = out.trim();
    for slug in ["sim/one", "sim/two"] {
        let (code, _, err) =
            installed(&alpha, "tasks", &["create", slug, "A task", "--as", handle]);
        assert_eq!(code, 0, "{err}");
    }
    listed(Engine::SurrealDb);

    let (code, out, err) = installed(&alpha, "tasks-move", &["postgresql", "--as", "mover"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("✔ the board is on PostgreSQL"), "{out}");
    assert!(out.contains("rows compared with what was read"), "{out}");
    listed(Engine::PostgreSql);

    let (code, out, err) = installed(&alpha, "tasks-move", &["surrealdb", "--as", "mover"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("✔ the board is on SurrealDB"), "{out}");
    listed(Engine::SurrealDb);
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
