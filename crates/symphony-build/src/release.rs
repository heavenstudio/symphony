//! `symphony-build release` (`just release`): Symphony's releases (docs/RELEASES.md), a command of
//! this repository's own build binary, which no project is given. A run takes main's tip as
//! fetched from the remote, the commit a project can fetch, into a clean checkout of its own
//! outside the repository's folder ([`place`]) and runs the full verification there: a release is
//! judged by checks this repository owns, never by a consuming project's suite. Every red check,
//! and a run that stopped short of deciding, is a task on the board ([`board`]); when every check
//! is green it tags the commit as the next release (`release-YYYY-MM-DD`, [`tags`]) with what it
//! holds ([`notes`]) and pushes that one tag, then installs it on the machine ([`install`]),
//! moves each registered project's board to its schema under the writer rule and points `current`
//! at it ([`refresh`]), writing no project's checkout. `--commit COMMIT` does the same out of
//! cycle for a commit already on the remote's main and past the newest release. A run of main's
//! tip red in a check holds that commit as the release candidate ([`candidate`]) while its reds
//! are fixed: until each is answered
//! no run checks main's tip, and then the candidate is rerun, with what its fixes changed (each
//! step and lane whose inputs, as the tree tells them, are not the ones it was green on:
//! [`inputs`]), and released or held again. Each run is recorded in the git common dir ([`record`]), which
//! `--status` reads; `--schedule` prints the entry that runs it each day ([`schedule`]), which the
//! owner installs.
//!
//! It never tags a commit that is not on the remote's main, that is behind the newest release or
//! that a check found red, never moves or deletes a tag, and pushes nothing but its new tag. After
//! each check it looks at its checkout, and before it tags at what git names there once more:
//! another HEAD, a path `git status` names outside `.target/`, or a process `lsof` sees working
//! there or holding a file there open stops the run, and nothing is tagged. A look sees a moment,
//! and only what git does not ignore: a file git ignores (`build/`, `artifacts/`, `.target/`,
//! which checks read) or a change undone between two looks goes unseen. What keeps those off is
//! that the checkout is the run's own, made and locked by it, that each check's process group is
//! ended with it, and the same `lsof` look before anything is checked; that one sees a process only
//! while it works in the checkout or holds a file there open. One run at a time per repository,
//! none while a check of an earlier run killed while it ran still runs ([`checks`]), and a run that
//! ended without recording itself is recorded by the next ([`record::recover`]); everything it runs
//! is bounded by the run's time and stops when the run is told to (HUP, INT or TERM, [`group`]).

mod board;
pub(crate) mod cadence;
mod candidate;
mod checks;
pub(crate) mod consumer;
pub(crate) mod crates;
mod failures;
mod git;
mod gone;
mod group;
mod inputs;
mod install;
mod nextest;
mod notes;
mod place;
mod record;
mod refresh;
mod rerun;
mod schedule;
mod span;
mod steps;
mod tags;
#[cfg(test)]
mod waits;

use std::fs::{self, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use checks::{Ran, Step, Within};
use git::{commit, git, git_until, is_ancestor, short};
use group::Bound;
use place::{Place, normal};
use record::{Checking, Run, Running, local};
use symphony_board::sys::{Interrupts, pid_alive};
use tags::Release;

/// A test stall's observed load, using the release run's own load reader and wording.
pub(crate) fn test_load() -> String {
    checks::Load::now().map_or_else(|| "unavailable".into(), |load| load.said())
}

use symphony::cli::{Flag, parse};
use symphony::project::Project;

/// A refusal of the command line, as `symphony::cli::refuse` words one but under the binary that
/// runs it: the usage, then the error. Exit 2.
fn refuse(err: &mut dyn Write, usage: &str, command: &str, problem: &str) -> i32 {
    let _ = writeln!(err, "{usage}\nsymphony-build {command}: error: {problem}");
    2
}

const USAGE: &str = "usage: symphony-build release [-h] [--root ROOT] [--remote NAME] \
[--branch NAME] [--worktree PATH] [--check NAME=COMMAND] [--commit COMMIT] [--no-push] \
[--timeout SECONDS] [--status] [--schedule] [--at HH:MM] [--due [SLUG COMMIT]] [--refresh] \
[--rerun] [--drop]";
const HELP: &str =
    "Symphony's releases: main's tip as fetched from the remote, verified in a clean \
checkout outside the repository's folder, building and testing nothing of the projects this \
machine serves; each red check filed on the board as tests/release-red-NAME, and a green commit \
tagged release-YYYY-MM-DD and pushed, then the machine refreshed to it: the release built and \
installed in the machine folder; while no registered project names a linked plugin, each \
project's board moved to the release's schema, snapshotted first, once every writer seen there in \
the last week is a release's own or at that schema, and then current pointed at it, which the \
machine's server follows; no project's checkout, branch or pin is written. \
A run red in a check holds its commit as the release candidate, the local branch \
release-candidate, until its reds' own lands answer them; then it is rerun, and main's tip is run \
only once none is held (docs/RELEASES.md).

options:
  -h, --help            show this help message and exit
  --root ROOT           a checkout of the repository to release (default: the current folder)
  --remote NAME         the remote whose main is released and that takes the tag (default: origin)
  --branch NAME         its main line (default: the project's default branch)
  --worktree PATH       the run's own checkout, outside the repository's folder (default: release
                        in the project's agent worktrees folder)
  --check NAME=COMMAND  a check, run in that checkout with sh -c; repeat for each, in order
                        (default: verify=just verify). One that lists its steps in the folder
                        SYMPHONY_CHECK_STEPS names is told by them, a red each
  --commit COMMIT       out of cycle: the same checks on COMMIT, which must be on the remote's main
                        and past the newest release
  --no-push             tag the release here, push nothing
  --timeout SECONDS     the run's time: a check still running then is stopped, and it and every
                        check not yet started are red (default: 36000, ten hours; at most
                        86400)
  --status              print the newest release, the last run and its age, the release
                        candidate, the landings since the last release and what the next run
                        waits for, the last publication on crates.io, the last refresh, and the
                        release current names, the machine's server and what holds the switch;
                        run nothing
  --schedule            print the schedule entry that runs `just release` each day; install nothing
  --at HH:MM            the time of day --schedule names (default: 03:00)
  --due [SLUG COMMIT]   start a run only when [releases] every_landings tasks have landed since
                        the last release or the last run that checked something, and no run is
                        in progress; else say what it waits for and run nothing, counting with
                        git and building nothing. A due run goes on from a build of the tip made
                        in the run's own checkout. A landing's on_land command runs it, naming
                        the task and its merge
  --refresh             refresh the machine to the newest release here again, checking nothing,
                        after publishing on crates.io what it left while [releases] publish is
                        on: an orchestrator's retry of a red or interrupted refresh or a failed
                        upload, or of one a writer behind the release held, once it is rebuilt
  --rerun               rerun the release candidate now, once every red of it is answered: each
                        check or step that was red, did not run or whose inputs changed; refused
                        while none is held or a red waits
  --drop                drop the release candidate, so the next run checks main's tip: an
                        orchestrator's act";

/// The checks a run runs unless told others: the full verification.
const VERIFY: (&str, &str) = ("verify", "just verify");
/// The run's time unless `--timeout` says otherwise.
const RUN_SECONDS: u64 = 10 * 3600;
/// The longest time `--timeout` may give a run: a check of a run killed while it ran holds later
/// runs off, unreported, for that run's time.
const MOST_SECONDS: u64 = 24 * 3600;
/// How long past its time a run may still hold the lock (stopping its checks, telling the board)
/// before a later run reports it stuck.
const LATE_SECONDS: f64 = 15.0 * 60.0;
/// What a run whose checks all ended green within its time keeps past it to tag and push: well
/// within [`LATE_SECONDS`], so no later run takes it for stuck.
const FINISH: Duration = Duration::from_secs(5 * 60);
/// Why a check is red without having run.
const NOT_STARTED: &str = "the run's time was up before it started";
/// The time a release's install has, past its run's checks: the release build of its four binaries
/// at the machine's load, and the questions put to them.
const INSTALL: Duration = Duration::from_secs(3600);
/// The time the rest of a release's refresh has, past its install: each board's snapshot and move,
/// and the wait for the machine's server to follow `current`.
const REFRESH: Duration = Duration::from_secs(3600);
/// The time a release's publication on crates.io has, past its push: cargo's build of each package
/// it uploads, the upload and cargo's wait for crates.io's index to list it.
const PUBLISH: Duration = Duration::from_secs(3600);
/// The tests' clock: the seconds since the epoch a run takes as now, in place of this machine's.
/// No flag sets it, and a run on it pushes only to a remote that is a folder on this machine
/// ([`far_push`]), so a release named for another day than the machine's never leaves it.
const CLOCK: &str = "SYMPHONY_RELEASE_NOW";
/// How long a run that finds the lock held and no run saying so waits for the run that has just
/// taken it to say so.
const SAYS_HELD: Duration = Duration::from_secs(2);
/// How long past the run's time a look at its checkout may take: a check that ran to the end of
/// the run's time is still looked after.
const LOOK: Duration = Duration::from_secs(60);
/// How often a run waiting for the regenerator looks at it again ([`regenerated`]).
const REGENERATING: Duration = Duration::from_secs(2);
/// How long a run gives the regenerator a land has just started to take the candidate, while
/// nothing makes main's generated files and nothing was written of it ([`regenerated`]); and how
/// long a move of the main checkout's branch has to reach the remote.
const REGENERATOR: Duration = Duration::from_secs(60);
/// The tests' wait in place of [`REGENERATOR`], in seconds; no flag sets it.
const REGENERATOR_SECONDS: &str = "SYMPHONY_RELEASE_REGENERATOR_SECONDS";

/// What a run is asked to do.
struct Options {
    remote: String,
    branch: String,
    worktree: PathBuf,
    /// Whether `--worktree` named it: a checkout named by hand that is not the release run's is a
    /// refusal, the default one an error the board hears of.
    named: bool,
    checks: Vec<Step>,
    out_of_cycle: Option<String>,
    push: bool,
    now: f64,
    timeout: Duration,
    /// What started it, when the cadence did: the landings it counted.
    cause: Option<String>,
    /// Whether it refreshes the machine to the newest release alone, checking nothing.
    refresh: bool,
    /// Whether it was asked to rerun the release candidate (`--rerun`).
    rerun: bool,
}

/// The repository a run releases.
struct Repo {
    /// The checkout the command runs from.
    root: PathBuf,
    /// Its git common dir.
    common: PathBuf,
    /// Its main checkout.
    main: PathBuf,
    project: Arc<Project>,
}

/// Now, by the tests' clock when [`CLOCK`] is set, else this machine's.
fn clock() -> Result<f64, String> {
    match std::env::var(CLOCK) {
        Ok(now) => match now.parse::<f64>() {
            Ok(now) if now.is_finite() && now >= 0.0 => Ok(now),
            _ => Err(format!("{CLOCK}={now:?} is not a time")),
        },
        Err(_) => Ok(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64())),
    }
}

/// `symphony-build release ARGS`: its exit code. 0 is green (or nothing new to release), 1 a red or
/// a run that could not finish, 2 a command line refused.
pub(crate) fn main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let flag = |name, value| Flag { name, value };
    let flags = [
        flag("--root", true),
        flag("--remote", true),
        flag("--branch", true),
        flag("--worktree", true),
        flag("--check", true),
        flag("--commit", true),
        flag("--no-push", false),
        flag("--timeout", true),
        flag("--status", false),
        flag("--schedule", false),
        flag("--at", true),
        flag("--due", false),
        flag("--refresh", false),
        flag("--rerun", false),
        flag("--drop", false),
    ];
    if names_legacy_commit(args, &flags) {
        let _ = writeln!(err, "`{LEGACY_COMMIT} COMMIT` is now `--commit COMMIT`");
        return 2;
    }
    let parsed = match parse(args, &flags) {
        Ok(parsed) if parsed.help => {
            let _ = writeln!(out, "{USAGE}\n\n{HELP}");
            return 0;
        }
        // The landing a `--due` comes after, as a landing's command names it.
        Ok(parsed) if parsed.flag("--due") && parsed.positionals.len() == 2 => parsed,
        Ok(parsed) if !parsed.positionals.is_empty() => {
            let extra = parsed.positionals.join(" ");
            return refuse(
                err,
                USAGE,
                "release",
                &format!("unrecognized arguments: {extra}"),
            );
        }
        Ok(parsed) => parsed,
        Err(problem) => return refuse(err, USAGE, "release", &problem),
    };
    let now = match clock() {
        Ok(now) => now,
        Err(problem) => return refuse(err, USAGE, "release", &problem),
    };
    let timeout = match parsed.value("--timeout") {
        None => Duration::from_secs(RUN_SECONDS),
        Some(seconds) => match seconds.parse::<u64>() {
            Ok(seconds) if seconds > MOST_SECONDS => {
                return refuse(
                    err,
                    USAGE,
                    "release",
                    &format!(
                        "--timeout {seconds} is longer than a run may take: at most \
                         {MOST_SECONDS} seconds (24 hours)"
                    ),
                );
            }
            Ok(seconds) if seconds > 0 => Duration::from_secs(seconds),
            _ => {
                return refuse(
                    err,
                    USAGE,
                    "release",
                    &format!("--timeout {seconds:?} is not a number of seconds"),
                );
            }
        },
    };
    let modes = [
        "--status",
        "--schedule",
        "--commit",
        "--due",
        "--refresh",
        "--rerun",
        "--drop",
    ]
    .into_iter()
    .filter(|mode| parsed.flag(mode))
    .count();
    if modes > 1 {
        return refuse(
            err,
            USAGE,
            "release",
            "--status, --schedule, --commit, --due, --refresh, --rerun and --drop are one each",
        );
    }
    let repo = match repo(Path::new(parsed.value("--root").unwrap_or("."))) {
        Ok(repo) => repo,
        Err(problem) => {
            let _ = writeln!(err, "symphony-build release: {problem}");
            return 1;
        }
    };
    let remote = parsed.value("--remote").unwrap_or("origin").to_string();
    let branch = parsed
        .value("--branch")
        .map_or_else(|| repo.project.default_branch.clone(), str::to_string);
    if parsed.flag("--status") {
        return status(&repo, &remote, &branch, now, out, err);
    }
    if parsed.flag("--schedule") {
        let at = parsed.value("--at").unwrap_or("03:00");
        let Some(time) = schedule::time(at) else {
            return refuse(err, USAGE, "release", &format!("--at {at:?} is not HH:MM"));
        };
        let _ = write!(
            out,
            "{}",
            schedule::entry(
                &repo.main,
                &schedule::label(&repo.project.id),
                time,
                &std::env::var("PATH").unwrap_or_default(),
                &record::folder(&repo.common).join("scheduled.log"),
                cfg!(target_os = "macos"),
            )
        );
        return 0;
    }
    let checks = match checks_of(parsed.values("--check")) {
        Ok(checks) => checks,
        Err(problem) => return refuse(err, USAGE, "release", &problem),
    };
    let worktree = match parsed.value("--worktree") {
        Some(path) => normal(&std::env::current_dir().unwrap_or_default().join(path)),
        None => default_worktree(&repo),
    };
    if parsed.flag("--drop") {
        return drop_candidate(&repo, now, out, err);
    }
    let mut cause = None;
    if parsed.flag("--due") {
        if let [slug, landed] = &parsed.positionals[..] {
            let _ = writeln!(out, "release: {slug} landed ({})", short(landed));
        }
        // The release candidate settled first, so a land that answers its red is heard at once.
        let held = match settled(&repo, &remote, &branch, now, out) {
            Ok(held) => held.as_ref().and_then(candidate::Candidate::hold),
            Err(problem) if repo.project.releases.every_landings > 0 => {
                // A record this build cannot read may be a newer build's: the run goes on from a
                // build of the tip ([`hand_off`]), which reads it, or records why it cannot.
                let _ = writeln!(out, "release: the release candidate: {problem}");
                cause = Some(format!("the release candidate's record: {problem}"));
                None
            }
            Err(problem) => {
                let _ = writeln!(out, "release: the release candidate: {problem}");
                None
            }
        };
        if cause.is_none() {
            let cadence = match cadence_of(&repo, &remote, &branch, held) {
                Ok(cadence) => cadence,
                Err(problem) => {
                    let _ = writeln!(
                        err,
                        "symphony-build release: the landings cannot be counted: {problem}"
                    );
                    return 1;
                }
            };
            let _ = write!(out, "{}", cadence.lines());
            if !cadence.due() {
                return 0;
            }
            cause = Some(cadence.cause());
            let _ = write!(out, "{}", cadence.starts());
        }
    }
    let options = Options {
        remote,
        branch,
        worktree,
        named: parsed.value("--worktree").is_some(),
        checks,
        out_of_cycle: parsed.value("--commit").map(str::to_string),
        push: !parsed.flag("--no-push"),
        now,
        timeout,
        cause,
        refresh: parsed.flag("--refresh"),
        rerun: parsed.flag("--rerun"),
    };
    if options.push
        && std::env::var_os(CLOCK).is_some()
        && let Some(url) = far_push(&repo.root, &options.remote)
    {
        return refuse(
            err,
            USAGE,
            "release",
            &format!(
                "{CLOCK} is the tests' clock: a run on it pushes only to a remote that is a folder \
                 on this machine, and {} pushes to {url}; add --no-push",
                options.remote
            ),
        );
    }
    if options.cause.is_some()
        && let Some(code) = hand_off(&repo, &options, args, out)
    {
        return code;
    }
    run(&repo, &options, out, err)
}

/// What a run a due count handed its run to is told ([`hand_off`]): the build it is.
const DRIVER: &str = "SYMPHONY_RELEASE_DRIVER";
/// The time the checkout and the build a due count's run starts from have.
const DRIVER_BUILD: Duration = Duration::from_secs(3600);

/// `--commit` as it was named before the rename, which older briefs and reds say.
const LEGACY_COMMIT: &str = "--release";

/// Whether `args` give [`LEGACY_COMMIT`], by its name or by a start of it that no flag of `flags`
/// begins with, as the parser then took it (with its value after `=` or not), before a `--`. A
/// flag's value never starts with `--`, which the parser refuses, so any such word is one.
fn names_legacy_commit(args: &[String], flags: &[Flag]) -> bool {
    (args.iter())
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| {
            let name = arg.split_once('=').map_or(arg.as_str(), |(name, _)| name);
            name.len() > 2
                && LEGACY_COMMIT.starts_with(name)
                && !flags.iter().any(|flag| flag.name.starts_with(name))
        })
}

