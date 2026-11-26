//! What tells a project's people of its notices, one look and one hand at a time
//! ([`super`]): a look makes a notice for each person newly told of a card that waits or of a board
//! line after the record's, and the hand of each one due runs its program, again after a `75`
//! within [`MOST_HANDS`], each step written to the record before the next. A notice goes by the
//! way its person's profile chose for its kind ([`Kind::chosen_as`]: a held message by a
//! question's).

use std::path::{Path, PathBuf};
use symphony::notices::{Key, Kind, Programs, line};
use symphony_board::people::ownership::{Owning, told};
use symphony_board::people::{People, Profiles};

use super::hand::Handed;
use super::record::{Pending, Record, new_id};

/// The most times a notice's program is started for it: after the last `75` it is dropped.
pub(crate) const MOST_HANDS: u32 = 8;
/// The wait before the first hand again, doubled after each, up to [`LONGEST_AGAIN`].
const FIRST_AGAIN: f64 = 60.0;
const LONGEST_AGAIN: f64 = 3600.0;

/// How long after its `hands`-th hand, which gave a `75`, a notice is handed again: a minute,
/// doubled each time, at most an hour.
pub(crate) fn again_after(hands: u32) -> f64 {
    let doubled = FIRST_AGAIN * f64::from(1u32 << hands.saturating_sub(1).min(16));
    doubled.min(LONGEST_AGAIN)
}

/// What the key of a comment held on a document begins with, `held:N` ([`Waiting::key`]).
pub(crate) const HELD: &str = "held:";

/// A thing waiting now, as a look sees it: a card waiting on its group, or a comment held on a
/// document waiting for someone to pass it on.
#[derive(Clone, Debug)]
pub(crate) struct Waiting {
    /// `DOC#SLUG` for a card, `held:N` ([`HELD`]) for a held comment, as the record keeps it.
    pub key: String,
    /// Its page under the project: `/DOC#SLUG`, a held comment's `/DOC#held-N`.
    pub page: String,
    pub owning: Owning,
    /// [`Kind::Card`], or [`Kind::Held`].
    pub kind: Kind,
    /// Who is told of it where that is not whose it is ([`told`]): of a held comment, those who
    /// may pass it on.
    pub told: Option<Vec<String>>,
}

impl Waiting {
    /// The card `key` waiting on its group, `owning`, at `page`.
    pub(crate) fn card(key: String, page: String, owning: Owning) -> Waiting {
        Waiting {
            key,
            page,
            owning,
            kind: Kind::Card,
            told: None,
        }
    }

    /// Who on `people` is told of it.
    fn told(&self, people: &People) -> Vec<String> {
        (self.told.clone()).unwrap_or_else(|| told(people, &self.owning))
    }
}

/// A board line after the record's that tells: an agent's question to the person at the machine,
/// a reviewer's verdict, or the board's line of a held message.
#[derive(Clone, Debug)]
pub(crate) struct Line {
    /// Its id on the board.
    pub id: i64,
    pub kind: Kind,
    /// Its page under the project: its task's, `/task/SLUG`, and a held message's place on it,
    /// `/task/SLUG#message-ID`.
    pub page: String,
    /// Who is told of it, by short name: whose its task is (`ownership::told`), and of a held
    /// message who may pass it on.
    pub told: Vec<String>,
}

/// What one look reads.
pub(crate) struct Look<'a> {
    pub people: &'a People,
    /// The cards waiting now; none where the documents are not read yet, and the spells stand.
    pub cards: Option<Vec<Waiting>>,
    /// Whether `cards` holds the comments held on documents: where they did not read, it holds
    /// none of them, and theirs stand as the record has them.
    pub held_read: bool,
    /// The lines after [`Runner::cursor`], through `latest`.
    pub lines: Vec<Line>,
    /// The board's newest line now.
    pub latest: i64,
}

/// One project's runner: its id, its record's file, the machine's key for the digests the record
/// keeps, the lock it holds on the record, and the record.
pub(crate) struct Runner {
    project: String,
    path: PathBuf,
    key: Key,
    /// Held while it is open ([`symphony::notices::RECORD_LOCK`]): no other runner, of this server
    /// or another, opens the record meanwhile.
    _held: std::fs::File,
    /// None until the first look, which takes what stands then as told.
    record: Option<Record>,
}

/// Prints the project and the record's place, never an address (the record holds none).
impl std::fmt::Debug for Runner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runner")
            .field("project", &self.project)
            .field("path", &self.path)
            .field("record", &self.record)
            .finish()
    }
}

