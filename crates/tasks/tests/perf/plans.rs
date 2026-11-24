//! The plan check, on PostgreSQL. The commands of every per-write, per-recipient and per-tick path,
//! and a store's own read for a tick's read no command makes, run on the generated board through
//! the recording relay ([`crate::proxy`]) while the server gives the plan of every statement they
//! run, each a block or a trigger runs too, as a notice the relay keeps ([`Board::explained`]); the
//! recipients' first looks were
//! planned so as the board was made ([`crate::board::Looked`]). The check fails on what
//! [`crate::findings`] finds in a plan: a scan of message, delivery, role, session or
//! callback_state under a condition no index serves, or a subquery run again for every row that
//! reads a table by no index each time. A statement a request sent that the server plans and no
//! plan was given of fails it too, and so does a plan it cannot read: neither is skipped.
//! Deterministic: the plans, never a time.
//!
//! Before its commands it files a task nobody orchestrates, so the give pass a dispatch and a look
//! make is among what it plans on every run, whatever the tests before it on the same board left
//! ([`unowned`]); then, after a look by another orchestrator that makes it one that stays (as the
//! timings' looks make theirs), it files another and dispatches again. Each give is judged by its
//! having been made ([`gave`]), never by who the board picked, which turns on the orchestrators
//! earlier tests made stay. Last, a session holding no orchestrator role takes it on a third such
//! task, so the read of the taker's roles that the limit on the open tasks an orchestrator holds
//! makes ([`GAINING`]) is planned, and held to its holder's index alone.

use crate::board::{Board, DATABASE, board};
use crate::findings::{Finding, Logged, findings, indexes, outline, whole_reads};
use crate::generated::{self, NOW};
use crate::proxy::{Proxy, Request};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use symphony_board::{Database, NewTask, On, Opening, Raw, Store, sys};

/// One statement as the server planned it: how often it was planned, a plan of it (one with a finding,
/// where one has), and what is wrong with any of its plans.
#[derive(Debug)]
pub struct Planned {
    pub sent: usize,
    pub plan: Value,
    pub findings: Vec<Finding>,
}

/// Each distinct statement `logged` holds a plan of, by its text.
pub fn planned(logged: &Logged) -> BTreeMap<String, Planned> {
    let mut planned: BTreeMap<String, Planned> = BTreeMap::new();
    for one in &logged.plans {
        let found = findings(&one.plan);
        match planned.get_mut(&one.query) {
            Some(seen) => {
                seen.sent += 1;
                if seen.findings.is_empty() && !found.is_empty() {
                    seen.plan = one.plan.clone();
                }
                seen.findings.extend(found);
                seen.findings.sort();
                seen.findings.dedup();
            }
            None => {
                planned.insert(
                    one.query.clone(),
                    Planned {
                        sent: 1,
                        plan: one.plan.clone(),
                        findings: found,
                    },
                );
            }
        }
    }
    planned
}

/// Whether the server plans `sql`, a statement a request sent: a `SELECT`, a `WITH`, or a row
/// written. A block (`DO`), a setting or a schema's statement it runs without a plan.
fn planned_by_the_server(sql: &str) -> bool {
    let head: String = (sql.trim_start().chars())
        .take_while(char::is_ascii_alphabetic)
        .collect();
    ["SELECT", "WITH", "INSERT", "UPDATE", "DELETE"]
        .iter()
        .any(|verb| verb.eq_ignore_ascii_case(&head))
}

/// Each statement `requests` sent that the server plans and `planned` holds no plan of: a plan
/// the server should have given and did not, reported rather than left unchecked.
pub fn unplanned(requests: &[Request], planned: &BTreeMap<String, Planned>) -> Vec<String> {
    let mut missing: Vec<String> = (requests.iter())
        .flat_map(|request| &request.steps)
        .filter(|step| planned_by_the_server(&step.sql) && !planned.contains_key(&step.sql))
        .map(|step| step.sql.clone())
        .collect();
    missing.sort();
    missing.dedup();
    missing
}

