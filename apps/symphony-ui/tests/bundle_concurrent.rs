//! Exercise the real bundle/packaging scripts without compiling: DX uses the same intermediate
//! folder for every invocation in a tree, independently of --out-dir. A handshake holds one DX
//! process there while another bundle starts. Each run owns its home, tools and published output.
//!
//! Every wait ends on something the fixture's processes do, never on a clock: a line on a
//! bundle's stderr, or the end of that stream once bundle.sh and every child holding it have
//! exited. The one fake DX that blocks does so after printing its line, until the test writes to
//! its stdin, so a bundle that goes wrong ends its stream and the wait fails with its log. A
//! bundle that never ends is bounded by the native test runner's own budget.
//!
//! No freshly written file is executed: macOS assesses each new executable on its first run in
//! one machine-wide queue, seconds apiece under load. The stand-ins link to the workspace's
//! prepared fixture script, which reads each one's body as data.

#[path = "../../../tools/test-support/script_std.rs"]
mod fixture_script;

use std::fs;
use std::io::{BufRead, BufReader, Lines, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, Command, Stdio};

/// The program's kinds (src/split.rs), one build of it: the one kind dx builds.
const PROGRAM: [&str; 4] = ["tasks", "panel", "overlay", "timeline"];
/// The program's chunks in the order of its split points' names: its kinds' and the media layer's,
/// which is no kind.
const CHUNKS: [&str; 5] = ["media", "overlay", "panel", "tasks", "timeline"];
/// The web client's kinds, one Vite build of them all, and whether each has a style sheet of its
/// own (the task board's styles are the program's board.css, in the shared file).
const WEB: [(&str, bool); 5] = [
    ("board", false),
    ("docs", true),
    ("queue", true),
    ("reports", true),
    ("review", true),
];

/// The stand-in dx: the first bundle's program build holds the intermediate folder until released.
/// It writes the program's main module, a chunk for each of its kinds and their own styles.
const DX: &str = r#"#!/bin/bash
set -euo pipefail
while [ "$#" -gt 0 ]; do
    case "$1" in
        --out-dir) out="$2"; shift 2 ;;
        --features) kind="${2#page-}"; shift 2 ;;
        *) shift ;;
    esac
done
work="$CARGO_TARGET_DIR/dx/symphony-ui/release/web"
mkdir -p "$work/public/assets"
printf '%s' "$BUNDLE_ID-$kind" > "$work/owner"
if [ "$BUNDLE_ID-$kind" = first-program ]; then
    echo 'fixture: first DX holds its intermediate files' >&2
    read -r release
    [ "$release" = release ]
    [ "$FIRST_EXIT" = 0 ] || exit "$FIRST_EXIT"
fi
[ "$(cat "$work/owner")" = "$BUNDLE_ID-$kind" ]
printf '<script type="module" src="/assets/symphony-ui-%s.js"></script>\n' "$kind" > "$work/public/index.html"
printf 'export default function init(){return Promise.resolve()} init({module_or_path:"/assets/symphony-ui_bg-%s.wasm"}).then(()=>{});/*%s*/\n' "$kind" "$BUNDLE_ID" > "$work/public/assets/symphony-ui-$kind.js"
printf '\0asm\1\0\0\0%s-%s' "$BUNDLE_ID" "$kind" > "$work/public/assets/symphony-ui_bg-$kind.wasm"
for chunk in 0_media 1_overlay 2_panel 3_tasks 4_timeline; do
    printf '\0asm\1\0\0\0%s-%s' "$BUNDLE_ID" "$chunk" > "$work/public/assets/module_$chunk-$kind.wasm"
done
for own in task-timeline style; do
    printf '/*%s-%s*/body{}' "$BUNDLE_ID" "$own" > "$work/public/assets/$own-$kind.css"
done
mkdir -p "$out"
cp -R "$work/public" "$out/"
"#;

