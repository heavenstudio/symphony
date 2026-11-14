//! The release candidate's record, its rounds and plan, and its settle and branch on scratch
//! repositories with a board of the test's own.

use std::cell::RefCell;
use std::path::PathBuf;

use super::*;
use crate::release::checks::Ran;
use crate::release::record::{self, Board as Told};
use crate::test_tree::Tree;

/// The tests' now: 2026-11-12 10:00 UTC.
const NOW: f64 = 1_794_477_600.0;
const SLUG: &str = "tests/release-red-verify-tests";
const BOUNDS: Bounds = Bounds {
    reruns: 3,
    hours: 24,
};

/// The check or step `name` that exited `code`, listing `steps`.
fn ran(name: &str, code: i32, steps: Vec<Ran>) -> Ran {
    Ran {
        name: name.into(),
        command: "just verify".into(),
        exit_code: Some(code),
        signal: None,
        seconds: 1.0,
        log: PathBuf::from(format!("/logs/{name}.log")),
        steps,
        error: None,
        timed_out: false,
        interrupted: false,
        load: None,
    }
}

/// `verify` with its steps `lint` and `tests`, each green or not as said.
fn verify(lint: bool, tests: bool) -> Ran {
    let step = |name: &str, green: bool| ran(name, i32::from(!green), Vec::new());
    ran(
        "verify",
        i32::from(!(lint && tests)),
        vec![step("verify/lint", lint), step("verify/tests", tests)],
    )
}

/// The run `id` of `commit` that ran `checks`.
fn run(id: &str, commit: &str, checks: Vec<Ran>) -> Run {
    let green = checks.iter().all(Ran::green);
    Run {
        version: record::VERSION,
        id: id.into(),
        kind: "release".into(),
        commit: commit.into(),
        tip: commit.into(),
        started_at: NOW,
        ended_at: NOW + 60.0,
        result: if green { "green" } else { "red" }.into(),
        error: None,
        previous: None,
        checks,
        tag: None,
        pushed: false,
        board: Told::default(),
        remote: "origin".into(),
        unpushed: None,
        cause: None,
        refreshed: None,
        published: None,
        rerun: None,
    }
}

/// The red of `verify/tests` on its task [`SLUG`].
fn held() -> Vec<Held> {
    vec![Held {
        key: "verify-tests".into(),
        name: "verify/tests".into(),
        slug: SLUG.into(),
    }]
}

/// The candidate a run of `commit` red in `verify/tests` holds, its red on [`SLUG`].
fn candidate_at(commit: &str) -> Candidate {
    let mut candidate = Candidate::of(&run("r1", commit, vec![verify(true, false)]));
    candidate.named(&held());
    candidate
}

#[test]
fn a_record_reads_back_as_written_and_one_that_does_not_read_is_an_error() {
    let tree = Tree::new();
    let folder = tree.path("release");
    std::fs::create_dir_all(&folder).unwrap();
    assert_eq!(read(&folder), Ok(None), "no record: none held");
    let candidate = candidate_at(&"a".repeat(40));
    write(&folder, &candidate).unwrap();
    assert_eq!(read(&folder), Ok(Some(candidate.clone())));
    let text = tree.read("release/candidate.json");
    assert!(text.contains("\"version\": 1"), "{text}");
    assert!(text.ends_with("}\n"), "{text}");

    let newer = text.replace("\"version\": 1", "\"version\": 2");
    tree.write("release/candidate.json", &newer);
    let said = read(&folder).unwrap_err();
    assert!(
        said.contains("is of version 2, a newer build's than this one (1)"),
        "{said}"
    );
    tree.write("release/candidate.json", "{\"version\": 1, \"state\": ");
    let said = read(&folder).unwrap_err();
    assert!(said.contains("does not read"), "{said}");
}

#[test]
fn a_red_run_holds_its_commit_each_red_and_how_each_check_stood() {
    let commit = "c".repeat(40);
    let audit = ran("audit", 1, Vec::new());
    let refresh = ran("refresh-lamp-shop", 1, Vec::new());
    let first = run("r1", &commit, vec![verify(true, false), audit, refresh]);
    let mut candidate = Candidate::of(&first);
    assert!(candidate.open());
    assert_eq!(
        (candidate.commit.as_str(), candidate.first.as_str()),
        (commit.as_str(), commit.as_str())
    );
    assert_eq!((candidate.first_run.as_str(), candidate.since), ("r1", NOW));
    assert_eq!(candidate.rounds, 1);
    let reds: Vec<(&str, &str, Option<&str>)> = (candidate.reds.iter())
        .map(|red| (red.key.as_str(), red.name.as_str(), red.slug.as_deref()))
        .collect();
    assert_eq!(
        reds,
        vec![
            ("verify-tests", "verify/tests", None),
            ("audit", "audit", None),
        ],
        "a release's refresh is no red of the candidate"
    );
    let on = Some(On {
        commit: commit.clone(),
        run: "r1".into(),
    });
    assert_eq!(
        candidate.standing[0],
        Standing {
            name: "verify".into(),
            green: None,
            beyond: false,
            steps: vec![
                StepStanding {
                    name: "verify/lint".into(),
                    green: on.clone(),
                },
                StepStanding {
                    name: "verify/tests".into(),
                    green: None,
                },
            ],
        }
    );
    assert!(
        candidate
            .standing
            .iter()
            .all(|s| !s.name.starts_with("refresh"))
    );
    candidate.named(&held());
    assert_eq!(candidate.reds[0].slug.as_deref(), Some(SLUG));
    assert_eq!(
        candidate.reds[1].slug, None,
        "the board named no task for it"
    );
    let waiting: Vec<String> = candidate.waiting().iter().map(|red| red.said()).collect();
    assert_eq!(
        waiting,
        vec![
            SLUG.to_string(),
            "audit (no task: the board did not file it)".into()
        ]
    );
}

