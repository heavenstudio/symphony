//! What a refresh does with the machine's schema updates (docs/workflow/RELEASES.md, "An update"), kept in
//! the machine folder's ledger ([`symphony::machine::updates`]): once the boards and `current` are
//! settled and the machine's server waited for, each registered project's board, this
//! repository's included, is read again.
//!
//! - **Its goal.** This repository's board is to be written at the schema it is at; a project's
//!   board at the release's schema, or its own when it is past it.
//! - **Complete** once the board is at its goal and the machine's server writes it: its project list
//!   says so (`writes`), or, from a server that says nothing of writes, it is at that schema itself.
//!   A server at the goal that refuses or only reads the board is no completion, and one that writes
//!   a board ahead of it is.
//! - **Failed** when the move of its board failed (a red) or its project holds it (its writers, a
//!   board from before the writer record), whatever serves; or when a green release serving the
//!   goal is installed, a server answers, and the update is not complete. With no release serving
//!   it yet, no server answering, or a board that only waits for another's move, it waits, and
//!   nothing is filed.
//!
//! The first failure files one task, on the board that can act ([`owner`]): the project's own when
//! its project holds the board and a release installed here writes that board's schema, else this
//! repository's. Each failure, the first too, is a post on that same task, found by the ledger, and
//! the refresh that completes the update closes it. One completed the first time files nothing.
//! An update ends, its task closed, when its board is found below it (it went back: served or not,
//! a later arrival is a new generation and never overwrites the old entry), past it, or still at it
//! under a higher goal, unless the server writes it there (then it is served); and when its project
//! is no longer registered. Nothing here moves a board, points `current` or touches a server or a
//! database: it reads, and writes the ledger and the tasks.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use symphony::machine::release::Manifest;
use symphony::machine::updates::{self, Failure, Ledger, Stamp, Task, Update};
use symphony::machine::{Entry, Registry};
use symphony::taskcli::CliBoard;
use symphony_api::Front;
use symphony_board::server::Home;

use super::board::{self, Reds};
use super::refresh::{Against, End, Ended, SERVER_LOG, UNSET, surreal_for};

/// What a refresh found once it settled the boards, `current` and the server.
pub(crate) struct Look<'a> {
    pub(crate) against: &'a Against<'a>,
    pub(crate) registry: &'a Registry,
    /// The release refreshed to.
    pub(crate) release: &'a Manifest,
    /// The schema of the release `current` named before.
    pub(crate) kept: Option<i64>,
    /// Whether this refresh pointed `current` at the release.
    pub(crate) pointed: bool,
    /// How each board the refresh judged ended.
    pub(crate) ends: &'a [Ended],
    /// The machine's server as it answers now, and what its project list says it writes: by id,
    /// whether it writes the board, none where it cannot say yet; a project an older server says
    /// nothing of is not listed ([`symphony::machine::command::writes`], [`unsure`]).
    pub(crate) front: Option<Front>,
    pub(crate) writes: Option<Vec<(String, Option<bool>)>>,
    /// What the refresh said of the server's following `current`, when it waited for it.
    pub(crate) server: Option<&'a str>,
    pub(crate) now: f64,
}

/// The boards an update's task is filed on.
pub(crate) trait Boards {
    /// This repository's board.
    fn ours(&self, main: &Path) -> Box<dyn Reds>;
    /// A project's board, at its checkout `root`, through the board binary `tasks` (a release's own
    /// of that board's schema).
    fn theirs(&self, root: &Path, tasks: &Path) -> Box<dyn Reds>;
}

/// The boards through the board's binary: this repository's as every red is filed, a project's
/// through a release's own `tasks` run in its main checkout without this run's board variables.
pub(crate) struct CliBoards;

impl Boards for CliBoards {
    fn ours(&self, main: &Path) -> Box<dyn Reds> {
        Box::new(CliBoard::new(main))
    }

    fn theirs(&self, root: &Path, tasks: &Path) -> Box<dyn Reds> {
        let mut cleared: Vec<&str> = UNSET.to_vec();
        cleared.extend(symphony_board::sys::GIT_VARS);
        Box::new(
            CliBoard::with_command(vec![tasks.to_string_lossy().into_owned()], &[], root)
                .without(&cleared),
        )
    }
}

