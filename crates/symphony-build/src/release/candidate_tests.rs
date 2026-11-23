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
        alone: None,
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
        checks,
        board: Told::default(),
        remote: "origin".into(),
        ..Run::default()
    }
}

/// A tip that tells no inputs and names no environment, judged by its tree alone, as `same` says.
fn by_tree(same: &dyn Fn(&str) -> bool) -> Tip<'_> {
    Tip {
        same,
        inputs: None,
        environment: None,
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
        digest: None,
        environment: None,
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
                    beyond: false,
                    parts: Vec::new(),
                },
                StepStanding {
                    name: "verify/tests".into(),
                    green: None,
                    beyond: false,
                    parts: Vec::new(),
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
        candidate.plan("lint-only", &by_tree(&same_as_a)),
        (Planned::Whole, Vec::new()),
        "a check no run of it ran"
    );
    let carried = |name: &str, run: &str| Carried {
        name: name.into(),
        commit: a.clone(),
        run: run.into(),
        digest: None,
    };
    assert_eq!(
        candidate.plan("docs", &by_tree(&same_as_a)),
        (Planned::Carried, vec![carried("docs", "r0")]),
        "green whole on this tree"
    );
    assert_eq!(
        candidate.plan("docs", &by_tree(&none_same)),
        (Planned::Whole, Vec::new()),
        "green on another tree: its inputs changed"
    );
    assert_eq!(
        candidate.plan("verify", &by_tree(&same_as_a)),
        (
            Planned::Steps(vec!["tests".into()]),
            vec![carried("verify/lint", "r1")]
        ),
        "the red step reruns, the green one is carried"
    );
    assert_eq!(
        candidate.plan("verify", &by_tree(&none_same)),
        (Planned::Whole, Vec::new()),
        "no step kept: the check runs whole"
    );

    // Red beyond its steps (it stopped at the run's time): whole.
    let mut stopped = verify(true, false);
    stopped.timed_out = true;
    let beyond = Candidate::of(&run("r1", &b, vec![stopped]));
    assert_eq!(
        beyond.plan("verify", &by_tree(&|commit| commit == b)),
        (Planned::Whole, Vec::new())
    );
    // A red check that lists no steps: whole.
    let plain = Candidate::of(&run("r1", &b, vec![ran("verify", 1, Vec::new())]));
    assert_eq!(
        plain.plan("verify", &by_tree(&|commit| commit == b)),
        (Planned::Whole, Vec::new())
    );
}

/// What a tree tells of its inputs: each name with its digest.
fn told(inputs: &[(&str, &str)]) -> Inputs {
    Inputs {
        told: (inputs.iter())
            .map(|(name, digest)| crate::release::inputs::Input {
                name: (*name).into(),
                digest: (*digest).into(),
            })
            .collect(),
        error: None,
    }
}

/// The environment the tests' runs ran on.
fn environment() -> crate::release::inputs::Environment {
    crate::release::inputs::Environment {
        system: "Stand-in 1".into(),
        chrome: "Chrome 2".into(),
        postgresql: "postgres 18".into(),
        postgresql_16: "postgres 16".into(),
    }
}

/// `verify` with its steps `lint` and `tests`, the tests listing the lanes `browser` and `perf`,
/// each green or not as said.
fn laned(lint: bool, browser: bool, perf: bool) -> Ran {
    let part = |name: &str, green: bool| ran(name, i32::from(!green), Vec::new());
    let tests = ran(
        "verify/tests",
        i32::from(!(browser && perf)),
        vec![
            part("verify/tests/browser", browser),
            part("verify/tests/perf", perf),
        ],
    );
    ran(
        "verify",
        i32::from(!(lint && browser && perf)),
        vec![part("verify/lint", lint), tests],
    )
}

/// The run `id` of `commit` that ran `checks`, told `inputs`, on the tests' environment.
fn run_told(id: &str, commit: &str, checks: Vec<Ran>, inputs: &[(&str, &str)]) -> Run {
    let mut run = run(id, commit, checks);
    run.inputs = Some(told(inputs));
    run.environment = Some(environment());
    run
}

const ALL: [(&str, &str); 3] = [
    ("verify/lint", "l1"),
    ("verify/tests/browser", "b1"),
    ("verify/tests/perf", "p1"),
];