#[test]
fn each_red_rerun_is_a_round_and_the_last_one_the_bound_allows_drops_it() {
    let mut candidate = candidate_at(&"c".repeat(40));
    candidate.reds[0].answer = Some(Answer::Closed {
        status: "done".into(),
    });
    let rerun = run("r2", &"c".repeat(40), vec![verify(true, false)]);
    candidate.decided(&rerun);
    assert_eq!((candidate.rounds, candidate.red_reruns()), (2, 1));
    assert_eq!(
        candidate.reds.len(),
        2,
        "a red again after its answer is a new one"
    );
    assert_eq!(candidate.reds[1].round, 2);
    candidate.decided(&rerun);
    assert_eq!(
        candidate.reds.len(),
        2,
        "one unanswered of its key is that red"
    );
    assert!(!candidate.after_red(NOW, BOUNDS), "2 red reruns of 3");
    candidate.decided(&rerun);
    assert!(candidate.after_red(NOW + 5.0, BOUNDS));
    assert_eq!(candidate.state, State::Dropped);
    let ended = candidate.ended.clone().unwrap();
    assert_eq!(ended.at, NOW + 5.0);
    assert!(
        ended
            .why
            .starts_with("3 reruns came back red, as many as [releases] candidate_reruns allows"),
        "{ended:?}"
    );
    assert!(candidate.due());
    assert_eq!(
        candidate.hold(),
        Some(Hold::Dropped {
            why: ended.why.clone()
        })
    );
    candidate.followed();
    assert!(!candidate.due());
    assert_eq!(candidate.hold(), None, "the run its drop made due started");
}

#[test]
fn what_the_cadence_holds_follows_its_reds() {
    let tip = "c".repeat(40);
    let mut candidate = candidate_at(&tip);
    assert_eq!(
        candidate.hold(),
        Some(Hold::Waiting {
            tip: tip.clone(),
            reds: vec![SLUG.into()]
        })
    );
    candidate.reds[0].answer = Some(Answer::Closed {
        status: "cancelled".into(),
    });
    assert_eq!(candidate.hold(), Some(Hold::Answered { tip: tip.clone() }));
    candidate.release(NOW, "release-2026-11-12");
    assert_eq!(candidate.state, State::Released);
    assert!(!candidate.due());
    assert_eq!(candidate.hold(), None);
}

#[test]
fn a_rerun_carries_only_what_was_green_on_the_same_tree() {
    let (a, b) = ("a".repeat(40), "b".repeat(40));
    let same_as_a = |commit: &str| commit == a;
    let none_same = |_: &str| false;
    let mut candidate = Candidate::of(&run("r1", &a, vec![verify(true, false)]));
    candidate.decided(&run("r0", &a, vec![ran("docs", 0, Vec::new())]));

    assert_eq!(
        candidate.plan("lint-only", &same_as_a),
        (Planned::Whole, Vec::new()),
        "a check no run of it ran"
    );
    let carried = |name: &str, run: &str| Carried {
        name: name.into(),
        commit: a.clone(),
        run: run.into(),
    };
    assert_eq!(
        candidate.plan("docs", &same_as_a),
        (Planned::Carried, vec![carried("docs", "r0")]),
        "green whole on this tree"
    );
    assert_eq!(
        candidate.plan("docs", &none_same),
        (Planned::Whole, Vec::new()),
        "green on another tree: its inputs changed"
    );
    assert_eq!(
        candidate.plan("verify", &same_as_a),
        (
            Planned::Steps(vec!["tests".into()]),
            vec![carried("verify/lint", "r1")]
        ),
        "the red step reruns, the green one is carried"
    );
    assert_eq!(
        candidate.plan("verify", &none_same),
        (Planned::Whole, Vec::new()),
        "no step kept: the check runs whole"
    );

    // Red beyond its steps (it stopped at the run's time): whole.
    let mut stopped = verify(true, false);
    stopped.timed_out = true;
    let beyond = Candidate::of(&run("r1", &b, vec![stopped]));
    assert_eq!(
        beyond.plan("verify", &|commit| commit == b),
        (Planned::Whole, Vec::new())
    );
    // A red check that lists no steps: whole.
    let plain = Candidate::of(&run("r1", &b, vec![ran("verify", 1, Vec::new())]));
    assert_eq!(
        plain.plan("verify", &|commit| commit == b),
        (Planned::Whole, Vec::new())
    );
}

