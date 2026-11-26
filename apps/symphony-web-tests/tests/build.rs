//! The client's build (apps/symphony-web/build.sh, package.sh): one real Vite build made into one
//! shell and published, the manifest step alone over builds a case writes, the recipes that run
//! them, and what the shell's source holds of the wire types' and the client's rules.
//!
//! The real builds use the pinned Node and the lockfile's packages (npm.sh, offline once
//! installed) and the workspace's own publisher (`tools/rs-bin.sh --build symphony-build`); each
//! writes a folder of its case's own under the temporary folder, never the tree's `build/`.
#![forbid(unsafe_code)]

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// The client's kinds, each with the styles it loads after the shared stylesheet, in order: the
/// CSS of the chunks its entry imports (the media layer's, which brings the media sheet, for the
/// kinds that show media), then its entry's own. The task board's own sheet is
/// `src/styles/boardwait.css`; the agents page has none. Their common styles
/// (`src/styles/board.css`) are in the shared file. The menus (`overlay`) and the chat (`panel`)
/// are no page's but the shell's persistent kinds.
const KINDS: [(&str, &[&str]); 12] = [
    ("agents", &[]),
    ("board", &["board"]),
    ("docs", &["media-layer", "docs"]),
    ("overlay", &["overlay"]),
    ("panel", &["media-layer", "panel"]),
    ("people", &["people"]),
    ("queue", &["queue"]),
    ("reports", &["reports"]),
    ("review", &["media-layer"]),
    ("runs", &["runs"]),
    ("settings", &["settings"]),
    ("task", &["media-layer", "task"]),
];

/// The shared stylesheet's sheets in the order `src/styles/shared.css` writes them, each with a
/// selector only it opens with a rule for.
const SHARED: [(&str, &str); 7] = [
    ("board.css", ":root{--background:"),
    ("controls-theme.css", "html[data-theme=dark]{--dark:"),
    ("frame.css", ".app-header{"),
    ("textarea.css", ".dx-textarea{"),
    ("input.css", ".dx-input{"),
    ("button.css", ".dx-button{"),
    ("select-trigger.css", ".dx-select{"),
];

fn web() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../symphony-web")
}

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A folder of the test's own under the temporary folder, removed when it drops.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = std::env::temp_dir().join(format!("symphony-web-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Scratch(path.canonicalize().unwrap())
    }

    fn write(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn succeeded(done: &Output, what: &str) {
    assert!(
        done.status.success(),
        "{what} failed: {}",
        String::from_utf8_lossy(&done.stderr)
    );
}

/// `build.sh OUT` run from `from`, the coverage flags of this driver kept from the publisher it
/// builds.
fn build(from: &Path, out: &str) -> Command {
    let mut command = Command::new("bash");
    command
        .arg(web().join("build.sh"))
        .arg(out)
        .current_dir(from)
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTFLAGS");
    command
}

/// The shell a published folder's page loads: its manifest and the rest of the script.
fn shell_of(public: &Path) -> (serde_json::Value, String) {
    let index = fs::read_to_string(public.join("index.html")).unwrap();
    let script = index
        .split("src=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    assert!(script.starts_with("/assets/symphony-shell-"), "{script}");
    let shell = fs::read_to_string(public.join(script.trim_start_matches('/'))).unwrap();
    let (manifest, body) = shell
        .strip_prefix("const modules = ")
        .expect("the shell starts with its manifest line")
        .split_once(";\n")
        .unwrap();
    (serde_json::from_str(manifest).unwrap(), body.to_owned())
}

/// Every file under `dir`, by its path there, with its bytes.
fn files_in(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut found = BTreeMap::new();
    let mut folders = vec![dir.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
            } else {
                let bytes = fs::read(&path).unwrap();
                found.insert(path.strip_prefix(dir).unwrap().to_path_buf(), bytes);
            }
        }
    }
    found
}

