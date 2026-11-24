//! The release run on a line whose lands a pull request delivers (`[land] delivery`): each land is
//! one squashed commit the host made, which the land's receipt names. The cadence counts by the
//! receipts, a landing whose `on_land` never ran is counted by the next count, a release
//! candidate advances to the head a receipt kept and never takes main, and a release on such a
//! head is one the line moves forward from. The host and the land are the test's own: a squash
//! pushed to the scratch remote, and the receipt and the retained head the land keeps of it. No
//! test asks a host anything.

use std::fs;

use symphony_board::land::delivery::{RETAINED, Receipt};

use super::candidate::{self, RED};
use super::{CHECK, DAY, Fixture, NOW, cadence, git};

/// `[land] delivery` set, with `releases` before it in the project file.
fn delivering(releases: &str) -> String {
    format!("{releases}\n[land]\ncheck = false\ndelivery = [\"host\", \"deliver\"]\n")
}

/// The fixture counting every `every` landings on a line that delivers by pull request.
fn counting(every: u32) -> Fixture {
    let fixture = Fixture::new();
    fixture.land(
        "symphony.toml",
        Some(&Fixture::project_file(&delivering(&format!(
            "every_landings = {every}\n"
        )))),
        "count every few landings, delivered by pull request",
    );
    fixture
}

/// The branch `slug` made at main's tip with one commit of its own: its head.
fn work(fixture: &Fixture, slug: &str) -> String {
    git(&fixture.main, &["checkout", "-q", "-b", slug, "main"]);
    fixture.write(&format!("{}.txt", slug.replace('/', "-")), slug);
    let head = fixture.commit(&format!("work on {slug}"));
    git(&fixture.main, &["checkout", "-q", "main"]);
    head
}

/// What the host does with the pull request of `slug` at `head`: one squashed commit on the
/// remote's main, which this checkout's main follows. The receipt a land would keep of it, not
/// kept yet.
fn squashed(fixture: &Fixture, slug: &str, head: &str) -> Receipt {
    git(&fixture.main, &["checkout", "-q", "main"]);
    let parent = git(&fixture.main, &["rev-parse", "HEAD"]);
    git(&fixture.main, &["merge", "-q", "--squash", head]);
    git(
        &fixture.main,
        &["commit", "-q", "-m", &format!("{slug}: its title")],
    );
    git(&fixture.main, &["push", "-q", "origin", "main"]);
    Receipt {
        slug: slug.into(),
        task: 7,
        operation: "task".into(),
        repository: 11,
        name: "owner/repository".into(),
        pull: 5,
        head: head.into(),
        base_observed: parent.clone(),
        parent,
        squash: git(&fixture.main, &["rev-parse", "HEAD"]),
        tree: git(&fixture.main, &["rev-parse", "HEAD^{tree}"]),
        revision: "r1".into(),
        by: "lander".into(),
        import: None,
    }
}

/// The lands' folder of the fixture's repository.
fn lands(fixture: &Fixture) -> std::path::PathBuf {
    symphony_board::sweep::trigger::folder(&fixture.main.join(".git"))
        .join(symphony_board::land::FOLDER)
}

/// What the land keeps once it proved the squash: the receipt, and the head under [`RETAINED`].
fn keep(fixture: &Fixture, receipt: &Receipt) {
    let path = Receipt::path(&lands(fixture), &receipt.slug);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, serde_json::to_string_pretty(receipt).unwrap()).unwrap();
    let retained = format!("{RETAINED}{}", receipt.slug);
    git(&fixture.main, &["update-ref", &retained, &receipt.head]);
}

/// `slug` worked on and delivered whole: squashed, its receipt kept.
fn deliver(fixture: &Fixture, slug: &str) -> Receipt {
    let head = work(fixture, slug);
    let receipt = squashed(fixture, slug, &head);
    keep(fixture, &receipt);
    receipt
}

/// `release --due SLUG COMMIT` with the stand-in check, as a landing's command runs it.
fn due(fixture: &Fixture, now: f64, slug: &str, commit: &str) -> String {
    fixture.run(now, &["--check", CHECK, "--due", slug, commit], 0)
}

/// The two cadence lines of `--status`.
fn counted(fixture: &Fixture, now: f64) -> String {
    let status = fixture.run(now, &["--status"], 0);
    (status.lines())
        .filter(|line| line.starts_with("landings: ") || line.starts_with("next run: "))
        .map(|line| format!("{line}\n"))
        .collect()
}

