//! An agent turned off (`[agents.launch.on]`) or whose program the machine lacks starts nowhere: a
//! launch naming it, a restore of its closed pane and a reviewer's resume are each refused before
//! anything is made, naming the switch's place; the other agent starts, an agent already running
//! keeps running, and a project that writes no switch launches as before.

use crate::common::refused;
use crate::{PROGRAMS, World, programs};
use std::fs;
use std::path::PathBuf;
use symphony_board::NewLaunch;
use symphony_board::launch::{Ask, Machine, restore};
use symphony_board::rows::TaskSessionRow;

/// The project names claude its launch default, so codex may be turned off.
const CLAUDE: &str = "[agents.launch]\ndefault = \"claude\"\n";

/// The three places a switch is written, each with what a refusal names it by.
#[derive(Clone, Copy, Debug)]
enum Place {
    Committed,
    Store,
    Local,
}

const PLACES: [Place; 3] = [Place::Committed, Place::Store, Place::Local];

impl Place {
    /// Codex turned off here in `w`, whose project names claude its default; what a refusal says
    /// of where.
    fn turn_off(self, w: &World) -> &'static str {
        let file = w.repo.join("symphony.toml");
        match self {
            Place::Committed => {
                fs::write(
                    &file,
                    format!("{CLAUDE}[agents.launch.on]\ncodex = false\n"),
                )
                .unwrap();
                "symphony.toml [agents.launch.on] codex, line 4"
            }
            Place::Store => {
                fs::write(&file, CLAUDE).unwrap();
                w.store()
                    .store_setting(
                        "agents.launch.on.codex",
                        &toml::Value::Boolean(false),
                        "amber-heron",
                        &file,
                    )
                    .unwrap();
                "the board's store [agents.launch.on] codex"
            }
            Place::Local => {
                fs::write(&file, CLAUDE).unwrap();
                fs::write(
                    w.repo.join("symphony.local.toml"),
                    "[agents.launch.on]\ncodex = false\n",
                )
                .unwrap();
                "symphony.local.toml [agents.launch.on] codex, line 2"
            }
        }
    }
}

/// What a refusal of codex turned off at `said` says after its prefix.
fn off(said: &str) -> String {
    format!(
        "codex is turned off ({said}): the board starts none while it is, and turning it on again \
         is that switch alone"
    )
}

fn codex(w: &World) -> Ask {
    Ask {
        kind: Some("codex".into()),
        ..w.ask("sim/jump")
    }
}

/// A Codex executor of sim/jump, recorded with its thread, its terminal closed and the closure
/// recorded: what a restore starts again.
fn closed_codex(w: &World) -> TaskSessionRow {
    let s = w.try_launch(&codex(w)).unwrap().row;
    let s = w
        .store()
        .record_launch(&NewLaunch {
            claude_session: "codex:0abc-def".into(),
            worktree: s.worktree,
            branch: s.branch,
            pane: s.pane,
            model: "gpt".into(),
            ..NewLaunch::new("sim/jump", &s.handle, &s.role)
        })
        .unwrap();
    use symphony_board::launch::hc::Hc as _;
    w.hc().close(&s.pane).unwrap();
    w.store()
        .close_task_session(&s.slug, &s.handle, "idle", "restore line")
        .unwrap();
    s
}

#[test]
fn a_launch_naming_an_agent_turned_off_is_refused_from_each_place_with_nothing_made() {
    for place in PLACES {
        let w = World::new();
        let said = place.turn_off(&w);
        let why = refused(w.try_launch(&codex(&w)));
        assert_eq!(why, format!("no launch: {}", off(said)), "{place:?}");
        assert!(w.calls_of(&["run"]).is_empty(), "{place:?}: no terminal");
        assert!(!w.wt.join("sim-jump").exists(), "{place:?}: no worktree");
        assert!(
            w.store().task_sessions("sim/jump").unwrap().is_empty(),
            "{place:?}: no record"
        );
        // The other agent, the launch default, starts.
        let started = w
            .try_launch(&Ask {
                kind: Some("claude".into()),
                ..w.ask("sim/jump")
            })
            .unwrap();
        assert_eq!(started.row.role, "executor", "{place:?}");
    }
}

#[test]
fn turning_it_on_again_is_the_switch_alone_and_the_next_launch_starts() {
    let w = World::new();
    Place::Store.turn_off(&w);
    assert!(refused(w.try_launch(&codex(&w))).starts_with("no launch: codex is turned off"));
    w.store()
        .store_setting(
            "agents.launch.on.codex",
            &toml::Value::Boolean(true),
            "amber-heron",
            &w.repo.join("symphony.toml"),
        )
        .unwrap();
    w.try_launch(&codex(&w)).expect("on again, it starts");
}

