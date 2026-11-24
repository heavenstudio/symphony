//! A task's stamps as its gates: each needed one met, missing or held at the tip and stale at a
//! later commit, each left out waived by its assessment, and which tasks keep the gates they had
//! before stamps.

use super::*;
use std::collections::BTreeMap;
use symphony_board::cli::machine_work::recorded::{Phase, Step};
use symphony_board::stamps::posted::Posted;
use symphony_board::stamps::{NEEDS, Needs};

const TIP: &str = "4be91c2aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const LATER: &str = "7c0ffee1bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const SLUG: &str = "tools/x";
const FOLDER: &str = "artifacts/tools/x/";

/// A checkout that is not there: git says nothing of the cards a branch posted in it.
fn nowhere() -> &'static Path {
    Path::new("/nowhere")
}

fn task(status: &str, needs: Option<Needs>) -> Task {
    let mut fields = BTreeMap::new();
    if let Some(needs) = needs {
        fields.insert(
            symphony_board::red::FIELDS.to_string(),
            BTreeMap::from([(NEEDS.to_string(), serde_json::to_value(needs).unwrap())]),
        );
    }
    Task {
        branch: None,
        id: 1,
        slug: SLUG.into(),
        title: "x".into(),
        body: String::new(),
        status: status.into(),
        priority: 1,
        parent: None,
        created_by: "quiet-plover".into(),
        created: 1.0,
        updated: 1.0,
        links: Vec::new(),
        depends: Vec::new(),
        roles: Vec::new(),
        instruction: false,
        fields,
    }
}

fn assessed(stamps: &[&str], tip: Option<&str>) -> Needs {
    Needs {
        stamps: stamps.iter().map(|stamp| stamp.to_string()).collect(),
        tip: tip.map(str::to_string),
        by: "slate-owl".into(),
        why: "a page's look and a store query".into(),
        import: None,
    }
}

fn line(id: i64, kind: &str, sender: &str, text: &str) -> Message {
    Message {
        id,
        slug: SLUG.into(),
        sender: sender.into(),
        recipient: None,
        kind: kind.into(),
        text: text.into(),
        ts: 100.0 * f64::from(u16::try_from(id).unwrap()),
    }
}

fn run(id: u8, label: &str, phase: Phase, commit: &str) -> Record {
    let started = 1000.0 * f64::from(id);
    Record {
        version: 1,
        operation_id: format!("{id:032x}"),
        parent_id: None,
        phase,
        label: label.into(),
        state: Step::Finished,
        handle: Some("teal-merlin".into()),
        task: Some("tools/x/work".into()),
        checkout: "/checkout".into(),
        branch: Some(SLUG.into()),
        commit: Some(commit.into()),
        machine_id: None,
        host: "host".into(),
        pid: 10,
        child_pid: Some(11),
        queued_at: started - 5.0,
        started_at: Some(started),
        ended_at: Some(started + 60.0),
        beat_at: started,
        exit_code: Some(0),
        signal: None,
        reason: String::new(),
        run: None,
        reading: None,
    }
}

/// The thread of a task given every stamp at `TIP`: its assessment, its pictures, its reviewer's
/// approve and the owner's.
fn given() -> Vec<Message> {
    vec![
        line(
            1,
            "system",
            "slate-owl",
            "needs tests, review at 4be91c2 (was review): a store query; by slate-owl",
        ),
        line(
            2,
            "msg",
            "teal-merlin",
            "Wide at 4be91c2: artifacts/tools/x/wide.png",
        ),
        line(
            3,
            "verdict",
            "lucid-heron",
            &format!("approve: {TIP}: holds"),
        ),
        line(4, "msg", "owner", &format!("approve {TIP}")),
    ]
}

/// The gates of `record` at `tip` from `thread` and `runs`, the Symphony's catalogue.
fn read(record: &Task, thread: &[Message], runs: &[Record], tip: Option<&str>) -> Vec<Gate> {
    read_with(record, thread, runs, tip, &[], &Catalogue::symphony())
}