/// The one real build: OUT is named from where the script runs, and its `public` holds the page,
/// the shell with each kind's script, imports and styles, the shared stylesheet in its written
/// order, and a `builds.json` naming every file, with nothing of Vite's own beside them.
#[test]
fn a_build_publishes_every_kind_over_one_shared_stylesheet_into_out_as_named_from_where_it_runs() {
    let scratch = Scratch::new("build");
    let done = build(&scratch.0, "build/ui")
        .output()
        .expect("run build.sh");
    succeeded(&done, "build.sh");
    assert!(done.stdout.is_empty(), "{done:?}");
    let public = scratch.0.join("build/ui/public");
    let file = |path: &str| {
        fs::read(public.join(path.trim_start_matches('/')))
            .unwrap_or_else(|e| panic!("{path}: {e}"))
    };
    let (manifest, body) = shell_of(&public);
    // Each of the client's kinds, and nothing else: the shared stylesheet's entry is no kind.
    let kinds: Vec<&str> = manifest
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(kinds, KINDS.map(|(kind, _)| kind), "{manifest}");
    // The rest of the shell is the store and the shell's source, minified: no line of either is
    // left as written, and the manifest's placeholder is gone.
    assert!(!body.contains("__MODULES__"), "{body}");
    assert!(body.contains("symphonyCache"), "the client cache's store");
    assert!(body.contains("reviewShell"), "the shell");
    bundled_notices(&public, &body);
    let source = fs::read_to_string(web().join("shell/shell.ts")).unwrap();
    let store = fs::read_to_string(web().join("shell/cache.ts")).unwrap();
    assert!(
        body.len() * 2 < source.len() + store.len(),
        "the shell is minified: {} bytes of {}",
        body.len(),
        source.len() + store.len()
    );
    assert!(
        body.trim_end().ends_with(&format!(
            "// Notice styles: {}",
            &sha256(&workspace().join("apps/symphony-server/src/notices.css"))[..16]
        )),
        "the shell's last line names the notice styles it was built with"
    );

    let shared = manifest["panel"]["styles"][0].as_str().unwrap();
    assert!(
        shared.starts_with("/assets/shared-") && shared.ends_with(".css"),
        "{shared}"
    );
    let sheet = String::from_utf8(file(shared)).unwrap();
    let mut last = 0;
    for (name, opens) in SHARED {
        let at = sheet
            .find(opens)
            .unwrap_or_else(|| panic!("the shared stylesheet holds {name} ({opens})"));
        assert!(at >= last, "{name} comes in its written order");
        last = at;
    }
    assert_eq!(
        sheet.lines().count(),
        1,
        "the shared stylesheet is minified"
    );

    for (kind, sheets) in KINDS {
        let script = manifest[kind]["script"].as_str().unwrap();
        assert!(
            script.starts_with(&format!("/assets/{kind}-")) && script.ends_with(".js"),
            "{script}"
        );
        let module = String::from_utf8(file(script)).unwrap();
        assert!(
            module.contains(" as run") && module.contains(" as stop"),
            "{kind}"
        );
        assert!(manifest[kind].get("wasm").is_none(), "{kind}");
        // The chunks the script imports, each once, which the shell preloads beside it: published
        // files that import none outside them.
        let imports: Vec<_> = manifest[kind]["imports"]
            .as_array()
            .unwrap_or_else(|| panic!("{kind} names its imports"))
            .iter()
            .map(|chunk| chunk.as_str().unwrap())
            .collect();
        let mut named = imports.clone();
        named.sort_unstable();
        named.dedup();
        assert_eq!(named.len(), imports.len(), "{kind}: {imports:?}");
        for inner in common::imported(script, &module) {
            assert!(imports.contains(&inner.as_str()), "{kind} imports {inner}");
        }
        for chunk in &imports {
            let chunk_module = String::from_utf8(file(chunk)).unwrap();
            for inner in common::imported(chunk, &chunk_module) {
                assert!(
                    imports.contains(&inner.as_str()),
                    "{kind}: {chunk} imports {inner}"
                );
            }
        }
        let styles = manifest[kind]["styles"].as_array().unwrap();
        assert_eq!(styles[0], shared, "{kind}");
        let own: Vec<_> = styles[1..]
            .iter()
            .map(|style| style.as_str().unwrap())
            .collect();
        assert_eq!(
            own.len(),
            sheets.len(),
            "{kind} loads the shared stylesheet, then its own: {styles:?}"
        );
        for (style, sheet) in own.into_iter().zip(sheets) {
            assert!(
                style.starts_with(&format!("/assets/{sheet}-")),
                "{kind}: {style}"
            );
            assert!(!file(style).is_empty(), "{style}");
        }
    }

    let board = manifest["board"]["styles"][1].as_str().unwrap();
    let waiting = String::from_utf8(file(board)).unwrap();
    assert!(
        waiting.contains(".waiting-board{"),
        "the board's own stylesheet holds boardwait.css"
    );
    assert!(
        !sheet.contains(".waiting-board{"),
        "the waiting board's rules stay out of the shared stylesheet"
    );

    // builds.json names this build, every file under assets/ and no other; nothing of Vite's own
    // (its manifest) is published.
    let builds: serde_json::Value =
        serde_json::from_slice(&fs::read(public.join("builds.json")).unwrap()).unwrap();
    let listed: Vec<&str> = builds["builds"][0]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|name| name.as_str().unwrap())
        .collect();
    let mut present: Vec<String> = fs::read_dir(public.join("assets"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    present.sort();
    assert_eq!(listed, present);
    assert!(
        present
            .iter()
            .all(|name| !name.ends_with(".wasm") && !name.contains("-dxh")),
        "{present:?}"
    );
    let mut top: Vec<String> = fs::read_dir(&public)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    top.sort();
    assert_eq!(top, ["assets", "builds.json", "index.html"]);

    // A second build of the same sources, into the same OUT, publishes the same bytes.
    let before = files_in(&public);
    succeeded(
        &build(&scratch.0, "build/ui")
            .output()
            .expect("run build.sh"),
        "build.sh again",
    );
    let after = files_in(&public);
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>()
    );
    for (path, bytes) in &before {
        // builds.json's list grows by the build kept; every other file is the same bytes.
        if path != Path::new("builds.json") {
            assert!(after[path] == *bytes, "{}", path.display());
        }
    }
}

fn sha256(path: &Path) -> String {
    let done = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .expect("run shasum");
    succeeded(&done, "shasum");
    String::from_utf8(done.stdout).unwrap()
}