#[test]
fn status_and_notes_say_where_it_stands_and_how_it_was_answered() {
    let tip = "c".repeat(40);
    let mut candidate = candidate_at(&tip);
    candidate.decided(&run("r1", &tip, vec![ran("docs", 1, Vec::new())]));
    candidate.reds[0].seen = Some("in-progress".into());
    let said = candidate.status(NOW + 7200.0, BOUNDS);
    assert!(
        said.starts_with(
            "release candidate: cccccccccccc, first judged cccccccccccc by the run r1"
        ),
        "{said}"
    );
    assert!(said.contains("2 h ago"), "{said}");
    assert!(
        said.contains("; 2 runs of it, 1 red rerun of at most 3;"),
        "{said}"
    );
    assert!(
        said.contains(&format!(
            "  {SLUG} (verify/tests, round 1): open (in-progress)\n"
        )),
        "{said}"
    );
    assert!(
        said.contains("  docs (no task: the board did not file it) (docs, round 2): open (not seen on the board yet)\n"),
        "{said}"
    );
    assert!(said.contains("  rerun: waits for "), "{said}");

    let (fix, merge) = ("f".repeat(40), "e".repeat(40));
    candidate.reds[0].answer = Some(Answer::Landed {
        merge: merge.clone(),
        commit: fix.clone(),
    });
    candidate.reds[1].answer = Some(Answer::Closed {
        status: "cancelled".into(),
    });
    candidate.settled_at = Some(NOW + 60.0);
    let said = candidate.status(NOW + 7200.0, BOUNDS);
    assert!(
        said.contains(&format!(
            "  {SLUG} (verify/tests, round 1): answered: its land eeeeeeeeeeee advanced the \
             candidate to ffffffffffff\n"
        )),
        "{said}"
    );
    assert!(
        said.contains(
            "  docs (no task: the board did not file it) (docs, round 2): answered: its task \
             cancelled with no landing, its fix made outside this repository\n"
        ),
        "{said}"
    );
    assert!(
        said.contains("  rerun: due, every red answered: the next landing starts it, or `just release --rerun`\n"),
        "{said}"
    );
    assert!(said.contains("  as last settled, "), "{said}");

    let notes = candidate.notes(&[Carried {
        name: "verify/lint".into(),
        commit: tip.clone(),
        run: "r1".into(),
    }]);
    assert!(
        notes[0].starts_with("Release candidate: first judged cccccccccccc by the run r1 ("),
        "{notes:?}"
    );
    assert!(
        notes[0].ends_with("), released after 2 reruns."),
        "{notes:?}"
    );
    assert_eq!(
        notes[1..],
        [
            "Its reds and how each was answered:".to_string(),
            format!("- {SLUG} (verify/tests): advanced it to ffffffffffff"),
            "- no task (docs): answered outside this repository".into(),
            "Carried green from a commit whose tree is this one's:".into(),
            "- verify/lint: green on cccccccccccc in the run r1".into(),
        ]
    );

    candidate.release(NOW + 7200.0, "release-2026-11-12");
    let said = candidate.status(NOW + 7260.0, BOUNDS);
    assert!(
        said.starts_with("last release candidate: released, "),
        "{said}"
    );
    assert!(
        said.contains(": released as release-2026-11-12; first judged"),
        "{said}"
    );
    // Its two red runs, the green rerun that released it not among them.
    assert!(said.ends_with(", 2 red runs of it\n"), "{said}");
    candidate.state = State::Dropped;
    candidate.ended = Some(Ended {
        at: NOW,
        why: "dropped by `just release --drop`".into(),
        tag: None,
        due: true,
    });
    let said = candidate.status(NOW, BOUNDS);
    assert!(
        said.contains(": dropped by `just release --drop`; a run of main's tip is due;"),
        "{said}"
    );
}

#[test]
fn only_a_verdict_that_approves_is_an_approve() {
    let messages: Vec<serde_json::Value> = serde_json::from_str(
        r#"[
            {"kind": "verdict", "text": "approve: abc1234 reads well"},
            {"kind": "verdict", "text": "approve"},
            {"kind": "verdict", "text": "approved abc1234"},
            {"kind": "verdict", "text": "request-changes: abc1234"},
            {"kind": "message", "text": "approve: def5678"},
            {"kind": "verdict"}
        ]"#,
    )
    .unwrap();
    assert_eq!(
        approving(&messages),
        vec!["approve: abc1234 reads well".to_string(), "approve".into()]
    );
}

#[test]
fn only_a_threads_last_verdict_closing_it_as_covered_names_the_covering_landing() {
    let line = symphony_board::covered_line("tests/x", &"c".repeat(40), "otter", "its fix");
    let thread = |messages: &[(&str, &str)]| -> Vec<serde_json::Value> {
        (messages.iter())
            .map(|(kind, text)| serde_json::json!({"kind": kind, "text": text}))
            .collect()
    };
    let covered = Some(Covered {
        task: "tests/x".into(),
        commit: "c".repeat(12),
    });
    assert_eq!(
        covering(&thread(&[("verdict", "approve: abc"), ("verdict", &line)])),
        covered
    );
    assert_eq!(
        covering(&thread(&[("verdict", &line), ("message", "a word after")])),
        covered,
        "a message is no verdict"
    );
    assert_eq!(
        covering(&thread(&[("verdict", &line), ("verdict", "approve: def")])),
        None,
        "a verdict after the close stands over it"
    );
    assert_eq!(covering(&thread(&[("message", &line)])), None);
    assert_eq!(
        covering(&thread(&[("verdict", "covered by tests/x")])),
        None
    );
    assert_eq!(covering(&[]), None);
}