#[test]
fn the_count_takes_a_squash_by_its_receipt_and_a_due_count_starts_one_run_however_often_asked() {
    let fixture = counting(3);
    let merge = cadence::landing(&fixture, "a/one");
    let two = deliver(&fixture, "b/two");
    assert_eq!(
        counted(&fixture, NOW),
        format!(
            "landings: 2 on origin/main since the line's start: no release and no run on record, \
             so every landing counts; the last b/two ({})\n\
             next run: after 1 more landing (2 of 3), or the scheduled run\n",
            &two.squash[..12]
        ),
        "a merge and a squash on one line, the key set at its tip"
    );
    // A squash with no receipt is no landing, whatever its subject says.
    let stray = work(&fixture, "c/stray");
    let unkept = squashed(&fixture, "c/stray", &stray);
    let out = due(&fixture, NOW + 60.0, "c/stray", &unkept.squash);
    assert!(out.contains("landings: 2 on origin/main since "), "{out}");
    assert!(fixture.runs().is_empty(), "{out}");

    let three = deliver(&fixture, "d/three");
    let out = due(&fixture, NOW + 120.0, "d/three", &three.squash);
    for said in [
        format!("release: d/three landed ({})\n", &three.squash[..12]),
        format!(
            "landings: 3 on origin/main since the line's start: no release and no run on record, \
             so every landing counts; the last d/three ({})\n",
            &three.squash[..12]
        ),
        "release: green; tagged release-2026-11-12, pushed to origin\n".to_string(),
    ] {
        assert!(out.contains(&said), "{said}\n\n{out}");
    }
    assert_eq!(fixture.tagged("release-2026-11-12"), three.squash);
    let run = fixture.runs().pop().unwrap();
    assert_eq!(
        run["cause"],
        "3 landings since the line's start: no release and no run on record, so every landing \
         counts, the last d/three"
    );
    // The same landing told again, by a land run again or by hand: counted once, no second run.
    for (at, args) in [
        (180.0, vec!["d/three", three.squash.as_str()]),
        (240.0, vec!["b/two", two.squash.as_str()]),
        (300.0, vec![]),
    ] {
        let mut line = vec!["--check", CHECK, "--due"];
        line.extend(args);
        let out = fixture.run(NOW + at, &line, 0);
        assert!(
            out.ends_with(&format!(
                "landings: 0 on origin/main since release-2026-11-12 ({})\n\
                 next run: after 3 more landings (0 of 3), or the scheduled run\n",
                &three.squash[..12]
            )),
            "{out}"
        );
    }
    assert_eq!(fixture.runs().len(), 1);
    let _ = merge;
}

#[test]
fn a_landing_whose_on_land_never_ran_is_counted_and_its_run_starts_at_the_next_count_asked() {
    let fixture = counting(2);
    let one = deliver(&fixture, "a/one");
    let out = due(&fixture, NOW, "a/one", &one.squash);
    assert!(
        out.ends_with("next run: after 1 more landing (1 of 2), or the scheduled run\n"),
        "{out}"
    );
    // The land of the second is cut after it kept the receipt: the main checkout is not
    // fast-forwarded, the task is not closed, and no `on_land` runs.
    let head = work(&fixture, "b/two");
    let receipt = squashed(&fixture, "b/two", &head);
    keep(&fixture, &receipt);
    git(&fixture.main, &["reset", "-q", "--hard", &receipt.parent]);
    assert!(fixture.runs().is_empty());
    // Nothing counts by itself: with no later landing, the count says it is due and how to ask.
    assert_eq!(
        counted(&fixture, NOW + 60.0),
        format!(
            "landings: 2 on origin/main since the line's start: no release and no run on record, \
             so every landing counts; the last b/two ({})\n\
             next run: due: 2 of 2 landed; the next landing starts it, or `just release --due`\n",
            &receipt.squash[..12]
        )
    );
    assert!(fixture.runs().is_empty(), "a status runs nothing");
    // Asked by hand, with no landing named: the run starts once.
    let out = fixture.run(NOW + 120.0, &["--check", CHECK, "--due"], 0);
    assert!(
        out.contains("release: green; tagged release-2026-11-12, pushed to origin\n"),
        "{out}"
    );
    assert_eq!(fixture.tagged("release-2026-11-12"), receipt.squash);
    // The land run again finishes and tells its landing late: nothing is counted twice.
    git(&fixture.main, &["merge", "-q", "--ff-only", "origin/main"]);
    let out = due(&fixture, NOW + 180.0, "b/two", &receipt.squash);
    assert!(
        out.contains("landings: 0 on origin/main since release-2026-11-12 "),
        "{out}"
    );
    assert_eq!(fixture.runs().len(), 1);
}

