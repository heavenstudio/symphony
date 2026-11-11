//! Going to a launched agent's session through the fake hc: the Herdr app shows its pane, or its
//! screen is read, and nothing is ever typed into it.

use crate::World;
use serde_json::{Value, json};
use symphony_board::launch::go::{focus, screen};

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
    let focused = focus(&w.hc(), &terminal).unwrap();
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
        screen(&w.hc(), &agent).unwrap(),
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