#[test]
fn a_rerun_carries_a_step_or_lane_green_on_the_digest_the_tip_tells() {
    let a = "a".repeat(40);
    let other_tree = |_: &str| false;
    let said = environment().said();
    let candidate = Candidate::of(&run_told("r1", &a, vec![laned(true, false, true)], &ALL));
    let lanes = &candidate.standing[0].steps[1].parts;
    assert_eq!(lanes.len(), 2);
    assert_eq!(lanes[0].green, None, "the red lane stands red");
    assert_eq!(
        lanes[1].green,
        Some(On {
            commit: a.clone(),
            run: "r1".into(),
            digest: Some("p1".into()),
            environment: Some(said.clone()),
        }),
        "a green stands with its digest and its environment"
    );
    let carried = |name: &str, digest: &str| Carried {
        name: name.into(),
        commit: a.clone(),
        run: "r1".into(),
        digest: Some(digest.into()),
    };
    let plan = |inputs: &Inputs| {
        candidate.plan(
            "verify",
            &Tip {
                same: &other_tree,
                inputs: Some(inputs),
                environment: Some(&said),
            },
        )
    };

    // The fix changed the browser lane's inputs alone: it runs, the rest is carried.
    let fixed = told(&[
        ("verify/lint", "l1"),
        ("verify/tests/browser", "b2"),
        ("verify/tests/perf", "p1"),
    ]);
    assert_eq!(
        plan(&fixed),
        (
            Planned::Steps(vec!["tests/browser".into()]),
            vec![
                carried("verify/lint", "l1"),
                carried("verify/tests/perf", "p1")
            ]
        )
    );
    // It changed the green lane's inputs too: both lanes run, by name, and lint is carried.
    let both = told(&[
        ("verify/lint", "l1"),
        ("verify/tests/browser", "b2"),
        ("verify/tests/perf", "p2"),
    ]);
    assert_eq!(
        plan(&both),
        (
            Planned::Steps(vec!["tests".into()]),
            vec![carried("verify/lint", "l1")]
        ),
        "no lane kept: the step runs whole"
    );
    // It changed lint's inputs: lint runs beside the red lane.
    let lint = told(&[
        ("verify/lint", "l2"),
        ("verify/tests/browser", "b1"),
        ("verify/tests/perf", "p1"),
    ]);
    assert_eq!(
        plan(&lint),
        (
            Planned::Steps(vec!["lint".into(), "tests/browser".into()]),
            vec![carried("verify/tests/perf", "p1")]
        ),
        "a red lane runs whatever its digest"
    );
    // Every digest changed: nothing is kept, and the check runs whole.
    let all = told(&[
        ("verify/lint", "l2"),
        ("verify/tests/browser", "b2"),
        ("verify/tests/perf", "p2"),
    ]);
    assert_eq!(plan(&all), (Planned::Whole, Vec::new()));
}

