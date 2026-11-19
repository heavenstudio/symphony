//! The attach monitor's unit tests: what a session sees of the board, and the pane watch.

use super::*;
use crate::cli::agents::{Answer, Pane};
use crate::health::Health;
use crate::rows::{SessionRow, TaskSessionRow};
use crate::{Board, Role, Session, Task};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

fn task(slug: &str, parent: Option<&str>, roles: &[(&str, &str)]) -> Task {
    Task {
        branch: None,
        id: 0,
        slug: slug.into(),
        title: String::new(),
        body: String::new(),
        status: "in_progress".into(),
        priority: 2,
        parent: parent.map(str::to_string),
        created_by: String::new(),
        created: 0.0,
        updated: 0.0,
        links: Vec::new(),
        depends: Vec::new(),
        roles: roles
            .iter()
            .map(|(handle, role)| Role {
                handle: handle.to_string(),
                role: role.to_string(),
                since: 0.0,
                expires: None,
                health: Health::Live,
            })
            .collect(),
        instruction: false,
        fields: Default::default(),
    }
}

fn session(handle: &str, parent: Option<&str>) -> Session {
    Session {
        row: SessionRow {
            handle: handle.into(),
            parent: parent.map(str::to_string),
            ..SessionRow::default()
        },
        health: Health::Live,
    }
}

fn launched(handle: &str, slug: &str, pane: &str, conversation: &str) -> TaskSessionRow {
    TaskSessionRow {
        slug: slug.into(),
        handle: handle.into(),
        pane: pane.into(),
        claude_session: conversation.into(),
        ..TaskSessionRow::default()
    }
}

/// Terminal `id` as `hc get` describes it: a codex resuming `conversation`, or a shell given "".
fn pane(id: &str, status: &str, conversation: &str) -> Pane {
    let argv = if conversation.is_empty() {
        json!(["zsh"])
    } else {
        json!(["codex", "--no-daemon", "resume", conversation])
    };
    let terminal = json!({"address": id, "status": status, "argv": argv});
    Pane::from_hc(terminal.as_object().unwrap()).unwrap()
}

/// The clock of a look that names none.
const NOW: f64 = 1_000.0;

/// One look, and the lines it said.
fn look(
    board: &Board,
    who: &str,
    open: &[TaskSessionRow],
    panes: Answer<Vec<Pane>>,
    before: &PaneWatch,
) -> (PaneWatch, Vec<String>) {
    look_at(board, who, open, panes, NOW, before)
}

/// One look at `now`, and the lines it said.
fn look_at(
    board: &Board,
    who: &str,
    open: &[TaskSessionRow],
    panes: Answer<Vec<Pane>>,
    now: f64,
    before: &PaneWatch,
) -> (PaneWatch, Vec<String>) {
    let mut said = Vec::new();
    let after = pane_watch(
        board,
        &overseen(board, who),
        open,
        &|| panes.clone(),
        now,
        before,
        &mut |line| said.push(line.to_string()),
    );
    (after, said)
}

#[test]
fn one_oversees_the_roles_below_what_it_orchestrates_and_its_children() {
    // Never itself, nor a child that left; a task it only reviews is not its to oversee.
    let mut gone = session("gone", Some("orch"));
    gone.row.left_at = Some(1.0);
    let board = Board {
        tasks: vec![
            task("t/root", None, &[("orch", "orchestrator")]),
            task("t/root/a", Some("t/root"), &[("deep", "reviewer")]),
            task("t/root/a/b", Some("t/root/a"), &[("deeper", "executor")]),
            task(
                "t/side",
                None,
                &[("side", "executor"), ("orch", "reviewer")],
            ),
        ],
        sessions: ["orch", "deep", "deeper", "side"]
            .into_iter()
            .map(|h| session(h, None))
            .chain([session("kid", Some("orch")), gone])
            .collect(),
        latest_message: 0,
        workflow_generation: None,
    };
    assert_eq!(
        overseen(&board, "orch").sessions,
        BTreeSet::from(["deep", "deeper", "kid"])
    );
    assert_eq!(overseen(&board, "deep").sessions, BTreeSet::new());
}

