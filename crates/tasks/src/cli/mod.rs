//! The `tasks` command line (`tools/tasks.py`'s), over the store: each command of `just tasks`
//! ([`commands`]), read as its argparse parser read them ([`args`]), and `api`, the seam the Python
//! readers call the board through ([`api`]).

pub mod agent_send;
pub mod agents;
mod api;
mod args;
pub mod attach;
mod beat;
mod commands;
mod detached;
mod embedding;
mod field;
pub mod machine_work;
pub mod probes;
pub mod pull;
mod recorder;
mod search;
mod shape;
mod status;
mod sweep;
mod text;
mod usage;

pub(crate) use beat::{keys_folder, mark_exactly};

use crate::rpc::{self, Client};
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
                .collect(),
            cwd: std::env::current_dir().unwrap_or_default(),
        }
    }

    pub(crate) fn var(&self, name: &str) -> Option<&str> {
        self.vars
            .get(name)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
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
    let args: Vec<String> = std::env::args().skip(1).collect();
    let env = Env::of_process();
    let (mut input, mut out, mut err) = (std::io::stdin(), std::io::stdout(), std::io::stderr());
    let code = match std::env::current_exe() {
        Ok(program) => main_detaching(&args, &env, &mut input, &mut out, &mut err, &program),
        Err(_) => main(&args, &env, &mut input, &mut out, &mut err),
    };
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
    // TASKS_DATABASE override, without a shell environment prefix or a second board server.
    if args.first().is_some_and(|arg| arg == "--database") {
        let Some(database) = args.get(1).filter(|name| rpc::plain(name)) else {
            let _ = writeln!(err, "--database needs a plain database name");
            return 2;
        };
        let mut scratch = env.clone();
        scratch
            .vars
            .insert("TASKS_DATABASE".into(), database.clone());
        if database != DATABASE {
            scratch.vars.insert("TASKS_SWEEP".into(), "off".into());
        }
        return run(&args[2..], &scratch, input, out, err, program);
    }
    if args.len() == 1 && args[0] == detached::COMMAND {
        return detached::run(env, err);
    }
    // Before the board's argument table: what follows `--` is another program's command line.
    if args.first().is_some_and(|arg| arg == recorder::COMMAND) {
        return recorder::run(&args[1..], env, err, program.is_some());
    }
    let board = || connect(env).map(|store| detaching(store, env, program));
    if args.len() == 1 && args[0] == "api" {
        return api::run(input, out, board);
    }
    if args.len() == 2 && args[0] == "api" && args[1] == "--lines" {
        return api::serve(&mut std::io::BufReader::new(input), out, board);
    }
    let parsed = match args::parse(args) {
        args::Outcome::Run(parsed) => parsed,
        args::Outcome::Help(text) => {
            let _ = write!(out, "{text}");
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
    let reached = Clock::of(env).and_then(|clock| {
        let store = open(reach(env, &env.cwd, true)?, &clock)?;
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
    };
    let (code, said) = match commands::run(&store, &mut context) {
        Ok(()) => return 0,
        Err(commands::Stop::NoSession) => (
            2,
            "no session: pass --as HANDLE or set TASKS_SESSION (get one with `just tasks-attach` or `just tasks register`)"
                .to_string(),
        ),
        Err(commands::Stop::Refused(why)) => (1, why),
        Err(commands::Stop::Board(why)) => (2, why),
    };
    let _ = writeln!(context.err, "✖ {said}");
    code
}

/// The board as `env` reaches it: the server `TASKS_SERVER` names, in database `TASKS_DATABASE`, else
/// the one in the git common dir of the working directory, started if it is not running; its clock
/// pinned at `TASKS_NOW` if that is set; its starts held to the cap the project sets ([`capped`]).
pub fn connect(env: &Env) -> Result<Store, String> {
    Ok(capped(
        open(reach(env, &env.cwd, true)?, &Clock::of(env)?)?,
        env,
    ))
}

/// `store`, its starts held to the cap on each orchestrator's open tasks that the main checkout of
/// `env`'s repository configures ([`crate::slots::of_env`]). A cap that cannot be read is said on
/// stderr and caps nothing: a broken project file must not stop the board, nor the landing that
/// mends it.
pub fn capped(store: Store, env: &Env) -> Store {
    capped_saying(store, env, &mut |problem| eprintln!("tasks: {problem}"))
}

/// As [`capped`], what is wrong with the cap said by `say`.
fn capped_saying(store: Store, env: &Env, say: &mut dyn FnMut(&str)) -> Store {
    match crate::slots::of_env(env) {
        Ok(slots) => store.capping(slots),
        Err(problem) => {
            say(&format!(
                "{problem}; no cap on an orchestrator's open tasks applies"
            ));
            store
        }
    }
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

    /// Wait `seconds`. A pinned clock moves on by them instead, says `STEP <now>` on `err`, and reads a
    /// line on `input` (or its end) before going on, so a test acts between a monitor's looks.
    pub fn wait(&self, seconds: f64, input: &mut dyn Read, err: &mut dyn Write) {
        match self {
            Clock::Machine => {
                std::thread::sleep(Duration::try_from_secs_f64(seconds).unwrap_or_default())
            }
            Clock::Pinned(now) => {
                let at = {
                    let mut now = now.lock().unwrap_or_else(|held| held.into_inner());
                    *now += seconds;
                    *now
                };
                let _ = writeln!(err, "STEP {}", crate::pyjson::float(at));
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

/// The board `env` reaches, its server started if it is down, whether or not the board has moved
/// there: what `tasks-copy --board` and `just tasks-cutover` write into.
pub fn board_client(env: &Env) -> Result<Client, String> {
    reach_board(env, &env.cwd, true, false).map(|reached| reached.client)
}

/// A board a run reaches, and the id of the project it is the board of: none for a server
/// `TASKS_SERVER` names, which is no project's.
pub struct Reached {
    pub client: Client,
    pub project: Option<String>,
}

/// The board of the project `env`'s working directory is in ([`crate::project::find`]), its
/// server started if it is down, or the one `TASKS_SERVER` and `TASKS_DATABASE` name, as
/// [`board_client`] reaches it: what the Symphony reads a project's board from.
pub fn board(env: &Env) -> Result<Reached, String> {
    reach_board(env, &env.cwd, true, false)
}

/// The board `env` reaches from `cwd`; a server that is not running is started if `start`, else
/// refused.
fn reach(env: &Env, cwd: &Path, start: bool) -> Result<Reached, String> {
    reach_board(env, cwd, start, true)
}

/// The machine folder `env` names ([`crate::project::machine_folder`]), if it names one.
fn machine(env: &Env) -> Option<PathBuf> {
    crate::project::machine_folder(
        |name| env.var(name).map(std::ffi::OsString::from),
        cfg!(target_os = "macos"),
    )
    .ok()
}

/// The board of the project `cwd` is in, in that project's common dir, refused (with `unmoved`)
/// while the SQLite board there has not moved to it: what was written to it would be lost when the
/// move replaces it.
fn reach_board(env: &Env, cwd: &Path, start: bool, unmoved: bool) -> Result<Reached, String> {
    let db = env.var("TASKS_DATABASE").unwrap_or(DATABASE);
    if !rpc::plain(db) {
        return Err(format!(
            "TASKS_DATABASE={db:?} is not a plain name for a database"
        ));
    }
    if let Some(addr) = env.var("TASKS_SERVER") {
        return Ok(Reached {
            client: Client::new(addr, NAMESPACE, db),
            project: None,
        });
    }
    let project = crate::project::find(cwd, machine(env).as_deref())?;
    let common = &project.place.common;
    if unmoved && let Some(why) = not_here(common) {
        return Err(why);
    }
    let home = Home::in_common(common, env.var("SF_SURREAL").map(Path::new));
    let running = if start {
        home.ensure().map_err(|e| e.to_string())?
    } else {
        home.recorded()
            .ok_or_else(|| format!("no server of the board in {}", home.dir().display()))?
    };
    Ok(Reached {
        client: Client::new(&running.addr, NAMESPACE, db),
        project: Some(project.id),
    })
}

/// Why the board in `common` is not served here: the SQLite board there has not moved yet, or a move
/// froze it and has not finished (it runs, or was cut short).
fn not_here(common: &Path) -> Option<String> {
    // `<board>.moved` is what `just tasks-cutover` writes beside it once its copy into SurrealDB is
    // checked ([`crate::cutover`]), and it stays.
    let name = crate::cutover::board_name(common)?;
    let board = common.join(&name);
    if !board.exists() || common.join(format!("{name}.moved")).exists() {
        return None;
    }
    Some(if crate::cutover::frozen(&board) {
        "the task board is frozen by a move to SurrealDB that has not finished: unless one is running, run `just tasks-cutover` again (docs/workflow/TASKS.md#the-move-to-surrealdb)".into()
    } else {
        "the task board has not moved to SurrealDB yet: the lander runs `just tasks-cutover` once, right after landing (docs/workflow/TASKS.md#the-move-to-surrealdb)".into()
    })
}

/// The board `reached`, to read and write by this build's schema, read by `clock`: a newer board
/// is refused before any write, naming its project.
fn open(reached: Reached, clock: &Clock) -> Result<Store, String> {
    let (clock, probe, host) = parts(clock);
    let how = crate::Opening::write().of(reached.project.as_deref());
    Store::opening(reached.client, clock, probe, &host, &how).map_err(|e| e.to_string())
}

/// The board `reached`, read by `clock`, its tables taken as they are: the heartbeat's one request.
/// Its write still meets this build's schema guard, which names the project.
fn open_unchecked(reached: Reached, clock: &Clock) -> Store {
    let (clock, probe, host) = parts(clock);
    let how = crate::Opening::write().of(reached.project.as_deref());
    Store::unchecked_as(reached.client, clock, probe, &host, &how)
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
}