fn read_with(
    record: &Task,
    thread: &[Message],
    runs: &[Record],
    tip: Option<&str>,
    cards: &[&PendingItem],
    catalogue: &Catalogue,
) -> Vec<Gate> {
    read_posted(record, thread, runs, tip, cards, catalogue, &|_| {
        Posted::Nothing
    })
}

/// [`read_with`], the cards the branch posted saying `posted` of the owner's stamp.
fn read_posted(
    record: &Task,
    thread: &[Message],
    runs: &[Record],
    tip: Option<&str>,
    cards: &[&PendingItem],
    catalogue: &Catalogue,
    posted: &dyn Fn(&str) -> Posted,
) -> Vec<Gate> {
    let approval = |tip: &str| Approval {
        posted: posted(tip),
        by: "owner".into(),
    };
    let read = Reads {
        thread,
        runs,
        tip,
        cards,
    };
    read_deciding(record, &read, catalogue, &[], None, &approval)
}

/// What a case's gates are read from.
struct Reads<'a> {
    thread: &'a [Message],
    runs: &'a [Record],
    tip: Option<&'a str>,
    cards: &'a [&'a PendingItem],
}

/// [`read_posted`] on a board where `deciders` hold decide, the cards saying `posted`.
fn read_deciding(
    record: &Task,
    read: &Reads<'_>,
    catalogue: &Catalogue,
    deciders: &[String],
    machine: Option<&symphony_board::people::MachinePerson>,
    posted: &dyn Fn(&str) -> Approval,
) -> Vec<Gate> {
    let Reads {
        thread,
        runs,
        tip,
        cards,
    } = *read;
    let ours = [SLUG.to_string(), "tools/x/work".to_string()];
    let evidence = Evidence {
        thread,
        runs,
        ours: &ours,
        folder: FOLDER,
        branch: SLUG,
        tip,
        cards,
        deciders,
        machine,
        posted,
    };
    stamps(record, catalogue, &evidence)
}

fn states(gates: &[Gate]) -> Vec<(&str, GateState, &str)> {
    (gates.iter())
        .map(|gate| (gate.name.as_str(), gate.state, gate.detail.as_str()))
        .collect()
}

#[test]
fn an_open_or_assessed_top_level_task_has_stamps_and_history_keeps_its_gates() {
    for status in ["ready", "in_progress", "blocked", "review"] {
        assert!(stamped(&task(status, None)), "{status}");
    }
    for status in ["done", "cancelled"] {
        assert!(!stamped(&task(status, None)), "{status} with no assessment");
        assert!(
            stamped(&task(status, Some(assessed(&[], None)))),
            "{status}"
        );
    }
    let mut subtask = task("in_progress", Some(assessed(&["tests"], None)));
    subtask.parent = Some("tools".into());
    assert!(!stamped(&subtask));
}

#[test]
fn a_task_no_assessment_read_needs_review_and_the_other_stamps_are_waived_by_default() {
    let gates = read(&task("in_progress", None), &[], &[], Some(TIP));
    assert_eq!(
        states(&gates),
        [
            (
                "tests",
                GateState::Waived,
                "Not needed by default: no assessment yet"
            ),
            ("review", GateState::Missing, "Not reviewed yet"),
            (
                "pictures",
                GateState::Waived,
                "Not needed by default: no assessment yet"
            ),
            (
                "owner",
                GateState::Waived,
                "Not needed by default: no assessment yet"
            ),
        ]
    );
    assert_eq!(
        gates[0].why.as_deref(),
        Some("a task no assessment has read needs only the catalogue's defaults: review")
    );
}

