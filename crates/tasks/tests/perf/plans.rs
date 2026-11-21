//! The plan check. The commands of every per-write, per-recipient and per-tick path, and a store's
//! own read for a tick's read no command makes, run on the generated board through the recording
//! relay ([`crate::proxy`]); each statement their requests hold that reads rows under a condition
//! ([`crate::sql`]: every `SELECT`, nested ones too, and each `UPDATE` or `DELETE` of a table
//! under a `WHERE` read as the `SELECT` of the rows it would change, the server planning only a
//! `SELECT`) is planned by the server (`EXPLAIN`) with the request's own variables, and a variable
//! an earlier statement of the request set given a value of its kind ([`bound`]). The relay sees only what a client sends, so what the schema's events
//! run in the write that fires them is planned from the schema itself ([`crate::sql::events`]),
//! `$before` and `$after` a message of the board's. The check fails on what [`crate::findings`]
//! finds in a plan: a filtered scan of message, delivery, role, session or callback_state, or a
//! subquery in a predicate. Before its commands it files a task nobody orchestrates, so the give
//! pass a dispatch and a look make is among what it plans on every run, whatever the tests before
//! it on the same board left ([`unowned`]); then, after a look by another orchestrator that makes
//! it one that stays (as the timings' looks make theirs), it files another and dispatches again.
//! Each give is judged by its having been made ([`gave`]), never by who the board picked, which
//! turns on the orchestrators earlier tests made stay. Last, a session holding no orchestrator role
//! takes it on a third such task, so the read of the taker's roles that the limit on the open tasks
//! an orchestrator holds makes ([`GAINING`]) is planned, and held to its holder's index alone.

use crate::board::board;
use crate::findings::{Finding, findings, reads_rows, whole_read};
use crate::generated::{self, NOW};
use crate::proxy::{Proxy, Request};
use crate::sql::{events, sequence, statements, variables};
use crate::store;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use symphony_board::rpc::Client;

/// A value of the kind a variable set by an earlier statement of a request holds, by its name, for
/// planning a statement that reads it: the server picks an index by the value it compares, and an
/// unbound name reads as NONE, for which it picks none.
pub fn bound(name: &str) -> Option<Value> {
    let open = json!([generated::thread(), generated::slug(generated::TASKS - 1)]);
    Some(match name {
        "me" => json!({"connected": NOW - 5.0 * 86_400.0, "left_at": null}),
        "connected" => json!(NOW - 5.0 * 86_400.0),
        // `mine`: the tasks a take's taker holds the orchestrator role on, read by its roles.
        "slugs" | "open" | "waiting" | "mine" => open,
        "tasks" => json!([{"slug": generated::thread()}]),
        "orchestrators" => json!([[generated::thread(), generated::orchestrator()]]),
        // The unanswered lookup's answers below a task: the sequence its ask reads after.
        "sid" | "same" | "last" | "post" | "after" => json!(1),
        "ts" => json!(NOW),
        "counted" => json!({"last": 1, "ts": NOW}),
        "agent" | "binding" | "old" | "other" | "had" | "made" | "was" => Value::Null,
        "s" | "state" | "c" | "lease" | "recipient" | "channel" | "before" | "pull" => json!({}),
        "candidates" | "acked" | "wanted" | "pool" | "keys" | "waiting_keys" | "said" => json!([]),
        "line" => json!({"slug": generated::thread(), "sid": 1}),
        // A give pass's fence names each task it decided on with its version as read.
        "g" => json!({"slug": generated::thread(), "version": 1}),
        "address" => json!("uds:/tmp/perf.sock"),
        // The pending-post read is planned outside the closure that supplies each task's slug.
        "slug" | "dependent" => json!(generated::thread()),
        "sender" | "handle" => json!(generated::writer()),
        "kind" => json!("msg"),
        // The slot ledger's reads name their tasks and holds in an earlier statement.
        "slot_wanted" => json!([generated::thread()]),
        "slot_holds" => json!([[generated::thread(), NOW]]),
        // The sessions a read shows, named by the roles an earlier statement read.
        "shown_sessions" => json!([generated::writer()]),
        "prefix" | "pull_kept" => json!("pull-1"),
        // The workflow position a task's write puts it at, which the schema's `workflow_position`
        // event reads its transitions by.
        "w" => json!({"phase": "build", "state": "working", "revision": 1}),
        // The transition a return recorded for a task whose position goes, which the same event
        // reads its activation by.
        "r" => json!({"status": "ready", "activation": 1}),
        _ => return None,
    })
}

