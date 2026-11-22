use super::*;
use crate::release::waits::far;
use crate::test_tree::Tree;

fn zip(from: &Path, to: &Path) {
    let done = symphony_board::sys::output(
        Command::new("zip")
            .args(["-q", "-r", "--symlinks"])
            .arg(to)
            .arg(".")
            .current_dir(from),
    )
    .unwrap();
    assert!(done.status.success(), "{done:?}");
}

fn listed(name: &str) -> Ran {
    Ran {
        name: name.into(),
        command: "just lint-rs".into(),
        exit_code: Some(0),
        signal: None,
        seconds: 12.0,
        log: PathBuf::from("lint.log"),
        steps: Vec::new(),
        error: None,
        timed_out: false,
        interrupted: false,
        load: None,
        alone: None,
    }
}

#[test]
fn only_the_full_verification_by_its_own_command_is_asked_of_github() {
    assert!(serves(&Step::shell("verify", "just verify")));
    assert!(!serves(&Step::shell("verify", "test ! -e red")));
    assert!(!serves(&Step::shell("lint", "just verify")));
    // A rerun of named steps is another command: it runs here.
    let only = Step::shell("verify", "just verify").only(&["lint".into()]);
    assert!(!serves(&only), "{only:?}");
}

#[test]
fn what_judged_is_said_with_the_run_or_with_why_it_was_this_machine() {
    let here = Hosted::here("verify", "unavailable: GitHub answered 503");
    assert!(!here.there());
    assert_eq!(
        here.said(),
        "verify verified here, not on GitHub: unavailable: GitHub answered 503"
    );
    // A run the hosted clock started is judged by no one then, and its record says so and reads.
    let nowhere = Hosted::nowhere("verify", "unavailable: GitHub answered 503");
    assert!(!nowhere.there());
    assert_eq!(
        nowhere.said(),
        "verify not verified: unavailable: GitHub answered 503; the hosted clock's run verifies \
         nothing here"
    );
    let kept = serde_json::to_value(&nowhere).unwrap();
    assert_eq!(
        kept,
        json!({"check": "verify", "judged": "nowhere",
               "why": "unavailable: GitHub answered 503"})
    );
    assert_eq!(serde_json::from_value::<Hosted>(kept).unwrap(), nowhere);
    let there = Hosted {
        judged: "GitHub".into(),
        run: Some("https://github.com/o/n/actions/runs/1".into()),
        attempt: Some(2),
        workflow_commit: Some("c".repeat(40)),
        why: None,
        ..Hosted::here("verify", "")
    };
    assert!(there.there());
    assert_eq!(
        there.said(),
        "verify verified on GitHub (https://github.com/o/n/actions/runs/1)"
    );
    // A check stopped while its run there was waited for was GitHub's to judge, and is said so.
    let waited = Hosted {
        judged: "GitHub".into(),
        ..Hosted::here(
            "verify",
            "its run there had not ended when this run stopped",
        )
    };
    assert_eq!(
        waited.said(),
        "verify asked of GitHub: its run there had not ended when this run stopped"
    );
    // Its environment is that run's runners: no run here, and no other run there, is on it.
    let on = environment(&there).said();
    assert!(
        on.starts_with("GitHub's runners, https://github.com/o/n/actions/runs/1; "),
        "{on}"
    );
    assert_ne!(
        on,
        environment(&Hosted {
            run: Some("another".into()),
            ..there.clone()
        })
        .said()
    );
    // A record reads back, a run judged here keeping no run's fields.
    let text = serde_json::to_string(&here).unwrap();
    assert_eq!(
        text,
        r#"{"check":"verify","judged":"here","why":"unavailable: GitHub answered 503"}"#
    );
    assert_eq!(serde_json::from_str::<Hosted>(&text).unwrap(), here);
    let text = serde_json::to_string(&there).unwrap();
    assert_eq!(serde_json::from_str::<Hosted>(&text).unwrap(), there);
}

#[test]
fn a_path_inside_a_folder_is_neither_empty_absolute_nor_a_parents() {
    for inside in ["lint.log", "tests-lanes/browser.log", "job-5.log"] {
        assert_eq!(plain(inside), Some(Path::new(inside)), "{inside}");
    }
    for outside in [
        "",
        "/etc/passwd",
        "../lint.log",
        "tests-lanes/../../x",
        "./lint.log",
    ] {
        assert_eq!(plain(outside), None, "{outside}");
    }
}

