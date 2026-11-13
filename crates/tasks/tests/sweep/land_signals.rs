//! A land stopped by a signal, or killed outright: what it ends, what it leaves, and the land after
//! it. The land runs as the `tasks` binary, in a process group of its own as a terminal's
//! foreground job is, its children in groups of theirs, which a terminal's Ctrl-C never reaches.

use super::*;
use std::collections::BTreeMap;
use std::os::unix::process::ExitStatusExt as _;
use std::process::{Command, ExitStatus};
use symphony_board::land::{Named, RUNNING, started as start_of};
use symphony_board::sys::{self, Child, Spawning as _, Stdio, pid_alive};

/// The signals that stop a land, each at its default for the land these tests signal, as a
/// terminal's job has them, whatever the run of the tests inherited: a background job inherits
/// SIGINT ignored, `nohup` SIGHUP, and a land rightly leaves an ignored one so.
const STOPPING: [libc::c_int; 3] = [libc::SIGHUP, libc::SIGINT, libc::SIGTERM];

/// The land of `SLUG` as `actor`, the `tasks` binary run from the main checkout as `just land`
/// runs it, leading a process group of its own, with each of [`STOPPING`] at its default; what it
/// prints goes to `land.out` and `land.err` beside the repository.
fn started(r: &Repo, actor: &str) -> Child {
    started_with(r, actor, r.vars())
}

/// [`started`] in the environment `vars`.
fn started_with(r: &Repo, actor: &str, vars: BTreeMap<String, String>) -> Child {
    let mut command = sys::Command::new(env!("CARGO_BIN_EXE_tasks"));
    for signal in STOPPING {
        command.default_signal(signal);
    }
    command
        .args(["land", SLUG, "--as", actor])
        .env_clear()
        .envs(vars)
        .env(RECIPE, "1")
        .current_dir(&r.root)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(fs::File::create(r.dir.join("land.out")).unwrap())
        .stderr(fs::File::create(r.dir.join("land.err")).unwrap());
    command.spawn().unwrap()
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
        folder.join(RUNNING).exists(),
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
    let log = fs::read_to_string(r.root.join(".git/symphony/tasks/land/sim%2Fjump.log")).unwrap();
    assert!(log.ends_with("ended: the land took SIGINT\n"), "{log}");
    // Its lock and its folder free, the next land lands.
    fs::remove_file(r.dir.join("hang")).unwrap();
    landed(&r, SLUG, &people.orch).1.unwrap();
    assert_eq!(head(&r.root, "main^1"), before);
}

#[test]
fn a_terminate_to_the_land_alone_ends_its_hung_check() {
    let r = Repo::new();
    hanging_check(&r, "");
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let before = head(&r.root, "main");
    let mut land = started(&r, &people.orch);
    let check = pid_in(&r.dir.join("check.pid"));
    // `kill PID`, as a supervisor ends it: the land alone.
    send(land.id(), libc::SIGTERM, false);
    assert_eq!(ended(&mut land).signal(), Some(libc::SIGTERM));
    assert_eq!(
        refused_with(&r),
        "✖ interrupted by SIGTERM: nothing landed, main where it was"
    );
    assert!(gone(check), "the check {check} was ended");
    assert_eq!(left(&r), (false, false));
    assert_eq!(head(&r.root, "main"), before);
}

