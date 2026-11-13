//! The unanswered-post lookup a PULL digest runs, on the generated board: it reads the threads of
//! the open tasks it asks about and the launches on them, by record range, never every thread of
//! the board nor each pending post's recipient suffix, within a bound of the server's CPU. The
//! reader sets no deadline on the lookup ([`Store::looking_up_within`]), so its client's own wait
//! is the backstop and the posts it finds never turn on how loaded the machine is.

use crate::board::{DATABASE, board};
use crate::generated::NOW;
use crate::proxy::{Proxy, Request};
use std::time::Duration;
use symphony_board::rpc::Client;
use symphony_board::{NAMESPACE, Store, sys};

/// The statuses the lookup reads the threads of: an executor's post is overdue only in these.
const OPEN: [&str; 3] = ["in_progress", "blocked", "review"];

/// The most server CPU the lookup's reads may take, replayed one after another: it holds them to
/// the open tasks' threads, since every thread of this board and each pending post's answers after
/// it take several seconds of the server's CPU. Measured on 2026-11-13, its two reads (the open
/// threads, and one for the posts still asked after it) took 0.14 s at load 14 to 15 on 16 cores
/// and 0.18 s at load 40 to 127. The bound is about ten times that.
const LOOKUP: Duration = Duration::from_millis(1_500);

/// Whether `request` is one of the lookup's reads: its threads and launches, the answers below
/// them, or a reviewer's launches elsewhere.
fn lookup(request: &Request) -> bool {
    request.sql.contains("threads: $slugs.map(")
        || request.sql.contains("$asks.map(")
        || request
            .sql
            .contains("FROM task_session\nWHERE handle IN $handles")
}

#[test]
fn the_digests_unanswered_lookup_reads_the_open_threads_alone_on_a_board_at_scale() {
    let board = board();
    let relay = Proxy::to(&board.addr);
    let reader = Store::with(
        Client::new(&relay.addr, NAMESPACE, DATABASE),
        Box::new(|| NOW),
        Box::new(|_| false),
        &sys::hostname(),
    )
    .unwrap();
    let open = (reader.board().unwrap().tasks.iter())
        .filter(|t| OPEN.contains(&t.status.as_str()))
        .count();
    relay.take();
    let digest = symphony_board::cli::pull::digest(&reader, &[], None, NOW, &[]).unwrap();
    let requests = relay.take();
    assert!(
        digest.count("unanswered") > 0,
        "the board has overdue posts: {:?}",
        digest.lines()
    );
    let reads: Vec<&Request> = requests.iter().filter(|r| lookup(r)).collect();
    let threads: Vec<&Request> = (reads.iter().copied())
        .filter(|r| r.sql.contains("threads: $slugs.map("))
        .collect();
    assert_eq!(threads.len(), 1, "one read of the threads: {reads:#?}");
    let asked = threads[0].vars["slugs"].as_array().map_or(0, Vec::len);
    assert_eq!(asked, open, "the open tasks' threads alone, of the board's");
    assert!(
        !(requests.iter())
            .any(|r| r.sql.contains("WITH INDEX message_sender") && r.sql.contains("sid > $post")),
        "no recipient's suffix of records after each post"
    );
    let ((), cpu) = board.bill(|| {
        for read in &reads {
            board.client().query(&read.sql, read.vars.clone()).unwrap();
        }
    });
    eprintln!(
        "perf: unanswered lookup: {} reads, {asked} threads of {open} open tasks, server CPU {cpu:.3?}, bound {LOOKUP:.1?}; {} unanswered",
        reads.len(),
        digest.count("unanswered")
    );
    assert!(
        cpu <= LOOKUP,
        "the lookup's reads took {cpu:.3?} of the server's CPU, over {LOOKUP:.1?}"
    );
}