/// The stand-in web client's build, which bundle.sh runs as `bash apps/symphony-web/npm.sh run
/// --silent build -- --outDir OUT --emptyOutDir`: each kind's script, the style sheet of each kind
/// but the task board, and Vite's manifest naming them.
const WEB_BUILD: &str = r#"#!/bin/bash
set -euo pipefail
[ "$*" = "run --silent build -- --outDir $6 --emptyOutDir" ]
out="$6"
rm -rf "$out"
mkdir -p "$out/.vite" "$out/assets"
entries=""
for kind in board docs queue reports review; do
    printf 'export{a as run,b as stop};/*%s*/\n' "$BUNDLE_ID" > "$out/assets/$kind-$BUNDLE_ID.js"
    css=""
    if [ "$kind" != board ]; then
        printf '/*%s-%s*/body{}' "$BUNDLE_ID" "$kind" > "$out/assets/$kind-$BUNDLE_ID.css"
        css="$(printf ',"css":["assets/%s-%s.css"]' "$kind" "$BUNDLE_ID")"
    fi
    entries="${entries:+$entries,}$(printf '"src/kinds/%s.tsx":{"file":"assets/%s-%s.js","name":"%s","src":"src/kinds/%s.tsx","isEntry":true%s}' "$kind" "$kind" "$BUNDLE_ID" "$kind" "$kind" "$css")"
done
printf '{%s}\n' "$entries" > "$out/.vite/manifest.json"
"#;

struct Fixture(PathBuf);

