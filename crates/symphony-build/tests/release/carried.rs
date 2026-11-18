//! A release candidate's rerun carries the green of each step and lane whose inputs its fixes left
//! alone (docs/RELEASES.md, "The rerun"): the tree says what each reads (`tools/check-inputs.sh`),
//! a green stands with the digest it was green on, and the rerun runs what was red, what a fix
//! changed and what it cannot tell the inputs of, and names each green it carried with its commit,
//! its run and its digest, in its record and in the release's message. A first run carries nothing.

use std::fs;
use std::path::PathBuf;

use super::candidate::{approve, hands, holding, land, record};
use super::{Fixture, NOW, git};

/// The red task of the stand-in verify's tests step.
const RED: &str = "tests/release-red-verify-tests";
const VERIFY: &str = "verify=sh steps.sh";

/// A stand-in verify with the steps `lint` and `tests`, the tests in the lanes `a`, `b` and `all`,
/// each listed after the step as a part of it. Named `lint`, `tests` or `tests/LANE`, it runs
/// those alone. Lint and each lane are red while their folder (`src-NAME`) holds `red`, and each
/// that runs is written in `OUTSIDE/ran`.
const STEPS: &str = r#"d="$SYMPHONY_CHECK_STEPS"
code=0
line() { printf '%s\t%s\t1\t%s\tsh steps.sh %s\n' "$1" "$2" "$3" "$1" >> "$d/steps.tsv"; }
status() { if [ -e "src-$1/red" ]; then echo 1; else echo 0; fi; }
args="${*:-lint tests}"
case " $args " in
*" lint "*)
    echo lint >> "OUTSIDE/ran"
    s="$(status lint)"; [ "$s" = 0 ] || code=1
    echo "lint ended $s" > "$d/lint.log"
    line lint "$s" lint.log
    ;;
