//! `tools/seed-target.sh`, run by `tools/cargo.sh` before Cargo: a linked worktree's new build
//! folder starts as clones of the main checkout's compiled dependencies, never of the workspace's
//! own units, and nothing is made anywhere else. A repository of its own under the temp folder,
//! its main checkout's build a few files named as Cargo names them, over a stand-in Cargo whose
//! `metadata` names the workspace's packages.

use super::{Ran, git, scratch};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime};

/// The stand-in Cargo: `metadata` names the workspace's two packages as Cargo spells them, one by a
/// name of its own and one by its folder's; anything else says what it ran, and into which folder.
const CARGO: &str = r#"#!/bin/sh
if [ "$1" = metadata ]; then
    echo '{"packages":[],"workspace_members":["path+file:///w/crates/tasks#symphony-board@0.1.0","path+file:///w/crates/symphony#0.1.0"],"resolve":null,"version":1}'
    exit 0
fi
echo "cargo $* into $CARGO_TARGET_DIR"
"#;

/// The main checkout's dependencies, under `.target/debug`: what a seeded worktree starts with. Two
/// are named like a workspace package and a word more, which no unit of the package is.
const DEPENDENCIES: [&str; 12] = [
    ".cargo-lock",
    ".fingerprint/serde-0123456789abcdef/lib-serde",
    ".fingerprint/serde-0123456789abcdef/lib-serde.json",
    "deps/libserde-0123456789abcdef.rlib",
    "deps/libserde-0123456789abcdef.rmeta",
    "deps/serde-0123456789abcdef.d",
    ".fingerprint/serde-fedcba9876543210/run-build-script-build-script-build",
    "build/serde-fedcba9876543210/out/private.rs",
    ".fingerprint/symphony-board-extra-0a0a0a0a0a0a0a0a/lib-symphony_board_extra",
    "deps/libsymphony_board_extra-0a0a0a0a0a0a0a0a.rlib",
    ".fingerprint/symphony-theme-0b0b0b0b0b0b0b0b/lib-symphony_theme",
    "deps/libsymphony_theme-0b0b0b0b0b0b0b0b.rlib",
];

/// The workspace's own units in the main checkout's build: a library, a binary and its copy under
/// its own name, a build script's output, an example and its copy, and the other package's library.
const WORKSPACE_UNITS: [&str; 19] = [
    ".fingerprint/symphony-board-1111111111111111/lib-symphony_board",
    ".fingerprint/symphony-board-1111111111111111/lib-symphony_board.json",
    "deps/libsymphony_board-1111111111111111.rlib",
    "deps/libsymphony_board-1111111111111111.rmeta",
    "deps/symphony_board-1111111111111111.d",
    ".fingerprint/symphony-board-2222222222222222/bin-tasks",
    ".fingerprint/symphony-board-2222222222222222/bin-tasks.json",
    "deps/tasks-2222222222222222",
    "deps/tasks-2222222222222222.d",
    "tasks",
    "tasks.d",
    ".fingerprint/symphony-board-3333333333333333/run-build-script-build-script-build",
    "build/symphony-board-3333333333333333/out/generated.rs",
    ".fingerprint/symphony-board-4444444444444444/example-search_bench",
    "examples/search_bench-4444444444444444",
    "examples/search_bench",
    "examples/search_bench.d",
    ".fingerprint/symphony-5555555555555555/lib-symphony",
    "deps/libsymphony-5555555555555555.rlib",
];

/// When the main checkout built, to the second: a clone keeps it.
fn built_at() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)
}

/// The shared stand-in Cargo, run once so no case pays its first run.
fn standin() -> &'static Path {
    static CARGO_PATH: OnceLock<PathBuf> = OnceLock::new();
    CARGO_PATH.get_or_init(|| {
        let cargo = scratch("seed-standin").join("cargo");
        fs::write(&cargo, CARGO).unwrap();
        fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(Command::new(&cargo).output().unwrap().status.success());
        cargo
    })
}

/// Every file under `dir`, by its path from there; none when it does not exist.
fn files(dir: &Path) -> BTreeSet<String> {
    fn walk(top: &Path, dir: &Path, found: &mut BTreeSet<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if fs::symlink_metadata(&path).unwrap().is_dir() {
                walk(top, &path, found);
            } else {
                let relative = path.strip_prefix(top).unwrap();
                found.insert(relative.to_str().unwrap().to_string());
            }
        }
    }
    let mut found = BTreeSet::new();
    walk(dir, dir, &mut found);
    found
}