/// The build's own inventory, not package.json's development tools: every bundled package has
/// its installed text verbatim, with the copied controls' provenance and selected MIT text.
fn bundled_notices(public: &Path, shell: &str) {
    let relative = shell
        .lines()
        .find_map(|line| line.strip_prefix("// Third-party notices: /"))
        .expect("the shell names its bundled notices");
    let path = public.join(relative);
    assert_eq!(
        relative,
        format!("assets/third-party-{}.txt", &sha256(&path)[..16])
    );
    let notice = fs::read_to_string(&path).unwrap();
    let inventory: Vec<serde_json::Value> = serde_json::from_slice(
        &fs::read(
            public
                .parent()
                .unwrap()
                .join(".modules/web/.vite/licenses.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        inventory
            .iter()
            .map(|entry| entry["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        // Vite orders the inventory by name@version, so '-' precedes '@'.
        ["react-dom", "react", "scheduler"],
        "review any change to the actual bundled dependency inventory"
    );
    for entry in inventory {
        let name = entry["name"].as_str().unwrap();
        let installed = web().join("node_modules").join(name);
        let package: serde_json::Value =
            serde_json::from_slice(&fs::read(installed.join("package.json")).unwrap()).unwrap();
        let text = fs::read_to_string(installed.join("LICENSE")).unwrap();
        assert_eq!(entry["version"], package["version"], "{name}");
        assert_eq!(entry["identifier"], package["license"], "{name}");
        assert_eq!(entry["text"], text.trim(), "{name}");
        assert!(
            notice.contains(&format!(
                "{name}@{} (MIT)\n\n{}\n",
                entry["version"].as_str().unwrap(),
                text.trim()
            )),
            "{name}"
        );
    }
    assert!(notice.contains(&fs::read_to_string(web().join("copied-controls.txt")).unwrap()));
    assert!(notice.ends_with(&format!(
        "{}\n",
        fs::read_to_string(web().join("src/styles/LICENSE-MIT")).unwrap()
    )));
    assert!(
        !notice.contains(&web().display().to_string()),
        "no checkout path"
    );
}

/// Two builds into one OUT at once take turns at its lock, and each publishes a whole client.
#[test]
fn two_builds_into_one_out_take_turns_and_both_publish() {
    let scratch = Scratch::new("build-turns");
    let start = || {
        build(&scratch.0, "out")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("run build.sh")
    };
    let (first, second) = (start(), start());
    for run in [first, second] {
        let done = run.wait_with_output().unwrap();
        succeeded(&done, "a build beside another");
        let said = String::from_utf8_lossy(&done.stderr);
        assert!(said.contains("one SPA shell: "), "{said}");
    }
    let public = scratch.0.join("out/public");
    let (manifest, _) = shell_of(&public);
    assert_eq!(manifest.as_object().unwrap().len(), KINDS.len());
    let stages: Vec<_> = fs::read_dir(scratch.0.join("out"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.starts_with(".public-stage."))
        .collect();
    assert!(stages.is_empty(), "no staging folder is left: {stages:?}");
}

#[test]
fn a_build_without_its_one_argument_is_refused() {
    for args in [&[][..], &[""], &["one", "two"]] {
        let done = Command::new("bash")
            .arg(web().join("build.sh"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(done.status.code(), Some(2), "{args:?}");
        assert_eq!(
            String::from_utf8_lossy(&done.stderr),
            "usage: build.sh OUT\n",
            "{args:?}"
        );
    }
}

/// The web client's build (the real Vite build build.sh runs) is the same bytes whatever NODE_ENV
/// the shell that starts it exports: a shell's `development` would otherwise build React's
/// development code, twice the size, and a release run's check from a clean shell would find the
/// published client is not what it builds.
#[test]
fn the_web_clients_build_is_the_same_bytes_whatever_node_env_says() {
    let scratch = Scratch::new("node-env");
    let build = |name: &str, node_env: Option<&str>| {
        let out = scratch.0.join(name);
        let mut npm = Command::new("bash");
        npm.arg(web().join("npm.sh"))
            .args(["run", "--silent", "build", "--", "--outDir"])
            .arg(&out)
            .arg("--emptyOutDir")
            .env_remove("NODE_ENV");
        if let Some(value) = node_env {
            npm.env("NODE_ENV", value);
        }
        succeeded(&npm.output().expect("run npm.sh"), name);
        files_in(&out)
    };
    let plain = build("plain", None);
    assert!(
        plain
            .keys()
            .any(|path| path.extension().is_some_and(|e| e == "js")),
        "{:?}",
        plain.keys()
    );
    for node_env in ["development", "test"] {
        let built = build(node_env, Some(node_env));
        assert_eq!(
            built.keys().collect::<Vec<_>>(),
            plain.keys().collect::<Vec<_>>(),
            "NODE_ENV={node_env}"
        );
        for (path, bytes) in &plain {
            assert!(
                built[path] == *bytes,
                "NODE_ENV={node_env}: {}",
                path.display()
            );
        }
    }
}

/// A Vite build as package.sh reads one, written by the case: `OUT/.modules/web` with a manifest
/// of two kinds over a chunk they share, and the shared stylesheet's entry unless `shared` is
/// false.
fn modules(scratch: &Scratch, shared: bool, kinds: bool) -> PathBuf {
    let mut manifest = serde_json::Map::new();
    if kinds {
        for (kind, css) in [("board", None), ("queue", Some("assets/queue-22.css"))] {
            let file = format!("assets/{kind}-11.js");
            scratch.write(
                &format!("out/.modules/web/{file}"),
                "export{a as run,b as stop};",
            );
            let mut entry = serde_json::json!({
                "file": file, "name": kind, "isEntry": true, "imports": ["_shared-33.js"],
            });
            if let Some(css) = css {
                scratch.write(&format!("out/.modules/web/{css}"), ".queue{}");
                entry["css"] = serde_json::json!([css]);
            }
            manifest.insert(format!("src/kinds/{kind}.tsx"), entry);
        }
        scratch.write("out/.modules/web/assets/shared-33.js", "export const a=1;");
        scratch.write("out/.modules/web/assets/layer-44.css", ".layer{}");
        manifest.insert(
            "_shared-33.js".into(),
            serde_json::json!({"file": "assets/shared-33.js", "name": "shared", "css": ["assets/layer-44.css"]}),
        );
    }
    if shared {
        scratch.write("out/.modules/web/assets/shared-55.css", ".frame{}");
        manifest.insert(
            "src/styles/shared.css".into(),
            serde_json::json!({"file": "assets/shared-55.css", "name": "shared", "isEntry": true}),
        );
    }
    fs::create_dir_all(scratch.0.join("out/.modules/web/assets")).unwrap();
    scratch.write(
        "out/.modules/web/.vite/manifest.json",
        &serde_json::Value::Object(manifest).to_string(),
    );
    scratch.write(
        "out/.modules/web/.vite/licenses.json",
        r#"[{"name":"fixture-library","version":"1.2.3","identifier":"MIT","text":"Exact upstream permission text."}]"#,
    );
    scratch.0.join("out")
}

/// A publisher standing in for `symphony-build`: it moves the stage into place, as the real one
/// does over an empty folder, and says what it was asked.
fn publisher(scratch: &Scratch) -> PathBuf {
    let path = scratch.write(
        "publisher",
        "#!/bin/bash\nset -e\n[ \"$1\" = publish-client ]\nrm -rf \"$3\"\nmv \"$2\" \"$3\"\n",
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// The lockfile's packages installed in the client's folder, as npm.sh installs them on its first
/// run (`npm.sh --version`, whose own output is no case's). A first run of this target installs
/// them in whichever case reaches npm.sh first, and a case that reaches it meanwhile waits its
/// turn and says so on its stderr: a case that reads what a step says on stderr installs first,
/// so it reads the step's words alone, on a first run as on every other.
fn installed() {
    let done = Command::new("bash")
        .arg(web().join("npm.sh"))
        .arg("--version")
        .output()
        .expect("run npm.sh");
    succeeded(&done, "npm.sh --version");
}

fn package(out: &Path, publisher: &Path) -> Output {
    Command::new("bash")
        .arg(web().join("package.sh"))
        .arg(out)
        .arg(publisher)
        .output()
        .expect("run package.sh")
}

fn report(args: &[&Path]) -> Output {
    Command::new("bash")
        .arg(web().join("report.sh"))
        .args(args)
        .output()
        .expect("run report.sh")
}

/// The bundle report of a build of the case's own: each kind's first visit, the shared stylesheet
/// first, with its files' bytes, then the build's every file once; and it writes nothing.
#[test]
fn the_bundle_report_says_each_kinds_first_visit_and_the_builds_files() {
    let scratch = Scratch::new("report");
    let out = modules(&scratch, true, true);
    let before = files_in(&out);
    let done = report(&[&out]);
    succeeded(&done, "report.sh");
    let said = String::from_utf8(done.stdout).unwrap();
    let lines: Vec<&str> = said.lines().collect();
    let begins = |line: &str, start: &str| assert!(line.starts_with(start), "{line}: {said}");
    assert_eq!(lines.len(), 12, "{said}");
    begins(lines[0], "board: first visit 4 files, 60 bytes, ");
    begins(lines[1], "  assets/shared-55.css  8 bytes, ");
    begins(lines[2], "  assets/layer-44.css  8 bytes, ");
    begins(lines[3], "  assets/shared-33.js  17 bytes, ");
    begins(lines[4], "  assets/board-11.js  27 bytes, ");
    begins(lines[5], "queue: first visit 5 files, 68 bytes, ");
    begins(lines[7], "  assets/layer-44.css  8 bytes, ");
    begins(lines[8], "  assets/queue-22.css  8 bytes, ");
    begins(lines[11], "the build: 6 files, 95 bytes, ");
    assert!(
        lines.iter().all(|line| line.ends_with(" gzipped")),
        "{said}"
    );
    assert_eq!(files_in(&out), before, "the report wrote in the build");
}

/// Without its one folder, or over a folder that holds no build, the report is refused.
#[test]
fn a_bundle_report_of_no_build_is_refused() {
    let scratch = Scratch::new("report-none");
    let empty = scratch.0.join("empty");
    fs::create_dir_all(&empty).unwrap();
    for (args, said) in [
        (vec![], "usage: report.sh OUT\n".to_string()),
        (
            vec![scratch.0.join("missing")],
            "usage: report.sh OUT\n".to_string(),
        ),
        (
            vec![empty.clone()],
            format!("symphony-web: no bundle report of {}: ", empty.display()),
        ),
    ] {
        let args: Vec<&Path> = args.iter().map(PathBuf::as_path).collect();
        let done = report(&args);
        assert_eq!(done.status.code(), Some(2), "{args:?}: {done:?}");
        assert!(done.stdout.is_empty(), "{args:?}: {done:?}");
        let stderr = String::from_utf8_lossy(&done.stderr);
        assert!(stderr.starts_with(&said), "{args:?}: {stderr}");
    }
    // A build with no shared stylesheet is no build to report either.
    let out = modules(&scratch, false, true);
    let done = report(&[&out]);
    assert_eq!(done.status.code(), Some(2), "{done:?}");
    assert!(
        String::from_utf8_lossy(&done.stderr)
            .ends_with("the manifest has no shared stylesheet (src/styles/shared.css)\n"),
        "{done:?}"
    );
}

/// The manifest step over a build of the case's own: each entry but the shared stylesheet's is a
/// kind, its imports the chunks it reaches, its styles the shared sheet, then its chunks', then
/// its own.
#[test]
fn the_manifest_step_names_each_kinds_script_imports_and_styles_shared_sheet_first() {
    let scratch = Scratch::new("package");
    let out = modules(&scratch, true, true);
    // An assembly a publisher that was killed left, and one a live process still owns.
    let dead = out.join(".public-stage.999999999.left");
    let live = out.join(format!(".public-stage.{}.mine", std::process::id()));
    for stage in [&dead, &live] {
        fs::create_dir_all(stage).unwrap();
    }
    installed();
    let done = package(&out, &publisher(&scratch));
    succeeded(&done, "package.sh");
    assert!(done.stdout.is_empty(), "{done:?}");
    assert_eq!(
        String::from_utf8_lossy(&done.stderr),
        format!(
            "symphony-web: the client's kinds (board, queue) over one shared stylesheet, one SPA \
             shell: {}/public\n",
            out.display()
        )
    );
    assert!(!dead.exists(), "a dead publisher's assembly is removed");
    assert!(live.exists(), "a live publisher's assembly is left alone");
    let (manifest, _) = shell_of(&out.join("public"));
    assert_eq!(
        manifest,
        serde_json::json!({
            "board": {
                "script": "/assets/board-11.js",
                "imports": ["/assets/shared-33.js"],
                "styles": ["/assets/shared-55.css", "/assets/layer-44.css"],
            },
            "queue": {
                "script": "/assets/queue-11.js",
                "imports": ["/assets/shared-33.js"],
                "styles": ["/assets/shared-55.css", "/assets/layer-44.css", "/assets/queue-22.css"],
            },
        })
    );
    let index = fs::read_to_string(out.join("public/index.html")).unwrap();
    let notices =
        fs::read_to_string(workspace().join("apps/symphony-server/src/notices.css")).unwrap();
    assert!(
        index.contains(&format!(
            "<style id=\"shell-notices-style\">{}</style>",
            notices.trim_end()
        )),
        "the page carries the server's notice styles"
    );
}

/// Packaging refuses a missing or incomplete inventory instead of silently publishing a client
/// without notices. A new license needs a deliberate selection before it can ship.
#[test]
fn incomplete_bundled_notices_refuse_packaging_without_replacing_the_client() {
    let valid = serde_json::json!({
        "name": "fixture-library", "version": "1.2.3", "identifier": "MIT", "text": "Permission text."
    });
    let mut cases = vec![
        None,
        Some("bad json".into()),
        Some("{}".into()),
        Some("[]".into()),
    ];
    for field in ["name", "version", "identifier", "text"] {
        let mut missing = valid.clone();
        missing.as_object_mut().unwrap().remove(field);
        cases.push(Some(serde_json::json!([missing]).to_string()));
        for value in [serde_json::json!(""), serde_json::json!(42)] {
            let mut wrong = valid.clone();
            wrong[field] = value;
            cases.push(Some(serde_json::json!([wrong]).to_string()));
        }
    }
    let mut other = valid.clone();
    other["identifier"] = serde_json::json!("MIT OR Apache-2.0");
    cases.push(Some(serde_json::json!([other]).to_string()));
    // One good entry must not hide an incomplete second one.
    cases.push(Some(
        serde_json::json!([valid, {"name": "missing-text"}]).to_string(),
    ));
    for (case, inventory) in cases.into_iter().enumerate() {
        let scratch = Scratch::new(&format!("notice-refused-{case}"));
        let out = modules(&scratch, true, true);
        let path = out.join(".modules/web/.vite/licenses.json");
        if let Some(inventory) = inventory {
            fs::write(&path, inventory).unwrap();
        } else {
            fs::remove_file(&path).unwrap();
        }
        scratch.write("out/public/index.html", "last good client");
        let before = files_in(&out.join("public"));
        let done = package(&out, &publisher(&scratch));
        assert_eq!(done.status.code(), Some(2), "case {case}: {done:?}");
        assert!(
            String::from_utf8_lossy(&done.stderr)
                .contains("missing or incomplete bundled MIT license inventory"),
            "case {case}: {done:?}"
        );
        assert_eq!(files_in(&out.join("public")), before, "case {case}");
        assert!(fs::read_dir(&out).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".public-stage.")
        }));
    }
}

#[test]
fn bundled_notices_are_sorted_and_part_of_the_builds_identity() {
    let scratch = Scratch::new("notice-order");
    let out = modules(&scratch, true, true);
    let first = serde_json::json!({"name":"alpha", "version":"1", "identifier":"MIT", "text":"Alpha license"});
    let second = serde_json::json!({"name":"zulu", "version":"2", "identifier":"MIT", "text":"Zulu license"});
    let inventory = out.join(".modules/web/.vite/licenses.json");
    let publisher = publisher(&scratch);
    fs::write(&inventory, serde_json::json!([second, first]).to_string()).unwrap();
    succeeded(&package(&out, &publisher), "package notices");
    let before = files_in(&out.join("public"));
    fs::write(&inventory, serde_json::json!([first, second]).to_string()).unwrap();
    succeeded(&package(&out, &publisher), "package reordered notices");
    assert_eq!(files_in(&out.join("public")), before);
    let (manifest, shell) = shell_of(&out.join("public"));
    assert_eq!(manifest.as_object().unwrap().len(), 2, "no new page kind");
    let notice = shell
        .lines()
        .find_map(|line| line.strip_prefix("// Third-party notices: /"))
        .unwrap();
    let text = fs::read_to_string(out.join("public").join(notice)).unwrap();
    assert!(
        text.find("alpha@1 (MIT)\n\nAlpha license\n").unwrap()
            < text.find("zulu@2 (MIT)\n\nZulu license\n").unwrap()
    );
    let builds: serde_json::Value =
        serde_json::from_slice(&fs::read(out.join("public/builds.json")).unwrap()).unwrap();
    assert!(
        builds["builds"][0]["files"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!(notice.strip_prefix("assets/").unwrap()))
    );
    let mut changed = first;
    changed["text"] = serde_json::json!("Changed upstream license text");
    fs::write(&inventory, serde_json::json!([changed, second]).to_string()).unwrap();
    succeeded(&package(&out, &publisher), "package changed notice");
    let after = files_in(&out.join("public"));
    assert_ne!(
        after[Path::new("index.html")],
        before[Path::new("index.html")],
        "notice changes select a new shell/build"
    );
    assert!(
        !out.join("public").join(notice).exists(),
        "the new stage carries its own notice"
    );
}

/// A build the manifest step cannot make a client of is refused, saying which part is missing,
/// and nothing is published or left staged.
#[test]
fn a_build_with_no_manifest_shared_stylesheet_or_kind_is_refused_and_publishes_nothing() {
    for (name, shared, kinds, manifest, said) in [
        (
            "no-manifest",
            true,
            true,
            false,
            "the client's build is missing: ",
        ),
        (
            "no-shared",
            false,
            true,
            true,
            "the client's build has no shared stylesheet (src/styles/shared.css): ",
        ),
        (
            "no-kind",
            true,
            false,
            true,
            "the client's build has no kind: ",
        ),
    ] {
        let scratch = Scratch::new(name);
        let out = modules(&scratch, shared, kinds);
        if !manifest {
            fs::remove_file(out.join(".modules/web/.vite/manifest.json")).unwrap();
        }
        let done = package(&out, &publisher(&scratch));
        assert_eq!(done.status.code(), Some(2), "{name}: {done:?}");
        assert_eq!(
            String::from_utf8_lossy(&done.stderr),
            format!(
                "symphony-web: {said}{}/.modules/web/.vite/manifest.json\n",
                out.display()
            ),
            "{name}"
        );
        assert!(!out.join("public").exists(), "{name}");
        let left: Vec<_> = fs::read_dir(&out)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|entry| entry.starts_with(".public-stage."))
            .collect();
        assert!(left.is_empty(), "{name}: {left:?}");
    }
}

/// The minifier strips each TypeScript file it is named on its own and minifies them joined, in
/// their order, stdin where one is `-`; it refuses a file it cannot read or strip and a join that is
/// no module, so a shell half minified is never published.
#[test]
fn the_shells_minifier_joins_its_files_stripped_and_refuses_one_it_cannot_read() {
    use std::io::Write;
    let scratch = Scratch::new("minify");
    let minify = |files: &[&Path], stdin: &str| {
        let mut child = Command::new("bash")
            .arg(web().join("npm.sh"))
            .args(["run", "--silent", "minify", "--"])
            .args(files)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("run npm.sh");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    };
    let first = scratch.write(
        "first.ts",
        "const longName: number = 1;\nexport function shown(): number {\n    return longName + 1;\n}\n",
    );
    let stdin = Path::new("-");
    let done = minify(
        &[&first, stdin],
        "const said: string = `${shown()}`;\nconsole.log(said);\n",
    );
    succeeded(&done, "the minifier");
    let small = String::from_utf8(done.stdout).unwrap();
    assert!(
        small.ends_with('\n') && small.lines().count() == 1,
        "{small}"
    );
    assert!(
        small.contains("shown") && small.contains("console.log") && !small.contains("longName"),
        "{small}"
    );
    assert!(
        !small.contains(": number") && !small.contains(": string"),
        "{small}"
    );

    let refused = |done: Output, said: &str| {
        assert_eq!(done.status.code(), Some(1), "{done:?}");
        assert!(done.stdout.is_empty(), "{done:?}");
        let stderr = String::from_utf8_lossy(&done.stderr);
        assert!(stderr.starts_with(said), "{said}: {stderr}");
    };
    let missing = scratch.0.join("missing.ts");
    refused(
        minify(&[&first, &missing], ""),
        &format!(
            "symphony-web: the shell's {} cannot be read: ",
            missing.display()
        ),
    );
    refused(
        minify(&[stdin], "const = ;\n"),
        "symphony-web: the shell's stdin does not strip: ",
    );
    refused(
        minify(&[stdin], "enum Kind { Board }\n"),
        "symphony-web: the shell's stdin does not strip: ",
    );
    let again = scratch.write("again.ts", "const longName = 2;\n");
    refused(
        minify(&[&first, &again], ""),
        "symphony-web: the shell is no module: Identifier `longName` has already been declared",
    );
    let done = minify(&[], "");
    assert_eq!(done.status.code(), Some(2), "{done:?}");
    assert_eq!(
        String::from_utf8_lossy(&done.stderr),
        "usage: node shell/minify.ts FILE…\n"
    );
}

/// typescript-only.sh over a repository of the case's own, holding `files` (path, tracked): its
/// output and status.
fn typescript_only(scratch: &Scratch, files: &[(&str, bool)]) -> Output {
    let tree = scratch.0.join("checkout with spaces");
    let git = |args: &[&str]| {
        let done = Command::new("git")
            .arg("-C")
            .arg(&tree)
            .args(args)
            .output()
            .expect("run git");
        succeeded(&done, "git");
    };
    fs::create_dir_all(&tree).unwrap();
    git(&["init", "--quiet"]);
    fs::write(tree.join(".gitignore"), "node_modules/\n").unwrap();
    for (path, tracked) in files {
        let at = tree.join(path);
        fs::create_dir_all(at.parent().unwrap()).unwrap();
        fs::write(&at, "export {};\n").unwrap();
        if *tracked {
            git(&["add", path]);
        }
    }
    Command::new("bash")
        .arg(web().join("typescript-only.sh"))
        .arg(&tree)
        .output()
        .expect("run typescript-only.sh")
}

/// The checkout's hand-written code is TypeScript: its JavaScript is the client's distribution, and
/// an ignored folder's is no one's.
#[test]
fn typescript_only_passes_javascript_built_or_ignored() {
    let scratch = Scratch::new("typescript-only-passes");
    let done = typescript_only(
        &scratch,
        &[
            (
                "apps/symphony-server/client/public/assets/shell-0123.js",
                true,
            ),
            (
                "apps/symphony-server/client/public/assets/overlay-4567.mjs",
                false,
            ),
            ("apps/symphony-web/node_modules/vite/index.js", false),
            ("apps/symphony-web/shell/shell.ts", true),
            ("docs/analysis/board.html", true),
        ],
    );
    succeeded(&done, "typescript-only.sh");
    assert!(done.stdout.is_empty() && done.stderr.is_empty(), "{done:?}");
}

/// A JavaScript file anywhere else is refused, each named, whether git tracks it yet or not, in any
/// case of its extension, in a folder named like the exception or where the experiments were.
#[test]
fn typescript_only_refuses_javascript_anywhere_else_naming_each() {
    let scratch = Scratch::new("typescript-only-refuses");
    let done = typescript_only(
        &scratch,
        &[
            (
                "apps/symphony-server/client/public/assets/shell-0123.js",
                true,
            ),
            ("apps/symphony-web/shell/shell.js", true),
            ("apps/symphony-server/client/publicity/notice.cjs", true),
            ("experiments/keyed-reads/src/read.mjs", false),
            ("docs/analysis/board-design/BOARD.JS", true),
            ("tools/page.jsx", false),
        ],
    );
    assert_eq!(done.status.code(), Some(1), "{done:?}");
    assert!(done.stdout.is_empty(), "{done:?}");
    let said = String::from_utf8(done.stderr).unwrap();
    let mut named: Vec<&str> = said.lines().skip(1).map(str::trim).collect();
    named.sort_unstable();
    assert_eq!(
        named,
        [
            "apps/symphony-server/client/publicity/notice.cjs",
            "apps/symphony-web/shell/shell.js",
            "docs/analysis/board-design/BOARD.JS",
            "experiments/keyed-reads/src/read.mjs",
            "tools/page.jsx",
        ],
        "{said}"
    );
    assert!(
        said.starts_with("typescript-only: hand-written code here is TypeScript"),
        "{said}"
    );
}

/// The checkout itself holds no hand-written JavaScript.
#[test]
fn the_checkouts_hand_written_code_is_typescript() {
    let done = Command::new("bash")
        .arg(web().join("typescript-only.sh"))
        .output()
        .expect("run typescript-only.sh");
    succeeded(&done, "typescript-only.sh");
}

fn executable(path: &Path, script: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// npm.sh in a tree of the case's own, over a Node launcher that says what it was asked: its
/// install speaks on both streams, and fails with 7 while the tree holds `fail`.
fn installer(scratch: &Scratch) -> PathBuf {
    let tree = scratch.0.join("tree with spaces");
    let package = tree.join("apps/symphony-web");
    fs::create_dir_all(&package).unwrap();
    fs::copy(web().join("npm.sh"), package.join("npm.sh")).unwrap();
    fs::write(package.join("package-lock.json"), "{\"lock\": 1}\n").unwrap();
    fs::create_dir_all(tree.join("tools")).unwrap();
    fs::copy(
        workspace().join("tools/check_lock.sh"),
        tree.join("tools/check_lock.sh"),
    )
    .unwrap();
    executable(
        &tree.join("tools/node.sh"),
        concat!(
            "#!/bin/bash\n",
            "if [ \"$2\" != ci ]; then printf 'ran %s\\n' \"$*\"; printf 'warned\\n' >&2; exit 3; fi\n",
            "printf '%s\\n' \"$*\" >>../../installs\n",
            "printf '\\nadded 58 packages in 4s\\n'\n",
            "printf 'npm error from the install\\n' >&2\n",
            "if [ -e ../../fail ]; then exit 7; fi\n",
            "mkdir -p node_modules\n",
        ),
    );
    tree
}

fn npm(tree: &Path) -> Output {
    Command::new("bash")
        .arg(tree.join("apps/symphony-web/npm.sh"))
        .args(["run", "a script"])
        .output()
        .expect("run npm.sh")
}

/// A first run installs the lockfile's packages and says nothing of it: both streams and the exit
/// status are the command's own, on the run that installs as on the next, which installs nothing.
/// A changed lockfile installs once more, as quietly.
#[test]
fn an_install_that_succeeds_says_nothing_so_a_first_run_reads_as_every_other() {
    let scratch = Scratch::new("install");
    let tree = installer(&scratch);
    let package = tree.join("apps/symphony-web");
    let installs = || {
        fs::read_to_string(tree.join("installs"))
            .unwrap_or_default()
            .lines()
            .count()
    };
    for (run, installed) in [("the first", 1), ("the second", 1)] {
        let done = npm(&tree);
        assert_eq!(done.status.code(), Some(3), "{run}: {done:?}");
        assert_eq!(
            String::from_utf8_lossy(&done.stdout),
            "ran npm run a script\n",
            "{run}"
        );
        assert_eq!(String::from_utf8_lossy(&done.stderr), "warned\n", "{run}");
        assert_eq!(installs(), installed, "{run}");
    }
    assert_eq!(
        fs::read_to_string(tree.join("installs")).unwrap(),
        "npm ci --prefer-offline --no-audit --no-fund --loglevel=error\n"
    );
    assert_eq!(
        fs::read(package.join("node_modules/.symphony-lock")).unwrap(),
        fs::read(package.join("package-lock.json")).unwrap()
    );
    fs::write(package.join("package-lock.json"), "{\"lock\": 2}\n").unwrap();
    let done = npm(&tree);
    assert_eq!(String::from_utf8_lossy(&done.stderr), "warned\n");
    assert_eq!(installs(), 2);
    assert_eq!(
        fs::read_to_string(package.join("node_modules/.symphony-lock")).unwrap(),
        "{\"lock\": 2}\n"
    );
}

/// An install that fails says all it said, on stderr, and exits as it did: npm is not run, and
/// the packages are not taken as the lockfile's, so the next run installs again.
#[test]
fn an_install_that_fails_says_all_it_said_and_runs_nothing() {
    let scratch = Scratch::new("install-fails");
    let tree = installer(&scratch);
    fs::write(tree.join("fail"), "").unwrap();
    let done = npm(&tree);
    assert_eq!(done.status.code(), Some(7), "{done:?}");
    assert!(done.stdout.is_empty(), "{done:?}");
    assert_eq!(
        String::from_utf8_lossy(&done.stderr),
        "\nadded 58 packages in 4s\nnpm error from the install\n"
    );
    assert!(
        !tree
            .join("apps/symphony-web/node_modules/.symphony-lock")
            .exists()
    );
    fs::remove_file(tree.join("fail")).unwrap();
    let done = npm(&tree);
    assert_eq!(done.status.code(), Some(3), "{done:?}");
    assert_eq!(String::from_utf8_lossy(&done.stderr), "warned\n");
    assert_eq!(
        fs::read_to_string(tree.join("installs"))
            .unwrap()
            .lines()
            .count(),
        2
    );
}

/// The justfile's own recipes: `symphony-ui-dist` runs the client's build script with OUT as
/// given, from the workspace, and `lint-rs` runs Cargo's checks (the wire types' TypeScript feature
/// and the hosted board's feature among them, then the board tests' raw-statement fence), the
/// build's two readings of the branch (its consumer notes, its published packages) and then the
/// client's, with no wasm32 pass.
#[test]
fn the_justfile_builds_the_client_with_its_script_and_lints_it_with_its_checks() {
    let scratch = Scratch::new("recipes");
    let tree = scratch.0.join("workspace with spaces");
    fs::create_dir_all(&tree).unwrap();
    // The justfile and the Symphony's recipes it imports.
    for name in ["justfile", "symphony.just"] {
        fs::copy(workspace().join(name), tree.join(name)).unwrap();
    }
    executable(
        &tree.join("tools/cargo.sh"),
        "#!/bin/bash\nprintf 'cwd=%s\\n' \"$PWD\"\nprintf 'arg=%s\\n' \"$@\"\n",
    );
    // The launcher of the workspace's binaries, which the lint asks for the build's own.
    executable(
        &tree.join("tools/rs-bin.sh"),
        "#!/bin/bash\nprintf 'bin cwd=%s\\n' \"$PWD\"\nprintf 'arg=%s\\n' \"$@\"\n",
    );
    // The client's build and its checks, which bash reads as data.
    executable(
        &tree.join("apps/symphony-web/build.sh"),
        "printf 'build cwd=%s\\n' \"$PWD\"\nprintf 'arg=%s\\n' \"$@\"\n",
    );
    executable(
        &tree.join("apps/symphony-web/check.sh"),
        "printf 'client-lint cwd=%s\\n' \"$PWD\"\n",
    );
    for (args, said) in [
        (
            &["symphony-ui-dist", "output with spaces"][..],
            format!("build cwd={}\narg=output with spaces\n", tree.display()),
        ),
        (
            &["symphony-ui-dist"],
            format!("build cwd={}\narg=build/symphony-ui\n", tree.display()),
        ),
    ] {
        let done = Command::new("just")
            .current_dir(&tree)
            .args(args)
            .output()
            .unwrap();
        succeeded(&done, "own workspace dist recipe");
        assert_eq!(String::from_utf8(done.stdout).unwrap(), said, "{args:?}");
    }
    let done = Command::new("just")
        .current_dir(&tree)
        .arg("lint-rs")
        .output()
        .unwrap();
    succeeded(&done, "own workspace lint recipe");
    assert_eq!(
        String::from_utf8(done.stdout).unwrap(),
        format!(
            concat!(
                "cwd={0}\narg=fmt\narg=--all\narg=--check\n",
                "cwd={0}\narg=clippy\narg=--workspace\narg=--all-targets\narg=--locked\narg=--\narg=-D\narg=warnings\n",
                "cwd={0}\narg=clippy\narg=-p\narg=symphony-api\narg=--features\narg=typescript\narg=--all-targets\narg=--locked\narg=--\narg=-D\narg=warnings\n",
                "cwd={0}\narg=clippy\narg=-p\narg=symphony-board\narg=--features\narg=hosted-postgresql\narg=--all-targets\narg=--locked\narg=--\narg=-D\narg=warnings\n",
                "cwd={0}\narg=test\narg=-p\narg=symphony-board\narg=--test\narg=raw_fence\narg=--locked\n",
                "bin cwd={0}\narg=symphony-build\narg=consumer-notes\narg=--check\n",
                "bin cwd={0}\narg=symphony-build\narg=crates-check\narg=--branch\narg=.\n",
                "client-lint cwd={0}\n"
            ),
            tree.display()
        )
    );
}

/// The key the regenerator reads (tools/client-key.sh) is one line naming the web package's
/// bytes: the same for the same tree, whatever folder it is asked from.
#[test]
fn the_clients_key_is_one_line_of_the_web_packages_hash() {
    let key = |from: &Path| {
        let done = Command::new("bash")
            .arg(workspace().join("tools/client-key.sh"))
            .current_dir(from)
            .output()
            .unwrap();
        succeeded(&done, "client-key.sh");
        assert!(done.stderr.is_empty(), "{done:?}");
        String::from_utf8(done.stdout).unwrap()
    };
    let here = key(&workspace());
    let hash = here
        .strip_prefix("web-")
        .and_then(|rest| rest.strip_suffix('\n'))
        .unwrap_or_else(|| panic!("one line, web- and a hash: {here:?}"));
    assert!(
        hash.len() == 16 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
        "{here:?}"
    );
    assert_eq!(key(&std::env::temp_dir()), here);
}

#[test]
fn the_shell_keys_storage_by_the_clients_rule() {
    let shell = fs::read_to_string(web().join("shell/shell.ts")).unwrap();
    assert!(
        shell.contains("return id ? `${id}:${key}` : key;"),
        "shell.ts's stored takes the project's id first, as the client's `stored` does"
    );
    assert!(
        !shell.contains("localStorage.getItem(\"") && !shell.contains("localStorage.setItem(\""),
        "every key the shell reads or writes goes through stored"
    );
}

#[test]
fn the_shell_yields_the_same_routes_to_the_symphony_as_the_server() {
    let shell = fs::read_to_string(web().join("shell/shell.ts")).unwrap();
    // The list as the formatter writes it: a quoted route a line, between `[` and `];`.
    let (_, rest) = shell
        .split_once("const SYMPHONY_ROUTES = [")
        .expect("shell.ts names the Symphony's routes");
    let (list, _) = rest.split_once("];").expect("the routes' list ends");
    let routes: Vec<&str> = list
        .split(',')
        .map(str::trim)
        .filter(|route| !route.is_empty())
        .map(|route| {
            route
                .strip_prefix('"')
                .and_then(|route| route.strip_suffix('"'))
                .unwrap_or_else(|| panic!("a route is a quoted word: {route}"))
        })
        .collect();
    assert_eq!(routes, symphony_api::SYMPHONY_ROUTES);
}
