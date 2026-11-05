//! The attach monitor's unit tests: what a session sees of the board, and the pane watch.

use super::*;
use crate::rows::SessionRow;
use crate::{Role, Session};
use serde_json::json;

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

fn pane(id: &str, status: &str, conversation: &str) -> Pane {
    Pane::from_json(&json!({"pane_id": id, "agent_status": status,
                            "agent_session": {"value": conversation}}))
    .unwrap()
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
        overseen(&board, "orch"),
        BTreeSet::from(["deep", "deeper", "kid"])
    );
    assert_eq!(overseen(&board, "deep"), BTreeSet::new());
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
    assert_eq!(overseen(&board, "orch"), BTreeSet::from(["mine"]));
    assert_eq!(overseen(&board, "other"), BTreeSet::from(["theirs"]));
    let orch = seen(&board, "orch", "orchestrator");
    assert_eq!(
        orch.statuses.keys().collect::<Vec<_>>(),
        ["t/root", "t/root/a"]
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
    assert!(seen(&board, "mine", "worker").children.contains_key("sub"));
}

#[test]
fn a_launched_pane_is_found_by_its_conversation_once_its_recorded_id_is_gone() {
    let board = Board {
        tasks: vec![task(
            "t/a",
            None,
            &[("orch", "orchestrator"), ("w", "executor")],
        )],
        sessions: vec![session("orch", None), session("w", None)],
        latest_message: 0,
    };
    let open = [launched("w", "t/a", "w1:p1", "codex:thread-1")];
    let (after, said) = look(
        &board,
        "orch",
        &open,
        Ok(vec![pane("w1:p7", "blocked", "thread-1")]),
        &PaneWatch::default(),
    );
    assert_eq!(
        said,
        ["BLOCKED w t/a w1:p7 — its pane waits on an approval or a question"]
    );
    assert_eq!(after.blocked["w"].pane, "w1:p7");
}

#[test]
fn a_session_no_longer_launched_is_forgotten_while_herdr_cannot_be_asked() {
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
        blocked: BTreeMap::from([
            ("w".into(), blocked("w1:p1")),
            ("v".into(), blocked("w1:p2")),
        ]),
        unanswered: false,
    };
    // v's pane closed: only w is still launched.
    let open = [launched("w", "t/a", "w1:p1", "")];
    let (after, said) = look(&board, "orch", &open, Err("down".into()), &before);
    assert_eq!(
        said,
        ["WATCH herdr unanswered: down — blocked panes go unreported until it answers"]
    );
    let expected = PaneWatch {
        blocked: BTreeMap::from([("w".into(), blocked("w1:p1"))]),
        unanswered: true,
    };
    assert_eq!(after, expected);
    let (again, said) = look(&board, "orch", &open, Err("down".into()), &after);
    assert_eq!((again, said), (expected, Vec::new()));
}

#[test]
fn with_nothing_launched_herdr_is_not_asked_and_the_watch_forgets() {
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
        &|| panic!("herdr asked with nothing to watch"),
        &before,
        &mut |line| panic!("said {line}"),
    );
    assert_eq!(after, PaneWatch::default());
}
