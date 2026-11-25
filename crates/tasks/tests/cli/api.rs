//! `tasks api`: one call, or a list of them, as JSON on stdin; each answer `{"ok": result}`,
//! `{"refused": why}` or `{"error": why}` on stdout. These hold the seam's own rules.

use crate::{Case, env, run};
use serde_json::{Value, json};

mod assessment;
mod attempts;
mod bound;
mod contract;
mod red;

fn answer(env: &symphony_board::cli::Env, call: Value) -> Value {
    let ran = run(env, &["api"], &call.to_string());
    assert_eq!(ran.code, 0, "{}", ran.err);
    serde_json::from_str(&ran.out).unwrap_or_else(|e| panic!("{e}: {:?}", ran.out))
}

#[test]
fn a_call_gives_the_stores_result_in_pythons_field_order() {
    let c = Case::new();
    let actor = c.register("worker");
    let env = c.env;
    let made = answer(
        &env,
        json!({"method": "create", "args": {"slug": "sim/a", "title": "A", "by": actor, "links": ["docs/X.md"]}}),
    );
    let task = &made["ok"];
    // Sorted here: a parsed map keeps serde_json's order, which a workspace build changes (tools/picture turns
    // on its preserve_order for every member). The order the answer has is the raw text's, below.
    let mut keys: Vec<&str> = task
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    symphony_board::order::sort_by(&mut keys, &Ord::cmp);
    assert_eq!(
        keys,
        [
            "body",
            "created",
            "created_by",
            "depends",
            "id",
            "instruction",
            "links",
            "parent",
            "priority",
            "roles",
            "slug",
            "status",
            "title",
            "updated"
        ],
        "the fields are Python's, and the task's number since minor 1"
    );
    assert_eq!(
        (
            &task["slug"],
            &task["status"],
            &task["created"],
            &task["links"]
        ),
        (
            &json!("sim/a"),
            &json!("ready"),
            &json!(common_now()),
            &json!(["docs/X.md"])
        )
    );
    let raw = run(
        &env,
        &["api"],
        r#"{"method": "get", "args": {"slug": "sim/a"}}"#,
    )
    .out;
    let order: Vec<usize> = [
        "\"slug\"",
        "\"title\"",
        "\"body\"",
        "\"status\"",
        "\"roles\"",
        "\"instruction\"",
    ]
    .iter()
    .map(|k| raw.find(k).unwrap_or_else(|| panic!("{k} in {raw}")))
    .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{raw}");
}

fn common_now() -> f64 {
    crate::common::NOW
}

#[test]
fn a_refusal_is_its_words() {
    let env = env();
    assert_eq!(
        answer(&env, json!({"method": "get", "args": {"slug": "sim/none"}})),
        json!({"refused": "no task 'sim/none'"})
    );
}

#[test]
fn a_list_of_calls_is_a_list_of_answers_in_order() {
    let c = Case::new();
    let actor = c.register("worker");
    let env = c.env;
    let answers = answer(
        &env,
        json!([
            {"method": "create", "args": {"slug": "a", "title": "A", "by": actor}},
            {"method": "get", "args": {"slug": "b"}},
            {"method": "ancestors", "args": {"slug": "a"}},
        ]),
    );
    assert_eq!(answers[0]["ok"]["slug"], json!("a"));
    assert_eq!(answers[1], json!({"refused": "no task 'b'"}));
    assert_eq!(answers[2], json!({"ok": []}));
}

