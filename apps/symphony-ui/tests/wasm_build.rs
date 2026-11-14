//! The client freshness guards use the caller-provided target folder. Ordinary builds preserve
//! dependency products while clearing another workspace image; canonical release snapshots also
//! clear private wasm release dependencies whose asset metadata names a different source path.
//! Records stay within the held target, and repeated calls retain the current image.
//!
//! These cases use real Cargo and the owned native publisher with protocol stand-ins for dx
//! and wasm-bindgen. Real seven-module compilation and browser acceptance are separate checks.

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

const WASM32: &str = "wasm32-unknown-unknown";
/// Fingerprints as cargo names them: the package, then the hash of how it was built.
const OURS: [&str; 2] = [
    "symphony-ui-0123456789abcdef",
    "symphony-api-0123456789abcdef",
];
const A_DEPENDENCY: &str = "serde-0123456789abcdef";
const KINDS: [&str; 8] = [
    "board", "tasks", "docs", "review", "queue", "panel", "overlay", "timeline",
];

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A folder of the test's own under the temporary folder, removed when it drops.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = std::env::temp_dir().join(format!("symphony-ui-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn executable(path: &Path, script: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// The wasm-bindgen version Cargo.lock pins, as bundle.sh reads it.
fn bindgen_version() -> String {
    let lock = fs::read_to_string(crate_dir().join("../../Cargo.lock")).unwrap();
    let mut lines = lock.lines();
    while let Some(line) = lines.next() {
        if line == "name = \"wasm-bindgen\"" {
            let version = lines.next().unwrap();
            return version
                .trim_start_matches("version = \"")
                .trim_end_matches('"')
                .to_owned();
        }
    }
    panic!("Cargo.lock pins no wasm-bindgen")
}

/// Where the guard records the tree that last built for wasm32 in the build folder `target`: inside it,
/// beside the wrapper's own records, so it goes when the wrapper empties the folder.
fn record_in(target: &Path) -> PathBuf {
    target.join(format!("symphony-tree-{WASM32}"))
}

/// A build folder as another tree left it: its record naming that tree, and fingerprints of this
/// workspace's crates and of a dependency in both wasm32 profiles.
fn another_trees_build(common: &Path) -> PathBuf {
    let target = common.join("symphony/tools").join("target");
    for profile in ["debug", "wasm-release"] {
        let prints = target.join(WASM32).join(profile).join(".fingerprint");
        for name in OURS.iter().chain([&A_DEPENDENCY]) {
            fs::create_dir_all(prints.join(name)).unwrap();
        }
    }
    fs::write(record_in(&target), "/another/tree 1 2").unwrap();
    target
}

/// git in `dir` alone, whatever repository a hook that runs these tests has named in the environment.
fn git(dir: &Path, args: &[&str]) -> String {
    let mut git = Command::new("git");
    git.args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .current_dir(dir);
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
    ] {
        git.env_remove(key);
    }
    let done = git.output().unwrap();
    succeeded(&done, &format!("git {args:?}"));
    String::from_utf8(done.stdout).unwrap()
}

