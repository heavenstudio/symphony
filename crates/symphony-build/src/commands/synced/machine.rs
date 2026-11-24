//! What the machine was doing beside a visit: its load averages, the share of its CPU busy, and
//! the memory resident in the server answering the project's address and in the board databases
//! that server holds connections to (each a board's PostgreSQL, with the processes it started),
//! read with `lsof` and `ps` (what a missing one cannot read is said unknown).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

#[cfg(test)]
#[path = "machine_tests.rs"]
mod tests;

/// The machine's load averages over 1, 5 and 15 minutes.
pub(super) fn load_now() -> Option<[f64; 3]> {
    let mut averages = [0.0_f64; 3];
    // SAFETY: getloadavg writes at most `nelem` doubles, and `averages` has room for 3.
    let said = unsafe { libc::getloadavg(averages.as_mut_ptr(), 3) };
    (said == 3).then_some(averages)
}

pub(super) fn load_said(load: Option<[f64; 3]>) -> String {
    load.map_or_else(
        || "load unknown".into(),
        |[one, five, fifteen]| format!("load {one:.0} {five:.0} {fifteen:.0}"),
    )
}

/// The machine's CPU time so far, all its cores', as `[busy, all]` in its own ticks: a visit's
/// share of busy CPU is the difference of two ([`busy_share`]).
#[cfg(target_os = "macos")]
pub(super) fn cpu_ticks() -> Option<[u64; 2]> {
    unsafe extern "C" {
        // libc's own is deprecated in favour of another crate, for this one call.
        fn mach_host_self() -> libc::mach_port_t;
    }
    let mut info = libc::host_cpu_load_info { cpu_ticks: [0; 4] };
    let mut count = libc::HOST_CPU_LOAD_INFO_COUNT;
    // SAFETY: host_statistics writes at most `count` integers (the four states' ticks) into
    // `info`, which has room for them and outlives the call.
    let said = unsafe {
        libc::host_statistics(
            mach_host_self(),
            libc::HOST_CPU_LOAD_INFO,
            (&raw mut info).cast(),
            &raw mut count,
        )
    };
    let [user, system, idle, nice] = info.cpu_ticks.map(u64::from);
    (said == libc::KERN_SUCCESS).then_some([user + system + nice, user + system + idle + nice])
}

/// [`cpu_ticks`] from the first line of `/proc/stat`.
#[cfg(target_os = "linux")]
pub(super) fn cpu_ticks() -> Option<[u64; 2]> {
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    proc_stat_ticks(stat.lines().next()?)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(super) fn cpu_ticks() -> Option<[u64; 2]> {
    None
}

/// `[busy, all]` from `/proc/stat`'s `cpu` line (user nice system idle iowait irq softirq steal…):
/// idle and waiting on a disk are not busy.
#[cfg_attr(
    not(any(target_os = "linux", test)),
    expect(dead_code, reason = "read on Linux alone, and by the tests")
)]
pub(super) fn proc_stat_ticks(line: &str) -> Option<[u64; 2]> {
    let mut words = line.split_whitespace();
    if words.next() != Some("cpu") {
        return None;
    }
    let ticks: Vec<u64> = words.map_while(|word| word.parse().ok()).collect();
    if ticks.len() < 4 {
        return None;
    }
    let all: u64 = ticks.iter().sum();
    let idle = ticks[3] + ticks.get(4).copied().unwrap_or(0);
    Some([all - idle, all])
}

/// The share of the machine's CPU busy between two [`cpu_ticks`], in percent.
pub(super) fn busy_share(before: Option<[u64; 2]>, after: Option<[u64; 2]>) -> Option<f64> {
    let ([busy_before, all_before], [busy_after, all_after]) = (before?, after?);
    let all = all_after.checked_sub(all_before).filter(|all| *all > 0)?;
    let busy = busy_after.checked_sub(busy_before)?;
    #[expect(clippy::cast_precision_loss, reason = "a share of ticks, said whole")]
    Some(busy as f64 * 100.0 / all as f64)
}

