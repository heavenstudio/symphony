//! A start of the one server, and `project add`: the checkout's project registered in the machine
//! folder, the host rule ([`crate::host`]) applied to the running server, then the project's URL,
//! a start of this build, or the hand-off to it. A start in a linked worktree, whichever build it
//! runs, never starts the one server, registers a project with it or replaces it. It prints the
//! running server's page for that worktree when the server serves its project, and is refused
//! otherwise, naming `--port 0`, a server of its own; `--shared` is the owner's word that a start
//! there may start the one server, and the host rule never lets it replace a running one. A
//! worktree of a repository git names no main checkout of (a bare one's) is refused either way.

use std::path::Path;
use std::time::{Duration, Instant};

use symphony::machine::{self, Entry, Registry};

use crate::home::{self, Home, Running};
use crate::host::{Build, Handover, Verdict};
use crate::repo::Checkout;
use crate::sys::Taken;

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
/// else this build started. In a linked worktree it only finds the running server ([`found`]),
/// unless `shared` lets the start there start it.
pub(crate) fn one(
    checkout: &Checkout,
    home: &Home,
    if_up: bool,
    shared: bool,
) -> Result<Started, String> {
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
    if checkout.linked() && checkout.mainless() {
        return Err(format!(
            "{} is a linked worktree of {}, a repository git names no main checkout of for the \
             machine's Symphony to serve: `just symphony --port 0` here serves this worktree on a \
             server of its own",
            checkout.top.display(),
            checkout.common.display()
        ));
    }
    if checkout.linked() && !shared {
        return found(checkout, home);
    }
    let machine = home.dir().to_path_buf();
    let entry = Entry::of(&checkout.main)?;
    let ours = Build::this(&checkout.main).started_in(&checkout.top);
    // Its start read as soon as the look `up` makes finds it answering as its pid: the start of
    // that server, the window between that answer and this read aside.
    let running = home.up().map(|running| {
        let server = Taken::of(running.pid);
        (running, server)
    });
    let front = running
        .as_ref()
        .and_then(|(running, _)| crate::probe::front(running.port, Duration::from_secs(5)));
    match (running, front) {
        (Some((running, _)), Some(front)) if front.pin == 0 => Ok(predates(running, &front)),
        (Some((running, server)), Some(front)) => {
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
                        &server,
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

/// `running`, a server from before the machine folder (`front` says pin 0): it serves its one
/// checkout at `/` and takes no hand-off request; it updates itself from its sources, or a stop and
/// a start replace it.
fn predates(running: Running, front: &symphony_api::Front) -> Started {
    eprintln!(
        "· the running Symphony predates the registry: it serves {} alone, at /; \
         `just symphony --stop`, then `just symphony`, serves every project",
        front.root
    );
    Started::Serving(running, String::new())
}

/// A start in the linked worktree `checkout.top` without `--shared`: the running server, found but
/// only read ([`Home::found`]), when it serves the project registered at the main checkout, the
/// registry only read too; else why nothing was started, naming what to run.
fn found(checkout: &Checkout, home: &Home) -> Result<Started, String> {
    let running = home.found();
    let front = running
        .as_ref()
        .and_then(|running| crate::probe::front(running.port, Duration::from_secs(5)));
    let (Some(running), Some(front)) = (running, front) else {
        return Err(refused(checkout, "no Symphony answers"));
    };
    if front.pin == 0 {
        return Ok(predates(running, &front));
    }
    let registry = Registry::read(home.dir())?;
    let served = registry.projects.iter().find(|entry| {
        entry.root == checkout.main
            && front.projects.contains(&entry.id)
            && Build::of(&front).lacks(entry).is_none()
    });
    match served {
        Some(entry) => Ok(Started::Serving(running, entry.id.clone())),
        None => Err(refused(
            checkout,
            &format!(
                "the running Symphony does not serve {}",
                checkout.main.display()
            ),
        )),
    }
}

/// Why a start in the linked worktree of `checkout` started nothing: what it found, `what`, and
/// what to run instead.
fn refused(checkout: &Checkout, what: &str) -> String {
    format!(
        "{what}, and a start in a linked worktree ({}) never starts the machine's Symphony, \
         registers a project with it or replaces it: `just symphony --port 0` here serves this \
         worktree on a server of its own; `just symphony` in {} has the machine's serve it; \
         `--shared` lets a start here start the machine's, never replacing one that runs",
        checkout.top.display(),
        checkout.main.display()
    )
}

/// Ask `running`, `server` as it was found, to hand its sockets to `request`'s build and wait for
/// the successor to answer, holding `ours`' plugins: the successor's record.
fn hand_off(
    home: &Home,
    running: &Running,
    server: &Taken,
    ours: &Build,
    request: &Handover,
) -> Result<Running, String> {
    request.ask(home.dir())?;
    ask(server, &request.exe)?;
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

/// SIGUSR1 to `server`, the running one, to hand over to `exe`, said on stderr first, while its pid
/// still names it ([`Taken`]): the signal's default action ends a process that does not catch it,
/// so a process given its pid since is sent nothing, and the start is refused.
fn ask(server: &Taken, exe: &Path) -> Result<(), String> {
    let said = || {
        crate::front::tell(format_args!(
            "· Symphony: SIGUSR1 to the server, pid {}: asked to hand over to {}",
            server.pid,
            exe.display()
        ));
    };
    if server.signal(libc::SIGUSR1, said) {
        return Ok(());
    }
    Err(format!(
        "the running Symphony (pid {}) ended before it was asked to hand over; start again",
        server.pid
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphony_board::sys::{Command, Start};

    /// A hand-off asks only the server found: a pid whose start differs from the one recorded when
    /// it was found is sent nothing (SIGUSR1 would end the `sleep` holding it), and the start is
    /// refused; the process found is asked, and said.
    #[test]
    fn a_hand_off_is_asked_of_the_server_found_and_of_no_process_given_its_pid_since() {
        let mut holder = Command::new("/bin/sleep").arg("60").spawn().unwrap();
        let pid = i64::from(holder.id());
        let exe = Path::new("/bin/symphony-server");
        let since = Taken::started(pid, Start::At(1.0));
        let (asked, told) = crate::front::told::during(|| ask(&since, exe));
        assert_eq!(
            asked,
            Err(format!(
                "the running Symphony (pid {pid}) ended before it was asked to hand over; start \
                 again"
            ))
        );
        assert!(told.is_empty(), "{told:?}");
        assert!(holder.try_wait().unwrap().is_none(), "it was signalled");
        let (asked, told) = crate::front::told::during(|| ask(&Taken::of(pid), exe));
        assert_eq!(asked, Ok(()));
        assert_eq!(
            told,
            [format!(
                "· Symphony: SIGUSR1 to the server, pid {pid}: asked to hand over to \
                 /bin/symphony-server"
            )]
        );
        assert_eq!(
            std::os::unix::process::ExitStatusExt::signal(&holder.wait().unwrap()),
            Some(libc::SIGUSR1)
        );
    }
}