impl Runner {
    /// The runner of the project `project` at `root`, its record read from its artifacts folder,
    /// once it holds the record's lock, and its digests made under `key`; with why where a record
    /// is there and does not read, which is then made anew at the first look, as one never made.
    ///
    /// # Errors
    ///
    /// Another runner holds the lock, of this server or another (a reload's other generation), or
    /// the lock cannot be made: the record is not read.
    pub(crate) fn open(
        root: &Path,
        project: &str,
        key: Key,
    ) -> Result<(Runner, Option<String>), String> {
        let project_file = symphony::project::Project::of(root);
        let path = root.join(project_file.artifact(symphony::notices::RECORD));
        let held = hold(&root.join(project_file.artifact(symphony::notices::RECORD_LOCK)))?;
        let (record, unread) = match Record::read(&path) {
            Ok(record) => (record, None),
            Err(why) => (None, Some(why)),
        };
        let runner = Runner {
            project: project.into(),
            path,
            key,
            _held: held,
            record,
        };
        Ok((runner, unread))
    }

    /// The newest board line looked at; none before the first look.
    pub(crate) fn cursor(&self) -> Option<i64> {
        self.record.as_ref().map(|record| record.line)
    }

    /// When the next notice is due, in seconds since the epoch: none with none to hand.
    pub(crate) fn next_due(&self) -> Option<f64> {
        let pending = &self.record.as_ref()?.pending;
        (pending.iter())
            .map(|notice| if notice.handing { 0.0 } else { notice.due })
            .reduce(f64::min)
    }

    /// Takes `look` in at `now`: a notice made for each person newly told, reached by
    /// `profiles` by a way `programs` names a program for; `origin` (`http://HOST:PORT`) begins
    /// each link, asked once by a look that makes one and never by one that makes none. The first
    /// look makes none: what waits then, and the lines before it, are told already. A first look
    /// with no cards read, or no held comments, waits for one that has them.
    ///
    /// # Errors
    ///
    /// When the record cannot be written: the look is taken again at the next one.
    pub(crate) fn look(
        &mut self,
        look: &Look<'_>,
        profiles: &Profiles,
        programs: &Programs,
        now: f64,
        origin: &mut dyn FnMut() -> String,
    ) -> Result<(), String> {
        let Some(before) = &self.record else {
            let Some(cards) = look.cards.as_ref().filter(|_| look.held_read) else {
                return Ok(());
            };
            let mut told = std::collections::BTreeMap::new();
            for card in cards {
                let mut people = std::collections::BTreeSet::new();
                people.extend(card.told(look.people));
                told.insert(card.key.clone(), people);
            }
            let record = Record {
                line: look.latest,
                cards: told,
                pending: Vec::new(),
            };
            record.write(&self.path)?;
            self.record = Some(record);
            return Ok(());
        };
        let mut record = before.clone();
        let mut begun: Option<String> = None;
        let mut make = |record: &mut Record, person: &str, kind: Kind, page: &str| {
            let Some(reach) = profiles.reach(person, kind.chosen_as()) else {
                return;
            };
            if programs.program(reach.way()).is_none() {
                return;
            }
            record.pending.push(Pending {
                id: new_id(),
                kind: kind.word().into(),
                link: format!(
                    "{}/{}{page}",
                    begun.get_or_insert_with(&mut *origin),
                    self.project
                ),
                person: person.into(),
                way: reach.way().into(),
                digest: self.key.digest(reach.address()),
                hands: 0,
                due: now,
                handing: false,
            });
        };
        let after = record.line;
        for line in look.lines.iter().filter(|line| line.id > after) {
            for person in &line.told {
                make(&mut record, person, line.kind, &line.page);
            }
        }
        record.line = record.line.max(look.latest);
        if let Some(cards) = &look.cards {
            let mut spells = std::collections::BTreeMap::new();
            for card in cards {
                let mut spell = record.cards.remove(&card.key).unwrap_or_default();
                for person in card.told(look.people) {
                    if spell.insert(person.clone()) {
                        make(&mut record, &person, card.kind, &card.page);
                    }
                }
                spells.insert(card.key.clone(), spell);
            }
            // The held comments did not read: each stands as told, so none is told again once
            // they read.
            if !look.held_read {
                let held = std::mem::take(&mut record.cards).into_iter();
                spells.extend(held.filter(|(key, _)| key.starts_with(HELD)));
            }
            record.cards = spells;
        }
        if record != *before {
            record.write(&self.path)?;
            self.record = Some(record);
        }
        Ok(())
    }