#[test]
fn each_job_is_a_step_as_github_concluded_it_and_only_a_passed_or_failed_one_keeps_its_list() {
    let tree = Tree::new();
    let job = |name: &str, conclusion: Value, log: Option<&str>| {
        json!({"id": 5, "name": name, "conclusion": conclusion, "seconds": 90,
               "url": "https://github.com/o/n/actions/job/5", "log": log,
               "no_log": "GitHub answered 404 and no download"})
    };
    let answer = json!({"jobs": [
        job(PLAN, json!("failure"), None),
        job("", json!("failure"), None),
        job("lint", json!("success"), Some("job-5.log")),
        job("size-check", json!("failure"), Some("job-5.log")),
        job("tests", json!("cancelled"), Some("../outside.log")),
        job("hosted", json!("failure"), Some("job-5.log")),
        job("docs-audit", json!("success"), None),
        job("crates-check", json!("skipped"), None),
        job("semver-check", Value::Null, None),
    ]});
    let mut steps = vec![
        listed("verify/lint"),
        Ran {
            exit_code: Some(1),
            ..listed("verify/size-check")
        },
        listed("verify/tests"),
    ];
    jobs(&answer, &tree.root, "verify", &mut steps);
    let told: Vec<(&str, Option<i32>, Option<&str>)> = (steps.iter())
        .map(|step| (step.name.as_str(), step.exit_code, step.error.as_deref()))
        .collect();
    assert_eq!(
        told,
        [
            ("verify/lint", Some(0), None),
            ("verify/size-check", Some(1), None),
            (
                "verify/tests",
                None,
                Some("GitHub ended its job cancelled: the step did not run to its end")
            ),
            ("verify/hosted", Some(1), None),
            (
                "verify/docs-audit",
                None,
                Some("its job on GitHub ended success and listed no step: its result is not known")
            ),
            (
                "verify/crates-check",
                None,
                Some("GitHub ended its job skipped: the step did not run to its end")
            ),
            (
                "verify/semver-check",
                None,
                Some("GitHub ended its job no conclusion: the step did not run to its end")
            ),
        ]
    );
    // The plan's job is no step; what the artifacts listed is kept for a job that ended.
    assert_eq!(steps[0].log, Path::new("lint.log"));
    assert!(steps[..2].iter().all(|step| step.command == "just lint-rs"));
    // None that did not end is green, or passed over as a step its check did not need.
    for step in &steps[2..] {
        assert!(!step.green() && !steps::not_run(step), "{step:?}");
    }
    // A job's own log where it has one inside the folder, else a file saying why there is none.
    assert_eq!(steps[3].log, tree.root.join("job-5.log"));
    assert_eq!(steps[3].command, "just verify hosted");
    assert!(steps[3].seconds == 90.0);
    let none = fs::read_to_string(&steps[2].log).unwrap();
    assert_eq!(steps[2].log, tree.root.join("job-5.none.log"));
    assert_eq!(
        none,
        "GitHub gave no log of this job: GitHub answered 404 and no download\n\
         \"https://github.com/o/n/actions/job/5\"\n"
    );
}

