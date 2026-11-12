//! The browser cases share one published client build or an explicitly supplied build.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The client's build, made once for this run (or the one `SYMPHONY_UI_DIST` names).
pub(super) fn dist() -> &'static Path {
    static DIST: OnceLock<PathBuf> = OnceLock::new();
    DIST.get_or_init(|| {
        if let Some(named) = symphony_env::UI_DIST.os() {
            return PathBuf::from(named);
        }
        // As `just symphony-ui-dist` builds it: cargo's subcommand `symphony-ui dist`, found on PATH. This
        // binary already runs under the cargo wrapper, so it calls cargo itself, not the wrapper.
        let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let out = crate_dir.join("../../build/symphony-ui");
        let path = format!(
            "{}:{}",
            crate_dir.join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .args(["symphony-ui", "dist"])
            .arg(&out)
            .env("PATH", path)
            // Coverage instruments this native driver. Its flags cannot reach the separate
            // wasm release build: wasm32 has no profiler runtime and the page is shipped code.
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("RUSTFLAGS")
            .status()
            .expect("run cargo symphony-ui dist");
        assert!(status.success(), "the client's build failed: {status}");
        out.join("public")
    })
}