/// Every open task has an orchestrator: a task a tool files through the API (a release run's red,
/// by a filer) comes back given one, the live orchestrator holding fewest.
#[test]
fn a_task_filed_through_the_api_comes_back_with_its_orchestrator() {
    let mut c = Case::new();
    let orch = c.register("orchestrator");
    // An orchestrator that stays: waiting, past its first live line.
    let later = crate::common::NOW + symphony_board::health::LIVE_SECONDS + 1.0;
    c.env.vars.insert("TASKS_NOW".into(), later.to_string());
    c.ok(&orch, &["wait", "--timeout", "0"]);
    // A filer is a session the API's `register` takes, as the tools that file reds register.
    let filer = answer(
        &c.env,
        json!({"method": "register", "args": {"kind": "filer"}}),
    );
    let filer = filer["ok"]["handle"].as_str().unwrap().to_string();
    let made = answer(
        &c.env,
        json!({"method": "create", "args": {"slug": "sim/red", "title": "Red", "by": filer}}),
    );
    let roles = &made["ok"]["roles"];
    assert_eq!(
        (
            &roles[0]["handle"],
            &roles[0]["role"],
            roles.as_array().map(Vec::len)
        ),
        (&json!(orch), &json!("orchestrator"), Some(1)),
        "{made}"
    );
    let thread = c.store().thread("sim/red").unwrap();
    assert_eq!(
        thread.last().map(|m| m.text.as_str()),
        Some(format!("{orch} was given orchestrator (filed by {filer})").as_str())
    );
}

#[test]
fn create_refuses_fast_fix_without_filing_and_keeps_serving() {
    let c = Case::new();
    let actor = c.register("worker");
    let mut calls = Vec::new();
    for value in [
        json!("one line"),
        json!(""),
        json!(null),
        json!(false),
        json!({}),
    ] {
        calls.push(json!({"method": "create", "args": {
            "slug": "fix/retired", "title": "Retired", "by": actor, "fast_fix": value
        }}));
    }
    calls.push(
        json!({"method": "create", "args": {"slug": "fix/plain", "title": "Plain", "by": actor}}),
    );
    let answers = answer(&c.env, json!(calls));
    for result in &answers.as_array().unwrap()[..5] {
        assert!(
            result["error"]
                .as_str()
                .is_some_and(|why| why.contains("unknown field `fast_fix`")),
            "{result}"
        );
    }
    assert_eq!(answers[5]["ok"]["slug"], "fix/plain");
    assert!(c.store().get("fix/retired").is_err());
    *calls.last_mut().unwrap() = json!({"method": "get", "args": {"slug": "fix/plain"}});
    let lines = calls
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let (_, answers) = served(&c.env, &lines);
    for result in &answers[..5] {
        assert!(
            result["error"]
                .as_str()
                .is_some_and(|why| why.contains("unknown field `fast_fix`")),
            "{result}"
        );
    }
    assert_eq!(answers[5]["ok"]["slug"], "fix/plain");
    assert!(c.store().get("fix/retired").is_err());
}

#[test]
fn a_call_it_cannot_read_is_an_error_naming_what() {
    let env = env();
    for (call, named) in [
        (json!({"method": "no_such_method"}), "no_such_method"),
        (json!({"method": "get", "args": {}}), "slug"),
        (
            json!({"method": "get", "args": {"slug": "a", "extra": 1}}),
            "extra",
        ),
        (json!({"method": "get", "args": {"slug": 3}}), "slug"),
        (
            json!({"method": "move", "args": {"slug": "a", "status": "cancelled"}}),
            "handle",
        ),
        (
            json!({"method": "move", "args": {"slug": "a", "status": "cancelled", "handle": 3}}),
            "handle",
        ),
        (
            json!({"method": "move", "args": {"slug": "a", "status": "cancelled", "handle": null}}),
            "handle",
        ),
        (json!({"args": {}}), "method"),
        (
            json!({"method": "register", "args": {"kind": "worker", "colour": "red"}}),
            "colour",
        ),
    ] {
        let said = answer(&env, call.clone());
        let error = said["error"]
            .as_str()
            .unwrap_or_else(|| panic!("{call} gave {said}"));
        assert!(error.contains(named), "{call}: {error}");
    }
}