fn left_in(target: &Path, profile: &str) -> Vec<String> {
    let prints = target.join(WASM32).join(profile).join(".fingerprint");
    let mut names: Vec<String> = fs::read_dir(prints)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The folder a case's bundle.sh runs dx from (`SYMPHONY_CLIENT_TOOLS`), in place of the machine's
/// `/private/tmp/symphony-client-tools`, where a stand-in would take the real dx's place: private,
/// as `dx-path.sh` asks.
fn client_tools(scratch: &Scratch) -> PathBuf {
    let stand = scratch.0.join("client-tools");
    fs::create_dir(&stand).unwrap();
    fs::set_permissions(&stand, fs::Permissions::from_mode(0o700)).unwrap();
    stand
}

/// Each page kind's dx ran as the stand's, by its canonical path, never from the tools' folder.
fn ran_from(said: &Path, stand: &Path) {
    let dx = stand.canonicalize().unwrap().join("dx-0.7.10/bin/dx");
    for kind in KINDS {
        let ran = fs::read_to_string(said.join(format!("{kind}.dx"))).expect("dx never ran");
        assert_eq!(Path::new(ran.trim_end()), dx, "{kind}");
    }
}

/// Stand-ins for dx, which writes the arguments it was given to `said` (and the path it ran as),
/// and for wasm-bindgen, where bundle.sh looks for them under `common`.
fn stand_in_tools(common: &Path, said: &Path) {
    // Publication and snapshot capture use the actual declared native member. Clone only its
    // native Cargo products into this private fixture; every adapter still runs Cargo itself.
    static NATIVE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    let native = NATIVE.get_or_init(|| {
        let root = crate_dir().join("../..").canonicalize().unwrap();
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join(".target"));
        let done = Command::new(root.join("tools/rs-bin.sh"))
            .args(["--build", "symphony-build"])
            .env("CARGO_TARGET_DIR", &target)
            .output()
            .unwrap();
        succeeded(&done, "owned native publication member");
        target
    });
    fs::create_dir_all(common.join("symphony/tools")).unwrap();
    git(common, &["init", "--bare", "-q"]);
    let target = common.join("symphony/tools/target");
    fs::create_dir_all(&target).unwrap();
    let copied = Command::new("cp")
        .arg(if cfg!(target_os = "macos") {
            "-cR"
        } else {
            "-R"
        })
        .arg(native.join("debug"))
        .arg(target.join("debug"))
        .output()
        .unwrap();
    succeeded(&copied, "private native dependency copy");
    let capture = format!("'{}'", said.to_string_lossy().replace('\'', "'\\''"));
    executable(
        &common.join("symphony/tools/dx-0.7.10/bin/dx"),
        &r#"#!/bin/bash
set -euo pipefail
capture=__CAPTURE__
args=("$@")
while [ "$#" -gt 0 ]; do
    case "$1" in
        --out-dir) out="$2"; shift 2 ;;
        --features) features="$2"; shift 2 ;;
        --bin) bin="$2"; shift 2 ;;
        *) shift ;;
    esac
done
kind="${features#page-}"
kind="${kind%%,*}"
# A page kind's build is named by its kind, a size probe's by its bin.
if [ "$bin" != symphony-ui ]; then kind="$bin"; fi
mkdir -p "$capture" "$out/public/assets"
printf '%s\n' "${args[@]}" > "$capture/$kind"
printf '%s\n' "$0" > "$capture/$kind.dx"
printf '<script type="module" src="/assets/%s-%s.js"></script>\n' "$bin" "$kind" > "$out/public/index.html"
printf 'export default function init(){return Promise.resolve()} init({module_or_path:"/assets/%s_bg-%s.wasm"}).then(()=>{});\n' "$bin" "$kind" > "$out/public/assets/$bin-$kind.js"
printf '\0asm\1\0\0\0' > "$out/public/assets/${bin}_bg-$kind.wasm"
printf 'body{}' > "$out/public/assets/board-$kind.css"
# Every page kind's two shared stylesheets, one sorting before its own and one after; the task
# timeline, which carries few of the pages' styles, has only the first.
printf '.a{}' > "$out/public/assets/a-shared.css"
if [ "$kind" != timeline ]; then printf '.z{}\n' > "$out/public/assets/z-shared.css"; fi
"#.replace("__CAPTURE__", &capture),
    );
    executable(
        &common.join(format!(
            "symphony/tools/dx-home/tools/wasm-bindgen-{}/wasm-bindgen",
            bindgen_version()
        )),
        "#!/bin/bash\n",
    );
    // The shell's minifier: it names its arguments, then passes the script through.
    executable(
        &common.join("symphony/tools/dx-home/tools/esbuild-fixture/esbuild"),
        "#!/bin/bash\nprintf '/*esbuild %s*/\\n' \"$*\"\ncat\n",
    );
}

/// `cargo symphony-ui ARGS` as cargo runs it, an external subcommand: `cargo-symphony-ui symphony-ui ARGS`.
fn cargo_symphony_ui(args: &[&str]) -> Command {
    let mut command = Command::new(crate_dir().join("bin/cargo-symphony-ui"));
    command.arg("symphony-ui").args(args);
    command
}