/// One registered board, read again after the refresh.
struct Seen<'a> {
    entry: &'a Entry,
    common: PathBuf,
    ours: bool,
    /// Its schema now; none for a board the refresh judged red whose database does not answer.
    live: Option<i64>,
    end: Option<&'a End>,
}

/// The ledger brought up to what `look` found, each update's task filed, told or closed on
/// `boards`: the lines the refresh says of it.
pub(crate) fn reconcile(look: &Look<'_>, boards: &dyn Boards) -> Vec<String> {
    let machine = look.against.machine;
    let mut lines = Vec::new();
    let mut ledger = match Ledger::read(machine) {
        Ok(ledger) => ledger,
        Err(why) => {
            return vec![format!(
                "updates: the ledger cannot be read, so no update is recorded or filed: {why}"
            )];
        }
    };
    let before = ledger.clone();
    for seen in read(look) {
        lines.extend(one(look, boards, &mut ledger, &seen));
    }
    lines.extend(unregistered(look, boards, &mut ledger));
    if ledger != before
        && let Err(why) = ledger.write(machine)
    {
        lines.push(format!("updates: the ledger cannot be written: {why}"));
    }
    lines
}

/// Each open update of a project no longer registered ended, its task closed where its board can
/// be reached (the Symphony's own): said.
fn unregistered(look: &Look<'_>, boards: &dyn Boards, ledger: &mut Ledger) -> Vec<String> {
    let mut lines = Vec::new();
    for update in &mut ledger.updates {
        let registered = (look.registry.projects.iter()).any(|entry| entry.id == update.project);
        if registered || !update.open() {
            continue;
        }
        let why = "its project is no longer registered on this machine";
        update.left = Some(Stamp {
            at: look.now,
            source: format!("this refresh found it ended: {why}"),
        });
        lines.push(format!("updates: {} ended unserved: {why}", update.id));
        lines.extend(close(look, boards, update, why));
    }
    lines
}

/// Each registered board this refresh can read, this repository's included, and each the refresh
/// judged red or its project's whose schema cannot be read now (its database down): a project with
/// no board, or one not made yet, is passed over, its updates left as they are.
fn read<'a>(look: &'a Look<'a>) -> Vec<Seen<'a>> {
    let mut seen = Vec::new();
    for entry in &look.registry.projects {
        if entry.id.is_empty() || entry.id.contains('/') || !entry.root.is_dir() {
            continue;
        }
        let Some(common) = symphony_board::git::checkout_common_dir(&entry.root) else {
            continue;
        };
        if !common.join(symphony_board::server::FOLDER).is_dir() {
            continue;
        }
        let home = Home::in_common(&common, Some(&surreal_for(&common, look.against.surreal)));
        let end = (look.ends.iter())
            .find(|ended| ended.id == entry.id)
            .map(|ended| &ended.end);
        let live = home.schema(symphony_board::DATABASE).ok().flatten();
        if live.is_none() && !matches!(end, Some(End::Red { .. } | End::Theirs(_))) {
            continue;
        }
        let ours = common == look.against.ours;
        seen.push(Seen {
            entry,
            common,
            ours,
            live,
            end,
        });
    }
    seen
}

/// The schema `seen`'s board is to be written at: its own for this repository's, else the
/// release's, or its own past it; the release's where it cannot be read.
fn goal(look: &Look<'_>, seen: &Seen<'_>) -> i64 {
    let release = look.release.serves.schema;
    match seen.live {
        Some(live) if seen.ours => live,
        Some(live) => live.max(release),
        None => release,
    }
}

/// How long a refresh waits, once the machine's server answers as the release, for it to say
/// whether it writes each board there is: a server that has just taken over reads its boards in its
/// first seconds.
pub(crate) const SETTLE: Duration = Duration::from_secs(30);

/// The registered projects whose board is there: those whose server's word on writes a refresh
/// waits for.
pub(crate) fn with_boards(registry: &Registry) -> Vec<String> {
    (registry.projects.iter())
        .filter(|entry| {
            symphony_board::git::checkout_common_dir(&entry.root)
                .is_some_and(|common| common.join(symphony_board::server::FOLDER).is_dir())
        })
        .map(|entry| entry.id.clone())
        .collect()
}

