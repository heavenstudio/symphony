//! The terminal the owner may be looking at (`herdr-app status`): cleanup, finish and the idle sweep
//! never close it, and keep it when the window cannot say.

use crate::{Hooked, World};
use serde_json::{Value, json};
use std::cell::Cell;
use std::rc::Rc;
use std::slice::from_ref;
use symphony_board::launch::hc::{Hc, Object};
use symphony_board::launch::reap::{self, REAP_SECONDS, Reap, Sweep};
use symphony_board::launch::screen::{ON_SCREEN, Screen};
use symphony_board::launch::{Closure, LaunchError};

/// A window showing no tab, no agent and nothing over them.
fn nothing_shown() -> Value {
    json!({"tab": null, "agent": null, "overlays": [], "floats": {}, "errors": []})
}

impl World {
    /// The window shows what `shown` says over a window showing nothing.
    fn shows(&self, shown: Value) {
        let mut screen = nothing_shown();
        if let Some(shown) = shown.as_object() {
            for (key, value) in shown {
                screen[key] = value.clone();
            }
        }
        let mut state = self.state();
        state["screen"] = screen;
        self.set_state(state);
    }

    /// One cleanup through `hc`, closing what it would.
    pub(crate) fn clean_through(&self, hc: &dyn Hc) -> Vec<Closure> {
        let m = self.machine();
        symphony_board::launch::cleanup(self.store(), hc, &*m.processes, false, &[]).unwrap()
    }

    fn tab_of(&self, terminal: &str) -> String {
        self.terminal(terminal)["labels"]["herdr.tab"]
            .as_str()
            .unwrap()
            .to_string()
    }

    /// One idle sweep through `hc`, then the timer's interval.
    pub(crate) fn sweep_through(&self, hc: &dyn Hc) -> Sweep {
        let m = self.machine();
        let how = Reap {
            state: self.dir().join("reap.json"),
            apply: true,
            processes: &*m.processes,
            caller: None,
            min_idle: REAP_SECONDS,
        };
        let swept = reap::sweep(self.store(), hc, &how, &mut Vec::new()).unwrap();
        self.f.advance(REAP_SECONDS);
        swept.expect("no other sweep holds the lock")
    }
}

/// `terminal` of `handle`, left as it is for `why`.
fn left(terminal: &str, handle: Option<&str>, why: &str) -> Closure {
    Closure {
        pane: terminal.into(),
        handle: handle.map(str::to_string),
        why: why.into(),
        restore: String::new(),
        closed: false,
        kept: true,
    }
}

/// An hc through which the owner looks at what `looked` shows while the `nth` `hc get` waits: the
/// window shows it by the time that call answers. `gets` counts the calls.
fn looks_during_get<'w>(
    w: &'w World,
    nth: usize,
    looked: Value,
    gets: &'w Cell<usize>,
) -> Hooked<'w> {
    let mut hooked = Hooked::new(w);
    hooked.after = Box::new(move |args| {
        if args[0] == "get" {
            gets.set(gets.get() + 1);
            if gets.get() == nth {
                w.shows(looked.clone());
            }
        }
    });
    hooked
}

/// What the owner may come to look at: the terminal itself, or a window that cannot say.
/// An hc whose `nth` `hc get` finds the terminal at work again: `resume` changes it just before
/// that call answers. `gets` counts the calls.
fn resumes_at_get<'w>(
    w: &'w World,
    nth: usize,
    resume: &'w dyn Fn(&str),
    gets: &'w Cell<usize>,
) -> Hooked<'w> {
    let mut hooked = Hooked::new(w);
    hooked.instead = Box::new(move |args| {
        if args[0] == "get" {
            gets.set(gets.get() + 1);
            if gets.get() == nth {
                resume(args[1]);
            }
        }
        None
    });
    hooked
}

