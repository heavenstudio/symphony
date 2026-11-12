//! `symphony nightly` (`just nightly`): Symphony's releases (docs/RELEASES.md). A run takes main's
//! tip as fetched from the remote, the commit a project can fetch, into a clean checkout of its own
//! outside the repository's folder ([`place`]), runs the full verification there and conciliates
//! the projects the machine serves against it ([`conciliate`]). Every red check, and a run that
//! stopped short of deciding, is a task on the board ([`board`]); when every check is green it tags
//! the commit as the next release (`release-YYYY-MM-DD`, [`tags`]) with what it holds ([`notes`])
//! and pushes that one tag. `--release COMMIT` does the same out of cycle for a commit already on
//! the remote's main and past the newest release. Each run is recorded in the git common dir
//! ([`record`]), which `--status` reads; `--schedule` prints the entry that runs it each night
//! ([`schedule`]), which the owner installs.
//!
//! It never tags a commit that is not on the remote's main, that is behind the newest release or
//! that a check found red, never moves or deletes a tag, and pushes nothing but its new tag. After
//! each check and each project's conciliation it looks at its checkout (and at that project's
//! scratch copy), and before it tags at what git names there once more: another HEAD, a path `git
//! status` names outside `.target/`, or a process `lsof` sees working there or holding a file there
//! open stops the run, and nothing is tagged. A look sees a moment, and only what git does not
//! ignore: a file git ignores (`build/`, `artifacts/`, `.target/`, which checks read) or a change
//! undone between two looks goes unseen. What keeps those off is that the checkout is the run's
//! own, made and locked by it, that each check's process group is ended with it, and the same
//! `lsof` look before anything is checked; that one sees a process only while it works in those
//! folders or holds a file there open. One run at a time per repository, none while a check of an
//! earlier run killed while it ran still runs ([`checks`]), and a run that ended without recording
//! itself is recorded by the next ([`record::recover`]); everything it runs is bounded by the run's
//! time and stops when the run is told to (HUP, INT or TERM, [`group`]).

mod board;
mod cadence;
mod checks;
mod conciliate;
mod git;
mod group;
mod notes;
mod pin;
mod place;
mod record;
mod refresh;
mod schedule;
mod tags;

use std::fs::{self, OpenOptions, TryLockError};
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

use crate::cli::{Flag, parse, refuse};
use crate::project::Project;

const USAGE: &str = "usage: symphony nightly [-h] [--root ROOT] [--remote NAME] [--branch NAME] \
[--worktree PATH] [--check NAME=COMMAND] [--release COMMIT] [--no-push] [--timeout SECONDS] \
[--status] [--schedule] [--at HH:MM] [--due [SLUG COMMIT]] [--refresh]";
const HELP: &str =
    "Symphony's releases: main's tip as fetched from the remote, verified in a clean \
checkout outside the repository's folder and conciliated with the projects this machine serves; \
each red check filed on the board as tests/nightly-red-NAME, and a green commit tagged \
release-YYYY-MM-DD and pushed, then the machine refreshed to it: each project's pin moved on a \
branch through its own commit gate, its main fast-forwarded to it in its main checkout when that \
is on its main and clean, and its board moved to the schema of the pin its main locks \
(docs/RELEASES.md).

