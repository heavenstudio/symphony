//! Who bypasses the default branch's rules is verified again by each run, where the project names
//! the command `[releases] rulesets`: a stand-in for `symphony-github rulesets --verify` is the
//! check `rulesets`, after the checks, in the main checkout. Where the policy holds the release
//! goes on and its notes name it green; where it does not, the run is red in it, files it as any
//! red and tags nothing, and the run after it was put right closes that red and releases. A
//! command that cannot be run, an adapter that cannot read the rulesets whole or is turned off,
//! are red alike; a run told to stop while it runs stops it and tags nothing. A release candidate's
//! rerun runs it again when it was red and carries its green from the same tree as any check's. A
//! project that names no command runs none and records no such check.

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;

use symphony_board::sys::Stdio;

use super::candidate::hands;
use super::stops::{Process, ended_within, ends_within, written_pid};
use super::{CHECK, Fixture, NOW};

/// A stand-in for the adapter's `rulesets --verify` in a folder of the fixture's own, which keeps
/// each argument line it is run with (`asked.log`) and the folder it ran in (`cwd.log`), and says
/// what the adapter says: that the policy holds, or, while the file `bypassed` is beside it, that
/// a ruleset is bypassed and a delivery would be refused, exiting 1.
struct Stand {
    folder: PathBuf,
    program: PathBuf,
}

const HOLDS: &str = "o/n (id 7), main: 1 ruleset(s) in effect, read with gh\n  ruleset 42 \
                     \"protect\" (active), last changed 2026-11-12T09:00:00Z: bypassed by nobody\n\
                     holds, and verified: 1 ruleset(s) kept, which a delivery takes until one \
                     changes\n";
const MISSING: &str = "  missing: a `pull_request` rule nobody bypasses";