#[test]
fn a_subtree_another_orchestrator_holds_is_that_ones_to_see() {
    let mut ready = task("t/free", None, &[]);
    ready.status = "ready".into();
    let mut theirs = task("t/root/b/ready", Some("t/root/b"), &[]);
    theirs.status = "ready".into();
    let board = Board {
        tasks: vec![
            task("t/root", None, &[("orch", "orchestrator")]),
            task("t/root/a", Some("t/root"), &[("mine", "executor")]),
            task(
                "t/root/b",
                Some("t/root"),
                &[("other", "orchestrator"), ("theirs", "executor")],
            ),
            theirs,
            ready,
        ],
        sessions: ["orch", "other", "mine", "theirs"]
            .into_iter()
            .map(|h| session(h, None))
            .collect(),
        latest_message: 0,
        workflow_generation: None,
    };
    assert_eq!(overseen(&board, "orch").sessions, BTreeSet::from(["mine"]));
    assert_eq!(
        overseen(&board, "other").sessions,
        BTreeSet::from(["theirs"])
    );
    let orch = seen(&board, "orch", "orchestrator");
    // Its own, and the unclaimed task every orchestrator follows; never other's subtree.
    assert_eq!(
        orch.statuses.keys().collect::<Vec<_>>(),
        ["t/free", "t/root", "t/root/a"]
    );
    assert_eq!(orch.ready.keys().collect::<Vec<_>>(), ["t/free"]);
    let theirs = seen(&board, "other", "orchestrator");
    assert_eq!(
        theirs.ready.keys().collect::<Vec<_>>(),
        ["t/free", "t/root/b/ready"]
    );
    // A worker's children stay its parent's to hear, whatever orchestrates their work.
    let mut board = board;
    board.sessions.push(session("sub", Some("mine")));
    let children = seen(&board, "mine", "worker").children;
    assert_eq!(children.get("sub"), Some(&Health::Live), "{children:?}");
}

#[test]
fn a_parent_task_owner_hears_only_its_direct_coordinator_on_the_delegated_task() {
    let mut root = session("root", None);
    root.row.kind = "orchestrator".into();
    let mut coordinator = session("coordinator", Some("root"));
    coordinator.row.kind = "orchestrator".into();
    let mut board = Board {
        tasks: vec![
            task("root", None, &[("root", "orchestrator")]),
            task(
                "delegated",
                Some("root"),
                &[
                    ("coordinator", "orchestrator"),
                    ("coordinator", "executor"),
                    ("worker", "reviewer"),
                ],
            ),
            task(
                "foreign",
                None,
                &[("foreign", "orchestrator"), ("coordinator", "reviewer")],
            ),
        ],
        sessions: vec![
            root,
            coordinator,
            session("worker", Some("coordinator")),
            session("foreign", None),
        ],
        latest_message: 0,
        workflow_generation: None,
    };
    let hears = |board: &Board, who: &str, slug: &str| {
        overseen(board, "root").hears_of(
            board,
            &crate::orchestration::Owners::new(&board.tasks),
            who,
            slug,
        )
    };
    assert!(hears(&board, "coordinator", "delegated"));
    assert!(!hears(&board, "worker", "delegated"));
    assert!(!hears(&board, "coordinator", "foreign"));
    assert!(!hears(&board, "root", "root"));
    board.sessions[1].row.left_at = Some(1.0);
    assert!(!hears(&board, "coordinator", "delegated"));
    board.sessions[1].row.left_at = None;
    board.sessions[1].row.parent = None;
    assert!(!hears(&board, "coordinator", "delegated"));
    board.sessions[1].row.parent = Some("root".into());
    board.tasks[1].roles[0].handle = "foreign".into();
    assert!(!hears(&board, "coordinator", "delegated"));
}