/// One statement as planned: how often the requests sent it, its plan and what is wrong with it.
#[derive(Debug)]
struct Planned {
    sent: usize,
    plan: String,
    findings: Vec<Finding>,
}

/// The variables `statement` is planned with: those its request sent (`sent_with`), and a value of
/// [`bound`]'s for each other it reads; and the names it reads that neither gives.
fn given(statement: &str, sent_with: &Value) -> (Map<String, Value>, Vec<String>) {
    let mut vars: Map<String, Value> = sent_with.as_object().cloned().unwrap_or_default();
    let mut unbound = Vec::new();
    let read = variables(statement);
    for name in read {
        if vars.contains_key(&name) {
            continue;
        }
        match bound(&name) {
            Some(value) => {
                vars.insert(name, value);
            }
            None => unbound.push(name),
        }
    }
    (vars, unbound)
}

/// The server's plan of `statement` with `vars`, or why it gave none.
fn explain(client: &Client, statement: &str, vars: Map<String, Value>) -> Result<String, String> {
    match client.query(&format!("EXPLAIN {statement};"), Value::Object(vars)) {
        Ok(mut answers) => Ok((answers.pop())
            .and_then(|answer| answer.as_str().map(String::from))
            .unwrap_or_default()),
        Err(why) => Err(format!("not planned: {why:?}")),
    }
}

/// `statement`, sent with `sent_with`, planned on the board `client` reaches.
fn plan_one(client: &Client, statement: &str, sent_with: &Value) -> Planned {
    let (vars, unbound) = given(statement, sent_with);
    let (plan, found) = if !unbound.is_empty() {
        (
            format!(
                "not planned: give ${} a value in plans::bound",
                unbound.join(", $")
            ),
            vec![Finding::NotPlanned],
        )
    } else {
        match explain(client, statement, vars) {
            Ok(plan) => {
                let found = findings(&plan);
                (plan, found)
            }
            Err(why) => (why, vec![Finding::NotPlanned]),
        }
    };
    Planned {
        sent: 1,
        plan,
        findings: found,
    }
}

/// Each statement `requests` run one after another ([`sequence`]) that `read` reads no statement
/// out of but the server plans as a read of rows ([`reads_rows`]), with its plan: a spelling the
/// check does not know, reported rather than left unplanned. A variable it reads that neither its
/// request nor [`bound`] gives is NONE: what it reads is in the plan's shape, whatever the value.
/// The server plans no `UPDATE`, `UPSERT` or `DELETE` (3.3.0: `EXPLAIN is only supported with the
/// new execution model`), so their spellings rest on [`crate::sql`]'s reading alone.
fn unread(client: &Client, requests: &[Request], read: fn(&str) -> Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut unread = Vec::new();
    for request in requests {
        for statement in sequence(&request.sql) {
            if !read(&statement).is_empty() || !seen.insert(statement.clone()) {
                continue;
            }
            let (vars, _) = given(&statement, &request.vars);
            if let Ok(plan) = explain(client, &statement, vars)
                && reads_rows(&plan)
            {
                unread.push(format!("{statement}\n{}", plan.trim_end()));
            }
        }
    }
    unread
}

/// Each distinct statement of `requests`, planned on the board `client` reaches.
fn plan_all(client: &Client, requests: &[Request]) -> BTreeMap<String, Planned> {
    let mut planned: BTreeMap<String, Planned> = BTreeMap::new();
    for request in requests {
        let held = statements(&request.sql);
        for statement in held {
            if let Some(seen) = planned.get_mut(&statement) {
                seen.sent += 1;
                continue;
            }
            let one = plan_one(client, &statement, &request.vars);
            planned.insert(statement, one);
        }
    }
    planned
}