/// The registry a run reads of crates.io: the tests' when [`crates::REGISTRY`] names one, else
/// crates.io; none on the tests' clock without one, so no test reads crates.io.
fn run_registry() -> Option<crates::Registry> {
    if std::env::var_os(crates::REGISTRY).is_none() && std::env::var_os(CLOCK).is_some() {
        return None;
    }
    Some(crates::Registry::of_env())
}

/// Whether this process is the build a due count handed its run to.
fn driven() -> bool {
    std::env::var_os(DRIVER).is_some()
}

/// Say each line of `noted` on `out`, as a run says what it did on the way.
fn note(out: &mut dyn Write, noted: &[String]) {
    for line in noted {
        let _ = writeln!(out, "release: {line}");
    }
}

/// A due count's run, handed to a build of the tip it verifies. The count ran on the build the
/// main checkout links, building nothing; now, the lock held, the release run's own checkout is
/// brought to the line's tip, `tools/rs-bin.sh --build symphony-build` builds there, in that
/// checkout's own build folder and never the main checkout's (where a landing's `just tasks` would
/// wait on its lock), and this process becomes that build, with `args` as given. None when the run
/// goes on here: this is that build already, the tip holds no `tools/rs-bin.sh`, or the checkout or
/// the build failed, which is said and which the run then meets as it would. An exit code when a
/// run holds the lock: the count waits for it and nothing more runs.
fn hand_off(repo: &Repo, options: &Options, args: &[String], out: &mut dyn Write) -> Option<i32> {
    use std::os::unix::process::CommandExt as _;
    if driven() {
        return None;
    }
    let lock = record::lock(&repo.common).ok()?;
    match lock.taken {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            let _ = writeln!(
                out,
                "release: a run holds the lock: the count waits for it, and nothing more runs"
            );
            return Some(0);
        }
        Err(TryLockError::Error(_)) => return None,
    }
    let folder = lock.folder.clone();
    // Said while it builds, so a count or a run started meanwhile hears who holds the lock.
    let building = Running {
        version: record::VERSION,
        id: format!("build-{}", std::process::id()),
        kind: "release".into(),
        pid: std::process::id(),
        started_at: options.now,
        until: options.now + DRIVER_BUILD.as_secs_f64(),
        check: None,
    };
    let _ = record::hold(&folder, &building);
    let built = driver(repo, options, out);
    record::release(&folder, &building.id);
    drop(lock);
    let driver = match built {
        Ok(Some(driver)) => driver,
        Ok(None) => return None,
        Err(why) => {
            let _ = writeln!(out, "release: the run goes on from this build: {why}");
            return None;
        }
    };
    let _ = writeln!(
        out,
        "release: the run goes on from {}, built in its own checkout",
        driver.display()
    );
    let _ = out.flush();
    let error = std::process::Command::new(&driver)
        .arg("release")
        .args(args)
        .env(DRIVER, &driver)
        .exec();
    let _ = writeln!(
        out,
        "release: {} cannot run ({error}): the run goes on from this build",
        driver.display()
    );
    None
}

/// The build of `symphony-build` a due count's run starts from: the release run's checkout brought
/// to the line's tip as fetched, then built there in its own build folder ([`hand_off`]), what the
/// build printed going to `out`. None when the tip holds no `tools/rs-bin.sh`; why, when any of it
/// fails.
fn driver(repo: &Repo, options: &Options, out: &mut dyn Write) -> Result<Option<PathBuf>, String> {
    let until = Bound::until(Instant::now() + DRIVER_BUILD);
    let mut place = place::place(&repo.root, &repo.main, &repo.common, &options.worktree)?;
    let tracking = format!("refs/remotes/{}/{}", options.remote, options.branch);
    let refspec = format!("+refs/heads/{}:{tracking}", options.branch);
    let fetch = ["fetch", "--quiet", "--no-tags", &options.remote, &refspec];
    git_until(&repo.root, &fetch, until)?;
    let tip = commit(&repo.root, &tracking)?;
    place.checkout(&repo.root, &repo.common, &tip, until, options.now)?;
    let launcher = place.checkout.join("tools/rs-bin.sh");
    if !launcher.is_file() {
        return Ok(None);
    }
    let tree = place.checkout.display();
    let mut build = symphony::children::command(&launcher);
    build
        .args(["--build", "symphony-build"])
        .current_dir(&place.checkout)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove(DRIVER);
    let done = group::output_until(&mut build, until).map_err(|failed| match failed {
        group::Failed::Io(e) => format!("{} could not run: {e}", launcher.display()),
        group::Failed::Late(took) | group::Failed::Interrupted(took) => format!(
            "the build of its tip in {tree} was stopped after {} s",
            took.as_secs()
        ),
    })?;
    let _ = out.write_all(&done.stderr);
    if !done.status.success() {
        return Err(format!(
            "the build of its tip in {tree} failed ({})",
            done.status
        ));
    }
    let printed = String::from_utf8_lossy(&done.stdout);
    let path = PathBuf::from(printed.lines().last().unwrap_or_default().trim());
    if !path.is_file() {
        return Err(format!(
            "the build of its tip in {tree} named no program ({})",
            path.display()
        ));
    }
    Ok(Some(path))
}

/// The first place `remote` pushes to, as the repository at `root` says, that is not a folder on
/// this machine; none when every one is. A name that is no remote is the place itself. Git reads a
/// place with a `:` before its first `/` as a URL, a `host:path` or a helper's `name::address`,
/// never as a folder, whatever folder of that name there is; any other, or one after `file://`, is
/// a path.
fn far_push(root: &Path, remote: &str) -> Option<String> {
    let urls: Vec<String> = git(root, &["remote", "get-url", "--push", "--all", remote])
        .map_or_else(
            |_| vec![remote.to_string()],
            |listed| listed.lines().map(str::to_string).collect(),
        );
    urls.into_iter().find(|url| {
        let path = url.strip_prefix("file://").unwrap_or(url);
        let first = path.split('/').next().unwrap_or_default();
        path.is_empty() || first.contains(':') || !root.join(path).is_dir()
    })
}

/// The checks `--check NAME=COMMAND` names, in order; [`VERIFY`] when none does. A name keyed as
/// one of the release run's own reds ([`board::RUN`], a refresh's) is not a check's.
fn checks_of<'a>(given: impl Iterator<Item = &'a str>) -> Result<Vec<Step>, String> {
    let mut checks: Vec<Step> = Vec::new();
    for check in given {
        let Some((name, command)) = check.split_once('=') else {
            return Err(format!("--check {check:?} is not NAME=COMMAND"));
        };
        if name.trim().is_empty() || command.trim().is_empty() {
            return Err(format!("--check {check:?} names no check or no command"));
        }
        let key = board::key(name);
        if key == board::RUN || board::is_refresh(name) {
            return Err(format!(
                "--check {name:?} takes a name the release run keeps for its own reds"
            ));
        }
        if checks.iter().any(|known| board::key(&known.name) == key) {
            return Err(format!("--check {name:?} is named twice"));
        }
        checks.push(Step::shell(name, command));
    }
    if checks.is_empty() {
        checks.push(Step::shell(VERIFY.0, VERIFY.1));
    }
    Ok(checks)
}

