//! A land stopped by a signal, or killed outright: what it ends, what it leaves, and the land after
//! it. The land runs as the `tasks` binary, in a process group of its own as a terminal's
//! foreground job is, its children in groups of theirs, which a terminal's Ctrl-C never reaches.

use super::*;
use std::os::unix::process::{CommandExt as _, ExitStatusExt as _};
use std::process::{Child, Command, ExitStatus, Stdio};
use symphony_board::sys::{Spawning as _, pid_alive};

/// The land of `SLUG` as `actor`, the `tasks` binary run from the main checkout as `just land`
/// runs it, leading a process group of its own; what it prints goes to `land.out` and `land.err`
/// beside the repository.
fn started(r: &Repo, actor: &str) -> Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tasks"));
    command
        .args(["land", SLUG, "--as", actor])
        .env_clear()
        .envs(r.vars())
        .env(RECIPE, "1")
        .current_dir(&r.root)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(fs::File::create(r.dir.join("land.out")).unwrap())
        .stderr(fs::File::create(r.dir.join("land.err")).unwrap());
    command.spawn_locked().unwrap()
}

/// `signal` to the process `pid`, or to its whole group when `group`.
fn send(pid: u32, signal: i32, group: bool) {
    let pid = i32::try_from(pid).unwrap();
    let target = if group { -pid } else { pid };
    // SAFETY: a signal to a process, or the group, this test started.
    assert_eq!(unsafe { libc::kill(target, signal) }, 0, "kill {target}");
}