/// A scratch repository on `main`.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(tree: &Tree) -> Scratch {
        let root = tree.path("repo");
        std::fs::create_dir_all(&root).unwrap();
        let scratch = Scratch { root };
        scratch.git(&["init", "-q", "-b", "main"]);
        scratch.commit("a.txt", "first\n", "first");
        scratch
    }

    fn git(&self, args: &[&str]) -> String {
        let named = ["-c", "user.name=t", "-c", "user.email=t@example.com"];
        git(&self.root, &[&named[..], args].concat()).unwrap()
    }

    /// `file` written with `text` and committed as `message`: the commit.
    fn commit(&self, file: &str, text: &str, message: &str) -> String {
        std::fs::write(self.root.join(file), text).unwrap();
        self.git(&["add", file]);
        self.git(&["commit", "-q", "-m", message]);
        self.head()
    }

    fn head(&self) -> String {
        self.git(&["rev-parse", "HEAD"])
    }

    fn checkout(&self, args: &[&str]) {
        self.git(&[&["checkout", "-q"][..], args].concat());
    }

    /// `branch` merged into the branch checked out with `message`, never fast-forwarded.
    fn merge(&self, branch: &str, message: &str) -> String {
        self.git(&["merge", "-q", "--no-ff", "-m", message, branch]);
        self.head()
    }

    /// `branch` landed on `main` as `just land` merges it.
    fn land(&self, branch: &str) -> String {
        self.checkout(&["main"]);
        self.merge(branch, &format!("Merge branch '{branch}'"))
    }
}

/// A board of the test's own: each task's status, the approves on each thread, the landing each
/// task was closed as covered by, and the open red of each key; and what the settle asked.
#[derive(Default)]
struct Fake {
    statuses: Vec<(String, String)>,
    approves: Vec<(String, Vec<String>)>,
    covered: Vec<(String, Covered)>,
    open: Vec<(String, String)>,
    asked: RefCell<Vec<String>>,
}

impl Board for Fake {
    fn status(&self, slug: &str) -> Result<Option<String>, String> {
        self.asked.borrow_mut().push(format!("status {slug}"));
        Ok((self.statuses.iter())
            .find(|(known, _)| known == slug)
            .map(|(_, status)| status.clone()))
    }

    fn approves(&self, slug: &str) -> Result<Vec<String>, String> {
        self.asked.borrow_mut().push(format!("approves {slug}"));
        Ok((self.approves.iter())
            .find(|(known, _)| known == slug)
            .map(|(_, texts)| texts.clone())
            .unwrap_or_default())
    }

    fn covered(&self, slug: &str) -> Result<Option<Covered>, String> {
        self.asked.borrow_mut().push(format!("covered {slug}"));
        Ok((self.covered.iter())
            .find(|(known, _)| known == slug)
            .map(|(_, covered)| covered.clone()))
    }

    fn open_red(&self, key: &str) -> Result<Option<String>, String> {
        self.asked.borrow_mut().push(format!("open {key}"));
        Ok((self.open.iter())
            .find(|(known, _)| known == key)
            .map(|(_, slug)| slug.clone()))
    }
}

/// The line of `scratch` with the remote's main at `remote` and the main checkout's at `local`.
fn line<'a>(scratch: &'a Scratch, remote: &'a str, local: Option<&'a str>) -> Line<'a> {
    Line {
        root: &scratch.root,
        branch: "main",
        remote_name: "origin/main",
        remote,
        local,
        newest: None,
    }
}

/// A repository whose `main` holds the candidate, a red's fix branch [`SLUG`] started at it with
/// two commits, and main moved on with another task's landing: the candidate, the fix's commits
/// and main's tip.
fn fixing(scratch: &Scratch) -> (String, [String; 2], String) {
    let candidate = scratch.commit("a.txt", "candidate\n", "the candidate");
    scratch.checkout(&["-b", SLUG]);
    let one = scratch.commit("fix.txt", "one\n", "fix one");
    let two = scratch.commit("fix.txt", "two\n", "fix two");
    scratch.checkout(&["-b", "tools/other", "main"]);
    scratch.commit("other.txt", "other\n", "other work");
    let main = scratch.land("tools/other");
    (candidate, [one, two], main)
}

