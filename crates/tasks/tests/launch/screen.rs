//! The terminal the owner may be looking at (`herdr-app status`): cleanup, finish and the idle sweep
//! never close it, and keep it when the window cannot say.

use crate::{Hooked, World};
use serde_json::{Value, json};
use std::cell::Cell;
use std::slice::from_ref;
use symphony_board::launch::hc::{Hc, Object};
use symphony_board::launch::reap::{self, REAP_SECONDS, Reap, Sweep};
use symphony_board::launch::screen::Screen;
use symphony_board::launch::{Closure, LaunchError};

/// A window showing no tab, no agent and nothing over them.
fn nothing_shown() -> Value {
    json!({"tab": null, "agent": null, "overlays": [], "floats": {}, "errors": []})
}

impl World {
    /// The window shows what `shown` says over a window showing nothing.
    fn shows(&self, shown: Value) {
        let mut screen = nothing_shown();
        for (key, value) in shown.as_object().into_iter().flatten() {
            screen[key] = value.clone();
        }
        let mut state = self.state();
        state["screen"] = screen;
        self.set_state(state);
    }

    fn tab_of(&self, terminal: &str) -> String {
        self.terminal(terminal)["labels"]["herdr.tab"]
            .as_str()
            .unwrap()
            .to_string()
    }

    /// One idle sweep through `hc`, then the timer's interval.
    fn sweep_through(&self, hc: &dyn Hc) -> Sweep {
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
        assert!(w.has_terminal(&terminal));
    }
    assert!(w.calls_of(&["kill"]).is_empty());
    w.shows(json!({"tab": "tb-elsewhere", "agent": "t-other"}));
    let [closed] = <[_; 1]>::try_from(w.clean(false, &[])).unwrap();
    assert!(closed.closed, "{closed:?}");
    assert!(!w.has_terminal(&terminal));
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
    assert!(w.has_terminal(&terminal));
    assert!(w.calls_of(&["kill"]).is_empty());
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
    assert!(w.has_terminal(&ended));
    w.shows(json!({}));
    let [closed] = <[_; 1]>::try_from(w.clean(false, &[])).unwrap();
    assert_eq!(
        (closed.why.as_str(), closed.closed),
        ("its program ended", true)
    );
    assert!(!w.has_terminal(&ended));
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
    assert!(w.has_terminal(&terminal));
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
    assert!(!w.has_terminal(&terminal));
}

#[test]
fn the_idle_sweep_keeps_a_terminal_the_owner_looks_at_or_cannot_be_told_of_right_before_its_close()
{
    let shown: [fn(&str) -> Value; 2] = [
        |terminal| json!({"agent": terminal}),
        |_| json!({"errors": ["broken"]}),
    ];
    for shown in shown {
        let w = World::new();
        let (handle, terminal) = w.worker("sim/jump", Some("done"));
        let looked = shown(&terminal);
        assert_eq!(w.sweep_through(&w.hc()).idle, from_ref(&handle));
        // The second sweep reads it twice: once as it looks, once right before the close.
        let gets = Cell::new(0);
        let mut hooked = Hooked::new(&w);
        hooked.after = Box::new(|args| {
            if args[0] == "get" {
                gets.set(gets.get() + 1);
                if gets.get() == 2 {
                    w.shows(looked.clone());
                }
            }
        });
        assert_eq!(w.sweep_through(&hooked), Sweep::default(), "{looked}");
        assert!(w.has_terminal(&terminal));
        assert!(w.calls_of(&["kill"]).is_empty());
        // Its idle wait starts again.
        w.shows(json!({}));
        assert_eq!(w.sweep_through(&w.hc()).idle, [handle]);
    }
}
