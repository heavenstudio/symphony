//! Explicit scratch tasks/tasks-attach recipes use this tree's build, preserving
//! output, exit code and stdin. A failed recipe adds just's diagnostic after
//! the binary's own stderr.

use crate::beat::bash;
use crate::{Case, Ran, run};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use tasks::Beat;
use tasks::cli::Env;

const FAKE: &str = env!("CARGO_BIN_EXE_fake-herdr");

/// What a process needs from this one's environment to start. Everything else it gets is the case's
/// own: an agent's own TASKS_SESSION, or the board it works on, must not reach it.
/// The common directory selects the launcher's immutable build cache, including in a generation;
/// it is operational plumbing, not a selection of the board the command reaches.
const PROCESS_ENV: [&str; 9] = [
    "PATH",
    "HOME",
    "TMPDIR",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TZ",
    "LLVM_PROFILE_FILE",
    "SF_GIT_COMMON_DIR",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap()
}

/// `program`, given only what it needs of this process's environment.
fn process(program: &Path) -> Command {
    let mut command = Command::new(program);
    command.env_clear();
    for key in PROCESS_ENV {
        if let Ok(value) = std::env::var(key) {
            command.env(key, value);
        }
    }
    command
}

/// Child side of the environment boundary probe, isolated from parallel tests' process state.
#[test]
fn process_environment_actor() {
    if std::env::var_os("SF_RECIPE_ENV_ACTOR").is_none() {
        return;
    }
    let output = process(Path::new("/usr/bin/env")).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let environment = String::from_utf8(output.stdout).unwrap();
    assert!(
        environment
            .lines()
            .any(|line| line == "SF_GIT_COMMON_DIR=/cache with spaces"),
        "{environment}"
    );
    for key in [
        "TASKS_SESSION",
        "TASKS_DATABASE",
        "TASKS_SERVER",
        "SF_RECIPE_ENV_ACTOR",
    ] {
        assert!(
            !environment
                .lines()
                .any(|line| line.starts_with(&format!("{key}="))),
            "{environment}"
        );
    }
}

#[test]
fn recipe_process_preserves_the_build_cache_without_inheriting_the_board() {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "recipe::process_environment_actor"])
        .env_clear()
        .env("SF_RECIPE_ENV_ACTOR", "1")
        .env("SF_GIT_COMMON_DIR", "/cache with spaces")
        .env("TASKS_SESSION", "unrelated-agent")
        .env("TASKS_DATABASE", "unrelated-board")
        .env("TASKS_SERVER", "unrelated-server")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("1 passed")
    );
}

/// The binary this tree's sources build, built once before any recipe runs it: a recipe's first run
/// of new sources says on stderr that it builds them, which the binary itself never says. Building
/// does not replace the live heartbeat link, including while recipe tests run concurrently.
fn built() {
    static BUILT: OnceLock<()> = OnceLock::new();
    BUILT.get_or_init(|| {
        let done = process(&root().join("tools/rs-bin.sh"))
            .args(["--build", "tasks"])
            .output()
            .unwrap();
        assert!(done.status.success(), "{done:?}");
    });
}

/// `just RECIPE ARGS` in this tree, given `env`'s variables and `input` on stdin.
fn just(env: &Env, args: &[&str], input: &str) -> Ran {
    built();
    let root = root();
    let mut command = process(Path::new("just"));
    command
        .arg("--justfile")
        .arg(root.join("justfile"))
        .arg("--working-directory")
        .arg(&root)
        .args(args)
        .envs(&env.vars)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let done = child.wait_with_output().unwrap();
    Ran {
        code: done.status.code().unwrap_or(-1),
        out: String::from_utf8(done.stdout).unwrap(),
        err: String::from_utf8(done.stderr).unwrap(),
    }
}

/// The recipe said what the binary said; on a failure `just` adds its own line after it.
fn same(recipe: &Ran, binary: &Ran) {
    assert_eq!(
        (recipe.code, &recipe.out),
        (binary.code, &binary.out),
        "{}",
        recipe.err
    );
    assert!(
        recipe.err.starts_with(&binary.err),
        "{:?} / {:?}",
        recipe.err,
        binary.err
    );
    assert_eq!(recipe.err == binary.err, binary.code == 0, "{}", recipe.err);
}

