//! The task board's rules over SurrealDB: a
//! session's health ([`health`]); sessions registering, beating, leaving ([`NewSession`], [`Beat`]);
//! tasks made and arranged in a tree ([`NewTask`]), joined, released, moved ([`can_transition`]),
//! landed; claim, reserve, verdict and done, `next`, threads, inboxes and their acknowledgement, and
//! the record of launched sessions ([`NewLaunch`]) ([`Store`]), over a local SurrealDB server
//! ([`rpc`]); the board read in one round trip; and the copy of a whole board into it ([`Store::copy`],
//! the `tasks-copy` binary). The board's own server keeps its data in the git common dir ([`git`])
//! and is started by whatever needs it ([`server`]), on a loopback port reserved for it ([`port`]).
//!
//! A rule reads what it decides on, decides here (a session's health needs this machine's process
//! table), and writes in one transaction that first checks the task's `version` is still the one it
//! read. SurrealDB's transactions are optimistic: where SQLite's `BEGIN IMMEDIATE` makes a second
//! writer wait, here it fails at commit ("can be retried"), and so does a write whose task moved
//! since its read; either way the rule reads and decides again. A write that needs nothing decided
//! here (a session seen, leaving) checks and refuses inside its one transaction instead. They only
//! meet where both write the same record, so writes that must not pass each other write one: the
//! task's `version`, the message counter, an agent's own counter for its binding.
//! Transactional mutation events also advance the structural `counter:board_structure` for changes to board
//! inputs, except session mutations, which advance their own per-handle revisions. Authorization
//! and liveness guards fence the sessions they depend on. Callback readers combine both markers
//! to reuse unchanged board inputs without making independent heartbeats write one counter.

// The library's tests start their own fixtures and stand-ins; clippy checks the library without
// them too, where only `sys::command` and `sys::command_with` make a child (clippy.toml).
#![cfg_attr(
    test,
    expect(
        clippy::disallowed_methods,
        reason = "a test starts its fixtures as it likes; the library alone is checked without it"
    )
)]

#[cfg(test)]
extern crate self as symphony_board;

pub mod alarm;

#[cfg(test)]
#[path = "../tests/store/common.rs"]
#[expect(
    dead_code,
    reason = "library regressions reuse parts of the shared scratch store harness"
)]
mod store_fixture;

pub mod board_health;
pub mod cbor;
pub mod cli;
mod codex_queue;
pub mod embedding;
pub mod fast_fix;
pub mod git;
pub mod health;
pub mod land;
pub mod launch;
pub mod live;
pub mod on_review;
pub mod orchestration;
pub mod port;
pub mod project;
pub mod pyjson;
pub mod red;
pub mod rows;
pub mod rpc;
pub mod runs;
pub mod schema;
pub mod server;
pub mod session_url;
pub mod slots;
pub mod stages;
mod store;
pub mod sweep;
pub mod sys;

#[path = "cli/api/assessment/publication.rs"]
pub mod assessment_publication;
pub use health::Health;
pub use schema::{Access, Schema};
pub use store::{
    AssessmentReceipt, AssessmentState, BEFORE_WORK, Beat, Binding, Board, Callback,
    EmbeddingStats, Error, HandOff, Held, KINDS, LANDS_FROM, Landed, Message, NewLaunch,
    NewSession, NewTask, Opening, PIPELINE, PULLABLE, Pick, Presence, ROLES, Role, SLOT_READS,
    STATUSES, SearchHit, Session, StageRecord, Store, Task, VectorHit, VectorSearchProfile,
    approved, can_transition,
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
/// (`TASKS_SERVER`, `TASKS_DATABASE`, else the server in the git common dir, started if it is down),
/// or `tasks-copy --board --out`, which writes that
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
        "{} tasks, {} links, {} sessions, {} roles, {} messages, {} deliveries, {} barred keys, {} task sessions, {} delivery keys into {NAMESPACE}/{db} at {addr}",
        dump.tasks.len(),
        dump.task_links.len(),
        dump.sessions.len(),
        dump.roles.len(),
        dump.messages.len(),
        dump.deliveries.len(),
        dump.barred_keys.len(),
        dump.task_sessions.len(),
        dump.delivery_keys.len(),
    )))
}

