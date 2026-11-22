//! The gates the Symphony answers of its own, how a project's replace them, and how a plugin's
//! answers are kept by the branch's tip.

use super::*;
use crate::test_tree::Tree;
use std::sync::atomic::{AtomicUsize, Ordering};

const TIP: &str = "4be91c2aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OLD: &str = "e19d3a0bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn line(id: i64, kind: &str, sender: &str, text: &str, ts: f64) -> symphony_board::Message {
    symphony_board::Message {
        id,
        slug: "tools/x".into(),
        sender: sender.into(),
        recipient: None,
        kind: kind.into(),
        text: text.into(),
        ts,
    }
}

fn gate(name: &str, state: GateState) -> Gate {
    Gate {
        name: name.into(),
        state,
        ..Gate::default()
    }
}

fn names(gates: &[Gate]) -> Vec<(&str, GateState)> {
    gates
        .iter()
        .map(|gate| (gate.name.as_str(), gate.state))
        .collect()
}

#[test]
fn review_rounds_are_the_approve_and_changes_verdicts_on_the_thread_in_order() {
    let thread = [
        line(
            1,
            "system",
            "glacial-merlin",
            "created by glacial-merlin",
            10.0,
        ),
        line(
            2,
            "msg",
            "lucid-heron",
            "approve: not a verdict, a message",
            11.0,
        ),
        line(
            3,
            "verdict",
            "lucid-heron",
            "changes: the verify keeps running\nmore",
            20.0,
        ),
        line(
            4,
            "verdict",
            "lucid-heron",
            "approved: not a round's word",
            25.0,
        ),
        line(5, "verdict", "land", "landed tools/x on main", 26.0),
        line(6, "verdict", "quiet-wren", "done: a slice closed", 27.0),
        line(7, "verdict", "lucid-heron", "changes", 28.0),
        line(8, "verdict", "lucid-heron", "approve", 30.0),
    ];
    let rounds = review_rounds(&thread);
    assert_eq!(
        rounds,
        [
            Round {
                id: 3,
                by: "lucid-heron".into(),
                at: 20.0,
                approved: false,
                said: "the verify keeps running\nmore".into(),
                tip: None,
            },
            Round {
                id: 7,
                by: "lucid-heron".into(),
                at: 28.0,
                approved: false,
                said: String::new(),
                tip: None,
            },
            Round {
                id: 8,
                by: "lucid-heron".into(),
                at: 30.0,
                approved: true,
                said: String::new(),
                tip: None,
            },
        ]
    );
    assert_eq!(review_rounds(&[]), Vec::<Round>::new());
}

#[test]
fn the_review_gate_is_met_by_the_last_rounds_approval_with_the_rounds_before_it_its_past() {
    let thread = [
        line(
            3,
            "verdict",
            "lucid-heron",
            "changes: the verify keeps running\nsee below",
            20.0,
        ),
        line(9, "verdict", "lucid-heron", "approve: it stops now", 30.0),
    ];
    let gate = review("tools/x", "done", &[], &review_rounds(&thread));
    assert_eq!(
        gate,
        Gate {
            name: "review".into(),
            state: GateState::Met,
            by: Some("lucid-heron".into()),
            at: Some(30.0),
            detail: "Approved in round 2 of 2".into(),
            link: Some("/task/tools/x#message-9".into()),
            past: vec![GatePast {
                state: GateState::Missing,
                at: 20.0,
                by: Some("lucid-heron".into()),
                commit: None,
                detail: "Changes asked in round 1: the verify keeps running".into(),
            }],
            ..Gate::default()
        }
    );
    // A changes verdict last leaves it missing, saying the reviewer's first line.
    let gate = review("tools/x", "in_progress", &[], &review_rounds(&thread[..1]));
    assert_eq!(gate.state, GateState::Missing);
    assert_eq!(
        gate.detail,
        "Changes asked in round 1: the verify keeps running"
    );
    assert_eq!(gate.at, Some(20.0));
    assert_eq!(gate.past, Vec::<GatePast>::new());
    // A bare `changes` says only its round.
    let bare = [line(4, "verdict", "lucid-heron", "changes", 20.0)];
    let gate = review("tools/x", "in_progress", &[], &review_rounds(&bare));
    assert_eq!(gate.detail, "Changes asked in round 1");
}

#[test]
fn a_review_gate_with_no_round_says_whether_the_task_waits_on_a_reviewer() {
    let none = |status: &str, reviewers: &[&str]| {
        let gate = review("tools/x", status, reviewers, &[]);
        assert_eq!(
            (gate.state, gate.at, gate.link.as_deref()),
            (GateState::Missing, None, None)
        );
        (gate.by, gate.detail)
    };
    assert_eq!(
        none("review", &["lucid-heron"]),
        (
            Some("lucid-heron".into()),
            "In review, no verdict yet".into()
        )
    );
    assert_eq!(
        none("review", &[]),
        (None, "In review, waiting on a reviewer".into())
    );
    assert_eq!(
        none("in_progress", &["lucid-heron"]),
        (None, "Not reviewed yet".into())
    );
    // A task done with no round landed without review, as a bug fix may: waived, saying why.
    let landed = review("tools/x", "done", &["lucid-heron"], &[]);
    assert_eq!(
        (landed.state, landed.detail.as_str(), landed.by, landed.link),
        (
            GateState::Waived,
            "Landed with no review verdict",
            None,
            None
        )
    );
    assert!(
        landed
            .why
            .is_some_and(|why| why.contains("workflow/fast-bugfixes")),
        "a waiver says why"
    );
}

/// A covered close's line, as the board writes it (`symphony_board::covered_line`).
fn covered_line(id: i64, ts: f64) -> symphony_board::Message {
    let text = symphony_board::covered_line(
        "tests/timeline",
        "75b4516a0123456789",
        "fleet-otter",
        "its fix is 97e84883\nthe rest",
    );
    line(id, "verdict", "fleet-otter", &text, ts)
}

#[test]
fn a_task_done_by_a_covered_close_shows_it_never_landed_without_review_nor_approved() {
    let waived = |past: Vec<GatePast>| Gate {
        name: "review".into(),
        state: GateState::Waived,
        by: Some("fleet-otter".into()),
        at: Some(40.0),
        commit: Some("75b4516a0123".into()),
        why: Some(
            "its work landed with tests/timeline's branch, so none of its own is left to review; \
             closed by fleet-otter: its fix is 97e84883"
                .into(),
        ),
        detail: "Covered by tests/timeline, landed at 75b4516a0123".into(),
        link: Some("/task/tools/x#message-5".into()),
        past,
    };
    // With no round, not "Landed with no review verdict".
    let alone = [covered_line(5, 40.0)];
    assert_eq!(
        review_of("tools/x", "done", &[], &alone),
        waived(Vec::new())
    );
    // An approve before it, the task sent back to review since: not met, the approve its past.
    let approved = [
        line(3, "verdict", "lucid-heron", "approve: good", 20.0),
        line(
            4,
            "system",
            "fleet-otter",
            "fleet-otter moved it done → review",
            30.0,
        ),
        covered_line(5, 40.0),
    ];
    assert_eq!(
        review_of("tools/x", "done", &["lucid-heron"], &approved),
        waived(vec![GatePast {
            state: GateState::Met,
            at: 20.0,
            by: Some("lucid-heron".into()),
            commit: None,
            detail: "Approved in round 1 of 1".into(),
        }])
    );
    // Not done (an owner's move since), its rounds are read as ever; with no covered close last,
    // the gate is the rounds' too.
    let rounds = review_rounds(&approved);
    assert_eq!(
        review_of("tools/x", "review", &[], &approved),
        review("tools/x", "review", &[], &rounds)
    );
    let after = [
        covered_line(5, 40.0),
        line(6, "verdict", "lucid-heron", "changes", 50.0),
    ];
    assert_eq!(covered_close(&after), None);
    assert_eq!(
        review_of("tools/x", "done", &[], &after),
        review("tools/x", "done", &[], &review_rounds(&after))
    );
    assert_eq!(
        covered_close(&alone).map(|c| (c.other, c.commit, c.note)),
        Some((
            "tests/timeline".into(),
            "75b4516a0123".into(),
            "its fix is 97e84883\nthe rest".into()
        ))
    );
}

