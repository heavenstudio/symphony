//! The board's public names that consuming projects' code calls, each in the shape it calls it: a
//! signature pinned by coercing the function to its pointer type, or a call written as a consumer
//! writes it (a few functions are named alone, their signatures left to the project, below), a
//! struct its code builds by its fields built by them all, a variant it builds, compares or matches
//! built, compared or matched as it does, a trait it implements implemented with the methods it
//! writes, a struct it builds by the fields it names and fills from `Default` or a constructor
//! built by those. Nothing it defines is called but `asserted`, which does nothing: it is compiled
//! by the test build and by `just lint-rs` (clippy over every target), so a change that would
//! break a consumer in what this file holds fails a task's gate rather than a run later. The
//! signature of a function named here alone, and a field it reads or a method it calls on a value
//! that is not named here, the project meets when it moves its pin, its own gate building it on
//! the new one; the other packages' guards are named in `crates/symphony/tests/consumer_api.rs`.
//!
//! Each enum whose variants are held here is also matched by every variant, with no `_` arm (an
//! enum named only as a type, as `port::NotStarted` is, or only by its constructors, as
//! `sys::Stdio` is, has no variant held and no such match). That match is this repository's own
//! choice, wider than a consumer's use unless it matches the enum whole, as one matches `Error`: a
//! tripwire, so a variant added fails this file and whoever adds it reads what the consumers do
//! with that enum. One that matches it whole breaks, and the variant waits for a consumer release
//! plan; otherwise the match takes the new variant. A variant that only the match names may be
//! renamed or removed along with it; one built, compared or matched beside it is a consumer's.
//!
//! A consumer takes a release by moving its pin in its own landing, and nothing here changes its
//! code, so a consumer's code never changes with a release: a name or signature here that a
//! consumer uses changes only with a consumer release plan (the consumer's code moved first,
//! against a release that still has the old name). New behaviour goes behind the name a consumer
//! calls, or beside it under a new one. A lint that asks for a signature pinned here to change is
//! answered by an `#[expect]` on the item, never by the change (`clippy.toml`). Names were read
//! from the consuming projects' code on 2026-11-12 and 2026-11-13 (every `symphony_board::` path
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
use symphony_board::recovery;
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
    // A consumer's fixture reads the store's PostgreSQL client by a refutable pattern, which its
    // lint (`-D warnings`) refuses once the pattern is irrefutable. `Database` is
    // `#[non_exhaustive]`, so the pattern stays refutable; it is matched here as that consumer
    // matches it, not by every variant (the header), as a variant added breaks no consumer.
    let _ = |store: &Store| {
        let symphony_board::Database::PostgreSql(client) = store.database() else {
            panic!("the board is on PostgreSQL");
        };
        client.database().len()
    };
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
    // The writes bound to a recovery generation, beside `Store::amend` and `Store::copy`, which
    // keep their shapes: the token as a program keeps it, by its fields and as JSON, and the
    // authority a deployer implements.
    let _: fn(&Store, &str, &str, &[String], &str) -> Result<Task> = Store::amend;
    let _: fn(Store, std::sync::Arc<dyn recovery::Authority>) -> Store = Store::recovering;
    let _: fn(&Store, &str) -> Result<recovery::BoundTask> = Store::bound;
    let _: fn(
        &Store,
        &str,
        &recovery::Bound,
        &str,
        &[String],
        &str,
    ) -> Result<recovery::BoundTask> = Store::amend_bound;
    // The bound edit, which rewrites a brief too, and where an integration keeps its state.
    let _: fn(
        &Store,
        &str,
        &recovery::Bound,
        &recovery::Edit,
        &str,
    ) -> Result<recovery::BoundTask> = Store::edit_bound;
    let _ = |title: Option<String>, body: Option<String>, links: Vec<String>| {
        let edit = recovery::Edit { title, body, links };
        (
            edit.clone() == recovery::Edit::default(),
            serde_json::to_string(&edit).unwrap(),
        )
    };
    let _: &str = symphony_board::folders::INTEGRATIONS;
    let _: fn(&Path, &str) -> std::result::Result<PathBuf, String> =
        symphony_board::folders::integration;
    let _: fn(&Store) -> Result<String> = Store::rebind;
    let _: fn(&Store) -> Result<Option<String>> = Store::recovery_generation;
    let _ = |read: recovery::BoundTask| {
        let recovery::BoundTask { bound, title, body } = read;
        let recovery::Bound {
            compat,
            generation,
            id,
            version,
        } = bound.clone();
        let kept = serde_json::to_string(&bound).unwrap();
        let again: recovery::Bound = serde_json::from_str(&kept).unwrap();
        (
            compat == recovery::COMPAT,
            generation,
            id,
            version,
            title,
            body,
            again == bound,
        )
    };
    struct Deployers;
    impl recovery::Authority for Deployers {
        fn bound(&self) -> std::result::Result<Option<String>, String> {
            Ok(None)
        }
        fn revoke(&self) -> std::result::Result<(), String> {
            Ok(())
        }
        fn publish(&self, _generation: &str) -> std::result::Result<(), String> {
            Ok(())
        }
    }
    let _: std::sync::Arc<dyn recovery::Authority> = std::sync::Arc::new(Deployers);
    let _: fn(&server::Home, &str) -> server::FileAuthority = server::Home::recovery;
    let _: fn(Store, &Env) -> Store = cli::recovery::recovering;
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
    // The board's error. A consumer builds both its variants and compares them in `assert_eq!`; it
    // matches each alone, the rest through a catch-all, and in one place matches it whole, by both
    // variants, as below: a variant added breaks it.
    let _: fn(String) -> Error = Error::Db;
    let _: fn(String) -> Error = Error::Refused;
    asserted::<Error>();
    let _ = |e: Error| match e {
        Error::Refused(why) | Error::Db(why) => why,
    };
    // A raw query's error. A consumer's fixture builds each of its three variants, clones them and
    // `assert_eq!`s them, and matches `Conflict` alone, taking the rest through a catch-all; the
    // match of every variant below is the header's tripwire.
    let _: fn(
        &Client,
        &str,
        serde_json::Value,
    ) -> std::result::Result<Vec<serde_json::Value>, DbError> = Client::query;
    let _ = |why: String| {
        [
            DbError::Conflict,
            DbError::Thrown(why.clone()),
            DbError::Failed(why),
        ]
    };
    let _ = |e: &DbError| e.clone();
    asserted::<DbError>();
    let _ = |e: &DbError| match e {
        DbError::Conflict | DbError::Thrown(_) | DbError::Failed(_) => {}
    };
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
    // A task built both ways a consumer builds one: by its slug, title, author and body over
    // `Default`, and over `NewTask::new` by every other field it names (its parent it also sets on
    // one built by `new`).
    let _ = |slug: String, by: String, body: String| NewTask {
        slug: slug.clone(),
        title: slug,
        by,
        body,
        ..Default::default()
    };
    let _ = |slug: &str, status: String, parent: Option<String>| NewTask {
        body: String::new(),
        priority: 0,
        status,
        parent,
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
    // `Left` compared in `assert_eq!` with a session's health.
    let _ = Health::Left;
    asserted::<Health>();
    let _ = |health: Health| match health {
        Health::Live | Health::Stale | Health::Unknown | Health::Lost | Health::Left => {}
    };
    let _: &str = red::FILER;
    let _: fn(&str, &str) -> bool = red::filed_in;
    let _: fn(&str) -> bool = red::open;
    let _: &str = server::FOLDER;
    // A board's home in a consumer's fixtures: by its folder and the SurrealDB binary an earlier
    // release served it by, which nothing reads now, and by its folder alone, which replaces it.
    let _: fn(&Path, &Path) -> server::Home = server::Home::new;
    let _: fn(&Path) -> server::Home = server::Home::at;
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
    // A start's two variants built just above, as a consumer's fixture builds them; matched whole.
    let _ = |start: port::Start<u16>| match start {
        port::Start::Up(_) | port::Start::Ended(_) => {}
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

/// A type a consumer compares in `assert_eq!`, which prints both sides when they differ.
fn asserted<T: PartialEq + std::fmt::Debug>() {}

#[test]
fn the_shared_ordering_helpers_keep_stable_ties_for_consuming_readers() {
    let mut rows = [(2, "first"), (1, "middle"), (2, "last")];
    symphony_board::order::sort_by_key(&mut rows, &|row| row.0);
    assert_eq!(rows, [(1, "middle"), (2, "first"), (2, "last")]);
    symphony_board::order::sort_by(&mut rows, &|a, b| b.0.cmp(&a.0));
    assert_eq!(rows, [(2, "first"), (2, "last"), (1, "middle")]);
}

/// The execution attempts a host runs (`symphony_board::execution`), in the shapes its host code
/// calls: no consumer calls them yet, and they are pinned from their first release, so a change
/// to one waits for a consumer release plan as every name above does. `Refusal`, `Worked`,
/// `Collected`, `Act`, `Admission`, `Attempt`, `Pacing` and `Eligibility` are `#[non_exhaustive]`:
/// a variant or a field added is no break, so a host's match keeps a `_` arm.
#[test]
fn the_execution_coordinator_keeps_the_shapes_a_host_calls() {
    use symphony_board::execution::{
        Act, Adapter, Admission, Artifact, Attempt, Cleanup, Collected, Collection, Continuation,
        Coordinator, Eligibility, ExternalLink, Grant, Input, Pacing, Principal, Profile, Recorded,
        Refusal, Scope, StopMode, Worked,
    };
    type Answer<T> = std::result::Result<T, Refusal>;

    /// Each method of a coordinator borrowing for `'a`.
    fn of<'a>(_: std::marker::PhantomData<&'a ()>) {
        type C<'a> = Coordinator<'a>;
        let _: fn(&'a Store, Profile, &'a dyn Adapter, Grant) -> Answer<C<'a>> = C::<'a>::new;
        let _: fn(C<'a>, Pacing) -> Answer<C<'a>> = C::<'a>::with_pacing;
        let _: fn(C<'a>, &str, u32) -> C<'a> = C::<'a>::with_capacity;
        let _: fn(&C<'a>, &Grant, Admission) -> Answer<Attempt> = C::<'a>::admit;
        let _: fn(&C<'a>, &Grant, &str, Continuation, Option<&str>) -> Answer<Attempt> =
            C::<'a>::resume;
        let _: fn(&C<'a>, &Grant, &str, &str, Option<&str>) -> Answer<Attempt> = C::<'a>::retry;
        let _: fn(&C<'a>, &Grant, &str) -> Answer<Attempt> = C::<'a>::cancel;
        let _: fn(&C<'a>, &Grant, &str, StopMode) -> Answer<Attempt> = C::<'a>::stop;
        let _: fn(&C<'a>, &Grant, &str) -> Answer<ExternalLink> = C::<'a>::external_link;
        let _: fn(&C<'a>, &Grant, &str) -> Answer<Attempt> = C::<'a>::attempt;
        let _: fn(&C<'a>, &Grant, &str) -> Answer<Vec<Attempt>> = C::<'a>::history;
        let _: fn(&C<'a>, &Grant, &str) -> Answer<Vec<Recorded>> = C::<'a>::facts;
        let _: fn(&C<'a>, &Grant, &str) -> Answer<Vec<Artifact>> = C::<'a>::artifacts;
        let _: fn(&C<'a>, &Grant, &str, &str) -> Answer<Eligibility> = C::<'a>::eligibility;
        let _: fn(&C<'a>, &Grant, &str) -> Answer<Attempt> = C::<'a>::begin_collection;
        let _: fn(&C<'a>, &Grant, &str, Artifact) -> Answer<Collected> = C::<'a>::record_artifact;
        let _: fn(&C<'a>, &Grant, &str, Collection, Option<&str>) -> Answer<Attempt> =
            C::<'a>::end_collection;
        let _: fn(&C<'a>, &Grant, &str) -> Answer<String> = C::<'a>::begin_cleanup;
        let _: fn(&C<'a>, &Grant, &str, &str, Cleanup, Option<&str>) -> Answer<Attempt> =
            C::<'a>::end_cleanup;
        let _: fn(&C<'a>, &str) -> Answer<Worked> = C::<'a>::step;
        let _: fn(&C<'a>, usize) -> Answer<Vec<(String, Answer<Worked>)>> = C::<'a>::run_due;
    }
    of(std::marker::PhantomData);
    let _: fn(&Store, &str) -> Result<Vec<String>> = Store::live_attempts;

    // Its policy revision is any text, as a host names it.
    let _ = |principal: Principal, scope: Scope| -> Grant {
        Grant::from_host(principal, scope, &[Act::Start, Act::Read], "policy-1")
    };
    let _: fn(Grant, &[&str]) -> Grant = Grant::on_tasks;
    let _: fn(&str, &str, &str, Input) -> Admission = Admission::new;
    let _: fn(Admission, &str) -> Admission = Admission::handle;
    let _: fn(Admission, &str) -> Admission = Admission::request_key;
    let _ = |pacing: &mut Pacing| {
        pacing.lease = std::time::Duration::from_secs(60);
        pacing.unknowns = 8;
    };
    asserted::<Attempt>();
    asserted::<Refusal>();
    asserted::<Worked>();
}
