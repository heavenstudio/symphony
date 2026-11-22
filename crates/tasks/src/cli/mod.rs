//! The `tasks` command line, over the store: each command of `just tasks`
//! ([`commands`]), read as its argparse parser read them ([`args`]), and `api`, the seam the Python
//! readers call the board through ([`api`]).

pub mod agent_send;
pub mod agents;
mod api;
mod args;
pub mod attach;
mod beat;
mod commands;
pub(crate) mod database;
mod detached;
mod field;
pub mod machine_work;
mod migrate;
pub mod probes;
pub mod pull;
pub(crate) mod recorder;
pub mod recovery;
mod schema;
mod search;
mod shape;
mod snapshot;
mod status;
mod sweep;
pub(crate) mod text;
mod usage;

use crate::maps::Collect as _;
use crate::rpc::Client;
use crate::server::Home;
use crate::{DATABASE, NAMESPACE, Store, sys};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// What a run reads from where it runs: its environment and working directory.
#[derive(Debug, Clone, Default)]
pub struct Env {
    pub vars: BTreeMap<String, String>,
    pub cwd: PathBuf,
}

impl Env {
    /// This process's own: each variable whose name and value are text (a byte that is not ends
    /// no command here, as `std::env::vars` would).
    pub fn of_process() -> Env {
        Env {
            vars: std::env::vars_os()
                .filter_map(|(name, value)| {
                    Some((name.into_string().ok()?, value.into_string().ok()?))
                })
                .collect_map(),
            cwd: std::env::current_dir().unwrap_or_default(),
        }
    }

    pub(crate) fn var(&self, name: &str) -> Option<&str> {
        self.vars
            .get(name)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    /// Whether what the switch `name` guards may start here: `on` or `off` says; else only on the
    /// repository's own board, which no `TASKS_SERVER` names (a test's starts none). The one rule
    /// of every command a board write starts detached: the review and land commands, the
    /// dispatch, the regenerator and the sweep.
    pub(crate) fn switch(&self, name: &str) -> bool {
        match self.var(name) {
            Some("off") => false,
            Some("on") => true,
            _ => self.var("TASKS_SERVER").is_none(),
        }
    }

    /// The deadline `name` sets in seconds (a fraction or none), else `fallback`: how a test gives
    /// a product's deadline the time a loaded machine needs, or makes it run out at once, without
    /// changing it for anyone else. A value that is no such number leaves `fallback`.
    pub(crate) fn seconds(&self, name: &str, fallback: Duration) -> Duration {
        self.var(name)
            .and_then(|text| text.trim().parse::<f64>().ok())
            .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok())
            .unwrap_or(fallback)
    }

    /// One of Symphony's variables, by [`Env::var`]'s rule: an empty value is unset.
    pub(crate) fn read(&self, var: symphony_env::Var) -> Option<&str> {
        self.var(var.name)
    }
}

/// Board-state writes, including JSON calls, act as agents. Derived-cache maintenance has no
/// agent identity; owner saves call the store directly.
fn check_actor(actor: &str) -> Result<(), crate::Error> {
    if actor == "owner" {
        return Err(crate::Error::Refused(
            "the owner acts from the Symphony; use a registered agent handle on the command line"
                .into(),
        ));
    }
    Ok(())
}

/// `tasks ARGS`: what it says goes to `out` and `err`; the exit code is returned. Each write makes
/// its callback dispatch before it answers, as the library's stores do.
pub fn main(
    args: &[String],
    env: &Env,
    input: &mut dyn Read,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    run(args, env, input, out, err, None)
}

/// `tasks ARGS` as the `tasks` binary at `program` runs them: as [`main`], but each write hands its
/// callback dispatch to a detached `program dispatch` it starts and answers once it has committed
/// ([`detached`]); one that cannot start it dispatches itself.
pub fn main_detaching(
    args: &[String],
    env: &Env,
    input: &mut dyn Read,
    out: &mut dyn Write,
    err: &mut dyn Write,
    program: &Path,
) -> i32 {
    run(args, env, input, out, err, Some(program))
}

/// A `tasks` executable's whole `main`: this process's arguments, environment and standard streams,
/// run as [`main_detaching`] with this executable as the dispatcher it starts, or as [`main`] when
/// it cannot name itself. Every build of `tasks` is this one call (this crate's binary and the
/// project's board-only one, which the live board runs), so no build leaves its writes' delivery
/// in the command.
pub fn process_main() -> ExitCode {
    let words: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let env = Env::of_process();
    let (mut input, mut out, mut err) = (std::io::stdin(), std::io::stdout(), std::io::stderr());
    // First, with no other thread running yet: a recorded command's words are another program's,
    // whatever bytes they hold, and its recorder may fork this process ([`recorder`]).
    if words.first().is_some_and(|word| word == recorder::COMMAND) {
        let code = recorder::run(&words[1..], &env, &mut out, &mut err, true);
        return ExitCode::from(u8::try_from(code).unwrap_or(2));
    }
    let Ok(args) = words
        .into_iter()
        .map(std::ffi::OsString::into_string)
        .collect::<Result<Vec<String>, _>>()
    else {
        let _ = writeln!(err, "tasks: an argument is not UTF-8 text");
        return ExitCode::from(2);
    };
    let code = match std::env::current_exe() {
        Ok(program) => main_detaching(&args, &env, &mut input, &mut out, &mut err, &program),
        Err(_) => main(&args, &env, &mut input, &mut out, &mut err),
    };
    // A land a signal stopped has cleaned up and said why: it ends by that signal, so whatever ran
    // it (`just`, a shell) stops too, as with any command a signal ends.
    if let Some(signal) = crate::land::interrupted() {
        let _ = out.flush();
        let _ = err.flush();
        crate::sys::end_by(signal);
    }
    ExitCode::from(u8::try_from(code).unwrap_or(2))
}

