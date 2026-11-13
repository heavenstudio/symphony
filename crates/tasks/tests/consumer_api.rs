//! The board's public names that consuming projects' code calls, each in the shape it calls it: a
//! signature pinned by coercing the function to its pointer type, a struct its code builds by its
//! fields built by them all, an enum it matches matched by its variants, a trait it implements
//! implemented with the methods it writes, a struct it builds by the fields it names and fills from
//! `Default` or a constructor built by those. Nothing here runs; it is compiled by the test build and
//! by `just lint-rs` (clippy over every target), so a change that would break a consumer fails a
//! task's gate rather than a run later. A field it reads or a method it calls on a value that is
//! not named here the nightly's conciliation (`crates/symphony/src/nightly/conciliate.rs`, which
//! builds each project the machine serves on the candidate) alone catches; the other packages'
//! guards are named in `crates/symphony/tests/consumer_api.rs`.
//!
//! A release's refresh moves a consumer's pin in a commit that touches only its `Cargo.toml` and
//! `Cargo.lock`, so a consumer's code never changes with a release: a name or signature here changes
//! only with a consumer release plan (the consumer's code moved first, against a release that still
//! has the old name). New behaviour goes behind the name a consumer calls, or beside it under a new
//! one. Names were read from the consuming projects' code on 2026-11-12 (every `symphony_board::` path
//! and import); add one when a consumer starts calling it.
#![expect(
    clippy::type_complexity,
    reason = "each signature is written out whole: it is what a consumer's code depends on"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use symphony_board::assessment_publication::{self, Binding, Bindings, Decision, Publication};
use symphony_board::cli::{self, Env};
use symphony_board::land::Generator;
use symphony_board::launch::LaunchError;
use symphony_board::launch::hc::Hc;
use symphony_board::launch::reap::{self, REAP_SECONDS, Reap};
use symphony_board::rows::{Dump, LinkRow, RoleRow, SessionRow, TaskRow, TaskSessionRow};
use symphony_board::rpc::{Client, DbError};
use symphony_board::sweep::git::{DISPOSABLE, Listed};
use symphony_board::sys::{Child, Command, Spawning as _, Stdio};
use symphony_board::{
    AssessmentReceipt, AssessmentState, Beat, Error, Health, Landed, Message, NewLaunch,
    NewSession, NewTask, STATUSES, Session, Store, Task,
};
use symphony_board::{alarm, health, on_review, port, red, server, slots, sweep, sys};

type Result<T> = std::result::Result<T, Error>;