/// A run of `label` as the board's recorder keeps it: `id` its operation, finished with `exit`
/// unless `state` says otherwise.
pub(super) fn run(
    id: u8,
    label: &str,
    branch: &str,
    commit: &str,
    state: Step,
    exit: Option<i32>,
) -> Record {
    let started = 1000.0 * f64::from(id);
    Record {
        version: 1,
        operation_id: format!("{id:032x}"),
        parent_id: None,
        phase: recorded::Phase::Test,
        label: label.into(),
        state,
        handle: Some(format!("agent-{id}")),
        task: None,
        checkout: "/checkout".into(),
        branch: Some(branch.into()),
        commit: Some(commit.into()),
        machine_id: None,
        host: "host".into(),
        pid: 10,
        child_pid: (state != Step::Queued).then_some(11),
        queued_at: started - 5.0,
        started_at: (state != Step::Queued).then_some(started),
        ended_at: (state == Step::Finished).then_some(started + 60.0),
        beat_at: started,
        exit_code: if state == Step::Finished { exit } else { None },
        signal: None,
        reason: String::new(),
        run: None,
        reading: None,
    }
}

#[test]
fn the_check_gate_reads_the_newest_run_of_the_check_at_the_tip() {
    use Step::{Finished, Queued, Running};
    let label = "just verify";
    let runs = [
        run(1, label, "tools/x", OLD, Finished, Some(1)),
        run(2, label, "tools/x", OLD, Finished, Some(0)),
        run(3, label, "tools/x", TIP, Finished, Some(101)),
        run(4, "just lint-rs", "tools/x", TIP, Finished, Some(0)),
        run(5, label, "tools/other", "c0ffee0", Finished, Some(0)),
        // A clean checkout runs the tip detached, under a branch of its own.
        run(6, label, "HEAD", TIP, Finished, Some(0)),
    ];
    let gate = check(label, "tools/x", Some(TIP), &runs);
    assert_eq!(gate.state, GateState::Met);
    assert_eq!(gate.detail, "`just verify` green at 4be91c2");
    assert_eq!(gate.by.as_deref(), Some("agent-6"));
    assert_eq!(gate.at, Some(6060.0));
    assert_eq!(gate.commit.as_deref(), Some(TIP));
    assert_eq!(
        gate.past
            .iter()
            .map(|p| (p.state, p.at, p.detail.as_str()))
            .collect::<Vec<_>>(),
        [
            (
                GateState::Missing,
                1060.0,
                "`just verify` red at e19d3a0: exit 1"
            ),
            (GateState::Met, 2060.0, "`just verify` green at e19d3a0"),
            (
                GateState::Missing,
                3060.0,
                "`just verify` red at 4be91c2: exit 101"
            ),
        ],
        "the earlier runs on the branch or at its tip, not another branch's or another command's"
    );
    // The run that began last decides, whatever order the records come in: red after green at one
    // tip is red.
    let later = run(9, label, "tools/x", TIP, Finished, Some(2));
    let red = check(label, "tools/x", Some(TIP), &[later, runs[5].clone()]);
    assert_eq!(
        (red.state, red.detail.as_str(), red.by.as_deref()),
        (
            GateState::Missing,
            "`just verify` red at 4be91c2: exit 2",
            Some("agent-9")
        )
    );
    assert_eq!(red.past.len(), 1);
    // A run going now, or waiting, is missing since it began.
    let going = check(
        label,
        "tools/x",
        Some(TIP),
        &[run(7, label, "tools/x", TIP, Running, None)],
    );
    assert_eq!(
        (going.state, going.at, going.detail.as_str()),
        (
            GateState::Missing,
            Some(7000.0),
            "`just verify` running at 4be91c2"
        )
    );
    let waiting = check(
        label,
        "tools/x",
        Some(TIP),
        &[run(8, label, "tools/x", TIP, Queued, None)],
    );
    assert_eq!(
        (waiting.state, waiting.at, waiting.detail.as_str()),
        (
            GateState::Missing,
            Some(7995.0),
            "`just verify` queued at 4be91c2"
        )
    );
}

#[test]
fn a_check_with_no_run_at_the_tip_or_no_tip_is_unknown_and_a_run_ended_without_a_status_is_red() {
    let label = "just verify";
    let older = run(1, label, "tools/x", OLD, Step::Finished, Some(0));
    let gate = check(label, "tools/x", Some(TIP), std::slice::from_ref(&older));
    assert_eq!(
        (gate.state, gate.detail.as_str(), gate.commit.as_deref()),
        (
            GateState::Unknown,
            "`just verify` has not run at 4be91c2",
            Some(TIP)
        )
    );
    assert_eq!(gate.past.len(), 1, "its runs at earlier tips are its past");
    let gate = check(label, "tools/gone", None, &[]);
    assert_eq!(
        (gate.state, gate.detail.as_str(), gate.commit),
        (
            GateState::Unknown,
            "No branch tools/gone in this repository",
            None
        )
    );
    let mut killed = run(2, label, "tools/x", TIP, Step::Finished, None);
    killed.signal = Some(15);
    assert_eq!(
        check(label, "tools/x", Some(TIP), std::slice::from_ref(&killed)).detail,
        "`just verify` red at 4be91c2: ended by signal 15"
    );
    killed.signal = None;
    assert_eq!(
        check(label, "tools/x", Some(TIP), std::slice::from_ref(&killed)).detail,
        "`just verify` at 4be91c2 ended without a status"
    );
    killed.reason = "its recorder ended before saying how its command did".into();
    let lost = check(label, "tools/x", Some(TIP), &[killed]);
    assert_eq!(lost.state, GateState::Missing);
    assert_eq!(
        lost.detail,
        "`just verify` at 4be91c2 ended without a status: its recorder ended before saying how \
         its command did"
    );
}