fn run(
    args: &[String],
    env: &Env,
    input: &mut dyn Read,
    out: &mut dyn Write,
    err: &mut dyn Write,
    program: Option<&Path>,
) -> i32 {
    // A plain-word spelling for scratch runs, also accepted before `api`. It is the exact
    // TASKS_DATABASE override, without a shell environment prefix or a second board server; the
    // last of several leading ones wins.
    let mut lead = 0;
    let mut scratch = None;
    while args.get(lead).is_some_and(|arg| arg == "--database") {
        let Some(database) = args.get(lead + 1).filter(|name| crate::db::plain(name)) else {
            let _ = writeln!(err, "--database needs a plain database name");
            return 2;
        };
        let scratch = scratch.get_or_insert_with(|| env.clone());
        scratch
            .vars
            .insert("TASKS_DATABASE".into(), database.clone());
        if database != DATABASE {
            scratch.vars.insert("TASKS_SWEEP".into(), "off".into());
        }
        lead += 2;
    }
    let env = scratch.as_ref().unwrap_or(env);
    let args = &args[lead..];
    if symphony_args::version(args, "tasks", env!("CARGO_PKG_VERSION"), out) {
        return 0;
    }
    if args.len() == 1 && args[0] == detached::COMMAND {
        return detached::run(env, err);
    }
    // Before the board's argument table: what follows `--` is another program's command line. Only
    // here, first after any `--database NAME`, which the `tasks` recipe `symphony init` installs
    // refuses in the same spelling (`shipped/symphony.just`): it runs any command, and the allow
    // rules pass `just tasks` unasked. Any other word before it is the table's, which has no `run`.
    if args.first().is_some_and(|arg| arg == recorder::COMMAND) {
        let words: Vec<std::ffi::OsString> = args[1..].iter().map(Into::into).collect();
        return recorder::run(&words, env, out, err, program.is_some());
    }
    // Before the board's argument table too, as `run` is: what alone moves a live board whose main
    // checkout holds no board source.
    if args.first().is_some_and(|arg| arg == migrate::COMMAND) {
        return migrate::run(&args[1..], env, out, err);
    }
    // Before the board's argument table, and without opening the board: what holds its database
    // stopped never starts it.
    if args.first().is_some_and(|arg| arg == database::COMMAND) {
        return database::run(&args[1..], env, out, err);
    }
    // Before the board's argument table too: its revoke opens no board, as it comes before
    // restored data is served.
    if args.first().is_some_and(|arg| arg == recovery::COMMAND) {
        return recovery::run(&args[1..], env, out, err);
    }
    let board = || connect(env).map(|store| detaching(store, env, program));
    // The api's store answers to the board home's recovery authority, so its bound calls are
    // taken; every other command makes none.
    let bound = || board().map(|store| recovery::recovering(store, env));
    if args.len() == 1 && args[0] == "api" {
        return api::run_in(input, out, bound, Some(env));
    }
    if args.len() == 2 && args[0] == "api" && args[1] == "--lines" {
        return api::serve_in(&mut std::io::BufReader::new(input), out, bound, Some(env));
    }
    // Every line the parse suggests begins with the command that reaches this board, however its
    // server and database were chosen, so typed again in any shell it reaches the same board.
    let parsed = match args::parse_after(&board_of(env), args) {
        args::Outcome::Run(parsed) => parsed,
        args::Outcome::Help(command) => {
            let _ = write!(out, "{}", usage::shown(command, env));
            return 0;
        }
        args::Outcome::Usage(text) => {
            let _ = write!(err, "{text}");
            return 2;
        }
    };
    if parsed.command == "beat" {
        return beat::run(input, env);
    }
    if parsed.command == "field" {
        return field::run(&parsed, env, out, err);
    }
    if parsed.command == "sweep" {
        return sweep::run(&parsed, env, out, err);
    }
    if parsed.command == "status" {
        return status::run(&parsed, env, out, err);
    }
    if parsed.command == "schema" {
        return schema::run(&parsed, env, out, err);
    }
    if parsed.command == "snapshot" {
        return snapshot::run(&parsed, env, out, err);
    }
    let reached = Clock::of(env).and_then(|clock| {
        let store = looking_up(open(reach(env, &env.cwd, true)?, &clock)?, env);
        let store = capped_saying(store, env, &mut |problem| {
            let _ = writeln!(err, "⚠ {problem}");
        });
        Ok((detaching(store, env, program), clock))
    });
    let (store, clock) = match reached {
        Ok(reached) => reached,
        Err(why) => {
            let _ = writeln!(err, "✖ {why}");
            return 2;
        }
    };
    let mut context = commands::Context {
        parsed: &parsed,
        env,
        input,
        out,
        err,
        clock: &clock,
        program,
    };
    let (code, said) = match commands::run(&store, &mut context) {
        Ok(()) => return 0,
        Err(commands::Stop::NoSession) => (
            2,
            format!(
                "no session: pass --as HANDLE or set TASKS_SESSION (get one with `{} register`)",
                store.board_command()
            ),
        ),
        Err(commands::Stop::Refused(why)) => (1, with_nearest_slug(&store, why)),
        Err(commands::Stop::Board(why)) => (2, why),
    };
    let _ = writeln!(context.err, "✖ {said}");
    code
}

/// The command that reaches the board `env` names, spelled as that board's refusals spell it
/// ([`crate::store::board_command`]): the server `TASKS_SERVER` names, and the database
/// `--database` or `TASKS_DATABASE` chose when it is not the project's, each as a shell reads it
/// back. Known before the board is reached: what a usage error's line begins with.
fn board_of(env: &Env) -> String {
    let server = env.var("TASKS_SERVER").map(args::shell_word);
    let database = args::shell_word(env.var("TASKS_DATABASE").unwrap_or(DATABASE));
    crate::store::board_command(server.as_deref(), &database)
}

