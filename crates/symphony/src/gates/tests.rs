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
}

/// A run of `label` as the board's recorder keeps it: `id` its operation, finished with `exit`
/// unless `state` says otherwise.
fn run(id: u8, label: &str, branch: &str, commit: &str, state: Step, exit: Option<i32>) -> Record {
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
    let linked = linked_cards(&links, &pending);
    assert_eq!(
        linked.iter().map(|c| c.slug.as_str()).collect::<Vec<_>>(),
        ["x", "y"]
    );
    assert_eq!(linked_cards(&[], &pending), Vec::<&PendingItem>::new());
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
fn an_answerer_that_failed_is_one_unknown_gate_saying_the_first_line_of_why() {
    assert_eq!(
        failed("stamps", "land exited 2\nits whole log"),
        Gate {
            name: "stamps".into(),
            state: GateState::Unknown,
            detail: "land exited 2".into(),
            ..Gate::default()
        }
    );
}

/// An answerer whose answer the test sets, counting its calls: it answers `gates`, or `error`,
/// after `delay`, or panics.
struct Fake {
    calls: AtomicUsize,
    delay: Mutex<Duration>,
    error: Mutex<Option<String>>,
    panics: bool,
    kept_while: Mutex<String>,
}

impl Fake {
    fn leaked(panics: bool) -> &'static Fake {
        Box::leak(Box::new(Fake {
            calls: AtomicUsize::new(0),
            delay: Mutex::new(Duration::ZERO),
            error: Mutex::new(None),
            panics,
            kept_while: Mutex::new(String::new()),
        }))
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Gates for Fake {
    fn name(&self) -> &str {
        "fake"
    }

    fn answer(&self, _: &Path, task: &TaskRef<'_>, tip: Option<&str>) -> Result<Vec<Gate>, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(*self.delay.lock().unwrap());
        assert!(!self.panics, "the fake panics");
        if let Some(error) = self.error.lock().unwrap().clone() {
            return Err(error);
        }
        let mut gate = gate("certificate", GateState::Met);
        gate.commit = tip.map(str::to_string);
        gate.detail = format!("{} {}", task.slug, task.branch);
        Ok(vec![gate])
    }

    fn kept_while(&self, _: &Path, _: &TaskRef<'_>) -> String {
        self.kept_while.lock().unwrap().clone()
    }
}

fn asked(tip: &str) -> Asked {
    Asked {
        slug: "tools/x".into(),
        status: "review".into(),
        branch: "tools/x".into(),
        tip: Some(tip.into()),
    }
}

#[test]
fn an_answer_is_kept_by_the_tip_and_what_the_answerer_reads_besides() {
    let root = Path::new("/project");
    let fake = Fake::leaked(false);
    let kept = Kept::new(Duration::from_secs(30), Duration::from_secs(30));
    assert_eq!(
        kept.kept(root, "plugin", "fake", "tools/x", "tools/x", Some(OLD)),
        None
    );
    let first = kept.ask(root, "plugin", fake, &asked(OLD)).unwrap();
    assert_eq!(first[0].commit.as_deref(), Some(OLD));
    assert_eq!(first[0].detail, "tools/x tools/x");
    assert_eq!(kept.ask(root, "plugin", fake, &asked(OLD)).unwrap(), first);
    assert_eq!(fake.calls(), 1, "the same tip is asked once");
    assert_eq!(
        kept.kept(root, "plugin", "fake", "tools/x", "tools/x", Some(OLD)),
        Some(Ok(first.clone()))
    );
    let moved = kept.ask(root, "plugin", fake, &asked(TIP)).unwrap();
    assert_eq!(moved[0].commit.as_deref(), Some(TIP));
    assert_eq!(fake.calls(), 2, "a new tip is asked");
    assert_eq!(
        kept.kept(root, "plugin", "fake", "tools/x", "tools/x", Some(OLD)),
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
fn an_error_is_kept_until_it_is_asked_again_and_a_panic_is_an_error() {
    let root = Path::new("/project");
    let fake = Fake::leaked(false);
    *fake.error.lock().unwrap() = Some("land exited 2".into());
    let kept = Kept::new(Duration::from_secs(30), Duration::from_millis(200));
    assert_eq!(
        kept.ask(root, "plugin", fake, &asked(TIP)),
        Err("land exited 2".into())
    );
    assert_eq!(
        kept.ask(root, "plugin", fake, &asked(TIP)),
        Err("land exited 2".into())
    );
    assert_eq!(fake.calls(), 1, "an error is kept for a while");
    *fake.error.lock().unwrap() = None;
    std::thread::sleep(Duration::from_millis(250));
    assert!(kept.ask(root, "plugin", fake, &asked(TIP)).is_ok());
    assert_eq!(fake.calls(), 2, "then asked again");
    kept.ask(root, "plugin", fake, &asked(TIP)).unwrap();
    assert_eq!(fake.calls(), 2, "and an answer is kept for good");

    let panics = Fake::leaked(true);
    assert_eq!(
        kept.ask(root, "panicking", panics, &asked(TIP)),
        Err("fake panicked".into())
    );
}

#[test]
fn a_slow_answer_reads_unknown_while_it_goes_on_and_is_kept_once_it_comes() {
    let root = Path::new("/project");
    let fake = Fake::leaked(false);
    *fake.delay.lock().unwrap() = Duration::from_millis(600);
    let kept = Kept::new(Duration::from_millis(100), Duration::from_secs(30));
    let began = Instant::now();
    assert_eq!(
        kept.ask(root, "plugin", fake, &asked(TIP)),
        Err("fake gave no answer within 0.1 s".into())
    );
    assert!(
        began.elapsed() < Duration::from_millis(500),
        "the read waited only its wait"
    );
    // A read while the call goes on starts none, and waits no longer: the wait ran out.
    let again = Instant::now();
    assert!(kept.ask(root, "plugin", fake, &asked(TIP)).is_err());
    assert!(again.elapsed() < Duration::from_millis(50));
    assert_eq!(fake.calls(), 1);
    while kept
        .kept(root, "plugin", "fake", "tools/x", "tools/x", Some(TIP))
        .is_none()
    {
        assert!(
            began.elapsed() < Duration::from_secs(30),
            "the answer never came"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(kept.ask(root, "plugin", fake, &asked(TIP)).is_ok());
    assert_eq!(fake.calls(), 1, "the answer that came late is the one kept");
    // A read that waits for a call under way gets its answer once it comes.
    *fake.delay.lock().unwrap() = Duration::from_millis(100);
    let patient = Kept::new(Duration::from_secs(30), Duration::from_secs(30));
    std::thread::scope(|scope| {
        let first = scope.spawn(|| patient.ask(root, "plugin", fake, &asked(OLD)));
        let second = scope.spawn(|| patient.ask(root, "plugin", fake, &asked(OLD)));
        assert!(first.join().unwrap().is_ok());
        assert!(second.join().unwrap().is_ok());
    });
    assert_eq!(fake.calls(), 2, "two reads at once start one call");
}

/// `git` with `args` in `root`, its output trimmed.
fn git(root: &Path, args: &[&str]) -> String {
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