/// The repository the checkout at `at` belongs to.
fn repo(at: &Path) -> Result<Repo, String> {
    let root = PathBuf::from(git(at, &["rev-parse", "--show-toplevel"])?);
    let root = root.canonicalize().unwrap_or(root);
    let common = PathBuf::from(git(
        &root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?);
    let common = common.canonicalize().unwrap_or(common);
    let main = match common.parent() {
        Some(main) if common.file_name().is_some_and(|name| name == ".git") => main.to_path_buf(),
        _ => root.clone(),
    };
    Ok(Repo {
        project: Project::of(&main),
        root,
        common,
        main,
    })
}

/// The run's own checkout: `release` in the project's agent worktrees folder, else in a folder
/// beside the main checkout named for it with `-wt`.
fn default_worktree(repo: &Repo) -> PathBuf {
    let folder = match &repo.project.agents.worktrees {
        Some(folder) => repo.main.join(folder),
        None => {
            let name = repo
                .main
                .file_name()
                .map_or_else(|| "project".into(), |name| name.to_string_lossy());
            repo.main.join("..").join(format!("{name}-wt"))
        }
    };
    normal(&folder.join("release"))
}

/// `--status`: the newest release, the last run and the last green run, the release candidate, the
/// landings on `remote`'s `branch` since the last release and what the next run waits for, and the
/// last refresh, read from the record, the local tags and the refs as last fetched; nothing is
/// written or fetched.
fn status(
    repo: &Repo,
    remote: &str,
    branch: &str,
    now: f64,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let ordered = match ordered_releases(&repo.root) {
        Ok(ordered) => ordered,
        Err(problem) => {
            let _ = writeln!(err, "symphony-build release: {problem}");
            return 1;
        }
    };
    let folder = record::folder(&repo.common);
    let runs = record::read(&folder);
    let _ = write!(out, "{}", record::status(&runs, &ordered, now));
    // What a run waits for before it checks main ([`regenerated`]), in a project that has any.
    let generates = symphony_board::land::Settings::of(&repo.main)
        .is_ok_and(|settings| !settings.generators.is_empty());
    if generates && let Ok(standing) = symphony_board::land::regenerate::status(&at_main(repo)) {
        let _ = writeln!(out, "regenerator: {}", standing.line);
    }
    // The release candidate as last settled: `--status` writes nothing, so a land not settled yet
    // is the next run's or count's to hear.
    let held = match candidate::read(&folder) {
        Ok(Some(held)) => {
            let bounds = candidate::Bounds::of(&repo.project.releases);
            let _ = write!(out, "{}", held.status(now, bounds));
            held.hold()
        }
        Ok(None) if repo.project.releases.holds_candidates() => {
            let _ = writeln!(out, "release candidate: none held");
            None
        }
        Ok(None) => {
            let _ = writeln!(
                out,
                "release candidate: off ([releases] candidate_reruns or candidate_hours is 0): a \
                 red run holds none"
            );
            None
        }
        Err(problem) => {
            let _ = writeln!(out, "release candidate: {problem}");
            None
        }
    };
    let every = repo.project.releases.every_landings;
    match cadence::of(&repo.root, &folder, &ordered, every, remote, branch, held) {
        Ok(cadence) => {
            let _ = write!(out, "{}", cadence.lines());
        }
        Err(problem) => {
            let _ = writeln!(out, "landings: cannot be counted: {problem}");
        }
    }
    let _ = write!(out, "{}", record::publish_status(&runs, now));
    let _ = write!(out, "{}", record::refresh_status(&runs, now));
    // Whether the machine's server is the release `current` names, or what holds it on a
    // project's build ([`symphony::machine::switch`]).
    match symphony::machine::dir() {
        Ok(machine) => {
            let _ = write!(out, "{}", symphony::machine::switch::standing(&machine));
        }
        Err(problem) => {
            let _ = writeln!(out, "machine: {problem}");
        }
    }
    0
}

/// This repository's release tags, newest first, each with the commit it tags.
fn ordered_releases(root: &Path) -> Result<Vec<(Release, String)>, String> {
    let releases = local_releases(root)?;
    let mut ordered: Vec<(Release, String)> = Vec::new();
    for release in tags::newest_first(releases.iter().map(|(name, _)| name.as_str())) {
        let name = release.name();
        if let Some((_, commit)) = releases.iter().find(|(tag, _)| *tag == name) {
            ordered.push((release, commit.clone()));
        }
    }
    Ok(ordered)
}

/// Where the cadence of `repo` stands on `remote`'s `branch`, with what a release candidate `held`
/// of it.
fn cadence_of(
    repo: &Repo,
    remote: &str,
    branch: &str,
    held: Option<cadence::Hold>,
) -> Result<cadence::Cadence, String> {
    let ordered = ordered_releases(&repo.root)?;
    cadence::of(
        &repo.root,
        &record::folder(&repo.common),
        &ordered,
        repo.project.releases.every_landings,
        remote,
        branch,
        held,
    )
}

/// Settle the open release candidate `held` of `repo` at `now` against `remote`'s `branch` as last fetched or
/// pushed, the main checkout's own branch, the newest release tagged here and the board
/// ([`candidate::settle`]), then bring its branch to it ([`candidate::steer`]): what it did, each
/// said. The caller holds the run's lock and writes the record.
fn settle_held(
    repo: &Repo,
    remote: &str,
    branch: &str,
    held: &mut candidate::Candidate,
    now: f64,
) -> Vec<String> {
    let mut noted = Vec::new();
    let (remote_name, remote_tip) = match cadence::line_of(&repo.root, remote, branch) {
        Ok(line) => line,
        Err(problem) => {
            noted.push(format!(
                "the release candidate is left as it was: {problem}"
            ));
            return noted;
        }
    };
    let local = commit(&repo.root, &format!("refs/heads/{branch}")).ok();
    let newest = (ordered_releases(&repo.root).unwrap_or_default().first())
        .map(|(release, commit)| (release.name(), commit.clone()));
    let line = candidate::Line {
        root: &repo.root,
        branch,
        remote_name: &remote_name,
        remote: &remote_tip,
        local: local.as_deref(),
        newest: (newest.as_ref()).map(|(name, commit)| (name.as_str(), commit.as_str())),
    };
    let board = symphony::taskcli::CliBoard::new(&repo.main);
    let bounds = candidate::Bounds::of(&repo.project.releases);
    noted.extend(candidate::settle(held, &line, &board, now, bounds));
    noted.extend(candidate::steer(&repo.root, held));
    noted
}

/// The release candidate of `repo`, settled at `now` ([`settle_held`]) and written back while the
/// run's lock is free to take, else as it was last written: a run holds the lock then, and it
/// settles the candidate itself. None when none was ever held; an error when its record does not
/// read.
fn settled(
    repo: &Repo,
    remote: &str,
    branch: &str,
    now: f64,
    out: &mut dyn Write,
) -> Result<Option<candidate::Candidate>, String> {
    let lock = record::lock(&repo.common)?;
    if lock.taken.is_err() {
        return candidate::read(&lock.folder);
    }
    let mut found = candidate::read(&lock.folder)?;
    if let Some(held) = found.as_mut().filter(|held| held.open()) {
        note(out, &settle_held(repo, remote, branch, held, now));
        candidate::write(&lock.folder, held)?;
    }
    Ok(found)
}

/// `--drop`: the open release candidate dropped at `now`, its branch gone, so the next run checks
/// main's tip: its exit code. Refused while a run holds the lock or none is held.
fn drop_candidate(repo: &Repo, now: f64, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let refused = |err: &mut dyn Write, why: &str| {
        let _ = writeln!(err, "symphony-build release: {why}");
        1
    };
    let lock = match record::lock(&repo.common) {
        Ok(lock) => lock,
        Err(problem) => return refused(err, &problem),
    };
    if lock.taken.is_err() {
        return refused(
            err,
            "a release run holds the lock: drop the release candidate once it ends",
        );
    }
    let mut held = match candidate::read(&lock.folder) {
        Ok(Some(held)) if held.open() => held,
        Ok(_) => {
            return refused(
                err,
                "no release candidate is held, so there is none to drop",
            );
        }
        Err(problem) => return refused(err, &problem),
    };
    held.drop(now, "dropped by `just release --drop`".into());
    note(out, candidate::steer(&repo.root, &mut held).as_slice());
    if let Err(problem) = candidate::write(&lock.folder, &held) {
        return refused(err, &problem);
    }
    let _ = writeln!(
        out,
        "release: dropped the release candidate {}: the next landing starts a run of main's tip, \
         or `just release`",
        short(&held.commit)
    );
    0
}

/// This repository's release tags, each with the commit it tags.
fn local_releases(root: &Path) -> Result<Vec<(String, String)>, String> {
    let listed = git(
        root,
        &[
            "for-each-ref",
            "--format=%(refname:strip=2) %(objectname) %(*objectname)",
            &format!("refs/tags/{}*", tags::PREFIX),
        ],
    )?;
    Ok(listed
        .lines()
        .filter_map(|line| {
            let mut words = line.split(' ');
            let name = words.next()?;
            let object = words.next()?;
            let peeled = words.next().filter(|peeled| !peeled.is_empty());
            Some((name.to_string(), peeled.unwrap_or(object).to_string()))
        })
        .collect())
}

/// Why a run stopped before its checks decided anything.
enum Stop {
    /// What it was asked cannot be done; nothing is recorded.
    Refused(String),
    /// It could not get as far as deciding: recorded as an error, and the board's red [`board::RUN`].
    Error(String),
    /// A process is in the run's folders: an error, and the red [`board::RUN`] for the owner to
    /// clear, since the process may be anyone's and no executor ends another's.
    InUse(String),
}

/// A release run, an out-of-cycle release or a refresh: its exit code.
fn run(repo: &Repo, options: &Options, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let lock = match record::lock(&repo.common) {
        Ok(lock) => lock,
        Err(problem) => {
            let _ = writeln!(err, "symphony-build release: {problem}");
            return 1;
        }
    };
    let folder = lock.folder.clone();
    let lock_path = lock.path.clone();
    let started = span::Span::begin();
    let when = local(options.now);
    let mut record = Run {
        version: record::VERSION,
        id: format!(
            "{}-{}",
            when.replace(' ', "T").replace(':', ""),
            std::process::id()
        ),
        kind: if options.out_of_cycle.is_some() {
            "out-of-cycle"
        } else if options.refresh {
            "refresh"
        } else {
            "release"
        }
        .into(),
        commit: String::new(),
        tip: String::new(),
        started_at: options.now,
        ended_at: options.now,
        result: "error".into(),
        error: None,
        previous: None,
        checks: Vec::new(),
        tag: None,
        pushed: false,
        board: record::Board::default(),
        remote: options.remote.clone(),
        unpushed: None,
        cause: options.cause.clone(),
        refreshed: None,
        published: None,
        rerun: None,
        slept: 0.0,
        inputs: None,
        environment: None,
    };
    // HUP, INT and TERM stop the run, its check's group and its git, and let it say so and release
    // the lock; a signal this process was started ignoring stays ignored.
    let interrupts = Interrupts::listen().ok();
    let heard = || interrupts.as_ref().is_some_and(Interrupts::received);
    match &lock.taken {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return held(repo, options, &folder, record, out, err),
        Err(TryLockError::Error(e)) => {
            let _ = writeln!(
                err,
                "symphony-build release: {}",
                symphony::text::os_error(e, &lock_path)
            );
            return 1;
        }
    }
    // A run killed while its check ran left that said; while that check's group lives, it runs on.
    let earlier = record::holder(&folder).filter(|earlier| earlier.id != record.id);
    if let Some(earlier) = &earlier
        && let Some(check) = &earlier.check
        && group::lives(check.group, check.start.as_ref())
    {
        return outlived(repo, options, &folder, record, earlier, check, out, err);
    }
    let running = Running {
        version: record::VERSION,
        id: record.id.clone(),
        kind: record.kind.clone(),
        pid: std::process::id(),
        started_at: options.now,
        until: options.now + options.timeout.as_secs_f64(),
        check: None,
    };
    let _ = record::hold(&folder, &running);
    span::awake();
    // A run that ended without recording itself (killed, or the machine stopped) is recorded first,
    // so its logs stay with it and `--status` names it.
    for found in record::recover(&folder, earlier.as_ref(), &record.id, options.now) {
        let _ = writeln!(
            out,
            "release: recorded the run {} (started {}) as interrupted: it {}",
            found.id,
            local(found.started_at),
            found.error.as_deref().unwrap_or_default()
        );
    }
    let say = |check: Option<&Checking>| {
        let running = Running {
            check: check.cloned(),
            ..running.clone()
        };
        let _ = record::hold(&folder, &running);
    };
    let within = Within {
        // The run's time is counted while the machine is awake: a sleep takes none of it.
        bound: Bound::hearing(started.awake + options.timeout, &heard),
        running: Some(&say),
    };
    let refreshing = Refreshing {
        repo,
        options,
        folder: &folder,
        running: &running,
        started,
        heard: &heard,
    };
    // The release candidate, when this run is one of main's line: none or ended runs main's tip, an
    // open one is rerun once every red of it is answered, and before that nothing runs.
    let mut held: Option<candidate::Candidate> = None;
    let turn = if options.refresh || options.out_of_cycle.is_some() {
        Ok(Turn::Main)
    } else {
        turn(repo, options, &folder, &mut held, within.bound, out)
    };
    let rerun = matches!(turn, Ok(Turn::Rerun));
    let ran = match turn {
        Ok(Turn::Wait(why)) => {
            let _ = writeln!(out, "release: {why}: nothing runs");
            record::release(&folder, &record.id);
            return 0;
        }
        Err(stop) => Err(stop),
        Ok(Turn::Main) if options.refresh => refresh_only(&refreshing, &mut record, out),
        Ok(Turn::Main) => night(repo, options, &folder, &mut record, None, out, within),
        Ok(Turn::Rerun) => night(
            repo,
            options,
            &folder,
            &mut record,
            held.as_ref(),
            out,
            within,
        ),
    };
    // A green release, tagged and pushed, publishes on crates.io, then refreshes the machine
    // (docs/RELEASES.md).
    if ran.is_ok()
        && !options.refresh
        && record.pushed
        && let Some(tag) = record.tag.clone()
    {
        let commit = record.commit.clone();
        record.published = Some(refreshing.publish(&record.id, &tag, &commit, out));
    }
    if ran.is_ok()
        && !options.refresh
        && let Some(tag) = record.tag.clone()
    {
        if !repo.project.releases.refresh {
            let _ = writeln!(out, "release: refresh: off ([releases] refresh = false)");
        } else if !record.pushed {
            let _ = writeln!(
                out,
                "release: refresh: {tag} is not pushed, so no project can take it: nothing is \
                 refreshed"
            );
        } else {
            let commit = record.commit.clone();
            record.refreshed = Some(refreshing.to(&record.id, &tag, &commit, out));
        }
    }
    // By the wall clock, with how long of it the machine slept ([`span`]).
    let took = started.took();
    record.ended_at = options.now + took.seconds;
    record.slept = took.slept;
    if let Some(slept) = span::said(record.slept) {
        let _ = writeln!(
            out,
            "release: the machine slept {slept} under this run, which its time counts"
        );
    }
    // What the run makes of the release candidate, known before the board is told, whose red
    // tasks say the candidate they are fixed on.
    let decided_red = ran.is_ok() && record.result == "red";
    let mut followed = None;
    let mut next = match held {
        Some(mut held) if rerun => {
            if let Some(tag) = &record.tag {
                held.release(record.ended_at, tag);
            } else if decided_red {
                held.decided(&record);
                held.after_red(
                    record.ended_at,
                    candidate::Bounds::of(&repo.project.releases),
                );
            }
            Some(held)
        }
        earlier
            if decided_red
                && options.out_of_cycle.is_none()
                && !options.refresh
                && repo.project.releases.holds_candidates() =>
        {
            let mut new = candidate::Candidate::of(&record);
            // The branch the run last set, so the new candidate's move starts from it.
            new.branch = earlier.and_then(|earlier| earlier.branch);
            Some(new)
        }
        // The run of main's tip a drop made due reached its checks, or found that tip released
        // already, so the drop is followed; one stopped before them leaves it due, and the next
        // landing asks for it again.
        Some(mut ended)
            if ended.due() && (cadence::attempted(&record) || record.result == "unchanged") =>
        {
            ended.followed();
            followed = Some(ended);
            None
        }
        _ => None,
    };
    let holding = (next.as_ref())
        .filter(|next| decided_red && next.open())
        .map(|next| next.commit.clone());
    let mut for_owner = false;
    match ran {
        Err(Stop::Refused(why)) => {
            let _ = writeln!(err, "symphony-build release: {why}");
            record::release(&folder, &record.id);
            return 1;
        }
        Err(Stop::Error(why)) => {
            record.result = "error".into();
            record.error = Some(why);
        }
        Err(Stop::InUse(why)) => {
            record.result = "error".into();
            record.error = Some(why);
            for_owner = true;
        }
        Ok(()) => {}
    }
    let mut code = 0;
    if record.result != "green" && record.result != "unchanged" {
        code = 1;
    }
    if let Some(error) = &record.error {
        let _ = writeln!(err, "symphony-build release: {error}");
        code = 1;
    }
    if record
        .refreshed
        .as_ref()
        .is_some_and(|refresh| !refresh.green())
        || (record.published.as_ref()).is_some_and(|published| published.failed)
    {
        code = 1;
    }
    record.board = tell(
        repo,
        options,
        &folder,
        &record,
        for_owner,
        holding.as_deref(),
    );
    code = code.max(said(&record.board, out, err));
    if let Err(problem) = record::write(&folder, &record) {
        let _ = writeln!(
            err,
            "symphony-build release: cannot record the run: {problem}"
        );
        code = 1;
    }
    if let Some(next) = next.as_mut() {
        next.named(&record.board.reds);
        note(out, candidate::steer(&repo.root, next).as_slice());
        note(out, &[candidate_said(next)]);
        if let Err(problem) = candidate::write(&folder, next) {
            let _ = writeln!(
                err,
                "symphony-build release: cannot record the release candidate: {problem}"
            );
            code = 1;
        }
    }
    if let Some(ended) = &followed
        && let Err(problem) = candidate::write(&folder, ended)
    {
        let _ = writeln!(
            err,
            "symphony-build release: cannot record the release candidate: {problem}"
        );
        code = 1;
    }
    record::prune(&folder, record::KEPT_RUNS, record::KEPT_LOGS);
    record::release(&folder, &record.id);
    drop(lock);
    code
}

/// What a run makes of the release candidate.
enum Turn {
    /// Run main's tip, or what was asked: no candidate is open.
    Main,
    /// Rerun the open candidate: every red of it is answered.
    Rerun,
    /// Nothing, for this reason: the candidate's reds wait.
    Wait(String),
}

/// What the run of `repo` in `folder` makes of its release candidate, read into `held`: an open
/// one is settled, within `until`, against the remote's main fetched again ([`settle_held`]), and
/// then is rerun once every red of it is answered, or else waits, which `--rerun` refuses. A
/// dropped one whose drop made a run due is followed by this run of main's tip, once it reaches its
/// checks ([`candidate::Candidate::followed`]). A record that does not read stops the run.
fn turn(
    repo: &Repo,
    options: &Options,
    folder: &Path,
    held: &mut Option<candidate::Candidate>,
    until: Bound<'_>,
    out: &mut dyn Write,
) -> Result<Turn, Stop> {
    *held = candidate::read(folder).map_err(Stop::Error)?;
    let (remote, branch) = (options.remote.as_str(), options.branch.as_str());
    if let Some(open) = held.as_mut().filter(|held| held.open()) {
        let tracking = format!("refs/remotes/{remote}/{branch}");
        let refspec = format!("+refs/heads/{branch}:{tracking}");
        let fetch = ["fetch", "--quiet", "--no-tags", remote, &refspec];
        git_until(&repo.root, &fetch, until).map_err(Stop::Error)?;
        note(out, &settle_held(repo, remote, branch, open, options.now));
        candidate::write(folder, open).map_err(Stop::Error)?;
    }
    match held.as_mut() {
        Some(open) if open.open() => {
            let waiting: Vec<&candidate::Red> = open.waiting();
            if waiting.is_empty() {
                return Ok(Turn::Rerun);
            }
            let reds: Vec<String> = waiting.iter().map(|red| red.said()).collect();
            let why = format!(
                "the release candidate {} waits for {}",
                short(&open.commit),
                reds.join(", ")
            );
            if options.rerun {
                return Err(Stop::Refused(format!(
                    "{why}: --rerun reruns it once every red of it is answered"
                )));
            }
            Ok(Turn::Wait(why))
        }
        _ if options.rerun => Err(Stop::Refused(
            "no release candidate is held, so there is none to rerun: `just release` runs main's \
             tip"
            .into(),
        )),
        // A dropped one whose drop made a run of main's tip due stays due until that run reaches
        // its checks ([`candidate::Candidate::followed`]), so one stopped short is asked for again.
        _ => Ok(Turn::Main),
    }
}

/// What the run says of the release candidate it leaves: held, released or dropped.
fn candidate_said(held: &candidate::Candidate) -> String {
    match &held.ended {
        None => format!(
            "holds the release candidate {} (first judged at {}, {} run{} of it): its reds' fixes \
             start from the branch {}",
            short(&held.commit),
            short(&held.first),
            held.rounds,
            if held.rounds == 1 { "" } else { "s" },
            candidate::BRANCH
        ),
        Some(ended) => format!(
            "the release candidate {} is {}",
            short(&held.commit),
            if ended.tag.is_some() {
                ended.why.clone()
            } else {
                format!("dropped: {}; the next run checks main's tip", ended.why)
            }
        ),
    }
}

/// What a release's refresh runs in: the run's repository, options and record folder, what its
/// lock says, when it started and what tells it to stop.
struct Refreshing<'a> {
    repo: &'a Repo,
    options: &'a Options,
    folder: &'a Path,
    running: &'a Running,
    started: span::Span,
    heard: &'a dyn Fn() -> bool,
}