/// The most slugs a refusal names as nearest: past it, a search finds the task better.
const NEAREST_SLUGS: usize = 3;

/// `why`, a refusal; when it is that the board has no task by a slug, with the board's slugs nearest
/// to it (no more than [`NEAREST_SLUGS`]), else the commands that find one, on this board. Only the
/// slugs are read, never the tasks they name.
fn with_nearest_slug(store: &Store, why: String) -> String {
    // The slug as `repr` quotes it: in single quotes, or in double ones when it holds a single one.
    let quoted = |open: char| {
        why.strip_prefix(crate::Error::NO_TASK)
            .and_then(|rest| rest.strip_prefix(open))
            .and_then(|rest| rest.strip_suffix(open))
            .filter(|slug| !slug.contains(open) && !slug.contains('\\'))
    };
    let Some(slug) = quoted('\'').or_else(|| quoted('"')) else {
        return why;
    };
    let slugs = store.task_slugs(None).unwrap_or_default();
    let near: Vec<String> = symphony_args::nearest(slug, slugs.iter().map(String::as_str))
        .into_iter()
        .map(crate::quote::repr)
        .collect();
    let board = store.board_command();
    match near.as_slice() {
        [one] => format!("{why}; did you mean {one}?"),
        [rest @ .., last] if near.len() <= NEAREST_SLUGS => {
            format!("{why}; did you mean {} or {last}?", rest.join(", "))
        }
        _ => format!(
            "{why}: `{board} search \"WORDS\"` finds a task by its words, `{board} list --all` \
             lists every one"
        ),
    }
}

/// The board as `env` reaches it: the server `TASKS_SERVER` names, in database `TASKS_DATABASE`, else
/// the one in the git common dir of the working directory, started if it is not running; its clock
/// pinned at `TASKS_NOW` if that is set; its starts held to the cap the project sets ([`capped`]).
pub fn connect(env: &Env) -> Result<Store, String> {
    let store = open(reach(env, &env.cwd, true)?, &Clock::of(env)?)?;
    Ok(capped(looking_up(store, env), env))
}

/// As [`connect`], following no workflow ([`following`]): `agent-send`'s, whose message moves no
/// task, so the program that sends it links none of the workflow, its parser or its git reading.
/// The next write a `tasks` command makes follows what the branch commits.
pub(super) fn connect_unfollowing(env: &Env) -> Result<Store, String> {
    let reached = reach(env, &env.cwd, true)?;
    let database = reached.database();
    let (store, _) = opened(reached, database, &Clock::of(env)?, crate::Opening::write())?;
    Ok(capped(looking_up(store, env), env))
}

/// `store`, each read of its unanswered-post lookup waiting [`crate::UNANSWERED_WAIT`] at most, or
/// what `TASKS_UNANSWERED_WAIT` sets ([`Env::seconds`]), and sent once: a command does without the
/// posts rather than wait minutes on them.
fn looking_up(store: Store, env: &Env) -> Store {
    store.looking_up_within(env.seconds(crate::UNANSWERED_WAIT_VAR, crate::UNANSWERED_WAIT))
}

/// `store`, held to the limits the main checkout of `env`'s repository writes ([`crate::limits`]):
/// the cap on each orchestrator's open tasks, the low-volume threshold, the agents a machine runs
/// and the pipeline alarm's thresholds, as its `symphony.toml` is at each read, so a store kept
/// open follows a change to the file. A value that cannot be read is said on stderr and takes its
/// default: a broken project file must not stop the board, nor the landing that mends it. Outside
/// a repository, every default.
pub fn capped(store: Store, env: &Env) -> Store {
    capped_saying(store, env, &mut |problem| eprintln!("tasks: {problem}"))
}

/// As [`capped`], what is wrong with the file as it is now said by `say`; a later version of it,
/// read again by the store, on stderr ([`crate::limits::Watched`]).
fn capped_saying(store: Store, env: &Env, say: &mut dyn FnMut(&str)) -> Store {
    let Some(path) = crate::limits::file_of(env) else {
        return store.limiting(crate::limits::Limits::default());
    };
    let (watched, read) = crate::limits::Watched::new(path);
    for problem in &read.problems {
        say(problem);
    }
    store.watching(watched)
}

/// `store`, its writes handing their dispatch to a detached `program dispatch` when a `program` runs,
/// unless `TASKS_DISPATCH=inline` asks each write to dispatch before it answers (a test that reads
/// what the command delivered once it has ended).
fn detaching(store: Store, env: &Env, program: Option<&Path>) -> Store {
    match program {
        Some(program) if env.var(detached::INLINE.0) != Some(detached::INLINE.1) => {
            store.dispatching_by(detached::hand_off(env, program))
        }
        _ => store,
    }
}

/// The time a run reads: this machine's, or the one `TASKS_NOW` pins, which only a monitor's own waits
/// move (each a step a test takes: see [`Clock::wait`]).
pub enum Clock {
    Machine,
    Pinned(Arc<Mutex<f64>>),
}

impl Clock {
    fn of(env: &Env) -> Result<Clock, String> {
        let Some(text) = env.var("TASKS_NOW") else {
            return Ok(Clock::Machine);
        };
        let now = text
            .parse::<f64>()
            .ok()
            .filter(|now| now.is_finite())
            .ok_or_else(|| format!("TASKS_NOW={text} is not a time in seconds"))?;
        Ok(Clock::Pinned(Arc::new(Mutex::new(now))))
    }