/// The reclaim's usage, said when its arguments are none of its forms.
const RECLAIM_USAGE: &str =
    "usage: tasks-reclaim, in a checkout of the board's repository, or tasks-reclaim --dir FOLDER";

/// `tasks-reclaim`: the store of the board in the git common dir of `env`'s working directory rebuilt
/// from an export of itself ([`server::Home::reclaim`]), the board's server down meanwhile; or, with
/// `--dir FOLDER`, the store of a board folder of that shape elsewhere, a copy to rehearse on. Either
/// way served by the pinned `surreal` beside that common dir, or `SYMPHONY_SURREAL`'s. What it did
/// goes to `out`, a refusal to `err`; the exit code is returned.
pub fn reclaim_main(
    args: &[String],
    env: &cli::Env,
    out: &mut dyn std::io::Write,
    err: &mut dyn std::io::Write,
) -> i32 {
    let folder = match args {
        [] => Ok(None),
        [dir, folder] if dir == "--dir" => Ok(Some(env.cwd.join(folder))),
        _ => Err(RECLAIM_USAGE.to_string()),
    };
    let reclaimed = folder.and_then(|folder| {
        let common = git::common_dir(&env.cwd).map_err(|e| e.to_string())?;
        let home = server::Home::in_common(
            &common,
            env.read(symphony_env::SURREAL).map(std::path::Path::new),
        );
        let home = match folder {
            Some(folder) => server::Home::new(&folder, home.surreal()),
            None => home,
        };
        home.reclaim().map_err(|e| e.to_string())
    });
    match reclaimed {
        Ok(done) => {
            let _ = writeln!(
                out,
                "tasks-reclaim: the board's store took {} and takes {}; {} databases carried, each \
                 exported the same from both stores; their exports are in {}; the board's server is \
                 pid {} at {}",
                size(done.before),
                size(done.after),
                done.databases.len(),
                done.exports.display(),
                done.running.pid,
                done.running.addr
            );
            0
        }
        Err(why) => {
            let _ = writeln!(err, "❌ tasks-reclaim: {why}");
            2
        }
    }
}

/// `bytes` as a person reads it, to a tenth: GiB from one on, else MiB.
fn size(bytes: u64) -> String {
    let mib = bytes as f64 / f64::from(1 << 20);
    if mib >= 1024.0 {
        format!("{:.1} GiB", mib / 1024.0)
    } else {
        format!("{mib:.1} MiB")
    }
}

#[cfg(test)]
mod reclaim_tests {
    use super::*;

    #[test]
    fn a_size_is_said_in_mib_below_one_gib_and_in_gib_from_it() {
        assert_eq!(size(0), "0.0 MiB");
        assert_eq!(size(575_736 * 1024), "562.2 MiB");
        assert_eq!(size((1 << 30) - 1), "1024.0 MiB");
        assert_eq!(size(1 << 30), "1.0 GiB");
        assert_eq!(size(109_490_057_216), "102.0 GiB");
    }

    #[test]
    fn arguments_of_no_form_are_refused_before_a_board_is_reached() {
        let env = cli::Env::of_process();
        for args in [
            vec!["--board"],
            vec!["--dir"],
            vec!["folder"],
            vec!["--dir", "folder", "more"],
        ] {
            let args: Vec<String> = args.into_iter().map(String::from).collect();
            let (mut out, mut err) = (Vec::new(), Vec::new());
            assert_eq!(reclaim_main(&args, &env, &mut out, &mut err), 2, "{args:?}");
            assert!(out.is_empty());
            assert_eq!(
                String::from_utf8(err).unwrap(),
                format!("❌ tasks-reclaim: {RECLAIM_USAGE}\n")
            );
        }
    }
}
