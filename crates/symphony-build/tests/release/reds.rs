//! A red check leaves no tag and files one red task on the board, with the failing step and its
//! log; a second red run updates it; a green rerun closes it and releases; a commit before a red
//! released out of cycle leaves that red open.

use super::{CHECK, Fixture, NOW, git};
use symphony_board::{NewSession, NewTask};

#[test]
fn a_red_check_leaves_no_tag_and_files_exactly_one_red_task_a_green_rerun_closes() {
    let fixture = Fixture::new();
    let green = git(&fixture.main, &["rev-parse", "HEAD"]);
    fixture.night(NOW, 0);
    assert!(fixture.reds().is_empty(), "a green run files nothing");
    let planted = fixture.land("red", Some("planted"), "a change that turns the check red");

    let out = fixture.night(NOW + 3600.0, 1);
    assert!(out.contains("release: verify red (exit 1"), "{out}");
    assert!(out.contains("release: red: verify; no release"), "{out}");
    assert!(
        out.contains("release: filed tests/release-red-verify"),
        "{out}"
    );
    assert_eq!(
        fixture.remote_tags(),
        ["release-2026-11-12"],
        "no tag for a red tip"
    );
    assert_eq!(git(&fixture.main, &["tag", "-l"]), "release-2026-11-12");

    let reds = fixture.reds();
    assert_eq!(
        reds.keys().collect::<Vec<_>>(),
        ["tests/release-red-verify"]
    );
    let red = &reds["tests/release-red-verify"];
    assert_eq!(red.priority, 0);
    assert_eq!(red.status, "ready");
    assert!(red.red(), "marked red: it takes no orchestrator slot");
    assert_eq!(
        red.fields
            .get("_board")
            .and_then(|board| board.get("fast_fix")),
        None,
        "filed unmarked: its assessor reads what its fix needs at review"
    );
    assert_eq!(
        red.title,
        format!("Release red: verify on {}", &planted[..12])
    );
    assert_eq!(red.links, ["docs/workflow/RELEASES.md"]);
    let run = fixture.runs().last().unwrap().clone();
    let log = run["checks"][0]["log"].as_str().unwrap().to_string();
    assert!(
        log.starts_with(
            &fixture
                .records()
                .join("logs")
                .to_string_lossy()
                .into_owned()
        ),
        "{log}"
    );
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "$ test ! -e red\n");
    let load = &run["checks"][0]["load"];
    assert!(
        load["processors"].as_u64().is_some_and(|n| n >= 1)
            && load["averages"].as_array().is_some_and(|a| a.len() == 3),
        "the record keeps the load the check ended under: {run}"
    );
    assert!(!red.body.contains("not recorded"), "{}", red.body);
    // The machine stayed awake under it: neither the run, its record nor its red says a sleep.
    assert!(!out.contains("slept"), "{out}");
    assert!(run.get("slept").is_none(), "{run}");
    assert!(!red.body.contains("slept"), "{}", red.body);
    for said in [
        format!(
            "The Symphony release run is red in `verify` on main at {}",
            &planted[..12]
        ),
        "Command: `test ! -e red`, in the release run's clean checkout of that commit: exit 1."
            .into(),
        format!("Log: {log}\nLoad average when it ended: "),
        format!(
            "Rerun alone (RULES.md `tests/rerun-the-failure-alone`): its log names no failed \
             test, so nothing narrower than its own command, `test ! -e red`, in a checkout of \
             {}.",
            &planted[..12]
        ),
        "It blocks every release, so it is staffed before anything else".into(),
        "never a test or check loosened".into(),
        "A test the run reran alone that passed is no red, and has a task of its own; a test red \
         here failed alone too, or was not rerun. One red on the machine's load all the same (it \
         fails on a wall-clock bound, a sleep-ordered step or a timeout under load) is discarded \
         in this fix instead, as RULES.md `tests/no-test-fails-on-load` says"
            .into(),
        "What its fix needs to land is read at review by an assessor, before any reviewer \
         (`/assess-task`): a fix in code that leaves the failing tests unchanged needs its tests"
            .into(),
        "one that changes a test, a check, a budget or a run step (`[land] check_paths`) needs a \
         fresh reviewer's approve as well"
            .into(),
        format!(
            "Commits since the last green run, `git log --oneline {}..{}` (1):\n- {} a change that turns the check red",
            &green[..12],
            &planted[..12],
            git(&fixture.main, &["rev-parse", "--short", &planted])
        ),
        format!("\nbetween: {green}..{planted}\nkey: verify\ncheck: verify"),
    ] {
        assert!(red.body.contains(&said), "{said:?} in:\n{}", red.body);
    }
    assert_eq!(run["board"]["filed"][0], "tests/release-red-verify");

    // Red again: the same task hears of it.
    let out = fixture.night(NOW + 7200.0, 1);
    assert!(
        out.contains("release: updated tests/release-red-verify"),
        "{out}"
    );
    assert_eq!(
        fixture.reds().len(),
        1,
        "never a second task for the same red"
    );
    let thread = fixture.thread("tests/release-red-verify");
    assert!(
        thread
            .iter()
            .any(|text| text.starts_with(&format!("Still red on {}", &planted[..12]))),
        "{thread:#?}"
    );

    // The fix lands: green, released, and the red closed.
    let fixed = fixture.land("red", None, "the fix");
    let out = fixture.night(NOW + 10_800.0, 0);
    assert!(
        out.contains("release: closed tests/release-red-verify"),
        "{out}"
    );
    assert_eq!(
        fixture.reds()["tests/release-red-verify"].status,
        "cancelled"
    );
    assert!(
        fixture.thread("tests/release-red-verify").iter().any(|text| text
            == &format!(
                "Green on {} in run {} (2026-11-12 13:00): `verify` passed. Closed by the release run.",
                &fixed[..12],
                fixture.runs().last().unwrap()["id"].as_str().unwrap()
            )),
        "{:#?}",
        fixture.thread("tests/release-red-verify")
    );
    assert_eq!(fixture.tagged("release-2026-11-12.2"), fixed);

    // A later red is a new task, the closed one left as it is.
    let urgent = fixture.land("fix.txt", Some("urgent"), "an urgent fix");
    fixture.land("red", Some("again"), "red again");
    let out = fixture.night(NOW + 14_400.0, 1);
    assert!(
        out.contains("release: filed tests/release-red-verify-2"),
        "{out}"
    );
    let reds = fixture.reds();
    assert_eq!(
        reds.keys().collect::<Vec<_>>(),
        ["tests/release-red-verify", "tests/release-red-verify-2"]
    );
    assert_eq!(reds["tests/release-red-verify"].status, "cancelled");
    assert_eq!(reds["tests/release-red-verify-2"].status, "ready");

    // The urgent fix from before that red, released out of cycle: green, but main's tip is still
    // red, so its red stays open.
    let out = fixture.run(NOW + 18_000.0, &["--check", CHECK, "--commit", &urgent], 0);
    assert_eq!(fixture.tagged("release-2026-11-12.3"), urgent);
    assert!(!out.contains("closed"), "{out}");
    assert_eq!(fixture.reds()["tests/release-red-verify-2"].status, "ready");
    assert!(
        !(fixture.thread("tests/release-red-verify-2").iter())
            .any(|text| text.starts_with("Green")),
        "{:#?}",
        fixture.thread("tests/release-red-verify-2")
    );

    // Fixed on main, the run closes it.
    fixture.land("red", None, "the fix again");
    let out = fixture.night(NOW + 21_600.0, 0);
    assert!(
        out.contains("release: closed tests/release-red-verify-2"),
        "{out}"
    );
}

