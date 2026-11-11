//! `symphony nightly` (`just nightly`): Symphony's releases (docs/RELEASES.md). A run takes main's
//! tip as fetched from the remote, the commit a project can fetch, into a clean checkout of its own
//! outside the repository's folder, runs the full verification there and conciliates the projects
//! the machine serves against it ([`conciliate`]). Every red check is a task on the board
//! ([`board`]); when every check is green it tags the commit as the next release
//! (`release-YYYY-MM-DD`, [`tags`]) with what it holds ([`notes`]) and pushes that tag alone.
//! `--release COMMIT` does the same out of cycle for a commit already on the remote's main.
//! Each run is recorded in the git common dir ([`record`]), which `--status` reads; `--schedule`
//! prints the entry that runs it each night ([`schedule`]), which the owner installs.
//!
//! It never tags a commit that is not on the remote's main or that a check found red, never moves
//! or deletes a tag, and pushes nothing but its new tag. One run at a time per repository.

mod board;
mod checks;
mod conciliate;
mod git;
mod notes;
mod record;
mod schedule;
mod tags;

use std::fs::{self, File, TryLockError};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use checks::{Ran, Step};
use git::{commit, git, is_ancestor, short};
use record::{Run, local};
use tags::Release;

use crate::cli::{Flag, parse, refuse};
use crate::project::Project;

const USAGE: &str = "usage: symphony nightly [-h] [--root ROOT] [--remote NAME] [--branch NAME] \
[--worktree PATH] [--check NAME=COMMAND] [--release COMMIT] [--no-push] [--now SECONDS] [--status] \
[--schedule] [--at HH:MM]";
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
  --no-push             tag the release here, push nothing
  --now SECONDS         the time the run takes as now, seconds since the epoch (default: now)
  --status              print the newest release, the last night and its age; run nothing
  --schedule            print the schedule entry that runs `just nightly` each day; install nothing
  --at HH:MM            the time of day --schedule names (default: 03:00)";

/// The checks a night runs unless told others: the full verification.
const VERIFY: (&str, &str) = ("verify", "just verify");

/// What a run is asked to do.
struct Options {
    remote: String,
    branch: String,
    worktree: PathBuf,
    checks: Vec<Step>,
    release: Option<String>,
    push: bool,
    now: f64,
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
        flag("--now", true),
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
    let now = match parsed.value("--now") {
        Some(now) => match now.parse::<f64>() {
            Ok(now) if now.is_finite() && now >= 0.0 => now,
            _ => {
                return refuse(
                    err,
                    USAGE,
                    "nightly",
                    &format!("--now {now:?} is not a time"),
                );
            }
        },
        None => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64()),
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
    if worktree.starts_with(&repo.main) || worktree.starts_with(&repo.root) {
        let _ = writeln!(
            err,
            "symphony nightly: its checkout {} must be outside the repository's folder {}",
            worktree.display(),
            repo.main.display()
        );
        return 1;
    }
    let options = Options {
        remote: parsed.value("--remote").unwrap_or("origin").to_string(),
        branch: parsed
            .value("--branch")
            .map_or_else(|| repo.project.default_branch.clone(), str::to_string),
        worktree,
        checks,
        release: parsed.value("--release").map(str::to_string),
        push: !parsed.flag("--no-push"),
        now,
    };
    run(&repo, &options, out, err)
}

/// The checks `--check NAME=COMMAND` names, in order; [`VERIFY`] when none does.
fn checks_of<'a>(given: impl Iterator<Item = &'a str>) -> Result<Vec<Step>, String> {
    let mut checks: Vec<Step> = Vec::new();
    for check in given {
        let Some((name, command)) = check.split_once('=') else {
            return Err(format!("--check {check:?} is not NAME=COMMAND"));
        };
        if name.trim().is_empty() || command.trim().is_empty() {
            return Err(format!("--check {check:?} names no check or no command"));
        }
        if checks
            .iter()
            .any(|known| board::key(&known.name) == board::key(name))
        {
            return Err(format!("--check {name:?} is named twice"));
        }
        checks.push(Step::shell(name, command));
    }
    if checks.is_empty() {
        checks.push(Step::shell(VERIFY.0, VERIFY.1));
    }
    Ok(checks)
}