#[test]
fn artifacts_are_laid_out_as_a_run_here_lists_its_steps_and_nothing_leaves_the_folder() {
    let tree = Tree::new();
    let (folder, steps) = (
        tree.root.join("verify.hosted"),
        tree.root.join("verify.steps"),
    );
    let made = tree.root.join("made");
    fs::create_dir_all(made.join("tests-lanes")).unwrap();
    fs::create_dir_all(&folder).unwrap();
    // What an earlier check left in the steps' folder is no part of this run's.
    fs::create_dir_all(&steps).unwrap();
    fs::write(steps.join("stale.log"), "old").unwrap();
    fs::write(tree.root.join("secret"), "this machine's").unwrap();
    fs::write(made.join("lint.log"), "lint green\n").unwrap();
    fs::write(made.join("tests-lanes/browser.log"), "browser\n").unwrap();
    std::os::unix::fs::symlink(tree.root.join("secret"), made.join("linked.log")).unwrap();
    let runner = "/Users/runner/work/n/n/build/verify";
    fs::write(
        made.join("steps.tsv"),
        format!(
            "lint\t0\t12\t{runner}/lint.log\tjust lint-rs\t1.50 1.20 1.00 3\n\
             lint\t1\t1\t{runner}/lint.log\tjust lint-rs\n\
             tests\t1\t30\t{runner}/absent.log\ttools/test-lanes.sh\t\n\
             tests/browser\t1\t20\t{runner}/tests-lanes/browser.log\ttools/test-lanes.sh browser\n\
             linked\t0\t1\t{runner}/linked.log\tjust linked\n\
             parent\t0\t1\t{runner}/../../secret\tjust parent\n\
             elsewhere\t0\t1\t/etc/passwd\tjust elsewhere\n\
             short\t0\t1\n"
        ),
    )
    .unwrap();
    zip(&made, &folder.join("artifact-7.zip"));
    let answer = json!({"artifacts": [
        {"id": 7, "name": "verify-lint", "file": "artifact-7.zip"},
        {"id": 8, "name": "coverage", "file": "artifact-7.zip"},
        {"id": 9, "name": "verify-../x", "file": "artifact-7.zip"},
        {"id": 10, "name": "verify-tests", "file": "../artifact-7.zip"},
    ]});
    lay_out(&answer, &folder, &steps, Within::bound(far())).unwrap();
    let list = fs::read_to_string(steps.join(steps::FILE)).unwrap();
    assert_eq!(
        list,
        "lint\t0\t12\tlint.log\tjust lint-rs\t1.50 1.20 1.00 3\n\
         tests\t1\t30\ttests.missing.log\ttools/test-lanes.sh\t\n\
         tests/browser\t1\t20\ttests-lanes/browser.log\ttools/test-lanes.sh browser\n\
         linked\t0\t1\tlinked.missing.log\tjust linked\n\
         parent\t0\t1\tparent.missing.log\tjust parent\n\
         elsewhere\t0\t1\telsewhere.missing.log\tjust elsewhere\n",
        "each step once, its log by its place in the folder or a file saying it has none"
    );
    assert!(!steps.join("stale.log").exists());
    assert_eq!(
        fs::read_to_string(steps.join("lint.log")).unwrap(),
        "lint green\n"
    );
    assert_eq!(
        fs::read_to_string(steps.join("tests-lanes/browser.log")).unwrap(),
        "browser\n"
    );
    assert_eq!(
        fs::read_to_string(steps.join("linked.missing.log")).unwrap(),
        format!("the artifact verify-lint holds no log at {runner}/linked.log\n"),
        "an archive's link is followed nowhere"
    );
    for file in fs::read_dir(&steps).unwrap() {
        let text = fs::read_to_string(file.unwrap().path()).unwrap_or_default();
        assert!(!text.contains("this machine's"), "{text}");
    }
    // The list reads as a run's own, the lane a part of its step.
    let read = steps::read(&steps, "verify").unwrap();
    assert_eq!(read.len(), 5);
    assert_eq!(read[1].steps[0].name, "verify/tests/browser");
    assert_eq!(read[0].log, steps.join("lint.log"));

    // An archive that does not open is said, and an artifact with no list lists nothing.
    fs::write(folder.join("broken.zip"), "not an archive").unwrap();
    let broken = json!({"artifacts": [{"name": "verify-lint", "file": "broken.zip"}]});
    let why = lay_out(&broken, &folder, &steps, Within::bound(far())).unwrap_err();
    assert!(
        why.starts_with("the artifact verify-lint does not open: unzip: "),
        "{why}"
    );
    fs::remove_file(made.join("steps.tsv")).unwrap();
    zip(&made, &folder.join("unlisted.zip"));
    let unlisted = json!({"artifacts": [{"name": "verify-size-check", "file": "unlisted.zip"}]});
    lay_out(&unlisted, &folder, &steps, Within::bound(far())).unwrap();
    assert_eq!(fs::read_to_string(steps.join(steps::FILE)).unwrap(), "");
    lay_out(&json!({}), &folder, &steps, Within::bound(far())).unwrap();
    assert_eq!(
        steps::read(&steps, "verify").unwrap().len(),
        0,
        "{:?}",
        steps::read(&steps, "verify").unwrap()
    );
}

#[test]
fn the_downloads_of_a_check_are_in_a_folder_of_its_own_among_the_runs_logs() {
    assert_eq!(
        folder(Path::new("/logs/1"), "verify"),
        Path::new("/logs/1/verify.hosted")
    );
    assert_ne!(
        folder(Path::new("/logs/1"), "verify"),
        steps::folder(Path::new("/logs/1"), "verify")
    );
}