#[test]
fn input_that_is_not_json_is_an_error_and_exit_two() {
    let ran = run(&env(), &["api"], "{not json");
    assert_eq!(ran.code, 2);
    let said: Value = serde_json::from_str(&ran.out).unwrap();
    assert!(said["error"].as_str().unwrap().contains("JSON"), "{said}");
}

#[test]
fn a_board_it_cannot_reach_is_an_error_in_one_line_naming_it() {
    let mut env = env();
    env.vars
        .insert("TASKS_SERVER".into(), crate::UNREACHED_SERVER.into());
    let said = answer(&env, json!([{"method": "sessions"}, {"method": "tasks"}]));
    for each in said.as_array().unwrap() {
        let error = each["error"].as_str().unwrap();
        assert!(
            error.contains(crate::UNREACHED_FOLDER) && !error.contains('\n'),
            "{error}"
        );
    }
}

#[test]
fn a_database_name_that_is_not_plain_is_refused_before_anything_is_sent() {
    let mut env = env();
    env.vars.insert("TASKS_DATABASE".into(), "a;REMOVE".into());
    let said = answer(&env, json!({"method": "sessions"}));
    assert!(
        said["error"].as_str().unwrap().contains("a;REMOVE"),
        "{said}"
    );
}

#[test]
fn tasks_now_is_the_clock() {
    let mut env = env();
    env.vars.insert("TASKS_NOW".into(), "1234.5".into());
    let s = answer(
        &env,
        json!({"method": "register", "args": {"kind": "worker"}}),
    );
    assert_eq!(
        (&s["ok"]["connected"], &s["ok"]["health"]),
        (&json!(1234.5), &json!("live"))
    );
    env.vars.insert("TASKS_NOW".into(), "soon".into());
    let said = answer(&env, json!({"method": "sessions"}));
    assert!(
        said["error"].as_str().unwrap().contains("TASKS_NOW"),
        "{said}"
    );
}

/// `tasks api --lines` with `lines` on stdin: its exit code and each line it answered, read as JSON.
fn served(env: &symphony_board::cli::Env, lines: &str) -> (i32, Vec<Value>) {
    let ran = run(env, &["api", "--lines"], lines);
    let answers = ran
        .out
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {line:?}")))
        .collect();
    assert!(
        ran.out.is_empty() || ran.out.ends_with('\n'),
        "{:?}",
        ran.out
    );
    (ran.code, answers)
}

#[test]
fn lines_answers_each_call_on_a_line_of_its_own_in_order() {
    let c = Case::new();
    let actor = c.register("worker");
    let env = c.env;
    let calls = [
        json!({"method": "create", "args": {"slug": "sim/a", "title": "A\nnext", "by": actor}}),
        json!({"method": "get", "args": {"slug": "sim/b"}}),
        json!([{"method": "ancestors", "args": {"slug": "sim/a"}}]),
    ];
    let input: String = calls.iter().map(|c| format!("{c}\n")).collect();
    let (code, answers) = served(&env, &input);
    assert_eq!(code, 0);
    assert_eq!(answers.len(), 3, "{answers:?}");
    assert_eq!(answers[0]["ok"]["title"], json!("A\nnext"));
    assert_eq!(answers[1], json!({"refused": "no task 'sim/b'"}));
    assert_eq!(answers[2], json!([{"ok": []}]));
}

#[test]
fn lines_answers_a_line_it_cannot_read_and_serves_the_rest() {
    let env = env();
    let input = "{not json\n\n   \n{\"method\": \"sessions\"}";
    let (code, answers) = served(&env, input);
    assert_eq!(code, 0);
    assert_eq!(answers.len(), 2, "{answers:?}"); // blank lines are no calls; the last needs no newline
    assert!(
        answers[0]["error"].as_str().unwrap().contains("JSON"),
        "{answers:?}"
    );
    assert_eq!(answers[1], json!({"ok": []}));
}