#[test]
fn runs_are_the_historys_and_the_operations_still_in_the_folder_each_once() {
    let tree = Tree::new();
    let folder = tree.path("work");
    assert_eq!(
        runs(&folder),
        Vec::<Record>::new(),
        "a folder not made yet holds none"
    );
    let label = "just verify";
    let kept = run(1, label, "tools/x", OLD, Step::Finished, Some(1));
    recorded::keep(&folder, &kept).unwrap();
    let mut again = kept.clone();
    again.reason = "read from its record".into();
    let operations = folder.join(recorded::OPERATIONS);
    std::fs::create_dir_all(&operations).unwrap();
    recorded::write(
        &operations.join(format!("{}.json", again.operation_id)),
        &again,
    )
    .unwrap();
    let going = run(2, label, "tools/x", TIP, Step::Running, None);
    let path = operations.join(format!("{}.json", going.operation_id));
    recorded::write(&path, &going).unwrap();
    // A run going on counts while its recorder holds its lease.
    let lease = std::fs::File::create(path.with_extension("lease")).unwrap();
    lease.lock().unwrap();
    let found = runs(&folder);
    assert_eq!(found, [again.clone(), going.clone()]);
    // Let go as a recorder lets go: unlocked, then closed. Its close alone would leave the lock
    // with a child another test's thread forked meanwhile, until that child's exec.
    lease.unlock().unwrap();
    drop(lease);
    assert_eq!(
        runs(&folder),
        [again],
        "a run whose recorder is gone is not one"
    );
}

fn card(path: &str, slug: &str) -> PendingItem {
    PendingItem {
        path: path.into(),
        slug: slug.into(),
        href: format!("/{path}#{slug}"),
        branch: None,
    }
}