fn succeeded(done: &std::process::Output, what: &str) {
    assert!(
        done.status.success(),
        "{what} failed: {}",
        String::from_utf8_lossy(&done.stderr)
    );
}

include!("wasm_build/release_tests.rs");

#[test]
fn cargo_symphony_ui_dist_bundles_into_out_as_named_from_where_it_runs() {
    let scratch = Scratch::new("wasm-build-dist");
    let common = scratch.0.join("common");
    let said = scratch.0.join("dx-said");
    stand_in_tools(&common, &said);
    let stand = client_tools(&scratch);
    let done = cargo_symphony_ui(&["dist", "build/ui"])
        .current_dir(&scratch.0)
        .env("SYMPHONY_GIT_COMMON_DIR", &common)
        .env("CARGO_TARGET_DIR", common.join("symphony/tools/target"))
        .env("SYMPHONY_CLIENT_TOOLS", &stand)
        .output()
        .expect("run cargo-symphony-ui");
    succeeded(&done, "cargo symphony-ui dist");
    ran_from(&said, &stand);
    let out = scratch.0.join("build/ui");
    for kind in KINDS {
        let said = fs::read_to_string(said.join(kind)).expect("dx never built this kind");
        let args: Vec<_> = said.lines().collect();
        let argument = |flag| {
            args.iter()
                .position(|arg| *arg == flag)
                .map(|at| args[at + 1])
                .unwrap()
        };
        assert_eq!(
            Path::new(argument("--out-dir")).canonicalize().unwrap(),
            out.join(".modules").join(kind).canonicalize().unwrap()
        );
        assert_eq!(argument("--features"), format!("page-{kind}"));
        assert_eq!(argument("--debug-symbols"), "false");
        assert!(args.contains(&"--no-default-features"));
    }
    let index = fs::read_to_string(out.join("public/index.html")).unwrap();
    let shell = index
        .split("src=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    let shell = fs::read_to_string(out.join("public").join(shell.trim_start_matches('/'))).unwrap();
    let (manifest, body) = shell
        .strip_prefix("const modules = ")
        .expect("the shell starts with its manifest line")
        .split_once(";\n")
        .unwrap();
    let manifest: serde_json::Value = serde_json::from_str(manifest).unwrap();
    assert_eq!(manifest.as_object().unwrap().len(), KINDS.len());
    let store = fs::read_to_string(crate_dir().join("assets/cache.js")).unwrap();
    let source = fs::read_to_string(crate_dir().join("assets/shell.js")).unwrap();
    let minified = format!(
        "/*esbuild --minify --format=esm --log-level=warning*/\n{store}{}\n",
        source
            .lines()
            .filter(|line| *line != "const modules = __MODULES__;")
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(
        body.starts_with(&minified),
        "the rest of the shell is the client cache's store (assets/cache.js), then \
         assets/shell.js, through the minifier: {body}"
    );
    let shared = manifest["board"]["styles"][0].as_str().unwrap();
    assert!(
        shared.starts_with("/assets/shared-") && shared.ends_with(".css"),
        "{shared}"
    );
    assert_eq!(
        fs::read_to_string(out.join("public").join(shared.trim_start_matches('/'))).unwrap(),
        ".a{}\n.z{}\n\n",
        "the shared stylesheets in their order, each ending its line"
    );
    for kind in KINDS {
        for key in ["script", "wasm"] {
            let path = manifest[kind][key].as_str().unwrap();
            assert!(
                out.join("public")
                    .join(path.trim_start_matches('/'))
                    .is_file(),
                "{kind} {key}: {path}"
            );
        }
        // The task timeline, which lacks one of the pages' shared styles, does not narrow the
        // shared file: it is made from the pages' lists alone, and the timeline names it too.
        assert_eq!(
            manifest[kind]["styles"],
            serde_json::json!([shared, format!("/assets/board-{kind}.css")]),
            "{kind} loads the shared stylesheet, then its own"
        );
    }
}

/// The lines a stand-in that records its arguments wrote, one call per line.
fn calls(said: &Path) -> Vec<String> {
    fs::read_to_string(said)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// A probe and a page kind, each bundled as the release bundles it, then named: the linked module
/// without DWARF, through the release's wasm-bindgen and wasm-opt keeping the names.
#[test]
fn cargo_symphony_ui_size_bundles_a_probe_and_a_kind_and_names_their_linked_modules() {
    let scratch = Scratch::new("wasm-build-size");
    let common = scratch.0.join("common");
    let target = common.join("symphony/tools/target");
    let dx_said = scratch.0.join("dx-said");
    stand_in_tools(&common, &dx_said);
    let quoted = |path: &Path| format!("'{}'", path.display());
    let bindgen_said = scratch.0.join("bindgen-said");
    executable(
        &common.join(format!(
            "symphony/tools/dx-home/tools/wasm-bindgen-{}/wasm-bindgen",
            bindgen_version()
        )),
        &format!(
            r#"#!/bin/bash
echo "$*" >> {}
while [ "$#" -gt 0 ]; do
    case "$1" in
        --out-dir) dir="$2"; shift 2 ;;
        --out-name) name="$2"; shift 2 ;;
        *) shift ;;
    esac
done
mkdir -p "$dir"
printf 'bound' > "$dir/${{name}}_bg.wasm"
"#,
            quoted(&bindgen_said)
        ),
    );
    let opt_said = scratch.0.join("opt-said");
    executable(
        &common.join("symphony/tools/dx-home/tools/binaryen-129/bin/wasm-opt"),
        &format!(
            r#"#!/bin/bash
echo "$*" >> {}
while [ "$#" -gt 0 ]; do
    if [ "$1" = -o ]; then printf 'named' > "$2"; fi
    shift
done
"#,
            quoted(&opt_said)
        ),
    );
    let cargo_said = scratch.0.join("cargo-said");
    let cargo = scratch.0.join("cargo");
    executable(
        &cargo,
        &format!(
            r#"#!/bin/bash
echo "$*" >> {}
if [ "${{@: -3:1}}" = --strip-dwarf ]; then cp "${{@: -2:1}}" "${{@: -1}}"; fi
"#,
            quoted(&cargo_said)
        ),
    );
    let linked = target.join(WASM32).join("wasm-release");
    fs::create_dir_all(&linked).unwrap();
    for stem in ["floor_dioxus", "symphony-ui"] {
        fs::write(linked.join(format!("{stem}.wasm")), stem).unwrap();
    }
    let size = |names: &[&str]| {
        cargo_symphony_ui(&[&["size", "sizes"], names].concat())
            .current_dir(&scratch.0)
            .env("SYMPHONY_GIT_COMMON_DIR", &common)
            .env("CARGO_TARGET_DIR", &target)
            .env("CARGO", &cargo)
            .output()
            .expect("run cargo-symphony-ui")
    };
    let done = size(&["floor_dioxus", "review"]);
    succeeded(&done, "cargo symphony-ui size");
    let out = scratch.0.join("sizes").canonicalize().unwrap();

    // bundle.sh's arguments for a page kind (the dist test's), the probe's with its own bin.
    for (name, bin, features) in [
        ("floor_dioxus", "floor_dioxus", "web,size-probe"),
        ("review", "symphony-ui", "page-review"),
    ] {
        let dx = fs::read_to_string(dx_said.join(name)).expect("dx never bundled it");
        assert_eq!(
            dx.lines().collect::<Vec<_>>(),
            [
                "bundle",
                "--web",
                "--bin",
                bin,
                "--release",
                "--locked",
                "--no-default-features",
                "--features",
                features,
                "--debug-symbols",
                "false",
                "--out-dir",
                &out.join(name).display().to_string(),
            ]
        );
    }

    let stripped: Vec<_> = calls(&cargo_said)
        .into_iter()
        .filter(|call| call.contains("--strip-dwarf"))
        .collect();
    assert_eq!(
        stripped,
        ["floor_dioxus", "symphony-ui"].map(|stem| format!(
            "run --quiet --locked -p symphony-ui --example wasm_size -- --strip-dwarf {} {}",
            linked.join(format!("{stem}.wasm")).display(),
            out.join(format!(
                "{}-nodwarf.wasm",
                if stem == "symphony-ui" {
                    "review"
                } else {
                    stem
                }
            ))
            .display()
        ))
    );
    let bindgen = calls(&bindgen_said);
    assert_eq!(bindgen.len(), 2);
    for (call, (name, stem)) in bindgen
        .iter()
        .zip([("floor_dioxus", "floor_dioxus"), ("review", "symphony-ui")])
    {
        assert_eq!(
            *call,
            format!(
                "{} --target web --no-demangle --keep-lld-exports --out-name {stem} --out-dir {}",
                out.join(format!("{name}-nodwarf.wasm")).display(),
                out.join(format!("{name}-bindgen")).display()
            )
        );
    }
    let opt = calls(&opt_said);
    assert_eq!(opt.len(), 2);
    for (call, (name, stem)) in opt
        .iter()
        .zip([("floor_dioxus", "floor_dioxus"), ("review", "symphony-ui")])
    {
        assert_eq!(
            *call,
            format!(
                "{} -Oz -o {} --enable-reference-types --enable-bulk-memory --enable-mutable-globals --enable-nontrapping-float-to-int --enable-threads --debuginfo",
                out.join(format!("{name}-bindgen/{stem}_bg.wasm")).display(),
                out.join(format!("{name}-named.wasm")).display()
            )
        );
        assert_eq!(
            fs::read(out.join(format!("{name}.wasm"))).unwrap(),
            b"\0asm\x01\0\0\0"
        );
        assert_eq!(
            fs::read_to_string(out.join(format!("{name}-nodwarf.wasm"))).unwrap(),
            stem
        );
    }
    let stdout = String::from_utf8(done.stdout).unwrap();
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "{stdout}");
    for (line, name) in lines.iter().zip(["floor_dioxus", "review"]) {
        let numbers: Vec<usize> = line
            .split(|c: char| !c.is_ascii_digit())
            .filter(|word| !word.is_empty())
            .take(3)
            .map(|word| word.parse().unwrap())
            .collect();
        assert!(line.starts_with(&format!("{name}: WASM ")), "{line}");
        assert!(numbers[0] > 0 && numbers[1] > 0, "{line}");
        assert_eq!(numbers[0] + numbers[1], numbers[2], "{line}");
        assert!(line.ends_with(&format!(
            "named copy {}",
            out.join(format!("{name}-named.wasm")).display()
        )));
    }

    let refused = size(&["floor_dioxus", "nonsense"]);
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr)
            .contains("nonsense is neither a page kind nor a probe"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
    assert_eq!(
        calls(&opt_said).len(),
        2,
        "a refused name still built something"
    );
}

