//! A green release's refresh (docs/RELEASES.md, "The refresh"): the machine brought to the release
//! once it is tagged and pushed, or once `--refresh` names it again. It is the machine's install of
//! the release and nothing in any project's checkout: no branch, no commit, no pin moved, no main
//! fast-forwarded. A project takes the release's kit and helpers by its own landing.
//!
//! - **The install** ([`super::install`]): the release built once and put in the machine folder,
//!   `current` not yet pointed. One that fails is the refresh's red, `current` as it was.
//! - **The switch.** While a registered project names a plugin linked into the host
//!   ([`switch::held`]), the release is installed and nothing more: no board moves and `current`
//!   is not pointed, the line naming the project that holds it, and `--status` says it too
//!   ([`switch::standing`]). A board moved past the machine's server, a project's build then, would
//!   leave that server reading it alone.
//! - **The boards.** A release that keeps the board schema `current` names moves nothing. One that
//!   raises it, or the first, with no `current`, judges each registered project's live board first,
//!   this repository's aside (its board moves with its own main): a board moves only when every
//!   writer seen there in the last week (the board's writer record, `crates/tasks/src/store/
//!   writers.rs`) is a release's own, a binary of a release installed in this machine folder on
//!   this host, which `current` takes on, or one built at the release's schema ([`lagging`]). A
//!   board whose writers lag is named with each writer to rebuild, the project's to finish, and
//!   holds the release; one whose database does not run, or whose move could put it on a newer
//!   SurrealDB, holds it too, red. A board below schema [`symphony_board::WRITERS_SINCE`], from
//!   before the writer record, is behind every release and never moved here: its own `just tasks
//!   migrate` brings it up. Only when no board holds the release is every board snapshotted
//!   through its running database (`Home::export`), all of them before the first move, so a
//!   snapshot that fails moves nothing; then each is moved in turn by the release's own `tasks
//!   migrate` in its project's main checkout, the one command that moves a project's board.
//! - **`current`**, pointed at the release once nothing holds it ([`install::point`]), which every
//!   recipe of a project with no build of its own then runs. A move that fails stops the rest and
//!   leaves `current` where it was, so does a failure to point it, the install's red. The boards
//!   moved before it stay at the release's schema, ahead of `current`'s, until a retry, which
//!   judges them at the release's schema, nothing to move, and finishes the rest.
//! - **The machine's server.** A release's server follows `current` by its own reload
//!   (`apps/symphony-server/src/reload.rs`): the refresh waits, bounded, until it answers as the
//!   release, or says it has not yet, with what its reload said in its log since `current` moved
//!   ([`server`], [`reload_said`]). A project's build serving takes the release by a stop and a
//!   start, the owner's word, never the refresh's.
//!
//! Each project whose board is judged is one step, `refresh-ID`, ending in one of three: green
//! (moved, or nothing to move); **the project's to finish** (writers to rebuild, a board below the
//! writer record), green here and filed nowhere; or **red**, the refresh's own work failed, filed on
//! the Symphony's board. Nothing here starts, stops or signals a server or a database.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use symphony_api::Front;
use symphony_board::Writer;
use symphony_board::server::{Home, ServerError};

use super::board;
use super::checks::{Ran, Step};
use super::git::git;
use super::group::{Bound, Failed, output_until};
use super::install::{self, Installed};
use super::record::{Refresh, TAKES, THEIRS};
use super::span::Span;
use symphony::children::command;
use symphony::machine::{Entry, Registry, release, switch};