/// A statement of the give pass's fence ([`symphony_board::Store::give_unowned`]): what the requests
/// hold once the check's commands made a give.
const GIVE_FENCE: &str = "type::record('task', $g.slug)";

/// What a take of the orchestrator role reads of its taker for the limit on the open tasks it holds
/// the role on, its slot counter beside its roles: in the request of that read alone.
const GAINING: &str = "['slots', $handle]";

/// The two reads of [`symphony_board::Store::questions_waiting`]: the questions to a person, by the
/// recipient's index, and each one's thread after it, by its range.
const QUESTIONS: [&str; 2] = ["WHERE recipient = $person", "WHERE sender = $person"];

/// A new top-level task nobody orchestrates on `board`, named for `which` and this run, filed by the
/// owner as a plain create, which gives none: the next reader of the whole board that gives (`tasks
/// dispatch`, a look of `tasks wait`) gives it, whatever the board's other tasks are, and however
/// earlier tests left them.
fn unowned(board: &crate::board::Board, which: &str) -> String {
    let store = symphony_board::Store::with(
        board.client(),
        Box::new(|| NOW),
        Box::new(|_| false),
        &symphony_board::sys::hostname(),
    )
    .unwrap();
    let slug = format!("perf/unowned-{which}-{}", std::process::id());
    let new = symphony_board::NewTask::new(&slug, "Nobody orchestrates it", "owner");
    let task = store.create(&new).unwrap();
    assert!(
        task.roles.iter().all(|role| role.role != "orchestrator"),
        "{:?}",
        task.roles
    );
    slug
}

/// Who a give pass gave `slug` to, or `waiting` when it marked it waiting for one: it gave it when
/// exactly one session holds its orchestrator role and that session is an orchestrator, whichever
/// the board picked (the one holding fewest of those that stay, which earlier tests decide), or
/// when it carries the waiting mark and nobody holds it. Fails the test otherwise.
fn gave(board: &crate::board::Board, slug: &str) -> String {
    let found = board
        .client()
        .query(
            "LET $held = (SELECT VALUE handle FROM role WHERE slug = $slug AND role = 'orchestrator');
             RETURN {
                held: $held,
                kinds: (SELECT VALUE kind FROM $held.map(|$h| type::record('session', $h))),
                waiting: (SELECT VALUE fields._board.waiting FROM ONLY type::record('task', $slug)),
             };",
            json!({"slug": slug}),
        )
        .unwrap()
        .pop()
        .unwrap();
    match (&found["held"], &found["kinds"], &found["waiting"]) {
        (Value::Array(held), kinds, _) if held.len() == 1 && *kinds == json!(["orchestrator"]) => {
            held[0].as_str().unwrap_or_default().to_owned()
        }
        (Value::Array(held), _, waiting) if held.is_empty() && waiting.is_number() => {
            "waiting".to_owned()
        }
        _ => panic!("no give pass gave {slug} to one orchestrator or marked it waiting: {found}"),
    }
}