impl Refreshing<'_> {
    /// The lock's word made to cover `more` from now, so no later run takes this one for stuck.
    fn hold_for(&self, more: Duration) {
        let until = self.options.now + self.started.took().seconds + more.as_secs_f64();
        let _ = record::hold(
            self.folder,
            &Running {
                until: until.max(self.running.until),
                check: None,
                ..self.running.clone()
            },
        );
    }

    /// The release `tag` (`commit`) published on crates.io by the run `id` ([`crates::publish`]),
    /// each line of it said.
    fn publish(
        &self,
        id: &str,
        tag: &str,
        commit: &str,
        out: &mut dyn Write,
    ) -> crates::Publication {
        let published = self.publishing(id, tag, commit, out);
        for line in published.lines() {
            let _ = writeln!(out, "release: crates.io: {line}");
        }
        published
    }

    /// [`Refreshing::publish`]'s work: nothing while `[releases] publish` is off or under
    /// `--no-push`, and on the tests' clock nothing but to the tests' registry through their
    /// stand-in for cargo; else from
    /// the run's own checkout, brought to `commit` first (a later run may have left it elsewhere),
    /// within [`PUBLISH`] from now, which the lock's word is made to cover.
    fn publishing(
        &self,
        id: &str,
        tag: &str,
        commit: &str,
        out: &mut dyn Write,
    ) -> crates::Publication {
        let not = |why: String| crates::Publication::not(tag, &why, true);
        if !self.repo.project.releases.publish {
            return crates::Publication::not(tag, crates::OFF, false);
        }
        // As a run with --no-push pushes nothing, a refresh with it publishes nothing.
        if !self.options.push {
            return crates::Publication::not(tag, "--no-push publishes nothing", false);
        }
        if std::env::var_os(CLOCK).is_some()
            && [crates::REGISTRY, crates::PUBLISHER]
                .iter()
                .any(|name| std::env::var_os(name).is_none())
        {
            return not(format!(
                "{CLOCK} is the tests' clock: a run on it publishes only to the registry {} \
                 names, through the stand-in for cargo {} names",
                crates::REGISTRY,
                crates::PUBLISHER
            ));
        }
        self.hold_for(PUBLISH);
        let until = Bound::hearing(Instant::now() + PUBLISH, self.heard);
        let place = match self.checkout_at(commit, until, out) {
            Ok(place) => place,
            Err(why) => return not(why),
        };
        let cargo = std::env::var_os(crates::PUBLISHER).unwrap_or_else(|| "cargo".into());
        let log = record::logs(self.folder, id).join("publish.log");
        let registry = crates::Registry::of_env();
        crates::publish(&place.checkout, tag, &registry, &cargo, &log, until)
    }

    /// The run's own checkout brought to `commit` by `until` (a later run may have left it
    /// elsewhere), unused by any process, what that noted said on `out`.
    fn checkout_at(
        &self,
        commit: &str,
        until: Bound<'_>,
        out: &mut dyn Write,
    ) -> Result<Place, String> {
        let (repo, options) = (self.repo, self.options);
        let mut place = place::place(&repo.root, &repo.main, &repo.common, &options.worktree)
            .map_err(|why| format!("the run's checkout: {why}"))?;
        if let Err(Stop::Refused(why) | Stop::Error(why) | Stop::InUse(why)) = unused(&place, until)
        {
            return Err(why);
        }
        let noted = place
            .checkout(&repo.root, &repo.common, commit, until, options.now)
            .map_err(|why| format!("the run's checkout: {why}"))?;
        note(out, &noted);
        Ok(place)
    }

    /// The release `tag` (`commit`) installed on the machine folder `machine` by the run `id`
    /// ([`install`]), built in the run's own checkout, brought to `commit` first, within
    /// [`INSTALL`] from now, which the lock's word is made to cover. None on the tests' clock
    /// without their stand-in for cargo, so no test builds the workspace.
    fn install(
        &self,
        id: &str,
        tag: &str,
        commit: &str,
        machine: &Path,
        out: &mut dyn Write,
    ) -> Option<install::Installed> {
        let builder = std::env::var_os(install::BUILDER);
        if std::env::var_os(CLOCK).is_some() && builder.is_none() {
            return None;
        }
        self.hold_for(INSTALL);
        let until = Bound::hearing(Instant::now() + INSTALL, self.heard);
        let logs = record::logs(self.folder, id);
        let _ = fs::create_dir_all(&logs);
        let named = install::step(tag);
        Some(match self.checkout_at(commit, until, out) {
            Err(why) => install::never(&named, &install::log(&logs), why),
            Ok(place) => install::install(&install::Install {
                checkout: &place.checkout,
                machine,
                tag,
                commit,
                logs: &logs,
                builder: builder.as_deref(),
                until,
            }),
        })
    }

    /// The machine refreshed to the release `tag` (`commit`) by the run `id`: the release installed
    /// on it first ([`Refreshing::install`]), then the switch, the boards, `current` and the
    /// machine's server ([`refresh::release`]) within [`REFRESH`] from the install's end, which the
    /// lock's word is made to cover, so no later run takes it for stuck. Nothing past the install
    /// on the tests' clock without their stand-in for cargo: there is no release to bring it to.
    fn to(&self, id: &str, tag: &str, commit: &str, out: &mut dyn Write) -> record::Refresh {
        let machine = match symphony::machine::dir() {
            Ok(machine) => machine,
            Err(why) => {
                let logs = record::logs(self.folder, id);
                let _ = fs::create_dir_all(&logs);
                let named = install::step(tag);
                let why = format!("the machine folder cannot be named: {why}");
                let installed = install::never(&named, &install::log(&logs), why);
                return refresh::unreached(tag, installed, out);
            }
        };
        match self.install(id, tag, commit, &machine, out) {
            Some(installed) => self.refresh(id, tag, commit, &machine, installed, out),
            None => record::Refresh {
                release: tag.to_string(),
                ..record::Refresh::default()
            },
        }
    }

    /// [`Refreshing::to`] once the install `installed` ended.
    fn refresh(
        &self,
        id: &str,
        tag: &str,
        commit: &str,
        machine: &Path,
        installed: install::Installed,
        out: &mut dyn Write,
    ) -> record::Refresh {
        self.hold_for(REFRESH);
        let until = Bound::hearing(Instant::now() + REFRESH, self.heard);
        let logs = record::logs(self.folder, id);
        let surreal = symphony_board::server::Home::in_common(&self.repo.common, None)
            .surreal()
            .to_path_buf();
        let against = refresh::Against {
            root: &self.repo.root,
            ours: &self.repo.common,
            machine,
            tag,
            commit,
            logs: &logs,
            surreal: Some(&surreal),
            until,
        };
        refresh::release(&against, installed, out)
    }
}

