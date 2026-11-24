//! Sweep rules, and the land that ends with a sweep, tested in isolated repositories and a
//! private PostgreSQL server.
//!
//! Every test builds its own repository outside this one: nothing here reads or touches the
//! worktrees of the checkout it runs in. The sweep is called as a function where the command line
//! cannot reach (a clock, `ps`, a pause, the process scan) and as the command line, in this process,
//! where it is the command line that is tested.

// How a test starts its own children ([`children::Child`]).
#[path = "../support/children.rs"]
mod children;

#[path = "../store/common.rs"]
#[allow(dead_code)]
mod common;

mod build_output;
mod cached;
mod causes;
mod clean_cache;
mod clean_races;
mod cli;
mod delivered;
mod finished;
mod kept;
mod land;
mod land_delivery;
mod on_review;
mod policy_cache;
mod report;
mod rules;
mod sessionless;
mod triggers;

use crate::children::Child as _;
use common::{Fixture, Seed};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, FileTimes};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};
use symphony_board::cli::Env;
use symphony_board::sweep::{Options, Sweep, sweep};
use symphony_board::sys::Spawning as _;
use symphony_board::{NewSession, NewTask};

/// Past the sweep's two-hour window of recent activity.
pub const IDLE_HOURS: f64 = 3.0;
/// -03 all year: a lock written in UTC and a `ps` in local time differ by 3 h.
pub const NOT_UTC: &str = "America/Sao_Paulo";

static REPOS: AtomicUsize = AtomicUsize::new(0);
static TEMPLATE: OnceLock<PathBuf> = OnceLock::new();

unsafe extern "C" {
    fn atexit(at_exit: extern "C" fn()) -> std::ffi::c_int;
}

extern "C" fn remove_template() {
    if let Some(dir) = TEMPLATE.get() {
        let _ = fs::remove_dir_all(dir);
    }
}

