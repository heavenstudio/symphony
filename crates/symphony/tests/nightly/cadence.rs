//! The release cadence counted in landings: `--due`, which a landing's `on_land` command runs,
//! starts a night on main's tip once `[releases] every_landings` tasks have landed since the last
//! release or the last run that checked something, never while a run is in progress; `--status`
//! says the count and what the next run waits for.

use std::fs;

use super::{CHECK, DAY, Fixture, NOW, git};

/// The fixture counting every `every` landings.
fn counting(every: u32) -> Fixture {
    let fixture = Fixture::new();
    fixture.land(
        "symphony.toml",
        Some(&Fixture::project_file(&format!(
            "every_landings = {every}\n"
        ))),
        "count every few landings",
    );
    fixture
}

/// Land the task `slug` as `just land` does: its branch merged on main with the land's message,
/// and pushed. The merge.
pub(super) fn landing(fixture: &Fixture, slug: &str) -> String {
    landing_with(
        fixture,
        slug,
        &format!("{}.txt", slug.replace('/', "-")),
        Some(slug),
    )
}

/// The same, its branch writing `file` with `text` (removing it with none).
fn landing_with(fixture: &Fixture, slug: &str, file: &str, text: Option<&str>) -> String {
    git(&fixture.main, &["checkout", "-q", "-b", slug]);
    match text {
        Some(text) => fixture.write(file, text),
        None => fs::remove_file(fixture.main.join(file)).unwrap(),
    }
    fixture.commit(&format!("work on {slug}"));
    git(&fixture.main, &["checkout", "-q", "main"]);
    git(
        &fixture.main,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            &format!("Merge branch '{slug}'"),
            slug,
        ],
    );
    git(&fixture.main, &["push", "-q", "origin", "main"]);
    git(&fixture.main, &["rev-parse", "HEAD"])
}

/// `nightly --due SLUG COMMIT` with the stand-in check at `now`, as a landing's command runs it,
/// which must end with `code`: its stdout.
fn due(fixture: &Fixture, now: f64, landed: &str, code: i32) -> String {
    fixture.run(
        now,
        &["--check", CHECK, "--due", "tools/last", landed],
        code,
    )
}

/// The two cadence lines `--status` ends its night lines with.
fn cadence_lines(fixture: &Fixture, now: f64) -> String {
    let status = fixture.run(now, &["--status"], 0);
    status
        .lines()
        .filter(|line| line.starts_with("landings: ") || line.starts_with("next run: "))
        .map(|line| format!("{line}\n"))
        .collect()
}

#[test]
fn a_night_starts_once_enough_tasks_have_landed_since_the_last_release() {
    let fixture = counting(3);
    assert_eq!(
        cadence_lines(&fixture, NOW),
        "landings: 0 on origin/main since the line's start, with no release yet\n\
         next run: after 3 more landings (0 of 3), or the night's run\n"
    );
    landing(&fixture, "tools/one");
    let second = landing(&fixture, "tools/two");
    let out = due(&fixture, NOW, &second, 0);
    assert!(
        out.starts_with(&format!(
            "nightly: tools/last landed ({})\nlandings: 2 on origin/main since the line's start, \
             with no release yet; the last tools/two ({})\nnext run: after 1 more landing (2 of \
             3), or the night's run\n",
            &second[..12],
            &second[..12]
        )),
        "{out}"
    );
    assert!(!out.contains("nightly: due"), "{out}");
    assert!(
        fixture.runs().is_empty(),
        "short of the count, nothing runs"
    );
    assert!(fixture.remote_tags().is_empty());

    // A plain commit is no landing.
    fixture.land("notes.txt", Some("a note\n"), "a generator's commit");
    let third = landing(&fixture, "tools/three");
    let out = due(&fixture, NOW, &third, 0);
    assert!(
        out.contains(
            "next run: due: 3 of 3 landed; the next landing starts it, or `just nightly --due`\n"
        ),
        "{out}"
    );
    assert!(
        out.contains("nightly: due: a night on origin/main's tip starts\n"),
        "{out}"
    );
    assert!(
        out.contains("nightly: green; tagged release-2026-11-12, pushed to origin"),
        "{out}"
    );
    assert_eq!(fixture.remote_tags(), ["release-2026-11-12"]);
    assert_eq!(fixture.tagged("release-2026-11-12"), third);
    let run = fixture.runs().pop().unwrap();
    assert_eq!(run["kind"], "nightly", "a night like the schedule's");
    assert_eq!(
        run["cause"],
        "3 landings since the line's start, with no release yet, the last tools/three"
    );

    // The count starts again after the release, and a landing short of it runs nothing.
    let status = fixture.run(NOW + 60.0, &["--status"], 0);
    let tagged = &third[..12];
    assert!(
        status.contains(&format!(
            "landings: 0 on origin/main since release-2026-11-12 ({tagged})\nnext run: after 3 \
             more landings (0 of 3), or the night's run\n"
        )),
        "{status}"
    );
    let fourth = landing(&fixture, "tools/four");
    let out = due(&fixture, NOW + 120.0, &fourth, 0);
    assert!(
        out.contains(&format!(
            "landings: 1 on origin/main since release-2026-11-12 ({tagged}); the last tools/four"
        )),
        "{out}"
    );
    assert_eq!(fixture.runs().len(), 1, "{out}");
}