/// `--refresh`: the machine refreshed again to the newest release tagged here, checking nothing,
/// into `record`.
fn refresh_only(
    refreshing: &Refreshing<'_>,
    record: &mut Run,
    out: &mut dyn Write,
) -> Result<(), Stop> {
    let ordered = ordered_releases(&refreshing.repo.root).map_err(Stop::Error)?;
    let Some((release, commit)) = ordered.first() else {
        return Err(Stop::Refused(
            "no release is tagged here, so there is nothing to refresh to".into(),
        ));
    };
    let tag = release.name();
    record.commit.clone_from(commit);
    record.tip.clone_from(commit);
    let _ = writeln!(out, "release: refresh to {tag} ({})", short(commit));
    record.published = Some(refreshing.publish(&record.id, &tag, commit, out));
    let refreshed = refreshing.to(&record.id, &tag, commit, out);
    record.result = if refreshed.green() { "green" } else { "red" }.into();
    record.refreshed = Some(refreshed);
    Ok(())
}

/// What the run did on the board, said: 1 when the board refused something, else 0.
fn said(board: &record::Board, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    for (what, slugs) in [
        ("filed", &board.filed),
        ("updated", &board.updated),
        ("closed", &board.closed),
    ] {
        if !slugs.is_empty() {
            let _ = writeln!(out, "release: {what} {}", slugs.join(", "));
        }
    }
    match &board.error {
        Some(error) => {
            let _ = writeln!(err, "symphony-build release: the board: {error}");
            1
        }
        None => 0,
    }
}

/// A run refused because another process holds the lock: its exit code. Only a run holds it, and
/// says so in `running.json`; one that holds it well past its own time is stuck. A lock held with no
/// living run saying so is held by a process no run knows of. Either is recorded and reported as the
/// red [`board::RUN`], naming who holds the lock. One held while a killed run's check runs on is the
/// run reporting that check's, and is refused as that check's run is.
fn held(
    repo: &Repo,
    options: &Options,
    folder: &Path,
    mut record: Run,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    // A run that has just taken the lock says so at once, over what an earlier run left.
    let said_by = Instant::now() + SAYS_HELD;
    let living = || record::holder(folder).filter(|holder| pid_alive(i64::from(holder.pid)));
    let mut holder = living();
    while holder.is_none() && Instant::now() < said_by {
        #[expect(
            clippy::disallowed_methods,
            reason = "no-timed-waits baseline: looks again for the folder's holder to say itself, which no event announces"
        )]
        std::thread::sleep(group::POLL);
        holder = living();
    }
    if holder.is_none()
        && let Some(earlier) = record::holder(folder)
        && let Some(check) = &earlier.check
        && group::lives(check.group, check.start.as_ref())
    {
        let _ = writeln!(err, "symphony-build release: {}", runs_on(&earlier, check));
        return 1;
    }
    let lock = folder.join("run.lock");
    let why = match holder {
        Some(holder) if options.now <= holder.until + LATE_SECONDS => {
            let _ = writeln!(
                err,
                "symphony-build release: another release run of this repository is running \
                 (run {}, process {}, since {})",
                holder.id,
                holder.pid,
                local(holder.started_at)
            );
            return 1;
        }
        Some(holder) => format!(
            "{} is still held past the time of the run {} (process {}, started {}, its time up \
             at {}), by {}: that run may be stuck",
            lock.display(),
            holder.id,
            holder.pid,
            local(holder.started_at),
            local(holder.until),
            held_by(&lock),
        ),
        None => format!(
            "{} is held, but no run says it holds it: it is held by {}, which no run of this \
             repository is; nothing runs until that ends",
            lock.display(),
            held_by(&lock),
        ),
    };
    let _ = writeln!(err, "symphony-build release: {why}");
    record.error = Some(why);
    record.board = tell(repo, options, folder, &record, false, None);
    said(&record.board, out, err);
    let _ = record::write(folder, &record);
    1
}

/// The processes `lsof` sees holding `file`, said; this one, which has it open to ask, left out.
fn held_by(file: &Path) -> String {
    let others = group::holders(file).map(|mut pids| {
        pids.retain(|pid| *pid != std::process::id());
        pids
    });
    match others {
        Ok(pids) if pids.is_empty() => "no process lsof can see".to_string(),
        Ok(pids) => {
            let pids: Vec<String> = pids.iter().map(u32::to_string).collect();
            format!("process {}", pids.join(", "))
        }
        Err(problem) => problem,
    }
}

/// Why a run is refused while the check `check` of the run `earlier`, killed while it ran, runs on.
fn runs_on(earlier: &Running, check: &Checking) -> String {
    format!(
        "another release run of this repository is running (run {}, process {}, since {}): \
         it ended, but its check `{}` runs on in process group {}",
        earlier.id,
        earlier.pid,
        local(earlier.started_at),
        check.name,
        check.group
    )
}

/// A run that took the lock while the check `check` of the run `earlier`, killed while that check
/// ran, still runs: its exit code. Refused while that run's time lasts; past it, recorded and
/// reported as the red [`board::RUN`], naming the check's group. Nothing of it is stopped.
#[expect(
    clippy::too_many_arguments,
    reason = "the run, the earlier run and its check, and where to say it"
)]
fn outlived(
    repo: &Repo,
    options: &Options,
    folder: &Path,
    mut record: Run,
    earlier: &Running,
    check: &Checking,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    if options.now <= earlier.until + LATE_SECONDS {
        let _ = writeln!(err, "symphony-build release: {}", runs_on(earlier, check));
        return 1;
    }
    let why = format!(
        "the check `{}` of the run {} (process {}, started {}, its time up at {}) still runs in \
         process group {}, though that run ended: nothing runs until it ends",
        check.name,
        earlier.id,
        earlier.pid,
        local(earlier.started_at),
        local(earlier.until),
        check.group
    );
    let _ = writeln!(err, "symphony-build release: {why}");
    record.error = Some(why);
    record.board = tell(repo, options, folder, &record, false, None);
    said(&record.board, out, err);
    let _ = record::write(folder, &record);
    1
}

/// Whether `run` was red in the key `key`: the run's own when it stopped short, else a check's.
fn red_in(run: &Run, key: &str) -> bool {
    if key == board::RUN {
        return run.error.is_some();
    }
    let refreshed = (run.refreshed.iter()).flat_map(|refresh| refresh.steps.iter());
    (run.checks.iter().chain(refreshed))
        .flat_map(|ran| std::iter::once(ran).chain(&ran.steps))
        .any(|ran| board::key(&ran.name) == key && !ran.green())
}

/// Whether this run (`record`), green in `key`, closes that key's open red: its candidate holds the
/// commit the newest of the `earlier` runs red in it was on. With no such run recorded, only a
/// run of main's tip closes it, never an out-of-cycle release.
fn closes(root: &Path, earlier: &[Run], record: &Run, key: &str) -> bool {
    match earlier.iter().rev().find(|run| red_in(run, key)) {
        None => record.kind == "release",
        Some(red) if red.commit.is_empty() => true,
        Some(red) => {
            !record.commit.is_empty()
                && is_ancestor(root, &red.commit, &record.commit).unwrap_or(false)
        }
    }
}

/// Tell the board of `record`: its reds filed or updated, the reds it found green closed, and the
/// red [`board::RUN`] when it stopped short, never a refresh step that is its project's to finish
/// ([`record::Refresh::told`]); that red is the owner's to clear when it is
/// `for_owner` (a process in the run's folders). Its reds are a release candidate's when it holds
/// one, at the tip `holding` ([`candidate`]).
fn tell(
    repo: &Repo,
    options: &Options,
    folder: &Path,
    record: &Run,
    for_owner: bool,
    holding: Option<&str>,
) -> record::Board {
    let earlier = record::read(folder);
    let green_at = record::last_green(&earlier)
        .map(|run| run.commit.clone())
        .or_else(|| (record.previous.as_ref()).and_then(|tag| commit(&repo.root, tag).ok()));
    let since = match &green_at {
        Some(green) if !record.commit.is_empty() => git(
            &repo.root,
            &["log", "--oneline", &format!("{green}..{}", record.commit)],
        )
        .map(|listed| listed.lines().map(str::to_string).collect())
        .unwrap_or_default(),
        _ => Vec::new(),
    };
    let when = local(options.now);
    let closing = |key: &str| closes(&repo.root, &earlier, record, key);
    let night = board::Night {
        id: &record.id,
        when: &when,
        commit: &record.commit,
        green: green_at.as_deref(),
        since: &since,
        stopped: record.error.as_deref(),
        for_owner,
        closes: &closing,
        whole: !options.refresh,
        candidate: holding,
        rerun: record.rerun.is_some(),
        slept: record.slept,
    };
    let board = symphony::taskcli::CliBoard::new(&repo.main);
    let mut steps = record.checks.clone();
    // A refresh step its project stopped is its project's to finish: filed nowhere, closing none.
    steps.extend((record.refreshed.iter()).flat_map(|refresh| refresh.told().cloned()));
    board::report(&board, &night, &steps)
}

/// A release tag, here or on the remote: its name, its commit and whether the remote holds it.
struct Known {
    name: String,
    commit: String,
    on_remote: bool,
}

/// The release tags here and on the remote, within `until`: a remote's tag wins a name both hold.
fn releases(repo: &Repo, remote: &str, until: Bound<'_>) -> Result<Vec<Known>, String> {
    let pattern = format!("refs/tags/{}*", tags::PREFIX);
    let listed = git_until(
        &repo.root,
        &["ls-remote", "--tags", remote, &pattern],
        until,
    )?;
    let mut all: Vec<Known> = (git::remote_tags(&listed).into_iter())
        .map(|(name, commit)| Known {
            name,
            commit,
            on_remote: true,
        })
        .collect();
    for (name, commit) in local_releases(&repo.root)? {
        if !all.iter().any(|known| known.name == name) {
            all.push(Known {
                name,
                commit,
                on_remote: false,
            });
        }
    }
    all.retain(|known| Release::parse(&known.name).is_some());
    Ok(all)
}