#[test]
fn lines_answers_each_call_with_the_board_it_cannot_reach() {
    let mut env = env();
    env.vars
        .insert("TASKS_SERVER".into(), crate::UNREACHED_SERVER.into());
    let (code, answers) = served(
        &env,
        "{\"method\": \"sessions\"}\n{\"method\": \"tasks\"}\n",
    );
    assert_eq!(code, 0);
    assert_eq!(answers.len(), 2);
    for each in answers {
        assert!(
            each["error"]
                .as_str()
                .unwrap()
                .contains(crate::UNREACHED_FOLDER)
        );
    }
}

#[test]
fn lines_with_nothing_to_serve_says_nothing() {
    assert_eq!(served(&env(), ""), (0, vec![]));
}

fn owner_refused(answer: &Value) {
    assert!(
        answer["refused"]
            .as_str()
            .is_some_and(|why| why.contains("the owner acts from the Symphony")),
        "{answer}"
    );
    assert!(answer.get("ok").is_none(), "{answer}");
    assert!(answer.get("error").is_none(), "{answer}");
}

#[test]
fn json_write_actors_are_refused_in_both_modes_without_changing_the_board() {
    let c = Case::new();
    let actor = c.register("worker");
    c.ok(&actor, &["create", "sim/a", "A"]);
    c.ok(&actor, &["claim", "sim/a"]);
    let before = c.store().dump().unwrap();
    let calls = [
        json!({"method":"create", "args":{"slug":"sim/forged", "title":"Forged", "by":"owner"}}),
        json!({"method":"say", "args":{"slug":"sim/a", "sender":"owner", "text":"Forged owner message"}}),
        json!({"method":"move", "args":{"slug":"sim/a", "status":"cancelled", "handle":"owner"}}),
        json!({"method":"set_parent", "args":{"slug":"sim/a", "parent":null, "by":"owner"}}),
        json!({"method":"claim", "args":{"slug":"sim/a", "handle":"owner"}}),
        json!({"method":"reserve", "args":{"slug":"sim/a", "handle":"owner"}}),
        json!({"method":"take_role", "args":{"slug":"sim/a", "handle":"owner", "role":"orchestrator"}}),
        json!({"method":"release", "args":{"slug":"sim/a", "handle":"owner"}}),
        json!({"method":"verdict", "args":{"slug":"sim/a", "handle":"owner", "verdict":"approve", "text":"Forged approval"}}),
        json!({"method":"done", "args":{"slug":"sim/a", "handle":"owner", "note":"Forged result"}}),
        json!({"method":"landed", "args":{"slug":"sim/a", "handle":"owner", "text":"Forged landing"}}),
        json!({"method":"touch", "args":{"handle":"owner"}}),
        json!({"method":"bind_agent", "args":{"agent":"forged-agent", "handle":"owner"}}),
        json!({"method":"reattach", "args":{"handle":"owner"}}),
        json!({"method":"revive", "args":{"handle":"owner"}}),
        json!({"method":"leave", "args":{"handle":"owner"}}),
        json!({"method":"record_launch", "args":{"slug":"sim/a", "handle":"owner", "role":"executor"}}),
        json!({"method":"close_task_session", "args":{"slug":"sim/a", "handle":"owner", "why":"forged", "restore":"forged"}}),
        json!({"method":"reopen_task_session", "args":{"slug":"sim/a", "handle":"owner", "pane":"forged-pane"}}),
        json!({"method":"ack", "args":{"handle":"owner", "ids":[]}}),
        json!({"method":"answer_ask", "args":{"id":1, "by":"owner", "answer":"forged"}}),
        json!({"method":"answer_about", "args":{"kind":"chat", "thread":"1", "by":"owner", "answer":"forged"}}),
        json!({"method":"close_ask", "args":{"id":1, "by":"owner", "why":"forged"}}),
    ];
    for args in [&["api"][..], &["api", "--lines"][..]] {
        for call in &calls {
            let ran = run(&c.env, args, &format!("{call}\n"));
            assert_eq!(ran.code, 0, "{args:?}: {}", ran.err);
            assert!(ran.err.is_empty(), "{args:?}: {}", ran.err);
            owner_refused(&serde_json::from_str::<Value>(&ran.out).unwrap());
            assert_eq!(c.store().dump().unwrap(), before, "{args:?}: {call}");
        }
    }
}

