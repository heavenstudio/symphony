//! A live board whose store went back below the highest schema it was seen at
//! ([`symphony_board::live::seen`]): a snapshot of its PostgreSQL taken by `tasks snapshot` while
//! no server ran on it, the board moved forward since, then its cluster put back from the snapshot,
//! as a store that lost its newest writes comes back. Every program that opens it refuses it, `tasks
//! migrate` included, in the rollback's words, writing nothing, until the owner accepts the older
//! board with `tasks schema --accept`; a store that lost the whole board is refused the same way.

use super::{COPY, Held, Live, SEND, TASKS, before, refused, this};
use crate::children::Child as _;
use crate::common::NOW;
use std::path::{Path, PathBuf};
use symphony_board::live::seen::{self, Rollback};
use symphony_board::{Opening, Raw, Store};

impl Live {
    /// The board's PostgreSQL data, in its folder in the common dir.
    fn data(&self) -> PathBuf {
        self.main.join(".git/symphony/tasks/postgresql")
    }

    /// The folder its PostgreSQL snapshots are kept in.
    fn snapshots(&self) -> PathBuf {
        self.main.join(".git/symphony/tasks/postgresql-snapshots")
    }

    /// The board, as the project's live one.
    fn board(&self) -> symphony_board::live::Live {
        symphony_board::live::Live {
            project: "live".into(),
            common: self.main.join(".git").canonicalize().unwrap(),
        }
    }

    /// The rollback of the board from this build's schema, first seen at [`NOW`], to `to`.
    fn rollback(&self, to: Option<i64>) -> String {
        Rollback {
            project: "live".into(),
            from: this(),
            since: NOW,
            to,
            record: seen::path(&self.board().common),
        }
        .refusal()
    }

    /// How many tasks the board holds: one read.
    fn tasks_held(&self) -> i64 {
        let held = Raw::of(&self.database("tasks")).table("task").count();
        i64::try_from(held.unwrap()).unwrap()
    }
}

/// `snapshot` cloned to `data`, which must not exist, as a restore puts a cluster back:
/// copy-on-write, never the snapshot itself.
fn restore(snapshot: &Path, data: &Path) {
    let clone = if cfg!(target_os = "macos") {
        &["-c", "-R"][..]
    } else {
        &["-R", "--reflink=auto"][..]
    };
    let done = std::process::Command::of("cp")
        .args(clone)
        .arg(snapshot)
        .arg(data)
        .output()
        .unwrap();
    assert!(done.status.success(), "{done:?}");
}