/// The memory a visit left: the tab's JavaScript heap in use, and what is resident in the server
/// and, summed, in the board databases it is connected to, each in bytes.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Memory {
    pub(super) heap: Option<f64>,
    pub(super) server: Option<f64>,
    pub(super) databases: Option<f64>,
    /// How many board databases the server was connected to.
    pub(super) database_count: usize,
}

impl Memory {
    /// In a few words, in MB: `heap 12 MB  server 95 MB  board db 423 MB (1)`.
    pub(super) fn said(&self) -> String {
        format!("heap {} MB  {}", mb(self.heap), self.machine_said())
    }

    /// The server's and the board databases' alone: `server 95 MB  board db 423 MB (1)`.
    pub(super) fn machine_said(&self) -> String {
        let mut said = format!(
            "server {} MB  board db {} MB",
            mb(self.server),
            mb(self.databases)
        );
        if self.databases.is_some() {
            let _ = write!(said, " ({})", self.database_count);
        }
        said
    }
}

/// `bytes` in whole MB, or `-` for none.
pub(super) fn mb(bytes: Option<f64>) -> String {
    bytes.map_or_else(|| "-".into(), |bytes| format!("{:.0}", bytes / 1e6))
}

/// The processes whose resident memory a visit records: the one listening on the project's port
/// and the board databases it holds a connection to, each its PostgreSQL server's process and the
/// processes that server started ([`board_servers`], [`connected`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Watched {
    pub(super) server: Option<u32>,
    pub(super) databases: Vec<Vec<u32>>,
}