#[test]
fn a_red_run_is_counted_from_and_does_not_hold_the_next_count() {
    let fixture = counting(2);
    landing(&fixture, "tools/one");
    let red = landing_with(&fixture, "tools/red", "red", Some("red\n"));
    let out = due(&fixture, NOW, &red, 1);
    assert!(out.contains("nightly: red: verify; no release"), "{out}");
    assert!(fixture.remote_tags().is_empty());
    let run_id = fixture.runs().pop().unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        fixture.reds().keys().collect::<Vec<_>>(),
        ["tests/nightly-red-verify"]
    );

    // Asked again at once, the red run is what the count starts from: no second run of it.
    let out = due(&fixture, NOW + 60.0, &red, 0);
    assert!(
        out.contains(&format!(
            "landings: 0 on origin/main since the run {run_id} on {} (red)\n",
            &red[..12]
        )),
        "{out}"
    );
    assert_eq!(fixture.runs().len(), 1);

    let fixed = landing_with(&fixture, "tools/fix", "red", None);
    let out = due(&fixture, NOW + 120.0, &fixed, 0);
    assert!(
        out.contains("next run: after 1 more landing (1 of 2)"),
        "{out}"
    );
    let next = landing(&fixture, "tools/next");
    let out = due(&fixture, NOW + DAY, &next, 0);
    assert!(
        out.contains("nightly: green; tagged release-2026-11-13"),
        "{out}"
    );
    assert!(
        out.contains("nightly: closed tests/nightly-red-verify"),
        "{out}"
    );
}

#[test]
fn a_run_in_progress_is_never_doubled() {
    let fixture = counting(1);
    let landed = landing(&fixture, "tools/one");
    fs::create_dir_all(fixture.records()).unwrap();
    let lock = fs::File::create(fixture.records().join("run.lock")).unwrap();
    lock.lock().unwrap();
    let running = serde_json::json!({
        "id": "another",
        "kind": "nightly",
        "pid": std::process::id(),
        "started_at": NOW,
        "until": NOW + 3600.0,
    });
    fs::write(fixture.records().join("running.json"), running.to_string()).unwrap();
    let out = due(&fixture, NOW + 60.0, &landed, 0);
    assert!(
        out.ends_with(&format!(
            "next run: in progress: the run another (process {}, since 2026-11-12 10:00); the \
             count waits for it\n",
            std::process::id()
        )),
        "{out}"
    );
    assert!(fixture.runs().is_empty(), "nothing ran beside it: {out}");
    let status = fixture.run(NOW + 60.0, &["--status"], 0);
    assert!(
        status.contains("next run: in progress: the run another"),
        "{status}"
    );

    // Once it ends, the count is due and the next landing's command starts the night.
    fs::remove_file(fixture.records().join("running.json")).unwrap();
    lock.unlock().unwrap();
    let out = due(&fixture, NOW + 120.0, &landed, 0);
    assert!(
        out.contains("nightly: green; tagged release-2026-11-12"),
        "{out}"
    );
}

#[test]
fn a_cadence_of_none_starts_no_run_and_says_the_night_still_runs() {
    let fixture = counting(0);
    let landed = landing(&fixture, "tools/one");
    let out = due(&fixture, NOW, &landed, 0);
    assert!(
        out.contains(
            "next run: off ([releases] every_landings = 0): only the night's run or `just \
             nightly`\n"
        ),
        "{out}"
    );
    assert!(fixture.runs().is_empty());

    // With no line to count on, it says so and runs nothing.
    let (code, _, err) = fixture.nightly_at(NOW, &["--due", "--branch", "trunk"]);
    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains("the landings cannot be counted: neither origin/trunk nor trunk"),
        "{err}"
    );
}
