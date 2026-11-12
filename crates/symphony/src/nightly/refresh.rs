//! A green release's refresh (docs/RELEASES.md), on the owner's word of 2026-11-12 ("yes, a green
//! release should refresh"): what a release does for the machine once it is tagged and pushed, or
//! once `--refresh` names it again. Each project the machine serves that builds on the Symphony
//! ([`conciliate::consumers`]) is one step, `refresh-ID`, which moves what it can and nothing more:
//!
//! - **Its pin.** While its default branch's `Cargo.lock` takes an older Symphony, the move to the
//!   release ([`pin`]) is made in a worktree of the project's own beside the nightly's checkout
//!   (`NIGHTLY-refresh/ID`, detached at that branch and locked), and committed there through the
//!   project's own hooks, its commit gate, never past them. A green commit is left on the branch
//!   `symphony/RELEASE` of the project's repository and never pushed: the project's own landing
//!   lands it, as its land command and approval decide, and nothing here moves its main.
//! - **Its board.** Once its default branch takes the release, its live board is moved to the
//!   schema its pin holds by `just tasks migrate` in its main checkout, the one command that moves
//!   a project's board, after a snapshot of the store as it runs (`Home::export`). A board whose
//!   database is not running is refused, since nothing here starts one, and so is one the move
//!   would put on a newer SurrealDB, which would replace the running server.
//!
//! Then the shared server is said: it takes the release by its own reload once the main checkout
//! it is built from does. Nothing here stops, signals or restarts a server or a database, and a red
//! step leaves its project as it was, but for a branch it made.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

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
pub(crate) const REASON: &str = "the Symphony nightly's refresh";
/// The prefix of the branch a project's pin moves on, before the release's name.
pub(crate) const BRANCH: &str = "symphony/";
/// What a refresh step's name starts with, before the project's id: no check takes it.
pub(crate) const STEP: &str = "refresh";
/// The snapshots a project's board keeps of its store before a schema move, newest first.
const KEPT_SNAPSHOTS: usize = 5;
/// The variables no command run in a project keeps: the board, session and task a run of the
/// Symphony's names (a project's command reaches its own), the landing's marker, and the build
/// folder and toolchain, which are the project's own.
const UNSET: [&str; 9] = [
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
        let _ = writeln!(out, "nightly: refresh: {line}");
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
        let (ran, said, takes) = step(&consumer, &name, &names, against);
        for line in said {
            say(&mut refresh, line);
        }
        if let Some(ran) = ran {
            refresh.steps.push(ran);
            taking.push((consumer, takes));
        }
    }
    let server = server(&machine, &taking, against);
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
            "nightly: {} {verdict} ({:.0} s){why}; log {}",
            ran.name,
            ran.seconds,
            ran.log.display()
        );
    }
    refresh
}