#[test]
fn the_names_consumers_call_keep_their_shapes() {
    // The command line and the board a consumer's own process reaches.
    let _: fn() -> Env = Env::of_process;
    let _ = |vars: BTreeMap<String, String>, cwd: PathBuf| Env { vars, cwd };
    let _: fn(&Env) -> std::result::Result<Client, String> = cli::board_client;
    let _: fn(&Env) -> std::result::Result<Store, String> = cli::connect;
    let _: fn(&[String], &Env, &mut dyn Read, &mut dyn Write, &mut dyn Write) -> i32 = cli::main;
    let _: fn() -> ExitCode = cli::process_main;
    let _: fn(&[String], &Env, &mut dyn Write, &mut dyn Write) -> i32 = cli::agent_send::main;
    let _ = |store: &Store, root: &Path, now: f64| {
        let tips = cli::probes::tips(Some(root));
        let _ = cli::pull::digest_with_tips(store, &[], None, Some(&tips), now, &[]);
        cli::pull::digest(store, &[], None, now, &[]).map(|digest| digest.count("unanswered"))
    };

    // The store, opened on a client, and what consumers ask of it.
    let _: fn(&str, &str, &str) -> Client = Client::new;
    let _: fn(Client) -> Result<Store> = Store::open;
    let _ = |client: Client| Store::with(client, Box::new(|| 0.0), Box::new(|_| false), "host");
    let _: fn(&Store) -> &Client = Store::client;
    let _: fn(&Store, &str) -> Result<Task> = Store::get;
    let _: fn(&Store, Option<&str>) -> Result<Vec<Task>> = Store::tasks;
    let _: fn(&Store, &str) -> Result<Vec<Message>> = Store::thread;
    let _: fn(&Store, &NewTask) -> Result<Task> = Store::create;
    let _: fn(&Store, &str, &str) -> Result<Task> = Store::claim;
    let _: fn(&Store, &str, &str, &str) -> Result<Task> = Store::take_role;
    let _: fn(&Store, &str, &str, &str) -> Result<Task> = Store::move_to;
    let _: fn(&Store, &str, &[&str], &str, &str) -> Result<Task> = Store::move_from;
    let _: fn(&Store, &str, &str) -> Result<Task> = Store::hold_blocked;
    let _: fn(&Store, &str, &str, &str, &str) -> Result<Task> = Store::verdict;
    let _: fn(&Store, &str, &str, &str, bool, &[&str]) -> Result<Landed> = Store::landed;
    let _: fn(&Store, &str, &str, &str, Option<&str>) -> Result<Message> = Store::say;
    let _: fn(&Store, &str, &str, &str, Option<&str>, &str) -> Result<Message> =
        Store::adopt_or_say;
    let _: fn(&Store, &str, &str, &str) -> Result<Task> = Store::mark_red;
    let _: fn(&Store, &str, &str) -> Result<Task> = Store::unmark_red;
    let _: fn(&Store, &str) -> Result<AssessmentState> = Store::assessment_state;
    let _: fn(&Store, &str, &str, &str, &str) -> Result<Task> = Store::record_waits;
    let _ = Store::record_waits_guarded;
    let _: fn(&Store, &NewSession) -> Result<Session> = Store::register;
    let _: fn(&Store, &str) -> Result<Session> = Store::session;
    let _: fn(&Store) -> Result<Vec<Session>> = Store::sessions;
    let _: fn(&Store, &str) -> Result<()> = Store::leave;
    let _: fn(&Store, &str, &Beat) -> Result<Option<String>> = Store::agent_beat;
    let _: fn(&Store, &Dump) -> Result<()> = Store::copy;
    let _: fn(&Store) -> Result<Dump> = Store::dump;
    let _: fn(Store, Option<usize>) -> Store = Store::capping;
    let _: fn(&Store, &str) -> Result<Vec<String>> = Store::ancestors;
    let _: fn(&Store, &str, Option<&str>, bool, bool) -> Result<()> = Store::callback;
    let _: fn(&Store, &str) -> Result<BTreeSet<String>> = Store::ever_executed;
    let _: fn(&Store, &str, &str, &str) -> Result<Task> = Store::hand_over;
    let _: fn(&Store, &str) -> Result<Vec<Message>> = Store::inbox;
    let _: fn(&Store, &str, Option<i64>) -> Result<Session> = Store::reattach;
    let _: fn(&Store, &NewLaunch) -> Result<TaskSessionRow> = Store::record_launch;
    let _: fn(&Store, &str, &str, Option<&str>) -> Result<Task> = Store::release;
    let _: fn(&Store, &str, &str, &str, Option<&str>, &str) -> Result<Message> = Store::say_keyed;
    let _: fn(&Store, &str, Option<&str>, &str) -> Result<Task> = Store::set_parent;
    let _: fn(&Store, &str) -> Result<Vec<Task>> = Store::tasks_for_branch;
    let _: fn(&Store, &str) -> Result<()> = Store::touch;
    let _ = |task: &Task| task.holders("orchestrator").len();
    // Matched by both its variants, and built by `Error::Db`.
    let _ = |e: Error| match e {
        Error::Refused(why) | Error::Db(why) => why,
    };
    let _: fn(String) -> Error = Error::Db;
    let _ = |e: DbError| e;
    let _: Option<&AssessmentReceipt> = None;

    // What consumers build by its fields: every field named, so one added here breaks them, or
    // the fields they name before filling the rest from `Default` or a constructor.
    let _: fn(&str, &str, &str) -> NewTask = NewTask::new;
    let _: fn(&str) -> NewSession = NewSession::new;
    let _: fn(&str, &str, &str) -> NewLaunch = NewLaunch::new;
    let _ = |worktree: String, branch: String, pid: Option<i64>| NewSession {
        worktree,
        branch,
        pid,
        ..NewSession::new("worker")
    };
    let _ = |branch: String| NewLaunch {
        branch,
        ..NewLaunch::new("slug", "author", "executor")
    };
    let _ = |slug: String, status: String, version: i64| AssessmentState {
        slug,
        status,
        version,
    };
    let _ = |text: String, since: f64, expires: Option<f64>| RoleRow {
        slug: text.clone(),
        handle: text.clone(),
        role: text,
        since,
        expires,
    };
    let _ = |text: String| LinkRow {
        slug: text.clone(),
        kind: text.clone(),
        target: text,
    };
    let _ = |id: i64, text: String, now: f64| TaskRow {
        id,
        slug: text.clone(),
        title: text.clone(),
        status: text.clone(),
        priority: 2,
        created_by: text,
        created: now,
        updated: now,
        ..Default::default()
    };
    let _ = |text: String, now: f64| SessionRow {
        handle: text.clone(),
        kind: text,
        connected: now,
        last_seen: now,
        ..Default::default()
    };
    let _ = |slug: &str| NewTask {
        body: String::new(),
        priority: 0,
        red: None,
        links: Vec::new(),
        ..NewTask::new(slug, "title", "by")
    };
    let _ = |id: i64, ts: f64| Message {
        id,
        slug: String::new(),
        sender: String::new(),
        recipient: None,
        kind: String::new(),
        text: String::new(),
        ts,
    };
    let _ = |name: String, outputs: String, command: Vec<String>| Generator {
        name,
        outputs,
        command,
        when_changed: false,
    };
    let _ = |expected: AssessmentState, verification: Option<String>| {
        let publication = Publication {
            id: String::new(),
            branch: String::new(),
            tip: String::new(),
            verification: verification.clone(),
        };
        let binding = Binding {
            id: String::new(),
            expected,
            signature: String::new(),
            decision: Decision {
                branch: publication.branch.clone(),
                task: String::new(),
                tip: publication.tip.clone(),
                actor: String::new(),
                record: String::new(),
                changes_requested: false,
                verification,
            },
        };
        (publication, binding)
    };
    let _: Option<&Bindings> = None;
    let _: fn(&serde_json::Value) -> String = assessment_publication::fingerprint;
    let _: fn(serde_json::Value) -> serde_json::Value = assessment_publication::seal;
    let _ = assessment_publication::unseal;
    let _: fn(&str) -> bool = assessment_publication::valid_id;
    let _ = assessment_publication::fence;

    // The board's words and folders.
    let _: [&str; 9] = STATUSES;
    let _: &str = alarm::CHANGES;
    fn waits(text: &str) -> Option<(&str, Vec<&str>)> {
        alarm::Waits::parse(text).map(|waits| (waits.tip, waits.on))
    }
    let _ = waits;
    let _: (f64, f64) = (health::LIVE_SECONDS, health::STALE_SECONDS);
    let _ = Health::Left;
    let _: &str = red::FILER;
    let _: fn(&str, &str) -> bool = red::filed_in;
    let _: fn(&str) -> bool = red::open;
    let _: &str = server::FOLDER;
    let _: Option<&server::Running> = None;
    let _: fn(Option<&str>, bool) -> bool = on_review::delivery_task;
    let _ = |branch: &str, found: &[(String, Option<String>, Option<String>)]| {
        slots::branch_task(
            branch,
            found.iter().map(|(slug, recorded, parent)| {
                (slug.as_str(), recorded.as_deref(), parent.as_deref())
            }),
        )
        .map(str::to_string)
    };

    // Processes, ports and worktrees.
    let _: usize = port::ATTEMPTS;
    let _ = |until: std::time::Instant| {
        port::start_on_free_port(port::ATTEMPTS, until, |addr| {
            let listening = port::listens(1, addr, sys::output).unwrap_or(false);
            Ok::<_, String>(if listening {
                port::Start::Up(addr.port())
            } else {
                port::Start::Ended(String::new())
            })
        })
        .map_err(|_: port::NotStarted<String>| ())
    };
    let _ = |command: &mut Command| {
        command.stdin(Stdio::null()).stdout(Stdio::piped());
    };
    let _ = |command: &mut std::process::Command| command.status_locked();
    let _ = |command: &mut std::process::Command| sys::Spawning::status_locked(command);
    let _ = |child: Child| child;
    let _ = |command: &mut Command| sys::output(command.arg("status"));
    let _: fn() -> String = sys::hostname;
    let _: fn(i64) -> bool = sys::pid_alive;
    let _ = (sys::pipe, sys::spawn_piped, sys::wait_output);
    let _: fn(&Path) -> std::result::Result<Vec<Listed>, String> = sweep::git::worktrees;
    let _: [&str; 18] = DISPOSABLE;
    let _: fn(&str, &dyn Fn(i64) -> bool, &dyn Fn(i64) -> Option<String>) -> Option<String> =
        sweep::process::dead_lock;
    let _: fn(i64) -> bool = sweep::process::pid_alive;
    let _ = |text: &str| sweep::process::parse_start(text);
    let _: fn(i64, &BTreeMap<String, String>) -> Option<String> = sweep::process::process_start;

    // The launch sweep a consumer's fixture runs, with a terminal host of its own.
    struct NoTerminals;
    impl Hc for NoTerminals {
        fn run(&self, _: &[&str]) -> std::result::Result<String, LaunchError> {
            Ok(String::new())
        }
        fn feed(&self, _: &[&str], _: &str) -> std::result::Result<String, LaunchError> {
            Err(LaunchError(String::new()))
        }
        fn app(&self, _: &[&str]) -> std::result::Result<String, LaunchError> {
            Err(LaunchError(String::new()))
        }
        fn caller(&self) -> Option<String> {
            None
        }
        fn board(&self) -> String {
            String::new()
        }
    }
    let _ = |store: &Store, state: PathBuf| {
        let sweep = Reap {
            state,
            apply: true,
            processes: &|_| unreachable!(),
            caller: None,
            min_idle: REAP_SECONDS,
        };
        let swept = reap::sweep(store, &NoTerminals, &sweep, &mut Vec::new());
        (swept.is_ok(), reap::lock_path(&sweep.state))
    };
}