#[test]
fn each_needed_stamp_is_met_at_the_tip_it_names() {
    let every = assessed(&["tests", "review", "pictures", "owner"], Some(TIP));
    let record = task("review", Some(every));
    let runs = [
        run(1, "just test-rs", Phase::Test, TIP),
        run(2, "just lint-rs", Phase::Build, TIP),
    ];
    let gates = read(&record, &given(), &runs, Some(TIP));
    assert_eq!(
        states(&gates),
        [
            (
                "tests",
                GateState::Met,
                "Green at 4be91c2: `just test-rs`, `just lint-rs`"
            ),
            ("review", GateState::Met, "Approved in round 1 of 1"),
            (
                "pictures",
                GateState::Met,
                "Pictures at 4be91c2, posted by teal-merlin"
            ),
            ("owner", GateState::Met, "Approved by the owner at 4be91c2"),
        ]
    );
    for gate in &gates {
        assert_eq!(gate.commit.as_deref(), Some(TIP), "{}", gate.name);
        assert!(gate.signed(), "{}", gate.name);
    }
    assert_eq!(gates[1].by.as_deref(), Some("lucid-heron"));
    assert_eq!(gates[2].link.as_deref(), Some("/task/tools/x#message-2"));
    assert_eq!(gates[3].link.as_deref(), Some("/task/tools/x#message-4"));
}

#[test]
fn each_needed_stamp_read_at_a_later_commit_is_stale_and_names_the_one_it_stood_on() {
    let every = assessed(&["tests", "review", "pictures", "owner"], Some(TIP));
    let record = task("review", Some(every));
    let runs = [run(1, "just test-rs", Phase::Test, TIP)];
    let gates = read(&record, &given(), &runs, Some(LATER));
    assert_eq!(
        states(&gates),
        [
            (
                "tests",
                GateState::Missing,
                "No build or test at 7c0ffee: the last ran at 4be91c2"
            ),
            (
                "review",
                GateState::Missing,
                "Approved in round 1 of 1 at 4be91c2, not the tip 7c0ffee"
            ),
            (
                "pictures",
                GateState::Missing,
                "Pictures posted at 4be91c2, none at 7c0ffee"
            ),
            (
                "owner",
                GateState::Held,
                "The owner approved 4be91c2, not the tip 7c0ffee"
            ),
        ]
    );
    assert_eq!(gates[1].commit.as_deref(), Some(TIP));
    assert_eq!(gates[2].commit.as_deref(), Some("4be91c2"));
    assert_eq!(
        gates[3].why.as_deref(),
        Some("the task needs the owner's approve of its tip, which its page offers")
    );
    // An approve naming no commit stands on no tip either.
    let thread = [line(3, "verdict", "lucid-heron", "approve: holds")];
    let review = &read(&record, &thread, &runs, Some(TIP))[1];
    assert_eq!(
        (
            review.state,
            review.detail.as_str(),
            review.commit.as_deref()
        ),
        (
            GateState::Missing,
            "Approved in round 1 of 1 naming no commit, not the tip 4be91c2",
            None
        )
    );
}

