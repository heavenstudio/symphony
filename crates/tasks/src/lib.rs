//! The task board's rules over PostgreSQL: a session's health ([`health`]); sessions registering,
//! beating, leaving ([`NewSession`], [`Beat`]); tasks made and arranged in a tree ([`NewTask`]),
//! joined, released, moved ([`can_transition`]), landed; claim, reserve, verdict and done, `next`,
//! threads, inboxes and their acknowledgement, and the record of launched sessions ([`NewLaunch`])
//! ([`Store`]), over the board's own PostgreSQL ([`pg`]); the board read in one round trip; and the
//! copy of a whole board into it ([`Store::copy`], the `tasks-copy` binary). The board's own
//! server keeps its data in the git common dir ([`git`]) and is started by whatever needs it
//! ([`server`]).
//!
//! A rule reads what it decides on, decides here (a session's health needs this machine's process
//! table), and writes in one transaction that first checks the task's `version` is still the one it
//! read. The transactions are optimistic: where SQLite's `BEGIN IMMEDIATE` makes a second writer
//! wait, here one fails at commit and can be retried, and so does a write whose task moved since
//! its read; either way the rule reads and decides again. A write that needs nothing decided here
//! (a session seen, leaving) checks and refuses inside its one transaction instead. They only meet
//! where both write the same row, so writes that must not pass each other write one: the task's
//! `version`, the message counter, an agent's own counter for its binding. Mutation triggers also
//! advance the structural counter (`counter` `board_structure`) for changes to board inputs, except
//! session mutations, which advance their own per-handle revisions. Authorization and liveness
//! guards fence the sessions they depend on. Callback readers combine both markers to reuse
//! unchanged board inputs without making independent heartbeats write one counter.

#[cfg(test)]
extern crate self as symphony_board;

// The library's tests start their own fixtures and stand-ins through it; the library itself makes
// a child by `sys::command` and `sys::command_with` alone (clippy.toml).
#[cfg(test)]
#[path = "../tests/support/children.rs"]
mod children;

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
pub mod db;
pub mod execution;
pub mod folders;
pub mod gates;
pub mod git;
pub mod health;
mod json_object;
pub mod land;
pub mod launch;
pub mod limits;
pub mod live;
pub mod local;
pub mod lock;
mod maps;
mod move_cli;
mod objects;
pub mod on_land;
pub mod on_review;
pub mod orchestration;
pub mod order;
pub mod pg;
pub mod port;
pub mod project;
mod quote;
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
pub mod settings;
pub mod slots;
pub mod stages;
pub mod stamps;
mod store;
pub use move_cli::{MOVE_HELP, MOVE_USAGE, move_main};
pub mod sweep;
pub mod sys;
pub mod usage;
pub mod waits_on;
pub mod workflow;

