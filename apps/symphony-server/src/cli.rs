//! `symphony-server`'s command line, `just symphony ARGS`.

use crate::repo;
use std::cell::Cell;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

thread_local! {
    static STARTUP_TRACE: Cell<bool> = const { Cell::new(false) };
}

/// Opt-in is local to the starter's synchronous call stack, never a server or stop command.
struct StartupTrace(bool);

impl StartupTrace {
    fn enter() -> Self {
        let enabled = symphony_env::STARTUP_TRACE.os().is_some_and(|v| v == "1");
        Self(STARTUP_TRACE.with(|trace| trace.replace(enabled)))
    }
}

impl Drop for StartupTrace {
    fn drop(&mut self) {
        STARTUP_TRACE.with(|trace| trace.set(self.0));
    }
}

/// A requested startup stage stays visible when the caller times out before launch.
pub(crate) fn startup<T>(name: &str, run: impl FnOnce() -> T) -> T {
    if !STARTUP_TRACE.with(Cell::get) {
        return run();
    }
    let began = Instant::now();
    eprintln!("· Symphony startup {}: begin {name}", std::process::id());
    let result = run();
    eprintln!(
        "· Symphony startup {}: end {name}: {:.3} ms",
        std::process::id(),
        began.elapsed().as_secs_f64() * 1000.0
    );
    result
}

pub const USAGE: &str = "\
usage: symphony-server [--open] [--if-up] [--shared] [--host H]... [--root DIR] [--port N | PORT]
       symphony-server --stop
       symphony-server project add [DIR]
       symphony-server --version
       symphony-server --serves

The Symphony (`just symphony`): one server per machine, under a lock in the machine folder, serving
every registered project at `/<id>/` and each of its worktrees at `/<id>/@<tree id>/`. A start
registers its checkout's project, finds the running server and prints the project's URL, or
starts the server, detached, and prints that. A running server that lacks the project's plugins
is replaced by this build, its sockets handed over, every project kept registered. A start in a
linked worktree, whichever build it runs, only prints the running server's URL for that worktree,
when that server serves its project, and otherwise starts, registers and replaces nothing.

  --open       open the URL in the browser (interactive human terminals only)
  --if-up      a start of the one server only finds the running one: prints (and opens) its URL,
               or exits 3 having started nothing, as it does when this build must replace it; a
               stop or a port of its own is done at once
  --shared     in a linked worktree, the owner's word that this start may start the one server:
               it registers the project and starts the one server when none runs, and never
               replaces a running one
  --host H     the address it listens on, repeatable (default: every interface, each request
               judged by its client address: this machine or a confirmed tailnet peer)
  --port N     a server of its own on port N, not the one server: it runs in the foreground and
               ends with the process that started it (PORT alone is the same). It serves the
               current checkout at `/`; --root selects another folder. The one server's port is
               refused, on every address
  --root DIR   the project it serves; a root outside this repository is a server of its own too
  --stop       stop the one server
  project add  register DIR's project (default: here) with the one server, and say whether the
               running server can serve it; run in a linked worktree, it registers none of that
               worktree's own repository

  --serves     what this build serves, as one line of JSON: the plugin protocol's major and minor,
               the board schema and the Symphony pin

The one server ignores --host and an in-repository --root; use --port to customize a server.
";

