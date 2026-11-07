//! `symphony-server`'s command line, `just symphony ARGS`.

use crate::home::Home;
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
        let enabled = std::env::var_os("SF_SYMPHONY_STARTUP_TRACE").is_some_and(|v| v == "1");
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
usage: symphony-server [--open] [--if-up] [--host H]... [--root DIR] [--port N | PORT]
       symphony-server --stop

The Symphony (`just symphony`): one server per machine, under a lock in the git common dir, serving
every worktree. A start finds the running one and prints its URL (`/@<tree id>/` from a branch's
worktree), or starts it, detached, and prints that.

  --open       open the URL in the browser (interactive human terminals only)
  --if-up      a start of the one server only finds the running one: prints (and opens) its URL,
               or exits 3 having started nothing; a stop or a port of its own is done at once
  --host H     the address it listens on, repeatable (default: every interface, each request
               judged by its client address: this machine or a confirmed tailnet peer)
  --port N     a server of its own on port N, not the one server: it runs in the foreground and
               ends with the process that started it (PORT alone is the same). Like the one server
               it serves the current checkout; --root selects another folder. The one server's
               port is refused, on every address
  --root DIR   the project it serves; a root outside this repository is a server of its own too
  --stop       stop the one server

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
    /// The server itself, as a start runs it: `serve --home DIR --root DIR --port N`.
    Serve(Serve),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Options {
    pub open: bool,
    pub root: Option<PathBuf>,
    pub port: Option<u16>,
    pub hosts: Vec<String>,
    pub if_up: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Serve {
    /// The one server's folder; none for a server of its own, which ends with its parent instead.
    pub home: Option<PathBuf>,
    pub root: PathBuf,
    pub port: u16,
    pub hosts: Vec<String>,
}

/// `args` without the program name.
pub fn parse(args: &[String]) -> Result<Action, String> {
    let (serve, args) = match args.split_first() {
        Some((first, rest)) if first == "serve" => (true, rest),
        _ => (false, args),
    };
    let mut options = Options::default();
    let mut home = None;
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
            "--stop" => stop = true,
            "--root" => options.root = Some(PathBuf::from(value()?)),
            "--home" if serve => home = Some(PathBuf::from(value()?)),
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
        return Ok(Action::Serve(Serve {
            home,
            root,
            port,
            hosts: options.hosts,
        }));
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

/// The whole of `symphony-server`: the exit code.
pub fn main(args: &[String]) -> i32 {
    // Keep scratch runs explicit and self-contained without mutating the process environment
    // after threads exist. The child serves normally against its own board and asks file.
    if args.first().is_some_and(|arg| arg == "--scratch") {
        let Some(database) = args.get(1).filter(|database| {
            symphony_board::rpc::plain(database) && database.as_str() != symphony_board::DATABASE
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

/// The one server's port: `SF_SYMPHONY_PORT` (a test's own), else the app's.
fn one_port() -> u16 {
    std::env::var("SF_SYMPHONY_PORT")
        .ok()
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
    let exe = match std::env::current_exe() {
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
    let home = Home::new(
        &checkout.common.join(crate::home::FOLDER),
        &exe,
        &checkout.main,
        one_port(),
        hosts(Vec::new()),
    );
    let found = if options.if_up {
        home.up().ok_or(None)
    } else {
        startup("ensure shared server", || home.ensure()).map_err(Some)
    };
    let running = match found {
        Ok(running) => running,
        Err(None) => return NOT_UP,
        Err(Some(why)) => {
            eprintln!("✖ Symphony: {why}");
            return 2;
        }
    };
    let page = checkout
        .tree_id
        .map(|id| format!("@{id}/"))
        .unwrap_or_default();
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
    match front {
        Some(front) if !front.ready => eprintln!(
            "· its app does not answer yet: {}",
            home.dir().join(crate::home::LOG).display()
        ),
        // Report the available count; a pending census must not hold the starter.
        Some(front) => {
            let counted = if front.uncounted && !front.census_pending {
                home.counted(&running)
            } else {
                Some(front)
            };
            if let Some(front) = counted.filter(|front| front.orphans > 0) {
                eprintln!(
                    "· {} Symphony processes from test runs run with no parent; none is stopped for you: \
                     `ps -axo pid,ppid,command | grep -e docs_server -e 'symphony-server serve'` lists them",
                    front.orphans
                );
            }
        }
        None => {}
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
    let home = Home::new(
        &checkout.common.join(crate::home::FOLDER),
        Path::new(""),
        &checkout.main,
        one_port(),
        Vec::new(),
    );
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
mod tests {
    use super::*;

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_string).collect()
    }

    /// Set in this test binary's own child, run by the case below: the server a scratch run starts.
    const SCRATCH_SERVER: &str = "SF_SCRATCH_RUN_SERVER";

    /// Only in that child: a scratch run on the streams and folder the case gave it, ending with its code.
    #[test]
    fn scratch_run_in_a_child() {
        if let Some(server) = std::env::var_os(SCRATCH_SERVER) {
            let args = ["--port".to_owned(), "an argument".to_owned()];
            let code = scratch(
                Path::new(&server),
                &args,
                "scratchdb",
                Path::new("/asks.db"),
            );
            std::process::exit(code);
        }
    }

    /// A scratch run starts the server with its arguments, its own board and this process's
    /// environment, folder and streams, and ends with its code; one that cannot start or ends on a
    /// signal is 2.
    #[test]
    fn a_scratch_run_keeps_its_arguments_board_folder_streams_and_exit() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("scratch-run-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let script = |name: &str, body: &str| {
            let path = dir.join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        let server = script(
            "server",
            "IFS= read -r line\n\
             printf '%s\\n' \"$line\" \"$PWD\" \"$TASKS_DATABASE\" \"$ASKS_DB\" \"$TASKS_SWEEP\" \"$KEPT\" \"$@\"\n\
             printf 'diagnostic\\n' >&2\nexit 7\n",
        );
        let killed = script("killed", "kill -TERM $$\n");
        std::fs::write(dir.join("input"), "request\n").unwrap();
        let run = |server: &Path| {
            let file = |name: &str| std::fs::File::create(dir.join(name)).unwrap();
            let status = symphony_board::sys::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "cli::tests::scratch_run_in_a_child"])
                .args(["--nocapture", "--test-threads=1"])
                .env(SCRATCH_SERVER, server)
                .env("KEPT", "inherited")
                .current_dir(&dir)
                .stdin(std::fs::File::open(dir.join("input")).unwrap())
                .stdout(file("out"))
                .stderr(file("err"))
                .spawn()
                .unwrap()
                .wait()
                .unwrap();
            let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
            (status.code(), read("out"), read("err"))
        };
        let (code, out, err) = run(&server);
        let said = format!(
            "request\n{}\nscratchdb\n/asks.db\noff\ninherited\n--port\nan argument\n",
            dir.display()
        );
        assert_eq!(code, Some(7), "{out}{err}");
        assert!(out.ends_with(&said), "{out}");
        assert!(err.ends_with("diagnostic\n"), "{err}");
        assert_eq!(run(&dir.join("missing")).0, Some(2));
        assert_eq!(run(&killed).0, Some(2));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_opener_is_the_systems_own_given_the_url_alone() {
        let url = "http://127.0.0.1:8765/@tools~branch/";
        let opener = opener(url);
        let system = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        assert_eq!(opener.get_program(), system);
        assert_eq!(opener.get_args().collect::<Vec<_>>(), [url]);
        assert_eq!(opener.get_current_dir(), None);
    }

    #[test]
    fn no_arguments_start_the_one_server() {
        assert_eq!(parse(&[]), Ok(Action::Start(Options::default())));
    }

    #[test]
    fn a_port_alone_or_named_and_hosts_are_read() {
        let want = Options {
            open: true,
            port: Some(9000),
            ..Options::default()
        };
        assert_eq!(parse(&args("--open 9000")), Ok(Action::Start(want.clone())));
        assert_eq!(parse(&args("--port 9000 --open")), Ok(Action::Start(want)));
        let Ok(Action::Start(hosts)) = parse(&args("--host 127.0.0.1 --host ::1 --root /x")) else {
            panic!("not a start");
        };
        assert_eq!(hosts.hosts, ["127.0.0.1", "::1"]);
        assert_eq!(hosts.root, Some(PathBuf::from("/x")));
    }

    #[test]
    fn if_up_asks_for_the_running_server_alone() {
        let Ok(Action::Start(options)) = parse(&args("--if-up --open")) else {
            panic!("not a start");
        };
        assert!(options.if_up && options.open);
        assert!(USAGE.contains("--if-up"));
    }

    #[test]
    fn stop_and_help_are_their_own_actions() {
        assert!(matches!(
            parse(&args("--host 127.0.0.1 --stop")),
            Ok(Action::Stop(_))
        ));
        assert_eq!(parse(&args("--help")), Ok(Action::Help));
        assert_eq!(parse(&args("-h")), Ok(Action::Help));
        assert!(USAGE.contains("--port"));
    }

    #[test]
    fn a_server_of_its_own_is_refused_the_one_servers_port_and_the_apps() {
        // The one server on a test's port: that port and the app's are both refused.
        for taken in [41000, crate::home::DEFAULT_PORT] {
            let why = reserved(taken, 41000).expect("a reserved port");
            assert!(
                why.starts_with(&format!("port {taken} is the one server's")),
                "{why}"
            );
        }
        assert_eq!(reserved(41001, 41000), None);
        // The server picks its own port; a one server on a picked port reserves none.
        assert_eq!(reserved(0, 41000), None);
        assert_eq!(reserved(41000, 0), None);
        assert!(reserved(crate::home::DEFAULT_PORT, 0).is_some());
    }

    #[test]
    fn a_bad_line_is_refused() {
        for bad in [
            "--port x",
            "--port",
            "--bogus",
            "1 2",
            "--port 70000",
            "--root",
        ] {
            assert!(parse(&args(bad)).is_err(), "{bad}");
        }
    }
}