#[test]
fn the_reds_own_land_advances_the_candidate_to_its_newest_approved_commit() {
    let tree = Tree::new();
    let scratch = Scratch::new(&tree);
    let (tip, [one, two], _) = fixing(&scratch);
    let merge = scratch.land(SLUG);
    let mut candidate = candidate_at(&tip);
    let board = Fake {
        approves: vec![(
            SLUG.into(),
            vec![
                format!("approve: {one} first"),
                format!("approve: {two}: ok"),
            ],
        )],
        ..Fake::default()
    };
    let said = settle(
        &mut candidate,
        &line(&scratch, &merge, Some(&merge)),
        &board,
        NOW + 60.0,
        BOUNDS,
    );
    assert_eq!(candidate.commit, two, "{said:?}");
    assert!(candidate.open());
    assert_eq!(
        candidate.reds[0].answer,
        Some(Answer::Landed {
            merge: merge.clone(),
            commit: two.clone()
        })
    );
    assert_eq!(
        candidate.advances,
        vec![Advance {
            slug: SLUG.into(),
            from: tip.clone(),
            to: two.clone(),
            merge: merge.clone(),
        }]
    );
    assert_eq!(
        said,
        vec![format!(
            "advanced the release candidate from {} to {}, the newest approved commit of {SLUG}'s \
             land ({})",
            short(&tip),
            short(&two),
            short(&merge)
        )]
    );
    assert_eq!(candidate.settled_at, Some(NOW + 60.0));
    assert_eq!(candidate.hold(), Some(Hold::Answered { tip: two.clone() }));
    // Settled again, nothing moves: its red is answered.
    let again = settle(
        &mut candidate,
        &line(&scratch, &merge, Some(&merge)),
        &board,
        NOW + 120.0,
        BOUNDS,
    );
    assert!(again.is_empty(), "{again:?}");
    assert_eq!(candidate.commit, two);
}

#[test]
fn a_stray_task_forked_at_the_candidate_moves_nothing() {
    let tree = Tree::new();
    let scratch = Scratch::new(&tree);
    let (tip, _, _) = fixing(&scratch);
    scratch.checkout(&["-b", "tools/stray", &tip]);
    let stray = scratch.commit("stray.txt", "stray\n", "stray work");
    let merge = scratch.land("tools/stray");
    let mut candidate = candidate_at(&tip);
    let board = Fake {
        statuses: vec![(SLUG.into(), "in-progress".into())],
        approves: vec![("tools/stray".into(), vec![format!("approve: {stray}")])],
        ..Fake::default()
    };
    let said = settle(
        &mut candidate,
        &line(&scratch, &merge, Some(&merge)),
        &board,
        NOW + 60.0,
        BOUNDS,
    );
    assert!(said.is_empty(), "{said:?}");
    assert_eq!(candidate.commit, tip);
    assert!(candidate.advances.is_empty());
    assert_eq!(candidate.reds[0].answer, None);
    assert_eq!(candidate.reds[0].seen.as_deref(), Some("in-progress"));
    assert!(
        !board
            .asked
            .borrow()
            .iter()
            .any(|asked| asked.contains("tools/stray")),
        "a task that is no red's is never read: {:?}",
        board.asked.borrow()
    );
}

#[test]
fn a_land_that_merged_main_after_its_approve_advances_to_the_commit_approved_before() {
    let tree = Tree::new();
    let scratch = Scratch::new(&tree);
    let (tip, [one, _], _) = fixing(&scratch);
    // The land conflicted: its executor merged main, fixed on, and was approved again.
    scratch.checkout(&[SLUG]);
    let merged = scratch.merge("main", &format!("Merge branch 'main' into {SLUG}"));
    let after = scratch.commit("fix.txt", "three\n", "fix three, after main");
    let merge = scratch.land(SLUG);
    let mut candidate = candidate_at(&tip);
    let board = Fake {
        approves: vec![(
            SLUG.into(),
            vec![
                format!("approve: {one}"),
                format!("approve: {merged}"),
                format!("approve: {after} again"),
            ],
        )],
        ..Fake::default()
    };
    let said = settle(
        &mut candidate,
        &line(&scratch, &merge, Some(&merge)),
        &board,
        NOW + 60.0,
        BOUNDS,
    );
    assert_eq!(candidate.commit, one, "{said:?}");
    assert!(candidate.open());
    let root = &scratch.root;
    let before = commit(root, &format!("{merge}^1")).unwrap();
    let landed = commit(root, &format!("{merge}^2")).unwrap();
    assert!(passes(root, &tip, &before, &landed, &one).unwrap());
    for held_main in [&merged, &after] {
        assert!(
            !passes(root, &tip, &before, &landed, held_main).unwrap(),
            "holds what main held before the land"
        );
    }
    assert!(
        !passes(root, &tip, &before, &landed, &tip).unwrap(),
        "the candidate itself is no advance"
    );
    let main_before = commit(root, &format!("{before}^1")).unwrap();
    assert!(
        !passes(root, &tip, &before, &landed, &main_before).unwrap(),
        "not the branch's"
    );
}