#[test]
fn a_later_landing_starts_the_run_a_cut_land_left_due() {
    let fixture = counting(2);
    deliver(&fixture, "a/one");
    deliver(&fixture, "b/two");
    // Neither land told its landing. The third's does, and its count takes all three.
    let three = deliver(&fixture, "c/three");
    let out = due(&fixture, NOW, "c/three", &three.squash);
    assert!(out.contains("landings: 3 on origin/main since "), "{out}");
    assert!(
        out.contains("release: green; tagged release-2026-11-12, pushed to origin\n"),
        "{out}"
    );
    assert_eq!(fixture.runs().len(), 1);
}

#[test]
fn a_receipt_git_does_not_bear_out_stops_the_count_by_name_and_starts_nothing() {
    let fixture = counting(1);
    let one = deliver(&fixture, "a/one");
    let wrong = Receipt {
        tree: one.parent.clone(),
        ..one.clone()
    };
    keep(&fixture, &wrong);
    let (code, out, err) = fixture.release_at(
        NOW,
        &["--check", CHECK, "--due", "a/one", one.squash.as_str()],
    );
    assert_eq!(code, 1, "{out}\n{err}");
    assert!(
        err.contains(&format!(
            "the landings cannot be counted: the receipt of a/one names {} on main, whose parent \
             or tree is not the one it recorded",
            &one.squash[..12]
        )),
        "{err}"
    );
    assert!(fixture.runs().is_empty());
    let status = fixture.run(NOW + 60.0, &["--status"], 0);
    assert!(
        status.contains("landings: cannot be counted: the receipt of a/one names "),
        "{status}"
    );
}

/// The fixture holding release candidates on a line that delivers by pull request.
fn holding() -> Fixture {
    candidate::holding(&delivering(""))
}

/// The run that holds a candidate on this line files its red in the delivered form: the project
/// file main committed names `[land] delivery`, which the run reads as the land does, so the
/// red's fix is rebased onto `release-candidate` and never merges it, and its client fix merges
/// main once, from a second head. The direct form's clauses are not said
/// (`candidate::a_red_run_holds_its_commit_until_its_reds_own_land_answers_it_and_the_rerun_releases_it`
/// is the line that delivers nothing).
#[test]
fn a_candidates_red_on_a_line_delivered_by_pull_request_says_rebase_and_never_merge() {
    let fixture = holding();
    let red = candidate::held(&fixture);
    let body = &fixture.reds()[RED].body;
    for said in [
        format!(
            "The release candidate: this run holds {} as the commit to release",
            &red[..12]
        ),
        "start this task's branch, named for its slug, at `release-candidate`, never at main; \
         never merge `release-candidate`, as this project's land delivers by pull request"
            .into(),
        "is rebased onto `release-candidate` before its assessment and review".into(),
        "Never merge main before its land, but for the one merge of a client fix.".into(),
        "Its delivered land refuses that head and says how it lands".into(),
    ] {
        assert!(body.contains(&said), "{said}\n\n{body}");
    }
    for unsaid in [
        "merge `release-candidate` again",
        "the land keeps main's side of it",
    ] {
        assert!(!body.contains(unsaid), "{unsaid}\n\n{body}");
    }
}

