//! A green release's refresh (docs/RELEASES.md), on the owner's word of 2026-11-12 ("yes, a green
//! release should refresh"): what a release does for the machine once it is tagged and pushed, or
//! once `--refresh` names it again. Each project the machine serves that builds on the Symphony
//! ([`conciliate::consumers`]) is one step, `refresh-ID`, which moves what it can and nothing more:
//!
//! - **Its pin.** While its default branch's `Cargo.lock` takes an older Symphony, one the release
//!   descends from, the move to the release ([`pin`]) is made in a worktree of the project's own
//!   beside the release run's checkout (`RELEASE-refresh/ID`, detached at that branch and locked), and
//!   committed there through the project's own hooks, its commit gate, never past them, on the
//!   branch `symphony/RELEASE`. A pin moves only forward: one neither ahead of the release nor
//!   behind it ([`Stands::Beside`]) is left as it is, its files and its board with it, and named
//!   with the release's commit; the next release that descends from it moves it.
//! - **Its main.** Then its default branch is fast-forwarded to that exact commit in its main
//!   checkout (`git merge --ff-only COMMIT`, its hooks running), when and only when the commit is
//!   the move and nothing more ([`vet`]: one commit on its tip, changing only manifests and the
//!   lock, every Symphony package at the release, read as git holds it after the hooks ran), that
//!   checkout is on its default branch at that tip and has no tracked edits, and its hooks, asked
//!   first by a ref transaction then aborted, allow it: never pushed (its own next push carries
//!   it), never a merge commit made, never a hook passed by. Otherwise the branch is left, the
//!   step is red (a task on the Symphony's board naming what finishes it), and its main stays
//!   where it was, its checkout too: what git wrote of the move before refusing it is put back
//!   path by path where it is still just that, and named where it is not ([`refused`]).
//! - **Its board.** In every refresh but one that leaves its pin beside the release, its live board
//!   is moved to the schema of the pin its main locks then, the release once its main took it or an
//!   older one it is still behind, by `just tasks migrate` in its main checkout, the one command
//!   that moves a project's board, after a snapshot of the store as it runs (`Home::export`). A
//!   board whose database is not running is refused, since nothing here starts one, and so is one
//!   the move could put on a newer SurrealDB, which would replace the running server, or whose
//!   versions cannot be read.
//!
//! Then the shared server is said. Its own reload (`apps/symphony-server/src/reload.rs`) watches the
//! checkout it is built from: once that checkout's `Cargo.lock` changes it builds and installs the
//! new server (`crates/symphony/src/reload.rs`, `cargo install`) and hands its sockets to it,
//! a failed build leaving the old one serving. When this refresh moved that checkout's main, it
//! waits, bounded, until another process answers for it, or says it has not yet. Nothing here stops,
//! signals or restarts a server or a database, and a red step leaves its project's main as it was:
//! what it moves is a branch it made and the board, to the pin that main already locks.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use symphony_api::Front;
use symphony_board::server::{Home, ServerError};

use super::board;
use super::checks::{Ran, Step};
use super::conciliate::{self, Consumer};
use super::git::{commit, git, is_ancestor, short};
use super::group::{Bound, Failed, output_until};
use super::notes;
use super::pin::{self, Locked};
use super::record::Refresh;
use crate::children::command;
use crate::machine::Registry;

/// Why a refresh's worktree of a project is locked, as `git worktree list` says it.
pub(crate) const REASON: &str = "the Symphony release run's refresh";
/// The prefix of the branch a project's pin moves on, before the release's name.
pub(crate) const BRANCH: &str = "symphony/";
/// What a refresh step's name starts with, before the project's id: no check takes it.
pub(crate) const STEP: &str = "refresh";
/// The snapshots a project's board keeps of its store before a schema move, newest first.
const KEPT_SNAPSHOTS: usize = 5;
/// The longest a refresh that moved the main the shared server is built from waits for another
/// server process to answer: its reload's release build at the machine's load.
const RELOAD: Duration = Duration::from_secs(20 * 60);
/// The tests' wait in place of [`RELOAD`], in seconds; no flag sets it.
const RELOAD_SECONDS: &str = "SYMPHONY_RELEASE_RELOAD_SECONDS";
/// How often that wait asks the server what it is.
const RELOAD_LOOK: Duration = Duration::from_millis(500);
/// The variables no command run in a project keeps: a repository git was pointed at (each command
/// names its own folder), the board, session and task a run of the Symphony's names (a project's
/// command reaches its own), the landing's marker, and the build folder and toolchain, which are
/// the project's own.
const UNSET: [&str; 13] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "TASKS_SERVER",
    "TASKS_DATABASE",
    "TASKS_SESSION",
    "TASKS_TASK",
    "TASKS_RUN_PARENT",
    "TASKS_WORK",
    "SYMPHONY_LAND",
    "CARGO_TARGET_DIR",
    "RUSTUP_TOOLCHAIN",
];

/// What a refresh runs against: the Symphony's repository (its checkout, its git common dir and
/// its main checkout), the release and its commit, where the projects' worktrees and the steps'
/// logs go, the `surreal` a snapshot falls back to, and its time.
pub(crate) struct Against<'a> {
    pub(crate) root: &'a Path,
    pub(crate) ours: &'a Path,
    pub(crate) main: &'a Path,
    pub(crate) tag: &'a str,
    pub(crate) commit: &'a str,
    pub(crate) trees: &'a Path,
    pub(crate) logs: &'a Path,
    pub(crate) surreal: Option<&'a Path>,
    pub(crate) until: Bound<'a>,
}

/// The refresh of the machine to `against`'s release, each step and line said to `out` as it goes.
pub(crate) fn release(against: &Against<'_>, out: &mut dyn Write) -> Refresh {
    let mut refresh = Refresh {
        release: against.tag.to_string(),
        steps: Vec::new(),
        said: Vec::new(),
    };
    let mut say = |refresh: &mut Refresh, line: String| {
        let _ = writeln!(out, "release: refresh: {line}");
        refresh.said.push(line);
    };
    let never = |refresh: &mut Refresh, name: &str, why: String| {
        let step = Step::shell(name, &format!("the refresh to {}", against.tag));
        let log = against.logs.join(format!("{}.log", board::key(name)));
        let _ = fs::create_dir_all(against.logs);
        refresh.steps.push(Ran::never(&step, &log, why));
    };
    let read = crate::machine::dir().and_then(|dir| Ok((Registry::read(&dir)?, dir)));
    let (registry, machine) = match read {
        Ok(read) => read,
        Err(problem) => {
            never(
                &mut refresh,
                STEP,
                format!(
                    "the machine's registry cannot be read, so nothing was refreshed: {problem}"
                ),
            );
            return told(refresh, out);
        }
    };
    let names = match pin::packages_at(against.root, against.commit) {
        Ok(names) => names,
        Err(problem) => {
            never(
                &mut refresh,
                STEP,
                format!(
                    "the release's packages cannot be read, so nothing was refreshed: {problem}"
                ),
            );
            return told(refresh, out);
        }
    };
    let (consumers, skipped) = conciliate::consumers(&registry, against.ours);
    for skipped in skipped {
        say(&mut refresh, format!("left out: {skipped}"));
    }
    // The server as it answers before any main moves, to tell its reload's build after.
    let before = crate::machine::command::running(&machine, None);
    let mut taking = Vec::new();
    for consumer in consumers {
        let plain = !consumer.id.is_empty()
            && consumer.id != "."
            && consumer.id != ".."
            && !consumer.id.contains('/');
        if !plain {
            say(
                &mut refresh,
                format!(
                    "left out: {}: its id names no folder of its own",
                    consumer.id
                ),
            );
            continue;
        }
        let name = format!("{STEP}-{}", consumer.id);
        if against.until.interrupted() {
            never(
                &mut refresh,
                &name,
                "the run was told to stop (HUP, INT or TERM) before it started".into(),
            );
            continue;
        }
        if Instant::now() >= against.until.until {
            never(
                &mut refresh,
                &name,
                "the refresh's time was up before it started".into(),
            );
            continue;
        }
        let (ran, said, taken) = step(&consumer, &name, &names, against);
        for line in said {
            say(&mut refresh, line);
        }
        if let Some(ran) = ran {
            refresh.steps.push(ran);
            taking.push((consumer, taken));
        }
    }
    let server = server(&machine, before.as_ref(), &taking, against);
    say(&mut refresh, server);
    told(refresh, out)
}