#[test]
fn every_query_a_write_a_recipient_or_a_tick_sends_is_planned_on_an_index() {
    let mut board = board();
    // The give pass runs on a condition the check sets, never on what earlier tests or the clock
    // left: a task nobody orchestrates, made before the relay records.
    let first = unowned(&board, "first");
    let relay = Proxy::to(&board.addr);
    let thread = generated::thread();
    let writer = generated::writer();
    let recipient = generated::recipient();
    let orchestrator = generated::orchestrator();
    let claimed = board.unclaimed();
    let api = |method: &str, args: Value| json!({"method": method, "args": args}).to_string();
    let commands: Vec<(Vec<&str>, String)> = vec![
        (
            vec![
                "say",
                &thread,
                "a post to its recipient",
                "--to",
                &recipient,
                "--as",
                &writer,
            ],
            String::new(),
        ),
        (
            vec![
                "say",
                &thread,
                "a post to the orchestrator",
                "--to",
                "@orchestrator",
                "--as",
                &writer,
            ],
            String::new(),
        ),
        (vec!["claim", &claimed, "--as", &writer], String::new()),
        (vec!["inbox", "--as", &orchestrator], String::new()),
        (vec!["inbox", "--as", &recipient], String::new()),
        // The owner's asks, the open ones and those answered in the last day.
        (vec!["asks", "--all"], String::new()),
        (vec!["dispatch"], String::new()),
        // The dispatch's pass for a session with no channel, read on its own output.
        (
            vec!["wait", "--timeout", "0", "--as", &writer],
            String::new(),
        ),
    ];
    let calls = [
        api("get", json!({"slug": thread})),
        api("tasks", json!({"status": "review"})),
        api("ever_executed", json!({"slug": thread})),
        api("sessions", json!({})),
        api("session", json!({"handle": writer})),
        api("ancestors", json!({"slug": thread})),
        api("thread", json!({"slug": thread})),
        api(
            "say",
            json!({"slug": thread, "sender": writer, "text": "a keyed notice", "to": "@orchestrator", "key": "perf"}),
        ),
        api("landings_posted", json!({})),
        api("last_messages", json!({})),
        api("to_owner", json!({"limit": 40})),
        api("inbox", json!({"handle": orchestrator})),
    ];
    for (args, input) in &commands {
        let ran = board.run_unbilled(&relay.addr, args, input);
        assert!(
            ran.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&ran.stderr)
        );
    }
    for call in &calls {
        let ran = board.run_unbilled(&relay.addr, &["api"], call);
        assert!(
            ran.status.success(),
            "{call}: {}",
            String::from_utf8_lossy(&ran.stderr)
        );
        let out = String::from_utf8_lossy(&ran.stdout);
        assert!(out.contains("\"ok\""), "{call}: {out}");
    }
    // The questions waiting on each person, which the Symphony's hub counts again each time the
    // board's lines or its open tasks moved (`symphony::taskview::questions`) and no api call
    // reads: a store's own read, through the relay. The newest verdict it reads once, at its
    // start, is no tick's, and is not planned here.
    let reader = symphony_board::Store::unchecked(
        Client::new(
            &relay.addr,
            symphony_board::NAMESPACE,
            crate::board::DATABASE,
        ),
        Box::new(|| NOW),
        Box::new(|_| false),
        &symphony_board::sys::hostname(),
    );
    let open = [thread.clone(), generated::slug(generated::TASKS - 1)];
    reader.questions_waiting("owner", &open).unwrap();
    let window = reader.thread_window(&thread, None).unwrap();
    assert!(window.lines.len() <= symphony_board::THREAD_WINDOW_LIMIT);
    reader
        .thread_window(&thread, window.lines.first().map(|line| line.id))
        .unwrap();
    let to_first = gave(&board, &first);
    // Another orchestrator's look makes it one that stays, holding fewer than the live one, as a
    // test before this one may have made the waiter: the next give may go to it. Its look is not
    // the check's; the dispatch after it is.
    let joiner = generated::joiner();
    let looked = board.run_unbilled(
        &board.addr,
        &["wait", "--timeout", "0", "--as", &joiner],
        "",
    );
    assert!(
        looked.status.success(),
        "{joiner}'s look: {}",
        String::from_utf8_lossy(&looked.stderr)
    );
    let second = unowned(&board, "second");
    let ran = board.run_unbilled(&relay.addr, &["dispatch"], "");
    assert!(
        ran.status.success(),
        "the dispatch after {joiner}'s look: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let to_second = gave(&board, &second);
    eprintln!("perf: the give passes gave {first} to {to_first}, then {second} to {to_second}");
    // A take of the role, by a session holding it on no task, of one filed after the last pass,
    // which no pass gives first: its read of the taker for the limit, then the rule's write.
    let taken = unowned(&board, "taken");
    let took = board.run_unbilled(
        &relay.addr,
        &["role", &taken, "orchestrator", "--as", &writer],
        "",
    );
    assert!(
        took.status.success(),
        "{writer}'s take of {taken}: {}",
        String::from_utf8_lossy(&took.stderr)
    );
    // A completed task block exercises the prerequisite read and fenced resume write as well
    // as the candidate scan every dispatch makes. Task 1 is done in the generated history.
    let named = format!("task:{}", generated::slug(1));
    let resumed = reader
        .move_on(
            &taken,
            "blocked",
            &writer,
            None,
            Some(symphony_board::On {
                thing: &named,
                why: None,
            }),
        )
        .unwrap();
    assert_eq!(resumed.status, "ready");
    assert!(
        resumed
            .fields
            .get("_board")
            .is_none_or(|fields| !fields.contains_key("blocked"))
    );
    let mut requests = board.first_looks.clone();
    requests.extend(relay.take());
    assert!(
        requests
            .iter()
            .any(|request| request.sql.contains("FROM $required.map"))
    );
    assert!(
        requests
            .iter()
            .any(|request| request.sql.contains("FOR $g IN $watch"))
    );
    let planned = plan_all(&board.client(), &requests);
    assert!(
        planned
            .keys()
            .any(|statement| statement.contains(GIVE_FENCE)),
        "the requests hold the give pass's fence"
    );
    // The take reads the taker's roles by their holder's index alone: a condition on the role's
    // kind beside it has the server read `role_role` too, every orchestrator role on the board,
    // which is on an index and no finding.
    let gaining: Vec<&Request> = (requests.iter())
        .filter(|request| request.sql.contains(GAINING))
        .collect();
    assert!(!gaining.is_empty(), "the requests hold the take's read");
    for request in gaining {
        let roles = statements(&request.sql).into_iter();
        for statement in roles.filter(|statement| statement.contains("FROM role")) {
            let plan = &planned[&statement].plan;
            assert!(
                plan.contains("role_handle") && !plan.contains("role_role"),
                "the take's read of its taker's roles by role_handle alone: {statement}\n{plan}"
            );
        }
    }
    for read in QUESTIONS {
        assert!(
            planned.keys().any(|statement| statement.contains(read)),
            "the requests hold the questions' read {read:?}"
        );
    }
    let failed = failures(&planned);
    let unread = unread(&board.client(), &requests, statements);
    eprintln!(
        "perf: {} requests, {} distinct statements planned, {} with a finding, {} read by the server and not by the check",
        requests.len(),
        planned.len(),
        failed.len(),
        unread.len()
    );
    assert!(failed.is_empty(), "{}", failed.join("\n\n"));
    assert!(
        unread.is_empty(),
        "the server plans a read of rows in each, and the check read no statement out of it:\n{}",
        unread.join("\n\n")
    );
}

