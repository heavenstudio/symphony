//! A live board whose store went back below the highest schema it was seen at
//! ([`symphony_board::live::seen`]): a snapshot of its store taken by `tasks snapshot` while no
//! server ran on it, the board moved forward since, then its store put back from the snapshot, as a
//! store that lost its newest writes comes back. Every reader and writer refuses it, `tasks
//! migrate` included, in the rollback's words, writing nothing, until the owner accepts the older
//! board with `tasks schema --accept`; a store that lost the whole board is refused the same way.

use super::{Held, Live, SEND, TASKS, before, refused, this};
use crate::common::NOW;
use std::path::{Path, PathBuf};
use symphony_board::live::seen::{self, Rollback};
use symphony_board::{Opening, Store};

impl Live {
    /// The board's store, in its folder in the common dir.
    fn data(&self) -> PathBuf {
        self.main.join(".git/sf-tasks/data")
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
        self.client("tasks")
            .query(
                "RETURN array::len(SELECT VALUE id FROM task);",
                serde_json::json!({}),
            )
            .unwrap()
            .last()
            .and_then(serde_json::Value::as_i64)
            .unwrap()
    }
}

/// `snapshot` cloned to `data`, which must not exist, as a restore puts a store back: copy-on-write,
/// never the snapshot itself.
fn restore(snapshot: &Path, data: &Path) {
    let clone = if cfg!(target_os = "macos") {
        &["-c", "-R"][..]
    } else {
        &["-R", "--reflink=auto"][..]
    };
    let done = std::process::Command::new("cp")
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
        ran.err.starts_with("✖ the board's server (pid ")
            && ran.err.contains(&format!(
                "runs on {}: a snapshot is taken only while no process serves the store, never of \
                 files a server is writing, so none was taken",
                live.data().display()
            )),
        "{}",
        ran.err
    );
    live.home().stop().unwrap();
    let ran = live.tasks(&live.main, &["snapshot"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert!(
        ran.out
            .starts_with("✔ took a snapshot of the live board's store: "),
        "{}",
        ran.out
    );
    let listed = live.tasks(&live.wt, &["snapshot", "--list"]);
    assert_eq!(listed.code, 0, "{}", listed.err);
    let rows: Vec<&str> = listed.out.lines().collect();
    assert_eq!(rows.len(), 1, "{}", listed.out);
    let (name, at) = rows[0].split_once("  ").unwrap();
    let snapshot = PathBuf::from(at);
    assert_eq!(
        snapshot,
        live.main.join(".git/sf-tasks/snapshots").join(name)
    );
    assert!(
        ran.out.trim_end().ends_with(at),
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
    live.home().ensure().unwrap();
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
    live.home().stop().unwrap();
    std::fs::rename(live.data(), live.main.join(".git/sf-tasks/data.newer")).unwrap();
    restore(&snapshot, &live.data());
    live.home().ensure().unwrap();
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
        let reader = Store::opening(
            live.client("tasks"),
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

    live.home().stop().unwrap();
    std::fs::rename(live.data(), live.main.join(".git/sf-tasks/data.lost")).unwrap();
    live.home().ensure().unwrap();
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
        (ran.code, ran.out),
        (
            0,
            format!(
                "no snapshot of the live board's store is kept in {}\n",
                live.main.join(".git/sf-tasks/snapshots").display()
            )
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

    // Four snapshots in the same second (the clock is pinned): the fourth says the oldest went.
    live.home().stop().unwrap();
    let stamp = symphony_board::server::snapshot::stamp(NOW);
    for _ in 0..3 {
        assert_eq!(live.tasks(&live.wt, &["snapshot"]).code, 0);
    }
    let ran = live.tasks(&live.wt, &["snapshot"]);
    assert_eq!(
        (ran.code, ran.out),
        (
            0,
            format!(
                "✔ took a snapshot of the live board's store: {}; the newest 3 are kept, so {stamp} \
                 went\n",
                live.main
                    .join(".git/sf-tasks/snapshots")
                    .join(format!("{stamp}.3"))
                    .display()
            )
        ),
        "{}",
        ran.err
    );
}
