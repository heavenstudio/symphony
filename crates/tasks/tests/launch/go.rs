//! Going to a launched agent's session through the fake hc: the Herdr app shows its pane, or its
//! screen is read, and nothing is ever typed into it.

use crate::{Folder, World};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use symphony_board::cli::Env;
use symphony_board::launch::go::{DEADLINE, asked, focus, screen};
use symphony_board::launch::hc::Binary;
use symphony_board::sys::pid_alive;

/// A stand-in for hc or the Herdr app that never answers: each call adds its words to `NAME.calls`
/// and its pid to `NAME.pids`, then becomes a `sleep` that outlasts any wait, in the same process,
/// so the pid written is the one a kill must end.
fn never_answering(dir: &Path, name: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    let (calls, pids) = (
        dir.join(format!("{name}.calls")),
        dir.join(format!("{name}.pids")),
    );
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\necho $$ >> '{}'\nexec sleep 600\n",
            calls.display(),
            pids.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// The lines a stand-in wrote to `dir/FILE`, none when it wrote none.
fn lines(dir: &Path, file: &str) -> Vec<String> {
    std::fs::read_to_string(dir.join(file))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// An environment whose hc and Herdr app are stand-ins that never answer.
fn hung(folder: &Folder) -> Env {
    let mut env = Env {
        vars: Default::default(),
        cwd: folder.0.clone(),
    };
    for (key, value) in [
        ("PATH", std::env::var("PATH").unwrap_or_default()),
        (
            "HC_BIN",
            never_answering(&folder.0, "hc").display().to_string(),
        ),
        (
            "HERDR_APP_BIN",
            never_answering(&folder.0, "app").display().to_string(),
        ),
    ] {
        env.vars.insert(key.into(), value);
    }
    env
}

/// Every pid a stand-in wrote to `dir/FILE` ends and is reaped (a zombie still answers a signal)
/// within a few seconds of the call that started it giving up.
fn reaped(dir: &Path, file: &str) {
    let pids = lines(dir, file);
    assert!(!pids.is_empty(), "{file}: no call was made");
    for pid in pids {
        let pid: i64 = pid.parse().unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        while pid_alive(pid) {
            assert!(
                Instant::now() < until,
                "{file}: {pid} was never killed and reaped"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

#[test]
fn a_hung_window_or_hc_answers_within_the_deadline_all_calls_together_and_its_child_is_reaped() {
    let folder = Folder::new();
    let env = hung(&folder);
    // The page gives up after 15 s; a focus and a screen read have ended well before.
    assert!(DEADLINE < Duration::from_secs(10), "{DEADLINE:?}");
    let slack = Duration::from_secs(3);
    let (focused, read) = std::thread::scope(|s| {
        let focused = s.spawn(|| {
            let started = Instant::now();
            (focus(&asked(&env), "t7"), started.elapsed())
        });
        let read = s.spawn(|| {
            let started = Instant::now();
            (screen(&asked(&env), "t7"), started.elapsed())
        });
        (focused.join().unwrap(), read.join().unwrap())
    });
    let given = DEADLINE.as_secs();
    let (refused, took) = (focused.0.unwrap_err(), focused.1);
    assert!(took < DEADLINE + slack, "the focus took {took:?}");
    assert_eq!(
        refused.0,
        format!(
            "the Herdr app did not show t7: herdr-app do agent.focus: no answer within the {given} s \
             it is given; herdr-app do pane.focus: not asked: the {given} s it is given are spent"
        )
    );
    // One call took the whole deadline: the pane's focus and the raise were never started.
    assert_eq!(
        lines(&folder.0, "app.calls"),
        ["do agent.focus --terminal t7"]
    );
    let (refused, took) = (read.0.unwrap_err(), read.1);
    assert!(took < DEADLINE + slack, "the screen read took {took:?}");
    assert_eq!(
        refused.0,
        format!("hc read t7: no answer within the {given} s it is given")
    );
    assert_eq!(lines(&folder.0, "hc.calls"), ["read t7 --lines 0"]);
    reaped(&folder.0, "app.pids");
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
    // As the server asks it: every call within the deadline, which a window that answers keeps.
    let focused = focus(&asked(&w.env), &terminal).unwrap();
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
        screen(&asked(&w.env), &agent).unwrap(),
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