impl Stand {
    fn new(fixture: &Fixture) -> Stand {
        let folder = fixture.root.join("rulesets-stand-in");
        fs::create_dir_all(&folder).unwrap();
        let program = folder.join("stand-in");
        fs::write(
            &program,
            format!(
                r#"#!/bin/sh
here='{folder}'
echo "$*" >> "$here/asked.log"
pwd >> "$here/cwd.log"
if [ -e "$here/bypassed" ]; then
    echo 'o/n (id 7), main: 1 ruleset(s) in effect, read with gh'
    echo '  ruleset 42 "protect" (active), last changed 2026-11-12T11:00:00Z: bypassed by Team 3 (always)'
    echo 'does not hold: a delivery would be refused; nothing kept'
    echo '{MISSING}'
    exit 1
fi
printf '%s' '{HOLDS}'
"#,
                folder = folder.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        Stand { folder, program }
    }

    fn bypassed(&self, bypassed: bool) {
        let file = self.folder.join("bypassed");
        if bypassed {
            fs::write(file, "").unwrap();
        } else {
            fs::remove_file(file).unwrap();
        }
    }

    /// The argument lines it was run with, in order.
    fn asked(&self) -> Vec<String> {
        let text = fs::read_to_string(self.folder.join("asked.log")).unwrap_or_default();
        text.lines().map(str::to_string).collect()
    }

    /// The check's command, as its record and its red task say it.
    fn command(&self, fixture: &Fixture) -> String {
        format!(
            "cd {} && {} rulesets --verify",
            fixture.main.display(),
            self.program.display()
        )
    }
}

/// The fixture with a project file naming the stand-in's command, committed and pushed.
fn fixture() -> (Fixture, Stand) {
    fixture_with("")
}

/// The same, with `more` lines in its `[releases]` table.
fn fixture_with(more: &str) -> (Fixture, Stand) {
    let fixture = Fixture::new();
    let stand = Stand::new(&fixture);
    let setting = format!(
        "{more}rulesets = [\"{}\", \"rulesets\", \"--verify\"]\n",
        stand.program.display()
    );
    fixture.write("symphony.toml", &Fixture::project_file(&setting));
    fixture.commit("the project verifies its rulesets in each release run");
    super::git(&fixture.main, &["push", "-q", "origin", "main"]);
    (fixture, stand)
}

/// The names of the checks the run `run` recorded, in order.
fn checks(run: &serde_json::Value) -> Vec<String> {
    (run["checks"].as_array().unwrap().iter())
        .map(|check| check["name"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn where_the_policy_holds_the_check_is_green_after_the_others_and_the_release_goes_on() {
    let (fixture, stand) = fixture();
    let out = fixture.night(NOW, 0);
    let verify = out.find("release: verify green").expect(&out);
    let rulesets = out.find("release: rulesets green (exit 0, ").expect(&out);
    assert!(verify < rulesets, "after the checks given: {out}");
    assert!(
        out.contains("release: green; tagged release-2026-11-12, pushed to origin\n"),
        "{out}"
    );
    assert_eq!(fixture.remote_tags(), ["release-2026-11-12"]);
    // Asked once, as named, in the main checkout, where the adapter reads its settings.
    assert_eq!(stand.asked(), ["rulesets --verify"]);
    assert_eq!(
        fs::read_to_string(stand.folder.join("cwd.log")).unwrap(),
        format!("{}\n", fixture.main.display())
    );
    let run = fixture.runs()[0].clone();
    assert_eq!(checks(&run), ["verify", "rulesets"]);
    let check = &run["checks"][1];
    assert_eq!(check["command"], stand.command(&fixture).as_str());
    assert_eq!(
        fs::read_to_string(check["log"].as_str().unwrap()).unwrap(),
        format!("$ {}\n{HOLDS}", stand.command(&fixture))
    );
    assert!(
        fixture
            .message("release-2026-11-12")
            .contains("\nGreen: verify, rulesets.\n"),
        "{}",
        fixture.message("release-2026-11-12")
    );
    assert!(fixture.reds().is_empty());
}

#[test]
fn where_it_does_not_hold_the_run_is_red_in_it_alone_and_green_again_once_put_right() {
    let (fixture, stand) = fixture();
    stand.bypassed(true);
    let out = fixture.night(NOW, 1);
    assert!(out.contains("release: verify green"), "{out}");
    assert!(out.contains("release: rulesets red (exit 1, "), "{out}");
    assert!(out.contains("release: red: rulesets; no release"), "{out}");
    assert!(
        out.contains("release: filed tests/release-red-rulesets"),
        "{out}"
    );
    assert!(fixture.remote_tags().is_empty(), "nothing is tagged");
    let reds = fixture.reds();
    assert_eq!(
        reds.keys().collect::<Vec<_>>(),
        ["tests/release-red-rulesets"]
    );
    let red = &reds["tests/release-red-rulesets"];
    assert!(red.red() && red.priority == 0, "{red:?}");
    for said in [
        "The Symphony release run is red in `rulesets` on main at ".to_string(),
        format!("Command: `{}`, ", stand.command(&fixture)),
        MISSING.trim().to_string(),
        "bypassed by Team 3 (always)".into(),
        "The next run of `just release` closes this task once `rulesets` is green".into(),
        // Whose act answers it: no executor's commit.
        "Who fixes: whoever administers the repository on GitHub, not an executor with a commit."
            .into(),
        "then cancels the task once it is put right, saying where".into(),
    ] {
        assert!(red.body.contains(&said), "{said}\n{}", red.body);
    }
    assert!(
        !red.body.contains("any free executor takes this"),
        "{}",
        red.body
    );

    // Put right on GitHub, with nothing landed: the next run is green in it, closes its red and
    // releases.
    stand.bypassed(false);
    let out = fixture.night(NOW + 3600.0, 0);
    assert!(out.contains("release: rulesets green"), "{out}");
    assert!(
        out.contains("release: green; tagged release-2026-11-12, pushed to origin\n"),
        "{out}"
    );
    assert!(
        out.contains("release: closed tests/release-red-rulesets"),
        "{out}"
    );
    assert_eq!(
        fixture.reds()["tests/release-red-rulesets"].status,
        "cancelled"
    );
    assert_eq!(stand.asked().len(), 2);
}

#[test]
fn a_check_given_its_name_while_the_project_names_one_is_refused_and_runs_nothing() {
    let (fixture, stand) = fixture();
    let (code, _, err) = fixture.release_at(NOW, &["--check", "rulesets=true"]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("--check \"rulesets\" takes the name of the check [releases] rulesets names"),
        "{err}"
    );
    assert!(stand.asked().is_empty());
    assert!(fixture.runs().is_empty());
}

#[test]
fn a_project_that_names_no_command_runs_none_and_records_no_such_check() {
    // A `symphony-github` on the PATH that would say so if anything ran it.
    let fixture = Fixture::new();
    let bin = fixture.root.join("trap-bin");
    fs::create_dir_all(&bin).unwrap();
    let trap = bin.join("symphony-github");
    fs::write(
        &trap,
        format!("#!/bin/sh\necho \"$*\" >> '{}/ran.log'\n", bin.display()),
    )
    .unwrap();
    fs::set_permissions(&trap, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let (code, out, _) = fixture.release_with(NOW, &["--check", CHECK], &[("PATH", &path)]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("rulesets"), "{out}");
    assert!(!bin.join("ran.log").exists());
    assert_eq!(checks(&fixture.runs()[0]), ["verify"]);
}

/// The red task `slug` cancelled by its orchestrator, as one fixed outside this repository is.
fn answered_outside(fixture: &Fixture, slug: &str) {
    let hands = hands(fixture);
    let store = fixture.store();
    (store.take_role(slug, &hands.orchestrator, "orchestrator")).unwrap();
    store
        .move_with(
            slug,
            "cancelled",
            &hands.orchestrator,
            Some("put right on GitHub, outside this repository"),
        )
        .unwrap();
}

#[test]
fn a_command_that_cannot_run_or_an_adapter_unavailable_or_turned_off_is_red_and_says_why() {
    for (script, code, why) in [
        // The shell's own line from the log (`sh: /nowhere/symphony-github: No such file or
        // directory`, or dash's `not found`): the red's Command line names the program with no
        // colon after it.
        (None, 127, "/nowhere/symphony-github: "),
        (
            Some("echo 'not read whole, nothing kept: gh api repos/o/n: HTTP 503'; exit 1"),
            1,
            "not read whole, nothing kept: gh api repos/o/n: HTTP 503",
        ),
        // Named while the adapter's settings deliver nothing: a setting to put right, never a
        // green passed over.
        (
            Some(
                "echo 'turned off: the committed settings do not say enabled = true and \
                 pull_requests = \"publish\"'; exit 1",
            ),
            1,
            "turned off: the committed settings do not say enabled = true and pull_requests = \
             \"publish\"",
        ),
    ] {
        let (fixture, stand) = fixture();
        match script {
            Some(script) => fs::write(&stand.program, format!("#!/bin/sh\n{script}\n")).unwrap(),
            None => {
                let named =
                    "rulesets = [\"/nowhere/symphony-github\", \"rulesets\", \"--verify\"]\n";
                fixture.land(
                    "symphony.toml",
                    Some(&Fixture::project_file(named)),
                    "a command that is not there",
                );
            }
        }
        let out = fixture.night(NOW, 1);
        assert!(
            out.contains(&format!("release: rulesets red (exit {code}, ")),
            "{why}\n{out}"
        );
        assert!(out.contains("release: red: rulesets; no release"), "{out}");
        assert!(fixture.remote_tags().is_empty());
        let run = fixture.runs().pop().unwrap();
        assert_eq!(checks(&run), ["verify", "rulesets"]);
        assert_eq!(run["checks"][1]["exit_code"], code);
        let red = &fixture.reds()["tests/release-red-rulesets"];
        assert!(red.body.contains(why), "{why}\n{}", red.body);
    }
}

#[test]
fn a_run_told_to_stop_while_it_runs_stops_it_and_tags_nothing() {
    let (fixture, stand) = fixture();
    let pid_file = fixture.root.join("rulesets.pid");
    fs::write(
        &stand.program,
        format!(
            "#!/bin/sh\necho $$ > '{}'\nexec sleep 1000\n",
            pid_file.display()
        ),
    )
    .unwrap();
    // SAFETY: signal sets this process's disposition for a valid signal; the release run started
    // below inherits it, as a scheduler starts it.
    unsafe {
        libc::signal(libc::SIGTERM, libc::SIG_DFL);
    }
    let said_file = fixture.root.join("said.txt");
    let mut night = fixture
        .command(NOW, &["--check", CHECK, "--timeout", "600"], &[])
        .stdout(fs::File::create(&said_file).unwrap())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let check_pid = written_pid(&pid_file);
    let running = Process(libc::pid_t::try_from(check_pid).unwrap());
    // SAFETY: kill sends the signal to the release run this test started and has not reaped.
    unsafe {
        libc::kill(libc::pid_t::try_from(night.id()).unwrap(), libc::SIGTERM);
    }
    assert_eq!(ended_within(&mut night, 60), Some(1));
    assert!(ends_within(check_pid, 5), "the check was stopped");
    running.ended();
    let out = fs::read_to_string(&said_file).unwrap();
    assert!(out.contains("release: verify green"), "{out}");
    assert!(
        out.contains("release: rulesets stopped (interrupted: stopped after "),
        "{out}"
    );
    let run = fixture.runs().pop().unwrap();
    assert_eq!(run["result"], "error", "{run}");
    assert!(
        run["error"].as_str().unwrap().starts_with(
            "the run was told to stop (HUP, INT or TERM) while `rulesets` ran, which was stopped \
             after "
        ),
        "{run}"
    );
    assert_eq!(checks(&run), ["verify"], "a check stopped is kept as none");
    assert!(fixture.remote_tags().is_empty());
    assert_eq!(
        fixture.reds().keys().collect::<Vec<_>>(),
        ["tests/release-red-run"],
        "a check stopped is the run's red, not its own"
    );
}

#[test]
fn a_held_candidate_red_in_it_alone_reruns_it_alone_once_answered_and_releases() {
    let (fixture, stand) = fixture_with("candidate_reruns = 3\n");
    stand.bypassed(true);
    let tip = super::git(&fixture.main, &["rev-parse", "HEAD"]);
    let out = fixture.night(NOW, 1);
    assert!(
        out.contains(&format!(
            "release: holds the release candidate {}",
            &tip[..12]
        )),
        "{out}"
    );
    // Put right on GitHub, and its red answered outside this repository: the rerun carries
    // verify's green from this tree and asks the stand-in again.
    stand.bypassed(false);
    answered_outside(&fixture, "tests/release-red-rulesets");
    let out = fixture.run(NOW + 60.0, &["--check", CHECK, "--rerun"], 0);
    for said in [
        "release: `verify` carried green: the commit it was green on holds this tree\n",
        "release: rulesets green (exit 0, ",
        "release: green; tagged release-2026-11-12, pushed to origin\n",
    ] {
        assert!(out.contains(said), "{said}\n{out}");
    }
    assert_eq!(stand.asked().len(), 2);
    let rerun = fixture.runs().pop().unwrap();
    assert_eq!(checks(&rerun), ["rulesets"]);
    assert_eq!(fixture.tagged("release-2026-11-12"), tip);
    let message = fixture.message("release-2026-11-12");
    assert!(message.contains("Green: verify, rulesets.\n"), "{message}");
}

#[test]
fn a_held_candidate_red_elsewhere_carries_its_green_from_the_same_tree_as_any_checks() {
    let (fixture, stand) = fixture_with("candidate_reruns = 3\n");
    let outside = fixture.root.join("outside-red");
    fs::write(&outside, "red").unwrap();
    let verify = format!("verify=test ! -e '{}'", outside.display());
    fixture.run(NOW, &["--check", &verify], 1);
    assert_eq!(
        fixture.reds().keys().collect::<Vec<_>>(),
        ["tests/release-red-verify"]
    );
    fs::remove_file(&outside).unwrap();
    answered_outside(&fixture, "tests/release-red-verify");
    let out = fixture.run(NOW + 60.0, &["--check", &verify, "--rerun"], 0);
    for said in [
        "release: verify green (exit 0, ",
        "release: `rulesets` carried green: the commit it was green on holds this tree\n",
        "release: green; tagged release-2026-11-12, pushed to origin\n",
    ] {
        assert!(out.contains(said), "{said}\n{out}");
    }
    assert_eq!(stand.asked().len(), 1, "asked by the first round alone");
    assert_eq!(checks(&fixture.runs().pop().unwrap()), ["verify"]);
}
