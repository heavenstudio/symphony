//! The switch of a home's data under a journal, which a restore over the board
//! ([`super::Postgres::restore_over`]) takes, and which a move between the engines took in the
//! releases that had one ([`Home::move_to`] settles a journal one of them left).
//!
//! The switch, each step durable before the next, under a journal (`move.json`) written first:
//! the old data moved aside into `moved/STAMP-ENGINE/`, the new data moved in, the identity
//! written, the journal marked committed, the home's recovery bindings revoked, the journal
//! removed. Marking the journal is the commit point. Before it, a failure is undone at once and
//! the board is on its old data as it was, its bindings untouched; after it, the board is on its
//! new data and what is left is finished. While the journal stands nothing reads the home as
//! either engine ([`Home::backend`] refuses), no start runs and the hold is not released; only
//! the holder's next `tasks-move` settles it, undoing what never committed or finishing what
//! did, each step told by the journal and by what is on disk. A step that can be neither
//! finished nor undone leaves the hold and the journal, and says so.
//!
//! Nothing is deleted but the switch's work folder: the old data stays in `moved/`, and the
//! snapshot stays with the snapshots.
//!
//! The new board holds a recovery generation of its own, and the home is bound to none
//! ([`crate::store::recovery`]): a token read before the switch is refused, and `tasks recovery
//! rebind` binds the board again.

use super::backend::Engine;
use super::snapshot::stamp;
use super::starters::Doing;
use super::{Home, ServerError, io};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// The journal of a switch under way, in the board's home.
pub(in crate::server) const JOURNAL: &str = "move.json";
/// Where the stores a switch replaced are kept, each in a folder of its own.
const ASIDE: &str = "moved";
/// The export a move of an earlier release kept beside the store it replaced.
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

/// The move [`Home::move_to`] finished: one an earlier release's `tasks-move` committed.
#[derive(Debug, Clone, PartialEq)]
pub struct Moved {
    pub from: Engine,
    pub to: Engine,
    /// The folder that keeps the store the move replaced, and its export.
    pub aside: PathBuf,
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
         settles it with `tasks-move`, which finishes what committed and undoes what did not",
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

    /// A journal an earlier call left settled, as `by`, before anything else: a committed move
    /// finished, which is what this answers, or a switch that never committed undone, which is
    /// refused, saying so. Then the board stays where it is: this build moves no board between
    /// the engines, so a board on `to` already or not made yet has nothing to move, and one on
    /// the other engine is refused, naming the way it moves ([`super::GONE`]). The hold a
    /// settle takes is released after it where the switch had set it.
    ///
    /// # Errors
    ///
    /// Each refusal and failure, naming the engine the board is on and whether it is held.
    pub fn move_to(&self, to: Engine, by: &str) -> Result<Moved, ServerError> {
        let _locked = self.lock(Doing::Moving)?;
        match self.settle(by)? {
            Settled::Nothing => {}
            Settled::Finished(journal) => {
                return Ok(Moved {
                    from: journal.from,
                    to: journal.to,
                    aside: journal.aside,
                    held: !journal.took,
                });
            }
            Settled::Undone(journal) => {
                return Err(ServerError::Io(format!(
                    "a {} to {} had stopped before it committed, and is now undone: the board \
                     is on {} as it was, {}. Nothing was moved by this command",
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
        if self.unmade()? {
            return Err(ServerError::Io(
                "the board is not made yet: it has nothing to move".into(),
            ));
        }
        let from = self.backend()?;
        if from == to {
            return Err(ServerError::Io(format!(
                "the board is on {to} already: nothing was moved"
            )));
        }
        Err(ServerError::Io(format!(
            "the board is on {from}, and this build moves no board to {to}: {}. Nothing was moved",
            super::GONE
        )))
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

#[cfg(test)]
mod tests {
    use super::*;

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