#[test]
fn only_recognized_generated_pane_notices_receive_historical_wording() {
    let legacy = "BLOCKED worker task t2 — its terminal waits on an approval or a question";
    let observed = "BLOCKED worker task t2 — observed waiting on an approval or a question; inspect its terminal before acting";
    assert_eq!(PaneWatch::delivery_text("pane-2-0", legacy), observed);
    assert_eq!(PaneWatch::delivery_text("pane-2-0", observed), observed);
    for key in [
        "message-1",
        "event-2",
        "pull-3",
        "pane-user",
        "pane-2-0-extra",
        "pane--0",
    ] {
        assert_eq!(PaneWatch::delivery_text(key, legacy), legacy, "{key}");
    }
    for text in [
        "BLOCKED worker task t2 — please approve",
        "BLOCKED worker task t2 extra — its terminal waits on an approval or a question",
        "WATCH hc unanswered: user prose",
    ] {
        assert_eq!(PaneWatch::delivery_text("pane-2-0", text), text);
    }
    assert_eq!(
        PaneWatch::delivery_text(
            "pane-2-1",
            "WATCH hc unanswered: down — blocked terminals go unreported until it answers"
        ),
        "WATCH hc unanswered at observation: down — blocked terminals may have gone unreported; inspect hc before acting"
    );
    let observed = "WATCH hc unanswered at observation: down — blocked terminals may have gone unreported; inspect hc before acting";
    assert_eq!(PaneWatch::delivery_text("pane-2-1", observed), observed);
}

#[test]
fn watermarks_reject_older_unknown_and_resolved_equal_turn_observations() {
    let board = Board {
        tasks: vec![task(
            "t/a",
            None,
            &[("orch", "orchestrator"), ("w", "executor")],
        )],
        sessions: vec![session("orch", None), session("w", None)],
        latest_message: 0,
        workflow_generation: None,
    };
    let open = [launched("w", "t/a", "t1", "")];
    let legacy: PaneWatch = serde_json::from_value(json!({
        "blocked": {"w": {"task":"t/a", "pane":"t1"}}, "unanswered":false
    }))
    .unwrap();
    assert!(legacy.known.is_empty());
    assert_eq!(legacy.blocked["w"].turn, None);
    assert_eq!(
        (legacy.unanswered_since, legacy.unanswered_at),
        (None, None)
    );
    let mut before = PaneWatch::default();
    for (status, turn, alerts, blocked, known) in [
        ("blocked", Some(3), 1, true, 3),
        ("blocked", Some(3), 0, true, 3),
        ("blocked", Some(4), 1, true, 4),
        ("blocked", Some(3), 0, true, 4),
        ("working", Some(3), 0, true, 4),
        ("blocked", None, 0, true, 4),
        ("working", None, 0, true, 4),
        ("working", Some(4), 0, false, 4),
        ("blocked", Some(4), 0, false, 4),
        ("blocked", Some(3), 0, false, 4),
        ("blocked", Some(5), 1, true, 5),
        ("idle", Some(5), 0, false, 5),
        ("blocked", Some(6), 1, true, 6),
    ] {
        let mut observed = pane("t1", status, "");
        observed.turn = turn;
        let (after, said) = look(&board, "orch", &open, Ok(vec![observed]), &before);
        assert_eq!(said.len(), alerts, "{status} {turn:?}: {said:?}");
        assert_eq!(
            after.blocked.contains_key("w"),
            blocked,
            "{status} {turn:?}"
        );
        assert_eq!(after.known["w"].turn, Some(known));
        if blocked {
            assert_eq!(after.blocked["w"].turn, Some(known));
        }
        before = after;
    }
    let (unknown, said) = look(&board, "orch", &open, Err("down".into()), &before);
    assert_eq!(said, Vec::<String>::new(), "one failed sample says nothing");
    assert_eq!(unknown.blocked, before.blocked);
    assert_eq!(unknown.known, before.known);
    let (retired, _) = look(&board, "orch", &[], Err("down".into()), &unknown);
    assert_eq!(retired, PaneWatch::default());
}

#[test]
fn a_launched_terminal_is_found_by_its_conversation_once_its_recorded_address_is_gone() {
    let board = Board {
        tasks: vec![task(
            "t/a",
            None,
            &[("orch", "orchestrator"), ("w", "executor")],
        )],
        sessions: vec![session("orch", None), session("w", None)],
        latest_message: 0,
        workflow_generation: None,
    };
    let open = [launched("w", "t/a", "t1", "codex:thread-1")];
    let (after, said) = look(
        &board,
        "orch",
        &open,
        Ok(vec![pane("t7", "blocked", "thread-1")]),
        &PaneWatch::default(),
    );
    assert_eq!(
        said,
        [
            "BLOCKED w t/a t7 — observed waiting on an approval or a question; inspect its terminal before acting"
        ]
    );
    assert_eq!(after.blocked["w"].pane, "t7");
}