#[test]
fn cargo_symphony_ui_clippy_cleans_another_trees_wasm32_build_before_it_lints_the_browser_code() {
    let scratch = Scratch::new("wasm-build-clippy");
    let target = another_trees_build(&scratch.0.join("common"));
    let said = scratch.0.join("cargo-said");
    let cargo = scratch.0.join("cargo");
    executable(
        &cargo,
        &format!("#!/bin/bash\necho \"$*\" >> '{}'\n", said.display()),
    );
    let lint = || {
        let done = cargo_symphony_ui(&["clippy"])
            .env("CARGO", &cargo)
            .env("CARGO_TARGET_DIR", &target)
            .output()
            .expect("run cargo-symphony-ui");
        succeeded(&done, "cargo symphony-ui clippy");
    };
    let mut clippy: Vec<_> = KINDS.iter().map(|kind| format!("clippy -p symphony-ui --target {WASM32} --lib --bins --locked --no-default-features --features web,page-{kind} -- -D warnings")).collect();
    clippy.push(format!("clippy -p symphony-ui --target {WASM32} --bin symphony-ui-dev --locked --no-default-features --features web,devtools -- -D warnings"));
    clippy.push(format!("clippy -p symphony-ui --target {WASM32} --bin floor_dioxus --bin floor_websys --bin entry_websys --locked --no-default-features --features web,size-probe -- -D warnings"));
    let mut expected = vec![
        format!("clean --workspace --target {WASM32} --profile dev --quiet --locked"),
        format!("clean --workspace --target {WASM32} --profile wasm-release --quiet --locked"),
    ];
    expected.extend(clippy.clone());
    lint();
    assert_eq!(
        fs::read_to_string(&said)
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        expected
    );
    lint();
    assert_eq!(
        fs::read_to_string(&said)
            .unwrap()
            .lines()
            .skip(expected.len())
            .collect::<Vec<_>>(),
        clippy,
        "the same tree again cleans nothing"
    );
}

