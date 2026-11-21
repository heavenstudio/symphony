//! A release run of a project that names `[releases] hosted` asks that command to verify its
//! commit on GitHub's machines, through a stand-in for the GitHub adapter that answers as it
//! does: a run there that is believed judges the release, each job a step with the log its
//! artifact holds, a red step filed as a run here files it, and nothing verified here; whatever is
//! no such answer is said and the commit is verified here, the record saying which judged. A
//! project that names no command, and a check named by hand, ask nothing.

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use symphony_board::sys::{Command, output};

use super::{Fixture, NOW};

pub(super) const URL: &str = "https://github.com/o/n/actions/runs/9000";
pub(super) const WORKFLOW: &str = ".github/workflows/release-verify.yml";

/// A stand-in for `symphony-github verify` in a folder of the fixture's own, run as `stand-in
/// verify --request FILE`: it keeps each request (`asked.jsonl`) with the folder it was run in
/// (`cwd.log`), copies its `downloads` into the folder the request names, and prints its
/// `answer`, `COMMIT` in it the request's commit; while the file `fails` is beside it, it fails.
pub(super) struct Stand {
    pub(super) folder: PathBuf,
    program: PathBuf,
}

impl Stand {
    pub(super) fn new(fixture: &Fixture) -> Stand {
        let folder = fixture.root.join("hosted-stand-in");
        fs::create_dir_all(folder.join("downloads")).unwrap();
        let program = folder.join("stand-in");
        fs::write(
            &program,
            format!(
                r#"#!/bin/sh
here='{folder}'
[ "$1" = verify ] && [ "$2" = --request ] || {{ echo "usage" >&2; exit 2; }}
cat "$3" >> "$here/asked.jsonl"
pwd >> "$here/cwd.log"
if [ -e "$here/fails" ]; then echo "symphony-github: no working directory" >&2; exit 2; fi
commit=$(sed -n 's/.*"commit":"\([^"]*\)".*/\1/p' "$3")
to=$(sed -n 's/.*"folder":"\([^"]*\)".*/\1/p' "$3")
cp -R "$here/downloads/." "$to/"
sed "s/COMMIT/$commit/g" "$here/answer"
"#,
                folder = folder.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        Stand { folder, program }
    }

    /// The project file's `[releases]` line that names the stand-in.
    pub(super) fn setting(&self) -> String {
        format!("hosted = [\"{}\", \"verify\"]\n", self.program.display())
    }

    pub(super) fn answer(&self, answer: &Value) {
        fs::write(self.folder.join("answer"), format!("{answer}\n")).unwrap();
    }

    pub(super) fn asked(&self) -> Vec<Value> {
        fs::read_to_string(self.folder.join("asked.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    /// A job's log among its downloads.
    pub(super) fn log(&self, name: &str, text: &str) {
        fs::write(self.folder.join("downloads").join(name), text).unwrap();
    }

    /// An artifact among its downloads, as the workflow uploads a step's `build/verify`: `list`
    /// its `steps.tsv`, and each of `files` a file in it.
    pub(super) fn artifact(&self, name: &str, list: &str, files: &[(&str, &str)]) {
        let made = self.folder.join("made").join(name);
        fs::create_dir_all(&made).unwrap();
        fs::write(made.join("steps.tsv"), list).unwrap();
        for (file, text) in files {
            let path = made.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        let archive = self.folder.join("downloads").join(name);
        let _ = fs::remove_file(&archive);
        let done = output(
            Command::new("zip")
                .args(["-q", "-r"])
                .arg(&archive)
                .arg(".")
                .current_dir(&made),
        )
        .unwrap();
        assert!(done.status.success(), "{done:?}");
    }
}

/// A line of a runner's `steps.tsv`, its log where the runner's checkout holds it.
pub(super) fn line(step: &str, exit: &str, log: &str) -> String {
    format!(
        "{step}\t{exit}\t12\t/Users/runner/work/n/n/build/verify/{log}\tjust {step}\t1.50 1.20 1.00 3\n"
    )
}

/// The fixture with a project file naming `stand`, the workflow file and a `just verify` of its
/// own that leaves `ran-here` beside the fixture and is red while the candidate holds `red`.
pub(super) fn fixture() -> (Fixture, Stand) {
    let fixture = Fixture::new();
    let stand = Stand::new(&fixture);
    fixture.write("symphony.toml", &Fixture::project_file(&stand.setting()));
    fixture.write(WORKFLOW, "name: Release verification\n");
    fixture.write(
        "justfile",
        &format!(
            "verify *STEPS:\n\ttouch '{}'\n\ttest ! -e red\n",
            fixture.root.join("ran-here").display()
        ),
    );
    fixture.commit("the project verifies on GitHub's machines");
    super::git(&fixture.main, &["push", "-q", "origin", "main"]);
    (fixture, stand)
}

pub(super) fn ran_here(fixture: &Fixture) -> bool {
    fs::remove_file(fixture.root.join("ran-here")).is_ok()
}

/// What the adapter answers of a run that ended `conclusion` with `jobs` and `artifacts`, its
/// workflow file the one of `workflow_commit`.
pub(super) fn ended(
    workflow_commit: &str,
    conclusion: &str,
    jobs: &Value,
    artifacts: &Value,
) -> Value {
    json!({
        "state": "ended", "commit": "COMMIT",
        "run": {"id": 9000, "url": URL, "attempt": 1, "conclusion": conclusion,
                "workflow_commit": workflow_commit,
                "started": "2026-11-12T10:00:00Z", "ended": "2026-11-12T10:49:00Z"},
        "jobs": jobs, "artifacts": artifacts, "missing": [],
    })
}

pub(super) fn job(id: u64, name: &str, conclusion: &str, log: Option<&str>) -> Value {
    json!({
        "id": id, "name": name, "conclusion": conclusion, "seconds": 90,
        "url": format!("https://github.com/o/n/actions/job/{id}"), "log": log,
        "no_log": if log.is_some() { Value::Null } else { json!("GitHub answered 404 and no download") },
    })
}

pub(super) fn tip(fixture: &Fixture) -> String {
    super::git(&fixture.main, &["rev-parse", "origin/main"])
}

fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path.as_ref()).unwrap_or_else(|e| panic!("{}: {e}", path.as_ref().display()))
}

#[test]
fn a_green_run_on_github_releases_its_steps_and_logs_kept_and_nothing_verified_here() {
    let (fixture, stand) = fixture();
    let commit = tip(&fixture);
    stand.log("job-1.log", "steps: [lint, tests]\n");
    stand.log("job-2.log", "verify: lint: green (12 s)\n");
    stand.log("job-3.log", "verify: tests: green (12 s)\n");
    stand.artifact(
        "artifact-7.zip",
        &line("lint", "0", "lint.log"),
        &[("lint.log", "lint green\n")],
    );
    stand.artifact(
        "artifact-8.zip",
        &format!(
            "{}{}{}",
            line("nextest-setup", "0", "nextest-setup.log"),
            line("tests", "0", "tests.log"),
            line("tests/browser", "0", "tests-lanes/browser.log")
        ),
        &[
            ("nextest-setup.log", "ready\n"),
            ("tests.log", "tests green\n"),
            ("tests-lanes/browser.log", "browser green\n"),
        ],
    );
    stand.answer(&ended(
        &commit,
        "success",
        &json!([
            job(1, "The commit's steps", "success", Some("job-1.log")),
            job(2, "lint", "success", Some("job-2.log")),
            job(3, "tests", "success", Some("job-3.log")),
        ]),
        &json!([
            {"id": 7, "name": "verify-lint", "file": "artifact-7.zip"},
            {"id": 8, "name": "verify-tests", "file": "artifact-8.zip"},
        ]),
    ));

    let out = fixture.run(NOW, &[], 0);
    for said in [
        format!(
            "release: `verify` is asked of GitHub's runners for {}",
            &commit[..12]
        ),
        format!("release: verify verified on GitHub ({URL})"),
        "release:   verify/lint green (exit 0, 12 s); log ".to_string(),
        "release:   verify/tests green (exit 0, 12 s); log ".to_string(),
        "release: green; tagged release-2026-11-12, pushed to origin".to_string(),
    ] {
        assert!(out.contains(&said), "{said:?} in:\n{out}");
    }
    assert!(!ran_here(&fixture), "nothing is verified here:\n{out}");
    assert_eq!(fixture.tagged("release-2026-11-12"), commit);

    // Asked once, in the main checkout, for the commit, its downloads among the run's logs.
    let asked = stand.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0]["commit"], commit.as_str());
    assert_eq!(
        read(stand.folder.join("cwd.log")).trim(),
        fixture.main.to_string_lossy()
    );
    let run = fixture.runs().last().unwrap().clone();
    let id = run["id"].as_str().unwrap();
    let logs = fixture.records().join("logs").join(id);
    assert_eq!(asked[0]["by"], id);
    assert_eq!(
        asked[0]["folder"],
        logs.join("verify.hosted").to_string_lossy().as_ref()
    );
    assert!(
        asked[0]["seconds"]
            .as_u64()
            .is_some_and(|left| left > 30_000),
        "{asked:?}"
    );

    // The record names the workflow run, and each step with the log its artifact held.
    assert_eq!(
        run["hosted"],
        json!([{"check": "verify", "judged": "GitHub", "run": URL, "attempt": 1,
                "workflow_commit": commit}])
    );
    assert_eq!(
        run["environment"]["system"],
        format!("GitHub's runners, {URL}")
    );
    let check = &run["checks"][0];
    assert_eq!(
        check["command"],
        format!("just verify on GitHub's runners ({URL})")
    );
    assert_eq!(check["exit_code"], 0);
    let steps: Vec<(&str, &str)> = (check["steps"].as_array().unwrap().iter())
        .map(|step| {
            (
                step["name"].as_str().unwrap(),
                step["log"].as_str().unwrap(),
            )
        })
        .collect();
    let at = |file: &str| {
        logs.join("verify.steps")
            .join(file)
            .to_string_lossy()
            .into_owned()
    };
    assert_eq!(
        steps,
        [
            ("verify/lint", at("lint.log").as_str()),
            ("verify/nextest-setup", at("nextest-setup.log").as_str()),
            ("verify/tests", at("tests.log").as_str()),
        ]
    );
    assert_eq!(
        check["steps"][2]["steps"][0]["name"],
        "verify/tests/browser"
    );
    assert_eq!(read(at("tests-lanes/browser.log")), "browser green\n");
    assert_eq!(read(at("lint.log")), "lint green\n");
    assert_eq!(
        read(logs.join("verify.hosted/job-2.log")),
        "verify: lint: green (12 s)\n"
    );
    assert!(
        read(logs.join("verify.log")).contains(&format!("the workflow run {URL} ended success"))
    );
    assert_eq!(fixture.reds().len(), 0, "{:?}", fixture.reds());
    let status = fixture.run(NOW + 60.0, &["--status"], 0);
    assert!(
        status.contains(&format!("; verify verified on GitHub ({URL})")),
        "{status}"
    );
}

#[test]
fn a_red_run_on_github_files_each_red_step_and_a_job_that_did_not_end_is_never_green() {
    let (fixture, stand) = fixture();
    let commit = tip(&fixture);
    stand.log("job-2.log", "verify: lint: red, exit 1\n");
    stand.log("job-4.log", "brew: postgresql@16 failed\n");
    stand.artifact(
        "artifact-7.zip",
        &line("lint", "1", "lint.log"),
        &[("lint.log", "error: a lint is red\n")],
    );
    // A cancelled job's artifact says its step passed: GitHub's conclusion is what is believed.
    stand.artifact(
        "artifact-8.zip",
        &line("tests", "0", "tests.log"),
        &[("tests.log", "ok\n")],
    );
    stand.answer(&ended(
        &commit,
        "failure",
        &json!([
            job(1, "The commit's steps", "success", None),
            job(2, "lint", "failure", Some("job-2.log")),
            job(3, "tests", "cancelled", None),
            job(4, "hosted", "failure", Some("job-4.log")),
            job(5, "size-check", "timed_out", None),
            job(6, "docs-audit", "success", None),
        ]),
        &json!([
            {"id": 7, "name": "verify-lint", "file": "artifact-7.zip"},
            {"id": 8, "name": "verify-tests", "file": "artifact-8.zip"},
        ]),
    ));

    let out = fixture.run(NOW, &[], 1);
    for said in [
        "release: verify red (exit 1, ",
        "release:   verify/lint red (exit 1, 12 s); log ",
        "release:   verify/tests red (did not run: GitHub ended its job cancelled: the step did \
         not run to its end",
        "release:   verify/hosted red (exit 1, 90 s); log ",
        "release:   verify/size-check red (did not run: GitHub ended its job timed_out",
        "release:   verify/docs-audit red (did not run: its job on GitHub ended success and \
         listed no step",
        "; no release",
    ] {
        assert!(out.contains(said), "{said:?} in:\n{out}");
    }
    assert!(
        !ran_here(&fixture),
        "a red there is not verified again here:\n{out}"
    );
    assert_eq!(
        fixture.remote_tags().len(),
        0,
        "{:?}",
        fixture.remote_tags()
    );
    let reds = fixture.reds();
    assert_eq!(
        reds.keys().collect::<Vec<_>>(),
        [
            "tests/release-red-verify-docs-audit",
            "tests/release-red-verify-hosted",
            "tests/release-red-verify-lint",
            "tests/release-red-verify-size-check",
            "tests/release-red-verify-tests",
        ]
    );
    // The red's reader reads the step's own log, the artifact's, and a job without one its job's.
    let text = |slug: &str| format!("{}\n{}", reds[slug].body, fixture.thread(slug).join("\n"));
    let lint = text("tests/release-red-verify-lint");
    assert!(lint.contains("error: a lint is red"), "{lint}");
    let hosted = text("tests/release-red-verify-hosted");
    assert!(hosted.contains("brew: postgresql@16 failed"), "{hosted}");
    let run = fixture.runs().last().unwrap().clone();
    assert_eq!(run["result"], "red");
    assert_eq!(run["hosted"][0]["judged"], "GitHub");
    let steps = run["checks"][0]["steps"].as_array().unwrap();
    let cancelled = (steps.iter())
        .find(|step| step["name"] == "verify/tests")
        .unwrap();
    assert!(cancelled["exit_code"].is_null(), "{cancelled}");
    let none = cancelled["log"].as_str().unwrap();
    assert!(
        read(none).contains("GitHub gave no log of this job: GitHub answered 404"),
        "{none}"
    );
}

#[test]
fn what_is_no_believed_result_is_said_and_the_commit_is_verified_here() {
    let (fixture, stand) = fixture();
    let first = super::git(&fixture.main, &["rev-list", "--max-parents=0", "HEAD"]);
    // The workflow file as another commit of main had it, and the tip red here, so each run
    // judges the same commit again.
    let other = fixture.land(
        WORKFLOW,
        Some("name: Another workflow\n"),
        "another workflow",
    );
    fixture.land(
        WORKFLOW,
        Some("name: Release verification\n"),
        "the workflow again",
    );
    let commit = fixture.land("red", Some("red"), "red here");
    let green = json!([job(2, "lint", "success", None)]);
    let lint = json!([{"id": 7, "name": "verify-lint", "file": "artifact-7.zip"}]);
    stand.artifact(
        "artifact-7.zip",
        &line("lint", "0", "lint.log"),
        &[("lint.log", "ok\n")],
    );
    fs::write(stand.folder.join("downloads/broken.zip"), "not an archive").unwrap();
    let cases: Vec<(Value, String)> = vec![
        (
            json!({"state": "unavailable", "why": "GitHub answered 503 to GET /repos/o/n"}),
            "unavailable: GitHub answered 503 to GET /repos/o/n".into(),
        ),
        (
            json!({"state": "refused", "why": "turned off: the committed settings do not say"}),
            "refused: turned off: the committed settings do not say".into(),
        ),
        (
            json!({"state": "published"}),
            "the command answered the state \"published\"".into(),
        ),
        (json!(["ended"]), "its answer is not a JSON object".into()),
        (
            json!({"state": "ended", "commit": first, "run": {}}),
            format!(
                "the command answered for the commit \"{first}\", not {}",
                &commit[..12]
            ),
        ),
        (
            ended(&other, "success", &green, &lint),
            format!(
                "the run {URL} is not believed: its run ran the {WORKFLOW} of {}, which is not \
                 the file main commits at {}",
                &other[..12],
                &commit[..12]
            ),
        ),
        (
            ended(&first, "success", &green, &lint),
            format!(
                "the run {URL} is not believed: its run ran the {WORKFLOW} of {}, a commit \
                 origin/main does not hold",
                &first[..12]
            ),
        ),
        (
            ended(&"f".repeat(40), "success", &green, &lint),
            "a commit origin/main does not hold".into(),
        ),
        (
            ended("main", "success", &green, &lint),
            "its run names no commit its workflow file is of: \"main\"".into(),
        ),
        (
            ended(
                &commit,
                "success",
                &json!([job(1, "The commit's steps", "success", None)]),
                &json!([]),
            ),
            format!("the run {URL} ended success and listed no step"),
        ),
        (
            ended(
                &commit,
                "success",
                &green,
                &json!([{"id": 9, "name": "verify-lint", "file": "broken.zip"}]),
            ),
            format!(
                "what the run {URL} left does not read: the artifact verify-lint does not open: unzip: "
            ),
        ),
    ];
    for (at, (answer, why)) in cases.iter().enumerate() {
        stand.answer(answer);
        #[expect(clippy::cast_precision_loss, reason = "a few cases")]
        let out = fixture.run(NOW + 60.0 * at as f64, &[], 1);
        let said = (out.lines())
            .find_map(|line| line.strip_prefix("release: verify verified here, not on GitHub: "))
            .unwrap_or_else(|| panic!("{why}: not said in:\n{out}"));
        assert!(said.contains(why.as_str()), "{why:?} in {said:?}");
        assert!(ran_here(&fixture), "{why}: verified here:\n{out}");
        let run = fixture.runs().last().unwrap().clone();
        assert_eq!(run["hosted"][0]["judged"], "here", "{run}");
        assert!(
            run["hosted"][0]["why"]
                .as_str()
                .unwrap()
                .contains(why.as_str()),
            "{run}"
        );
        assert!(run["hosted"][0].get("run").is_none(), "{run}");
        assert_eq!(run["checks"][0]["command"], "just verify");
        assert_ne!(
            run["environment"]["chrome"], "the runner's",
            "this machine's"
        );
    }
    assert_eq!(stand.asked().len(), cases.len());

    // A command that fails, and one that is not there, are said the same way; then the commit
    // green here is released by this machine's verification.
    fs::write(stand.folder.join("fails"), "").unwrap();
    let out = fixture.run(NOW + 3000.0, &[], 1);
    assert!(
        out.contains("verified here, not on GitHub: ")
            && out.contains("symphony-github: no working directory"),
        "{out}"
    );
    assert!(ran_here(&fixture));
    fs::remove_file(&stand.program).unwrap();
    fixture.land("red", None, "green here");
    let out = fixture.run(NOW + 3600.0, &[], 0);
    assert!(
        out.contains("verified here, not on GitHub: cannot run "),
        "{out}"
    );
    assert!(
        out.contains("release: green; tagged release-2026-11-12"),
        "{out}"
    );
    assert!(ran_here(&fixture));
    let status = fixture.run(NOW + 3660.0, &["--status"], 0);
    assert!(
        status.contains("; verify verified here, not on GitHub: cannot run "),
        "{status}"
    );
}

#[test]
fn a_check_named_by_hand_and_a_project_that_names_no_command_ask_nothing() {
    let (fixture, stand) = fixture();
    stand.answer(&json!({"state": "unavailable", "why": "never asked"}));
    let out = fixture.night(NOW, 0);
    assert!(!out.contains("GitHub"), "{out}");
    assert!(stand.asked().is_empty(), "a check named by hand runs here");
    let run = fixture.runs().last().unwrap().clone();
    assert!(run.get("hosted").is_none(), "{run}");

    // Without the key the run is today's: the full verification, here.
    fixture.write("symphony.toml", &Fixture::project_file(""));
    fixture.commit("the project verifies here");
    super::git(&fixture.main, &["push", "-q", "origin", "main"]);
    let out = fixture.run(NOW + DAY_AFTER, &[], 0);
    assert!(!out.contains("GitHub"), "{out}");
    assert!(ran_here(&fixture));
    assert_eq!(stand.asked().len(), 0, "{:?}", stand.asked());
    assert!(fixture.runs().last().unwrap().get("hosted").is_none());
}

/// A day after [`NOW`], so a second release takes the next day's name.
const DAY_AFTER: f64 = 86_400.0;

// The time ending under the command itself is a unit test of the bound (hosted_tests.rs), where no
// load decides which ends first; here the command's own wait ends, which is the same red.
#[test]
fn a_run_github_had_not_ended_in_time_is_red_and_verifies_nothing_here() {
    let (fixture, stand) = fixture();
    stand.answer(&json!({"state": "late", "why": "the run 9000 had not ended within 60 s"}));
    let out = fixture.run(NOW, &[], 1);
    assert!(
        out.contains("release: verify red (timed out: stopped after "),
        "{out}"
    );
    assert!(
        !ran_here(&fixture),
        "its time was up: nothing is verified here:\n{out}"
    );
    assert!(
        out.contains(
            "release: verify asked of GitHub: its run there had not ended in time: the run 9000 \
             had not ended within 60 s"
        ),
        "{out}"
    );
    assert_eq!(
        fixture.remote_tags().len(),
        0,
        "{:?}",
        fixture.remote_tags()
    );
    let run = fixture.runs().last().unwrap().clone();
    assert_eq!(run["checks"][0]["timed_out"], true, "{run}");
    assert_eq!(run["hosted"][0]["judged"], "GitHub");
    let log = read(run["checks"][0]["log"].as_str().unwrap());
    assert!(
        log.contains("a run it started on GitHub is left to end, and writes nothing"),
        "{log}"
    );
}