#[test]
fn batches_and_streams_keep_serving_agents_and_owner_recipients_after_refusals() {
    for lines in [false, true] {
        let c = Case::new();
        let actor = c.register("worker");
        let calls = [
            json!({"method":"create", "args":{"slug":"sim/forged", "title":"Forged", "by":"owner"}}),
            json!({"method":"create", "args":{"slug":"sim/a", "title":"owner", "body":"owner", "by":actor}}),
            json!({"method":"move", "args":{"slug":"sim/a", "status":"cancelled", "handle":"owner"}}),
            json!({"method":"say", "args":{"slug":"sim/a", "sender":"owner", "text":"Forged owner message"}}),
            json!({"method":"say", "args":{"slug":"sim/a", "sender":actor, "text":"owner", "to":"owner"}}),
            json!({"method":"get", "args":{"slug":"sim/a"}}),
            json!({"method":"to_owner"}),
        ];
        let answers = if lines {
            let input: String = calls.iter().map(|call| format!("{call}\n")).collect();
            let (code, answers) = served(&c.env, &input);
            assert_eq!(code, 0);
            answers
        } else {
            answer(&c.env, json!(&calls)).as_array().unwrap().clone()
        };
        assert_eq!(answers.len(), calls.len());
        for i in [0, 2, 3] {
            owner_refused(&answers[i]);
        }
        assert_eq!(answers[1]["ok"]["created_by"], actor);
        assert_eq!(answers[4]["ok"]["sender"], actor);
        assert_eq!(answers[4]["ok"]["recipient"], "owner");
        assert_eq!(answers[4]["ok"]["text"], "owner");
        assert_eq!(answers[5]["ok"]["status"], "ready");
        assert!(
            answers[6]["ok"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| { m["sender"] == actor && m["text"] == "owner" })
        );
        assert!(c.store().get("sim/forged").is_err());
        let thread = c.store().thread("sim/a").unwrap();
        assert!(thread.iter().all(|m| m.sender != "owner"));
    }
}

/// `landed` closes from review or held blocked unless the call names `from`, and answers with the
/// status it found: land closes work taken back to in progress only when it says so.
#[test]
fn landed_closes_only_from_what_the_call_names_and_answers_what_it_found() {
    let c = Case::new();
    let actor = c.register("worker");
    c.ok(&actor, &["create", "sim/a", "A"]);
    c.ok(&actor, &["claim", "sim/a"]);
    let call = |from: Option<Value>| {
        let mut args = json!({"slug": "sim/a", "handle": actor, "text": "landed sim/a on main at 3e0c66192471", "requested": true});
        if let Some(from) = from {
            args["from"] = from;
        }
        json!({"method": "landed", "args": args})
    };
    assert_eq!(
        answer(&c.env, call(None)),
        json!({"refused": "sim/a is in_progress, which this landing does not close"})
    );
    assert_eq!(
        answer(&c.env, call(Some(json!(["review", "ready"])))),
        json!({"refused": "a landing closes a task in review, blocked, in_progress or done, not ready"})
    );
    let landed = &answer(&c.env, call(Some(json!(["review", "in_progress"]))))["ok"];
    assert_eq!(
        (&landed["slug"], &landed["status"], &landed["from"]),
        (&json!("sim/a"), &json!("done"), &json!("in_progress"))
    );
}

