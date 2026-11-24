//! The `tasks` command line over a real PostgreSQL server: the store's tests' server, one per
//! binary, a database of its own per test.

// How a test starts its own children ([`children::Child`]).
#[path = "../support/children.rs"]
mod children;

#[path = "../store/common.rs"]
#[allow(dead_code)]
mod common;

// The agents' programs a launch finds, stand-ins in a folder of the test's own.
#[path = "../support/stand_ins.rs"]
#[allow(dead_code)]
mod stand_ins;

mod agents;
mod api;
mod asks;
mod attach;
mod beat;
mod blocks;
mod boards;
mod close;
mod commands;
mod database;
mod detached;
mod finish;
mod gates;
mod handover;
mod machine_work;
// Each run's scratch in a folder of its own under the temp folder, swept when a killed run left it.
#[path = "../../../chrome-devtools/src/harness_scratch.rs"]
mod harness_scratch;
mod kind;
mod launch;
mod launch_rollback;
mod limits;
mod main_only;
mod monitor;
mod output;
mod people;
mod pg_lifecycle;
mod priority;
mod probes;
mod project;
mod pull;
mod record_script;
mod recorder;
mod recovery;
mod runs;
mod scratch_folder;
mod search;
mod settings;
mod slots;
mod stamps;
mod stats;
mod sweep;
mod test_lanes;
mod workflow;
mod write_load;

use crate::children::Child as _;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::AtomicUsize;
use symphony_board::Store;
use symphony_board::cli::{Env, connect};

/// What one run of `tasks` said: its exit code, its stdout and its stderr.
pub struct Ran {
    pub code: i32,
    pub out: String,
    pub err: String,
}