#[test]
fn a_needed_stamp_with_nothing_given_is_missing_and_the_owners_is_held() {
    let every = assessed(&["tests", "review", "pictures", "owner"], Some(TIP));
    let record = task("in_progress", Some(every.clone()));
    let gates = read(&record, &[], &[], Some(TIP));
    assert_eq!(
        states(&gates),
        [
            (
                "tests",
                GateState::Missing,
                "No build or test recorded for it at 4be91c2"
            ),
            ("review", GateState::Missing, "Not reviewed yet"),
            (
                "pictures",
                GateState::Missing,
                "No picture posted at 4be91c2"
            ),
            (
                "owner",
                GateState::Held,
                "Waits on the owner's approve of 4be91c2"
            ),
        ]
    );
    // With no branch, nothing stands on a tip.
    let gates = read(&record, &given(), &[], None);
    let no_branch = "No branch tools/x in this repository";
    assert_eq!(
        states(&gates),
        [
            ("tests", GateState::Missing, no_branch),
            (
                "review",
                GateState::Missing,
                "Approved in round 1 of 1, but no branch tools/x is in this repository"
            ),
            ("pictures", GateState::Missing, no_branch),
            ("owner", GateState::Held, no_branch),
        ]
    );
    // Not started, it has no branch yet: nothing is missing from the repository, and each needed
    // stamp still waits as an open task's does.
    for status in ["backlog", "concept", "design", "ready"] {
        let gates = read(&task(status, Some(every.clone())), &given(), &[], None);
        assert_eq!(
            states(&gates),
            [
                ("tests", GateState::Missing, NO_BRANCH_YET),
                (
                    "review",
                    GateState::Missing,
                    "Approved in round 1 of 1, but it has no branch yet"
                ),
                ("pictures", GateState::Missing, NO_BRANCH_YET),
                ("owner", GateState::Held, NO_BRANCH_YET),
            ],
            "{status}"
        );
    }
    // Blocked or in review, it started: its branch is missing.
    for status in ["blocked", "review"] {
        let gates = read(&task(status, Some(every.clone())), &given(), &[], None);
        assert_eq!(gates[0].detail, no_branch, "{status}");
    }
    // Closed, with no branch and no land naming what it merged, where it stood cannot be read:
    // unknown, never missing, and nothing is held.
    let every = assessed(&["tests", "review", "pictures", "owner"], Some(TIP));
    for status in ["done", "cancelled"] {
        let gates = read(&task(status, Some(every.clone())), &given(), &[], None);
        let gone = "No branch tools/x in this repository, nor a land on its thread naming the \
                    commit it merged";
        assert_eq!(
            states(&gates),
            [
                ("tests", GateState::Unknown, gone),
                (
                    "review",
                    GateState::Unknown,
                    "Approved in round 1 of 1, but no branch tools/x is in this repository, nor a \
                     land on its thread naming the commit it merged"
                ),
                ("pictures", GateState::Unknown, gone),
                ("owner", GateState::Unknown, gone),
            ],
            "{status}"
        );
        assert!(gates.iter().all(|gate| gate.why.is_none()), "{status}");
    }
}

#[test]
fn a_stamp_the_assessment_left_out_is_waived_its_line_the_reason_until_a_later_commit() {
    let record = task("review", Some(assessed(&["tests"], Some(TIP))));
    let runs = [run(1, "just test-rs", Phase::Test, TIP)];
    let gates = read(&record, &given(), &runs, Some(TIP));
    let left_out = "Left out by the assessment at 4be91c2";
    assert_eq!(
        states(&gates),
        [
            ("tests", GateState::Met, "Green at 4be91c2: `just test-rs`"),
            ("review", GateState::Waived, left_out),
            ("pictures", GateState::Waived, left_out),
            ("owner", GateState::Waived, left_out),
        ]
    );
    // Waived even where it would stand: the assessment says what the task needs.
    assert_eq!(
        gates[1],
        Gate {
            name: "review".into(),
            state: GateState::Waived,
            by: Some("slate-owl".into()),
            at: Some(100.0),
            commit: Some(TIP.into()),
            why: Some("a page's look and a store query".into()),
            detail: left_out.into(),
            link: Some("/task/tools/x#message-1".into()),
            past: Vec::new(),
        }
    );
    // A commit after the one it read brings the defaults back: review is needed again.
    let gates = read(&record, &given(), &runs, Some(LATER));
    assert_eq!(
        states(&gates)[..2],
        [
            (
                "tests",
                GateState::Missing,
                "No build or test at 7c0ffee: the last ran at 4be91c2"
            ),
            (
                "review",
                GateState::Missing,
                "Approved in round 1 of 1 at 4be91c2, not the tip 7c0ffee"
            ),
        ]
    );
    assert_eq!(gates[2].state, GateState::Waived);
    // An assessment of the brief waives what it leaves out but the defaults, with no link to a
    // line it cannot find.
    let brief = task("ready", Some(assessed(&[], None)));
    let gates = read(&brief, &[], &[], Some(TIP));
    assert_eq!(gates[1].state, GateState::Missing);
    assert_eq!(
        (
            gates[0].detail.as_str(),
            gates[0].link.as_deref(),
            gates[0].at
        ),
        ("Left out by the assessment from the brief", None, None)
    );
}

