//! Going to a launched agent's session through the fake hc: the Herdr app shows its pane, or its
//! screen is read, and nothing is ever typed into it.

use crate::children::Child as _;
use crate::{Folder, World};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use symphony_board::cli::Env;
use symphony_board::launch::go::{DEADLINE, focus, screen};
use symphony_board::launch::hc::Binary;
use symphony_board::sys::{pid_alive, terminate};

/// The deadline a stand-in that never answers is first asked under, doubled for each try in which
/// it was not yet up when it ran out ([`once_up`]).
const FIRST_TRY: Duration = Duration::from_secs(1);

/// The longest deadline [`once_up`] tries: a stand-in not up in that long is broken, not slow.
const LAST_TRY: Duration = Duration::from_secs(32);

/// A deadline every call that answers keeps on a loaded machine: the fake hc's first start once
/// it is linked can take seconds while macOS assesses it.
const ANSWERING: Duration = Duration::from_secs(100);

/// Writes `script` as the executable `dir/NAME`.
fn stand_in(dir: &Path, name: &str, script: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// A stand-in for hc or the Herdr app that never answers: each call adds its words to `NAME.calls`
/// and its pid to `NAME.pids`, then becomes a `sleep` that outlasts any wait, in the same process,
/// so the pid written is the one a kill must end.
fn never_answering(dir: &Path, name: &str) -> PathBuf {
    let (calls, pids) = (
        dir.join(format!("{name}.calls")),
        dir.join(format!("{name}.pids")),
    );
    stand_in(
        dir,
        name,
        &format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\necho $$ >> '{}'\nexec sleep 600\n",
            calls.display(),
            pids.display()
        ),
    )
}

/// A stand-in that never answers and whose output is held by a process it started: each call adds
/// its words to `NAME.calls`, starts a `sleep` in the background, which keeps its stdout and
/// stderr, writes that `sleep`'s pid and its own on one line of `NAME.pids`, and waits for it. The
/// `sleep` ends by itself within ten minutes, should a test lose its pid: still running when the
/// call answers, it shows the call did not wait for it.
fn holding_its_output_in_a_grandchild(dir: &Path, name: &str) -> PathBuf {
    let (calls, pids) = (
        dir.join(format!("{name}.calls")),
        dir.join(format!("{name}.pids")),
    );
    stand_in(
        dir,
        name,
        &format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\nsleep 600 &\necho \"$! $$\" >> '{}'\nwait\n",
            calls.display(),
            pids.display()
        ),
    )
}