/// What one project's step did: its record (none when it takes nothing from the Symphony by
/// git), what it said, and whether its default branch takes the release.
type Stepped = (Option<Ran>, Vec<String>, bool);

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
        error: None,
        timed_out: false,
        interrupted: false,
    };
    match done {
        Ok(Done::Skipped(why)) => (None, vec![format!("left out: {why}")], false),
        Ok(Done::Refreshed) => (Some(ran), found.said, found.takes),
        Err(why) => {
            log.line(&format!("red: {why}"));
            ran.exit_code = None;
            ran.interrupted = against.until.interrupted();
            ran.timed_out = !ran.interrupted && Instant::now() >= against.until.until;
            ran.error = Some(why);
            (Some(ran), found.said, found.takes)
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
/// it among them, and whether its default branch takes the release.
#[derive(Default)]
struct Found {
    said: Vec<String>,
    takes: bool,
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
    let takes = pinned
        .iter()
        .all(|pin| is_ancestor(against.root, against.commit, pin).unwrap_or(false));
    if !takes {
        let read = Branch {
            tip: &tip,
            lock: &lock,
            locked: &locked,
            sources: &plan.sources,
            names,
            manifests: &manifests(&metadata, &consumer.root),
        };
        found.said.push(prepare(consumer, &read, against, log)?);
        found.said.push(format!(
            "{id}: its board moves once its {branch} takes {}",
            against.tag
        ));
        return Ok(Done::Refreshed);
    }
    let at = pinned.iter().next().copied().unwrap_or_default();
    found.takes = true;
    found.said.push(format!(
        "{id}: its {branch} takes {} already (its lock pins {})",
        against.tag,
        short(at)
    ));
    found.said.push(board_move(consumer, at, against, log)?);
    Ok(Done::Refreshed)
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
/// tag, `cargo update --workspace` moves its lock, and the commit goes through its own hooks.
/// What it made.
fn prepare(
    consumer: &Consumer,
    read: &Branch<'_>,
    against: &Against<'_>,
    log: &mut Log,
) -> Result<String, String> {
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
    if let Ok(made) = commit(&consumer.root, &format!("refs/heads/{branch}")) {
        return Ok(format!(
            "{id}: its pin to {} is on the branch {branch} ({}) already, for its own landing",
            against.tag,
            short(&made)
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
    let stray: Vec<&str> = (staged.lines())
        .filter(|path| {
            *path != "Cargo.lock" && !(*path == "Cargo.toml" || path.ends_with("/Cargo.toml"))
        })
        .collect();
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
    git(&consumer.root, &["branch", &branch, &made])?;
    let also = if others.is_empty() {
        String::new()
    } else {
        format!("; the move also takes {}", others.join(", "))
    };
    Ok(format!(
        "{id}: its pin to {} is on the branch {branch} ({}), made through its own commit gate and \
         not pushed; its own landing lands it{also}",
        against.tag,
        short(&made)
    ))
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
         Made by the Symphony nightly's refresh of that release (docs/RELEASES.md in the \
         Symphony), for this project's own landing.\n",
        tag = against.tag,
        from = from.join(", "),
        commit = short(against.commit),
    )
}

/// Make the place `tree` ready for a worktree of `project`: the nightly's own worktree there, from
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
            "{} is in the way and is not the nightly's worktree of {}: it is left as it is; move \
             it away and retry",
            tree.display(),
            project.display()
        ));
    }
    Ok(())
}

/// The nightly's worktree of a project, removed with it, whatever it holds.
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
    let home = Home::in_common(&common, Some(&surreal_for(&common, against.surreal)));
    let live = match home.schema(symphony_board::DATABASE) {
        Ok(Some(live)) => live,
        Ok(None) => return Ok(format!("{id}: its board is not made yet: nothing to move")),
        Err(ServerError::NotUp(why)) => {
            return Err(format!(
                "{why}: its schema was not read, snapshotted or moved; once it runs (any `just \
                 tasks` in {root} starts it), `just nightly --refresh` moves it"
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
    if let (Some(running), Some(pinned)) = (home.recorded(), surreal_at(against.root, at))
        && older(&running.version, &pinned)
    {
        return Err(format!(
            "its board's database runs SurrealDB {} and its pin {} pins {pinned}: moving its \
             schema would replace that server, and the refresh stops no board's database, so \
             nothing was moved; `cd {root} && just tasks migrate` moves it when its owner says",
            running.version,
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

/// The `surreal` a snapshot of the board in the git common dir `common` runs: `SF_SURREAL`, else
/// the one pinned beside that board, else `fallback` (the Symphony's own).
fn surreal_for(common: &Path, fallback: Option<&Path>) -> PathBuf {
    let named = std::env::var_os("SF_SURREAL").map(PathBuf::from);
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
/// records, and whether the main checkout it is built from takes the release (`taking`, each
/// project refreshed and whether its default branch takes it).
fn server(machine: &Path, taking: &[(Consumer, bool)], against: &Against<'_>) -> String {
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
    match taking.iter().find(|(consumer, _)| same(&consumer.root)) {
        Some((consumer, true)) => format!(
            "{named}, whose {} takes {}: its own reload builds, installs and replaces it, and a \
             failed build leaves it serving (`just symphony` in {} starts that build through the \
             host rule)",
            consumer.branch,
            against.tag,
            root.display()
        ),
        Some((consumer, false)) => format!(
            "{named}: it takes {} by its own reload once {}'s {} lands its pin",
            against.tag, consumer.id, consumer.branch
        ),
        None if same(against.main) => {
            format!("{named}, this repository's main checkout, which its own reload follows")
        }
        None => format!("{named}, which takes nothing of the Symphony's from this release"),
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
             nightly's refresh of that release (docs/RELEASES.md in the Symphony), for this \
             project's own landing.\n"
        );
        assert!(
            message(&against, &["a", "b"], &["serde 1.0.200 → 1.0.210".into()]).contains(
                "from a, b to it (0123456789ab), and, as the release needs, serde 1.0.200 → \
                 1.0.210."
            )
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
}
