//! A held update tried again by the machine's server (docs/RELEASES.md, "A held update tried
//! again"): a registered project's board that a release's refresh left below the schema the release
//! `current` names serves, its update open in the machine's ledger ([`super::updates`]), is judged
//! again at each round of the server's idle sweep for that project, by the refresh's own writer
//! rule ([`super::writers`]), and moved once nothing holds it, without a refresh run.
//!
//! - **Who.** The machine's server, only while it is the installed release `current` names (its
//!   own executable is that release's), in each project's own maintenance worker, never a request.
//!   Its round looks at two files first, the release's manifest and the ledger, and does nothing
//!   more unless that project's update to the release's schema is open, or served with its task
//!   still open. The Symphony's own board, in the checkout that keeps the release run's record,
//!   moves with its own main and never here.
//! - **The lock.** It takes the machine's updates lock without waiting ([`updates::try_lock`]), and
//!   does nothing this round while anything holds it: a release run of every kind holds it from
//!   just after its own run lock to its end. A run of a build from before that lock is seen by what
//!   it says of itself in the release run's record (`running.json`, a living process), and nothing
//!   moves while it runs either. It never takes the run's own lock. Under the lock, `current` is
//!   read again: it still names this server's release, or nothing moves.
//! - **The move.** As the refresh moves a board: a board below the writer record, or one a writer
//!   behind holds, is its project's to finish; one whose database is down, whose writers or schema
//!   cannot be read, or whose database runs an older SurrealDB than the release pins, is red.
//!   Otherwise its store is snapshotted just before its move ([`super::moving`]), then the
//!   release's own `tasks migrate` (`releases/RELEASE/bin/tasks`, named by the machine folder and
//!   the release, never by a board's record) runs in its main checkout, `--leave-behind` where the
//!   only writers behind are a release's own, gone or unused ([`super::writers`]), within its
//!   bound, and its schema is read again.
//! - **The ledger.** The same update, by its identity: its failure is recorded and posted on its
//!   task only when what holds it changed since the last one said, the refresh's included
//!   ([`super::writers::steady`]); the move stamps when its board reached the schema; and once a
//!   read of the board made now finds it at the schema this server writes, the update is served,
//!   its task closed saying how long it took. Each step leaves the ledger as the next round can
//!   take it up: a snapshot or a migrate that failed or ran out of time is a failure, and the next
//!   round judges the board again; a move whose ledger was not written is found at the schema and
//!   finished; a close that failed is tried again.

use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Duration;

use symphony_board::server::{Engine, Home, ServerError};
use symphony_board::sys::Command;

use super::moving::{self, KEPT_SNAPSHOTS, UNSET};
use super::updates::{self, Failure, Ledger, Stamp, Task, TaskBoard, Update};
use super::writers;
use super::{Registry, release};

/// The file in the release run's record folder (`symphony_board::folders::RELEASE`) that names the
/// run holding its lock, while one does: its process among what it says.
const RUNNING: &str = "running.json";

/// What the machine's server lends a retry: its clocks, its way of running a command within a
/// bound, its word that it writes a board, and the boards an update's task may be on.
pub trait Host {
    /// Now, in seconds since the epoch.
    fn now(&self) -> f64;
    /// `command` run to its end, its group stopped past `bound` or once the server stops: its
    /// output, or why it has none.
    ///
    /// # Errors
    ///
    /// When it could not run, ran past `bound` or was stopped.
    fn run(&self, command: Command, bound: Duration) -> Result<Output, String>;
    /// Whether the machine's server writes the project's board at `schema`, by a read of it made
    /// now: the project loaded, its board opened afresh and found at the schema this server's own
    /// build writes.
    ///
    /// # Errors
    ///
    /// Why it does not, in words.
    fn writes(&self, schema: i64) -> Result<(), String>;
    /// The Symphony's own board, through its main checkout `main`.
    fn ours(&self, main: &Path) -> Box<dyn TaskBoard>;
    /// A project's board, at its checkout `root`, through `tasks`, a release's own board binary.
    fn theirs(&self, root: &Path, tasks: &Path) -> Box<dyn TaskBoard>;
}

/// One project's round: the machine folder, this server's release by its own executable, the
/// project's registered id, and each command's bound.
pub struct Retry<'a> {
    pub machine: &'a Path,
    pub release: &'a str,
    pub project: &'a str,
    pub bound: Duration,
}