esac
lanes=''
case " $args " in
*" tests "*) lanes='a b all' ;;
*) for arg in $args; do case "$arg" in tests/*) lanes="$lanes ${arg#tests/}" ;; esac; done ;;
esac
if [ -n "$lanes" ]; then
    tests=0
    for lane in $lanes; do
        echo "tests/$lane" >> "OUTSIDE/ran"
        [ "$(status "$lane")" = 0 ] || { tests=1; code=1; }
    done
    echo "tests ended $tests" > "$d/tests.log"
    line tests "$tests" tests.log
    for lane in $lanes; do line "tests/$lane" "$(status "$lane")" tests.log; done
fi
exit $code
"#;

/// The stand-in tree's word on its inputs: lint and the lanes `a` and `b` read their own folder,
/// the lane `all` the whole tree, and each but that one the check itself (`tools/`, `steps.sh`).
/// It fails while `OUTSIDE/fail-inputs` is there, and leaves the lane `b` out while
/// `OUTSIDE/leave-out-b` is.
const INPUTS: &str = r#"if [ -e "OUTSIDE/fail-inputs" ]; then echo 'inputs: told to fail' >&2; exit 2; fi
h() { git rev-parse --verify --quiet "HEAD:$1" || echo absent; }
self="$(h tools)-$(h steps.sh)"
printf 'verify/lint\t%s\n' "$self-$(h src-lint)"
printf 'verify/tests/a\t%s\n' "$self-$(h src-a)"
[ -e "OUTSIDE/leave-out-b" ] || printf 'verify/tests/b\t%s\n' "$self-$(h src-b)"
printf 'verify/tests/all\t%s\n' "$(git rev-parse 'HEAD^{tree}')"
"#;

/// A stand-in Chrome, which says the version `OUTSIDE/chrome` holds.
const CHROME: &str = "#!/bin/sh\ncat \"OUTSIDE/chrome\"\n";

/// A candidate held red in the lane `a` alone, after its first run.
struct Laned {
    fixture: Fixture,
    outside: PathBuf,
    /// The commit first judged.
    red: String,
    /// The first run, as recorded.
    first: serde_json::Value,
}

impl Laned {
    fn new() -> Laned {
        let fixture = holding("");
        let outside = fixture.root.join("outside");
        fs::create_dir_all(&outside).unwrap();
        let at = outside.to_string_lossy().into_owned();
        fs::write(outside.join("chrome"), "Stand-in Chrome 1\n").unwrap();
        let chrome = outside.join("chrome.sh");
        fs::write(&chrome, CHROME.replace("OUTSIDE", &at)).unwrap();
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&chrome, fs::Permissions::from_mode(0o755)).unwrap();
        }
        fixture.write("steps.sh", &STEPS.replace("OUTSIDE", &at));
        fixture.write("tools/check-inputs.sh", &INPUTS.replace("OUTSIDE", &at));
        for folder in ["src-lint", "src-a", "src-b"] {
            fixture.write(&format!("{folder}/source"), "1\n");
        }
        fixture.land("src-a/red", Some("red"), "the lane a turns red");
        let red = git(&fixture.main, &["rev-parse", "HEAD"]);
        let laned = Laned {
            fixture,
            outside,
            red,
            first: serde_json::Value::Null,
        };
        let out = laned.run(NOW, &[], 1);
        assert!(
            out.contains("release: red: verify/tests; no release\n"),
            "{out}"
        );
        assert!(
            !out.contains("carried") && !out.contains("inputs") && !out.contains("environment"),
            "a first run carries nothing and says nothing of it:\n{out}"
        );
        assert_eq!(record(&laned.fixture)["reds"][0]["slug"], RED);
        assert_eq!(laned.ran(), ["lint", "tests/a", "tests/b", "tests/all"]);
        let first = laned.fixture.runs().pop().unwrap();
        Laned { first, ..laned }
    }

    /// `release --check VERIFY ARGS` at `now` on the stand-in Chrome, which must end with `code`:
    /// its stdout.
    fn run(&self, now: f64, args: &[&str], code: i32) -> String {
        let mut line = vec!["--check", VERIFY];
        line.extend(args);
        let chrome = self.outside.join("chrome.sh");
        let extra = [("SYMPHONY_CHROME", chrome.to_str().unwrap())];
        let (said, out, err) = self.fixture.release_with(now, &line, &extra);
        assert_eq!(
            said, code,
            "release {line:?}\nstdout:\n{out}\nstderr:\n{err}"
        );
        out
    }

    /// Each step and lane that ran, in order, since the last ask.
    fn ran(&self) -> Vec<String> {
        let path = self.outside.join("ran");
        let text = fs::read_to_string(&path).unwrap_or_default();
        let _ = fs::remove_file(path);
        text.lines().map(str::to_string).collect()
    }

    /// The digest the first run was told for `name`.
    fn digest(&self, name: &str) -> String {
        let told = self.first["inputs"]["told"].as_array().unwrap();
        let input = told.iter().find(|input| input["name"] == name).unwrap();
        input["digest"].as_str().unwrap().to_string()
    }

    /// The red's fix, started at the candidate: the lane `a` green again and each of `more`
    /// written, approved and landed: the commit and its land's merge.
    fn fix(&self, more: &[(&str, &str)]) -> (String, String) {
        let fixture = &self.fixture;
        git(
            &fixture.main,
            &["checkout", "-q", "-b", RED, "release-candidate"],
        );
        fs::remove_file(fixture.main.join("src-a/red")).unwrap();
        for (file, text) in more {
            fixture.write(file, text);
        }
        let fixed = fixture.commit("the lane a fixed");
        git(&fixture.main, &["checkout", "-q", "main"]);
        approve(fixture, &hands(fixture), RED, &fixed, true);
        (fixed, land(fixture, RED, true))
    }

    /// The rerun the land `merge` of the red's fix starts, which must end with `code`: its stdout.
    fn rerun(&self, merge: &str, code: i32) -> String {
        self.run(NOW + 300.0, &["--due", RED, merge], code)
    }

    /// The names the newest run carried green.
    fn carried(&self) -> Vec<String> {
        let rerun = self.fixture.runs().pop().unwrap();
        (rerun["rerun"]["carried"].as_array().unwrap().iter())
            .map(|kept| kept["name"].as_str().unwrap().to_string())
            .collect()
    }
}

#[test]
fn a_first_run_carries_nothing_and_records_its_inputs_and_what_it_ran_on() {
    let laned = Laned::new();
    let first = &laned.first;
    assert!(first.get("rerun").is_none(), "{first:#}");
    let told: Vec<&str> = (first["inputs"]["told"].as_array().unwrap().iter())
        .map(|input| input["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        told,
        [
            "verify/lint",
            "verify/tests/a",
            "verify/tests/b",
            "verify/tests/all"
        ]
    );
    assert_eq!(first["environment"]["chrome"], "Stand-in Chrome 1");
    for named in ["system", "postgresql"] {
        let said = first["environment"][named].as_str().unwrap();
        assert!(!said.is_empty(), "{named}: {first:#}");
    }
    // The lanes are parts of the tests step, whose red is one red and one task.
    let steps = first["checks"][0]["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 2, "{first:#}");
    let lanes: Vec<(&str, i64)> = (steps[1]["steps"].as_array().unwrap().iter())
        .map(|lane| {
            (
                lane["name"].as_str().unwrap(),
                lane["exit_code"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        lanes,
        [
            ("verify/tests/a", 1),
            ("verify/tests/b", 0),
            ("verify/tests/all", 0)
        ]
    );
    assert_eq!(laned.fixture.reds().keys().collect::<Vec<_>>(), [RED]);
    // Its greens stand with the digest they were green on.
    let standing = &record(&laned.fixture)["standing"][0];
    assert_eq!(
        standing["steps"][0]["green"]["digest"],
        laned.digest("verify/lint").as_str()
    );
    assert_eq!(
        standing["steps"][1]["parts"][1]["green"]["digest"],
        laned.digest("verify/tests/b").as_str()
    );
    assert!(standing["steps"][1]["parts"][0]["green"].is_null());
}

#[test]
fn a_rerun_runs_the_lane_its_fix_changed_and_the_whole_tree_lane_and_carries_the_rest() {
    let laned = Laned::new();
    let (fixed, merge) = laned.fix(&[]);
    let out = laned.rerun(&merge, 0);
    for said in [
        "release: `verify` reruns tests/a, tests/all, the rest carried green on the inputs they \
         were green on\n",
        "release: green; tagged release-2026-11-12, pushed to origin\n",
    ] {
        assert!(out.contains(said), "{said}\n\n{out}");
    }
    assert_eq!(
        laned.ran(),
        ["tests/a", "tests/all"],
        "the lane the fix changed and the lane that reads the whole tree, and nothing else"
    );
    assert_eq!(laned.fixture.tagged("release-2026-11-12"), fixed);
    let rerun = laned.fixture.runs().pop().unwrap();
    assert_eq!(
        rerun["checks"][0]["command"],
        "sh steps.sh tests/a tests/all"
    );
    let run = laned.first["id"].as_str().unwrap();
    let carried = rerun["rerun"]["carried"].as_array().unwrap();
    assert_eq!(carried.len(), 2, "{rerun:#}");
    for (kept, name) in carried.iter().zip(["verify/lint", "verify/tests/b"]) {
        assert_eq!(kept["name"], name);
        assert_eq!(kept["commit"], laned.red.as_str());
        assert_eq!(kept["run"], run);
        assert_eq!(kept["digest"], laned.digest(name).as_str());
    }
    let message = laned.fixture.message("release-2026-11-12");
    let said = format!(
        "Carried green from a run of this candidate, not run on this commit: its inputs' digest \
         is the one it was green on:\n\
         - verify/lint: green on {red} in the run {run}, inputs {}\n\
         - verify/tests/b: green on {red} in the run {run}, inputs {}\n",
        laned.digest("verify/lint"),
        laned.digest("verify/tests/b"),
        red = &laned.red[..12],
    );
    assert!(message.contains(&said), "{said}\n\n{message}");
    assert!(message.contains("Green: verify.\n"), "{message}");
}

#[test]
fn a_fix_that_changes_another_lanes_inputs_runs_that_lane_too() {
    let laned = Laned::new();
    let (_, merge) = laned.fix(&[("src-b/source", "2\n")]);
    let out = laned.rerun(&merge, 0);
    assert!(
        out.contains(
            "release: `verify` reruns tests, the rest carried green on the inputs they were \
             green on\n"
        ),
        "no lane kept, so the step runs whole:\n{out}"
    );
    assert_eq!(laned.ran(), ["tests/a", "tests/b", "tests/all"]);
    assert_eq!(laned.carried(), ["verify/lint"]);
}

#[test]
fn a_fix_that_changes_the_check_itself_carries_nothing() {
    for file in ["tools/helper.sh", "steps.sh"] {
        let laned = Laned::new();
        let text = fs::read_to_string(laned.fixture.main.join("steps.sh")).unwrap();
        let (_, merge) = laned.fix(&[(file, &format!("{text}# changed\n"))]);
        let out = laned.rerun(&merge, 0);
        assert!(!out.contains("carried green"), "{file}:\n{out}");
        assert_eq!(
            laned.ran(),
            ["lint", "tests/a", "tests/b", "tests/all"],
            "{file}"
        );
        let rerun = laned.fixture.runs().pop().unwrap();
        assert_eq!(rerun["checks"][0]["command"], "sh steps.sh", "{file}");
        assert_eq!(laned.carried(), Vec::<String>::new(), "{file}");
    }
}

#[test]
fn inputs_that_cannot_be_told_carry_nothing_and_a_name_left_out_is_run_each_said() {
    // The tip's inputs script fails.
    let laned = Laned::new();
    let (_, merge) = laned.fix(&[]);
    fs::write(laned.outside.join("fail-inputs"), "").unwrap();
    let out = laned.rerun(&merge, 0);
    assert!(
        out.contains(
            "release: no step's inputs can be told (tools/check-inputs.sh failed (exit status: \
             2): inputs: told to fail): a green is carried only from a commit of this tree\n"
        ),
        "{out}"
    );
    assert!(!out.contains("carried green"), "{out}");
    assert_eq!(laned.ran(), ["lint", "tests/a", "tests/b", "tests/all"]);
    let rerun = laned.fixture.runs().pop().unwrap();
    assert!(
        rerun["inputs"]["error"]
            .as_str()
            .is_some_and(|why| why.contains("told to fail")),
        "{rerun:#}"
    );
    assert_eq!(laned.carried(), Vec::<String>::new());

    // It leaves one lane out: that lane runs, and the rest is carried.
    let laned = Laned::new();
    let (_, merge) = laned.fix(&[]);
    fs::write(laned.outside.join("leave-out-b"), "").unwrap();
    let out = laned.rerun(&merge, 0);
    assert!(
        out.contains(
            "release: this tree tells no inputs for verify/tests/b: a green is carried only \
             from a commit of this tree\n"
        ),
        "{out}"
    );
    assert_eq!(laned.ran(), ["tests/a", "tests/b", "tests/all"]);
    assert_eq!(laned.carried(), ["verify/lint"]);

    // The tip has no inputs script at all.
    let laned = Laned::new();
    git(
        &laned.fixture.main,
        &["checkout", "-q", "-b", RED, "release-candidate"],
    );
    fs::remove_file(laned.fixture.main.join("src-a/red")).unwrap();
    fs::remove_file(laned.fixture.main.join("tools/check-inputs.sh")).unwrap();
    let fixed = laned
        .fixture
        .commit("the lane a fixed, the inputs script gone");
    git(&laned.fixture.main, &["checkout", "-q", "main"]);
    approve(&laned.fixture, &hands(&laned.fixture), RED, &fixed, true);
    let merge = land(&laned.fixture, RED, true);
    let out = laned.rerun(&merge, 0);
    assert!(
        out.contains(
            "release: this tree has no tools/check-inputs.sh, so no step's inputs are told: a \
             green is carried only from a commit of this tree\n"
        ),
        "{out}"
    );
    assert_eq!(laned.ran(), ["lint", "tests/a", "tests/b", "tests/all"]);
}

#[test]
fn a_green_of_another_environment_is_not_carried() {
    let laned = Laned::new();
    let (_, merge) = laned.fix(&[]);
    fs::write(laned.outside.join("chrome"), "Stand-in Chrome 2\n").unwrap();
    let out = laned.rerun(&merge, 0);
    assert!(
        out.contains(
            "release: 3 greens of the candidate stood on another environment than this one ("
        ) && out.contains("Chrome: Stand-in Chrome 2; ")
            && out.contains("): none of them is carried\n"),
        "{out}"
    );
    assert!(!out.contains("carried green"), "{out}");
    assert_eq!(laned.ran(), ["lint", "tests/a", "tests/b", "tests/all"]);
    let rerun = laned.fixture.runs().pop().unwrap();
    assert_eq!(rerun["environment"]["chrome"], "Stand-in Chrome 2");
}

#[test]
fn a_rerun_on_the_same_tree_carries_by_the_tree_and_runs_the_red_lane_alone() {
    let laned = Laned::new();
    // Its red is answered outside this repository: nothing lands, and the tree is the same.
    let hands = hands(&laned.fixture);
    let store = laned.fixture.store();
    (store.take_role(RED, &hands.orchestrator, "orchestrator")).unwrap();
    store
        .move_with(
            RED,
            "cancelled",
            &hands.orchestrator,
            Some("fixed outside this repository"),
        )
        .unwrap();
    // The lane is still red in this tree: one more round, the red lane alone having run.
    let out = laned.run(NOW + 60.0, &["--rerun"], 1);
    assert!(
        out.contains("release: `verify` reruns tests/a, the rest carried green from this tree\n"),
        "{out}"
    );
    assert_eq!(laned.ran(), ["tests/a"]);
    assert_eq!(
        laned.carried(),
        ["verify/lint", "verify/tests/b", "verify/tests/all"]
    );
    let rerun = laned.fixture.runs().pop().unwrap();
    assert!(
        (rerun["rerun"]["carried"].as_array().unwrap().iter())
            .all(|kept| kept.get("digest").is_none()),
        "carried by the tree, which names no digest: {rerun:#}"
    );
}