/// The run itself, into `record`, `within` the run's bound. With `held`, a rerun of that release
/// candidate: its tip is checked, with no wait for the regenerator (a fix commits the client its
/// own sources make), each check as the candidate plans it ([`candidate::Candidate::plan`]).
#[expect(
    clippy::too_many_lines,
    reason = "one run, step by step, reads best in one place"
)]
fn night(
    repo: &Repo,
    options: &Options,
    folder: &Path,
    record: &mut Run,
    held: Option<&candidate::Candidate>,
    out: &mut dyn Write,
    within: Within<'_>,
) -> Result<(), Stop> {
    let until = within.bound;
    let placed = |why: String| {
        if options.named {
            Stop::Refused(why)
        } else {
            Stop::Error(why)
        }
    };
    let mut place =
        place::place(&repo.root, &repo.main, &repo.common, &options.worktree).map_err(placed)?;
    let remote = options.remote.as_str();
    let tracking = format!("refs/remotes/{remote}/{}", options.branch);
    git_until(
        &repo.root,
        &[
            "fetch",
            "--quiet",
            "--no-tags",
            remote,
            &format!("+refs/heads/{}:{tracking}", options.branch),
        ],
        until,
    )
    .map_err(Stop::Error)?;
    let main_line = format!("{remote}/{}", options.branch);
    let tip = commit(&repo.root, &tracking).map_err(Stop::Error)?;
    let mut candidate = match (&options.out_of_cycle, held) {
        (_, Some(held)) => {
            if !is_ancestor(&repo.root, &held.commit, &tip).map_err(Stop::Error)? {
                return Err(Stop::Error(format!(
                    "the release candidate {} is not on {main_line}: only a commit on it is \
                     released",
                    short(&held.commit)
                )));
            }
            held.commit.clone()
        }
        (None, None) => tip.clone(),
        (Some(rev), None) => {
            let named = commit(&repo.root, rev).map_err(Stop::Refused)?;
            if !is_ancestor(&repo.root, &named, &tip).map_err(Stop::Error)? {
                return Err(Stop::Refused(format!(
                    "{rev} ({}) is not on {main_line}: only a commit on it is released",
                    short(&named)
                )));
            }
            named
        }
    };
    record.commit.clone_from(&candidate);
    record.tip = tip;
    let releases = releases(repo, remote, until).map_err(Stop::Error)?;
    let names = || releases.iter().map(|known| known.name.as_str());
    let newest_first = tags::newest_first(names());
    let known = |release: &Release| releases.iter().find(|known| known.name == release.name());
    let newest = newest_first.first().and_then(known);
    let mut unpushed: Vec<String> = (newest_first.iter())
        .filter_map(known)
        .filter(|known| !known.on_remote)
        .map(|known| known.name.clone())
        .collect();
    record.unpushed = Some(unpushed.clone());
    if let Some(released) = releases.iter().find(|known| known.commit == candidate) {
        if options.out_of_cycle.is_some() {
            return Err(Stop::Refused(format!(
                "{} is {} already",
                short(&candidate),
                released.name
            )));
        }
        if let Some(newest) = newest.filter(|newest| newest.name != released.name) {
            return Err(Stop::Error(format!(
                "{main_line} is {} ({}), behind the newest release {}: main went back, and \
                 nothing is released until it moves past {}",
                released.name,
                short(&candidate),
                newest.name,
                newest.name
            )));
        }
        let _ = writeln!(
            out,
            "release: {main_line} is {} already ({}): nothing new to check",
            released.name,
            short(&candidate)
        );
        record.result = "unchanged".into();
        if options.push && !released.on_remote {
            record.error = Some(format!(
                "{} is tagged here but not on {remote}: {} delivers it",
                released.name,
                record::push_line(remote, &released.name)
            ));
        }
        return Ok(());
    }
    if let Some(newest) = newest
        && !is_ancestor(&repo.root, &newest.commit, &candidate).map_err(Stop::Error)?
    {
        let named = options.out_of_cycle.as_deref().unwrap_or(&main_line);
        let why = format!(
            "{named} ({}) is not a descendant of the newest release {} ({}): a release only \
             moves forward",
            short(&candidate),
            newest.name,
            short(&newest.commit)
        );
        return Err(if options.out_of_cycle.is_some() {
            Stop::Refused(why)
        } else {
            Stop::Error(why)
        });
    }
    let previous = newest.map(|newest| notes::Previous {
        facts: notes::facts(&repo.root, &newest.commit),
        tag: newest.name.clone(),
        commit: newest.commit.clone(),
    });
    record.previous = previous.as_ref().map(|p| p.tag.clone());
    if options.out_of_cycle.is_none() && held.is_none() {
        regenerated(repo, options, &mut candidate, record, until, out)?;
    }
    let what = match held {
        Some(held) => format!("round {} of the release candidate", held.next_round()),
        None => record.kind.clone(),
    };
    let _ = writeln!(
        out,
        "release: {what} of {} ({main_line} at {}) in {}",
        short(&candidate),
        short(&record.tip),
        place.checkout.display()
    );
    unused(&place, until)?;
    let noted = place
        .checkout(&repo.root, &repo.common, &candidate, until, options.now)
        .map_err(Stop::Error)?;
    for noted in noted {
        let _ = writeln!(out, "release: {noted}");
    }
    let logs = record::logs(folder, &record.id);
    fs::create_dir_all(&logs).map_err(|e| Stop::Error(symphony::text::os_error(&e, &logs)))?;
    record::so_far(&logs, record);
    let log = |step: &Step| logs.join(format!("{}.log", board::key(&step.name)));
    // What the tree says its checks read, and what they run on: a green this run finds is
    // recorded with both, and a rerun carries one only on the same of each ([`inputs`]). A first
    // run carries nothing, and says nothing of either.
    record.environment = Some(inputs::Environment::now(&repo.root, until));
    record.inputs = inputs::ask(&place.checkout, until);
    record::so_far(&logs, record);
    let environment = record.environment.as_ref().map(inputs::Environment::said);
    let same = |commit: &str| candidate::same_tree(&repo.root, commit, &candidate);
    let told = record.inputs.clone();
    let tip = candidate::Tip {
        same: &same,
        inputs: told.as_ref(),
        environment: environment.as_deref(),
    };
    if let Some(held) = held {
        for said in held.uncarried(&tip) {
            let _ = writeln!(out, "release: {said}");
        }
    }
    let mut carried: Vec<record::Carried> = Vec::new();
    for step in &options.checks {
        told_to_stop(until, &format!("before `{}` started", step.name))?;
        let (planned, kept) = match held {
            Some(held) => held.plan(&step.name, &tip),
            None => (candidate::Planned::Whole, Vec::new()),
        };
        // How the greens it keeps are kept: all from a commit of this tree, as a rerun always
        // carried, or some by their inputs' digest.
        let by_tree = kept.iter().all(|kept| kept.digest.is_none());
        carried.extend(kept);
        let only;
        let step = match &planned {
            candidate::Planned::Whole => step,
            candidate::Planned::Carried => {
                let _ = writeln!(
                    out,
                    "release: `{}` carried green: {}",
                    step.name,
                    if by_tree {
                        "the commit it was green on holds this tree"
                    } else {
                        "its inputs are as they were when it was green"
                    }
                );
                continue;
            }
            candidate::Planned::Steps(names) => {
                let _ = writeln!(
                    out,
                    "release: `{}` reruns {}, the rest carried green {}",
                    step.name,
                    names.join(", "),
                    if by_tree {
                        "from this tree"
                    } else {
                        "on the inputs they were green on"
                    }
                );
                only = step.only(names);
                &only
            }
        };
        // What the run's time leaves out is red, never passed over: a release is checked whole.
        let ran = if Instant::now() >= until.until {
            Ran::never(step, &log(step), NOT_STARTED.into())
        } else {
            steps::run(step, &place.checkout, &logs, &log(step), within, out)
        };
        tell_check(&ran, out);
        stopped_in(&ran)?;
        let looked = look(&place, &candidate, until, &after(&ran), true);
        keep(record, &logs, ran, looked)?;
    }
    if let Some(held) = held {
        record.rerun = Some(record::Rerun {
            first: held.first.clone(),
            round: held.next_round(),
            carried: carried.clone(),
        });
        record::so_far(&logs, record);
    }
    let green =
        record.checks.iter().all(Ran::green) && !(record.checks.is_empty() && carried.is_empty());
    record.result = if green { "green" } else { "red" }.into();
    if !green {
        let red: Vec<&str> = (record.checks.iter())
            .flat_map(steps::reds)
            .map(|ran| ran.name.as_str())
            .collect();
        let _ = writeln!(out, "release: red: {}; no release", red.join(", "));
        return Ok(());
    }
    let name = tags::next(&when_day(options.now), names());
    if let Some(newest) = newest_first.first()
        && Release::parse(&name).is_none_or(|next| next <= *newest)
    {
        record.error = Some(format!(
            "this machine's clock names the release {name}, which does not sort after the newest \
             release {}: no tag",
            newest.name()
        ));
        return Ok(());
    }
    let changes = match &previous {
        Some(previous) => {
            notes::changes(&repo.root, &previous.commit, &candidate).map_err(Stop::Error)?
        }
        None => Vec::new(),
    };
    told_to_stop(until, "before it tagged")?;
    let until = until.later(FINISH);
    // What git names alone: the processes were listed after the last check, and nothing has run
    // in the checkout since.
    look(&place, &candidate, until, "before it tagged", false)?;
    // A check carried whole is green as much as one run, in the order the checks are named: the
    // notes say which were carried.
    let ran = |name: &str| record.checks.iter().any(|ran| ran.name == name);
    let checked: Vec<String> = (options.checks.iter())
        .map(|step| step.name.clone())
        .filter(|name| {
            // Carried whole, by its own green or by each step and part it listed.
            let under = format!("{name}/");
            let kept = |kept: &record::Carried| kept.name == *name || kept.name.starts_with(&under);
            ran(name) || carried.iter().any(kept)
        })
        .collect();
    let held_notes = held.map(|held| held.notes(&carried)).unwrap_or_default();
    // What it publishes on crates.io once pushed: each package whose version crates.io lacks now.
    let publishes = match run_registry() {
        Some(registry) if options.push && repo.project.releases.publish => {
            crates::notes(&place.checkout, &registry, until)
        }
        _ => Vec::new(),
    };
    // A tag is never held back for its notes: one git cannot read is said, and left out.
    let taking = consumer::taking(&repo.root, &name, &candidate).unwrap_or_else(|why| {
        let _ = writeln!(
            out,
            "release: {name}'s message says nothing of what a project does to take it: {why}"
        );
        Vec::new()
    });
    let message = notes::message(
        &name,
        &candidate,
        notes::facts(&repo.root, &candidate),
        previous.as_ref(),
        &checked,
        &publishes,
        &held_notes,
        &taking,
        &changes,
    );
    if let Err(problem) = git_until(
        &repo.root,
        &["tag", "-a", &name, &candidate, "-m", &message],
        until,
    ) {
        record.error = Some(format!("cannot tag {name}: {problem}"));
        return Ok(());
    }
    record.tag = Some(name.clone());
    unpushed.insert(0, name.clone());
    if options.push {
        // That one ref, whatever the configuration says to push beside it.
        let refspec = format!("refs/tags/{name}:refs/tags/{name}");
        let pushed = git_until(
            &repo.root,
            &[
                "push",
                "--quiet",
                "--no-follow-tags",
                "--recurse-submodules=no",
                remote,
                &refspec,
            ],
            until,
        );
        match pushed {
            Ok(_) => {
                record.pushed = true;
                unpushed.retain(|tag| *tag != name);
            }
            Err(problem) => {
                record.error = Some(format!(
                    "tagged {name} here, but the push to {remote} failed: {problem}; {} \
                     delivers it",
                    record::push_line(remote, &name)
                ));
            }
        }
    }
    record.unpushed = Some(unpushed);
    let pushed = if record.pushed {
        format!(", pushed to {remote}")
    } else {
        ", not pushed".into()
    };
    let _ = writeln!(out, "release: green; tagged {name}{pushed}");
    Ok(())
}

/// The environment of this process, run from the main checkout: where the regenerator's standing
/// is read from.
fn at_main(repo: &Repo) -> symphony_board::cli::Env {
    symphony_board::cli::Env {
        cwd: repo.main.clone(),
        ..symphony_board::cli::Env::of_process()
    }
}

/// Main's generated outputs, the client distribution among them, as the regenerator a land starts
/// makes them after it (`just tasks regenerate --status`), when the run's `candidate` is the
/// main checkout's branch: the run waits, within its time, while a regenerator makes them, and
/// releases nothing of a main whose committed outputs are behind its sources (its `just
/// client-check` would be red), filing nothing, since the regenerator's own red is the task then.
/// A repository with no generators, or none the regenerator reads, has nothing to wait for.
///
/// What the regenerator last wrote refuses the run at once only when it is of the candidate
/// (red or stopped on it, or ended there without a word). A land starts the regenerator just
/// before the `[board] on_land` that starts a run of the cadence, so one may not have written
/// anything yet, its state of an earlier tip or none at all: while nothing makes them and nothing
/// was written of the candidate, the run gives it [`REGENERATOR`] (the tests'
/// [`REGENERATOR_SECONDS`]) to take the candidate before it refuses.
///
/// Main moving on in the main checkout while the run waits (the regenerator landed what it made,
/// or a land came in) is followed: `remote`'s branch fetched again, what it moved to, a descendant
/// of the candidate, is the run's `candidate` from then on, in its `record` too, and waited for
/// in its turn. A move that does not reach the remote within [`REGENERATOR`] refuses the run as
/// a candidate seen behind does, filing nothing: the regenerator was last seen not to have made
/// the candidate's files.
fn regenerated(
    repo: &Repo,
    options: &Options,
    candidate: &mut String,
    record: &mut Run,
    until: Bound<'_>,
    out: &mut dyn Write,
) -> Result<(), Stop> {
    let env = at_main(repo);
    let lands =
        symphony_board::sweep::trigger::folder(&repo.common).join(symphony_board::land::FOLDER);
    let branch = options.branch.as_str();
    let remote = options.remote.as_str();
    let grace = (std::env::var(REGENERATOR_SECONDS).ok())
        .and_then(|seconds| seconds.parse::<u64>().ok())
        .map_or(REGENERATOR, Duration::from_secs);
    let pause = || {
        #[expect(
            clippy::disallowed_methods,
            reason = "no-timed-waits baseline: the release run looks again for the regenerator each period, which no event announces"
        )]
        std::thread::sleep(REGENERATING.min(until.until.saturating_duration_since(Instant::now())));
    };
    // Whether the main checkout's branch was the candidate at a look: from then on its moves are
    // the run's to follow.
    let mut followed = false;
    let mut said = false;
    // Since when nothing has made them while nothing was written of the candidate.
    let mut quiet: Option<Instant> = None;
    // Since when the main checkout's branch has been past what the remote holds.
    let mut unpushed: Option<Instant> = None;
    loop {
        let local = commit(&repo.main, &format!("refs/heads/{branch}")).ok();
        if local.as_deref() != Some(candidate.as_str()) {
            if !followed {
                // Its standing is of another commit than the one the run checks, whose own
                // `just client-check` says.
                return Ok(());
            }
            if let Some(tip) = moved_on(repo, remote, branch, candidate, until)? {
                let _ = writeln!(
                    out,
                    "release: {remote}/{branch} moved on to {} while the run waited for the \
                     regenerator: that is what it checks",
                    short(&tip)
                );
                candidate.clone_from(&tip);
                record.commit.clone_from(&tip);
                record.tip = tip;
                quiet = None;
                unpushed = None;
                continue;
            }
            let since = *unpushed.get_or_insert_with(Instant::now);
            if since.elapsed() >= grace || Instant::now() >= until.until {
                // The candidate was last seen not current: refused as one seen behind is.
                return Err(Stop::Refused(format!(
                    "{branch} moved on to {} here, but {remote}/{branch} is still at {}, whose \
                     generated files the regenerator was last seen not to have made: no release \
                     of a main whose generated files are behind its sources",
                    local.as_deref().map_or("nothing", short),
                    short(candidate)
                )));
            }
            told_to_stop(until, "while it waited for main's move to reach the remote")?;
            pause();
            continue;
        }
        followed = true;
        unpushed = None;
        let standing = match symphony_board::land::regenerate::status(&env) {
            Ok(standing) => standing,
            Err(why) => {
                let _ = writeln!(
                    out,
                    "release: the regenerator's status: {why}; not waited for"
                );
                return Ok(());
            }
        };
        if standing.current {
            if said {
                let _ = writeln!(out, "release: {}", standing.line);
            }
            return Ok(());
        }
        let refused = || {
            Stop::Refused(format!(
                "{}: no release of a main whose generated files are behind its sources",
                standing.line
            ))
        };
        if Instant::now() >= until.until {
            return Err(refused());
        }
        if standing.making {
            quiet = None;
        } else {
            // Read, never locked: the regenerator's lock is its own.
            let of_candidate =
                symphony_board::land::regenerate::State::read(&lands).is_some_and(|state| {
                    state.tip == *candidate || state.landed.as_deref() == Some(candidate.as_str())
                });
            let since = *quiet.get_or_insert_with(Instant::now);
            if of_candidate || since.elapsed() >= grace {
                return Err(refused());
            }
        }
        if !said {
            let waiting = if standing.making {
                "waiting for the regenerator"
            } else {
                "waiting for the regenerator a land starts to take it"
            };
            let _ = writeln!(out, "release: {waiting}: {}", standing.line);
            said = true;
        }
        told_to_stop(until, "while it waited for the regenerator")?;
        pause();
    }
}