/// `refresh`, each step's end said to `out`.
fn told(refresh: Refresh, out: &mut dyn Write) -> Refresh {
    for ran in &refresh.steps {
        let verdict = if ran.green() { "green" } else { "red" };
        let why = (ran.error.as_ref()).map_or_else(String::new, |why| format!(": {why}"));
        let _ = writeln!(
            out,
            "release: {} {verdict} ({:.0} s){why}; log {}",
            ran.name,
            ran.seconds,
            ran.log.display()
        );
    }
    refresh
}

/// What one project's step did: its record (none when it takes nothing from the Symphony by
/// git), what it said, and where its default branch stands with the release.
type Stepped = (Option<Ran>, Vec<String>, Taken);

/// Where a project's default branch stands with the release, once its step ended.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Taken {
    /// It does not take it: the step made no move, or left it on a branch.
    #[default]
    Behind,
    /// The step fast-forwarded it to the release in its main checkout.
    Moved,
    /// It took it before the step.
    Already,
    /// It pins the Symphony beside it, neither ahead nor behind, and the step left it so.
    Beside,
}

/// The step `name` refreshing `consumer` to `against`'s release, whose packages are `names`.
fn step(consumer: &Consumer, name: &str, names: &[String], against: &Against<'_>) -> Stepped {
    let started = Instant::now();
    let path = against.logs.join(format!("{}.log", board::key(name)));
    let mut log = Log::open(&path);
    log.line(&format!(
        "the refresh of {} ({}) to {} ({})",
        consumer.id,
        consumer.root.display(),
        against.tag,
        short(against.commit)
    ));
    let mut found = Found::default();
    let done = refresh(consumer, names, against, &mut log, &mut found);
    for line in &found.said {
        log.line(line);
    }
    let mut ran = Ran {
        name: name.to_string(),
        command: format!("the refresh of {} to {}", consumer.id, against.tag),
        exit_code: Some(0),
        signal: None,
        seconds: started.elapsed().as_secs_f64(),
        log: path,
        steps: Vec::new(),
        error: None,
        timed_out: false,
        interrupted: false,
        load: None,
    };
    match done {
        Ok(Done::Skipped(why)) => (None, vec![format!("left out: {why}")], Taken::Behind),
        Ok(Done::Refreshed) => (Some(ran), found.said, found.taken),
        Err(why) => {
            log.line(&format!("red: {why}"));
            ran.exit_code = None;
            ran.interrupted = against.until.interrupted();
            ran.timed_out = !ran.interrupted && Instant::now() >= against.until.until;
            ran.error = Some(why);
            (Some(ran), found.said, found.taken)
        }
    }
}

/// What a project's refresh came to.
enum Done {
    /// It takes nothing of the Symphony's from git, said why.
    Skipped(String),
    /// It did all it could: what it said is in its [`Found`].
    Refreshed,
}

/// What a project's refresh found, green or red: what it said as it went, a red's lines before
/// it among them, and where its default branch stands with the release.
#[derive(Default)]
struct Found {
    said: Vec<String>,
    taken: Taken,
}

/// Refresh `consumer`, logging each command to `log` and saying what it found to `found`: what it
/// did, or why it is red.
fn refresh(
    consumer: &Consumer,
    names: &[String],
    against: &Against<'_>,
    log: &mut Log,
    found: &mut Found,
) -> Result<Done, String> {
    let id = &consumer.id;
    let branch = &consumer.branch;
    let tip = commit(&consumer.root, &format!("refs/heads/{branch}"))?;
    let candidate: Vec<(String, PathBuf)> = names
        .iter()
        .map(|name| (name.clone(), PathBuf::new()))
        .collect();
    let metadata = conciliate::metadata(&consumer.root, against.until)?;
    let plan = conciliate::plan(&metadata, &candidate);
    if plan.sources.is_empty() {
        return Ok(Done::Skipped(format!(
            "{id}: none of its packages takes one of the Symphony's from git"
        )));
    }
    let lock = git(&consumer.root, &["show", &format!("{tip}:Cargo.lock")])
        .map_err(|_| format!("its {branch} ({}) commits no Cargo.lock", short(&tip)))?;
    let locked = pin::locked(&lock, &plan.sources, names)?;
    if locked.is_empty() {
        return Ok(Done::Skipped(format!(
            "{id}: its Cargo.lock takes none of the Symphony's packages from git"
        )));
    }
    // A release keeps a project's state in `symphony/` of its common dir: one whose state an older
    // build still keeps in its old folders takes no pin of it, nor a move of its board, until the
    // owner moves that state (`tools/move-state.sh`), as each of the release's builds would refuse
    // it.
    let common = PathBuf::from(git(
        &consumer.root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?);
    if let Some(why) = symphony_board::unmoved::refusal(&common, symphony_board::server::FOLDER) {
        return Err(format!("{why}: nothing of {id} was moved"));
    }
    let pinned: BTreeSet<&str> = locked
        .iter()
        .map(|package| package.commit.as_str())
        .collect();
    for pin in &pinned {
        if commit(against.root, pin).is_err() {
            return Err(format!(
                "its {branch} pins the Symphony at {pin}, a commit this repository does not hold: \
                 nothing was moved"
            ));
        }
    }
    // The pin its main locks now, whose schema its board is moved to whatever release is newest.
    let now = pinned.iter().next().copied().unwrap_or_default();
    match stands(against.root, against.commit, &pinned)? {
        Stands::Takes => {
            found.taken = Taken::Already;
            found.said.push(format!(
                "{id}: its {branch} takes {} already (its lock pins {})",
                against.tag,
                short(now)
            ));
            found.said.push(board_move(consumer, now, against, log)?);
            return Ok(Done::Refreshed);
        }
        Stands::Beside(lacking) => {
            // A pin moves only forward: one the release does not descend from is left, its files
            // and its board with it, for the next release that descends from it.
            found.taken = Taken::Beside;
            found
                .said
                .push(beside(consumer, &pinned, &lacking, against));
            return Ok(Done::Refreshed);
        }
        Stands::Behind => {}
    }
    let read = Branch {
        tip: &tip,
        lock: &lock,
        locked: &locked,
        sources: &plan.sources,
        names,
        manifests: &manifests(&metadata, &consumer.root),
    };
    let (said, made) = prepare(consumer, &read, against, log)?;
    found.said.push(said);
    let moved = vet(consumer, &read, &made, against)
        .and_then(|()| advance(consumer, &tip, &made, against, log));
    if let Err(why) = moved {
        // Its main stays where it is; its board still catches up with the pin it locks there.
        match board_move(consumer, now, against, log) {
            Ok(said) => found.said.push(said),
            Err(also) => found.said.push(format!(
                "{id}: its board, at the pin {} its {branch} locks now: {also}",
                short(now)
            )),
        }
        return Err(why);
    }
    found.taken = Taken::Moved;
    found.said.push(format!(
        "{id}: its {branch} is fast-forwarded to {BRANCH}{} ({}) in its main checkout {}, \
         through its own hooks and not pushed: its own next push carries it",
        against.tag,
        short(&made),
        consumer.root.display()
    ));
    found
        .said
        .push(board_move(consumer, against.commit, against, log)?);
    Ok(Done::Refreshed)
}

/// Where the commits a project's lock pins the Symphony at stand with a release.
#[derive(Debug, PartialEq, Eq)]
enum Stands {
    /// The release is each pin or behind it: the lock takes it already.
    Takes,
    /// Each pin is the release or behind it, and one is behind: a move to it only goes forward.
    Behind,
    /// The release descends from none of these pins, and the lock does not take it: a move to it
    /// would drop what they hold.
    Beside(Vec<String>),
}

/// Where the lock pinning the Symphony at `pins` stands with the release `release`, read in the
/// Symphony's repository at `root`, which holds each of them.
fn stands(root: &Path, release: &str, pins: &BTreeSet<&str>) -> Result<Stands, String> {
    let mut takes = true;
    let mut lacking = Vec::new();
    for pin in pins {
        takes &= is_ancestor(root, release, pin)?;
        if !is_ancestor(root, pin, release)? {
            lacking.push((*pin).to_string());
        }
    }
    Ok(if takes {
        Stands::Takes
    } else if lacking.is_empty() {
        Stands::Behind
    } else {
        Stands::Beside(lacking)
    })
}

/// What a step says of `consumer`'s lock pinning the Symphony at `pins`, neither ahead of
/// `against`'s release nor behind it: the release does not descend from `lacking`, so its pin is
/// left as it is.
fn beside(
    consumer: &Consumer,
    pins: &BTreeSet<&str>,
    lacking: &[String],
    against: &Against<'_>,
) -> String {
    fn named<'a>(commits: impl Iterator<Item = &'a str>) -> String {
        commits.map(short).collect::<Vec<_>>().join(" and ")
    }
    let pins = named(pins.iter().copied());
    let lacking = named(lacking.iter().map(String::as_str));
    format!(
        "{id}: its {branch}'s lock pins the Symphony at {pins}, neither ahead of {tag} ({release}) \
         nor behind it: {tag} does not descend from {lacking}, so a move to it would drop what \
         {lacking} holds; its pin, its files and its board are left as they are, and the next \
         release that descends from {lacking} moves it",
        id = consumer.id,
        branch = consumer.branch,
        tag = against.tag,
        release = short(against.commit),
    )
}