/// The ignore file of a consuming project, as the sweep's disposable and regenerable paths
/// (`sweep::git::files`) are written for it: its engine's caches and imports, the golden test's
/// failure images, its tools' environment and bytecode, the agents' local state, the Codex app's
/// imported hook pair, run evidence under `artifacts/`, export output and the tree's build folder.
/// The fixture holds its own, never the ignore file of the checkout running the tests.
const IGNORE: &str = "\
.godot/
*.import
tests/golden/*.actual.png
tests/golden/*.expected.png
tests/golden/*.diff.png
.venv/
.claude/*
!.claude/skills/
!.claude/hooks/
!.claude/settings.json
/.codex/hooks.json
/.codex/hooks/
artifacts/*
!artifacts/.gdignore
build/*
!build/.gdignore
__pycache__/
/.target/
*.profraw
*.profdata
.obsidian/
.env
.DS_Store
";

/// The repository every test's starts as, made once per test binary and removed at its exit: `main`
/// with one commit, the consuming project's [`IGNORE`] file and `sim/world.gd`.
fn template() -> &'static Path {
    TEMPLATE.get_or_init(|| {
        let root =
            std::env::temp_dir().join(format!("symphony-sweep-template-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        for (key, value) in [
            ("user.email", "t@example.com"),
            ("user.name", "t"),
            ("commit.gpgsign", "false"),
        ] {
            git(&root, &["config", key, value]);
        }
        write(&root, ".gitignore", IGNORE);
        write(&root, "sim/world.gd", "extends RefCounted\n");
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "base"]);
        assert_eq!(
            unsafe { atexit(remove_template) },
            0,
            "the template's end is set"
        );
        root
    })
}

/// `git ARGS` in `dir`, which must succeed: its output, trimmed.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::of("git")
            .args(args)
            .current_dir(dir),
    )
    .unwrap();
    assert!(
        done.status.success(),
        "git {args:?} in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&done.stderr)
    );
    String::from_utf8(done.stdout).unwrap().trim().to_string()
}

/// What a test's repository looks like to `lsof`: a process works in the checkout running the tests,
/// none in the repository. Asked of `lsof` itself it costs half a second of system time on a busy
/// machine, so only the command line's tests ask it.
pub fn nobody_inside() -> Option<Vec<PathBuf>> {
    Some(vec![PathBuf::from(env!("CARGO_MANIFEST_DIR"))])
}

/// A process that stays in a directory until its owner drops it, without a timing deadline.
pub struct HeldCwd(std::process::Child);

impl HeldCwd {
    pub fn at(path: &Path) -> Self {
        let mut cat = Command::of("cat");
        cat.current_dir(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // Under the lock, as every std spawn beside the helpers: outside it, the process could keep
        // a pipe's first ends, made in another thread, for its whole life.
        let _spawning = symphony_board::sys::spawning();
        Self(cat.spawn().unwrap())
    }
}

impl Drop for HeldCwd {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The pid of a process that has exited and been reaped.
pub fn dead_pid() -> i64 {
    let mut child = Command::of("true").spawn_locked().unwrap();
    child.wait().unwrap();
    i64::from(child.id())
}

/// `date`, BSD's else GNU's, with `args` for each, in zone `tz`: its output, trimmed.
fn date(tz: &str, bsd: &[&str], gnu: &[&str]) -> String {
    for args in [bsd, gnu] {
        let done = symphony_board::sys::output(
            symphony_board::sys::Command::of("date")
                .args(args)
                .env("TZ", tz)
                .env("LC_ALL", "C"),
        )
        .unwrap();
        if done.status.success() {
            return String::from_utf8(done.stdout).unwrap().trim().to_string();
        }
    }
    panic!("date could not convert {bsd:?}");
}

/// When `pid` started, as Claude Code writes it into a worktree's lock: in UTC (`Wed Sep 30 13:10:22
/// 2026`). Read from `ps` in a zone that is not UTC (-03, like the owner's machine) and converted by
/// `date`, so the sweep's own reading of `ps` is not what a test compares with.
pub fn started(pid: i64) -> String {
    let local = symphony_board::sys::output(
        symphony_board::sys::Command::of("ps")
            .args(["-o", "lstart=", "-p", &pid.to_string()])
            .env("LC_ALL", "C")
            .env("TZ", NOT_UTC),
    )
    .unwrap();
    let local = String::from_utf8(local.stdout).unwrap();
    let local = local.split_whitespace().collect::<Vec<_>>().join(" ");
    let format = "%a %b %d %H:%M:%S %Y";
    let seconds = date(
        NOT_UTC,
        &["-j", "-f", format, &local, "+%s"],
        &["-d", &local, "+%s"],
    );
    date(
        "UTC",
        &["-u", "-r", &seconds, &format!("+{format}")],
        &["-u", "-d", &format!("@{seconds}"), &format!("+{format}")],
    )
}

/// A repository with one commit on `main`, checked out at its root, and a board of its own.
pub struct Repo {
    pub dir: PathBuf,
    pub root: PathBuf,
    pub board: Fixture,
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

impl Default for Repo {
    fn default() -> Repo {
        Repo::with(Seed::default())
    }
}

impl Repo {
    pub fn new() -> Repo {
        Repo::default()
    }

    /// With the board seeded.
    pub fn with(seed: Seed) -> Repo {
        let dir = std::env::temp_dir().join(format!(
            "symphony-sweep-{}-{}",
            std::process::id(),
            REPOS.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let root = dir.join("repo");
        // A copy of one repository made once: six git commands a test, saved.
        let copied = Command::of("cp")
            .arg("-R")
            .arg(template())
            .arg(&root)
            .status_locked()
            .unwrap();
        assert!(copied.success(), "cp -R of the template repository");
        Repo {
            dir,
            root,
            board: Fixture::new(seed),
        }
    }

    /// A worktree at `base`: detached, or on a new `branch`.
    pub fn add(&self, name: &str, branch: Option<&str>, base: &str) -> PathBuf {
        let wt = self.dir.join(name);
        let at = wt.to_str().unwrap();
        match branch {
            Some(branch) => git(
                &self.root,
                &["worktree", "add", "-q", "-b", branch, at, base],
            ),
            None => git(&self.root, &["worktree", "add", "-q", "--detach", at, base]),
        };
        wt
    }

    /// A detached worktree at main.
    pub fn detached(&self, name: &str) -> PathBuf {
        self.add(name, None, "main")
    }

    /// A worktree on a new `branch` from main, a commit on it, merged into main.
    pub fn merged(&self, name: &str, branch: &str) -> PathBuf {
        let wt = self.add(name, Some(branch), "main");
        self.commit(&wt, &format!("extends RefCounted\n# {name}\n"));
        git(&self.root, &["merge", "-q", "--ff-only", branch]);
        wt
    }

    /// A commit in `wt` changing sim/world.gd to `text`.
    pub fn commit(&self, wt: &Path, text: &str) {
        write(wt, "sim/world.gd", text);
        git(wt, &["commit", "-qam", "work"]);
    }

    pub fn lock(&self, wt: &Path, reason: &str) {
        git(
            &self.root,
            &["worktree", "lock", "--reason", reason, wt.to_str().unwrap()],
        );
    }

    /// Claude Code's lock of `wt` by process `pid` started at `start`, naming the agent whose
    /// worktree it is: `agent-X` for the worktree `agent-X`, as Claude Code names both, else
    /// `agent-NAME`, an agent whose worktree it is not.
    pub fn claude_lock(&self, wt: &Path, pid: &str, start: &str) {
        let name = wt.file_name().unwrap().to_string_lossy();
        let agent = if name.starts_with("agent-") {
            name.to_string()
        } else {
            format!("agent-{name}")
        };
        self.lock(
            wt,
            &format!("claude agent {agent} (pid {pid} start {start})"),
        );
    }

    /// What the recent-activity rule reads: the admin dir's HEAD, index and logs/HEAD, and the top
    /// folder.
    pub fn activity(wt: &Path) -> Vec<PathBuf> {
        let admin = PathBuf::from(git(
            wt,
            &["rev-parse", "--path-format=absolute", "--git-dir"],
        ));
        vec![
            admin.join("HEAD"),
            admin.join("index"),
            admin.join("logs/HEAD"),
            wt.to_path_buf(),
        ]
    }

    /// Make `wt` look untouched for `hours`.
    pub fn age(wt: &Path, hours: f64) {
        let then = SystemTime::now() - Duration::from_secs_f64(hours * 3600.0);
        for path in Repo::activity(wt) {
            set_time(&path, then);
        }
    }

    /// Every worktree but the root and `fresh` was last touched hours ago: the other rules decide them.
    pub fn idle(&self, fresh: &[&Path]) {
        let fresh: BTreeSet<PathBuf> = fresh.iter().map(|f| f.canonicalize().unwrap()).collect();
        for wt in self.worktrees() {
            if wt != self.root && !fresh.contains(&wt) && wt.is_dir() {
                Repo::age(&wt, IDLE_HOURS);
            }
        }
    }

    pub fn worktrees(&self) -> BTreeSet<PathBuf> {
        git(&self.root, &["worktree", "list", "--porcelain"])
            .lines()
            .filter_map(|line| line.strip_prefix("worktree "))
            .map(|p| PathBuf::from(p).canonicalize().unwrap_or_else(|_| p.into()))
            .collect()
    }

    /// The environment that reaches this board, its clock where the board's is.
    pub fn vars(&self) -> BTreeMap<String, String> {
        let db = self.board.other_database();
        let mut vars: BTreeMap<String, String> = [
            ("PATH", std::env::var("PATH").unwrap()),
            ("HOME", self.dir.to_string_lossy().into_owned()),
            ("TASKS_SERVER", db.addr()),
            ("TASKS_DATABASE", db.database().to_string()),
            ("TASKS_NOW", self.board.now().to_string()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        // Child coverage belongs to the runner even though each child's board is isolated.
        for key in ["TZ", "LLVM_PROFILE_FILE"] {
            if let Ok(value) = std::env::var(key) {
                vars.insert(key.into(), value);
            }
        }
        vars
    }

    pub fn env(&self, cwd: &Path) -> Env {
        Env {
            vars: self.vars(),
            cwd: cwd.to_path_buf(),
        }
    }

    /// The sweep called as a function, every other worktree idle, nobody inside, with `change` made
    /// to the machine's options.
    pub fn sweep_with(&self, change: impl FnOnce(&mut Options)) -> Sweep {
        self.idle(&[]);
        self.sweep_now(change)
    }

    /// As `sweep_with`, the worktrees as they are.
    pub fn sweep_now(&self, change: impl FnOnce(&mut Options)) -> Sweep {
        self.sweep_from(&self.root, change)
    }

    /// As `sweep_now`, run from worktree `here`.
    pub fn sweep_from(&self, here: &Path, change: impl FnOnce(&mut Options)) -> Sweep {
        let mut options = Options::machine(&self.vars());
        options.cwds = Box::new(nobody_inside);
        options.occupied = Box::new(|_| Some(false));
        change(&mut options);
        sweep(here, here, &self.board.store, &options).unwrap()
    }

    pub fn sweep(&self) -> String {
        text(&self.sweep_with(|_| {}))
    }

    /// A session of `kind` bound to `wt`: its handle.
    pub fn bind(&self, wt: &Path) -> String {
        self.board.session_with(NewSession {
            worktree: wt.to_string_lossy().into_owned(),
            ..NewSession::new("worker")
        })
    }

    /// A task the owner made, ready.
    pub fn task(&self, slug: &str) {
        self.board
            .store
            .create(&NewTask::new(slug, "A task", "owner"))
            .unwrap();
    }

    pub fn assert_kept(&self, wt: &Path, text: &str) {
        assert!(
            self.worktrees().contains(&wt.canonicalize().unwrap()),
            "{}\n{text}",
            wt.display()
        );
        assert!(wt.is_dir(), "{text}");
        let shown = wt.to_string_lossy();
        assert!(
            !text.lines().any(
                |l| (l.starts_with("removed ") || l.starts_with("would remove "))
                    && l.contains(&*shown)
            ),
            "{text}"
        );
    }

    pub fn assert_removed(&self, wt: &Path, text: &str) {
        let listed = self.worktrees();
        assert!(
            !listed.contains(wt),
            "{}\n{text}\nstatus: {}",
            wt.display(),
            git(
                wt,
                &[
                    "status",
                    "--porcelain",
                    "--ignored=matching",
                    "--untracked-files=all"
                ]
            )
        );
        assert!(!wt.exists(), "{text}");
        assert!(text.contains(&*wt.to_string_lossy()), "{text}");
    }

    /// `git branch --list NAME`.
    pub fn branch(&self, name: &str) -> String {
        git(&self.root, &["branch", "--list", name])
    }
}

/// Write `text` to `path` under `dir`, making its folders.
pub fn write(dir: &Path, path: &str, text: &str) {
    let file = dir.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
}

/// Set `path`'s access and modification times to `when`; a folder too.
pub fn set_time(path: &Path, when: SystemTime) {
    if !path.exists() {
        return;
    }
    File::open(path)
        .unwrap()
        .set_times(FileTimes::new().set_accessed(when).set_modified(when))
        .unwrap();
}

/// A sweep's lines, one text.
pub fn text(result: &Sweep) -> String {
    result.lines().join("\n")
}
