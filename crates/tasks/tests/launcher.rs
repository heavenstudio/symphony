//! The SDK launcher routes live-board commands to main and keeps explicit scratch runs local.
//! Each case owns its repositories and uses a stand-in Cargo and executable; no real board is reached.

#![expect(
    clippy::disallowed_methods,
    reason = "a test starts its fixtures and stand-ins as it likes; the board's children are sys's"
)]

// Each run's scratch in a folder of its own under the temp folder, swept when a killed run left it.
#[path = "../../../tools/test-support/harness_scratch.rs"]
mod harness_scratch;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The stand-in for `cargo build --quiet --locked -p PACKAGE --bin NAME`, run in the checkout's root;
/// `@BUILD@` is the shared stand-in executable; no real compiler or board client runs.
const CARGO: &str = r#"#!/usr/bin/env bash
set -euo pipefail
name=""; while [ $# -gt 0 ]; do if [ "$1" = --bin ]; then name="$2"; fi; shift; done
[ -n "$name" ] || exit 0
mkdir -p "$CARGO_TARGET_DIR/debug"
ln -f "@BUILD@" "$CARGO_TARGET_DIR/debug/$name"
"#;

/// The shared stand-in build, run as `CHECKOUT/.target/debug/NAME`: what that checkout's source of
/// NAME holds, where it runs and with what.
const BUILD: &str = r#"#!/bin/sh
root="${0%/.target/debug/*}"
name="${0##*/}"
for source in "$root/crates/tasks/src/bin/$name.rs" "$root/crates/symphony/src/bin/$name.rs"; do
    [ -f "$source" ] && break
done
echo "$name from $root source $(cat "$source" 2>/dev/null) exe $0 cwd $(pwd -P) args $*"
"#;

/// The shared stand-in Cargo, its build made and both run once, so no case pays their first run.
fn standin_cargo() -> &'static Path {
    static CARGO_PATH: OnceLock<PathBuf> = OnceLock::new();
    CARGO_PATH.get_or_init(|| {
        let folder = scratch("standin");
        let (cargo, build) = (folder.join("cargo"), folder.join("build"));
        fs::write(&build, BUILD).unwrap();
        let build_path = build.to_str().unwrap();
        fs::write(&cargo, CARGO.replace("@BUILD@", build_path)).unwrap();
        for script in [&cargo, &build] {
            fs::set_permissions(script, fs::Permissions::from_mode(0o755)).unwrap();
            assert!(Command::new(script).output().unwrap().status.success());
        }
        cargo
    })
}

/// The binaries a fixture declares, each with the source folder the launcher checks.
const BINARIES: [(&str, &str); 4] = [
    ("tasks", "crates/tasks"),
    ("agent-send", "crates/tasks"),
    ("fake-hc", "crates/tasks"),
    ("symphony", "crates/symphony"),
];

/// A repository whose main checkout (`main/`) holds the launcher and `main` as every binary's source,
/// and a linked worktree of it (`wt/`, branch `feature`) holding `branch`.
struct Fixture {
    base: PathBuf,
    main: PathBuf,
    wt: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        let base = scratch(name);
        let main = base.join("main");
        let tools = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools")
            .canonicalize()
            .unwrap();
        fs::create_dir_all(main.join("tools")).unwrap();
        for tool in ["rs-bin.sh", "cargo.sh"] {
            std::os::unix::fs::symlink(tools.join(tool), main.join("tools").join(tool)).unwrap();
        }
        fs::write(main.join("Cargo.toml"), "[workspace]\n").unwrap();
        sources(&main, "main");
        git(&base, &["init", "-q", "-b", "main", "main"]);
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-qm", "main"]);
        git(&main, &["worktree", "add", "-q", "-b", "feature", "../wt"]);
        let wt = base.join("wt");
        sources(&wt, "branch");
        Fixture { base, main, wt }
    }

    /// `checkout/tools/rs-bin.sh ARGS` run in `cwd`, with `vars` on an environment that names no
    /// board, no target folder and no git repository of its own.
    fn run(&self, checkout: &Path, cwd: &Path, vars: &[(&str, &str)], args: &[&str]) -> Ran {
        let mut command = Command::new(checkout.join("tools/rs-bin.sh"));
        for name in [
            "TASKS_SERVER",
            "TASKS_DATABASE",
            "CARGO_TARGET_DIR",
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_COMMON_DIR",
        ] {
            command.env_remove(name);
        }
        let done = command
            .args(args)
            .current_dir(cwd)
            .env("CARGO", standin_cargo())
            .envs(vars.iter().copied())
            .output()
            .unwrap();
        Ran {
            code: done.status.code().unwrap_or(-1),
            out: String::from_utf8(done.stdout).unwrap().trim().to_string(),
            err: String::from_utf8(done.stderr).unwrap(),
        }
    }

    /// The worktree's launcher, run there.
    fn in_wt(&self, vars: &[(&str, &str)], args: &[&str]) -> Ran {
        self.run(&self.wt, &self.wt, vars, args)
    }

    /// What `name` built in `checkout` says it ran, its arguments `args`, in `cwd`.
    fn said(&self, name: &str, checkout: &Path, cwd: &Path, args: &str) -> String {
        let source = if checkout == self.main {
            "main"
        } else {
            "branch"
        };
        format!(
            "{name} from {} source {source} exe {} cwd {} args {args}",
            checkout.display(),
            checkout.join(".target/debug").join(name).display(),
            cwd.display()
        )
    }

    /// The live board's server record, as the board writes it in the common dir.
    fn record(&self, text: &str) {
        let folder = self.main.join(".git/sf-tasks");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("server.json"), text).unwrap();
    }
}