/// A run's commit not held here is fetched for once, and a fetch that fails is said where it
/// failed, on the run's `err`, before the run is judged by what is held: this is the one site of
/// [`super::unsaid`] in this file.
#[test]
fn a_fetch_for_a_commit_not_held_that_fails_is_said_and_the_run_is_not_believed() {
    let tree = Tree::new();
    let root = &tree.root;
    git(root, &["init", "-q", "-b", "main"]).unwrap();
    tree.write(WORKFLOW, "name: Release verification\n");
    let identity = ["-c", "user.name=t", "-c", "user.email=t@example.com"];
    git(root, &["add", WORKFLOW]).unwrap();
    git(
        root,
        &[&identity[..], &["commit", "-q", "-m", "workflow"]].concat(),
    )
    .unwrap();
    let tip = git(root, &["rev-parse", "HEAD"]).unwrap();
    // No remote `origin` here, so the fetch fails at once, reaching no network.
    let hosting = Hosting {
        root,
        main: root,
        command: &[],
        commit: &tip,
        tip: &tip,
        remote: "origin",
        branch: "main",
        by: "release-run",
    };
    let absent = "f".repeat(40);
    let mut err = Vec::new();
    assert_eq!(
        hosting.committed(&absent, Within::bound(far()), &mut err),
        Some(format!(
            "its run ran the {WORKFLOW} of {}, a commit origin/main does not hold",
            short(&absent)
        ))
    );
    let said = String::from_utf8(err).unwrap();
    let head = format!(
        "symphony-build release: origin/main could not be fetched for the commit {}: ",
        short(&absent)
    );
    assert!(
        said.starts_with(&head) && said.len() > head.len() && said.lines().count() == 1,
        "{said}"
    );

    // A commit held here is not fetched for: nothing is said, and its file is main's.
    let mut err = Vec::new();
    assert_eq!(
        hosting.committed(&tip, Within::bound(far()), &mut err),
        None
    );
    assert!(err.is_empty(), "{}", String::from_utf8_lossy(&err));
}

#[test]
fn the_workflow_main_commits_names_its_plan_job_and_artifacts_as_they_are_read_here() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(WORKFLOW);
    let workflow = fs::read_to_string(&file).unwrap();
    for said in [
        format!("    name: {PLAN}\n"),
        format!("          name: {ARTIFACT}${{{{ matrix.step }}}}\n"),
        "    name: ${{ matrix.step }}\n".to_string(),
        format!("          path: {}\n", FOLDER.trim_start_matches('/')),
    ] {
        assert!(workflow.contains(&said), "{said:?} in {}", file.display());
    }
}

/// What [`Hosting::verify`] judged for the command `command`, with the run's time up at `until`.
fn judged_by(tree: &Tree, command: &[&str], bound: crate::release::group::Bound<'_>) -> Judged {
    let command: Vec<String> = command.iter().map(ToString::to_string).collect();
    let commit = "c".repeat(40);
    let hosting = Hosting {
        root: &tree.root,
        main: &tree.root,
        command: &command,
        commit: &commit,
        tip: &commit,
        remote: "origin",
        branch: "main",
        by: "release-run",
    };
    let (name, command) = crate::release::VERIFY;
    hosting.verify(
        &Step::shell(name, command),
        &tree.root,
        &tree.root.join("verify.log"),
        Within::bound(bound),
    )
}

/// The check timed out on GitHub's side, asked there and never verified here.
fn timed_out(judged: Judged, why: &str, log: &str) {
    let Judged::There(ran, hosted) = judged else {
        panic!("a run whose time is up is verified nowhere else");
    };
    assert!(ran.timed_out && !ran.interrupted && !ran.green(), "{ran:?}");
    assert_eq!(ran.error, None);
    assert_eq!(hosted.judged, "GitHub");
    assert_eq!(hosted.run, None);
    assert_eq!(hosted.why.as_deref(), Some(why));
    assert_eq!(
        hosted.said(),
        format!("verify asked of GitHub: {why}"),
        "never said as verified"
    );
    assert!(
        log.contains("a run it started on GitHub is left to end, and writes nothing"),
        "{log}"
    );
}

// The bound is past before the command starts, so no load decides which ends first.
#[test]
fn a_command_still_waiting_when_the_runs_time_is_up_is_the_check_timed_out_and_nothing_here() {
    let tree = Tree::new();
    let past = crate::release::group::Bound::until(std::time::Instant::now());
    let judged = judged_by(&tree, &["sleep", "600"], past);
    let log = fs::read_to_string(tree.root.join("verify.log")).unwrap();
    timed_out(
        judged,
        "its run there had not ended when this run stopped",
        &log,
    );
    assert!(log.contains("the run's time was up"), "{log}");
}

#[test]
fn a_command_that_answers_late_is_the_check_timed_out_and_nothing_here() {
    let tree = Tree::new();
    let script = r#"echo '{"state":"late","why":"the run 9000 had not ended within 60 s"}'"#;
    let judged = judged_by(&tree, &["sh", "-c", script, "adapter"], far());
    let log = fs::read_to_string(tree.root.join("verify.log")).unwrap();
    timed_out(
        judged,
        "its run there had not ended in time: the run 9000 had not ended within 60 s",
        &log,
    );
    assert!(
        log.contains("the run 9000 had not ended within 60 s; a run it started"),
        "{log}"
    );
}