#[test]
fn a_terminate_to_a_land_waiting_for_another_ends_its_wait_and_lands_nothing() {
    let r = Repo::new();
    manifest(&r, "\n[land]\ntimeout_seconds = 1\n");
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let path = r.root.join(".git/symphony/tasks/land.lock");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut held = fs::File::create(&path).unwrap();
    held.lock().unwrap();
    held.write_all(b"the land of sim/fall (pid 4242)").unwrap();
    let before = head(&r.root, "main");
    let mut land = started(&r, &people.orch);
    wait_for(|| {
        fs::read_to_string(r.dir.join("land.out"))
            .unwrap_or_default()
            .contains("waiting for the land of sim/fall (pid 4242) to finish")
    });
    // Past its timeout, which bounds a check and not a wait, it waits on.
    std::thread::sleep(Duration::from_millis(1500));
    assert!(
        land.try_wait().unwrap().is_none(),
        "it waits with no timeout"
    );
    send(land.id(), libc::SIGTERM, false);
    assert_eq!(ended(&mut land).signal(), Some(libc::SIGTERM));
    assert_eq!(
        refused_with(&r),
        "✖ interrupted by SIGTERM while waiting for the land of sim/fall (pid 4242): nothing \
         landed"
    );
    assert_eq!(head(&r.root, "main"), before);
    assert!(runs(&r).is_empty(), "no check ran");
    assert_eq!(left(&r), (false, false));
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

#[test]
fn a_full_sweep_after_a_land_stopped_past_main_keeps_its_branch_and_the_land_finishes() {
    let r = Repo::new();
    manifest(&r, "");
    let bare = remote(&r);
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
    send(land.id(), libc::SIGHUP, true);
    assert_eq!(ended(&mut land).signal(), Some(libc::SIGHUP));
    assert!(gone(push), "the push {push} was ended");
    let merge = head(&r.root, "main");
    assert_eq!(head(&r.root, "main^2"), tip);
    assert!(Record::read(&r.root.join(".git/symphony/tasks/land"), SLUG).is_some());
    // A full pass, as any done, move or verdict starts, once the window has passed: the worktree,
    // merged and clean, goes; its branch, which the land's record names, stays.
    let said = r.sweep();
    r.assert_removed(&wt, &said);
    assert!(
        said.contains(&format!(
            "kept branch {SLUG}: a land's record of its merge names it, and `just land {SLUG}` run \
             again finishes that land from it"
        )),
        "{said}"
    );
    assert_eq!(head(&r.root, SLUG), tip, "{said}");
    // Run again, it finishes: the push of what it checked, the landed line, the close.
    git(&r.root, &["config", "--unset", "remote.origin.receivepack"]);
    let line = landed(&r, SLUG, &people.orch).1.unwrap();
    assert_eq!(
        line,
        format!(
            "landed {SLUG} on main at {}, finishing a land interrupted after its merge; pushed \
             to origin",
            &merge[..12]
        )
    );
    assert_eq!(head(&bare, "main"), merge);
    assert_eq!(r.board.store.get(SLUG).unwrap().status, "done");
    assert_eq!(last(&r, SLUG), line);
    assert!(!Record::path(&r.root.join(".git/symphony/tasks/land"), SLUG).exists());
}

// -- whose a group or a worktree an earlier land left is -------------------------------------------

/// A process of the test's own (`sleep 60`) leading a process group of its own: a stranger to any
/// land, ended with its group when dropped.
struct Stranger(Child);

impl Stranger {
    fn new() -> Stranger {
        let mut sleep = sys::Command::new("sleep");
        sleep.arg("60").process_group(0).stdin(Stdio::null());
        Stranger(sleep.spawn().unwrap())
    }

    fn pid(&self) -> u32 {
        self.0.id()
    }
}

impl Drop for Stranger {
    fn drop(&mut self) {
        // SAFETY: a signal to the group of the process this test started.
        unsafe { libc::kill(-i32::try_from(self.0.id()).unwrap(), libc::SIGKILL) };
        let _ = self.0.wait();
    }
}

#[test]
fn a_group_is_named_the_lands_only_while_its_first_process_is_the_one_the_land_started() {
    let r = Repo::new();
    manifest(&r, "");
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let folder = r.root.join(".git/symphony/tasks/land");
    fs::create_dir_all(&folder).unwrap();
    let left = |group: u32, start: Option<String>| {
        let named = Named {
            group,
            start,
            waits: false,
            command: "sh -c check".into(),
        };
        fs::write(folder.join(RUNNING), named.line()).unwrap();
    };
    let stranger = Stranger::new();
    let group = stranger.pid();
    // Its first process started when the land named it: the group is the land's, to end.
    left(group, start_of(i64::from(group)));
    assert_eq!(
        landed(&r, SLUG, &people.orch).1.unwrap_err(),
        format!(
            "the process group {group} an earlier land started (`sh -c check`) outlived it and \
             still runs: end it (`kill -TERM -{group}`), then run the land again"
        )
    );
    // Named by an earlier land, with no start, or with its first process gone and the rest of the
    // group alive: nothing proves the group the land's, and no line says to end it.
    let unproven = |group: u32| {
        format!(
            "the process group {group} an earlier land started (`sh -c check`) may still run, but \
             its first process is gone or was never dated, so nothing proves the processes in \
             group {group} now are the land's: `ps -A -o pid,pgid,lstart,command` lists them \
             with their group and start; end those that are the land's, then remove {} and run \
             the land again",
            folder.join(RUNNING).display()
        )
    };
    fs::write(folder.join(RUNNING), format!("{group}\tsh -c check\n")).unwrap();
    assert_eq!(
        landed(&r, SLUG, &people.orch).1.unwrap_err(),
        unproven(group)
    );
    let quit = r.dir.join("quit");
    let mut leader = sys::Command::new("sh");
    leader
        .args([
            "-c",
            &format!(
                "sleep 60 & echo $! > '{}'; while [ ! -e '{}' ]; do sleep 0.02; done",
                r.dir.join("bg.pid").display(),
                quit.display()
            ),
        ])
        .process_group(0)
        .stdin(Stdio::null());
    let mut leader = leader.spawn().unwrap();
    let background = pid_in(&r.dir.join("bg.pid"));
    let first = leader.id();
    left(first, start_of(i64::from(first)));
    write(&r.dir, "quit", "");
    leader.wait().unwrap();
    assert!(pid_alive(background), "the rest of its group lives on");
    assert_eq!(
        landed(&r, SLUG, &people.orch).1.unwrap_err(),
        unproven(first)
    );
    send(first, libc::SIGKILL, true);
    assert!(gone(background));
    // A first process of another start has the number now: no pid is reused while a group of
    // that number lives, so the land's group is gone. It is forgotten, and the stranger is never
    // ended.
    left(group, Some("0.000000".into()));
    let (said, result) = landed(&r, SLUG, &people.orch);
    result.unwrap();
    assert!(!said.contains("kill"), "{said}");
    assert!(!folder.join(RUNNING).exists());
    assert!(pid_alive(i64::from(group)), "the stranger runs on");
}

#[test]
fn a_worktree_whose_lock_names_a_number_another_process_took_since_is_replaced() {
    let r = Repo::new();
    manifest(&r, "");
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let tree = r.root.join(".git/symphony/tasks/land/worktree");
    let shown = tree.to_string_lossy().into_owned();
    let stranger = Stranger::new();
    let pid = stranger.pid();
    let locked = |start: &str| {
        let reason = format!("symphony land: x (pid {pid}, started {start})");
        git(
            &r.root,
            &[
                "worktree", "add", "-q", "--detach", "--lock", "--reason", &reason, &shown, "main",
            ],
        );
        reason
    };
    // Its pid the process of the start the lock names: a land still running, kept.
    let own = start_of(i64::from(pid)).unwrap();
    let reason = locked(&own);
    assert_eq!(
        landed(&r, SLUG, &people.orch).1.unwrap_err(),
        format!(
            "{shown} is the worktree of a land still running (pid {pid}, {reason}): wait for it to \
             end"
        )
    );
    assert!(tree.join("sim/world.gd").is_file());
    // The land that locked it was killed and a stranger took its number after it: replaced.
    git(&r.root, &["worktree", "unlock", &shown]);
    git(&r.root, &["worktree", "remove", "--force", &shown]);
    locked("0.000000");
    landed(&r, SLUG, &people.orch).1.unwrap();
    assert!(!tree.exists());
    assert!(pid_alive(i64::from(pid)), "the stranger runs on");
}

/// `name` in `dir` as an executable shell script of `body`.
fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    write(dir, name, &format!("#!/bin/sh\n{body}"));
    let path = dir.join(name);
    let chmod = Command::new("chmod")
        .arg("+x")
        .arg(&path)
        .status_locked()
        .unwrap();
    assert!(chmod.success());
    path
}