/// Whether the pin's commit `made` is the move and nothing more, before its default branch takes
/// it: one commit whose only parent is that branch's `tip` as `read`, changing only manifests and
/// the lock, and putting each of the Symphony's packages the lock takes at the release. Read from
/// the commit as git holds it, after the project's hooks ran (one may have rewritten or staged
/// more), and the same for a branch an earlier refresh left, which its owner may have added to.
/// Else why its main is not moved to it: the branch stays as it is.
fn vet(
    consumer: &Consumer,
    read: &Branch<'_>,
    made: &str,
    against: &Against<'_>,
) -> Result<(), String> {
    let root = &consumer.root;
    let main = &consumer.branch;
    let pin = format!("{BRANCH}{}", against.tag);
    let commit = format!("the commit {} on {pin}", short(made));
    let left = format!(
        "so its {main} was not moved to it: the branch is left as it is, for its own landing, or for \
         `git -C {} branch -D {pin}` and an orchestrator's `just release --refresh` to make it again",
        root.display()
    );
    let parents = git(root, &["rev-list", "--parents", "-n", "1", made])?;
    let parents: Vec<&str> = parents.split_whitespace().skip(1).collect();
    if parents != [read.tip] {
        let past = git(
            root,
            &["rev-list", "--count", &format!("{}..{made}", read.tip)],
        )
        .unwrap_or_else(|_| "more".into());
        return Err(format!(
            "{pin} ({}) is {past} commits past its {main} ({}), not the refresh's one commit on \
             it, {left}",
            short(made),
            short(read.tip)
        ));
    }
    let changed = git(
        root,
        &[
            "diff-tree",
            "-r",
            "--name-only",
            "--no-renames",
            read.tip,
            made,
        ],
    )?;
    let stray: Vec<&str> = changed.lines().filter(|path| !pin_path(path)).collect();
    if !stray.is_empty() {
        return Err(format!(
            "{commit} changes more than its manifests and its lock ({}), {left}",
            stray.join(", ")
        ));
    }
    let lock = git(root, &["show", &format!("{made}:Cargo.lock")])
        .map_err(|_| format!("{commit} holds no Cargo.lock, {left}"))?;
    let locked = pin::locked(&lock, read.sources, read.names)?;
    let behind: Vec<String> = (locked.iter())
        .filter(|package| package.commit != against.commit)
        .map(|package| format!("{} at {}", package.spec(), short(&package.commit)))
        .collect();
    if locked.is_empty() || !behind.is_empty() {
        return Err(format!(
            "{commit} leaves {} short of {} ({}), {left}",
            if behind.is_empty() {
                "every Symphony package".to_string()
            } else {
                behind.join(", ")
            },
            against.tag,
            short(against.commit)
        ));
    }
    Ok(())
}

/// Whether `path` is one a pin's move may change: a manifest, or the workspace's lock.
fn pin_path(path: &str) -> bool {
    path == "Cargo.lock" || path == "Cargo.toml" || path.ends_with("/Cargo.toml")
}

/// Fast-forward `consumer`'s default branch from its `tip` to the pin's commit `made`, that exact
/// commit, in its main checkout, its hooks running: only when that checkout is on its default
/// branch at `tip` and has no tracked edits. Its hooks are asked first, by a ref transaction that
/// is prepared and then aborted, so a refusal they can tell writes nothing in its checkout (the
/// shared server may be built from it). Never pushed, never a merge commit. Else why it was not
/// moved, with what finishes it.
fn advance(
    consumer: &Consumer,
    tip: &str,
    made: &str,
    against: &Against<'_>,
    log: &mut Log,
) -> Result<(), String> {
    let root = &consumer.root;
    let shown = root.display();
    let main = &consumer.branch;
    let pin = format!("{BRANCH}{}", against.tag);
    let then = format!(
        "the branch {pin} is left for it, and once that is put right an orchestrator's `just \
         release --refresh` fast-forwards it and moves its board"
    );
    let landing = format!(
        "the branch {pin} is left for its own landing, and once that lands it an orchestrator's \
         `just release --refresh` moves its board"
    );
    let head = git(root, &["symbolic-ref", "--quiet", "--short", "HEAD"]).unwrap_or_default();
    if head != *main {
        let on = if head.is_empty() {
            "a detached HEAD".to_string()
        } else {
            format!("the branch {head}")
        };
        return Err(format!(
            "its main checkout {shown} is on {on}, not its {main}, so its {main} was not moved: \
             {then}"
        ));
    }
    let edits = git(root, &["status", "--porcelain", "--untracked-files=no"])?;
    if !edits.is_empty() {
        let paths: Vec<&str> = (edits.lines().map(|line| line.get(3..).unwrap_or(line)))
            .take(5)
            .collect();
        return Err(format!(
            "its main checkout {shown} has tracked edits ({}), so its {main} was not moved: \
             {then}",
            paths.join(", ")
        ));
    }
    let now = commit(root, &format!("refs/heads/{main}"))?;
    if now != tip {
        return Err(format!(
            "its {main} moved on to {} while the move was made, so {} is no longer one commit on \
             it and nothing was moved: an orchestrator's `just release --refresh` makes the move \
             again on its tip",
            short(&now),
            short(made)
        ));
    }
    // Asked first: the reference-transaction hooks see the very update, prepared, then aborted.
    let ask = against.logs.join(format!(
        "{}.transaction",
        board::key(&format!("{STEP}-{}", consumer.id))
    ));
    fs::write(
        &ask,
        format!("start\nupdate refs/heads/{main} {made} {tip}\nprepare\nabort\n"),
    )
    .map_err(|e| crate::text::os_error(&e, &ask))?;
    let asked = log.run(
        root,
        &[
            "sh",
            "-c",
            "exec git update-ref --stdin <\"$1\"",
            "sh",
            &ask.to_string_lossy(),
        ],
        against.until,
    );
    if let Err(why) = asked {
        return Err(format!(
            "git refuses the move of its {main} to {} when asked first, in a ref transaction then \
             aborted ({why}), so its {main} was not moved and nothing was written in its \
             checkout: {landing}",
            short(made)
        ));
    }
    if let Err(why) = log.run(
        root,
        &["git", "merge", "--ff-only", "--quiet", made],
        against.until,
    ) {
        return Err(refused(consumer, tip, made, &why, &landing, against, log));
    }
    let moved = commit(root, &format!("refs/heads/{main}"))?;
    if moved != made {
        return Err(format!(
            "`git merge --ff-only {made}` in {shown} left its {main} at {}, not {}",
            short(&moved),
            short(made)
        ));
    }
    Ok(())
}