impl Watched {
    /// Found now, for the server at `base`: none for an address with no port this machine
    /// listens on, or with no lsof to ask; no database where none of its connections reaches one,
    /// or where lsof names no socket's other end, which macOS's does.
    pub(super) fn find(base: &str) -> Watched {
        let Some(port) = port_of(base) else {
            return Watched::default();
        };
        let Some(listening) = run(
            "lsof",
            &["-nP", "-w", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-t"],
        ) else {
            return Watched::default();
        };
        let Some(server) = listening
            .split_whitespace()
            .find_map(|word| word.parse::<u32>().ok())
        else {
            return Watched::default();
        };
        Watched {
            server: Some(server),
            databases: databases_of(server),
        }
    }

    /// What is resident in each now, beside the tab's `heap` its caller read: a database's, all
    /// its processes' summed.
    pub(super) fn memory(&self, heap: Option<f64>) -> Memory {
        let pids: Vec<String> = self
            .server
            .iter()
            .chain(self.databases.iter().flatten())
            .map(u32::to_string)
            .collect();
        let resident = if pids.is_empty() {
            BTreeMap::new()
        } else {
            run("ps", &["-o", "pid=,rss=", "-p", &pids.join(",")])
                .map(|said| resident_bytes(&said))
                .unwrap_or_default()
        };
        let databases: Vec<f64> = (self.databases.iter())
            .filter_map(|processes| {
                let read: Vec<f64> = (processes.iter())
                    .filter_map(|pid| resident.get(pid).copied())
                    .collect();
                (!read.is_empty()).then(|| read.iter().sum())
            })
            .collect();
        Memory {
            heap,
            server: self.server.and_then(|pid| resident.get(&pid).copied()),
            database_count: databases.len(),
            databases: (!databases.is_empty()).then(|| databases.iter().sum()),
        }
    }
}

/// The board databases the process `server` holds a connection to: the other ends of its Unix
/// sockets as lsof names them, matched with the sockets of the processes each board's PostgreSQL
/// started, its connections' backends among them. None where a step says nothing.
fn databases_of(server: u32) -> Vec<Vec<u32>> {
    let Some(peers) = run(
        "lsof",
        &["-nP", "-w", "-a", "-p", &server.to_string(), "-U", "-Fn"],
    )
    .map(|said| peers(&said))
    .filter(|peers| !peers.is_empty()) else {
        return Vec::new();
    };
    let servers = run("ps", &["-axo", "pid=,ppid=,command="])
        .map(|said| board_servers(&said))
        .unwrap_or_default();
    let started: Vec<String> = (servers.iter())
        .flat_map(|processes| processes.iter().skip(1))
        .map(u32::to_string)
        .collect();
    if started.is_empty() {
        return Vec::new();
    }
    let sockets = run(
        "lsof",
        &["-nP", "-w", "-a", "-p", &started.join(","), "-U", "-Fpd"],
    )
    .map(|said| sockets(&said))
    .unwrap_or_default();
    connected(servers, &peers, &sockets)
}

/// What `program` printed, run to its end; none when it cannot run or says nothing.
fn run(program: &str, args: &[&str]) -> Option<String> {
    let done = symphony::children::output(symphony::children::command(program).args(args)).ok()?;
    let said = String::from_utf8_lossy(&done.stdout).into_owned();
    (!said.trim().is_empty()).then_some(said)
}

/// The port an `http://host[:port]/…` address is served on, 80 or 443 when it names none.
pub(super) fn port_of(base: &str) -> Option<u16> {
    let (scheme, rest) = base.split_once("://")?;
    let host = rest.split('/').next().unwrap_or_default();
    let named = host
        .rsplit_once(':')
        .filter(|(_, port)| !port.ends_with(']'));
    match named {
        Some((_, port)) => port.parse().ok(),
        None if scheme == "https" => Some(443),
        None => Some(80),
    }
}

/// The boards' PostgreSQL servers in a `ps -axo pid=,ppid=,command=` listing, each its server on a
/// board's data in a git common dir ([`symphony_board::server::board_served`]) first, then each
/// process that server started, its connections' backends among them.
pub(super) fn board_servers(said: &str) -> Vec<Vec<u32>> {
    let rows: Vec<(u32, u32, &str)> = said
        .lines()
        .filter_map(|line| {
            let (pid, rest) = line.trim_start().split_once(char::is_whitespace)?;
            let (parent, command) = rest.trim_start().split_once(char::is_whitespace)?;
            Some((pid.parse().ok()?, parent.parse().ok()?, command.trim()))
        })
        .collect();
    (rows.iter())
        .filter(|(_, _, command)| symphony_board::server::board_served(command).is_some())
        .map(|(server, _, _)| {
            let started = (rows.iter())
                .filter(|(pid, parent, _)| parent == server && pid != server)
                .map(|(pid, _, _)| *pid);
            std::iter::once(*server).chain(started).collect()
        })
        .collect()
}

/// The other ends of a process's Unix sockets in `lsof -U -Fn`'s output: each `n->ADDRESS`.
pub(super) fn peers(said: &str) -> BTreeSet<String> {
    said.lines()
        .filter_map(|line| line.strip_prefix("n->"))
        .map(str::to_string)
        .collect()
}

/// Each process's own Unix sockets in `lsof -U -Fpd`'s output: under each `pPID`, each `dADDRESS`.
pub(super) fn sockets(said: &str) -> BTreeMap<u32, BTreeSet<String>> {
    let mut sockets: BTreeMap<u32, BTreeSet<String>> = BTreeMap::new();
    let mut process = None;
    for line in said.lines() {
        if let Some(pid) = line.strip_prefix('p') {
            process = pid.parse().ok();
        } else if let (Some(pid), Some(address)) = (process, line.strip_prefix('d')) {
            sockets.entry(pid).or_default().insert(address.to_string());
        }
    }
    sockets
}

/// Of `servers` ([`board_servers`]), the ones a process whose sockets' other ends are `peers` holds
/// a connection to: one of the processes each started owns one of those ends.
pub(super) fn connected(
    servers: Vec<Vec<u32>>,
    peers: &BTreeSet<String>,
    sockets: &BTreeMap<u32, BTreeSet<String>>,
) -> Vec<Vec<u32>> {
    servers
        .into_iter()
        .filter(|processes| {
            processes
                .iter()
                .skip(1)
                .any(|pid| sockets.get(pid).is_some_and(|own| !own.is_disjoint(peers)))
        })
        .collect()
}

/// Each process's resident memory in bytes, from `ps -o pid=,rss=` (in KB).
pub(super) fn resident_bytes(said: &str) -> BTreeMap<u32, f64> {
    said.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let pid = words.next()?.parse::<u32>().ok()?;
            let kb = words.next()?.parse::<f64>().ok()?;
            Some((pid, kb * 1024.0))
        })
        .collect()
}