/// The project's held update tried again: what was done, in lines for the server's log, none when
/// there was nothing to do.
pub fn retry(retry: &Retry<'_>, host: &dyn Host) -> Vec<String> {
    let (machine, release, id) = (retry.machine, retry.release, retry.project);
    // Two files read with no lock: most rounds end here.
    let Ok(Some(manifest)) = release::installed(machine, release) else {
        return Vec::new();
    };
    let target = manifest.serves.schema;
    let Ok(ledger) = Ledger::read(machine) else {
        return Vec::new();
    };
    let Some(update) = due(&ledger, id, target).map(|index| ledger.updates[index].id.clone())
    else {
        return Vec::new();
    };
    let Ok(registry) = Registry::read(machine) else {
        return Vec::new();
    };
    let Some(entry) = registry.get(id) else {
        return Vec::new();
    };
    let Some(common) = symphony_board::git::checkout_common_dir(&entry.root) else {
        return Vec::new();
    };
    // The Symphony's own board moves with its own main.
    if common.join(symphony_board::folders::RELEASE).is_dir() {
        return Vec::new();
    }
    let _lock = match updates::try_lock(machine) {
        Ok(Some(lock)) => lock,
        Ok(None) => {
            return vec![format!(
                "{update}: the machine's updates lock is held (a release run, a refresh or another \
                 project's retry): nothing moves until a round finds it free"
            )];
        }
        Err(why) => {
            return vec![format!(
                "{update}: the machine's updates lock cannot be taken, so nothing moves: {why}"
            )];
        }
    };
    if let Some(run) = running(&registry) {
        return vec![format!(
            "{update}: a release run holds its lock ({run}): nothing moves until a round finds none"
        )];
    }
    match release::current(machine) {
        Some(named) if named == release => {}
        named => {
            return vec![format!(
                "{update}: current names {}, not this server's release {release}: nothing moves",
                named.as_deref().unwrap_or("no release")
            )];
        }
    }
    let mut ledger = match Ledger::read(machine) {
        Ok(ledger) => ledger,
        Err(why) => return vec![format!("{update}: the ledger cannot be read: {why}")],
    };
    let Some(index) = due(&ledger, id, target) else {
        return Vec::new();
    };
    let before = ledger.clone();
    let board = Board {
        machine,
        release,
        root: &entry.root,
        common: &common,
        target,
        bound: retry.bound,
        registry: &registry,
    };
    let mut lines = board.one(&mut ledger.updates[index], host);
    if ledger != before
        && let Err(why) = ledger.write(machine)
    {
        lines.push(format!("{update}: the ledger cannot be written: {why}"));
    }
    lines
}

/// The index of `project`'s update to `target` a round has to do with: the newest of its
/// generations, while it is open, or served with its task still open; none otherwise.
fn due(ledger: &Ledger, project: &str, target: i64) -> Option<usize> {
    let (index, update) = (ledger.updates.iter().enumerate())
        .filter(|(_, update)| update.project == project && update.schema == target)
        .max_by_key(|(_, update)| update.generation)?;
    let closing = update.served.is_some()
        && update.left.is_none()
        && update.task.as_ref().is_some_and(|task| !task.closed);
    (update.open() || closing).then_some(index)
}

/// A release run holding its lock, as it says of itself: in the record folder of each registered
/// repository that keeps one, a `running.json` naming a living process. A run of a build from
/// before the machine's updates lock holds none, and says this all the same.
fn running(registry: &Registry) -> Option<String> {
    (registry.projects.iter()).find_map(|entry| {
        let common = symphony_board::git::checkout_common_dir(&entry.root)?;
        let path = common.join(symphony_board::folders::RELEASE).join(RUNNING);
        let said: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
        let pid = said["pid"].as_i64()?;
        symphony_board::sys::pid_alive(pid).then(|| {
            format!(
                "run {}, process {pid}, by {}",
                said["id"].as_str().unwrap_or("unnamed"),
                path.display()
            )
        })
    })
}

/// The board a round moves, and where everything it needs is.
struct Board<'a> {
    machine: &'a Path,
    release: &'a str,
    /// Its project's main checkout, where its migrate runs.
    root: &'a Path,
    /// Its repository's git common dir.
    common: &'a Path,
    /// The schema the release serves.
    target: i64,
    bound: Duration,
    registry: &'a Registry,
}