#[test]
fn a_stamp_a_gate_decision_waived_at_the_tip_is_waived_by_it_and_needed_again_at_a_later_one() {
    use symphony_board::gates::{Decision, GATES, Kind};
    let mut record = task("review", Some(assessed(&["tests", "pictures"], Some(TIP))));
    let waiver = Decision {
        gate: "pictures".into(),
        kind: Kind::Waive,
        from: None,
        to: None,
        tip: Some(TIP.into()),
        by: "amber-kite".into(),
        why: "the row is measured, never seen".into(),
        at: 500.0,
    };
    // A waiver of tests, which no command records, waives nothing.
    let never = Decision {
        gate: "tests".into(),
        ..waiver.clone()
    };
    (record.fields.get_mut(symphony_board::red::FIELDS).unwrap()).insert(
        GATES.into(),
        serde_json::to_value([&waiver, &never]).unwrap(),
    );
    let mut thread = given();
    thread.push(line(5, "system", "amber-kite", &waiver.line()));
    let runs = [run(1, "just test-rs", Phase::Test, TIP)];
    let gates = read(&record, &thread, &runs, Some(TIP));
    assert_eq!(gates[0].state, GateState::Met);
    assert_eq!(
        gates[2],
        Gate {
            name: "pictures".into(),
            state: GateState::Waived,
            by: Some("amber-kite".into()),
            at: Some(500.0),
            commit: Some(TIP.into()),
            why: Some("the row is measured, never seen".into()),
            detail: "Waived by a gate decision at 4be91c2".into(),
            link: Some("/task/tools/x#message-5".into()),
            past: Vec::new(),
        }
    );
    assert!(
        !left_out(&gates[2]),
        "its lane is drawn: a decision, not the assessment"
    );
    // At a later commit the stamp is needed again, read as any needed stamp is.
    let gates = read(&record, &thread, &runs, Some(LATER));
    assert_eq!(gates[2].state, GateState::Missing);
}

#[test]
fn a_done_task_with_an_assessment_reads_its_stamps_and_a_covered_close_stays_waived() {
    // Done with no round, its review needed: it landed without one, which nothing waived.
    let record = task("done", Some(assessed(&["tests", "review"], Some(TIP))));
    let gates = read(&record, &[], &[], Some(TIP));
    assert_eq!(gates[1].state, GateState::Missing);
    assert_eq!(gates[1].detail, "Landed with no review verdict");
    assert_eq!(gates[1].why, None);
    // Done with its review waived by its assessment: waived from it, not from a bug fix's rule.
    let record = task("done", Some(assessed(&["tests"], Some(TIP))));
    let gates = read(&record, &[], &[], Some(TIP));
    assert_eq!(gates[1].state, GateState::Waived);
    assert_eq!(
        gates[1].why.as_deref(),
        Some("a page's look and a store query")
    );
    // Done by a covered close: its work landed with another's, needed or not.
    let covered = symphony_board::covered_line("tools/y", TIP, "quiet-plover", "its fix");
    let thread = [line(5, "verdict", "quiet-plover", &covered)];
    for needs in [assessed(&["review"], Some(TIP)), assessed(&[], Some(TIP))] {
        let gates = read(&task("done", Some(needs)), &thread, &[], Some(TIP));
        assert_eq!(gates[1].state, GateState::Waived);
        assert!(
            gates[1].detail.starts_with("Covered by tools/y"),
            "{}",
            gates[1].detail
        );
    }
}