/// The lines a stand-in wrote to `dir/FILE`, none when it wrote none.
fn lines(dir: &Path, file: &str) -> Vec<String> {
    std::fs::read_to_string(dir.join(file))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// A stand-in was asked `call` once, or nothing when it was killed before it was up; never another.
fn asked_at_most(dir: &Path, file: &str, call: &str) {
    let asked = lines(dir, file);
    assert!(asked.is_empty() || asked == [call], "{file}: {asked:?}");
}

/// An environment whose hc and Herdr app are stand-ins that never answer, written in `folder`.
fn hung(folder: &Folder) -> Env {
    never_answering(&folder.0, "hc");
    never_answering(&folder.0, "app");
    hung_in(&folder.0)
}

/// An environment whose hc and Herdr app are the stand-ins [`hung`] wrote in `dir`.
fn hung_in(dir: &Path) -> Env {
    let mut env = Env {
        vars: Default::default(),
        cwd: dir.to_path_buf(),
    };
    for (key, value) in [
        ("PATH", std::env::var("PATH").unwrap_or_default()),
        ("HC_BIN", dir.join("hc").display().to_string()),
        ("HERDR_APP_BIN", dir.join("app").display().to_string()),
    ] {
        env.vars.insert(key.into(), value);
    }
    env
}

/// Every pid a stand-in wrote to `dir/FILE` ends and is reaped (a zombie still answers a signal)
/// soon after the call that started it gave up.
fn reaped(dir: &Path, file: &str) {
    for pid in lines(dir, file)
        .iter()
        .flat_map(|line| line.split_whitespace())
    {
        let pid: i64 = pid.parse().unwrap();
        test_time::until_true(
            format!("{file}: {pid} was killed and reaped"),
            test_time::backstop(Duration::from_secs(30)),
            || !pid_alive(pid),
        )
        .unwrap();
    }
}

/// Tries `ask`, handed a deadline, of stand-ins that never answer, from [`FIRST_TRY`] and doubling,
/// until each stand-in named in `up` was up before the deadline ran out (it wrote its pids). A
/// deadline counts from the call, a loaded machine can take longer than any fixed one to start a
/// stand-in, and only one that was up shows it was asked and then killed; so no try depends on how
/// soon a process starts. `ask` checks every try as it answers, whether its stand-ins were up or
/// not, and their files are cleared before the next.
fn once_up(dir: &Path, up: &[&str], mut ask: impl FnMut(Duration)) {
    let mut given = FIRST_TRY;
    loop {
        for name in up {
            for file in [format!("{name}.calls"), format!("{name}.pids")] {
                let _ = std::fs::remove_file(dir.join(file));
            }
        }
        ask(given);
        if up
            .iter()
            .all(|name| !lines(dir, &format!("{name}.pids")).is_empty())
        {
            return;
        }
        assert!(given < LAST_TRY, "{up:?} not up within {given:?}");
        given *= 2;
    }
}

#[test]
fn a_hung_window_or_hc_answers_within_the_deadline_all_calls_together_and_its_child_is_reaped() {
    // The page gives up after 15 s; the server's focus and screen read have ended before.
    assert!(DEADLINE < Duration::from_secs(15), "{DEADLINE:?}");
    let folder = Folder::new();
    let env = hung(&folder);
    once_up(&folder.0, &["app", "hc"], |given| {
        let (focused, read) = std::thread::scope(|s| {
            let focused = s.spawn(|| {
                let started = Instant::now();
                (
                    focus(&Binary::of(&env).within(given), "t7"),
                    started.elapsed(),
                )
            });
            let read = s.spawn(|| {
                let started = Instant::now();
                (
                    screen(&Binary::of(&env).within(given), "t7"),
                    started.elapsed(),
                )
            });
            (focused.join().unwrap(), read.join().unwrap())
        });
        // The refusals name the deadline's time only when the deadline, not the 120 s every call
        // has, was the limit that ran out: that, not how soon they came, says it was the deadline's.
        let secs = given.as_secs();
        let (refused, took) = (focused.0.unwrap_err(), focused.1);
        assert!(took >= given, "the focus took {took:?} of {given:?}");
        assert_eq!(
            refused.0,
            format!(
                "the Herdr app did not show t7: herdr-app do agent.focus: no answer within the \
                 {secs} s it is given; herdr-app do pane.focus: not asked: the {secs} s it is \
                 given are spent"
            )
        );
        // One call took the whole deadline: the pane's focus and the raise were never started.
        asked_at_most(&folder.0, "app.calls", "do agent.focus --terminal t7");
        let (refused, took) = (read.0.unwrap_err(), read.1);
        assert!(took >= given, "the screen read took {took:?} of {given:?}");
        assert_eq!(
            refused.0,
            format!("hc read t7: no answer within the {secs} s it is given")
        );
        asked_at_most(&folder.0, "hc.calls", "read t7 --lines 0");
        reaped(&folder.0, "app.pids");
        reaped(&folder.0, "hc.pids");
    });
}

/// The deadline's kill reaches the hung call's own process alone: a process it started that holds
/// its output lingers until it ends, and keeps the killed one unreaped meanwhile (docs/workflow/TASKS.md,
/// "One deadline, one at a time"). The call answers at the deadline all the same.
#[test]
fn a_hung_call_answers_at_the_deadline_though_what_it_started_holds_its_output() {
    let folder = Folder::new();
    let mut env = hung(&folder);
    env.vars.insert(
        "HC_BIN".into(),
        holding_its_output_in_a_grandchild(&folder.0, "holder")
            .display()
            .to_string(),
    );
    once_up(&folder.0, &["holder"], |given| {
        let started = Instant::now();
        let refused = screen(&Binary::of(&env).within(given), "t7").unwrap_err();
        let took = started.elapsed();
        // At the deadline, though what the call started still holds its output: the refusal names
        // the deadline only when it was the limit that ran out.
        assert!(took >= given, "the screen read took {took:?} of {given:?}");
        assert_eq!(
            refused.0,
            format!(
                "hc read t7: no answer within the {} s it is given",
                given.as_secs()
            )
        );
        asked_at_most(&folder.0, "holder.calls", "read t7 --lines 0");
        // What the stand-in started outlived the kill, and the call did not wait for it: once it
        // is ended, both are reaped.
        for line in lines(&folder.0, "holder.pids") {
            let started: i64 = line.split_whitespace().next().unwrap().parse().unwrap();
            assert!(
                pid_alive(started),
                "{started} ended with the call's kill, or the call waited for it"
            );
            assert!(terminate(started, true), "{started}");
        }
        reaped(&folder.0, "holder.pids");
    });
    let up = lines(&folder.0, "holder.pids");
    assert_eq!(up.len(), 1, "{up:?}");
    assert_eq!(up[0].split_whitespace().count(), 2, "{up:?}");
}

/// Names the folder of the stand-ins the re-run of
/// [`a_caller_killed_through_its_group_takes_its_hung_call_with_it`] asks.
const CALLER: &str = "SYMPHONY_GO_CALLER_STAND_INS";

/// A hung call stays in its caller's process group: a caller killed through it, as the server
/// ends a `tasks reap` it runs or a terminal's Ctrl-C reaches a command, takes the call's process
/// with it. The caller is a re-run of this test leading a group of its own, which asks a stand-in
/// hc that never answers under no deadline but the 120 s every call has.
#[test]
fn a_caller_killed_through_its_group_takes_its_hung_call_with_it() {
    use std::os::unix::process::ExitStatusExt;
    use symphony_board::sys::{Command, Stdio};
    if let Some(dir) = std::env::var_os(CALLER) {
        let asked = screen(&Binary::of(&hung_in(Path::new(&dir))), "t7");
        panic!("the hung call answered before its caller was killed: {asked:?}");
    }
    let folder = Folder::new();
    hung(&folder);
    let mut caller = Command::of(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "go::a_caller_killed_through_its_group_takes_its_hung_call_with_it",
        ])
        .env(CALLER, &folder.0)
        .process_group(0)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // The stand-in is up once it has written its pid, however long a loaded machine takes. A
    // caller that ended first gave its call up (the 120 s every call has) before it was up.
    test_time::until_true(
        "the caller asked hc, or ended",
        test_time::backstop(Duration::from_secs(110)),
        || !lines(&folder.0, "hc.pids").is_empty() || caller.try_wait().unwrap().is_some(),
    )
    .unwrap();
    assert!(
        !lines(&folder.0, "hc.pids").is_empty(),
        "the caller ended without asking hc: {:?}",
        caller.try_wait()
    );
    let group = libc::pid_t::try_from(caller.id()).unwrap();
    // SAFETY: killpg only sends a signal, to the group this test's own unreaped caller leads.
    assert_eq!(unsafe { libc::killpg(group, libc::SIGKILL) }, 0);
    assert_eq!(caller.wait().unwrap().signal(), Some(libc::SIGKILL));
    assert_eq!(lines(&folder.0, "hc.calls"), ["read t7 --lines 0"]);
    // The stand-in ended with its caller's group, and the system reaped it once orphaned.
    reaped(&folder.0, "hc.pids");
}

