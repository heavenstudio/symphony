//! A start of the one server, and `project add`: the checkout's project registered in the machine
//! folder, the host rule ([`crate::host`]) applied to the running server, then the project's URL,
//! a start of this build, or the hand-off to it.

use std::path::Path;
use std::time::{Duration, Instant};

use symphony::machine::{self, Entry, Registry};

use crate::home::{self, Home, Running};
use crate::host::{Build, Handover, Verdict};
use crate::repo::Checkout;

/// How long a replaced server is given to hand over and its successor to answer: a build's
/// warm-up on a loaded machine.
const HANDOVER_PATIENCE: Duration = Duration::from_secs(180);

/// The one server of this machine, as a start from `checkout` sees it: the machine folder for its
/// record, this checkout's install folder for its build and client, the old record beside it.
pub(crate) fn home(
    checkout: &Checkout,
    exe: &Path,
    port: u16,
    hosts: Vec<String>,
) -> Result<Home, String> {
    let install = checkout.common.join(home::FOLDER);
    Ok(
        Home::new(&machine::dir()?, exe, &checkout.main, port, hosts)
            .legacy(&install)
            .install(&install),
    )
}

/// What a start of the one server found or made.
pub(crate) enum Started {
    /// The server serving the project, under `/<id>/`, or at `/` for a server before pins.
    Serving(Running, String),
    /// `--if-up` found none, or one this build must replace: the caller builds and starts.
    NotUp,
}

/// Register `checkout`'s project and have the one server serve it: the running one when it does,
/// this build in its place when the host rule says so (`if_up` asks only, and starts nothing),
/// else this build started.
pub(crate) fn one(checkout: &Checkout, home: &Home, if_up: bool) -> Result<Started, String> {
    // A checkout whose state an older build keeps in its old folders registers nothing and starts
    // nothing from here: its board would start empty beside the old one, and its build install
    // beside it. One registered otherwise (an older build's `project add`) is served, and nothing
    // is made in it: its board refuses with the move's line, and its health record is kept in
    // memory (`board_health::file`).
    for (new, _) in symphony_board::unmoved::OLD {
        if let Some(why) = symphony_board::unmoved::refusal(&checkout.common, new) {
            return Err(why);
        }
    }
    let machine = home.dir().to_path_buf();
    let entry = Entry::of(&checkout.main)?;
    let ours = Build::this(&checkout.main);
    let running = home.up();
    let front = running
        .as_ref()
        .and_then(|running| crate::probe::front(running.port, Duration::from_secs(5)));
    match (running, front) {
        (Some(running), Some(front)) if front.pin == 0 => {
            // A server from before the machine folder serves its one checkout at `/` and takes no
            // hand-off request: it updates itself from its sources, or a stop and a start replace it.
            eprintln!(
                "· the running Symphony predates the registry: it serves {} alone, at /; \
                 `just symphony --stop`, then `just symphony`, serves every project",
                front.root
            );
            Ok(Started::Serving(running, String::new()))
        }
        (Some(running), Some(front)) => {
            let registry = Registry::read(&machine)?;
            match crate::host::judge(&Build::of(&front), &ours, &entry, &registry) {
                Verdict::Serves => {
                    register(&machine, &entry)?;
                    Ok(Started::Serving(running, entry.id))
                }
                Verdict::Waits(why) => {
                    register(&machine, &entry)?;
                    Err(why)
                }
                Verdict::Refuse(why) => Err(why),
                Verdict::Replace(_) if if_up => Ok(Started::NotUp),
                Verdict::Replace(why) => {
                    register(&machine, &entry)?;
                    eprintln!("· {why}");
                    let exe = std::env::current_exe()
                        .map_err(|e| format!("cannot find this program: {e}"))?;
                    let install = checkout.common.join(home::FOLDER);
                    let replaced = hand_off(
                        home,
                        &running,
                        &ours,
                        &Handover {
                            exe,
                            root: entry.root.clone(),
                            install,
                        },
                    )?;
                    Ok(Started::Serving(replaced, entry.id))
                }
            }
        }
        _ if if_up => Ok(Started::NotUp),
        _ => {
            register(&machine, &entry)?;
            let running = crate::cli::startup("ensure shared server", || home.ensure())
                .map_err(|e| e.to_string())?;
            Ok(Started::Serving(running, entry.id))
        }
    }
}

fn register(machine: &Path, entry: &Entry) -> Result<(), String> {
    Registry::change(machine, |registry| registry.add(entry.clone())).map(|_| ())
}

/// Ask `running` to hand its sockets to `request`'s build and wait for the successor to answer,
/// holding `ours`' plugins: the successor's record.
fn hand_off(
    home: &Home,
    running: &Running,
    ours: &Build,
    request: &Handover,
) -> Result<Running, String> {
    request.ask(home.dir())?;
    crate::sys::signal(running.pid, libc::SIGUSR1);
    let began = Instant::now();
    while began.elapsed() < HANDOVER_PATIENCE {
        if let Some(front) = crate::probe::front(running.port, Duration::from_secs(2))
            && front.pid != running.pid
            && ours.plugins.iter().all(|name| front.plugins.contains(name))
        {
            return Ok(Running {
                port: running.port,
                pid: front.pid,
                root: front.root,
            });
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!(
        "the running Symphony (pid {}) did not hand over within {} s; its log is {}",
        running.pid,
        HANDOVER_PATIENCE.as_secs(),
        home.dir().join(home::LOG).display()
    ))
}