    /// The time now, in seconds since the epoch.
    fn now(&self) -> f64 {
        match self {
            Clock::Pinned(now) => *now.lock().unwrap_or_else(|held| held.into_inner()),
            Clock::Machine => crate::store::now(),
        }
    }

    /// What the machine's clock sleeps for a wait of `seconds`: none for a negative span or no
    /// number.
    pub fn span(seconds: f64) -> Duration {
        Duration::try_from_secs_f64(seconds).unwrap_or_default()
    }

    /// Wait `seconds` ([`Clock::span`]). A pinned clock moves on by them instead, says `STEP <now>` on
    /// `err`, and reads a line on `input` (or its end) before going on, so a test acts between a
    /// monitor's looks.
    pub fn wait(&self, seconds: f64, input: &mut dyn Read, err: &mut dyn Write) {
        match self {
            #[expect(
                clippy::disallowed_methods,
                reason = "no-timed-waits baseline: the board's monitor looks again each period, behind the clock its tests replace"
            )]
            Clock::Machine => std::thread::sleep(Clock::span(seconds)),
            Clock::Pinned(now) => {
                let at = {
                    let mut now = now.lock().unwrap_or_else(|held| held.into_inner());
                    *now += seconds;
                    *now
                };
                let _ = writeln!(err, "STEP {at:?}");
                let _ = err.flush();
                let mut byte = [0u8; 1];
                while matches!(input.read(&mut byte), Ok(1)) && byte[0] != b'\n' {}
            }
        }
    }
}

/// The clock since boot a run reads (a hook, for a subagent's binding): this machine's
/// ([`sys::boot_clock`]), or the reading `TASKS_BOOT_CLOCK` pins, `BOOT-ID SECONDS` (anything else,
/// as `none`: no reading), as `TASKS_NOW` pins the board's clock.
fn boot_clock(env: &Env) -> Option<sys::Boottime> {
    let Some(pinned) = env.var("TASKS_BOOT_CLOCK") else {
        return sys::boot_clock();
    };
    let (boot, at) = pinned.split_once(' ')?;
    let at: f64 = at.parse::<f64>().ok().filter(|at| at.is_finite())?;
    (!boot.is_empty()).then(|| sys::Boottime {
        boot: boot.into(),
        at,
    })
}

/// The board `env` reaches, its server started if it is down: what `tasks-copy --board` reads and
/// writes, opening its store as the live board it is ([`Reached::opening`]), so one that went back
/// is refused it. A process Cargo runs from the live board's repository, or one it runs under, is
/// refused it before its server is started ([`crate::live::cargo_refusal`]); the store each opens
/// on it judges whether it may move it ([`crate::live`]).
pub fn writable_board(env: &Env) -> Result<Reached, String> {
    reach_board(env, &env.cwd, Reaching::Client, None)
}

/// The client of the board [`writable_board`] reaches, for a caller that opens its own store on it
/// ([`crate::Store::open`]): kept, with this signature, for consumers' code, which a release never
/// changes (a consumer moves its pin by its own landing). New code uses [`writable_board`] and
/// opens its store by [`Reached::opening`]. A store opened on a bare client names no live board, so
/// a live board that went back below its record is refused here instead, in the rollback's words,
/// before the client is returned, its schema read once when there is a record, as
/// [`open_unchecked`] does ([`crate::live::seen::checked`]).
pub fn board_client(env: &Env) -> Result<Client, String> {
    let reached = writable_board(env)?;
    not_gone_back(&reached)?;
    Ok(reached.client)
}

/// A board a run reaches, and the id of the project it is the board of: none for a server
/// `TASKS_SERVER` names, which is no project's.
pub struct Reached {
    pub client: Client,
    pub project: Option<String>,
    /// The project's live board this is ([`crate::live`]), none for a scratch board.
    pub live: Option<crate::live::Live>,
}

impl Reached {
    /// A client of this board's selected engine. The existing `client` remains available for
    /// consumers that name SurrealDB alone; new readers use this database-neutral entry.
    pub fn database(&self) -> crate::Database {
        named_database(
            Client::new(
                self.client.addr(),
                self.client.namespace(),
                self.client.database(),
            )
            .waiting(self.client.wait_limit()),
        )
    }

    /// `how`, naming this board's project and opening it as the live board it is, if it is one: a
    /// store opened by it refuses a live board that went back ([`crate::live::seen`]).
    pub fn opening(&self, how: crate::Opening) -> crate::Opening {
        how.of(self.project.as_deref()).live(self.live.clone())
    }
}

/// The board of the project `env`'s working directory is in ([`crate::project::find`]), its
/// server started if it is down, or the one `TASKS_SERVER` and `TASKS_DATABASE` name, as
/// [`writable_board`] reaches it: what the Symphony reads a project's board from. A process Cargo
/// runs from that project's repository is refused its live board before the server is started. A
/// working directory that is gone is refused ([`crate::project::at`]), never walked up from.
pub fn board(env: &Env) -> Result<Reached, String> {
    reach_board(env, &env.cwd, Reaching::Read, None)
}

/// The board [`board`] reaches, held to the repository whose common dir is `repository`: the one
/// the Symphony found from `env`'s working directory when it began to serve that folder, none for a
/// folder no repository held then. Refused, as a folder that is gone, when the lookup finds another
/// ([`crate::project::held`]), and never started there: the board is the one in the common dir that
/// check read, so no check before the reach is needed.
pub fn held_board(env: &Env, repository: Option<&Path>) -> Result<Reached, String> {
    reach_board(env, &env.cwd, Reaching::Read, repository)
}

/// The board [`board`] reaches, refused rather than started when its server is not running: what a
/// read that can go without the board asks for.
pub fn running_board(env: &Env) -> Result<Reached, String> {
    reach_board(env, &env.cwd, Reaching::Peek, None)
}

