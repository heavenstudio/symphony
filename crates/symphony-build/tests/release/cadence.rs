//! The release cadence counted in landings: `--due`, which a landing's `on_land` command runs,
//! starts a run on main's tip once `[releases] every_landings` tasks have landed since the last
//! release or the last run that checked something, never while a run is in progress; `--status`
//! says the count and what the next run waits for.

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::time::Duration;

use symphony_board::sys::Stdio;

use super::{CHECK, DAY, Fixture, NOW, git};

/// The fixture counting every `every` landings.
pub(super) fn counting(every: u32) -> Fixture {
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

/// `release --due SLUG COMMIT` with the stand-in check at `now`, as a landing's command runs it,
/// which must end with `code`: its stdout.
fn due(fixture: &Fixture, now: f64, landed: &str, code: i32) -> String {
    fixture.run(
        now,
        &["--check", CHECK, "--due", "tools/last", landed],
        code,
    )
}

/// The two cadence lines `--status` ends its run lines with.
fn cadence_lines(fixture: &Fixture, now: f64) -> String {
    let status = fixture.run(now, &["--status"], 0);
    status
        .lines()
        .filter(|line| line.starts_with("landings: ") || line.starts_with("next run: "))
        .map(|line| format!("{line}\n"))
        .collect()
}

/// The line the counts say from the line's start, with no release and no run on record.
const START: &str = "the line's start: no release and no run on record, so every landing counts";

#[test]
fn a_night_starts_once_enough_tasks_have_landed_since_the_last_release() {
    let fixture = counting(3);
    assert_eq!(
        cadence_lines(&fixture, NOW),
        format!(
            "landings: 0 on origin/main since {START}\n\
             next run: after 3 more landings (0 of 3), or the scheduled run\n"
        )
    );
    landing(&fixture, "tools/one");
    let second = landing(&fixture, "tools/two");
    let out = due(&fixture, NOW, &second, 0);
    assert!(
        out.starts_with(&format!(
            "release: tools/last landed ({})\nlandings: 2 on origin/main since {START}; the last \
             tools/two ({})\nnext run: after 1 more landing (2 of 3), or the scheduled run\n",
            &second[..12],
            &second[..12]
        )),
        "{out}"
    );
    assert!(!out.contains("release: due"), "{out}");
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
            "next run: due: 3 of 3 landed; the next landing starts it, or `just release --due`\n"
        ),
        "{out}"
    );
    // The first count says so: it takes the line's whole history at once.
    assert!(
        out.contains(
            "release: due: the first count, 3 landings from origin/main's start with no release \
             and no run on record: a run on its tip starts now\n"
        ),
        "{out}"
    );
    assert!(
        out.contains("release: green; tagged release-2026-11-12, pushed to origin"),
        "{out}"
    );
    assert_eq!(fixture.remote_tags(), ["release-2026-11-12"]);
    assert_eq!(fixture.tagged("release-2026-11-12"), third);
    let run = fixture.runs().pop().unwrap();
    assert_eq!(run["kind"], "release", "a run like the schedule's");
    assert_eq!(
        run["cause"],
        format!("3 landings since {START}, the last tools/three")
    );

    // The count starts again after the release, and a landing short of it runs nothing.
    let status = fixture.run(NOW + 60.0, &["--status"], 0);
    let tagged = &third[..12];
    assert!(
        status.contains(&format!(
            "landings: 0 on origin/main since release-2026-11-12 ({tagged})\nnext run: after 3 \
             more landings (0 of 3), or the scheduled run\n"
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
    assert!(out.contains("release: red: verify; no release"), "{out}");
    assert!(fixture.remote_tags().is_empty());
    let run_id = fixture.runs().pop().unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        fixture.reds().keys().collect::<Vec<_>>(),
        ["tests/release-red-verify"]
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
        out.contains("release: green; tagged release-2026-11-13"),
        "{out}"
    );
    assert!(
        out.contains("release: closed tests/release-red-verify"),
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
        "kind": "release",
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

    // The lock held with no run saying so: the count is due, but nothing starts beside it.
    fs::remove_file(fixture.records().join("running.json")).unwrap();
    let out = due(&fixture, NOW + 90.0, &landed, 0);
    assert!(
        out.ends_with(
            "release: a run holds the lock: the count waits for it, and nothing more runs\n"
        ),
        "{out}"
    );
    assert!(fixture.runs().is_empty(), "nothing ran beside it: {out}");
    assert!(!fixture.worktree().exists(), "nor was its checkout touched");

    // Once it ends, the count is due and the next landing's command starts the run.
    lock.unlock().unwrap();
    let out = due(&fixture, NOW + 120.0, &landed, 0);
    assert!(
        out.contains("release: green; tagged release-2026-11-12"),
        "{out}"
    );
}

#[test]
fn a_night_run_by_hand_holds_the_lock_so_a_due_count_starts_no_second() {
    let fixture = counting(1);
    let landed = landing(&fixture, "tools/one");
    let go = fixture.root.join("go");
    let check = format!(
        "verify=while [ ! -e '{}' ]; do sleep 0.1; done",
        go.display()
    );
    let mut by_hand = fixture
        .command(NOW, &["--check", &check], &[])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // Its check runs: the run says so beside the lock it holds.
    let running = fixture.records().join("running.json");
    test_time::until_true(
        "the run said it runs its check",
        test_time::backstop(Duration::from_secs(60)),
        || fs::read_to_string(&running).is_ok_and(|text| text.contains("\"verify\"")),
    )
    .unwrap_or_else(|stall| panic!("{stall}"));
    let out = due(&fixture, NOW + 60.0, &landed, 0);
    assert!(
        out.contains("next run: in progress: the run ") && !out.contains("release: due"),
        "{out}"
    );
    fs::write(&go, "").unwrap();
    assert!(by_hand.wait().unwrap().success());
    assert_eq!(fixture.runs().len(), 1, "one run, the one run by hand");
    // Its release is what the next count starts from: nothing is due.
    let out = due(&fixture, NOW + 120.0, &landed, 0);
    assert!(
        out.contains("landings: 0 on origin/main since release-2026-11-12"),
        "{out}"
    );
    assert_eq!(fixture.runs().len(), 1);
}

/// Stand-ins for `cargo`, `rustc` and `just` first on a `PATH`, each logging its run to
/// `spied.log` and failing: the `PATH`.
fn spies(fixture: &Fixture) -> String {
    let bin = fixture.root.join("spies");
    fs::create_dir_all(&bin).unwrap();
    for name in ["cargo", "rustc", "just"] {
        let path = bin.join(name);
        fs::write(
            &path,
            format!(
                "#!/bin/sh\necho \"$0 $*\" >> '{}'\nexit 1\n",
                fixture.root.join("spied.log").display()
            ),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

#[test]
fn a_count_short_of_due_builds_nothing_and_a_due_one_builds_in_the_release_runs_own_checkout() {
    let fixture = counting(2);
    let path = spies(&fixture);
    // The tip's launcher, a stand-in: it says what it was asked to build, where and into which
    // build folder, and names this test's own build as what it built.
    let built = fixture.root.join("built.log");
    fixture.write(
        "tools/rs-bin.sh",
        &format!(
            "#!/bin/sh\necho \"$* in $(pwd -P) into ${{CARGO_TARGET_DIR:-its own .target}}\" >> \
             '{}'\necho '{}'\n",
            built.display(),
            super::BIN
        ),
    );
    fs::set_permissions(
        fixture.main.join("tools/rs-bin.sh"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    fixture.commit("its launcher");
    git(&fixture.main, &["push", "-q", "origin", "main"]);
    let extra = [
        ("PATH", path.as_str()),
        ("CARGO_TARGET_DIR", "/nowhere/target"),
    ];

    let first = landing(&fixture, "tools/one");
    let args = ["--check", CHECK, "--due", "tools/one", &first];
    let (code, out, err) = fixture.release_with(NOW, &args, &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains("next run: after 1 more landing (1 of 2)"),
        "{out}"
    );
    assert!(
        !fixture.root.join("spied.log").exists(),
        "no cargo, rustc or just ran"
    );
    assert!(!built.exists(), "nothing was built");
    assert!(
        !fixture.worktree().exists(),
        "the release run's checkout was not even made"
    );

    let second = landing(&fixture, "tools/two");
    let args = ["--check", CHECK, "--due", "tools/two", &second];
    let (code, out, err) = fixture.release_with(NOW, &args, &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(
        fs::read_to_string(&built).unwrap(),
        format!(
            "--build symphony-build in {} into its own .target\n",
            fixture.worktree().display()
        )
    );
    assert!(
        out.contains(&format!(
            "release: the run goes on from {}, built in its own checkout\n",
            super::BIN
        )),
        "{out}"
    );
    assert!(
        out.contains("release: green; tagged release-2026-11-12"),
        "{out}"
    );
    assert!(
        !fixture.root.join("spied.log").exists(),
        "no cargo, rustc or just ran here"
    );
    assert!(
        !fixture.main.join(".target").exists(),
        "the main checkout's build folder is never touched"
    );
    assert_eq!(fixture.runs().len(), 1, "one run");
    assert!(!fixture.records().join("running.json").exists());
}

#[test]
fn a_due_tip_that_does_not_build_says_why_and_its_night_runs_from_the_linked_build() {
    let fixture = counting(1);
    fixture.write(
        "tools/rs-bin.sh",
        "#!/bin/sh\necho 'error: the tip does not build' >&2\nexit 1\n",
    );
    fs::set_permissions(
        fixture.main.join("tools/rs-bin.sh"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    fixture.commit("a launcher that fails");
    git(&fixture.main, &["push", "-q", "origin", "main"]);

    let landed = landing(&fixture, "tools/one");
    let args = ["--check", CHECK, "--due", "tools/one", &landed];
    let (code, out, err) = fixture.release_at(NOW, &args);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains(&format!(
            "error: the tip does not build\nrelease: the run goes on from this build: the build of \
             its tip in {} failed (exit status: 1)\n",
            fixture.worktree().display()
        )),
        "{out}"
    );
    assert!(
        out.contains("release: green; tagged release-2026-11-12"),
        "{out}"
    );
    assert_eq!(fixture.runs().len(), 1, "one run");
    assert!(!fixture.records().join("running.json").exists());
}

/// A fixture counting each landing whose tip's launcher names a build that cannot be started: the
/// build it names.
fn unstartable() -> (Fixture, std::path::PathBuf) {
    let fixture = counting(1);
    let built = fixture.root.join("built");
    fs::write(&built, "a file nobody may execute").unwrap();
    fixture.write(
        "tools/rs-bin.sh",
        &format!("#!/bin/sh\necho '{}'\n", built.display()),
    );
    fs::set_permissions(
        fixture.main.join("tools/rs-bin.sh"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    fixture.commit("a launcher whose build cannot start");
    git(&fixture.main, &["push", "-q", "origin", "main"]);
    (fixture, built)
}

/// The run goes on in the process whose hand-off failed, so that start must not have made it the
/// program it did not become (`replace`, whose own test holds its signals and environment to that).
#[test]
fn a_due_tips_build_that_cannot_be_started_says_why_and_its_night_runs_from_the_linked_build() {
    let (fixture, built) = unstartable();

    let landed = landing(&fixture, "tools/one");
    let args = ["--check", CHECK, "--due", "tools/one", &landed];
    let (code, out, err) = fixture.release_at(NOW, &args);
    assert_eq!(code, 0, "{out}{err}");
    let handed = format!(
        "release: the run goes on from {0}, built in its own checkout\nrelease: {0} cannot run \
         (Permission denied (os error 13)): the run goes on from this build\n",
        built.display()
    );
    assert!(out.contains(&handed), "{out}");
    assert!(
        out.contains("release: green; tagged release-2026-11-12"),
        "{out}"
    );
    assert_eq!(
        fixture.runs().len(),
        1,
        "one run, of the build that counted"
    );
    assert!(!fixture.records().join("running.json").exists());
}

/// The same run, what it says going to a pipe nobody reads: a start made as std's `exec` makes it
/// returns with SIGPIPE back at its default, and the line that says the start failed then kills
/// the run before it records anything. Left ignored, as Rust's runtime set it, each such write
/// fails and the run goes on to its release.
#[test]
fn a_run_that_goes_on_after_a_start_that_failed_still_outlives_a_pipe_nobody_reads() {
    use std::os::unix::process::ExitStatusExt as _;

    let (fixture, _) = unstartable();
    let landed = landing(&fixture, "tools/one");
    let args = ["--check", CHECK, "--due", "tools/one", &landed];
    let (reader, said) = std::io::pipe().unwrap();
    drop(reader);
    let ended = fixture
        .command(NOW, &args, &[])
        .stdin(Stdio::null())
        .stdout(said)
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(
        (ended.signal(), ended.code()),
        (None, Some(0)),
        "SIGPIPE is {}",
        libc::SIGPIPE
    );
    let runs = fixture.runs();
    assert_eq!(runs.len(), 1, "the run it went on with is recorded");
    assert_eq!(runs[0]["result"], "green", "{}", runs[0]);
    assert_eq!(fixture.remote_tags(), ["release-2026-11-12"]);
}

#[test]
fn a_cadence_of_none_starts_no_run_and_says_the_night_still_runs() {
    let fixture = counting(0);
    let landed = landing(&fixture, "tools/one");
    let out = due(&fixture, NOW, &landed, 0);
    assert!(
        out.contains(
            "next run: off ([releases] every_landings = 0): only the scheduled run or `just \
             release`\n"
        ),
        "{out}"
    );
    assert!(fixture.runs().is_empty());

    // With no line to count on, it says so and runs nothing.
    let (code, _, err) = fixture.release_at(NOW, &["--due", "--branch", "trunk"]);
    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains("the landings cannot be counted: neither origin/trunk nor trunk"),
        "{err}"
    );
}
