//! A board's store moved from one engine to the other ([`Home::move_to`]), and the switch of a
//! home's data that it and a restore over the board ([`super::Postgres::restore_over`]) share.
//!
//! A move holds the board, stops its server, keeps a snapshot of its store, and reads it through
//! a server of its own: the board's schema, which must be this build's (a move migrates
//! nothing), the dump ([`Store::dump`]), the tables no copy holds ([`Store::carried`]), the
//! marker, and the databases the store holds beside the board's own (a scratch board's, `just
//! tasks --database NAME`), which are not moved: they stay in the old store, kept whole with
//! it, and the move names them. It writes the board as the export
//! it keeps, makes a store of the other engine in a work folder of its own, copies and carries
//! them there, moves that board's marker past the one it read, reads both back and compares
//! them with what it read, row for row. Until then the home is untouched, and a failure removes
//! the work folder and nothing else.
//!
//! Then the switch, each step durable before the next, under a journal (`move.json`) written
//! first: the old store moved aside into `moved/STAMP-ENGINE/`, the new one moved in, the
//! identity written, the journal marked committed, the home's recovery bindings revoked, the
//! journal removed. Marking the journal is the commit point. Before it, a failure is undone at
//! once and the board is on its old engine as it was, its bindings untouched; after it, the
//! board is on its new engine and what is left is finished. While the journal stands nothing
//! reads the home as either engine ([`Home::backend`] refuses), no start runs and the hold is
//! not released; only the holder's next move or restore settles it, undoing what never
//! committed or finishing what did, each step told by the journal and by what is on disk. A
//! step that can be neither finished nor undone leaves the hold and the journal, and says so.
//!
//! Nothing is deleted but the work folder: the old store stays in `moved/`, with the export
//! and every database it held beside the board's, and the snapshot stays with the snapshots.
//!
//! The new board holds a recovery generation of its own, and the home is bound to none
//! ([`crate::store::recovery`]): a token read before the move is refused, and `tasks recovery
//! rebind` binds the board again.

use super::backend::Engine;
use super::postgres::Postgres;
use super::snapshot::{Before, Kept, stamp};
use super::starters::Doing;
use super::storage::store;
use super::{Home, Running, ServerError, io};
use crate::rows::Dump;
use crate::store::{Database, Opening, Store};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

/// The journal of a switch under way, in the board's home.
pub(in crate::server) const JOURNAL: &str = "move.json";
/// The work folder of a move, the move's own: all that one removes.
const WORK: &str = "moving";
/// Where the stores a switch replaced are kept, each in a folder of its own.
const ASIDE: &str = "moved";
/// The export a move keeps beside the store it replaced.
pub const EXPORT: &str = "export.json";

/// The steps of a switch, in order, each durable before the next: what a test stops one after
/// ([`Home::stopping_after`]). `committed` is the commit point.
pub const STEPS: [&str; 6] = [
    "journal",
    "aside",
    "placed",
    "identity",
    "committed",
    "revoked",
];

/// How a test cuts a switch at one of its [`STEPS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Cut {
    /// The process ends there: nothing more is done, not even an undo.
    Stop(usize),
    /// The step after it fails, and the switch answers as it does to any failure.
    Fail(usize),
    /// As [`Cut::Fail`], and the undo fails too.
    Stuck(usize),
}

/// What a switch is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Move,
    Restore,
}

/// A switch under way, as `move.json` says it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Journal {
    format: u32,
    pub kind: Kind,
    /// The engine whose data the home held, and the one it holds after: one engine for a restore.
    pub from: Engine,
    pub to: Engine,
    /// The folder the replaced data is kept in.
    pub aside: PathBuf,
    /// The new data, where it waits to be moved in.
    staged: PathBuf,
    /// The folder the switch made and removes once it is settled.
    work: PathBuf,
    /// Who holds the board for it, and whether the switch set that hold itself.
    pub by: String,
    took: bool,
    /// Whether the commit point is behind it.
    pub committed: bool,
}

/// A switch that stopped.
pub(in crate::server) enum Left {
    /// The home is as it was before the switch, no journal standing.
    Unchanged(ServerError),
    /// The journal and the hold stand: the holder's next move or restore settles it.
    Standing(ServerError),
}