#[test]
fn a_land_with_no_approved_commit_that_passes_drops_the_candidate() {
    let tree = Tree::new();
    let scratch = Scratch::new(&tree);
    let (tip, _, _) = fixing(&scratch);
    scratch.checkout(&[SLUG]);
    let merged = scratch.merge("main", &format!("Merge branch 'main' into {SLUG}"));
    let merge = scratch.land(SLUG);
    let mut candidate = candidate_at(&tip);
    let board = Fake {
        approves: vec![(SLUG.into(), vec![format!("approve: {merged}")])],
        ..Fake::default()
    };
    let said = settle(
        &mut candidate,
        &line(&scratch, &merge, Some(&merge)),
        &board,
        NOW + 60.0,
        BOUNDS,
    );
    assert_eq!(candidate.state, State::Dropped, "{said:?}");
    assert_eq!(candidate.commit, tip);
    let why = candidate.ended.clone().unwrap().why;
    assert_eq!(
        why,
        format!(
            "{SLUG} landed ({}) with no approved commit that descends from the candidate ({}) \
             and holds beyond it nothing main held before the land",
            short(&merge),
            short(&tip)
        )
    );
    assert_eq!(said, vec![format!("dropped the release candidate: {why}")]);
    assert!(candidate.due(), "a drop makes a run of main's tip due");
}

#[test]
fn a_land_the_remote_lacks_yet_waits_for_its_push() {
    let tree = Tree::new();
    let scratch = Scratch::new(&tree);
    let (tip, [_, two], main) = fixing(&scratch);
    let merge = scratch.land(SLUG);
    let mut candidate = candidate_at(&tip);
    let board = Fake {
        statuses: vec![(SLUG.into(), "done".into())],
        approves: vec![(SLUG.into(), vec![format!("approve: {two}")])],
        ..Fake::default()
    };
    let said = settle(
        &mut candidate,
        &line(&scratch, &main, Some(&merge)),
        &board,
        NOW + 60.0,
        BOUNDS,
    );
    assert_eq!(
        said,
        vec![format!(
            "{SLUG} landed here ({}), but origin/main does not hold it yet: the release \
             candidate waits for its push",
            short(&merge)
        )]
    );
    assert_eq!(candidate.commit, tip);
    assert_eq!(
        candidate.reds[0].answer, None,
        "done, but not closed: it landed"
    );
    assert_eq!(
        candidate.reds[0].seen.as_deref(),
        Some("landed here, not pushed")
    );
    // Pushed, it advances.
    settle(
        &mut candidate,
        &line(&scratch, &merge, Some(&merge)),
        &board,
        NOW + 120.0,
        BOUNDS,
    );
    assert_eq!(candidate.commit, two);
}

#[test]
fn a_red_whose_task_is_cancelled_with_no_landing_is_answered_and_one_done_waits_for_its_land() {
    let tree = Tree::new();
    let scratch = Scratch::new(&tree);
    let tip = scratch.commit("a.txt", "candidate\n", "the candidate");
    let size = "tests/release-red-size";
    let mut candidate = Candidate::of(&run(
        "r1",
        &tip,
        vec![
            verify(true, false),
            ran("size", 1, Vec::new()),
            ran("docs", 1, Vec::new()),
        ],
    ));
    let board = Fake {
        statuses: vec![
            (SLUG.into(), "cancelled".into()),
            (size.into(), "done".into()),
            ("tests/release-red-docs".into(), "review".into()),
        ],
        open: vec![
            ("verify-tests".into(), SLUG.into()),
            ("size".into(), size.into()),
            ("docs".into(), "tests/release-red-docs".into()),
        ],
        ..Fake::default()
    };
    let said = settle(
        &mut candidate,
        &line(&scratch, &tip, Some(&tip)),
        &board,
        NOW + 60.0,
        BOUNDS,
    );
    assert_eq!(
        candidate.reds[0].slug.as_deref(),
        Some(SLUG),
        "found by its key"
    );
    assert_eq!(
        candidate.reds[0].answer,
        Some(Answer::Closed {
            status: "cancelled".into()
        })
    );
    assert_eq!(
        said,
        vec![format!(
            "{SLUG} is cancelled with no landing: its red is answered, its fix made outside this \
             repository"
        )]
    );
    assert_eq!(candidate.reds[1].answer, None, "approved, not landed");
    assert_eq!(
        candidate.reds[1].seen.as_deref(),
        Some("approved, waiting for its land")
    );
    assert_eq!(candidate.reds[2].answer, None);
    assert_eq!(candidate.reds[2].seen.as_deref(), Some("review"));
    assert_eq!(candidate.commit, tip, "a fix made elsewhere moves nothing");
}

/// The task of the red of `verify/lint`, beside [`SLUG`]'s.
const LINT: &str = "tests/release-red-verify-lint";

/// The candidate a run of `commit` red in both of verify's steps holds, each red on its task.
fn two_reds_at(commit: &str) -> Candidate {
    let mut candidate = Candidate::of(&run("r1", commit, vec![verify(false, false)]));
    candidate.named(&[
        Held {
            key: "verify-lint".into(),
            name: "verify/lint".into(),
            slug: LINT.into(),
        },
        Held {
            key: "verify-tests".into(),
            name: "verify/tests".into(),
            slug: SLUG.into(),
        },
    ]);
    candidate
}

/// The red of `candidate` on the task `slug`.
fn red_of<'a>(candidate: &'a Candidate, slug: &str) -> &'a Red {
    (candidate.reds.iter())
        .find(|red| red.slug.as_deref() == Some(slug))
        .unwrap()
}