/// What a fast-forward of `consumer`'s default branch from `tip` to `made` that git refused
/// (`why`) left, each part read back, never assumed: where that branch is, and each path the move
/// changes. Git writes the move's files into the checkout before its hooks may refuse the
/// branch's move, and leaves them there when they do; each is put back to `tip`, by itself, when
/// and only when its index entry and its file still hold exactly what the move wrote, so nothing
/// of its owner's is touched, staged or not. One that holds anything else is left and named, with
/// the command that puts it back.
fn refused(
    consumer: &Consumer,
    tip: &str,
    made: &str,
    why: &str,
    landing: &str,
    against: &Against<'_>,
    log: &mut Log,
) -> String {
    let root = &consumer.root;
    let main = &consumer.branch;
    let said = format!(
        "git refused the fast-forward of its {main} to {}, through its hooks or its checkout \
         ({why})",
        short(made)
    );
    match commit(root, &format!("refs/heads/{main}")) {
        Ok(now) if now == tip => {}
        Ok(now) => {
            return format!(
                "{said}, and its {main} is at {} now, not {}: its checkout {} is left as it is, to \
                 be looked at before anything more",
                short(&now),
                short(tip),
                root.display()
            );
        }
        Err(e) => return format!("{said}, and where its {main} is cannot be read ({e})"),
    }
    let paths = git(
        root,
        &["diff-tree", "-r", "--name-only", "--no-renames", tip, made],
    )
    .unwrap_or_default();
    let (mut back, mut left) = (Vec::new(), Vec::new());
    for path in paths.lines().filter(|path| !path.is_empty()) {
        match put_back(root, tip, made, path, against, log) {
            Back::Untouched => {}
            Back::Restored => back.push(path),
            Back::Left => left.push(path),
        }
    }
    let mut lines = vec![format!("{said}; its {main} is still at {}", short(tip))];
    if !back.is_empty() {
        lines.push(format!(
            "git had written the move's {} into its checkout first, now put back as its {main} \
             has {}",
            back.join(" and "),
            if back.len() == 1 { "it" } else { "them" }
        ));
    }
    for path in left {
        lines.push(format!(
            "{path} is not just as the move wrote it, so it is left as it is: `git -C {} restore \
             --staged --worktree -- {path}` puts it back as its {main} has it, losing what changed \
             there",
            root.display()
        ));
    }
    format!("{}: {landing}", lines.join("; "))
}

/// What [`put_back`] did with a path.
#[derive(Debug, PartialEq, Eq)]
enum Back {
    /// Its index entry and its file are its default branch's: git never wrote it.
    Untouched,
    /// They held exactly what the move wrote, and are its default branch's again.
    Restored,
    /// They hold something else, which is left.
    Left,
}

/// Put `path` back to `tip` in `root`'s index and checkout when both still hold exactly `made`'s
/// content of it, read back after.
fn put_back(
    root: &Path,
    tip: &str,
    made: &str,
    path: &str,
    against: &Against<'_>,
    log: &mut Log,
) -> Back {
    let blob = |rev: &str| {
        git(
            root,
            &["rev-parse", "--verify", "--quiet", &format!("{rev}:{path}")],
        )
        .ok()
    };
    let staged = || {
        let entry = git(
            root,
            &["ls-files", "--stage", "--", &format!(":(literal){path}")],
        )
        .unwrap_or_default();
        (entry.lines())
            .find(|line| line.split_once('\t').is_some_and(|(_, at)| at == path))
            .and_then(|line| line.split_whitespace().nth(1).map(str::to_string))
    };
    let file = || {
        root.join(path)
            .is_file()
            .then(|| git(root, &["hash-object", "--", path]).ok())
            .flatten()
    };
    let (was, wrote) = (blob(tip), blob(made));
    let holds = |want: &Option<String>| staged() == *want && file() == *want;
    if holds(&was) {
        return Back::Untouched;
    }
    if !holds(&wrote) {
        return Back::Left;
    }
    let source = format!("--source={tip}");
    let spec = format!(":(literal){path}");
    let words = [
        "git",
        "restore",
        source.as_str(),
        "--staged",
        "--worktree",
        "--",
        spec.as_str(),
    ];
    match log.run(root, &words, against.until) {
        Ok(_) if holds(&was) => Back::Restored,
        _ => Back::Left,
    }
}

/// What a refresh read of a project's default branch: its tip, its lock and the Symphony packages
/// that lock takes from the git `sources` (of the release's `names`), and its workspace's
/// manifests, relative to its checkout.
struct Branch<'a> {
    tip: &'a str,
    lock: &'a str,
    locked: &'a [Locked],
    sources: &'a [String],
    names: &'a [String],
    manifests: &'a [PathBuf],
}

/// The manifests of the workspace `metadata` describes, relative to its checkout `root`: the
/// workspace's own and each member's.
fn manifests(metadata: &serde_json::Value, root: &Path) -> Vec<PathBuf> {
    let members: Vec<&str> = (metadata["workspace_members"].as_array().into_iter())
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .collect();
    let mut found = BTreeSet::from([PathBuf::from("Cargo.toml")]);
    for package in (metadata["packages"].as_array().into_iter()).flatten() {
        if !package["id"]
            .as_str()
            .is_some_and(|id| members.contains(&id))
        {
            continue;
        }
        let Some(path) = package["manifest_path"].as_str() else {
            continue;
        };
        let path = Path::new(path);
        let relative = (path.strip_prefix(root).ok())
            .or_else(|| path.strip_prefix(root.canonicalize().ok()?).ok());
        found.extend(relative.map(Path::to_path_buf));
    }
    found.into_iter().collect()
}

