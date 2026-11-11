//! `symphony nightly` end to end: a repository of the test's own whose remote is a bare repository
//! beside it, a scratch board (the store's test server, a database per test), a machine folder of
//! its own and stand-in checks. No test reaches a real remote, the live board or the machine's
//! registry, and none runs the real verification.

#[path = "../common/mod.rs"]
mod common;

/// The task store's test server, as its own tests run on it: a database of its own per test.
#[path = "../../../tasks/tests/store/common.rs"]
#[allow(dead_code)]
mod board;

mod conciliation;
mod recorder;
mod reds;
mod releases;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use symphony_board::sys::{Command, output};
use symphony_board::{Store, Task, rpc};

use common::Tree;

const BIN: &str = env!("CARGO_BIN_EXE_symphony");
/// 2026-11-12 10:00 UTC; the runs read time in UTC.
const NOW: f64 = 1_794_477_600.0;
const DAY: f64 = 86_400.0;
/// The stand-in check: red while the candidate holds a file named `red`.
const CHECK: &str = "verify=test ! -e red";

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
        let env = vec![
            ("TASKS_SERVER".into(), db.addr().to_string()),
            ("TASKS_DATABASE".into(), db.database().to_string()),
            ("SF_TASKS_CLI".into(), common::tasks_binary().to_string()),
            (
                "SYMPHONY_MACHINE_DIR".into(),
                root.join("machine").to_string_lossy().into_owned(),
            ),
            ("TZ".into(), "UTC".into()),
        ];
        let fixture = Fixture {
            _tree: tree,
            root,
            origin,
            main,
            env,
        };
        fixture.write(
            "symphony.toml",
            "[project]\nid = \"fixture\"\n\n[agents]\nworktrees = \"../wt\"\n",
        );
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

    /// `symphony nightly ARGS` from the main checkout at `now`: its exit code, stdout and stderr.
    fn nightly_at(&self, now: f64, args: &[&str]) -> (i32, String, String) {
        self.nightly_with(now, args, &[])
    }

    /// The same, with `extra` in its environment.
    fn nightly_with(
        &self,
        now: f64,
        args: &[&str],
        extra: &[(&str, &str)],
    ) -> (i32, String, String) {
        let done = output(
            Command::new(BIN)
                .arg("nightly")
                .arg("--now")
                .arg(now.to_string())
                .args(args)
                .current_dir(&self.main)
                .env_remove("TASKS_SESSION")
                .env_remove("TASKS_TASK")
                .env_remove("TASKS_RUN_PARENT")
                .env_remove("TASKS_RECORDER")
                .env_remove("TASKS_WORK")
                .envs(identity())
                .envs(self.env.iter().map(|(k, v)| (k, v)))
                .envs(extra.iter().copied()),
        )
        .unwrap();
        (
            done.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&done.stdout).into_owned(),
            String::from_utf8_lossy(&done.stderr).into_owned(),
        )
    }

    /// A night with the stand-in check, which must end with `code`: its stdout.
    fn night(&self, now: f64, code: i32) -> String {
        self.run(now, &["--check", CHECK], code)
    }

    /// `nightly ARGS` at `now`, which must end with `code`: its stdout.
    fn run(&self, now: f64, args: &[&str], code: i32) -> String {
        let (said, out, err) = self.nightly_at(now, args);
        assert_eq!(
            said, code,
            "nightly {args:?}\nstdout:\n{out}\nstderr:\n{err}"
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
        self.root.join("wt/nightly")
    }

    /// The nightly's folder in the git common dir.
    fn records(&self) -> PathBuf {
        self.main.join(".git/sf-symphony/nightly")
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

    /// The board's tasks of the nightly's red series, by slug.
    fn reds(&self) -> BTreeMap<String, Task> {
        self.store()
            .tasks(None)
            .unwrap()
            .into_iter()
            .filter(|task| task.slug.starts_with("tests/nightly-red-"))
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
        (&["--release", "HEAD", "--status"][..], "are one each"),
        (&["stray"][..], "unrecognized arguments: stray"),
        (&["--schedule", "--at", "25:00"][..], "is not HH:MM"),
    ] {
        let (code, _, err) = fixture.nightly_at(NOW, args);
        assert_eq!(code, 2, "{args:?}: {err}");
        assert!(err.contains(said), "{args:?}: {err}");
    }
    let (code, _, err) = fixture.nightly_at(
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
        err.contains("must be outside the repository's folder"),
        "{err}"
    );
    let (code, out, _) = fixture.nightly_at(NOW, &["--help"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("usage: symphony nightly"), "{out}");
    assert!(!fixture.records().join("runs").exists(), "nothing ran");
    assert!(!fixture.worktree().exists());
    assert!(fixture.remote_tags().is_empty());
}

#[test]
fn one_run_at_a_time() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.records()).unwrap();
    let lock = fs::File::create(fixture.records().join("run.lock")).unwrap();
    lock.lock().unwrap();
    let (code, _, err) = fixture.nightly_at(NOW, &["--check", CHECK]);
    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains("another nightly or release of this repository is running"),
        "{err}"
    );
    assert!(fixture.remote_tags().is_empty());
    lock.unlock().unwrap();
    fixture.night(NOW, 0);
    assert_eq!(fixture.remote_tags(), ["release-2026-11-12"]);
}

#[test]
fn the_schedule_entry_runs_the_nightly_in_the_main_checkout() {
    let fixture = Fixture::new();
    let out = fixture.run(NOW, &["--schedule", "--at", "04:30"], 0);
    let main = fixture.main.to_string_lossy();
    if cfg!(target_os = "macos") {
        assert!(
            out.contains("<string>symphony.nightly.fixture</string>"),
            "{out}"
        );
        assert!(
            out.contains(&format!(
                "<string>cd '{main}' &amp;&amp; exec just nightly</string>"
            )),
            "{out}"
        );
        assert!(
            out.contains("<integer>4</integer>") && out.contains("<integer>30</integer>"),
            "{out}"
        );
    } else {
        assert!(
            out.contains(&format!("30 4 * * * cd '{main}' && PATH=")),
            "{out}"
        );
    }
    assert!(
        out.contains(&format!("{main}/.git/sf-symphony/nightly/scheduled.log")),
        "{out}"
    );
    assert!(
        !fixture.records().exists(),
        "printing the entry installs and records nothing"
    );
}
