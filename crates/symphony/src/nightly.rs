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
//! that a check found red, never moves or deletes a tag, and pushes nothing but its new tag. One
//! run at a time per repository, and none while a check of an earlier run killed while it ran still
//! runs ([`checks`]); everything it runs is bounded by the run's time and stops when the run is told
//! to (HUP, INT or TERM, [`group`]).

mod board;
mod checks;
mod conciliate;
mod git;
mod group;
mod notes;
mod place;
mod record;
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
[--status] [--schedule] [--at HH:MM]";
const HELP: &str =
    "Symphony's releases: main's tip as fetched from the remote, verified in a clean \
checkout outside the repository's folder and conciliated with the projects this machine serves; \
each red check filed on the board as tests/nightly-red-NAME, and a green commit tagged \
release-YYYY-MM-DD and pushed (docs/RELEASES.md).

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
                        check not yet started are red (default: 36000, ten hours)
  --status              print the newest release, the last night and its age; run nothing
  --schedule            print the schedule entry that runs `just nightly` each day; install nothing
  --at HH:MM            the time of day --schedule names (default: 03:00)";

/// The checks a night runs unless told others: the full verification.
const VERIFY: (&str, &str) = ("verify", "just verify");
/// The run's time unless `--timeout` says otherwise.
const RUN_SECONDS: u64 = 10 * 3600;
/// How long past its time a run may still hold the lock (stopping its checks, telling the board)
/// before a later run reports it stuck.
const LATE_SECONDS: f64 = 15.0 * 60.0;
/// What a run whose checks all ended green within its time keeps past it to tag and push: well
/// within [`LATE_SECONDS`], so no later run takes it for stuck.
const FINISH: Duration = Duration::from_secs(5 * 60);
/// Why a check, or a project's conciliation, is red without having run.
const NOT_STARTED: &str = "the run's time was up before it started";
/// The tests' clock: the seconds since the epoch a run takes as now, in place of this machine's.
/// No flag sets it, and a run on it pushes only to a remote that is a folder on this machine
/// ([`far_push`]), so a release named for another day than the machine's never leaves it.
const CLOCK: &str = "SYMPHONY_NIGHTLY_NOW";
/// How long a run that finds the lock held and no run saying so waits for the run that has just
/// taken it to say so.
const SAYS_HELD: Duration = Duration::from_secs(2);

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
    ];
    let parsed = match parse(args, &flags) {
        Ok(parsed) if parsed.help => {
            let _ = writeln!(out, "{USAGE}\n\n{HELP}");
            return 0;
        }
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
    let modes = ["--status", "--schedule", "--release"]
        .into_iter()
        .filter(|mode| parsed.flag(mode))
        .count();
    if modes > 1 {
        return refuse(
            err,
            USAGE,
            "nightly",
            "--status, --schedule and --release are one each",
        );
    }
    let repo = match repo(Path::new(parsed.value("--root").unwrap_or("."))) {
        Ok(repo) => repo,
        Err(problem) => {
            let _ = writeln!(err, "symphony nightly: {problem}");
            return 1;
        }
    };
    if parsed.flag("--status") {
        return status(&repo, now, out, err);
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
    let options = Options {
        remote: parsed.value("--remote").unwrap_or("origin").to_string(),
        branch: parsed
            .value("--branch")
            .map_or_else(|| repo.project.default_branch.clone(), str::to_string),
        worktree,
        named: parsed.value("--worktree").is_some(),
        checks,
        release: parsed.value("--release").map(str::to_string),
        push: !parsed.flag("--no-push"),
        now,
        timeout,
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
    run(&repo, &options, out, err)
}

/// The first place `remote` pushes to, as the repository at `root` says, that is not a folder on
/// this machine; none when every one is. A name that is no remote is the place itself.
fn far_push(root: &Path, remote: &str) -> Option<String> {
    let urls: Vec<String> = git(root, &["remote", "get-url", "--push", "--all", remote])
        .map_or_else(
            |_| vec![remote.to_string()],
            |listed| listed.lines().map(str::to_string).collect(),
        );
    urls.into_iter().find(|url| {
        let path = url.strip_prefix("file://").unwrap_or(url);
        url.contains("::") || path.is_empty() || !root.join(path).is_dir()
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

/// `--status`: the newest release, the last night and the last green run, read from the record
/// and the local tags; nothing is written or fetched.
fn status(repo: &Repo, now: f64, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let releases = match local_releases(&repo.root) {
        Ok(releases) => releases,
        Err(problem) => {
            let _ = writeln!(err, "symphony nightly: {problem}");
            return 1;
        }
    };
    let mut ordered: Vec<(Release, String)> = Vec::new();
    for release in tags::newest_first(releases.iter().map(|(name, _)| name.as_str())) {
        let name = release.name();
        if let Some((_, commit)) = releases.iter().find(|(tag, _)| *tag == name) {
            ordered.push((release, commit.clone()));
        }
    }
    let runs = record::read(&record::folder(&repo.common));
    let _ = write!(out, "{}", record::status(&runs, &ordered, now));
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
    if let Some(earlier) = record::holder(&folder)
        && let Some(check) = &earlier.check
        && group::lives(check.group, check.start.as_ref())
    {
        return outlived(repo, options, &folder, record, &earlier, check, out, err);
    }
    let running = Running {
        id: record.id.clone(),
        pid: std::process::id(),
        started_at: options.now,
        until: options.now + options.timeout.as_secs_f64(),
        check: None,
    };
    let _ = record::hold(&folder, &running);
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
    let ran = night(repo, options, &folder, &mut record, out, within);
    record.ended_at = options.now + started.elapsed().as_secs_f64();
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
    record.board = tell(repo, options, &folder, &record, &cleared);
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
/// red [`board::RUN`], naming who holds the lock.
fn held(
    repo: &Repo,
    options: &Options,
    folder: &Path,
    mut record: Run,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    // A run that has just taken the lock says so at once.
    let said_by = Instant::now() + SAYS_HELD;
    let mut holder = record::holder(folder);
    while holder.is_none() && Instant::now() < said_by {
        std::thread::sleep(group::POLL);
        holder = record::holder(folder);
    }
    let holder = holder.filter(|holder| pid_alive(i64::from(holder.pid)));
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
    record.board = tell(repo, options, folder, &record, &[]);
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
        let _ = writeln!(
            err,
            "symphony nightly: another nightly or release of this repository is running (run {}, \
             process {}, since {}): it ended, but its check `{}` runs on in process group {}",
            earlier.id,
            earlier.pid,
            local(earlier.started_at),
            check.name,
            check.group
        );
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
    record.board = tell(repo, options, folder, &record, &[]);
    said(&record.board, out, err);
    let _ = record::write(folder, &record);
    1
}

/// Whether `run` was red in the key `key`: the run's own when it stopped short, else a check's.
fn red_in(run: &Run, key: &str) -> bool {
    if key == board::RUN {
        return run.error.is_some();
    }
    (run.checks.iter()).any(|ran| board::key(&ran.name) == key && !ran.green())
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
/// red [`board::RUN`] when it stopped short, with the keys it `cleared` beyond its checks.
fn tell(
    repo: &Repo,
    options: &Options,
    folder: &Path,
    record: &Run,
    cleared: &[&str],
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
        cleared,
        closes: &closing,
    };
    let board = crate::taskcli::CliBoard::new(&repo.main);
    board::report(&board, &night, &record.checks)
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
    let candidate = match &options.release {
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
    let _ = writeln!(
        out,
        "nightly: {} of {} ({main_line} at {}) in {}",
        record.kind,
        short(&candidate),
        short(&record.tip),
        place.checkout.display()
    );
    let noted = place
        .checkout(&repo.root, &repo.common, &candidate, until, options.now)
        .map_err(Stop::Error)?;
    for noted in noted {
        let _ = writeln!(out, "nightly: {noted}");
    }
    let logs = record::logs(folder, &record.id);
    fs::create_dir_all(&logs).map_err(|e| Stop::Error(crate::text::os_error(&e, &logs)))?;
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
        record.checks.push(ran);
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

/// A check's end, said.
fn tell_check(ran: &Ran, out: &mut dyn Write) {
    let verdict = if ran.green() { "green" } else { "red" };
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
        let ran = match conciliate::step(&consumer, &against) {
            Ok((planned, noted)) => {
                for noted in noted {
                    let _ = writeln!(out, "nightly: {noted}");
                }
                match planned {
                    Ok(step) => checks::run(&step, &place.checkout, &step_log(&step), within),
                    Err(none) => {
                        record.skipped.push(none);
                        continue;
                    }
                }
            }
            Err(problem) => {
                let step = Step::shell(&name, "(the scratch copy and its plan)");
                Ran::never(&step, &step_log(&step), problem)
            }
        };
        tell_check(&ran, out);
        stopped_in(&ran)?;
        told_to_stop(within.bound, &format!("while `{name}` was prepared"))?;
        record.checks.push(ran);
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
        git(&root, &["init", "-q"]).unwrap();
        for (name, url) in [
            ("here", here.as_str()),
            ("file", "file://bare.git"),
            ("far", "https://example.invalid/far.git"),
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
        assert_eq!(
            far_push(&root, "far").as_deref(),
            Some("https://example.invalid/far.git")
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