#[test]
fn a_reds_fix_delivered_by_pull_request_advances_the_candidate_to_its_head_and_never_to_main() {
    let fixture = holding();
    let red = candidate::held(&fixture);
    // Main moves under the candidate, by a squash of another task.
    let other = deliver(&fixture, "tools/other");
    let fixed = candidate::fix(&fixture, RED, "red", None);

    // The pull request's merge fails: no squash and no receipt. The candidate waits.
    let out = candidate::due(&fixture, NOW + 60.0, "tools/other", &other.squash, 0);
    assert!(
        out.contains(&format!(
            "next run: held for the release candidate {}: its rerun waits for {RED}",
            &red[..12]
        )),
        "{out}"
    );
    assert_eq!(candidate::record(&fixture)["commit"], red.as_str());

    // Delivered: the squash is on the remote's main and the receipt kept, and the land is cut
    // there, before it closes the task or tells its landing. The next count asked advances the
    // candidate from the receipt alone.
    let receipt = squashed(&fixture, RED, &fixed);
    keep(&fixture, &receipt);
    let runs = fixture.runs().len();
    let out = fixture.run(NOW + 120.0, &["--check", CHECK, "--due"], 0);
    for said in [
        format!(
            "release: advanced the release candidate from {} to {}, the newest approved commit \
             of {RED}'s land ({}), the squash of {} by pull request #5 of owner/repository\n",
            &red[..12],
            &fixed[..12],
            &receipt.squash[..12],
            &fixed[..12]
        ),
        format!(
            "release: due: the rerun of the release candidate {} starts\n",
            &fixed[..12]
        ),
        format!(
            "release: round 2 of the release candidate of {} (origin/main at {}) in ",
            &fixed[..12],
            &receipt.squash[..12]
        ),
        "release: green; tagged release-2026-11-12, pushed to origin\n".into(),
        format!(
            "release: the release candidate {} is released as release-2026-11-12\n",
            &fixed[..12]
        ),
    ] {
        assert!(out.contains(&said), "{said}\n\n{out}");
    }
    assert_eq!(fixture.runs().len(), runs + 1);
    assert_eq!(
        fixture.tagged("release-2026-11-12"),
        fixed,
        "the candidate and its fix: not the squash, which holds the other task"
    );
    assert_ne!(receipt.squash, fixed);
    assert!(
        !fixture.remote_refs().contains("release-candidate"),
        "never pushed: {}",
        fixture.remote_refs()
    );
    assert_eq!(candidate::branch(&fixture), None, "its branch goes with it");
    let record = candidate::record(&fixture);
    assert_eq!(record["state"], "released");
    let advance = &record["advances"][0];
    assert_eq!(
        (&advance["from"], &advance["to"], &advance["merge"]),
        (
            &red.as_str().into(),
            &fixed.as_str().into(),
            &receipt.squash.as_str().into()
        ),
        "both destinations, of one receipt"
    );
    assert_eq!(advance["delivered"]["head"], fixed.as_str());
    assert_eq!(advance["delivered"]["parent"], receipt.parent.as_str());
    assert_eq!(advance["delivered"]["pull"], 5);
    assert_eq!(
        record["reds"][0]["answer"]["merge"],
        receipt.squash.as_str()
    );
    let rerun = fixture.runs().pop().unwrap();
    assert_eq!(
        (&rerun["commit"], &rerun["tip"]),
        (&fixed.as_str().into(), &receipt.squash.as_str().into())
    );

    // Told again once the land run again finishes: nothing runs, and the count goes on from the
    // release on the delivered head, which no history of main holds.
    let out = candidate::due(&fixture, NOW + 180.0, RED, &receipt.squash, 0);
    assert!(
        out.contains(&format!(
            "landings: 2 on origin/main since release-2026-11-12 ({}); the last {RED} ({})\n",
            &fixed[..12],
            &receipt.squash[..12]
        )),
        "what main holds beyond the release: the other task and the fix's squash\n{out}"
    );
    assert_eq!(fixture.runs().len(), runs + 1);

    // Main's tip, which holds the squash and not the head, is released after it: a release only
    // moves forward, and by the receipt this one does.
    let out = fixture.night(NOW + DAY, 0);
    assert!(
        out.contains("release: green; tagged release-2026-11-13, pushed to origin\n"),
        "{out}"
    );
    assert_eq!(fixture.tagged("release-2026-11-13"), receipt.squash);
}

#[test]
fn a_candidate_dropped_before_its_fix_is_delivered_stays_dropped_and_mains_tip_is_run() {
    let fixture = holding();
    let red = candidate::held(&fixture);
    let fixed = candidate::fix(&fixture, RED, "red", None);
    let out = fixture.run(NOW + 60.0, &["--drop"], 0);
    assert!(out.contains("dropped"), "{out}");
    assert_eq!(candidate::record(&fixture)["state"], "dropped");
    assert_eq!(candidate::branch(&fixture), None);
    // The pull request was in flight: its squash lands on main after the drop.
    let receipt = squashed(&fixture, RED, &fixed);
    keep(&fixture, &receipt);
    let out = candidate::due(&fixture, NOW + 120.0, RED, &receipt.squash, 0);
    assert!(
        !out.contains("advanced the release candidate"),
        "a candidate that ended is not advanced: {out}"
    );
    assert!(
        out.contains("release: due: the release candidate was dropped, so a run on origin/main's tip starts\n"),
        "{out}"
    );
    assert!(
        out.contains("release: green; tagged release-2026-11-12, pushed to origin\n"),
        "{out}"
    );
    assert_eq!(
        fixture.tagged("release-2026-11-12"),
        receipt.squash,
        "main's tip, which the squash fixed"
    );
    let record = candidate::record(&fixture);
    assert_eq!(record["state"], "dropped");
    assert_eq!(record["commit"], red.as_str());
    assert_eq!(record["advances"].as_array().map(Vec::len), Some(0));
}