/// A repository whose main checkout (`main/`) holds `tools/cargo.sh` and `tools/seed-target.sh` and
/// has built ([`DEPENDENCIES`] and [`WORKSPACE_UNITS`]), and a linked worktree of it (`wt/`) that
/// has not.
struct Seeding {
    base: PathBuf,
    main: PathBuf,
    wt: PathBuf,
}

impl Seeding {
    fn new(name: &str) -> Seeding {
        let base = scratch(name);
        let main = base.join("main");
        let tools = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools")
            .canonicalize()
            .unwrap();
        fs::create_dir_all(main.join("tools")).unwrap();
        for tool in ["cargo.sh", "seed-target.sh"] {
            std::os::unix::fs::symlink(tools.join(tool), main.join("tools").join(tool)).unwrap();
        }
        fs::write(main.join("Cargo.toml"), "[workspace]\n").unwrap();
        git(&base, &["init", "-q", "-b", "main", "main"]);
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-qm", "main"]);
        git(&main, &["worktree", "add", "-q", "-b", "feature", "../wt"]);
        let debug = main.join(".target/debug");
        for file in DEPENDENCIES.iter().chain(&WORKSPACE_UNITS) {
            let path = debug.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, format!("compiled {file}\n")).unwrap();
            File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(built_at())
                .unwrap();
        }
        let wt = base.join("wt");
        Seeding { base, main, wt }
    }

    /// `checkout/tools/cargo.sh build --locked` run there, with `vars` on an environment that names
    /// no target folder, no seed setting and no git repository of its own.
    fn cargo(&self, checkout: &Path, vars: &[(&str, &str)]) -> Ran {
        let mut command = Command::new(checkout.join("tools/cargo.sh"));
        for name in [
            "CARGO_TARGET_DIR",
            "SF_SEED_TARGET",
            "SF_SEED_WAIT",
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_COMMON_DIR",
        ] {
            command.env_remove(name);
        }
        let done = command
            .args(["build", "--locked"])
            .current_dir(checkout)
            .env("CARGO", standin())
            .envs(vars.iter().copied())
            .output()
            .unwrap();
        Ran {
            code: done.status.code().unwrap_or(-1),
            out: String::from_utf8(done.stdout).unwrap().trim().to_string(),
            err: String::from_utf8(done.stderr).unwrap(),
        }
    }

    /// What the stand-in Cargo says it ran, building into `target`.
    fn built(target: &Path) -> String {
        format!("cargo build --locked into {}", target.display())
    }

    /// That the build ran into `target`, saying nothing of a seed.
    fn quietly(ran: Ran, target: &Path) {
        assert_eq!(ran.err, "");
        assert_eq!(ran.ok(), Seeding::built(target));
    }

    /// That the worktree's build ran into its own folder, seeding nothing, and left it as it was.
    fn unseeded(&self, ran: Ran) {
        let target = self.wt.join(".target");
        Seeding::quietly(ran, &target);
        assert!(!target.exists() && fs::symlink_metadata(&target).is_err());
        self.no_staging_left();
    }

    /// That no clone being made is left beside the worktree's build folder.
    fn no_staging_left(&self) {
        let left: Vec<String> = fs::read_dir(&self.wt)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .filter(|name| name.starts_with(".target.seed."))
            .collect();
        assert!(left.is_empty(), "{left:?}");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn a_worktrees_first_build_starts_from_clones_of_the_main_checkouts_dependencies() {
    let f = Seeding::new("seeds");
    let (source, target) = (f.main.join(".target/debug"), f.wt.join(".target"));
    let ran = f.cargo(&f.wt, &[]);
    assert_eq!(
        ran.err,
        format!(
            "symphony seed: {}/debug starts as a clone of {} (no disk until a build changes it)\n",
            target.display(),
            source.display()
        )
    );
    assert_eq!(ran.ok(), Seeding::built(&target));

    let mut seeded: BTreeSet<String> = DEPENDENCIES.iter().map(|f| format!("debug/{f}")).collect();
    seeded.insert("SEEDED".to_string());
    assert_eq!(files(&target), seeded);
    for file in DEPENDENCIES {
        let clone = target.join("debug").join(file);
        assert_eq!(
            fs::read_to_string(&clone).unwrap(),
            format!("compiled {file}\n")
        );
        assert_eq!(
            fs::metadata(&clone).unwrap().modified().unwrap(),
            built_at(),
            "{file} keeps the time it was built"
        );
    }
    assert!(
        fs::read_to_string(target.join("SEEDED"))
            .unwrap()
            .starts_with(&format!("cloned from {} at ", source.display()))
    );
    let everything: BTreeSet<String> = DEPENDENCIES
        .iter()
        .chain(&WORKSPACE_UNITS)
        .map(|f| f.to_string())
        .collect();
    assert_eq!(
        files(&source),
        everything,
        "the main checkout's build stays"
    );
    f.no_staging_left();

    // From then on the folder is the worktree's: a build changes it, and no run seeds it again.
    fs::remove_file(target.join("debug/deps/libserde-0123456789abcdef.rlib")).unwrap();
    fs::write(target.join("debug/deps/libwork-9999999999999999.rlib"), "").unwrap();
    let after_build = files(&target);
    Seeding::quietly(f.cargo(&f.wt, &[]), &target);
    assert_eq!(files(&target), after_build);
}

#[test]
fn nothing_is_seeded_but_a_linked_worktrees_new_build_folder() {
    let f = Seeding::new("unseeded");

    // Turned off.
    f.unseeded(f.cargo(&f.wt, &[("SF_SEED_TARGET", "off")]));

    // The main checkout's own build, into a folder not made yet.
    let elsewhere = f.base.join("elsewhere");
    let ran = f.cargo(
        &f.main,
        &[("CARGO_TARGET_DIR", elsewhere.to_str().unwrap())],
    );
    Seeding::quietly(ran, &elsewhere);
    assert!(!elsewhere.exists());

    // A folder whose parent does not exist, and one that is a link to nowhere.
    let unparented = f.wt.join("missing/target");
    let ran = f.cargo(&f.wt, &[("CARGO_TARGET_DIR", unparented.to_str().unwrap())]);
    Seeding::quietly(ran, &unparented);
    assert!(!f.wt.join("missing").exists());
    let link = f.wt.join(".target");
    std::os::unix::fs::symlink(f.base.join("nowhere"), &link).unwrap();
    Seeding::quietly(f.cargo(&f.wt, &[]), &link);
    assert!(fs::read_link(&link).is_ok() && !f.base.join("nowhere").exists());
    fs::remove_file(&link).unwrap();

    // A Cargo that cannot name the workspace's packages.
    let mute = f.base.join("mute-cargo");
    fs::write(
        &mute,
        "#!/bin/sh\n[ \"$1\" = metadata ] && exit 101\necho \"cargo $* into $CARGO_TARGET_DIR\"\n",
    )
    .unwrap();
    fs::set_permissions(&mute, fs::Permissions::from_mode(0o755)).unwrap();
    f.unseeded(f.cargo(&f.wt, &[("CARGO", mute.to_str().unwrap())]));

    // A main checkout whose build is not Cargo's: no lock file in it.
    fs::remove_file(f.main.join(".target/debug/.cargo-lock")).unwrap();
    f.unseeded(f.cargo(&f.wt, &[]));
}

#[cfg(target_os = "macos")]
#[test]
fn a_build_holding_the_main_checkouts_lock_past_the_wait_leaves_the_worktree_to_build_from_nothing()
{
    let f = Seeding::new("locked");
    let (source, target) = (f.main.join(".target/debug"), f.wt.join(".target"));
    // Cargo's build holds its folder's lock, alone, for as long as it builds.
    let lock = File::open(source.join(".cargo-lock")).unwrap();
    lock.lock().unwrap();
    let start = Instant::now();
    let ran = f.cargo(&f.wt, &[("SF_SEED_WAIT", "1")]);
    assert!(
        start.elapsed() >= Duration::from_secs(1),
        "it waits its second"
    );
    assert_eq!(
        ran.err,
        format!(
            "symphony seed: no clone of {} (its build held the lock past 1s, or the volume refused); building from nothing\n",
            source.display()
        )
    );
    assert_eq!(ran.ok(), Seeding::built(&target));
    assert!(!target.exists());
    f.no_staging_left();

    lock.unlock().unwrap();
    let ran = f.cargo(&f.wt, &[("SF_SEED_WAIT", "1")]);
    assert!(ran.err.contains("starts as a clone of"), "{}", ran.err);
    assert_eq!(ran.ok(), Seeding::built(&target));
    assert!(target.join("SEEDED").is_file());
}