/// What settling a journal found standing did ([`Home::settle`]).
pub(in crate::server) enum Settled {
    /// There was none.
    Nothing,
    /// One that had committed was finished.
    Finished(Journal),
    /// One that had not was undone.
    Undone(Journal),
}

/// What [`Home::move_to`] did.
#[derive(Debug, Clone, PartialEq)]
pub struct Moved {
    pub from: Engine,
    pub to: Engine,
    /// The folder that keeps the store the move replaced, and its export.
    pub aside: PathBuf,
    /// The databases the old store held beside the board's own, by name, which the move left
    /// in it where it is kept (`aside`). None for a move an earlier call committed and this one
    /// finished, which did not read them.
    pub left: Vec<String>,
    /// The snapshot kept of the store before anything read it, where one was taken now.
    pub snapshot: Option<PathBuf>,
    /// The rows of the dump and of the carried tables, each compared on the new store. Zero for
    /// a move that an earlier call compared and this one finished.
    pub rows: usize,
    /// Whether this call finished a move an earlier one had committed.
    pub finished: bool,
    /// The hold that stands after it: the caller's own, set before the move.
    pub held: bool,
}

/// What a start or a reach says of a home whose journal stands.
pub(super) fn unsettled(journal: &Path) -> String {
    let what = match read(journal) {
        Ok(Some(journal)) if journal.committed => format!(
            "a {} to {} committed and is not finished",
            journal.word(),
            journal.to
        ),
        Ok(Some(journal)) => format!(
            "a {} to {} stopped before it committed",
            journal.word(),
            journal.to
        ),
        _ => "a move or a restore stopped part way".into(),
    };
    format!(
        "{what} ({}): the board is held and nothing reads or starts its store until its holder \
         settles it with `tasks-move`, a move or a restore, which finishes what \
         committed and undoes what did not",
        journal.display()
    )
}

fn read(path: &Path) -> Result<Option<Journal>, ServerError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(why) => return Err(io("could not read", path)(why)),
    };
    match crate::objects::parse::<Journal>(&text) {
        Some(journal) if journal.format == 1 => Ok(Some(journal)),
        _ => Err(ServerError::Io(format!(
            "{} does not read as a switch's journal: nothing was moved, and the board stays held",
            path.display()
        ))),
    }
}

fn present(path: &Path) -> Result<bool, ServerError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(why) => Err(io("could not inspect", path)(why)),
    }
}

fn renamed(from: &Path, to: &Path) -> Result<(), ServerError> {
    fs::rename(from, to).map_err(io("could not move", from))
}

impl Journal {
    pub(in crate::server) fn new(
        kind: Kind,
        (from, to): (Engine, Engine),
        aside: &Path,
        staged: &Path,
        work: &Path,
        (by, took): (&str, bool),
    ) -> Journal {
        Journal {
            format: 1,
            kind,
            from,
            to,
            aside: aside.into(),
            staged: staged.into(),
            work: work.into(),
            by: by.into(),
            took,
            committed: false,
        }
    }

    fn word(&self) -> &'static str {
        match self.kind {
            Kind::Move => "move",
            Kind::Restore => "restore",
        }
    }
}

impl Home {
    /// A test's: the switch of this home ends as a process killed would, once `step` of
    /// [`STEPS`] is durable.
    pub fn stopping_after(self, step: &str) -> Home {
        self.cut(Cut::Stop(at(step)))
    }

    /// A test's: the step after `step` fails.
    pub fn failing_after(self, step: &str) -> Home {
        self.cut(Cut::Fail(at(step)))
    }

    /// A test's: the step after `step` fails, and so does undoing the switch.
    pub fn stuck_after(self, step: &str) -> Home {
        self.cut(Cut::Stuck(at(step)))
    }

    fn cut(self, cut: Cut) -> Home {
        Home {
            cut: Some(cut),
            ..self
        }
    }

    /// The journal standing in this home, if any.
    pub fn switching(&self) -> Result<Option<Journal>, ServerError> {
        read(&self.dir.join(JOURNAL))
    }