#[test]
fn two_reds_delivered_in_turn_advance_the_candidate_twice_and_it_is_released() {
    const ONE: &str = "tests/release-red-one";
    const TWO: &str = "tests/release-red-two";
    let checks = [
        "--check",
        "one=test ! -e red1",
        "--check",
        "two=test ! -e red2",
    ];
    let run = |fixture: &Fixture, now: f64, more: &[&str], code: i32| {
        fixture.run(now, &[&checks[..], more].concat(), code)
    };
    let fixture = holding();
    fixture.write("red1", "planted");
    let red = fixture.land("red2", Some("planted"), "two red changes");
    let out = run(&fixture, NOW, &[], 1);
    assert!(
        out.contains(&format!(
            "release: holds the release candidate {}",
            &red[..12]
        )),
        "{out}"
    );
    let reds: Vec<String> = (candidate::record(&fixture)["reds"]
        .as_array()
        .unwrap()
        .iter())
    .map(|red| red["slug"].as_str().unwrap().to_string())
    .collect();
    assert_eq!(reds, [ONE, TWO]);

    // The first red's fix, started at the candidate and delivered: the candidate takes its head
    // and waits for the second.
    let one = candidate::fix(&fixture, ONE, "red1", None);
    let first = squashed(&fixture, ONE, &one);
    keep(&fixture, &first);
    let out = run(&fixture, NOW + 60.0, &["--due", ONE, &first.squash], 0);
    assert!(
        out.contains(&format!(
            "release: advanced the release candidate from {} to {}, ",
            &red[..12],
            &one[..12]
        )),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "next run: held for the release candidate {}: its rerun waits for {TWO}",
            &one[..12]
        )),
        "{out}"
    );
    assert_eq!(candidate::branch(&fixture), Some(one.clone()));

    // The second starts at the candidate as it is now, a head main's history does not hold, and
    // merges nothing: a delivery takes a head with no merge of a commit the remote lacks.
    let two = candidate::fix(&fixture, TWO, "red2", None);
    assert_eq!(
        git(&fixture.main, &["rev-parse", &format!("{two}^")]),
        one,
        "on the first fix's head"
    );
    assert_eq!(
        git(
            &fixture.main,
            &["rev-list", "--merges", &format!("origin/main..{two}")]
        ),
        "",
        "no merge commit to deliver"
    );
    let second = squashed(&fixture, TWO, &two);
    keep(&fixture, &second);
    let out = run(&fixture, NOW + 120.0, &["--due", TWO, &second.squash], 0);
    for said in [
        format!(
            "release: advanced the release candidate from {} to {}, the newest approved commit \
             of {TWO}'s land ({}), the squash of {} by pull request #5 of owner/repository\n",
            &one[..12],
            &two[..12],
            &second.squash[..12],
            &two[..12]
        ),
        format!(
            "release: due: the rerun of the release candidate {} starts\n",
            &two[..12]
        ),
        "release: green; tagged release-2026-11-12, pushed to origin\n".into(),
    ] {
        assert!(out.contains(&said), "{said}\n\n{out}");
    }
    assert_eq!(fixture.tagged("release-2026-11-12"), two);
    let record = candidate::record(&fixture);
    assert_eq!(record["state"], "released");
    let advances: Vec<(String, String)> = (record["advances"].as_array().unwrap().iter())
        .map(|advance| {
            let said = |key: &str| advance[key].as_str().unwrap().to_string();
            (said("to"), said("merge"))
        })
        .collect();
    assert_eq!(
        advances,
        [
            (one.clone(), first.squash.clone()),
            (two.clone(), second.squash.clone())
        ]
    );
    // Main's tip holds both squashes and neither head, and is released after it.
    let out = run(&fixture, NOW + DAY, &[], 0);
    assert!(
        out.contains("release: green; tagged release-2026-11-13, pushed to origin\n"),
        "{out}"
    );
    assert_eq!(fixture.tagged("release-2026-11-13"), second.squash);
}