#[test]
fn only_a_stamp_left_out_by_an_assessment_or_by_default_goes_without_a_timeline_lane() {
    let named = |gates: &[Gate]| -> Vec<String> {
        (gates.iter())
            .filter(|gate| left_out(gate))
            .map(|gate| gate.name.clone())
            .collect()
    };
    // With no assessment, the stamps not needed by default; review, needed, keeps its lane.
    let gates = read(&task("ready", None), &[], &[], Some(TIP));
    assert_eq!(named(&gates), ["tests", "pictures", "owner"]);
    // With one, each it left out.
    let record = task("review", Some(assessed(&["tests"], Some(TIP))));
    let gates = read(&record, &given(), &[], Some(TIP));
    assert_eq!(named(&gates), ["review", "pictures", "owner"]);
    // A review a covered close waived, a bug fix's waiver of a task with no stamps and a plugin's
    // waived gate are what happened to the task: their lanes stay.
    let covered = symphony_board::covered_line("tools/y", TIP, "quiet-plover", "its fix");
    let thread = [line(5, "verdict", "quiet-plover", &covered)];
    let gates = read(
        &task("done", Some(assessed(&[], Some(TIP)))),
        &thread,
        &[],
        Some(TIP),
    );
    assert_eq!(named(&gates), ["tests", "pictures", "owner"]);
    let fixed = review_of(SLUG, "done", &[], &[]);
    let plugins = Gate {
        name: "dependency".into(),
        state: GateState::Waived,
        detail: "Left out by the assessment at 4be91c2".into(),
        ..Gate::default()
    };
    for gate in [fixed, plugins] {
        assert_eq!(gate.state, GateState::Waived, "{gate:?}");
        assert!(!left_out(&gate), "{gate:?}");
    }
}

#[test]
fn a_card_the_task_posted_holds_its_owner_stamp_needed_or_not_and_in_any_catalogue() {
    let card = PendingItem {
        path: "docs/DECISIONS.md".into(),
        slug: "x".into(),
        href: "/docs/DECISIONS.md#x".into(),
        branch: None,
    };
    let record = task("in_progress", Some(assessed(&["owner"], Some(TIP))));
    let thread = given();
    for record in [record, task("in_progress", None)] {
        let gates = read_with(
            &record,
            &thread,
            &[],
            Some(TIP),
            &[&card],
            &Catalogue::symphony(),
        );
        assert_eq!(
            states(&gates)[3],
            (
                "owner",
                GateState::Held,
                "Card x in docs/DECISIONS.md waits on the owner"
            )
        );
    }
    // A project's catalogue with no owner stamp: the card still holds the task, after its stamps.
    let tests_only = Catalogue::parse("[stamps.tests]\ndefault = true\n").unwrap();
    let record = task("in_progress", None);
    let gates = read_with(&record, &thread, &[], Some(TIP), &[&card], &tests_only);
    assert_eq!(
        states(&gates),
        [
            (
                "tests",
                GateState::Missing,
                "No build or test recorded for it at 4be91c2"
            ),
            (
                "owner",
                GateState::Held,
                "Card x in docs/DECISIONS.md waits on the owner"
            ),
        ]
    );
    assert_eq!(
        read_with(&record, &thread, &[], Some(TIP), &[], &tests_only).len(),
        1
    );
}

/// The owner's approve of the cards the task's branch posted meets the `owner` stamp at the tip
/// where the thread names no approve of it, naming the cards; what the cards say is asked only
/// then, at the tip read.
#[test]
fn the_owners_stamp_is_met_by_the_approve_of_the_cards_the_branch_posted() {
    let record = task("review", Some(assessed(&["owner"], Some(TIP))));
    let asked = std::cell::RefCell::new(Vec::new());
    let posted = |tip: &str| {
        asked.borrow_mut().push(tip.to_string());
        Posted::Approved {
            cards: vec!["look".into()],
        }
    };
    let catalogue = Catalogue::symphony();
    let gates = read_posted(&record, &[], &[], Some(TIP), &[], &catalogue, &posted);
    let owner = gates.iter().find(|gate| gate.name == "owner").unwrap();
    assert_eq!(
        (owner.state, owner.detail.as_str(), owner.link.as_deref()),
        (
            GateState::Met,
            "Approved by the owner on card look, at 4be91c2",
            None
        )
    );
    assert_eq!(owner.commit.as_deref(), Some(TIP));
    assert_eq!(*asked.borrow(), [TIP]);
    // The thread's approve of the tip stands first: the cards are not asked.
    asked.borrow_mut().clear();
    let gates = read_posted(&record, &given(), &[], Some(TIP), &[], &catalogue, &posted);
    let owner = gates.iter().find(|gate| gate.name == "owner").unwrap();
    assert_eq!(owner.detail, "Approved by the owner at 4be91c2");
    assert!(asked.borrow().is_empty());
}