/// Whether the machine's server's project list (`said`) cannot say yet whether it writes one of
/// `boards`; never where no server answers, nor for a project it says nothing of writes for.
pub(crate) fn unsure(boards: &[String], said: Option<&[(String, Option<bool>)]>) -> bool {
    (said.into_iter().flatten()).any(|(id, writes)| writes.is_none() && boards.contains(id))
}

/// What the machine's server's project list says of `seen`'s board: whether it writes it, none
/// where it cannot say yet; none at all where it says nothing of writes (an older server).
fn listed(look: &Look<'_>, seen: &Seen<'_>) -> Option<Option<bool>> {
    (look.writes.as_ref())
        .and_then(|listed| listed.iter().find(|(id, _)| *id == seen.entry.id))
        .map(|(_, writes)| *writes)
}

/// Whether the machine's server writes `seen`'s board at `schema`: its project list's word, else,
/// from a server that says nothing of writes, its own schema; never while none answers, nor while
/// its list cannot say yet.
fn written(look: &Look<'_>, seen: &Seen<'_>, schema: i64) -> Option<&'static str> {
    if seen.live != Some(schema) {
        return None;
    }
    let front = look.front.as_ref()?;
    match listed(look, seen) {
        Some(Some(true)) => Some("its project list says it writes the board"),
        Some(_) => None,
        None if front.schema == schema => {
            Some("at the board's schema; its project list says nothing of writes")
        }
        None => None,
    }
}

/// The release installed here that serves `schema`, the release refreshed to first, then the
/// earliest installed: its name and when it was installed (its manifest's time).
fn serving(look: &Look<'_>, schema: i64) -> Option<(String, Stamp)> {
    let machine = look.against.machine;
    let stamp = |name: &str| Stamp {
        at: installed_at(machine, name).unwrap_or(look.now),
        source: format!("{name}'s manifest, written by its install"),
    };
    if look.release.serves.schema == schema {
        return Some((look.release.release.clone(), stamp(&look.release.release)));
    }
    installed(machine)
        .into_iter()
        .filter(|manifest| manifest.serves.schema == schema)
        .map(|manifest| {
            let at = stamp(&manifest.release);
            (manifest.release, at)
        })
        .min_by(|a, b| a.1.at.total_cmp(&b.1.at))
}

/// When the release `name` was installed in the machine folder `machine` ([`updates::installed_at`]).
fn installed_at(machine: &Path, name: &str) -> Option<f64> {
    updates::installed_at(machine, name)
}

/// Every release installed whole in the machine folder `machine` ([`updates::every_installed`]).
fn installed(machine: &Path) -> Vec<Manifest> {
    updates::every_installed(machine)
}

/// The board binary that writes a board at `schema` ([`updates::writer_of`]).
fn writer(machine: &Path, schema: i64) -> Option<PathBuf> {
    updates::writer_of(machine, schema)
}

/// When `seen`'s board reached `schema`, as a record says, once it is there: its
/// `schema-seen.log`, where the first store that saw it at that schema wrote it (the move was then,
/// or just before); else the move this refresh made; else none.
fn reached(seen: &Seen<'_>, schema: i64) -> Option<Stamp> {
    if seen.live != Some(schema) {
        return None;
    }
    if let Some(stamp) = updates::seen_at(&seen.common, schema) {
        return Some(stamp);
    }
    match seen.end {
        Some(End::Moved { at, .. }) => Some(Stamp {
            at: *at,
            source: "this refresh's move, by its clock when the move was done".into(),
        }),
        _ => None,
    }
}

/// When the machine's server began writing at `schema`: its reload's hand-off line in its log for
/// the process that answers now, when that came after `after`; else this refresh's look, which
/// bounds it from above.
fn served(look: &Look<'_>, after: Option<f64>) -> Stamp {
    let handed = look
        .front
        .as_ref()
        .and_then(|front| hand_off(look.against.machine, front.pid))
        .filter(|at| after.is_none_or(|after| *at >= after));
    match handed {
        Some(at) => Stamp {
            at,
            source: format!(
                "the server's hand-off: its reload's line in {SERVER_LOG}, the new server ready"
            ),
        },
        None => Stamp {
            at: look.now,
            source: "this refresh found the server writing it: then or before".into(),
        },
    }
}