options:
  -h, --help            show this help message and exit
  --root ROOT           a checkout of the repository to release (default: the current folder)
  --remote NAME         the remote whose main is released and that takes the tag (default: origin)
  --branch NAME         its main line (default: the project's default branch)
  --worktree PATH       the run's own checkout, outside the repository's folder (default: nightly
                        in the project's agent worktrees folder)
  --check NAME=COMMAND  a check, run in that checkout with sh -c; repeat for each, in order
                        (default: verify=just verify)
  --release COMMIT      out of cycle: the same checks on COMMIT, which must be on the remote's main
                        and past the newest release
  --no-push             tag the release here, push nothing
  --timeout SECONDS     the run's time: a check still running then is stopped, and it and every
                        check not yet started are red (default: 36000, ten hours; at most
                        86400)
  --status              print the newest release, the last night and its age, the landings since
                        the last release and what the next run waits for, and the last refresh;
                        run nothing
  --schedule            print the schedule entry that runs `just nightly` each day; install nothing
  --at HH:MM            the time of day --schedule names (default: 03:00)
  --due [SLUG COMMIT]   run a night only when [releases] every_landings tasks have landed since the
                        last release or the last run that checked something, and no run is in
                        progress; else say what it waits for and run nothing, counting with git
                        and building nothing. A due night runs from a build of the tip made in
                        the run's own checkout. A landing's on_land command runs it, naming the
                        task and its merge
  --refresh             refresh the machine to the newest release here again, checking nothing:
                        an orchestrator's retry of a red or interrupted refresh";

/// The checks a night runs unless told others: the full verification.
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
/// Why a check, or a project's conciliation, is red without having run.
const NOT_STARTED: &str = "the run's time was up before it started";
/// The time a release's refresh has, past its run's checks: a project's commit gate and its board's
/// move each run within it.
const REFRESH: Duration = Duration::from_secs(2 * 3600);
/// The tests' clock: the seconds since the epoch a run takes as now, in place of this machine's.
/// No flag sets it, and a run on it pushes only to a remote that is a folder on this machine
/// ([`far_push`]), so a release named for another day than the machine's never leaves it.
const CLOCK: &str = "SYMPHONY_NIGHTLY_NOW";
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
const REGENERATOR_SECONDS: &str = "SYMPHONY_NIGHTLY_REGENERATOR_SECONDS";

/// What a run is asked to do.
struct Options {
    remote: String,
    branch: String,
    worktree: PathBuf,
    /// Whether `--worktree` named it: a checkout named by hand that is not the nightly's is a
    /// refusal, the default one an error the board hears of.
    named: bool,
    checks: Vec<Step>,
    release: Option<String>,
    push: bool,
    now: f64,
    timeout: Duration,
    /// What started it, when the cadence did: the landings it counted.
    cause: Option<String>,
    /// Whether it refreshes the machine to the newest release alone, checking nothing.
    refresh: bool,
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

/// `symphony nightly ARGS`: its exit code. 0 is green (or nothing new to release), 1 a red or a
/// run that could not finish, 2 a command line refused.
pub fn main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let flag = |name, value| Flag { name, value };
    let flags = [
        flag("--root", true),
        flag("--remote", true),
        flag("--branch", true),
        flag("--worktree", true),
        flag("--check", true),
        flag("--release", true),
        flag("--no-push", false),
        flag("--timeout", true),
        flag("--status", false),
        flag("--schedule", false),
        flag("--at", true),
        flag("--due", false),
        flag("--refresh", false),
    ];
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
                "nightly",
                &format!("unrecognized arguments: {extra}"),
            );
        }
        Ok(parsed) => parsed,
        Err(problem) => return refuse(err, USAGE, "nightly", &problem),
    };
    let now = match clock() {
        Ok(now) => now,
        Err(problem) => return refuse(err, USAGE, "nightly", &problem),
    };
    let timeout = match parsed.value("--timeout") {
        None => Duration::from_secs(RUN_SECONDS),
        Some(seconds) => match seconds.parse::<u64>() {
            Ok(seconds) if seconds > MOST_SECONDS => {
                return refuse(
                    err,
                    USAGE,
                    "nightly",
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
                    "nightly",
                    &format!("--timeout {seconds:?} is not a number of seconds"),
                );
            }
        },
    };
    let modes = ["--status", "--schedule", "--release", "--due", "--refresh"]
        .into_iter()
        .filter(|mode| parsed.flag(mode))
        .count();
    if modes > 1 {
        return refuse(
            err,
            USAGE,
            "nightly",
            "--status, --schedule, --release, --due and --refresh are one each",
        );
    }
    let repo = match repo(Path::new(parsed.value("--root").unwrap_or("."))) {
        Ok(repo) => repo,
        Err(problem) => {
            let _ = writeln!(err, "symphony nightly: {problem}");
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
            return refuse(err, USAGE, "nightly", &format!("--at {at:?} is not HH:MM"));
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
        Err(problem) => return refuse(err, USAGE, "nightly", &problem),
    };
    let worktree = match parsed.value("--worktree") {
        Some(path) => normal(&std::env::current_dir().unwrap_or_default().join(path)),
        None => default_worktree(&repo),
    };
    let mut cause = None;
    if parsed.flag("--due") {
        if let [slug, landed] = &parsed.positionals[..] {
            let _ = writeln!(out, "nightly: {slug} landed ({})", short(landed));
        }
        let cadence = match cadence_of(&repo, &remote, &branch) {
            Ok(cadence) => cadence,
            Err(problem) => {
                let _ = writeln!(
                    err,
                    "symphony nightly: the landings cannot be counted: {problem}"
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
    let options = Options {
        remote,
        branch,
        worktree,
        named: parsed.value("--worktree").is_some(),
        checks,
        release: parsed.value("--release").map(str::to_string),
        push: !parsed.flag("--no-push"),
        now,
        timeout,
        cause,
        refresh: parsed.flag("--refresh"),
    };
    if options.push
        && std::env::var_os(CLOCK).is_some()
        && let Some(url) = far_push(&repo.root, &options.remote)
    {
        return refuse(
            err,
            USAGE,
            "nightly",
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

/// What a run a due count handed its night to is told ([`hand_off`]): the build it is.
const DRIVER: &str = "SYMPHONY_NIGHTLY_DRIVER";
/// The time the checkout and the build a due count's night runs from have.
const DRIVER_BUILD: Duration = Duration::from_secs(3600);

/// A due count's night, handed to a build of the tip it verifies. The count ran on the build the
/// main checkout links, building nothing; now, the lock held, the nightly's own checkout is brought
/// to the line's tip, `tools/rs-bin.sh --build symphony` builds there, in that checkout's own build
/// folder and never the main checkout's (where a landing's `just tasks` would wait on its lock), and
/// this process becomes that build, with `args` as given. None when the night runs here: this is
/// that build already, the tip holds no `tools/rs-bin.sh`, or the checkout or the build failed,
/// which is said and which the night then meets as it would. An exit code when a run holds the lock:
/// the count waits for it and nothing more runs.
fn hand_off(repo: &Repo, options: &Options, args: &[String], out: &mut dyn Write) -> Option<i32> {
    use std::os::unix::process::CommandExt as _;
    if std::env::var_os(DRIVER).is_some() {
        return None;
    }
    let folder = record::folder(&repo.common);
    fs::create_dir_all(&folder).ok()?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(folder.join("run.lock"))
        .ok()?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            let _ = writeln!(
                out,
                "nightly: a run holds the lock: the count waits for it, and nothing more runs"
            );
            return Some(0);
        }
        Err(TryLockError::Error(_)) => return None,
    }
    // Said while it builds, so a count or a night started meanwhile hears who holds the lock.
    let building = Running {
        id: format!("build-{}", std::process::id()),
        kind: "nightly".into(),
        pid: std::process::id(),
        started_at: options.now,
        until: options.now + DRIVER_BUILD.as_secs_f64(),
        check: None,
    };
    let _ = record::hold(&folder, &building);
    let built = driver(repo, options, out);
    record::release(&folder, &building.id);
    let _ = lock.unlock();
    drop(lock);
    let driver = match built {
        Ok(Some(driver)) => driver,
        Ok(None) => return None,
        Err(why) => {
            let _ = writeln!(out, "nightly: the night runs from this build: {why}");
            return None;
        }
    };
    let _ = writeln!(
        out,
        "nightly: the night runs from {}, built in its own checkout",
        driver.display()
    );
    let _ = out.flush();
    let error = std::process::Command::new(&driver)
        .arg("nightly")
        .args(args)
        .env(DRIVER, &driver)
        .exec();
    let _ = writeln!(
        out,
        "nightly: {} cannot run ({error}): the night runs from this build",
        driver.display()
    );
    None
}

/// The build of `symphony` a due count's night runs from: the nightly's checkout brought to the
/// line's tip as fetched, then built there in its own build folder ([`hand_off`]), what the build
/// printed going to `out`. None when the tip holds no `tools/rs-bin.sh`; why, when any of it fails.
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
    let mut build = crate::children::command(&launcher);
    build
        .args(["--build", "symphony"])
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
/// one of the nightly's own reds ([`board::RUN`], conciliation's) is not a check's.
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
        if key == board::RUN
            || key == board::CONCILIATE
            || key.starts_with(&format!("{}-", board::CONCILIATE))
            || board::is_refresh(name)
        {
            return Err(format!(
                "--check {name:?} takes a name the nightly keeps for its own reds"
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

/// The run's own checkout: `nightly` in the project's agent worktrees folder, else in a folder
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
    normal(&folder.join("nightly"))
}

/// `--status`: the newest release, the last night and the last green run, the landings on
/// `remote`'s `branch` since the last release and what the next run waits for, and the last
/// refresh, read from the record, the local tags and the refs as last fetched; nothing is written
/// or fetched.
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
            let _ = writeln!(err, "symphony nightly: {problem}");
            return 1;
        }
    };
    let folder = record::folder(&repo.common);
    let runs = record::read(&folder);
    let _ = write!(out, "{}", record::status(&runs, &ordered, now));
    // What a night waits for before it checks main ([`regenerated`]), in a project that has any.
    let generates = symphony_board::land::Settings::of(&repo.main)
        .is_ok_and(|settings| !settings.generators.is_empty());
    if generates && let Ok(standing) = symphony_board::land::regenerate::status(&at_main(repo)) {
        let _ = writeln!(out, "regenerator: {}", standing.line);
    }
    let every = repo.project.releases.every_landings;
    match cadence::of(&repo.root, &folder, &ordered, every, remote, branch) {
        Ok(cadence) => {
            let _ = write!(out, "{}", cadence.lines());
        }
        Err(problem) => {
            let _ = writeln!(out, "landings: cannot be counted: {problem}");
        }
    }
    let _ = write!(out, "{}", record::refresh_status(&runs, now));
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

/// Where the cadence of `repo` stands on `remote`'s `branch`.
fn cadence_of(repo: &Repo, remote: &str, branch: &str) -> Result<cadence::Cadence, String> {
    let ordered = ordered_releases(&repo.root)?;
    cadence::of(
        &repo.root,
        &record::folder(&repo.common),
        &ordered,
        repo.project.releases.every_landings,
        remote,
        branch,
    )
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

/// A night, or an out-of-cycle release: its exit code.
fn run(repo: &Repo, options: &Options, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let folder = record::folder(&repo.common);
    if let Err(e) = fs::create_dir_all(&folder) {
        let _ = writeln!(
            err,
            "symphony nightly: {}",
            crate::text::os_error(&e, &folder)
        );
        return 1;
    }
    let lock_path = folder.join("run.lock");
    let opened = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path);
    let lock = match opened {
        Ok(lock) => lock,
        Err(e) => {
            let _ = writeln!(
                err,
                "symphony nightly: {}",
                crate::text::os_error(&e, &lock_path)
            );
            return 1;
        }
    };
    let started = Instant::now();
    let when = local(options.now);
    let mut record = Run {
        version: 1,
        id: format!(
            "{}-{}",
            when.replace(' ', "T").replace(':', ""),
            std::process::id()
        ),
        kind: if options.release.is_some() {
            "release"
        } else if options.refresh {
            "refresh"
        } else {
            "nightly"
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
        skipped: Vec::new(),
        tag: None,
        pushed: false,
        board: record::Board::default(),
        remote: options.remote.clone(),
        unpushed: None,
        cause: options.cause.clone(),
        refreshed: None,
    };
    // HUP, INT and TERM stop the run, its check's group and its git, and let it say so and release
    // the lock; a signal this process was started ignoring stays ignored.
    let interrupts = Interrupts::listen().ok();
    let heard = || interrupts.as_ref().is_some_and(Interrupts::received);
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return held(repo, options, &folder, record, out, err),
        Err(TryLockError::Error(e)) => {
            let _ = writeln!(
                err,
                "symphony nightly: {}",
                crate::text::os_error(&e, &lock_path)
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
        id: record.id.clone(),
        kind: record.kind.clone(),
        pid: std::process::id(),
        started_at: options.now,
        until: options.now + options.timeout.as_secs_f64(),
        check: None,
    };
    let _ = record::hold(&folder, &running);
    // A run that ended without recording itself (killed, or the machine stopped) is recorded first,
    // so its logs stay with it and `--status` names it.
    for found in record::recover(&folder, earlier.as_ref(), &record.id, options.now) {
        let _ = writeln!(
            out,
            "nightly: recorded the run {} (started {}) as interrupted: it {}",
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
        bound: Bound::hearing(started + options.timeout, &heard),
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
    let ran = if options.refresh {
        refresh_only(&refreshing, &mut record, out)
    } else {
        night(repo, options, &folder, &mut record, out, within)
    };
    // A green release, tagged and pushed, refreshes the machine (docs/RELEASES.md).
    if ran.is_ok()
        && !options.refresh
        && let Some(tag) = record.tag.clone()
    {
        if !repo.project.releases.refresh {
            let _ = writeln!(out, "nightly: refresh: off ([releases] refresh = false)");
        } else if !record.pushed {
            let _ = writeln!(
                out,
                "nightly: refresh: {tag} is not pushed, so no project can take it: nothing is \
                 refreshed"
            );
        } else {
            let commit = record.commit.clone();
            record.refreshed = Some(refreshing.to(&record.id, &tag, &commit, out));
        }
    }
    record.ended_at = options.now + started.elapsed().as_secs_f64();
    let mut for_owner = false;
    let cleared = match ran {
        Err(Stop::Refused(why)) => {
            let _ = writeln!(err, "symphony nightly: {why}");
            record::release(&folder, &record.id);
            return 1;
        }
        Err(Stop::Error(why)) => {
            record.result = "error".into();
            record.error = Some(why);
            Vec::new()
        }
        Err(Stop::InUse(why)) => {
            record.result = "error".into();
            record.error = Some(why);
            for_owner = true;
            Vec::new()
        }
        Ok(cleared) => cleared,
    };
    let mut code = 0;
    if record.result != "green" && record.result != "unchanged" {
        code = 1;
    }
    if let Some(error) = &record.error {
        let _ = writeln!(err, "symphony nightly: {error}");
        code = 1;
    }
    if record
        .refreshed
        .as_ref()
        .is_some_and(|refresh| !refresh.green())
    {
        code = 1;
    }
    record.board = tell(repo, options, &folder, &record, &cleared, for_owner);
    code = code.max(said(&record.board, out, err));
    if let Err(problem) = record::write(&folder, &record) {
        let _ = writeln!(err, "symphony nightly: cannot record the run: {problem}");
        code = 1;
    }
    record::prune(&folder, record::KEPT_RUNS, record::KEPT_LOGS);
    record::release(&folder, &record.id);
    drop(lock);
    code
}

/// What a release's refresh runs in: the run's repository, options and record folder, what its
/// lock says, when it started and what tells it to stop.
struct Refreshing<'a> {
    repo: &'a Repo,
    options: &'a Options,
    folder: &'a Path,
    running: &'a Running,
    started: Instant,
    heard: &'a dyn Fn() -> bool,
}

impl Refreshing<'_> {
    /// The machine refreshed to the release `tag` (`commit`) by the run `id`, within [`REFRESH`]
    /// from now, which the lock's word is made to cover, so no later run takes it for stuck.
    fn to(&self, id: &str, tag: &str, commit: &str, out: &mut dyn Write) -> record::Refresh {
        let until = self.options.now + self.started.elapsed().as_secs_f64() + REFRESH.as_secs_f64();
        let _ = record::hold(
            self.folder,
            &Running {
                until: until.max(self.running.until),
                check: None,
                ..self.running.clone()
            },
        );
        let name = (self.options.worktree.file_name())
            .map_or_else(|| "nightly".into(), |name| name.to_string_lossy());
        let trees = place::real(
            &self
                .options
                .worktree
                .with_file_name(format!("{name}-refresh")),
        );
        let logs = record::logs(self.folder, id);
        let surreal = symphony_board::server::Home::in_common(&self.repo.common, None)
            .surreal()
            .to_path_buf();
        let against = refresh::Against {
            root: &self.repo.root,
            ours: &self.repo.common,
            main: &self.repo.main,
            tag,
            commit,
            trees: &trees,
            logs: &logs,
            surreal: Some(&surreal),
            until: Bound::hearing(Instant::now() + REFRESH, self.heard),
        };
        refresh::release(&against, out)
    }
}

/// `--refresh`: the machine refreshed again to the newest release tagged here, checking nothing,
/// into `record`.
fn refresh_only(
    refreshing: &Refreshing<'_>,
    record: &mut Run,
    out: &mut dyn Write,
) -> Result<Vec<&'static str>, Stop> {
    let ordered = ordered_releases(&refreshing.repo.root).map_err(Stop::Error)?;
    let Some((release, commit)) = ordered.first() else {
        return Err(Stop::Refused(
            "no release is tagged here, so there is nothing to refresh to".into(),
        ));
    };
    let tag = release.name();
    record.commit.clone_from(commit);
    record.tip.clone_from(commit);
    let _ = writeln!(out, "nightly: refresh to {tag} ({})", short(commit));
    let refreshed = refreshing.to(&record.id, &tag, commit, out);
    record.result = if refreshed.green() { "green" } else { "red" }.into();
    record.refreshed = Some(refreshed);
    Ok(Vec::new())
}

/// What the run did on the board, said: 1 when the board refused something, else 0.
fn said(board: &record::Board, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    for (what, slugs) in [
        ("filed", &board.filed),
        ("updated", &board.updated),
        ("closed", &board.closed),
    ] {
        if !slugs.is_empty() {
            let _ = writeln!(out, "nightly: {what} {}", slugs.join(", "));
        }
    }
    match &board.error {
        Some(error) => {
            let _ = writeln!(err, "symphony nightly: the board: {error}");
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
        std::thread::sleep(group::POLL);
        holder = living();
    }
    if holder.is_none()
        && let Some(earlier) = record::holder(folder)
        && let Some(check) = &earlier.check
        && group::lives(check.group, check.start.as_ref())
    {
        let _ = writeln!(err, "symphony nightly: {}", runs_on(&earlier, check));
        return 1;
    }
    let lock = folder.join("run.lock");
    let why = match holder {
        Some(holder) if options.now <= holder.until + LATE_SECONDS => {
            let _ = writeln!(
                err,
                "symphony nightly: another nightly or release of this repository is running \
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
    let _ = writeln!(err, "symphony nightly: {why}");
    record.error = Some(why);
    record.board = tell(repo, options, folder, &record, &[], false);
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
        "another nightly or release of this repository is running (run {}, process {}, since {}): \
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
        let _ = writeln!(err, "symphony nightly: {}", runs_on(earlier, check));
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
    let _ = writeln!(err, "symphony nightly: {why}");
    record.error = Some(why);
    record.board = tell(repo, options, folder, &record, &[], false);
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
    (run.checks.iter().chain(refreshed)).any(|ran| board::key(&ran.name) == key && !ran.green())
}

/// Whether this run (`record`), green in `key`, closes that key's open red: its candidate holds the
/// commit the newest of the `earlier` runs red in it was on. With no such run recorded, only a
/// night closes it, never an out-of-cycle release.
fn closes(root: &Path, earlier: &[Run], record: &Run, key: &str) -> bool {
    match earlier.iter().rev().find(|run| red_in(run, key)) {
        None => record.kind == "nightly",
        Some(red) if red.commit.is_empty() => true,
        Some(red) => {
            !record.commit.is_empty()
                && is_ancestor(root, &red.commit, &record.commit).unwrap_or(false)
        }
    }
}

/// Tell the board of `record`: its reds filed or updated, the reds it found green closed, and the
/// red [`board::RUN`] when it stopped short, with the keys it `cleared` beyond its checks; that red
/// is the owner's to clear when it is `for_owner` (a process in the run's folders).
fn tell(
    repo: &Repo,
    options: &Options,
    folder: &Path,
    record: &Run,
    cleared: &[&str],
    for_owner: bool,
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
        cleared,
        closes: &closing,
        whole: !options.refresh,
    };
    let board = crate::taskcli::CliBoard::new(&repo.main);
    let mut steps = record.checks.clone();
    steps.extend((record.refreshed.iter()).flat_map(|refresh| refresh.steps.iter().cloned()));
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

/// The run itself, into `record`, `within` the run's bound: the keys it cleared beyond its
/// checks.
#[expect(
    clippy::too_many_lines,
    reason = "one run, step by step, reads best in one place"
)]
fn night(
    repo: &Repo,
    options: &Options,
    folder: &Path,
    record: &mut Run,
    out: &mut dyn Write,
    within: Within<'_>,
) -> Result<Vec<&'static str>, Stop> {
    let until = within.bound;
    let mut place =
        place::place(&repo.root, &repo.main, &repo.common, &options.worktree).map_err(|why| {
            if options.named {
                Stop::Refused(why)
            } else {
                Stop::Error(why)
            }
        })?;
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
    let mut candidate = match &options.release {
        None => tip.clone(),
        Some(rev) => {
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
        if options.release.is_some() {
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
            "nightly: {main_line} is {} already ({}): nothing new to check",
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
        return Ok(Vec::new());
    }
    if let Some(newest) = newest
        && !is_ancestor(&repo.root, &newest.commit, &candidate).map_err(Stop::Error)?
    {
        let named = options.release.as_deref().unwrap_or(&main_line);
        let why = format!(
            "{named} ({}) is not a descendant of the newest release {} ({}): a release only \
             moves forward",
            short(&candidate),
            newest.name,
            short(&newest.commit)
        );
        return Err(if options.release.is_some() {
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
    if options.release.is_none() {
        regenerated(repo, options, &mut candidate, record, until, out)?;
    }
    let _ = writeln!(
        out,
        "nightly: {} of {} ({main_line} at {}) in {}",
        record.kind,
        short(&candidate),
        short(&record.tip),
        place.checkout.display()
    );
    unused(&place, until)?;
    let noted = place
        .checkout(&repo.root, &repo.common, &candidate, until, options.now)
        .map_err(Stop::Error)?;
    for noted in noted {
        let _ = writeln!(out, "nightly: {noted}");
    }
    let logs = record::logs(folder, &record.id);
    fs::create_dir_all(&logs).map_err(|e| Stop::Error(crate::text::os_error(&e, &logs)))?;
    record::so_far(&logs, record);
    let log = |step: &Step| logs.join(format!("{}.log", board::key(&step.name)));
    for step in &options.checks {
        told_to_stop(until, &format!("before `{}` started", step.name))?;
        // What the run's time leaves out is red, never passed over: a release is checked whole.
        let ran = if Instant::now() >= until.until {
            Ran::never(step, &log(step), NOT_STARTED.into())
        } else {
            checks::run(step, &place.checkout, &log(step), within)
        };
        tell_check(&ran, out);
        stopped_in(&ran)?;
        look(
            &[Watched::checkout(&place, &candidate)],
            until,
            &after(&ran),
            true,
        )?;
        record.checks.push(ran);
        record::so_far(&logs, record);
    }
    let cleared = conciliation(repo, options, &place, &logs, record, out, within)?;
    for skipped in &record.skipped {
        let _ = writeln!(out, "nightly: left out: {skipped}");
    }
    let green = record.checks.iter().all(Ran::green) && !record.checks.is_empty();
    record.result = if green { "green" } else { "red" }.into();
    if !green {
        let red: Vec<&str> = (record.checks.iter())
            .filter(|ran| !ran.green())
            .map(|ran| ran.name.as_str())
            .collect();
        let _ = writeln!(out, "nightly: red: {}; no release", red.join(", "));
        return Ok(cleared);
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
        return Ok(cleared);
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
    look(
        &[Watched::checkout(&place, &candidate)],
        until,
        "before it tagged",
        false,
    )?;
    let checked: Vec<String> = record.checks.iter().map(|ran| ran.name.clone()).collect();
    let message = notes::message(
        &name,
        &candidate,
        notes::facts(&repo.root, &candidate),
        previous.as_ref(),
        &checked,
        &record.skipped,
        &changes,
    );
    if let Err(problem) = git_until(
        &repo.root,
        &["tag", "-a", &name, &candidate, "-m", &message],
        until,
    ) {
        record.error = Some(format!("cannot tag {name}: {problem}"));
        return Ok(cleared);
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
    let _ = writeln!(out, "nightly: green; tagged {name}{pushed}");
    Ok(cleared)
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
/// makes them after it (`just tasks regenerate --status`), when the night's `candidate` is the
/// main checkout's branch: the run waits, within its time, while a regenerator makes them, and
/// releases nothing of a main whose committed outputs are behind its sources (its `just
/// client-check` would be red), filing nothing, since the regenerator's own red is the task then.
/// A repository with no generators, or none the regenerator reads, has nothing to wait for.
///
/// What the regenerator last wrote refuses the night at once only when it is of the candidate
/// (red or stopped on it, or ended there without a word). A land starts the regenerator just
/// before the `[board] on_land` that starts a night of the cadence, so one may not have written
/// anything yet, its state of an earlier tip or none at all: while nothing makes them and nothing
/// was written of the candidate, the night gives it [`REGENERATOR`] (the tests'
/// [`REGENERATOR_SECONDS`]) to take the candidate before it refuses.
///
/// Main moving on in the main checkout while the night waits (the regenerator landed what it made,
/// or a land came in) is followed: `remote`'s branch fetched again, what it moved to, a descendant
/// of the candidate, is the night's `candidate` from then on, in its `record` too, and waited for
/// in its turn. A move that does not reach the remote within [`REGENERATOR`] leaves the candidate
/// as it was, whose own check says.
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
        std::thread::sleep(REGENERATING.min(until.until.saturating_duration_since(Instant::now())));
    };
    // Whether the main checkout's branch was the candidate at a look: from then on its moves are
    // the night's to follow.
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
                // Its standing is of another commit than the one the night checks, whose own
                // `just client-check` says.
                return Ok(());
            }
            if let Some(tip) = moved_on(repo, remote, branch, candidate, until)? {
                let _ = writeln!(
                    out,
                    "nightly: {remote}/{branch} moved on to {} while the night waited for the \
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
                let _ = writeln!(
                    out,
                    "nightly: {branch} moved on to {} here, but {remote}/{branch} is still at {}: \
                     that is what it checks",
                    local.as_deref().map_or("nothing", short),
                    short(candidate)
                );
                return Ok(());
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
                    "nightly: the regenerator's status: {why}; not waited for"
                );
                return Ok(());
            }
        };
        if standing.current {
            if said {
                let _ = writeln!(out, "nightly: {}", standing.line);
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
            let _ = writeln!(out, "nightly: {waiting}: {}", standing.line);
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
/// use and by what, with the way out. None when no process is in any.
fn in_use(trees: &[&Path], found: &[Vec<group::Inside>]) -> Option<(String, String)> {
    let mut said = Vec::new();
    let mut processes = 0;
    for (tree, inside) in trees.iter().zip(found) {
        if inside.is_empty() {
            continue;
        }
        processes += inside.len();
        let named: Vec<String> = inside.iter().map(group::Inside::said).collect();
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

/// Stop the run before it checks anything when a process, anyone's, works in its checkout or in its
/// conciliation copies' folder or holds a file inside one open, within `until`: those folders
/// would change under its checks.
fn unused(place: &Place, until: Bound<'_>) -> Result<(), Stop> {
    let trees: Vec<&Path> = [place.checkout.as_path(), place.copies.as_path()]
        .into_iter()
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

/// The point after the check `ran`, said: its name and how it ended.
fn after(ran: &Ran) -> String {
    let verdict = if ran.green() { "green" } else { "red" };
    format!("after `{}` ended {verdict} ({})", ran.name, ran.ended())
}

/// A folder a look looks at: a checkout (real path) that must hold the commit `holds`, but for the
/// files `beside` names, which what ran there rewrites by design.
struct Watched<'a> {
    path: &'a Path,
    holds: &'a str,
    beside: &'a [&'a str],
}

impl<'a> Watched<'a> {
    /// The run's checkout, which must hold `candidate`.
    fn checkout(place: &'a Place, candidate: &'a str) -> Watched<'a> {
        Watched {
            path: &place.checkout,
            holds: candidate,
            beside: &[],
        }
    }
}

/// Stop the run when what it looks at changed `at` that point: a checkout that no longer holds its
/// commit (HEAD moved, or `git status` names a path outside `.target/`), or, when it lists
/// `processes`, a process that works in one or holds a file inside one open. What was checked is
/// then not what would be tagged, so that check counts for nothing, green or red. The look has a
/// minute past `until`, and sees that moment alone and nothing git ignores.
fn look(watched: &[Watched<'_>], until: Bound<'_>, at: &str, processes: bool) -> Result<(), Stop> {
    let until = until.later(LOOK);
    let mut changes = Vec::new();
    for tree in watched {
        let path = tree.path.display();
        let holds = short(tree.holds);
        let changed = place::changed(tree.path, tree.holds, tree.beside, until).map_err(|e| {
            Stop::Error(format!(
                "{at}, cannot tell whether {path} still holds {holds}: {e}"
            ))
        })?;
        if !changed.is_empty() {
            changes.push(format!(
                "{path} no longer holds {holds}: {}",
                changed.join(", ")
            ));
        }
    }
    let trees: Vec<&Path> = watched.iter().map(|tree| tree.path).collect();
    let found = if processes {
        group::inside(&trees, until).map_err(|problem| {
            let named: Vec<String> = trees
                .iter()
                .map(|tree| tree.display().to_string())
                .collect();
            Stop::Error(format!(
                "{at}, cannot tell whether a process is in {}: {problem}",
                named.join(" or ")
            ))
        })?
    } else {
        Vec::new()
    };
    if let Some((said, way_out)) = in_use(&trees, &found) {
        let changed = if changes.is_empty() {
            String::new()
        } else {
            format!("; and {}", changes.join("; "))
        };
        return Err(Stop::InUse(format!(
            "{at}, {said}{changed}: what was checked may have changed, so nothing checked is \
             released; {way_out}"
        )));
    }
    if changes.is_empty() {
        return Ok(());
    }
    Err(Stop::Error(format!(
        "{at}, {}; a process may be writing there, so nothing checked is released",
        changes.join("; ")
    )))
}

/// A check's end, said.
fn tell_check(ran: &Ran, out: &mut dyn Write) {
    let verdict = if ran.green() {
        "green"
    } else if ran.interrupted {
        "stopped"
    } else {
        "red"
    };
    let _ = writeln!(
        out,
        "nightly: {} {verdict} ({}, {:.0} s); log {}",
        ran.name,
        ran.ended(),
        ran.seconds,
        ran.log.display()
    );
}

/// Conciliate the projects the machine's registry names into `record`, each in its copy in
/// `place`, with a machine folder of the run's own under `logs`: the keys it cleared. A registry
/// that does not read is the red check [`board::CONCILIATE`], never a silent skip. A run told to
/// stop stops here.
fn conciliation(
    repo: &Repo,
    options: &Options,
    place: &Place,
    logs: &Path,
    record: &mut Run,
    out: &mut dyn Write,
    within: Within<'_>,
) -> Result<Vec<&'static str>, Stop> {
    let unread = |record: &mut Run, out: &mut dyn Write, problem: String| {
        let step = Step::shell(board::CONCILIATE, "(read the machine's registry)");
        let log = logs.join(format!("{}.log", board::CONCILIATE));
        let ran = Ran::never(&step, &log, problem);
        tell_check(&ran, out);
        record.checks.push(ran);
        Ok(Vec::new())
    };
    let registry = crate::machine::dir().and_then(|dir| crate::machine::Registry::read(&dir));
    let registry = match registry {
        Ok(registry) => registry,
        Err(problem) => {
            let problem = format!(
                "the machine's registry cannot be read, so nothing was conciliated: {problem}"
            );
            return unread(record, out, problem);
        }
    };
    let machine = logs.join("machine");
    if let Err(problem) = registry.write(&machine) {
        let problem = format!("cannot copy the machine's registry for conciliation: {problem}");
        return unread(record, out, problem);
    }
    let ids: Vec<String> = registry.projects.iter().map(|p| p.id.clone()).collect();
    for removed in place::prune_copies(&place.copies, &ids) {
        let _ = writeln!(out, "nightly: {removed}");
    }
    let (consumers, skipped) = conciliate::consumers(&registry, &repo.common);
    record.skipped.extend(skipped);
    for consumer in consumers {
        let name = format!("conciliate-{}", consumer.id);
        let plain = !consumer.id.is_empty()
            && consumer.id != "."
            && consumer.id != ".."
            && !consumer.id.contains('/');
        if !plain {
            record.skipped.push(format!(
                "{}: its id names no folder of its own",
                consumer.id
            ));
            continue;
        }
        told_to_stop(within.bound, &format!("before `{name}` started"))?;
        let step_log = |step: &Step| logs.join(format!("{}.log", board::key(&step.name)));
        if Instant::now() >= within.bound.until {
            let step = Step::shell(&name, "(the scratch copy and its plan)");
            let ran = Ran::never(&step, &step_log(&step), NOT_STARTED.into());
            tell_check(&ran, out);
            record.checks.push(ran);
            continue;
        }
        let scratch = place.copies.join(&consumer.id);
        let against = conciliate::Against {
            scratch: &scratch,
            checkout: &place.checkout,
            machine: &machine,
            until: within.bound,
            now: options.now,
        };
        // The copy is looked at with the checkout once it holds the project's commit; one that
        // could not be made ready may not be the nightly's, and is left as it is.
        let (ran, copy_at) = match conciliate::step(&consumer, &against) {
            Ok(prepared) => {
                for noted in prepared.noted {
                    let _ = writeln!(out, "nightly: {noted}");
                }
                match prepared.step {
                    Ok(step) => (
                        checks::run(&step, &place.checkout, &step_log(&step), within),
                        Some(prepared.at),
                    ),
                    Err(none) => {
                        record.skipped.push(none);
                        continue;
                    }
                }
            }
            Err(problem) => {
                let step = Step::shell(&name, "(the scratch copy and its plan)");
                (Ran::never(&step, &step_log(&step), problem), None)
            }
        };
        tell_check(&ran, out);
        stopped_in(&ran)?;
        told_to_stop(within.bound, &format!("while `{name}` was prepared"))?;
        let mut watched = vec![Watched::checkout(place, &record.commit)];
        if let Some(at) = &copy_at {
            watched.push(Watched {
                path: &scratch,
                holds: at,
                beside: &conciliate::REWRITTEN,
            });
        }
        look(&watched, within.bound, &after(&ran), true)?;
        record.checks.push(ran);
        record::so_far(logs, record);
    }
    Ok(vec![board::CONCILIATE])
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
            (&["Run=true"][..], "the nightly keeps for its own reds"),
            (
                &["conciliate=true"][..],
                "the nightly keeps for its own reds",
            ),
            (
                &["conciliate star=true"][..],
                "the nightly keeps for its own reds",
            ),
        ] {
            let said = checks_of(bad.iter().copied()).unwrap_err();
            assert!(said.contains(why), "{bad:?}: {said}");
        }
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
            tree.path("plain-wt/nightly")
        );
        tree.write(
            "named/symphony.toml",
            "[project]\nid = \"named\"\n\n[agents]\nworktrees = \"../elsewhere\"\n",
        );
        assert_eq!(
            default_worktree(&repo(&tree.path("named"))),
            tree.path("elsewhere/nightly")
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
        let trees = [
            Path::new("/wt/nightly"),
            Path::new("/wt/nightly-conciliate"),
        ];
        assert_eq!(in_use(&trees, &[vec![], vec![]]), None);
        let counts = " (a shell, a terminal tab, an editor or a Finder window there counts), and the \
                      next run goes";
        assert_eq!(
            in_use(&trees, &[vec![inside(10, "zsh", "working there")], vec![]]),
            Some((
                "/wt/nightly is in use by process 10 (zsh), working there".into(),
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
                "/wt/nightly is in use by process 10 (zsh), working there; process 11 (Finder), \
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
                "/wt/nightly is in use by process 10 (zsh), working there; \
                 /wt/nightly-conciliate is in use by process 12 (cargo), working in consumer"
                    .into(),
                format!("end them, or move them out of those folders{counts}")
            ))
        );
    }

    fn run_of(id: &str, kind: &str, commit: &str) -> Run {
        Run {
            version: 1,
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
            skipped: Vec::new(),
            tag: None,
            pushed: false,
            board: record::Board::default(),
            remote: "origin".into(),
            unpushed: None,
            cause: None,
            refreshed: None,
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
            error: None,
            timed_out: false,
            interrupted: false,
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
        let mut red_night = run_of("a", "nightly", &reddened);
        red_night.checks = vec![red("verify")];
        let mut stopped = run_of("b", "nightly", "");
        stopped.error = Some("cannot fetch".into());
        let earlier = [red_night, stopped];

        let release_of = |commit: &str| run_of("c", "release", commit);
        assert!(!closes(&root, &earlier, &release_of(&older), "verify"));
        assert!(closes(&root, &earlier, &release_of(&reddened), "verify"));
        assert!(closes(&root, &earlier, &release_of(&fixed), "verify"));
        assert!(
            closes(&root, &earlier, &release_of(&older), board::RUN),
            "a stop before any candidate is cleared by any run that gets through"
        );
        assert!(
            closes(&root, &earlier, &run_of("d", "nightly", &older), "lint"),
            "a red no run recorded: a night closes it"
        );
        assert!(
            !closes(&root, &earlier, &release_of(&fixed), "lint"),
            "an out-of-cycle release does not"
        );
    }
}
