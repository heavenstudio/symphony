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
pub mod arm;

#[cfg(test)]
#[path = "../tests/store/common.rs"]
#[expect(
    dead_code,
    reason = "library regressions reuse parts of the shared scratch store harness"
)]
mod store_fixture;

pub mod board_health;
pub mod builds;
pub mod cbor;
pub mod cli;
pub mod codex_queue;
pub mod confine;
pub mod covering;
pub mod embedding;
pub mod fast_fix;
pub mod folders;
pub mod git;
pub mod health;
pub mod land;
pub mod launch;
pub mod limits;
pub mod live;
pub mod on_land;
pub mod on_review;
pub mod orchestration;
pub mod port;
pub mod project;
pub mod pyjson;
pub mod recording;
pub mod red;
pub mod reports;
pub mod rows;
pub mod rpc;
pub mod run_logs;
pub mod runs;
pub mod schema;
pub mod server;
pub mod session_url;
pub mod slots;
pub mod stages;
pub mod stamps;
mod store;
pub mod sweep;
pub mod sys;

#[path = "cli/api/assessment/publication.rs"]
pub mod assessment_publication;
pub use health::Health;
pub use schema::{Access, Schema};
pub use store::{
    ASK_KINDS, Ask, AssessmentReceipt, AssessmentState, BEFORE_WORK, Beat, Binding, Board, COVERED,
    Callback, ChatSeat, CoveredLine, EmbeddingStats, Error, Given, HandOff, Held, KINDS,
    LANDS_FROM, LATE_SECONDS, Landed, Message, NewAsk, NewLaunch, NewSession, NewTask, Opening,
    PIPELINE, PULLABLE, Pick, Presence, ROLES, ReportRead, ReportTask, Role, SLOT_READS, STATUSES,
    SearchHit, Session, StageRecord, Store, Task, UNANSWERED_WAIT, UNANSWERED_WAIT_VAR, Unfresh,
    VectorHit, VectorSearchProfile, approved, can_transition, covered_by, covered_line, landing,
    reviewer_verdict,
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

/// The copy's `--help`: every flag and variable its refusals name.
pub const COPY_HELP: &str =
    "usage: tasks-copy ADDR [DATABASE] < dump.json, or tasks-copy --board [--out]

Copy a whole board, read as a dump on standard input, into SurrealDB, replacing
what the database held, in one transaction: into DATABASE (default tasks) of
namespace sf on the server at ADDR, or, with --board, into the board this run
reaches. With --board --out it writes that board's dump instead, in the shape
the copy reads, and changes nothing.

positional arguments:
  ADDR            the server, HOST:PORT
  DATABASE        the database there (default: tasks)

options:
  -h, --help      show this help message and exit
  --version       say the version and exit
  --board         the board this run reaches, not one at ADDR
  --out           with --board: write its dump, change nothing

environment, with --board:
  TASKS_SERVER    the board's server, HOST:PORT, in place of the one the
                  repository's common dir records
  TASKS_DATABASE  the board's database, in place of tasks
";

/// How the copy's refusals name a scratch board: its two forms take one each.
const COPY_SCRATCH: &str = "`TASKS_DATABASE=NAME` with `--board`, or `tasks-copy ADDR NAME`";

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
    if symphony_args::version(args, "tasks-copy", env!("CARGO_PKG_VERSION"), out) {
        return 0;
    }
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
    if let [help] = args
        && (help == "--help" || help == "-h")
    {
        write!(out, "{COPY_HELP}").map_err(|e| format!("cannot write the help: {e}"))?;
        return Ok(None);
    }
    let opening = Opening::write().scratch(COPY_SCRATCH);
    // Either way the store opens a live board as one, so a board that went back is refused it
    // ([`live::seen`]): the board this run reaches, or the one whose server an address names.
    let board = || {
        cli::writable_board(env).map(|reached| (reached.opening(opening.clone()), reached.client))
    };
    let server = |addr: &str, db: &str| {
        let how = opening.clone().live(live::find_here(addr, db));
        (how, rpc::Client::new(addr, NAMESPACE, db))
    };
    let ((opening, client), dumping) = match args {
        [board_word] if board_word == "--board" => (board()?, false),
        [board_word, dump] if board_word == "--board" && dump == "--out" => (board()?, true),
        [addr] if named(addr) => (server(addr, DATABASE), false),
        [addr, db] if named(addr) && named(db) => {
            if !rpc::plain(db) {
                return Err(format!("{db:?} is not a plain name for a database"));
            }
            (server(addr, db), false)
        }
        _ => return Err(COPY_USAGE.into()),
    };
    if dumping {
        let dump = Store::open_as(client, &opening)
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
    let store = Store::open_as(client, &opening).map_err(|e| e.to_string())?;
    store.copy(&dump).map_err(|e| e.to_string())?;
    Ok(Some(format!(
        "{} tasks, {} links, {} sessions, {} roles, {} messages, {} deliveries, {} barred keys, {} task sessions, {} delivery keys, {} asks into {NAMESPACE}/{db} at {addr}",
        dump.tasks.len(),
        dump.task_links.len(),
        dump.sessions.len(),
        dump.roles.len(),
        dump.messages.len(),
        dump.deliveries.len(),
        dump.barred_keys.len(),
        dump.task_sessions.len(),
        dump.delivery_keys.len(),
        dump.asks.len(),
    )))
}

/// The reclaim's usage, said when its arguments are none of its forms.
const RECLAIM_USAGE: &str =
    "usage: tasks-reclaim, in a checkout of the board's repository, or tasks-reclaim --dir FOLDER";

/// The reclaim's `--help`: what it does, its one option and the variable it reads.
pub const RECLAIM_HELP: &str =
    "usage: tasks-reclaim, in a checkout of the board's repository, or tasks-reclaim --dir FOLDER

Rebuild the store of the board in this checkout's git common dir from an export
of itself, which holds only what is live: its server is stopped meanwhile and
started again on the new store. The old store is kept as a snapshot, and the
line it ends with says where and until when.

options:
  -h, --help        show this help message and exit
  --version         say the version and exit
  --dir FOLDER      a board folder elsewhere, a copy of the board's to rehearse on

environment:
  SYMPHONY_SURREAL  the SurrealDB binary that serves the store, in place of
                    the pinned one beside the common dir
";

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
    if symphony_args::version(args, "tasks-reclaim", env!("CARGO_PKG_VERSION"), out) {
        return 0;
    }
    if let [help] = args
        && (help == "--help" || help == "-h")
    {
        let _ = write!(out, "{RECLAIM_HELP}");
        return 0;
    }
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
                 exported the same from both stores; their exports are in {}; {}; the board's \
                 server is pid {} at {}",
                size(done.before),
                size(done.after),
                done.databases.len(),
                done.exports.display(),
                old_store_kept(&done.kept),
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

/// Where `tasks-reclaim` says the old store is kept, and until when: a snapshot, kept as any is
/// ([`server::snapshot`]), the room it takes freed only once it is removed.
fn old_store_kept(kept: &server::Kept) -> String {
    let path = kept.path();
    let how = match kept {
        server::Kept::Cloned(_) => "a copy-on-write clone",
        server::Kept::Copied(_) => "a plain copy, as this volume cannot clone",
        server::Kept::Already(_) => "a snapshot taken before, which holds it as it was",
    };
    let until = if server::snapshot::holds_log(path) {
        "its write-ahead log holds writes, so it is kept until the owner removes it".to_string()
    } else {
        format!(
            "its log is empty, so it is kept among the newest {} such snapshots, which later \
             starts' snapshots prune, unless the owner removes it first",
            server::snapshot::KEEP
        )
    };
    format!(
        "the old store is kept at {} ({how}): {until} (`rm -rf {}`), and the room it takes comes \
         back only then",
        path.display(),
        path.display()
    )
}

/// `bytes` as a person reads it, to a tenth: GiB from one on, else MiB.
pub(crate) fn size(bytes: u64) -> String {
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

    #[test]
    fn help_alone_says_what_it_does_on_stdout_and_exits_zero() {
        let env = cli::Env::of_process();
        for help in ["--help", "-h"] {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            assert_eq!(
                reclaim_main(&[help.to_string()], &env, &mut out, &mut err),
                0
            );
            assert_eq!(String::from_utf8(out).unwrap(), RECLAIM_HELP);
            assert!(err.is_empty());
        }
        assert!(RECLAIM_HELP.starts_with(RECLAIM_USAGE));
        for word in ["--dir FOLDER", "SYMPHONY_SURREAL", "snapshot"] {
            assert!(RECLAIM_HELP.contains(word), "{word}");
        }
        // Beside another argument it is no form of the reclaim's.
        let args = ["--help".to_string(), "more".to_string()];
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(reclaim_main(&args, &env, &mut out, &mut err), 2);
        assert!(out.is_empty());
    }
}

#[cfg(test)]
mod version_tests {
    use super::*;

    /// Each board command's answer to `--version` alone, with its exit code and what it said on
    /// stderr.
    fn answers(command: &str) -> (i32, String, Vec<u8>) {
        let env = cli::Env::of_process();
        let args = ["--version".to_string()];
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = match command {
            "tasks" => cli::main(&args, &env, &mut &b""[..], &mut out, &mut err),
            "agent-send" => cli::agent_send::main(&args, &env, &mut out, &mut err),
            "tasks-copy" => copy_main(&args, &env, &mut &b""[..], &mut out, &mut err),
            "tasks-reclaim" => reclaim_main(&args, &env, &mut out, &mut err),
            _ => unreachable!("{command}"),
        };
        (code, String::from_utf8(out).unwrap(), err)
    }

    #[test]
    fn each_board_command_says_its_name_and_the_packages_version_and_reaches_no_board() {
        for command in ["tasks", "agent-send", "tasks-copy", "tasks-reclaim"] {
            assert_eq!(
                answers(command),
                (
                    0,
                    format!("{command} {}\n", env!("CARGO_PKG_VERSION")),
                    Vec::new()
                ),
                "{command}"
            );
        }
    }
}
