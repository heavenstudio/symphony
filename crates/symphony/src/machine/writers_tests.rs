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
    // The project's build is the one it runs now, so it holds however long ago it last wrote.
    let runs = Runs {
        builds: vec![project.canonicalize().unwrap()],
        hours: UNUSED_HOURS,
    };
    let Lagging {
        own: mine,
        behind,
        gone,
        unused,
        ..
    } = lagging(lines, 19, now, &tree.path("linked"), &runs);
    assert_eq!(unused, [], "{unused:?}");
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
        &runs,
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
    let found = lagging(
        lines,
        19,
        now,
        &tree.path("machine"),
        &Runs::of(&tree.path("alpha")),
    );
    assert!(found.unused.is_empty() && !found.linked);
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
fn a_held_board_names_its_writers_what_its_project_does_and_what_moves_it_then() {
    let now = 1_000_000.0;
    let gate = writer("gate", Path::new("/w/gate"), "h", 18, now, 60.0);
    let tasks = writer("tasks", Path::new("/w/tasks"), "h", 17, now, 7200.0);
    let one = holding(18, std::slice::from_ref(&gate), "release-1", 19, "", now);
    assert_eq!(
        steady(&one),
        "its board stays at schema 18: a writer seen there in the last week is behind \
         release-1's schema 19: gate at schema 18 (/w/gate on h); rebuild each at schema 19 and \
         run it once, which records it, then the Symphony moves it"
    );
    // Two writers, and one not counted: its words come after what moves the board.
    let gone = [(
        writer("old", Path::new("/w/old"), "h", 17, now, 60.0),
        Gone::Missing,
    )];
    let two = holding(
        18,
        &[gate, tasks],
        "release-1",
        19,
        &not_counted(&gone, now),
        now,
    );
    assert!(
        steady(&two).starts_with(
            "its board stays at schema 18: 2 writers seen there in the last week are behind \
             release-1's schema 19: gate at schema 18 (/w/gate on h); tasks at schema 17 \
             (/w/tasks on h); rebuild each"
        ),
        "{two}"
    );
    assert!(
        two.ends_with(&format!("moves it{}", not_counted(&gone, now))),
        "{two}"
    );
    // A project reads it on its own board: it names none of the Symphony's own commands, and
    // promises no server's round, as which of a refresh and the machine's server moves the board
    // is theirs to say.
    for said in [&one, &two] {
        assert!(
            !said.contains("just ") && !said.contains("server"),
            "{said}"
        );
    }
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

/// The project's main checkout `root` under `tree`, its link `.target/bin/NAME` naming `build`.
fn linked(root: &Path, name: &str, build: &Path) {
    let links = root.join(symphony_board::builds::LINKS);
    fs::create_dir_all(&links).unwrap();
    std::os::unix::fs::symlink(build, links.join(name)).unwrap();
}

/// A build its project no longer runs is not counted once it has not run for longer than the
/// project's bound; the build a link names holds however long ago it ran, and so does one on
/// another host, whose links are not this machine's to read. A program gone from its path is gone
/// first.
#[test]
fn a_build_its_project_no_longer_runs_unseen_past_its_bound_is_not_counted_and_its_own_build_holds()
{
    let tree = Tree::new();
    let here = symphony_board::sys::hostname();
    let now = 1_000_000.0;
    let hour = 3600.0;
    let root = tree.path("alpha");
    let new = tree.path("alpha/.git/cargo/bin/tasks-new");
    let old = tree.path("alpha/.git/cargo/bin/tasks-old");
    let recent = tree.path("alpha/.git/cargo/bin/agent-send-old");
    let edge = tree.path("alpha/.git/cargo/bin/gate-old");
    for build in [&new, &old, &recent, &edge] {
        built(build, now, 90.0 * hour);
    }
    linked(&root, "tasks", &new);
    fs::create_dir_all(tree.path("machine/releases")).unwrap();
    let releases = tree.path("machine/releases").canonicalize().unwrap();
    let own = releases.join("release-1/bin/tasks");
    built(&own, now, 90.0 * hour);
    let missing = tree.path("alpha/.git/cargo/bin/tasks-gone");
    let lines = vec![
        writer("tasks-new", &new, &here, 18, now, 72.0 * hour),
        writer("tasks-old", &old, &here, 18, now, 25.0 * hour),
        writer("agent-send-old", &recent, &here, 18, now, 23.0 * hour),
        // Exactly at the bound is not past it.
        writer("gate-old", &edge, &here, 18, now, 24.0 * hour),
        writer("tasks-old", &old, "another-host", 18, now, 72.0 * hour),
        writer("tasks", &own, &here, 18, now, 72.0 * hour),
        writer("tasks-gone", &missing, &here, 18, now, 72.0 * hour),
    ];
    let runs = Runs::of(&root);
    assert_eq!(runs.builds, [new.canonicalize().unwrap()]);
    assert_eq!(runs.hours, UNUSED_HOURS);
    let found = lagging(lines, 19, now, &tree.path("machine"), &runs);
    let path = |path: &Path| path.to_string_lossy().into_owned();
    let named = |writers: &[Writer]| -> Vec<(String, String)> {
        (writers.iter())
            .map(|w| (w.path.clone(), w.host.clone()))
            .collect()
    };
    assert_eq!(named(&found.own), [(path(&own), here.clone())]);
    assert_eq!(
        (found.gone.iter())
            .map(|(w, why)| (w.path.clone(), *why))
            .collect::<Vec<_>>(),
        [(path(&missing), Gone::Missing)]
    );
    assert_eq!(named(&found.unused), [(path(&old), here.clone())]);
    assert_eq!(
        named(&found.behind),
        [
            (path(&recent), here.clone()),
            (path(&edge), here.clone()),
            (path(&new), here.clone()),
            (path(&old), "another-host".to_string()),
        ],
        "by program, host and path"
    );
    assert!(found.linked, "the build its link names is among them");
    assert!(found.leaves());
    let said = found.not_counted(now);
    assert!(
        said.starts_with("; not counted, as its program is gone: tasks-gone at schema 18 (")
            && said.contains(&format!(
                "; not counted, as its project runs another build now and it has not run here \
                 for over 24 h ([board] stale_writer_hours): tasks-old at schema 18 ({} on \
                 {here}, last seen 25 h ago)",
                path(&old)
            )),
        "{said}"
    );
    let holding = found.holding(18, "release-1", 19, now);
    assert!(
        holding.starts_with("its board stays at schema 18: 4 writers seen there in the last week")
            && holding.contains(&said)
            && holding.ends_with(OWN_BUILD),
        "{holding}"
    );
    assert_eq!(
        steady(&holding),
        steady(&found.holding(18, "release-1", 19, now + 60.0))
    );

    // Its link moved on to another build: the one it named before holds no more once unused.
    let other = tree.path("beta");
    linked(&other, "tasks", &recent);
    let found = lagging(
        vec![
            writer("tasks-new", &new, &here, 18, now, 72.0 * hour),
            writer("agent-send-old", &recent, &here, 18, now, 72.0 * hour),
        ],
        19,
        now,
        &tree.path("machine"),
        &Runs::of(&other),
    );
    assert_eq!(named(&found.unused), [(path(&new), here.clone())]);
    assert_eq!(named(&found.behind), [(path(&recent), here.clone())]);
    assert!(found.linked);
}

/// A board held by builds its project does not run says nothing of its own build, and a judgement
/// with nothing left uncounted leaves nobody behind.
#[test]
fn a_hold_by_no_build_the_project_runs_names_no_link_and_nothing_uncounted_leaves_nobody() {
    let tree = Tree::new();
    let here = symphony_board::sys::hostname();
    let now = 1_000_000.0;
    let build = tree.path("builds/tasks");
    built(&build, now, 10.0);
    let found = lagging(
        vec![writer("tasks", &build, &here, 18, now, 60.0)],
        19,
        now,
        &tree.path("machine"),
        &Runs::of(&tree.path("alpha")),
    );
    assert!(!found.linked && !found.leaves());
    assert_eq!(found.not_counted(now), "");
    let holding = found.holding(18, "release-1", 19, now);
    assert!(!holding.contains(OWN_BUILD), "{holding}");
    assert!(
        holding.ends_with(", then the Symphony moves it"),
        "{holding}"
    );
    // A project reads it on its own board: it names none of the Symphony's own commands.
    assert!(!holding.contains("just "), "{holding}");
}

/// The bound is the project's own `[board] stale_writer_hours`, a whole number of hours over 0;
/// anything else, or no file, is a day, and a link that leads nowhere names no build.
#[test]
fn the_bound_is_the_projects_own_and_anything_but_whole_hours_over_0_is_a_day() {
    let tree = Tree::new();
    let root = tree.path("alpha");
    assert_eq!(
        Runs::of(&root),
        Runs {
            builds: Vec::new(),
            hours: UNUSED_HOURS
        }
    );
    fs::create_dir_all(&root).unwrap();
    for (text, hours) in [
        ("[board]\nstale_writer_hours = 48\n", 48),
        ("[board]\nstale_writer_hours = 1\n", 1),
        ("[board]\nstale_writer_hours = 0\n", UNUSED_HOURS),
        ("[board]\nstale_writer_hours = -3\n", UNUSED_HOURS),
        ("[board]\nstale_writer_hours = 1.5\n", UNUSED_HOURS),
        ("[board]\nstale_writer_hours = \"48\"\n", UNUSED_HOURS),
        ("[board]\nstale_writer_hours = 99999999999\n", UNUSED_HOURS),
        ("[board]\nguide = \"TASKS.md\"\n", UNUSED_HOURS),
        ("stale_writer_hours = 48\n", UNUSED_HOURS),
        ("not toml [", UNUSED_HOURS),
    ] {
        fs::write(root.join("symphony.toml"), text).unwrap();
        assert_eq!(Runs::of(&root).hours, hours, "{text}");
    }
    // A link to nothing names nothing; one to a build names the file it leads to.
    linked(&root, "agent-send", &tree.path("nowhere"));
    let build = tree.path("builds/tasks");
    built(&build, 1_000_000.0, 10.0);
    linked(&root, "tasks", &build);
    assert_eq!(Runs::of(&root).builds, [build.canonicalize().unwrap()]);
    // A bound of 48 hours keeps a build unseen for 25 hours counted.
    fs::write(
        root.join("symphony.toml"),
        "[board]\nstale_writer_hours = 48\n",
    )
    .unwrap();
    let other = tree.path("builds/tasks-old");
    built(&other, 1_000_000.0, 30.0 * 3600.0);
    let here = symphony_board::sys::hostname();
    let found = lagging(
        vec![writer(
            "tasks-old",
            &other,
            &here,
            18,
            1_000_000.0,
            25.0 * 3600.0,
        )],
        19,
        1_000_000.0,
        &tree.path("machine"),
        &Runs::of(&root),
    );
    assert!(
        found.unused.is_empty() && found.behind.len() == 1,
        "{found:?}"
    );
}