/// What a round's judgement of a board below the release's schema came to.
enum Judged {
    /// It cannot move now, said: its project's to finish, or red.
    Held(String),
    /// It moves from schema `live`, `--leave-behind` when `leave`, the writers not counted said
    /// in `gone`.
    Moves {
        live: i64,
        leave: bool,
        gone: String,
    },
}

impl Board<'_> {
    fn home(&self) -> Home {
        Home::in_common(self.common, Some(&moving::surreal_for(self.common, None)))
    }

    /// `update` of this board taken as far as it goes this round: what was done, said.
    fn one(&self, update: &mut Update, host: &dyn Host) -> Vec<String> {
        let mut lines = Vec::new();
        if update.served.is_none() {
            // A project that keeps no board has none to move.
            if !self.common.join(symphony_board::server::FOLDER).is_dir() {
                return lines;
            }
            let home = self.home();
            match home.schema(symphony_board::DATABASE) {
                // A board not made yet, or moved past the release, is no move of this one's.
                Ok(None) => return lines,
                Ok(Some(live)) if live > self.target => return lines,
                Ok(Some(live)) if live < self.target => match self.judge(&home, live, host) {
                    Judged::Held(why) => return self.failed(update, why, host),
                    Judged::Moves { live, leave, gone } => {
                        match self.moved(&home, live, leave, host) {
                            Ok(said) => {
                                update.reached = updates::seen_at(self.common, self.target)
                                    .or_else(|| {
                                        Some(Stamp {
                                            at: host.now(),
                                            source: "the machine server's move, by its clock \
                                                     when the move was done"
                                                .into(),
                                        })
                                    });
                                lines.push(format!("{}: {said}{gone}", update.id));
                            }
                            Err(why) => return self.failed(update, why, host),
                        }
                    }
                },
                Ok(Some(_)) => {}
                Err(ServerError::NotUp(why)) => {
                    return self.failed(
                        update,
                        format!(
                            "{why}: its schema and writers were not read, and nothing was moved; \
                             once it runs (any `just tasks` in {} starts it), the machine's server \
                             moves it at its next round",
                            self.root.display()
                        ),
                        host,
                    );
                }
                Err(error) => return self.failed(update, error.to_string(), host),
            }
            // At the release's schema now: its arrival there, its release, and its server.
            if update.reached.is_none() {
                update.reached = updates::seen_at(self.common, self.target).or_else(|| {
                    Some(Stamp {
                        at: host.now(),
                        source: "first found at this schema by the machine's server: it was \
                                 there then or earlier"
                            .into(),
                    })
                });
            }
            if update.available.is_none() {
                update.release = Some(self.release.to_string());
                update.available = Some(Stamp {
                    at: updates::installed_at(self.machine, self.release)
                        .unwrap_or_else(|| host.now()),
                    source: format!("{}'s manifest, written by its install", self.release),
                });
            }
            if let Err(why) = host.writes(self.target) {
                lines.push(format!(
                    "{}: its board is at schema {}, and the machine's server does not write it \
                     yet ({why}): its next round looks again",
                    update.id, self.target
                ));
                return lines;
            }
            update.served = Some(Stamp {
                at: host.now(),
                source: "the machine server's own read of the board after its move: at the schema \
                         it writes"
                    .into(),
            });
            update.served_by = Some(self.release.to_string());
            lines.push(
                format!(
                    "{}: served: the machine's server writes the board at schema {}. {}",
                    update.id,
                    self.target,
                    updates::took(update)
                )
                .trim_end()
                .to_string(),
            );
        }
        lines.extend(self.close(update, host));
        lines
    }

    /// The board at schema `live`, below the release's, judged by the refresh's rule: what holds
    /// it, or how it moves.
    fn judge(&self, home: &Home, live: i64, host: &dyn Host) -> Judged {
        if live < symphony_board::WRITERS_SINCE {
            return Judged::Held(writers::before_the_record(live, self.root));
        }
        let lines = match home.writers(symphony_board::DATABASE) {
            Ok(lines) => lines,
            Err(error) => return Judged::Held(format!("{error}: nothing was moved")),
        };
        let now = host.now();
        let runs = writers::Runs::of(self.root);
        let lagging = writers::lagging(lines, self.target, now, self.machine, &runs);
        if !lagging.behind.is_empty() {
            return Judged::Held(lagging.holding(live, self.release, self.target, now));
        }
        let moves = Judged::Moves {
            live,
            leave: lagging.leaves(),
            gone: lagging.not_counted(now),
        };
        // A release pins no PostgreSQL: the board's is the machine's own, which no move replaces.
        match home.backend() {
            Ok(Engine::PostgreSql) => return moves,
            Ok(Engine::SurrealDb) => {}
            Err(error) => return Judged::Held(format!("{error}: nothing was moved")),
        }
        // This server is the release's own build: the SurrealDB it pins is its own.
        let pinned = symphony_board::server::SURREAL_VERSION;
        let running = match home.recorded() {
            Some(running) if !running.version.trim().is_empty() => running.version,
            _ => {
                return Judged::Held(
                    "the SurrealDB its board's database runs cannot be read (its record names no \
                     version), so the move might replace that database and the machine's server \
                     stops none: nothing was moved"
                        .into(),
                );
            }
        };
        if moving::older(&running, pinned) {
            return Judged::Held(format!(
                "its board's database runs SurrealDB {running} and {} pins {pinned}: moving its \
                 schema would replace that server, and the machine's server stops no board's \
                 database, so nothing was moved",
                self.release
            ));
        }
        moves
    }

    /// The board, at schema `live`, snapshotted and moved by the release's own `tasks migrate`,
    /// `--leave-behind` when `leave`: what was done, or why it was not.
    fn moved(
        &self,
        home: &Home,
        live: i64,
        leave: bool,
        host: &dyn Host,
    ) -> Result<String, String> {
        let (release, target) = (self.release, self.target);
        let engine = home
            .backend()
            .map_err(|e| format!("the snapshot of its store failed, so it was not moved: {e}"))?;
        let folder = self.common.join(symphony_board::server::FOLDER);
        let snapshot = moving::snapshot_file(&folder, target, release, engine);
        home.export_within(symphony_board::DATABASE, &snapshot, self.bound)
            .map_err(|e| format!("the snapshot of its store failed, so it was not moved: {e}"))?;
        if let Some(kept) = snapshot.parent() {
            moving::prune(kept, KEPT_SNAPSHOTS);
        }
        let tasks = release::binary(self.machine, release, "tasks");
        let mut command = symphony_board::sys::command(&tasks);
        command.arg("migrate");
        if leave {
            command.arg("--leave-behind");
        }
        command
            .current_dir(self.root)
            .env("GIT_TERMINAL_PROMPT", "0");
        for name in UNSET.iter().chain(&symphony_board::sys::GIT_VARS) {
            command.env_remove(name);
        }
        let words = if leave {
            "tasks migrate --leave-behind"
        } else {
            "tasks migrate"
        };
        let root = self.root.display();
        let kept = snapshot.display();
        let done = host.run(command, self.bound).map_err(|why| {
            format!(
                "{release}'s `{words}` in {root} did not end ({why}); its store as it was before \
                 is in {kept}, and the machine's server judges it again at its next round"
            )
        })?;
        if !done.status.success() {
            let said = format!(
                "{}{}",
                String::from_utf8_lossy(&done.stderr),
                String::from_utf8_lossy(&done.stdout)
            );
            let last: Vec<&str> = (said.lines().map(str::trim))
                .filter(|line| !line.is_empty())
                .collect();
            let last = last[last.len().saturating_sub(3)..].join(" / ");
            return Err(format!(
                "{release}'s `{words}` in {root} failed ({}: {last}); its store as it was before \
                 is in {kept}, and the machine's server judges it again at its next round",
                done.status
            ));
        }
        match home.schema(symphony_board::DATABASE) {
            Ok(Some(moved)) if moved >= target => Ok(format!(
                "its board moved from schema {live} to {moved} by {release}'s own `{words}`, its \
                 store snapshotted first to {kept}"
            )),
            read => Err(format!(
                "{release}'s `{words}` in {root} ended, but its board reads {}, short of schema \
                 {target}; its store as it was before is in {kept}",
                match read {
                    Ok(Some(moved)) => format!("schema {moved}"),
                    Ok(None) => "no schema".into(),
                    Err(e) => e.to_string(),
                }
            )),
        }
    }

    /// `update` not moved, for `why`: a failure recorded and posted on its task only when what
    /// holds it changed since the last failure said, the refresh's included. What was done, said.
    fn failed(&self, update: &mut Update, why: String, host: &dyn Host) -> Vec<String> {
        let steady = writers::steady(&why);
        if (update.failures.last()).is_some_and(|last| writers::steady(&last.why) == steady) {
            return vec![format!(
                "{}: still held, as its last failure says",
                update.id
            )];
        }
        update.failures.push(Failure {
            at: host.now(),
            run: format!("the machine's server, {}", self.release),
            why: why.clone(),
        });
        let mut line = format!("{}: not moved: {why}", update.id);
        if let Some(task) = update.task.clone().filter(|task| !task.closed) {
            let text = format!(
                "Failure {} of {} (the machine's server, {}, at its round): {why}",
                update.failures.len(),
                update.id,
                self.release
            );
            match self.board_of(&task, host) {
                Some(board) => match updates::post(board.as_ref(), &task.slug, &text) {
                    Ok(()) => line.push_str(&format!("; heard on {}", task.slug)),
                    Err(refused) => {
                        line.push_str(&format!("; {} could not be told: {refused}", task.slug))
                    }
                },
                None => line.push_str(&format!(
                    "; {}'s board, which holds {}, cannot be reached now",
                    task.project, task.slug
                )),
            }
        }
        vec![line]
    }

    /// Close `update`'s open task, now it is served, saying how long it took; what was done,
    /// said, none without one.
    fn close(&self, update: &mut Update, host: &dyn Host) -> Option<String> {
        let task = update.task.clone().filter(|task| !task.closed)?;
        let Some(board) = self.board_of(&task, host) else {
            return Some(format!(
                "{}: {}'s board cannot be reached to close {}; the next round tries again",
                update.id, task.project, task.slug
            ));
        };
        let words = format!(
            "{}: it is served (the machine's server, {}, moved its board once nothing held it). \
             {}Closed by the machine's server, without a refresh.",
            update.id,
            self.release,
            updates::took(update)
        );
        Some(
            match updates::close_task(board.as_ref(), &task.slug, &words) {
                Ok(()) => {
                    if let Some(task) = &mut update.task {
                        task.closed = true;
                    }
                    format!(
                        "{}: closed {} on {}'s board",
                        update.id, task.slug, task.project
                    )
                }
                Err(refused) => format!(
                    "{}: {} could not be closed on {}'s board ({refused}); the next round tries again",
                    update.id, task.slug, task.project
                ),
            },
        )
    }

    /// The board `task` is on: the Symphony's own, through its main checkout, or its project's
    /// through the release that writes that board's schema now; none while neither can be found.
    fn board_of(&self, task: &Task, host: &dyn Host) -> Option<Box<dyn TaskBoard>> {
        let keeps_record = |root: &Path| {
            symphony_board::git::checkout_common_dir(root)
                .is_some_and(|common| common.join(symphony_board::folders::RELEASE).is_dir())
        };
        if task.ours {
            let entry = (self.registry.projects.iter())
                .find(|entry| entry.id == task.project && keeps_record(&entry.root))
                .or_else(|| {
                    (self.registry.projects.iter()).find(|entry| keeps_record(&entry.root))
                })?;
            return Some(host.ours(&entry.root));
        }
        let entry = self.registry.get(&task.project)?;
        let common = symphony_board::git::checkout_common_dir(&entry.root)?;
        let home = Home::in_common(&common, Some(&moving::surreal_for(&common, None)));
        let live = home.schema(symphony_board::DATABASE).ok().flatten()?;
        let tasks: PathBuf = updates::writer_of(self.machine, live)?;
        Some(host.theirs(&entry.root, &tasks))
    }
}

/// Whether the machine's server writes the board of the checkout at `root` at `schema`, by a read
/// of it made now: its board opened afresh, never starting its database, as this build reads it,
/// and found at `schema`, which must be this build's own. A board this build only reads (one ahead
/// of it) or refuses is not written.
///
/// # Errors
///
/// Why it is not, in words.
pub fn writes(root: &Path, schema: i64) -> Result<(), String> {
    if schema != symphony_board::SCHEMA {
        return Err(format!(
            "this server's build writes schema {}, not {schema}",
            symphony_board::SCHEMA
        ));
    }
    let store = crate::taskview::connect_running(root)?;
    match store.board_version() {
        Ok(Some(found)) if found == schema => Ok(()),
        Ok(found) => Err(format!(
            "a read of it now finds {}",
            found.map_or_else(
                || "no schema".to_string(),
                |found| format!("schema {found}")
            )
        )),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
#[path = "held_tests.rs"]
mod tests;
