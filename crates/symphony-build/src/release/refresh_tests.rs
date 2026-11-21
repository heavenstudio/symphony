//! The refresh's parts that need no board or server: the SurrealDB a release pins, how a command
//! it runs fails and what the server's reload said. The writer rule, the snapshots kept and the
//! SurrealDB a move never replaces are the library's
//! (`symphony::machine::writers`, `symphony::machine::moving`). The whole refresh, on scratch
//! boards and a stand-in server, is `tests/release/refresh.rs`.

use super::*;
use crate::test_tree::Tree;

#[test]
fn a_release_at_currents_schema_or_behind_it_says_which_and_moves_only_a_board_left_behind() {
    assert_eq!(
        no_move("release-2", 19, 19),
        "release-2 serves board schema 19, as the release current names serves 19: only a board \
         left behind it moves"
    );
    assert_eq!(
        no_move("release-2", 18, 19),
        "release-2 serves board schema 18, behind the release current names, which serves 19: none \
         moves back, and only a board left behind 18 moves"
    );
}

#[test]
fn the_pinned_surreal_is_read_from_the_boards_source_at_the_commit() {
    let tree = Tree::new();
    let root = tree.path("repo");
    fs::create_dir_all(root.join("crates/tasks/src")).unwrap();
    fs::write(
        root.join("crates/tasks/src/server.rs"),
        "//! The server.\n\n/// Pinned.\npub const SURREAL_VERSION: &str = \"3.3.0\";\n",
    )
    .unwrap();
    let run = |args: &[&str]| {
        git(
            &root,
            &[
                &["-c", "user.name=t", "-c", "user.email=t@example.com"][..],
                args,
            ]
            .concat(),
        )
        .unwrap()
    };
    run(&["init", "-q", "-b", "main"]);
    run(&["add", "-A"]);
    run(&["commit", "-q", "-m", "first"]);
    assert_eq!(surreal_at(&root, "HEAD").as_deref(), Some("3.3.0"));
    fs::write(root.join("crates/tasks/src/server.rs"), "// none\n").unwrap();
    run(&["commit", "-q", "-am", "none"]);
    assert_eq!(surreal_at(&root, "HEAD"), None);
    assert_eq!(surreal_at(&root, "nowhere"), None);
}

#[test]
fn a_commands_failure_says_how_it_ended_and_its_last_lines_and_the_log_keeps_it_all() {
    let tree = Tree::new();
    let dir = tree.path("");
    let path = tree.path("logs/refresh-consumer.log");
    let mut log = Log::open(&path);
    let later = || Bound::until(Instant::now() + Duration::from_secs(3600));
    let failed = |log: &mut Log, words: &[&str]| log.run(&dir, words, later()).unwrap_err();
    assert_eq!(
        failed(&mut log, &["sh", "-c", "echo one; echo two >&2; exit 3"]),
        "`sh -c echo one; echo two >&2; exit 3` exited 3: two / one"
    );
    assert_eq!(
        failed(&mut log, &["sh", "-c", "kill -9 $$"]),
        "`sh -c kill -9 $$` was killed"
    );
    let why = failed(&mut log, &["no-such-program-here"]);
    assert!(why.contains("could not run in"), "{why}");
    assert_eq!(
        log.run(&dir, &["sh", "-c", "echo said"], later()).unwrap(),
        "said\n"
    );
    let late = log
        .run(&dir, &["sleep", "5"], Bound::until(Instant::now()))
        .unwrap_err();
    assert!(late.contains("did not end within"), "{late}");
    let kept = fs::read_to_string(&path).unwrap();
    assert!(
        kept.contains("$ sh -c echo said   (in ")
            && kept.contains("said\n")
            && kept.contains("exited 3: two / one"),
        "{kept}"
    );
}

#[test]
fn what_the_reload_said_is_its_last_line_since_current_moved_or_its_last_before_it() {
    let tree = Tree::new();
    let machine = tree.path("machine");
    fs::create_dir_all(&machine).unwrap();
    let log = machine.join(SERVER_LOG);
    // No log at all, as a server started in a terminal leaves none: nothing is said of its reload.
    assert_eq!(
        reload_said(&machine, 0),
        format!(
            "it keeps no log at {} to say why (a server started in a terminal says it there)",
            log.display()
        )
    );
    let none = format!(
        "the newest 256 KiB of its log ({}) hold no line of its reload, where one that follows \
         says each hand-off it begins: its reload may not be running, which a line starting `· \
         Symphony reload:` further back in that log, or none in it, tells",
        log.display()
    );
    let sweep = "[2026-11-18T10:58:50.851Z] · idle-session sweep of symphony ended after 7.261s\n";
    fs::write(&log, sweep).unwrap();
    assert_eq!(
        reload_said(&machine, 0),
        none,
        "a possibility, never a fact"
    );
    let failed = "[2026-11-18T13:13:02.000Z] · Symphony reload: Symphony server update failed; the \
                  previous server is still serving. The release release-2 did not take over: \
                  replacement did not become ready: it ended. It is tried again at about \
                  2026-11-18 14:18.";
    let attempt = "[2026-11-18T13:13:01.000Z] · Symphony reload: current names release-2; handing \
                   over to its server";
    let said = format!("{sweep}{attempt}\n{failed}\n{sweep}");
    fs::write(&log, &said).unwrap();
    // Since the move: its last line, the refusal and when it is tried again.
    let moved = sweep.len() as u64;
    assert_eq!(
        reload_said(&machine, moved),
        format!("its log ({}) says last: {failed}", log.display())
    );
    // A retry of the refresh while the reload waits to try the release again: `current` named it
    // already, so the log's length is taken after the refusal, and nothing follows it. The reload
    // is alive, and its last line says when it tries again.
    assert_eq!(
        reload_said(&machine, said.len() as u64),
        format!(
            "its reload wrote no line in its log ({}) since current moved, as one that waits to \
             try a release again writes none; its last line before: {failed}",
            log.display()
        )
    );
    // A log started again since, shorter than it was at the move, is all since the move.
    assert_eq!(
        reload_said(&machine, said.len() as u64 + 10_000),
        format!("its log ({}) says last: {failed}", log.display())
    );
    // Only its newest part is read: a line further back than that is not found, and said so.
    let sweeps = sweep.repeat(usize::try_from(LOG_READ).unwrap() / sweep.len() + 1);
    fs::write(&log, format!("{said}{sweeps}")).unwrap();
    assert_eq!(reload_said(&machine, moved), none);
    assert_eq!(
        reload_said(&machine, (said.len() + sweeps.len()) as u64),
        none
    );
}

