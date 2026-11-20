//! The writer rule: which writers hold a move, which are a release's own, which are gone from their
//! paths, and what a judgement says of them.

use std::fs::{self, File};
use std::path::Path;
use std::time::{Duration, SystemTime};

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

/// A file at `path` last written `ago` seconds before `now`.
fn built(path: &Path, now: f64, ago: f64) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "").unwrap();
    let file = File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs_f64(now - ago))
        .unwrap();
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
    // A project's build still at its path, as it was when it last wrote.
    let project = &tree.path("alpha/.target/release/tasks");
    built(project, now, 10.0 * day);
    built(&releases.join("release-1/tasks"), now, 10.0 * day);
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
    let Lagging {
        own: mine,
        behind,
        gone,
    } = lagging(lines, 19, now, &tree.path("linked"));
    assert_eq!(gone, [], "each program is where it wrote from");
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
    built(&own, now, 10.0 * day);
    let found = lagging(
        vec![writer("tasks", &own, &here, 18, now, 60.0)],
        19,
        now,
        &tree.path("empty"),
    );
    assert!(found.own.is_empty() && found.behind.len() == 1 && found.gone.is_empty());
}

#[test]
fn a_writer_whose_program_is_gone_from_its_path_on_this_host_is_not_counted() {
    let tree = Tree::new();
    let here = symphony_board::sys::hostname();
    let now = 1_000_000.0;
    let hour = symphony_board::WRITERS_EVERY;
    // Seen two hours ago: nothing there, nothing under a file, a build put there since the line's
    // last possible write, one put there within its hour, and the build that wrote.
    let seen = 2.0 * hour;
    let missing = tree.path("bin/tasks-0a1b");
    let under_a_file = tree.path("bin/kept/deeper/tasks");
    built(&tree.path("bin/kept"), now, 10.0 * hour);
    let rebuilt = tree.path("rs-bin/tasks");
    built(&rebuilt, now, 60.0);
    let within = tree.path("rs-bin/agent-send");
    built(&within, now, seen - hour);
    let standing = tree.path("rs-bin/gate");
    built(&standing, now, 10.0 * hour);
    let lines = vec![
        writer("tasks-0a1b", &missing, &here, 18, now, seen),
        writer("tasks", &under_a_file, &here, 18, now, seen),
        writer("tasks", &rebuilt, &here, 18, now, seen),
        writer("agent-send", &within, &here, 18, now, seen),
        writer("gate", &standing, &here, 18, now, seen),
        // Another host's paths are never looked at here, gone from this machine or not.
        writer("tasks-0a1b", &missing, "another-host", 18, now, seen),
        writer("tasks", &rebuilt, "another-host", 18, now, seen),
    ];
    let found = lagging(lines, 19, now, &tree.path("machine"));
    let named = |writer: &Writer| (writer.path.clone(), writer.host.clone());
    let path = |path: &Path| path.to_string_lossy().into_owned();
    assert!(found.own.is_empty());
    assert_eq!(
        (found.gone.iter())
            .map(|(writer, why)| (writer.path.clone(), *why))
            .collect::<Vec<_>>(),
        [
            (path(&under_a_file), Gone::Missing),
            (path(&rebuilt), Gone::Replaced),
            (path(&missing), Gone::Missing),
        ],
        "by program, host and path"
    );
    assert_eq!(
        found.behind.iter().map(named).collect::<Vec<_>>(),
        [
            (path(&within), here.clone()),
            (path(&standing), here.clone()),
            (path(&rebuilt), "another-host".to_string()),
            (path(&missing), "another-host".to_string()),
        ]
    );
    let said = not_counted(&found.gone, now);
    assert!(
        said.starts_with("; not counted, as its program is gone: tasks at schema 18 (")
            && said.contains(&format!("{} on {here}, last seen ", path(&missing)))
            && said.contains(" ago): nothing is at its path now; ")
            && said.contains(
                " ago): the file at its path was written after it last ran there, a build that \
                 has not opened this board"
            ),
        "{said}"
    );
    assert_eq!(not_counted(&[], now), "");
}

/// A path that cannot be read is not gone: the writer there still holds.
#[test]
fn a_path_that_cannot_be_read_still_holds() {
    use std::os::unix::fs::PermissionsExt;
    let tree = Tree::new();
    let closed = tree.path("closed");
    let path = closed.join("tasks");
    built(&path, 1_000_000.0, 60.0);
    fs::set_permissions(&closed, fs::Permissions::from_mode(0o000)).unwrap();
    let line = writer("tasks", &path, "here", 18, 1_000_000.0, 7200.0);
    // The superuser reads every folder: only where this one is barred does the case stand.
    let barred = fs::metadata(&path).is_err();
    let read = gone(&line);
    fs::set_permissions(&closed, fs::Permissions::from_mode(0o755)).unwrap();
    if barred {
        assert_eq!(read, None);
    }
    // The line's hour is its own: a file written exactly at its end is not past it.
    built(&path, 1_000_000.0, 3600.0);
    assert_eq!(gone(&line), None);
    built(&path, 1_000_000.0, 3599.0);
    assert_eq!(gone(&line), Some(Gone::Replaced));
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
fn a_judgement_read_later_says_the_same_steadily_while_the_same_writers_hold() {
    let now = 1_000_000.0;
    let line = |ago| writer("tasks", Path::new("/w/tasks"), "h", 18, now, ago);
    let early = format!(
        "held: {}; {}; rebuild",
        said(&line(60.0), now),
        said(&line(7200.0), now)
    );
    let later = format!(
        "held: {}; {}; rebuild",
        said(&line(400.0), now),
        said(&line(9000.0), now)
    );
    assert_ne!(early, later, "each says how long ago");
    assert_eq!(steady(&early), steady(&later));
    assert_eq!(
        steady(&early),
        "held: tasks at schema 18 (/w/tasks on h); tasks at schema 18 (/w/tasks on h); rebuild"
    );
    // Another writer, or another schema, is another judgement.
    let other = format!(
        "held: {}; rebuild",
        said(&writer("gate", Path::new("/w/g"), "h", 18, now, 60.0), now)
    );
    assert_ne!(steady(&early), steady(&other));
    // Words with no time in them stay as they are, a stray opening too.
    assert_eq!(
        steady("its database is not running"),
        "its database is not running"
    );
    assert_eq!(steady("x, last seen never"), "x, last seen never");
}

#[test]
fn a_board_from_before_the_writer_record_is_its_projects_own_migrates_to_bring_up() {
    let said = before_the_record(17, Path::new("/work/alpha"));
    assert_eq!(
        said,
        format!(
            "its board is at schema 17, from before the board recorded who writes it (schema {}), \
             so it is never moved here: `cd /work/alpha && just tasks migrate`, its own, brings it \
             up",
            symphony_board::WRITERS_SINCE
        )
    );
    assert_eq!(steady(&said), said, "it names no writer's time");
}