/// Each statement of `planned` with its plan, on stderr, and one line for each with a finding.
fn failures(planned: &BTreeMap<String, Planned>) -> Vec<String> {
    let mut failed = Vec::new();
    for (statement, one) in planned {
        let whole = whole_read(&one.plan).map_or(String::new(), |t| format!(" (reads {t} whole)"));
        eprintln!(
            "--- sent {}×{whole}: {statement}\n{}",
            one.sent,
            one.plan.trim_end()
        );
        if !one.findings.is_empty() {
            failed.push(format!(
                "{:?} in {statement}\n{}",
                one.findings,
                one.plan.trim_end()
            ));
        }
    }
    failed
}

/// What the schema's events run in the write that fires them is planned on an index too: `said`'s
/// look back at a sender's word before the one a delete or an edit took, a post's and a verdict's,
/// each the last key of its range. `$before` and `$after` are a word of the generated board's
/// writer, as a delete of it gives them.
#[test]
fn every_query_the_schemas_events_run_is_planned_on_an_index() {
    let board = board();
    let word = json!({"slug": generated::thread(), "sid": 1, "sender": generated::writer(),
        "recipient": null, "kind": "msg", "text": "a word", "ts": NOW});
    let vars = json!({"before": word, "after": word, "event": "DELETE"});
    let defined = events(symphony_board::schema::DEFINE);
    let requests: Vec<Request> = (defined.into_iter())
        .map(|event| Request {
            sql: event,
            vars: vars.clone(),
        })
        .collect();
    let planned = plan_all(&board.client(), &requests);
    let looks_back: Vec<&String> = (planned.keys())
        .filter(|statement| statement.contains("FROM message"))
        .collect();
    assert_eq!(
        looks_back.len(),
        2,
        "said's look back, a post's and a verdict's: {planned:#?}"
    );
    let failed = failures(&planned);
    eprintln!(
        "perf: {} events, {} distinct statements planned, {} with a finding",
        requests.len(),
        planned.len(),
        failed.len()
    );
    assert!(failed.is_empty(), "{}", failed.join("\n\n"));
}