#[test]
fn a_call_started_once_the_deadline_is_spent_is_never_run() {
    let folder = Folder::new();
    let env = hung(&folder);
    let spent = Binary::of(&env).within(Duration::ZERO);
    assert_eq!(
        screen(&spent, "t7").unwrap_err().0,
        "hc read t7: not asked: the 0 s it is given are spent"
    );
    assert!(focus(&spent, "t7").is_err());
    assert!(lines(&folder.0, "hc.calls").is_empty());
    assert!(lines(&folder.0, "app.calls").is_empty());
}

impl World {
    /// The calls that would type into a pane: none of them may ever come from going to it.
    fn typed(&self) -> Vec<Vec<String>> {
        self.calls()
            .into_iter()
            .filter(|c| ["send", "keys", "paste", "run"].contains(&c[0].as_str()))
            .collect()
    }

    fn focused(&self) -> Value {
        self.state().get("focused").cloned().unwrap_or(Value::Null)
    }
}

#[test]
fn a_launched_agents_pane_is_shown_by_its_terminal_and_nothing_is_typed() {
    let w = World::new();
    let started = w.launch("sim/jump");
    let terminal = started.row.pane.clone();
    let (typed, before) = (w.typed(), w.calls().len());
    // Under a deadline for all its calls, as the server asks it, which a window that answers keeps.
    let focused = focus(&Binary::of(&w.env).within(ANSWERING), &terminal).unwrap();
    assert_eq!(w.focused(), json!(terminal));
    assert_eq!(
        w.calls()[before..],
        [vec![
            "app".to_string(),
            "do".into(),
            "agent.focus".into(),
            "--terminal".into(),
            terminal.clone(),
        ]]
    );
    // The stand-in is in no app bundle: nothing is brought forward, and the page says so.
    assert_eq!(focused.raised, Ok(false));
    assert!(
        focused.said().contains("bring its window forward"),
        "{}",
        focused.said()
    );
    assert_eq!(w.typed(), typed, "going to the pane typed into it");
}

