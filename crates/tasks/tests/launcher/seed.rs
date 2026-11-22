//! `tools/seed-target.sh`, run by `tools/cargo.sh` before Cargo: a linked worktree's new build
//! folder starts as clones of the main checkout's compiled dependencies, never of the workspace's
//! own units, and nothing is made anywhere else; a seed ended by a signal leaves nothing, and what
//! one killed outright left goes on a later run. A repository of its own under the temp folder,
//! its main checkout's build a few files named as Cargo names them, over a stand-in Cargo whose
//! `metadata` names the workspace's packages.

use super::{Ran, git, scratch};
use crate::children::Child as _;
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime};

/// The environment a fixture's run starts without: no target folder, no seed setting, no git
/// repository of its own and no compiler of the caller's choosing.
const UNSET: [&str; 7] = [
    "CARGO_TARGET_DIR",
    "SYMPHONY_SEED_TARGET",
    "SYMPHONY_SEED_WAIT",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "RUSTC",
];

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
    ".fingerprint/symphony-board-4444444444444444/example-dispatch_load",
    "examples/dispatch_load-4444444444444444",
    "examples/dispatch_load",
    "examples/dispatch_load.d",
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
        assert!(Command::of(&cargo).output().unwrap().status.success());
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

/// The workspace's own folder.
fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// The pid of a process that has exited and been reaped.
fn dead_pid() -> u32 {
    let mut done = Command::of("true").spawn().unwrap();
    done.wait().unwrap();
    done.id()
}