#[test]
fn a_leftover_worktree_lsof_cannot_judge_is_kept_and_the_line_says_how_to_look() {
    let r = Repo::new();
    manifest(&r, "");
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let tree = r.root.join(".git/symphony/tasks/land/worktree");
    let shown = tree.to_string_lossy().into_owned();
    let reason = format!("symphony land: x (pid {}, started 0.000000)", dead_pid());
    git(
        &r.root,
        &[
            "worktree", "add", "-q", "--detach", "--lock", "--reason", &reason, &shown, "main",
        ],
    );
    // An lsof that cannot answer, first on the land's PATH.
    let bin = r.dir.join("bin");
    script(&bin, "lsof", "echo 'lsof: no answer' >&2\nexit 1\n");
    let mut vars = r.vars();
    let path = format!("{}:{}", bin.display(), vars["PATH"]);
    vars.insert("PATH".into(), path);
    let mut land = started_with(&r, &people.orch, vars);
    assert!(!ended(&mut land).success());
    assert_eq!(
        refused_with(&r),
        format!(
            "✖ lsof cannot say whether a process works in {shown}, the worktree an earlier land \
             left: once nothing does (`lsof +D {shown}` lists each process with a file or its \
             folder there, and none once nothing works there), `git worktree remove --force \
             --force {shown}`, then run the land again"
        )
    );
    assert!(tree.join("sim/world.gd").is_file(), "it is kept");
}

