//! The hourly roll-up's one read of the board ([`Store::report_records`]) on the generated board,
//! as the shared server makes it once an hour for each project: from yesterday's start, as the
//! first roll-up after midnight reads, in one request and no write, within a bound of the CPU it
//! costs the board's server ([`crate::cpu`]), from quiet to quiet again. The figures of the two day
//! files made from it are bounded too, by the CPU this thread took for them, and the read of every
//! task and every line the board holds (from the epoch) is measured beside, as the most a read
//! could ever take. The bounds are stated once, at the end; the measurements are printed for
//! reading.

use crate::board::board;
use crate::generated::NOW;
use std::time::{Duration, Instant};
use symphony_board::reports::{DAY, day, day_start};
use symphony_board::{ReportRead, Store};

/// The read, its wall and the board server's CPU for it, and what it held.
fn read(
    board: &crate::board::Board,
    store: &Store,
    since: f64,
) -> (ReportRead, Duration, Duration) {
    let ((read, wall), cpu) = board.bill(|| {
        let began = Instant::now();
        let read = store.report_records(since).unwrap();
        (read, began.elapsed())
    });
    (read, wall, cpu)
}

fn lines(read: &ReportRead) -> usize {
    read.tasks.iter().map(|task| task.lines.len()).sum()
}

#[test]
fn the_roll_up_reads_the_board_once_an_hour_within_a_bound() {
    let board = board();
    let store = board.store("perf-reports");
    let today = day_start(NOW);
    let yesterday = today - DAY;
    // The store's own requests and writes, as its client counts them.
    let log = store.database().queries();
    let (requests, writes) = log.report(symphony_board::sweep::trigger::now());
    let (hourly, wall, cpu) = read(&board, &store, yesterday);
    let (after, writes_after) = log.report(symphony_board::sweep::trigger::now());
    assert_eq!(after.count - requests.count, 1, "one request");
    assert_eq!(writes_after.count, writes.count, "no write");
    let (files, figures) = test_time::cpu::spent(|| {
        [
            day("perf", yesterday, today, NOW, &hourly),
            day("perf", today, NOW, NOW, &hourly),
        ]
    });
    let bytes: usize = files
        .iter()
        .map(|file| serde_json::to_vec(file).unwrap().len() + 1)
        .sum();
    eprintln!(
        "perf: the roll-up's read: {} tasks, {} lines in {wall:.1?}, {cpu:.1?} of the board \
         server's CPU; both day files' figures in {figures:.1?} of CPU, {bytes} bytes written",
        hourly.tasks.len(),
        lines(&hourly)
    );
    let (whole, whole_wall, whole_cpu) = read(&board, &store, 0.0);
    eprintln!(
        "perf: every task and line the board holds: {} tasks, {} lines in {whole_wall:.1?}, \
         {whole_cpu:.1?} of the board server's CPU",
        whole.tasks.len(),
        lines(&whole)
    );
    assert!(
        cpu <= HOURLY,
        "the roll-up's read took {cpu:?} of the board server's CPU, past {HOURLY:?}"
    );
    assert!(
        figures <= FIGURES,
        "the day files' figures took {figures:?}, past {FIGURES:?}"
    );
}

/// The roll-up's read, once an hour: each open task's thread and each one written or with a line
/// since yesterday, 230 tasks and 9.7k lines of this board, measured at 0.17 s of the server's CPU
/// (0.49 s wall) at load 60 on 2026-11-13, on SurrealDB, where the whole board's 2.2k tasks and 50k lines took
/// 1.7 s. Five times the hourly read is still a second an hour, beside the board's tick every
/// second.
const HOURLY: Duration = Duration::from_secs(1);
/// Both day files' figures from it, the CPU this thread took for them, measured at 83 ms of wall in
/// a debug build on SurrealDB, where they were timed by the wall.
const FIGURES: Duration = Duration::from_secs(1);
