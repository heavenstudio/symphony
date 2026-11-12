//! `just tasks asks`: the owner's open asks, oldest first, each with its age and holder; `--all` adds
//! those answered in the last day; `--json` says them as the board holds them; `asks close ID WHY`
//! closes one by hand. And the `tasks api` methods the Symphony opens, holds and answers them by.

use crate::common::NOW;
use crate::{Case, run};
use serde_json::{Value, json};
use symphony_board::NewAsk;

fn chat(thread: &str, words: &str, at: f64) -> NewAsk {
    NewAsk {
        kind: "chat".into(),
        thread: Some(thread.into()),
        words: words.into(),
        at: Some(at),
        ..NewAsk::default()
    }
}

/// A board holding three asks: a verdict asked 15 minutes ago and held by `holder` from now, a chat
/// asked now, and a chat asked an hour ago that `holder` answered now.
fn three_asks(c: &Case, holder: &str) {
    let store = c.store();
    store
        .open_ask(&NewAsk {
            kind: "verdict".into(),
            path: Some("docs/DECISIONS.md".into()),
            card: Some("owner-asks".into()),
            words: "why not B?".into(),
            at: Some(NOW - 900.0),
            ..NewAsk::default()
        })
        .unwrap();
    store.hold_ask(1, Some(holder), "first").unwrap();
    store.open_ask(&chat("7", "hello?", NOW)).unwrap();
    store.open_ask(&chat("8", "done?", NOW - 3600.0)).unwrap();
    store.answer_ask(3, Some(holder), "yes").unwrap();
}

#[test]
fn with_no_open_ask_it_says_so() {
    let c = Case::new();
    assert_eq!(c.ok("", &["asks"]), "no open asks\n");
    let json: Value = serde_json::from_str(&c.ok("", &["asks", "--json"])).unwrap();
    assert_eq!(json, json!({"asks": []}));
}

#[test]
fn the_open_asks_are_listed_oldest_first_and_all_adds_the_answered() {
    let c = Case::new();
    let amber = c.register("worker");
    three_asks(&c, &amber);
    let open = format!(
        "ask 1  verdict  owner-asks on docs/DECISIONS.md  15 min, late  held by {amber} for 0 min\n  \"why not B?\"\n\
         ask 2  chat  chat 7  0 min  held by nobody\n  \"hello?\"\n"
    );
    assert_eq!(c.ok("", &["asks"]), open);
    assert_eq!(
        c.ok("", &["asks", "--all"]),
        format!("{open}ask 3  chat  chat 8  answered by {amber} after 1 h: yes\n  \"done?\"\n")
    );
    // Every ask answered: the answered ones, all at the same time here, the newest number first,
    // follow the line that says none is open.
    c.store().answer_ask(1, None, "done").unwrap();
    c.store().answer_ask(2, None, "hi").unwrap();
    let all = c.ok("", &["asks", "--all"]);
    let heads: Vec<&str> = all.lines().filter(|l| !l.starts_with("  ")).collect();
    assert_eq!(
        heads,
        [
            "no open asks".to_string(),
            format!("ask 3  chat  chat 8  answered by {amber} after 1 h: yes"),
            "ask 2  chat  chat 7  answered after 0 min: hi".to_string(),
            "ask 1  verdict  owner-asks on docs/DECISIONS.md  answered after 15 min: done"
                .to_string(),
        ]
    );
}

#[test]
fn json_says_each_ask_as_the_board_holds_it_with_its_age() {
    let c = Case::new();
    let amber = c.register("worker");
    three_asks(&c, &amber);
    let json: Value = serde_json::from_str(&c.ok("", &["asks", "--json", "--all"])).unwrap();
    let asks = json["asks"].as_array().unwrap();
    assert_eq!(
        asks.iter().map(|a| a["id"].clone()).collect::<Vec<_>>(),
        [json!(1), json!(2), json!(3)]
    );
    assert_eq!(
        asks[0],
        json!({
            "id": 1, "kind": "verdict", "path": "docs/DECISIONS.md", "card": "owner-asks",
            "block": null, "tree": null, "task": null, "thread": null, "words": "why not B?",
            "asked": NOW - 900.0, "lines": 1, "last": NOW - 900.0, "holder": amber, "held": NOW,
            "before": [], "open": true, "answered": null, "answered_by": null, "answer": null,
            "closed": false, "gone": null, "gone_since": null, "age": 900.0, "late": true,
        })
    );
    assert_eq!(
        (&asks[2]["late"], &asks[2]["age"], &asks[2]["answered_by"]),
        (&json!(false), &json!(3600.0), &json!(amber))
    );
}

