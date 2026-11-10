//! Explicit scratch tasks/tasks-attach recipes use this tree's build, preserving
//! output, exit code and stdin. A failed recipe adds just's diagnostic after
//! the binary's own stderr.

use crate::beat::bash;
use crate::{Case, Ran, run};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use symphony_board::Beat;
use symphony_board::cli::Env;
use symphony_board::sys::Command;

mod input;

const FAKE: &str = env!("CARGO_BIN_EXE_fake-hc");

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
    let output = symphony_board::sys::output(&mut process(Path::new("/usr/bin/env"))).unwrap();
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
    let output = symphony_board::sys::output(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "recipe::process_environment_actor"])
            .env_clear()
            .env("SF_RECIPE_ENV_ACTOR", "1")
            .env("SF_GIT_COMMON_DIR", "/cache with spaces")
            .env("TASKS_SESSION", "unrelated-agent")
            .env("TASKS_DATABASE", "unrelated-board")
            .env("TASKS_SERVER", "unrelated-server"),
    )
    .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("1 passed")
    );
}

/// Stored diagnostics only; actual per-use selection, bytes and lifetime remain unknown.
#[derive(Debug)]
struct Receipt {
    status: Result<std::process::ExitStatus, (std::io::ErrorKind, Option<i32>)>,
    stdout_bytes: Option<usize>,
    stderr_bytes: Option<usize>,
    selection: Result<(PathBuf, String), &'static str>,
    file: Option<FileSummary>,
    inputs: input::Evidence,
}

#[derive(Debug)]
struct FileSummary {
    regular: bool,
    detail: String,
}

static WARM: OnceLock<Receipt> = OnceLock::new();

fn selected_path(stdout: &[u8]) -> Result<(PathBuf, String), &'static str> {
    let text = std::str::from_utf8(stdout).map_err(|_| "non-UTF-8 stdout")?;
    let line = text.strip_suffix('\n').ok_or("unterminated path")?;
    if line.contains('\n') || line.contains('\r') || line.contains('\0') {
        return Err("ambiguous path framing");
    }
    let path = PathBuf::from(line);
    let key = path
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix("tasks-"))
        .ok_or("not a tasks selector")?;
    if key.len() != 16
        || !key.bytes().all(|b| b.is_ascii_hexdigit())
        || !path.is_absolute()
        || !path.parent().is_some_and(|p| p.ends_with("sf-cargo/bin"))
        || path
            .components()
            .any(|p| p == std::path::Component::ParentDir)
    {
        return Err("not a public selector path");
    }
    Ok((path.clone(), key.to_owned()))
}

impl Receipt {
    fn of(result: &std::io::Result<std::process::Output>) -> Self {
        match result {
            Ok(done) => Self {
                status: Ok(done.status),
                stdout_bytes: Some(done.stdout.len()),
                stderr_bytes: Some(done.stderr.len()),
                selection: if done.status.success() {
                    selected_path(&done.stdout)
                } else {
                    Err("command failed")
                },
                file: None,
                inputs: input::Evidence::unknown(),
            },
            Err(error) => Self {
                status: Err((error.kind(), error.raw_os_error())),
                stdout_bytes: None,
                stderr_bytes: None,
                selection: Err("command unavailable"),
                file: None,
                inputs: input::Evidence::unknown(),
            },
        }
    }
}

fn file_summary(result: std::io::Result<std::fs::Metadata>) -> FileSummary {
    use std::os::unix::fs::MetadataExt;
    match result {
        Ok(m) => FileSummary {
            regular: m.is_file(),
            detail: format!(
                "device={} inode={} mode={:o} length={}",
                m.dev(),
                m.ino(),
                m.mode(),
                m.len()
            ),
        },
        Err(error) => FileSummary {
            regular: false,
            detail: format!(
                "unavailable: kind={:?} os_code={:?}",
                error.kind(),
                error.raw_os_error()
            ),
        },
    }
}

fn initialize_warm(
    cell: &OnceLock<Receipt>,
    attempt: impl FnOnce() -> (
        std::io::Result<std::process::Output>,
        Option<input::Capture>,
    ),
) -> &Receipt {
    cell.get_or_init(|| {
        let (result, capture) = attempt();
        let mut receipt = Receipt::of(&result);
        receipt.inputs = input::finish(
            capture,
            receipt
                .selection
                .as_ref()
                .ok()
                .map(|(path, key)| (path.as_path(), key.as_str())),
        );
        let done = result.unwrap_or_else(|_| panic!("warm command unavailable: {receipt:?}"));
        assert!(done.status.success(), "{receipt:?}");
        drop(done); // Raw buffers are not retained or dumped, including on failed initialization.
        if let Ok((path, _)) = &receipt.selection {
            // One admitted extra observation, possibly blocking; not a pathname lifetime seal.
            receipt.file = Some(file_summary(std::fs::metadata(path)));
        }
        receipt
    })
}

