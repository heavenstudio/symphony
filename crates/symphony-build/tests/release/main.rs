//! `symphony-build release` end to end: a repository of the test's own whose remote is a bare
//! repository beside it, a scratch board (the store's test server, a database per test), a machine
//! folder of its own and stand-in checks. No test reaches a real remote, the live board or the
//! machine's registry, and none runs the real verification. The release run runs with a hostile
//! user git configuration ([`HOSTILE`]), which asks every push to take more than its refspec.

#[path = "../../../symphony/tests/common/mod.rs"]
mod common;

/// The task store's test server, as its own tests run on it: a database of its own per test.
#[path = "../../../tasks/tests/store/common.rs"]
#[allow(dead_code)]
mod board;

mod alone;
mod cadence;
mod candidate;
mod carried;
mod clock;
mod consumers;
mod delivered;
mod github;
mod held;
mod hosted;
mod install;
mod places;
mod publish;
mod recorder;
mod reds;
mod refresh;
mod regenerated;
mod releases;
mod renamed;
mod rulesets;
mod steps;
mod stops;
mod unread;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use symphony_board::sys::{Command, output};
use symphony_board::{Store, Task, rpc};

use common::Tree;

const BIN: &str = env!("CARGO_BIN_EXE_symphony-build");
/// 2026-11-12 10:00 UTC; the runs read time in UTC.
const NOW: f64 = 1_794_477_600.0;
const DAY: f64 = 86_400.0;
/// The stand-in check: red while the candidate holds a file named `red`.
const CHECK: &str = "verify=test ! -e red";
/// The user git configuration the release run runs with: every push follows tags, matches every
/// branch, and recurses into submodules, unless the command says otherwise.
const HOSTILE: &str = "[push]\n\tfollowTags = true\n\tdefault = matching\n\
                       \trecurseSubmodules = on-demand\n\
                       [remote \"origin\"]\n\tpush = refs/heads/*:refs/heads/*\n\
                       \tpush = refs/tags/*:refs/tags/*\n";

/// `git ARGS` in `dir` as the fixture runs it, which must succeed: what it printed.
fn git(dir: &Path, args: &[&str]) -> String {
    let done = output(
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .envs(identity()),
    )
    .unwrap();
    assert!(
        done.status.success(),
        "git {args:?} in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&done.stderr)
    );
    String::from_utf8_lossy(&done.stdout).trim_end().to_string()
}

/// Who commits and tags, with no configuration of the machine's in the way.
fn identity() -> Vec<(&'static str, &'static str)> {
    vec![
        ("GIT_AUTHOR_NAME", "Fixture"),
        ("GIT_AUTHOR_EMAIL", "fixture@example.com"),
        ("GIT_COMMITTER_NAME", "Fixture"),
        ("GIT_COMMITTER_EMAIL", "fixture@example.com"),
        ("GIT_CONFIG_GLOBAL", "/dev/null"),
        ("GIT_CONFIG_NOSYSTEM", "1"),
    ]
}

/// A Symphony repository of the test's own: its remote, its main checkout, a scratch board and a
/// machine folder.
struct Fixture {
    /// The temporary folder everything is in, removed with the fixture.
    _tree: Tree,
    root: PathBuf,
    origin: PathBuf,
    main: PathBuf,
    env: Vec<(String, String)>,
}

impl Fixture {
    /// The repository at its first commit on the remote's main: a project file whose agents'
    /// worktrees are `../wt`, and the files the board schema and the pin are read from.
    fn new() -> Fixture {
        let tree = Tree::new();
        let root = tree.root.canonicalize().unwrap();
        let origin = root.join("origin.git");
        let main = root.join("symphony");
        fs::create_dir_all(&origin).unwrap();
        git(&origin, &["init", "-q", "--bare", "-b", "main"]);
        git(
            &root,
            &["clone", "-q", &origin.to_string_lossy(), "symphony"],
        );
        let db = board::client();
        let hostile = root.join("hostile.gitconfig");
        fs::write(&hostile, HOSTILE).unwrap();
        let env = vec![
            ("TASKS_SERVER".into(), db.addr().to_string()),
            ("TASKS_DATABASE".into(), db.database().to_string()),
            (
                "SYMPHONY_TASKS_CLI".into(),
                common::tasks_binary().to_string(),
            ),
            (
                "SYMPHONY_MACHINE_DIR".into(),
                root.join("machine").to_string_lossy().into_owned(),
            ),
            ("TZ".into(), "UTC".into()),
            // Each board write of a run delivers in the write, as the board's own tests ask: no
            // test here reads what a detached dispatcher did, so none is started for nobody.
            ("TASKS_DISPATCH".into(), "inline".into()),
            (
                "GIT_CONFIG_GLOBAL".into(),
                hostile.to_string_lossy().into_owned(),
            ),
        ];
        let fixture = Fixture {
            _tree: tree,
            root,
            origin,
            main,
            env,
        };
        fixture.write("symphony.toml", &Fixture::project_file(""));
        fixture.write(
            "crates/tasks/src/schema.rs",
            "pub const VERSION: i64 = 16;\n",
        );
        fixture.write(
            "crates/symphony/src/machine.rs",
            "pub const PIN: u32 = 1;\n",
        );
        fixture.commit("the first commit");
        git(&fixture.main, &["push", "-q", "-u", "origin", "main"]);
        fixture
    }