/// The land's end, within a minute.
fn ended(land: &mut Child) -> ExitStatus {
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(status) = land.try_wait().unwrap() {
            return status;
        }
        assert!(Instant::now() < until, "the land never ended");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The pid a script wrote to `path`, once it has.
fn pid_in(path: &Path) -> i64 {
    wait_for(|| {
        fs::read_to_string(path)
            .ok()
            .is_some_and(|text| text.trim().parse::<i64>().is_ok())
    });
    fs::read_to_string(path).unwrap().trim().parse().unwrap()
}

/// Whether process `pid` is gone within ten seconds (once ended, its new parent reaps it).
fn gone(pid: i64) -> bool {
    let until = Instant::now() + Duration::from_secs(10);
    while pid_alive(pid) {
        if Instant::now() > until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
}

/// The land's worktree and what names its group, left behind or not.
fn left(r: &Repo) -> (bool, bool) {
    let folder = r.root.join(".git/symphony/tasks/land");
    (
        folder.join("worktree").exists()
            || git(&r.root, &["worktree", "list"]).contains("symphony/tasks"),
        folder.join(symphony_board::land::RUNNING).exists(),
    )
}

/// What the land said on stderr, its last line.
fn refused_with(r: &Repo) -> String {
    let err = fs::read_to_string(r.dir.join("land.err")).unwrap();
    err.trim_end()
        .lines()
        .last()
        .unwrap_or_default()
        .to_string()
}

/// A check that hangs while `hang` beside the repository is there, its pid in `check.pid` and
/// the pid of a child it left in the background in `bg.pid`.
fn hanging_check(r: &Repo, more: &str) {
    let dir = r.dir.display();
    let check = format!(
        "test -e '{dir}/hang' || exit 0; echo $$ > '{dir}/check.pid'; sleep 60 & echo $! > \
         '{dir}/bg.pid'; wait"
    );
    checked_with(r, &check, more);
    write(&r.dir, "hang", "");
}

#[test]
fn an_interrupt_to_the_land_ends_its_hung_check_whole_and_removes_its_worktree() {
    let r = Repo::new();
    hanging_check(&r, "");
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let before = head(&r.root, "main");
    let thread = r.board.said(SLUG);
    let mut land = started(&r, &people.orch);
    let check = pid_in(&r.dir.join("check.pid"));
    let background = pid_in(&r.dir.join("bg.pid"));
    // A terminal's Ctrl-C: the land's group, never the check's.
    send(land.id(), libc::SIGINT, true);
    let status = ended(&mut land);
    assert_eq!(status.signal(), Some(libc::SIGINT), "it ends by the signal");
    assert_eq!(
        refused_with(&r),
        "✖ interrupted by SIGINT: nothing landed, main where it was"
    );
    assert!(gone(check), "the check {check} was ended");
    assert!(
        gone(background),
        "what the check left running {background} was ended"
    );
    assert_eq!(
        left(&r),
        (false, false),
        "its worktree removed, nothing named"
    );
    assert_eq!(head(&r.root, "main"), before);
    assert_eq!(r.board.said(SLUG), thread, "nothing said or recorded");
    let log = fs::read_to_string(r.root.join(".git/symphony/tasks/land/sim-jump.log")).unwrap();
    assert!(log.ends_with("ended: the land took SIGINT\n"), "{log}");
    // Its lock and its folder free, the next land lands.
    fs::remove_file(r.dir.join("hang")).unwrap();
    landed(&r, SLUG, &people.orch).1.unwrap();
    assert_eq!(head(&r.root, "main^1"), before);
}

#[test]
fn a_terminate_to_the_land_alone_ends_its_hung_generator() {
    let r = Repo::new();
    let pid = r.dir.join("generator.pid");
    let generator = format!("echo $$ > '{}'; exec sleep 60", pid.display());
    manifest(
        &r,
        &format!(
            "\n[land]\ngenerators = [\n  {{ name = \"the slow one\", outputs = \"gen/\", command \
             = [\"sh\", \"-c\", {}] }},\n]\n",
            toml_string(&generator)
        ),
    );
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let before = head(&r.root, "main");
    let mut land = started(&r, &people.orch);
    let generating = pid_in(&pid);
    // `kill PID`, as a supervisor ends it: the land alone.
    send(land.id(), libc::SIGTERM, false);
    assert_eq!(ended(&mut land).signal(), Some(libc::SIGTERM));
    assert_eq!(
        refused_with(&r),
        "✖ interrupted by SIGTERM: nothing landed, main where it was"
    );
    assert!(gone(generating), "the generator {generating} was ended");
    assert_eq!(left(&r), (false, false));
    assert_eq!(head(&r.root, "main"), before);
    assert!(runs(&r).is_empty(), "no check ran");
}

#[test]
fn a_land_killed_outright_leaves_its_check_and_the_next_refuses_while_anything_works_there() {
    let r = Repo::new();
    hanging_check(&r, "");
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let before = head(&r.root, "main");
    let mut land = started(&r, &people.orch);
    let check = pid_in(&r.dir.join("check.pid"));
    pid_in(&r.dir.join("bg.pid"));
    // Killed outright, the land ends nothing: its check runs on in its group, in the worktree.
    send(land.id(), libc::SIGKILL, false);
    assert_eq!(ended(&mut land).signal(), Some(libc::SIGKILL));
    assert!(pid_alive(check));
    assert_eq!(left(&r), (true, true));
    let tree = r.root.join(".git/symphony/tasks/land/worktree");
    let held = head(&tree, "HEAD");
    // The next land refuses beside it, naming the group, and rebuilds nothing.
    let (_, result) = landed(&r, SLUG, &people.orch);
    let dir = r.dir.display();
    let line = format!(
        "the process group {check} an earlier land started (`sh -c test -e '{dir}/hang' || exit \
         0; echo $$ > '{dir}/check.pid'; sleep 60 & echo $! > '{dir}/bg.pid'; wait`) outlived it \
         and still runs: end it (`kill -TERM -{check}`), then run the land again"
    );
    assert_eq!(result.as_ref().map_err(String::as_str), Err(line.as_str()));
    assert_eq!(
        head(&tree, "HEAD"),
        held,
        "the worktree is as the check has it"
    );
    assert_eq!(head(&r.root, "main"), before);
    // Its group ended, a process still working in the worktree keeps it too.
    send(u32::try_from(check).unwrap(), libc::SIGKILL, true);
    assert!(gone(check));
    let working = crate::HeldCwd::at(&tree);
    let (_, result) = landed(&r, SLUG, &people.orch);
    let line = format!(
        "a process still works in {}, the worktree an earlier land left: end it, then run the \
         land again",
        tree.display()
    );
    assert_eq!(result.as_ref().map_err(String::as_str), Err(line.as_str()));
    assert!(tree.is_dir());
    // With nothing left there, the land replaces it and lands.
    drop(working);
    fs::remove_file(r.dir.join("hang")).unwrap();
    landed(&r, SLUG, &people.orch).1.unwrap();
    assert_eq!(head(&r.root, "main^1"), before);
    assert_eq!(left(&r), (false, false));
}

#[test]
fn a_land_stopped_after_main_moved_is_finished_by_a_run_again_pushing_what_it_checked() {
    let r = Repo::new();
    manifest(&r, "");
    let bare = remote(&r);
    let pushed = head(&bare, "main");
    // An origin whose push never answers, until the test says.
    let pushing = r.dir.join("push.pid");
    write(
        &r.dir,
        "receive-pack",
        &format!(
            "#!/bin/sh\necho $$ > '{}'\nexec sleep 60\n",
            pushing.display()
        ),
    );
    let receive = r.dir.join("receive-pack");
    let chmod = Command::new("chmod")
        .arg("+x")
        .arg(&receive)
        .status_locked()
        .unwrap();
    assert!(chmod.success());
    git(
        &r.root,
        &[
            "config",
            "remote.origin.receivepack",
            &receive.to_string_lossy(),
        ],
    );
    let wt = branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let tip = head(&wt, "HEAD");
    let people = approved(&r, SLUG, &format!("{tip}: good"));
    let mut land = started(&r, &people.orch);
    let push = pid_in(&pushing);
    // A hang-up while it pushes: main holds the merge already.
    send(land.id(), libc::SIGHUP, true);
    assert_eq!(ended(&mut land).signal(), Some(libc::SIGHUP));
    assert!(gone(push), "the push {push} was ended");
    let merge = head(&r.root, "main");
    assert_eq!(head(&r.root, "main^2"), tip);
    assert_eq!(
        refused_with(&r),
        format!(
            "✖ interrupted by SIGHUP after main moved to the merge of {SLUG}, {}: run `just land \
             {SLUG}` again to finish its push, its landed line and its sweep",
            &merge[..7]
        )
    );
    assert_eq!(head(&bare, "main"), pushed, "nothing pushed");
    assert_eq!(r.board.store.get(SLUG).unwrap().status, "done");
    // The thread holds the land's record of its merge, and no landing.
    assert_eq!(
        last(&r, SLUG),
        format!("land {SLUG}: merged {tip} as {merge}, checked at {merge}; moving main there")
    );
    assert_eq!(runs(&r).len(), 1);
    // A commit on main by hand meanwhile, which no check passed on.
    write(&r.root, "notes.md", "later\n");
    git(&r.root, &["add", "notes.md"]);
    git(&r.root, &["commit", "-qm", "later on main"]);
    let top = head(&r.root, "main");
    git(&r.root, &["config", "--unset", "remote.origin.receivepack"]);
    let (said, result) = landed(&r, SLUG, &people.orch);
    let line = result.unwrap();
    assert_eq!(
        line,
        format!(
            "landed {SLUG} on main at {}, finishing a land interrupted after its merge; pushed \
             to origin",
            &merge[..12]
        )
    );
    assert!(
        said.contains(&format!(
            "land {SLUG}: its tip {} is on main already, by this land's merge {}, checked at {}: \
             finishing the land (the push, the landed line, the sweep)",
            &tip[..7],
            &merge[..7],
            &merge[..7]
        )),
        "{said}"
    );
    // What it checked is pushed, never the commit made by hand after it.
    assert_eq!(head(&bare, "main"), merge);
    assert_eq!(head(&r.root, "main"), top, "nothing merged again");
    assert_eq!(runs(&r).len(), 1, "nothing checked again");
    assert_eq!(last(&r, SLUG), line);
    assert!(!Record::path(&r.root.join(".git/symphony/tasks/land"), SLUG).exists());
    // Finished, it is closed: a land again finds no approve standing.
    let again = landed(&r, SLUG, &people.orch).1.unwrap_err();
    assert!(
        again.starts_with(&format!(
            "{SLUG} was closed after its approval: landed {SLUG} on main"
        )),
        "{again}"
    );
}