/// `path` with its `.` and `NAME/..` parts folded, as written: no link is followed.
fn normal(path: &Path) -> PathBuf {
    let mut folded = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                folded.pop();
            }
            other => folded.push(other),
        }
    }
    folded
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
    /// It could not get as far as its checks; recorded as an error.
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
    let lock = match File::create(&lock_path) {
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
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            let _ = writeln!(
                err,
                "symphony nightly: another nightly or release of this repository is running"
            );
            return 1;
        }
        Err(TryLockError::Error(e)) => {
            let _ = writeln!(
                err,
                "symphony nightly: {}",
                crate::text::os_error(&e, &lock_path)
            );
            return 1;
        }
    }
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
    };
    let ran = night(repo, options, &folder, &mut record, out);
    record.ended_at = options.now + started.elapsed().as_secs_f64();
    let mut code = 0;
    match ran {
        Err(Stop::Refused(why)) => {
            let _ = writeln!(err, "symphony nightly: {why}");
            return 1;
        }
        Err(Stop::Error(why)) => {
            let _ = writeln!(err, "symphony nightly: {why}");
            record.result = "error".into();
            record.error = Some(why);
            code = 1;
        }
        Ok(()) => {
            if record.result != "green" && record.result != "unchanged" {
                code = 1;
            }
            if let Some(error) = &record.error {
                let _ = writeln!(err, "symphony nightly: {error}");
                code = 1;
            }
            if let Some(error) = &record.board.error {
                let _ = writeln!(err, "symphony nightly: the board: {error}");
                code = 1;
            }
        }
    }
    if let Err(problem) = record::write(&folder, &record) {
        let _ = writeln!(err, "symphony nightly: cannot record the run: {problem}");
        code = 1;
    }
    record::prune(&folder, record::KEPT_RUNS, record::KEPT_LOGS);
    drop(lock);
    code
}

/// The release tags here and on the remote, each with its commit: a remote's tag wins a name both
/// hold.
fn releases(repo: &Repo, remote: &str) -> Result<Vec<(String, String)>, String> {
    let pattern = format!("refs/tags/{}*", tags::PREFIX);
    let listed = git(&repo.root, &["ls-remote", "--tags", remote, &pattern])?;
    let mut all = git::remote_tags(&listed);
    for (name, commit) in local_releases(&repo.root)? {
        if !all.iter().any(|(known, _)| *known == name) {
            all.push((name, commit));
        }
    }
    all.retain(|(name, _)| Release::parse(name).is_some());
    Ok(all)
}

/// Bring the run's checkout at `path` to the commit `to`, clean: made the first time, as a detached
/// worktree locked against sweeps; later reset, every untracked and ignored file gone but its
/// build folder (`.target/`), which keeps a night's build incremental.
fn checkout(repo: &Repo, path: &Path, to: &str) -> Result<(), String> {
    let ours = |path: &Path| {
        git(
            path,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
        .ok()
        .map(PathBuf::from)
        .and_then(|common| common.canonicalize().ok())
        .is_some_and(|common| common == repo.common)
    };
    if path.exists() {
        if !ours(path) || !path.join(".git").is_file() {
            return Err(format!(
                "{} is not a worktree of this repository; move it away",
                path.display()
            ));
        }
        git(path, &["checkout", "--quiet", "--force", "--detach", to])?;
        git(path, &["clean", "-ffdxq", "-e", "/.target/"])?;
    } else {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| crate::text::os_error(&e, parent))?;
        }
        let at = path.to_string_lossy();
        git(
            &repo.root,
            &[
                "worktree", "add", "--quiet", "-f", "-f", "--detach", &at, to,
            ],
        )?;
        let _ = git(
            &repo.root,
            &[
                "worktree",
                "lock",
                "--reason",
                "the Symphony nightly's checkout (docs/RELEASES.md)",
                &at,
            ],
        );
    }
    let head = commit(path, "HEAD")?;
    let dirty = git(path, &["status", "--porcelain"])?;
    if head != to || !dirty.is_empty() {
        return Err(format!(
            "{} is not a clean checkout of {}: {}",
            path.display(),
            short(to),
            if dirty.is_empty() { &head } else { &dirty }
        ));
    }
    Ok(())
}