#[test]
fn a_move_whose_git_is_ended_after_main_moved_is_a_move_and_the_land_goes_on_to_close() {
    let r = Repo::new();
    manifest(&r, "");
    // A post-merge hook that waits in the main checkout alone, after git moved the branch there.
    let dir = r.dir.display();
    let root = r.root.display();
    script(
        &r.root.join(".git/hooks"),
        "post-merge",
        &format!(
            "if [ \"$(pwd -P)\" = '{root}' ] && [ ! -e '{dir}/go' ]; then\n  echo $$ > \
             '{dir}/hook.pid'\n  while [ ! -e '{dir}/go' ]; do sleep 0.02; done\nfi\n"
        ),
    );
    let wt = branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let tip = head(&wt, "HEAD");
    let people = approved(&r, SLUG, &format!("{tip}: good"));
    let before = head(&r.root, "main");
    let mut land = started(&r, &people.orch);
    pid_in(&r.dir.join("hook.pid"));
    let (group, _) = moving_group(&r);
    // git ends by the signal, its exit a failure, but main holds the merge: the move happened, the
    // record stands, and the land goes on to close it.
    send(group, libc::SIGTERM, true);
    assert!(ended(&mut land).success());
    assert_eq!(head(&r.root, "main^1"), before);
    assert_eq!(head(&r.root, "main^2"), tip);
    let merge = head(&r.root, "main");
    assert_eq!(
        last(&r, SLUG),
        format!(
            "landed {SLUG} on main at {}; not pushed: no origin remote",
            &merge[..12]
        )
    );
    assert_eq!(runs(&r).len(), 1, "checked once");
    assert_eq!(git(&r.root, &["status", "--porcelain"]), "");
}

// -- a signal while the main checkout moves ---------------------------------------------------------

/// A main checkout whose fast-forward stalls on `sim/zz.gd` until `go` is beside the repository: a
/// smudge filter that waits there once the check has run (it leaves `moving`), its pid in
/// `stall.pid`. The branch adds `sim/jump.gd` and `sim/zz.gd` and changes `sim/world.gd`, which
/// git writes in that order, so a move stalled on the last has written the others. Its people
/// and its tip.
fn stalling_move(r: &Repo) -> (People, String) {
    let dir = r.dir.display();
    write(
        &r.dir,
        "stall.sh",
        &format!(
            "if [ -e '{dir}/moving' ] && [ ! -e '{dir}/go' ] && [ \"$1\" = sim/zz.gd ]; then\n  \
             echo $$ > '{dir}/stall.pid'\n  while [ ! -e '{dir}/go' ]; do sleep 0.02; done\nfi\n\
             exec cat\n"
        ),
    );
    git(
        &r.root,
        &[
            "config",
            "filter.stall.smudge",
            &format!("sh '{dir}/stall.sh' %f"),
        ],
    );
    write(&r.root, ".gitattributes", "sim/zz.gd filter=stall\n");
    git(&r.root, &["add", ".gitattributes"]);
    git(&r.root, &["commit", "-qm", "a filter that can stall"]);
    checked_by(r, &format!("touch '{dir}/moving'"));
    let wt = branch(r, SLUG, "sim/jump.gd", "extends Node\n");
    write(&wt, "sim/world.gd", "extends Node2D\n");
    write(&wt, "sim/zz.gd", "extends Node3D\n");
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "more work"]);
    let tip = head(&wt, "HEAD");
    let people = approved(r, SLUG, &format!("{tip}: good"));
    (people, tip)
}