/// Every process's command line.
fn processes() -> Vec<String> {
    let listed = Command::of("ps")
        .args(["-axww", "-o", "command="])
        .output()
        .unwrap();
    assert!(listed.status.success());
    String::from_utf8_lossy(&listed.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

/// A repository whose main checkout (`main/`) holds `tools/cargo.sh`, `tools/seed-target.sh` and
/// `tools/seed-clone.rs` and has built ([`DEPENDENCIES`] and [`WORKSPACE_UNITS`]), and a linked
/// worktree of it (`wt/`) that has not.
struct Seeding {
    base: PathBuf,
    main: PathBuf,
    wt: PathBuf,
}

impl Seeding {
    fn new(name: &str) -> Seeding {
        let base = scratch(name);
        let main = base.join("main");
        let tools = workspace().join("tools");
        fs::create_dir_all(main.join("tools")).unwrap();
        for tool in ["cargo.sh", "seed-target.sh", "seed-clone.rs"] {
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

    /// `checkout/tools/cargo.sh build --locked` run there, with `vars` on an environment without
    /// [`UNSET`].
    fn cargo(&self, checkout: &Path, vars: &[(&str, &str)]) -> Ran {
        let mut command = Command::of(checkout.join("tools/cargo.sh"));
        for name in UNSET {
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

    /// What is beside the worktree's build folder, by name: `.target.…`.
    fn beside(&self) -> BTreeSet<String> {
        fs::read_dir(&self.wt)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .filter(|name| name.starts_with(".target."))
            .collect()
    }

    /// That no clone being made is left beside the worktree's build folder.
    fn no_staging_left(&self) {
        let left = self.beside();
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
    // The lock is held for one call that clones the folder whole, not one a file.
    let seeded = fs::read_to_string(target.join("SEEDED")).unwrap();
    assert!(
        seeded.starts_with(&format!(
            "cloned from {} by one clonefile call at ",
            source.display()
        )),
        "{seeded}"
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
    f.unseeded(f.cargo(&f.wt, &[("SYMPHONY_SEED_TARGET", "off")]));

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
    let refused = format!(
        "symphony seed: no clone of {} (its build held the lock past 1s, or the volume refused); building from nothing\n",
        source.display()
    );
    let start = Instant::now();
    let ran = f.cargo(&f.wt, &[("SYMPHONY_SEED_WAIT", "1")]);
    assert!(
        start.elapsed() >= Duration::from_secs(1),
        "it waits its second"
    );
    assert_eq!(ran.err, refused);
    assert_eq!(ran.ok(), Seeding::built(&target));
    assert!(!target.exists());
    f.no_staging_left();

    lock.unlock().unwrap();
    let ran = f.cargo(&f.wt, &[("SYMPHONY_SEED_WAIT", "1")]);
    assert!(ran.err.contains("starts as a clone of"), "{}", ran.err);
    assert_eq!(ran.ok(), Seeding::built(&target));
    assert!(target.join("SEEDED").is_file());
}

#[cfg(target_os = "macos")]
#[test]
fn without_a_compiler_for_the_helper_the_seed_clones_each_file() {
    let f = Seeding::new("by-cp");
    let (source, target) = (f.main.join(".target/debug"), f.wt.join(".target"));
    let nowhere = f.base.join("no-rustc");
    let ran = f.cargo(&f.wt, &[("RUSTC", nowhere.to_str().unwrap())]);
    assert!(ran.err.contains("starts as a clone of"), "{}", ran.err);
    assert_eq!(ran.ok(), Seeding::built(&target));
    let seeded = fs::read_to_string(target.join("SEEDED")).unwrap();
    assert!(
        seeded.starts_with(&format!(
            "cloned from {} by /bin/cp -c -R -p at ",
            source.display()
        )),
        "{seeded}"
    );
    let mut expected: BTreeSet<String> =
        DEPENDENCIES.iter().map(|f| format!("debug/{f}")).collect();
    expected.insert("SEEDED".to_string());
    assert_eq!(files(&target), expected);
    f.no_staging_left();
}

/// The signals a seed traps, by number and by the name `kill` takes.
#[cfg(target_os = "macos")]
const TRAPPED: [(libc::c_int, &str); 3] = [
    (libc::SIGTERM, "TERM"),
    (libc::SIGINT, "INT"),
    (libc::SIGHUP, "HUP"),
];

#[cfg(target_os = "macos")]
impl Seeding {
    /// The worktree's seed started alone, as `tools/cargo.sh` starts it, with `vars` besides and its
    /// stderr into `said`; it waits for the main checkout's lock as long as the test does. Each
    /// signal it traps starts at its default, as a terminal's job has it, whatever the run of the
    /// tests inherited: a background job inherits SIGINT ignored, and a shell cannot trap a signal
    /// ignored when it started, so a seed started so rightly lets it pass.
    fn seed(&self, said: &Path, vars: &[(&str, &std::ffi::OsStr)]) -> symphony_board::sys::Child {
        let mut command = symphony_board::sys::Command::of(self.wt.join("tools/seed-target.sh"));
        for name in UNSET {
            command.env_remove(name);
        }
        for (signal, _) in TRAPPED {
            command.default_signal(signal);
        }
        command
            .arg(self.wt.join(".target"))
            .current_dir(&self.wt)
            .env("CARGO", standin())
            .env("SYMPHONY_SEED_WAIT", "600")
            .envs(vars.iter().copied())
            .stdin(symphony_board::sys::Stdio::null())
            .stdout(symphony_board::sys::Stdio::null())
            .stderr(File::create(said).unwrap())
            .spawn()
            .unwrap()
    }

    /// Until `what` is so (`ready`), the seed still waiting: within the backstop of the 300 s it
    /// once had, which only the seed's own SYMPHONY_SEED_WAIT, ending it, comes before.
    fn until_ready(
        seed: &mut symphony_board::sys::Child,
        said: &Path,
        what: &str,
        ready: impl Fn() -> bool,
    ) {
        test_time::until_true(what, test_time::backstop(Duration::from_secs(300)), || {
            assert!(
                seed.try_wait().unwrap().is_none(),
                "the seed ended before {what}: {}",
                fs::read_to_string(said).unwrap()
            );
            ready()
        })
        .unwrap();
    }

    /// `seed`'s end within the backstop of the minute it once had, never the lock's wait: a seed
    /// the signal (`how`) did not end waits on with its clone for all of SYMPHONY_SEED_WAIT, so it
    /// is killed and the test says so.
    fn end(
        seed: &mut symphony_board::sys::Child,
        how: &str,
        said: &Path,
    ) -> std::process::ExitStatus {
        test_time::until(
            format!("{how} ended the seed"),
            test_time::backstop(Duration::from_secs(60)),
            || seed.try_wait().unwrap(),
        )
        .unwrap_or_else(|stall| {
            seed.kill().unwrap();
            seed.wait().unwrap();
            panic!("{stall}: {}", fs::read_to_string(said).unwrap());
        })
    }

    /// That the seed ended by `signal` (`how` says where it came) and left nothing: no staging, no
    /// build folder, no process of its clone, and not a word on its stderr.
    fn left_nothing(
        &self,
        ended: std::process::ExitStatus,
        signal: libc::c_int,
        how: &str,
        said: &Path,
    ) {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(ended.signal(), Some(signal), "{how}: the run ends by it");
        self.no_staging_left();
        assert!(!self.wt.join(".target").exists(), "{how}");
        let ours = format!("{}/", self.base.display());
        let left: Vec<String> = processes()
            .into_iter()
            .filter(|command| command.contains(&ours))
            .collect();
        assert!(
            left.is_empty(),
            "{how}: the clone ended with the seed: {left:?}"
        );
        assert_eq!(fs::read_to_string(said).unwrap(), "", "{how}");
    }
}

/// `signal` sent to `seed`, which this test started and has not reaped.
#[cfg(target_os = "macos")]
fn send(seed: &symphony_board::sys::Child, signal: libc::c_int) {
    let pid = libc::pid_t::try_from(seed.id()).unwrap();
    // SAFETY: kill only sends a signal, to the seed this test started and has not reaped.
    assert_eq!(unsafe { libc::kill(pid, signal) }, 0);
}

#[cfg(target_os = "macos")]
#[test]
fn a_seed_ended_by_a_signal_ends_its_clone_and_leaves_nothing() {
    let f = Seeding::new("signalled");
    let lock_file = f.main.join(".target/debug/.cargo-lock");
    // A build holds the main checkout's lock, so the clone waits for it under the lock tool, a job
    // of the seed's.
    let lock = File::open(&lock_file).unwrap();
    lock.lock().unwrap();
    let waiting = || {
        processes().iter().any(|command| {
            command.starts_with("lockf ") && command.contains(lock_file.to_str().unwrap())
        })
    };
    for (signal, name) in TRAPPED {
        let said = f.base.join(format!("said-{name}"));
        let mut seed = f.seed(&said, &[]);
        Seeding::until_ready(&mut seed, &said, "the clone waited on the lock", waiting);
        assert!(
            f.beside().contains(&format!(".target.seed.{}", seed.id())),
            "{:?}",
            f.beside()
        );
        send(&seed, signal);
        let how = format!("SIG{name} while the clone waited");
        let ended = Seeding::end(&mut seed, &how, &said);
        f.left_nothing(ended, signal, &how, &said);
    }
    lock.unlock().unwrap();
}

/// A bash startup file (`BASH_ENV`) under which a seed sends itself `signal` before the `step`-th
/// command its own shell runs once its clone's job leads a group of its own, or before its wait on
/// that job if that comes first, and writes that command to `sent`: a DEBUG trap, which functions
/// inherit under `set -T`, counting in the seed's own shell alone (`BASH_SUBSHELL` 0). Bash 3.2
/// refuses `${!:-}` under `set -u` before any job, so `$!` is read with it off for the moment.
#[cfg(target_os = "macos")]
fn signalled_at(step: usize, signal: &str, sent: &Path) -> String {
    format!(
        r#"set -T
probe_seen=0
probe_sent=""
trap '
probe_flags=$-
set +u
probe_job=$!
case $probe_flags in *u*) set -u ;; esac
if [ -z "$probe_sent" ] && [ "$BASH_SUBSHELL" -eq 0 ] && [ -n "$probe_job" ] &&
    kill -0 -- "-$probe_job" 2>/dev/null; then
    probe_seen=$((probe_seen + 1))
    case "$BASH_COMMAND" in wait\ *) probe_seen={step} ;; esac
    if [ "$probe_seen" -ge {step} ]; then
        probe_sent=1
        printf "%s\n" "$BASH_COMMAND" > "{sent}"
        kill -{signal} $$
    fi
fi' DEBUG
"#,
        sent = sent.display()
    )
}

#[cfg(target_os = "macos")]
#[test]
fn a_signal_at_any_step_of_the_clones_start_ends_the_clone_too() {
    let f = Seeding::new("signalled-starting");
    // A build holds the main checkout's lock, so the clone's job waits on it and only the signal
    // ends the seed.
    let lock = File::open(f.main.join(".target/debug/.cargo-lock")).unwrap();
    lock.lock().unwrap();
    // A trap runs between two of the seed's commands: the first step comes between the job's fork
    // and the seed's record of it, and from the fifth on the signal comes before its wait.
    for step in 1..=5 {
        for (signal, name) in TRAPPED {
            let said = f.base.join(format!("said-{step}-{name}"));
            let sent = f.base.join(format!("sent-{step}-{name}"));
            let startup = f.base.join(format!("startup-{step}-{name}"));
            fs::write(&startup, signalled_at(step, name, &sent)).unwrap();
            let mut seed = f.seed(&said, &[("BASH_ENV", startup.as_os_str())]);
            let ended = Seeding::end(&mut seed, &format!("SIG{name} at step {step}"), &said);
            let before = fs::read_to_string(&sent).unwrap_or_else(|_| {
                panic!(
                    "the seed ended ({ended}) with no SIG{name} sent at step {step}: {}",
                    fs::read_to_string(&said).unwrap()
                )
            });
            let how = format!("SIG{name} at step {step}, before `{}`", before.trim());
            f.left_nothing(ended, signal, &how, &said);
        }
    }
    lock.unlock().unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn a_seed_ended_by_a_signal_ends_a_lock_tool_slow_to_end_before_it_ends() {
    let f = Seeding::new("signalled-slow");
    // A lock tool that takes seconds to end once its group is sent TERM, as a copy does in its one
    // clonefile call: the seed ends only once it has, and its staging goes after. It marks itself
    // ready once its trap is set.
    let tools = f.base.join("slow-lockf");
    fs::create_dir(&tools).unwrap();
    let slow = tools.join("lockf");
    fs::write(
        &slow,
        "#!/bin/bash\ntrap '/bin/sleep 5; exit 143' TERM\n: > \"$0.ready\"\n/bin/sleep 600 &\nwait\n",
    )
    .unwrap();
    fs::set_permissions(&slow, fs::Permissions::from_mode(0o755)).unwrap();
    let ready = tools.join("lockf.ready");
    let path = first_on_path(&tools);
    for (signal, name) in TRAPPED {
        let said = f.base.join(format!("said-slow-{name}"));
        let _ = fs::remove_file(&ready);
        let mut seed = f.seed(&said, &[("PATH", path.as_os_str())]);
        Seeding::until_ready(&mut seed, &said, "the lock tool was ready", || {
            ready.exists()
        });
        send(&seed, signal);
        let how = format!("SIG{name} while the lock tool ran");
        let ended = Seeding::end(&mut seed, &how, &said);
        f.left_nothing(ended, signal, &how, &said);
    }
}

/// A member of the job that its group's KILL misses (a lock tool that left the group, here) keeps
/// the job's FIFO open past the seed's reads of it, so they time out: the seed reads again and ends
/// only once that member is gone, a timed-out read never taken for its end. A BASH_ENV DEBUG trap
/// writes each command of the seed's own shell to `ran`, so the member is ended only after a second
/// read, which comes once the first timed out.
#[cfg(target_os = "macos")]
#[test]
fn a_seed_ends_only_once_its_jobs_last_member_is_gone_however_often_its_read_times_out() {
    let f = Seeding::new("signalled-left");
    let tools = f.base.join("left-lockf");
    fs::create_dir(&tools).unwrap();
    let left = tools.join("lockf");
    // It leaves the job's group, keeps the FIFO the job handed it, says its pid and waits for the
    // test's signal, or ends by itself after the 600 s the seed's lock wait is.
    fs::write(
        &left,
        r#"#!/bin/sh
exec /usr/bin/perl -MPOSIX -e '
setpgid(0, 0) or die "setpgid: $!";
open(my $out, ">", "$ARGV[0].pid.part") or die "$!";
print $out $$;
close($out);
rename("$ARGV[0].pid.part", "$ARGV[0].pid") or die "$!";
alarm(600);
pause();
' "$0"
"#,
    )
    .unwrap();
    fs::set_permissions(&left, fs::Permissions::from_mode(0o755)).unwrap();
    let pid = tools.join("lockf.pid");
    let path = first_on_path(&tools);
    for (signal, name) in TRAPPED {
        let said = f.base.join(format!("said-left-{name}"));
        let ran = f.base.join(format!("ran-left-{name}"));
        let startup = f.base.join(format!("startup-left-{name}"));
        fs::write(&startup, commands_to(&ran)).unwrap();
        let _ = fs::remove_file(&pid);
        let mut seed = f.seed(
            &said,
            &[
                ("PATH", path.as_os_str()),
                ("BASH_ENV", startup.as_os_str()),
            ],
        );
        Seeding::until_ready(&mut seed, &said, "the lock tool left its group", || {
            pid.exists()
        });
        let member: libc::pid_t = fs::read_to_string(&pid).unwrap().parse().unwrap();
        send(&seed, signal);
        let reads = || {
            fs::read_to_string(&ran)
                .unwrap_or_default()
                .lines()
                .filter(|command| command.starts_with("read -r -t "))
                .count()
        };
        Seeding::until_ready(
            &mut seed,
            &said,
            "the seed read for its job's end again",
            || reads() >= 2,
        );
        // SAFETY: kill only sends SIGKILL to the lock tool this test's seed started; the seed,
        // still waiting for it, says it has not ended, so its pid is its own.
        assert_eq!(unsafe { libc::kill(member, libc::SIGKILL) }, 0);
        let how = format!("SIG{name} while a lock tool outside its group ran");
        let ended = Seeding::end(&mut seed, &how, &said);
        f.left_nothing(ended, signal, &how, &said);
    }
}

/// A bash startup file (`BASH_ENV`) under which a seed writes each command its own shell runs
/// (`BASH_SUBSHELL` 0) to `ran`, one a line, before it runs it: a DEBUG trap, which functions
/// inherit under `set -T`.
#[cfg(target_os = "macos")]
fn commands_to(ran: &Path) -> String {
    format!(
        r#"set -T
trap '[ "$BASH_SUBSHELL" -eq 0 ] && printf "%s\n" "$BASH_COMMAND" >> "{ran}"' DEBUG
"#,
        ran = ran.display()
    )
}

/// The search path with `tools` first, so a stand-in there is the program a seed runs.
#[cfg(target_os = "macos")]
fn first_on_path(tools: &Path) -> std::ffi::OsString {
    std::env::join_paths(
        std::iter::once(tools.to_path_buf())
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap()
}

#[test]
fn what_a_seed_or_a_clear_left_goes_on_a_later_run_once_its_process_is_gone() {
    let f = Seeding::new("stale");
    let (dead, live) = (dead_pid(), std::process::id());
    let left = |name: String| {
        let deps = f.wt.join(&name).join("debug/deps");
        fs::create_dir_all(&deps).unwrap();
        fs::write(deps.join("libwork.rlib"), "compiled\n").unwrap();
        name
    };
    left(format!(".target.seed.{dead}"));
    left(format!(".target.clear.{dead}"));
    // A seed or a clear still running, a name with no process in it, and a link named as one.
    let mut kept: BTreeSet<String> = [
        left(format!(".target.seed.{live}")),
        left(format!(".target.clear.{live}")),
        left(".target.seed.12ab".to_string()),
        left(".target.backup".to_string()),
    ]
    .into();
    let outside = f.base.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep"), "keep\n").unwrap();
    let link = format!(".target.seed.{}", dead_pid());
    std::os::unix::fs::symlink(&outside, f.wt.join(&link)).unwrap();
    kept.insert(link);

    // The worktree's build folder is made already, so nothing is seeded, and the run still clears.
    let target = f.wt.join(".target");
    fs::create_dir(&target).unwrap();
    Seeding::quietly(f.cargo(&f.wt, &[]), &target);
    assert_eq!(f.beside(), kept);
    assert_eq!(fs::read_to_string(outside.join("keep")).unwrap(), "keep\n");
    assert!(files(&target).is_empty());
}

#[test]
fn git_ignores_a_clone_being_staged_and_a_build_folder_being_removed() {
    let names = [
        ".target.seed.123/",
        ".target.seed.123/debug/deps/libwork.rlib",
        ".target.clear.456/",
    ];
    let checked = Command::of("git")
        .args(["check-ignore", "--no-index"])
        .args(names)
        .current_dir(workspace())
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(checked.stdout)
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        names
    );
}