#[test]
fn a_task_posted_the_cards_of_its_linked_documents_that_its_texts_name_whole() {
    let pending = Pending {
        items: vec![
            card("docs/DECISIONS.md", "x"),
            card("docs/DECISIONS.md", "y"),
            card("docs/OTHER.md", "z"),
        ],
        docs: Vec::new(),
    };
    let links = vec!["docs/DECISIONS.md#Gates".to_string()];
    let tree = Tree::new();
    let names = crate::docview::LinkNames::of(&tree.root);
    let linked = linked_cards(&links, &pending, &names);
    assert_eq!(
        linked.iter().map(|c| c.slug.as_str()).collect::<Vec<_>>(),
        ["x", "y"]
    );
    assert_eq!(
        linked_cards(&[], &pending, &names),
        Vec::<&PendingItem>::new()
    );
    let posted = |texts: &[&str]| {
        posted_cards(&linked, texts.iter().copied())
            .iter()
            .map(|c| c.slug.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        posted(&["x-2 and xy are other cards", "z is not linked"]),
        Vec::<String>::new()
    );
    assert_eq!(
        posted(&["see y.", "card x waits"]),
        ["x", "y"],
        "in the cards' order"
    );
    // Once every card is found, no more text is read.
    let read = AtomicUsize::new(0);
    let texts = ["x", "y", "never read"].into_iter().inspect(|_| {
        read.fetch_add(1, Ordering::SeqCst);
    });
    assert_eq!(posted_cards(&linked, texts).len(), 2);
    assert_eq!(read.load(Ordering::SeqCst), 2);
}

#[test]
fn a_link_filed_before_its_document_moved_still_finds_its_cards_by_name_alone() {
    let tree = Tree::new();
    for path in [
        "docs/area/MOVED.md",
        "docs/a/SHARED.md",
        "docs/b/SHARED.md",
        "docs/STAYED.md",
        "docs/area/STAYED.md",
    ] {
        tree.write(path, "# A document\n");
    }
    let pending = Pending {
        items: vec![
            card("docs/area/MOVED.md", "moved"),
            card("docs/a/SHARED.md", "first"),
            card("docs/b/SHARED.md", "second"),
            card("docs/area/STAYED.md", "elsewhere"),
            card("docs/STAYED.md", "stayed"),
        ],
        docs: Vec::new(),
    };
    let names = crate::docview::LinkNames::of(&tree.root);
    let slugs = |link: &str| {
        linked_cards(&[link.to_string()], &pending, &names)
            .iter()
            .map(|c| c.slug.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        slugs("docs/MOVED.md#The order"),
        ["moved"],
        "found by its name"
    );
    assert_eq!(
        slugs("docs/MOVED"),
        ["moved"],
        "`.md` added, as a wikilink's"
    );
    assert_eq!(
        slugs("docs/SHARED.md#h"),
        Vec::<String>::new(),
        "a name documents share holds none of their cards"
    );
    assert_eq!(
        slugs("docs/STAYED.md"),
        ["stayed"],
        "a route a file has names that document alone, not another of its name"
    );
    assert_eq!(slugs("docs/GONE.md"), Vec::<String>::new());
}

#[test]
fn the_owner_gate_holds_while_a_card_the_task_posted_waits_and_is_absent_otherwise() {
    assert_eq!(owner(&[]), None);
    let (x, y) = (card("docs/DECISIONS.md", "x"), card("docs/OTHER.md", "y"));
    assert_eq!(
        owner(&[&x]),
        Some(Gate {
            name: "owner".into(),
            state: GateState::Held,
            by: Some("owner".into()),
            why: Some("the task asked the owner and waits on the answer".into()),
            detail: "Card x in docs/DECISIONS.md waits on the owner".into(),
            link: Some("/docs/DECISIONS.md#x".into()),
            ..Gate::default()
        })
    );
    let both = owner(&[&x, &y]).unwrap();
    assert_eq!(both.detail, "Cards x, y wait on the owner");
    assert_eq!(both.link.as_deref(), Some("/docs/DECISIONS.md#x"));
}

#[test]
fn a_projects_gate_takes_the_place_of_the_symphonys_of_its_name_and_the_rest_follow_ranked() {
    let mut theirs_review = gate("review", GateState::Met);
    theirs_review.detail = "the project's".into();
    let gates = merged(
        vec![
            gate("review", GateState::Missing),
            gate("check", GateState::Unknown),
            gate("owner", GateState::Held),
        ],
        vec![
            gate("certificate", GateState::Missing),
            theirs_review,
            gate("dependency", GateState::Waived),
            gate("certificate", GateState::Met),
            gate("nightly", GateState::Met),
        ],
    );
    assert_eq!(
        names(&gates),
        [
            ("certificate", GateState::Missing),
            ("owner", GateState::Held),
            ("check", GateState::Unknown),
            ("dependency", GateState::Waived),
            ("review", GateState::Met),
            ("nightly", GateState::Met),
        ],
        "the project's review in the Symphony's place, a name given twice kept once"
    );
    assert_eq!(gates[4].detail, "the project's");
    assert_eq!(merged(Vec::new(), Vec::new()), Vec::<Gate>::new());
}

#[test]
fn a_gate_keeps_its_link_only_when_it_is_a_path_of_the_app_or_a_web_address() {
    let linked = |name: &str, link: &str| Gate {
        link: Some(link.into()),
        ..gate(name, GateState::Met)
    };
    let kept = [
        "/task/tools/x#message-4",
        "/docs/analysis/x.md#card",
        "https://ci.example/runs/1",
        "HTTP://ci.example",
    ];
    let dropped = [
        "javascript:alert(1)",
        "JAVASCRIPT:alert(1)",
        "  javascript:alert(1)",
        "\u{7}javascript:alert(1)",
        "data:text/html,x",
        "vbscript:x",
        "//evil.example",
        "/\\evil.example",
        "",
    ];
    let own = vec![linked("review", "javascript:alert(1)")];
    let project: Vec<Gate> = kept
        .iter()
        .chain(&dropped)
        .enumerate()
        .map(|(i, link)| linked(&format!("g{i}"), link))
        .collect();
    let gates = merged(own, project);
    let links: Vec<Option<&str>> = gates.iter().map(|g| g.link.as_deref()).collect();
    let mut expected: Vec<Option<&str>> = vec![None];
    expected.extend(kept.iter().map(|link| Some(*link)));
    expected.extend(dropped.iter().map(|_| None));
    assert_eq!(links, expected, "the Symphony's own and a plugin's alike");
}

#[test]
fn the_shape_of_a_tasks_last_answer_is_kept_by_its_checkout_and_slug() {
    let kept = Kept::default();
    let (main, tree) = (Path::new("/repo"), Path::new("/repo-wt"));
    assert_eq!(kept.seen(main, "tools/x"), None);
    let answer = |gates: Vec<Gate>| TaskGates {
        slug: "tools/x".into(),
        gates,
        ..TaskGates::default()
    };
    let signed = Gate {
        by: Some("owner".into()),
        ..gate("owner", GateState::Held)
    };
    kept.saw(
        main,
        &answer(vec![
            gate("certificate", GateState::Missing),
            signed,
            gate("review", GateState::Met),
        ]),
    );
    let seen = |count, waiting, unsigned| {
        Some(GatesSeen {
            count,
            waiting,
            unsigned,
        })
    };
    assert_eq!(kept.seen(main, "tools/x"), seen(3, 2, 2));
    assert_eq!(kept.seen(tree, "tools/x"), None, "another checkout's");
    assert_eq!(kept.seen(main, "tools/y"), None, "another task's");
    kept.saw(main, &answer(Vec::new()));
    assert_eq!(
        kept.seen(main, "tools/x"),
        seen(0, 0, 0),
        "the newest answer's"
    );
}

#[test]
fn a_task_that_closes_or_leaves_the_board_has_its_kept_shape_forgotten_in_every_checkout() {
    let kept = Kept::default();
    let (main, tree) = (Path::new("/repo"), Path::new("/repo-wt"));
    let answer = |slug: &str| TaskGates {
        slug: slug.into(),
        gates: vec![gate("review", GateState::Missing)],
        ..TaskGates::default()
    };
    for (root, slug) in [
        (main, "tools/open"),
        (tree, "tools/open"),
        (main, "tools/done"),
        (tree, "tools/done"),
        (main, "tools/gone"),
    ] {
        kept.saw(root, &answer(slug));
    }
    kept.keep_open(|slug| slug == "tools/open");
    let still: Vec<bool> = [
        (main, "tools/open"),
        (tree, "tools/open"),
        (main, "tools/done"),
        (tree, "tools/done"),
        (main, "tools/gone"),
    ]
    .iter()
    .map(|(root, slug)| kept.seen(root, slug).is_some())
    .collect();
    assert_eq!(still, [true, true, false, false, false]);
    kept.keep_open(|_| false);
    assert_eq!(kept.seen(main, "tools/open"), None);
}

#[test]
fn an_answerer_that_cannot_answer_stands_in_for_the_gates_it_answered_before() {
    assert_eq!(
        failed("stamps", &[], "land exited 2\nits whole log"),
        [Gate {
            name: "stamps".into(),
            state: GateState::Unknown,
            by: Some("stamps".into()),
            detail: "land exited 2".into(),
            ..Gate::default()
        }],
        "before it ever answered, one gate named after it"
    );
    let answered = ["review".to_string(), "certificate".to_string()];
    let standing = failed("stamps", &answered, "land exited 2");
    assert_eq!(
        names(&standing),
        [
            ("review", GateState::Unknown),
            ("certificate", GateState::Unknown)
        ]
    );
    assert!(
        standing
            .iter()
            .all(|gate| gate.detail == "land exited 2" && gate.by.as_deref() == Some("stamps"))
    );
}

/// A wait a read gives a plugin's answer that only a stall outlasts: the backstop of the page's own
/// ([`WAIT`]).
const PATIENT: Duration = test_time::backstop(WAIT);

/// An answerer whose answer the test sets, counting its calls: it answers a gate of each of
/// `answers`, or `error`, or panics, once it is not held ([`Fake::hold`]).
struct Fake {
    calls: AtomicUsize,
    held: Mutex<bool>,
    released: Condvar,
    error: Mutex<Option<String>>,
    panics: bool,
    kept_while: Mutex<String>,
    answers: Mutex<Vec<&'static str>>,
}

impl Fake {
    fn leaked(panics: bool) -> &'static Fake {
        Box::leak(Box::new(Fake {
            calls: AtomicUsize::new(0),
            held: Mutex::new(false),
            released: Condvar::new(),
            error: Mutex::new(None),
            panics,
            kept_while: Mutex::new(String::new()),
            answers: Mutex::new(vec!["certificate"]),
        }))
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    /// Its calls give no answer until [`Fake::release`]: a call that is still under way however
    /// long the machine takes to look at it.
    fn hold(&self) {
        *self.held.lock().unwrap() = true;
    }

    fn release(&self) {
        *self.held.lock().unwrap() = false;
        self.released.notify_all();
    }
}

impl Gates for Fake {
    fn name(&self) -> &str {
        "fake"
    }

    fn answer(&self, _: &Path, task: &TaskRef<'_>, tip: Option<&str>) -> Result<Vec<Gate>, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        drop(
            self.released
                .wait_while(self.held.lock().unwrap(), |held| *held)
                .unwrap(),
        );
        assert!(!self.panics, "the fake panics");
        if let Some(error) = self.error.lock().unwrap().clone() {
            return Err(error);
        }
        let answers = self.answers.lock().unwrap().clone();
        Ok(answers
            .into_iter()
            .map(|name| Gate {
                commit: tip.map(str::to_string),
                detail: format!("{} {} {}", task.slug, task.branch, task.status),
                ..gate(name, GateState::Met)
            })
            .collect())
    }

    fn kept_while(&self, _: &Path, _: &TaskRef<'_>) -> String {
        self.kept_while.lock().unwrap().clone()
    }
}

/// How many of the answerer `fake`'s calls `kept` runs for the plugin `plugin`, counted as each
/// starts: with its calls held, the calls it started.
fn running(kept: &Kept, plugin: &str) -> usize {
    kept.ledgers
        .lock()
        .recover()
        .get(&Who::new(Path::new("/project"), plugin, "fake"))
        .map_or(0, |ledger| ledger.running.load(Ordering::SeqCst))
}

/// How many hold the answer the fake of `plugin` gives `tools/x` in `kept`: the ledger, the call
/// under way and each read waiting on it.
fn holding(kept: &Kept, plugin: &str) -> usize {
    kept.ledgers
        .lock()
        .recover()
        .get(&Who::new(Path::new("/project"), plugin, "fake"))
        .and_then(|ledger| ledger.slots.get("tools/x"))
        .map_or(0, Arc::strong_count)
}

fn asked(tip: &str) -> Asked {
    Asked {
        slug: "tools/x".into(),
        status: "review".into(),
        branch: "tools/x".into(),
        tip: Some(tip.into()),
    }
}

fn limits(wait: Duration, retry: Duration) -> Limits {
    Limits {
        wait,
        retry,
        ..Limits::default()
    }
}

#[test]
fn an_answer_is_kept_by_the_task_its_tip_and_what_the_answerer_reads_besides() {
    let root = Path::new("/project");
    let fake = Fake::leaked(false);
    let kept = Kept::new(limits(PATIENT, Duration::from_secs(30)));
    assert_eq!(kept.kept(root, "plugin", fake, &asked(OLD)), None);
    let first = kept.ask(root, "plugin", fake, &asked(OLD)).unwrap();
    assert_eq!(first[0].commit.as_deref(), Some(OLD));
    assert_eq!(first[0].detail, "tools/x tools/x review");
    assert_eq!(kept.ask(root, "plugin", fake, &asked(OLD)).unwrap(), first);
    assert_eq!(fake.calls(), 1, "the same tip is asked once");
    assert_eq!(
        kept.kept(root, "plugin", fake, &asked(OLD)),
        Some(Ok(first.clone()))
    );
    let moved = kept.ask(root, "plugin", fake, &asked(TIP)).unwrap();
    assert_eq!(moved[0].commit.as_deref(), Some(TIP));
    assert_eq!(fake.calls(), 2, "a new tip is asked");
    assert_eq!(
        kept.kept(root, "plugin", fake, &asked(OLD)),
        None,
        "one answer is kept a task: the newest tip's"
    );
    *fake.kept_while.lock().unwrap() = "main at c0ffee".into();
    kept.ask(root, "plugin", fake, &asked(TIP)).unwrap();
    assert_eq!(fake.calls(), 3, "what it reads besides moved");
    kept.ask(root, "plugin", fake, &asked(TIP)).unwrap();
    // Another task, another plugin of the same answerer's name, or another project are their own.
    let mut other = asked(TIP);
    other.slug = "tools/y".into();
    kept.ask(root, "plugin", fake, &other).unwrap();
    kept.ask(root, "other-plugin", fake, &asked(TIP)).unwrap();
    kept.ask(Path::new("/elsewhere"), "plugin", fake, &asked(TIP))
        .unwrap();
    assert_eq!(fake.calls(), 6);
}

#[test]
fn an_answer_follows_the_status_it_was_handed() {
    let root = Path::new("/project");
    let fake = Fake::leaked(false);
    let kept = Kept::new(limits(PATIENT, Duration::from_secs(30)));
    let mut task = asked(TIP);
    task.status = "in_progress".into();
    kept.ask(root, "plugin", fake, &task).unwrap();
    task.status = "done".into();
    let now = kept.ask(root, "plugin", fake, &task).unwrap();
    assert_eq!(
        now[0].detail, "tools/x tools/x done",
        "the kept answer is of the status before"
    );
    assert_eq!(fake.calls(), 2);
}

#[test]
fn kept_reads_only_what_ask_would_still_use() {
    let root = Path::new("/project");
    let fake = Fake::leaked(false);
    let kept = Kept::new(limits(PATIENT, PATIENT));
    kept.ask(root, "plugin", fake, &asked(TIP)).unwrap();
    assert!(kept.kept(root, "plugin", fake, &asked(TIP)).is_some());
    let mut done = asked(TIP);
    done.status = "done".into();
    assert_eq!(
        kept.kept(root, "plugin", fake, &done),
        None,
        "an answer of another status"
    );
    *fake.kept_while.lock().unwrap() = "main moved".into();
    assert_eq!(
        kept.kept(root, "plugin", fake, &asked(TIP)),
        None,
        "kept() gave an answer whose kept_while no longer holds"
    );
    // An error, until it is to be asked again: at once, where its retry is none.
    *fake.error.lock().unwrap() = Some("land exited 2".into());
    kept.ask(root, "plugin", fake, &asked(TIP)).unwrap_err();
    assert_eq!(
        kept.kept(root, "plugin", fake, &asked(TIP)),
        Some(Err("land exited 2".into()))
    );
    let retried = Kept::new(limits(PATIENT, Duration::ZERO));
    retried.ask(root, "plugin", fake, &asked(TIP)).unwrap_err();
    assert_eq!(retried.kept(root, "plugin", fake, &asked(TIP)), None);
    // Nothing while a call runs.
    *fake.error.lock().unwrap() = None;
    fake.hold();
    let slow = Kept::new(limits(Duration::from_millis(10), Duration::from_secs(30)));
    slow.ask(root, "plugin", fake, &asked(TIP)).unwrap_err();
    assert_eq!(slow.kept(root, "plugin", fake, &asked(TIP)), None);
    fake.release();
}

#[test]
fn an_error_is_kept_until_it_is_asked_again_and_a_panic_is_an_error() {
    let root = Path::new("/project");
    let fake = Fake::leaked(false);
    *fake.error.lock().unwrap() = Some("land exited 2".into());
    let kept = Kept::new(limits(PATIENT, PATIENT));
    assert_eq!(
        kept.ask(root, "plugin", fake, &asked(TIP)),
        Err("land exited 2".into())
    );
    assert_eq!(
        kept.ask(root, "plugin", fake, &asked(TIP)),
        Err("land exited 2".into())
    );
    assert_eq!(fake.calls(), 1, "an error is kept for its retry");
    // Past its retry, which is none here, it is asked again.
    let retried = Kept::new(limits(PATIENT, Duration::ZERO));
    retried.ask(root, "plugin", fake, &asked(TIP)).unwrap_err();
    assert_eq!(fake.calls(), 2);
    *fake.error.lock().unwrap() = None;
    assert!(retried.ask(root, "plugin", fake, &asked(TIP)).is_ok());
    assert_eq!(fake.calls(), 3, "then asked again");
    retried.ask(root, "plugin", fake, &asked(TIP)).unwrap();
    assert_eq!(fake.calls(), 3, "and an answer is kept for good");

    let panics = Fake::leaked(true);
    assert_eq!(
        kept.ask(root, "panicking", panics, &asked(TIP)),
        Err("fake panicked answering: the fake panics".into())
    );
}

/// An answerer whose [`Gates::kept_while`] panics, as one reading a stamp file that is gone.
struct PanicsInKeptWhile;

impl Gates for PanicsInKeptWhile {
    fn name(&self) -> &str {
        "kw"
    }

    fn answer(&self, _: &Path, _: &TaskRef<'_>, _: Option<&str>) -> Result<Vec<Gate>, String> {
        Ok(Vec::new())
    }

    fn kept_while(&self, _: &Path, _: &TaskRef<'_>) -> String {
        panic!("stamp file missing")
    }
}

#[test]
fn a_panic_in_kept_while_stays_inside_ask_as_the_answerers_error() {
    let root = Path::new("/project");
    let kept = Kept::new(limits(Duration::from_secs(1), Duration::from_secs(1)));
    let answerer: &'static PanicsInKeptWhile = Box::leak(Box::new(PanicsInKeptWhile));
    let said: Answer =
        Err("kw panicked telling what its answer reads: stamp file missing".to_string());
    let caught = std::panic::catch_unwind(AssertUnwindSafe(|| {
        kept.ask(root, "plugin", answerer, &asked(TIP))
    }));
    assert_eq!(
        caught.ok(),
        Some(said.clone()),
        "a panic in kept_while escaped Kept::ask"
    );
    assert_eq!(kept.kept(root, "plugin", answerer, &asked(TIP)), Some(said));
    assert_eq!(
        names(&theirs(
            &kept,
            root,
            &[("plugin", answerer as &dyn Gates)],
            &asked(TIP)
        )),
        [("kw", GateState::Unknown)]
    );
}

#[test]
fn a_slow_answer_reads_unknown_while_it_goes_on_and_is_kept_once_it_comes() {
    let root = Path::new("/project");
    let fake = Fake::leaked(false);
    // Held, its call gives no answer until the reads below are done: one that waited for it
    // would never end.
    fake.hold();
    let kept = Kept::new(limits(Duration::from_millis(100), Duration::from_secs(30)));
    assert_eq!(
        kept.ask(root, "plugin", fake, &asked(TIP)),
        Err("fake gave no answer within 0.1 s".into()),
        "the read waited only its wait"
    );
    // A read while the call goes on starts none.
    assert!(kept.ask(root, "plugin", fake, &asked(TIP)).is_err());
    assert_eq!(running(&kept, "plugin"), 1);
    fake.release();
    test_time::until_true("the late answer kept", test_time::FLOOR, || {
        kept.kept(root, "plugin", fake, &asked(TIP)).is_some()
    })
    .unwrap_or_else(|stall| panic!("{stall}"));
    assert!(kept.ask(root, "plugin", fake, &asked(TIP)).is_ok());
    assert_eq!(fake.calls(), 1, "the answer that came late is the one kept");
    // A read that waits for a call under way gets its answer once it comes: the fake held, the
    // second read starts while the first read's call is under way, and waits on it.
    let patient = Kept::new(limits(PATIENT, Duration::from_secs(30)));
    fake.hold();
    std::thread::scope(|scope| {
        let first = scope.spawn(|| patient.ask(root, "plugin", fake, &asked(OLD)));
        test_time::until_true("the first read's call under way", test_time::FLOOR, || {
            running(&patient, "plugin") == 1
        })
        .unwrap_or_else(|stall| panic!("{stall}"));
        let alone = holding(&patient, "plugin");
        let second = scope.spawn(|| patient.ask(root, "plugin", fake, &asked(OLD)));
        test_time::until_true("the second read began", test_time::FLOOR, || {
            holding(&patient, "plugin") > alone || running(&patient, "plugin") > 1
        })
        .unwrap_or_else(|stall| panic!("{stall}"));
        assert_eq!(running(&patient, "plugin"), 1, "it started no call");
        assert_eq!(
            holding(&patient, "plugin"),
            alone + 1,
            "it waits on the first"
        );
        fake.release();
        assert!(first.join().unwrap().is_ok());
        assert!(second.join().unwrap().is_ok());
    });
    assert_eq!(fake.calls(), 2, "two reads at once start one call");
}

#[test]
fn a_read_while_a_call_goes_on_waits_only_what_is_left_of_the_calls_wait() {
    let root = Path::new("/project");
    let fake = Fake::leaked(false);
    fake.hold();
    // A wait no test outlasts, and a call still under way that was asked that long ago, its slot
    // in the ledger as the read that started it left it: what is left of its wait is nothing.
    let wait = Duration::from_secs(3600);
    let kept: &'static Kept = Box::leak(Box::new(Kept::new(Limits {
        wait,
        ceiling: 2 * wait,
        ..Limits::default()
    })));
    let asked_long_ago = Arc::new(Slot {
        asked: asked(TIP),
        kept_while: told(fake, root, &asked(TIP)).unwrap(),
        at: Instant::now()
            .checked_sub(wait)
            .expect("a machine up an hour"),
        done: Mutex::new(None),
        ready: Condvar::new(),
    });
    kept.ledgers
        .lock()
        .recover()
        .entry(Who::new(root, "plugin", "fake"))
        .or_default()
        .slots
        .insert("tools/x".into(), asked_long_ago);
    // On a thread of its own, so a read that waited its whole wait again fails at the backstop.
    let read = std::thread::spawn(move || kept.ask(root, "plugin", fake, &asked(TIP)));
    test_time::until_true("the read ended", test_time::FLOOR, || read.is_finished())
        .unwrap_or_else(|stall| panic!("{stall}"));
    assert_eq!(
        read.join().unwrap(),
        Err("fake gave no answer within 3600 s".into())
    );
    assert_eq!(fake.calls(), 0, "it started no call");
    fake.release();
}