#[path = "cli/api/assessment/publication.rs"]
pub mod assessment_publication;
pub use health::Health;
pub use schema::{Access, Schema};
pub use store::recovery;
pub use store::{
    ASK_KINDS, Ask, AssessmentReceipt, AssessmentState, BEFORE_WORK, Beat, Binding, Board, CARRIED,
    COVERED, Callback, ChatSeat, CoveredLine, Database, Error, Filed, Given, HandOff, Held, KINDS,
    LANDS_FROM, LATE_SECONDS, Landed, Message, NewAsk, NewLaunch, NewSession, NewTask, On, Opening,
    PIPELINE, PULLABLE, Pick, Presence, Program, ROLES, Raw, RawRows, ReportPlace, ReportRead,
    ReportTask, Role, STATUSES, SearchHit, SearchSource, Session, StageRecord, Standing, Store,
    THREAD_WINDOW_LIMIT, Task, TaskStamp, ThreadWindow, UNANSWERED_WAIT, UNANSWERED_WAIT_VAR,
    Unfresh, WRITERS_EVERY, WRITERS_RECENT, WRITERS_SINCE, Writer, approved, can_transition,
    covered_by, covered_line, landing, leases_past, reviewer_verdict,
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

Copy a whole board, read as a dump on standard input, into its database,
replacing what that held, in one transaction: into DATABASE (default tasks) on
the server at ADDR, or, with --board, into the board this run reaches. With
--board --out it writes that board's dump instead, in the shape the copy
reads, and changes nothing.

positional arguments:
  ADDR            the server: PostgreSQL's socket folder as postgresql:/PATH
  DATABASE        the database there (default: tasks)

options:
  -h, --help      show this help message and exit
  --version       say the version and exit
  --board         the board this run reaches, not one at ADDR
  --out           with --board: write its dump, change nothing

environment, with --board:
  TASKS_SERVER    the board's server, as ADDR names one, in place of the one
                  the repository's common dir records
  TASKS_DATABASE  the board's database, in place of tasks
";

/// How the copy's refusals name a scratch board: its two forms take one each.
const COPY_SCRATCH: &str = "`TASKS_DATABASE=NAME` with `--board`, or `tasks-copy ADDR NAME`";

/// `tasks-copy ADDR [DATABASE] < dump.json`, or `tasks-copy --board` for the board this run reaches
/// (`TASKS_SERVER`, `TASKS_DATABASE`, else the server in the git common dir, started if it is down),
/// or `tasks-copy --board --out`, which writes that
/// board's dump on one line, in the shape the copy reads, and changes nothing: how a test seeds a
/// board too big to build call by call, and reads it back. The board is read and written in the
/// database it is in ([`cli::Reached::database`]), by the one reader and the one copy
/// ([`Store::dump`], [`Store::copy`]), and a refusal names that database.
/// `args` without the program name; what it says goes to `out`, a refusal to `err`; the exit code
/// is returned.
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
    // The board is read and written in the database it is in, whichever its reach names.
    let board = || {
        cli::writable_board(env)
            .and_then(|reached| Ok((reached.opening(opening.clone()), reached.database()?)))
    };
    let server = |addr: &str, db: &str| {
        let how = opening.clone().live(live::find_here(addr, db));
        cli::database_at(addr, db, "ADDR").map(|database| (how, database))
    };
    let ((opening, database), dumping) = match args {
        [board_word] if board_word == "--board" => (board()?, false),
        [board_word, dump] if board_word == "--board" && dump == "--out" => (board()?, true),
        [addr] if named(addr) => (server(addr, DATABASE)?, false),
        [addr, db] if named(addr) && named(db) => {
            if !crate::db::plain(db) {
                return Err(format!("{db:?} is not a plain name for a database"));
            }
            (server(addr, db)?, false)
        }
        _ => return Err(COPY_USAGE.into()),
    };
    let (addr, db) = (database.addr(), database.database().to_string());
    // A refusal names the database the board is in, and where.
    let refused = |why: Error| match why {
        Error::Db(said) => format!(
            "PostgreSQL: {said} (the board's database {db} at {addr}: `just tasks database` \
             says what serves it and whether it is held)"
        ),
        why => why.to_string(),
    };
    let host = sys::hostname();
    let store = Store::opening_on(
        database,
        Box::new(store::now),
        Box::new(sys::pid_alive),
        &host,
        &opening,
    )
    .map_err(refused)?;
    if dumping {
        let dump = store.dump().map_err(refused)?;
        let text = serde_json::to_string(&dump).map_err(|e| e.to_string())?;
        writeln!(out, "{text}").map_err(|e| format!("cannot write the dump: {e}"))?;
        return Ok(None);
    }
    let mut text = String::new();
    input
        .read_to_string(&mut text)
        .map_err(|e| format!("cannot read the dump: {e}"))?;
    let dump = rows::Dump::from_json(&text)?;
    store.copy(&dump).map_err(refused)?;
    let place = &db;
    Ok(Some(format!(
        "{} tasks, {} links, {} sessions, {} roles, {} messages, {} deliveries, {} barred keys, {} task sessions, {} delivery keys, {} asks, {} usage rows{} into {place} at {addr}",
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
        dump.usages.len(),
        // Named only on a board that holds a custom workflow's records, so a board without one
        // is said as before.
        match dump.workflow.len() {
            0 => String::new(),
            n => format!(", {n} workflow records"),
        },
    )))
}