#[test]
fn a_session_no_longer_launched_is_forgotten_while_hc_cannot_be_asked() {
    let board = Board {
        tasks: vec![task(
            "t/a",
            None,
            &[
                ("orch", "orchestrator"),
                ("w", "executor"),
                ("v", "reviewer"),
            ],
        )],
        sessions: vec![
            session("orch", None),
            session("w", None),
            session("v", None),
        ],
        latest_message: 0,
        workflow_generation: None,
    };
    let blocked = |pane: &str| Blocked {
        task: "t/a".into(),
        pane: pane.into(),
        turn: None,
    };
    let before = PaneWatch {
        blocked: BTreeMap::from([("w".into(), blocked("t1")), ("v".into(), blocked("t2"))]),
        unanswered: false,
        ..PaneWatch::default()
    };
    // v's pane closed: only w is still launched.
    let open = [launched("w", "t/a", "t1", "")];
    let (first, said) = look(&board, "orch", &open, Err("down".into()), &before);
    assert_eq!(said, Vec::<String>::new());
    let failing = PaneWatch {
        blocked: BTreeMap::from([("w".into(), blocked("t1"))]),
        unanswered_since: Some(NOW),
        unanswered_at: Some(NOW),
        ..PaneWatch::default()
    };
    assert_eq!(first, failing);
    let later = NOW + OUTAGE_SECONDS;
    let (after, said) = look_at(&board, "orch", &open, Err("down".into()), later, &first);
    assert_eq!(
        said,
        [
            "WATCH hc unanswered at observation: down — blocked terminals may have gone unreported; inspect hc before acting"
        ]
    );
    let expected = PaneWatch {
        unanswered: true,
        unanswered_at: Some(later),
        ..failing
    };
    assert_eq!(after, expected);
    let (again, said) = look_at(
        &board,
        "orch",
        &open,
        Err("down".into()),
        later + 1.0,
        &after,
    );
    assert_eq!((again, said), (expected, Vec::new()));
}

/// The times a run of samples, each `(at, answered)`, said WATCH at, from the watch `before`, of
/// `w` launched into t1 for t/a, which `orch` orchestrates; and what the watch keeps after them.
fn watched_at(samples: &[(f64, bool)], before: PaneWatch) -> (Vec<f64>, PaneWatch) {
    let board = Board {
        tasks: vec![task(
            "t/a",
            None,
            &[("orch", "orchestrator"), ("w", "executor")],
        )],
        sessions: vec![session("orch", None), session("w", None)],
        latest_message: 0,
        workflow_generation: None,
    };
    let open = [launched("w", "t/a", "t1", "")];
    let mut watch = before;
    let mut said_at = Vec::new();
    for &(at, answered) in samples {
        let panes = match answered {
            true => Ok(vec![pane("t1", "working", "")]),
            false => Err("down".into()),
        };
        let (after, said) = look_at(&board, "orch", &open, panes, at, &watch);
        assert!(
            said.iter()
                .all(|line| line.starts_with("WATCH hc unanswered")),
            "{said:?}"
        );
        said_at.extend(said.iter().map(|_| at));
        watch = after;
    }
    (said_at, watch)
}

/// The server clock's samples, one a second from `from` to `to`, each answered or failed.
fn every_second(from: u32, to: u32, answered: bool) -> Vec<(f64, bool)> {
    (from..=to).map(|at| (f64::from(at), answered)).collect()
}

/// A run of samples, in order.
fn runs(parts: &[Vec<(f64, bool)>]) -> Vec<(f64, bool)> {
    parts.concat()
}