/// What a refresh step's name starts with, before the project's id: no check takes it.
pub(crate) const STEP: &str = "refresh";
/// The snapshots a project's board keeps of its store before a schema move, newest first.
const KEPT_SNAPSHOTS: usize = 5;
/// The longest a refresh that pointed `current` waits for a release's server to follow it: its
/// reload looks once a second and hands over by exec, so a minute's work at the machine's load.
const RELOAD: Duration = Duration::from_secs(5 * 60);
/// The tests' wait in place of [`RELOAD`], in seconds; no flag sets it.
const RELOAD_SECONDS: &str = "SYMPHONY_RELEASE_RELOAD_SECONDS";
/// How often that wait asks the server what it is.
const RELOAD_LOOK: Duration = Duration::from_millis(500);
/// The machine's server's log, in the machine folder (the server's `home::LOG`).
const SERVER_LOG: &str = "server.log";
/// What starts each line the server's reload says in that log.
const RELOAD_LINE: &str = "· Symphony reload: ";
/// The most of that log read for what the reload said: its newest part.
const LOG_READ: u64 = 256 * 1024;
/// The variables no command run in a project keeps, beside the repository git was pointed at
/// (`symphony_board::sys::GIT_VARS`: each command names its own folder): the board, session and
/// task a run of the Symphony's names (a project's command reaches its own), the landing's marker,
/// and the build folder and toolchain, which are the project's own.
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

/// What a refresh runs against: the Symphony's repository (its checkout, which holds the release's
/// commit, and its git common dir), the machine folder, the release and its commit, where the
/// steps' logs go, the `surreal` a snapshot falls back to, and its time.
pub(crate) struct Against<'a> {
    pub(crate) root: &'a Path,
    pub(crate) ours: &'a Path,
    pub(crate) machine: &'a Path,
    pub(crate) tag: &'a str,
    pub(crate) commit: &'a str,
    pub(crate) logs: &'a Path,
    pub(crate) surreal: Option<&'a Path>,
    pub(crate) until: Bound<'a>,
}

/// The refresh of the machine to `against`'s release once `installed` ended, each line said to
/// `out` as it goes and each step's end after them.
pub(crate) fn release(
    against: &Against<'_>,
    mut installed: Installed,
    out: &mut dyn Write,
) -> Refresh {
    let mut refresh = Refresh {
        release: against.tag.to_string(),
        ..Refresh::default()
    };
    for line in std::mem::take(&mut installed.said) {
        say(&mut refresh, out, line);
    }
    let machine = against.machine;
    let named = naming(machine);
    let Some(manifest) = installed.manifest.clone() else {
        say(&mut refresh, out, format!("current is as it was: {named}"));
        refresh.steps.push(installed.ran);
        return told(refresh, out);
    };
    let registry = match Registry::read(machine) {
        Ok(registry) => registry,
        Err(problem) => {
            refresh.steps.push(installed.ran);
            never(
                &mut refresh,
                against,
                STEP,
                format!(
                    "the machine's registry cannot be read, so no board moved and current is as it \
                     was ({named}): {problem}"
                ),
            );
            return told(refresh, out);
        }
    };
    if let Some(hold) = switch::held(&registry) {
        say(
            &mut refresh,
            out,
            format!(
                "the switch to {} is held: {}; {} is installed, no board moves, and current is not \
                 pointed at it ({named})",
                against.tag,
                hold.said(),
                against.tag
            ),
        );
        refresh.steps.push(installed.ran);
        return told(refresh, out);
    }
    let kept = release::current_installed(machine).map(|current| current.serves.schema);
    let mut steps = Vec::new();
    let clear = boards(
        against,
        &registry,
        manifest.serves.schema,
        kept,
        &mut refresh,
        &mut steps,
        out,
    );
    // The server as it answers before `current` moves, to tell its reload's hand-off after.
    let before = clear
        .then(|| symphony::machine::command::running(machine, None))
        .flatten();
    // Its log's length then: what its reload says of the move comes after.
    let logged = fs::metadata(machine.join(SERVER_LOG)).map_or(0, |log| log.len());
    let pointed = clear && install::point(machine, &mut installed);
    if clear {
        let line = installed.said.pop().unwrap_or_default();
        say(&mut refresh, out, line);
    } else {
        say(
            &mut refresh,
            out,
            format!(
                "current is not pointed at {}: {named}, until each board above can move",
                against.tag
            ),
        );
    }
    refresh.steps.push(installed.ran);
    refresh.steps.extend(steps);
    if pointed {
        let line = server(machine, before, logged, against);
        say(&mut refresh, out, line);
    }
    told(refresh, out)
}