#[test]
fn a_close_by_hand_names_the_ask_says_why_and_needs_a_session() {
    let c = Case::new();
    let amber = c.register("worker");
    three_asks(&c, &amber);
    let none = c.run_as("", &["asks", "close", "2", "a greeting"], "");
    assert_eq!(
        (none.code, none.err),
        (
            2,
            format!(
                "✖ no session: pass --as HANDLE or set TASKS_SESSION (get one with `{} register`)\n",
                c.board()
            )
        )
    );
    let ran = c.run_as(&amber, &["asks", "close", "#2", "a greeting"], "");
    assert_eq!(
        (ran.code, ran.out.as_str(), ran.err.as_str()),
        (0, "closed ask 2: a greeting\n", "")
    );
    let closed = c.store().ask(2).unwrap();
    assert_eq!(
        (closed.open, closed.closed, closed.answered_by),
        (false, true, Some(amber.clone()))
    );
    for (args, said) in [
        (
            &["asks", "close", "2", "again"][..],
            "✖ ask 2 is answered already\n",
        ),
        (
            &["asks", "close", "3", "late"][..],
            "✖ ask 3 is answered already\n",
        ),
        (&["asks", "close", "9", "none"][..], "✖ no ask 9\n"),
        (
            &["asks", "close", "two", "x"][..],
            "✖ name the ask by its number: `just tasks asks close ID \"WHY\"`, not 'two'\n",
        ),
        (
            &["asks", "close", "1"][..],
            "✖ say why ask 1 needs no answer: `just tasks asks close 1 \"WHY\"`\n",
        ),
    ] {
        let ran = c.run_as(&amber, args, "");
        assert_eq!(
            (ran.code, ran.out.as_str(), ran.err.as_str()),
            (1, "", said),
            "{args:?}"
        );
    }
    assert!(
        c.store().ask(1).unwrap().open,
        "the refusals closed nothing"
    );
    let bare = c.run_as(&amber, &["asks", "1"], "");
    assert_eq!(
        (bare.code, bare.err.lines().last()),
        (
            2,
            Some(
                "just tasks asks: error: argument action: invalid choice: '1' (choose from 'close')"
            )
        )
    );
}

/// `tasks api` answers `call`.
fn api(c: &Case, call: Value) -> Value {
    let ran = run(&c.env, &["api"], &call.to_string());
    assert_eq!(ran.code, 0, "{}", ran.err);
    serde_json::from_str(&ran.out).unwrap()
}

#[test]
fn the_api_opens_holds_answers_and_closes_asks() {
    let c = Case::new();
    let amber = c.register("worker");
    c.ok(&amber, &["create", "sim/jump", "Jump"]);
    let opened = api(
        &c,
        json!({"method": "open_ask", "args": {"kind": "task", "task": "sim/jump", "words": "status?"}}),
    );
    assert_eq!(
        (
            &opened["ok"]["id"],
            &opened["ok"]["kind"],
            &opened["ok"]["asked"],
            &opened["ok"]["late"]
        ),
        (&json!(1), &json!("task"), &json!(NOW), &json!(false))
    );
    let joined = api(
        &c,
        json!({"method": "open_ask", "args": {"kind": "task", "task": "sim/jump", "words": "and?", "at": NOW + 5.0}}),
    );
    assert_eq!(
        (&joined["ok"]["id"], &joined["ok"]["lines"]),
        (&json!(1), &json!(2))
    );
    let held = api(
        &c,
        json!({"method": "hold_ask", "args": {"id": 1, "holder": amber, "why": "first"}}),
    );
    assert_eq!(held["ok"]["holder"], json!(amber));
    let nobody = api(
        &c,
        json!({"method": "hold_ask", "args": {"id": 1, "holder": null, "why": "left"}}),
    );
    assert_eq!(
        (
            &nobody["ok"]["holder"],
            &nobody["ok"]["before"][0]["handle"]
        ),
        (&Value::Null, &json!(amber))
    );
    let answered = api(
        &c,
        json!({"method": "answer_about", "args": {"kind": "task", "task": "sim/jump", "by": amber, "answer": "on the thread"}}),
    );
    assert_eq!(
        (&answered["ok"]["open"], &answered["ok"]["answer"]),
        (&json!(false), &json!("on the thread"))
    );
    assert_eq!(
        api(
            &c,
            json!({"method": "answer_about", "args": {"kind": "task", "task": "sim/jump", "answer": "x"}})
        ),
        json!({"ok": null}),
        "none is open"
    );
    api(
        &c,
        json!({"method": "open_ask", "args": {"kind": "chat", "thread": "4", "words": "thanks"}}),
    );
    let first = api(
        &c,
        json!({"method": "answer_ask", "args": {"id": 2, "answer": "hi"}}),
    );
    assert_eq!(
        (&first["ok"]["answered_by"], &first["ok"]["answer"]),
        (&Value::Null, &json!("hi"))
    );
    assert_eq!(
        api(
            &c,
            json!({"method": "close_ask", "args": {"id": 2, "by": amber, "why": "x"}})
        ),
        json!({"refused": "ask 2 is answered already"})
    );
    api(
        &c,
        json!({"method": "open_ask", "args": {"kind": "chat", "thread": "5", "words": "ok"}}),
    );
    let closed = api(
        &c,
        json!({"method": "close_ask", "args": {"id": 3, "by": amber, "why": "an ok"}}),
    );
    assert_eq!(closed["ok"]["closed"], json!(true));
    let all = api(&c, json!({"method": "asks", "args": {"since": NOW}}));
    let ids: Vec<&Value> = all["ok"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| &a["id"])
        .collect();
    assert_eq!(ids, [&json!(3), &json!(2), &json!(1)]);
    assert_eq!(
        api(&c, json!({"method": "asks", "args": {}})),
        json!({"ok": []})
    );
    assert_eq!(
        api(&c, json!({"method": "ask", "args": {"id": 1}}))["ok"]["answered_by"],
        json!(amber)
    );
    for (call, why) in [
        (
            json!({"method": "open_ask", "args": {"kind": "task", "task": "sim/none", "words": "?"}}),
            "no task 'sim/none'",
        ),
        (json!({"method": "ask", "args": {"id": 9}}), "no ask 9"),
    ] {
        assert_eq!(api(&c, call), json!({"refused": why}));
    }
    let unread = api(
        &c,
        json!({"method": "open_ask", "args": {"kind": "chat", "thread": "4", "words": "x", "colour": "red"}}),
    );
    assert!(
        unread["error"].as_str().unwrap().contains("colour"),
        "{unread}"
    );
}