/// The run itself, into `record`.
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
) -> Result<(), Stop> {
    let remote = options.remote.as_str();
    let tracking = format!("refs/remotes/{remote}/{}", options.branch);
    git(
        &repo.root,
        &[
            "fetch",
            "--quiet",
            "--no-tags",
            remote,
            &format!("+refs/heads/{}:{tracking}", options.branch),
        ],
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
    let releases = releases(repo, remote).map_err(Stop::Error)?;
    let names = || releases.iter().map(|(name, _)| name.as_str());
    let newest = tags::newest_first(names());
    if let Some(release) = newest.iter().find(|release| {
        releases
            .iter()
            .any(|(n, c)| *n == release.name() && *c == candidate)
    }) {
        if options.release.is_some() {
            return Err(Stop::Refused(format!(
                "{} is {} already",
                short(&candidate),
                release.name()
            )));
        }
        let _ = writeln!(
            out,
            "nightly: {main_line} is {} already ({}): nothing new to check",
            release.name(),
            short(&candidate)
        );
        record.result = "unchanged".into();
        return Ok(());
    }
    let mut previous = None;
    for release in &newest {
        let name = release.name();
        let Some((_, at)) = releases.iter().find(|(tag, _)| *tag == name) else {
            continue;
        };
        if is_ancestor(&repo.root, at, &candidate).unwrap_or(false) {
            previous = Some(notes::Previous {
                facts: notes::facts(&repo.root, at),
                tag: name,
                commit: at.clone(),
            });
            break;
        }
    }
    record.previous = previous.as_ref().map(|p| p.tag.clone());
    let _ = writeln!(
        out,
        "nightly: {} of {} ({main_line} at {}) in {}",
        record.kind,
        short(&candidate),
        short(&record.tip),
        options.worktree.display()
    );
    checkout(repo, &options.worktree, &candidate).map_err(Stop::Error)?;
    let logs = record::logs(folder, &record.id);
    fs::create_dir_all(&logs).map_err(|e| Stop::Error(crate::text::os_error(&e, &logs)))?;
    let log = |step: &Step| logs.join(format!("{}.log", board::key(&step.name)));
    let said = |ran: &Ran, out: &mut dyn Write| {
        let verdict = if ran.green() { "green" } else { "red" };
        let _ = writeln!(
            out,
            "nightly: {} {verdict} ({}, {:.0} s); log {}",
            ran.name,
            ran.ended(),
            ran.seconds,
            ran.log.display()
        );
    };
    for step in &options.checks {
        let ran = checks::run(step, &options.worktree, &log(step));
        said(&ran, out);
        record.checks.push(ran);
    }
    let scratch = options.worktree.with_file_name(format!(
        "{}-conciliate",
        options
            .worktree
            .file_name()
            .map_or_else(|| "nightly".into(), |name| name.to_string_lossy())
    ));
    let registry = crate::machine::dir().and_then(|dir| crate::machine::Registry::read(&dir));
    match registry {
        Err(problem) => record
            .skipped
            .push(format!("no project conciliated: {problem}")),
        Ok(registry) => {
            let (consumers, skipped) = conciliate::consumers(&registry, &repo.common);
            record.skipped.extend(skipped);
            for consumer in consumers {
                let name = format!("conciliate-{}", consumer.id);
                let place = scratch.join(&consumer.id);
                let ran = match conciliate::step(&consumer, &place, &options.worktree) {
                    Ok(Ok(step)) => checks::run(&step, &options.worktree, &log(&step)),
                    Ok(Err(skipped)) => {
                        record.skipped.push(skipped);
                        continue;
                    }
                    Err(problem) => {
                        let step = Step::shell(&name, "(the scratch copy and its plan)");
                        Ran::never(&step, &log(&step), problem)
                    }
                };
                said(&ran, out);
                record.checks.push(ran);
            }
        }
    }
    for skipped in &record.skipped {
        let _ = writeln!(out, "nightly: left out: {skipped}");
    }
    let green = record.checks.iter().all(Ran::green);
    record.result = if green { "green" } else { "red" }.into();
    if green {
        let facts = notes::facts(&repo.root, &candidate);
        let changes = match &previous {
            Some(previous) => {
                notes::changes(&repo.root, &previous.commit, &candidate).map_err(Stop::Error)?
            }
            None => Vec::new(),
        };
        let checked: Vec<String> = record.checks.iter().map(|ran| ran.name.clone()).collect();
        let name = tags::next(&when_day(options.now), names());
        let message = notes::message(
            &name,
            &candidate,
            facts,
            previous.as_ref(),
            &checked,
            &changes,
        );
        match git(
            &repo.root,
            &["tag", "-a", &name, &candidate, "-m", &message],
        ) {
            Err(problem) => record.error = Some(format!("cannot tag {name}: {problem}")),
            Ok(_) => {
                record.tag = Some(name.clone());
                if options.push {
                    match git(
                        &repo.root,
                        &["push", "--quiet", remote, &format!("refs/tags/{name}")],
                    ) {
                        Ok(_) => record.pushed = true,
                        Err(problem) => {
                            record.error = Some(format!(
                                "tagged {name} here, but the push to {remote} failed: {problem}"
                            ));
                        }
                    }
                }
                let pushed = if record.pushed {
                    format!(", pushed to {remote}")
                } else {
                    ", not pushed".into()
                };
                let _ = writeln!(out, "nightly: green; tagged {name}{pushed}");
            }
        }
    } else {
        let red: Vec<&str> = (record.checks.iter())
            .filter(|ran| !ran.green())
            .map(|ran| ran.name.as_str())
            .collect();
        let _ = writeln!(out, "nightly: red: {}; no release", red.join(", "));
    }
    let earlier = record::read(folder);
    let green_at = record::last_green(&earlier)
        .map(|run| run.commit.clone())
        .or_else(|| previous.as_ref().map(|p| p.commit.clone()));
    let since = match &green_at {
        Some(green) => git(
            &repo.root,
            &["log", "--oneline", &format!("{green}..{candidate}")],
        )
        .map(|listed| listed.lines().map(str::to_string).collect())
        .unwrap_or_default(),
        None => Vec::new(),
    };
    let when = local(options.now);
    let night = board::Night {
        id: &record.id,
        when: &when,
        commit: &candidate,
        green: green_at.as_deref(),
        since: &since,
    };
    let board = crate::taskcli::CliBoard::new(&repo.main);
    record.board = board::report(&board, &night, &record.checks);
    for (what, slugs) in [
        ("filed", &record.board.filed),
        ("updated", &record.board.updated),
        ("closed", &record.board.closed),
    ] {
        if !slugs.is_empty() {
            let _ = writeln!(out, "nightly: {what} {}", slugs.join(", "));
        }
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
        ] {
            let said = checks_of(bad.iter().copied()).unwrap_err();
            assert!(said.contains(why), "{bad:?}: {said}");
        }
    }

    #[test]
    fn a_path_is_folded_as_written() {
        assert_eq!(normal(Path::new("/a/b/../c/./d")), PathBuf::from("/a/c/d"));
        assert_eq!(
            normal(Path::new("/w/symphony/../symphony-wt/nightly")),
            PathBuf::from("/w/symphony-wt/nightly")
        );
    }

    #[test]
    fn a_day_is_the_local_date_of_a_time() {
        assert_eq!(when_day(1_800_000_000.0).len(), 10);
        assert!(tags::is_day(&when_day(1_800_000_000.0)));
    }
}