/// The guard names the tree as the wrapper does, by the workspace manifest's inode and birth time. A
/// write of the manifest's metadata alone moves its ctime and nothing else, as macOS rewriting a file a
/// test once cloned did (tools/cargo-tree-key-ctime): the same tree, which cleans nothing. The manifest
/// made again is another tree. A record in the key from before birth times, the manifest's inode and
/// ctime, names this tree while that key names the manifest as it is now.
#[test]
fn a_write_of_the_manifests_metadata_alone_keeps_the_wasm32_builds() {
    let scratch = Scratch::new("wasm-build-metadata");
    let tree = scratch.0.join("tree");
    let fresh = tree.join("apps/symphony-ui/wasm-fresh.sh");
    executable(
        &fresh,
        &fs::read_to_string(crate_dir().join("wasm-fresh.sh")).unwrap(),
    );
    let manifest = tree.join("Cargo.toml");
    fs::write(&manifest, "[workspace]\n").unwrap();
    if fs::metadata(&manifest).unwrap().created().is_err() {
        eprintln!(
            "{}: no birth time here, so the key is the ctime a metadata write moves",
            scratch.0.display()
        );
        return;
    }
    let said = scratch.0.join("cargo-said");
    let cargo = scratch.0.join("cargo");
    executable(
        &cargo,
        &format!("#!/bin/bash\necho \"$*\" >> '{}'\n", said.display()),
    );
    let target = tree.join(".target");
    let cleaned = || {
        let before = calls(&said).len();
        let done = Command::new("bash")
            .arg(&fresh)
            .env("CARGO", &cargo)
            .env("CARGO_TARGET_DIR", &target)
            .output()
            .expect("run wasm-fresh.sh");
        succeeded(&done, "wasm-fresh.sh");
        calls(&said)[before..].to_vec()
    };
    let both = [
        format!("clean --workspace --target {WASM32} --profile dev --quiet --locked"),
        format!("clean --workspace --target {WASM32} --profile wasm-release --quiet --locked"),
    ];
    assert_eq!(cleaned(), both, "a folder that records no tree");
    assert!(cleaned().is_empty(), "the same tree again");

    // The key holds whole seconds: the write lands in a second after the one the manifest was made in.
    let ctime = fs::metadata(&manifest).unwrap().ctime();
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    let next = Duration::from_secs(u64::try_from(ctime).unwrap() + 1);
    std::thread::sleep(next.saturating_sub(now) + Duration::from_millis(20));
    for mode in [0o600, 0o644] {
        fs::set_permissions(&manifest, fs::Permissions::from_mode(mode)).unwrap();
    }
    assert_ne!(
        fs::metadata(&manifest).unwrap().ctime(),
        ctime,
        "the write kept the ctime, so it proves nothing"
    );
    assert!(cleaned().is_empty(), "a metadata write cleaned the builds");

    let again = tree.join("Cargo.toml.new");
    fs::write(&again, "[workspace]\n").unwrap();
    fs::rename(&again, &manifest).unwrap();
    assert_eq!(cleaned(), both, "the manifest made again");

    let record = record_in(&target);
    let key = fs::read_to_string(&record).unwrap();
    let (named, _) = key.rsplit_once(' ').unwrap();
    let meta = fs::metadata(&manifest).unwrap();
    assert!(
        named.ends_with(&format!(" {}", meta.ino())),
        "{key:?} names no inode"
    );
    // Its ctime a second after its birth time, so the two keys differ.
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    let next = Duration::from_secs(u64::try_from(meta.ctime()).unwrap() + 1);
    std::thread::sleep(next.saturating_sub(now) + Duration::from_millis(20));
    for mode in [0o600, 0o644] {
        fs::set_permissions(&manifest, fs::Permissions::from_mode(mode)).unwrap();
    }
    let old = format!("{named} {}", fs::metadata(&manifest).unwrap().ctime());
    assert_ne!(old, key);
    fs::write(&record, &old).unwrap();
    assert!(
        cleaned().is_empty(),
        "the earlier key's record cleaned the builds"
    );
    assert_eq!(fs::read_to_string(&record).unwrap(), key);
}