#[test]
fn a_rerun_runs_what_it_cannot_tell_the_inputs_of() {
    let a = "a".repeat(40);
    let other_tree = |_: &str| false;
    let said = environment().said();
    let candidate = Candidate::of(&run_told("r1", &a, vec![laned(true, false, true)], &ALL));
    let tip = |inputs: Option<&'_ Inputs>, environment: &'_ str| {
        let tip = Tip {
            same: &other_tree,
            inputs,
            environment: Some(environment),
        };
        (candidate.plan("verify", &tip), candidate.uncarried(&tip))
    };

    // The tip's tree tells no inputs at all.
    let (plan, why) = tip(None, &said);
    assert_eq!(plan, (Planned::Whole, Vec::new()));
    assert_eq!(
        why,
        [
            "this tree has no tools/check-inputs.sh, so no step's inputs are told: a green is \
          carried only from a commit of this tree"
        ]
    );
    // Its script failed.
    let failed = Inputs {
        told: Vec::new(),
        error: Some("tools/check-inputs.sh failed (exit status: 2)".into()),
    };
    let (plan, why) = tip(Some(&failed), &said);
    assert_eq!(plan, (Planned::Whole, Vec::new()));
    assert_eq!(
        why,
        [
            "no step's inputs can be told (tools/check-inputs.sh failed (exit status: 2)): a green \
          is carried only from a commit of this tree"
        ]
    );
    // It leaves a name out: that one runs, and what it names is carried.
    let partial = told(&[("verify/lint", "l1"), ("verify/tests/browser", "b1")]);
    let (plan, why) = tip(Some(&partial), &said);
    assert_eq!(
        plan,
        (
            Planned::Steps(vec!["tests".into()]),
            vec![Carried {
                name: "verify/lint".into(),
                commit: a.clone(),
                run: "r1".into(),
                digest: Some("l1".into()),
            }]
        ),
        "the lane it leaves out is not carried, so no lane is kept"
    );
    assert_eq!(
        why,
        [
            "this tree tells no inputs for verify/tests/perf: a green is carried only from a \
          commit of this tree"
        ]
    );
    // The environment changed: nothing is carried, whatever the digests.
    let same = told(&ALL);
    let (plan, why) = tip(
        Some(&same),
        "Stand-in 2; Chrome: Chrome 3; PostgreSQL: postgres 18",
    );
    assert_eq!(plan, (Planned::Whole, Vec::new()));
    assert_eq!(
        why,
        [
            "2 greens of the candidate stood on another environment than this one (Stand-in 2; \
          Chrome: Chrome 3; PostgreSQL: postgres 18): none of them is carried"
        ]
    );
    // The hosted check's PostgreSQL 16 alone changed, the tree, the system, Chrome and 18 the
    // same; or the tip's is not what the greens' record names (a record written before the 16
    // was kept names none): another environment, and nothing is carried.
    for sixteen in ["postgres 16 of another build", ""] {
        let now = crate::release::inputs::Environment {
            postgresql_16: sixteen.into(),
            ..environment()
        }
        .said();
        let (plan, why) = tip(Some(&same), &now);
        assert_eq!(plan, (Planned::Whole, Vec::new()), "{sixteen:?}");
        assert_eq!(
            why,
            [format!(
                "2 greens of the candidate stood on another environment than this one ({now}): \
                 none of them is carried"
            )]
        );
    }
    // Nor on the same tree, where a green was always carried.
    let this_tree = |_: &str| true;
    assert_eq!(
        candidate.plan(
            "verify",
            &Tip {
                same: &this_tree,
                inputs: Some(&same),
                environment: Some("another"),
            }
        ),
        (Planned::Whole, Vec::new())
    );
    // Same inputs, same environment: said of nothing.
    assert_eq!(tip(Some(&same), &said).1, Vec::<String>::new());
}

#[test]
fn a_rerun_runs_a_step_or_lane_the_tip_names_that_no_run_listed() {
    let a = "a".repeat(40);
    let other_tree = |_: &str| false;
    let said = environment().said();
    let candidate = Candidate::of(&run_told("r1", &a, vec![laned(true, false, true)], &ALL));
    let inputs = told(&[
        ("verify/lint", "l1"),
        ("verify/size-check", "s1"),
        ("verify/tests/browser", "b1"),
        ("verify/tests/perf", "p1"),
        ("verify/tests/doc", "d1"),
        ("other/step", "o1"),
    ]);
    let (planned, carried) = candidate.plan(
        "verify",
        &Tip {
            same: &other_tree,
            inputs: Some(&inputs),
            environment: Some(&said),
        },
    );
    assert_eq!(
        planned,
        Planned::Steps(vec![
            "tests/browser".into(),
            "tests/doc".into(),
            "size-check".into()
        ]),
        "what no run reached is never passed over"
    );
    assert_eq!(carried.len(), 2, "{carried:?}");
}

