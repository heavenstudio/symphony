//! The highest schema a project's live board has been seen at, kept in the board's folder beside its
//! data (`symphony/tasks/schema-seen.log` in the git common dir), never in the database, so a store
//! that loses its writes cannot take the record with it.
//!
//! Every store that opens a live board reads its schema once ([`crate::Store::opening`]); before it
//! decides anything by it, it reads this record. A board that reads below the record went back
//! ([`went_back`]): its store came back older than it was, so what was written since is lost, and a
//! migrate would write on top of the older state. Every program that opens the live board refuses
//! it with the [`Rollback`]'s words, before anything is read or written: the stores that open it
//! as their project's, `tasks migrate` before it asks where it runs ([`checked`]), the stores that
//! take its tables as they are ([`checked`], and the heartbeat's [`past_guard`]) and
//! `tasks-reclaim` ([`went_back_in`]); the Symphony's pages say it on the board's health
//! ([`Rollback::line_in`]). docs/TASKS.md names each. A board not made yet reads
//! as below any schema: a store that lost the whole board went back too. Otherwise the store adds
//! the version it found to the record when it is higher, and the one it migrated to ([`record`]).
//!
//! The record is a log a line of which is appended for each version the board is first seen at:
//! `VERSION SECONDS seen DATE`. Appends of one short line need no lock: two openers at once each
//! write a whole line, and the record is the highest line. A board that is older on purpose (a fresh
//! store, a restore the owner chose) is accepted by `tasks schema --accept VERSION` ([`accept`]): a
//! line `VERSION SECONDS accepted DATE`, from which the record counts again, so the record is the
//! highest line since the last acceptance.
//!
//! A line that does not read as one (cut short, edited by hand, a word it does not know) is passed
//! over: it neither raises nor lowers the record, which the lines that do read decide. It is never
//! read as a schema. The next line appended goes on a line of its own, after a newline the cut one
//! lacked, so it is never glued onto it.

use super::Live;
use crate::server;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The record's name in the board's folder.
pub const FILE: &str = "schema-seen.log";

/// Where the recovery of a board that went back is described, and how the owner accepts an older
/// board: the Symphony's guide to the board, whichever project's board it is.
pub const RECOVERY: &str = "the Symphony's docs/BOARD_OPERATIONS.md#a-board-that-went-back";

/// What a rollback's words begin with after the board's name, which [`Rollback::line_in`] finds.
const WENT_BACK: &str = " board went back from schema ";

/// A line seeing a version.
const SEEN: &str = "seen";
/// A line accepting one.
const ACCEPTED: &str = "accepted";

/// The record of the board whose repository's common dir is `common`.
pub fn path(common: &Path) -> PathBuf {
    common.join(server::FOLDER).join(FILE)
}

/// What the record says: the highest schema the board was seen at since the last acceptance, and
/// when it was first seen at it (or accepted at it).
#[derive(Clone, Debug, PartialEq)]
pub struct Seen {
    /// The schema version; 0 for a store accepted with no board in it.
    pub version: i64,
    /// When, in seconds since the epoch.
    pub at: f64,
}

/// The record's text read: its highest line since the last acceptance; none when no line reads as
/// one (a line that does not, half written or edited by hand, is passed over).
pub fn of(text: &str) -> Option<Seen> {
    let mut seen: Option<Seen> = None;
    for line in text.lines() {
        let mut words = line.split_whitespace();
        let (Some(version), Some(at), Some(word)) = (words.next(), words.next(), words.next())
        else {
            continue;
        };
        let (Ok(version), Ok(at)) = (version.parse::<i64>(), at.parse::<f64>()) else {
            continue;
        };
        if version < 0 || !at.is_finite() {
            continue;
        }
        let line = Seen { version, at };
        match word {
            ACCEPTED => seen = Some(line),
            SEEN if seen.as_ref().is_none_or(|seen| version > seen.version) => seen = Some(line),
            _ => {}
        }
    }
    seen
}