/// The refresh to the release `tag` of a machine whose folder cannot be named: the install's red,
/// `installed`, alone, said to `out`.
pub(crate) fn unreached(tag: &str, installed: Installed, out: &mut dyn Write) -> Refresh {
    let mut refresh = Refresh {
        release: tag.to_string(),
        ..Refresh::default()
    };
    for line in installed.said {
        say(&mut refresh, out, line);
    }
    refresh.steps.push(installed.ran);
    told(refresh, out)
}

/// `line` said to `out` and kept in `refresh`.
fn say(refresh: &mut Refresh, out: &mut dyn Write, line: String) {
    let _ = writeln!(out, "release: refresh: {line}");
    refresh.said.push(line);
}

/// The step `name` of `against`'s refresh, red before it ran for `why`, in `refresh`.
fn never(refresh: &mut Refresh, against: &Against<'_>, name: &str, why: String) {
    let step = Step::shell(name, &format!("the refresh to {}", against.tag));
    let log = against.logs.join(format!("{}.log", board::key(name)));
    let _ = fs::create_dir_all(against.logs);
    refresh.steps.push(Ran::never(&step, &log, why));
}

/// What `machine`'s `current` names now, in words.
fn naming(machine: &Path) -> String {
    match release::current(machine) {
        Some(name) => format!("it names {name}"),
        None => "it names no release".into(),
    }
}