#[test]
fn a_shell_is_shown_as_a_pane_and_a_gone_or_unshown_one_is_refused() {
    let w = World::new();
    let shell = w.add_terminal(false, None, json!({}));
    focus(&w.hc(), &shell).unwrap();
    assert_eq!(w.focused(), json!(shell));
    assert_eq!(w.calls_of(&["app", "do", "pane.focus"]).len(), 1);
    let refused = focus(&w.hc(), "t999").unwrap_err();
    assert!(
        refused.0.starts_with("the Herdr app did not show t999: "),
        "{refused}"
    );
    assert!(refused.0.contains("no terminal \"t999\""), "{refused}");
    let ended = w.add_terminal(true, Some("amber-heron"), json!({"state": "finished"}));
    assert!(focus(&w.hc(), &ended).is_err());
    assert_eq!(w.focused(), json!(shell));
}

#[test]
fn a_closed_window_refuses_the_focus() {
    let mut w = World::new();
    let agent = w.add_terminal(true, Some("amber-heron"), json!({}));
    w.set("FAKE_HC_FAIL", "app do");
    let refused = focus(&w.hc(), &agent).unwrap_err();
    assert!(refused.0.contains("the window did not answer"), "{refused}");
    assert_eq!(w.focused(), Value::Null);
}

#[test]
fn the_screen_is_read_from_anywhere_without_a_change_to_any_pane() {
    let w = World::new();
    let agent = w.add_terminal(
        true,
        Some("amber-heron"),
        json!({"screen": "❯ running the focused tests\n"}),
    );
    assert_eq!(
        screen(&Binary::of(&w.env).within(ANSWERING), &agent).unwrap(),
        "❯ running the focused tests\n\n"
    );
    assert_eq!(
        w.calls_of(&["read"]),
        [vec![
            "read".to_string(),
            agent.clone(),
            "--lines".into(),
            "0".into()
        ]]
    );
    assert_eq!(
        screen(&w.hc(), "t999").unwrap_err().0,
        "hc knows no terminal t999: its pane is gone"
    );
    assert!(w.changes().is_empty(), "{:?}", w.changes());
    assert_eq!(w.focused(), Value::Null);
}