/// The terminal's agent goes back to work.
const WORKING: fn(&World, &str) = |w, t| {
    w.edit_terminal(t, |t| {
        t.insert("status".into(), json!("working"));
    })
};
/// A job starts under the terminal's program.
const JOB: fn(&World, &str) = |w, t| w.run_under(w.pid_of(t), &["cargo test -p x"]);

const LOOKS: [fn(&str) -> Value; 2] = [
    |terminal| json!({"agent": terminal}),
    |_| json!({"errors": ["broken"]}),
];

/// Why a terminal stays when the window shows `looked`.
fn kept_for(looked: &Value) -> String {
    if looked["errors"].is_null() {
        ON_SCREEN.into()
    } else {
        "never closed on no information: herdr-app status reports errors: broken".into()
    }
}

fn labelled(address: &str, tab: &str) -> Object {
    let terminal = json!({"address": address, "labels": {"herdr.tab": tab}});
    terminal.as_object().unwrap().clone()
}

#[test]
fn a_terminal_is_on_screen_in_the_shown_tab_as_the_shown_agent_or_held_over_them() {
    let shown = json!({
        "tab": "tb1", "agent": "t2", "overlays": [{"terminal": "t3"}],
        "floats": {"f1": {"terminal": "t4"}}, "errors": [],
    });
    let screen = Screen::of(&shown.to_string()).unwrap();
    for (address, tab, on) in [
        ("t1", "tb1", true),
        ("t2", "tb9", true),
        ("t3", "tb9", true),
        ("t4", "", true),
        ("t5", "tb9", false),
    ] {
        assert_eq!(screen.shows(&labelled(address, tab)), on, "{address}");
    }
    // A window showing no tab or agent shows no terminal of no tab, nor one with no address.
    let nothing = Screen::of(&nothing_shown().to_string()).unwrap();
    assert!(!nothing.shows(&labelled("t1", "")));
    assert!(!nothing.shows(&labelled("", "")));
}

#[test]
fn a_status_that_cannot_say_what_is_on_screen_is_an_error_naming_what_it_lacks() {
    let with = |key: &str, value: Value| {
        let mut said = nothing_shown();
        said[key] = value;
        said.to_string()
    };
    let without = |key: &str| {
        let mut said = nothing_shown();
        said.as_object_mut().unwrap().remove(key);
        said.to_string()
    };
    for (said, why) in [
        ("not json".to_string(), "printed no JSON object"),
        ("[]".to_string(), "printed no JSON object"),
        (
            with("errors", json!(["mods/x: broken", {"at": 1}])),
            r#"reports errors: mods/x: broken; {"at":1}"#,
        ),
        (without("errors"), "gives no list of errors"),
        (without("tab"), "names no tab"),
        (with("agent", json!(7)), "names no agent"),
        (without("overlays"), "gives no overlays"),
        (with("floats", json!("none")), "gives no floats"),
        (
            with("overlays", json!([{"kind": "dialog"}])),
            "does not say which terminal one of its overlays holds",
        ),
        (
            with("floats", json!({"f1": {"terminal": ""}})),
            "does not say which terminal one of its floats holds",
        ),
    ] {
        assert_eq!(
            Screen::of(&said),
            Err(LaunchError(format!("herdr-app status {why}"))),
            "{said}"
        );
    }
}

#[test]
fn cleanup_keeps_a_terminal_the_owner_is_looking_at_dry_run_or_not_and_closes_it_once_out_of_sight()
{
    let w = World::new();
    let (handle, terminal) = w.worker("sim/jump", Some("done"));
    let on_screen = left(&terminal, Some(&handle), "on screen in the Herdr app");
    for shown in [
        json!({"tab": w.tab_of(&terminal)}),
        json!({"agent": terminal}),
        json!({"overlays": [{"terminal": terminal}]}),
        json!({"floats": {"f1": {"terminal": terminal}}}),
    ] {
        w.shows(shown.clone());
        assert_eq!(w.clean(true, &[]), from_ref(&on_screen), "{shown}");
        assert_eq!(w.clean(false, &[]), from_ref(&on_screen), "{shown}");
        assert!(w.has_terminal(&terminal), "{terminal} was closed");
    }
    assert_eq!(w.calls_of(&["kill"]), Vec::<Vec<String>>::new());
    w.shows(json!({"tab": "tb-elsewhere", "agent": "t-other"}));
    let [closed] = <[_; 1]>::try_from(w.clean(false, &[])).unwrap();
    assert!(closed.closed, "{closed:?}");
    assert!(!w.has_terminal(&terminal), "{terminal} stayed");
}