    /// A folder of its own under `moved/` for the data of `engine` replaced at `now`: named, not
    /// made.
    pub(super) fn aside(&self, now: f64, engine: Engine) -> PathBuf {
        let base = format!("{}-{}", stamp(now), engine.word());
        let folder = self.dir.join(ASIDE);
        let mut name = base.clone();
        let mut again = 0;
        while folder.join(&name).exists() {
            again += 1;
            name = format!("{base}.{again}");
        }
        folder.join(name)
    }

    fn written(&self, journal: &Journal) -> Result<(), ServerError> {
        let text = serde_json::to_vec(journal).expect("a journal serializes");
        self.publish(JOURNAL, &text)
    }

    /// The journal removed, durably, then the work folder the switch made.
    fn closed(&self, journal: &Journal) -> Result<(), ServerError> {
        let path = self.dir.join(JOURNAL);
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(why) if why.kind() == std::io::ErrorKind::NotFound => {}
            Err(why) => return Err(io("could not remove", &path)(why)),
        }
        self.synced()?;
        if present(&journal.work)? {
            fs::remove_dir_all(&journal.work).map_err(io("could not remove", &journal.work))?;
        }
        Ok(())
    }

    /// Whether a test cut the switch after the step `done`: ended there, or failing next.
    fn cut_at(&self, done: usize) -> Option<Cut> {
        self.cut.filter(|cut| {
            let (Cut::Stop(at) | Cut::Fail(at) | Cut::Stuck(at)) = cut;
            *at == done
        })
    }

    /// The switch `journal` names, from its first step: the old data moved aside, the staged
    /// data moved in, the identity written, the commit, the bindings revoked, the journal
    /// removed. The caller holds the lock and the board's hold, and every server is stopped.
    pub(super) fn switch(&self, journal: Journal) -> Result<(), Left> {
        let (old, new) = (
            self.dir.join(journal.from.data()),
            self.dir.join(journal.to.data()),
        );
        let kept = journal.aside.join(journal.from.data());
        fs::create_dir_all(&journal.aside)
            .map_err(io("could not make", &journal.aside))
            .and_then(|()| self.written(&journal))
            .map_err(Left::Unchanged)?;
        let steps: [&dyn Fn() -> Result<(), ServerError>; 3] = [
            &|| renamed(&old, &kept).and_then(|()| self.synced()),
            &|| renamed(&journal.staged, &new).and_then(|()| self.synced()),
            &|| self.identify(journal.to),
        ];
        let mut done = 0;
        for step in steps {
            let failed = match self.cut_at(done) {
                Some(Cut::Stop(_)) => return Err(Left::Standing(stopped(done))),
                Some(_) => Err(ServerError::Io(format!(
                    "a test failed the step after {:?}",
                    STEPS[done]
                ))),
                None => step(),
            };
            if let Err(why) = failed {
                return Err(self.undone(&journal, why));
            }
            done += 1;
        }
        if let Some(Cut::Stop(_)) = self.cut_at(done) {
            return Err(Left::Standing(stopped(done)));
        }
        let committed = Journal {
            committed: true,
            ..journal.clone()
        };
        match self.cut_at(done) {
            Some(_) => Err(ServerError::Io("a test failed the commit".into())),
            None => self.written(&committed),
        }
        .map_err(|why| self.undone(&journal, why))?;
        self.finish(&committed, done + 1).map_err(Left::Standing)
    }

    /// What is left of a switch past its commit point, from the step `done` of [`STEPS`]: the
    /// bindings revoked and the journal removed. A failure names the board's state as it is.
    fn finish(&self, journal: &Journal, mut done: usize) -> Result<(), ServerError> {
        let pending = |why: ServerError| {
            ServerError::Io(format!(
                "the {} committed: the board is on {} and its {} data is kept in {}; what is \
                 left of it did not finish ({why}), so the board stays held and its journal \
                 stands. Run the command again to finish it",
                journal.word(),
                journal.to,
                journal.from,
                journal.aside.display()
            ))
        };
        let steps: [&dyn Fn() -> Result<(), ServerError>; 2] =
            [&|| self.revoke_recoveries(), &|| self.closed(journal)];
        for step in steps {
            match self.cut_at(done) {
                Some(Cut::Stop(_)) => return Err(stopped(done)),
                Some(_) => {
                    return Err(pending(ServerError::Io(format!(
                        "a test failed the step after {:?}",
                        STEPS[done]
                    ))));
                }
                None => step().map_err(pending)?,
            }
            done += 1;
        }
        Ok(())
    }

    /// A switch that failed before its commit point undone, and what its caller is told: the
    /// home as it was, or, where the undo failed too, the journal and the hold left standing.
    fn undone(&self, journal: &Journal, why: ServerError) -> Left {
        let stuck = matches!(self.cut, Some(Cut::Stuck(_)));
        let back = if stuck {
            Err(ServerError::Io("a test failed the undo".into()))
        } else {
            self.back(journal).and_then(|()| self.closed(journal))
        };
        match back {
            Ok(()) => Left::Unchanged(why),
            Err(again) => Left::Standing(ServerError::Io(format!(
                "the {} to {} failed before it committed ({why}) and could not be undone \
                 ({again}): the board stays held and its journal stands at {}; nothing was \
                 deleted, and the {} data is at {} or in {}. Run the command again to undo it",
                journal.word(),
                journal.to,
                self.dir.join(JOURNAL).display(),
                journal.from,
                self.dir.join(journal.from.data()).display(),
                journal.aside.display()
            ))),
        }
    }

    /// The steps of a switch that never committed, taken back, each told by what is on disk:
    /// the old identity, the new data back where it was staged, the old data back in place.
    fn back(&self, journal: &Journal) -> Result<(), ServerError> {
        let (old, new) = (
            self.dir.join(journal.from.data()),
            self.dir.join(journal.to.data()),
        );
        let kept = journal.aside.join(journal.from.data());
        if self.identity()? != Some(journal.from) {
            self.identify(journal.from)?;
        }
        // The new data is in place exactly when it is no longer where it was staged.
        if !present(&journal.staged)? && present(&new)? {
            if let Some(folder) = journal.staged.parent() {
                fs::create_dir_all(folder).map_err(io("could not make", folder))?;
            }
            renamed(&new, &journal.staged)?;
        }
        if present(&kept)? && !present(&old)? {
            renamed(&kept, &old)?;
        }
        self.synced()?;
        if !present(&old)? {
            return Err(ServerError::Io(format!(
                "the {} data is neither at {} nor at {}",
                journal.from,
                old.display(),
                kept.display()
            )));
        }
        Ok(())
    }

    /// The hold a switch runs under, as `by`: the one it finds, its own, or one it sets now,
    /// and whether it set it. Another's refuses. Under the lock.
    pub(super) fn held_for(&self, by: &str, why: &str) -> Result<bool, ServerError> {
        let took = self.held().is_none();
        self.hold_as(by, why)?;
        Ok(took)
    }

    /// A journal found standing settled, as `by`, who must hold the board: finished where it
    /// had committed, undone where it had not; then the hold released where the switch had set
    /// it. Under the lock.
    pub(super) fn settle(&self, by: &str) -> Result<Settled, ServerError> {
        let Some(journal) = self.switching()? else {
            return Ok(Settled::Nothing);
        };
        self.hold_as(by, "settling a switch that stopped")?;
        if journal.committed {
            self.finish(&journal, at("committed") + 1)?;
        } else {
            self.back(&journal)
                .and_then(|()| self.closed(&journal))
                .map_err(|why| {
                    ServerError::Io(format!(
                        "the {} to {} that stopped before it committed could not be undone \
                         ({why}): the board stays held and its journal stands",
                        journal.word(),
                        journal.to
                    ))
                })?;
        }
        if journal.took {
            self.release_as(by, None)?;
        }
        Ok(if journal.committed {
            Settled::Finished(journal)
        } else {
            Settled::Undone(journal)
        })
    }

    /// The board's store moved to `to`, as `by`, at `now` (the module's doc says how).
    /// `programs` is the folder of the PostgreSQL programs that serve the side that is
    /// PostgreSQL. The hold is the caller's where it set one before, and is then left standing;
    /// otherwise the move sets it and releases it once the board is whole on one engine. A
    /// journal an earlier call left is settled instead: a committed move finished, which is
    /// what this then answers, or one that never committed undone, which is refused, saying so.
    ///
    /// # Errors
    ///
    /// Each refusal and failure, naming the engine the board is on and whether it is held.
    pub fn move_to(
        &self,
        to: Engine,
        programs: &Path,
        by: &str,
        now: f64,
    ) -> Result<Moved, ServerError> {
        let _locked = self.lock(Doing::Moving)?;
        match self.settle(by)? {
            Settled::Nothing => {}
            Settled::Finished(journal) => {
                return Ok(Moved {
                    from: journal.from,
                    to: journal.to,
                    aside: journal.aside,
                    left: Vec::new(),
                    snapshot: None,
                    rows: 0,
                    finished: true,
                    held: !journal.took,
                });
            }
            Settled::Undone(journal) => {
                return Err(ServerError::Io(format!(
                    "a {} to {} had stopped before it committed, and is now undone: the board \
                     is on {} as it was, {}. Nothing was moved by this command; run it again",
                    journal.word(),
                    journal.to,
                    journal.from,
                    if journal.took {
                        "its hold released"
                    } else {
                        "still held"
                    }
                )));
            }
        }
        let from = self.backend()?;
        if from == to {
            return Err(ServerError::Io(format!(
                "the board is on {to} already: nothing was moved"
            )));
        }
        if !present(&self.dir.join(from.data()))? {
            return Err(ServerError::Io(format!(
                "the board has no {from} store at {} to move: nothing was moved",
                self.dir.join(from.data()).display()
            )));
        }
        let took = self.held_for(by, &format!("moving the board to {to}"))?;
        let work = self.dir.join(WORK);
        let left = |why: ServerError, released: bool| {
            ServerError::Io(format!(
                "the board was not moved: {why}. It is on {from} as it was, {}",
                if released {
                    "and its hold is released"
                } else {
                    "and still held"
                }
            ))
        };
        let prepared = self.prepare((from, to), programs, &work, now);
        let (aside, snapshot, rows, others) = match prepared {
            Ok(prepared) => prepared,
            Err(why) => {
                // The work folder is the move's own; the home was never touched.
                let _ = fs::remove_dir_all(&work);
                if took {
                    self.release_as(by, None)?;
                }
                return Err(left(why, took));
            }
        };
        let journal = Journal::new(
            Kind::Move,
            (from, to),
            &aside,
            &work.join(to.data()),
            &work,
            (by, took),
        );
        match self.switch(journal) {
            Ok(()) => {}
            Err(Left::Unchanged(why)) => {
                let _ = fs::remove_dir_all(&work);
                if took {
                    self.release_as(by, None)?;
                }
                return Err(left(why, took));
            }
            Err(Left::Standing(why)) => return Err(why),
        }
        if took {
            self.release_as(by, None)?;
        }
        Ok(Moved {
            from,
            to,
            aside,
            left: others,
            snapshot,
            rows,
            finished: false,
            held: !took,
        })
    }

    /// Everything of a move before its switch: the old store's server stopped and its snapshot
    /// kept, the board read from it and written as the export, the new store made in `work`,
    /// filled and compared. The folder the old store will be kept in, the snapshot taken, the
    /// rows compared, and the databases the old store holds beside the board's, which it keeps.
    fn prepare(
        &self,
        (from, to): (Engine, Engine),
        programs: &Path,
        work: &Path,
        now: f64,
    ) -> Result<(PathBuf, Option<PathBuf>, usize, Vec<String>), ServerError> {
        let halted = match from {
            Engine::SurrealDb => self.halt()?,
            Engine::PostgreSql => {
                self.postgresql(programs).stop_held()?;
                Vec::new()
            }
        };
        let snapshot = match from {
            Engine::SurrealDb => match self.keep(now, Before::Move)? {
                Kept::Cloned(path) | Kept::Copied(path) => Some(path),
                Kept::Already(_) => None,
            },
            Engine::PostgreSql => Some(self.postgresql(programs).snapshot_held(now)?),
        };
        let source = Side::start(self, from, programs, &halted)?;
        let read = (source.board()).and_then(|board| Ok((board, source.others()?)));
        source.end()?;
        let ((board, carried, marker), others) = read?;
        // No lease stands across the hold, and none is its old holder's after it.
        let carried = crate::store::leases_ended(carried);

        let aside = self.aside(now, from);
        fs::create_dir_all(&aside).map_err(io("could not make", &aside))?;
        let export = aside.join(EXPORT);
        let text = serde_json::to_vec(&json!({"board": board, "carried": carried}))
            .expect("an export serializes");
        fs::write(&export, text).map_err(io("could not write", &export))?;

        if present(work)? {
            fs::remove_dir_all(work).map_err(io("could not remove", work))?;
        }
        self.make(work)?;
        let made = Home {
            dir: work.into(),
            cut: None,
            ..self.clone()
        }
        .supervised(true)
        .snapshotting(false);
        made.identify(to)?;
        let target = Side::start(&made, to, programs, &halted)?;
        let compared = target.fill(&board, &carried, marker);
        target.end()?;
        Ok((aside, snapshot, compared?, others))
    }
}