struct Ran {
    code: i32,
    out: String,
    err: String,
}

impl Ran {
    /// What it printed, once it exited 0.
    fn ok(self) -> String {
        assert_eq!(self.code, 0, "{}", self.err);
        self.out
    }
}

#[test]
fn a_worktree_reaching_the_live_board_runs_the_main_checkouts_build_where_it_was_called() {
    let f = Fixture::new("live");
    let main = |name: &str, args: &str| f.said(name, &f.main, &f.wt, args);
    assert_eq!(f.in_wt(&[], &["tasks", "list"]).ok(), main("tasks", "list"));
    assert_eq!(
        f.in_wt(&[("TASKS_DATABASE", "tasks")], &["tasks", "list"])
            .ok(),
        main("tasks", "list")
    );
    // A leading option names the database over the environment, as the binary reads it.
    assert_eq!(
        f.in_wt(
            &[("TASKS_DATABASE", "scratch")],
            &[
                "tasks",
                "--database",
                "scratch",
                "--database",
                "tasks",
                "list"
            ]
        )
        .ok(),
        main("tasks", "--database scratch --database tasks list")
    );
    // Every board binary, its options anywhere but leading read as the live board's.
    assert_eq!(
        f.in_wt(
            &[],
            &["agent-send", "someone", "hi", "--database", "scratch"]
        )
        .ok(),
        main("agent-send", "someone hi --database scratch")
    );
    // The main checkout builds in its own target folder, never the caller's.
    let elsewhere = f.base.join("elsewhere");
    let target = elsewhere.to_str().unwrap();
    assert_eq!(
        f.in_wt(&[("CARGO_TARGET_DIR", target)], &["tasks", "show", "a b"])
            .ok(),
        main("tasks", "show a b")
    );
    assert!(
        !elsewhere.exists(),
        "main's build went to the caller's target folder"
    );
    assert!(
        !f.wt.join(".target").exists(),
        "the worktree built its own board code for the live board"
    );
}

#[test]
fn a_scratch_board_runs_the_worktrees_own_build() {
    let f = Fixture::new("scratch");
    let own = |name: &str, args: &str| f.said(name, &f.wt, &f.wt, args);
    assert_eq!(
        f.in_wt(&[("TASKS_DATABASE", "scratch")], &["tasks", "list"])
            .ok(),
        own("tasks", "list")
    );
    assert_eq!(
        f.in_wt(&[], &["tasks", "--database", "scratch", "list"])
            .ok(),
        own("tasks", "--database scratch list")
    );
    assert_eq!(
        f.in_wt(
            &[("TASKS_DATABASE", "scratch")],
            &["agent-send", "--database", "x", "someone", "hi"]
        )
        .ok(),
        own("agent-send", "--database x someone hi")
    );
    // A server named while the repository records no live one is a test's.
    assert_eq!(
        f.in_wt(&[("TASKS_SERVER", "127.0.0.1:4321")], &["tasks", "list"])
            .ok(),
        own("tasks", "list")
    );
}