#[test]
fn cleanup_keeps_a_terminal_when_the_window_cannot_say_what_it_shows() {
    let mut w = World::new();
    let (handle, terminal) = w.worker("sim/jump", Some("done"));
    let unknown = |why: &str| {
        let why = format!("never closed on no information: herdr-app status{why}");
        [left(&terminal, Some(&handle), &why)]
    };
    w.set("FAKE_HC_FAIL", "app status");
    assert_eq!(w.clean(false, &[]), unknown(": the window did not answer"));
    w.set("FAKE_HC_FAIL", "");
    w.shows(json!({"errors": ["mods/x: broken"]}));
    assert_eq!(
        w.clean(false, &[]),
        unknown(" reports errors: mods/x: broken")
    );
    assert!(w.has_terminal(&terminal), "{terminal} was closed");
    assert_eq!(w.calls_of(&["kill"]), Vec::<Vec<String>>::new());
}

#[test]
fn an_ended_terminal_on_screen_stays_until_the_owner_looks_away() {
    let w = World::new();
    let ended = w.add_terminal(false, None, json!({"state": "finished"}));
    w.shows(json!({"agent": ended}));
    assert_eq!(
        w.clean(false, &[]),
        [left(&ended, None, "on screen in the Herdr app")]
    );
    assert!(w.has_terminal(&ended), "{ended} was closed");
    w.shows(json!({}));
    let [closed] = <[_; 1]>::try_from(w.clean(false, &[])).unwrap();
    assert_eq!(
        (closed.why.as_str(), closed.closed),
        ("its program ended", true)
    );
    assert!(!w.has_terminal(&ended), "{ended} stayed");
}

#[test]
fn finish_names_the_terminal_it_leaves_on_screen() {
    let w = World::new();
    let (handle, terminal) = w.worker("sim/jump", Some("done"));
    w.shows(json!({"tab": w.tab_of(&terminal)}));
    assert_eq!(
        w.finish("sim/jump"),
        [format!(
            "kept pane {terminal} of {handle} (on screen in the Herdr app)"
        )]
    );
    assert!(w.has_terminal(&terminal), "{terminal} was closed");
    let [open] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
    assert!(open.closed.is_none());
}

#[test]
fn the_idle_sweep_never_closes_a_terminal_on_screen_and_waits_again_once_the_owner_looks_away() {
    let w = World::new();
    let (handle, terminal) = w.worker("sim/jump", Some("done"));
    w.shows(json!({"agent": terminal}));
    for _ in 0..3 {
        assert_eq!(w.sweep_through(&w.hc()), Sweep::default());
    }
    w.shows(json!({}));
    assert_eq!(w.sweep_through(&w.hc()).idle, from_ref(&handle));
    w.shows(json!({"tab": w.tab_of(&terminal)}));
    assert_eq!(w.sweep_through(&w.hc()), Sweep::default());
    // Out of sight again, it waits a whole idle wait from the start.
    w.shows(json!({}));
    assert_eq!(w.sweep_through(&w.hc()).idle, from_ref(&handle));
    assert_eq!(w.sweep_through(&w.hc()).closed.len(), 1);
    assert!(!w.has_terminal(&terminal), "{terminal} stayed");
}

