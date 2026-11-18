//! The refresh's parts that need no board or server: which writers hold a move, what the release's
//! own binaries are, the snapshots kept, the SurrealDB a release pins and how a command it runs
//! fails. The whole refresh, on scratch boards and a stand-in server, is `tests/release/refresh.rs`.

use super::*;
use crate::test_tree::Tree;

/// A writer's line, `program` at `path` on `host`, at `schema`, seen `ago` seconds before `now`.
fn writer(program: &str, path: &Path, host: &str, schema: i64, now: f64, ago: f64) -> Writer {
    Writer {
        program: program.into(),
        path: path.to_string_lossy().into_owned(),
        host: host.into(),
        schema,
        seen: now - ago,
    }
}

#[test]
fn a_writer_behind_holds_the_move_unless_it_is_a_release_of_this_machine_or_unseen_for_a_week() {
    let tree = Tree::new();
    fs::create_dir_all(tree.path("machine/releases/release-1/bin")).unwrap();
    // Named through a link, as a machine folder may be: its releases are read by their real path.
    std::os::unix::fs::symlink(tree.path("machine"), tree.path("linked")).unwrap();
    let releases = tree.path("machine/releases").canonicalize().unwrap();
    let here = symphony_board::sys::hostname();
    let now = 1_000_000.0;
    let day = 24.0 * 3600.0;
    let own = releases.join("release-1/bin/tasks");
    let project = Path::new("/work/alpha/.target/release/tasks");
    let lines = vec![
        writer("tasks", &own, &here, 18, now, 60.0),
        writer(
            "agent-send",
            &releases.join("release-0/bin/agent-send"),
            &here,
            17,
            now,
            day,
        ),
        writer("tasks", &own, "another-host", 18, now, 60.0),
        writer("tasks", project, &here, 18, now, 6.0 * day),
        writer(
            "tasks",
            &releases.join("release-1/tasks"),
            &here,
            18,
            now,
            60.0,
        ),
        writer("gate", Path::new("/work/alpha/gate"), &here, 19, now, 60.0),
        writer(
            "old",
            Path::new("/work/beta/old"),
            &here,
            18,
            now,
            8.0 * day,
        ),
    ];
    let (mine, behind) = lagging(lines, 19, now, &tree.path("linked"));
    let paths = |writers: &[Writer]| -> Vec<(String, String)> {
        (writers.iter())
            .map(|w| (w.path.clone(), w.host.clone()))
            .collect()
    };
    assert_eq!(
        paths(&mine),
        [
            (own.to_string_lossy().into_owned(), here.clone()),
            (
                releases
                    .join("release-0/bin/agent-send")
                    .to_string_lossy()
                    .into_owned(),
                here.clone()
            ),
        ],
        "a release's own binaries on this host, whatever release they are of"
    );
    // Another host's copy of a release, a project's build, and a file in a release's folder that is
    // no binary of it, in order of program, host and path. At the release's schema, or unseen for
    // over a week, a writer is not behind.
    let mut expected = vec![
        (
            own.to_string_lossy().into_owned(),
            "another-host".to_string(),
        ),
        (project.to_string_lossy().into_owned(), here.clone()),
        (
            releases
                .join("release-1/tasks")
                .to_string_lossy()
                .into_owned(),
            here.clone(),
        ),
    ];
    expected.sort_by(|a, b| (&a.1, &a.0).cmp(&(&b.1, &b.0)));
    assert_eq!(paths(&behind), expected);
    // A machine folder with no release installed has no release's own.
    let (mine, behind) = lagging(
        vec![writer("tasks", &own, &here, 18, now, 60.0)],
        19,
        now,
        &tree.path("empty"),
    );
    assert!(mine.is_empty() && behind.len() == 1);
}

#[test]
fn a_release_binary_is_one_in_its_bin_folder_and_nothing_deeper_or_beside_it() {
    let releases = Path::new("/m/releases");
    assert!(of_a_release(Path::new("/m/releases/r/bin/tasks"), releases));
    assert!(!of_a_release(
        Path::new("/m/releases/r/bin/x/tasks"),
        releases
    ));
    assert!(!of_a_release(
        Path::new("/m/releases/r/lib/tasks"),
        releases
    ));
    assert!(!of_a_release(Path::new("/m/releases/bin"), releases));
    assert!(!of_a_release(Path::new("/elsewhere/r/bin/tasks"), releases));
}

#[test]
fn what_holds_the_release_is_a_lagging_writer_or_the_refreshs_own_red_and_never_an_old_board() {
    let theirs = |holds| Verdict::Theirs {
        why: String::new(),
        holds,
    };
    assert!(theirs(true).holds());
    assert!(!theirs(false).holds(), "a board below the writer record");
    assert!(Verdict::Red(String::new()).holds());
    assert!(!Verdict::Done(String::new()).holds());
    assert!(
        !Verdict::Move {
            live: 18,
            leave: true
        }
        .holds()
    );
}