/// The group the land names in [`RUNNING`] while it moves the main checkout, and the move's
/// command.
fn moving_group(r: &Repo) -> (u32, String) {
    let path = r.root.join(".git/symphony/tasks/land").join(RUNNING);
    let named = || {
        fs::read_to_string(&path)
            .ok()
            .and_then(|text| Named::parse(&text))
            .filter(|named| named.waits)
    };
    wait_for(|| named().is_some());
    let named = named().unwrap();
    (named.group, named.command)
}

/// What the main checkout holds of the branch's files: `sim/world.gd` as main had it, and neither
/// new file, or each as the branch wrote it.
fn main_checkout_is(r: &Repo, moved: bool) {
    let read = |file: &str| fs::read_to_string(r.root.join(file)).ok();
    if moved {
        assert_eq!(read("sim/world.gd").as_deref(), Some("extends Node2D\n"));
        assert_eq!(read("sim/jump.gd").as_deref(), Some("extends Node\n"));
        assert_eq!(read("sim/zz.gd").as_deref(), Some("extends Node3D\n"));
    } else {
        assert_eq!(
            read("sim/world.gd").as_deref(),
            Some("extends RefCounted\n")
        );
        assert_eq!(read("sim/jump.gd"), None);
        assert_eq!(read("sim/zz.gd"), None);
    }
    assert_eq!(git(&r.root, &["status", "--porcelain"]), "");
}

/// What the land says of a move that did not happen and the files it restored, main at `before`.
fn restored(before: &str) -> String {
    format!(
        "in the main checkout, which the move half-wrote, the land restored sim/world.gd as main \
         at {b} has it; removed sim/jump.gd, which main at {b} lacks",
        b = &before[..7]
    )
}

#[test]
fn a_terminals_interrupt_while_main_moves_lets_the_move_end_and_a_run_again_finishes_the_land() {
    let r = Repo::new();
    let (people, tip) = stalling_move(&r);
    let before = head(&r.root, "main");
    let mut land = started(&r, &people.orch);
    pid_in(&r.dir.join("stall.pid"));
    let (group, _) = moving_group(&r);
    // A terminal's Ctrl-C while git writes the main checkout: the land's group, never the move's,
    // which the land waits for.
    send(land.id(), libc::SIGINT, true);
    std::thread::sleep(Duration::from_millis(200));
    assert!(pid_alive(i64::from(group)), "the move runs on");
    write(&r.dir, "go", "");
    assert_eq!(ended(&mut land).signal(), Some(libc::SIGINT));
    let merge = head(&r.root, "main");
    assert_eq!(head(&r.root, "main^1"), before);
    assert_eq!(head(&r.root, "main^2"), tip);
    assert_eq!(
        refused_with(&r),
        format!(
            "✖ interrupted by SIGINT after main moved to the merge of {SLUG}, {}: run `just land \
             {SLUG}` again to finish its push, its landed line and its sweep",
            &merge[..7]
        )
    );
    main_checkout_is(&r, true);
    let folder = r.root.join(".git/symphony/tasks/land");
    assert!(Record::path(&folder, SLUG).exists(), "its record is kept");
    assert!(!folder.join(symphony_board::land::MOVING).exists());
    assert!(!folder.join(RUNNING).exists());
    let (_, result) = landed(&r, SLUG, &people.orch);
    assert_eq!(
        result.unwrap(),
        format!(
            "landed {SLUG} on main at {}, finishing a land interrupted after its merge; not \
             pushed: no origin remote",
            &merge[..12]
        )
    );
    assert_eq!(head(&r.root, "main"), merge, "nothing merged again");
}