/// `refresh`, each step's end said to `out`: green, red, or the project's to finish.
fn told(refresh: Refresh, out: &mut dyn Write) -> Refresh {
    for ran in &refresh.steps {
        let verdict = if refresh.is_theirs(&ran.name) {
            THEIRS
        } else if ran.green() {
            "green"
        } else {
            "red"
        };
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

/// Each registered project's board brought to the release's schema `target` over `kept`, the
/// schema of the release `current` names (none: none named), its lines said in `refresh` and to
/// `out` and its steps put in `steps`: whether nothing holds the release, every board it moves
/// moved.
fn boards(
    against: &Against<'_>,
    registry: &Registry,
    target: i64,
    kept: Option<i64>,
    refresh: &mut Refresh,
    steps: &mut Vec<Ran>,
    out: &mut dyn Write,
) -> bool {
    if let Some(kept) = kept.filter(|kept| target <= *kept) {
        say(refresh, out, no_move(against.tag, target, kept));
        return true;
    }
    let mut judged = Vec::new();
    for entry in &registry.projects {
        match project(entry, against) {
            Err(Some(line)) => say(refresh, out, line),
            Err(None) => {}
            Ok(common) => judged.push(judge(entry, &common, target, against)),
        }
    }
    let holding: Vec<String> = (judged.iter())
        .filter(|board| board.verdict.holds())
        .map(|board| board.id.clone())
        .collect();
    // What keeps a board that can move where it is: the boards that hold the release, a snapshot
    // that failed, or the move that failed before it.
    let mut kept_by = match holding.len() {
        0 => None,
        1 => Some(format!("{} holds the release", holding[0])),
        _ => Some(format!("{} hold the release", holding.join(", "))),
    };
    // Every board's snapshot before the first move, so one that fails moves nothing.
    if kept_by.is_none() {
        for board in &mut judged {
            if !matches!(board.verdict, Verdict::Move { .. }) {
                continue;
            }
            if let Err(why) = snapshot(board, against) {
                kept_by = Some(format!("the snapshot of {}'s store failed", board.id));
                board.verdict = Verdict::Red(why);
                break;
            }
        }
    }
    for mut board in judged {
        let ended = match std::mem::replace(&mut board.verdict, Verdict::Done(String::new())) {
            Verdict::Move { live, leave } => match &kept_by {
                Some(why) => Verdict::Done(format!(
                    "{}: its board can move from schema {live} to {target}, and stays at {live} \
                     while {why}",
                    board.id
                )),
                None => match moved(&mut board, live, leave, against) {
                    Ok(said) => Verdict::Done(said),
                    Err(why) => {
                        kept_by = Some(format!("the move of {}'s board failed", board.id));
                        Verdict::Red(why)
                    }
                },
            },
            verdict => verdict,
        };
        let theirs = matches!(ended, Verdict::Theirs { .. });
        let (ran, line) = board.ended(ended, against);
        if theirs {
            refresh.theirs.push(ran.name.clone());
        }
        say(refresh, out, line);
        steps.push(ran);
    }
    kept_by.is_none()
}

/// Why the release `tag`, serving board schema `target`, moves no board where the release
/// `current` names serves `kept`, at `target` or past it: no board moves back.
fn no_move(tag: &str, target: i64, kept: i64) -> String {
    if target == kept {
        format!(
            "{tag} serves board schema {target}, as the release current names serves {kept}: no \
             board moves"
        )
    } else {
        format!(
            "{tag} serves board schema {target}, behind the release current names, which serves \
             {kept}: no board moves, as none moves back"
        )
    }
}

/// The git common dir of `entry`'s repository, when the refresh judges its board: never this
/// repository's, a project whose id names no folder of its own, whose checkout is gone or that is
/// no git checkout, each but the first said why.
fn project(entry: &Entry, against: &Against<'_>) -> Result<PathBuf, Option<String>> {
    let id = &entry.id;
    if id.is_empty() || id == "." || id == ".." || id.contains('/') {
        return Err(Some(format!(
            "left out: {id}: its id names no folder of its own"
        )));
    }
    if !entry.root.is_dir() {
        return Err(Some(format!(
            "left out: {id}: its checkout {} is gone",
            entry.root.display()
        )));
    }
    match symphony_board::git::checkout_common_dir(&entry.root) {
        Some(common) if common == against.ours => Err(None),
        Some(common) => Ok(common),
        None => Err(Some(format!(
            "left out: {id}: {} is not a git checkout",
            entry.root.display()
        ))),
    }
}

/// One project's board, judged before anything moves.
struct Judged {
    id: String,
    /// Its main checkout, where its migrate runs.
    root: PathBuf,
    home: Home,
    /// Its board's folder in its git common dir.
    folder: PathBuf,
    target: i64,
    verdict: Verdict,
    /// Its store as it was before the move, once [`snapshot`] took it.
    snapshot: Option<PathBuf>,
    name: String,
    log: Log,
    started: Span,
}

/// What a board's judgment, then its move, came to.
enum Verdict {
    /// Green: moved, or nothing to move, said.
    Done(String),
    /// Its project's to finish, said with what finishes it; `holds`, whether the release waits for
    /// it.
    Theirs { why: String, holds: bool },
    /// The refresh's own work cannot judge or move it, said: red, and the release waits for it.
    Red(String),
    /// To move from schema `live`, with `--leave-behind` when `leave`: the only writers behind are a
    /// release's own.
    Move { live: i64, leave: bool },
}

impl Verdict {
    /// Whether the release waits for the board it judged: no board moves, and `current` stays.
    fn holds(&self) -> bool {
        match self {
            Verdict::Theirs { holds, .. } => *holds,
            Verdict::Red(_) => true,
            Verdict::Done(_) | Verdict::Move { .. } => false,
        }
    }
}

impl Judged {
    /// Its step, and the line it says, once it `ended`.
    fn ended(mut self, ended: Verdict, against: &Against<'_>) -> (Ran, String) {
        let id = &self.id;
        let (line, error) = match ended {
            Verdict::Done(said) => (said, None),
            Verdict::Theirs { why, .. } => (format!("{id}: {THEIRS}: {why} ({TAKES})"), None),
            Verdict::Red(why) => (format!("{id}: red: {why}"), Some(why)),
            Verdict::Move { live, .. } => (
                format!("{id}: its board is at schema {live}, not moved"),
                None,
            ),
        };
        self.log.line(&line);
        let ran = Ran {
            name: self.name.clone(),
            command: format!("the refresh of {id}'s board to {}", against.tag),
            exit_code: error.is_none().then_some(0),
            signal: None,
            seconds: self.started.took().seconds,
            log: self.log.path.clone(),
            steps: Vec::new(),
            interrupted: error.is_some() && against.until.interrupted(),
            timed_out: error.is_some()
                && !against.until.interrupted()
                && Instant::now() >= against.until.until,
            error,
            load: None,
        };
        (ran, line)
    }
}

/// `entry`'s board, in the git common dir `common`, judged against the release's schema `target`:
/// what holds it, or what moves it.
fn judge(entry: &Entry, common: &Path, target: i64, against: &Against<'_>) -> Judged {
    let name = format!("{STEP}-{}", entry.id);
    let path = against.logs.join(format!("{}.log", board::key(&name)));
    let mut log = Log::open(&path);
    log.line(&format!(
        "the refresh of {}'s board ({}) to {}, at board schema {target}",
        entry.id,
        entry.root.display(),
        against.tag
    ));
    let folder = common.join(symphony_board::server::FOLDER);
    let home = Home::in_common(common, Some(&surreal_for(common, against.surreal)));
    let mut judged = Judged {
        id: entry.id.clone(),
        root: entry.root.clone(),
        home,
        folder,
        target,
        verdict: Verdict::Done(String::new()),
        snapshot: None,
        name,
        log,
        started: Span::begin(),
    };
    judged.verdict = verdict(&judged, against);
    judged
}

/// [`judge`]'s verdict on `board`.
fn verdict(board: &Judged, against: &Against<'_>) -> Verdict {
    let (id, target) = (&board.id, board.target);
    let root = board.root.display();
    if !board.folder.is_dir() {
        return Verdict::Done(format!(
            "{id}: it keeps no board ({} is not there): nothing to move",
            board.folder.display()
        ));
    }
    let live = match board.home.schema(symphony_board::DATABASE) {
        Ok(Some(live)) => live,
        Ok(None) => {
            return Verdict::Done(format!("{id}: its board is not made yet: nothing to move"));
        }
        Err(ServerError::NotUp(why)) => {
            return Verdict::Red(format!(
                "{why}: its schema and writers were not read, and nothing was moved; once it runs \
                 (any `just tasks` in {root} starts it), an orchestrator's `just release --refresh` \
                 moves it"
            ));
        }
        Err(error) => return Verdict::Red(error.to_string()),
    };
    if live >= target {
        return Verdict::Done(format!(
            "{id}: its board is at schema {live}, {}'s or past it: nothing to move",
            against.tag
        ));
    }
    if live < symphony_board::WRITERS_SINCE {
        return Verdict::Theirs {
            why: format!(
                "its board is at schema {live}, from before the board recorded who writes it \
                 (schema {}), so it is never moved here: `cd {root} && just tasks migrate`, its own, \
                 brings it up",
                symphony_board::WRITERS_SINCE
            ),
            holds: false,
        };
    }
    let writers = match board.home.writers(symphony_board::DATABASE) {
        Ok(writers) => writers,
        Err(error) => {
            return Verdict::Red(format!("{error}: nothing was moved"));
        }
    };
    let (own, behind) = lagging(writers, target, seen_now(), against.machine);
    if !behind.is_empty() {
        return Verdict::Theirs {
            why: format!(
                "its board stays at schema {live}: {} seen there in the last week {} behind {}'s \
                 schema {target}: {}; rebuild each at schema {target} and run it once, which \
                 records it, then an orchestrator's `just release --refresh` moves it",
                match behind.len() {
                    1 => "a writer".to_string(),
                    n => format!("{n} writers"),
                },
                if behind.len() == 1 { "is" } else { "are" },
                against.tag,
                (behind.iter().map(said)).collect::<Vec<_>>().join("; ")
            ),
            holds: true,
        };
    }
    // Fails closed: a version that cannot be read could be one the move replaces the server for.
    let Some(pinned) = surreal_at(against.root, against.commit) else {
        return Verdict::Red(format!(
            "the SurrealDB {} pins cannot be read (crates/tasks/src/server.rs there names no \
             SURREAL_VERSION), so the move might replace its board's database and the refresh stops \
             none: nothing was moved",
            against.tag
        ));
    };
    let running = match board.home.recorded() {
        Some(running) if !running.version.trim().is_empty() => running.version,
        _ => {
            return Verdict::Red(format!(
                "the SurrealDB its board's database runs cannot be read (its record in {} names no \
                 version), so the move might replace that database and the refresh stops none: \
                 nothing was moved",
                board.folder.display()
            ));
        }
    };
    if older(&running, &pinned) {
        return Verdict::Red(format!(
            "its board's database runs SurrealDB {running} and {} pins {pinned}: moving its schema \
             would replace that server, and the refresh stops no board's database, so nothing was \
             moved",
            against.tag
        ));
    }
    Verdict::Move {
        live,
        leave: !own.is_empty(),
    }
}

/// The writers of `lines` seen in the last week before `now` that are behind `target`, as two
/// lists: a release's own (a binary of a release installed in the machine folder `machine`, on this
/// host), which `current` takes on, and the rest, which hold a move.
fn lagging(
    lines: Vec<Writer>,
    target: i64,
    now: f64,
    machine: &Path,
) -> (Vec<Writer>, Vec<Writer>) {
    let releases = machine
        .join(release::RELEASES)
        .canonicalize()
        .unwrap_or_else(|_| machine.join(release::RELEASES));
    let host = symphony_board::sys::hostname();
    let behind = (lines.into_iter())
        .filter(|line| line.schema < target && now - line.seen <= symphony_board::WRITERS_RECENT);
    let (own, mut rest): (Vec<Writer>, Vec<Writer>) = behind
        .partition(|line| line.host == host && of_a_release(Path::new(&line.path), &releases));
    rest.sort_by(|a, b| (&a.program, &a.host, &a.path).cmp(&(&b.program, &b.host, &b.path)));
    (own, rest)
}

/// Whether `path` is a binary of a release in `releases`: `releases/NAME/bin/BINARY`.
fn of_a_release(path: &Path, releases: &Path) -> bool {
    let Ok(rest) = path.strip_prefix(releases) else {
        return false;
    };
    let parts: Vec<_> = rest.components().collect();
    parts.len() == 3 && parts[1].as_os_str() == "bin"
}

/// A writer, in words.
fn said(writer: &Writer) -> String {
    format!(
        "{} at schema {} ({} on {}, last seen {} ago)",
        writer.program,
        writer.schema,
        writer.path,
        writer.host,
        symphony_board::board_health::ago(seen_now() - writer.seen)
    )
}

/// The time a writer's line is read by: this machine's clock, as the line is written by.
fn seen_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |since| since.as_secs_f64())
}