#[test]
fn a_nights_red_is_offered_first_and_starts_past_an_orchestrators_cap() {
    let fixture = Fixture::new();
    let store = fixture.store().capping(Some(1));
    let session = |new: NewSession| store.register(&new).unwrap().row.handle;
    let orchestrator = session(NewSession::new("orchestrator"));
    let worker = || {
        session(NewSession {
            parent: Some(orchestrator.clone()),
            ..NewSession::new("worker")
        })
    };
    // Urgent work filed before the run, and the orchestrator's one slot taken by a feature.
    for slug in ["work/older", "work/feature"] {
        let urgent = NewTask {
            priority: 0,
            ..NewTask::new(slug, "Urgent work", "owner")
        };
        store.create(&urgent).unwrap();
    }
    store
        .take_role("work/feature", &orchestrator, "orchestrator")
        .unwrap();
    store.claim("work/feature", &worker()).unwrap();
    fixture.land("red", Some("planted"), "a change that turns the check red");
    fixture.night(NOW, 1);

    let executor = worker();
    assert_eq!(
        store.next_for(&executor).unwrap().map(|task| task.slug),
        Some("tests/release-red-verify".into()),
        "offered before older work of its priority"
    );
    let refused = store
        .claim("work/older", &executor)
        .unwrap_err()
        .to_string();
    assert!(
        refused.contains("work/older cannot start: ")
            && refused.contains("the cap of 1 an orchestrator holds: work/feature (in_progress)"),
        "{refused}"
    );
    assert_eq!(
        store
            .claim("tests/release-red-verify", &executor)
            .unwrap()
            .status,
        "in_progress",
        "red work takes no slot"
    );
    store
        .take_role("tests/release-red-verify", &orchestrator, "orchestrator")
        .unwrap();
    let refused = store
        .claim("work/older", &worker())
        .unwrap_err()
        .to_string();
    assert!(
        refused.contains(
            "not counted, as red work takes no slot: tests/release-red-verify (in_progress)"
        ),
        "{refused}"
    );
}

