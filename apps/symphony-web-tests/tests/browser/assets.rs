//! The browser cases share one published client build or an explicitly supplied build.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// One kind's files (apps/symphony-web), as the shell's manifest names them: its script, the
/// chunks that script imports at any depth and its styles, the shared stylesheet first.
#[derive(Debug, serde::Deserialize)]
pub(super) struct Module {
    pub script: String,
    pub imports: Vec<String>,
    pub styles: Vec<String>,
}

/// The files of the module `kind` (`panel`, `overlay`, `board`…) in `dist`'s shell.
pub(super) fn module(dist: &Path, kind: &str) -> Module {
    modules(dist)
        .remove(kind)
        .unwrap_or_else(|| panic!("no {kind} module in the shell's manifest"))
}

/// Every module of `dist`'s shell, by its kind.
fn modules(dist: &Path) -> std::collections::HashMap<String, Module> {
    let shell = std::fs::read_to_string(dist.join("index.html")).expect("the build's shell");
    let start = shell
        .find("/assets/symphony-shell-")
        .expect("the shell's script");
    let end = start + shell[start..].find(".js").expect("its name") + 3;
    let script = std::fs::read_to_string(dist.join(&shell[start + 1..end]))
        .expect("the shell's script file");
    let declared = "const modules = ";
    let start = script.find(declared).expect("the shell's module manifest") + declared.len();
    let line = &script[start..start + script[start..].find('\n').expect("one line")];
    serde_json::from_str(line.trim_end_matches(';')).expect("the manifest's JSON")
}

/// The client's build, made once for this run (or the one `SYMPHONY_UI_DIST` names).
pub(super) fn dist() -> &'static Path {
    static DIST: OnceLock<PathBuf> = OnceLock::new();
    DIST.get_or_init(|| {
        if let Some(named) = symphony_env::UI_DIST.os() {
            return PathBuf::from(named);
        }
        // As `just symphony-ui-dist` builds it: the client's own build script.
        let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let out = crate_dir.join("../../build/symphony-ui");
        let status = Command::new("bash")
            .arg(crate_dir.join("../symphony-web/build.sh"))
            .arg(&out)
            // Coverage instruments this native driver. Its flags must not reach the publisher the
            // build script builds (`symphony-build`), which is shipped to no page.
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("RUSTFLAGS")
            .status()
            .expect("run apps/symphony-web/build.sh");
        assert!(status.success(), "the client's build failed: {status}");
        out.join("public")
    })
}

/// The device cache's store as a page runs it, made once for this run: `shell/cache.ts` with its
/// types stripped by the client's pinned Node (`shell/strip.ts`), as the shell's build strips it
/// before minifying. Stripping leaves each other byte in its place, so a line or offset of this is
/// the source's, and the store's coverage names the source's lines.
pub(super) fn store() -> &'static str {
    static STORE: OnceLock<String> = OnceLock::new();
    STORE.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let done = Command::new("bash")
            .arg(root.join("tools/node.sh"))
            .args(["node", "--disable-warning=ExperimentalWarning"])
            .arg(root.join("apps/symphony-web/shell/strip.ts"))
            .arg(root.join("apps/symphony-web/shell/cache.ts"))
            .output()
            .expect("run apps/symphony-web/shell/strip.ts");
        assert!(
            done.status.success(),
            "the store does not strip: {}",
            String::from_utf8_lossy(&done.stderr)
        );
        String::from_utf8(done.stdout).expect("the stripped store is UTF-8")
    })
}