/// Make the pin's move to the release on the branch `symphony/RELEASE` of `consumer`'s repository,
/// from its default branch as `read`, in a worktree of its own: its manifests take the release's
/// tag, `cargo update --workspace` moves its lock, and the commit goes through its own hooks. A
/// branch an earlier refresh made is taken as it is while it is a fast-forward of the default
/// branch, and made again on its tip once that moved past it. What it said, and the commit.
fn prepare(
    consumer: &Consumer,
    read: &Branch<'_>,
    against: &Against<'_>,
    log: &mut Log,
) -> Result<(String, String), String> {
    let Branch {
        tip,
        lock,
        locked,
        sources,
        names,
        manifests,
    } = *read;
    let id = &consumer.id;
    let branch = format!("{BRANCH}{}", against.tag);
    let existing = commit(&consumer.root, &format!("refs/heads/{branch}")).ok();
    if let Some(made) = &existing {
        if is_ancestor(&consumer.root, tip, made)? {
            let said = format!(
                "{id}: its pin to {} is on the branch {branch} ({}) already",
                against.tag,
                short(made)
            );
            return Ok((said, made.clone()));
        }
        if !own_commit(&consumer.root, tip, made, against.tag) {
            return Err(format!(
                "its branch {branch} ({}) is behind its {} and holds more than the refresh's own \
                 commit, so it is left as it is: `git -C {} branch -D {branch}`, then an \
                 orchestrator's `just release --refresh`, makes it again",
                short(made),
                consumer.branch,
                consumer.root.display()
            ));
        }
        log.line(&format!(
            "its branch {branch} ({}) is behind its {}: made again on its tip",
            short(made),
            consumer.branch
        ));
    }
    let tree = against.trees.join(id);
    clear(&consumer.root, &tree, log)?;
    fs::create_dir_all(against.trees).map_err(|e| crate::text::os_error(&e, against.trees))?;
    let path = tree.to_string_lossy().into_owned();
    log.run(
        &consumer.root,
        &[
            "git", "worktree", "add", "--quiet", "--lock", "--reason", REASON, "--detach", &path,
            tip,
        ],
        against.until,
    )?;
    let _removed = Worktree {
        project: consumer.root.clone(),
        path: tree.clone(),
    };
    // Its manifests name the release's tag for the Symphony's sources; the lock follows them.
    let mut moved = 0;
    for manifest in manifests {
        let file = tree.join(manifest);
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        let (retagged, count) = pin::retag(&text, sources, against.tag);
        let off = pin::tagged(&retagged, sources, against.tag)
            .map_err(|e| format!("{} {e}", manifest.display()))?;
        if !off.is_empty() {
            return Err(format!(
                "{} names the Symphony's git source in a form the refresh does not rewrite ({}): \
                 nothing was moved",
                manifest.display(),
                off.join(", ")
            ));
        }
        if retagged != text {
            fs::write(&file, retagged).map_err(|e| crate::text::os_error(&e, &file))?;
            log.line(&format!(
                "{}: {count} named {}",
                manifest.display(),
                against.tag
            ));
        }
        moved += count;
    }
    if moved == 0 {
        return Err(format!(
            "none of its manifests names the Symphony's git source ({}): nothing was moved",
            sources.join(", ")
        ));
    }
    log.run(&tree, &["cargo", "update", "--workspace"], against.until)?;
    let lock_file = tree.join("Cargo.lock");
    let after =
        fs::read_to_string(&lock_file).map_err(|e| crate::text::os_error(&e, &lock_file))?;
    let behind: Vec<String> = (pin::locked(&after, sources, names)?.iter())
        .filter(|package| package.commit != against.commit)
        .map(|package| format!("{} at {}", package.spec(), short(&package.commit)))
        .collect();
    if !behind.is_empty() {
        return Err(format!(
            "`cargo update` left {} short of {}: nothing was moved",
            behind.join(", "),
            short(against.commit)
        ));
    }
    let others = pin::others(lock, &after, sources)?;
    log.run(&tree, &["git", "add", "--update"], against.until)?;
    let staged = log.run(
        &tree,
        &["git", "diff", "--cached", "--name-only"],
        against.until,
    )?;
    let stray: Vec<&str> = (staged.lines()).filter(|path| !pin_path(path)).collect();
    if !stray.is_empty() {
        return Err(format!(
            "the move changed more than its manifests and its lock ({}): nothing was moved",
            stray.join(", ")
        ));
    }
    let from: Vec<&str> = (locked.iter().map(|package| short(&package.commit)))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let message = against
        .logs
        .join(format!("{}.message", board::key(&format!("{STEP}-{id}"))));
    fs::write(&message, self::message(against, &from, &others))
        .map_err(|e| crate::text::os_error(&e, &message))?;
    let message = message.to_string_lossy().into_owned();
    log.run(
        &tree,
        &["git", "commit", "--quiet", "--file", &message],
        against.until,
    )
    .map_err(|why| {
        format!(
            "its commit gate refused the move to {} ({why}), so nothing of {id} moved: no \
                 branch was made, and its {} is as it was",
            against.tag, consumer.branch
        )
    })?;
    let made = commit(&tree, "HEAD")?;
    if existing.is_some() {
        git(&consumer.root, &["branch", "--force", &branch, &made])?;
    } else {
        git(&consumer.root, &["branch", &branch, &made])?;
    }
    let also = if others.is_empty() {
        String::new()
    } else {
        format!("; the move also takes {}", others.join(", "))
    };
    let said = format!(
        "{id}: its pin to {} is on the branch {branch} ({}), made {}through its own commit gate \
         and not pushed{also}",
        against.tag,
        short(&made),
        if existing.is_some() {
            "again on its tip "
        } else {
            ""
        }
    );
    Ok((said, made))
}

/// Whether `made` is one commit on `tip`'s line that the refresh to `tag` made: its only commit
/// past it, saying it takes that release.
fn own_commit(root: &Path, tip: &str, made: &str, tag: &str) -> bool {
    let base = git(root, &["merge-base", tip, made]).unwrap_or_default();
    let past = git(root, &["rev-list", "--count", &format!("{base}..{made}")]);
    let subject = git(root, &["log", "-1", "--format=%s", made]);
    past.is_ok_and(|count| count == "1")
        && subject.is_ok_and(|subject| subject == format!("Take Symphony {tag}"))
}

/// The pin's commit message: the release, the commits it moves from, what else the lock took.
fn message(against: &Against<'_>, from: &[&str], others: &[String]) -> String {
    let also = if others.is_empty() {
        "and nothing else".to_string()
    } else {
        format!("and, as the release needs, {}", others.join(", "))
    };
    format!(
        "Take Symphony {tag}\n\n\
         Its manifests name the tag {tag} for the Symphony's git source, and Cargo.lock moves \
         the Symphony's packages from {from} to it ({commit}), {also}.\n\
         Made by the Symphony release run's refresh of that release (docs/RELEASES.md in the \
         Symphony), for this project's own landing.\n",
        tag = against.tag,
        from = from.join(", "),
        commit = short(against.commit),
    )
}

/// Make the place `tree` ready for a worktree of `project`: the release run's own worktree there, from
/// an earlier run, removed; any other folder in the way refused and left as it is.
fn clear(project: &Path, tree: &Path, log: &mut Log) -> Result<(), String> {
    let listed = git(project, &["worktree", "list", "--porcelain"])?;
    let registered = (listed.lines())
        .filter_map(|line| line.strip_prefix("worktree "))
        .any(|path| Path::new(path) == tree);
    let path = tree.to_string_lossy().into_owned();
    if registered && tree.exists() {
        let removed = ["git", "worktree", "remove", "--force", "--force", &path];
        return log
            .run(
                project,
                &removed,
                Bound::until(Instant::now() + super::git::GIT_BOUND),
            )
            .map(|_| ());
    }
    if registered {
        return log
            .run(
                project,
                &["git", "worktree", "prune"],
                Bound::until(Instant::now() + super::git::GIT_BOUND),
            )
            .map(|_| ());
    }
    if fs::symlink_metadata(tree).is_ok() {
        return Err(format!(
            "{} is in the way and is not the release run's worktree of {}: it is left as it is; move \
             it away and retry",
            tree.display(),
            project.display()
        ));
    }
    Ok(())
}

/// The release run's worktree of a project, removed with it, whatever it holds.
struct Worktree {
    project: PathBuf,
    path: PathBuf,
}

impl Drop for Worktree {
    fn drop(&mut self) {
        let path = self.path.to_string_lossy().into_owned();
        let _ = git(
            &self.project,
            &["worktree", "remove", "--force", "--force", &path],
        );
    }
}