#[test]
fn a_live_board_put_back_below_the_schema_it_was_seen_at_is_refused_everywhere_until_accepted() {
    let live = Live::older("main-only-rolled-back", Held::Own);
    let record = seen::path(&live.board().common);

    // A snapshot of the store at the older schema: refused while its server runs, taken once none
    // does, and listed.
    let ran = live.tasks(&live.main, &["snapshot"]);
    assert_eq!(ran.code, 2, "{}", ran.out);
    assert!(
        ran.err.starts_with(&format!(
            "✖ PostgreSQL at {} is running or its status is unknown: no snapshot was taken (",
            live.data().display()
        )),
        "{}",
        ran.err
    );
    live.home().stop_board().unwrap();
    let ran = live.tasks(&live.main, &["snapshot"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert!(
        ran.out
            .starts_with("✔ took a PostgreSQL snapshot of the live board's store: "),
        "{}",
        ran.out
    );
    let listed = live.tasks(&live.wt, &["snapshot", "--list"]);
    assert_eq!(listed.code, 0, "{}", listed.err);
    let rows: Vec<&str> = listed.out.lines().collect();
    assert_eq!(rows.len(), 1, "{}", listed.out);
    let snapshot = PathBuf::from(rows[0]);
    assert_eq!(
        snapshot,
        live.snapshots()
            .join(symphony_board::server::snapshot::stamp(NOW))
    );
    assert!(
        ran.out.trim_end().ends_with(rows[0]),
        "the taking names what is listed: {}",
        ran.out
    );
    assert_eq!(
        seen::read(&live.board().common),
        Some(seen::Seen {
            version: before(),
            at: NOW
        }),
        "the build that made the board recorded its schema: {:?}",
        std::fs::read_to_string(&record)
    );

    // Main's build moves the board forward from any folder, and the record says what it saw. (The
    // server is the test's, which outlives each command.)
    live.start();
    let ran = live.tasks(&live.wt, &["list"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(live.version("tasks"), Some(this()));
    assert_eq!(
        seen::read(&live.board().common),
        Some(seen::Seen {
            version: this(),
            at: NOW
        }),
        "{:?}",
        std::fs::read_to_string(&record)
    );

    // The store goes back: stopped, and put back from the snapshot, at the older schema.
    live.home().stop_board().unwrap();
    std::fs::rename(
        live.data(),
        live.main.join(".git/symphony/tasks/postgresql.newer"),
    )
    .unwrap();
    restore(&snapshot, &live.data());
    live.start();
    let why = live.rollback(Some(before()));

    for cwd in [&live.wt, &live.main] {
        refused(&live.tasks(cwd, &["list"]), &why);
    }
    let call = r#"{"method": "get", "args": {"slug": "live/one"}}"#;
    let ran = live.run(Path::new(TASKS), &live.wt, &[], &["api"], call);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&ran.out).ok(),
        Some(serde_json::json!({"error": why})),
        "{}{}",
        ran.err,
        ran.out
    );
    let ran = live.run(
        Path::new(SEND),
        &live.wt,
        &[],
        &["someone", "hello", "--as", "me"],
        "",
    );
    refused(&ran, &why);
    // `tasks migrate`, which alone would move a consuming project's board, writes nothing either.
    refused(&live.tasks(&live.main, &["migrate"]), &why);
    // The Symphony's reader, at this build's schema and one behind it.
    for schema in [
        symphony_board::Schema::this_build(),
        symphony_board::Schema::this_build().before(),
    ] {
        let reader = Store::opening_on(
            live.database("tasks"),
            Box::new(|| NOW),
            Box::new(|_| false),
            "this-host",
            &Opening::read()
                .at(schema)
                .of(Some("live"))
                .live(Some(live.board())),
        );
        assert_eq!(reader.err().map(|e| e.to_string()), Some(why.clone()));
    }
    assert_eq!(live.version("tasks"), Some(before()), "nothing moved it");
    assert_eq!(live.tasks_held(), 1);

    // `tasks schema` says it went back; only the schema the board holds is accepted.
    let ran = live.tasks(&live.wt, &["schema"]);
    assert_eq!(
        (ran.code, ran.out),
        (1, format!("⚠ {why}\n")),
        "{}",
        ran.err
    );
    refused(
        &live.tasks(&live.wt, &["schema", "--accept", &this().to_string()]),
        &format!(
            "the live board holds schema {}, not schema {}: nothing was accepted",
            before(),
            this()
        ),
    );
    refused(&live.tasks(&live.wt, &["list"]), &why);
    let ran = live.tasks(&live.wt, &["schema", "--accept", &before().to_string()]);
    assert_eq!(
        (ran.code, ran.out),
        (
            0,
            format!(
                "✔ accepted the live board at schema {}, below the schema {} it was seen at: its \
                 record ({}) counts from there now, and the board is read, and moved forward, \
                 again\n",
                before(),
                this(),
                record.display()
            )
        ),
        "{}",
        ran.err
    );

    // Accepted, it is moved forward again as any board behind main's build is.
    let ran = live.tasks(&live.wt, &["list"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert!(ran.out.contains("live/one"), "{}", ran.out);
    assert_eq!(live.version("tasks"), Some(this()));
    let ran = live.tasks(&live.wt, &["schema"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert!(
        ran.out.starts_with(&format!(
            "the live board is at schema {}; the highest schema it was seen at is {}, on ",
            this(),
            this()
        )),
        "{}",
        ran.out
    );
    let ran = live.tasks(&live.wt, &["schema", "--accept", &this().to_string()]);
    assert_eq!(
        (ran.code, ran.out),
        (
            0,
            format!(
                "✔ the live board is at schema {} and reads below no schema it was seen at: \
                 nothing to accept\n",
                this()
            )
        )
    );
    // Nothing was ever started on the snapshot itself.
    assert!(snapshot.is_dir());
    assert_eq!(
        live.tasks(&live.main, &["snapshot", "--list"]).out,
        listed.out
    );
}

#[test]
fn a_store_that_lost_the_whole_board_went_back_too_and_is_accepted_as_none() {
    // A consuming project's: only `tasks migrate` in its main checkout moves its board.
    let live = Live::new("main-only-lost-store", Held::Nothing);
    let ran = live.tasks(&live.wt, &["list"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(live.version("tasks"), Some(this()));

    // Its cluster gone, the next start makes an empty one, which holds no board.
    live.home().stop_board().unwrap();
    std::fs::rename(
        live.data(),
        live.main.join(".git/symphony/tasks/postgresql.lost"),
    )
    .unwrap();
    live.start();
    let why = live.rollback(None);
    assert!(
        why.starts_with(&format!(
            "the live board went back from schema {} to no board at all: ",
            this()
        )),
        "{why}"
    );
    refused(&live.tasks(&live.wt, &["list"]), &why);
    refused(&live.tasks(&live.main, &["migrate"]), &why);
    assert_eq!(
        live.version("tasks"),
        None,
        "nothing made a board in its place"
    );

    let ran = live.tasks(&live.wt, &["schema"]);
    assert_eq!((ran.code, ran.out), (1, format!("⚠ {why}\n")));
    refused(
        &live.tasks(&live.wt, &["schema", "--accept", "1"]),
        "the live board holds no board, not schema 1: nothing was accepted",
    );
    let ran = live.tasks(&live.wt, &["schema", "--accept", "none"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert!(
        ran.out.starts_with(&format!(
            "✔ accepted the live board with no board, below the schema {} it was seen at",
            this()
        )),
        "{}",
        ran.out
    );
    let ran = live.tasks(&live.wt, &["list"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(
        live.version("tasks"),
        Some(this()),
        "the first writer makes it"
    );
}

#[test]
fn a_scratch_board_keeps_no_record_and_a_board_tasks_server_names_has_no_store_to_snapshot() {
    let live = Live::new("main-only-rollback-scratch", Held::Own);
    live.seed("scratch");
    let ran = live.tasks(&live.wt, &["--database", "scratch", "list"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert!(!seen::path(&live.board().common).exists());
    let ran = live.tasks(&live.wt, &["--database", "scratch", "schema"]);
    assert_eq!(
        (ran.code, ran.out),
        (
            0,
            format!(
                "the board in database scratch at {} is at schema {}; a scratch board keeps no \
                 record of the schema it was seen at\n",
                live.addr(),
                this()
            )
        )
    );
    refused(
        &live.tasks(
            &live.wt,
            &["--database", "scratch", "schema", "--accept", "1"],
        ),
        &format!(
            "the board in database scratch at {} is a scratch board, which keeps no record of \
             the schema it was seen at: nothing was accepted",
            live.addr()
        ),
    );
    refused(
        &live.tasks(&live.wt, &["schema", "--accept", "x"]),
        "--accept takes the schema version the board holds, or none for a store with no board, \
         not \"x\"",
    );
    let addr = live.addr();
    let ran = live.run(
        Path::new(TASKS),
        &live.wt,
        &[("TASKS_SERVER", &addr)],
        &["snapshot"],
        "",
    );
    refused(
        &ran,
        "a board TASKS_SERVER names has no store here to take a snapshot of: nothing was taken",
    );
    let ran = live.tasks(&live.wt, &["snapshot", "--list"]);
    assert_eq!(
        (ran.code, ran.out.as_str()),
        (
            0,
            "no PostgreSQL snapshot of the live board's store is kept\n"
        )
    );
    // The live board, never opened, has no record yet.
    let ran = live.tasks(&live.wt, &["schema"]);
    assert_eq!(
        (ran.code, ran.out),
        (
            0,
            format!(
                "the live board holds no board; no schema it was seen at is recorded yet ({})\n",
                seen::path(&live.board().common).display()
            )
        ),
        "{}",
        ran.err
    );
}

/// `cli::board_client`, which consumers' code calls in its own process and opens its own store on
/// with `Store::open`, naming no live board: it passes a live board with no record yet and one at its
/// record, refuses one below its record in the rollback's words before it returns a client, by an
/// address too, and passes a scratch board whatever the live board's record says.
#[test]
fn board_client_refuses_a_live_board_below_its_record_and_passes_a_healthy_or_scratch_one() {
    let live = Live::new("main-only-rollback-board-client", Held::Own);
    let reach = |vars: &[(&str, &str)]| {
        let mut env = symphony_board::cli::Env {
            vars: Default::default(),
            cwd: live.wt.clone(),
        };
        for (name, value) in [
            ("HOME", live.base.join("home")),
            ("SYMPHONY_MACHINE_DIR", live.machine.clone()),
        ] {
            env.vars.insert(name.into(), value.to_str().unwrap().into());
        }
        for (name, value) in vars {
            env.vars.insert(name.to_string(), value.to_string());
        }
        symphony_board::cli::board_client(&env)
    };
    let record = seen::path(&live.board().common);

    // Never opened, the live board has no record, and nothing is read.
    let client = reach(&[]).unwrap();
    assert_eq!(
        (client.addr(), client.database()),
        (live.addr().as_str(), "tasks")
    );
    assert!(!record.exists());

    // Main's build makes it and records its schema: at its record it passes, and the store a
    // consumer opens on the client reads it.
    let ran = live.tasks(&live.wt, &["list"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(
        seen::read(&live.board().common).map(|s| s.version),
        Some(this())
    );
    let store = Store::open(reach(&[]).unwrap()).unwrap();
    assert_eq!(store.board_version().unwrap(), Some(this()));

    // A newer build saw it one schema above this one's: it went back, and no client is returned.
    let mut text = std::fs::read_to_string(&record).unwrap();
    text.push_str(&format!("{} {} seen x\n", this() + 1, NOW as i64));
    std::fs::write(&record, text).unwrap();
    let why = Rollback {
        project: "live".into(),
        from: this() + 1,
        since: NOW.floor(),
        to: Some(this()),
        record: record.clone(),
    }
    .refusal();
    assert_eq!(reach(&[]).err(), Some(why.clone()));
    let addr = live.addr();
    assert_eq!(
        reach(&[("TASKS_SERVER", &addr)]).err(),
        Some(why),
        "an address names the same board"
    );

    // A scratch board keeps no record: passed, and moved forward by the store opened on it.
    live.seed("scratch");
    let client = reach(&[("TASKS_DATABASE", "scratch")]).unwrap();
    assert_eq!(client.database(), "scratch");
    let store = Store::open(client).unwrap();
    assert_eq!(store.board_version().unwrap(), Some(this()));
    assert_eq!(
        live.version("tasks"),
        Some(this()),
        "the live board untouched"
    );
}

/// The record above this build's schema over a board at it: a board that went back from a schema a
/// newer build moved it to, read by this one. The schema guard refuses a write only to a board at
/// another schema, so it would let every writer of this build through; each refuses the board in
/// the rollback's words all the same, and writes nothing: `tasks migrate` from a linked worktree
/// (never sent to the main checkout), `tasks-copy` into it, out of it and by its server's address,
/// the heartbeat (quietly: it never refuses a tool call, and binds nothing), the recorder (which
/// runs its command and asks the board nothing), and the detached dispatcher.
#[test]
fn every_opener_refuses_a_board_below_its_record_even_one_at_its_own_builds_schema() {
    use symphony_board::cli::machine_work::recorded;
    let live = Live::new("main-only-rollback-openers", Held::Own);
    let keys = live.base.join("keys");
    let keys_var = ("TASKS_KEYS", keys.to_str().unwrap());
    let ran = live.tasks(&live.wt, &["register", "--kind", "worker"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    let handle = ran.out.trim().to_string();
    for args in [
        &["create", "live/held", "Held by the worker"][..],
        &["claim", "live/held"],
    ] {
        let ran = live.tasks(&live.wt, &[args, &["--as", &handle]].concat());
        assert_eq!(ran.code, 0, "{args:?}: {}", ran.err);
    }
    let beat = |session: &str| {
        let event = serde_json::json!({
            "hook_event_name": "PreToolUse", "tool_name": "Bash", "session_id": session,
            "tool_input": {"command": format!("TASKS_SESSION={handle} just tasks inbox")},
            "cwd": live.wt,
        });
        live.run(
            Path::new(TASKS),
            &live.wt,
            &[keys_var],
            &["beat"],
            &event.to_string(),
        )
    };
    let work = recorded::in_common(&live.board().common);
    // One recorded run labelled `label`: the session and task its finished record names, or none
    // when its recorder let it go unrecorded (no record of it, and no lease left in the work folder,
    // which a recorder takes before its command starts and lets go once it finished it).
    let recorded_once = |label: &str| {
        let ran = live.run(
            Path::new(TASKS),
            &live.wt,
            &[("TASKS_SESSION", &handle)],
            &["run", "--phase", "build", "--label", label, "--", "true"],
            "",
        );
        assert_eq!((ran.code, ran.err.as_str()), (0, ""), "{}", ran.out);
        let operations = work.join(recorded::OPERATIONS);
        test_time::until(
            format!("the run {label} finished, or let go unrecorded"),
            test_time::backstop(std::time::Duration::from_secs(30)),
            || {
                let found = (recorded::read(&work).records.into_iter())
                    .find(|(run, _)| run.label == label && run.state == recorded::Step::Finished);
                if let Some((run, _)) = found {
                    return Some(Some((run.handle, run.task)));
                }
                let leased = std::fs::read_dir(&operations).is_ok_and(|entries| {
                    entries
                        .flatten()
                        .any(|entry| entry.path().extension().is_some_and(|ext| ext == "lease"))
                });
                (!leased).then_some(None)
            },
        )
        .unwrap()
    };
    // A recorded run for `label`, run again under a label of its own each time until its recorder
    // `answered`, at most ATTEMPTS times. On a live board's database the recorder keeps its own
    // waits, a second for its word to its command and a second for the board's answer (a test's
    // scale is never taken there), so a loaded machine may let one run go unanswered; a recorder
    // that never finds the answer fails every attempt. What a run found: its session and task.
    const ATTEMPTS: usize = 5;
    type Found = (Option<String>, Option<String>);
    let recorded_run = |label: &str, answered: &dyn Fn(&Found) -> bool| {
        let mut said = Vec::new();
        for attempt in 1..=ATTEMPTS {
            match recorded_once(&format!("{label}-{attempt}")) {
                Some(found) if answered(&found) => return found,
                other => said.push(other),
            }
        }
        panic!("the recorder never answered for {label} in {ATTEMPTS} runs: {said:?}");
    };
    // While it reads at its record, the beat binds and the recorder finds the session's task.
    let ran = beat("s-before");
    assert_eq!((ran.code, ran.out.as_str(), ran.err.as_str()), (0, "", ""));
    assert!(keys.join("s-before").is_file());
    // Answered once the board said a task: a board that did not answer within its second says none.
    assert_eq!(
        recorded_run("before", &|(_, task)| task.is_some()),
        (Some(handle.clone()), Some("live/held".into()))
    );

    // A newer build saw it one schema above this one's.
    let record = seen::path(&live.board().common);
    let mut text = std::fs::read_to_string(&record).unwrap();
    text.push_str(&format!("{} {} seen x\n", this() + 1, NOW as i64));
    std::fs::write(&record, text).unwrap();
    let why = Rollback {
        project: "live".into(),
        from: this() + 1,
        since: NOW.floor(),
        to: Some(this()),
        record: record.clone(),
    }
    .refusal();

    refused(&live.tasks(&live.wt, &["list"]), &why);
    for cwd in [&live.wt, &live.main] {
        refused(&live.tasks(cwd, &["migrate"]), &why);
    }
    let copy_refused = format!("❌ tasks-copy: {why}\n");
    for (cwd, args) in [
        (&live.wt, vec!["--board".to_string()]),
        (&live.wt, vec!["--board".into(), "--out".into()]),
        (&live.outside, vec![live.addr()]),
    ] {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let ran = live.run(Path::new(COPY), cwd, &[], &args, "{}");
        assert_eq!(
            (ran.code, ran.err.as_str()),
            (2, copy_refused.as_str()),
            "{args:?}"
        );
        assert_eq!(ran.out, "", "{args:?}");
    }
    let ran = beat("s-after");
    assert_eq!((ran.code, ran.out.as_str(), ran.err.as_str()), (0, "", ""));
    assert!(!keys.join("s-after").exists(), "no key written");
    // Answered once recorded at all: the board refused, so the recorder names no task.
    assert_eq!(
        recorded_run("after", &|_| true),
        (Some(handle.clone()), None)
    );
    let ran = live.tasks(&live.wt, &["dispatch"]);
    assert_eq!(ran.code, 2, "{}", ran.out);
    assert!(
        ran.err.ends_with(&format!("] CALLBACK deferred: {why}\n")),
        "{}",
        ran.err
    );

    // Accepted, the board reads again: the beat after the record bound nothing.
    let ran = live.tasks(&live.wt, &["schema", "--accept", &this().to_string()]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    let bound = |agent: &str| {
        let call = serde_json::json!({"method": "handle_for_agent", "args": {"agent": agent}});
        let ran = live.run(Path::new(TASKS), &live.wt, &[], &["api"], &call.to_string());
        serde_json::from_str::<serde_json::Value>(&ran.out).unwrap()["ok"].clone()
    };
    assert_eq!(bound("s-before"), serde_json::json!(handle));
    assert_eq!(bound("s-after"), serde_json::Value::Null);
}