#[test]
fn the_owners_approve_is_one_commits_id_posted_as_an_approve_line() {
    assert_eq!(
        approve_line(&format!(" {TIP}\n")).unwrap(),
        format!("approve {TIP}")
    );
    assert_eq!(approve_line("4be91c2").unwrap(), "approve 4be91c2");
    for refused in [
        "",
        "4be91c",
        "defaced",
        "4be91c2 and more",
        "approve 4be91c2",
        "4BE91C2",
    ] {
        let why = approve_line(refused).unwrap_err();
        assert!(why.contains("7 to 40 hex digits"), "{refused:?}: {why}");
    }
}

/// The land leaves a task lacking only the owner's stamp done by its approve, so the page offers
/// the owner's approve there too, until a landing or the owner's approve of the tip answers it.
/// A decider's approve meets the task's `owner` gate as the owner's does, on the thread and on the
/// cards, named as theirs; the same approve on a board where they hold no decide does not.
#[test]
fn a_deciders_approve_meets_the_owner_gate_on_the_thread_and_on_the_cards() {
    let record = task("review", Some(assessed(&["owner"], Some(TIP))));
    let catalogue = Catalogue::symphony();
    let hers = [line(4, "msg", "ana", &format!("approve {TIP}"))];
    let deciders = ["ana".to_string()];
    let nothing = |_: &str| Approval {
        posted: Posted::Nothing,
        by: "owner".into(),
    };
    let owner_gate =
        |thread: &[Message], deciders: &[String], posted: &dyn Fn(&str) -> Approval| {
            let read = Reads {
                thread,
                runs: &[],
                tip: Some(TIP),
                cards: &[],
            };
            let gates = read_deciding(&record, &read, &catalogue, deciders, None, posted);
            let owner = gates.into_iter().find(|gate| gate.name == "owner").unwrap();
            (owner.state, owner.detail)
        };
    assert_eq!(
        owner_gate(&hers, &deciders, &nothing),
        (GateState::Met, "Approved by ana at 4be91c2".to_string())
    );
    assert_eq!(owner_gate(&hers, &[], &nothing).0, GateState::Held);
    let cards = |_: &str| Approval {
        posted: Posted::Approved {
            cards: vec!["look".into()],
        },
        by: "ana".into(),
    };
    assert_eq!(
        owner_gate(&[], &deciders, &cards),
        (
            GateState::Met,
            "Approved by ana on card look, at 4be91c2".to_string()
        )
    );
}

/// The person at the machine, once named, meets the task's `owner` gate by an approve under their
/// name, and the gate names them by how they are shown, waiting and met.
#[test]
fn the_owner_gate_names_the_machines_person_by_how_they_are_shown() {
    let record = task("review", Some(assessed(&["owner"], Some(TIP))));
    let catalogue = Catalogue::symphony();
    let stefano = symphony_board::people::MachinePerson {
        name: "stefano".into(),
        shown: "Stefano".into(),
    };
    let deciders = symphony_board::people::with_machine(Vec::new(), Some(&stefano));
    let nothing = |_: &str| Approval {
        posted: Posted::Nothing,
        by: "owner".into(),
    };
    let owner_gate = |thread: &[Message]| {
        let read = Reads {
            thread,
            runs: &[],
            tip: Some(TIP),
            cards: &[],
        };
        let gates = read_deciding(&record, &read, &catalogue, &deciders, Some(&stefano), &nothing);
        let owner = gates.into_iter().find(|gate| gate.name == "owner").unwrap();
        (owner.state, owner.detail)
    };
    assert_eq!(
        owner_gate(&[]),
        (GateState::Held, "Waits on Stefano's approve of 4be91c2".to_string())
    );
    for by in ["owner", "stefano"] {
        let theirs = [line(4, "msg", by, &format!("approve {TIP}"))];
        assert_eq!(
            owner_gate(&theirs),
            (GateState::Met, "Approved by Stefano at 4be91c2".to_string()),
            "{by}"
        );
    }
}