/// When the server's log says the process `pid` took over by its reload's hand-off (`· Symphony
/// reload: replacement pid PID ready`), by the line's own time; none when the newest part of the
/// log says nothing of it.
fn hand_off(machine: &Path, pid: i64) -> Option<f64> {
    const READ: u64 = 4 * 1024 * 1024;
    let mut log = fs::File::open(machine.join(SERVER_LOG)).ok()?;
    let length = log.metadata().ok()?.len();
    log.seek(SeekFrom::Start(length.saturating_sub(READ)))
        .ok()?;
    let mut text = Vec::new();
    log.read_to_end(&mut text).ok()?;
    let text = String::from_utf8_lossy(&text);
    let needle = format!("Symphony reload: replacement pid {pid} ready");
    let line = text.lines().rev().find(|line| line.contains(&needle))?;
    utc(line.strip_prefix('[')?.split(']').next()?)
}

/// `YYYY-MM-DDTHH:MM:SS[.fff]Z`, a time the server's log writes in UTC, as seconds since the epoch;
/// none for anything else.
fn utc(text: &str) -> Option<f64> {
    let text = text.strip_suffix('Z')?;
    let (date, time) = text.split_once('T')?;
    let mut day = date.splitn(3, '-').map(str::parse::<i64>);
    let (year, month, mday) = (day.next()?.ok()?, day.next()?.ok()?, day.next()?.ok()?);
    let mut clock = time.splitn(3, ':');
    let hour: i64 = clock.next()?.parse().ok()?;
    let minute: i64 = clock.next()?.parse().ok()?;
    let seconds = clock.next()?;
    let (second, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
    let second: i64 = second.parse().ok()?;
    let valid = (1..=12).contains(&month)
        && (1..=31).contains(&mday)
        && (0..24).contains(&hour)
        && (0..60).contains(&minute)
        && (0..61).contains(&second)
        && fraction.bytes().all(|byte| byte.is_ascii_digit());
    if !valid {
        return None;
    }
    // Days since 1970-01-01 of the proleptic Gregorian date (Howard Hinnant's days_from_civil).
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let of_era = year - era * 400;
    let of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + mday - 1;
    let of_cycle = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
    let days = era * 146_097 + of_cycle - 719_468;
    // Read whole, so the fraction is the nearest the decimal has, never a sum's rounding.
    let whole = days * 86_400 + hour * 3600 + minute * 60 + second;
    let fraction = if fraction.is_empty() { "0" } else { fraction };
    format!("{whole}.{fraction}").parse().ok()
}

/// What one board's updates come to, said; the ledger changed in place.
fn one(look: &Look<'_>, boards: &dyn Boards, ledger: &mut Ledger, seen: &Seen<'_>) -> Vec<String> {
    let id = &seen.entry.id;
    let goal = goal(look, seen);
    let mut lines = Vec::new();
    if let Some(live) = seen.live {
        lines.extend(ended(look, boards, ledger, seen, live, goal));
    }
    let complete = written(look, seen, goal);
    let reached = reached(seen, goal);
    // The entry this arrival is: the newest of the board at its goal, unless it ended, or the
    // board's record dates this arrival after the one it served.
    let index = ledger
        .updates
        .iter()
        .enumerate()
        .filter(|(_, u)| u.project == *id && u.schema == goal)
        .max_by_key(|(_, u)| u.generation)
        .map(|(index, _)| index);
    let again = index.is_some_and(|index| {
        let newest = &ledger.updates[index];
        newest.left.is_some() || (newest.served.is_some() && later(reached.as_ref(), newest))
    });
    let index = match index {
        Some(index) if !again => index,
        _ => {
            let generation = index.map_or(1, |index| ledger.updates[index].generation + 1);
            // A board written at a schema this refresh neither moved it to nor pointed current at
            // is no update: it was written before, and there is nothing to measure.
            let happened = matches!(seen.end, Some(End::Moved { .. }))
                || (look.pointed && look.kept != Some(goal));
            if complete.is_some() && !happened {
                return lines;
            }
            ledger.updates.push(Update::new(id, goal, generation, None));
            ledger.updates.len() - 1
        }
    };
    let update = &mut ledger.updates[index];
    // When it reached its schema, once it is there: the record's time, else this refresh's, the
    // first to find it there, which bounds it from above.
    if update.open() && update.reached.is_none() && seen.live == Some(goal) {
        update.reached = reached.or_else(|| {
            Some(Stamp {
                at: look.now,
                source: "first found at this schema by this refresh: it was there then or earlier"
                    .into(),
            })
        });
    }
    let update = &mut ledger.updates[index];
    if !update.open() {
        return lines;
    }
    let available = serving(look, goal);
    if update.available.is_none()
        && let Some((name, stamp)) = &available
    {
        update.release = Some(name.clone());
        update.available = Some(stamp.clone());
    }
    if let Some(how) = complete {
        update.served = Some(served(look, update.reached.as_ref().map(|r| r.at)));
        update.served_by = look.front.as_ref().and_then(|front| front.release.clone());
        lines.push(said_served(update, id, how));
        lines.extend(close(look, boards, update, "it is served"));
        return lines;
    }
    // A move that failed, or a board its project holds, is a failure whatever serves; else, with
    // no release serving the schema or no server, it waits.
    let failed = matches!(seen.end, Some(End::Red { .. } | End::Theirs(_)));
    // A server whose list cannot say yet whether it writes the board, once the refresh waited for
    // it ([`unsure`]), is neither its completion nor its failure: the next refresh judges it.
    let unsure = seen.live == Some(goal) && listed(look, seen) == Some(None);
    if !failed && (available.is_none() || look.front.is_none() || unsure) {
        lines.push(format!(
            "updates: {} waits: {}",
            update.id,
            if available.is_none() {
                format!("no release serving schema {goal} is installed yet")
            } else if look.front.is_none() {
                "no server answers on this machine to write it".to_string()
            } else {
                "the machine's server cannot say yet whether it writes its board".to_string()
            }
        ));
        return lines;
    }
    let why = failure(look, seen, goal);
    update.failures.push(Failure {
        at: look.now,
        run: look.against.run.to_string(),
        why: why.clone(),
    });
    lines.push(match told(look, boards, update, seen, &why) {
        Ok(said) => format!("updates: {} failed: {why}; {said}", update.id),
        Err(refused) => format!(
            "updates: {} failed: {why}; its task could not be written: {refused}",
            update.id
        ),
    });
    lines
}

/// Each other update of `seen`'s board its schema `live` ends, said: an open one served at `live`
/// where the server writes it there, else superseded by the update to `goal`; one the board moved
/// past; and one it went back below, served or not, so its next arrival there is a new generation.
fn ended(
    look: &Look<'_>,
    boards: &dyn Boards,
    ledger: &mut Ledger,
    seen: &Seen<'_>,
    live: i64,
    goal: i64,
) -> Vec<String> {
    let mut lines = Vec::new();
    for update in &mut ledger.updates {
        let ends = update.project == seen.entry.id
            && update.left.is_none()
            && update.schema != goal
            && (update.open() || live < update.schema);
        if !ends {
            continue;
        }
        if update.schema == live
            && let Some(how) = written(look, seen, live)
        {
            update.served = Some(served(look, update.reached.as_ref().map(|r| r.at)));
            update.served_by = look.front.as_ref().and_then(|front| front.release.clone());
            lines.push(said_served(update, &seen.entry.id, how));
            lines.extend(close(look, boards, update, "it is served"));
            continue;
        }
        let why = if live < update.schema {
            format!("its board went back: it is at schema {live}")
        } else if live > update.schema {
            format!("its board moved on to schema {live}")
        } else {
            format!(
                "superseded by {}: its board is to be written at schema {goal}",
                updates::name(&update.project, goal, 1)
            )
        };
        update.left = Some(Stamp {
            at: look.now,
            source: format!("this refresh found it ended: {why}"),
        });
        let state = if update.served.is_some() {
            "after it was served"
        } else {
            "unserved"
        };
        lines.push(format!("updates: {} ended {state}: {why}", update.id));
        lines.extend(close(look, boards, update, &why));
    }
    lines
}

/// The line that says `update` of `project`'s board is served, and how it is known.
fn said_served(update: &Update, project: &str, how: &str) -> String {
    format!(
        "updates: {} served: the machine's server writes {project}'s board at schema {} ({how}). {}",
        update.id,
        update.schema,
        latency(update)
    )
    .trim_end()
    .to_string()
}

/// Whether the record `reached` dates an arrival after the one `served` recorded: the board left
/// and came back, and its record started again from an accepted rollback.
fn later(reached: Option<&Stamp>, served: &Update) -> bool {
    match (reached, served.reached.as_ref()) {
        (Some(now), Some(then)) => {
            now.source.starts_with("schema-seen.log")
                && then.source.starts_with("schema-seen.log")
                && now.at > then.at + 1.0
        }
        _ => false,
    }
}

/// Why `seen`'s update to `goal` is not complete after this refresh, in its words.
fn failure(look: &Look<'_>, seen: &Seen<'_>, goal: i64) -> String {
    if let Some(End::Theirs(why) | End::Red { why, .. }) = seen.end {
        return why.clone();
    }
    // A board whose schema cannot be read failed by its own red, said above.
    if let Some(live) = seen.live.filter(|live| *live < goal) {
        return format!("its board is at schema {live}, not moved to {goal}");
    }
    let front = look.front.as_ref();
    let server = front.map_or_else(
        || "no server".to_string(),
        |front| match &front.release {
            Some(name) => format!(
                "the machine's server, the release {name} at schema {}",
                front.schema
            ),
            None => format!(
                "the machine's server, the build of {} at schema {}",
                front.root, front.schema
            ),
        },
    );
    format!(
        "{server}, does not write its board at schema {goal}{}",
        look.server
            .map_or_else(String::new, |line| format!(": {line}"))
    )
}

/// Which board takes `seen`'s update's task: the project's own when its project acts (its writers
/// or its own migrate hold its board) and a release installed here writes that board's schema;
/// else this repository's, for the refresh's own failure, this repository's update, a board whose
/// database is down (filing there would start it) and a board nothing here can write. The binary
/// that writes the project's board comes with it.
fn owner(look: &Look<'_>, seen: &Seen<'_>) -> Option<PathBuf> {
    if seen.ours || !matches!(seen.end, Some(End::Theirs(_))) {
        return None;
    }
    writer(look.against.machine, seen.live?)
}

/// The board `task` is on: the Symphony's own, or its project's through the release that writes
/// that board's schema now; none while its project is not registered or nothing here writes it.
fn board_of(look: &Look<'_>, boards: &dyn Boards, task: &Task) -> Option<Box<dyn Reds>> {
    if task.ours {
        return Some(boards.ours(look.against.main));
    }
    let entry = look
        .registry
        .projects
        .iter()
        .find(|e| e.id == task.project)?;
    let common = symphony_board::git::checkout_common_dir(&entry.root)?;
    let home = Home::in_common(&common, Some(&surreal_for(&common, look.against.surreal)));
    let live = home.schema(symphony_board::DATABASE).ok().flatten()?;
    let tasks = writer(look.against.machine, live)?;
    Some(boards.theirs(&entry.root, &tasks))
}

/// The id this repository is registered under, if it is.
fn ours_id(look: &Look<'_>) -> Option<String> {
    (look.registry.projects.iter())
        .find(|entry| {
            symphony_board::git::checkout_common_dir(&entry.root)
                .is_some_and(|common| common == look.against.ours)
        })
        .map(|entry| entry.id.clone())
}

/// `update`'s failure `why` told: its first files its task on the board that can act, each later
/// one is a post on that task. What was done, said.
fn told(
    look: &Look<'_>,
    boards: &dyn Boards,
    update: &mut Update,
    seen: &Seen<'_>,
    why: &str,
) -> Result<String, String> {
    let (project, ours, board): (String, bool, Box<dyn Reds>) = match &update.task {
        Some(task) => {
            let board = board_of(look, boards, task).ok_or_else(|| {
                format!(
                    "{}'s board, which holds {}, cannot be reached now",
                    task.project, task.slug
                )
            })?;
            (task.project.clone(), task.ours, board)
        }
        None => match owner(look, seen) {
            Some(tasks) => (
                seen.entry.id.clone(),
                false,
                boards.theirs(&seen.entry.root, &tasks),
            ),
            None => (
                ours_id(look).unwrap_or_else(|| "this repository".into()),
                true,
                boards.ours(look.against.main),
            ),
        },
    };
    let key = updates::slug(&update.project, update.schema, update.generation)
        .trim_start_matches(board::SERIES)
        .to_string();
    let (slug, open) = board::generation(board.as_ref(), &key)?;
    let filer = board.register()?;
    // Each failure, the first too, is a post of its own on the update's one task.
    let done = if open {
        board
            .say(&slug, &filer, &posted(look, update, why))
            .map(|()| format!("heard on {slug}, {project}'s board"))
    } else {
        board
            .create(
                &slug,
                &title(update),
                &body(look, update, seen, why),
                &filer,
            )
            .and_then(|()| board.say(&slug, &filer, &posted(look, update, why)))
            .map(|()| format!("filed {slug} on {project}'s board"))
    };
    let _ = board.leave(&filer);
    let said = done?;
    update.task = Some(Task {
        project,
        ours,
        slug,
        closed: false,
    });
    Ok(said)
}

/// Close `update`'s open task, saying `why`: what was done, said; nothing without one.
fn close(look: &Look<'_>, boards: &dyn Boards, update: &mut Update, why: &str) -> Option<String> {
    let task = update.task.as_ref().filter(|task| !task.closed)?.clone();
    let Some(board) = board_of(look, boards, &task) else {
        return Some(format!(
            "updates: {} ({why}): {}'s board cannot be reached to close {}; the next refresh does",
            update.id, task.project, task.slug
        ));
    };
    let words = format!(
        "{}: {why} (run {}). {}Closed by the release run's refresh.",
        update.id,
        look.against.run,
        latency(update)
    );
    let closed = updates::close_task(&board::Told(board.as_ref()), &task.slug, &words);
    Some(match closed {
        Ok(()) => {
            if let Some(task) = &mut update.task {
                task.closed = true;
            }
            format!(
                "updates: closed {} on {}'s board: {}",
                task.slug, task.project, update.id
            )
        }
        Err(refused) => format!(
            "updates: {} could not be closed on {}'s board ({refused}); the next refresh tries again",
            task.slug, task.project
        ),
    })
}

/// What `update`'s times say: how long it took, and how much of it after a release serving it
/// ([`updates::took`]).
fn latency(update: &Update) -> String {
    updates::took(update)
}

/// An update's task's title.
fn title(update: &Update) -> String {
    format!(
        "Schema update failed: {}'s board at schema {} is not written by the machine's server",
        update.project, update.schema
    )
}

/// An update's task's body: the update, what failed, what finishes it and who acts, what closes it.
fn body(look: &Look<'_>, update: &Update, seen: &Seen<'_>, why: &str) -> String {
    let root = seen.entry.root.display();
    let action = match seen.end {
        Some(End::Theirs(_)) => format!(
            "Who acts: this project, in its main checkout ({root}): what the line above names (each \
             writer named rebuilt at schema {schema} and run once, which records it; or, for a \
             board from before the writer record, its own `just tasks migrate`). `just \
             symphony-cli release-notes` says what taking the release asks of it. Then the \
             Symphony's next refresh moves the board (the next release, or `just release \
             --refresh` in the Symphony's main checkout, an orchestrator's act there without \
             asking).",
            schema = update.schema
        ),
        Some(End::Red { down: true, .. }) => format!(
            "Who acts: its board's database is down, and nothing here starts it: any `just tasks` \
             in {root} does, the project's act. Then the Symphony's next refresh moves the board \
             (`just release --refresh` in the Symphony's main checkout)."
        ),
        _ => "Who acts: an orchestrator staffs it as a red (RULES.md `workflow/fast-bugfixes`): \
              the refresh's own work failed, or the machine's server did not come to write the \
              board. Fix what the line above names, then `just release --refresh` in the \
              Symphony's main checkout, an orchestrator's act without asking, finishes it. \
              Nothing moves a board back, points current back or restarts the shared server here: \
              those stay the owner's word (AGENTS.md)."
            .to_string(),
    };
    format!(
        "The machine's schema update {id} (docs/workflow/RELEASES.md, \"An update\") failed in the release \
         run's refresh of {release} (run {run}): {why}\n\n\
         Its board: {project} at {root}, now {live}; the update is complete once the \
         machine's server writes it at schema {schema}. {available}\n\n\
         {action}\n\n\
         One task for this update: each later failure of it is a post here, never another task, \
         and the refresh that completes it closes this task. It is red work: it takes no \
         orchestrator slot, and an orchestrator at its task limit is given it.\n\n\
         update: {id}\nkey: {key}",
        id = update.id,
        release = look.against.tag,
        run = look.against.run,
        project = update.project,
        live = seen.live.map_or_else(
            || "at a schema that cannot be read".to_string(),
            |live| format!("at schema {live}")
        ),
        schema = update.schema,
        available = match (&update.release, &update.available) {
            (Some(name), Some(at)) => format!(
                "A green release serving it, {name}, is installed here (since {}).",
                local(at.at)
            ),
            _ => String::new(),
        },
        key = updates::slug(&update.project, update.schema, update.generation)
            .trim_start_matches(board::SERIES),
    )
}

/// A failure's post on the update's task: its count, its run and what failed.
fn posted(look: &Look<'_>, update: &Update, why: &str) -> String {
    format!(
        "Failure {} of {} (run {}, the refresh of {}): {why}",
        update.failures.len(),
        update.id,
        look.against.run,
        look.against.tag
    )
}

/// `at` as this machine's local minute.
fn local(at: f64) -> String {
    super::record::local(at)
}

/// How many served updates `--status` names, the newest first.
const SERVED_SAID: usize = 5;

/// What `--status` says of the machine's updates in the machine folder `machine` at `now`: how
/// many are open and served, each open one with its failures and task, and the newest served with
/// how long each took, each time with where it comes from.
pub(crate) fn status(machine: &Path, now: f64) -> String {
    let ledger = match Ledger::read(machine) {
        Ok(ledger) => ledger,
        Err(why) => return format!("schema updates: the ledger cannot be read: {why}\n"),
    };
    if ledger.updates.is_empty() {
        return "schema updates: none recorded\n".into();
    }
    let open: Vec<&Update> = ledger.updates.iter().filter(|u| u.open()).collect();
    let served: Vec<&Update> = (ledger.updates.iter().rev())
        .filter(|update| update.served.is_some())
        .collect();
    let mut text = format!(
        "schema updates: {} open, {} served (ledger {})\n",
        open.len(),
        served.len(),
        updates::path(machine).display()
    );
    for update in open {
        let failing = match (update.failures.first(), update.failures.len()) {
            (None, _) => "no failure yet".to_string(),
            (Some(first), 1) => format!("failed once, {}", super::record::age(now, first.at)),
            (Some(first), n) => format!(
                "failed {n} times since {} ({})",
                local(first.at),
                super::record::age(now, first.at)
            ),
        };
        let task = update.task.as_ref().map_or_else(String::new, |task| {
            format!("; task {} on {}'s board", task.slug, task.project)
        });
        let reached = update.reached.as_ref().map_or_else(
            || "; not at its schema yet".to_string(),
            |at| format!("; reached {} ({})", local(at.at), short(&at.source)),
        );
        text.push_str(&format!("  open {}: {failing}{task}{reached}\n", update.id));
    }
    for update in served.into_iter().take(SERVED_SAID) {
        let Some(at) = &update.served else { continue };
        let took = match (update.total(), update.after_available()) {
            (Some(total), Some(after)) => format!(
                "{} from its board reaching it, {} of it after a release serving it",
                updates::said(total),
                updates::said(after)
            ),
            (Some(total), None) => format!("{} from its board reaching it", updates::said(total)),
            (None, Some(after)) => format!("{} after a release serving it", updates::said(after)),
            (None, None) => "how long is not known".to_string(),
        };
        let mut sources = vec![format!("served by {}", short(&at.source))];
        if let Some(reached) = &update.reached {
            sources.insert(0, format!("reached by {}", short(&reached.source)));
        }
        if let (Some(name), Some(available)) = (&update.release, &update.available) {
            sources.push(format!("{name} by {}", short(&available.source)));
        }
        text.push_str(&format!(
            "  served {} {}: {took}; times from {}\n",
            update.id,
            local(at.at),
            sources.join(", ")
        ));
    }
    text
}

/// A time's source as `--status` names it: its words before their first colon.
fn short(source: &str) -> &str {
    source.split(':').next().unwrap_or(source)
}

#[cfg(test)]
#[path = "updates_tests.rs"]
mod tests;