/// One line for each statement of `planned` with a finding, its plan's outline with it; each
/// statement and each whole read is said on stderr.
pub fn failures(planned: &BTreeMap<String, Planned>) -> Vec<String> {
    let mut failed = Vec::new();
    for (statement, one) in planned {
        let whole = whole_reads(&one.plan);
        let whole = match whole.is_empty() {
            true => String::new(),
            false => format!(" (reads {} whole)", whole.join(", ")),
        };
        let short: String = statement.split_whitespace().collect::<Vec<_>>().join(" ");
        let short: String = short.chars().take(200).collect();
        eprintln!("--- planned {}×{whole}: {short}", one.sent);
        if !whole.is_empty() || !one.findings.is_empty() {
            eprint!("{}", outline(&one.plan));
        }
        if !one.findings.is_empty() {
            failed.push(format!(
                "{:?} in {statement}\n{}",
                one.findings,
                outline(&one.plan).trim_end()
            ));
        }
    }
    failed
}

/// What the check holds a run's plans to: no finding, no statement sent and left unplanned, no
/// plan left unread. What is wrong, one paragraph each.
pub fn wrong(
    planned: &BTreeMap<String, Planned>,
    requests: &[Request],
    logged: &Logged,
) -> Vec<String> {
    let mut wrong = failures(planned);
    wrong.extend(
        unplanned(requests, planned)
            .into_iter()
            .map(|sql| format!("sent, and the server gave no plan of it: {sql}")),
    );
    wrong.extend(
        (logged.unread.iter()).map(|notice| format!("a plan the check could not read:\n{notice}")),
    );
    wrong
}

/// A statement of the give pass's fence ([`symphony_board::Store::give_unowned`]): each task it
/// decided on, at the version it read.
const GIVE_FENCE: &str = "jsonb_array_elements(v -> 'fenced')";

/// What a take of the orchestrator role reads of its taker for the limit on the open tasks it holds
/// the role on: its slot counter beside its roles, in that read alone.
const GAINING: &str = "jsonb_build_array('slots', v ->> 'handle')";

/// The read of [`symphony_board::Store::questions_waiting`]: the questions to a person, by the
/// recipient's index, and whether a post of theirs follows each on its thread.
const QUESTIONS: [&str; 2] = ["recipient = v ->> 'person'", "a.sender = v ->> 'person'"];

/// The resume's read of the tasks a block waited on, and its fenced write over them, as a
/// completed block of a task sends them.
const REQUIRED: &str = "jsonb_array_elements_text(v -> 'required')";
const WATCHED_RESUME: &str = "FOR g IN SELECT jsonb_array_elements(v -> 'watch')";