/// What `--if-up` exits with when it is asked to start the one server and finds none running.
pub const NOT_UP: i32 = 3;

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Help,
    Start(Options),
    Stop(Options),
    /// `project add [DIR]`.
    AddProject(PathBuf),
    /// The server itself, as a start runs it:
    /// `serve --home MACHINE --root DIR --port N [--host H]... [--install DIR]`.
    Serve(Serve),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Options {
    pub open: bool,
    pub root: Option<PathBuf>,
    pub port: Option<u16>,
    pub hosts: Vec<String>,
    pub if_up: bool,
    /// `--shared`: a start in a linked worktree may start the one server with this build.
    pub shared: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Serve {
    /// The one server's folder, the machine folder; none for a server of its own, which ends with
    /// its parent instead.
    pub home: Option<PathBuf>,
    /// The project that started it: today's paths redirect to its `/<id>/`.
    pub root: PathBuf,
    pub port: u16,
    pub hosts: Vec<String>,
    /// The folder holding its build and its client (`symphony/server/` in the starter's git common
    /// dir); the home when none is named.
    pub install: Option<PathBuf>,
}

impl Serve {
    /// Where its build and its client are: [`Serve::install`], else the home.
    pub fn installed(&self) -> Option<&Path> {
        self.install.as_deref().or(self.home.as_deref())
    }
}

/// `args` without the program name.
pub fn parse(args: &[String]) -> Result<Action, String> {
    if args.first().is_some_and(|word| word == "project") {
        return match &args[1..] {
            [add] if add == "add" => Ok(Action::AddProject(PathBuf::from("."))),
            [add, dir] if add == "add" && !dir.starts_with('-') => {
                Ok(Action::AddProject(PathBuf::from(dir)))
            }
            _ => Err("project takes `add [DIR]`".into()),
        };
    }
    let (serve, args) = match args.split_first() {
        Some((first, rest)) if first == "serve" => (true, rest),
        _ => (false, args),
    };
    let mut options = Options::default();
    let (mut home, mut install) = (None, None);
    let mut stop = false;
    let mut words = args.iter();
    while let Some(word) = words.next() {
        let mut value = || {
            words
                .next()
                .cloned()
                .ok_or_else(|| format!("{word} needs a value"))
        };
        match word.as_str() {
            "-h" | "--help" => return Ok(Action::Help),
            "--open" => options.open = true,
            "--if-up" => options.if_up = true,
            "--shared" if !serve => options.shared = true,
            "--stop" => stop = true,
            "--root" => options.root = Some(PathBuf::from(value()?)),
            "--home" if serve => home = Some(PathBuf::from(value()?)),
            "--install" if serve => install = Some(PathBuf::from(value()?)),
            "--host" => options.hosts.push(value()?),
            "--port" => options.port = Some(port(&value()?)?),
            other if !other.starts_with('-') && options.port.is_none() => {
                options.port = Some(port(other)?);
            }
            other => return Err(format!("{other:?} is not an argument")),
        }
    }
    if serve {
        let (Some(root), Some(port)) = (options.root, options.port) else {
            return Err("serve needs --root and --port".into());
        };
        // A generation from before the machine folder hands over to its successor with its git
        // common dir's folder as the home: that folder stays the install, the machine's is the home.
        if install.is_none()
            && let Some(old) =
                home.take_if(|home: &mut PathBuf| home.ends_with(crate::home::FOLDER))
        {
            install = Some(old);
            home = Some(symphony::machine::dir()?);
        }
        return Ok(Action::Serve(Serve {
            home,
            root,
            port,
            hosts: options.hosts,
            install,
        }));
    }
    if options.shared && options.port.is_some() {
        return Err("--shared starts the one server, which --port is not".into());
    }
    Ok(if stop {
        Action::Stop(options)
    } else {
        Action::Start(options)
    })
}

fn port(word: &str) -> Result<u16, String> {
    word.parse()
        .map_err(|_| format!("{word:?} is not a port (0 to 65535)"))
}

/// Run `exe`, this server, with `args` against its own board `database` and asks file `asks`, in this
/// process's folder and on its streams: the server's exit code, or 2 when it could not start or ended
/// on a signal.
fn scratch(exe: &Path, args: &[String], database: &str, asks: &Path) -> i32 {
    symphony::children::command(exe)
        .args(args)
        .env("TASKS_DATABASE", database)
        .env("ASKS_DB", asks)
        .env("TASKS_SWEEP", "off")
        .spawn()
        .and_then(|mut server| server.wait())
        .ok()
        .and_then(|status| status.code())
        .unwrap_or(2)
}

/// The flag that asks what this build serves ([`serves`]).
const SERVES: &str = "--serves";

/// What this build serves, as one line of JSON: the plugin protocol it speaks
/// (docs/engineering/PLUGIN_PROTOCOL.md), the board schema it reads and writes with its additive steps on it
/// (`symphony_board::schema::ADDITIONS`) and its Symphony pin (`symphony::machine::PIN`). A release's install asks the server it built, never the sources,
/// and writes the answer in the release's manifest (docs/workflow/RELEASES.md, "The install").
pub fn serves() -> String {
    let protocol = symphony_plugin::protocol::PROTOCOL;
    serde_json::json!({
        "protocol": {"major": protocol.major, "minor": protocol.minor},
        "schema": symphony_board::SCHEMA,
        "additions": symphony_board::schema::ADDITIONS,
        "pin": symphony::machine::PIN,
    })
    .to_string()
}

/// The whole of `symphony-server`: the exit code.
pub fn main(args: &[String]) -> i32 {
    if symphony::cli::version(
        args,
        "symphony-server",
        env!("CARGO_PKG_VERSION"),
        &mut std::io::stdout(),
    ) {
        return 0;
    }
    if matches!(args, [only] if only == SERVES) {
        println!("{}", serves());
        return 0;
    }
    // Keep scratch runs explicit and self-contained without mutating the process environment
    // after threads exist. The child serves normally against its own board and asks file.
    if args.first().is_some_and(|arg| arg == "--scratch") {
        let Some(database) = args.get(1).filter(|database| {
            symphony_board::db::plain(database) && database.as_str() != symphony_board::DATABASE
        }) else {
            eprintln!("--scratch needs a database name other than tasks");
            return 2;
        };
        if !args[2..].iter().any(|arg| arg == "--port") {
            eprintln!("--scratch requires --port for a server of its own");
            return 2;
        }
        let Ok(exe) = std::env::current_exe() else {
            return 2;
        };
        let asks = std::env::current_dir()
            .unwrap_or_default()
            .join("artifacts")
            .join(format!("callbacks-{database}"))
            .join("asks.db");
        return scratch(&exe, &args[2..], database, &asks);
    }
    match parse(args) {
        Ok(Action::Help) => {
            print!("{USAGE}");
            0
        }
        Ok(Action::Serve(serve)) => crate::front::serve(serve),
        Ok(Action::Start(options)) => start(options),
        Ok(Action::Stop(_)) => stop(),
        Ok(Action::AddProject(dir)) => symphony::machine::command::main(
            &["add".into(), dir.display().to_string()],
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        ),
        Err(why) => {
            eprintln!("✖ symphony-server: {why}\n\n{USAGE}");
            2
        }
    }
}

/// The hosts a server listens on when none is named: every interface, each request judged by its
/// client address (src/gate.rs).
fn hosts(named: Vec<String>) -> Vec<String> {
    if named.is_empty() {
        vec!["0.0.0.0".into(), "::".into()]
    } else {
        named
    }
}

/// The one server's port: `SYMPHONY_PORT` (a test's own), else the app's.
pub(crate) fn one_port() -> u16 {
    symphony_env::PORT
        .text()
        .and_then(|v| v.parse().ok())
        .unwrap_or(crate::home::DEFAULT_PORT)
}

/// Why a server of its own may not take `port`, when it is the one server's (`one`) or the app's:
/// outside Linux a bind of one address beside the one server's wildcard succeeds, and answers that
/// address (the tailnet's, the LAN's) in its place. Port 0 is the server's own pick.
fn reserved(port: u16, one: u16) -> Option<String> {
    (port != 0 && (port == one || port == crate::home::DEFAULT_PORT)).then(|| {
        format!(
            "port {port} is the one server's (`just symphony`); a server of its own takes another \
             port, or 0 to pick one"
        )
    })
}

/// [`reserved`] for this environment's one server.
pub(crate) fn refused_port(port: u16) -> Option<String> {
    reserved(port, one_port())
}

fn start(options: Options) -> i32 {
    let trace = StartupTrace::enter();
    if STARTUP_TRACE.with(Cell::get) {
        eprintln!(
            "· Symphony startup {}: start command entered at {} ms since Unix epoch",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );
    }
    let here = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let checkout = startup("checkout discovery", || repo::checkout(&here));
    let root = options.root.as_deref().map(|root| {
        root.canonicalize()
            .unwrap_or_else(|_| here.join(root).to_path_buf())
    });
    // A root of this repository is the one server's; any other root, or a port of its own, is not.
    let in_repo = |root: &Path| match (&checkout, repo::checkout(root)) {
        (Ok(here), Ok(there)) => here.common == there.common,
        _ => false,
    };
    let own = options.port.is_some() || root.as_deref().is_some_and(|root| !in_repo(root));
    if own {
        let root = match (root, &checkout) {
            (Some(root), _) => root,
            (None, Ok(checkout)) => checkout.top.clone(),
            (None, Err(why)) => {
                eprintln!("✖ Symphony: {why}");
                return 2;
            }
        };
        let serve = Serve {
            home: None,
            root,
            port: options.port.unwrap_or(0),
            hosts: hosts(options.hosts),
            install: None,
        };
        drop(trace);
        return crate::front::run(serve, options.open);
    }
    let checkout = match checkout {
        Ok(checkout) => checkout,
        Err(why) => {
            eprintln!("✖ Symphony: {why}");
            return 2;
        }
    };
    // A release's server, run through the machine's `current`, starts by its own folder's path, so
    // the server it starts stays that release when the link moves.
    let released = symphony::machine::release::this()
        .map(|(release, _)| release.join("bin").join(symphony::machine::release::SERVER));
    let exe = match released.map_or_else(std::env::current_exe, Ok) {
        Ok(exe) => exe,
        Err(why) => {
            eprintln!("✖ Symphony: cannot find this program: {why}");
            return 2;
        }
    };
    if options.root.is_some() || !options.hosts.is_empty() {
        eprintln!(
            "· --host and an in-repository --root are ignored for the one server; use --port"
        );
    }
    let started =
        crate::starter::home(&checkout, &exe, one_port(), hosts(Vec::new())).and_then(|home| {
            let started = crate::starter::one(&checkout, &home, options.if_up, options.shared)?;
            Ok((started, home))
        });
    let (running, project, home) = match started {
        Ok((crate::starter::Started::Serving(running, project), home)) => (running, project, home),
        Ok((crate::starter::Started::NotUp, _)) => return NOT_UP,
        Err(why) => {
            eprintln!("✖ Symphony: {why}");
            return 2;
        }
    };
    let page: String = [
        (!project.is_empty()).then(|| format!("{project}/")),
        checkout.tree_id.map(|id| format!("@{id}/")),
    ]
    .into_iter()
    .flatten()
    .collect();
    let url = format!("http://127.0.0.1:{}/{page}", running.port);
    let front = crate::probe::front(running.port, Duration::from_secs(5));
    let mut urls = url.clone();
    if let Some(name) = front.as_ref().and_then(|f| f.tailnet.clone()) {
        urls.push_str(&format!("  http://{name}:{}/{page}", running.port));
    }
    println!(
        "→ Symphony at {urls}  (pid {}; `just symphony --stop` stops it)",
        running.pid
    );
    if options.open {
        open(&url);
    }
    if front.is_some_and(|front| !front.ready) {
        eprintln!(
            "· its app does not answer yet: {}",
            home.dir().join(crate::home::LOG).display()
        );
    }
    0
}

fn stop() -> i32 {
    let here = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let checkout = match repo::checkout(&here) {
        Ok(checkout) => checkout,
        Err(why) => {
            eprintln!("✖ Symphony: {why}");
            return 2;
        }
    };
    let home = match crate::starter::home(&checkout, Path::new(""), one_port(), Vec::new()) {
        Ok(home) => home,
        Err(why) => {
            eprintln!("✖ Symphony: {why}");
            return 2;
        }
    };
    match home.stop() {
        Ok(Some(running)) => {
            println!(
                "· Symphony stopped (pid {}, port {})",
                running.pid, running.port
            );
            0
        }
        Ok(None) => {
            println!("· no Symphony was running");
            0
        }
        Err(why) => {
            eprintln!("✖ Symphony: {why}");
            2
        }
    }
}

/// The page in the browser; a browser that will not open is no failure.
pub(crate) fn open(url: &str) {
    if !std::io::stdout().is_terminal()
        || ["TASKS_SESSION", "CODEX_THREAD_ID", "CLAUDECODE"]
            .iter()
            .any(|name| std::env::var_os(name).is_some())
    {
        eprintln!("· printing the URL without opening a browser");
        return;
    }
    let _ = opener(url).spawn().and_then(|mut opener| opener.wait());
}

/// The system's opener for `url`, its streams null. A server of its own opens its page too
/// (front::start), once it accepts: the opener starts without a fork and inherits only its
/// streams, never a connection not yet close-on-exec.
fn opener(url: &str) -> symphony_board::sys::Command {
    let mut command = symphony::children::command(if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    });
    command
        .arg(url)
        .stdin(symphony_board::sys::Stdio::null())
        .stdout(symphony_board::sys::Stdio::null())
        .stderr(symphony_board::sys::Stdio::null());
    command
}

#[cfg(test)]
mod tests;