#[test]
fn a_move_ended_part_way_is_restored_its_record_dropped_and_said_and_the_next_land_lands() {
    let r = Repo::new();
    let (people, tip) = stalling_move(&r);
    let before = head(&r.root, "main");
    let mut land = started(&r, &people.orch);
    pid_in(&r.dir.join("stall.pid"));
    let (group, _) = moving_group(&r);
    // The move's own group ended while the land waits for it: git stops with the main checkout
    // half-written, main where it was.
    send(group, libc::SIGTERM, true);
    let status = ended(&mut land);
    assert_eq!(status.signal(), None, "the land took no signal");
    assert!(!status.success());
    assert_eq!(head(&r.root, "main"), before);
    let line = refused_with(&r);
    assert!(line.starts_with("✖ main could not move to "), "{line}");
    let said = format!(
        ": git merge --ff-only was ended by SIGTERM; {}; its record dropped: nothing landed",
        restored(&before)
    );
    assert!(line.ends_with(&said), "{line}");
    assert!(
        last(&r, SLUG).starts_with("land refused: main could not move to ")
            && last(&r, SLUG).ends_with(&said),
        "{}",
        last(&r, SLUG)
    );
    main_checkout_is(&r, false);
    let folder = r.root.join(".git/symphony/tasks/land");
    assert!(!Record::path(&folder, SLUG).exists());
    assert!(!folder.join(symphony_board::land::MOVING).exists());
    // Nothing is left to refuse on: the next land lands.
    write(&r.dir, "go", "");
    landed(&r, SLUG, &people.orch).1.unwrap();
    assert_eq!(head(&r.root, "main^1"), before);
    assert_eq!(head(&r.root, "main^2"), tip);
    main_checkout_is(&r, true);
}

#[test]
fn a_land_killed_while_main_moves_leaves_the_move_to_end_and_the_next_restores_what_it_half_wrote()
{
    let r = Repo::new();
    let (people, tip) = stalling_move(&r);
    let before = head(&r.root, "main");
    let mut land = started(&r, &people.orch);
    pid_in(&r.dir.join("stall.pid"));
    let (group, command) = moving_group(&r);
    // Killed outright, the land leaves its move running in its group, named.
    send(land.id(), libc::SIGKILL, false);
    assert_eq!(ended(&mut land).signal(), Some(libc::SIGKILL));
    assert!(pid_alive(i64::from(group)));
    let top = command.rsplit(' ').next().unwrap().to_string();
    assert_eq!(command, format!("git merge --ff-only --quiet {top}"));
    // The next land waits for it, never ending it, and touches nothing.
    let (_, result) = landed(&r, SLUG, &people.orch);
    assert_eq!(
        result.unwrap_err(),
        format!(
            "the process group {group} an earlier land started (`{command}`) still moves the main \
             checkout: wait for it to end, then run the land again, which restores what it \
             half-wrote should it end without moving it (were it to hang, `kill -TERM -{group}` \
             ends it)"
        )
    );
    assert!(pid_alive(i64::from(group)));
    // Ended part way, it leaves the main checkout half-written: the next land restores it, says
    // so here and on the thread, and lands.
    send(group, libc::SIGTERM, true);
    assert!(gone(i64::from(group)));
    assert_eq!(head(&r.root, "main"), before);
    assert_ne!(git(&r.root, &["status", "--porcelain"]), "");
    write(&r.dir, "go", "");
    let (said, result) = landed(&r, SLUG, &people.orch);
    let line = format!(
        "land {SLUG}: its move of main to {} in the main checkout ended without moving it, the \
         land killed meanwhile; {}; its record dropped: nothing of it landed",
        &top[..7],
        restored(&before)
    );
    assert!(said.starts_with(&format!("{line}\n")), "{said}");
    assert!(
        r.board.said(SLUG).contains(&line),
        "{:?}",
        r.board.said(SLUG)
    );
    result.unwrap();
    assert_eq!(head(&r.root, "main^1"), before);
    assert_eq!(head(&r.root, "main^2"), tip);
    main_checkout_is(&r, true);
}