/// The place of `step` in [`STEPS`].
fn at(step: &str) -> usize {
    (STEPS.iter().position(|named| *named == step))
        .unwrap_or_else(|| panic!("{step:?} is no step of a switch: {STEPS:?}"))
}

fn stopped(done: usize) -> ServerError {
    ServerError::Io(format!("a test ended the switch after {:?}", STEPS[done]))
}

/// One store of a move, served by the move's own server, which no record names.
enum Side<'a> {
    Surreal(&'a Home, Running),
    Postgres(Postgres),
}

impl<'a> Side<'a> {
    fn start(
        home: &'a Home,
        engine: Engine,
        programs: &Path,
        halted: &[u16],
    ) -> Result<Side<'a>, ServerError> {
        match engine {
            Engine::SurrealDb => {
                let running = home.spawn_held(
                    &store(&home.dir.join(engine.data())),
                    halted,
                    home.storage,
                    home.patience,
                    "the move's server",
                    &|_| Ok(()),
                )?;
                Ok(Side::Surreal(home, running))
            }
            Engine::PostgreSql => {
                let postgres = home.postgresql(programs);
                postgres.serve()?;
                Ok(Side::Postgres(postgres))
            }
        }
    }

    fn end(&self) -> Result<(), ServerError> {
        match self {
            Side::Surreal(home, running) => home.end(running.pid),
            Side::Postgres(postgres) => postgres.stop_held().map(|_| ()),
        }
    }

    fn database(&self) -> Database {
        match self {
            Side::Surreal(_, running) => {
                crate::rpc::Client::new(&running.addr, crate::NAMESPACE, crate::DATABASE).into()
            }
            Side::Postgres(postgres) => {
                crate::pg::Client::new(&postgres.socket(), crate::DATABASE).into()
            }
        }
    }

    /// The databases this store holds beside the board's own, which a move leaves in it.
    fn others(&self) -> Result<Vec<String>, ServerError> {
        match self {
            Side::Surreal(_, running) => {
                let mut others = Vec::new();
                for (ns, db) in super::reclaim::layout(running)?.databases {
                    if (ns.as_str(), db.as_str()) == (crate::NAMESPACE, crate::DATABASE) {
                        continue;
                    }
                    // SurrealDB makes a database of its own in every store, which defines
                    // nothing: only one that holds something is another's.
                    let info = super::reclaim::defined(running, &ns, &db)?;
                    if !defines_nothing(&info) {
                        others.push(format!("{ns}/{db}"));
                    }
                }
                Ok(others)
            }
            Side::Postgres(postgres) => {
                let answers = crate::pg::Client::new(&postgres.socket(), "postgres")
                    .simple(
                        "SELECT datname FROM pg_database WHERE NOT datistemplate \
                         AND datname <> 'postgres' ORDER BY 1",
                    )
                    .map_err(|why| {
                        ServerError::Io(format!(
                            "the databases of the board's PostgreSQL could not be read: {why:?}"
                        ))
                    })?;
                Ok((answers.iter().flat_map(|answer| &answer.rows))
                    .filter_map(|row| row.first().cloned().flatten())
                    .filter(|name| name != crate::DATABASE)
                    .collect())
            }
        }
    }

    fn opened(&self) -> Store {
        Store::unchecked_on(
            self.database(),
            Box::new(crate::store::now),
            Box::new(crate::sys::pid_alive),
            &crate::sys::hostname(),
            &Opening::write(),
        )
    }

    /// The board this store holds, the tables no copy holds, and its marker; a board whose
    /// schema is not this build's is refused. The databases beside it are not read.
    fn board(&self) -> Result<(Dump, Value, u64), ServerError> {
        if let Side::Surreal(home, running) = self {
            home.refuse_went_back(running)?;
        }
        let store = self.opened();
        let unread = |what: &str| {
            let what = what.to_string();
            move |why: crate::Error| ServerError::Io(format!("{what} could not be read: {why}"))
        };
        let schema = (store.board_version()).map_err(unread("the board's schema"))?;
        if schema != Some(crate::SCHEMA) {
            return Err(ServerError::Io(format!(
                "the board is at schema {}, and this build moves a board at its own, {}: a move \
                 migrates nothing",
                schema.map_or("none".into(), |schema| schema.to_string()),
                crate::SCHEMA
            )));
        }
        Ok((
            store.dump().map_err(unread("the board"))?,
            (store.carried()).map_err(unread("the tables a move carries"))?,
            store.marker().map_err(unread("the board's marker"))?,
        ))
    }

    /// `board` and `carried` written on this new store, its marker moved past `marker`, and
    /// both read back as they were written: how many rows.
    fn fill(&self, board: &Dump, carried: &Value, marker: u64) -> Result<usize, ServerError> {
        let unwritten = |what: &str| {
            let what = what.to_string();
            move |why: crate::Error| ServerError::Io(format!("{what}: {why}"))
        };
        let store = Store::opening_on(
            self.database(),
            Box::new(crate::store::now),
            Box::new(crate::sys::pid_alive),
            &crate::sys::hostname(),
            &Opening::write(),
        )
        .map_err(unwritten("the new store's board could not be made"))?;
        (store.copy(board)).map_err(unwritten("the board could not be copied"))?;
        (store.carry(carried)).map_err(unwritten("the carried tables could not be written"))?;
        (store.marker_past(marker)).map_err(unwritten("the marker could not be moved"))?;
        let (there, kept, _) = self.board()?;
        let rows = differing(&json!(board), &json!(there))? + differing(carried, &kept)?;
        Ok(rows)
    }
}