#[test]
fn a_server_on_the_live_servers_port_is_the_live_board() {
    let f = Fixture::new("server");
    f.record(r#"{"addr":"127.0.0.1:4321","pid":1,"version":"3.3.0"}"#);
    let main = f.said("tasks", &f.main, &f.wt, "list");
    let own = f.said("tasks", &f.wt, &f.wt, "list");
    // The port is the number the binary reads (Rust's `u16`); a spelling it reads as no number is
    // the live one too, since the binary reaches nothing with it.
    for (server, database, expected) in [
        ("127.0.0.1:4321", "", &main),
        ("localhost:4321", "tasks", &main),
        ("127.0.0.1:04321", "", &main),
        ("127.0.0.1:+4321", "", &main),
        ("127.0.0.1:+004321", "", &main),
        ("127.0.0.1:4321x", "", &main),
        ("127.0.0.1:", "", &main),
        ("127.0.0.1:4322", "", &own),
        ("127.0.0.1:04322", "", &own),
        ("127.0.0.1:4321", "scratch", &own),
    ] {
        let vars = [("TASKS_SERVER", server), ("TASKS_DATABASE", database)];
        assert_eq!(
            &f.in_wt(&vars, &["tasks", "list"]).ok(),
            expected,
            "{vars:?}"
        );
    }
    // A record naming no address may be the live server's still: the main checkout's build runs.
    f.record("{}");
    let vars = [("TASKS_SERVER", "127.0.0.1:4322")];
    assert_eq!(f.in_wt(&vars, &["tasks", "list"]).ok(), main);
}

#[test]
fn the_main_checkout_another_repository_and_other_binaries_run_the_callers_own_build() {
    let f = Fixture::new("own");
    assert_eq!(
        f.run(&f.main, &f.main, &[], &["tasks", "list"]).ok(),
        f.said("tasks", &f.main, &f.main, "list")
    );
    // From a worktree in a folder of another repository, the board reached is that repository's,
    // and from a folder of none, no board's.
    let other = f.base.join("other");
    fs::create_dir(&other).unwrap();
    git(&other, &["init", "-q"]);
    for cwd in [&other, &f.base] {
        assert_eq!(
            f.run(&f.wt, cwd, &[], &["tasks", "list"]).ok(),
            f.said("tasks", &f.wt, cwd, "list")
        );
    }
    // From another folder of this repository, the board is this one's.
    let folder = f.wt.join("crates");
    assert_eq!(
        f.run(&f.wt, &folder, &[], &["tasks", "list"]).ok(),
        f.said("tasks", &f.main, &folder, "list")
    );
    for name in ["symphony", "fake-hc"] {
        assert_eq!(
            f.in_wt(&[], &[name, "x"]).ok(),
            f.said(name, &f.wt, &f.wt, "x")
        );
    }
    // A build for the caller to run is its own checkout's, whatever board it then reaches.
    assert_eq!(
        f.in_wt(&[], &["--build", "tasks"]).ok(),
        f.wt.join(".target/debug/tasks").display().to_string()
    );
}

#[test]
fn a_worktree_whose_main_checkout_has_no_launcher_is_refused_before_anything_runs() {
    let f = Fixture::new("refused");
    fs::remove_file(f.main.join("tools/rs-bin.sh")).unwrap();
    let ran = f.in_wt(&[], &["tasks", "claim", "x"]);
    assert_eq!(ran.code, 2, "{}", ran.err);
    assert_eq!(ran.out, "");
    assert!(
        ran.err.contains(&format!(
            "has no launcher at {}",
            f.main.join("tools/rs-bin.sh").display()
        )),
        "{}",
        ran.err
    );
    assert!(!f.wt.join(".target").exists(), "the worktree's build ran");
    assert_eq!(
        f.in_wt(&[("TASKS_DATABASE", "scratch")], &["tasks", "list"])
            .ok(),
        f.said("tasks", &f.wt, &f.wt, "list")
    );
}

/// Every declared binary's manifest and source in `checkout`, the source holding `text`.
fn sources(checkout: &Path, text: &str) {
    for (binary, member) in BINARIES {
        let bin = checkout.join(member).join("src/bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(checkout.join(member).join("Cargo.toml"), "[package]\n").unwrap();
        fs::write(bin.join(format!("{binary}.rs")), text).unwrap();
    }
}

/// `git ARGS` in `dir` as a committer named `t`, which must succeed.
fn git(dir: &Path, args: &[&str]) {
    let done = Command::new("git")
        .args(["-c", "user.email=t@t", "-c", "user.name=t", "-C"])
        .arg(dir)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .unwrap();
    assert!(
        done.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&done.stderr)
    );
}

/// This process's folder of scratch, removed when the binary exits.
static OWN_SCRATCH: OnceLock<PathBuf> = OnceLock::new();

/// A new folder `name` in this process's scratch under the temp folder's `tasks-launcher` folder.
fn scratch(name: &str) -> PathBuf {
    let own = OWN_SCRATCH.get_or_init(|| {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = harness_scratch::harness_root(&std::env::temp_dir(), "tasks-launcher")
            .unwrap_or_else(|error| panic!("the tests' scratch folder: {error}"));
        harness_scratch::reap_dead(&root);
        let own = harness_scratch::create(&root, &NEXT)
            .unwrap_or_else(|error| panic!("a scratch folder in {}: {error}", root.display()));
        // SAFETY: atexit only records a function that takes nothing, to run once at the exit.
        assert_eq!(unsafe { libc::atexit(remove_own_scratch) }, 0);
        own.canonicalize().unwrap()
    });
    static TAKEN: AtomicUsize = AtomicUsize::new(0);
    let dir = own.join(format!("{name}-{}", TAKEN.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir(&dir).unwrap();
    dir
}

extern "C" fn remove_own_scratch() {
    if let Some(own) = OWN_SCRATCH.get() {
        let _ = fs::remove_dir_all(own);
    }
}