/// The board [`running_board`] reaches, held to `repository` as [`held_board`] holds its reach:
/// refused, as a folder that is gone, when the lookup finds another repository, and refused rather
/// than started when its server is not running.
pub fn held_running_board(env: &Env, repository: Option<&Path>) -> Result<Reached, String> {
    reach_board(env, &env.cwd, Reaching::Peek, repository)
}

/// The board `env` reaches from `cwd`; a server that is not running is started if `start`, else
/// refused.
fn reach(env: &Env, cwd: &Path, start: bool) -> Result<Reached, String> {
    let how = if start {
        Reaching::Command
    } else {
        Reaching::Running
    };
    reach_board(env, cwd, how, None)
}

/// How a run reaches its board.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reaching {
    /// A command's: its server started if it is down.
    Command,
    /// A command's that starts no server: refused when none is recorded.
    Running,
    /// The Symphony's, to read: its server started if it is down.
    Read,
    /// The Symphony's, to read what it can go without: refused when no server is recorded.
    Peek,
    /// A client for a store its caller opens: started if it is down.
    Client,
}

/// How long the Symphony's read waits on another's start of the board's server (the starters'
/// lock) before it is refused, naming that start: a start without a replay takes about a second.
const READ_WAIT: Duration = Duration::from_secs(5);

/// The machine folder `env` names ([`crate::project::machine_folder`]), if it names one.
pub(crate) fn machine(env: &Env) -> Option<PathBuf> {
    crate::project::machine_folder(
        |name| env.var(name).map(std::ffi::OsString::from),
        cfg!(target_os = "macos"),
    )
    .ok()
}

/// The board of the project `cwd` is in, in that project's common dir: for the Symphony's reaches,
/// the project whose folder `cwd` is, refused when it is gone ([`crate::project::at`]). Every
/// reach of a live board is refused to a process Cargo runs from that board's
/// repository, or one it runs under, before its server is started
/// ([`crate::live::cargo_refusal`]). A Symphony's reach `held` to a repository is refused any other
/// ([`crate::project::held`]).
fn reach_board(
    env: &Env,
    cwd: &Path,
    how: Reaching,
    held: Option<&Path>,
) -> Result<Reached, String> {
    let db = env.var("TASKS_DATABASE").unwrap_or(DATABASE);
    if !crate::db::plain(db) {
        return Err(format!(
            "TASKS_DATABASE={db:?} is not a plain name for a database"
        ));
    }
    let guard = |live: &Option<crate::live::Live>| match live {
        Some(live) => match crate::live::cargo_refusal(live, crate::live::cargo_here()) {
            Some(why) => Err(why),
            None => Ok(()),
        },
        None => Ok(()),
    };
    if let Some(addr) = env.var("TASKS_SERVER") {
        socket_named(addr, "TASKS_SERVER=")?;
        let live = crate::live::named_server(addr, db, cwd, machine(env).as_deref());
        guard(&live)?;
        return Ok(Reached {
            client: Client::new(addr, NAMESPACE, db),
            project: None,
            live,
        });
    }
    // The Symphony reaches a project's board from the project's folder, which may have been removed
    // under it; a command looks from where it runs.
    let project = if let Some(repository) = held {
        crate::project::held(cwd, machine(env).as_deref(), repository)?
    } else if matches!(how, Reaching::Read | Reaching::Peek) {
        crate::project::at(cwd, machine(env).as_deref())?
    } else {
        crate::project::find(cwd, machine(env).as_deref())?
    };
    let common = &project.place.common;
    let live = (db == DATABASE).then(|| crate::live::Live {
        project: project.id.clone(),
        common: common.clone(),
    });
    guard(&live)?;
    let mut home = Home::in_common(common, env.read(symphony_env::SURREAL).map(Path::new));
    if how == Reaching::Read {
        // The Symphony's thread that reads the board serves its pages too: it is refused, naming
        // the start under way, rather than wait out a replay of minutes, another's or its own,
        // which runs on a thread of its own.
        home = home.waiting_at_most(READ_WAIT).starting_apart();
    }
    // Held, every reach is refused naming the hold, one that would only read the record too.
    if let Some(hold) = home.held() {
        return Err(crate::server::ServerError::Held(hold).to_string());
    }
    // The Symphony's read starts a board's server but never makes a board: one not made yet is
    // refused in a reader's words for it, which say the project has no board
    // ([`crate::board_health::absent`]), and nothing is made or started.
    let unmade = home.unmade().map_err(|error| error.to_string())?;
    if how == Reaching::Read && unmade {
        let named = crate::schema::Named::of(Some(&project.id));
        return Err(format!(
            "{}{}: nothing was made or started; `just tasks list` in {} makes it, on PostgreSQL",
            named.board,
            crate::schema::NOT_MADE,
            named.project
        ));
    }
    let start = !matches!(how, Reaching::Running | Reaching::Peek);
    let running = home
        .board_server(&project.place.root, start)
        .map_err(|error| match error {
            // Where the board's server comes from is the project's board guide's to say, and what
            // a board not made yet is made on, which its start could not make it on.
            crate::server::ServerError::NoSurreal(_)
            | crate::server::ServerError::Unserved { .. } => {
                format!("{error} ({}#where-it-lives)", project.guide())
            }
            error if unmade && start => format!("{error} ({}#where-it-lives)", project.guide()),
            error => error.to_string(),
        })?;
    Ok(Reached {
        client: Client::new(&running.addr, NAMESPACE, db),
        project: Some(project.id),
        live,
    })
}