#[test]
fn a_release_at_currents_schema_or_behind_it_says_which_and_moves_no_board() {
    assert_eq!(
        no_move("release-2", 19, 19),
        "release-2 serves board schema 19, as the release current names serves 19: no board moves"
    );
    assert_eq!(
        no_move("release-2", 18, 19),
        "release-2 serves board schema 18, behind the release current names, which serves 19: no \
         board moves, as none moves back"
    );
}

#[test]
fn a_running_database_older_than_the_pins_is_one_the_move_would_replace() {
    assert!(older("3.2.9", "3.3.0"));
    assert!(older("v2.3.0", "3.0.0"));
    assert!(older("3.3", "3.3.1"));
    assert!(!older("3.3.0", "3.3.0"));
    assert!(!older("3.4.0", "3.3.9"));
    assert!(!older("3.3.0-beta", "3.3.0"), "a suffix is no older");
}

#[test]
fn the_snapshots_kept_are_the_newest_and_nothing_else_is_touched() {
    let tree = Tree::new();
    let folder = tree.path("snapshots");
    fs::create_dir_all(&folder).unwrap();
    for n in 0..7 {
        let file = folder.join(format!("before-schema-{n}-release.surql"));
        fs::write(&file, "x").unwrap();
        let when = UNIX_EPOCH + Duration::from_secs(1_000 + n);
        File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(when)
            .unwrap();
    }
    fs::write(folder.join("someone-else.surql"), "y").unwrap();
    prune(&folder, 5);
    let mut left = tree.names("snapshots");
    left.sort();
    assert_eq!(
        left,
        [
            "before-schema-2-release.surql",
            "before-schema-3-release.surql",
            "before-schema-4-release.surql",
            "before-schema-5-release.surql",
            "before-schema-6-release.surql",
            "someone-else.surql",
        ]
    );
    prune(&tree.path("none"), 5);
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
fn what_the_reload_said_is_its_last_line_since_current_moved_or_that_it_said_nothing() {
    let tree = Tree::new();
    let machine = tree.path("machine");
    fs::create_dir_all(&machine).unwrap();
    let log = machine.join(SERVER_LOG);
    let nothing = format!(
        "its log ({}) has no line of its reload since current moved, where a reload that follows \
         says each hand-off it begins: its reload is not running, and only a stop and a start of \
         the server, the owner's word, takes the release",
        log.display()
    );
    // No log at all, as a server started in a terminal leaves none.
    assert_eq!(reload_said(&machine, 0), nothing);
    let before = "[2026-11-18T10:00:00.000Z] · Symphony reload: current names release-1; handing \
                  over to its server\n[2026-11-18T10:58:50.851Z] · idle-session sweep of symphony \
                  ended after 7.261s\n";
    fs::write(&log, before).unwrap();
    let moved = before.len() as u64;
    // What it said before the move is not its answer to it.
    assert_eq!(reload_said(&machine, moved), nothing);
    let after = "[2026-11-18T13:13:01.000Z] · Symphony reload: current names release-2; handing \
                 over to its server\n[2026-11-18T13:13:02.000Z] · Symphony reload: Symphony server \
                 update failed; the previous server is still serving. The release release-2 did \
                 not take over: replacement did not become ready: it ended. It is tried again at \
                 about 2026-11-18 14:18.\n[2026-11-18T13:13:43.000Z] · idle-session sweep of \
                 symphony started\n";
    fs::write(&log, format!("{before}{after}")).unwrap();
    assert_eq!(
        reload_said(&machine, moved),
        format!(
            "its log ({}) says last: [2026-11-18T13:13:02.000Z] · Symphony reload: Symphony server \
             update failed; the previous server is still serving. The release release-2 did not \
             take over: replacement did not become ready: it ended. It is tried again at about \
             2026-11-18 14:18.",
            log.display()
        )
    );
    // A log started again since, shorter than it was at the move, is read whole.
    assert!(
        reload_said(&machine, moved + 10_000).contains("says last: [2026-11-18T13:13:02.000Z]")
    );
    // Only its newest part is read: a line further back than that is not found.
    let sweeps = "[2026-11-18T13:18:43.000Z] · idle-session sweep of symphony started\n"
        .repeat(usize::try_from(LOG_READ).unwrap() / 64);
    fs::write(&log, format!("{before}{after}{sweeps}")).unwrap();
    assert_eq!(reload_said(&machine, moved), nothing);
}