/// `remote`'s `branch` once fetched again, when it moved on from `candidate` to a descendant of it;
/// none while it still holds the candidate, or holds anything else.
fn moved_on(
    repo: &Repo,
    remote: &str,
    branch: &str,
    candidate: &str,
    until: Bound<'_>,
) -> Result<Option<String>, Stop> {
    let tracking = format!("refs/remotes/{remote}/{branch}");
    git_until(
        &repo.root,
        &[
            "fetch",
            "--quiet",
            "--no-tags",
            remote,
            &format!("+refs/heads/{branch}:{tracking}"),
        ],
        until,
    )
    .map_err(Stop::Error)?;
    let tip = commit(&repo.root, &tracking).map_err(Stop::Error)?;
    let on = tip != candidate && is_ancestor(&repo.root, candidate, &tip).map_err(Stop::Error)?;
    Ok(on.then_some(tip))
}

/// The processes `lsof` found in each of `trees` (`found`, a list for each), said: each folder in
/// use and by what (a process in a package's folder with that package, [`said_in`]), with the way
/// out. None when no process is in any.
fn in_use(trees: &[&Path], found: &[Vec<group::Inside>]) -> Option<(String, String)> {
    let mut said = Vec::new();
    let mut processes = 0;
    for (tree, inside) in trees.iter().zip(found) {
        if inside.is_empty() {
            continue;
        }
        processes += inside.len();
        let named: Vec<String> = inside.iter().map(|found| said_in(tree, found)).collect();
        said.push(format!(
            "{} is in use by {}",
            tree.display(),
            named.join("; ")
        ));
    }
    if said.is_empty() {
        return None;
    }
    let them = if processes == 1 { "it" } else { "them" };
    let folder = if said.len() == 1 {
        "that folder"
    } else {
        "those folders"
    };
    let way_out = format!(
        "end {them}, or move {them} out of {folder} (a shell, a terminal tab, an editor or a \
         Finder window there counts), and the next run goes"
    );
    Some((said.join("; "), way_out))
}

/// The process `found` in `tree`, said, and when it works in a package's folder there, that
/// package: cargo runs a package's tests in its folder, so what one of them left behind works there,
/// and that is as near to the test that left it as the run can tell (the process has outlived its
/// parent, and carries no environment that names its test).
fn said_in(tree: &Path, found: &group::Inside) -> String {
    let said = found.said();
    match found
        .working_in()
        .and_then(|folder| package_at(tree, folder))
    {
        Some(package) => format!(
            "{said}, where cargo runs the tests of the package {package} (a test of it may have \
             left it)"
        ),
        None => said,
    }
}

/// The name of the package whose folder `folder` (relative to `tree`) is, as its `Cargo.toml` says;
/// none when that folder holds no package's manifest.
fn package_at(tree: &Path, folder: &str) -> Option<String> {
    let text = fs::read_to_string(tree.join(folder).join("Cargo.toml")).ok()?;
    let manifest: toml::Table = toml::from_str(&text).ok()?;
    let name = manifest.get("package")?.get("name")?.as_str()?;
    Some(name.to_string())
}

/// Stop the run before it checks anything when a process, anyone's, works in its checkout or holds
/// a file inside it open, within `until`: the checkout would change under its checks.
fn unused(place: &Place, until: Bound<'_>) -> Result<(), Stop> {
    unused_in(&[place.checkout.as_path()], until)
}

/// [`unused`] of the folders `trees`, those of them that are there.
fn unused_in(trees: &[&Path], until: Bound<'_>) -> Result<(), Stop> {
    let trees: Vec<&Path> = (trees.iter().copied())
        .filter(|tree| fs::symlink_metadata(tree).is_ok())
        .collect();
    if trees.is_empty() {
        return Ok(());
    }
    let found = group::inside(&trees, until).map_err(|problem| {
        let named: Vec<String> = trees
            .iter()
            .map(|tree| tree.display().to_string())
            .collect();
        Stop::Error(format!(
            "cannot tell whether a process is in {}: {problem}",
            named.join(" or ")
        ))
    })?;
    match in_use(&trees, &found) {
        None => Ok(()),
        Some((said, way_out)) => Err(Stop::InUse(format!(
            "{said}: nothing is checked while a process is in the run's folders; {way_out}"
        ))),
    }
}

/// Record the check `ran` into `record` (and its record so far among `logs`) once the look after it
/// (`looked`) passed. A look that stops the run still records what of it is red ([`reds_only`]), so
/// each red it found is filed on its own before the stop ends the run, and no red hides behind the
/// red [`board::RUN`]; what it found green counts for nothing, as what it checked may have changed
/// under it, and closes nothing.
fn keep(record: &mut Run, logs: &Path, ran: Ran, looked: Result<(), Stop>) -> Result<(), Stop> {
    let kept = match &looked {
        Ok(()) => Some(ran),
        Err(_) => reds_only(&ran),
    };
    if let Some(kept) = kept {
        record.checks.push(kept);
        record::so_far(logs, record);
    }
    looked
}

/// What of the check `ran` is red: the check with only its red steps; none when it is green.
fn reds_only(ran: &Ran) -> Option<Ran> {
    (!ran.green()).then(|| Ran {
        steps: (ran.steps.iter())
            .filter(|step| !step.green())
            .cloned()
            .collect(),
        ..ran.clone()
    })
}

/// The point after the check `ran`, said: its name and how it ended.
fn after(ran: &Ran) -> String {
    let verdict = if ran.green() { "green" } else { "red" };
    format!("after `{}` ended {verdict} ({})", ran.name, ran.ended())
}

/// Stop the run when its checkout (in `place`) changed `at` that point: it no longer holds the
/// commit `holds` (HEAD moved, or `git status` names a path outside `.target/`), or, when it lists
/// `processes`, a process works in it or holds a file inside it open. What was checked is then not
/// what would be tagged, so nothing that check found green counts, and its reds are filed beside
/// the stop ([`keep`]). The look has a minute past `until`, and sees that moment alone and nothing
/// git ignores.
fn look(
    place: &Place,
    holds: &str,
    until: Bound<'_>,
    at: &str,
    processes: bool,
) -> Result<(), Stop> {
    let until = until.later(LOOK);
    let path = place.checkout.display();
    let short_holds = short(holds);
    let changed = place::changed(&place.checkout, holds, until).map_err(|e| {
        Stop::Error(format!(
            "{at}, cannot tell whether {path} still holds {short_holds}: {e}"
        ))
    })?;
    let changes = if changed.is_empty() {
        None
    } else {
        Some(format!(
            "{path} no longer holds {short_holds}: {}",
            changed.join(", ")
        ))
    };
    let trees = [place.checkout.as_path()];
    let found = if processes {
        group::inside(&trees, until).map_err(|problem| {
            Stop::Error(format!(
                "{at}, cannot tell whether a process is in {path}: {problem}"
            ))
        })?
    } else {
        Vec::new()
    };
    if let Some((said, way_out)) = in_use(&trees, &found) {
        let changed = changes.map(|c| format!("; and {c}")).unwrap_or_default();
        return Err(Stop::InUse(format!(
            "{at}, {said}{changed}: what was checked may have changed, so nothing checked is \
             released; {way_out}"
        )));
    }
    match changes {
        None => Ok(()),
        Some(changes) => Err(Stop::Error(format!(
            "{at}, {changes}; a process may be writing there, so nothing checked is released"
        ))),
    }
}

/// A check's end, said, then each step it listed, indented.
fn tell_check(ran: &Ran, out: &mut dyn Write) {
    fn said(ran: &Ran, indent: &str, out: &mut dyn Write) {
        let verdict = if ran.green() {
            "green"
        } else if ran.interrupted {
            "stopped"
        } else {
            "red"
        };
        let _ = writeln!(
            out,
            "release: {indent}{} {verdict} ({}, {:.0} s); log {}",
            ran.name,
            ran.ended(),
            ran.seconds,
            ran.log.display()
        );
    }
    said(ran, "", out);
    for step in &ran.steps {
        said(step, "  ", out);
    }
}

/// Why the run stops, when it was told to (HUP, INT or TERM), `at` that point.
fn told_to_stop(bound: Bound<'_>, at: &str) -> Result<(), Stop> {
    if bound.interrupted() {
        return Err(Stop::Error(format!(
            "the run was told to stop (HUP, INT or TERM) {at}"
        )));
    }
    Ok(())
}

/// Why the run stops, when `ran` was stopped because the run was told to: an interrupted check is
/// neither green nor red, so it is no check's red.
fn stopped_in(ran: &Ran) -> Result<(), Stop> {
    if ran.interrupted {
        return Err(Stop::Error(format!(
            "the run was told to stop (HUP, INT or TERM) while `{}` ran, which was stopped after \
             {:.0} s",
            ran.name, ran.seconds
        )));
    }
    Ok(())
}