/// Move `consumer`'s live board to the schema of its pin `at`, its store snapshotted first: what it
/// did, or why it moved nothing.
fn board_move(
    consumer: &Consumer,
    at: &str,
    against: &Against<'_>,
    log: &mut Log,
) -> Result<String, String> {
    let id = &consumer.id;
    let root = consumer.root.display();
    let Some(target) = notes::facts(against.root, at).schema else {
        return Ok(format!(
            "{id}: its pin {} names no board schema: nothing to move",
            short(at)
        ));
    };
    let common = PathBuf::from(git(
        &consumer.root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?);
    let folder = common.join(symphony_board::server::FOLDER);
    if !folder.is_dir() {
        return Ok(format!(
            "{id}: it keeps no board ({} is not there): nothing to move",
            folder.display()
        ));
    }
    let home = Home::in_common(&common, Some(&surreal_for(&common, against.surreal)));
    let live = match home.schema(symphony_board::DATABASE) {
        Ok(Some(live)) => live,
        Ok(None) => return Ok(format!("{id}: its board is not made yet: nothing to move")),
        Err(ServerError::NotUp(why)) => {
            return Err(format!(
                "{why}: its schema was not read, snapshotted or moved; once it runs (any `just \
                 tasks` in {root} starts it), an orchestrator's `just release --refresh` moves it"
            ));
        }
        Err(error) => return Err(error.to_string()),
    };
    log.line(&format!(
        "its board is at schema {live}; its pin {} holds {target}",
        short(at)
    ));
    if live >= target {
        return Ok(format!(
            "{id}: its board is at schema {live}, its pin's: nothing to move"
        ));
    }
    // Fails closed: a version that cannot be read could be one the move replaces the server for.
    let by_hand = format!("`cd {root} && just tasks migrate` moves it when its owner says");
    let Some(pinned) = surreal_at(against.root, at) else {
        return Err(format!(
            "the SurrealDB its pin {} pins cannot be read (crates/tasks/src/server.rs there names \
             no SURREAL_VERSION), so the move might replace its board's database and the refresh \
             stops none: nothing was moved; {by_hand}",
            short(at)
        ));
    };
    let running = match home.recorded() {
        Some(running) if !running.version.trim().is_empty() => running.version,
        _ => {
            return Err(format!(
                "the SurrealDB its board's database runs cannot be read (its record in {} names no \
                 version), so the move might replace that database and the refresh stops none: \
                 nothing was moved; {by_hand}",
                folder.display()
            ));
        }
    };
    if older(&running, &pinned) {
        return Err(format!(
            "its board's database runs SurrealDB {running} and its pin {} pins {pinned}: moving \
             its schema would replace that server, and the refresh stops no board's database, so \
             nothing was moved; {by_hand}",
            short(at)
        ));
    }
    let snapshots = common
        .join(symphony_board::server::FOLDER)
        .join("snapshots");
    let snapshot = snapshots.join(format!("before-schema-{target}-{}.surql", against.tag));
    home.export(symphony_board::DATABASE, &snapshot)
        .map_err(|e| {
            format!("the snapshot of its store before the move failed, so nothing was moved: {e}")
        })?;
    log.line(&format!("its store, as it ran: {}", snapshot.display()));
    prune(&snapshots, KEPT_SNAPSHOTS);
    log.run(&consumer.root, &["just", "tasks", "migrate"], against.until)
        .map_err(|why| {
            format!(
                "`just tasks migrate` in {root} failed ({why}); its store as it was before is in \
                 {}, and `cd {root} && just tasks migrate` retries",
                snapshot.display()
            )
        })?;
    match home.schema(symphony_board::DATABASE) {
        Ok(Some(moved)) if moved >= target => Ok(format!(
            "{id}: its board moved from schema {live} to {moved}, its store snapshotted first to {}",
            snapshot.display()
        )),
        read => Err(format!(
            "`just tasks migrate` in {root} ended, but its board reads {}, short of its pin's \
             schema {target}; its store as it was before is in {}",
            match read {
                Ok(Some(moved)) => format!("schema {moved}"),
                Ok(None) => "no schema".into(),
                Err(e) => e.to_string(),
            },
            snapshot.display()
        )),
    }
}

/// The `surreal` a snapshot of the board in the git common dir `common` runs: `SYMPHONY_SURREAL`,
/// else the one pinned beside that board, else `fallback` (the Symphony's own).
fn surreal_for(common: &Path, fallback: Option<&Path>) -> PathBuf {
    let named = symphony_env::SURREAL.os().map(PathBuf::from);
    let beside = Home::in_common(common, None).surreal().to_path_buf();
    (named.into_iter())
        .chain([beside.clone()])
        .chain(fallback.map(Path::to_path_buf))
        .find(|path| path.is_file())
        .unwrap_or(beside)
}

/// The SurrealDB version the board's source pins at `commit` of the repository at `root`.
fn surreal_at(root: &Path, commit: &str) -> Option<String> {
    let text = git(
        root,
        &["show", &format!("{commit}:crates/tasks/src/server.rs")],
    )
    .ok()?;
    (text.lines()).find_map(|line| {
        let rest = line
            .trim()
            .strip_prefix("pub const SURREAL_VERSION: &str = \"")?;
        rest.split_once('"').map(|(version, _)| version.to_string())
    })
}

/// Whether the version `running` is older than `pinned`, part by dotted part.
fn older(running: &str, pinned: &str) -> bool {
    let parts = |version: &str| -> Vec<u64> {
        (version.trim_start_matches('v').split(['.', '-', '+']))
            .map(|part| part.parse().unwrap_or(0))
            .collect()
    };
    parts(running) < parts(pinned)
}

/// Remove from `folder` the snapshots but the newest `kept`.
fn prune(folder: &Path, kept: usize) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = (entries.filter_map(Result::ok))
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("before-schema-"))
        })
        .filter_map(|path| Some((fs::metadata(&path).ok()?.modified().ok()?, path)))
        .collect();
    found.sort();
    let gone = found.len().saturating_sub(kept);
    for (_, path) in found.into_iter().take(gone) {
        let _ = fs::remove_file(path);
    }
}

/// What the shared server does with the release, said: the server this machine's folder `machine`
/// records, as it answered before the refresh (`before`) and now, and where the main it is built
/// from stands with the release (`taking`, each project refreshed). When the refresh moved that
/// main, its reload's build is waited for, bounded ([`reload`]).
fn server(
    machine: &Path,
    before: Option<&Front>,
    taking: &[(Consumer, Taken)],
    against: &Against<'_>,
) -> String {
    let Some(front) = crate::machine::command::running(machine, None) else {
        return "the shared server: none answers on this machine, so none takes the release".into();
    };
    let root = PathBuf::from(&front.root);
    let same = |path: &Path| {
        path == root
            || (path.canonicalize().ok()).is_some_and(|path| Some(path) == root.canonicalize().ok())
    };
    let named = format!(
        "the shared server (process {}, port {}) is built from {}",
        front.pid,
        front.port,
        root.display()
    );
    let reloads = "its own reload (apps/symphony-server/src/reload.rs) builds and installs a server \
                   from that checkout's new Cargo.lock and hands it its sockets, a failed build \
                   leaving the old one serving and saying why on its notice";
    match taking.iter().find(|(consumer, _)| same(&consumer.root)) {
        Some((consumer, Taken::Moved)) => {
            // With none answering for that checkout before the move, which build answers now
            // cannot be told against one before it: none is taken as the reload's.
            let Some(old) = before.filter(|old| same(Path::new(&old.root))) else {
                return format!(
                    "{named}, whose {} this refresh moved to {}: none built from it answered \
                     before the move, so whether this one is built after it cannot be told; \
                     {reloads} (`just symphony-status` says which process answers)",
                    consumer.branch, against.tag
                );
            };
            let target = notes::facts(against.root, against.commit).schema;
            match reload(machine, old.pid, target, against) {
                Ok(new) => format!(
                    "the shared server (port {}) is built from {}, whose {} this refresh moved to \
                     {}: its own reload (apps/symphony-server/src/reload.rs) built it again, and \
                     process {} answers in place of {}",
                    new.port,
                    root.display(),
                    consumer.branch,
                    against.tag,
                    new.pid,
                    old.pid,
                ),
                Err(waited) => format!(
                    "{named}, whose {} this refresh moved to {}: no other process answered within \
                     {} s, so it has not taken it yet; {reloads} (`just symphony-status` says \
                     which process answers)",
                    consumer.branch,
                    against.tag,
                    waited.as_secs()
                ),
            }
        }
        Some((consumer, Taken::Already)) => format!(
            "{named}, whose {} took {} before this refresh, as its own reload \
             (apps/symphony-server/src/reload.rs) follows that checkout's Cargo.lock",
            consumer.branch, against.tag
        ),
        Some((consumer, Taken::Behind)) => format!(
            "{named}: it takes {} once {}'s {} does, which this refresh did not move (its step \
             says why)",
            against.tag, consumer.id, consumer.branch
        ),
        Some((consumer, Taken::Beside)) => format!(
            "{named}: it does not take {}, as {}'s {} pins the Symphony beside it, which this \
             refresh left as it is (its step says why)",
            against.tag, consumer.id, consumer.branch
        ),
        None if same(against.main) => {
            format!("{named}, this repository's main checkout, which its own reload follows")
        }
        None => format!("{named}, which takes nothing of the Symphony's from this release"),
    }
}