#[test]
fn the_justfile_bundles_the_client_and_lints_its_browser_code_through_the_cargo_wrapper() {
    let scratch = Scratch::new("workspace-recipes");
    let tree = scratch.0.join("workspace with spaces");
    fs::create_dir_all(&tree).unwrap();
    let tree = tree.canonicalize().unwrap();
    // The justfile and the Symphony's recipes it imports.
    for name in ["justfile", "symphony.just"] {
        fs::copy(crate_dir().join("../..").join(name), tree.join(name)).unwrap();
    }
    executable(
        &tree.join("tools/cargo.sh"),
        r#"#!/bin/bash
printf 'cwd=%s\n' "$PWD"
case "$PATH" in "$PWD/apps/symphony-ui/bin:"*) printf 'client-path=yes\n' ;; *) printf 'client-path=no\n' ;; esac
printf 'arg=%s\n' "$@"
"#,
    );
    let output = tree.join("output with spaces");
    let done = Command::new("just")
        .current_dir(&tree)
        .args(["symphony-ui-dist", "output with spaces"])
        .output()
        .unwrap();
    succeeded(&done, "own workspace dist recipe");
    assert_eq!(
        String::from_utf8(done.stdout).unwrap(),
        format!(
            "cwd={}\nclient-path=yes\narg=symphony-ui\narg=dist\narg={}\narg=all\narg=z\n",
            tree.display(),
            output.display()
        )
    );
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
                "cwd={0}\nclient-path=no\narg=fmt\narg=--all\narg=--check\n",
                "cwd={0}\nclient-path=no\narg=clippy\narg=--workspace\narg=--all-targets\narg=--locked\narg=--\narg=-D\narg=warnings\n",
                "cwd={0}\nclient-path=yes\narg=symphony-ui\narg=clippy\n"
            ),
            tree.display()
        )
    );
}

