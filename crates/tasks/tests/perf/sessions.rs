//! The session rows the board's commands read, on the generated board: a read that shows tasks
//! returns the sessions holding a role on them (and `next` the session asking), never every
//! session the board has seen, a list that grows with the board's history. `who`, a whole board
//! read and the read a write decides on keep the whole list. Rows, never a time: each command
//! runs as the `tasks` binary through the recording relay ([`crate::proxy`]), and each request of
//! reads it sent is sent again to count the sessions it answers with: a list of sessions an item
//! a row (`Pg::Listed`'s statement over `session s`), or a value's `sessions` list.

use crate::board::{Board, board};
use crate::generated;
use crate::proxy::{Proxy, Request};
use serde_json::Value;
use std::collections::BTreeSet;
use symphony_board::{Task, pg};

/// A list of sessions read an item a row, as the store's reads list them (`store/postgres/mod.rs`'s
/// `List::rows`, ordered by when each connected): one statement each, whatever rows it picks.
const LISTED: &str = "AS item, s.connected AS place0, s.handle AS place1 FROM session s";

/// What a command's requests read of the sessions.
#[derive(Debug, PartialEq, Eq)]
struct Read {
    /// The session rows its reads answered with, in all.
    rows: usize,
    /// How many of its writes read the whole session table: a write is never sent again, so its
    /// rows are counted by what it asks for.
    whole_writes: usize,
}

/// Whether `sql` reads every session: the session table with no condition after it, as a list of
/// them, or one value of them, names it.
fn every_session(sql: &str) -> bool {
    (sql.match_indices("FROM session s")).any(|(at, from)| {
        let after = sql[at + from.len()..].trim_start();
        after.is_empty() || after.starts_with(')') || after.starts_with("ORDER BY")
    })
}

/// The session rows `answer`, the answer of `sql`, holds: its rows where it lists sessions an item
/// a row, else its value's `sessions` list.
fn sessions_in(sql: &str, answer: &pg::Answer) -> usize {
    if sql.contains(LISTED) {
        return answer.rows.len();
    }
    let value = (answer.value()).and_then(|value| serde_json::from_str::<Value>(value).ok());
    (value.as_ref())
        .and_then(|value| value.get("sessions")?.as_array())
        .map_or(0, Vec::len)
}

fn read(client: &pg::Client, requests: &[Request]) -> Read {
    let mut read = Read {
        rows: 0,
        whole_writes: 0,
    };
    for request in requests {
        if !request.reads() {
            read.whole_writes += usize::from(request.steps.iter().any(|s| every_session(&s.sql)));
            continue;
        }
        let answers = (request.again(client))
            .unwrap_or_else(|why| panic!("a read sent again: {why}: {request:#?}"));
        read.rows += (request.steps.iter())
            .zip(&answers)
            .map(|(step, answer)| sessions_in(&step.sql, answer))
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
    let whole = (board.store(&symphony_board::sys::hostname()).board()).unwrap();
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

#[cfg(test)]
mod counted {
    use super::*;

    /// A read of every session names the table with no condition after it; one of the sessions a
    /// view shows, or of one session, does not.
    #[test]
    fn a_read_of_every_session_names_the_table_with_no_condition() {
        assert!(every_session(
            "SELECT n.item FROM (SELECT $1::jsonb AS v) AS board_vars, LATERAL (SELECT (to_jsonb(s) - 'id')::text AS item, s.connected AS place0, s.handle AS place1 FROM session s) AS n ORDER BY n.place0, n.place1"
        ));
        assert!(every_session(
            "coalesce((SELECT jsonb_agg(x ORDER BY s.connected) FROM session s), '[]')"
        ));
        assert!(every_session("SELECT 1 FROM session s ORDER BY s.handle"));
        assert!(!every_session(
            "SELECT 1 FROM session s WHERE s.handle IN (SELECT handle FROM view_roles)"
        ));
        assert!(!every_session(
            "SELECT 1 FROM session s LEFT JOIN callback_state c ON c.id = s.id WHERE s.agent IS NOT NULL"
        ));
        assert!(!every_session("SELECT 1 FROM sessions"));
    }

    /// A list of sessions read an item a row counts its rows; a value counts its `sessions` list,
    /// and one with none counts none.
    #[test]
    fn a_list_counts_its_rows_and_a_value_its_sessions() {
        let row = |text: &str| vec![Some(text.to_string())];
        let listed = pg::Answer {
            rows: vec![row("{}"), row("{}"), row("{}")],
            tag: "SELECT 3".into(),
        };
        let sql = format!("SELECT n.item FROM x, LATERAL (SELECT 1 {LISTED} WHERE true) AS n");
        assert_eq!(sessions_in(&sql, &listed), 3);
        let value = pg::Answer {
            rows: vec![row(
                r#"{"sessions": [{"handle": "a"}, {"handle": "b"}], "held": []}"#,
            )],
            tag: "SELECT 1".into(),
        };
        assert_eq!(sessions_in("SELECT to_jsonb((x))::text", &value), 2);
        let none = pg::Answer {
            rows: vec![row(r#"{"tasks": []}"#)],
            tag: "SELECT 1".into(),
        };
        assert_eq!(sessions_in("SELECT to_jsonb((x))::text", &none), 0);
    }
}