#[test]
fn a_step_red_beyond_its_lanes_runs_whole_and_a_check_all_carried_is_carried_whole() {
    let (a, b) = ("a".repeat(40), "b".repeat(40));
    let other_tree = |_: &str| false;
    let said = environment().said();
    let inputs = told(&ALL);
    let tip = Tip {
        same: &other_tree,
        inputs: Some(&inputs),
        environment: Some(&said),
    };
    // The tests step red while every lane it listed is green: its red is its own.
    let mut beyond = laned(true, true, true);
    beyond.exit_code = Some(1);
    beyond.steps[1].exit_code = Some(1);
    let candidate = Candidate::of(&run_told("r1", &a, vec![beyond], &ALL));
    let (planned, carried) = candidate.plan("verify", &tip);
    assert_eq!(planned, Planned::Steps(vec!["tests".into()]));
    assert_eq!(carried.len(), 1, "lint alone: {carried:?}");

    // Green whole beside another check's red, and every input as it was: carried whole, each
    // step and lane named with its digest.
    let mut candidate = Candidate::of(&run_told(
        "r1",
        &a,
        vec![laned(true, true, true), ran("docs", 1, Vec::new())],
        &ALL,
    ));
    let (planned, carried) = candidate.plan("verify", &tip);
    assert_eq!(planned, Planned::Carried);
    let names: Vec<&str> = carried.iter().map(|kept| kept.name.as_str()).collect();
    assert_eq!(
        names,
        ["verify/lint", "verify/tests/browser", "verify/tests/perf"]
    );
    assert!(carried.iter().all(|kept| kept.digest.is_some()));
    assert_eq!(candidate.plan("docs", &tip), (Planned::Whole, Vec::new()));

    // A rerun that ran one lane alone leaves the others standing where they were green.
    let browser = ran("verify/tests/browser", 0, Vec::new());
    let tests = ran("verify/tests", 0, vec![browser]);
    let partial = ran("verify", 0, vec![tests]);
    let again = [
        ("verify/lint", "l1"),
        ("verify/tests/browser", "b2"),
        ("verify/tests/perf", "p1"),
    ];
    let mut second = run_told("r2", &b, vec![partial], &again);
    second.rerun = Some(record::Rerun {
        first: a.clone(),
        round: 2,
        carried: carried.clone(),
    });
    candidate.decided(&second);
    let lanes = &candidate.standing[0].steps[1].parts;
    assert_eq!(
        lanes[0].green.as_ref().map(|on| on.run.as_str()),
        Some("r2")
    );
    assert_eq!(
        lanes[0].green.as_ref().unwrap().digest.as_deref(),
        Some("b2")
    );
    assert_eq!(
        lanes[1].green.as_ref().map(|on| on.run.as_str()),
        Some("r1")
    );
    let lint = &candidate.standing[0].steps[0];
    assert_eq!(lint.green.as_ref().map(|on| on.run.as_str()), Some("r1"));
}

#[test]
fn a_tests_step_no_run_listed_is_run_though_the_tip_tells_only_its_lanes() {
    // The runner's setup red: the verify lists no tests step, and so no lane of it.
    let a = "a".repeat(40);
    let said = environment().said();
    let step = |name: &str, code: i32| ran(name, code, Vec::new());
    let first = ran(
        "verify",
        1,
        vec![step("verify/lint", 0), step("verify/nextest-setup", 2)],
    );
    let inputs = [
        ("verify/lint", "l1"),
        ("verify/nextest-setup", "n1"),
        ("verify/tests/browser", "b1"),
        ("verify/tests/perf", "p1"),
    ];
    let candidate = Candidate::of(&run_told("r1", &a, vec![first], &inputs));
    let tip = told(&inputs);
    // On another tree that tells every digest as it was, and on the same tree.
    for same in [false, true] {
        let same_tree = move |_: &str| same;
        let (planned, carried) = candidate.plan(
            "verify",
            &Tip {
                same: &same_tree,
                inputs: Some(&tip),
                environment: Some(&said),
            },
        );
        assert_eq!(
            planned,
            Planned::Steps(vec!["nextest-setup".into(), "tests".into()]),
            "the tests never ran on this candidate: they run whole (same tree: {same})"
        );
        let names: Vec<&str> = carried.iter().map(|kept| kept.name.as_str()).collect();
        assert_eq!(names, ["verify/lint"]);
    }
}

