//! The unanswered-post lookup a PULL digest runs, on the generated board: it reads the threads of
//! the open tasks it asks about and the launches on them, never every thread of the board nor a
//! recipient's answers after each pending post across the board, each read on an index
//! ([`crate::findings`]), within a bound of the server's CPU. The reader sets no deadline on the
//! lookup ([`symphony_board::Store::looking_up_within`]), so its client's own wait is the backstop
//! and the posts it finds never turn on how loaded the machine is.

use crate::board::{DATABASE, board};
use crate::generated::NOW;
use crate::plans::{planned, wrong};
use crate::proxy::{Proxy, Request};
use std::time::Duration;
use symphony_board::{Opening, Store, sys};

/// The statuses the lookup reads the threads of: an executor's post is overdue only in these.
const OPEN: [&str; 3] = ["in_progress", "blocked", "review"];

/// The most server CPU the lookup's reads may take, replayed one after another: it holds them to
/// the open tasks' threads, since every thread of this board and each pending post's answers after
/// it take several seconds of the server's CPU. Measured on SurrealDB on 2026-11-13, its two reads
/// (the open threads, and one for the posts still asked after it) took 0.14 s at load 14 to 15 on
/// 16 cores and 0.18 s at load 40 to 127. The bound is about ten times that.
const LOOKUP: Duration = Duration::from_millis(1_500);

/// The lookup's read of the open tasks' threads, each by its slug (`store/postgres/unanswered.rs`'s
/// `threads`), and the slugs it is asked of, its variables' `slugs`.
const THREADS: &str = "jsonb_build_object('threads'";

/// Its read of the answers below a task, for the posts still asked after the threads (`below`).
const BELOW: &str = "a.ask -> 'below'";

/// Its read of a reviewer's launches elsewhere (`reviewer_launches`).
const REVIEWERS: &str =
    "task_session l WHERE handle IN (SELECT jsonb_array_elements_text(v -> 'handles'))";

/// A recipient's answers after one post across every thread (`answers`): what a recovery reads
/// again before it resumes a conversation, never the lookup.
const SUFFIX: &str = "m.sender = v ->> 'handle' AND m.sid > (v ->> 'post')::bigint";

/// Whether `request` is one of the lookup's reads: its threads and launches, the answers below
/// them, or a reviewer's launches elsewhere.
fn lookup(request: &Request) -> bool {
    request.holds(THREADS) || request.holds(BELOW) || request.holds(REVIEWERS)
}

#[test]
fn the_digests_unanswered_lookup_reads_the_open_threads_alone_on_a_board_at_scale() {
    let board = board();
    let relay = Proxy::to(&board.addr);
    let reader = Store::opening_on(
        relay.client(DATABASE).into(),
        Box::new(|| NOW),
        Box::new(|_| false),
        &sys::hostname(),
        &Opening::write(),
    )
    .unwrap();
    let open = (reader.board().unwrap().tasks.iter())
        .filter(|t| OPEN.contains(&t.status.as_str()))
        .count();
    relay.take();
    // The digest planned as it runs, on a connection of its own that starts while the plans are
    // logged.
    let fresh = Store::unchecked_on(
        relay.client(DATABASE).into(),
        Box::new(|| NOW),
        Box::new(|_| false),
        &sys::hostname(),
        &Opening::write(),
    );
    let (digest, logged) = board.explained(&relay, || {
        symphony_board::cli::pull::digest(&fresh, &[], None, NOW, &[]).unwrap()
    });
    let requests = relay.take();
    assert!(
        digest.count("unanswered") > 0,
        "the board has overdue posts: {:?}",
        digest.lines()
    );
    let reads: Vec<&Request> = requests.iter().filter(|r| lookup(r)).collect();
    let threads: Vec<&Request> = (reads.iter().copied())
        .filter(|r| r.holds(THREADS))
        .collect();
    assert_eq!(threads.len(), 1, "one read of the threads: {reads:#?}");
    let asked = threads[0].vars["slugs"].as_array().map_or(0, Vec::len);
    assert_eq!(asked, open, "the open tasks' threads alone, of the board's");
    assert!(
        !requests.iter().any(|r| r.holds(SUFFIX)),
        "no recipient's answers after each post, across the board"
    );
    let planned = planned(&logged);
    let lookup_plans: Vec<&String> = (planned.keys())
        .filter(|statement| statement.contains(THREADS))
        .collect();
    assert!(!lookup_plans.is_empty(), "the threads' read is planned");
    let wrong = wrong(&planned, &requests, &logged);
    assert!(wrong.is_empty(), "{}", wrong.join("\n\n"));
    let client = board.client();
    let ((), cpu) = board.bill(|| {
        for read in &reads {
            read.again(&client).unwrap();
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