/// The reclaim's usage, said when its arguments are none of its forms.
const RECLAIM_USAGE: &str =
    "usage: tasks-reclaim, in a checkout of the board's repository, or tasks-reclaim --dir FOLDER";

/// The reclaim's `--help`: what it does and its one option.
pub const RECLAIM_HELP: &str =
    "usage: tasks-reclaim, in a checkout of the board's repository, or tasks-reclaim --dir FOLDER

Give the dead space of the board in this checkout's git common dir back to the
filesystem: PostgreSQL's VACUUM FULL and ANALYZE of its board database, with an
exclusive lock per table and temporary disk room for its rewrite. Its rows and
schema are kept, and nothing is exported or swapped. A board still on SurrealDB
is refused, naming how it moves.

options:
  -h, --help        show this help message and exit
  --version         say the version and exit
  --dir FOLDER      a board folder elsewhere, a copy of the board's to rehearse on
";

/// `tasks-reclaim`: the dead space of the board in the git common dir of `env`'s working
/// directory given back by PostgreSQL's VACUUM FULL and ANALYZE ([`server::Postgres::reclaim`]),
/// its server running meanwhile; or, with `--dir FOLDER`, of a board folder of that shape
/// elsewhere, a copy to rehearse on. Either way served by the PostgreSQL programs of the project
/// it runs in. A board still on SurrealDB is refused, naming how it moves ([`server::GONE`]).
/// What it did goes to `out`, a refusal to `err`; the exit code is returned.
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
    let reclaimed = reclaim_home(args, env).and_then(|home| {
        if home.backend().map_err(|e| e.to_string())? == server::Engine::SurrealDb {
            return Err(home.on_surrealdb("Nothing was reclaimed").to_string());
        }
        let project = project::find(&env.cwd, None)?;
        home.project_postgresql(&project.place.root)
            .and_then(|pg| pg.reclaim(DATABASE))
            .map_err(|e| e.to_string())
    });
    match reclaimed {
        Ok(()) => {
            let _ = writeln!(out, "{RECLAIMED}");
            0
        }
        Err(why) => {
            let _ = writeln!(err, "❌ tasks-reclaim: {why}");
            2
        }
    }
}

/// The line `tasks-reclaim` ends with on success.
const RECLAIMED: &str = "tasks-reclaim: PostgreSQL reclaimed the board's dead space with VACUUM \
                         FULL and ANALYZE; its rows and schema were kept";

/// The board home `tasks-reclaim`'s `args` name: with none, the one in the git common dir of
/// `env`'s working directory; with `--dir FOLDER`, that folder, read from the working directory,
/// whatever repository it runs in. Arguments of no form are refused with the usage before any
/// repository is looked for.
fn reclaim_home(args: &[String], env: &cli::Env) -> Result<server::Home, String> {
    let folder = match args {
        [] => None,
        [dir, folder] if dir == "--dir" => Some(env.cwd.join(folder)),
        _ => return Err(RECLAIM_USAGE.to_string()),
    };
    let common = git::common_dir(&env.cwd).map_err(|e| e.to_string())?;
    Ok(match folder {
        Some(folder) => server::Home::new(&folder),
        None => server::Home::in_common(&common),
    })
}