/// A new top-level task nobody orchestrates on `board`, named for `which` and this run, filed by the
/// owner as a plain create, which gives none: the next reader of the whole board that gives (`tasks
/// dispatch`, a look of `tasks wait`) gives it, whatever the board's other tasks are, and however
/// earlier tests left them.
fn unowned(board: &Board, which: &str) -> String {
    let store = board.store(&sys::hostname());
    let slug = format!("perf/unowned-{which}-{}", std::process::id());
    let new = NewTask::new(&slug, "Nobody orchestrates it", "owner");
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
fn gave(board: &Board, slug: &str) -> String {
    let task = board.store(&sys::hostname()).get(slug).unwrap();
    let held: Vec<&str> = (task.roles.iter())
        .filter(|role| role.role == "orchestrator")
        .map(|role| role.handle.as_str())
        .collect();
    let db = board.database();
    let kinds: Vec<Value> = (held.iter())
        .map(|handle| {
            let rows = Raw::of(&db)
                .table("session")
                .matching(json!({"handle": handle}));
            rows.fields(&["kind"]).unwrap()[0]["kind"].clone()
        })
        .collect();
    let waiting = (task.fields.get("_board")).and_then(|fields| fields.get("waiting"));
    match (held.as_slice(), kinds.as_slice(), waiting) {
        ([one], [kind], _) if *kind == json!("orchestrator") => (*one).to_owned(),
        ([], _, Some(waiting)) if waiting.is_number() => "waiting".to_owned(),
        _ => panic!(
            "no give pass gave {slug} to one orchestrator or marked it waiting: held by {held:?} of kinds {kinds:?}, waiting {waiting:?}"
        ),
    }
}

/// A store of the board through `relay`, on this host, its clock at [`NOW`], that takes the
/// board's tables as they are and opens with no request of its own.
fn reader(relay: &Proxy) -> Store {
    Store::unchecked_on(
        relay.client(DATABASE).into(),
        Box::new(|| NOW),
        Box::new(|_| false),
        &sys::hostname(),
        &Opening::write(),
    )
}

/// `tasks ARGS` through `relay`, unbilled, which must succeed; its output.
fn ran(board: &Board, relay: &Proxy, args: &[&str], input: &str) -> String {
    let ran = board.run_unbilled(&relay.addr, args, input);
    assert!(
        ran.status.success(),
        "{args:?} {input}: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8_lossy(&ran.stdout).into_owned()
}

#[test]
fn every_statement_a_write_a_recipient_or_a_tick_sends_is_planned_on_an_index() {
    let mut board = board();
    // The give pass runs on a condition the check sets, never on what earlier tests or the clock
    // left: a task nobody orchestrates, made before the plans are logged.
    let first = unowned(&board, "first");
    let relay = Proxy::to(&board.addr);
    let thread = generated::thread();
    let writer = generated::writer();
    let recipient = generated::recipient();
    let orchestrator = generated::orchestrator();
    let claimed = board.unclaimed();
    let api = |method: &str, args: Value| json!({"method": method, "args": args}).to_string();
    let commands: Vec<Vec<&str>> = vec![
        vec![
            "say",
            &thread,
            "a post to its recipient",
            "--to",
            &recipient,
            "--as",
            &writer,
        ],
        vec![
            "say",
            &thread,
            "a post to the orchestrator",
            "--to",
            "@orchestrator",
            "--as",
            &writer,
        ],
        vec!["claim", &claimed, "--as", &writer],
        vec!["inbox", "--as", &orchestrator],
        vec!["inbox", "--as", &recipient],
        // The owner's asks, the open ones and those answered in the last day.
        vec!["asks", "--all"],
        vec!["dispatch"],
        // The dispatch's pass for a session with no channel, read on its own output.
        vec!["wait", "--timeout", "0", "--as", &writer],
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
    // The questions waiting on each person, which the Symphony's hub counts again each time the
    // board's lines or its open tasks moved (`symphony::taskview::questions`) and no api call
    // reads: a store's own read, through the relay. The newest verdict it reads once, at its
    // start, is no tick's, and is not planned here.
    let reader = reader(&relay);
    let open = [thread.clone(), generated::slug(generated::TASKS - 1)];
    let ((), mut logged) = board.explained(&relay, || {
        for args in &commands {
            ran(&board, &relay, args, "");
        }
        for call in &calls {
            let out = ran(&board, &relay, &["api"], call);
            assert!(out.contains("\"ok\""), "{call}: {out}");
        }
        reader.questions_waiting("owner", &open).unwrap();
        let window = reader.thread_window(&thread, None).unwrap();
        assert!(window.lines.len() <= symphony_board::THREAD_WINDOW_LIMIT);
        reader
            .thread_window(&thread, window.lines.first().map(|line| line.id))
            .unwrap();
    });
    let to_first = gave(&board, &first);
    // Another orchestrator's look makes it one that stays, holding fewer than the live one, as a
    // test before this one may have made the waiter: the next give may go to it. Its look is not
    // the check's, and is not planned; the dispatch after it is.
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
    let (_, more) = board.explained(&relay, || ran(&board, &relay, &["dispatch"], ""));
    logged.extend(more);
    let to_second = gave(&board, &second);
    eprintln!("perf: the give passes gave {first} to {to_first}, then {second} to {to_second}");
    // A take of the role, by a session holding it on no task, of one filed after the last pass,
    // which no pass gives first: its read of the taker for the limit, then the rule's write. A
    // completed task block exercises the prerequisite read and fenced resume write as well as the
    // candidate scan every dispatch makes. Task 1 is done in the generated history.
    let taken = unowned(&board, "taken");
    let named = format!("task:{}", generated::slug(1));
    let ((), more) = board.explained(&relay, || {
        ran(
            &board,
            &relay,
            &["role", &taken, "orchestrator", "--as", &writer],
            "",
        );
        let resumed = reader
            .move_on(
                &taken,
                "blocked",
                &writer,
                None,
                Some(On {
                    thing: &named,
                    why: None,
                }),
            )
            .unwrap();
        assert_eq!(resumed.status, "ready");
        assert!(
            (resumed.fields.get("_board")).is_none_or(|fields| !fields.contains_key("blocked"))
        );
    });
    logged.extend(more);
    let mut requests = board.first_looks.requests.clone();
    requests.extend(relay.take());
    let mut every = board.first_looks.logged.clone();
    every.extend(logged);
    for read in [REQUIRED, WATCHED_RESUME] {
        assert!(
            requests.iter().any(|request| request.holds(read)),
            "the requests hold the resume's {read:?}"
        );
    }
    let planned = planned(&every);
    assert!(
        planned
            .keys()
            .any(|statement| statement.contains(GIVE_FENCE)),
        "the give pass's fence is planned"
    );
    // The take reads the taker's roles by their holder's index alone: a condition on the role's
    // kind beside it has the server read `role_role` too, every orchestrator role on the board,
    // which is on an index and no finding.
    let gaining: Vec<(&String, &Planned)> = (planned.iter())
        .filter(|(statement, _)| statement.contains(GAINING))
        .collect();
    assert!(!gaining.is_empty(), "the take's read is planned");
    for (statement, one) in gaining {
        let read = indexes(&one.plan);
        assert!(
            read.contains("role_handle") && !read.contains("role_role"),
            "the take's read of its taker's roles by role_handle alone: {statement}\n{}",
            outline(&one.plan)
        );
    }
    for read in QUESTIONS {
        assert!(
            planned.keys().any(|statement| statement.contains(read)),
            "the questions' read {read:?} is planned"
        );
    }
    let wrong = wrong(&planned, &requests, &every);
    eprintln!(
        "perf: {} requests, {} plans given, {} distinct statements, {} wrong",
        requests.len(),
        every.plans.len(),
        planned.len(),
        wrong.len()
    );
    assert!(wrong.is_empty(), "{}", wrong.join("\n\n"));
}

/// A sender of a few words the generated board has none of, whose word the trigger looks back
/// past every other sender's for.
const RARE: &str = "perf-plans-rare-sender";

/// What the schema's triggers run in the write that fires them is planned on an index too, as the
/// server runs it in that write: `said`'s look back at a sender's word before the one a delete
/// took, a rare sender's and the board's busiest writer's, each the last key of its sender's range.
#[test]
fn every_statement_the_triggers_run_is_planned_on_an_index() {
    let board = board();
    let relay = Proxy::to(&board.addr);
    let db: Database = relay.client(DATABASE).into();
    let raw = Raw::of(&db);
    let writer = generated::writer();
    let words = [
        (RARE, "perf/plans-rare-first", 1),
        (RARE, "perf/plans-rare-newest", 1_000_000_010),
        (writer.as_str(), "perf/plans-writer-newest", 1_000_000_011),
    ];
    let ((), logged) = board.explained(&relay, || {
        for (sender, slug, sid) in words {
            (raw.row("message", json!([slug, sid])))
                .create(
                    json!({"sid": sid, "slug": slug, "sender": sender, "recipient": null,
                    "kind": "msg", "text": "a word", "ts": NOW}),
                )
                .unwrap_or_else(|why| panic!("{sender}'s word {sid} on {slug}: {why:?}"));
        }
        for (sender, slug, sid) in words.iter().rev() {
            (raw.row("message", json!([slug, sid])).delete())
                .unwrap_or_else(|why| panic!("the delete of {sender}'s word {sid}: {why:?}"));
        }
    });
    let planned = planned(&logged);
    let looks_back: Vec<&String> = (planned.keys())
        .filter(|statement| {
            statement.contains("FROM message m") && statement.contains("OLD.sender")
        })
        .collect();
    assert!(
        !looks_back.is_empty(),
        "said's look back is planned: {:#?}",
        planned.keys().collect::<Vec<_>>()
    );
    let wrong = wrong(&planned, &relay.take(), &logged);
    eprintln!(
        "perf: the triggers: {} plans given, {} distinct statements, {} wrong",
        logged.plans.len(),
        planned.len(),
        wrong.len()
    );
    assert!(wrong.is_empty(), "{}", wrong.join("\n\n"));
}

#[cfg(test)]
mod check {
    use super::*;
    use crate::findings::Plan;
    use crate::proxy::Sent;

    fn plan(query: &str, plan: Value) -> Plan {
        Plan {
            query: query.into(),
            plan,
        }
    }

    /// A statement planned twice is one, sent twice, its findings each plan's; one sent and never
    /// planned is reported, a block, a setting or a vacuum not, as the server plans none of them;
    /// an entry the check could not read is reported.
    #[test]
    fn a_statement_unplanned_and_a_plan_unread_are_reported() {
        let scan = json!({"Node Type": "Seq Scan", "Relation Name": "message",
            "Filter": "(recipient = 'perf-0007'::text)"});
        let indexed = json!({"Node Type": "Index Scan", "Relation Name": "message",
            "Index Name": "message_recipient", "Index Cond": "(recipient = 'perf-0007'::text)"});
        let logged = Logged {
            plans: vec![
                plan(
                    "SELECT 1 FROM message WHERE recipient = $1",
                    indexed.clone(),
                ),
                plan("SELECT 1 FROM message WHERE recipient = $1", scan),
                plan("SELECT 2", indexed),
            ],
            unread: vec!["2026-11-22 10:00:00.000 UTC [7] LOG:  duration: 1 ms  plan:\n{".into()],
        };
        let planned = planned(&logged);
        assert_eq!(planned.len(), 2);
        let first = &planned["SELECT 1 FROM message WHERE recipient = $1"];
        assert_eq!(first.sent, 2);
        assert_eq!(first.findings, [Finding::FilteredScan("message".into())]);
        assert_eq!(
            first.plan["Node Type"], "Seq Scan",
            "the plan with the finding is kept"
        );
        let sent = |sqls: &[&str]| {
            Request::of(
                (sqls.iter())
                    .map(|sql| Sent {
                        sql: sql.to_string(),
                        param: None,
                    })
                    .collect(),
            )
        };
        let requests = [
            sent(&["SELECT 2", "DO $board$ BEGIN NULL; END $board$"]),
            sent(&["  with x AS (SELECT 1) SELECT * FROM x", "VACUUM (ANALYZE)"]),
            sent(&["SET statement_timeout = 1", "SELECT 2"]),
        ];
        assert_eq!(
            unplanned(&requests, &planned),
            ["  with x AS (SELECT 1) SELECT * FROM x"]
        );
        let wrong = wrong(&planned, &requests, &logged);
        assert_eq!(wrong.len(), 3, "{wrong:#?}");
        assert!(wrong[0].starts_with("[FilteredScan(\"message\")] in SELECT 1 FROM message"));
        assert!(wrong[1].starts_with("sent, and the server gave no plan of it"));
        assert!(wrong[2].starts_with("a plan the check could not read"));
    }
}