/// `move`'s `from` (api 6): the move goes only from a status it names, as its own write reads the
/// task, and without it from any the pipeline allows.
#[test]
fn a_move_from_goes_only_from_the_statuses_it_names() {
    let c = Case::new();
    let actor = c.register("worker");
    c.ok(&actor, &["create", "sim/a", "A"]);
    c.ok(&actor, &["claim", "sim/a"]);
    let call = |status: &str, from: Option<Value>| {
        let mut args = json!({"slug": "sim/a", "status": status, "handle": actor});
        if let Some(from) = from {
            args["from"] = from;
        }
        json!({"method": "move", "args": args})
    };
    assert_eq!(
        answer(&c.env, call("in_progress", Some(json!(["review"])))),
        json!({"refused": "sim/a is in_progress, not review: it stays in_progress"})
    );
    let moved = &answer(
        &c.env,
        call("review", Some(json!(["blocked", "in_progress"]))),
    )["ok"];
    assert_eq!(moved["status"], "review");
    assert_eq!(
        answer(&c.env, call("blocked", None))["ok"]["status"],
        "blocked"
    );
}

/// An approved task goes back to review over the API, as on the command line, only from a checkout
/// of its repository, which says whether its tip is on the default branch already.
#[test]
fn a_move_back_from_done_is_refused_away_from_the_repository() {
    let c = Case::new();
    let (exe, rev) = (c.register("worker"), c.register("reviewer"));
    c.ok(&exe, &["create", "sim/a", "A"]);
    c.ok(&exe, &["claim", "sim/a"]);
    c.ok(&exe, &["move", "sim/a", "review"]);
    c.ok(&rev, &["role", "sim/a", "reviewer"]);
    c.ok(&rev, &["verdict", "sim/a", "approve", "good"]);
    let call =
        json!({"method": "move", "args": {"slug": "sim/a", "status": "review", "handle": exe}});
    assert_eq!(
        answer(&c.env, call),
        json!({"refused": "sim/a is done: it goes back to review only from a checkout of its \
                           repository, which says whether its tip is on the default branch \
                           already"})
    );
    assert_eq!(c.store().get("sim/a").unwrap().status, "done");
}

/// `done`'s `covered_by` closes a task as covered over the API, as on the command line, only from a
/// checkout of its repository, after the board's own checks of the covering task.
#[test]
fn a_covered_close_over_the_api_reads_the_repository_first() {
    let c = Case::new();
    let exe = c.register("worker");
    for slug in ["sim/a", "sim/b"] {
        c.ok(&exe, &["create", slug, "A"]);
    }
    c.ok(&exe, &["claim", "sim/b"]);
    let call = |other: &str| {
        json!({"method": "done", "args": {"slug": "sim/b", "handle": exe, "note": "landed with it",
            "covered_by": other}})
    };
    assert_eq!(
        answer(&c.env, call("sim/a")),
        json!({"refused": "sim/a is ready: a task is closed as covered only by a task done and \
                           landed"})
    );
    c.ok(&exe, &["claim", "sim/a"]);
    c.ok(&exe, &["move", "sim/a", "review"]);
    c.store()
        .landed(
            "sim/a",
            &exe,
            "landed sim/a on main at 75b4516a0123",
            true,
            &["review"],
        )
        .unwrap();
    assert_eq!(
        answer(&c.env, call("sim/a")),
        json!({"refused": "sim/b is closed as covered only from a checkout of its repository, \
                           which says whether sim/a's landing is on the default branch and what \
                           sim/b's own branch holds"})
    );
    assert_eq!(c.store().get("sim/b").unwrap().status, "in_progress");
}