#[cfg(test)]
mod reclaim_tests {
    use super::*;

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
        for word in ["--dir FOLDER", "VACUUM FULL", "SurrealDB"] {
            assert!(RECLAIM_HELP.contains(word), "{word}");
        }
        // Beside another argument it is no form of the reclaim's.
        let args = ["--help".to_string(), "more".to_string()];
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(reclaim_main(&args, &env, &mut out, &mut err), 2);
        assert!(out.is_empty());
    }

    /// A folder of this test's own, holding a repository (`repo`, a `.git` folder and nothing
    /// more, which is all the common dir's reader asks) and a folder that is in none (`bare`).
    struct Place(std::path::PathBuf);

    impl Place {
        fn new(name: &str) -> Place {
            let dir =
                std::env::temp_dir().join(format!("reclaim-main-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("repo/.git")).unwrap();
            std::fs::create_dir_all(dir.join("repo/sub")).unwrap();
            std::fs::create_dir_all(dir.join("bare")).unwrap();
            Place(dir.canonicalize().unwrap())
        }

        fn env(&self, cwd: &str) -> cli::Env {
            cli::Env {
                vars: Default::default(),
                cwd: self.0.join(cwd),
            }
        }
    }

    impl Drop for Place {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn no_argument_names_the_board_in_the_common_dir_of_the_repository_it_runs_in() {
        let place = Place::new("repository");
        let common = place.0.join("repo/.git");
        for cwd in ["repo", "repo/sub"] {
            let home = reclaim_home(&[], &place.env(cwd)).unwrap();
            assert_eq!(home.dir(), common.join("symphony/tasks"), "{cwd}");
        }
    }

    #[test]
    fn a_folder_named_wins_over_the_repository_it_runs_in() {
        let place = Place::new("folder");
        let common = place.0.join("repo/.git");
        let elsewhere = place.0.join("bare/board");
        let absolute = strings(&["--dir", elsewhere.to_str().unwrap()]);
        let home = reclaim_home(&absolute, &place.env("repo")).unwrap();
        assert_eq!(home.dir(), elsewhere);
        assert!(!home.dir().starts_with(&common));
        // A relative folder is read from the working directory, not from the repository's top.
        let relative = strings(&["--dir", "copy"]);
        let home = reclaim_home(&relative, &place.env("repo/sub")).unwrap();
        assert_eq!(home.dir(), place.0.join("repo/sub/copy"));
        // Selecting a board makes none: nothing was written in either place.
        assert!(!elsewhere.exists());
        assert!(!common.join("symphony").exists());
    }

    #[test]
    fn a_reclaim_outside_any_repository_is_refused_with_or_without_a_folder() {
        let place = Place::new("outside");
        let env = place.env("bare");
        for args in [strings(&[]), strings(&["--dir", "board"])] {
            let why = reclaim_home(&args, &env).err().expect("no repository");
            assert_ne!(why, RECLAIM_USAGE, "{args:?}");
            let (mut out, mut err) = (Vec::new(), Vec::new());
            assert_eq!(reclaim_main(&args, &env, &mut out, &mut err), 2, "{args:?}");
            assert!(out.is_empty());
            assert_eq!(
                String::from_utf8(err).unwrap(),
                format!("❌ tasks-reclaim: {why}\n")
            );
        }
        assert!(!place.0.join("bare/board").exists());
        // Arguments of no form are refused before any repository is looked for.
        assert_eq!(
            reclaim_home(&strings(&["--dir"]), &env).err().as_deref(),
            Some(RECLAIM_USAGE)
        );
    }

    /// A board home still on SurrealDB is refused, naming how it moves, and nothing is
    /// reclaimed or started there.
    #[test]
    fn a_board_still_on_surrealdb_is_refused_naming_how_it_moves() {
        let place = Place::new("surrealdb");
        let board = place.0.join("repo/.git/symphony/tasks");
        std::fs::create_dir_all(board.join("data")).unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(reclaim_main(&[], &place.env("repo"), &mut out, &mut err), 2);
        assert!(out.is_empty());
        let said = String::from_utf8(err).unwrap();
        assert!(
            said.starts_with("❌ tasks-reclaim: the board in "),
            "{said}"
        );
        assert!(said.contains(server::GONE), "{said}");
        assert!(said.ends_with("Nothing was reclaimed\n"), "{said}");
        assert!(!board.join("postgresql").exists());
    }

    #[test]
    fn a_reclaim_says_the_vacuum_and_names_no_store_export_or_server() {
        assert_eq!(
            RECLAIMED,
            "tasks-reclaim: PostgreSQL reclaimed the board's dead space with VACUUM FULL and \
             ANALYZE; its rows and schema were kept"
        );
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