/// The record of the board whose repository's common dir is `common`; none while there is none, or
/// it cannot be read.
pub fn read(common: &Path) -> Option<Seen> {
    read_at(&path(common))
}

/// The record in the file `record`, as [`read`] reads it.
pub fn read_at(record: &Path) -> Option<Seen> {
    std::fs::read_to_string(record)
        .ok()
        .and_then(|text| of(&text))
}

/// A board's schema as the record compares it: one not made yet below every version.
fn level(found: Option<i64>) -> i64 {
    found.unwrap_or(0)
}

/// A board that went back: seen at `from`, it reads `to` now.
#[derive(Clone, Debug, PartialEq)]
pub struct Rollback {
    /// The project, as refusals name it.
    pub project: String,
    /// The highest schema it was seen at, and when it was first seen at it.
    pub from: i64,
    pub since: f64,
    /// The schema it reads now; none for a store with no board in it.
    pub to: Option<i64>,
    /// The record.
    pub record: PathBuf,
}

impl Rollback {
    /// Where it went back to, as its words say it.
    fn to_said(&self) -> String {
        match self.to {
            Some(to) => to.to_string(),
            None => "no board at all".into(),
        }
    }

    /// Its refusal, which every program that opens the board says, `tasks migrate` included.
    pub fn refusal(&self) -> String {
        let now = match self.to {
            Some(to) => format!("schema {to}"),
            None => "no board".into(),
        };
        format!(
            "the {}{WENT_BACK}{} to {}: it was seen at schema {} on {} ({}), and its store now \
             holds {now}, so what was written since is lost; this is a rollback, not a board \
             awaiting a migrate: nothing was read or written. Do not run `just tasks migrate`: it \
             would write on top of the older state. How to recover the store, and how the owner \
             accepts an older board they meant, is in {RECOVERY}",
            self.project,
            self.from,
            self.to_said(),
            self.from,
            crate::sys::local_minute(self.since),
            self.record.display(),
        )
    }

    /// What a page says of a board whose refusal (or any text holding it) is `said`: that it went
    /// back, and what not to do; none when `said` holds no rollback's words.
    pub fn line_in(said: &str) -> Option<String> {
        let at = said.find(WENT_BACK)?;
        let rest = &said[at + WENT_BACK.len()..];
        let (went, _) = rest.split_once(':')?;
        Some(format!(
            "it went back from schema {went}, so what was written since is lost: nothing was \
             written; do not run `just tasks migrate`; {RECOVERY} says how to recover it"
        ))
    }
}

/// The rollback `live` is in when its board, found at `found` (none: not made yet), reads below the
/// record ([`read`]); none while it does not, or there is no record.
pub fn went_back(live: &Live, found: Option<i64>) -> Option<Rollback> {
    went_back_in(&path(&live.common), found, || live.project.clone())
}

/// As [`went_back`], for the board whose record is the file `record`, named by `project` only when it
/// went back: a store's own folder's (`tasks-reclaim --dir`'s copy) as much as a live board's.
pub fn went_back_in(
    record: &Path,
    found: Option<i64>,
    project: impl FnOnce() -> String,
) -> Option<Rollback> {
    let seen = read_at(record)?;
    (level(found) < seen.version).then(|| Rollback {
        project: project(),
        from: seen.version,
        since: seen.at,
        to: found,
        record: record.to_path_buf(),
    })
}

/// The rollback of `live`'s board, its schema read by `found` once there is a record: what an opener
/// that reads the schema for nothing else (`tasks migrate` before it asks where it runs, the
/// recorder, the detached dispatcher) refuses itself. No record, no read.
pub fn checked(
    live: &Live,
    found: impl FnOnce() -> Result<Option<i64>, String>,
) -> Result<Option<Rollback>, String> {
    match read(&live.common) {
        Some(_) => Ok(went_back(live, found()?)),
        None => Ok(None),
    }
}