/// `board`'s store exported through its running database before any board moves, into
/// `snapshots/before-schema-TARGET-RELEASE.surql` in its folder, the newest [`KEPT_SNAPSHOTS`]
/// kept: kept in `board`, or why it failed.
fn snapshot(board: &mut Judged, against: &Against<'_>) -> Result<(), String> {
    let snapshots = board.folder.join("snapshots");
    let snapshot = snapshots.join(format!(
        "before-schema-{}-{}.surql",
        board.target, against.tag
    ));
    board
        .home
        .export(symphony_board::DATABASE, &snapshot)
        .map_err(|e| {
            format!(
                "the snapshot of its store failed, so no board was moved, this one or another: {e}"
            )
        })?;
    board
        .log
        .line(&format!("its store, as it ran: {}", snapshot.display()));
    prune(&snapshots, KEPT_SNAPSHOTS);
    board.snapshot = Some(snapshot);
    Ok(())
}

/// `board`, snapshotted ([`snapshot`]), moved from schema `live` to its target by the release's
/// own `tasks migrate` in its project's main checkout, `--leave-behind` when `leave`: what it did,
/// or why it failed.
fn moved(
    board: &mut Judged,
    live: i64,
    leave: bool,
    against: &Against<'_>,
) -> Result<String, String> {
    let (id, target) = (board.id.clone(), board.target);
    let root = board.root.display().to_string();
    let Some(snapshot) = board.snapshot.clone() else {
        return Err("its store was not snapshotted, so it was not moved".into());
    };
    let tasks = release::binary(against.machine, against.tag, "tasks");
    let tasks = tasks.to_string_lossy().into_owned();
    let mut words = vec![tasks.as_str(), "migrate"];
    if leave {
        words.push("--leave-behind");
    }
    let command = words[1..].join(" ");
    board
        .log
        .run(&board.root, &words, against.until)
        .map_err(|why| {
            format!(
                "{}'s `tasks {command}` in {root} failed ({why}); its store as it was before is in \
                 {}, and an orchestrator's `just release --refresh` retries",
                against.tag,
                snapshot.display()
            )
        })?;
    match board.home.schema(symphony_board::DATABASE) {
        Ok(Some(moved)) if moved >= target => Ok(format!(
            "{id}: its board moved from schema {live} to {moved} by {}'s own `tasks {command}`, its \
             store snapshotted first to {}",
            against.tag,
            snapshot.display()
        )),
        read => Err(format!(
            "{}'s `tasks {command}` in {root} ended, but its board reads {}, short of schema \
             {target}; its store as it was before is in {}",
            against.tag,
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
    let mut found: Vec<(SystemTime, PathBuf)> = (entries.filter_map(Result::ok))
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

/// What the machine's server does with the release `current` now names, said: the server this
/// machine's folder `machine` records, as it answered before `current` moved (`before`), waited for
/// while it is another release's, bounded ([`follows`]); one that did not follow is said with what
/// its reload wrote in its log since that log was `logged` bytes long ([`reload_said`]).
fn server(machine: &Path, before: Option<Front>, logged: u64, against: &Against<'_>) -> String {
    let tag = against.tag;
    let Some(front) = before else {
        return format!(
            "the machine's server: none answers on this machine, so none follows current; the next \
             start of the one server (`just symphony`) runs {tag} when it serves every registered \
             project"
        );
    };
    let named = format!(
        "the machine's server (process {}, port {})",
        front.pid, front.port
    );
    match front.release.as_deref() {
        Some(running) if running == tag => format!("{named} is {tag} already"),
        Some(running) => match follows(machine, against) {
            Ok(new) => format!(
                "{named}, the release {running}, followed current by its own reload \
                 (apps/symphony-server/src/reload.rs): process {} on port {} answers as {tag}",
                new.pid, new.port
            ),
            Err(waited) => format!(
                "{named} still answers as the release {running} after {} s; {}. Its own reload \
                 (apps/symphony-server/src/reload.rs) follows current once a second, and one whose \
                 server does not take over leaves it serving, says why in its log and on its notice \
                 and tries again (`just symphony-status` says which answers)",
                waited.as_secs(),
                reload_said(machine, logged)
            ),
        },
        None => format!(
            "{named} is the build of {}, whose reload follows that checkout and never current: a \
             stop and a start (`just symphony --stop`, then `just symphony`), the owner's word, make \
             {tag} the machine's server",
            front.root
        ),
    }
}

/// What the machine's server's reload said in its log (`machine`'s [`SERVER_LOG`]) of a `current`
/// that moved when that log was `from` bytes long: its last line since, which names the hand-off
/// it began or why the release did not take over. With none since, its last line before, said as
/// that: a reload that waits to try a release again writes nothing until that wait is over, as
/// when `current` named the release already and this refresh is a retry, so no line since is no
/// sign it stopped. Only with no line of it at all is it said that it may not be running, and
/// never as a fact: a server started in a terminal keeps no log, and only the log's newest
/// [`LOG_READ`] bytes are read. A log shorter than `from`, started again since, is all since.
fn reload_said(machine: &Path, from: u64) -> String {
    let path = machine.join(SERVER_LOG);
    let read = || {
        let mut log = File::open(&path).ok()?;
        let length = log.metadata().ok()?.len();
        let start = length.saturating_sub(LOG_READ);
        log.seek(SeekFrom::Start(start)).ok()?;
        let mut said = Vec::new();
        log.read_to_end(&mut said).ok()?;
        let since = if from > length { 0 } else { from };
        // How far into what was read the move is.
        Some((said, usize::try_from(since.saturating_sub(start)).ok()?))
    };
    let Some((said, since)) = read() else {
        return format!(
            "it keeps no log at {} to say why (a server started in a terminal says it there)",
            path.display()
        );
    };
    let last = |bytes: &[u8]| {
        let text = String::from_utf8_lossy(bytes);
        let line = text.lines().rev().find(|line| line.contains(RELOAD_LINE))?;
        Some(line.trim().to_string())
    };
    let since = since.min(said.len());
    if let Some(line) = last(&said[since..]) {
        return format!("its log ({}) says last: {line}", path.display());
    }
    match last(&said[..since]) {
        Some(line) => format!(
            "its reload wrote no line in its log ({}) since current moved, as one that waits to \
             try a release again writes none; its last line before: {line}",
            path.display()
        ),
        None => format!(
            "the newest {} KiB of its log ({}) hold no line of its reload, where one that follows \
             says each hand-off it begins: its reload may not be running, which a line starting \
             `{}` further back in that log, or none in it, tells",
            LOG_READ / 1024,
            path.display(),
            RELOAD_LINE.trim()
        ),
    }
}

/// The server this machine's folder `machine` records once it answers as `against`'s release: its
/// reload's hand-off. Waited for within [`RELOAD`] (the tests' [`RELOAD_SECONDS`]) and the
/// refresh's time; else how long it was waited for.
fn follows(machine: &Path, against: &Against<'_>) -> Result<Front, Duration> {
    let wait = (std::env::var(RELOAD_SECONDS).ok())
        .and_then(|seconds| seconds.parse::<u64>().ok())
        .map_or(RELOAD, Duration::from_secs);
    let began = Instant::now();
    let until = (began + wait).min(against.until.until);
    loop {
        if let Some(now) = symphony::machine::command::running(machine, None)
            && now.release.as_deref() == Some(against.tag)
        {
            return Ok(now);
        }
        if Instant::now() >= until || against.until.interrupted() {
            return Err(until.saturating_duration_since(began));
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "no-timed-waits baseline: looks again for the reloaded server, which no event announces"
        )]
        std::thread::sleep(RELOAD_LOOK.min(until.saturating_duration_since(Instant::now())));
    }
}

/// A step's log: each command it ran, with all it printed.
struct Log {
    path: PathBuf,
    file: Option<File>,
}

impl Log {
    fn open(path: &Path) -> Log {
        if let Some(folder) = path.parent() {
            let _ = fs::create_dir_all(folder);
        }
        Log {
            path: path.to_path_buf(),
            file: File::create(path).ok(),
        }
    }

    fn line(&mut self, text: &str) {
        if let Some(file) = &mut self.file {
            let _ = writeln!(file, "{text}");
        }
    }

    /// `words` run in `dir` within `until`, without the variables [`UNSET`] and
    /// [`symphony_board::sys::GIT_VARS`] name: what it printed, or why it failed, its output logged
    /// either way.
    fn run(&mut self, dir: &Path, words: &[&str], until: Bound<'_>) -> Result<String, String> {
        let line = words.join(" ");
        self.line(&format!("$ {line}   (in {})", dir.display()));
        let mut run = command(words[0]);
        run.args(&words[1..])
            .current_dir(dir)
            .env("GIT_TERMINAL_PROMPT", "0");
        for name in UNSET.iter().chain(&symphony_board::sys::GIT_VARS) {
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
        let status = (done.status.code())
            .map_or_else(|| "was killed".to_string(), |code| format!("exited {code}"));
        let why = format!(
            "`{line}` {status}{}",
            if last.is_empty() {
                String::new()
            } else {
                format!(": {last}")
            }
        );
        self.line(&why);
        Err(why)
    }
}

#[cfg(test)]
#[path = "refresh_tests.rs"]
mod tests;