    /// The fixture's project file: its agents' worktrees `../wt`, a release's refresh off (a test
    /// of the refresh turns it on) and no release candidate held (a test of the candidate holds
    /// one), with `releases` more lines of its `[releases]` table.
    fn project_file(releases: &str) -> String {
        let refresh = if releases.contains("refresh") {
            ""
        } else {
            "refresh = false\n"
        };
        let candidate = if releases.contains("candidate_") {
            ""
        } else {
            "candidate_reruns = 0\n"
        };
        format!(
            "[project]\nid = \"fixture\"\n\n[agents]\nworktrees = \"../wt\"\n\n[releases]\n\
             {refresh}{candidate}{releases}"
        )
    }

    fn write(&self, file: &str, text: &str) {
        let path = self.main.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// Commit everything in the checkout: the commit.
    fn commit(&self, message: &str) -> String {
        git(&self.main, &["add", "-A"]);
        git(
            &self.main,
            &["commit", "-q", "--allow-empty", "-m", message],
        );
        git(&self.main, &["rev-parse", "HEAD"])
    }

    /// `file` holding `text`, committed and pushed to the remote's main: the commit.
    fn land(&self, file: &str, text: Option<&str>, message: &str) -> String {
        match text {
            Some(text) => self.write(file, text),
            None => fs::remove_file(self.main.join(file)).unwrap(),
        }
        let commit = self.commit(message);
        git(&self.main, &["push", "-q", "origin", "main"]);
        commit
    }

    /// `symphony-build release ARGS` from the main checkout at `now`: its exit code, stdout and
    /// stderr.
    fn release_at(&self, now: f64, args: &[&str]) -> (i32, String, String) {
        self.release_with(now, args, &[])
    }

    /// The same, with `extra` in its environment.
    fn release_with(
        &self,
        now: f64,
        args: &[&str],
        extra: &[(&str, &str)],
    ) -> (i32, String, String) {
        let done = output(&mut self.command(now, args, extra)).unwrap();
        (
            done.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&done.stdout).into_owned(),
            String::from_utf8_lossy(&done.stderr).into_owned(),
        )
    }

    /// `symphony-build release ARGS` from the main checkout, its clock at `now`, with `extra` in
    /// its environment, to start.
    fn command(&self, now: f64, args: &[&str], extra: &[(&str, &str)]) -> Command {
        self.command_through(&[], now, args, extra)
    }

    /// The same, started through `before`: a program and its first arguments, which exec the rest.
    fn command_through(
        &self,
        before: &[&str],
        now: f64,
        args: &[&str],
        extra: &[(&str, &str)],
    ) -> Command {
        let mut line = before.to_vec();
        line.extend([BIN, "release"]);
        let mut command = Command::new(line[0]);
        command
            .args(&line[1..])
            .args(args)
            .current_dir(&self.main)
            .env_remove("TASKS_SESSION")
            .env_remove("TASKS_TASK")
            .env_remove("TASKS_RUN_PARENT")
            .env_remove("TASKS_RECORDER")
            .env_remove("TASKS_WORK")
            .env_remove("SYMPHONY_RELEASE_DRIVER")
            .env_remove("SYMPHONY_RELEASE_STALE")
            .env_remove("SYMPHONY_RELEASE_RELOAD_SECONDS")
            .env_remove("SYMPHONY_RELEASE_REGENERATOR_SECONDS")
            .env_remove("SYMPHONY_RELEASE_SLEPT")
            .envs(identity())
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .env("SYMPHONY_RELEASE_NOW", now.to_string())
            .envs(extra.iter().copied());
        command
    }