/// fleet-badger-100: in a tree's own build folder (`TREE/.target`, the wrapper's CARGO_TARGET_DIR) the
/// record went beside the folder, into the tree's root, a file the tree does not track: the client's
/// clippy, a line of `lint-rs`, changed the tree under the verify that ran it, and the verify's stamp
/// was refused. A lint leaves the tree's status as it was.
#[test]
fn a_lint_in_a_trees_own_build_folder_leaves_the_trees_status_as_it_was() {
    let scratch = Scratch::new("wasm-build-status");
    let tree = scratch.0.join("tree");
    fs::create_dir_all(&tree).unwrap();
    fs::copy(
        crate_dir().join("../../.gitignore"),
        tree.join(".gitignore"),
    )
    .unwrap();
    git(&tree, &["init", "-q"]);
    git(&tree, &["add", "."]);
    git(&tree, &["commit", "-qm", "the tree", "--no-verify"]);
    let status = || git(&tree, &["status", "--porcelain", "--untracked-files=all"]);
    let before = status();
    let cargo = scratch.0.join("cargo");
    executable(&cargo, "#!/bin/bash\n");
    let done = cargo_symphony_ui(&["clippy"])
        .current_dir(&tree)
        .env("CARGO", &cargo)
        .env("CARGO_TARGET_DIR", tree.join(".target"))
        .output()
        .expect("run cargo-symphony-ui");
    succeeded(&done, "cargo symphony-ui clippy");
    assert_eq!(status(), before, "the lint changed the tree it ran in");
    assert!(record_in(&tree.join(".target")).is_file());
}