/// An environment reaching a database of its own on the tests' server, its clock at `common::NOW`.
pub fn env() -> Env {
    let db = common::postgresql();
    let vars: BTreeMap<String, String> = [
        ("TASKS_SERVER", db.addr()),
        ("TASKS_DATABASE", db.database().to_string()),
        ("TASKS_NOW", common::NOW.to_string()),
        (
            symphony_board::UNANSWERED_WAIT_VAR,
            common::LOOKUP_BACKSTOP.to_string(),
        ),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    Env {
        vars,
        cwd: std::env::temp_dir(),
    }
}

/// `tasks ARGS` with `input` on stdin.
pub fn run(env: &Env, args: &[&str], input: &str) -> Ran {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = symphony_board::cli::main(&args, env, &mut input.as_bytes(), &mut out, &mut err);
    Ran {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

/// A board of its own, and `tasks ARGS` run on it as a session.
pub struct Case {
    pub env: Env,
}

impl Default for Case {
    fn default() -> Case {
        Case { env: env() }
    }
}

impl Case {
    pub fn new() -> Case {
        Case::default()
    }

    /// `tasks ARGS` as `who` (no session if it is empty), `input` on stdin.
    pub fn run_as(&self, who: &str, args: &[&str], input: &str) -> Ran {
        let mut env = self.env.clone();
        if !who.is_empty() {
            env.vars.insert("TASKS_SESSION".into(), who.into());
        }
        run(&env, args, input)
    }

    /// What `tasks ARGS` as `who` printed, which must have succeeded.
    pub fn ok(&self, who: &str, args: &[&str]) -> String {
        let ran = self.run_as(who, args, "");
        assert_eq!(ran.code, 0, "{args:?} as {who:?}: {}", ran.err);
        ran.out
    }

    /// A session of `kind` with `extra` flags to `register`: its handle.
    pub fn register_with(&self, kind: &str, extra: &[&str]) -> String {
        let mut args = vec!["register", "--kind", kind];
        args.extend_from_slice(extra);
        self.ok("", &args).trim().to_string()
    }

    pub fn register(&self, kind: &str) -> String {
        self.register_with(kind, &[])
    }

    /// A session of `kind` registered before the first live line and beating at the case's clock,
    /// as an orchestrator that stays must be: it stays once it also waits, watches or holds a task.
    pub fn register_settled(&mut self, kind: &str) -> String {
        let now = self.env.vars["TASKS_NOW"].clone();
        let before = now.parse::<f64>().unwrap() - symphony_board::health::LIVE_SECONDS - 1.0;
        self.env.vars.insert("TASKS_NOW".into(), before.to_string());
        let handle = self.register(kind);
        self.env.vars.insert("TASKS_NOW".into(), now);
        self.store().touch(&handle).unwrap();
        handle
    }

    /// The board, as of the clock `TASKS_NOW` pins.
    pub fn store(&self) -> Store {
        connect(&self.env).unwrap()
    }

    /// The command line that reaches this board, as the commands a message names spell it: the
    /// server `TASKS_SERVER` names, on a database of its own.
    pub fn board(&self) -> String {
        format!(
            "TASKS_SERVER={} just tasks --database {}",
            self.env.vars["TASKS_SERVER"], self.env.vars["TASKS_DATABASE"]
        )
    }
}

/// The folder under the temp folder that holds every run's scratch, and only those.
pub const SCRATCH_HARNESS: &str = "tasks-cli";

/// This process's folder of scratch, once made.
static OWN_SCRATCH: OnceLock<PathBuf> = OnceLock::new();

/// This process's folder in the temp folder's [`SCRATCH_HARNESS`] folder, `<pid>-<n>`: made on first
/// use, once the folders of runs whose process is gone are swept (a killed run cannot remove its
/// own), and removed with everything in it when the binary exits, its tests passed or not.
pub fn own_scratch() -> &'static Path {
    OWN_SCRATCH.get_or_init(|| {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = harness_scratch::harness_root(&std::env::temp_dir(), SCRATCH_HARNESS)
            .unwrap_or_else(|error| panic!("the tests' scratch folder: {error}"));
        // Only the harness's folder is listed, so the sweep costs the same whatever the temp folder holds.
        harness_scratch::reap_dead(&root);
        let own = harness_scratch::create(&root, &NEXT)
            .unwrap_or_else(|error| panic!("a scratch folder in {}: {error}", root.display()));
        // SAFETY: atexit only records a function that takes nothing, to run once at the exit.
        assert_eq!(
            unsafe { libc::atexit(remove_own_scratch) },
            0,
            "the scratch's end is set"
        );
        own.canonicalize().unwrap()
    })
}

/// At the binary's exit: this process's scratch goes.
extern "C" fn remove_own_scratch() {
    if let Some(own) = OWN_SCRATCH.get() {
        let _ = std::fs::remove_dir_all(own);
    }
}

/// A folder of its own outside any repository (the target dir is inside this one), in this process's
/// [`own_scratch`], emptied first.
pub fn scratch(name: &str) -> PathBuf {
    let dir = own_scratch().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `git ARGS` in `dir`, as a committer named `t`, which must succeed.
pub fn git(dir: &Path, args: &[&str]) {
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::of("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "-C"])
            .arg(dir)
            .args(args),
    )
    .unwrap();
    assert!(done.status.success(), "git {args:?}: {done:?}");
}

/// The socket folder of a board no server answers: a connection to it is refused at once.
pub const UNREACHED_FOLDER: &str = "/nonexistent/symphony-unreached";

/// A board that cannot be reached ([`UNREACHED_FOLDER`]), as `TASKS_SERVER` names it.
pub const UNREACHED_SERVER: &str = "postgresql:/nonexistent/symphony-unreached";

/// The text of the project file of the project `id`, `rest` after its `[project]` lines, naming
/// the machine's PostgreSQL (`[board] postgresql`): the programs its board's server runs on,
/// whatever a child's `PATH` holds.
pub fn project_file(id: &str, rest: &str) -> String {
    let programs =
        symphony_board::server::postgres::discover_programs(&common::project_root()).unwrap();
    format!(
        "[project]\nid = \"{id}\"\n{rest}[board]\npostgresql = {:?}\n",
        programs.display().to_string()
    )
}

/// A repository at `path` with one commit on `main`.
pub fn repository(path: &Path) -> PathBuf {
    std::fs::create_dir_all(path).unwrap();
    git(path, &["init", "-q", "-b", "main"]);
    git(path, &["commit", "-q", "--allow-empty", "-m", "base"]);
    path.to_path_buf()
}