/// As [`checked`], for a store writing at schema `own` that takes the board's tables as they are on
/// a path that must stay one request (the heartbeat's): its schema read only when the record is above
/// `own`. A record at or below `own` needs no read: a board below it is below `own` too, and the
/// schema guard refuses every write such a store makes to a board at any schema but its own.
pub fn past_guard(
    live: &Live,
    own: i64,
    found: impl FnOnce() -> Result<Option<i64>, String>,
) -> Result<Option<Rollback>, String> {
    match read(&live.common) {
        Some(seen) if seen.version > own => Ok(went_back(live, found()?)),
        _ => Ok(None),
    }
}

/// Add `version` to `live`'s record, seen at `now`, when it is higher than the record says or there
/// is none. What it cannot write it leaves: a record that cannot be written guards nothing, and
/// that must not stop the board.
pub fn record(live: &Live, version: i64, now: f64) {
    if read(&live.common).is_some_and(|seen| seen.version >= version) {
        return;
    }
    let _ = append(&live.common, version, now, SEEN);
}

/// What accepting `version` as `live`'s board, found at `found` (none: not made yet), did.
#[derive(Clone, Debug, PartialEq)]
pub enum Accepted {
    /// The board went back from `from`; the record counts from `version` now.
    Accepted { from: i64 },
    /// The board does not read below the record: nothing to accept.
    Nothing,
}

/// Accept `version` (0: a store with no board) as the schema of `live`'s board, found at `found`,
/// at `now`: refused unless the board reads exactly that, so what is accepted is what was read.
pub fn accept(live: &Live, version: i64, found: Option<i64>, now: f64) -> Result<Accepted, String> {
    if level(found) != version {
        return Err(format!(
            "the {} board holds {}, not {}: nothing was accepted",
            live.project,
            found.map_or_else(|| "no board".into(), |found| format!("schema {found}")),
            if version == 0 {
                "no board".into()
            } else {
                format!("schema {version}")
            }
        ));
    }
    let Some(rollback) = went_back(live, found) else {
        return Ok(Accepted::Nothing);
    };
    append(&live.common, version, now, ACCEPTED)
        .map_err(|why| format!("could not write {}: {why}", path(&live.common).display()))?;
    Ok(Accepted::Accepted {
        from: rollback.from,
    })
}

/// Append one line of `word` (seen, accepted) for `version` at `now` to the record in `common`,
/// in one write: a line two openers write at once is each whole. A record whose last line has no
/// newline (cut short, or edited by hand) gets one first, in the same write, so the new line is a
/// line of its own and never the tail of that one: "1" and "17 … seen" glued would read as 117.
fn append(common: &Path, version: i64, now: f64, word: &str) -> std::io::Result<()> {
    let file = path(common);
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder)?;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a time in seconds is far inside i64"
    )]
    let seconds = now.floor() as i64;
    let line = format!(
        "{version} {seconds} {word} {}\n",
        crate::sys::local_minute(now)
    );
    let mut record = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(&file)?;
    let text = if ends_open(&mut record)? {
        format!("\n{line}")
    } else {
        line
    };
    record.write_all(text.as_bytes())
}

