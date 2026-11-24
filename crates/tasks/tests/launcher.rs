//! `tools/rs-bin.sh`, the launcher every `just tasks`, `just agent-send` and `just symphony-cli` goes
//! through, in repositories of its own under the temp folder: links to the workspace's launcher and
//! cargo wrapper, over a stand-in Cargo (`CARGO`). Nothing here reaches a board or builds the
//! workspace.
//!
//! macOS assesses every new executable file at its first run, a script too, for seconds on a loaded
//! machine, one file at a time. So the fixtures run the workspace's own scripts through links and
//! share one stand-in Cargo, and by default the stand-in uplifts as Cargo does on Linux: a hard link
//! to one shared stand-in build, which says which checkout's target it runs from. Only the case of
//! the uplift Cargo does on macOS (`STANDIN_UPLIFT=copy`) links a build of its own: a script saying
//! which checkout's source it was built from, made only when that source changed, and copied to
//! `debug/NAME` on every build, a new file keeping the time of the one it copies.

// How a test starts its own children ([`children::Child`]).
#[path = "support/children.rs"]
mod children;

// Each run's scratch in a folder of its own under the temp folder, swept when a killed run left it.
#[path = "../../chrome-devtools/src/harness_scratch.rs"]
mod harness_scratch;
// `tools/seed-target.sh`, which `tools/cargo.sh` runs before Cargo.
#[path = "launcher/seed.rs"]
mod seed;
// A run that finds nothing Cargo reads changed runs its build without asking Cargo.
#[path = "launcher/fresh.rs"]
mod fresh;

use crate::children::Child as _;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The stand-in for `cargo build --quiet --locked -p PACKAGE --bin NAME`, run in the checkout's root;
/// `@BUILD@` is the shared stand-in build. As Cargo does, it builds one at a time per target folder,
/// writes `debug/NAME.d` naming the sources it read (the binary's, and those of a dependency crate
/// at `crates/dep` when there is one, with the folder `crates/dep/assets` its build script would
/// watch when there is that; none with `STANDIN_DEPINFO=none`, a path spelled with an escape with
/// `escaped`), leaves a build script's output declaring the variables it reads, and rustc's account
/// of the variables the sources read (`# env-dep:` in the binary's own and the build script's
/// dep-info; a binary whose source is `reads NAME` reads NAME too). Each run first appends its
/// binary's name to `STANDIN_CALLS`, when that names a file, and whether the target folder was new
/// to it. Building its own build (`STANDIN_UPLIFT=copy`), it writes `STANDIN_EDIT`, when that names
/// a file, once it has read the sources, and finishes only in a later second of the clock.
const CARGO: &str = r##"#!/usr/bin/env bash
set -euo pipefail
name=""; while [ $# -gt 0 ]; do if [ "$1" = --bin ]; then name="$2"; fi; shift; done
[ -n "$name" ] || exit 0
if [ -n "${STANDIN_CALLS:-}" ]; then
    if [ -d "$CARGO_TARGET_DIR" ]; then echo "$name" >> "$STANDIN_CALLS"; else echo "$name into a new target" >> "$STANDIN_CALLS"; fi
fi
mkdir -p "$CARGO_TARGET_DIR/debug/deps" "$CARGO_TARGET_DIR/debug/build/standin-0"
until mkdir "$CARGO_TARGET_DIR/standin.lock" 2>/dev/null; do sleep 0.01; done
trap 'rmdir "$CARGO_TARGET_DIR/standin.lock"' EXIT
for source in "crates/tasks/src/bin/$name.rs" "crates/symphony/src/bin/$name.rs" \
    "crates/symphony-build/src/bin/$name.rs"; do
    [ -f "$source" ] && break
done
inputs=("$PWD/$source")
if [ -d crates/dep/src ]; then
    for file in crates/dep/src/*.rs; do inputs+=("$PWD/$file"); done
fi
watched=""
if [ -d crates/dep/assets ]; then watched=" $PWD/crates/dep/assets"; fi
depinfo="$CARGO_TARGET_DIR/debug/$name.d"
case "${STANDIN_DEPINFO:-plain}" in
    none) rm -f "$depinfo" ;;
    escaped) printf '%s: %s %s\n' "$CARGO_TARGET_DIR/debug/$name" "${inputs[*]}" "$PWD/a\\ b.rs" > "$depinfo" ;;
    *) printf '%s: %s%s\n' "$CARGO_TARGET_DIR/debug/$name" "${inputs[*]}" "$watched" > "$depinfo" ;;
esac
printf 'cargo:rerun-if-env-changed=STANDIN_CC\ncargo:rerun-if-env-changed=CC_standin-triple\n' \
    > "$CARGO_TARGET_DIR/debug/build/standin-0/output"
probe="# env-dep:STANDIN_PROBE"
if [ -n "${STANDIN_PROBE+set}" ]; then probe="$probe=$STANDIN_PROBE"; fi
reads="$(cat "$source")"
case "$reads" in "reads "*) reads="# env-dep:${reads#reads }" ;; *) reads="" ;; esac
printf '%s: %s\n\n# env-dep:CARGO_PKG_NAME=standin\n# env-dep:CARGO_BIN_EXE_fake-hc=%s\n%s\n%s\n' \
    "$CARGO_TARGET_DIR/debug/deps/$name-0" "${inputs[*]}" "$CARGO_TARGET_DIR/debug/fake-hc" "$probe" \
    "$reads" > "$CARGO_TARGET_DIR/debug/deps/$name-0.d"
printf '# env-dep:STANDIN_BUILD_PROBE\n' > "$CARGO_TARGET_DIR/debug/build/standin-0/build_script_build-0.d"
uplifted="$CARGO_TARGET_DIR/debug/.$name.uplift"
if [ "${STANDIN_UPLIFT:-link}" = link ]; then
    ln -f "@BUILD@" "$uplifted"
    mv -f "$uplifted" "$CARGO_TARGET_DIR/debug/$name"
    exit 0
fi
text="$(cat "$source")"
for file in "${inputs[@]:1}"; do text="$text dep $(cat "$file")"; done
if [ -n "$watched" ]; then
    for file in $(find crates/dep/assets -type f | sort); do text="$text asset $(cat "$file")"; done
fi
if [ -n "${STANDIN_PROBE+set}" ]; then text="$text probe $STANDIN_PROBE"; fi
if [ -n "${STANDIN_EDIT:-}" ]; then
    printf 'edited while building' > "$STANDIN_EDIT"
    edited="$(date +%s)"
    while [ "$(date +%s)" -le "$edited" ]; do sleep 0.02; done
fi
deps="$CARGO_TARGET_DIR/debug/deps/$name-0"
if [ ! -f "$deps.source" ] || [ "$(cat "$deps.source")" != "$text" ]; then
    printf '#!/bin/sh\necho "%s from %s source %s exe $0 cwd $(pwd -P) args $*"\n' \
        "$name" "$PWD" "$text" > "$deps.new"
    chmod +x "$deps.new"
    mv -f "$deps.new" "$deps"
    printf '%s' "$text" > "$deps.source"
fi
cp -p "$deps" "$uplifted"
mv -f "$uplifted" "$CARGO_TARGET_DIR/debug/$name"
"##;

/// The shared stand-in build, run as `CHECKOUT/.target/rs-bin/NAME`: what that checkout's source of
/// NAME holds, where it runs and with what.
const BUILD: &str = r#"#!/bin/sh
root="${0%/.target/rs-bin/*}"
name="${0##*/}"
for source in "$root/crates/tasks/src/bin/$name.rs" "$root/crates/symphony/src/bin/$name.rs" \
    "$root/crates/symphony-build/src/bin/$name.rs"; do
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
            assert!(Command::of(script).output().unwrap().status.success());
        }
        cargo
    })
}