impl Fixture {
    fn new(case: &str) -> Self {
        let base = std::env::temp_dir().join(format!(
            "symphony-ui-concurrent {case}-{}",
            std::process::id()
        ));
        fs::create_dir(&base).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for name in [
            "apps/symphony-ui/bundle.sh",
            "apps/symphony-ui/tools.sh",
            "apps/symphony-ui/dx-path.sh",
            "apps/symphony-ui/wasm-fresh.sh",
            "apps/symphony-ui/package.sh",
            "apps/symphony-ui/assets-copy.sh",
            "apps/symphony-ui/remap-rustc.sh",
            "apps/symphony-ui/Dioxus.toml",
            "apps/symphony-ui/assets/shell.js",
            "apps/symphony-ui/assets/cache.js",
            "apps/symphony-ui/assets/chunks.js",
            "apps/symphony-server/src/notices.css",
            "tools/check_lock.sh",
            "Cargo.toml",
        ] {
            let to = base.join(name);
            fs::create_dir_all(to.parent().unwrap()).unwrap();
            fs::copy(source.join(name), to).unwrap();
        }
        // Only the pinned version is read; no cargo or tool bootstrap runs in this fixture.
        fs::write(
            base.join("Cargo.lock"),
            "name = \"wasm-bindgen\"\nversion = \"fixture\"\n",
        )
        .unwrap();
        executable(&base.join("cargo"), "#!/bin/bash\nexit 0\n");
        // Read by bash as data, never executed.
        executable(&base.join("apps/symphony-web/npm.sh"), WEB_BUILD);
        executable(
            &base.join("common/symphony/tools/dx-home/tools/wasm-bindgen-fixture/wasm-bindgen"),
            "#!/bin/bash\nexit 0\n",
        );
        // The shell's minifier passes the script through as it is.
        executable(
            &base.join("common/symphony/tools/dx-home/tools/esbuild-fixture/esbuild"),
            "#!/bin/bash\ncat\n",
        );
        release_protocol(&base);
        // dx where tools.sh installs it and where bundle.sh runs it from (dx-path.sh, its folder
        // named by SYMPHONY_CLIENT_TOOLS, private as it asks): one file, so the stand is found
        // current, nothing is linked, and the body is read beside the path it runs as.
        let stand = base.join("client-tools");
        fs::create_dir(&stand).unwrap();
        fs::set_permissions(&stand, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
        for dx in [
            base.join("common/symphony/tools/dx-0.7.10/bin/dx"),
            stand.join("dx-0.7.10/bin/dx"),
        ] {
            fs::create_dir_all(dx.parent().unwrap()).unwrap();
            fixture_script::write(&dx, DX, true);
        }
        owned_git(&base, &["init", "-q"]);
        owned_git(&base, &["commit", "-qm", "fixture", "--allow-empty"]);
        Self(base)
    }

    fn start(&self, id: &str, first_exit: i32) -> Bundle {
        let home = self.0.join(format!("home-{id}"));
        fs::create_dir(&home).unwrap();
        fs::create_dir(home.join(".cargo")).unwrap();
        let mut child = Command::new("bash")
            .arg(self.0.join("apps/symphony-ui/bundle.sh"))
            .arg(self.0.join(id))
            .current_dir(&self.0)
            .env("HOME", home)
            .env("SYMPHONY_GIT_COMMON_DIR", self.0.join("common"))
            .env("SYMPHONY_CLIENT_TOOLS", self.0.join("client-tools"))
            .env("CARGO_TARGET_DIR", self.0.join("target"))
            .env("CARGO", self.0.join("cargo"))
            .env("BUNDLE_ID", id)
            .env("BUNDLE_ROOT", &self.0)
            .env("FIRST_EXIT", first_exit.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let lines = BufReader::new(child.stderr.take().unwrap()).lines();
        Bundle {
            child,
            lines,
            log: String::new(),
        }
    }

    fn assert_complete(&self, id: &str) {
        let public = self.0.join(id).join("public");
        let index = fs::read_to_string(public.join("index.html")).unwrap();
        let shell_path = index
            .split("src=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let shell = fs::read_to_string(public.join(shell_path.trim_start_matches('/'))).unwrap();
        let manifest = shell
            .split("const modules = ")
            .nth(1)
            .unwrap()
            .split(";\n")
            .next()
            .unwrap();
        let manifest: serde_json::Value = serde_json::from_str(manifest).unwrap();
        assert_eq!(
            manifest.as_object().unwrap().len(),
            PROGRAM.len() + WEB.len()
        );
        // The stage holds its build alone, and its builds.json names every file of it by its shell,
        // for publish-client to keep beside the builds published before it.
        let mut files: Vec<String> = fs::read_dir(public.join("assets"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        files.sort();
        // The program's script and main module, a chunk for each of its kinds and the media
        // layer's, and its two own style sheets; each web kind's script and the style sheets of
        // its own; and the shell.
        let styled = WEB.iter().filter(|(_, styled)| *styled).count();
        assert_eq!(
            files.len(),
            2 + CHUNKS.len() + 2 + WEB.len() + styled + 1,
            "{files:?}"
        );
        let media: Vec<_> = files
            .iter()
            .filter(|name| name.starts_with("module_0_media-"))
            .collect();
        assert_eq!(media.len(), 1, "{files:?}");
        assert_eq!(
            fs::read(public.join("assets").join(media[0])).unwrap(),
            format!("\0asm\x01\0\0\0{id}-0_media").as_bytes()
        );
        let builds: serde_json::Value =
            serde_json::from_slice(&fs::read(public.join("builds.json")).unwrap()).unwrap();
        assert_eq!(
            builds,
            serde_json::json!({"builds": [{
                "shell": shell_path.trim_start_matches("/assets/"),
                "files": files,
            }]})
        );
        let read_asset = |url: &serde_json::Value| {
            fs::read(public.join(url.as_str().unwrap().trim_start_matches('/'))).unwrap()
        };
        // A web kind is its script and its styles, this bundle's: no wasm.
        for (kind, styled) in WEB {
            let entry = &manifest[kind];
            assert_eq!(
                read_asset(&entry["script"]),
                format!("export{{a as run,b as stop}};/*{id}*/\n").as_bytes()
            );
            assert!(entry.get("wasm").is_none(), "{kind}");
            let styles: Vec<_> = entry["styles"]
                .as_array()
                .unwrap()
                .iter()
                .map(read_asset)
                .collect();
            let own = styled.then(|| format!("/*{id}-{kind}*/body{{}}").into_bytes());
            assert_eq!(styles, own.into_iter().collect::<Vec<_>>(), "{kind}");
        }
        for (at, kind) in CHUNKS
            .iter()
            .enumerate()
            .filter(|(_, kind)| PROGRAM.contains(kind))
        {
            let entry = &manifest[kind];
            assert!(
                String::from_utf8(read_asset(&entry["script"]))
                    .unwrap()
                    .contains(&format!("/*{id}*/")),
                "{kind}"
            );
            assert_eq!(
                read_asset(&entry["wasm"]),
                format!("\0asm\x01\0\0\0{id}-program").as_bytes()
            );
            assert_eq!(
                read_asset(&entry["chunk"]),
                format!("\0asm\x01\0\0\0{id}-{at}_{kind}").as_bytes()
            );
            let own = match *kind {
                "timeline" => Some("task-timeline"),
                "panel" | "overlay" => Some("style"),
                _ => None,
            };
            let styles: Vec<_> = entry["styles"]
                .as_array()
                .unwrap()
                .iter()
                .map(read_asset)
                .collect();
            assert_eq!(
                styles,
                own.map(|own| format!("/*{id}-{own}*/body{{}}").into_bytes())
                    .into_iter()
                    .collect::<Vec<_>>(),
                "{kind}"
            );
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn executable(path: &Path, script: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fixture_script::write(path, script, false);
}

fn release_protocol(base: &Path) {
    // Source capture and atomic publication have unit and real-build coverage. These protocol
    // stand-ins keep this case on the target lock, seven-module assembly and failure release.
    executable(
        &base.join("tools/rs-bin.sh"),
        "#!/bin/bash\nset -euo pipefail\n[ \"$1 $2\" = '--build symphony-build' ]\nprintf '%s/publisher\\n' \"$BUNDLE_ROOT\"\n",
    );
    executable(
        &base.join("publisher"),
        r#"#!/bin/bash
set -euo pipefail
case "$1" in
release-inputs)
    if [ "$6" = --key ]; then
        mkdir -p "$BUNDLE_ROOT/inputs-lock"
        printf '%s/inputs-lock\n' "$BUNDLE_ROOT"
    else
        printf '%s\n' "$BUNDLE_ROOT"
    fi ;;
publish-client) mv "$2" "$3" ;;
# The repair itself has its own tests and the real build's: here its output is dx's as it is.
split-repair) ;;
*) exit 97 ;;
esac
"#,
    );
    executable(
        &base.join("tools/cargo.sh"),
        r#"#!/bin/bash
set -euo pipefail
[ "$1" = metadata ]
pin="$(git -C "$BUNDLE_ROOT" rev-parse HEAD)"
jq -nc --arg path "$BUNDLE_ROOT/primitives/Cargo.toml" --arg source "git+fixture#$pin" \
'{packages:[{name:"dioxus-primitives",manifest_path:$path,source:$source}]}'
"#,
    );
    fs::create_dir(base.join("primitives")).unwrap();
}

fn owned_git(root: &Path, args: &[&str]) {
    let mut command = Command::new("git");
    command
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .current_dir(root);
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
    ] {
        command.env_remove(key);
    }
    let done = command.output().unwrap();
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
}

struct Bundle {
    child: Child,
    lines: Lines<BufReader<ChildStderr>>,
    log: String,
}

impl Bundle {
    /// The bundle's next stderr line, or None once bundle.sh and every child holding its stderr
    /// have closed it. The read blocks until one of the two happens. Neither bundle reads the
    /// other's pipe, so the one not being read keeps running until its own pipe is full.
    fn line(&mut self) -> Option<String> {
        let line = self.lines.next()?.unwrap();
        self.log.push_str(&line);
        self.log.push('\n');
        Some(line)
    }

    fn until(&mut self, event: &str) {
        while let Some(line) = self.line() {
            if line.contains(event) {
                return;
            }
        }
        panic!("bundle ended before {event}:\n{}", self.log);
    }

    fn finish(mut self, expected: i32) {
        self.child.stdin.take();
        while self.line().is_some() {}
        let status = self.child.wait().unwrap();
        assert_eq!(status.code(), Some(expected), "{}", self.log);
    }
}

impl Drop for Bundle {
    fn drop(&mut self) {
        // Closing stdin also releases a blocked fake DX if an assertion fails.
        self.child.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn concurrent_bundles_in_one_tree_publish_complete_outputs_and_release_after_failure() {
    for first_exit in [0, 23] {
        let fixture = Fixture::new(&first_exit.to_string());
        let mut first = fixture.start("first", first_exit);
        first.until("fixture: first DX holds its intermediate files");
        let mut second = fixture.start("second", 0);
        second.until("waiting for the client bundle build folder");
        assert!(!fixture.0.join("second/.modules").exists());
        writeln!(first.child.stdin.as_mut().unwrap(), "release").unwrap();
        first.finish(first_exit);
        second.finish(0);
        if first_exit == 0 {
            fixture.assert_complete("first");
        } else {
            assert!(!fixture.0.join("first/public").exists());
        }
        fixture.assert_complete("second");
    }
}

/// The lines of the client's script `name` that find the git common dir: from the comment that
/// opens them to the `common=` line.
fn common_dir_lines(name: &str) -> String {
    let script = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(name)).unwrap();
    let from = script
        .find("# The common dir: SYMPHONY_GIT_COMMON_DIR")
        .unwrap_or_else(|| panic!("{name} finds no common dir"));
    let rest = script.get(from..).unwrap();
    let end = rest.find("\ncommon=").unwrap();
    let line_end = rest.get(end + 1..).unwrap().find('\n').unwrap();
    rest.get(..end + 1 + line_end + 1).unwrap().to_string()
}

#[test]
fn the_client_scripts_read_the_common_dir_by_its_name_else_gits() {
    let scripts = ["bundle.sh", "dev.sh", "tools.sh", "probes/size.sh"];
    let lines = common_dir_lines(scripts[0]);
    for name in &scripts[1..] {
        assert_eq!(
            common_dir_lines(name),
            lines,
            "{name} reads the common dir otherwise"
        );
    }
    let here = std::env::temp_dir().join(format!("symphony-ui-common-dir-{}", std::process::id()));
    let _ = fs::remove_dir_all(&here);
    fs::create_dir_all(&here).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(&here)
            .status()
            .unwrap()
            .success()
    );
    let found = |vars: &[(&str, &str)]| {
        let mut bash = Command::new("bash");
        bash.args([
            "-c",
            &format!("set -euo pipefail\n{lines}printf '%s|%s\\n' \"$common\" \"${{SYMPHONY_GIT_COMMON_DIR:-}}\""),
        ])
        .env("here", &here);
        for name in ["SYMPHONY_GIT_COMMON_DIR", "GIT_DIR", "GIT_COMMON_DIR"] {
            bash.env_remove(name);
        }
        for (name, value) in vars {
            bash.env(name, value);
        }
        let done = bash.output().unwrap();
        assert!(done.status.success(), "{done:?}");
        (
            String::from_utf8(done.stdout).unwrap(),
            String::from_utf8(done.stderr).unwrap(),
        )
    };
    // By its name, nothing said; else git's.
    assert_eq!(
        found(&[("SYMPHONY_GIT_COMMON_DIR", "/new")]),
        ("/new|/new\n".into(), String::new())
    );
    let git = here.join(".git").canonicalize().unwrap();
    let (out, said) = found(&[]);
    assert_eq!(
        (
            Path::new(out.trim_end().trim_end_matches('|'))
                .canonicalize()
                .unwrap(),
            said
        ),
        (git, String::new())
    );
    fs::remove_dir_all(&here).unwrap();
}