/// The shared server this machine's folder `machine` records, once a process other than `old`,
/// the one that answered before the move, answers for it, at the release's board schema `target`
/// or past it when both say one: its reload's new build. Waited for within [`RELOAD`] (the tests'
/// [`RELOAD_SECONDS`]) and the refresh's time; else how long it was waited for.
fn reload(
    machine: &Path,
    old: i64,
    target: Option<i64>,
    against: &Against<'_>,
) -> Result<Front, Duration> {
    let wait = (std::env::var(RELOAD_SECONDS).ok())
        .and_then(|seconds| seconds.parse::<u64>().ok())
        .map_or(RELOAD, Duration::from_secs);
    let began = Instant::now();
    let until = (began + wait).min(against.until.until);
    loop {
        if let Some(now) = crate::machine::command::running(machine, None)
            && now.pid != old
            && (now.schema == 0 || target.is_none_or(|target| now.schema >= target))
        {
            return Ok(now);
        }
        if Instant::now() >= until || against.until.interrupted() {
            return Err(until.saturating_duration_since(began));
        }
        std::thread::sleep(RELOAD_LOOK.min(until.saturating_duration_since(Instant::now())));
    }
}

/// A step's log: each command it ran, with all it printed.
struct Log {
    file: Option<File>,
}

impl Log {
    fn open(path: &Path) -> Log {
        if let Some(folder) = path.parent() {
            let _ = fs::create_dir_all(folder);
        }
        Log {
            file: File::create(path).ok(),
        }
    }

    fn line(&mut self, text: &str) {
        if let Some(file) = &mut self.file {
            let _ = writeln!(file, "{text}");
        }
    }

