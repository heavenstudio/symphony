//! What the symphony crate's tests share: a project tree of their own, and the CLI run in process.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub mod candidates;
pub mod items;
mod tree;
pub use tree::Tree;

/// What `file` reads from where it is to its end.
pub fn rest(mut file: std::fs::File) -> String {
    let mut text = String::new();
    std::io::Read::read_to_string(&mut file, &mut text).unwrap();
    text
}

/// `symphony status --root ROOT ARGS…`: its exit code, what it printed and what it said on stderr.
pub fn status(root: &Path, args: &[&str]) -> (i32, String, String) {
    let mut argv = vec!["--root".to_string(), root.display().to_string()];
    argv.extend(args.iter().map(|a| a.to_string()));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = symphony::cli::status_main(&argv, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

/// The same, asserting it exits 0, for its stdout.
pub fn status_ok(root: &Path, args: &[&str]) -> String {
    let (code, out, err) = status(root, args);
    assert_eq!(code, 0, "{err}");
    out
}

/// Whether `pattern` (a regex) matches somewhere in `text`.
pub fn matches(pattern: &str, text: &str) -> bool {
    regex::Regex::new(pattern).unwrap().is_match(text)
}

/// The text after the first `OPEN` in a board.
pub fn after_open(out: &str) -> &str {
    out.split_once("OPEN").map(|(_, rest)| rest).unwrap_or("")
}

/// No plugins: the register of a project that names none.
pub fn plain() -> symphony::plugins::Plugins {
    symphony::plugins::Plugins::default()
}

/// The repository this crate is in.
pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// The board `client` reaches, made at `now` by the build of schema `version` (this build's or an
/// older one): a board an older build left. A project's live board keeps the record of the schema
/// it was seen at (`symphony_board::live::seen`), which then says `version`, as that build's
/// opening wrote it. A board this build made whose `meta:schema` is lowered by hand is no such
/// board: its record says this build's schema, so it is a board that went back, and every opener
/// refuses it as a rollback.
pub fn older_board(
    client: symphony_board::rpc::Client,
    version: i64,
    now: f64,
) -> symphony_board::Store {
    let mut schema = symphony_board::Schema::this_build();
    assert!(
        version <= schema.version(),
        "no build of schema {version} is older than this one's {}",
        schema.version()
    );
    while schema.version() > version {
        schema = schema.before();
    }
    symphony_board::Store::opening(
        client,
        Box::new(move || now),
        Box::new(|_| false),
        "this-host",
        &symphony_board::Opening::write().at(schema),
    )
    .unwrap()
}

/// Build the scratch board's CLI without publishing it to the live heartbeat.
pub fn build_tasks(root: &Path) -> String {
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new(root.join("tools/rs-bin.sh")).args(["--build", "tasks"]),
    )
    .unwrap();
    assert!(
        done.status.success(),
        "rs-bin.sh --build tasks: {}",
        String::from_utf8_lossy(&done.stderr)
    );
    String::from_utf8(done.stdout).unwrap().trim().to_string()
}

/// The CLI for this tree's scratch boards: the one the workspace's test run built
/// (`SYMPHONY_TEST_TASKS_BINARY`, from a runner that built it first), else built once here without
/// changing the heartbeat link. Either is a freshly linked file, so it is run once ([`run_once`])
/// before it is handed out.
pub fn tasks_binary() -> &'static str {
    static BINARY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    BINARY.get_or_init(|| {
        let binary = std::env::var("SYMPHONY_TEST_TASKS_BINARY")
            .ok()
            .filter(|binary| !binary.is_empty())
            .unwrap_or_else(|| build_tasks(&repo()));
        run_once(Path::new(&binary));
        binary
    })
}

/// Run `binary --help` to its end, with no input and a bare environment (no board is named, so
/// none is reached). macOS assesses an executable file at its first run, one file at a time
/// machine-wide; under a land's load, behind every other new binary, that has held the CLI's
/// first run inside a case past its watchdog. Run here, in setup, it is paid before any case's
/// clock starts, and every later run of the same file starts at once.
pub fn run_once(binary: &Path) {
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new(binary)
            .arg("--help")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .stdin(symphony_board::sys::Stdio::Null),
    )
    .unwrap_or_else(|error| panic!("cannot run {} once: {error}", binary.display()));
    assert!(
        done.status.success(),
        "{} --help: {}{}",
        binary.display(),
        String::from_utf8_lossy(&done.stdout),
        String::from_utf8_lossy(&done.stderr)
    );
}

/// Resolve a setup-built CLI by this checkout's source hash, without building inside a test.
pub fn prebuilt_binary(root: &Path, name: &str) -> Result<PathBuf, String> {
    use std::os::unix::fs::PermissionsExt as _;
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new(root.join("tools/rs-bin.sh"))
            .args(["--where", name])
            .current_dir(root),
    )
    .map_err(|e| e.to_string())?;
    if !done.status.success() {
        return Err(format!(
            "rs-bin.sh --where {name}: {}",
            String::from_utf8_lossy(&done.stderr)
        ));
    }
    let path = PathBuf::from(
        String::from_utf8(done.stdout)
            .map_err(|e| e.to_string())?
            .trim(),
    );
    if !path.is_absolute()
        || !path
            .metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    {
        return Err(format!(
            "{name} has no setup-built executable for these sources: {}",
            path.display()
        ));
    }
    Ok(path)
}
