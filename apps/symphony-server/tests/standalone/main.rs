//! Ordinary exec contracts and one compiling copy of the actual standalone workspace.
#![forbid(unsafe_code)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::time::Instant;
use symphony_board::sys::{Command, Stdio, output, wait_output};

#[path = "../../../../crates/symphony/tests/common/tree.rs"]
mod tree;
use tree::Tree;
#[path = "../../build.rs"]
mod client_build;

fn owned_scripts(tree: &Tree) -> PathBuf {
    let root = tree.path("workspace with spaces");
    fs::create_dir_all(root.join("tools")).unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools");
    for name in ["cargo.sh", "rs-bin.sh"] {
        fs::copy(source.join(name), root.join("tools").join(name)).unwrap();
    }
    tree.write(
        "workspace with spaces/Cargo.toml",
        "[workspace]\nmembers=[]\n",
    );
    executable(
        &tree.path("tools/rs-bin.sh"),
        "#!/bin/sh\nprintf reached > \"${0%/*}/../hostile-parent-ran\"\nexit 91\n",
    );
    root.canonicalize().unwrap()
}

fn executable(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn cargo_exec_preserves_arguments_streams_status_and_inherited_limits() {
    let tree = Tree::new();
    let root = owned_scripts(&tree);
    let cargo = tree.path("cargo stand-in");
    executable(
        &cargo,
        r#"#!/bin/bash
printf 'cwd=%s\njobs=%s\nthreads=%s\ntarget=%s\n' "$PWD" "$CARGO_BUILD_JOBS" "$RUST_TEST_THREADS" "$CARGO_TARGET_DIR"
printf 'arg=<%s>\n' "$@"
cat
printf 'diagnostic\n' >&2
if [ "${SELF_TERM:-}" = yes ]; then kill -TERM $$; fi
exit 7
"#,
    );
    let input = tree.write("stdin", "caller input\n");
    let done = wait_output(
        Command::new(root.join("tools/cargo.sh"))
            .args(["test", "one argument", "--", "--exact"])
            .current_dir(&tree.root)
            .env("CARGO", &cargo)
            .env("CARGO_BUILD_JOBS", "1")
            .env("RUST_TEST_THREADS", "3")
            .env("CARGO_TARGET_DIR", "relative-target")
            .stdin(Stdio::from(fs::File::open(input).unwrap()))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(done.status.code(), Some(7));
    assert_eq!(done.stderr, b"diagnostic\n");
    assert_eq!(
        String::from_utf8(done.stdout).unwrap(),
        format!(
            "cwd={}\njobs=1\nthreads=3\ntarget=relative-target\narg=<test>\narg=<one argument>\narg=<-->\narg=<--exact>\ncaller input\n",
            root.display()
        )
    );
    let interrupted = output(
        Command::new(root.join("tools/cargo.sh"))
            .env("CARGO", &cargo)
            .env("SELF_TERM", "yes")
            .stdin(Stdio::null()),
    )
    .unwrap();
    assert_eq!(interrupted.status.signal(), Some(15));
    fs::remove_file(root.join("Cargo.toml")).unwrap();
    let missing = output(Command::new(root.join("tools/cargo.sh")).env("CARGO", cargo)).unwrap();
    assert_eq!(missing.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("missing workspace manifest"));
    assert!(missing.stdout.is_empty());
}

#[test]
fn binary_launcher_refuses_missing_own_sources_and_never_executes_a_parent() {
    let tree = Tree::new();
    let root = owned_scripts(&tree);
    for args in [
        vec!["--where", "symphony"],
        vec!["--build", "symphony"],
        vec!["slots", "cargo"],
        vec![],
    ] {
        let diagnostic = if args.first().is_some_and(|arg| arg.starts_with("--")) {
            format!(
                "symphony rs-bin: missing owned source for symphony under {}\n",
                root.display()
            )
        } else {
            format!(
                "symphony rs-bin: no declared binary {} in {}\n",
                args.first().copied().unwrap_or(""),
                root.display()
            )
        };
        let done = output(Command::new(root.join("tools/rs-bin.sh")).args(&args)).unwrap();
        assert_eq!(done.status.code(), Some(2));
        assert!(done.stdout.is_empty());
        assert_eq!(String::from_utf8(done.stderr).unwrap(), diagnostic);
        assert!(!tree.path("hostile-parent-ran").exists());
    }
    tree.write(
        "workspace with spaces/crates/symphony/Cargo.toml",
        "[package]\nname='symphony-app'\n",
    );
    tree.write(
        "workspace with spaces/crates/symphony/src/bin/symphony.rs",
        "fn main() {}\n",
    );
    let done = output(
        Command::new(root.join("tools/rs-bin.sh"))
            .args(["--where", "symphony"])
            .env_remove("CARGO_TARGET_DIR"),
    )
    .unwrap();
    assert!(done.status.success(), "{done:?}");
    assert_eq!(
        String::from_utf8(done.stdout).unwrap().trim(),
        root.join(".target/debug/symphony").to_str().unwrap()
    );
    assert!(!tree.path("hostile-parent-ran").exists());
    assert!(
        !root.join(".target/debug/symphony").exists(),
        "where is a location, not a prepared image"
    );
}

fn copy_source(source: &Path, destination: &Path, artifacts: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if entry.path() == artifacts
            || [".target", "build", ".git", "node_modules"]
                .iter()
                .any(|skip| name == *skip)
        {
            continue;
        }
        let to = destination.join(name);
        if entry.file_type().unwrap().is_dir() {
            copy_source(&entry.path(), &to, artifacts);
        } else if entry.file_type().unwrap().is_symlink() {
            std::os::unix::fs::symlink(fs::read_link(entry.path()).unwrap(), to).unwrap();
        } else {
            fs::copy(entry.path(), to).unwrap();
        }
    }
}

#[test]
fn source_copy_omits_dependencies_and_only_the_configured_artifacts() {
    for artifacts in ["artifacts", "scratch/proofs"] {
        let tree = Tree::new();
        let source = tree.path("source");
        let destination = tree.path("copy");
        let excluded = [
            ".git/HEAD",
            ".target/debug/server",
            "build/output",
            "node_modules/root-package/index.js",
            "apps/symphony-web/node_modules/nested-package/index.js",
            &format!("{artifacts}/standalone-proof/server"),
        ];
        for name in excluded {
            tree.write(&format!("source/{name}"), "excluded");
        }
        let kept = [
            "Cargo.toml",
            "tools/cargo.sh",
            "apps/symphony-server/src/main.rs",
            "apps/symphony-server/client/public/index.html",
            "apps/symphony-server/client/public/assets/module.js",
            &format!("{artifacts}-source/input"),
            &format!("crates/example/{artifacts}/input"),
        ];
        for name in kept {
            tree.write(&format!("source/{name}"), name);
        }
        if artifacts != "artifacts" {
            tree.write("source/artifacts/input", "ordinary source");
        }
        executable(&source.join("tools/rs-bin.sh"), "#!/bin/sh\nexit 0\n");
        for (name, target) in [
            ("linked-file", "Cargo.toml"),
            ("linked-directory", "apps/symphony-server/src"),
            ("dangling-link", "absent"),
            ("tools/node_modules", "../node_modules"),
        ] {
            std::os::unix::fs::symlink(target, source.join(name)).unwrap();
        }

        copy_source(&source, &destination, &source.join(artifacts));

        for name in excluded {
            assert!(!destination.join(name).exists(), "copied {name}");
        }
        assert!(!destination.join(artifacts).exists());
        assert!(fs::symlink_metadata(destination.join("tools/node_modules")).is_err());
        for name in kept {
            assert_eq!(fs::read_to_string(destination.join(name)).unwrap(), name);
        }
        if artifacts != "artifacts" {
            assert_eq!(tree.read("copy/artifacts/input"), "ordinary source");
        }
        assert_eq!(tree.mode("copy/tools/rs-bin.sh"), 0o755);
        for name in ["linked-file", "linked-directory", "dangling-link"] {
            assert_eq!(
                fs::read_link(destination.join(name)).unwrap(),
                fs::read_link(source.join(name)).unwrap()
            );
        }
    }
}

fn independent_path(tree: &Tree) -> std::ffi::OsString {
    let forbidden = tree.path("project-tools-forbidden");
    fs::create_dir_all(&forbidden).unwrap();
    let cargo = PathBuf::from(std::env::var_os("CARGO").expect("Cargo names its executable"));
    let directories = [
        cargo.parent().unwrap().to_path_buf(),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/usr/sbin"),
        PathBuf::from("/sbin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ];
    let directories: Vec<_> = directories
        .into_iter()
        .filter(|directory| directory.is_dir())
        .collect();
    // A consuming project's own tools, as a project's checkout puts them first on PATH: its slots,
    // gates, landing, nightly and hooks, none of which the workspace's recipes may run.
    for name in [
        "project-slots",
        "project-rust-tests",
        "project-land",
        "project-verify",
        "project-nightly",
        "project-hooks",
    ] {
        executable(
            &forbidden.join(name),
            "#!/bin/sh\nprintf '%s\\n' \"$0\" >> \"$SYMPHONY_FORBIDDEN_LOG\"\nexit 91\n",
        );
    }
    std::env::join_paths(std::iter::once(forbidden).chain(directories)).unwrap()
}

fn execute_server_recipe(
    source: &Path,
    root: &Path,
    target: &Path,
    path: &std::ffi::OsStr,
    forbidden_log: &Path,
) {
    use sha2::Digest;
    let done = output(
        Command::new("just")
            .arg("--justfile")
            .arg(root.join("justfile"))
            .args(["--", "symphony", "--help"])
            .env("PATH", path)
            .env("CARGO_TARGET_DIR", target)
            .env("CARGO_NET_OFFLINE", "true")
            .env("SYMPHONY_FORBIDDEN_LOG", forbidden_log)
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("RUSTC_WRAPPER")
            .env_remove("RUSTC_WORKSPACE_WRAPPER"),
    )
    .unwrap();
    assert_eq!(done.status.code(), Some(0), "{done:?}");
    let help = String::from_utf8(done.stdout).unwrap();
    assert!(help.starts_with("usage: symphony-server "), "{help}");
    assert!(help.contains("--port N"), "{help}");
    assert!(
        done.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    assert!(!forbidden_log.exists(), "a project executable ran");
    let binary = target.join("debug/symphony-server");
    let bytes = fs::read(&binary).unwrap();
    let hash: String = sha2::Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let evidence = source
        .join("artifacts/standalone-proof")
        .join(format!("{}-{time}", std::process::id()));
    fs::create_dir_all(evidence.parent().unwrap()).unwrap();
    fs::create_dir(&evidence).unwrap();
    fs::copy(&binary, evidence.join("symphony-server")).unwrap();
    fs::write(evidence.join("receipt.json"), serde_json::to_vec_pretty(&serde_json::json!({"source":root,"binary":binary,"sha256":hash,"bytes":bytes.len(),"command":["just","symphony","--help"],"status":0,"stdout":help,"stderr":"","project_executables":"forbidden on PATH and parent launcher"})).unwrap()).unwrap();
    eprintln!("standalone real execution evidence: {}", evidence.display());
}

fn copy_native_dependencies(source: &Path, target: &Path) {
    let donor = source.join(".target");
    if donor.join("debug").is_dir() {
        fs::create_dir_all(target).unwrap();
        let mut copy = Command::new("cp");
        copy.arg(if cfg!(target_os = "macos") {
            "-cR"
        } else {
            "-R"
        })
        .arg(donor.join("debug"))
        .arg(target.join("debug"));
        let done = output(&mut copy).unwrap();
        assert!(done.status.success(), "{done:?}");
        if donor.join(".rustc_info.json").is_file() {
            fs::copy(
                donor.join(".rustc_info.json"),
                target.join(".rustc_info.json"),
            )
            .unwrap();
        }
    }
}

#[test]
fn actual_standalone_workspace_compiles_its_server_without_a_containing_project() {
    let tree = Tree::new();
    let root = tree.path("actual standalone workspace");
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let setup = Instant::now();
    let path = independent_path(&tree);
    let forbidden_log = tree.path("project-executable-ran");
    let project = symphony::project::Project::load(&source).unwrap();
    copy_source(&source, &root, &source.join(project.artifacts));
    tree.write("Cargo.toml", "[workspace]\nmembers=[]\n");
    executable(
        &tree.path("tools/rs-bin.sh"),
        "#!/bin/sh\nprintf reached > \"${0%/*}/../hostile-parent-ran\"\nexit 91\n",
    );
    // Dependency artifacts only accelerate setup. Clean every copied workspace product before
    // compiling this source; a cache path/file never establishes a prepared executable.
    let target = tree.path("target");
    copy_native_dependencies(&source, &target);
    let setup_time = setup.elapsed();
    let run = |args: &[&str]| {
        let started = Instant::now();
        let done = output(
            Command::new(root.join("tools/cargo.sh"))
                .args(args)
                .env("CARGO_TARGET_DIR", &target)
                .env("CARGO_NET_OFFLINE", "true")
                .env("PATH", &path)
                .env("SYMPHONY_FORBIDDEN_LOG", &forbidden_log)
                .env_remove("RUSTFLAGS")
                .env_remove("CARGO_ENCODED_RUSTFLAGS")
                .env_remove("RUSTC_WRAPPER")
                .env_remove("RUSTC_WORKSPACE_WRAPPER")
                .env_remove("SYMPHONY_GIT_COMMON_DIR"),
        )
        .unwrap();
        eprintln!(
            "standalone {args:?}: {:.3}s",
            started.elapsed().as_secs_f64()
        );
        eprintln!(
            "standalone output: {}{}",
            String::from_utf8_lossy(&done.stdout),
            String::from_utf8_lossy(&done.stderr)
        );
        assert!(
            done.status.success(),
            "{}{}",
            String::from_utf8_lossy(&done.stdout),
            String::from_utf8_lossy(&done.stderr)
        );
    };
    run(&["clean", "--workspace", "--locked"]);
    // Building both checks and links the server: this one compile proves both.
    run(&[
        "build",
        "--locked",
        "--offline",
        "-p",
        "symphony-server",
        "--bin",
        "symphony-server",
    ]);
    execute_server_recipe(&source, &root, &target, &path, &forbidden_log);
    eprintln!(
        "standalone setup: {:.3}s; real compiled server help and no project executables",
        setup_time.as_secs_f64()
    );
    assert!(!tree.path("hostile-parent-ran").exists());
    assert!(!forbidden_log.exists());
    assert!(!root.join("symphony").exists());
    assert!(!root.join("apps/symphony-host").exists());
}

#[test]
fn binary_launcher_builds_then_execs_the_native_output_with_the_callers_cwd() {
    let tree = Tree::new();
    let root = owned_scripts(&tree);
    tree.write(
        "workspace with spaces/crates/symphony/Cargo.toml",
        "[package]\nname='symphony-app'\n",
    );
    tree.write(
        "workspace with spaces/crates/symphony/src/bin/symphony.rs",
        "fn main() {}\n",
    );
    let cargo = tree.path("cargo");
    executable(
        &cargo,
        r#"#!/bin/bash
printf '%s\n' "$@" > "$TRACE"
mkdir -p "$CARGO_TARGET_DIR/debug"
cat > "$CARGO_TARGET_DIR/debug/symphony" <<'PROGRAM'
#!/bin/bash
printf 'cwd=%s\n' "$PWD"
printf 'subject=<%s>\n' "$@"
cat
printf 'subject diagnostic\n' >&2
exit 6
PROGRAM
chmod 755 "$CARGO_TARGET_DIR/debug/symphony"
"#,
    );
    let trace = tree.path("build arguments");
    let input = tree.write("subject input", "input survives\n");
    let done = wait_output(
        Command::new(root.join("tools/rs-bin.sh"))
            .args(["symphony", "one argument", "--root", "."])
            .current_dir(&tree.root)
            .env("CARGO", &cargo)
            .env("TRACE", &trace)
            .env_remove("CARGO_TARGET_DIR")
            .stdin(Stdio::from(fs::File::open(input).unwrap()))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(done.status.code(), Some(6));
    assert_eq!(done.stderr, b"subject diagnostic\n");
    assert_eq!(
        String::from_utf8(done.stdout).unwrap(),
        format!(
            "cwd={}\nsubject=<one argument>\nsubject=<--root>\nsubject=<.>\ninput survives\n",
            tree.root.canonicalize().unwrap().display()
        )
    );
    assert_eq!(
        fs::read_to_string(trace).unwrap(),
        "build\n--quiet\n--locked\n-p\nsymphony-app\n--bin\nsymphony\n"
    );
}