#[test]
fn a_green_of_a_rerun_that_ran_it_in_part_stands_by_its_parts_each_on_its_own_run() {
    let (a, b) = ("a".repeat(40), "b".repeat(40));
    let said = environment().said();
    // r1 on a: lint and the browser lane red, the perf lane green.
    let mut candidate = Candidate::of(&run_told("r1", &a, vec![laned(false, false, true)], &ALL));
    // r2 on b ran lint, red again, and the browser lane alone, green; perf was carried from a.
    let perf = Carried {
        name: "verify/tests/perf".into(),
        commit: a.clone(),
        run: "r1".into(),
        digest: Some("p1".into()),
    };
    let browser = ran("verify/tests/browser", 0, Vec::new());
    let tests = ran("verify/tests", 0, vec![browser]);
    let partial = ran("verify", 1, vec![ran("verify/lint", 1, Vec::new()), tests]);
    let again = [
        ("verify/lint", "l2"),
        ("verify/tests/browser", "b2"),
        ("verify/tests/perf", "p1"),
    ];
    let mut second = run_told("r2", &b, vec![partial], &again);
    second.rerun = Some(record::Rerun {
        first: a.clone(),
        round: 2,
        carried: vec![perf.clone()],
    });
    candidate.decided(&second);
    let tests = &candidate.standing[0].steps[1];
    assert_eq!(
        (tests.green.as_ref(), tests.beyond),
        (None, false),
        "green in r2 by its lanes, never of itself: it was not run whole on b"
    );

    // A rerun on the tree of b: the tests are carried lane by lane, perf still from a in r1.
    let inputs = told(&again);
    let this_tree = |commit: &str| commit == b;
    let (planned, carried) = candidate.plan(
        "verify",
        &Tip {
            same: &this_tree,
            inputs: Some(&inputs),
            environment: Some(&said),
        },
    );
    assert_eq!(planned, Planned::Steps(vec!["lint".into()]));
    assert_eq!(
        carried,
        [
            Carried {
                name: "verify/tests/browser".into(),
                commit: b.clone(),
                run: "r2".into(),
                digest: None,
            },
            perf.clone(),
        ]
    );
    let notes = candidate.notes(&carried);
    assert_eq!(
        notes[notes.len() - 4..],
        [
            "Carried green from a commit whose tree is this one's:",
            "- verify/tests/browser: green on bbbbbbbbbbbb in the run r2",
            "Carried green from a run of this candidate, not run on this commit: its inputs' \
             digest is the one it was green on:",
            "- verify/tests/perf: green on aaaaaaaaaaaa in the run r1, inputs p1",
        ]
    );

    // A check green in a rerun that carried a step of it stands by its steps the same way, and
    // with nothing left to run it is carried by them, each named.
    let mut whole = Candidate::of(&run_told(
        "r1",
        &a,
        vec![laned(true, false, true), ran("docs", 1, Vec::new())],
        &ALL,
    ));
    let lint = Carried {
        name: "verify/lint".into(),
        commit: a.clone(),
        run: "r1".into(),
        digest: Some("l1".into()),
    };
    let rest = ran("verify", 0, vec![laned(true, true, true).steps.remove(1)]);
    let all_again = [
        ("verify/lint", "l1"),
        ("verify/tests/browser", "b2"),
        ("verify/tests/perf", "p2"),
    ];
    let mut second = run_told("r2", &b, vec![rest, ran("docs", 1, Vec::new())], &all_again);
    second.rerun = Some(record::Rerun {
        first: a.clone(),
        round: 2,
        carried: vec![lint.clone()],
    });
    whole.decided(&second);
    assert_eq!(whole.standing[0].green, None, "verify ran in part on b");
    let tests = &whole.standing[0].steps[1];
    assert_eq!(
        tests.green.as_ref().map(|on| on.run.as_str()),
        Some("r2"),
        "the tests step ran whole on b"
    );
    let inputs = told(&all_again);
    let (planned, carried) = whole.plan(
        "verify",
        &Tip {
            same: &this_tree,
            inputs: Some(&inputs),
            environment: Some(&said),
        },
    );
    assert_eq!(planned, Planned::Carried);
    assert_eq!(carried.len(), 2, "{carried:?}");
    assert_eq!(carried[0], lint, "lint is still the green of a in r1");
    assert_eq!(
        (carried[1].name.as_str(), carried[1].run.as_str()),
        ("verify/tests", "r2")
    );
}

