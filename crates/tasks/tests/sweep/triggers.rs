//! Work finishing starts a sweep by itself (src/sweep/trigger.rs): the command that finished the work
//! returns at once, and the main checkout's build of `tasks` sweeps, detached, from the main
//! checkout, into `sf-tasks/sweep.log`. A test's board starts none unless told (`TASKS_SWEEP=on`);
//! herdr is the crate's `fake-herdr`, holding no panes. The build is this test's
//! (`TASKS_BIN`), but in the tests that drop it: there the main checkout's `tools/rs-bin.sh` is a
//! fake naming a build, or there is none.
//!
//! A sweep that starts is waited for in its log. One that must not start is checked at once and
//! for certain: a sweep is started only after its log is opened (and the hourly one after its stamp is
//! written), both before the command returns, so a command that started none left neither.

use crate::common::Seed;
use crate::{IDLE_HOURS, Repo, write};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use symphony_board::NewTask;
use symphony_board::cli::Env;
use symphony_board::sweep::trigger::{HOURLY_SECONDS, LOG, STAMP, folder};

/// `tasks ARGS` as `handle`, from `cwd`, the sweep let start on this board, with `change` made to
/// the environment: what it printed (it must succeed).
fn run(
    r: &Repo,
    handle: &str,
    cwd: &Path,
    args: &[&str],
    change: impl FnOnce(&mut BTreeMap<String, String>),
) -> String {
    let mut vars = r.vars();
    let herdr = r.dir.join("herdr.json");
    if !herdr.exists() {
        fs::write(&herdr, r#"{"tabs": [], "next": 1, "panes": []}"#).unwrap();
    }
    let state = herdr.to_string_lossy().into_owned();
    for (key, value) in [
        ("TASKS_SWEEP", "on"),
        ("TASKS_BIN", env!("CARGO_BIN_EXE_tasks")),
        ("HERDR_BIN", env!("CARGO_BIN_EXE_fake-herdr")),
        ("FAKE_HERDR_STATE", state.as_str()),
        ("TASKS_SESSION", handle),
    ] {
        vars.insert(key.into(), value.into());
    }
    change(&mut vars);
    let env = Env {
        vars,
        cwd: cwd.to_path_buf(),
    };
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = symphony_board::cli::main(&args, &env, &mut "".as_bytes(), &mut out, &mut err);
    let (out, err) = (
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    );
    assert_eq!(code, 0, "{args:?}: {out}{err}");
    out
}

fn home(r: &Repo) -> PathBuf {
    folder(&r.root.join(".git"))
}

fn log(r: &Repo) -> PathBuf {
    home(r).join(LOG)
}

/// The log once `count` of its lines start with `start`.
fn logged(r: &Repo, start: &str, count: usize) -> String {
    let began = Instant::now();
    loop {
        let text = std::fs::read_to_string(log(r)).unwrap_or_default();
        if text.lines().filter(|l| l.starts_with(start)).count() >= count {
            return text;
        }
        assert!(
            began.elapsed() < Duration::from_secs(60),
            "no {start:?} line within a minute; the log:\n{text}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// The log once it holds `count` sweeps' summaries.
fn swept(r: &Repo, count: usize) -> String {
    logged(r, "worktree sweep: ", count)
}

/// An executable shell script at `path` under `dir`, running `body`.
fn script(dir: &Path, path: &str, body: &str) -> PathBuf {
    write(dir, path, &format!("#!/bin/sh\n{body}\n"));
    let file = dir.join(path);
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
    file
}

/// `checkout`'s own `tools/rs-bin.sh`, naming `build` as its build of `tasks`, and noting in
/// `ran` that it was asked.
fn names_its_build(checkout: &Path, build: &Path, ran: &Path) {
    script(
        checkout,
        "tools/rs-bin.sh",
        &format!(
            "echo \"$*\" >> '{}'\n[ \"$1 $2\" = '--where tasks' ] || exit 2\necho '{}'",
            ran.display(),
            build.display()
        ),
    );
}

/// The log says a sweep started for `why`, from the main checkout.
fn started_for(text: &str, why: &str) {
    assert!(
        text.lines()
            .any(|l| l.starts_with('[') && l.ends_with(&format!("] {why}"))),
        "no sweep for {why:?}:\n{text}"
    );
}

/// No sweep started: none opened the log or wrote the stamp.
fn none_started(r: &Repo) {
    assert!(!log(r).exists(), "{:?}", std::fs::read_to_string(log(r)));
    assert!(!home(r).join(STAMP).exists());
}

/// A worktree idle past the window, at main, that nothing keeps.
fn idle_worktree(r: &Repo, name: &str) -> PathBuf {
    let wt = r.detached(name);
    Repo::age(&wt, IDLE_HOURS);
    wt
}

#[test]
fn a_subtask_done_starts_a_sweep_in_which_its_closed_sessions_worktree_goes_at_once() {
    let r = Repo::with(Seed::default().task("sim/jump", "in_progress"));
    r.board
        .store
        .create(&NewTask {
            parent: Some("sim/jump".into()),
            instruction: true,
            ..NewTask::new("sim/jump/do", "Do", "owner")
        })
        .unwrap();
    // Touched just now, its executor live and bound to it: only the finish lets it go.
    let wt = r.detached("do");
    let handle = r.bind(&wt);
    r.board.store.claim("sim/jump/do", &handle).unwrap();
    r.board
        .store
        .record_launch(&symphony_board::NewLaunch {
            worktree: wt.to_string_lossy().into_owned(),
            ..symphony_board::NewLaunch::new("sim/jump/do", &handle, "executor")
        })
        .unwrap();
    r.board
        .store
        .close_task_session("sim/jump/do", &handle, "done", "just tasks restore")
        .unwrap();
    let out = run(&r, &handle, &wt, &["done", "sim/jump/do", "did it"], |_| {});
    assert_eq!(out, "sim/jump/do is done\n");
    let text = swept(&r, 1);
    started_for(&text, "sim/jump/do is done");
    r.assert_removed(&wt, &text);
}

#[test]
fn a_task_moved_to_cancelled_starts_one() {
    let r = Repo::with(
        Seed::default()
            .session("amber-heron")
            .task("sim/jump", "ready"),
    );
    let wt = idle_worktree(&r, "idle");
    run(
        &r,
        "amber-heron",
        &r.root,
        &["move", "sim/jump", "cancelled"],
        |_| {},
    );
    let text = swept(&r, 1);
    started_for(&text, "sim/jump is cancelled");
    r.assert_removed(&wt, &text);
}

#[test]
fn an_approving_verdict_starts_one() {
    let r = Repo::with(
        Seed::default()
            .session("amber-heron")
            .task("sim/jump", "review")
            .role("sim/jump", "amber-heron", "reviewer"),
    );
    let wt = idle_worktree(&r, "idle");
    let out = run(
        &r,
        "amber-heron",
        &r.root,
        &["verdict", "sim/jump", "approve", "fine"],
        |_| {},
    );
    assert_eq!(out, "sim/jump is done\n");
    let text = swept(&r, 1);
    started_for(&text, "sim/jump is done");
    r.assert_removed(&wt, &text);
}

#[test]
fn a_session_leaving_starts_one_from_the_main_checkout() {
    // It leaves from its own worktree, which then goes: the sweep did not run from there.
    let r = Repo::new();
    let wt = idle_worktree(&r, "mine");
    let handle = r.bind(&wt);
    Repo::age(&wt, IDLE_HOURS);
    run(&r, &handle, &wt, &["leave"], |_| {});
    let text = swept(&r, 1);
    started_for(&text, &format!("{handle} left"));
    r.assert_removed(&wt, &text);
}

#[test]
fn a_reap_starts_one_and_its_dry_run_none() {
    let r = Repo::with(Seed::default().session("amber-heron"));
    let wt = idle_worktree(&r, "idle");
    run(&r, "amber-heron", &r.root, &["reap", "--dry-run"], |_| {});
    none_started(&r);
    run(&r, "amber-heron", &r.root, &["reap"], |_| {});
    let text = swept(&r, 1);
    started_for(&text, "a reap");
    r.assert_removed(&wt, &text);
}

#[test]
fn inbox_starts_one_at_most_once_an_hour() {
    let r = Repo::with(Seed::default().session("amber-heron"));
    let wt = idle_worktree(&r, "idle");
    run(&r, "amber-heron", &r.root, &["inbox"], |_| {});
    let text = swept(&r, 1);
    started_for(&text, "hourly");
    r.assert_removed(&wt, &text);
    let stamp = home(&r).join(STAMP);
    let written = std::fs::read_to_string(&stamp).unwrap();
    // Within the hour: the stamp is read and left as it is, and nothing starts.
    run(&r, "amber-heron", &r.root, &["inbox"], |_| {});
    assert_eq!(std::fs::read_to_string(&stamp).unwrap(), written);
    // An hour on, the next inbox claims the stamp and starts the next.
    let then: f64 = written.parse().unwrap();
    std::fs::write(&stamp, (then - HOURLY_SECONDS - 1.0).to_string()).unwrap();
    let again = idle_worktree(&r, "idle-again");
    run(&r, "amber-heron", &r.root, &["inbox"], |_| {});
    assert_ne!(std::fs::read_to_string(&stamp).unwrap(), written);
    let text = swept(&r, 2);
    r.assert_removed(&again, &text);
}

#[test]
fn a_test_board_starts_none_unless_told_and_off_starts_none_anywhere() {
    for (sweep, unset_server) in [(None, false), (Some("off"), false), (Some("off"), true)] {
        let r = Repo::with(
            Seed::default()
                .session("amber-heron")
                .task("sim/jump", "ready"),
        );
        idle_worktree(&r, "idle");
        let change = |vars: &mut BTreeMap<String, String>| {
            match sweep {
                Some(value) => vars.insert("TASKS_SWEEP".into(), value.into()),
                None => vars.remove("TASKS_SWEEP"),
            };
            if unset_server {
                // The repository's own board: `off` still starts none (no server here, so the
                // command is not run; the rule is the trigger's own, checked directly).
                vars.remove("TASKS_SERVER");
            }
        };
        if unset_server {
            let mut vars = r.vars();
            change(&mut vars);
            let env = Env {
                vars,
                cwd: r.root.clone(),
            };
            assert!(!symphony_board::sweep::trigger::trigger(
                &env, &r.root, "x", None
            ));
            assert!(!symphony_board::sweep::trigger::hourly(&env, &r.root));
        } else {
            run(
                &r,
                "amber-heron",
                &r.root,
                &["move", "sim/jump", "cancelled"],
                change,
            );
            run(&r, "amber-heron", &r.root, &["inbox"], change);
            run(&r, "amber-heron", &r.root, &["leave"], change);
        }
        none_started(&r);
    }
}

#[test]
fn a_move_that_finishes_nothing_starts_none() {
    let r = Repo::with(
        Seed::default()
            .session("amber-heron")
            .task("sim/jump", "ready"),
    );
    idle_worktree(&r, "idle");
    run(
        &r,
        "amber-heron",
        &r.root,
        &["move", "sim/jump", "blocked"],
        |_| {},
    );
    none_started(&r);
}

#[test]
fn outside_a_repository_nothing_starts() {
    let r = Repo::new();
    let mut vars = r.vars();
    vars.insert("TASKS_SWEEP".into(), "on".into());
    let env = Env {
        vars,
        cwd: r.dir.clone(),
    };
    assert!(!symphony_board::sweep::trigger::trigger(
        &env, &r.dir, "x", None
    ));
    assert!(!symphony_board::sweep::trigger::hourly(&env, &r.dir));
    none_started(&r);
}

#[test]
fn a_trigger_from_a_worktree_whose_sources_differ_runs_the_main_checkouts_build_not_its_own() {
    let r = Repo::with(
        Seed::default()
            .session("amber-heron")
            .task("sim/jump", "ready"),
    );
    let gone = idle_worktree(&r, "idle");
    // The main checkout's build says so, then sweeps as this test's build would.
    let build = script(
        &r.dir,
        "root-tasks",
        &format!(
            "echo \"root build: $*\"\nexec '{}' \"$@\"",
            env!("CARGO_BIN_EXE_tasks")
        ),
    );
    let asked = r.dir.join("root-asked");
    names_its_build(&r.root, &build, &asked);
    // The worktree it is run from has sources of its own, and a build of them that must not run.
    let mine = r.detached("mine");
    let own = script(
        &r.dir,
        "own-tasks",
        &format!("touch '{}'", r.dir.join("own-ran").display()),
    );
    names_its_build(&mine, &own, &r.dir.join("own-asked"));
    run(
        &r,
        "amber-heron",
        &mine,
        &["move", "sim/jump", "cancelled"],
        |vars| {
            vars.remove("TASKS_BIN");
        },
    );
    let text = swept(&r, 1);
    assert!(
        text.lines()
            .any(|l| l == "root build: sweep --background=sim/jump is cancelled --task=sim/jump"),
        "{text}"
    );
    started_for(&text, "sim/jump is cancelled");
    r.assert_removed(&gone, &text);
    assert_eq!(fs::read_to_string(&asked).unwrap(), "--where tasks\n");
    assert!(!r.dir.join("own-asked").exists(), "{text}");
    assert!(!r.dir.join("own-ran").exists(), "{text}");
}

#[test]
fn with_no_build_of_the_main_checkouts_sources_nothing_sweeps_and_the_log_says_so() {
    // No tools/rs-bin.sh in the main checkout; one naming a build that is not there.
    for named in [false, true] {
        let r = Repo::with(
            Seed::default()
                .session("amber-heron")
                .task("sim/jump", "ready"),
        );
        let idle = idle_worktree(&r, "idle");
        let missing = r.dir.join("never-built");
        if named {
            names_its_build(&r.root, &missing, &r.dir.join("asked"));
        }
        run(
            &r,
            "amber-heron",
            &r.root,
            &["move", "sim/jump", "cancelled"],
            |vars| {
                vars.remove("TASKS_BIN");
            },
        );
        let text = logged(&r, "no sweep: ", 1);
        started_for(&text, "sim/jump is cancelled");
        let said = if named {
            format!(
                "no sweep: tasks is not built for the sources of the main checkout {} (no {}); a tasks command run there builds it",
                r.root.display(),
                missing.display()
            )
        } else {
            format!(
                "no sweep: {}/tools/rs-bin.sh named no build of tasks for the sources of the main checkout",
                r.root.display()
            )
        };
        assert!(text.lines().any(|l| l == said), "{said}\n{text}");
        assert!(!text.contains("worktree sweep: "), "{text}");
        r.assert_kept(&idle, &text);
    }
}