/// `move`'s and `hold`'s `on` and `why` (api 15): what the block waits on, kept on the task; a
/// block given neither records nothing, as before, and a `why` alone or with `from` is refused.
#[test]
fn a_block_over_the_api_names_what_it_waits_on_or_nothing_as_before() {
    use symphony_board::waits_on::Blocked;
    let c = Case::new();
    let exe = c.register("worker");
    c.ok(&exe, &["create", "sim/a", "A"]);
    c.ok(&exe, &["claim", "sim/a"]);
    let call = |method: &str, more: Value| {
        let mut args = json!({"slug": "sim/a", "handle": exe});
        if method == "move" {
            args["status"] = json!("blocked");
        }
        for (key, value) in more.as_object().unwrap() {
            args[key] = value.clone();
        }
        json!({"method": method, "args": args})
    };
    let record = || Blocked::of(&c.store().get("sim/a").unwrap());
    assert_eq!(
        answer(&c.env, call("move", json!({"why": "later"}))),
        json!({"refused": "a block's why goes with what it waits on (`on`, one of decision:NAME, \
                           check:NAME, task:SLUG or parked)"})
    );
    assert_eq!(
        answer(
            &c.env,
            call("move", json!({"on": "parked", "from": ["in_progress"]}))
        ),
        json!({"refused": "a move from named statuses names no block's thing"})
    );
    let moved = answer(
        &c.env,
        call("move", json!({"on": "check:nightly", "why": "red"})),
    );
    assert_eq!(moved["ok"]["status"], "blocked");
    let kept = record().unwrap();
    assert_eq!(
        (kept.on.as_str(), kept.why.as_deref()),
        ("check:nightly", Some("red"))
    );
    c.ok(&exe, &["move", "sim/a", "review"]);
    // A hold names its thing too, or, as before minor 15, nothing.
    let held = answer(&c.env, call("hold", json!({"on": "check:owner"})));
    assert_eq!(held["ok"]["status"], "blocked");
    assert_eq!(record().unwrap().on, "check:owner");
    c.ok(&exe, &["move", "sim/a", "review"]);
    assert_eq!(
        answer(&c.env, call("hold", json!({})))["ok"]["status"],
        "blocked"
    );
    assert_eq!(record(), None);
    c.ok(&exe, &["move", "sim/a", "in_progress"]);
    assert_eq!(
        answer(&c.env, call("move", json!({})))["ok"]["status"],
        "blocked"
    );
    assert_eq!(record(), None);
}

/// `move`'s `reason` is posted after the move's line; with `from` it is refused.
#[test]
fn a_move_over_the_api_says_its_reason() {
    let c = Case::new();
    let exe = c.register("worker");
    c.ok(&exe, &["create", "sim/a", "A"]);
    c.ok(&exe, &["claim", "sim/a"]);
    let call = |from: Option<Value>| {
        let mut args = json!({"slug": "sim/a", "status": "cancelled", "handle": exe,
            "reason": "superseded by sim/b"});
        if let Some(from) = from {
            args["from"] = from;
        }
        json!({"method": "move", "args": args})
    };
    assert_eq!(
        answer(&c.env, call(Some(json!(["in_progress"])))),
        json!({"refused": "a move from named statuses takes no reason"})
    );
    assert_eq!(answer(&c.env, call(None))["ok"]["status"], "cancelled");
    let last = c.store().thread("sim/a").unwrap().pop().unwrap();
    assert_eq!(
        (last.kind.as_str(), last.text.as_str()),
        ("msg", "cancelled: superseded by sim/b")
    );
}