    /// `words` run in `dir` within `until`, without the variables [`UNSET`] names: what it
    /// printed, or why it failed, its output logged either way.
    fn run(&mut self, dir: &Path, words: &[&str], until: Bound<'_>) -> Result<String, String> {
        let line = words.join(" ");
        self.line(&format!("$ {line}   (in {})", dir.display()));
        let mut run = command(words[0]);
        run.args(&words[1..])
            .current_dir(dir)
            .env("GIT_TERMINAL_PROMPT", "0");
        for name in UNSET {
            run.env_remove(name);
        }
        let done = output_until(&mut run, until).map_err(|failed| match failed {
            Failed::Io(e) => format!("`{line}` could not run in {}: {e}", dir.display()),
            Failed::Late(took) => format!(
                "`{line}` did not end within {} s, the refresh's time, and was stopped",
                took.as_secs()
            ),
            Failed::Interrupted(took) => format!(
                "`{line}` was stopped after {} s: the run was told to stop",
                took.as_secs()
            ),
        });
        let done = match done {
            Ok(done) => done,
            Err(why) => {
                self.line(&why);
                return Err(why);
            }
        };
        let stdout = String::from_utf8_lossy(&done.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&done.stderr).into_owned();
        for text in [&stdout, &stderr] {
            if !text.trim().is_empty() {
                self.line(text.trim_end());
            }
        }
        if done.status.success() {
            return Ok(stdout);
        }
        let last: Vec<&str> = (stderr.lines().chain(stdout.lines()))
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        let last = last[last.len().saturating_sub(3)..].join(" / ");
        let status = done
            .status
            .code()
            .map_or_else(|| "was killed".to_string(), |code| format!("exited {code}"));
        Err(format!(
            "`{line}` {status}{}",
            if last.is_empty() {
                String::new()
            } else {
                format!(": {last}")
            }
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_tree::Tree;

    #[test]
    fn a_running_database_older_than_the_pins_is_one_the_move_would_replace() {
        assert!(older("3.2.9", "3.3.0"));
        assert!(older("v2.3.0", "3.0.0"));
        assert!(older("3.3", "3.3.1"));
        assert!(!older("3.3.0", "3.3.0"));
        assert!(!older("3.4.0", "3.3.9"));
        assert!(!older("3.3.0-beta", "3.3.0"), "a suffix is no older");
    }

    #[test]
    fn the_snapshots_kept_are_the_newest_and_nothing_else_is_touched() {
        let tree = Tree::new();
        let folder = tree.path("snapshots");
        fs::create_dir_all(&folder).unwrap();
        let mut times = Vec::new();
        for n in 0..7 {
            let file = folder.join(format!("before-schema-{n}-release.surql"));
            fs::write(&file, "x").unwrap();
            let when =
                std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000 + n);
            File::options()
                .write(true)
                .open(&file)
                .unwrap()
                .set_modified(when)
                .unwrap();
            times.push(file);
        }
        fs::write(folder.join("someone-else.surql"), "y").unwrap();
        prune(&folder, 5);
        let mut left = tree.names("snapshots");
        left.sort();
        assert_eq!(
            left,
            [
                "before-schema-2-release.surql",
                "before-schema-3-release.surql",
                "before-schema-4-release.surql",
                "before-schema-5-release.surql",
                "before-schema-6-release.surql",
                "someone-else.surql",
            ]
        );
        prune(&tree.path("none"), 5);
    }

    #[test]
    fn the_pin_message_names_the_release_and_all_the_lock_took() {
        let tree = Tree::new();
        let none = tree.path("none");
        let against = Against {
            root: &none,
            ours: &none,
            main: &none,
            tag: "release-2026-11-12",
            commit: "0123456789abcdef0123",
            trees: &none,
            logs: &none,
            surreal: None,
            until: Bound::until(Instant::now()),
        };
        assert_eq!(
            message(&against, &["aaaaaaaaaaaa"], &[]),
            "Take Symphony release-2026-11-12\n\nIts manifests name the tag release-2026-11-12 \
             for the Symphony's git source, and Cargo.lock moves the Symphony's packages from \
             aaaaaaaaaaaa to it (0123456789ab), and nothing else.\nMade by the Symphony \
             release run's refresh of that release (docs/RELEASES.md in the Symphony), for this \
             project's own landing.\n"
        );
        assert!(
            message(&against, &["a", "b"], &["serde 1.0.200 → 1.0.210".into()]).contains(
                "from a, b to it (0123456789ab), and, as the release needs, serde 1.0.200 → \
                 1.0.210."
            )
        );
    }

    /// A repository at `root` whose main took a landing `P` beside a fix `F` branched from the
    /// base `B`, then merged that fix as `M`: `B`, `F`, `P` and `M` by name.
    fn fork(root: &Path) -> [String; 4] {
        fs::create_dir_all(root).unwrap();
        let run = |args: &[&str]| {
            git(
                root,
                &[
                    &["-c", "user.name=t", "-c", "user.email=t@example.com"][..],
                    args,
                ]
                .concat(),
            )
            .unwrap()
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["commit", "-q", "--allow-empty", "-m", "the base"]);
        let base = run(&["rev-parse", "HEAD"]);
        run(&["switch", "-q", "-c", "fix"]);
        run(&["commit", "-q", "--allow-empty", "-m", "the fix"]);
        let fix = run(&["rev-parse", "HEAD"]);
        run(&["switch", "-q", "main"]);
        run(&["commit", "-q", "--allow-empty", "-m", "a landing meanwhile"]);
        let meanwhile = run(&["rev-parse", "HEAD"]);
        run(&["merge", "-q", "--no-ff", "-m", "the fix's land", "fix"]);
        let merged = run(&["rev-parse", "HEAD"]);
        [base, fix, meanwhile, merged]
    }

    #[test]
    fn a_lock_stands_behind_the_release_only_when_every_pin_is_one_it_descends_from() {
        let tree = Tree::new();
        let root = tree.path("symphony");
        let [base, fix, meanwhile, merged] = fork(&root);
        let stand = |pins: &[&str]| stands(&root, &fix, &pins.iter().copied().collect());
        // The release is the fix: behind it, at it, past it and beside it.
        assert_eq!(stand(&[&base]), Ok(Stands::Behind));
        assert_eq!(stand(&[&fix]), Ok(Stands::Takes));
        assert_eq!(stand(&[&merged]), Ok(Stands::Takes));
        assert_eq!(
            stand(&[&meanwhile]),
            Ok(Stands::Beside(vec![meanwhile.clone()]))
        );
        // Two pins: at it and behind it moves forward; at it and past it takes it already.
        assert_eq!(stand(&[&fix, &base]), Ok(Stands::Behind));
        assert_eq!(stand(&[&fix, &merged]), Ok(Stands::Takes));
        // One past it and one behind, or one beside and one behind: a move would drop what the
        // one the release lacks holds, and that one alone is named.
        assert_eq!(
            stand(&[&merged, &base]),
            Ok(Stands::Beside(vec![merged.clone()]))
        );
        assert_eq!(
            stand(&[&meanwhile, &base]),
            Ok(Stands::Beside(vec![meanwhile.clone()]))
        );
        // The base released: the lock beside it before is past it now.
        let stand_base = stands(&root, &base, &[meanwhile.as_str()].into_iter().collect());
        assert_eq!(stand_base, Ok(Stands::Takes));
        // And the merge released: the pin left beside the fix is one it descends from.
        let stand_merged = stands(&root, &merged, &[meanwhile.as_str()].into_iter().collect());
        assert_eq!(stand_merged, Ok(Stands::Behind));
        // A pin git cannot answer for is said, never taken for one side or the other.
        let unknown = "0123456789abcdef0123456789abcdef01234567";
        let refused = stand(&[unknown]).unwrap_err();
        assert!(
            refused.contains(&format!(
                "git merge-base --is-ancestor {fix} {unknown} failed"
            )),
            "{refused}"
        );
    }

    #[test]
    fn a_pin_left_beside_the_release_is_named_with_both_commits() {
        let tree = Tree::new();
        let none = tree.path("none");
        let against = Against {
            root: &none,
            ours: &none,
            main: &none,
            tag: "release-2026-11-12",
            commit: "ffffffffffff0000",
            trees: &none,
            logs: &none,
            surreal: None,
            until: Bound::until(Instant::now()),
        };
        let consumer = Consumer {
            id: "consumer".into(),
            root: none.clone(),
            branch: "trunk".into(),
        };
        let one = ["aaaaaaaaaaaa1111"].into_iter().collect();
        assert_eq!(
            beside(&consumer, &one, &["aaaaaaaaaaaa1111".into()], &against),
            "consumer: its trunk's lock pins the Symphony at aaaaaaaaaaaa, neither ahead of \
             release-2026-11-12 (ffffffffffff) nor behind it: release-2026-11-12 does not descend \
             from aaaaaaaaaaaa, so a move to it would drop what aaaaaaaaaaaa holds; its pin, its \
             files and its board are left as they are, and the next release that descends from \
             aaaaaaaaaaaa moves it"
        );
        let two = ["aaaaaaaaaaaa1111", "bbbbbbbbbbbb2222"]
            .into_iter()
            .collect();
        let said = beside(&consumer, &two, &["bbbbbbbbbbbb2222".into()], &against);
        assert!(
            said.starts_with(
                "consumer: its trunk's lock pins the Symphony at aaaaaaaaaaaa and bbbbbbbbbbbb, \
                 neither ahead of release-2026-11-12 (ffffffffffff) nor behind it: \
                 release-2026-11-12 does not descend from bbbbbbbbbbbb, so"
            ),
            "{said}"
        );
    }

    #[test]
    fn the_pinned_surreal_is_read_from_the_boards_source_at_the_commit() {
        let tree = Tree::new();
        let root = tree.path("repo");
        fs::create_dir_all(root.join("crates/tasks/src")).unwrap();
        fs::write(
            root.join("crates/tasks/src/server.rs"),
            "//! The server.\n\n/// Pinned.\npub const SURREAL_VERSION: &str = \"3.3.0\";\n",
        )
        .unwrap();
        let run = |args: &[&str]| {
            git(
                &root,
                &[
                    &["-c", "user.name=t", "-c", "user.email=t@example.com"][..],
                    args,
                ]
                .concat(),
            )
            .unwrap()
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["add", "-A"]);
        run(&["commit", "-q", "-m", "first"]);
        assert_eq!(surreal_at(&root, "HEAD").as_deref(), Some("3.3.0"));
        fs::write(root.join("crates/tasks/src/server.rs"), "// none\n").unwrap();
        run(&["commit", "-q", "-am", "none"]);
        assert_eq!(surreal_at(&root, "HEAD"), None);
        assert_eq!(surreal_at(&root, "nowhere"), None);
    }

    #[test]
    fn its_own_worktree_left_behind_is_cleared_and_any_other_folder_is_left_as_it_is() {
        let tree = Tree::new();
        let root = tree.path("project");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(tree.path("wt/release-refresh")).unwrap();
        let root = root.canonicalize().unwrap();
        let place = (tree.path("wt/release-refresh").canonicalize().unwrap()).join("project");
        let at = place.to_string_lossy().into_owned();
        let run = |args: &[&str]| {
            git(
                &root,
                &[
                    &["-c", "user.name=t", "-c", "user.email=t@example.com"][..],
                    args,
                ]
                .concat(),
            )
            .unwrap()
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["commit", "-q", "--allow-empty", "-m", "first"]);
        let listed = || -> Vec<String> {
            (run(&["worktree", "list", "--porcelain"]).lines())
                .filter_map(|line| line.strip_prefix("worktree "))
                .map(str::to_string)
                .collect()
        };
        let alone = [root.to_string_lossy().into_owned()];
        let mut log = Log::open(&tree.path("logs/refresh-project.log"));

        // Nothing in its place: nothing to clear.
        clear(&root, &place, &mut log).unwrap();
        assert_eq!(listed(), alone);

        // One an interrupted refresh left, whatever it holds: removed.
        run(&["worktree", "add", "--quiet", "--detach", &at, "HEAD"]);
        fs::write(place.join("left.txt"), "x").unwrap();
        clear(&root, &place, &mut log).unwrap();
        assert!(!place.exists());
        assert_eq!(listed(), alone);

        // Registered, its folder gone: pruned.
        run(&["worktree", "add", "--quiet", "--detach", &at, "HEAD"]);
        fs::remove_dir_all(&place).unwrap();
        clear(&root, &place, &mut log).unwrap();
        assert_eq!(listed(), alone);

        // Any other folder there is someone's: refused, and left as it is.
        fs::create_dir_all(&place).unwrap();
        fs::write(place.join("mine.txt"), "keep").unwrap();
        let refused = clear(&root, &place, &mut log).unwrap_err();
        assert_eq!(
            refused,
            format!(
                "{at} is in the way and is not the release run's worktree of {}: it is left as it is; \
                 move it away and retry",
                root.display()
            )
        );
        assert_eq!(fs::read_to_string(place.join("mine.txt")).unwrap(), "keep");
        assert_eq!(listed(), alone);
    }
}