#[test]
fn a_release_names_each_green_carried_by_its_inputs_with_its_digest() {
    let a = "a".repeat(40);
    let candidate = Candidate::of(&run_told("r1", &a, vec![laned(true, false, true)], &ALL));
    let kept = |name: &str, digest: Option<&str>| Carried {
        name: name.into(),
        commit: a.clone(),
        run: "r1".into(),
        digest: digest.map(str::to_string),
    };
    let notes = candidate.notes(&[
        kept("verify/lint", Some("l1")),
        kept("docs", None),
        kept("verify/tests/perf", Some("p1")),
    ]);
    assert_eq!(
        notes[notes.len() - 5..],
        [
            "Carried green from a commit whose tree is this one's:",
            "- docs: green on aaaaaaaaaaaa in the run r1",
            "Carried green from a run of this candidate, not run on this commit: its inputs' \
             digest is the one it was green on:",
            "- verify/lint: green on aaaaaaaaaaaa in the run r1, inputs l1",
            "- verify/tests/perf: green on aaaaaaaaaaaa in the run r1, inputs p1",
        ]
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
        digest: None,
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

/// A red that needed no review lands with no approve: its landing's line names the tip its needed
/// stamps stood on, which the candidate moves to as to an approved commit.
#[test]
fn a_landing_by_its_needed_stamps_names_the_tip_they_stood_on() {
    let tip = "4be91c2aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let landed = format!(
        "landed tests/release-red-verify on main at 0123456789ab; its needs (tests) stood at {tip}; \
         pushed to origin"
    );
    // Only the land's own line counts: a verdict or a note that quotes one names nothing.
    let other = "7c0ffee1bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let quoted =
        format!("landed tests/x on main at 0123456789ab; its needs (tests) stood at {other}");
    let messages = vec![
        serde_json::json!({"kind": "verdict", "text": landed}),
        serde_json::json!({"kind": "verdict", "text": "landed tests/x on main at 0123456789ab"}),
        serde_json::json!({"kind": "message", "text": format!("its needs (tests) stood at {tip}")}),
        serde_json::json!({"kind": "verdict", "text": format!("changes: the land said `{quoted}`")}),
        serde_json::json!({"kind": "verdict", "text": format!("covered by tests/y; {quoted}")}),
    ];
    assert_eq!(approving(&messages), vec![tip.to_string()]);
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
            delivered: None,
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
    let mut releases = symphony::project::Releases::default();
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

/// [`SLUG`]'s `head` squashed onto `scratch`'s main as a pull request delivers it, its receipt
/// kept and its head retained: the receipt.
fn delivered(scratch: &Scratch, head: &str) -> Receipt {
    scratch.checkout(&["main"]);
    let receipt = crate::release::receipts::tests::squash(&scratch.root, SLUG, head);
    crate::release::receipts::tests::keep(&scratch.root, &receipt);
    receipt
}

#[test]
fn a_land_a_pull_request_delivered_advances_the_candidate_to_the_head_its_receipt_kept() {
    let tree = Tree::new();
    let scratch = Scratch::new(&tree);
    let (tip, [_, two], _) = fixing(&scratch);
    let receipt = delivered(&scratch, &two);
    let main = scratch.head();
    assert_ne!(receipt.squash, two);
    // No approve and no landed line on its thread: the land was cut after its receipt and
    // before it closed the task. The head the receipt kept vouches for itself.
    let board = Fake::default();
    let mut candidate = candidate_at(&tip);
    let said = settle(
        &mut candidate,
        &line(&scratch, &main, Some(&main)),
        &board,
        NOW + 60.0,
        BOUNDS,
    );
    assert_eq!(
        candidate.commit, two,
        "the head, never the squash: {said:?}"
    );
    assert!(
        candidate.open(),
        "a head its squash delivered is on the line"
    );
    assert_eq!(
        candidate.reds[0].answer,
        Some(Answer::Landed {
            merge: receipt.squash.clone(),
            commit: two.clone()
        })
    );
    assert_eq!(
        candidate.advances,
        vec![Advance {
            slug: SLUG.into(),
            from: tip.clone(),
            to: two.clone(),
            merge: receipt.squash.clone(),
            delivered: Some(Delivered {
                head: two.clone(),
                parent: receipt.parent.clone(),
                repository: 11,
                name: "owner/repository".into(),
                pull: 5,
                revision: "r1".into(),
            }),
        }]
    );
    assert_eq!(
        said,
        vec![format!(
            "advanced the release candidate from {} to {}, the newest approved commit of {SLUG}'s \
             land ({}), the squash of {} by pull request #5 of owner/repository",
            short(&tip),
            short(&two),
            short(&receipt.squash),
            short(&two)
        )]
    );
    // Settled again, and again after its record is read back: nothing more is done or said.
    let once = candidate.clone();
    let folder = tree.path("release");
    std::fs::create_dir_all(&folder).unwrap();
    write(&folder, &candidate).unwrap();
    let mut read_back = read(&folder).unwrap().unwrap();
    assert_eq!(read_back, once);
    for candidate in [&mut candidate, &mut read_back] {
        let again = settle(
            candidate,
            &line(&scratch, &main, Some(&main)),
            &board,
            NOW + 60.0,
            BOUNDS,
        );
        assert_eq!(again, Vec::<String>::new());
        assert_eq!(*candidate, once);
    }
    // Its branch follows without a force: from the tip the run set it to.
    let mut steered = candidate_at(&tip);
    assert_eq!(steer(&scratch.root, &mut steered), None);
    candidate.branch.clone_from(&steered.branch);
    assert_eq!(steer(&scratch.root, &mut candidate), None);
    assert_eq!(commit(&scratch.root, REFERENCE).unwrap(), two);
}

#[test]
fn an_advance_by_a_land_merged_here_is_recorded_as_it_was_before_receipts() {
    let advance = Advance {
        slug: SLUG.into(),
        from: "a".repeat(40),
        to: "b".repeat(40),
        merge: "c".repeat(40),
        delivered: None,
    };
    let text = serde_json::to_string(&advance).unwrap();
    assert!(!text.contains("delivered"), "{text}");
    assert_eq!(serde_json::from_str::<Advance>(&text).unwrap(), advance);
}

#[test]
fn a_delivered_head_that_merged_main_never_brings_main_into_the_candidate() {
    let tree = Tree::new();
    let scratch = Scratch::new(&tree);
    let (tip, [_, two], _) = fixing(&scratch);
    // The branch catches up with main, as one settling a conflict does, and that head is delivered.
    scratch.checkout(&[SLUG]);
    let caught_up = scratch.merge("main", &format!("Merge branch 'main' into {SLUG}"));
    let receipt = delivered(&scratch, &caught_up);
    let main = scratch.head();
    let settled = |board: &Fake| {
        let mut candidate = candidate_at(&tip);
        let said = settle(
            &mut candidate,
            &line(&scratch, &main, Some(&main)),
            board,
            NOW + 60.0,
            BOUNDS,
        );
        (candidate, said)
    };
    // An approve names the fix before it caught up: the candidate takes that commit alone.
    let approved = Fake {
        approves: vec![(SLUG.into(), vec![format!("approve: {two}: the fix")])],
        ..Fake::default()
    };
    let (candidate, said) = settled(&approved);
    assert_eq!(candidate.commit, two, "{said:?}");
    assert!(candidate.open(), "{said:?}");
    assert_eq!(
        candidate.advances[0].delivered.as_ref().unwrap().head,
        caught_up
    );
    // With none, the delivered head holds main's commits, and the candidate is dropped as for
    // any land with no commit of the fix alone: main's tip is what is run then.
    let (candidate, said) = settled(&Fake::default());
    assert_eq!(candidate.state, State::Dropped);
    assert_eq!(
        said,
        vec![format!(
            "dropped the release candidate: {SLUG} landed ({}) with no approved commit that \
             descends from the candidate ({}) and holds beyond it nothing main held before the \
             land",
            short(&receipt.squash),
            short(&tip)
        )]
    );
}

#[test]
fn a_delivered_merge_keeping_mains_outputs_leaves_the_candidate_the_fixs_own_outputs() {
    // A red's fix that changes what the client is built from, under delivery (RELEASES, a red's
    // fix): its head commits the outputs its own sources make, main makes its own meanwhile, and
    // the head delivered merges main keeping main's side of them. The land tells such a fix by
    // the branch this run keeps.
    assert_eq!(BRANCH, symphony_board::land::delivery::CANDIDATE);
    let tree = Tree::new();
    let scratch = Scratch::new(&tree);
    let tip = scratch.commit("dist.txt", "built of the candidate\n", "the candidate");
    scratch.checkout(&["-b", SLUG]);
    let fix = scratch.commit("dist.txt", "built of the fix\n", "the fix, and its build");
    scratch.checkout(&["main"]);
    scratch.commit("dist.txt", "built of main\n", "main's build");
    scratch.checkout(&[SLUG]);
    let named = ["-c", "user.name=t", "-c", "user.email=t@example.com"];
    let merge = git(
        &scratch.root,
        &[&named[..], &["merge", "-q", "--no-ff", "main"]].concat(),
    );
    assert!(merge.is_err(), "dist.txt conflicts");
    scratch.git(&["checkout", "main", "--", "dist.txt"]);
    scratch.git(&["commit", "-q", "--no-edit"]);
    let caught_up = scratch.head();
    let receipt = delivered(&scratch, &caught_up);
    let main = scratch.head();
    let show = |at: &str| scratch.git(&["show", &format!("{at}:dist.txt")]);
    assert_eq!(show(&receipt.squash), "built of main", "main keeps its own");
    let board = Fake {
        approves: vec![(
            SLUG.into(),
            vec![
                format!("approve: {fix}: the fix and its build"),
                format!("approve: {caught_up}: main merged, its outputs kept"),
            ],
        )],
        ..Fake::default()
    };
    let settled = |at: &str| {
        let mut candidate = candidate_at(at);
        let said = settle(
            &mut candidate,
            &line(&scratch, &main, Some(&main)),
            &board,
            NOW + 60.0,
            BOUNDS,
        );
        (candidate, said)
    };
    let (candidate, said) = settled(&tip);
    assert_eq!(candidate.commit, fix, "{said:?}");
    assert_eq!(show(&candidate.commit), "built of the fix");
    assert!(candidate.open(), "{said:?}");
    // A candidate that moved on since the fix started (another red's fix advanced it) takes
    // neither: the fix does not hold its tip, and it is dropped, said.
    scratch.checkout(&["-b", "tools/another-red", &tip]);
    let moved = scratch.commit("other.txt", "another red's fix\n", "another red's fix");
    let (candidate, said) = settled(&moved);
    assert_eq!(candidate.state, State::Dropped, "{said:?}");
    assert_eq!(candidate.commit, moved);
    assert_eq!(
        said,
        vec![format!(
            "dropped the release candidate: {SLUG} landed ({}) with no approved commit that \
             descends from the candidate ({}) and holds beyond it nothing main held before the \
             land",
            short(&receipt.squash),
            short(&moved)
        )]
    );
}

#[test]
fn a_receipt_whose_head_is_gone_makes_the_candidate_wait_and_drops_nothing() {
    let tree = Tree::new();
    let scratch = Scratch::new(&tree);
    let (tip, [_, two], _) = fixing(&scratch);
    scratch.checkout(&["main"]);
    let receipt = Receipt {
        head: "f".repeat(40),
        ..crate::release::receipts::tests::squash(&scratch.root, SLUG, &two)
    };
    let path = Receipt::path(
        &crate::release::receipts::lands(&scratch.root).unwrap(),
        SLUG,
    );
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_string(&receipt).unwrap()).unwrap();
    let main = scratch.head();
    let mut candidate = candidate_at(&tip);
    let board = Fake {
        statuses: vec![(SLUG.into(), "in_progress".into())],
        ..Fake::default()
    };
    let said = settle(
        &mut candidate,
        &line(&scratch, &main, None),
        &board,
        NOW + 60.0,
        BOUNDS,
    );
    assert_eq!(candidate.commit, tip);
    assert!(candidate.open(), "{said:?}");
    assert!(candidate.advances.is_empty());
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].starts_with(&format!(
            "{SLUG}'s land ({}) cannot be read, so it waits: ",
            short(&receipt.squash)
        )),
        "{said:?}"
    );
}