#[test]
fn a_call_that_never_returns_is_asked_again_after_its_ceiling() {
    let root = Path::new("/project");
    let fake = Fake::leaked(false);
    fake.hold();
    let limits = |ceiling| Limits {
        wait: Duration::from_millis(50),
        retry: Duration::from_millis(100),
        ceiling,
        calls: 4,
    };
    let kept = Kept::new(limits(PATIENT));
    assert!(kept.ask(root, "plugin", fake, &asked(TIP)).is_err());
    kept.ask(root, "plugin", fake, &asked(TIP)).unwrap_err();
    assert_eq!(
        running(&kept, "plugin"),
        1,
        "under its ceiling a call is waited for"
    );
    // Past its ceiling, which is none here, the call still under way is asked again.
    let past = Kept::new(limits(Duration::ZERO));
    past.ask(root, "plugin", fake, &asked(TIP)).unwrap_err();
    assert_eq!(running(&past, "plugin"), 1);
    past.ask(root, "plugin", fake, &asked(TIP)).unwrap_err();
    assert_eq!(
        running(&past, "plugin"),
        2,
        "past its ceiling a call is asked again"
    );
    fake.release();
}

#[test]
fn calls_that_never_return_stop_at_the_answerers_cap() {
    let root = Path::new("/project");
    let fake = Fake::leaked(false);
    fake.hold();
    // Each call past its ceiling at once, so each read starts one while the cap allows.
    let kept = Kept::new(Limits {
        wait: Duration::from_millis(10),
        retry: Duration::from_secs(30),
        ceiling: Duration::ZERO,
        calls: 2,
    });
    kept.ask(root, "plugin", fake, &asked(TIP)).unwrap_err();
    kept.ask(root, "plugin", fake, &asked(TIP)).unwrap_err();
    assert_eq!(running(&kept, "plugin"), 2);
    for _ in 0..3 {
        assert_eq!(
            kept.ask(root, "plugin", fake, &asked(TIP)),
            Err("fake has 2 calls still running".into())
        );
    }
    let mut other = asked(TIP);
    other.slug = "tools/y".into();
    assert_eq!(
        kept.ask(root, "plugin", fake, &other),
        Err("fake has 2 calls still running".into()),
        "the cap is the answerer's, over all its tasks"
    );
    assert_eq!(running(&kept, "plugin"), 2, "no call starts past the cap");
    assert!(
        kept.ask(root, "another", fake, &asked(TIP)).is_err(),
        "another plugin's answerer has its own"
    );
    assert_eq!(running(&kept, "another"), 1);
    fake.release();
}