/// `sql` as a request sending the one variable a spelling below reads: a projection, named
/// straight after its keyword.
fn request(sql: &str) -> Request {
    Request {
        sql: sql.to_string(),
        vars: json!({"projection": 1}),
    }
}

/// `sql`, as [`request`] sends it, as the check plans it on the board `client` reaches.
fn checked(client: &Client, sql: &str) -> BTreeMap<String, Planned> {
    plan_all(client, &[request(sql)])
}

/// Exercise the actual unanswered-post lookup a digest runs and its closure bindings on a small
/// owned board: its thread and the thread below it read by record range, and no recipient's suffix
/// of records across the board. This uses the existing recording/planning check without replaying
/// the generated board's commands.
#[test]
fn pending_posts_plan_their_threads_by_range_on_a_small_board() {
    let f = store::Fixture::empty();
    f.task("work");
    f.task_with(symphony_board::NewTask {
        parent: Some("work".into()),
        ..symphony_board::NewTask::new("work-child", "child", "owner")
    });
    let worker = f.session("worker");
    f.store.claim("work", &worker).unwrap();
    f.store
        .say("work", "owner", "continue", Some(&worker))
        .unwrap();
    let proxy = Proxy::to(&store::server_addr());
    let at = f.now() + 31.0 * 60.0;
    let reader = symphony_board::Store::with(
        Client::new(&proxy.addr, "sf", f.store.client().database()),
        Box::new(move || at),
        Box::new(|_| false),
        store::HOST,
    )
    .unwrap();
    proxy.take();
    let digest = symphony_board::cli::pull::digest(&reader, &[], None, at, &[]).unwrap();
    assert_eq!(digest.count("unanswered"), 1);
    let planned = plan_all(f.store.client(), &proxy.take());
    let ranged: Vec<_> = (planned.keys())
        .filter(|sql| sql.contains("FROM message:[$slug,"))
        .collect();
    assert_eq!(
        ranged.len(),
        2,
        "the thread and the one below it, each by its range: {planned:#?}"
    );
    assert!(
        ranged
            .iter()
            .any(|sql| sql.contains("message:[$slug, $after]")),
        "{ranged:#?}"
    );
    let suffix: Vec<_> = (planned.keys())
        .filter(|sql| sql.contains("WITH INDEX message_sender") && sql.contains("sid >"))
        .collect();
    assert!(suffix.is_empty(), "no recipient suffix: {suffix:#?}");
    let failed = failures(&planned);
    assert!(failed.is_empty(), "{}", failed.join("\n\n"));
}

/// What the check finds in `planned`, every statement's findings in turn.
fn found_in(planned: &BTreeMap<String, Planned>) -> Vec<&Finding> {
    (planned.values()).flat_map(|one| &one.findings).collect()
}

