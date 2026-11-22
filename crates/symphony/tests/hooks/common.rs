//! Running the hook binary the way Claude Code does, and the project files the tests read.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

pub(crate) const BIN: &str = env!("CARGO_BIN_EXE_symphony");

/// An otherwise isolated child keeps the collector's per-process profile path. Dropping it makes
/// concurrent instrumented hooks contend on one default profile and loses their coverage.
pub(crate) fn retain_coverage_env(command: &mut Command) {
    if let Some(path) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", path);
    }
}

/// What one hook call returned.
#[derive(Debug)]
pub(crate) struct Done {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `command`, writing `raw` to its stdin.
pub(crate) fn spawn(mut command: Command, raw: &str) -> Done {
    let mut child = command.spawn().expect("the hook starts");
    let mut stdin = child.stdin.take().expect("a stdin pipe");
    // A hook may exit before reading all of it; the broken pipe that leaves is no failure of the hook.
    let _ = stdin.write_all(raw.as_bytes());
    drop(stdin);
    let out = child.wait_with_output().expect("the hook ends");
    Done {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// The settings' hook commands for `event`, as (matcher, command).
pub(crate) fn hook_commands(settings: &Value, event: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for group in settings["hooks"][event].as_array().into_iter().flatten() {
        let matcher = group["matcher"].as_str().unwrap_or_default().to_string();
        for hook in group["hooks"].as_array().into_iter().flatten() {
            found.push((
                matcher.clone(),
                hook["command"].as_str().unwrap_or_default().to_string(),
            ));
        }
    }
    found
}

/// A fresh folder under the system's temporary one, removed when dropped.
pub(crate) struct TempDir(pub PathBuf);

impl TempDir {
    pub(crate) fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let base = std::env::temp_dir()
            .canonicalize()
            .expect("the temporary folder");
        let path = base.join(format!("symphony-hooks-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a temporary folder");
        Self(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl std::ops::Deref for TempDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