#[test]
fn the_idle_sweep_keeps_a_terminal_the_owner_looks_at_or_cannot_be_told_of_right_before_its_close()
{
    for shown in LOOKS {
        let w = World::new();
        let (handle, terminal) = w.worker("sim/jump", Some("done"));
        let looked = shown(&terminal);
        assert_eq!(w.sweep_through(&w.hc()).idle, from_ref(&handle));
        // The second sweep reads it as it looks, then once more to confirm it right before the
        // close: the owner looks while that call waits.
        let gets = Cell::new(0);
        let hooked = looks_during_get(&w, 2, looked.clone(), &gets);
        assert_eq!(w.sweep_through(&hooked), Sweep::default(), "{looked}");
        assert!(w.has_terminal(&terminal), "{terminal} was closed");
        assert_eq!(w.calls_of(&["kill"]), Vec::<Vec<String>>::new());
        // Its idle wait starts again.
        w.shows(json!({}));
        assert_eq!(w.sweep_through(&w.hc()).idle, [handle]);
    }
}

#[test]
fn the_idle_sweep_keeps_a_terminal_the_owner_looks_at_after_its_closure_is_recorded_reopened() {
    for shown in LOOKS {
        let w = World::new();
        let (handle, terminal) = w.worker("sim/jump", Some("done"));
        let looked = shown(&terminal);
        assert_eq!(w.sweep_through(&w.hc()).idle, from_ref(&handle));
        // The third look is the close's own, after the closure is recorded: the last call before
        // the kill, which may wait long.
        let gets = Cell::new(0);
        let hooked = looks_during_get(&w, 3, looked.clone(), &gets);
        assert_eq!(w.sweep_through(&hooked), Sweep::default(), "{looked}");
        assert_eq!(gets.get(), 3, "{looked}");
        assert!(w.has_terminal(&terminal), "{terminal} was closed");
        assert_eq!(w.calls_of(&["kill"]), Vec::<Vec<String>>::new());
        // The record it wrote is open again, on the same terminal.
        let [open] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
        assert_eq!((open.closed, open.pane.as_str()), (None, terminal.as_str()));
        // Its idle wait starts again; out of sight, it closes after a whole one.
        w.shows(json!({}));
        assert_eq!(w.sweep_through(&w.hc()).idle, from_ref(&handle));
        assert_eq!(w.sweep_through(&w.hc()).closed.len(), 1);
        assert!(!w.has_terminal(&terminal), "{terminal} stayed");
    }
}

#[test]
fn cleanup_keeps_a_terminal_the_owner_looks_at_while_the_close_looks_it_up_ended_or_idle() {
    for (shown, ended) in LOOKS.into_iter().flat_map(|s| [(s, false), (s, true)]) {
        let w = World::new();
        let (handle, terminal) = if ended {
            (
                None,
                w.add_terminal(false, None, json!({"state": "finished"})),
            )
        } else {
            let (handle, terminal) = w.worker("sim/jump", Some("done"));
            (Some(handle), terminal)
        };
        let looked = shown(&terminal);
        // The first look is cleanup's own; the second the close's, right before the kill.
        let gets = Cell::new(0);
        let closures = w.clean_through(&looks_during_get(&w, 2, looked.clone(), &gets));
        let kept = left(&terminal, handle.as_deref(), &kept_for(&looked));
        assert_eq!(closures, [kept], "{looked} ended {ended}");
        assert!(w.has_terminal(&terminal), "{terminal} was closed");
        assert_eq!(w.calls_of(&["kill"]), Vec::<Vec<String>>::new());
        let sessions = w.store().task_sessions("sim/jump").unwrap();
        assert!(sessions.iter().all(|s| s.closed.is_none()), "{sessions:?}");
    }
}