/// An answerer that answers its gate only once each of the `of` answerers sharing `asked` has
/// been asked, so answerers asked one after another never all answer.
struct Meets {
    gate: &'static str,
    asked: &'static AtomicUsize,
    of: usize,
}

impl Gates for Meets {
    fn name(&self) -> &str {
        self.gate
    }

    fn answer(&self, _: &Path, _: &TaskRef<'_>, _: Option<&str>) -> Result<Vec<Gate>, String> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        test_time::until_true("every answerer asked", test_time::FLOOR, || {
            self.asked.load(Ordering::SeqCst) >= self.of
        })
        .map_err(|stall| stall.to_string())?;
        Ok(vec![gate(self.gate, GateState::Met)])
    }

    fn kept_while(&self, _: &Path, _: &TaskRef<'_>) -> String {
        String::new()
    }
}

#[test]
fn answerers_are_asked_at_once_so_a_read_waits_for_the_slowest_alone() {
    let root = Path::new("/project");
    let asked_ = Box::leak(Box::new(AtomicUsize::new(0)));
    let meets = |gate| -> &'static dyn Gates {
        Box::leak(Box::new(Meets {
            gate,
            asked: asked_,
            of: 2,
        }))
    };
    // Each answer waits for the other's call: a read that waited for one before it asked the
    // next would have neither.
    let kept = Kept::new(limits(PATIENT, Duration::from_secs(30)));
    let answerers = [("one", meets("certificate")), ("two", meets("nightly"))];
    let gates = theirs(&kept, root, &answerers, &asked(TIP));
    assert_eq!(
        names(&gates),
        [("certificate", GateState::Met), ("nightly", GateState::Met)],
        "in the answerers' order"
    );
}