#[test]
fn board_authored_notices_reach_the_actors_own_inbox_and_still_validate_the_actor_and_recipient() {
    let c = Case::new();
    let actor = c.register("worker");
    c.ok(&actor, &["create", "tools/example", "Example"]);
    c.ok(&actor, &["claim", "tools/example"]);
    let before = answer(&c.env, json!({"method":"session", "args":{"handle":actor}}));
    let mut later = c.env.clone();
    later
        .vars
        .insert("TASKS_NOW".into(), (common_now() + 60.0).to_string());
    let text = "room: 16.5 GiB free; run `just check` once";
    let notified = answer(
        &later,
        json!({"method":"notify", "args":{
            "slug":"tools/example", "actor":actor, "handle":actor, "text":text
        }}),
    );
    assert_eq!(notified["ok"]["sender"], "board");
    assert_eq!(notified["ok"]["recipient"], actor);
    let after = answer(&later, json!({"method":"session", "args":{"handle":actor}}));
    assert_eq!(
        after["ok"]["last_seen"], before["ok"]["last_seen"],
        "a machine event is no heartbeat"
    );
    let inbox = answer(&c.env, json!({"method":"inbox", "args":{"handle":actor}}));
    let messages = inbox["ok"].as_array().unwrap();
    assert_eq!(messages.len(), 1, "{inbox}");
    assert_eq!(messages[0]["text"], text);
    assert_eq!(messages[0]["kind"], "msg");
    for (sender, recipient, reason) in [
        ("missing-otter", actor.as_str(), "no session"),
        (actor.as_str(), "missing-otter", "no session"),
        (actor.as_str(), "@orchestrator", "literal session handle"),
        (actor.as_str(), "owner", "literal session handle"),
        ("owner", actor.as_str(), "owner acts from the Symphony"),
    ] {
        let refused = answer(
            &c.env,
            json!({"method":"notify", "args":{
                "slug":"tools/example", "actor":sender, "handle":recipient, "text":text
            }}),
        );
        assert!(
            refused["refused"].as_str().unwrap().contains(reason),
            "{refused}"
        );
    }
    let ordinary = answer(
        &c.env,
        json!({"method":"say", "args":{
            "slug":"tools/example", "sender":actor, "to":actor, "text":"own post"
        }}),
    );
    assert_eq!(ordinary["ok"]["sender"], actor);
    assert_eq!(
        answer(&c.env, json!({"method":"inbox", "args":{"handle":actor}}))["ok"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_say_under_a_key_answers_the_first_post_under_it() {
    let c = Case::new();
    let actor = c.register("worker");
    c.ok(&actor, &["create", "sim/a", "A"]);
    let first = answer(
        &c.env,
        json!({"method":"say", "args":{"slug":"sim/a", "sender":actor, "text":"main is red", "to":"@executor", "key":"main-red:3e0c"}}),
    );
    let again = answer(
        &c.env,
        json!({"method":"say", "args":{"slug":"sim/a", "sender":actor, "text":"main is red, said again", "key":"main-red:3e0c"}}),
    );
    assert_eq!(again, first);
    assert_eq!(
        (
            first["ok"]["text"].as_str(),
            first["ok"]["recipient"].as_str()
        ),
        (Some("main is red"), Some("@executor"))
    );
    // The actor's one line; the board's post beside it says no executor read it.
    let posts: Vec<String> = c
        .store()
        .thread("sim/a")
        .unwrap()
        .into_iter()
        .filter(|m| m.kind == "msg" && m.sender == actor)
        .map(|m| m.text)
        .collect();
    assert_eq!(posts, vec!["main is red".to_string()]);
}

#[test]
fn a_say_asked_to_adopt_answers_the_threads_line_reading_it() {
    let c = Case::new();
    let actor = c.register("worker");
    c.ok(&actor, &["create", "sim/a", "A"]);
    let old = answer(
        &c.env,
        json!({"method":"say", "args":{"slug":"sim/a", "sender":actor, "text":"main is red", "to":"@executor"}}),
    );
    let taken = answer(
        &c.env,
        json!({"method":"say", "args":{"slug":"sim/a", "sender":actor, "text":"main is red", "to":"@executor", "key":"main-red:3e0c", "adopt":true}}),
    );
    assert_eq!(taken, old);
    let again = answer(
        &c.env,
        json!({"method":"say", "args":{"slug":"sim/a", "sender":actor, "text":"main is red, said again", "key":"main-red:3e0c"}}),
    );
    assert_eq!(again, old);
    let posts: Vec<String> = c
        .store()
        .thread("sim/a")
        .unwrap()
        .into_iter()
        .filter(|m| m.kind == "msg" && m.sender == actor)
        .map(|m| m.text)
        .collect();
    assert_eq!(posts, vec!["main is red".to_string()]);
}