/// The local day `seconds` since the epoch fall on, `YYYY-MM-DD`.
fn when_day(seconds: f64) -> String {
    local(seconds)[..10].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_are_named_once_each_and_default_to_the_full_verify() {
        let none: [&str; 0] = [];
        let default = checks_of(none.into_iter()).unwrap();
        assert_eq!(default, [Step::shell("verify", "just verify")]);
        let given = checks_of(["a=true", "b=echo x=y"].into_iter()).unwrap();
        assert_eq!(
            given,
            [Step::shell("a", "true"), Step::shell("b", "echo x=y")]
        );
        for (bad, why) in [
            (&["noequals"][..], "is not NAME=COMMAND"),
            (&["=true"][..], "names no check or no command"),
            (&["a= "][..], "names no check or no command"),
            (&["a=true", "A=false"][..], "is named twice"),
            (&["Run=true"][..], "the release run keeps for its own reds"),
        ] {
            let said = checks_of(bad.iter().copied()).unwrap_err();
            assert!(said.contains(why), "{bad:?}: {said}");
        }
    }

    #[test]
    fn the_out_of_cycle_flag_by_its_old_name_is_refused_in_one_line_and_runs_nothing() {
        let args = |words: &[&str]| -> Vec<String> {
            words.iter().map(|word| (*word).to_string()).collect()
        };
        let said = |words: &[&str]| {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = main(&args(words), &mut out, &mut err);
            let (out, err) = (String::from_utf8(out), String::from_utf8(err));
            (code, out.unwrap(), err.unwrap())
        };
        let refused = (
            2,
            String::new(),
            "`--release COMMIT` is now `--commit COMMIT`\n".into(),
        );
        // Refused before the root is looked for, so nothing here is a repository's.
        for words in [
            &["--release", "HEAD"][..],
            &["--release=HEAD"][..],
            &["--rel", "HEAD"][..],
            &["--releas=HEAD", "--status"][..],
            &["--check", "verify=true", "--release", "HEAD", "--help"][..],
            &["--root", "/no/such/root", "--release", "HEAD"][..],
            &["--release"][..],
        ] {
            assert_eq!(said(words), refused, "{words:?}");
        }
        let flags = [
            Flag {
                name: "--remote",
                value: true,
            },
            Flag {
                name: "--refresh",
                value: false,
            },
            Flag {
                name: "--commit",
                value: true,
            },
        ];
        let named = |words: &[&str]| names_legacy_commit(&args(words), &flags);
        assert!(named(&["--release", "x"]));
        assert!(named(&["--relea"]), "a start of it no flag begins with");
        assert!(!named(&["--re"]), "the start of flags of today: theirs");
        assert!(!named(&["--r"]) && !named(&["--"]) && !named(&["-r"]));
        assert!(!named(&["--releases"]), "not a start of it");
        assert!(!named(&["--", "--release"]), "a positional after `--`");
        assert!(!named(&["--commit", "HEAD", "release"]));
        assert!(
            !named(&["--check", "verify=cargo build --release"]),
            "inside a value"
        );
    }

    #[test]
    fn the_runs_checkout_is_in_the_agents_worktrees_folder_else_beside_the_repository() {
        let tree = crate::test_tree::Tree::new();
        let repo = |main: &Path| Repo {
            root: main.to_path_buf(),
            common: main.join(".git"),
            main: main.to_path_buf(),
            project: Project::of(main),
        };
        tree.write("plain/README.md", "no project file\n");
        assert_eq!(
            default_worktree(&repo(&tree.path("plain"))),
            tree.path("plain-wt/release")
        );
        tree.write(
            "named/symphony.toml",
            "[project]\nid = \"named\"\n\n[agents]\nworktrees = \"../elsewhere\"\n",
        );
        assert_eq!(
            default_worktree(&repo(&tree.path("named"))),
            tree.path("elsewhere/release")
        );
    }

    #[test]
    fn a_day_is_the_local_date_of_a_time() {
        assert_eq!(when_day(1_800_000_000.0).len(), 10);
        assert!(tags::is_day(&when_day(1_800_000_000.0)));
    }

    #[test]
    fn the_processes_in_the_runs_folders_are_named_plainly_with_the_way_out() {
        let inside = |pid, command: &str, what: &str| group::Inside {
            pid,
            command: command.into(),
            what: what.into(),
        };
        let trees = [Path::new("/wt/release"), Path::new("/wt/release-other")];
        assert_eq!(in_use(&trees, &[vec![], vec![]]), None);
        let counts = " (a shell, a terminal tab, an editor or a Finder window there counts), and the \
                      next run goes";
        assert_eq!(
            in_use(&trees, &[vec![inside(10, "zsh", "working there")], vec![]]),
            Some((
                "/wt/release is in use by process 10 (zsh), working there".into(),
                format!("end it, or move it out of that folder{counts}")
            ))
        );
        assert_eq!(
            in_use(
                &trees,
                &[
                    vec![
                        inside(10, "zsh", "working there"),
                        inside(11, "Finder", "holding a.txt open")
                    ],
                    vec![]
                ]
            ),
            Some((
                "/wt/release is in use by process 10 (zsh), working there; process 11 (Finder), \
                 holding a.txt open"
                    .into(),
                format!("end them, or move them out of that folder{counts}")
            ))
        );
        assert_eq!(
            in_use(
                &trees,
                &[
                    vec![inside(10, "zsh", "working there")],
                    vec![inside(12, "cargo", "working in consumer")]
                ]
            ),
            Some((
                "/wt/release is in use by process 10 (zsh), working there; \
                 /wt/release-other is in use by process 12 (cargo), working in consumer"
                    .into(),
                format!("end them, or move them out of those folders{counts}")
            ))
        );
    }

    #[test]
    fn a_process_working_in_a_packages_folder_is_said_with_that_package() {
        let tree = crate::test_tree::Tree::new();
        tree.write("Cargo.toml", "[workspace]\nmembers = [\"apps/ui\"]\n");
        tree.write("apps/ui/Cargo.toml", "[package]\nname = \"ui\"\n");
        tree.write("apps/ui/tests/browser.rs", "");
        tree.write("apps/odd/Cargo.toml", "not a manifest [");
        let inside = |pid, what: &str| group::Inside {
            pid,
            command: "chrome_crashpad_handler".into(),
            what: what.into(),
        };
        let found = [
            inside(10, "working in apps/ui"),
            inside(11, "working there"),
            inside(12, "working in apps/ui/tests"),
            inside(13, "holding apps/ui/Cargo.toml open"),
            inside(14, "working in apps/odd"),
            inside(15, "working in apps/gone"),
        ];
        let (said, _) = in_use(&[tree.root.as_path()], &[found.into()]).unwrap();
        assert_eq!(
            said,
            format!(
                "{} is in use by process 10 (chrome_crashpad_handler), working in apps/ui, where \
                 cargo runs the tests of the package ui (a test of it may have left it); process 11 \
                 (chrome_crashpad_handler), working there; process 12 (chrome_crashpad_handler), \
                 working in apps/ui/tests; process 13 (chrome_crashpad_handler), holding \
                 apps/ui/Cargo.toml open; process 14 (chrome_crashpad_handler), working in \
                 apps/odd; process 15 (chrome_crashpad_handler), working in apps/gone",
                tree.root.display()
            ),
            "a workspace's own manifest, a folder below a package's, a file held open, a manifest \
             that does not read and a folder gone name no package"
        );
    }

    #[test]
    fn a_check_a_look_stopped_the_run_after_keeps_its_reds_alone_and_a_passed_one_all() {
        let tree = crate::test_tree::Tree::new();
        let logs = tree.path("logs");
        fs::create_dir_all(&logs).unwrap();
        let step = |name: &str, code: i32| Ran {
            name: format!("verify/{name}"),
            exit_code: Some(code),
            ..red("x")
        };
        let verify = Ran {
            steps: vec![step("lint", 0), step("tests", 101), step("docs", 0)],
            ..red("verify")
        };
        let stopped = || Err(Stop::InUse("in use".into()));

        let mut record = run_of("a", "release", "abc");
        let kept = keep(&mut record, &logs, verify.clone(), stopped());
        assert!(matches!(kept, Err(Stop::InUse(why)) if why == "in use"));
        assert_eq!(
            record.checks,
            [Ran {
                steps: vec![step("tests", 101)],
                ..red("verify")
            }],
            "the red step is kept, the greens count for nothing"
        );
        assert!(red_in(&record, "verify-tests"));
        assert!(!red_in(&record, "verify-lint"));

        let mut record = run_of("b", "release", "abc");
        let green = Ran {
            exit_code: Some(0),
            ..red("verify")
        };
        let changed = Err(Stop::Error("changed".into()));
        assert!(keep(&mut record, &logs, green.clone(), changed).is_err());
        assert!(
            record.checks.is_empty(),
            "a green that may have changed counts for nothing"
        );

        // Red beyond its steps (every step it listed green): the check alone is kept, and filed.
        let beyond = Ran {
            steps: vec![step("lint", 0)],
            ..red("verify")
        };
        assert!(keep(&mut record, &logs, beyond, stopped()).is_err());
        assert_eq!(record.checks, [red("verify")]);
        assert!(steps::beyond(&record.checks[0]));

        let mut record = run_of("c", "release", "abc");
        assert!(keep(&mut record, &logs, verify.clone(), Ok(())).is_ok());
        assert!(keep(&mut record, &logs, green.clone(), Ok(())).is_ok());
        assert_eq!(
            record.checks,
            [verify, green],
            "a check the look passed is kept whole"
        );
        let so_far: Run =
            serde_json::from_str(&fs::read_to_string(logs.join("run.json")).unwrap()).unwrap();
        assert_eq!(
            so_far.checks, record.checks,
            "the record so far says what is kept"
        );
    }

    fn run_of(id: &str, kind: &str, commit: &str) -> Run {
        Run {
            version: record::VERSION,
            id: id.into(),
            kind: kind.into(),
            commit: commit.into(),
            tip: commit.into(),
            started_at: 0.0,
            ended_at: 0.0,
            result: "red".into(),
            error: None,
            previous: None,
            checks: Vec::new(),
            tag: None,
            pushed: false,
            board: record::Board::default(),
            remote: "origin".into(),
            unpushed: None,
            cause: None,
            refreshed: None,
            published: None,
            rerun: None,
            slept: 0.0,
            inputs: None,
            environment: None,
        }
    }

    fn red(name: &str) -> Ran {
        Ran {
            name: name.into(),
            command: "c".into(),
            exit_code: Some(1),
            signal: None,
            seconds: 0.0,
            log: PathBuf::from("/l"),
            steps: Vec::new(),
            error: None,
            timed_out: false,
            interrupted: false,
            load: None,
        }
    }

    #[test]
    fn a_remote_pushes_off_this_machine_unless_every_place_it_pushes_to_is_a_folder_here() {
        let tree = crate::test_tree::Tree::new();
        let root = tree.root.canonicalize().unwrap();
        let bare = root.join("bare.git");
        fs::create_dir_all(&bare).unwrap();
        let here = bare.to_string_lossy().into_owned();
        let absolute = format!("file://{here}");
        // Folders named as git reads a URL or `host:path`: git still pushes off this machine.
        fs::create_dir_all(root.join("https:/example.invalid/named.git")).unwrap();
        fs::create_dir_all(root.join("example.invalid:scp.git")).unwrap();
        git(&root, &["init", "-q"]).unwrap();
        for (name, url) in [
            ("here", here.as_str()),
            ("file", "file://bare.git"),
            ("absolute", absolute.as_str()),
            ("far", "https://example.invalid/far.git"),
            ("named", "https://example.invalid/named.git"),
            ("scp", "example.invalid:scp.git"),
            ("helper", "hang::nowhere"),
            ("both", here.as_str()),
        ] {
            git(&root, &["remote", "add", name, url]).unwrap();
        }
        git(
            &root,
            &["remote", "set-url", "--add", "--push", "both", &here],
        )
        .unwrap();
        git(
            &root,
            &[
                "remote",
                "set-url",
                "--add",
                "--push",
                "both",
                "https://example.invalid/both.git",
            ],
        )
        .unwrap();
        assert_eq!(far_push(&root, "here"), None);
        assert_eq!(far_push(&root, "file"), None);
        assert_eq!(far_push(&root, "absolute"), None);
        assert_eq!(
            far_push(&root, "far").as_deref(),
            Some("https://example.invalid/far.git")
        );
        assert_eq!(
            far_push(&root, "named").as_deref(),
            Some("https://example.invalid/named.git")
        );
        assert_eq!(
            far_push(&root, "scp").as_deref(),
            Some("example.invalid:scp.git")
        );
        assert_eq!(far_push(&root, "helper").as_deref(), Some("hang::nowhere"));
        assert_eq!(
            far_push(&root, "both").as_deref(),
            Some("https://example.invalid/both.git"),
            "every place it pushes to counts"
        );
        // A name that is no remote is the place itself.
        assert_eq!(far_push(&root, &here), None);
        assert_eq!(far_push(&root, "nowhere").as_deref(), Some("nowhere"));
    }

    #[test]
    fn a_green_closes_a_red_only_from_a_candidate_holding_the_reds_commit() {
        let tree = crate::test_tree::Tree::new();
        let root = tree.path("repo");
        fs::create_dir_all(&root).unwrap();
        let commit_here = |message: &str| {
            git(
                &root,
                &[
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@example.com",
                    "commit",
                    "-q",
                    "--allow-empty",
                    "-m",
                    message,
                ],
            )
            .unwrap();
            git(&root, &["rev-parse", "HEAD"]).unwrap()
        };
        git(&root, &["init", "-q", "-b", "main"]).unwrap();
        let older = commit_here("older");
        let reddened = commit_here("red");
        let fixed = commit_here("fixed");
        let mut red_night = run_of("a", "release", &reddened);
        red_night.checks = vec![red("verify")];
        let mut stopped = run_of("b", "release", "");
        stopped.error = Some("cannot fetch".into());
        let earlier = [red_night, stopped];

        let release_of = |commit: &str| run_of("c", "out-of-cycle", commit);
        assert!(!closes(&root, &earlier, &release_of(&older), "verify"));
        assert!(closes(&root, &earlier, &release_of(&reddened), "verify"));
        assert!(closes(&root, &earlier, &release_of(&fixed), "verify"));
        assert!(
            closes(&root, &earlier, &release_of(&older), board::RUN),
            "a stop before any candidate is cleared by any run that gets through"
        );
        assert!(
            closes(&root, &earlier, &run_of("d", "release", &older), "lint"),
            "a red no run recorded: a run of main's tip closes it"
        );
        assert!(
            !closes(&root, &earlier, &release_of(&fixed), "lint"),
            "an out-of-cycle release does not"
        );
    }

    #[test]
    fn a_step_a_check_listed_is_red_or_green_in_its_own_key() {
        let step = |name: &str, code: i32| Ran {
            name: format!("verify/{name}"),
            exit_code: Some(code),
            ..red("x")
        };
        let mut night = run_of("a", "release", "abc");
        night.checks = vec![Ran {
            steps: vec![step("lint", 0), step("tests", 101)],
            ..red("verify")
        }];
        assert!(red_in(&night, "verify-tests"));
        assert!(red_in(&night, "verify"), "the check is red in its step");
        assert!(!red_in(&night, "verify-lint"));
        assert!(!red_in(&night, "tests"), "a step is keyed under its check");
    }
}