#[test]
fn the_owner_is_waited_on_while_open_or_done_by_an_approve_no_landing_closed() {
    let approve = line(
        3,
        "verdict",
        "teal-merlin",
        &format!("approve: {TIP} reads right"),
    );
    let owner = line(4, "msg", "owner", &format!("approve {TIP}"));
    for status in ["ready", "in_progress", "review", "blocked"] {
        assert!(awaits_owner(status, &[], TIP, nowhere(), &[]), "{status}");
        assert!(
            !awaits_owner(status, std::slice::from_ref(&owner), TIP, nowhere(), &[]),
            "{status}"
        );
    }
    assert!(awaits_owner(
        "done",
        std::slice::from_ref(&approve),
        TIP,
        nowhere(),
        &[]
    ));
    assert!(!awaits_owner(
        "done",
        &[approve.clone(), owner.clone()],
        TIP,
        nowhere(),
        &[]
    ));
    // The owner's approve of another commit leaves it waiting on the tip.
    let other = line(4, "msg", "owner", &format!("approve {LATER}"));
    assert!(awaits_owner(
        "done",
        &[approve.clone(), other],
        TIP,
        nowhere(),
        &[]
    ));
    // Done by a landing, a covered close or no verdict at all: nothing waits.
    let landed = line(
        5,
        "verdict",
        "quiet-plover",
        "landed tools/x on main at 0123abc4",
    );
    let covered = line(
        5,
        "verdict",
        "quiet-plover",
        "covered by tools/y, whose landing at 0123abc4",
    );
    for thread in [
        vec![approve.clone(), landed],
        vec![approve.clone(), covered],
        Vec::new(),
    ] {
        assert!(
            !awaits_owner("done", &thread, TIP, nowhere(), &[]),
            "{thread:?}"
        );
    }
    assert!(!awaits_owner(
        "cancelled",
        std::slice::from_ref(&approve),
        TIP,
        nowhere(),
        &[]
    ));
    // A decider's approve ends the wait as the owner's does, and only while they decide.
    let hers = line(4, "msg", "ana", &format!("approve {TIP}"));
    let deciders = ["ana".to_string()];
    assert!(!awaits_owner(
        "review",
        std::slice::from_ref(&hers),
        TIP,
        nowhere(),
        &deciders
    ));
    assert!(awaits_owner(
        "review",
        std::slice::from_ref(&hers),
        TIP,
        nowhere(),
        &[]
    ));
    assert!(!awaits_owner(
        "done",
        &[approve, hers],
        TIP,
        nowhere(),
        &deciders
    ));
}

#[test]
fn the_catalogue_that_cannot_be_read_is_one_unknown_gate() {
    let gate = unreadable("symphony.toml: [stamps] names no stamp\nmore");
    assert_eq!(
        (gate.name.as_str(), gate.state, gate.detail.as_str()),
        (
            "stamps",
            GateState::Unknown,
            "symphony.toml: [stamps] names no stamp"
        )
    );
}

#[test]
fn a_picture_the_stamp_reads_is_one_the_uploads_store_and_draw() {
    assert_eq!(read::UPLOADS, crate::uploads::ROUTE);
    for kind in crate::uploads::KINDS.iter().filter(|kind| kind.picture) {
        assert!(read::PICTURES.contains(&kind.ext), "{}", kind.ext);
    }
}
