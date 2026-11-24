//! The refresh's parts that need no stand-in server: how a command it runs fails, what the server's
//! reload said, and how a board on PostgreSQL, or one still on SurrealDB, is judged. The writer
//! rule and the snapshots kept are the library's (`symphony::machine::writers`,
//! `symphony::machine::moving`). The whole refresh, on scratch boards and a stand-in server, is
//! `tests/release/refresh.rs`.

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
/// a release pins none, and its snapshot is `pg_dump`'s text.
#[test]
fn a_board_on_postgresql_is_judged_by_its_writers_alone_and_snapshotted_with_pg_dump() {
    use symphony_board::server::{Engine, postgres};
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
    let home = Home::in_common(&common);
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
    let against = against(&dir, &logs);
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
        "no writer behind"
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
    assert!(stopped.down, "its project starts it");
    assert!(pg.current().unwrap().is_none());
    fs::remove_dir_all(&dir).unwrap();
}

/// What a judge runs against in `dir`, its steps' logs in `logs`: no repository of the Symphony's
/// is read.
fn against<'a>(dir: &'a Path, logs: &'a Path) -> Against<'a> {
    Against {
        run: "2026-11-20T1000-1",
        main: dir,
        ours: dir,
        machine: dir,
        tag: "release-2026-11-20",
        logs,
        until: super::super::waits::far(),
    }
}

/// A registered project `alpha` under a tree of its own, its board's folder holding `held` alone:
/// the tree, the board's common dir and folder, its registry entry, and the folder for the steps'
/// logs.
fn alpha_holding(held: &str) -> (Tree, PathBuf, PathBuf, Entry, PathBuf) {
    let tree = Tree::new();
    let root = tree.path("alpha");
    let common = root.join(".git");
    let folder = common.join(symphony_board::server::FOLDER);
    fs::create_dir_all(folder.join(held)).unwrap();
    let entry: Entry = serde_json::from_value(serde_json::json!({
        "id": "alpha", "name": "alpha", "root": root, "pin": 1,
    }))
    .unwrap();
    let logs = tree.path("logs");
    fs::create_dir_all(&logs).unwrap();
    (tree, common, folder, entry, logs)
}

/// A board still on SurrealDB, as an earlier release kept it, is red, naming how it moves, and
/// nothing is read, made or started: no PostgreSQL is asked for it.
#[test]
fn a_board_still_on_surrealdb_is_red_naming_how_it_moves() {
    let (tree, common, folder, entry, logs) = alpha_holding("data");
    let dir = tree.path("");
    let judged = judge(
        &entry,
        &common,
        symphony_board::SCHEMA,
        &against(&dir, &logs),
    );
    assert!(
        matches!(&judged.verdict, Verdict::Red(why) if why.contains(symphony_board::server::GONE)),
        "{:?}",
        match &judged.verdict {
            Verdict::Red(why) | Verdict::Done(why) | Verdict::Theirs(why) => why.as_str(),
            Verdict::Move { .. } => "a move",
        }
    );
    assert!(
        !judged.down,
        "its database is not said down: it is on another engine"
    );
    assert!(!folder.join("postgresql").exists(), "nothing was made");
    assert!(!folder.join("backend.json").exists(), "nor named");
}

/// A board's folder its board was never made in, holding only what a session's heartbeat wrote
/// there, is a board not made yet: nothing to move, as before PostgreSQL made a new board, and
/// not a database down that its project should start. Nothing is made or started by the read.
#[test]
fn a_board_folder_with_no_board_made_in_it_is_nothing_to_move_and_not_down() {
    let (tree, common, _, entry, logs) = alpha_holding("keys");
    let dir = tree.path("");
    let judged = judge(
        &entry,
        &common,
        symphony_board::SCHEMA,
        &against(&dir, &logs),
    );
    let Verdict::Done(said) = &judged.verdict else {
        panic!("a board not made yet is judged done");
    };
    assert_eq!(said, "alpha: its board is not made yet: nothing to move");
    assert!(!judged.down);
    assert!(judged.home.unmade().unwrap(), "nothing was made");
    assert_eq!(tree.names("alpha/.git/symphony/tasks"), ["keys"]);
}

/// A release that keeps the schema current serves judges a board still on SurrealDB, as it does
/// one left behind, so that the first refresh to a build that refuses it says it red; a board not
/// made yet, and a project with no board folder, it passes over.
#[test]
fn a_release_at_currents_schema_judges_a_board_still_on_surrealdb_and_passes_over_none_made() {
    let (_surreal, common, ..) = alpha_holding("data");
    assert!(judged_again(&common, symphony_board::SCHEMA));
    let (_unmade, common, ..) = alpha_holding("keys");
    assert!(!judged_again(&common, symphony_board::SCHEMA));
    let (_none, common, folder, ..) = alpha_holding("keys");
    fs::remove_dir_all(&folder).unwrap();
    assert!(!judged_again(&common, symphony_board::SCHEMA));
}