#[test]
fn every_red_check_is_its_own_task_and_every_check_runs() {
    let fixture = Fixture::new();
    let out = fixture.run(
        NOW,
        &[
            "--check",
            "lint=echo lint said this; exit 3",
            "--check",
            CHECK,
            "--check",
            "Browser Tests=kill -TERM $$",
        ],
        1,
    );
    assert!(out.contains("release: lint red (exit 3"), "{out}");
    assert!(out.contains("release: verify green (exit 0"), "{out}");
    assert!(
        out.contains("release: Browser Tests red (signal 15"),
        "{out}"
    );
    let reds = fixture.reds();
    assert_eq!(
        reds.keys().collect::<Vec<_>>(),
        ["tests/release-red-browser-tests", "tests/release-red-lint"]
    );
    assert!(
        reds["tests/release-red-lint"]
            .body
            .contains("```\n$ echo lint said this; exit 3\nlint said this\n```")
    );
    assert!(fixture.remote_tags().is_empty());
}

#[test]
fn a_board_that_cannot_be_reached_fails_the_run_but_the_record_stays() {
    let fixture = Fixture::new();
    let (code, out, err) = fixture.release_with(
        NOW,
        &["--check", "verify=false"],
        &[("SYMPHONY_TASKS_CLI", "/no/such/tasks")],
    );
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        err.contains("symphony-build release: the board: cannot run /no/such/tasks"),
        "{err}"
    );
    let run = fixture.runs().last().unwrap().clone();
    assert_eq!(run["result"], "red");
    assert!(
        run["board"]["error"]
            .as_str()
            .unwrap()
            .contains("cannot run /no/such/tasks"),
        "{run}"
    );
}

#[test]
fn a_run_the_machine_slept_under_counts_the_sleep_and_says_it_in_its_record_status_and_red() {
    let fixture = Fixture::new();
    fixture.land("red", Some("planted"), "a change that turns the check red");
    // The tests' sleep: 26 min 29 s under each span the run times.
    let slept = [("SYMPHONY_RELEASE_SLEPT", "1589")];
    let (code, out, err) = fixture.release_with(NOW, &["--check", CHECK], &slept);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains("release: the machine slept 26 min under this run, which its time counts"),
        "{out}"
    );
    let run = fixture.runs().last().unwrap().clone();
    // The two clocks are read one after the other, so the sleep is the tests' to within what the
    // run takes for the wall clock's own correction (5 s), which only a stall reaches.
    let recorded = run["slept"].as_f64().unwrap();
    assert!((recorded - 1589.0).abs() < 5.0, "{run}");
    let took = run["ended_at"].as_f64().unwrap() - run["started_at"].as_f64().unwrap();
    assert!(took >= 1584.0, "the run's time counts the sleep: {run}");
    let check = run["checks"][0]["seconds"].as_f64().unwrap();
    assert!(
        check >= 1584.0 && check <= took + 5.0,
        "the check's seconds count the sleep, within the run's: {run}"
    );
    let reds = fixture.reds();
    let body = &reds["tests/release-red-verify"].body;
    let at = |said: &str| {
        body.find(said)
            .unwrap_or_else(|| panic!("{said:?} in:\n{body}"))
    };
    assert!(
        at("Load average when it ended: ") < at("\nThe machine slept 26 min under this run"),
        "{body}"
    );
    let (_, status, _) = fixture.release_at(NOW + 60.0, &["--status"]);
    assert!(
        status.contains("; the machine slept 26 min under it; red: verify (exit 1)"),
        "{status}"
    );
}
