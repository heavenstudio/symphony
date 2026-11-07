//! The crate's clippy settings (clippy.toml): the workspace's, then `disallowed-methods`, which
//! refuses a child made past [`super::command`] and [`super::command_with`], and the cwd scans whose
//! `lsof` keeps the session, however the path is spelled, imported, aliased or gated. The toolchain's
//! own clippy, run here over a probe crate that links the same `process-cwd` and reads a copy of
//! this crate's clippy.toml, says where it refuses one.

use std::fs;
use std::path::{Path, PathBuf};

fn crate_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Clippy reads the nearest clippy.toml alone: this crate's must hold the workspace's settings.
#[test]
fn the_crates_clippy_settings_start_with_the_workspaces() {
    let ours = fs::read_to_string(crate_dir().join("clippy.toml")).unwrap();
    let workspace = fs::read_to_string(crate_dir().join("../../clippy.toml")).unwrap();
    assert!(
        ours.starts_with(&workspace),
        "symphony/crates/tasks/clippy.toml must start with symphony/clippy.toml whole"
    );
}

/// A probe file: its name, its text, and each line clippy must refuse there with the method it
/// names.
type Probe = (&'static str, &'static str, &'static [(usize, &'static str)]);

const NEW: &str = "process_cwd::spawn::Command::new";
const STD_NEW: &str = "std::process::Command::new";

/// Each spelling of a child made past the two helpers that clippy must refuse, and the one place a
/// child may be made: an item that says so, which clippy lets pass.
const PROBES: [Probe; 9] = [
    (
        "spaced",
        "pub fn start() {\n    let _ = process_cwd::spawn::Command :: new(\"true\").spawn();\n}\n",
        &[(2, NEW)],
    ),
    (
        "line_broken",
        "pub fn start() {\n    let _ = process_cwd::spawn::Command::\n        new(\"true\").spawn();\n}\n",
        &[(2, NEW)],
    ),
    (
        "renamed",
        "use process_cwd::Command as Proc;\n\npub fn start() {\n    let _ = Proc::new(\"true\").spawn();\n}\n",
        &[(4, NEW)],
    ),
    (
        "after_a_test_module",
        "#[cfg(test)]\nmod tests {}\n\npub fn start() {\n    let _ = process_cwd::Command::new(\"true\").spawn();\n}\n",
        &[(5, NEW)],
    ),
    (
        "aliased",
        "type Proc = std::process::Command;\n\npub fn start() {\n    let _ = Proc::new(\"true\").status();\n}\n",
        &[(4, STD_NEW)],
    ),
    (
        "as_a_value",
        "pub fn start() {\n    let _ = [\"true\"].map(std::process::Command::new);\n}\n",
        &[(2, STD_NEW)],
    ),
    (
        "cleared",
        "pub fn clear(ours: &mut process_cwd::Command, theirs: &mut std::process::Command) {\n    \
         ours.env_clear();\n    theirs.env_clear();\n}\n",
        &[
            (2, "process_cwd::spawn::Command::env_clear"),
            (3, "std::process::Command::env_clear"),
        ],
    ),
    (
        "scans",
        "pub fn scan() {\n    let _ = process_cwd::process_cwds();\n    \
         let _ = process_cwd::occupied(std::path::Path::new(\"/\"));\n}\n",
        &[
            (2, "process_cwd::process_cwds"),
            (3, "process_cwd::occupied"),
        ],
    ),
    (
        "made_here",
        "#[expect(clippy::disallowed_methods, reason = \"the helper\")]\n\
         pub fn command(program: &str) -> process_cwd::Command {\n    \
         process_cwd::Command::new(program)\n}\n",
        &[],
    ),
];

/// A folder of the test's own under the temporary folder, removed when it drops.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The probe crate in `root`: a package of its own that links this workspace's `process-cwd` at
/// the versions its lockfile pins, with this crate's clippy.toml and one module per probe.
fn probe_crate(root: &Path) {
    let process_cwd = crate_dir().join("../process-cwd").canonicalize().unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"board-child-probe\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\
             publish = false\n\n[dependencies]\nprocess-cwd = {{ path = {:?} }}\n\n[workspace]\n",
            process_cwd.display().to_string()
        ),
    )
    .unwrap();
    fs::copy(
        crate_dir().join("../../Cargo.lock"),
        root.join("Cargo.lock"),
    )
    .unwrap();
    fs::copy(crate_dir().join("clippy.toml"), root.join("clippy.toml")).unwrap();
    let mut lib = String::new();
    for (name, text, _) in PROBES {
        fs::write(root.join("src").join(format!("{name}.rs")), text).unwrap();
        lib.push_str(&format!("pub mod {name};\n"));
    }
    fs::write(root.join("src/lib.rs"), lib).unwrap();
}

/// What clippy said of `src/`, one `file:line: method` per finding, sorted; a finding of anything
/// but a disallowed method keeps its whole message.
fn findings(said: &str) -> Vec<String> {
    let mut found: Vec<String> = said
        .lines()
        .filter_map(|line| line.strip_prefix("src/"))
        .map(|line| {
            let mut parts = line.splitn(4, ':');
            let (file, at) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
            let message = parts.nth(1).unwrap_or("").trim();
            let method = message
                .strip_prefix("warning: use of a disallowed method `")
                .and_then(|rest| rest.strip_suffix('`'))
                .unwrap_or(message);
            format!("{file}:{at}: {method}")
        })
        .collect();
    found.sort();
    found
}

#[test]
fn findings_are_read_as_file_line_and_method() {
    let said = "   Checking board-child-probe v0.0.0\n\
                src/a.rs:2:13: warning: use of a disallowed method `std::process::Command::new`\n\
                src/b.rs:7:1: warning: this expectation is unfulfilled\n\
                warning: `board-child-probe` (lib) generated 2 warnings\n";
    assert_eq!(
        findings(said),
        [
            "a.rs:2: std::process::Command::new",
            "b.rs:7: warning: this expectation is unfulfilled",
        ]
    );
}

/// Every spelling the probes hold is refused on its own line, naming the method, and nothing else
/// is said: an item that expects the lint passes, and every path in the settings resolves.
#[test]
fn clippy_refuses_a_child_made_past_the_helpers_however_it_is_spelled() {
    let scratch =
        Scratch(std::env::temp_dir().join(format!("board-child-probe-{}", std::process::id())));
    let _ = fs::remove_dir_all(&scratch.0);
    probe_crate(&scratch.0);
    let mut cargo = super::command(env!("CARGO"));
    cargo
        .args([
            "clippy",
            "--offline",
            "--quiet",
            "--message-format",
            "short",
        ])
        .current_dir(&scratch.0)
        .env("CARGO_TARGET_DIR", scratch.0.join("target"));
    for flags in [
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_BUILD_RUSTFLAGS",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "CLIPPY_CONF_DIR",
        "CLIPPY_ARGS",
    ] {
        cargo.env_remove(flags);
    }
    let done = super::output(&mut cargo).unwrap();
    let said = String::from_utf8_lossy(&done.stderr);
    assert!(done.status.success(), "{said}");
    let mut expected: Vec<String> = PROBES
        .iter()
        .flat_map(|(name, _, refused)| {
            refused
                .iter()
                .map(move |(at, method)| format!("{name}.rs:{at}: {method}"))
        })
        .collect();
    expected.sort();
    assert_eq!(findings(&said), expected, "{said}");
    assert!(!said.contains("clippy.toml"), "{said}");
}