#[test]
fn a_restore_of_a_closed_pane_of_an_agent_turned_off_is_refused_and_its_record_stays_closed() {
    for place in PLACES {
        let w = World::new();
        let s = closed_codex(&w);
        let runs = w.calls_of(&["run"]).len();
        let said = place.turn_off(&w);
        let why = refused(restore(
            w.store(),
            &w.hc(),
            &w.machine(),
            "sim/jump",
            None,
            None,
        ));
        assert_eq!(
            why,
            format!("no restore of {}: {}", s.handle, off(said)),
            "{place:?}"
        );
        assert_eq!(w.calls_of(&["run"]).len(), runs, "{place:?}: no terminal");
        let [record] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
        assert!(record.closed.is_some(), "{place:?}: still closed");
    }
}

#[test]
fn a_reviewer_launch_resuming_a_codex_reviewer_turned_off_is_refused() {
    let w = World::new();
    let s = w
        .try_launch(&Ask {
            role: "reviewer".into(),
            kind: Some("codex".into()),
            ..w.ask("sim/jump")
        })
        .unwrap()
        .row;
    let s = w
        .store()
        .record_launch(&NewLaunch {
            claude_session: "codex:0abc-review".into(),
            worktree: s.worktree,
            pane: s.pane,
            ..NewLaunch::new("sim/jump", &s.handle, "reviewer")
        })
        .unwrap();
    use symphony_board::launch::hc::Hc as _;
    w.hc().close(&s.pane).unwrap();
    w.store()
        .close_task_session(&s.slug, &s.handle, "idle", "restore line")
        .unwrap();
    let said = Place::Store.turn_off(&w);
    // The launch names claude, yet it would resume the recorded Codex reviewer: refused.
    let why = refused(w.try_launch(&Ask {
        role: "reviewer".into(),
        kind: Some("claude".into()),
        ..w.ask("sim/jump")
    }));
    assert_eq!(why, format!("no restore of {}: {}", s.handle, off(said)));
}

#[test]
fn an_agent_already_running_keeps_running_when_it_is_turned_off() {
    let w = World::new();
    let s = w.try_launch(&codex(&w)).unwrap().row;
    Place::Store.turn_off(&w);
    assert!(refused(w.try_launch(&codex(&w))).starts_with("no launch: codex is turned off"));
    assert_eq!(
        w.live(),
        std::slice::from_ref(&s.pane),
        "its terminal runs on"
    );
    let [record] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
    assert_eq!((record.closed, record.pane), (None, s.pane));
}

/// `w`'s machine, its search path a folder holding `names` alone.
fn finding(w: &World, names: &[&str]) -> Machine {
    let folder: PathBuf = w.dir().join(format!("only-{}", names.join("-")));
    programs(&folder, names);
    Machine {
        path: Some(folder.into_os_string()),
        ..w.machine()
    }
}

#[test]
fn an_agent_whose_program_this_machine_lacks_is_refused_with_that_reason_while_on() {
    let w = World::new();
    assert!(w.dir().join(PROGRAMS).join("codex").exists());
    let m = finding(&w, &["claude"]);
    let why = refused(symphony_board::launch::launch(
        w.store(),
        &w.hc(),
        &m,
        &codex(&w),
    ));
    assert_eq!(
        why,
        "no launch: this machine has no codex program on its search path, so it cannot start here"
    );
    assert!(w.calls_of(&["run"]).is_empty());
    // A restore of a closed pane of it, the same.
    let s = closed_codex(&w);
    let why = refused(restore(w.store(), &w.hc(), &m, "sim/jump", None, None));
    assert_eq!(
        why,
        format!(
            "no restore of {}: this machine has no codex program on its search path, so it \
             cannot start here",
            s.handle
        )
    );
}

#[test]
fn a_project_that_writes_no_switch_launches_either_agent_as_before() {
    let w = World::new();
    w.task("sim/fast", None);
    w.try_launch(&codex(&w)).expect("codex starts");
    w.try_launch(&Ask {
        kind: Some("claude".into()),
        ..w.ask("sim/fast")
    })
    .expect("claude starts");
}

#[test]
fn the_sessions_each_agent_runs_are_counted_from_the_board_and_a_closed_one_is_not() {
    let w = World::new();
    let count = || symphony_board::launch::running::by_agent(w.store()).unwrap();
    assert_eq!(count(), [0, 0]);
    w.task("sim/fast", None);
    let codex_one = w.try_launch(&codex(&w)).unwrap().row;
    w.try_launch(&Ask {
        kind: Some("claude".into()),
        ..w.ask("sim/fast")
    })
    .unwrap();
    assert_eq!(count(), [1, 1], "codex, then claude");
    // A pane closed and its closure recorded is no session running.
    use symphony_board::launch::hc::Hc as _;
    w.hc().close(&codex_one.pane).unwrap();
    w.store()
        .close_task_session(&codex_one.slug, &codex_one.handle, "idle", "closed")
        .unwrap();
    assert_eq!(count(), [0, 1]);
}
