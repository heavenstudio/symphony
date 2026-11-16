//! The runs kept between reads (`recorded::Kept`): what `recorded::runs` gives of the same folder
//! at every step, and a file read again only once it is another file.

use super::*;
use crate::cli::machine_work::recorded::{Kept, Reads};
use std::sync::Arc;

/// What `kept` gives of `dir`, which is what a read of everything gives, and what it read so far.
fn read(kept: &mut Kept, dir: &Path) -> (Arc<Vec<Record>>, Reads) {
    let runs = kept.runs(dir);
    assert_eq!(*runs, recorded::runs(dir), "as a read of everything");
    (runs, kept.reads())
}

fn reads(histories: usize, records: usize) -> Reads {
    Reads { histories, records }
}

fn ids_of(runs: &[Record]) -> Vec<(&str, Step)> {
    runs.iter()
        .map(|run| (run.operation_id.as_str(), run.state))
        .collect()
}

#[test]
fn a_read_with_nothing_new_reads_no_file_and_gives_the_list_it_gave() {
    let dir = folder("kept-nothing-new");
    recorded::keep(&dir, &record(ONE, Step::Finished)).unwrap();
    let _held = running(&dir, &record(TWO, Step::Running));
    recorded::write(&path(&dir, THREE), &record(THREE, Step::Finished)).unwrap();
    let mut kept = Kept::default();
    let (first, counted) = read(&mut kept, &dir);
    assert_eq!(
        ids_of(&first),
        [
            (ONE, Step::Finished),
            (TWO, Step::Running),
            (THREE, Step::Finished)
        ]
    );
    assert_eq!(counted, reads(1, 2), "the history once, each record once");
    for _ in 0..3 {
        let (again, counted) = read(&mut kept, &dir);
        assert!(Arc::ptr_eq(&first, &again), "the very list it gave");
        assert_eq!(counted, reads(1, 2), "nothing read again");
    }
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_beat_reads_its_record_alone_and_a_finished_run_the_history_too() {
    let dir = folder("kept-beat-finish");
    recorded::keep(&dir, &record(ONE, Step::Finished)).unwrap();
    let going = record(TWO, Step::Running);
    let held = running(&dir, &going);
    let _other = running(&dir, &record(THREE, Step::Running));
    let mut kept = Kept::default();
    assert_eq!(read(&mut kept, &dir).1, reads(1, 2));

    // A beat: the record is a new file renamed over the one before.
    let mut beaten = going.clone();
    beaten.beat_at += 2.0;
    beaten.reading = Some(reading());
    recorded::write(&path(&dir, TWO), &beaten).unwrap();
    let (runs, counted) = read(&mut kept, &dir);
    assert_eq!(runs[1], beaten);
    assert_eq!(counted, reads(1, 3), "its record alone, no history");

    // It finishes as a recorder finishes it: kept in the history, then its record, then its lease.
    let mut done = beaten.clone();
    done.state = Step::Finished;
    done.ended_at = Some(1040.0);
    done.exit_code = Some(1);
    recorded::keep(&dir, &done).unwrap();
    recorded::write(&path(&dir, TWO), &done).unwrap();
    gone(held);
    let (runs, counted) = read(&mut kept, &dir);
    assert_eq!(
        ids_of(&runs),
        [
            (ONE, Step::Finished),
            (TWO, Step::Finished),
            (THREE, Step::Running)
        ],
        "in the history's place now"
    );
    assert_eq!(counted, reads(2, 4), "the history and that record");
    assert_eq!(read(&mut kept, &dir).1, reads(2, 4));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_recorder_that_ended_takes_its_run_away_with_no_file_changed() {
    let dir = folder("kept-recorder-gone");
    let _held = running(&dir, &record(ONE, Step::Running));
    let lost = running(&dir, &record(TWO, Step::Running));
    let mut kept = Kept::default();
    let (runs, _) = read(&mut kept, &dir);
    assert_eq!(ids_of(&runs), [(ONE, Step::Running), (TWO, Step::Running)]);
    gone(lost);
    let (runs, _) = read(&mut kept, &dir);
    assert_eq!(ids_of(&runs), [(ONE, Step::Running)]);
    // Still gone, and the one that runs still there, read after read.
    let (runs, _) = read(&mut kept, &dir);
    assert_eq!(ids_of(&runs), [(ONE, Step::Running)]);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_record_over_the_historys_gives_way_to_it_once_its_file_goes() {
    let dir = folder("kept-over-history");
    let kept_first = record(ONE, Step::Finished);
    recorded::keep(&dir, &kept_first).unwrap();
    recorded::keep(&dir, &record(TWO, Step::Finished)).unwrap();
    let mut said_more = kept_first.clone();
    said_more.reason = "read from its record".into();
    recorded::write(&path(&dir, ONE), &said_more).unwrap();
    let mut kept = Kept::default();
    let (runs, _) = read(&mut kept, &dir);
    assert_eq!(runs[0], said_more, "the record says more than the history");
    // A sweep takes a record an hour done: the history's stands in its place again.
    fs::remove_file(path(&dir, ONE)).unwrap();
    let (runs, counted) = read(&mut kept, &dir);
    assert_eq!(*runs, [kept_first, record(TWO, Step::Finished)]);
    assert_eq!(counted, reads(1, 1), "from what was kept");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_rotated_history_is_read_again_and_a_list_a_reader_holds_is_left_as_it_was() {
    let dir = folder("kept-rotated");
    recorded::keep(&dir, &record(ONE, Step::Finished)).unwrap();
    let _held = running(&dir, &record(TWO, Step::Running));
    let mut kept = Kept::default();
    let (held_by_reader, _) = read(&mut kept, &dir);
    let before = held_by_reader.to_vec();
    fs::rename(dir.join(HISTORY), dir.join(ROTATED)).unwrap();
    recorded::keep(&dir, &record(THREE, Step::Finished)).unwrap();
    let (runs, counted) = read(&mut kept, &dir);
    assert_eq!(
        ids_of(&runs),
        [
            (ONE, Step::Finished),
            (THREE, Step::Finished),
            (TWO, Step::Running)
        ]
    );
    assert_eq!(counted, reads(2, 1));
    assert_eq!(*held_by_reader, before, "a list given is never changed");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_file_that_is_no_record_is_read_once_and_a_folder_not_made_holds_none() {
    let dir = folder("kept-no-record");
    let mut kept = Kept::default();
    let (none, counted) = read(&mut kept, &dir.join("never-made"));
    assert!(none.is_empty());
    assert_eq!(counted, reads(1, 0));

    let mut kept = Kept::default();
    fs::write(path(&dir, ONE), "not a record\n").unwrap();
    recorded::write(&path(&dir, TWO), &record(TWO, Step::Finished)).unwrap();
    let (runs, counted) = read(&mut kept, &dir);
    assert_eq!(ids_of(&runs), [(TWO, Step::Finished)]);
    assert_eq!(counted, reads(1, 2));
    assert_eq!(read(&mut kept, &dir).1, reads(1, 2), "neither read again");
    // Written whole, it is one.
    recorded::write(&path(&dir, ONE), &record(ONE, Step::Finished)).unwrap();
    let (runs, counted) = read(&mut kept, &dir);
    assert_eq!(
        ids_of(&runs),
        [(ONE, Step::Finished), (TWO, Step::Finished)]
    );
    assert_eq!(counted, reads(1, 3));
    fs::remove_dir_all(&dir).unwrap();
}