fn as_(env: &Env, who: &str) -> Env {
    let mut env = env.clone();
    env.vars.insert("TASKS_SESSION".into(), who.into());
    env
}

#[test]
fn just_tasks_is_the_binary() {
    let c = Case::new();
    let h = c.register("worker");
    c.ok(
        &h,
        &[
            "create",
            "sim/jump",
            "Jump",
            "--body",
            "a body with 'quotes' and $dollars",
        ],
    );
    let env = as_(&c.env, &h);
    for args in [
        &["list"][..],
        &["show", "sim/jump"],
        &["claim", "sim/nope"],
        &["no-such-command"],
        &["--as", &h, "inbox"],
    ] {
        let recipe: Vec<&str> = ["tasks"].iter().chain(args).copied().collect();
        same(&just(&env, &recipe, ""), &run(&env, args, ""));
    }
}

#[test]
fn just_tasks_passes_stdin_through() {
    let c = Case::new();
    let h = c.register("worker");
    let event = bash(&format!("TASKS_SESSION={h} just tasks inbox")).agent("agent-1");
    let ran = just(&c.env, &["tasks", "beat"], &event.0.to_string());
    assert_eq!((ran.code, ran.out.as_str(), ran.err.as_str()), (0, "", ""));
    let bound = c.store().agent_beat("agent-1", &Beat::default()).unwrap();
    assert_eq!(bound, Some(h));
}

#[test]
fn just_tasks_attach_is_the_binarys_attach() {
    let c = Case::new();
    for args in [&["--resume", "ghost-owl"][..], &["--bogus"]] {
        let recipe: Vec<&str> = ["tasks-attach"].iter().chain(args).copied().collect();
        let binary: Vec<&str> = ["attach"].iter().chain(args).copied().collect();
        same(&just(&c.env, &recipe, ""), &run(&c.env, &binary, ""));
    }
}

#[test]
fn just_tasks_runs_its_own_panes_with_no_launcher() {
    // cleanup is implemented by the tasks binary; the recipe names no launcher.
    let mut c = Case::new();
    let state = crate::scratch("recipe-herdr").join("herdr.json");
    std::fs::write(&state, r#"{"tabs": [], "panes": [], "next": 100}"#).unwrap();
    c.env.vars.insert("HERDR_BIN".into(), FAKE.into());
    c.env
        .vars
        .insert("FAKE_HERDR_STATE".into(), state.display().to_string());
    c.env.cwd = root();
    let recipe = just(&c.env, &["tasks", "cleanup", "--dry-run"], "");
    assert_eq!(recipe.code, 0, "{}", recipe.err);
    let binary = run(&c.env, &["cleanup", "--dry-run"], "");
    assert_eq!(
        (recipe.code, &recipe.out, &recipe.err),
        (binary.code, &binary.out, &binary.err)
    );
}

#[test]
fn just_tasks_scratch_build_preserves_the_live_heartbeat_link() {
    let c = Case::new();
    let root = root();
    // Under sf-verify this is the native target's captured generation, whose own .git is private.
    // The launcher's operational cache root must still be the supplied shared common directory.
    let common = std::env::var_os("SF_GIT_COMMON_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let output = Command::new("git")
                .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
                .current_dir(&root)
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            PathBuf::from(String::from_utf8(output.stdout).unwrap().trim())
        })
        .canonicalize()
        .unwrap();
    let private_cache = root.join(".git/sf-cargo/bin");
    let shared_cache = common.join("sf-cargo/bin");
    if private_cache != shared_cache {
        assert!(!private_cache.exists(), "{}", private_cache.display());
    }
    let before = std::fs::read_link(root.join(".target/bin/tasks")).ok();
    assert_eq!(just(&c.env, &["tasks", "--help"], "").code, 0);
    let selected = process(&root.join("tools/rs-bin.sh"))
        .args(["--where", "tasks"])
        .output()
        .unwrap();
    assert!(selected.status.success(), "{selected:?}");
    let helper = PathBuf::from(String::from_utf8(selected.stdout).unwrap().trim());
    assert!(helper.starts_with(&shared_cache), "{}", helper.display());
    assert!(helper.is_file(), "{}", helper.display());
    if private_cache != shared_cache {
        assert!(!private_cache.exists(), "{}", private_cache.display());
    }
    assert_eq!(
        std::fs::read_link(root.join(".target/bin/tasks")).ok(),
        before
    );
}