#[test]
fn a_failing_answerer_stands_in_for_the_names_it_answered_so_the_symphonys_never_show() {
    let root = Path::new("/project");
    let fake = Fake::leaked(false);
    *fake.answers.lock().unwrap() = vec!["review", "certificate"];
    let kept = Kept::new(limits(PATIENT, Duration::from_secs(30)));
    let answerers: [(&str, &'static dyn Gates); 1] = [("plugin", fake)];
    let first = theirs(&kept, root, &answerers, &asked(OLD));
    assert_eq!(
        names(&first),
        [("review", GateState::Met), ("certificate", GateState::Met)]
    );
    assert_eq!(
        kept.names(root, "plugin", "fake"),
        ["review", "certificate"]
    );
    *fake.error.lock().unwrap() = Some("land exited 2".into());
    let own = vec![gate("review", GateState::Missing)];
    let gates = merged(own, theirs(&kept, root, &answerers, &asked(TIP)));
    assert_eq!(
        names(&gates),
        [
            ("review", GateState::Unknown),
            ("certificate", GateState::Unknown)
        ],
        "the plugin's unknown review in the Symphony's place, never beside it"
    );
    assert!(gates.iter().all(|gate| gate.detail == "land exited 2"));
}

/// An answerer that answers the gates it holds, as they are.
struct Says(&'static [(&'static str, GateState)]);

impl Gates for Says {
    fn name(&self) -> &str {
        "says"
    }

    fn answer(&self, _: &Path, _: &TaskRef<'_>, _: Option<&str>) -> Result<Vec<Gate>, String> {
        Ok(self
            .0
            .iter()
            .map(|(name, state)| gate(name, *state))
            .collect())
    }
}

#[test]
fn kept_theirs_reads_only_what_is_kept_and_known_asking_nothing() {
    let root = Path::new("/project");
    let fake = Fake::leaked(false);
    *fake.answers.lock().unwrap() = vec!["owner"];
    let kept = Kept::new(limits(PATIENT, Duration::from_secs(30)));
    let answerers: [(&str, &'static dyn Gates); 1] = [("plugin", fake)];
    let own = || vec![gate("owner", GateState::Held)];
    // Before it ever answered: nothing, so the Symphony's owner decides.
    assert!(kept_theirs(&kept, root, &answerers, &asked(TIP)).is_empty());
    assert_eq!((fake.calls(), kept.answered()), (0, 0), "nothing asked");
    // An answer kept is read as given, once counted; reading it counts and asks nothing.
    theirs(&kept, root, &answerers, &asked(TIP));
    assert_eq!(kept.answered(), 1);
    let read = kept_theirs(&kept, root, &answerers, &asked(TIP));
    assert_eq!(names(&read), [("owner", GateState::Met)]);
    assert_eq!(names(&merged(own(), read)), [("owner", GateState::Met)]);
    assert_eq!((fake.calls(), kept.answered()), (1, 1));
    // Nothing kept for another tip, though it answered `owner` for this task's last one: nothing,
    // never a stand-in from the names it gave, so the Symphony's owner decides again.
    assert_eq!(kept.names(root, "plugin", "fake"), ["owner"]);
    let other = kept_theirs(&kept, root, &answerers, &asked(OLD));
    assert!(other.is_empty());
    assert_eq!(names(&merged(own(), other)), [("owner", GateState::Held)]);
    // An error kept gives nothing either, and counts as an answer.
    *fake.error.lock().unwrap() = Some("land exited 2".into());
    theirs(&kept, root, &answerers, &asked(OLD));
    assert!(
        kept.kept(root, "plugin", fake, &asked(OLD))
            .is_some_and(|answer| answer.is_err())
    );
    assert!(kept_theirs(&kept, root, &answerers, &asked(OLD)).is_empty());
    assert_eq!((fake.calls(), kept.answered()), (2, 2));
    // A gate answered unknown is left out; the known ones beside it stay.
    let says: &'static dyn Gates = Box::leak(Box::new(Says(&[
        ("owner", GateState::Unknown),
        ("certificate", GateState::Missing),
    ])));
    let answerers: [(&str, &'static dyn Gates); 1] = [("plugin", says)];
    theirs(&kept, root, &answerers, &asked(TIP));
    let known = kept_theirs(&kept, root, &answerers, &asked(TIP));
    assert_eq!(names(&known), [("certificate", GateState::Missing)]);
    assert_eq!(
        names(&merged(own(), known)),
        [
            ("certificate", GateState::Missing),
            ("owner", GateState::Held)
        ]
    );
}

/// `git` with `args` in `root`, its output trimmed.
pub(super) fn git(root: &Path, args: &[&str]) -> String {
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_COMMON_DIR"),
    )
    .unwrap();
    assert!(done.status.success(), "{args:?}: {done:?}");
    String::from_utf8(done.stdout).unwrap().trim().to_string()
}

#[test]
fn a_branchs_tip_is_its_commit_and_the_work_folder_is_in_the_common_dir() {
    let tree = Tree::new();
    git(&tree.root, &["init", "-q", "-b", "main"]);
    tree.write("a.txt", "a");
    git(&tree.root, &["add", "a.txt"]);
    git(&tree.root, &["commit", "-q", "-m", "a"]);
    git(&tree.root, &["branch", "tools/x"]);
    git(&tree.root, &["tag", "-a", "-m", "tag", "tools/tagged"]);
    let main = git(&tree.root, &["rev-parse", "main"]);
    assert_eq!(tip(&tree.root, "tools/x").as_deref(), Some(main.as_str()));
    assert_eq!(tip(&tree.root, "tools/missing"), None);
    assert_eq!(tip(&tree.root, "tools/tagged"), None, "a tag is no branch");
    assert_eq!(tip(&tree.path("nowhere"), "main"), None);
    let common = tree.root.join(".git").canonicalize().unwrap();
    assert_eq!(work_folder(&tree.root), Some(recorded::in_common(&common)));
}

