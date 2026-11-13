//! A red check leaves no tag and files one red task on the board, with the failing step and its
//! log; a second red run updates it; a green rerun closes it and releases.

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
        red.fast_fix(),
        Some(symphony_board::fast_fix::RED),
        "filed as a fast fix, under the bound the land holds a red's fix to"
    );
    assert_eq!(
        red.title,
        format!("Release red: verify on {}", &planted[..12])
    );
    assert_eq!(red.links, ["docs/RELEASES.md"]);
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
        "It is filed as a fast fix, under a bound the land holds it to: its orchestrator, which \
         did not execute it, approves a fix that changes none of the project's tests and checks \
         (`[land] check_paths`)"
            .into(),
        "The land refuses that approve on a fix that changes a test, a check, a budget or a run \
         step: a fresh reviewer"
            .into(),
        format!(
            "Commits since the last green run, `git log --oneline {}..{}` (1):\n- {} a change that turns the check red",
            &green[..12],
            &planted[..12],
            git(&fixture.main, &["rev-parse", "--short", &planted])
        ),
        "key: verify\ncheck: verify".into(),
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
    fixture.land("red", Some("again"), "red again");
    fixture.night(NOW + 14_400.0, 1);
    let reds = fixture.reds();
    assert_eq!(
        reds.keys().collect::<Vec<_>>(),
        ["tests/release-red-verify", "tests/release-red-verify-2"]
    );
    assert_eq!(reds["tests/release-red-verify"].status, "cancelled");
    assert_eq!(reds["tests/release-red-verify-2"].status, "ready");
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
        err.contains("symphony release: the board: cannot run /no/such/tasks"),
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
fn a_release_of_a_commit_before_a_red_leaves_that_red_open() {
    let fixture = Fixture::new();
    fixture.night(NOW, 0);
    let fix = fixture.land("fix.txt", Some("urgent"), "an urgent fix");
    fixture.land("red", Some("planted"), "a red change after it");
    let out = fixture.night(NOW + 3600.0, 1);
    assert!(
        out.contains("release: filed tests/release-red-verify"),
        "{out}"
    );

    // An urgent fix from before the red, released out of cycle: green, but main's tip is still
    // red, so its red stays open.
    let out = fixture.run(NOW + 7200.0, &["--check", CHECK, "--commit", &fix], 0);
    assert_eq!(fixture.tagged("release-2026-11-12.2"), fix);
    assert!(!out.contains("closed"), "{out}");
    assert_eq!(fixture.reds()["tests/release-red-verify"].status, "ready");
    assert!(
        !(fixture.thread("tests/release-red-verify").iter()).any(|text| text.starts_with("Green")),
        "{:#?}",
        fixture.thread("tests/release-red-verify")
    );

    // Fixed on main, the run closes it.
    fixture.land("red", None, "the fix");
    let out = fixture.night(NOW + 10_800.0, 0);
    assert!(
        out.contains("release: closed tests/release-red-verify"),
        "{out}"
    );
}
