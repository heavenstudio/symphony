//! The task board's rules over SurrealDB: a
//! session's health ([`health`]); sessions registering, beating, leaving ([`NewSession`], [`Beat`]);
//! tasks made and arranged in a tree ([`NewTask`]), joined, released, moved ([`can_transition`]),
//! landed; claim, reserve, verdict and done, `next`, threads, inboxes and their acknowledgement, and
//! the record of launched sessions ([`NewLaunch`]) ([`Store`]), over a local SurrealDB server
//! ([`rpc`]); the board read in one round trip; and the copy of a whole board into it ([`Store::copy`],
//! the `tasks-copy` binary), which the one-way move of the SQLite board makes ([`cutover`]). The
//! board's own server keeps its data in the git common dir ([`git`]) and is started by whatever needs
//! it ([`server`]), on a loopback port reserved for it ([`port`]). It takes that board once `just tasks-cutover` has moved the SQLite one into it.
//!
//! A rule reads what it decides on, decides here (a session's health needs this machine's process
//! table), and writes in one transaction that first checks the task's `version` is still the one it
//! read. SurrealDB's transactions are optimistic: where SQLite's `BEGIN IMMEDIATE` makes a second
//! writer wait, here it fails at commit ("can be retried"), and so does a write whose task moved
//! since its read; either way the rule reads and decides again. A write that needs nothing decided
//! here (a session seen, leaving) checks and refuses inside its one transaction instead. They only
//! meet where both write the same record, so writes that must not pass each other write one: the
//! task's `version`, the message counter, an agent's own counter for its binding.
//! Transactional mutation events also advance the shared `counter:board` for changes to board
//! inputs. Those writes can conflict across otherwise unrelated tasks; every such path retries
//! the whole transaction. Callback readers use that marker to reuse unchanged board inputs.

#[cfg(test)]
extern crate self as symphony_board;

pub mod cbor;
pub mod cli;
pub mod cutover;
pub mod embedding;
pub mod git;
pub mod health;
pub mod launch;
pub mod port;
pub mod project;
pub mod pyjson;
pub mod rows;
pub mod rpc;
mod schema;
pub mod server;
mod store;
pub mod sweep;
pub mod sys;

pub use health::Health;
pub use schema::{Access, Schema};
pub use store::{
    BEFORE_WORK, Beat, Board, Callback, EmbeddingStats, Error, KINDS, LANDS_FROM, Landed, Message,
    Moved, NewLaunch, NewSession, NewTask, Opening, PIPELINE, PULLABLE, Pick, ROLES, Role,
    STATUSES, SearchHit, Session, Store, Task, VectorHit, VectorSearchProfile, can_transition,
};

/// The board schema this build reads and writes: the one server serves no project whose
/// registering build wrote a newer one.
pub const SCHEMA: i64 = schema::VERSION;

/// The board's namespace and database on the server.
pub const NAMESPACE: &str = "sf";
pub const DATABASE: &str = "tasks";

/// The copy's usage, said when its arguments are none of its forms.
const COPY_USAGE: &str =
    "usage: tasks-copy ADDR [DATABASE] < dump.json, or tasks-copy --board [--out]";

/// `tasks-copy ADDR [DATABASE] < dump.json`, or `tasks-copy --board` for the board this run reaches
/// (`TASKS_SERVER`, `TASKS_DATABASE`, else the server in the git common dir, started if it is down,
/// whether or not the board has moved there), or `tasks-copy --board --out`, which writes that
/// board's dump on one line, in the shape the copy reads, and changes nothing: how a test seeds a
/// board too big to build call by call, and reads it back. `args` without the program name; what it
/// says goes to `out`, a refusal to `err`; the exit code is returned.
pub fn copy_main(
    args: &[String],
    env: &cli::Env,
    input: &mut dyn std::io::Read,
    out: &mut dyn std::io::Write,
    err: &mut dyn std::io::Write,
) -> i32 {
    match copy(args, env, input, out) {
        Ok(Some(said)) => {
            let _ = writeln!(out, "tasks-copy: {said}");
            0
        }
        Ok(None) => 0,
        Err(why) => {
            let _ = writeln!(err, "❌ tasks-copy: {why}");
            2
        }
    }
}

fn copy(
    args: &[String],
    env: &cli::Env,
    input: &mut dyn std::io::Read,
    out: &mut dyn std::io::Write,
) -> Result<Option<String>, String> {
    let named = |word: &String| !word.starts_with("--");
    let (client, dumping) = match args {
        [board] if board == "--board" => (cli::board_client(env)?, false),
        [board, dump] if board == "--board" && dump == "--out" => (cli::board_client(env)?, true),
        [addr] if named(addr) => (rpc::Client::new(addr, NAMESPACE, DATABASE), false),
        [addr, db] if named(addr) && named(db) => {
            if !rpc::plain(db) {
                return Err(format!("{db:?} is not a plain name for a database"));
            }
            (rpc::Client::new(addr, NAMESPACE, db), false)
        }
        _ => return Err(COPY_USAGE.into()),
    };
    if dumping {
        let dump = Store::open(client)
            .and_then(|store| store.dump())
            .map_err(|e| e.to_string())?;
        let text = serde_json::to_string(&dump).map_err(|e| e.to_string())?;
        writeln!(out, "{text}").map_err(|e| format!("cannot write the dump: {e}"))?;
        return Ok(None);
    }
    let mut text = String::new();
    input
        .read_to_string(&mut text)
        .map_err(|e| format!("cannot read the dump: {e}"))?;
    let dump = rows::Dump::from_json(&text)?;
    let (addr, db) = (client.addr().to_string(), client.database().to_string());
    let store = Store::open(client).map_err(|e| e.to_string())?;
    store.copy(&dump).map_err(|e| e.to_string())?;
    Ok(Some(format!(
        "{} tasks, {} links, {} sessions, {} roles, {} messages, {} deliveries, {} barred keys, {} task sessions into {NAMESPACE}/{db} at {addr}",
        dump.tasks.len(),
        dump.task_links.len(),
        dump.sessions.len(),
        dump.roles.len(),
        dump.messages.len(),
        dump.deliveries.len(),
        dump.barred_keys.len(),
        dump.task_sessions.len(),
    )))
}
