//! Words held for people as the command line holds to them (`symphony_board::held`): an agent's
//! `show`, its JSON, `inbox` and `wait` never carry a held message's words, and `show` says how
//! many are held; `pass SLUG ID` puts them on the thread under their author's name, the passer
//! first, where the task's agents read them once, and a second pass is refused with nothing
//! written.

use crate::Case;
use symphony_board::held::{Held, Said};
use symphony_board::people::Power;

/// The words the case holds, which no agent's read may carry.
const WORDS: &str = "the phone picture cuts the header off";

/// A board where `tools/x` is claimed by a worker, with `ana`, who may speak and not direct agents,
/// and her message held there: the case, the worker, the orchestrator and the held message.
fn held() -> (Case, String, String, Held) {
    let c = Case::new();
    let orchestrator = c.register("orchestrator");
    let worker = c.register("worker");
    c.ok(&orchestrator, &["create", "tools/x", "X"]);
    c.ok(&worker, &["claim", "tools/x"]);
    let store = c.store();
    store
        .add_person("ana", &[], &[Power::Speak], &orchestrator)
        .unwrap();
    c.ok(&worker, &["inbox"]);
    let said = store
        .for_person("ana")
        .say_or_hold_as("tools/x", "ana", WORDS, None);
    let Ok(Said::Held(held)) = said else {
        panic!("not held: {said:?}")
    };
    (c, worker, orchestrator, held)
}

#[test]
fn an_agents_reads_carry_how_many_are_held_and_nothing_of_their_words() {
    let (c, worker, orchestrator, held) = held();
    let marker = format!("a message is held for people: #{}", held.id);
    let shown = c.ok(&worker, &["show", "tools/x"]);
    assert!(
        shown.contains("\n  held for people: 1 message\n"),
        "{shown}"
    );
    assert!(shown.contains(&marker), "{shown}");
    let json = c.ok(&worker, &["show", "tools/x", "--json"]);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["held"], 1);
    for read in [
        shown,
        json,
        c.ok(&worker, &["inbox"]),
        c.ok(&orchestrator, &["inbox"]),
        c.ok(&worker, &["wait", "--timeout", "0"]),
        c.ok(&orchestrator, &["wait", "--timeout", "0"]),
        c.ok(&worker, &["search", "phone picture"]),
    ] {
        assert!(!read.contains(WORDS), "{read}");
        assert!(!read.contains("ana"), "{read}");
    }
    // A second held message is counted as more than one.
    let ana = c.store().for_person("ana");
    ana.say_or_hold_as("tools/x", "ana", "and the footer", None)
        .unwrap();
    let shown = c.ok(&worker, &["show", "tools/x"]);
    assert!(
        shown.contains("\n  held for people: 2 messages\n"),
        "{shown}"
    );
}

#[test]
fn a_pass_puts_the_words_before_the_tasks_agents_once_and_a_second_is_refused() {
    let (c, worker, orchestrator, held) = held();
    let id = held.id.to_string();
    let passed = c.ok(&orchestrator, &["pass", "tools/x", &id]);
    let line = c.store().thread_rows("tools/x").unwrap().last().unwrap().id;
    assert_eq!(passed, format!("passed on #{id} on tools/x as #{line}\n"));
    let text = format!("[passed on by {orchestrator}] {WORDS}");
    let shown = c.ok(&worker, &["show", "tools/x"]);
    assert!(!shown.contains("\n  held for people:"), "{shown}");
    assert!(shown.contains(&format!("ana (msg): {text}")), "{shown}");
    let json = c.ok(&worker, &["show", "tools/x", "--json"]);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["held"], 0);
    // The executor reads it once.
    let heard = c.ok(&worker, &["wait", "--timeout", "0"]);
    assert_eq!(heard.matches(&text).count(), 1, "{heard}");
    let again = c.ok(&worker, &["inbox"]);
    assert!(!again.contains(WORDS), "{again}");

    let refused = c.run_as(&worker, &["pass", "tools/x", &id], "");
    assert_eq!(refused.code, 1, "{}", refused.out);
    assert!(
        refused.err.contains(&format!(
            "passed on already, by {orchestrator}: nothing was written"
        )),
        "{}",
        refused.err
    );
    for (args, said) in [
        (
            vec!["pass", "tools/x", "x1"],
            "\"x1\" is no message's number",
        ),
        (
            vec!["pass", "tools/x", "999999"],
            "no message #999999 is held on tools/x",
        ),
    ] {
        let ran = c.run_as(&worker, &args, "");
        assert_eq!(ran.code, 1, "{args:?}: {}", ran.out);
        assert!(ran.err.contains(said), "{args:?}: {}", ran.err);
    }
    let lines = c.store().thread_rows("tools/x").unwrap();
    assert_eq!(lines.last().unwrap().id, line, "nothing more was written");
}