    /// Hands each notice due by `clock`, one at a time, through `hand` (the program, the message,
    /// the words it must not repeat): its address read again from `profiles`, by the program
    /// `programs` names for its way. Each is written to the record as handing before its program
    /// starts, and settled after: sent, or not sent for good, it leaves; not sent this time, it is
    /// due again ([`again_after`]) until its [`MOST_HANDS`]-th hand. One whose person no longer
    /// goes by its way, or whose address changed since it was made, leaves unhanded, and so does
    /// one whose way no program is named for now. A stop leaves the one under way handing, to be
    /// handed again with the same id. `log` hears each outcome, by the notice's id, kind, person
    /// and way, never its address or link.
    ///
    /// # Errors
    ///
    /// When the record cannot be written: nothing more is handed until it can. A hand whose mark
    /// is not written starts no program and uses up none of the notice's hands; an outcome not
    /// written stands in memory, and the next write carries it.
    pub(crate) fn hand_due(
        &mut self,
        profiles: &Profiles,
        programs: &Programs,
        clock: &dyn Fn() -> f64,
        hand: &mut dyn FnMut(&Path, &str, &[&str]) -> Handed,
        log: &mut dyn FnMut(String),
    ) -> Result<(), String> {
        loop {
            let Some(record) = &mut self.record else {
                return Ok(());
            };
            let now = clock();
            let Some(at) = (record.pending.iter()).position(|n| n.handing || n.due <= now) else {
                return Ok(());
            };
            let notice = record.pending[at].clone();
            let said = format!(
                "NOTICE {} {} {} for {} by {}",
                self.project, notice.id, notice.kind, notice.person, notice.way
            );
            let kind = Kind::of(&notice.kind);
            let reach = kind
                .and_then(|kind| profiles.reach(&notice.person, kind.chosen_as()))
                .filter(|reach| {
                    reach.way() == notice.way && self.key.digest(reach.address()) == notice.digest
                });
            let program = programs.program(&notice.way);
            let why = if kind.is_none() || reach.is_none() {
                "its person's way or address for it changed since it was made"
            } else {
                "no program is named for its way now"
            };
            let (Some(kind), Some(reach), Some(program)) = (kind, reach, program) else {
                log(format!("{said}: dropped: {why}"));
                record.pending.remove(at);
                record.write(&self.path)?;
                continue;
            };
            let program = program.to_path_buf();
            // Counted and marked handing on the record before its program starts, and in memory
            // only once that is written: a write that fails starts nothing and uses up no hand.
            let mut marked = record.clone();
            let handing = &mut marked.pending[at];
            handing.hands += 1;
            handing.handing = true;
            marked.write(&self.path)?;
            *record = marked;
            let message = line(
                &notice.id,
                kind,
                &self.project,
                &notice.link,
                &notice.way,
                reach.address(),
            );
            let handed = hand(&program, &message, &[reach.address(), &notice.link]);
            let hands = notice.hands + 1;
            match handed {
                Handed::Cut => return Ok(()),
                Handed::Sent => {
                    log(format!("{said}: sent"));
                    record.pending.remove(at);
                }
                Handed::Never(why) => {
                    log(format!("{said}: not sent: {why}"));
                    record.pending.remove(at);
                }
                Handed::Later(why) if hands >= MOST_HANDS => {
                    log(format!(
                        "{said}: not sent after {hands} hands, so dropped: {why}"
                    ));
                    record.pending.remove(at);
                }
                Handed::Later(why) => {
                    let again = again_after(hands);
                    log(format!(
                        "{said}: not sent this time (hand {hands} of {MOST_HANDS}), again in \
                         {again:.0} s: {why}"
                    ));
                    let waiting = &mut record.pending[at];
                    waiting.handing = false;
                    waiting.due = clock() + again;
                }
            }
            record.write(&self.path)?;
        }
    }
}

/// The lock at `path`, made where it is not there, held by the file this answers until it closes.
fn hold(path: &Path) -> Result<std::fs::File, String> {
    use std::os::unix::fs::OpenOptionsExt as _;
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|e| symphony::text::os_error(&e, folder))?;
    }
    let file = (std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true))
    .truncate(false)
    .mode(0o600)
    .open(path)
    .map_err(|e| symphony::text::os_error(&e, path))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(std::fs::TryLockError::WouldBlock) => Err(format!(
            "{} is held by another server's notices, which tells this project now",
            path.display()
        )),
        Err(std::fs::TryLockError::Error(error)) => Err(symphony::text::os_error(&error, path)),
    }
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;