/// A read spelled any way the server takes it (a keyword in lower case, after a comment holding a
/// letter outside ASCII, or straight before a comment, `*`, a line, a variable or white space
/// outside ASCII) is planned as the server plans the text as sent, and its read of session under a
/// condition, which no index serves, found; so is a change of session's rows spelled so (its
/// table quoted straight after the verb too), each taken by the server. Of the requests holding
/// the reads, the server plans a read of rows in each, and a reader knowing no spelling has each
/// reported; this one has none. A value whose text holds parentheses reads none, even to a reader
/// knowing no spelling. A keyword quoted, in a comment, a name or an object's key reads nothing.
/// Planned on a server of its own, not the generated board, which the other tests hold.
#[test]
fn a_read_in_any_spelling_the_server_takes_is_planned_and_its_scan_found() {
    let fixture = store::Fixture::empty();
    fixture.session("reviewer");
    let client = fixture.store.client();
    let scan = Finding::FilteredScan("session".into());
    let reads = [
        "select * from session where kind = 'reviewer'",
        "// unicode comment: café\nSELECT * FROM session WHERE kind = 'reviewer'",
        "SELECT/**/ * FROM session WHERE kind = 'reviewer'",
        "SELECT* FROM session WHERE kind = 'reviewer'",
        "select/* café */ * from session where kind = 'reviewer'",
        "SELECT-- comment\n* FROM session WHERE kind = 'reviewer'",
        "SELECT$projection FROM session WHERE kind = 'reviewer'",
        "SELECT\u{a0}* FROM session WHERE kind = 'reviewer'",
        "SELECT\u{2003}* FROM session WHERE kind = 'reviewer'",
        "LET $read = (SELECT\u{3000}* FROM session WHERE kind = 'reviewer')",
    ];
    for sql in reads {
        let mut as_sent = (client.query(&format!("EXPLAIN {sql};"), request(sql).vars))
            .unwrap_or_else(|why| panic!("the server takes {sql:?}: {why:?}"));
        let plan = (as_sent.pop())
            .and_then(|answer| answer.as_str().map(String::from))
            .unwrap_or_default();
        let expected = std::slice::from_ref(&scan);
        assert_eq!(findings(&plan), expected, "{sql:?} as sent:\n{plan}");
        let planned = checked(client, sql);
        assert_eq!(planned.len(), 1, "{sql:?}: {planned:#?}");
        assert_eq!(found_in(&planned), [&scan], "{sql:?}: {planned:#?}");
    }
    let requests: Vec<Request> = reads.map(request).into();
    assert_eq!(unread(client, &requests, statements), Vec::<String>::new());
    let unknown = unread(client, &requests, |_| Vec::new());
    assert_eq!(unknown.len(), reads.len(), "{unknown:#?}");
    let values = ["RETURN '(Ready)'", "RETURN {note: 'status (Ready)'}"];
    let values: Vec<Request> = values.map(request).into();
    assert_eq!(
        unread(client, &values, |_| Vec::new()),
        Vec::<String>::new()
    );
    let changes = [
        "DELETE/**/session WHERE kind = 'nobody'",
        "UPDATE-- the beat\nsession SET last_seen = 1 WHERE(kind = 'nobody')",
        "UPSERT session SET last_seen = 1 WHERE kind = 'nobody'",
        "DELETE\u{a0}FROM\u{2003}session\u{a0}WHERE kind = 'nobody'",
        "delete from⟨session⟩ where kind = 'nobody'",
        "DELETE `session`WHERE kind = 'nobody'",
        "DELETE`session` WHERE kind = 'nobody'",
        "DELETE⟨session⟩ WHERE kind = 'nobody'",
        "UPDATE`session` SET last_seen = 1.0 WHERE kind = 'nobody'",
        "UPSERT⟨session⟩ SET last_seen = 1.0 WHERE kind = 'nobody'",
    ];
    for sql in changes {
        (client.query(&format!("{sql} RETURN NONE;"), json!({})))
            .unwrap_or_else(|why| panic!("the server takes {sql:?}: {why:?}"));
        let planned = checked(client, sql);
        assert_eq!(found_in(&planned), [&scan], "{sql:?}: {planned:#?}");
    }
    let hidden = [
        (
            "SELECT * FROM task WHERE title = 'SELECT* FROM session WHERE kind = 1'",
            "SELECT * FROM task WHERE title = 'SELECT* FROM session WHERE kind = 1'",
        ),
        (
            "SELECT * FROM task -- SELECT/**/ * FROM session WHERE kind = 'x'",
            "SELECT * FROM task",
        ),
        (
            "/* select* from session where kind = 'x' */ SELECT * FROM task",
            "SELECT * FROM task",
        ),
        (
            "SELECT {select: 1, delete/**/: 2} AS keys, ⟨select⟩ FROM task WHERE body.select = 1 OR id = task:update",
            "SELECT {select: 1, delete : 2} AS keys, ⟨select⟩ FROM task WHERE body.select = 1 OR id = task:update",
        ),
    ];
    for (sql, read) in hidden {
        let planned = checked(client, sql);
        let statements: Vec<&String> = planned.keys().collect();
        assert_eq!(statements, [read], "{sql:?}");
        assert_eq!(found_in(&planned), Vec::<&Finding>::new(), "{sql:?}");
    }
}
