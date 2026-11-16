//! The session rows the board's commands read, on the generated board: a read that shows tasks
//! returns the sessions holding a role on them (and `next` the session asking), never every
//! session the board has seen, a list that grows with the board's history. `who`, a whole board
//! read and the read a write decides on keep the whole list. Rows, never a time: each command
//! runs as the `tasks` binary through the recording relay ([`crate::proxy`]), and each read it
//! sent is sent again to count the sessions it answers with.

use crate::board::{Board, board};
use crate::generated;
use crate::proxy::{Proxy, Request};
use std::collections::BTreeSet;
use symphony_board::rpc::{Client, Kind};
use symphony_board::{Store, Task};

/// The whole session table, as a read of every session names it.
const TABLE: &str = "FROM session ORDER BY";

/// What a command's requests read of the sessions.
#[derive(Debug, PartialEq, Eq)]
struct Read {
    /// The session rows its reads answered with, in all.
    rows: usize,
    /// How many of its writes read the whole session table: a write is never sent again, so its
    /// rows are counted by what it asks for.
    whole_writes: usize,
}

fn read(client: &Client, requests: &[Request]) -> Read {
    let mut read = Read {
        rows: 0,
        whole_writes: 0,
    };
    for request in requests {
        if Kind::of(&request.sql) == Kind::Write {
            read.whole_writes += usize::from(request.sql.contains(TABLE));
            continue;
        }
        let answer = (client.query(&request.sql, request.vars.clone()))
            .unwrap_or_else(|why| panic!("a read sent again: {why}: {}", request.sql));
        read.rows += (answer.iter())
            .filter_map(|v| v.get("sessions")?.as_array())
            .map(Vec::len)
            .sum::<usize>();
    }
    read
}

/// `tasks ARGS` through a relay, and what it read of the sessions.
fn ran(board: &Board, args: &[&str]) -> Read {
    let relay = Proxy::to(&board.addr);
    let ran = board.run(&relay.addr, args, "");
    assert_eq!(ran.code, 0, "tasks {args:?}: {}", ran.err);
    read(&board.client(), &relay.take())
}

/// The sessions holding a role on `tasks`, of those the board knows (`known`), and `more`.
fn holders(tasks: &[&Task], known: &BTreeSet<&str>, more: &[&str]) -> usize {
    let mut shown: BTreeSet<&str> = (tasks.iter())
        .flat_map(|t| t.roles.iter().map(|r| r.handle.as_str()))
        .filter(|handle| known.contains(handle))
        .collect();
    shown.extend(more);
    shown.len()
}

#[test]
fn a_read_that_shows_tasks_returns_the_sessions_on_them_and_a_whole_read_every_session() {
    let mut board = board();
    let writer = generated::writer();
    let thread = generated::thread();
    let free = board.unclaimed();
    let whole = Store::with(
        board.client(),
        Box::new(|| generated::NOW),
        Box::new(|_| false),
        &symphony_board::sys::hostname(),
    )
    .unwrap()
    .board()
    .unwrap();
    let every = whole.sessions.len();
    let known: BTreeSet<&str> = (whole.sessions.iter())
        .map(|s| s.row.handle.as_str())
        .collect();
    let all: Vec<&Task> = whole.tasks.iter().collect();
    let of = |pick: &dyn Fn(&Task) -> bool| -> Vec<&Task> {
        all.iter().copied().filter(|t| pick(t)).collect()
    };
    let shown = holders(&of(&|t| t.slug == thread), &known, &[]);
    // What an open task waits on is read over the tasks it depends on too, in one read of its own.
    let shown_task = all
        .iter()
        .find(|t| t.slug == thread)
        .expect("the thread's task");
    let waited = if symphony_board::health::FINISHED.contains(&shown_task.status.as_str()) {
        0
    } else {
        holders(&of(&|t| shown_task.depends.contains(&t.slug)), &known, &[])
    };
    let listed = holders(&all, &known, &[]);
    let in_progress = holders(&of(&|t| t.status == "in_progress"), &known, &[]);
    let pullable = holders(
        &of(&|t| ["concept", "design", "ready"].contains(&t.status.as_str())),
        &known,
        &[&writer],
    );
    let counted = [
        // The task, the tasks right under it and those it depends on, each a read of its own.
        (
            "show",
            ran(&board, &["show", &thread, "--as", &writer]),
            shown + holders(&of(&|t| t.parent.as_deref() == Some(&thread)), &known, &[]) + waited,
        ),
        ("list", ran(&board, &["list", "--as", &writer]), listed),
        (
            "list --status in_progress",
            ran(
                &board,
                &["list", "--status", "in_progress", "--as", &writer],
            ),
            in_progress,
        ),
        ("next", ran(&board, &["next", "--as", &writer]), pullable),
        // Every session, and the tasks each holds, read as a list is.
        (
            "who --all",
            ran(&board, &["who", "--all", "--as", &writer]),
            every + listed,
        ),
    ];
    for (command, read, _) in &counted {
        eprintln!(
            "perf: {command} read {} session rows of the board's {every}",
            read.rows
        );
    }
    let wrong: Vec<String> = (counted.iter())
        .filter(|(_, read, rows)| {
            *read
                != Read {
                    rows: *rows,
                    whole_writes: 0,
                }
        })
        .map(|(command, read, rows)| {
            format!("{command}: {read:?}, where {rows} rows are its answer's")
        })
        .collect();
    assert!(wrong.is_empty(), "of {every} sessions: {wrong:#?}");
    // A claim decides on the whole list, and its dispatch reads the whole board; the answer its
    // write returns is its task by the sessions on it, so the write reads no whole table.
    let claim = ran(&board, &["claim", &free, "--as", &writer]);
    eprintln!(
        "perf: claim read {} session rows of the board's {every}, and the whole table in {} of its writes",
        claim.rows, claim.whole_writes
    );
    assert!(claim.rows >= every, "a claim decides on every session");
    assert_eq!(claim.whole_writes, 0, "a claim's write answers by its task");
    assert!(
        shown < 10 && in_progress < every && listed < every && pullable < every,
        "the board has sessions on no task read: {shown}, {in_progress}, {listed}, {pullable} of {every}"
    );
}