    /// A run with the stand-in check, which must end with `code`: its stdout.
    fn night(&self, now: f64, code: i32) -> String {
        self.run(now, &["--check", CHECK], code)
    }

    /// `release ARGS` at `now`, which must end with `code`: its stdout.
    fn run(&self, now: f64, args: &[&str], code: i32) -> String {
        let (said, out, err) = self.release_at(now, args);
        assert_eq!(
            said, code,
            "release {args:?}\nstdout:\n{out}\nstderr:\n{err}"
        );
        out
    }

    /// The tags on the remote, sorted by name.
    fn remote_tags(&self) -> Vec<String> {
        let listed = git(&self.origin, &["tag", "-l"]);
        listed.lines().map(str::to_string).collect()
    }

    /// The commit the remote's tag `tag` names.
    fn tagged(&self, tag: &str) -> String {
        git(&self.origin, &["rev-parse", &format!("{tag}^{{commit}}")])
    }

    /// The remote's tag `tag`'s message, which ends its last line.
    fn message(&self, tag: &str) -> String {
        git(&self.origin, &["tag", "-l", "--format=%(contents)", tag]) + "\n"
    }

    /// The run's own checkout, where the project file puts it.
    fn worktree(&self) -> PathBuf {
        self.root.join("wt/release")
    }

    /// The release run's folder in the git common dir.
    fn records(&self) -> PathBuf {
        self.main.join(".git/symphony/release")
    }

    /// The remote's references, name and object, one per line.
    fn remote_refs(&self) -> String {
        git(
            &self.origin,
            &["for-each-ref", "--format=%(refname) %(objectname)"],
        )
    }

    /// The runs recorded, oldest first, as JSON.
    fn runs(&self) -> Vec<serde_json::Value> {
        let mut runs: Vec<serde_json::Value> = fs::read_dir(self.records().join("runs"))
            .map(|entries| {
                entries
                    .map(|entry| fs::read_to_string(entry.unwrap().path()).unwrap())
                    .map(|text| serde_json::from_str(&text).unwrap())
                    .collect()
            })
            .unwrap_or_default();
        runs.sort_by(|a, b| {
            let at = |run: &serde_json::Value| run["started_at"].as_f64().unwrap_or_default();
            at(a).total_cmp(&at(b))
        });
        runs
    }

    /// The scratch board, through the store itself.
    fn store(&self) -> Store {
        let value = |name: &str| {
            self.env
                .iter()
                .find(|(key, _)| key == name)
                .unwrap()
                .1
                .clone()
        };
        Store::with(
            rpc::Client::new(&value("TASKS_SERVER"), "sf", &value("TASKS_DATABASE")),
            Box::new(|| board::NOW),
            Box::new(|_| false),
            board::HOST,
        )
        .unwrap()
    }

    /// The board's tasks of the release run's red series, by slug.
    fn reds(&self) -> BTreeMap<String, Task> {
        self.store()
            .tasks(None)
            .unwrap()
            .into_iter()
            .filter(|task| task.slug.starts_with("tests/release-red-"))
            .map(|task| (task.slug.clone(), task))
            .collect()
    }

    /// The texts on `slug`'s thread, oldest first.
    fn thread(&self, slug: &str) -> Vec<String> {
        (self.store().thread(slug).unwrap().into_iter())
            .map(|message| message.text)
            .collect()
    }

    /// Everything under `dir`, path and bytes: what a reader must leave as it was.
    fn files(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut found = BTreeMap::new();
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    found.extend(Fixture::files(&path));
                } else {
                    found.insert(path.clone(), fs::read(&path).unwrap());
                }
            }
        }
        found
    }
}