#[test]
fn one_failed_sample_between_answers_says_nothing() {
    // The incident: hc failing at one sample in four under load, for two minutes.
    let flapping: Vec<_> = (0..=120).map(|at| (f64::from(at), at % 4 != 3)).collect();
    let (said, after) = watched_at(&flapping, PaneWatch::default());
    assert_eq!(said, Vec::<f64>::new());
    assert_eq!(
        (
            after.unanswered,
            after.unanswered_since,
            after.unanswered_at
        ),
        (false, None, None),
        "an answer forgets the failed sample"
    );
    // A stretch short of its last second, then an answer, says nothing either.
    let short = runs(&[every_second(0, 29, false), every_second(30, 30, true)]);
    assert_eq!(
        watched_at(&short, PaneWatch::default()).0,
        Vec::<f64>::new()
    );
}

#[test]
fn hc_failing_at_every_sample_for_the_stretch_says_watch_once() {
    let (said, after) = watched_at(&every_second(0, 600, false), PaneWatch::default());
    assert_eq!(
        said,
        [OUTAGE_SECONDS],
        "once, at the sample that ends the stretch"
    );
    assert!(after.unanswered);
    // A wait's looks, 15 s apart: two gaps make the stretch.
    let looks = [(0.0, false), (15.0, false), (30.0, false), (45.0, false)];
    assert_eq!(watched_at(&looks, PaneWatch::default()).0, [30.0]);
}

#[test]
fn once_said_a_failing_sample_leaves_the_record_as_it_was() {
    // So a tick of an outage already said takes no lease and writes nothing.
    let (_, said) = watched_at(&every_second(0, 30, false), PaneWatch::default());
    for at in [31.0, 300.0, 3_600.0, 10.0] {
        assert_eq!(
            watched_at(&[(at, false)], said.clone()),
            (vec![], said.clone()),
            "{at}"
        );
    }
}

#[test]
fn an_answer_ends_the_outage_and_rearms_the_watch() {
    let twice = runs(&[
        every_second(0, 30, false),
        every_second(31, 31, true),
        every_second(32, 70, false),
    ]);
    assert_eq!(watched_at(&twice, PaneWatch::default()).0, [30.0, 62.0]);
    // An answer inside the stretch starts it again from the next failed sample.
    let broken = runs(&[
        every_second(0, 20, false),
        every_second(21, 21, true),
        every_second(22, 60, false),
    ]);
    assert_eq!(watched_at(&broken, PaneWatch::default()).0, [52.0]);
}

#[test]
fn a_sleep_spanning_the_stretch_is_not_an_outage() {
    // The clock's last sample before the sleep failed, and so does its first after the wake.
    let night = runs(&[
        every_second(0, 1, false),
        every_second(1_800, 1_801, false),
        every_second(1_802, 1_802, true),
    ]);
    assert_eq!(
        watched_at(&night, PaneWatch::default()).0,
        Vec::<f64>::new()
    );
    // Awake time before the sleep does not count either: the stretch starts again at the wake.
    let nap = runs(&[every_second(0, 20, false), every_second(51, 90, false)]);
    assert_eq!(watched_at(&nap, PaneWatch::default()).0, [81.0]);
    // A gap of the stretch itself is still a sample's: a wait's looks, a stretch apart.
    let apart = [(0.0, false), (30.0, false)];
    assert_eq!(watched_at(&apart, PaneWatch::default()).0, [30.0]);
    let further = [(0.0, false), (30.5, false), (60.5, false)];
    assert_eq!(watched_at(&further, PaneWatch::default()).0, [60.5]);
}

#[test]
fn a_sleep_inside_an_outage_already_said_says_it_no_more() {
    let night = runs(&[
        every_second(0, 30, false),
        every_second(1_830, 1_900, false),
        every_second(1_901, 1_901, true),
        every_second(1_902, 1_932, false),
    ]);
    assert_eq!(watched_at(&night, PaneWatch::default()).0, [30.0, 1_932.0]);
}

#[test]
fn a_clock_set_back_starts_the_stretch_again() {
    let set_back = runs(&[every_second(100, 120, false), every_second(50, 90, false)]);
    assert_eq!(watched_at(&set_back, PaneWatch::default()).0, [80.0]);
}