#[test]
fn a_red_closed_as_covered_by_another_reds_land_waits_for_it_and_is_answered_by_its_advance() {
    let tree = Tree::new();
    let scratch = Scratch::new(&tree);
    let (tip, [_, two], main) = fixing(&scratch);
    let merge = scratch.land(SLUG);
    let mut candidate = two_reds_at(&tip);
    let board = Fake {
        statuses: vec![(LINT.into(), "done".into())],
        approves: vec![(SLUG.into(), vec![format!("approve: {two}")])],
        covered: vec![(
            LINT.into(),
            Covered {
                task: SLUG.into(),
                commit: merge[..12].into(),
            },
        )],
        ..Fake::default()
    };
    // Landed here, not pushed: the covering land has not advanced it, so the covered red waits.
    let said = settle(
        &mut candidate,
        &line(&scratch, &main, Some(&merge)),
        &board,
        NOW + 60.0,
        BOUNDS,
    );
    assert!(candidate.open(), "{said:?}");
    assert_eq!(candidate.commit, tip);
    let lint = red_of(&candidate, LINT);
    assert_eq!(lint.answer, None);
    assert_eq!(
        lint.seen.as_deref(),
        Some(
            format!("closed as covered by {SLUG}, whose land has not advanced the candidate yet")
                .as_str()
        )
    );
    // Pushed: the land advances it, and its fix answers the covered red too.
    let said = settle(
        &mut candidate,
        &line(&scratch, &merge, Some(&merge)),
        &board,
        NOW + 120.0,
        BOUNDS,
    );
    assert_eq!(candidate.commit, two, "{said:?}");
    assert_eq!(
        red_of(&candidate, LINT).answer,
        Some(Answer::Covered {
            task: SLUG.into(),
            merge: merge.clone(),
            commit: two.clone(),
        })
    );
    assert_eq!(
        said[1],
        format!(
            "{LINT} is closed as covered by {SLUG}'s land ({}), which advanced the release \
             candidate to {}: its red is answered",
            short(&merge),
            short(&two)
        ),
        "{said:?}"
    );
    assert_eq!(candidate.hold(), Some(Hold::Answered { tip: two.clone() }));
    let status = candidate.status(NOW + 120.0, BOUNDS);
    assert!(
        status.contains(&format!(
            "{LINT} (verify/lint, round 1): answered: its task closed as covered by {SLUG}'s land \
             {}, which advanced the candidate to {}",
            short(&merge),
            short(&two)
        )),
        "{status}"
    );
    let notes = candidate.notes(&[]).join("\n");
    assert!(
        notes.contains(&format!(
            "- {LINT} (verify/lint): covered by {SLUG}'s land, which advanced it to {}",
            short(&two)
        )),
        "{notes}"
    );
}

#[test]
fn a_red_closed_as_covered_by_a_landing_that_did_not_advance_it_drops_the_candidate() {
    let tree = Tree::new();
    let scratch = Scratch::new(&tree);
    // tools/other landed on main after the candidate; the lint red is closed as covered by it.
    let (tip, _, main) = fixing(&scratch);
    let mut candidate = two_reds_at(&tip);
    let board = Fake {
        statuses: vec![(LINT.into(), "done".into()), (SLUG.into(), "review".into())],
        covered: vec![(
            LINT.into(),
            Covered {
                task: "tools/other".into(),
                commit: main[..12].into(),
            },
        )],
        ..Fake::default()
    };
    let said = settle(
        &mut candidate,
        &line(&scratch, &main, Some(&main)),
        &board,
        NOW + 60.0,
        BOUNDS,
    );
    assert_eq!(candidate.state, State::Dropped, "{said:?}");
    let why = candidate.ended.clone().unwrap().why;
    assert_eq!(
        why,
        format!(
            "{LINT} was closed as covered by tools/other's land ({}), which did not advance the \
             candidate: its fix is on main, not in the candidate",
            &main[..12]
        )
    );
    assert_eq!(said, vec![format!("dropped the release candidate: {why}")]);
    assert!(candidate.due(), "a drop makes a run of main's tip due");
    assert_eq!(candidate.commit, tip);
}