/// Whether `a` and `b` are one value: alike in every part, a number by what it counts, as one
/// engine writes a whole number of seconds `3.0` where the other writes `3`.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => match (a.as_i64(), b.as_i64()) {
            (Some(a), Some(b)) => a == b,
            _ => a == b || a.as_f64() == b.as_f64(),
        },
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len() && (a.iter()).all(|(key, a)| b.get(key).is_some_and(|b| same(a, b)))
        }
        _ => a == b,
    }
}

/// Whether `info`, SurrealDB's `INFO FOR DB`, names nothing defined: an object, every part of
/// it empty. An answer of another shape is taken to hold something.
fn defines_nothing(info: &Value) -> bool {
    info.as_object().is_some_and(|parts| {
        parts.values().all(|part| match part {
            Value::Object(named) => named.is_empty(),
            Value::Array(listed) => listed.is_empty(),
            Value::Null => true,
            _ => false,
        })
    })
}

/// The rows of `read`, tables of rows by name, when `written` holds the same, row for row; the
/// first that differs is refused, named by its table and place.
fn differing(written: &Value, read: &Value) -> Result<usize, ServerError> {
    let none = Vec::new();
    let empty = serde_json::Map::new();
    let (a, b) = (
        written.as_object().unwrap_or(&empty),
        read.as_object().unwrap_or(&empty),
    );
    let mut rows = 0;
    for table in a
        .keys()
        .chain(b.keys().filter(|table| !a.contains_key(*table)))
    {
        let was = a.get(table).and_then(Value::as_array).unwrap_or(&none);
        let is = b.get(table).and_then(Value::as_array).unwrap_or(&none);
        let differs = (0..was.len().max(is.len())).find(|&row| match (was.get(row), is.get(row)) {
            (Some(was), Some(is)) => !same(was, is),
            _ => true,
        });
        if let Some(row) = differs {
            return Err(ServerError::Io(format!(
                "the new store does not hold the board as it was read: {table} has {} rows \
                 against {}, and row {row} is {} against {}",
                is.len(),
                was.len(),
                is.get(row).unwrap_or(&Value::Null),
                was.get(row).unwrap_or(&Value::Null)
            )));
        }
        rows += was.len();
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_the_same_only_row_for_row_and_the_first_difference_is_named() {
        let board = json!({"tasks": [{"id": 1}, {"id": 2}], "asks": []});
        assert_eq!(differing(&board, &board).unwrap(), 2);
        for (other, said) in [
            (
                json!({"tasks": [{"id": 1}], "asks": []}),
                "tasks has 1 rows against 2, and row 1",
            ),
            (
                json!({"tasks": [{"id": 1}, {"id": 3}], "asks": []}),
                "row 1 is {\"id\":3}",
            ),
            (
                json!({"tasks": [{"id": 1}, {"id": 2}], "asks": [], "more": [1]}),
                "more has 1 rows against 0",
            ),
            (json!({"asks": []}), "tasks has 0 rows against 2, and row 0"),
        ] {
            let refused = differing(&board, &other).unwrap_err().to_string();
            assert!(refused.contains(said), "{refused}");
        }
    }

    #[test]
    fn a_number_is_the_same_by_what_it_counts_and_nothing_else_is_the_same_loosely() {
        let whole = json!({"ts": 1_800_000_000.0, "n": [1, {"k": 2.5}], "t": "3"});
        assert!(same(
            &whole,
            &json!({"n": [1.0, {"k": 2.5}], "t": "3", "ts": 1_800_000_000})
        ));
        for other in [
            json!({"ts": 1_800_000_001, "n": [1, {"k": 2.5}], "t": "3"}),
            json!({"ts": 1_800_000_000, "n": [1, {"k": 2.5}], "t": 3}),
            json!({"ts": 1_800_000_000, "n": [1, {"k": 2.5}]}),
            json!({"ts": 1_800_000_000, "n": [1, {"k": 2.5}, 0], "t": "3"}),
            json!({"ts": 1_800_000_000, "n": [1, {"j": 2.5}], "t": "3"}),
            json!({"ts": null, "n": [1, {"k": 2.5}], "t": "3"}),
        ] {
            assert!(!same(&whole, &other), "{other}");
        }
        let (large, next) = (
            json!(9_007_199_254_740_993_i64),
            json!(9_007_199_254_740_992_i64),
        );
        assert!(
            !same(&large, &next),
            "whole numbers are never compared as fractions"
        );
    }

    #[test]
    fn a_database_defines_nothing_only_when_every_part_of_its_info_is_empty() {
        assert!(defines_nothing(
            &json!({"tables": {}, "users": {}, "configs": []})
        ));
        assert!(defines_nothing(&json!({})));
        for holds in [
            json!({"tables": {"row": "DEFINE TABLE row"}, "users": {}}),
            json!({"tables": {}, "strict": true}),
            json!(null),
            json!("an error"),
        ] {
            assert!(!defines_nothing(&holds), "{holds}");
        }
    }

    #[test]
    fn a_journal_reads_whole_or_not_at_all() {
        let dir = std::env::temp_dir().join(format!("moving-journal-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(JOURNAL);
        assert_eq!(read(&path).unwrap(), None);
        let journal = Journal::new(
            Kind::Move,
            (Engine::SurrealDb, Engine::PostgreSql),
            &dir.join("moved/one"),
            &dir.join("moving/postgresql"),
            &dir.join("moving"),
            ("amber-heron", true),
        );
        fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
        assert_eq!(read(&path).unwrap(), Some(journal.clone()));
        assert!(unsettled(&path).contains("a move to PostgreSQL stopped before it committed"));
        let committed = Journal {
            committed: true,
            ..journal.clone()
        };
        fs::write(&path, serde_json::to_vec(&committed).unwrap()).unwrap();
        assert!(unsettled(&path).contains("a move to PostgreSQL committed and is not finished"));
        for torn in ["", "{", "{\"format\":2}", "[]"] {
            fs::write(&path, torn).unwrap();
            let refused = read(&path).unwrap_err().to_string();
            assert!(
                refused.contains("does not read as a switch's journal"),
                "{refused}"
            );
            assert!(unsettled(&path).contains("a move or a restore stopped part way"));
        }
        let newer = serde_json::to_string(&Journal {
            format: 2,
            ..journal
        })
        .unwrap();
        fs::write(&path, newer).unwrap();
        assert!(read(&path).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    #[should_panic(expected = "is no step of a switch")]
    fn a_step_is_named_as_the_switch_names_it() {
        at("renamed");
    }
}