/// Build once before a recipe; --build does not publish the live heartbeat link.
fn built() {
    initialize_warm(&WARM, || {
        let root = root();
        let capture = input::Capture::new(&root, input::Caller::Warm);
        let mut command = process(&root.join("tools/rs-bin.sh"));
        command.args(["--build", "tasks"]);
        input::launch(capture, &mut std::io::stderr(), |capture| {
            input::bind(capture, &mut command);
            symphony_board::sys::output(&mut command)
        })
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
        .envs(&env.vars);
    let capture = input::Capture::new(&root, input::Caller::Recipe);
    let (done, capture) = input::launch(capture, &mut std::io::stderr(), |capture| {
        input::bind(capture, &mut command);
        let mut child = symphony_board::sys::spawn_piped(&mut command).unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        symphony_board::sys::wait_output(child).unwrap()
    });
    let (done, _evidence) = input::report_result(done, capture, None, &mut std::io::stderr());
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
        "{}; warm={:?}",
        recipe.err,
        WARM.get()
    );
    assert!(
        recipe.err.starts_with(&binary.err),
        "{:?} / {:?}; warm={:?}",
        recipe.err,
        binary.err,
        WARM.get()
    );
    assert_eq!(
        recipe.err == binary.err,
        binary.code == 0,
        "{}; warm={:?}",
        recipe.err,
        WARM.get()
    );
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
    assert_eq!(
        (ran.code, ran.out.as_str(), ran.err.as_str()),
        (0, "", ""),
        "warm={:?}",
        WARM.get()
    );
    let bound = c.store().agent_beat("agent-1", &Beat::default()).unwrap();
    assert_eq!(bound, Some(h), "warm={:?}", WARM.get());
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
fn just_tasks_runs_its_own_terminals_with_no_launcher() {
    // cleanup is implemented by the tasks binary; the recipe names no launcher.
    let mut c = Case::new();
    let state = crate::scratch("recipe-hc").join("hc.json");
    std::fs::write(&state, r#"{"terminals": [], "layout": {"spaces": []}}"#).unwrap();
    c.env.vars.insert("HC_BIN".into(), FAKE.into());
    c.env
        .vars
        .insert("FAKE_HC_STATE".into(), state.display().to_string());
    c.env.cwd = root();
    let recipe = just(&c.env, &["tasks", "cleanup", "--dry-run"], "");
    assert_eq!(recipe.code, 0, "{}; warm={:?}", recipe.err, WARM.get());
    let binary = run(&c.env, &["cleanup", "--dry-run"], "");
    assert_eq!(
        (recipe.code, &recipe.out, &recipe.err),
        (binary.code, &binary.out, &binary.err),
        "warm={:?}",
        WARM.get()
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
            let output = symphony_board::sys::output(
                Command::new("git")
                    .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
                    .current_dir(&root),
            )
            .unwrap();
            assert!(output.status.success(), "{output:?}");
            PathBuf::from(String::from_utf8(output.stdout).unwrap().trim())
        })
        .canonicalize()
        .unwrap();
    let private_cache = root.join(".git/sf-cargo/bin");
    let shared_cache = common.join("sf-cargo/bin");
    if private_cache != shared_cache {
        assert!(
            !private_cache.exists(),
            "{}; warm={:?}",
            private_cache.display(),
            WARM.get()
        );
    }
    let before_read = std::fs::read_link(root.join(".target/bin/tasks"));
    let before = before_read.as_ref().ok();
    assert_eq!(
        just(&c.env, &["tasks", "--help"], "").code,
        0,
        "warm={:?}",
        WARM.get()
    );
    let selected = symphony_board::sys::output(
        process(&root.join("tools/rs-bin.sh")).args(["--where", "tasks"]),
    );
    let mut receipt = Receipt::of(&selected);
    let selected = selected
        .unwrap_or_else(|_| panic!("selector unavailable: {receipt:?}; warm={:?}", WARM.get()));
    assert!(
        selected.status.success(),
        "{receipt:?}; warm={:?}",
        WARM.get()
    );
    let helper = PathBuf::from(
        std::str::from_utf8(&selected.stdout)
            .unwrap_or_else(|_| panic!("non-UTF-8 selector: {receipt:?}; warm={:?}", WARM.get()))
            .trim(),
    );
    assert!(
        helper.starts_with(&shared_cache),
        "{receipt:?}; warm={:?}",
        WARM.get()
    );
    // Reuse the followed metadata operation formerly inside Path::is_file, at this same point.
    let file = file_summary(std::fs::metadata(&helper));
    let regular = file.regular;
    receipt.file = Some(file);
    assert!(regular, "{receipt:?}; warm={:?}", WARM.get());
    if private_cache != shared_cache {
        assert!(
            !private_cache.exists(),
            "{}; selector={receipt:?}; warm={:?}",
            private_cache.display(),
            WARM.get()
        );
    }
    let after_read = std::fs::read_link(root.join(".target/bin/tasks"));
    assert_eq!(
        after_read.as_ref().ok(),
        before,
        "selector={receipt:?}; warm={:?}; link_error_kinds={:?}/{:?}",
        WARM.get(),
        before_read
            .as_ref()
            .err()
            .map(|e| (e.kind(), e.raw_os_error())),
        after_read
            .as_ref()
            .err()
            .map(|e| (e.kind(), e.raw_os_error()))
    );
}

fn receipt_output(code: i32, stdout: &[u8], stderr: &[u8]) -> std::process::Output {
    use std::os::unix::process::ExitStatusExt;
    std::process::Output {
        status: std::process::ExitStatus::from_raw(code << 8),
        stdout: stdout.to_vec(),
        stderr: stderr.to_vec(),
    }
}

#[test]
fn receipt_framing_preserves_public_path_spaces_and_rejects_ambiguous_names() {
    let path = "/public with spaces/sf-cargo/bin/tasks-0123456789abcdef";
    assert_eq!(
        selected_path(format!("{path}\n").as_bytes()),
        Ok((PathBuf::from(path), "0123456789abcdef".into()))
    );
    for invalid in [
        "",
        "/x\n",
        "relative/sf-cargo/bin/tasks-0123456789abcdef\n",
        "/a/../sf-cargo/bin/tasks-0123456789abcdef\n",
        "/wrong/tasks-0123456789abcdef\n",
        "/bad\0/sf-cargo/bin/tasks-0123456789abcdef\n",
        "/sf-cargo/bin/tasks-nothex\n",
        "/sf-cargo/bin/tasks-0123456789abcdeg\n",
        "/sf-cargo/bin/tasks-0123456789abcdef",
        "/sf-cargo/bin/tasks-0123456789abcdef\r\n",
        "/sf-cargo/bin/tasks-0123456789abcdef\nextra\n",
    ] {
        assert!(selected_path(invalid.as_bytes()).is_err(), "{invalid:?}");
    }
    assert_eq!(selected_path(&[0xff, b'\n']), Err("non-UTF-8 stdout"));
}

#[test]
fn receipt_failed_and_malformed_results_stay_unknown_without_leaking_stdio() {
    let raw = b"/secret=DO_NOT_PRINT\n";
    let done = receipt_output(2, b"/sf-cargo/bin/tasks-0123456789abcdef\n", raw);
    let receipt = Receipt::of(&Ok(done));
    assert_eq!(receipt.status.unwrap().code(), Some(2));
    assert_eq!(receipt.selection, Err("command failed"));
    let result = Ok(receipt_output(0, raw, raw));
    let receipt = Receipt::of(&result);
    assert_eq!(receipt.stdout_bytes, Some(raw.len()));
    assert_eq!(receipt.stderr_bytes, Some(raw.len()));
    assert!(receipt.selection.is_err(), "{receipt:?}");
    assert!(receipt.file.is_none(), "{receipt:?}");
    let display = format!("{receipt:?}");
    assert!(!display.contains("DO_NOT_PRINT"), "{display}");
    assert_eq!(result.as_ref().unwrap().stdout, raw);
    assert_eq!(result.as_ref().unwrap().stderr, raw);
    let failure = Receipt::of(&Err(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "SECRET",
    )));
    assert_eq!(
        failure.status,
        Err((std::io::ErrorKind::PermissionDenied, None))
    );
    assert_eq!((failure.stdout_bytes, failure.stderr_bytes), (None, None));
    assert_eq!(failure.selection, Err("command unavailable"));
    assert!(!format!("{failure:?}").contains("SECRET"), "{failure:?}");
}

#[test]
fn receipt_metadata_summaries_follow_the_existing_regular_file_predicate() {
    use std::os::unix::fs::symlink;
    let root = crate::scratch("receipt-metadata");
    let file = root.join("data");
    std::fs::write(&file, b"abc").unwrap();
    let summary = file_summary(std::fs::metadata(&file));
    assert!(summary.regular, "{summary:?}");
    for label in ["device=", "inode=", "mode=", "length=3"] {
        assert!(summary.detail.contains(label), "{summary:?}");
    }
    let alias = root.join("alias");
    symlink(&file, &alias).unwrap();
    assert!(file_summary(std::fs::metadata(alias)).regular);
    assert!(!file_summary(std::fs::metadata(&root)).regular);
    let missing = file_summary(std::fs::metadata(root.join("missing")));
    assert!(!missing.regular, "{missing:?}");
    assert!(missing.detail.contains("NotFound"), "{missing:?}");
    let denied = file_summary(Err(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "SECRET",
    )));
    assert!(!denied.regular, "{denied:?}");
    assert!(denied.detail.contains("PermissionDenied"), "{denied:?}");
    assert!(!denied.detail.contains("SECRET"), "{denied:?}");
}