#[test]
fn a_branch_holds_work_of_its_own_only_past_the_default_branch() {
    let tree = Tree::new();
    git(&tree.root, &["init", "-q", "-b", "main"]);
    let commit = |file: &str| {
        tree.write(file, file);
        git(&tree.root, &["add", file]);
        git(&tree.root, &["commit", "-q", "-m", file]);
        git(&tree.root, &["rev-parse", "HEAD"])
    };
    let made = commit("a.txt");
    git(&tree.root, &["checkout", "-q", "-b", "tools/x"]);
    let own = commit("x.txt");
    git(&tree.root, &["checkout", "-q", "main"]);
    let later = commit("b.txt");
    let own_here = |tip: &str, default: &str| own_commits(&tree.root, tip, default);
    assert!(
        !own_here(&made, "main"),
        "made at main and not committed to"
    );
    assert!(!own_here(&later, "main"), "main's own tip");
    assert!(
        own_here(&own, "main"),
        "a commit main does not hold, though main moved on"
    );
    git(&tree.root, &["merge", "-q", "--no-edit", "tools/x"]);
    assert!(!own_here(&own, "main"), "landed: main holds it");
    assert!(
        own_here(&made, "trunk"),
        "with no default branch to compare, a tip holds its own"
    );
    assert!(own_here(TIP, "main"), "a commit the repository lacks");
    assert!(
        own_commits(&tree.path("nowhere"), &own, "main"),
        "outside a repository, nothing says it is the default branch's"
    );
}

#[test]
fn posted_card_pointer_groups_and_lazy_texts_match_the_original_reader() {
    fn reference<'a, 't>(
        cards: &[&'a PendingItem],
        mut texts: impl Iterator<Item = &'t str>,
    ) -> Vec<&'a PendingItem> {
        let mut left = cards.to_vec();
        let mut posted = Vec::new();
        while !left.is_empty() {
            let Some(text) = texts.next() else { break };
            left.retain(|card| {
                let named = super::names(text, &card.slug);
                if named {
                    posted.push(*card);
                }
                !named
            });
        }
        posted.sort_by_key(|card| cards.iter().position(|c| std::ptr::eq(*c, *card)));
        posted
    }
    let a = card("docs/A.md", "x");
    let b = card("docs/B.md", "y");
    let equal_but_distinct = card("docs/A.md", "x");
    let cases = [
        vec![],
        vec![&a],
        vec![&a, &b, &a],
        vec![&b, &a, &b, &a],
        vec![&a, &equal_but_distinct, &b, &a, &equal_but_distinct],
        vec![&equal_but_distinct, &a, &equal_but_distinct, &b, &a],
    ];
    for cards in cases {
        for texts in [
            vec![],
            vec!["x"],
            vec!["y"],
            vec!["y", "x", "unused"],
            vec!["x and y", "unused"],
            vec!["xy or x-2", "none"],
            vec!["x", "x", "y", "unused"],
        ] {
            let old_reads = std::cell::Cell::new(0);
            let new_reads = std::cell::Cell::new(0);
            let expected = reference(
                &cards,
                texts
                    .iter()
                    .copied()
                    .inspect(|_| old_reads.set(old_reads.get() + 1)),
            );
            let actual = posted_cards(
                &cards,
                texts
                    .iter()
                    .copied()
                    .inspect(|_| new_reads.set(new_reads.get() + 1)),
            );
            assert_eq!(actual.len(), expected.len(), "{texts:?}");
            assert!(
                actual
                    .iter()
                    .zip(&expected)
                    .all(|(a, b)| std::ptr::eq(*a, *b)),
                "pointer order differs for {texts:?}"
            );
            assert_eq!(
                new_reads.get(),
                old_reads.get(),
                "text consumption for {texts:?}"
            );
        }
    }
    let records: Vec<_> = (0..256)
        .map(|index| card("docs/A.md", &format!("item-{index}")))
        .collect();
    let mut refs: Vec<_> = records.iter().collect();
    // Include nonadjacent pointer aliases; grouping must remain identical at larger sizes too.
    refs.extend(records.iter().step_by(7));
    let all = records
        .iter()
        .map(|card| card.slug.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let reverse: Vec<_> = records
        .iter()
        .rev()
        .map(|card| card.slug.as_str())
        .collect();
    for texts in [vec![all.as_str()], reverse] {
        let (expected, old_cpu) = test_time::cpu::spent(|| reference(&refs, texts.iter().copied()));
        let (actual, new_cpu) =
            test_time::cpu::spent(|| posted_cards(&refs, texts.iter().copied()));
        assert_eq!(actual.len(), expected.len());
        assert!(
            actual
                .iter()
                .zip(&expected)
                .all(|(a, b)| std::ptr::eq(*a, *b))
        );
        eprintln!(
            "posted-card pointer groups: refs={}, texts={}, old_thread_cpu={old_cpu:?}, new_thread_cpu={new_cpu:?}; one extra output allocation after dropping left, <= refs*selected pointer comparisons",
            refs.len(),
            texts.len()
        );
    }
    let aliases = posted_cards(&[&a, &b, &a], std::iter::once("x and y"));
    assert!(std::ptr::eq(aliases[0], &a));
    assert!(std::ptr::eq(aliases[1], &a));
    assert!(std::ptr::eq(aliases[2], &b));
}

#[test]
fn check_history_preserves_total_float_order_and_equal_timestamp_input_order() {
    let times = [
        1.0_f64,
        -0.0,
        f64::NAN,
        0.0,
        1.0,
        f64::NEG_INFINITY,
        -f64::NAN,
        f64::NAN,
    ];
    let records: Vec<_> = times
        .into_iter()
        .enumerate()
        .map(|(index, at)| {
            let mut record = run(
                u8::try_from(index).unwrap(),
                "check",
                "tools/x",
                OLD,
                Step::Finished,
                Some(0),
            );
            record.ended_at = Some(at);
            record
        })
        .collect();
    let mut expected: Vec<_> = records
        .iter()
        .map(|record| {
            (
                record.ended_at.unwrap(),
                record.handle.clone(),
                record.commit.clone(),
                format!("`check` green at {}", &OLD[..7]),
            )
        })
        .collect();
    expected.sort_by(|a, b| a.0.total_cmp(&b.0));
    let actual = check("check", "tools/x", Some(TIP), &records).past;
    assert_eq!(actual.len(), expected.len());
    for (actual, (at, by, commit, detail)) in actual.iter().zip(expected) {
        assert_eq!(actual.at.to_bits(), at.to_bits());
        assert_eq!(actual.by, by);
        assert_eq!(actual.commit, commit);
        assert_eq!(actual.detail, detail);
        assert_eq!(actual.state, GateState::Met);
    }
    assert!(check("check", "tools/x", Some(TIP), &[]).past.is_empty());
}