#[test]
fn a_candidate_past_a_release_on_a_delivered_head_is_not_passed_by_it() {
    let tree = Tree::new();
    let scratch = Scratch::new(&tree);
    let (_, [_, two], _) = fixing(&scratch);
    // An earlier candidate was released on the head `two`, which its squash delivered to main;
    // main's tip after it is held as the next candidate.
    let receipt = delivered(&scratch, &two);
    let tip = scratch.commit("a.txt", "next\n", "the next candidate");
    let released = |at: &str, remote: &str| {
        let mut candidate = candidate_at(&tip);
        let line = Line {
            newest: Some(("release-2026-11-12", at)),
            ..line(&scratch, remote, None)
        };
        let said = settle(&mut candidate, &line, &Fake::default(), NOW, BOUNDS);
        (candidate, said)
    };
    let (candidate, said) = released(&two, &tip);
    assert!(candidate.open(), "{said:?}");
    assert_eq!(said, Vec::<String>::new());
    // The same release with its receipt gone is on no line the candidate holds: dropped, as a
    // release that passed it is.
    std::fs::remove_file(Receipt::path(
        &crate::release::receipts::lands(&scratch.root).unwrap(),
        SLUG,
    ))
    .unwrap();
    let (candidate, said) = released(&two, &tip);
    assert_eq!(candidate.state, State::Dropped, "{said:?}");
    assert!(said[0].contains("passed it"), "{said:?}");
    let _ = receipt;
}