/// `addr`, when it names a PostgreSQL server (`postgresql:`), names its socket's folder by an
/// absolute path: refused otherwise, `what` saying where the address was given.
fn socket_named(addr: &str, what: &str) -> Result<(), String> {
    match addr.strip_prefix("postgresql:") {
        Some(socket)
            if !Path::new(socket).is_absolute()
                || socket.contains('\0')
                || socket.starts_with("//") =>
        {
            Err(format!(
                "{what}{addr:?} must name an absolute PostgreSQL socket folder as postgresql:/path"
            ))
        }
        _ => Ok(()),
    }
}

/// The database `db` on the server at `addr`, SurrealDB's `HOST:PORT` or PostgreSQL's
/// `postgresql:/SOCKET`, which `what` names in a refusal: what a program handed a server's
/// address opens its store on.
pub(crate) fn database_at(addr: &str, db: &str, what: &str) -> Result<crate::Database, String> {
    socket_named(addr, &format!("{what} "))?;
    Ok(named_database(Client::new(addr, NAMESPACE, db)))
}

/// A named scratch server can be a PostgreSQL socket folder. Repository discovery still
/// reaches the existing server; no configured or live board is switched by this adapter.
fn named_database(client: Client) -> crate::Database {
    match client.addr().strip_prefix("postgresql:") {
        Some(socket) => crate::pg::Client::new(Path::new(socket), client.database())
            .waiting(client.wait_limit())
            .into(),
        None => client.into(),
    }
}

/// The board `reached`, to read and write by this build's schema, read by `clock`: a newer board
/// is refused before any write, naming its project, and an older live one moved forward only as
/// [`crate::live::may_move`] lets this build.
fn open(reached: Reached, clock: &Clock) -> Result<Store, String> {
    open_as(reached, clock, crate::Opening::write())
}

/// As [`open`], by `how` (`tasks migrate`'s, which may move a live board). Its refusals name a
/// scratch board as `tasks` and `agent-send` take it, `--database NAME`.
fn open_as(reached: Reached, clock: &Clock, how: crate::Opening) -> Result<Store, String> {
    let database = reached.database();
    open_as_database(reached, database, clock, how)
}

fn open_as_database(
    reached: Reached,
    database: crate::Database,
    clock: &Clock,
    how: crate::Opening,
) -> Result<Store, String> {
    let (store, how) = opened(reached, database, clock, how)?;
    following(&store, &how);
    Ok(store)
}

/// As [`open_as_database`], following nothing: the store, and the opening it was made by.
fn opened(
    reached: Reached,
    database: crate::Database,
    clock: &Clock,
    how: crate::Opening,
) -> Result<(Store, crate::Opening), String> {
    let (clock, probe, host) = parts(clock);
    let how = how
        .of(reached.project.as_deref())
        .live(reached.live)
        .scratch(crate::live::SCRATCH_FLAG);
    let named = reached.project.is_none();
    let store = Store::opening_on(database, clock, probe, &host, &how)
        .map(|store| naming(store, named))
        .map_err(|e| e.to_string())?;
    Ok((store, how))
}

/// A live board opened to write follows the workflow its project's default branch commits
/// (automatic activation B, [`crate::workflow::follow`]): a project that commits none and never
/// activated one reads its file and branch tip and writes nothing. An activation refused leaves
/// the previous workflow active, and is said on stderr, as the command runs on under it. A
/// scratch board, a board no project names and a migrate follow nothing.
fn following(store: &Store, how: &crate::Opening) {
    if how.access != crate::schema::Access::Write || how.migrating {
        return;
    }
    let Some(main) = how.live.as_ref().and_then(crate::live::Live::main) else {
        return;
    };
    if let Err(why) = crate::workflow::follow(store, main) {
        eprintln!("{why}");
    }
}

/// The board `reached`, read by `clock`, its tables taken as they are: the recorder's and the
/// detached dispatcher's. Its writes still meet this build's schema guard, which names the project.
/// A live board that went back below its record is refused in the rollback's words, its schema read
/// once when there is a record ([`crate::live::seen::checked`]).
fn open_unchecked(reached: Reached, clock: &Clock) -> Result<Store, String> {
    let live = reached.live.clone();
    let store = unchecked(reached, clock);
    if let Some(live) = live
        && let Some(back) = crate::live::seen::checked(&live, || {
            store.board_version().map_err(|why| why.to_string())
        })?
    {
        return Err(back.refusal());
    }
    Ok(store)
}

/// Refused in the rollback's words when `reached` is a live board that went back below its record,
/// its schema read once when there is a record ([`crate::live::seen::checked`]); no record, no read.
fn not_gone_back(reached: &Reached) -> Result<(), String> {
    if let Some(live) = &reached.live
        && let Some(back) = crate::live::seen::checked(live, || migrate::version(&reached.client))?
    {
        return Err(back.refusal());
    }
    Ok(())
}

/// As [`open_unchecked`], for the heartbeat's one request: the board's schema is read only when the
/// record is above this build's ([`crate::live::seen::past_guard`]), since below it the schema guard
/// refuses the beat's write to a board that went back.
fn open_beating(reached: Reached, clock: &Clock) -> Result<Store, String> {
    let live = reached.live.clone();
    let store = unchecked(reached, clock);
    if let Some(live) = &live
        && let Some(back) =
            crate::live::seen::past_guard(live, crate::Schema::this_build().version(), || {
                store.board_version().map_err(|why| why.to_string())
            })?
    {
        return Err(back.refusal());
    }
    Ok(store)
}

/// The board `reached`, its tables taken as they are, by no check of its own.
fn unchecked(reached: Reached, clock: &Clock) -> Store {
    let (clock, probe, host) = parts(clock);
    let how = crate::Opening::write()
        .of(reached.project.as_deref())
        .live(reached.live);
    let named = reached.project.is_none();
    naming(
        Store::unchecked_on(named_database(reached.client), clock, probe, &host, &how),
        named,
    )
}