#[test]
fn an_older_dispatchers_record_is_read_as_it_meant() {
    // One that said WATCH keeps no start: no second WATCH, and an answer re-arms it.
    let said: PaneWatch =
        serde_json::from_value(json!({"blocked": {}, "unanswered": true})).unwrap();
    assert_eq!(
        watched_at(&every_second(0, 60, false), said.clone()).0,
        Vec::<f64>::new()
    );
    let rearmed = runs(&[every_second(0, 0, true), every_second(1, 31, false)]);
    assert_eq!(watched_at(&rearmed, said).0, [31.0]);
    // One that said nothing: the stretch starts at the first failed sample this one takes.
    let quiet: PaneWatch = serde_json::from_value(json!({"unanswered": false})).unwrap();
    assert_eq!(watched_at(&every_second(0, 30, false), quiet).0, [30.0]);
    // What this one keeps, an older one reads: the fields it knows, as they were.
    let (_, failing) = watched_at(&every_second(0, 10, false), PaneWatch::default());
    let saved = serde_json::to_value(&failing).unwrap();
    assert_eq!(saved["unanswered"], false);
    assert_eq!(
        (
            saved["unanswered_since"].as_f64(),
            saved["unanswered_at"].as_f64()
        ),
        (Some(0.0), Some(10.0))
    );
}

#[test]
fn with_nothing_launched_hc_is_not_asked_and_the_watch_forgets() {
    let board = Board {
        tasks: vec![task(
            "t/a",
            None,
            &[("orch", "orchestrator"), ("w", "executor")],
        )],
        sessions: vec![session("orch", None), session("w", None)],
        latest_message: 0,
        workflow_generation: None,
    };
    let before = PaneWatch {
        blocked: BTreeMap::new(),
        unanswered: true,
        unanswered_since: Some(NOW),
        unanswered_at: Some(NOW),
        ..PaneWatch::default()
    };
    let after = pane_watch(
        &board,
        &overseen(&board, "orch"),
        &[],
        &|| panic!("hc asked with nothing to watch"),
        NOW,
        &before,
        &mut |line| panic!("said {line}"),
    );
    assert_eq!(after, PaneWatch::default());
}

#[test]
fn a_blocked_terminal_is_said_only_to_the_orchestrator_of_the_task_it_was_launched_for() {
    let orchestrator = |handle: &str| {
        let mut session = session(handle, None);
        session.row.kind = "orchestrator".into();
        session
    };
    // One worker on a's task and on b's, launched for b's.
    let board = Board {
        tasks: vec![
            task(
                "scratch/a",
                None,
                &[("a", "orchestrator"), ("w", "executor")],
            ),
            task(
                "scratch/b",
                None,
                &[("b", "orchestrator"), ("w", "executor")],
            ),
        ],
        sessions: vec![orchestrator("a"), orchestrator("b"), session("w", None)],
        latest_message: 0,
        workflow_generation: None,
    };
    let open = [launched("w", "scratch/b", "t1", "")];
    let heard = |who: &str| {
        let panes = Ok(vec![pane("t1", "blocked", "")]);
        look(&board, who, &open, panes, &PaneWatch::default()).1
    };
    let blocked = "BLOCKED w scratch/b t1 — observed waiting on an approval or a question; inspect its terminal before acting";
    assert_eq!(
        [heard("a"), heard("b")],
        [vec![], vec![blocked.to_string()]]
    );
}

/// `orch` orchestrates t/root, and `other` t/root/b below it; t/free is nobody's. `kid`, orch's
/// child, reviews other's t/root/b/rev; `hand`, other's child, executes orch's t/root/a. Both lost.
fn two_orchestrators_and_a_lost_child_on_each_side() -> Board {
    let with = |mut task: Task, status: &str| {
        task.status = status.into();
        task
    };
    let lost = |handle: &str, parent: &str| {
        let mut session = session(handle, Some(parent));
        session.health = Health::Lost;
        session
    };
    Board {
        tasks: vec![
            task("t/root", None, &[("orch", "orchestrator")]),
            task("t/root/a", Some("t/root"), &[("hand", "executor")]),
            task("t/root/b", Some("t/root"), &[("other", "orchestrator")]),
            with(task("t/root/b/ready", Some("t/root/b"), &[]), "ready"),
            with(
                task("t/root/b/rev", Some("t/root/b"), &[("kid", "reviewer")]),
                "review",
            ),
            with(task("t/free", None, &[]), "ready"),
        ],
        sessions: vec![
            session("orch", None),
            session("other", None),
            lost("kid", "orch"),
            lost("hand", "other"),
        ],
        latest_message: 7,
        workflow_generation: None,
    }
}