/// The binaries a fixture declares, each with the source folder the launcher checks.
const BINARIES: [(&str, &str); 5] = [
    ("tasks", "crates/tasks"),
    ("agent-send", "crates/tasks"),
    ("fake-hc", "crates/tasks"),
    ("symphony", "crates/symphony"),
    ("symphony-build", "crates/symphony-build"),
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
        let mut command = Command::of(checkout.join("tools/rs-bin.sh"));
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
            checkout.join(".target/rs-bin").join(name).display(),
            cwd.display()
        )
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
fn a_server_on_the_live_boards_socket_is_the_live_board() {
    let f = Fixture::new("server");
    let main = f.said("tasks", &f.main, &f.wt, "list");
    let own = f.said("tasks", &f.wt, &f.wt, "list");
    // The board names its socket by its data's path, the common dir's links followed.
    let common = f.main.join(".git").canonicalize().unwrap();
    let socket = symphony_board::server::postgres::socket_folder(
        &common
            .join(symphony_board::server::FOLDER)
            .join("postgresql"),
    );
    let live = format!("postgresql:{}", socket.display());
    let other = symphony_board::server::postgres::socket_folder(Path::new("/elsewhere/postgresql"));
    let other = format!("postgresql:{}", other.display());
    let mut cases = vec![
        (live.clone(), "", &main),
        (live.clone(), "tasks", &main),
        (other, "", &own),
        (live.clone(), "scratch", &own),
        // A SurrealDB server's address, which every board binary refuses, is no live board's.
        ("127.0.0.1:4321".to_string(), "", &own),
    ];
    // Where `/tmp` is a link (`/private/tmp` on macOS), the socket's folder named through the link's
    // target is the live board's too, once there is that folder to follow, as the board follows it.
    let followed = Path::new("/private").join(socket.strip_prefix("/").unwrap());
    let made = Path::new("/private/tmp").is_dir() && fs::create_dir(&socket).is_ok();
    if made {
        cases.push((format!("postgresql:{}", followed.display()), "", &main));
    }
    for (server, database, expected) in &cases {
        let vars = [
            ("TASKS_SERVER", server.as_str()),
            ("TASKS_DATABASE", *database),
        ];
        assert_eq!(
            &f.in_wt(&vars, &["tasks", "list"]).ok(),
            *expected,
            "{vars:?}"
        );
    }
    if made {
        fs::remove_dir(&socket).unwrap();
    }
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
    for name in ["symphony", "symphony-build", "fake-hc"] {
        assert_eq!(
            f.in_wt(&[], &[name, "x"]).ok(),
            f.said(name, &f.wt, &f.wt, "x")
        );
    }
    // A build for the caller to run is its own checkout's, whatever board it then reaches.
    assert_eq!(
        f.in_wt(&[], &["--build", "tasks"]).ok(),
        f.wt.join(".target/rs-bin/tasks").display().to_string()
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

#[test]
fn a_build_that_relinked_nothing_runs_the_same_file_and_a_relink_a_new_one() {
    let f = Fixture::new("stable");
    let target = f.main.join(".target");
    let (stable, copy) = (target.join("rs-bin/tasks"), target.join("debug/tasks"));
    let inode = |path: &Path| fs::metadata(path).unwrap().ino();
    // Every run here asks Cargo: with no account of what a build read, none can be found unchanged
    // (launcher/fresh.rs).
    let copying = [("STANDIN_UPLIFT", "copy"), ("STANDIN_DEPINFO", "none")];
    let run = || f.run(&f.main, &f.main, &copying, &["tasks", "list"]).ok();
    // The hooks' link: one naming anything else is left as it is; a run points one naming Cargo's
    // own copy at the file it runs, the same build, and `--build` leaves it.
    let hook = f.main.join(".target/bin/tasks");
    fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink("/elsewhere/tasks", &hook).unwrap();
    run();
    assert_eq!(fs::read_link(&hook).unwrap(), Path::new("/elsewhere/tasks"));
    fs::remove_file(&hook).unwrap();
    std::os::unix::fs::symlink(&copy, &hook).unwrap();
    let built = f.run(&f.main, &f.main, &copying, &["--build", "tasks"]);
    assert_eq!(built.ok(), stable.display().to_string());
    assert_eq!(
        fs::read_link(&hook).unwrap(),
        copy,
        "--build moved the link"
    );
    let first = run();
    assert_eq!(fs::read_link(&hook).unwrap(), stable);
    assert_eq!(first, f.said("tasks", &f.main, &f.main, "list"));
    let (ran, copied) = (inode(&stable), inode(&copy));
    assert_eq!(run(), first);
    assert_ne!(
        inode(&copy),
        copied,
        "the stand-in made no new copy, as Cargo does on every build"
    );
    assert_eq!(
        inode(&stable),
        ran,
        "a build that relinked nothing ran a new file"
    );
    // A relink, by the launcher's build or by a build of Cargo's own before it, runs the new build.
    fs::write(f.main.join("crates/tasks/src/bin/tasks.rs"), "relinked").unwrap();
    assert!(run().contains(" source relinked "));
    let relinked = inode(&stable);
    assert_ne!(relinked, ran);
    fs::write(f.main.join("crates/tasks/src/bin/tasks.rs"), "again").unwrap();
    let cargo = Command::of(standin_cargo())
        .args(["build", "-p", "symphony-board", "--bin", "tasks"])
        .current_dir(&f.main)
        .env("CARGO_TARGET_DIR", &target)
        .envs(copying)
        .status()
        .unwrap();
    assert!(cargo.success());
    assert!(run().contains(" source again "));
    assert_ne!(inode(&stable), relinked);
    // `--build` names the file a run runs, `--link` points the hooks' link at it, `--where` names
    // Cargo's own copy without building.
    let ran = |args: &[&str]| f.run(&f.main, &f.main, &copying, args).ok();
    assert_eq!(ran(&["--build", "tasks"]), stable.display().to_string());
    assert_eq!(ran(&["--link", "tasks"]), "");
    assert_eq!(
        fs::read_link(f.main.join(".target/bin/tasks")).unwrap(),
        stable
    );
    assert_eq!(ran(&["--where", "tasks"]), copy.display().to_string());
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
    let done = Command::of("git")
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