/// A board on PostgreSQL is judged and snapshotted as it runs, by the machine's own PostgreSQL:
/// a release pins none, so its move waits on no SurrealDB version, and its snapshot is
/// `pg_dump`'s text.
#[test]
fn a_board_on_postgresql_is_judged_by_its_writers_alone_and_snapshotted_with_pg_dump() {
    use symphony_board::server::postgres;
    let dir = std::env::temp_dir().join(format!("refresh-pg-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let root = dir.join("alpha");
    let common = root.join(".git");
    fs::create_dir_all(&common).unwrap();
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let programs = postgres::discover_programs(&workspace).unwrap();
    fs::write(
        root.join("symphony.toml"),
        format!(
            "[project]\nid = \"alpha\"\n[board]\npostgresql = {:?}\n",
            programs.display().to_string()
        ),
    )
    .unwrap();
    let home = Home::in_common(&common, Some(Path::new("/no/surreal")));
    home.initialize_backend(Engine::PostgreSql).unwrap();
    let pg = home.project_postgresql(&root).unwrap();
    let serving = pg.ensure().unwrap();
    let store = symphony_board::Store::opening_on(
        symphony_board::pg::Client::new(&serving.socket, symphony_board::DATABASE).into(),
        Box::new(|| 1_800_000_000.0),
        Box::new(|_| true),
        "this-host",
        &symphony_board::Opening::write(),
    )
    .unwrap();
    store
        .create(&symphony_board::NewTask::new(
            "sim/kept",
            "A task the snapshot holds",
            "owner",
        ))
        .unwrap();
    let entry: Entry = serde_json::from_value(serde_json::json!({
        "id": "alpha", "name": "alpha", "root": root, "pin": 1,
    }))
    .unwrap();
    let logs = dir.join("logs");
    fs::create_dir_all(&logs).unwrap();
    let against = Against {
        run: "2026-11-20T1000-1",
        main: &dir,
        // No repository: the SurrealDB a release pins cannot be read, and is not asked for.
        root: &dir,
        ours: &dir,
        machine: &dir.join("machine"),
        tag: "release-2026-11-20",
        commit: "0000000",
        logs: &logs,
        surreal: None,
        until: super::super::waits::far(),
    };
    let schema = symphony_board::SCHEMA;
    let at = judge(&entry, &common, schema, &against);
    assert!(
        matches!(&at.verdict, Verdict::Done(said) if said.contains("nothing to move")),
        "a board at the release's schema"
    );
    // This test's own line among the writers is behind the next schema, and holds the move.
    let held = judge(&entry, &common, schema + 1, &against);
    assert!(matches!(&held.verdict, Verdict::Theirs(_)));
    store.raw().table("writer").delete().unwrap();
    let mut moving = judge(&entry, &common, schema + 1, &against);
    assert!(
        matches!(&moving.verdict, Verdict::Move { live, leave: false, .. } if *live == schema),
        "no writer behind, and no SurrealDB pin to wait on"
    );
    snapshot(&mut moving, &against).unwrap();
    let kept = common.join(format!(
        "symphony/tasks/snapshots/before-schema-{}-release-2026-11-20.sql",
        schema + 1
    ));
    assert_eq!(moving.snapshot.as_deref(), Some(kept.as_path()));
    let text = fs::read_to_string(&kept).unwrap();
    assert!(
        text.contains("CREATE TABLE") && text.contains("sim/kept"),
        "{text}"
    );
    assert_eq!(
        pg.current().unwrap().map(|s| s.pid),
        Some(serving.pid),
        "still serving"
    );
    // Stopped, it is red and nothing is started.
    pg.stop().unwrap();
    let stopped = judge(&entry, &common, schema + 1, &against);
    assert!(
        matches!(&stopped.verdict, Verdict::Red(why) if why.contains("is not running")),
        "a board whose PostgreSQL does not run"
    );
    assert!(pg.current().unwrap().is_none());
    fs::remove_dir_all(&dir).unwrap();
}