#[test]
fn its_bounds_drop_it_or_find_it_released() {
    let tree = Tree::new();
    let scratch = Scratch::new(&tree);
    let first = scratch.head();
    let tip = scratch.commit("a.txt", "candidate\n", "the candidate");
    let later = scratch.commit("a.txt", "later\n", "main moves on");
    let settled = |line: Line<'_>, now: f64| {
        let mut candidate = candidate_at(&tip);
        let said = settle(&mut candidate, &line, &Fake::default(), now, BOUNDS);
        (candidate, said)
    };

    let (candidate, said) = settled(line(&scratch, &later, None), NOW + 23.0 * 3600.0);
    assert!(candidate.open(), "{said:?}");
    let (candidate, said) = settled(line(&scratch, &later, None), NOW + 24.0 * 3600.0);
    assert_eq!(candidate.state, State::Dropped);
    assert!(
        said[0].starts_with("dropped the release candidate: it was first judged 24 hours ago, at "),
        "{said:?}"
    );
    assert!(
        said[0].ends_with(": [releases] candidate_hours holds one 24"),
        "{said:?}"
    );

    /// The line of `scratch` at `remote`, its newest release `name` at `at`.
    fn tagged<'a>(scratch: &'a Scratch, remote: &'a str, name: &'a str, at: &'a str) -> Line<'a> {
        Line {
            newest: Some((name, at)),
            ..line(scratch, remote, None)
        }
    }
    let (candidate, said) = settled(tagged(&scratch, &later, "release-2026-11-12", &tip), NOW);
    assert_eq!(candidate.state, State::Released, "{said:?}");
    assert_eq!(
        candidate.ended.unwrap().tag.as_deref(),
        Some("release-2026-11-12")
    );
    let (candidate, said) = settled(tagged(&scratch, &later, "release-2026-11-12", &later), NOW);
    assert_eq!(candidate.state, State::Dropped);
    assert_eq!(
        said,
        vec![format!(
            "dropped the release candidate: the release release-2026-11-12 ({}) passed it: its \
             tip {} does not descend from it, and a release only moves forward",
            short(&later),
            short(&tip)
        )]
    );
    let (candidate, said) = settled(tagged(&scratch, &later, "release-2026-11-11", &first), NOW);
    assert!(candidate.open(), "an older release on its line: {said:?}");

    let (candidate, said) = settled(line(&scratch, &first, None), NOW);
    assert_eq!(candidate.state, State::Dropped);
    assert_eq!(
        said,
        vec![format!(
            "dropped the release candidate: its tip {} is not on origin/main",
            short(&tip)
        )]
    );

    let mut ended = candidate_at(&tip);
    ended.release(NOW, "release-2026-11-12");
    let said = settle(
        &mut ended,
        &line(&scratch, &first, None),
        &Fake::default(),
        NOW,
        BOUNDS,
    );
    assert!(said.is_empty(), "an ended candidate is never settled again");
    assert_eq!(ended.state, State::Released);
}

#[test]
fn its_branch_follows_its_tip_and_is_never_taken_from_another() {
    let tree = Tree::new();
    let scratch = Scratch::new(&tree);
    let root = &scratch.root;
    let first = scratch.head();
    let tip = scratch.commit("a.txt", "candidate\n", "the candidate");
    let fix = scratch.commit("a.txt", "fixed\n", "the fix");
    let at = || commit(root, REFERENCE).ok();

    let mut candidate = candidate_at(&tip);
    assert_eq!(steer(root, &mut candidate), None);
    assert_eq!(at(), Some(tip.clone()));
    assert_eq!(candidate.branch, Some(tip.clone()));
    candidate.commit.clone_from(&fix);
    assert_eq!(steer(root, &mut candidate), None);
    assert_eq!(at(), Some(fix.clone()));
    assert_eq!(steer(root, &mut candidate), None, "already there");

    // Someone moved it: left as it is.
    scratch.git(&["update-ref", REFERENCE, &first]);
    let said = steer(root, &mut candidate).unwrap();
    assert_eq!(
        said,
        format!(
            "the branch {BRANCH} is at {}, which the release run did not set it to: it is left \
             as it is, and the candidate's tip is {}",
            short(&first),
            short(&fix)
        )
    );
    assert_eq!(at(), Some(first.clone()));
    scratch.git(&["update-ref", REFERENCE, &fix]);

    // Checked out in a worktree: left where it is.
    let worktree = tree.path("wt");
    scratch.git(&["worktree", "add", "-q", &worktree.to_string_lossy(), BRANCH]);
    candidate.commit.clone_from(&tip);
    let said = steer(root, &mut candidate).unwrap();
    assert!(said.contains("is checked out in a worktree"), "{said}");
    assert_eq!(at(), Some(fix.clone()));
    scratch.git(&["worktree", "remove", "--force", &worktree.to_string_lossy()]);

    // Ended: gone.
    candidate.commit.clone_from(&fix);
    candidate.release(NOW, "release-2026-11-12");
    assert_eq!(steer(root, &mut candidate), None);
    assert_eq!(at(), None);
    assert_eq!(candidate.branch, None);
    assert_eq!(steer(root, &mut candidate), None, "none, and none wanted");
}

#[test]
fn two_commits_are_the_same_tree_only_when_they_hold_the_same_files() {
    let tree = Tree::new();
    let scratch = Scratch::new(&tree);
    let first = scratch.head();
    scratch.git(&["commit", "-q", "--allow-empty", "-m", "nothing changed"]);
    let empty = scratch.head();
    let changed = scratch.commit("a.txt", "changed\n", "a change");
    assert!(same_tree(&scratch.root, &first, &empty));
    assert!(!same_tree(&scratch.root, &first, &changed));
    assert!(!same_tree(&scratch.root, &first, &"0".repeat(40)));
}

#[test]
fn its_bounds_are_the_projects() {
    let mut releases = crate::project::Releases::default();
    assert_eq!(
        Bounds::of(&releases),
        Bounds {
            reruns: 3,
            hours: 24
        }
    );
    assert!(releases.holds_candidates());
    releases.candidate_hours = 1;
    assert_eq!(Bounds::of(&releases).seconds(), 3600.0);
    releases.candidate_reruns = 0;
    assert!(!releases.holds_candidates());
}
