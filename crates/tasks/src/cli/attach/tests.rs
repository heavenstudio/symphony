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

/// One look, and the lines it said.
fn look(
    board: &Board,
    who: &str,
    open: &[TaskSessionRow],
    panes: Answer<Vec<Pane>>,
    before: &PaneWatch,
) -> (PaneWatch, Vec<String>) {
    let mut said = Vec::new();
    let after = pane_watch(
        board,
        &overseen(board, who),
        open,
        &|| panes.clone(),
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
fn a_launched_terminal_is_found_by_its_conversation_once_its_recorded_address_is_gone() {
    let board = Board {
        tasks: vec![task(
            "t/a",
            None,
            &[("orch", "orchestrator"), ("w", "executor")],
        )],
        sessions: vec![session("orch", None), session("w", None)],
        latest_message: 0,
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
        ["BLOCKED w t/a t7 — its terminal waits on an approval or a question"]
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
    };
    let blocked = |pane: &str| Blocked {
        task: "t/a".into(),
        pane: pane.into(),
    };
    let before = PaneWatch {
        blocked: BTreeMap::from([("w".into(), blocked("t1")), ("v".into(), blocked("t2"))]),
        unanswered: false,
    };
    // v's pane closed: only w is still launched.
    let open = [launched("w", "t/a", "t1", "")];
    let (after, said) = look(&board, "orch", &open, Err("down".into()), &before);
    assert_eq!(
        said,
        ["WATCH hc unanswered: down — blocked terminals go unreported until it answers"]
    );
    let expected = PaneWatch {
        blocked: BTreeMap::from([("w".into(), blocked("t1"))]),
        unanswered: true,
    };
    assert_eq!(after, expected);
    let (again, said) = look(&board, "orch", &open, Err("down".into()), &after);
    assert_eq!((again, said), (expected, Vec::new()));
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
    };
    let before = PaneWatch {
        blocked: BTreeMap::new(),
        unanswered: true,
    };
    let after = pane_watch(
        &board,
        &overseen(&board, "orch"),
        &[],
        &|| panic!("hc asked with nothing to watch"),
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
    };
    let open = [launched("w", "scratch/b", "t1", "")];
    let heard = |who: &str| {
        let panes = Ok(vec![pane("t1", "blocked", "")]);
        look(&board, who, &open, panes, &PaneWatch::default()).1
    };
    let blocked = "BLOCKED w scratch/b t1 — its terminal waits on an approval or a question";
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
