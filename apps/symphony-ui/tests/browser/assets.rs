//! The browser cases share one published client build or an explicitly supplied build.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// One module's files, as the shell's manifest names them: a program kind's (src/split.rs) are the
/// program's script and main module, which its kinds share, and its own chunk; a kind of the web
/// client (apps/symphony-web) is its script and styles, with no wasm (empty here).
#[derive(Debug, serde::Deserialize)]
pub(super) struct Module {
    pub script: String,
    #[serde(default)]
    pub wasm: String,
    pub styles: Vec<String>,
    #[serde(default)]
    pub chunk: Option<String>,
}

impl Module {
    /// A program kind's chunk.
    pub fn chunk(&self) -> &String {
        self.chunk.as_ref().expect("a program kind's chunk")
    }
}

/// The files of the module `kind` (`panel`, `overlay`, `board`…) in `dist`'s shell.
pub(super) fn module(dist: &Path, kind: &str) -> Module {
    modules(dist)
        .remove(kind)
        .unwrap_or_else(|| panic!("no {kind} module in the shell's manifest"))
}

/// Whether `dist`'s shell names `kind` a kind of the web client: a script with no wasm.
pub(super) fn web(dist: &Path, kind: &str) -> bool {
    modules(dist)
        .get(kind)
        .is_some_and(|module| module.wasm.is_empty())
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

/// The program's media layer's chunk in `dist` (src/split.rs `MediaPart`), which no kind is: the
/// shell's manifest never names it, the program script's map of its chunks does, as the kinds'
/// chunks are named there (the splitter's own loader, never used, names it `/./assets/…`).
pub(super) fn media_chunk(dist: &Path) -> String {
    let program = module(dist, "panel").script;
    let script = std::fs::read_to_string(dist.join(program.trim_start_matches('/')))
        .expect("the program's script");
    let named = |name: &&str| {
        name.starts_with("/assets/module_") && name.contains("_media-") && name.ends_with(".wasm")
    };
    let mut names = script.split('"').filter(named);
    let chunk = names
        .next()
        .expect("the media layer's chunk in the program's map");
    assert!(
        names.all(|other| other == chunk),
        "the program names two media chunks"
    );
    chunk.to_owned()
}

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