#[test]
fn cleanup_keeps_a_terminal_whose_last_look_hc_cannot_answer() {
    let w = World::new();
    let (handle, terminal) = w.worker("sim/jump", Some("done"));
    let gets = Cell::new(0);
    let hooked = Hooked {
        instead: Box::new(|args| {
            if args[0] != "get" {
                return None;
            }
            gets.set(gets.get() + 1);
            (gets.get() == 2).then(|| Err(LaunchError(format!("hc get {}: no answer", args[1]))))
        }),
        ..Hooked::new(&w)
    };
    let why = format!("never closed on no information: hc get {terminal}: no answer");
    assert_eq!(
        w.clean_through(&hooked),
        [left(&terminal, Some(&handle), &why)]
    );
    assert!(w.has_terminal(&terminal), "{terminal} was closed");
    assert_eq!(w.calls_of(&["kill"]), Vec::<Vec<String>>::new());
}

#[test]
fn a_terminal_gone_before_the_close_looks_it_up_is_closed_already_with_no_kill() {
    let w = World::new();
    let ended = w.add_terminal(false, None, json!({"state": "finished"}));
    let mut hooked = Hooked::new(&w);
    hooked.after = Box::new(|args| {
        if args[0] == "get" {
            w.edit_terminals(|all| all.clear());
        }
    });
    let [closed] = <[_; 1]>::try_from(w.clean_through(&hooked)).unwrap();
    assert_eq!(
        (closed.pane.as_str(), closed.closed, closed.kept),
        (ended.as_str(), true, false)
    );
    assert_eq!(w.calls_of(&["get", &ended]).len(), 2);
    assert_eq!(w.calls_of(&["kill"]), Vec::<Vec<String>>::new());
    assert_eq!(w.calls_of(&["rm"]), Vec::<Vec<String>>::new());
}

// -- work that resumes before the last look ----------------------------------------------------

#[test]
fn cleanup_keeps_a_terminal_its_last_get_finds_at_work_again_agent_shell_or_ended() {
    let runs_again: fn(&World, &str) = |w, t| {
        w.edit_terminal(t, |t| {
            t.insert("state".into(), json!("live"));
        })
    };
    let job = "work started under it: runs cargo test -p x";
    for (kind, resume, why) in [
        ("agent", WORKING, "its agent is working now"),
        ("agent", JOB, job),
        ("shell", JOB, job),
        ("ended", runs_again, "its program runs again"),
    ] {
        let w = World::new();
        let (handle, terminal) = match kind {
            "agent" => {
                let (handle, terminal) = w.worker("sim/jump", Some("done"));
                (Some(handle), terminal)
            }
            "shell" => (None, w.add_terminal(false, None, json!({}))),
            _ => (
                None,
                w.add_terminal(false, None, json!({"state": "finished"})),
            ),
        };
        // The first get is cleanup's own look; the second, the close's, finds the work.
        let gets = Cell::new(0);
        let resume = |t: &str| resume(&w, t);
        let closures = w.clean_through(&resumes_at_get(&w, 2, &resume, &gets));
        let kept = left(&terminal, handle.as_deref(), why);
        assert_eq!(closures, [kept], "{kind}: {why}");
        assert_eq!(gets.get(), 2, "{kind}: {why}");
        assert!(w.has_terminal(&terminal), "{terminal} was closed");
        assert_eq!(w.calls_of(&["kill"]), Vec::<Vec<String>>::new());
        // Work found, the window is not asked again: only cleanup's own look read it.
        assert_eq!(w.calls_of(&["app", "status"]).len(), 1, "{kind}: {why}");
        let sessions = w.store().task_sessions("sim/jump").unwrap();
        assert!(sessions.iter().all(|s| s.closed.is_none()), "{sessions:?}");
    }
}