/// `store`, saying its server in the commands it tells when `TASKS_SERVER` `named` it (a board
/// reached for no project, [`Reached`]).
fn naming(store: Store, named: bool) -> Store {
    if named {
        store.on_named_server()
    } else {
        store
    }
}

type Probe = Box<dyn Fn(i64) -> bool + Send + Sync>;

/// What a store reads the time, the process table and the host by.
fn parts(clock: &Clock) -> (Box<dyn Fn() -> f64 + Send + Sync>, Probe, String) {
    let read: Box<dyn Fn() -> f64 + Send + Sync> = match clock {
        Clock::Pinned(now) => {
            let now = Arc::clone(now);
            Box::new(move || *now.lock().unwrap_or_else(|held| held.into_inner()))
        }
        Clock::Machine => Box::new(crate::store::now),
    };
    (read, Box::new(sys::pid_alive), sys::hostname())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only a live board opened to write follows its project's committed workflow.
    #[test]
    fn a_live_board_opened_to_write_follows_its_committed_workflow_and_nothing_else_does() {
        use crate::store_fixture::{Backend, Fixture, Seed};
        use crate::workflow::activate::tests::{MAPPED, TWO};
        let repo = crate::workflow::tests::Scratch::new("following");
        std::fs::write(repo.0.join("symphony.toml"), format!("{TWO}{MAPPED}")).unwrap();
        repo.commit("workflow");
        let live = crate::live::Live {
            project: "demo".into(),
            common: repo.0.join(".git"),
        };
        let f = Fixture::on(
            Backend::SurrealDb,
            Seed::default().task("sim/ready", "ready"),
        );
        let write = crate::Opening::write();
        for how in [
            write.clone(),
            write.clone().live(Some(live.clone())).migrating(),
            crate::Opening::read().live(Some(live.clone())),
        ] {
            following(&f.store, &how);
            assert_eq!(f.store.workflow_active().unwrap(), None, "{how:?}");
        }
        following(&f.store, &write.live(Some(live)));
        assert_eq!(f.store.workflow_active().unwrap().unwrap().generation, 1);
        assert_eq!(f.store.get("sim/ready").unwrap().status, "ready");
        assert!(f.store.workflow_position("sim/ready").unwrap().is_some());
    }

    #[test]
    fn explicit_scratch_servers_keep_their_backend_and_database() {
        for (address, postgres) in [
            ("127.0.0.1:9", false),
            ("postgresql:/tmp/board socket", true),
        ] {
            let env = Env {
                vars: [
                    ("TASKS_SERVER".into(), address.into()),
                    ("TASKS_DATABASE".into(), "scratch_route".into()),
                ]
                .into(),
                cwd: std::env::temp_dir(),
            };
            let reached = reach_board(&env, &env.cwd, Reaching::Peek, None).unwrap();
            assert!(reached.project.is_none());
            assert!(reached.live.is_none());
            let database = named_database(reached.client);
            assert_eq!(database.is_postgresql(), postgres);
            assert_eq!(database.addr(), address);
            assert_eq!(database.database(), "scratch_route");
            assert_eq!(
                database.requests(),
                0,
                "selecting a route makes no connection"
            );
        }
    }

    #[test]
    fn malformed_explicit_postgresql_socket_addresses_are_refused_before_connecting() {
        for address in [
            "postgresql:",
            "postgresql:relative",
            "postgresql://host/database",
            "postgresql:/tmp/\0socket",
        ] {
            let env = Env {
                vars: [("TASKS_SERVER".into(), address.into())].into(),
                cwd: std::env::temp_dir(),
            };
            let why = match reach_board(&env, &env.cwd, Reaching::Peek, None) {
                Ok(_) => panic!("accepted {address:?}"),
                Err(why) => why,
            };
            assert!(why.contains("absolute PostgreSQL socket folder"), "{why}");
        }
    }

    #[test]
    fn a_deadline_a_setting_names_replaces_the_products_and_no_number_leaves_it() {
        let fallback = Duration::from_secs(3);
        let read = |value: &str| {
            Env {
                vars: [("TASKS_SOME_WAIT".to_string(), value.to_string())].into(),
                cwd: std::env::temp_dir(),
            }
            .seconds("TASKS_SOME_WAIT", fallback)
        };
        assert_eq!(read("120"), Duration::from_secs(120));
        assert_eq!(read("0.25"), Duration::from_millis(250));
        assert_eq!(read(" 7 "), Duration::from_secs(7));
        assert_eq!(read("0"), Duration::ZERO);
        for nothing in ["", "soon", "-1", "NaN", "inf", "1e400", "3 s"] {
            assert_eq!(read(nothing), fallback, "{nothing:?}");
        }
        assert_eq!(
            Env::default().seconds("TASKS_SOME_WAIT", fallback),
            fallback
        );
    }

    #[test]
    fn the_board_an_environment_names_is_the_command_its_refusals_name() {
        let board = |vars: &[(&str, &str)]| {
            board_of(&Env {
                vars: vars
                    .iter()
                    .map(|(name, value)| (name.to_string(), value.to_string()))
                    .collect(),
                cwd: std::env::temp_dir(),
            })
        };
        assert_eq!(board(&[]), "just tasks");
        // The project's own database, named or not, is no scratch board's: nothing names it.
        assert_eq!(board(&[("TASKS_DATABASE", DATABASE)]), "just tasks");
        assert_eq!(board(&[("TASKS_DATABASE", "")]), "just tasks");
        assert_eq!(
            board(&[("TASKS_DATABASE", "scratch1")]),
            "just tasks --database scratch1"
        );
        assert_eq!(
            board(&[("TASKS_SERVER", "127.0.0.1:9")]),
            "TASKS_SERVER=127.0.0.1:9 just tasks"
        );
        assert_eq!(
            board(&[("TASKS_SERVER", "127.0.0.1:9"), ("TASKS_DATABASE", "s1")]),
            "TASKS_SERVER=127.0.0.1:9 just tasks --database s1"
        );
        // Each as a shell reads it back, whatever the environment held.
        assert_eq!(
            board(&[("TASKS_SERVER", "a b"), ("TASKS_DATABASE", "it's")]),
            r"TASKS_SERVER='a b' just tasks --database 'it'\''s'"
        );
    }

    /// The recorder runs only first, or after `--database NAME` options, the spelling the `tasks`
    /// recipe `symphony init` installs refuses (`shipped/symphony.just`, `_symphony_board`): any
    /// other word before `run`, an option of the table's or not, is the table's, which has no `run`,
    /// so the command after it never starts.
    #[test]
    fn the_recorder_runs_only_first_or_after_database_options_as_the_board_recipe_reads_it() {
        let dir =
            std::env::temp_dir().join(format!("symphony-recorder-first-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let env = Env {
            vars: BTreeMap::new(),
            cwd: dir.clone(),
        };
        let called = |words: &[&str]| {
            let args: Vec<String> = words.iter().map(|word| word.to_string()).collect();
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = main(&args, &env, &mut std::io::empty(), &mut out, &mut err);
            (code, String::from_utf8(out).unwrap())
        };
        let probe = format!("{}\n", recorder::PROBE);
        for words in [
            &["run", "--probe"][..],
            &["--database", "scratch1", "run", "--probe"],
            &["--database", "a", "--database", "b", "run", "--probe"],
            // A database named `run` is a value, the command after it the recorder still.
            &["--database", "run", "run", "--probe"],
        ] {
            assert_eq!(called(words), (0, probe.clone()), "{words:?}");
        }
        let ran = dir.join("ran");
        let touch = format!("touch '{}'", ran.display());
        for lead in [
            &["--as", "h"][..],
            &["--as=h"],
            &["--database=scratch1"],
            &["--database", "a b"],
            &["--"],
        ] {
            let words: Vec<&str> = lead
                .iter()
                .copied()
                .chain(["run", "--phase", "test", "--", "sh", "-c", &touch])
                .collect();
            assert_eq!(called(&words), (2, String::new()), "{words:?}");
            assert!(!ran.exists(), "{words:?} ran its command");
        }
        // The same command first does run, so the words above are what kept it from running.
        let words = ["run", "--phase", "test", "--", "sh", "-c", &touch];
        let mut recording = env.clone();
        recording.vars.insert("PATH".into(), "/usr/bin:/bin".into());
        recording.vars.insert(
            "TASKS_WORK".into(),
            dir.join("work").to_string_lossy().into_owned(),
        );
        recording.vars.insert(
            "SYMPHONY_MACHINE_DIR".into(),
            dir.join("machine").to_string_lossy().into_owned(),
        );
        let args: Vec<String> = words.iter().map(|word| word.to_string()).collect();
        let code = main(
            &args,
            &recording,
            &mut std::io::empty(),
            &mut Vec::new(),
            &mut Vec::new(),
        );
        assert_eq!(code, 0);
        assert!(ran.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_pinned_clock_since_boot_is_a_boot_and_its_seconds_and_anything_else_reads_none() {
        let pinned = |text: &str| {
            let vars = [("TASKS_BOOT_CLOCK".to_string(), text.to_string())];
            boot_clock(&Env {
                vars: vars.into_iter().collect(),
                cwd: std::env::temp_dir(),
            })
        };
        let read = sys::Boottime {
            boot: "boot-1".into(),
            at: 12.5,
        };
        assert_eq!(pinned("boot-1 12.5"), Some(read));
        for text in ["none", "boot-1", " 12.5", "boot-1 soon", "boot-1 inf"] {
            assert_eq!(pinned(text), None, "{text}");
        }
        // Unpinned, the machine's: its boot, where starts are dated since boot.
        let machine = |read: Option<sys::Boottime>| read.map(|r| r.boot);
        assert_eq!(
            machine(boot_clock(&Env::default())),
            machine(sys::boot_clock())
        );
    }

    /// A switch says `on` or `off`; unset, empty or any other word leaves the rule: only the
    /// repository's own board, which no `TASKS_SERVER` names, starts what it guards.
    #[test]
    fn a_switch_is_on_or_off_else_only_the_boards_own_server_starts_it() {
        let env = |vars: &[(&str, &str)]| Env {
            vars: vars
                .iter()
                .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
                .collect(),
            cwd: PathBuf::new(),
        };
        let server = ("TASKS_SERVER", "127.0.0.1:1");
        assert!(env(&[]).switch("TASKS_X"));
        assert!(!env(&[server]).switch("TASKS_X"));
        assert!(env(&[server, ("TASKS_X", "on")]).switch("TASKS_X"));
        assert!(!env(&[("TASKS_X", "off")]).switch("TASKS_X"));
        // Another switch's word, an empty one and a word that is neither decide nothing.
        assert!(!env(&[server, ("TASKS_Y", "on")]).switch("TASKS_X"));
        assert!(env(&[("TASKS_Y", "off")]).switch("TASKS_X"));
        assert!(env(&[("TASKS_X", "")]).switch("TASKS_X"));
        assert!(!env(&[server, ("TASKS_X", "")]).switch("TASKS_X"));
        assert!(!env(&[server, ("TASKS_X", "yes")]).switch("TASKS_X"));
        assert!(env(&[("TASKS_X", "ON")]).switch("TASKS_X"));
        // An empty `TASKS_SERVER` names no server.
        assert!(env(&[("TASKS_SERVER", "")]).switch("TASKS_X"));
    }
}