#[test]
fn the_legacy_look_takes_in_the_whole_board_as_dispatchers_before_the_narrowing_did() {
    let board = two_orchestrators_and_a_lost_child_on_each_side();
    let legacy = legacy_seen(&board, "orch", "orchestrator");
    fn keys<V>(map: &BTreeMap<String, V>) -> Vec<&str> {
        map.keys().map(String::as_str).collect()
    }
    // Every ready task, everything below what it orchestrates, its children by parent.
    assert_eq!(keys(&legacy.ready), ["t/free", "t/root/b/ready"]);
    let below = [
        "t/root",
        "t/root/a",
        "t/root/b",
        "t/root/b/ready",
        "t/root/b/rev",
    ];
    assert_eq!(keys(&legacy.statuses), below);
    assert_eq!(legacy.verdict_tasks, below.map(String::from).into());
    assert_eq!(keys(&legacy.children), ["kid"]);
    assert!(
        legacy
            .staffing
            .contains(r#"["t/root/b/rev","kid","reviewer",true]"#),
        "the whole board's staffing: {}",
        legacy.staffing
    );
    // Its record has the shape older dispatchers write: no field they do not know.
    let record = serde_json::to_value(&legacy).unwrap();
    assert_eq!(record.get("unclaimed"), None);
    let heard = seen(&board, "orch", "orchestrator");
    assert_eq!(keys(&heard.ready), ["t/free"]);
    assert_eq!(keys(&heard.statuses), ["t/free", "t/root", "t/root/a"]);
    assert_eq!(keys(&heard.children), ["hand"]);
    assert_eq!(heard.staffing, r#"[["t/root/a","hand","executor",true]]"#);
}

#[test]
fn the_heard_look_is_the_legacy_record_as_it_stands_with_its_narrowing_on_it() {
    let board = two_orchestrators_and_a_lost_child_on_each_side();
    let legacy = legacy_seen(&board, "orch", "orchestrator");
    let heard = seen(&board, "orch", "orchestrator");
    let narrowing = Narrowing::between(&legacy, &heard);
    let stored: Narrowing =
        serde_json::from_value(serde_json::to_value(&narrowing).unwrap()).unwrap();
    assert_eq!(stored.over(&legacy), heard);
    // A worker's looks agree, but for nothing.
    let worker = legacy_seen(&board, "hand", "worker");
    assert_eq!(worker, seen(&board, "hand", "worker"));
    assert_eq!(Narrowing::between(&worker, &worker).over(&worker), worker);

    // An older dispatcher rewrote the record from a later board: it said NEW for t/root/next and
    // LOST for kid, and t/free went blocked, which it does not follow.
    let mut later = board.clone();
    later.tasks[5].status = "blocked".into();
    let mut next = task("t/root/next", Some("t/root"), &[]);
    next.status = "ready".into();
    later.tasks.push(next);
    let older = legacy_seen(&later, "orch", "orchestrator");
    let baseline = narrowing.over(&older);
    assert!(baseline.ready.contains_key("t/root/next"), "said by it");
    assert!(
        !baseline.ready.contains_key("t/root/b/ready"),
        "never heard"
    );
    assert!(!baseline.children.contains_key("kid"), "never heard");
    assert_eq!(baseline.children.get("hand"), Some(&Health::Lost));
    assert_eq!(
        baseline.statuses.get("t/free").map(String::as_str),
        Some("ready"),
        "a status it does not follow stays as this narrowing last heard it"
    );
    // A record that holds what the narrowing added keeps its own value.
    let mut holding = older.clone();
    holding.children.insert("hand".into(), Health::Live);
    assert_eq!(
        narrowing.over(&holding).children.get("hand"),
        Some(&Health::Live)
    );
}