/// Whether `record` ends in a line with no newline: false for an empty one.
fn ends_open(record: &mut std::fs::File) -> std::io::Result<bool> {
    use std::io::{Read, Seek, SeekFrom};
    if record.metadata()?.len() == 0 {
        return Ok(false);
    }
    record.seek(SeekFrom::End(-1))?;
    let mut last = [0u8; 1];
    record.read_exact(&mut last)?;
    Ok(last[0] != b'\n')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A common dir of the test's own, removed when it ends.
    struct Common(PathBuf);

    impl Common {
        fn new(name: &str) -> Common {
            let dir = std::env::temp_dir().join(format!(
                "seen-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&dir).unwrap();
            Common(dir)
        }

        fn live(&self) -> Live {
            Live {
                project: "alpha".into(),
                common: self.0.clone(),
            }
        }
    }

    impl Drop for Common {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const T0: f64 = 1_800_000_000.0;

    #[test]
    fn the_record_is_the_highest_line_since_the_last_acceptance_and_passes_over_what_reads_as_none()
    {
        assert_eq!(of(""), None);
        assert_eq!(
            of("13 100 seen 1970-01-01 01:01\n16 200 seen x\n14 300 seen\n"),
            Some(Seen {
                version: 16,
                at: 200.0
            }),
            "the highest, whatever the order"
        );
        assert_eq!(
            of("16 200 seen\n13 300 accepted\n"),
            Some(Seen {
                version: 13,
                at: 300.0
            }),
            "an acceptance lowers it"
        );
        assert_eq!(
            of("16 200 seen\n13 300 accepted\n15 400 seen\n"),
            Some(Seen {
                version: 15,
                at: 400.0
            }),
            "and it counts again from there"
        );
        assert_eq!(
            of("16 200 seen\n0 300 accepted\n"),
            Some(Seen {
                version: 0,
                at: 300.0
            }),
            "a store with no board accepted"
        );
        // Half a line, a hand's edit, a word it does not know, a time that is none: passed over.
        assert_eq!(
            of("16 200 seen\n17\n18 x seen\n-1 5 accepted\n19 inf seen\n20 5 kept\n1"),
            Some(Seen {
                version: 16,
                at: 200.0
            })
        );
    }

    #[test]
    fn a_board_below_its_record_went_back_and_one_at_or_above_it_did_not() {
        let common = Common::new("went-back");
        let live = common.live();
        assert_eq!(
            went_back(&live, Some(1)),
            None,
            "no record: nothing to compare"
        );
        assert_eq!(went_back(&live, None), None);

        record(&live, 16, T0);
        assert_eq!(went_back(&live, Some(16)), None);
        assert_eq!(
            went_back(&live, Some(17)),
            None,
            "ahead of it is no rollback"
        );
        assert_eq!(
            went_back(&live, Some(13)),
            Some(Rollback {
                project: "alpha".into(),
                from: 16,
                since: T0,
                to: Some(13),
                record: path(&common.0),
            })
        );
        assert_eq!(
            went_back(&live, None).map(|back| back.to),
            Some(None),
            "a store that lost the whole board went back too"
        );
    }

    #[test]
    fn an_opener_reads_the_schema_only_when_the_record_could_refuse_it() {
        let common = Common::new("checked");
        let live = common.live();
        let unread = || -> Result<Option<i64>, String> { panic!("read with nothing to compare") };
        assert_eq!(checked(&live, unread), Ok(None), "no record, no read");
        assert_eq!(past_guard(&live, 16, unread), Ok(None));

        record(&live, 16, T0);
        assert_eq!(checked(&live, || Ok(Some(16))), Ok(None));
        assert_eq!(
            checked(&live, || Ok(Some(13))).map(|back| back.map(|back| (back.from, back.to))),
            Ok(Some((16, Some(13))))
        );
        assert_eq!(
            checked(&live, || Err("no answer".into())),
            Err("no answer".into()),
            "a read that fails is the opener's failure"
        );
        // A record at or below the opener's own schema: the guard refuses its writes to any board
        // below it, so the heartbeat's one request stays one.
        assert_eq!(past_guard(&live, 16, unread), Ok(None));
        assert_eq!(past_guard(&live, 17, unread), Ok(None));
        // Above it, a board at the opener's own schema would pass the guard: read, and refused.
        assert_eq!(
            past_guard(&live, 15, || Ok(Some(15)))
                .map(|back| back.map(|back| (back.from, back.to))),
            Ok(Some((16, Some(15))))
        );
        assert_eq!(past_guard(&live, 15, || Ok(Some(16))), Ok(None));
        assert_eq!(
            past_guard(&live, 15, || Err("no answer".into())),
            Err("no answer".into())
        );
    }

    #[test]
    fn a_record_kept_beside_a_store_of_its_own_is_read_where_it_is_and_named_only_when_it_refuses()
    {
        let common = Common::new("in");
        let record_file = common.0.join("copy").join(FILE);
        let unnamed = || -> String { panic!("named with nothing to refuse") };
        assert_eq!(went_back_in(&record_file, Some(1), unnamed), None);
        fs::create_dir_all(record_file.parent().unwrap()).unwrap();
        fs::write(&record_file, "16 200 seen x\n").unwrap();
        assert_eq!(went_back_in(&record_file, Some(16), unnamed), None);
        assert_eq!(
            went_back_in(&record_file, Some(13), || "beta".into()),
            Some(Rollback {
                project: "beta".into(),
                from: 16,
                since: 200.0,
                to: Some(13),
                record: record_file.clone(),
            })
        );
    }

    #[test]
    fn a_version_is_recorded_once_when_it_is_higher_and_never_lowers_the_record() {
        let common = Common::new("record");
        let live = common.live();
        record(&live, 15, T0);
        record(&live, 15, T0 + 1.0);
        record(&live, 14, T0 + 2.0);
        record(&live, 16, T0 + 3.0);
        let text = fs::read_to_string(path(&common.0)).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert!(
            lines[0].starts_with("15 1800000000 seen ")
                && lines[1].starts_with("16 1800000003 seen "),
            "{text}"
        );
        assert_eq!(read(&common.0).map(|seen| seen.version), Some(16));
    }

    #[test]
    fn a_line_appended_to_a_record_cut_short_is_a_line_of_its_own_and_the_cut_one_counts_for_nothing()
     {
        let common = Common::new("torn");
        let live = common.live();
        fs::create_dir_all(path(&common.0).parent().unwrap()).unwrap();

        // A record that is only half a line: it reads as none, so it guards nothing yet.
        fs::write(path(&common.0), "1").unwrap();
        assert_eq!(read(&common.0), None);
        assert_eq!(went_back(&live, Some(17)), None);
        record(&live, 17, T0);
        let text = fs::read_to_string(path(&common.0)).unwrap();
        assert!(text.starts_with("1\n17 1800000000 seen "), "{text:?}");
        assert!(text.ends_with('\n'), "{text:?}");
        assert_eq!(
            read(&common.0),
            Some(Seen {
                version: 17,
                at: T0
            }),
            "17, never 117"
        );
        assert_eq!(went_back(&live, Some(17)), None, "a board at 17 opens");

        // A whole record whose last line lost its newline, and an acceptance after a cut.
        fs::write(path(&common.0), "16 200 seen x\n18 300 se").unwrap();
        assert_eq!(read(&common.0).map(|seen| seen.version), Some(16));
        record(&live, 17, T0 + 1.0);
        assert_eq!(read(&common.0).map(|seen| seen.version), Some(17));
        fs::write(path(&common.0), "17 200 seen x\n1").unwrap();
        assert_eq!(
            accept(&live, 13, Some(13), T0 + 2.0),
            Ok(Accepted::Accepted { from: 17 })
        );
        let text = fs::read_to_string(path(&common.0)).unwrap();
        assert!(
            text.starts_with("17 200 seen x\n1\n13 1800000002 accepted "),
            "{text:?}"
        );
        assert_eq!(read(&common.0).map(|seen| seen.version), Some(13));

        // A record that ends in its newline gets no empty line.
        record(&live, 14, T0 + 3.0);
        let text = fs::read_to_string(path(&common.0)).unwrap();
        assert!(!text.contains("\n\n"), "{text:?}");
        assert_eq!(text.lines().count(), 4, "{text:?}");
    }

    #[test]
    fn only_the_schema_the_board_holds_is_accepted_and_the_record_counts_from_it() {
        let common = Common::new("accept");
        let live = common.live();
        assert_eq!(
            accept(&live, 13, Some(13), T0),
            Ok(Accepted::Nothing),
            "no record: nothing went back"
        );
        record(&live, 16, T0);
        assert_eq!(accept(&live, 16, Some(16), T0), Ok(Accepted::Nothing));
        assert_eq!(
            accept(&live, 12, Some(13), T0),
            Err("the alpha board holds schema 13, not schema 12: nothing was accepted".into())
        );
        assert_eq!(
            accept(&live, 13, None, T0),
            Err("the alpha board holds no board, not schema 13: nothing was accepted".into())
        );
        assert!(
            went_back(&live, Some(13)).is_some(),
            "refused: still went back"
        );

        assert_eq!(
            accept(&live, 13, Some(13), T0 + 5.0),
            Ok(Accepted::Accepted { from: 16 })
        );
        assert_eq!(went_back(&live, Some(13)), None);
        assert!(
            went_back(&live, Some(12)).is_some(),
            "below the accepted one went back"
        );
        record(&live, 16, T0 + 6.0);
        assert_eq!(read(&common.0).map(|seen| seen.version), Some(16));

        // A store that lost the board, accepted as such, by 0.
        assert_eq!(
            accept(&live, 0, None, T0 + 7.0),
            Ok(Accepted::Accepted { from: 16 })
        );
        assert_eq!(went_back(&live, None), None);
    }

    #[test]
    fn the_refusal_says_from_and_to_that_nothing_was_written_not_to_migrate_and_where_to_recover() {
        let back = Rollback {
            project: "alpha".into(),
            from: 16,
            since: T0,
            to: Some(13),
            record: PathBuf::from("/repo/.git/symphony/tasks/schema-seen.log"),
        };
        let refusal = back.refusal();
        assert_eq!(
            refusal,
            format!(
                "the alpha board went back from schema 16 to 13: it was seen at schema 16 on {} \
                 (/repo/.git/symphony/tasks/schema-seen.log), and its store now holds schema 13, \
                 so what was written since is lost; this is a rollback, not a board awaiting a \
                 migrate: nothing was read or written. Do not run `just tasks migrate`: it would \
                 write on top of the older state. How to recover the store, and how the owner \
                 accepts an older board they meant, is in the Symphony's \
                 docs/BOARD_OPERATIONS.md#a-board-that-went-back",
                crate::sys::local_minute(T0)
            )
        );
        let line = "it went back from schema 16 to 13, so what was written since is lost: nothing \
                    was written; do not run `just tasks migrate`; the Symphony's \
                    docs/BOARD_OPERATIONS.md#a-board-that-went-back says how to recover it";
        assert_eq!(Rollback::line_in(&refusal).as_deref(), Some(line));
        // However it is wrapped on the way to the page.
        assert_eq!(
            Rollback::line_in(&format!(
                "no pass, the board could not be reached: {refusal}"
            ))
            .as_deref(),
            Some(line)
        );
        let lost = Rollback { to: None, ..back };
        assert!(
            lost.refusal()
                .starts_with("the alpha board went back from schema 16 to no board at all: "),
            "{}",
            lost.refusal()
        );
        assert!(lost.refusal().contains("its store now holds no board, so"));
        assert_eq!(
            Rollback::line_in(&lost.refusal()).as_deref(),
            Some(
                "it went back from schema 16 to no board at all, so what was written since is \
                 lost: nothing was written; do not run `just tasks migrate`; the Symphony's \
                 docs/BOARD_OPERATIONS.md#a-board-that-went-back says how to recover it"
            )
        );
        for other in [
            "the alpha board is at schema 13, behind this build's 16",
            "connection refused",
            "the alpha board went back from schema 16 to 13",
        ] {
            assert_eq!(Rollback::line_in(other), None, "{other}");
        }
    }
}
