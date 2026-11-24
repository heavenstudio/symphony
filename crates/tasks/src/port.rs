//! A child server started on a loopback port of its own, for a child that can neither take a
//! listening socket from its starter nor bind port 0 and say which port it got. The port is
//! reserved by binding port 0 and letting go of it, then the child is told to bind it; another
//! process can take the port in between, and the child then ends saying its address is in use.
//! Only that end starts the child again, on another port, at most a bounded number of times and
//! none after the caller's deadline; a child that ends for any other reason is a failure, with
//! what it said.
//!
//! What counts as up is the starter's own check, and an answer on the port counts only once the
//! child itself holds the port's listener ([`listens`]): a process that took the port answers too,
//! while the child is still on its way to finding the port taken.
//!
//! It uses std alone, so a crate that does not link this one can take the file as a module of its
//! own, and [`listens`] is handed what runs its `lsof`. It starts no child itself: the command is
//! [`Lsof`]'s, the board's from `crate::sys` and std's from the crate that takes the file.

use std::fmt;
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::process::Output;
use std::time::Instant;

/// How many starts are made before a child whose address is taken each time is given up on.
pub const ATTEMPTS: usize = 3;

/// What one start of a child came to.
pub enum Start<T> {
    /// The child answers on the address it was given: what its starter keeps of it.
    Up(T),
    /// The child ended before it answered: everything it said.
    Ended(String),
}

/// Why no child was had.
#[derive(Debug)]
pub enum NotStarted<E> {
    /// No loopback port could be reserved.
    NoPort(std::io::Error),
    /// The starter's own failure, as it gave it: the child could not be run, or ran and never
    /// answered.
    Failed(E),
    /// The child ended for another reason than a taken address: what it said.
    Ended(String),
    /// The child found its address taken on each of `attempts` starts, the deadline or the bound on
    /// starts leaving no other: what the last one said.
    InUse { attempts: usize, output: String },
}

impl<E: fmt::Display> fmt::Display for NotStarted<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotStarted::NoPort(error) => write!(f, "no loopback port could be reserved: {error}"),
            NotStarted::Failed(error) => write!(f, "{error}"),
            NotStarted::Ended(output) => write!(f, "it ended before it answered: {output}"),
            NotStarted::InUse { attempts, output } => write!(
                f,
                "its port was taken on each of {attempts} starts; the last one said: {output}"
            ),
        }
    }
}

/// Whether a child's output says the address it was to listen on is taken: the system's words for
/// `EADDRINUSE`, as Rust's `std::io::Error` prints them (`Address already in use (os error 48)`,
/// 98 on Linux) and Go's (`bind: address already in use`), or the error kind's own name.
pub fn in_use(output: &str) -> bool {
    output
        .to_ascii_lowercase()
        .contains("address already in use")
        || output.contains("AddrInUse")
}

/// Where [`listens`] runs `lsof` when none is found by name: macOS keeps it in `/usr/sbin`, which the
/// `PATH` of a launchd job, a cron line or a stripped agent environment may lack.
pub const LSOF_AT: &str = "/usr/sbin/lsof";

/// Whether the process `pid` itself has a TCP socket listening on `addr`: what makes an answer
/// there that process's. `lsof` is asked about that one process's listening sockets on that address
/// (`-a -p PID -iTCP@ADDR -sTCP:LISTEN`); it exits 1 with nothing to say when there are none. It is
/// run by name, and at [`LSOF_AT`] when none is found by name. An `lsof` that cannot run, or that
/// fails saying why, is the error: no answer, which a caller takes as unknown, never as no listener.
/// `run` runs it and reads what it printed to its end: `crate::sys::output`, which no child another
/// thread starts meanwhile can hold open, or std's `Command::output` in a crate that does not link
/// the board.
pub fn listens<C: Lsof>(
    pid: u32,
    addr: SocketAddr,
    mut run: impl FnMut(&mut C) -> std::io::Result<Output>,
) -> std::io::Result<bool> {
    let host = match addr {
        SocketAddr::V4(v4) => v4.ip().to_string(),
        SocketAddr::V6(v6) => format!("[{}]", v6.ip()),
    };
    let process = pid.to_string();
    let on = format!("-iTCP@{host}:{}", addr.port());
    let args = [
        "-nP",
        "-w",
        "-a",
        "-p",
        &process,
        &on,
        "-sTCP:LISTEN",
        "-Fp",
    ];
    let not_run = |problem: std::io::Error, why: String| std::io::Error::new(problem.kind(), why);
    let out = match run(&mut C::lsof("lsof", &args)) {
        Ok(out) => out,
        Err(missing) if missing.kind() == std::io::ErrorKind::NotFound => {
            run(&mut C::lsof(LSOF_AT, &args)).map_err(|problem| {
                let why = format!(
                    "lsof could not run: none is found on PATH, and {LSOF_AT} could not run \
                     either ({problem})"
                );
                not_run(problem, why)
            })?
        }
        Err(problem) => {
            let why = format!("lsof could not run: {problem}");
            return Err(not_run(problem, why));
        }
    };
    let mine = format!("p{pid}");
    if String::from_utf8_lossy(&out.stdout)
        .lines()
        .any(|line| line == mine)
    {
        return Ok(true);
    }
    let said = String::from_utf8_lossy(&out.stderr);
    if out.status.success() || said.trim().is_empty() {
        return Ok(false);
    }
    Err(std::io::Error::other(format!(
        "lsof could not say what pid {pid} listens on: {}",
        said.trim()
    )))
}

