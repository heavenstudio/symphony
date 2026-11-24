//! What tells a project's people of its notices, one look and one hand at a time
//! ([`super`]): a look makes a notice for each person newly told of a card that waits or of a board
//! line after the record's, and the hand of each one due runs its program, again after a `75`
//! within [`MOST_HANDS`], each step written to the record before the next.

use std::path::{Path, PathBuf};
use symphony::notices::{Kind, Programs, line};
use symphony_board::people::ownership::{Owning, told};
use symphony_board::people::{People, Profiles};

use super::hand::Handed;
use super::record::{Pending, Record, digest, new_id};

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

/// A card waiting now, as a look sees it.
#[derive(Clone, Debug)]
pub(crate) struct Waiting {
    /// `DOC#SLUG`, as the record keeps it.
    pub key: String,
    /// Its page under the project, `/DOC#SLUG`.
    pub page: String,
    pub owning: Owning,
}

/// A board line after the record's that tells: an agent's question to the person at the machine,
/// or a reviewer's verdict.
#[derive(Clone, Debug)]
pub(crate) struct Line {
    /// Its id on the board.
    pub id: i64,
    pub kind: Kind,
    /// Its task's page under the project, `/task/SLUG`.
    pub page: String,
    /// Whose its task is.
    pub owning: Owning,
}

/// What one look reads.
pub(crate) struct Look<'a> {
    pub people: &'a People,
    /// The cards waiting now; none where the documents are not read yet, and the spells stand.
    pub cards: Option<Vec<Waiting>>,
    /// The lines after [`Runner::cursor`], through `latest`.
    pub lines: Vec<Line>,
    /// The board's newest line now.
    pub latest: i64,
}

/// One project's runner: its id, its record's file and the record.
pub(crate) struct Runner {
    project: String,
    path: PathBuf,
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
    /// The runner of the project `project` at `root`, its record read from its artifacts folder;
    /// with why where a record is there and does not read, which is then made anew at the first
    /// look, as one never made.
    pub(crate) fn open(root: &Path, project: &str) -> (Runner, Option<String>) {
        let artifact = symphony::project::Project::of(root).artifact(symphony::notices::RECORD);
        let path = root.join(artifact);
        let (record, unread) = match Record::read(&path) {
            Ok(record) => (record, None),
            Err(why) => (None, Some(why)),
        };
        let runner = Runner {
            project: project.into(),
            path,
            record,
        };
        (runner, unread)
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
    /// each link. The first look makes none: what waits then, and the lines before it, are told
    /// already. A first look with no cards read waits for one that has them.
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
            let Some(cards) = &look.cards else {
                return Ok(());
            };
            let record = Record {
                line: look.latest,
                cards: (cards.iter())
                    .map(|card| {
                        let told = told(look.people, &card.owning).into_iter().collect();
                        (card.key.clone(), told)
                    })
                    .collect(),
                pending: Vec::new(),
            };
            record.write(&self.path)?;
            self.record = Some(record);
            return Ok(());
        };
        let mut record = before.clone();
        let mut make = |record: &mut Record, person: &str, kind: Kind, page: &str| {
            let Some(reach) = profiles.reach(person, kind.word()) else {
                return;
            };
            if programs.program(reach.way()).is_none() {
                return;
            }
            record.pending.push(Pending {
                id: new_id(),
                kind: kind.word().into(),
                link: format!("{}/{}{page}", origin(), self.project),
                person: person.into(),
                way: reach.way().into(),
                digest: digest(reach.address()),
                hands: 0,
                due: now,
                handing: false,
            });
        };
        let after = record.line;
        for line in look.lines.iter().filter(|line| line.id > after) {
            for person in told(look.people, &line.owning) {
                make(&mut record, &person, line.kind, &line.page);
            }
        }
        record.line = record.line.max(look.latest);
        if let Some(cards) = &look.cards {
            let mut spells = std::collections::BTreeMap::new();
            for card in cards {
                let mut spell = record.cards.remove(&card.key).unwrap_or_default();
                for person in told(look.people, &card.owning) {
                    if spell.insert(person.clone()) {
                        make(&mut record, &person, Kind::Card, &card.page);
                    }
                }
                spells.insert(card.key.clone(), spell);
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
    /// When the record cannot be written: nothing more is handed until it can.
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
                .and_then(|kind| profiles.reach(&notice.person, kind.word()))
                .filter(|reach| reach.way() == notice.way && digest(reach.address()) == notice.digest);
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
            {
                let handing = &mut record.pending[at];
                handing.hands += 1;
                handing.handing = true;
            }
            record.write(&self.path)?;
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

#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;