#[test]
fn the_idle_sweep_keeps_a_terminal_its_last_get_finds_at_work_again_and_reopens_its_record() {
    for resume in [WORKING, JOB] {
        let w = World::new();
        let (handle, terminal) = w.worker("sim/jump", Some("done"));
        assert_eq!(w.sweep_through(&w.hc()).idle, from_ref(&handle));
        // The third get is the close's own, after the closure is recorded.
        let gets = Cell::new(0);
        let resume = |t: &str| resume(&w, t);
        let hooked = resumes_at_get(&w, 3, &resume, &gets);
        assert_eq!(w.sweep_through(&hooked), Sweep::default());
        assert_eq!(gets.get(), 3);
        assert!(w.has_terminal(&terminal), "{terminal} was closed");
        assert_eq!(w.calls_of(&["kill"]), Vec::<Vec<String>>::new());
        let [open] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
        assert_eq!((open.closed, open.pane.as_str()), (None, terminal.as_str()));
        // Idle again, it waits a whole idle wait from the start before it closes.
        w.edit_terminal(&terminal, |t| {
            t.insert("status".into(), json!("idle"));
        });
        w.run_under(w.pid_of(&terminal), &[]);
        assert_eq!(w.sweep_through(&w.hc()).idle, from_ref(&handle));
        assert_eq!(w.sweep_through(&w.hc()).closed.len(), 1);
        assert!(!w.has_terminal(&terminal), "{terminal} stayed");
    }
}

#[test]
fn cleanup_keeps_a_terminal_whose_last_look_cannot_read_its_processes_or_finds_no_pid() {
    let w = World::new();
    let (handle, terminal) = w.worker("sim/jump", Some("done"));
    // The ps table answers cleanup's own look, then fails the close's.
    let tables = Rc::new(Cell::new(0));
    let read = Rc::clone(&tables);
    let processes = move |_: i64| {
        read.set(read.get() + 1);
        if read.get() == 1 {
            Ok(Vec::new())
        } else {
            Err(LaunchError("ps: no answer".into()))
        }
    };
    let closures = symphony_board::launch::cleanup(w.store(), &w.hc(), &processes, false, &[]);
    let why = "never closed on no information: ps: no answer";
    assert_eq!(closures.unwrap(), [left(&terminal, Some(&handle), why)]);
    assert_eq!(tables.get(), 2);
    // hc knows no pid for it by the close's look.
    let gets = Cell::new(0);
    let no_pid = |t: &str| {
        w.edit_terminal(t, |t| {
            t.insert("pid".into(), Value::Null);
        })
    };
    let closures = w.clean_through(&resumes_at_get(&w, 2, &no_pid, &gets));
    let why = "hc knows no pid for it now";
    assert_eq!(closures, [left(&terminal, Some(&handle), why)]);
    assert!(w.has_terminal(&terminal), "{terminal} was closed");
    assert_eq!(w.calls_of(&["kill"]), Vec::<Vec<String>>::new());
}

#[test]
fn the_idle_sweep_keeps_a_terminal_running_another_program_by_its_last_look_and_closes_an_ended_one()
 {
    let another: fn(&World, &str) = |w, t| {
        let pid = w.new_pid();
        w.edit_terminal(t, |t| {
            t.insert("pid".into(), json!(pid));
        })
    };
    let ended: fn(&World, &str) = |w, t| {
        w.edit_terminal(t, |t| {
            t.insert("state".into(), json!("finished"));
        })
    };
    for (change, closes) in [(another, false), (ended, true)] {
        let w = World::new();
        let (handle, terminal) = w.worker("sim/jump", Some("done"));
        assert_eq!(w.sweep_through(&w.hc()).idle, from_ref(&handle));
        let gets = Cell::new(0);
        let change = |t: &str| change(&w, t);
        let swept = w.sweep_through(&resumes_at_get(&w, 3, &change, &gets));
        assert_eq!(swept.closed.len(), usize::from(closes), "{swept:?}");
        assert_eq!(w.has_terminal(&terminal), !closes, "{terminal}");
        // An ended program has nothing to kill: it is only removed.
        assert_eq!(w.calls_of(&["kill"]), Vec::<Vec<String>>::new());
        let [record] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
        assert_eq!(record.closed.is_some(), closes, "{record:?}");
    }
}