/// The command [`listens`] runs `lsof` as: the board's own (`crate::sys::Command`), which starts it
/// without a fork or the starter's session, or std's, which a crate that takes this file gives.
pub trait Lsof {
    /// `program`, `lsof` by name or [`LSOF_AT`], with `args`.
    fn lsof(program: &str, args: &[&str]) -> Self;
}

/// A loopback address nothing was bound to as it was picked, on none of the ports in `avoid`.
fn reserve(avoid: &[u16]) -> std::io::Result<SocketAddr> {
    pick(avoid, || TcpListener::bind((Ipv4Addr::LOCALHOST, 0)))
}

/// The address of the first listener `bind` gives on none of the ports in `avoid`. A listener on
/// one of them is held while the next is bound, so the system cannot hand its port back.
fn pick(
    avoid: &[u16],
    mut bind: impl FnMut() -> std::io::Result<TcpListener>,
) -> std::io::Result<SocketAddr> {
    let mut held = Vec::new();
    loop {
        let listener = bind()?;
        let addr = listener.local_addr()?;
        if !avoid.contains(&addr.port()) {
            return Ok(addr);
        }
        held.push(listener);
    }
}

/// `start` a child on a reserved loopback address until it is up, and the address it is up on. It is
/// started once at least, and again, on a port not yet tried, only when it ended saying its address
/// was in use, while fewer than `attempts` starts were made and `until` has not passed: `until` is
/// the caller's one deadline for every start, which `start`'s own wait keeps to as well. A starter's
/// error is returned at once.
pub fn start_on_free_port<T, E>(
    attempts: usize,
    until: Instant,
    start: impl FnMut(SocketAddr) -> Result<Start<T>, E>,
) -> Result<(T, SocketAddr), NotStarted<E>> {
    start_on_free_port_except(&[], attempts, until, start)
}

/// As [`start_on_free_port`], never on a port of `except`: the port of a server that ended, which a
/// client may still hold as that server's address.
pub fn start_on_free_port_except<T, E>(
    except: &[u16],
    attempts: usize,
    until: Instant,
    mut start: impl FnMut(SocketAddr) -> Result<Start<T>, E>,
) -> Result<(T, SocketAddr), NotStarted<E>> {
    let mut avoid = except.to_vec();
    let mut tried = 0;
    loop {
        let addr = reserve(&avoid).map_err(NotStarted::NoPort)?;
        avoid.push(addr.port());
        tried += 1;
        match start(addr).map_err(NotStarted::Failed)? {
            Start::Up(up) => return Ok((up, addr)),
            Start::Ended(output) if !in_use(&output) => return Err(NotStarted::Ended(output)),
            Start::Ended(output) if tried >= attempts || Instant::now() >= until => {
                return Err(NotStarted::InUse {
                    attempts: tried,
                    output,
                });
            }
            Start::Ended(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_port_to_avoid_is_held_while_the_next_is_picked_and_never_given() {
        let bound: Vec<TcpListener> = (0..3)
            .map(|_| TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap())
            .collect();
        let ports: Vec<u16> = bound
            .iter()
            .map(|listener| listener.local_addr().unwrap().port())
            .collect();
        let mut given = bound.into_iter();
        let picked = pick(&ports[..2], || Ok(given.next().unwrap())).unwrap();
        assert_eq!(picked.port(), ports[2]);
        assert!(
            given.next().is_none(),
            "no listener was bound past the first free one"
        );
    }

    #[test]
    fn a_failed_bind_is_the_pick_s_error() {
        let refused = pick(&[], || Err(std::io::Error::other("no ports left")));
        assert_eq!(refused.unwrap_err().to_string(), "no ports left");
    }
}