#[test]
fn a_command_line_it_cannot_read_is_refused_and_runs_nothing() {
    let fixture = Fixture::new();
    for (args, said) in [
        (&["--check", "noname"][..], "is not NAME=COMMAND"),
        (
            &["--check", "a=true", "--check", "a=false"][..],
            "is named twice",
        ),
        (&["--status", "--schedule"][..], "are one each"),
        (&["--commit", "HEAD", "--status"][..], "are one each"),
        (&["stray"][..], "unrecognized arguments: stray"),
        (&["--schedule", "--at", "25:00"][..], "is not HH:MM"),
        (&["--timeout", "0"][..], "is not a number of seconds"),
        (&["--timeout", "soon"][..], "is not a number of seconds"),
        (
            &["--timeout", "86401"][..],
            "--timeout 86401 is longer than a run may take: at most 86400 seconds (24 hours)",
        ),
        (
            &["--timeout", "18446744073709551615"][..],
            "is longer than a run may take",
        ),
        (&["--check", "run=true"][..], "keeps for its own reds"),
        (&["--check", "refresh-x=true"][..], "keeps for its own reds"),
        (&["--due", "--status"][..], "are one each"),
        (&["--refresh", "--commit", "HEAD"][..], "are one each"),
        (&["--due", "tools/x"][..], "unrecognized arguments: tools/x"),
        (&["stray", "two"][..], "unrecognized arguments: stray two"),
        // No flag sets the clock a release is named by: only the tests' variable does.
        (&["--now", "1800000000"][..], "--now"),
    ] {
        let (code, _, err) = fixture.release_at(NOW, args);
        assert_eq!(code, 2, "{args:?}: {err}");
        assert!(err.contains(said), "{args:?}: {err}");
    }
    for now in ["soon", "inf", "NaN", "-1"] {
        let (code, _, err) =
            fixture.release_with(NOW, &["--check", CHECK], &[("SYMPHONY_RELEASE_NOW", now)]);
        assert_eq!(code, 2, "{now}: {err}");
        assert!(
            err.contains(&format!("SYMPHONY_RELEASE_NOW={now:?} is not a time")),
            "{now}: {err}"
        );
    }
    let elsewhere = fixture.root.join("not-a-checkout");
    fs::create_dir_all(&elsewhere).unwrap();
    let (code, _, err) = fixture.release_at(
        NOW,
        &["--check", CHECK, "--root", &elsewhere.to_string_lossy()],
    );
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("not a git repository"), "{err}");
    let (code, _, err) = fixture.release_at(
        NOW,
        &[
            "--check",
            CHECK,
            "--worktree",
            &fixture.main.join("inside").to_string_lossy(),
        ],
    );
    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains(&format!(
            "is inside the checkout {}: the release run works outside the repository's checkouts",
            fixture.main.display()
        )),
        "{err}"
    );
    let (code, out, _) = fixture.release_at(NOW, &["--help"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("usage: symphony-build release"), "{out}");
    assert!(!out.contains("--now"), "{out}");
    assert!(!fixture.records().join("runs").exists(), "nothing ran");
    assert!(!fixture.worktree().exists());
    assert!(fixture.remote_tags().is_empty());
}

#[test]
fn the_schedule_entry_runs_the_release_run_in_the_main_checkout_and_makes_its_log_folder() {
    let fixture = Fixture::new();
    let bin = fixture.root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let just = bin.join("just");
    fs::write(&just, "#!/bin/sh\necho \"just $* in $PWD\"\n").unwrap();
    fs::set_permissions(&just, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let (code, out, err) = fixture.release_with(
        NOW,
        &["--schedule", "--at", "04:30"],
        &[("PATH", path.as_str())],
    );
    assert_eq!(code, 0, "{err}");
    let main = fixture.main.to_string_lossy();
    let log = fixture.records().join("scheduled.log");
    let line = if cfg!(target_os = "macos") {
        assert!(
            out.contains("<string>symphony.release.fixture</string>"),
            "{out}"
        );
        assert!(
            out.contains("<integer>4</integer>") && out.contains("<integer>30</integer>"),
            "{out}"
        );
        let plist = fixture.root.join("agent.plist");
        fs::write(&plist, &out).unwrap();
        let read = |key: &str| {
            let done = output(
                Command::new("plutil")
                    .args(["-extract", key, "raw", "-o", "-"])
                    .arg(&plist),
            )
            .unwrap();
            assert!(done.status.success(), "{key}: {done:?}");
            String::from_utf8_lossy(&done.stdout).trim_end().to_string()
        };
        assert_eq!(read("EnvironmentVariables.PATH"), path);
        read("ProgramArguments.2")
    } else {
        let line = out.lines().nth(1).unwrap();
        assert!(line.starts_with("30 4 * * * mkdir -p "), "{out}");
        line.splitn(6, ' ').nth(5).unwrap().replace("\\%", "%")
    };
    assert!(line.contains(&format!("cd '{main}'")), "{line}");
    assert!(
        !fixture.records().exists(),
        "printing the entry installs, makes and records nothing"
    );
    // Run as the schedule runs it: from nowhere in particular, with the entry's PATH.
    let done = output(
        Command::new("/bin/sh")
            .args(["-c", &line])
            .current_dir("/")
            .env("PATH", &path),
    )
    .unwrap();
    assert!(done.status.success(), "{line}: {done:?}");
    assert_eq!(
        fs::read_to_string(&log).unwrap(),
        format!("just release in {main}\n")
    );
}
