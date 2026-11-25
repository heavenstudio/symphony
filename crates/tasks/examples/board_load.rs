//! The board under the work its agents give it, on a scratch board whose server this process
//! starts as the product starts it ([`Home`]) and which ends with it: what each command costs
//! (its wall, the `tasks` process's CPU), what the store's server took for the whole run (its CPU,
//! its memory, its bytes on disk), and how many commands failed.
//!
//!     cargo run -p symphony-board --example board_load -- --home DIR --postgresql PROGRAMS \
//!         --tasks BIN [--database load] [--dump FILE] [--sessions 30] [--calls 40] [--seed 1]
//!     cargo run -p symphony-board --example board_load -- --home DIR --postgresql PROGRAMS \
//!         [--sessions 30] [--calls 40]
//!
//! `--postgresql` names the folder of the machine's programs (`[board] postgresql`). With
//! `--tasks`, it runs the board work below. Without `--tasks`, a session's call is one connection
//! of `psql`'s running `select 1`, each served by a backend of its own: a separate measurement of
//! bare connection cost, whose command CPU is `psql`'s. Both modes start and stop a scratch server
//! through `Home`, the board made on one start and measured on the next, so the copy's CPU and
//! memory are outside the measurement.
//!
//! The server is metered with every process under it ([`tree`]), as PostgreSQL answers from
//! several: its CPU is each one's own and what the children it reaped took, so a backend that
//! ended during the run is counted in its parent's ([`tree_cpu`]), as it opened
//! (`server_opening_cpu_ms`) and over the run (`server_cpu_seconds`); its memory is their resident
//! sizes summed (`server_rss_mib`), which counts a page they share once for each, and what they
//! hold with what they share counted once (`server_memory_mib`, [`shared_once_mib`]), each read
//! off the sessions' threads ([`sample`]). A backend that lives for one connection may have ended
//! between the two readings of one sample, and is in the first alone.
//!
//! DIR is a scratch place ([`scratch`]) that exists, its links followed; the board is kept in `DIR/board`, which a
//! run leaves there (a second run on it starts from the first's rows, so a comparison gives each
//! run a folder of its own). `--dump` copies a board there first, as `tasks-copy --board --out`
//! writes one. It refuses the live database's name and never reaches the live board.
//!
//! The work is a fixed amount, not a time: each of `--sessions` sessions makes `--calls` tool
//! calls back to back, all sessions at once. A tool call beats twice through the hook, as every
//! agent's does (busy before it, ended after), and every other one also runs one board command,
//! picked by [`MIX`] from the session's own sequence of `--seed`: so two runs with one seed run
//! the same commands in each session, whatever store answers them, and their counts, CPU and
//! memory compare at any load of the machine, which only stretches the walls. Each command is the
//! `tasks` binary as an agent runs it, a process of its own, its dispatch made in the write
//! (`TASKS_DISPATCH=inline`), so what a write costs ends with its command. The report is one JSON
//! object on stdout, the machine's load average before and after beside the walls.

use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Barrier, mpsc};
use std::time::{Duration, Instant};
use symphony_board::rows::Dump;
use symphony_board::server::postgres::USER;
use symphony_board::server::{Home, Running, scratch};
use symphony_board::sys;
use symphony_board::{DATABASE, NewSession, NewTask, Opening, Store};

// How it makes a channel ([`test_channel::channel`]): std's, as the board's tests do.
#[path = "../tests/support/test_channel.rs"]
mod test_channel;

type Failed = Box<dyn std::error::Error>;

/// The board commands a tool call runs, each with its share in a hundred: what agents ran on the
/// live board when the stores were compared. A cycle is one step of [`CYCLE`] on the session's
/// own task, in turn.
const MIX: [(Kind, u64); 7] = [
    (Kind::Inbox, 30),
    (Kind::Say, 20),
    (Kind::Show, 15),
    (Kind::Cycle, 15),
    (Kind::List, 7),
    (Kind::Next, 6),
    (Kind::Pull, 7),
];

/// The sessions one orchestrator holds: under the default cap on an orchestrator's open work.
const PER_ORCHESTRATOR: usize = 5;

/// The server's memory is read once in this many commands ended: a count of the work, no clock.
const SAMPLE_EVERY: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Beat,
    Inbox,
    Say,
    Show,
    Cycle,
    List,
    Next,
    Pull,
    /// A connection to a bare PostgreSQL, running `select 1`.
    Connect,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Beat => "beat",
            Kind::Inbox => "inbox",
            Kind::Say => "say",
            Kind::Show => "show",
            Kind::Cycle => "cycle",
            Kind::List => "list",
            Kind::Next => "next",
            Kind::Pull => "pull",
            Kind::Connect => "connect",
        }
    }

    /// Whether the command writes the board: the beats, an inbox (its acknowledgement), a say and
    /// a cycle's step.
    fn writes(self) -> bool {
        matches!(self, Kind::Beat | Kind::Inbox | Kind::Say | Kind::Cycle)
    }
}

/// The command a roll of 0 to 99 picks from [`MIX`].
fn pick(roll: u64) -> Kind {
    let mut below = 0;
    for (kind, share) in MIX {
        below += share;
        if roll < below {
            return kind;
        }
    }
    MIX[MIX.len() - 1].0
}

/// SplitMix64: a session's sequence, the same for a seed on every machine.
struct Sequence(u64);

impl Sequence {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// What each of a session's `calls` tool calls runs beside its two beats: a board command, for
/// every other call, or none.
fn schedule(seed: u64, session: usize, calls: usize) -> Vec<Option<Kind>> {
    let mut sequence = Sequence(seed ^ (session as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93));
    (0..calls)
        .map(|call| (call % 2 == 0).then(|| pick(sequence.next() % 100)))
        .collect()
}

/// The value at quantile `q` of `sorted`, by nearest rank; none of an empty one.
fn quantile(sorted: &[f64], q: f64) -> Option<f64> {
    let rank = (q * sorted.len() as f64).ceil() as usize;
    sorted.get(rank.clamp(1, sorted.len().max(1)) - 1).copied()
}

/// `ps -axo pid=,ppid=,rss=` read: each process, its parent and its resident kilobytes. A line
/// that is not three numbers is passed over.
fn processes(ps: &str) -> Vec<(i64, i64, u64)> {
    ps.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let row = (
                fields.next()?.parse().ok()?,
                fields.next()?.parse().ok()?,
                fields.next()?.parse().ok()?,
            );
            fields.next().is_none().then_some(row)
        })
        .collect()
}

/// This machine's process table now ([`processes`]).
fn table() -> Option<Vec<(i64, i64, u64)>> {
    let said = sys::output(sys::command("ps").args(["-axo", "pid=,ppid=,rss="])).ok()?;
    Some(processes(&String::from_utf8_lossy(&said.stdout)))
}

/// `root` and every process under it in `rows`, `root` first; none when `root` is not among them.
fn tree(rows: &[(i64, i64, u64)], root: i64) -> Option<Vec<i64>> {
    rows.iter().find(|row| row.0 == root)?;
    let mut found = vec![root];
    let mut at = 0;
    while let Some(&parent) = found.get(at) {
        found.extend(
            (rows.iter())
                .filter(|row| row.1 == parent && row.0 != root)
                .map(|row| row.0),
        );
        at += 1;
    }
    Some(found)
}

/// The resident kilobytes of `root` and every process under it, or none when `root` is not among
/// `rows`. Memory two of them share is counted for each: a server that answers from processes of
/// its own over shared buffers reads higher here than it holds ([`shared_once_mib`] does not).
fn tree_rss(rows: &[(i64, i64, u64)], root: i64) -> Option<u64> {
    let tree = tree(rows, root)?;
    Some(
        (rows.iter())
            .filter(|row| tree.contains(&row.0))
            .map(|row| row.2)
            .sum(),
    )
}

/// The CPU the processes of `tree` took, its root first, as `read` gives each one's own and what
/// the children it reaped took: a process that ended and was reaped is in its parent's, as a
/// backend that served one connection is in its postmaster's. One that `read` no longer finds
/// ended after the table was read, and is passed over: its parent counts it once it reaps it. The
/// root's is needed. A reading taken while processes end can miss one, or count it twice, so the
/// run reads it when the server is quiet: as it opened and once every command has ended.
fn tree_cpu(tree: &[i64], read: impl Fn(i64) -> Option<(Duration, Duration)>) -> Option<Duration> {
    let (root, rest) = tree.split_first()?;
    let (own, reaped) = read(*root)?;
    let rest = rest.iter().filter_map(|pid| read(*pid));
    Some(own + reaped + rest.map(|(own, reaped)| own + reaped).sum::<Duration>())
}

/// The CPU the server `pid` took since it started, with every process under it ([`tree_cpu`]).
fn server_cpu(pid: i64) -> Option<Duration> {
    let tree = tree(&table()?, pid)?;
    tree_cpu(&tree, |pid| {
        let pid = u32::try_from(pid).ok()?;
        Some((
            test_time::cpu::of(pid).ok()?,
            test_time::cpu::reaped(pid).ok()?,
        ))
    })
}

/// What the processes of `tree` hold, in MiB, with what they share counted once: on macOS
/// `footprint` of them all, the memory they dirtied, which the system cannot drop, the pages two
/// of them map counted once ([`footprint_bytes`]); on Linux their proportional set sizes summed,
/// each page they share divided among those that map it ([`pss_kib`]). Each reads one process as
/// it holds, and the two read a server differently, so a comparison is made on one machine.
#[cfg(target_os = "macos")]
fn shared_once_mib(tree: &[i64]) -> Option<f64> {
    let mut command = sys::command("footprint");
    command.args(["-f", "bytes", "--noCategories"]);
    for pid in tree {
        command.arg("-p").arg(pid.to_string());
    }
    let said = sys::output(&mut command).ok()?;
    footprint_bytes(&String::from_utf8_lossy(&said.stdout)).map(|bytes| bytes as f64 / 1048576.0)
}

#[cfg(target_os = "linux")]
fn shared_once_mib(tree: &[i64]) -> Option<f64> {
    let pss =
        |pid: &i64| pss_kib(&std::fs::read_to_string(format!("/proc/{pid}/smaps_rollup")).ok()?);
    let (root, rest) = tree.split_first()?;
    let kib = pss(root)? + rest.iter().filter_map(pss).sum::<u64>();
    Some(kib as f64 / 1024.0)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn shared_once_mib(_: &[i64]) -> Option<f64> {
    None
}

/// The bytes `footprint -f bytes` says its processes hold: its summary's, the last line naming a
/// footprint, when it found more than one; else the one it found's. A process that ended before
/// it looked is left out of both, as it holds nothing.
#[cfg(any(target_os = "macos", test))]
fn footprint_bytes(said: &str) -> Option<u64> {
    let line = said.lines().rfind(|line| line.contains("Footprint: "))?;
    let mut words = line.split_once("Footprint: ")?.1.split_whitespace();
    let bytes = words.next()?.parse().ok()?;
    (words.next() == Some("B")).then_some(bytes)
}

/// The kibibytes `/proc/PID/smaps_rollup` gives as the process's proportional set size (`Pss:`).
#[cfg(any(target_os = "linux", test))]
fn pss_kib(rollup: &str) -> Option<u64> {
    let line = rollup.lines().find_map(|line| line.strip_prefix("Pss:"))?;
    let mut words = line.split_whitespace();
    let kib = words.next()?.parse().ok()?;
    (words.next() == Some("kB")).then_some(kib)
}

/// The server's memory at one moment, in MiB, with every process under it: resident, summed
/// ([`tree_rss`]), and with what they share counted once ([`shared_once_mib`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Reading {
    resident: Option<f64>,
    once: Option<f64>,
}

impl Reading {
    fn of(server: i64) -> Reading {
        let Some(rows) = table() else {
            return Reading::default();
        };
        Reading {
            resident: tree_rss(&rows, server).map(|kib| kib as f64 / 1024.0),
            once: tree(&rows, server).and_then(|tree| shared_once_mib(&tree)),
        }
    }
}

/// One session's count of the commands ended, which asks the sampler ([`sample`]) for a reading
/// once in [`SAMPLE_EVERY`] of them, the run's, whichever session ends it: a count of the work, no
/// clock.
struct Ended<'run> {
    count: &'run AtomicUsize,
    due: mpsc::Sender<()>,
}

impl Ended<'_> {
    fn one(&self) {
        if self
            .count
            .fetch_add(1, Ordering::Relaxed)
            .is_multiple_of(SAMPLE_EVERY)
        {
            // The sampler is there until every session's count has gone.
            let _ = self.due.send(());
        }
    }
}

/// A reading by `read` for each one `asked` hears, off the sessions' threads, so a slow one
/// (`footprint` of a server of forty processes takes a tenth of a second) stretches no command:
/// those asked while one was read are taken by the next, as one. Until every session's count has
/// gone.
fn sample<T>(asked: &mpsc::Receiver<()>, mut read: impl FnMut() -> T) -> Vec<T> {
    let mut readings = Vec::new();
    while asked.recv().is_ok() {
        while asked.try_recv().is_ok() {}
        readings.push(read());
    }
    readings
}

/// The bytes of every file under `folder`.
fn bytes_under(folder: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.metadata() {
            Ok(meta) if meta.is_dir() => bytes_under(&entry.path()),
            Ok(meta) => meta.len(),
            Err(_) => 0,
        })
        .sum()
}

fn arg<'args>(args: &'args [String], name: &str) -> Option<&'args str> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
}

fn number(args: &[String], name: &str, default: usize) -> Result<usize, String> {
    arg(args, name).map_or(Ok(default), |text| {
        text.parse().map_err(|e| format!("{name} {text}: {e}"))
    })
}

/// The server a run measures: the board's PostgreSQL, from the folder of its `programs`, under
/// the work of the `tasks` binary at `tasks`, or bare with none.
#[derive(Debug, PartialEq)]
struct Server {
    programs: PathBuf,
    tasks: Option<PathBuf>,
}

/// What the command line asks for.
#[derive(Debug)]
struct Options {
    home: PathBuf,
    server: Server,
    database: String,
    dump: Option<PathBuf>,
    sessions: usize,
    calls: usize,
    seed: u64,
}

impl Options {
    fn parse(args: &[String]) -> Result<Options, String> {
        let needed = |name: &str| arg(args, name).ok_or(format!("{name} is required"));
        // Where the folder is, its links followed: one under the temporary folder that leads
        // elsewhere is elsewhere.
        let named = needed("--home")?;
        let home = Path::new(named)
            .canonicalize()
            .map_err(|why| format!("--home {named} does not exist: {why}"))?;
        if !scratch(&home) {
            return Err(format!(
                "--home {} is not a scratch place: the temporary folder, or a checkout's artifacts/",
                home.display()
            ));
        }
        let database = arg(args, "--database").unwrap_or("load");
        if database == DATABASE || !symphony_board::db::plain(database) {
            return Err(format!(
                "--database {database}: a plain name, never the live board's"
            ));
        }
        let sessions = number(args, "--sessions", 30)?;
        if sessions == 0 {
            return Err("--sessions 0: nothing would run".into());
        }
        let programs = needed("--postgresql")?;
        let tasks = arg(args, "--tasks").map(PathBuf::from);
        if arg(args, "--dump").is_some() && tasks.is_none() {
            return Err("--dump with --postgresql needs --tasks to run the board".into());
        }
        let server = Server {
            programs: programs.into(),
            tasks,
        };
        Ok(Options {
            home,
            server,
            database: database.into(),
            dump: arg(args, "--dump").map(PathBuf::from),
            sessions,
            calls: number(args, "--calls", 40)?,
            seed: number(args, "--seed", 1)? as u64,
        })
    }
}

/// One session of the run: its handle, the key its beats are bound by, and its own task.
struct Session {
    handle: String,
    key: String,
    slug: String,
}

/// The sessions, each bound to its key and with a ready task of its own that one of the run's
/// orchestrators holds, written with no dispatch of their own.
fn set_up(options: &Options, store: Store) -> Result<Vec<Session>, Failed> {
    let quiet = store.dispatching_by(Box::new(|| true));
    let run = std::process::id();
    let register = |kind: &str| -> Result<String, Failed> {
        let mut new = NewSession::new(kind);
        new.host = sys::hostname();
        Ok(quiet.register(&new)?.row.handle)
    };
    let mut orchestrator = String::new();
    let mut sessions = Vec::new();
    for n in 0..options.sessions {
        if n % PER_ORCHESTRATOR == 0 {
            orchestrator = register("orchestrator")?;
        }
        let handle = register("worker")?;
        let slug = format!("load-{run}/s-{n}");
        let mut task = NewTask::new(&slug, "board load", &orchestrator);
        task.status = "ready".into();
        quiet.create(&task)?;
        quiet.take_role(&slug, &orchestrator, "orchestrator")?;
        let key = format!("load-{run}-{n}");
        quiet.bind_agent(&key, &handle, false)?;
        sessions.push(Session { handle, key, slug });
    }
    Ok(sessions)
}

/// What every command of the run is started with: the program, the `tasks` binary or `psql`.
struct Runner {
    program: PathBuf,
    /// The whole environment of each command ([`Runner::vars`]).
    vars: BTreeMap<String, String>,
    /// The folder the commands run in, `run` in the run's home, with an empty process table. Under
    /// the temporary folder it is in no repository; under a checkout's `artifacts/` it is in that
    /// checkout, whose board no command reaches: each names the run's server and database.
    folder: PathBuf,
}

/// One command, ended.
struct Ran {
    kind: Kind,
    wall: Duration,
    cpu: Duration,
    /// What it said on stderr when it failed.
    failed: Option<String>,
}

impl Runner {
    /// The environment of a command on the board `database` of the server at `server`, run in
    /// `folder`: nothing of this process's but its `path`, the board named, its dispatch in the
    /// write, no sweep, and no terminal or process table of this machine's.
    fn vars(server: &str, database: &str, folder: &Path, path: &str) -> BTreeMap<String, String> {
        let folder = folder.display().to_string();
        [
            ("PATH", path.to_string()),
            ("HOME", folder.clone()),
            ("SYMPHONY_MACHINE_DIR", format!("{folder}/machine")),
            ("TASKS_SERVER", server.to_string()),
            ("TASKS_DATABASE", database.to_string()),
            ("TASKS_DISPATCH", "inline".to_string()),
            ("TASKS_SWEEP", "off".to_string()),
            ("TASKS_PS_TABLE", format!("{folder}/ps")),
            ("HC_BIN", "/usr/bin/false".to_string()),
            ("HERDR_APP_BIN", "/usr/bin/false".to_string()),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect()
    }

    /// The program with `args` and `input` on its stdin, read to its end and reaped: its wall and
    /// the CPU it took with every child it reaped.
    fn run(&self, kind: Kind, args: &[&str], input: &str) -> Ran {
        let mut command = sys::command_with(&self.program, &self.vars);
        command.args(args).current_dir(&self.folder);
        let began = Instant::now();
        let ended = sys::spawn_piped(&mut command).and_then(|mut child| {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(input.as_bytes());
            }
            let stderr = child.stderr.take().map(|mut stderr| {
                std::thread::spawn(move || {
                    let mut said = String::new();
                    let _ = stderr.read_to_string(&mut said);
                    said
                })
            });
            if let Some(mut stdout) = child.stdout.take() {
                let _ = std::io::copy(&mut stdout, &mut std::io::sink());
            }
            let said = stderr.and_then(|reader| reader.join().ok());
            let (status, cpu) = test_time::cpu::reap(child.id())?;
            Ok((status, cpu, said.unwrap_or_default()))
        });
        let wall = began.elapsed();
        match ended {
            Ok((status, cpu, said)) => Ran {
                kind,
                wall,
                cpu,
                failed: (!status.success()).then(|| said.trim().to_string()),
            },
            Err(why) => Ran {
                kind,
                wall,
                cpu: Duration::ZERO,
                failed: Some(format!("the command did not run: {why}")),
            },
        }
    }

    /// A tool call's beat as the hook sends it: before the call (`pre`) or after it.
    fn beat(&self, session: &Session, call: usize, pre: bool) -> Ran {
        let event = json!({
            "hook_event_name": if pre { "PreToolUse" } else { "PostToolUse" },
            "agent_id": session.key,
            "tool_name": "Bash",
            "tool_use_id": format!("call-{call}"),
            "tool_input": {"command": "true"},
            "cwd": self.folder,
        });
        self.run(Kind::Beat, &["beat"], &event.to_string())
    }

    /// The board command `kind` as `session` runs it; a cycle runs its step `turn`.
    fn board(&self, session: &Session, kind: Kind, turn: usize) -> Ran {
        let (slug, handle) = (session.slug.as_str(), session.handle.as_str());
        let args: Vec<&str> = match kind {
            Kind::Beat => unreachable!("a beat is the hook's, not a board command"),
            Kind::Connect => unreachable!("a connection is a bare server's, not a board command"),
            Kind::Inbox => vec!["inbox"],
            Kind::Say => vec!["say", slug, SAID, "--to", "@orchestrator"],
            Kind::Show => vec!["show", slug],
            Kind::Cycle => CYCLE[turn % CYCLE.len()]
                .iter()
                .map(|word| word.at(slug))
                .collect(),
            Kind::List => vec!["list"],
            Kind::Next => vec!["next"],
            Kind::Pull => vec!["pull"],
        };
        let mut named = args;
        named.extend(["--as", handle]);
        self.run(kind, &named, "")
    }
}

/// A word of a cycle's step: itself, or the session's task.
enum Word {
    Is(&'static str),
    Task,
}

impl Word {
    fn at<'a>(&'a self, slug: &'a str) -> &'a str {
        match self {
            Word::Is(word) => word,
            Word::Task => slug,
        }
    }
}

/// A cycle's steps in turn: the task claimed, moved to review, taken back to work and let go,
/// which leaves it ready, where the next claim finds it.
const CYCLE: [&[Word]; 4] = [
    &[Word::Is("claim"), Word::Task],
    &[
        Word::Is("move"),
        Word::Task,
        Word::Is("review"),
        Word::Is("--reason"),
        Word::Is("load"),
    ],
    &[
        Word::Is("move"),
        Word::Task,
        Word::Is("in_progress"),
        Word::Is("--reason"),
        Word::Is("load"),
    ],
    &[Word::Is("release"), Word::Task],
];

/// A say's line: about 280 characters, as an agent's milestone runs.
const SAID: &str = "the slice is built and its focused tests are green at the tip: the store's \
    tests, the command line's and the lint, each with its command and its result on this thread; \
    the client is not touched and no release input changed, so nothing waits on a distribution, \
    and what is left out is named below with why";

/// One session's tool calls, back to back, each command's end counted on `ended`.
fn work(runner: &Runner, session: &Session, plan: &[Option<Kind>], ended: &Ended) -> Vec<Ran> {
    let mut ran = Vec::new();
    let mut turn = 0;
    let count = |ran: &mut Vec<Ran>, one: Ran| {
        ran.push(one);
        ended.one();
    };
    for (call, command) in plan.iter().enumerate() {
        count(&mut ran, runner.beat(session, call, true));
        if let Some(kind) = *command {
            let one = runner.board(session, kind, turn);
            // A cycle goes on to its next step only once this one was done.
            turn += usize::from(kind == Kind::Cycle && one.failed.is_none());
            count(&mut ran, one);
        }
        count(&mut ran, runner.beat(session, call, false));
    }
    ran
}

fn milliseconds(time: Duration) -> f64 {
    time.as_secs_f64() * 1000.0
}

fn rounded(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// What `ran` came to: how many, how many failed, the walls' quantiles and the CPU of the
/// `tasks` processes, in milliseconds.
fn summary(ran: &[&Ran]) -> Value {
    let mut walls: Vec<f64> = ran.iter().map(|one| milliseconds(one.wall)).collect();
    walls.sort_by(f64::total_cmp);
    let cpu: f64 = ran.iter().map(|one| milliseconds(one.cpu)).sum();
    let at = |q: f64| quantile(&walls, q).map(rounded);
    json!({
        "n": ran.len(),
        "failed": ran.iter().filter(|one| one.failed.is_some()).count(),
        "wall_ms": {"p50": at(0.5), "p95": at(0.95), "p99": at(0.99), "max": at(1.0)},
        "tasks_cpu_ms": {
            "total": rounded(cpu),
            "mean": (!ran.is_empty()).then(|| rounded(cpu / ran.len() as f64)),
        },
    })
}

/// Each kind's [`summary`], and every command's under `all`.
fn by_kind(ran: &[Ran]) -> Value {
    let mut kinds: BTreeMap<Kind, Vec<&Ran>> = BTreeMap::new();
    for one in ran {
        kinds.entry(one.kind).or_default().push(one);
    }
    let mut said: serde_json::Map<String, Value> = kinds
        .iter()
        .map(|(kind, ran)| (kind.name().to_string(), summary(ran)))
        .collect();
    said.insert("all".into(), summary(&ran.iter().collect::<Vec<_>>()));
    Value::Object(said)
}

/// The failures' texts, each once with how often, the most frequent first, at most ten.
fn failures(ran: &[Ran]) -> Vec<Value> {
    let mut texts: BTreeMap<(Kind, &str), usize> = BTreeMap::new();
    for one in ran {
        if let Some(text) = &one.failed {
            *texts.entry((one.kind, text.as_str())).or_default() += 1;
        }
    }
    let mut counted: Vec<_> = texts.into_iter().collect();
    counted.sort_by_key(|(_, times)| std::cmp::Reverse(*times));
    counted
        .into_iter()
        .take(10)
        .map(|((kind, text), times)| json!({"command": kind.name(), "times": times, "said": text}))
        .collect()
}

/// The server's memory in MiB, by one reading of it: as the run found it, right after its start (`opened`),
/// the samples' median, once every command had ended (`after`), and the peak: the highest of
/// every reading, the first and the last among them, as a server that gives nothing back is at
/// its highest when the run ends, after the last sample.
fn memory(opened: Option<f64>, samples: &[f64], after: Option<f64>) -> Value {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let peak = (samples.iter().copied())
        .chain(opened)
        .chain(after)
        .max_by(f64::total_cmp);
    json!({
        "opened": opened.map(rounded),
        "median": quantile(&sorted, 0.5).map(rounded),
        "peak": peak.map(rounded),
        "after": after.map(rounded),
        "samples": sorted.len(),
    })
}

/// When the board last saw each session, by handle.
fn last_seen(store: &Store) -> Result<BTreeMap<String, f64>, Failed> {
    let sessions = store.sessions()?;
    Ok((sessions.into_iter())
        .map(|session| (session.row.handle, session.row.last_seen))
        .collect())
}

/// How many of `handles` the board saw later in `after` than in `before`: the sessions whose
/// beats reached it, which a beat's exit never says (it is always 0).
fn beaten(
    before: &BTreeMap<String, f64>,
    after: &BTreeMap<String, f64>,
    handles: &[&str],
) -> usize {
    let later = |handle: &&&str| match (before.get(**handle), after.get(**handle)) {
        (Some(before), Some(after)) => after > before,
        _ => false,
    };
    handles.iter().filter(later).count()
}

/// Each of `sessions` sessions' `work` run at once, given its number and its count of commands
/// ended, with the server `server` measured around it: its CPU with every process under it
/// ([`server_cpu`]) as it opened and over the run, its memory ([`Reading`]) as it opened, once in
/// [`SAMPLE_EVERY`] commands ended and once all had, the bytes of `dir` before and after, and the
/// machine's load average. The commands' counts and costs, and those, as the report's fields.
fn measure(
    server: i64,
    dir: &Path,
    sessions: usize,
    work: impl Fn(usize, &Ended) -> Vec<Ran> + Sync,
) -> Result<(Vec<Ran>, f64, Value), Failed> {
    let unread = || format!("the CPU of the server {server} could not be read");
    let load_before = test_time::wait::load();
    let (bytes_before, opened) = (bytes_under(dir), Reading::of(server));
    let opening = server_cpu(server).ok_or_else(unread)?;
    let count = AtomicUsize::new(0);
    let (due, asked) = crate::test_channel::channel();
    let barrier = Barrier::new(sessions);
    let began = Instant::now();
    let (ran, wall, samples) = std::thread::scope(|scope| {
        let sampler = scope.spawn(move || sample(&asked, || Reading::of(server)));
        let threads: Vec<_> = (0..sessions)
            .map(|n| {
                let ended = Ended {
                    count: &count,
                    due: due.clone(),
                };
                let (work, barrier) = (&work, &barrier);
                scope.spawn(move || {
                    barrier.wait();
                    work(n, &ended)
                })
            })
            .collect();
        drop(due);
        let ran: Vec<Ran> = (threads.into_iter())
            .flat_map(|thread| thread.join().expect("a session's thread ends"))
            .collect();
        let wall = began.elapsed().as_secs_f64();
        (ran, wall, sampler.join().expect("the sampler ends"))
    });
    let cpu = server_cpu(server).ok_or_else(unread)?;
    let after = Reading::of(server);
    let read = |field: fn(&Reading) -> Option<f64>| -> Vec<f64> {
        samples.iter().filter_map(field).collect()
    };
    let done = ran.iter().filter(|one| one.failed.is_none()).count();
    let report = json!({
        "commands": by_kind(&ran),
        "failures": failures(&ran),
        "wall_seconds": rounded(wall),
        "commands_per_second": rounded(done as f64 / wall),
        "server_opening_cpu_ms": rounded(milliseconds(opening)),
        "server_cpu_seconds": rounded(cpu.saturating_sub(opening).as_secs_f64()),
        "server_rss_mib": memory(opened.resident, &read(|one| one.resident), after.resident),
        "server_memory_mib": memory(opened.once, &read(|one| one.once), after.once),
        "store_bytes": {"before": bytes_before, "after": bytes_under(dir)},
        "load_average": {"before": load_before, "after": test_time::wait::load()},
    });
    Ok((ran, wall, report))
}

fn main() -> Result<(), Failed> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = Options::parse(&args)?;
    let Server { programs, tasks } = &options.server;
    match tasks {
        None => postgresql_run(&options, programs),
        Some(tasks) => postgresql_board_run(&options, programs, tasks),
    }
}

/// The board's PostgreSQL bare, from the folder of `programs`, its data made on one start and
/// measured on the next, as a board's is: each session's calls are connections of
/// `psql`'s running `select 1` back to back, each served by a backend of its own that ends with
/// it. Stopped once it is measured.
fn postgresql_run(options: &Options, programs: &Path) -> Result<(), Failed> {
    let dir = options.home.join("board");
    let pg = Home::at(&dir).postgresql(programs);
    pg.ensure().map_err(|why| why.to_string())?;
    pg.stop().map_err(|why| why.to_string())?;
    let serving = pg.ensure().map_err(|why| why.to_string())?;
    let folder = options.home.join("run");
    std::fs::create_dir_all(&folder)?;
    let runner = Runner {
        program: pg.program("psql"),
        vars: [
            ("PATH", std::env::var("PATH").unwrap_or_default()),
            ("HOME", folder.display().to_string()),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect(),
        folder,
    };
    let socket = serving.socket.display().to_string();
    let connect = [
        "-X", "-q", "-h", &socket, "-U", USER, "-d", "postgres", "-c", "select 1",
    ];
    let measured = measure(serving.pid, &dir, options.sessions, |_, ended| {
        (0..options.calls)
            .map(|_| {
                let one = runner.run(Kind::Connect, &connect, "");
                ended.one();
                one
            })
            .collect()
    });
    let stopped = pg.stop().map_err(|why| why.to_string());
    let (_, _, mut report) = measured?;
    for (field, value) in [
        (
            "server",
            json!({"store": "PostgreSQL", "version": serving.version}),
        ),
        ("sessions", json!(options.sessions)),
        ("calls", json!(options.calls)),
    ] {
        report[field] = value;
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    stopped?;
    Ok(())
}

/// The board work over PostgreSQL, the `tasks` binary at `tasks` running each command.
fn postgresql_board_run(options: &Options, programs: &Path, tasks: &Path) -> Result<(), Failed> {
    let pg = Home::at(&options.home.join("board")).postgresql(programs);
    let open = || -> Result<Store, Failed> {
        Ok(Store::opening_on(
            symphony_board::pg::Client::new(&pg.socket(), &options.database).into(),
            Box::new(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs_f64()
            }),
            Box::new(sys::pid_alive),
            &sys::hostname(),
            &Opening::write(),
        )?)
    };
    pg.ensure().map_err(|why| why.to_string())?;
    let store = open()?;
    let mut board = json!({"tasks": 0, "messages": 0, "sessions": 0});
    if let Some(file) = &options.dump {
        let dump = Dump::from_json(&std::fs::read_to_string(file)?)?;
        board = json!({"tasks": dump.tasks.len(), "messages": dump.messages.len(), "sessions": dump.sessions.len()});
        let began = Instant::now();
        store.copy(&dump)?;
        eprintln!(
            "board_load: copied {board} in {:.1} s",
            began.elapsed().as_secs_f64()
        );
    }
    let sessions = set_up(options, store)?;
    pg.stop().map_err(|why| why.to_string())?;
    let serving = pg.ensure().map_err(|why| why.to_string())?;
    let running = Running {
        pid: serving.pid,
        addr: format!("postgresql:{}", serving.socket.display()),
        version: serving.version,
    };
    let measured = board_run(
        options,
        tasks,
        &running,
        open()?,
        board,
        sessions,
        "PostgreSQL",
    );
    let stopped = pg.stop().map_err(|why| why.to_string());
    measured?;
    stopped?;
    Ok(())
}

fn board_run(
    options: &Options,
    tasks: &Path,
    running: &Running,
    store: Store,
    board: Value,
    sessions: Vec<Session>,
    backend: &str,
) -> Result<(), Failed> {
    let folder = options.home.join("run");
    std::fs::create_dir_all(&folder)?;
    std::fs::write(folder.join("ps"), "")?;
    let runner = Runner {
        program: tasks.into(),
        vars: Runner::vars(
            &running.addr,
            &options.database,
            &folder,
            &std::env::var("PATH").unwrap_or_default(),
        ),
        folder,
    };
    let handles: Vec<&str> = (sessions.iter())
        .map(|session| session.handle.as_str())
        .collect();
    let seen_before = last_seen(&store)?;
    let (ran, wall, mut report) = measure(
        running.pid,
        &options.home.join("board"),
        sessions.len(),
        |n, ended| {
            let plan = schedule(options.seed, n, options.calls);
            work(&runner, &sessions[n], &plan, ended)
        },
    )?;
    let beaten = beaten(&seen_before, &last_seen(&store)?, &handles);
    let writes = (ran.iter())
        .filter(|one| one.failed.is_none() && one.kind.writes())
        .count();
    let tasks_cpu: f64 = ran.iter().map(|one| one.cpu.as_secs_f64()).sum();
    for (field, value) in [
        (
            "server",
            json!({"store": backend, "version": running.version}),
        ),
        ("board", board),
        ("sessions", json!(options.sessions)),
        ("calls", json!(options.calls)),
        ("seed", json!(options.seed)),
        ("sessions_beaten", json!(beaten)),
        ("writes", json!(writes)),
        ("writes_per_second", json!(rounded(writes as f64 / wall))),
        ("tasks_cpu_seconds", json!(rounded(tasks_cpu))),
    ] {
        report[field] = value;
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    if options.calls > 0 && beaten < options.sessions {
        let missed = options.sessions - beaten;
        return Err(format!(
            "the beats of {missed} of {} sessions never reached the board: the run measured \
             less than it says",
            options.sessions
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| word.to_string()).collect()
    }

    #[test]
    fn the_mix_is_a_hundred_shares_and_a_roll_picks_each_command_by_its_share() {
        assert_eq!(MIX.iter().map(|(_, share)| share).sum::<u64>(), 100);
        let mut picked: BTreeMap<Kind, u64> = BTreeMap::new();
        for roll in 0..100 {
            *picked.entry(pick(roll)).or_default() += 1;
        }
        assert_eq!(picked, MIX.into_iter().collect());
        assert_eq!(pick(0), Kind::Inbox);
        assert_eq!(pick(29), Kind::Inbox);
        assert_eq!(pick(30), Kind::Say);
        assert_eq!(pick(99), Kind::Pull);
        // A roll past the shares, which `% 100` never makes, is the last command's.
        assert_eq!(pick(100), Kind::Pull);
        assert!(!MIX.iter().any(|(kind, _)| *kind == Kind::Beat));
    }

    #[test]
    fn a_schedule_is_the_same_for_a_seed_and_a_command_every_other_call() {
        let first = schedule(1, 3, 40);
        assert_eq!(first, schedule(1, 3, 40));
        assert_eq!(first.len(), 40);
        assert_eq!(first.iter().filter(|call| call.is_some()).count(), 20);
        assert!(first.iter().step_by(2).all(Option::is_some));
        assert!(first.iter().skip(1).step_by(2).all(Option::is_none));
        // A longer run begins as the shorter one: more calls add work, they change none.
        assert_eq!(schedule(1, 3, 80)[..40], first[..]);
        assert_ne!(first, schedule(1, 4, 40));
        assert_ne!(first, schedule(2, 3, 40));
        assert_eq!(schedule(1, 0, 0), Vec::new());
        assert_eq!(schedule(1, 0, 1).len(), 1);
    }

    #[test]
    fn a_sequence_is_splitmix64() {
        // The reference implementation's first outputs from state 0.
        let mut sequence = Sequence(0);
        assert_eq!(sequence.next(), 0xE220_A839_7B1D_CDAF);
        assert_eq!(sequence.next(), 0x6E78_9E6A_A1B9_65F4);
    }

    #[test]
    fn a_quantile_is_the_nearest_rank() {
        let values = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        assert_eq!(quantile(&values, 0.5), Some(5.0));
        assert_eq!(quantile(&values, 0.95), Some(10.0));
        assert_eq!(quantile(&values, 0.9), Some(9.0));
        assert_eq!(quantile(&values, 1.0), Some(10.0));
        assert_eq!(quantile(&values, 0.0), Some(1.0));
        assert_eq!(quantile(&[7.0], 0.5), Some(7.0));
        assert_eq!(quantile(&[], 0.5), None);
    }

    #[test]
    fn a_server_is_counted_with_every_process_under_it_and_no_other() {
        let rows = processes(
            "  10   1  1000\n 11 10 200\n 12 11 30\n 13 1 5000\n\nps: a line\n 14 10 4 extra\n 15 10 x\n",
        );
        assert_eq!(
            rows,
            vec![(10, 1, 1000), (11, 10, 200), (12, 11, 30), (13, 1, 5000)]
        );
        assert_eq!(tree_rss(&rows, 10), Some(1230));
        assert_eq!(tree_rss(&rows, 12), Some(30));
        assert_eq!(tree_rss(&rows, 99), None);
        assert_eq!(tree(&rows, 10), Some(vec![10, 11, 12]));
        // A process table that names a root as its own parent ends.
        assert_eq!(tree_rss(&[(1, 1, 7), (2, 1, 3)], 1), Some(10));
    }

    #[test]
    fn the_postgresql_options_select_bare_connections_or_the_board_work() {
        let made = std::env::temp_dir().join(format!("board-load-pg-{}", std::process::id()));
        std::fs::create_dir_all(&made).unwrap();
        let home = made.to_str().unwrap();
        let parsed = |more: &[&str]| {
            let mut args = strings(&["--home", home]);
            args.extend(strings(more));
            Options::parse(&args)
        };
        let bare = parsed(&["--postgresql", "/pg/bin", "--calls", "3"]).unwrap();
        assert_eq!(
            bare.server,
            Server {
                programs: "/pg/bin".into(),
                tasks: None
            }
        );
        assert_eq!((bare.sessions, bare.calls), (30, 3));
        assert!(
            parsed(&["--postgresql", "/pg/bin", "--dump", "/d.json"])
                .unwrap_err()
                .contains("needs --tasks")
        );
        let board = parsed(&[
            "--postgresql",
            "/pg/bin",
            "--tasks",
            "/tasks",
            "--dump",
            "/d.json",
        ])
        .unwrap();
        assert_eq!(
            board.server,
            Server {
                programs: "/pg/bin".into(),
                tasks: Some("/tasks".into())
            }
        );
        assert_eq!(board.dump, Some("/d.json".into()));
        assert_eq!(
            parsed(&["--tasks", "/t"]).unwrap_err(),
            "--postgresql is required"
        );
        std::fs::remove_dir_all(&made).unwrap();
    }

    /// `ps -axo pid=,ppid=,rss=` with a board's PostgreSQL 18 just started under its supervisor,
    /// recorded on this Mac: the supervisor's shell (86892) and its reader (86896), the postmaster
    /// (86894) and its three I/O workers, checkpointer, background writer, WAL writer, autovacuum
    /// launcher and logical replication launcher; with, added, a backend serving one connection
    /// (87001) and a process of another tree (90000) whose parent is the reader.
    const RECORDED: &str = "\
        86892 86880  1472\n86894 86892 24336\n86896 86892  1472\n86936 86894  3664\n\
        86939 86894  3328\n86940 86894  3312\n86943 86894  3440\n86944 86894  3440\n\
        86968 86894  3424\n86969 86894  5072\n86970 86894  4784\n87001 86894  6000\n\
        90000 86896   800\n";

    #[test]
    fn a_servers_tree_is_its_postmaster_and_every_process_under_it_and_not_its_supervisor() {
        let rows = processes(RECORDED);
        let tree = tree(&rows, 86894).unwrap();
        assert_eq!(
            tree,
            [
                86894, 86936, 86939, 86940, 86943, 86944, 86968, 86969, 86970, 87001
            ]
        );
        // 24,336 KiB of the postmaster's, 30,464 of its eight children's, 6,000 of the backend's:
        // each child's counts again the pages of the shared buffers it maps.
        assert_eq!(tree_rss(&rows, 86894), Some(24_336 + 30_464 + 6_000));
    }

    #[test]
    fn a_servers_cpu_is_every_process_under_it_with_what_each_reaped_so_an_ended_backend_counts() {
        let rows = processes(RECORDED);
        let ms = Duration::from_millis;
        let opened = tree(&rows, 86894).unwrap()[..9].to_vec();
        let ended = tree(&rows, 86894).unwrap();
        // As it opened: the postmaster's own 30 ms and the 5 of the start-up process it reaped,
        // each of its eight children 2 ms.
        let before = tree_cpu(&opened, |pid| match pid {
            86894 => Some((ms(30), ms(5))),
            _ => Some((ms(2), ms(0))),
        });
        assert_eq!(before, Some(ms(51)));
        // Once the run ended: the postmaster reaped a backend that took 40 ms, the one still
        // serving took 12, each other child one more.
        let after = tree_cpu(&ended, |pid| match pid {
            86894 => Some((ms(30), ms(45))),
            87001 => Some((ms(12), ms(0))),
            _ => Some((ms(3), ms(0))),
        });
        assert_eq!(after, Some(ms(111)));
        assert_eq!(after.unwrap() - before.unwrap(), ms(40 + 12 + 8));
        // A process gone since the table was read is passed over; the server's own is needed.
        let gone = tree_cpu(&ended, |pid| (pid != 87001).then_some((ms(3), ms(0))));
        assert_eq!(gone, Some(ms(27)));
        assert_eq!(
            tree_cpu(&ended, |pid| (pid != 86894).then_some((ms(1), ms(0)))),
            None
        );
        assert_eq!(tree_cpu(&[], |_| Some((ms(1), ms(1)))), None);
    }

    #[test]
    fn footprint_says_what_its_processes_hold_with_what_they_share_once() {
        // `footprint -f bytes --noCategories -p 6673 -p 99083` on this Mac.
        let two = "\
======================================================================
claude [6673]: 64-bit    Footprint: 233932624 B (16384 bytes per page)
======================================================================

Auxiliary data:
    phys_footprint: 233965392 B
    phys_footprint_peak: 344491520 B

======================================================================
zsh [99083]: 64-bit    Footprint: 1835368 B (16384 bytes per page)
======================================================================

Auxiliary data:
    phys_footprint: 1851752 B
    phys_footprint_peak: 1851752 B

======================================================================
Summary Footprint: 235718840 B
======================================================================
";
        assert_eq!(footprint_bytes(two), Some(235_718_840));
        // One process found, the other gone (said on stderr): its own, and no summary.
        let one = "\
======================================================================
zsh [213]: 64-bit    Footprint: 1835368 B (16384 bytes per page)
======================================================================
";
        assert_eq!(footprint_bytes(one), Some(1_835_368));
        assert_eq!(footprint_bytes(""), None);
        assert_eq!(footprint_bytes("Summary Footprint: 1.8 MB\n"), None);
    }

    #[test]
    fn a_proportional_set_size_is_the_rollups_pss_line_in_kibibytes() {
        // `/proc/PID/smaps_rollup` as Linux writes it.
        let rollup = "\
55d0c0a00000-7ffd2a1fe000 ---p 00000000 00:00 0                          [rollup]
Rss:               24336 kB
Pss:                9120 kB
Pss_Dirty:          6000 kB
Pss_Anon:           4100 kB
Pss_File:           3020 kB
Pss_Shmem:          2000 kB
Shared_Clean:      12000 kB
";
        assert_eq!(pss_kib(rollup), Some(9120));
        assert_eq!(pss_kib("Rss: 1 kB\nPss_Anon: 2 kB\n"), None);
        assert_eq!(pss_kib("Pss: many kB\n"), None);
    }

    #[test]
    fn a_count_asks_for_a_reading_once_in_sample_every_commands_and_asks_taken_meanwhile_are_one() {
        let count = AtomicUsize::new(0);
        let (due, asked) = crate::test_channel::channel();
        let ended = Ended {
            count: &count,
            due: due.clone(),
        };
        drop(due);
        for _ in 0..2 * SAMPLE_EVERY + 1 {
            ended.one();
        }
        drop(ended);
        assert_eq!(
            asked.try_iter().count(),
            3,
            "at the 1st, the 17th and the 33rd"
        );
        // Three asked before the sampler looked: one reading; none asked, none.
        let (due, asked) = crate::test_channel::channel();
        for _ in 0..3 {
            due.send(()).unwrap();
        }
        drop(due);
        let mut readings = 0;
        assert_eq!(
            sample(&asked, || {
                readings += 1;
                readings
            }),
            [1]
        );
        let (due, asked) = crate::test_channel::channel::<()>();
        drop(due);
        assert_eq!(sample(&asked, || 0), Vec::<i32>::new());
    }

    #[test]
    fn the_bytes_under_a_folder_count_every_file_below_it() {
        let folder = std::env::temp_dir().join(format!("board-load-bytes-{}", std::process::id()));
        std::fs::create_dir_all(folder.join("a/b")).unwrap();
        std::fs::write(folder.join("one"), "12345").unwrap();
        std::fs::write(folder.join("a/b/two"), "123").unwrap();
        assert_eq!(bytes_under(&folder), 8);
        assert_eq!(bytes_under(&folder.join("none")), 0);
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn the_options_refuse_the_live_database_and_a_folder_that_is_no_scratch_place() {
        let made = std::env::temp_dir().join(format!("board-load-options-{}", std::process::id()));
        std::fs::create_dir_all(&made).unwrap();
        let home = made.to_str().unwrap();
        // A link in a scratch place to a folder that is none is refused as that folder is.
        let link = made.join("elsewhere");
        std::os::unix::fs::symlink("/usr", &link).unwrap();
        let linked = strings(&["--home", link.to_str().unwrap(), "--postgresql", "/pg"]);
        let refused = Options::parse(&linked).unwrap_err();
        assert!(refused.contains("/usr is not a scratch place"), "{refused}");
        let given = |more: &[&str]| {
            let mut args = strings(&["--home", home, "--postgresql", "/pg", "--tasks", "/t"]);
            args.extend(strings(more));
            Options::parse(&args)
        };
        let options = given(&[]).unwrap();
        assert_eq!((options.sessions, options.calls, options.seed), (30, 40, 1));
        assert_eq!(options.database, "load");
        assert_eq!(options.dump, None);
        let options = given(&["--sessions", "2", "--calls", "6", "--seed", "9"]).unwrap();
        assert_eq!((options.sessions, options.calls, options.seed), (2, 6, 9));
        assert!(
            given(&["--database", DATABASE])
                .unwrap_err()
                .contains("never the live")
        );
        assert!(
            given(&["--database", "a b"])
                .unwrap_err()
                .contains("plain name")
        );
        assert!(
            given(&["--sessions", "0"])
                .unwrap_err()
                .contains("nothing would run")
        );
        assert!(
            given(&["--calls", "many"])
                .unwrap_err()
                .starts_with("--calls many")
        );
        let elsewhere = strings(&[
            "--home",
            "/Users/nobody/board",
            "--postgresql",
            "/pg",
            "--tasks",
            "/t",
        ]);
        assert!(
            Options::parse(&elsewhere)
                .unwrap_err()
                .starts_with("--home /Users/nobody/board does not exist")
        );
        let missing = strings(&["--home", home, "--tasks", "/t"]);
        assert_eq!(
            Options::parse(&missing).unwrap_err(),
            "--postgresql is required"
        );
        assert_eq!(
            given(&[]).unwrap().server,
            Server {
                programs: "/pg".into(),
                tasks: Some("/t".into())
            }
        );
        assert_eq!(given(&[]).unwrap().home, made.canonicalize().unwrap());
        std::fs::remove_dir_all(&made).unwrap();
    }

    fn ran(kind: Kind, wall: u64, cpu: u64, failed: Option<&str>) -> Ran {
        Ran {
            kind,
            wall: Duration::from_millis(wall),
            cpu: Duration::from_millis(cpu),
            failed: failed.map(String::from),
        }
    }

    #[test]
    fn a_report_counts_each_kind_its_failures_and_its_cost() {
        let all = [
            ran(Kind::Beat, 10, 4, None),
            ran(Kind::Beat, 30, 6, None),
            ran(Kind::Say, 100, 50, Some("refused: no")),
            ran(Kind::Say, 200, 70, Some("refused: no")),
            ran(Kind::Show, 40, 20, Some("other")),
        ];
        let said = by_kind(&all);
        assert_eq!(
            said["beat"],
            json!({"n": 2, "failed": 0,
                "wall_ms": {"p50": 10.0, "p95": 30.0, "p99": 30.0, "max": 30.0},
                "tasks_cpu_ms": {"total": 10.0, "mean": 5.0}})
        );
        assert_eq!(said["say"]["failed"], 2);
        assert_eq!(said["all"]["n"], 5);
        assert_eq!(said["all"]["failed"], 3);
        assert_eq!(said["all"]["tasks_cpu_ms"]["total"], 150.0);
        assert!(said.get("pull").is_none());
        assert_eq!(
            failures(&all),
            vec![
                json!({"command": "say", "times": 2, "said": "refused: no"}),
                json!({"command": "show", "times": 1, "said": "other"}),
            ]
        );
        assert_eq!(summary(&[])["wall_ms"]["p50"], Value::Null);
        assert_eq!(summary(&[])["tasks_cpu_ms"]["mean"], Value::Null);
    }

    #[test]
    fn the_memory_says_its_median_and_peak_and_none_without_a_sample() {
        let said = memory(Some(50.04), &[300.0, 100.0, 200.0], None);
        assert_eq!(
            said,
            json!({"opened": 50.0, "median": 200.0, "peak": 300.0,
                "after": null, "samples": 3})
        );
        assert_eq!(memory(None, &[], None)["peak"], Value::Null);
        // The last reading is above every sample: it is the peak, and no sample's median.
        let grew = memory(Some(72.0), &[127.2, 189.3], Some(198.1));
        assert_eq!(
            (&grew["peak"], &grew["after"]),
            (&json!(198.1), &json!(198.1))
        );
        assert_eq!(
            (&grew["median"], &grew["samples"]),
            (&json!(127.2), &json!(2))
        );
        // And so is the first, of a server that gave memory back.
        assert_eq!(memory(Some(90.0), &[80.0], Some(70.0))["peak"], 90.0);
        assert_eq!(memory(None, &[], Some(61.0))["peak"], 61.0);
    }

    #[test]
    fn a_command_runs_on_the_named_board_alone_with_nothing_of_this_process_but_its_path() {
        let vars = Runner::vars("postgresql:/tmp/pg", "load", Path::new("/tmp/run"), "/bin");
        let names: Vec<&str> = vars.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            [
                "HC_BIN",
                "HERDR_APP_BIN",
                "HOME",
                "PATH",
                "SYMPHONY_MACHINE_DIR",
                "TASKS_DATABASE",
                "TASKS_DISPATCH",
                "TASKS_PS_TABLE",
                "TASKS_SERVER",
                "TASKS_SWEEP"
            ]
        );
        assert_eq!(vars["TASKS_SERVER"], "postgresql:/tmp/pg");
        assert_eq!(vars["TASKS_DATABASE"], "load");
        assert_eq!(vars["TASKS_DISPATCH"], "inline");
        assert_eq!(vars["TASKS_SWEEP"], "off");
        assert_eq!(vars["TASKS_PS_TABLE"], "/tmp/run/ps");
        assert_eq!(vars["HC_BIN"], "/usr/bin/false");
        assert_eq!(vars["HERDR_APP_BIN"], "/usr/bin/false");
        assert_eq!(vars["HOME"], "/tmp/run");
        assert_eq!(vars["SYMPHONY_MACHINE_DIR"], "/tmp/run/machine");
        assert_eq!(vars["PATH"], "/bin");
    }

    #[test]
    fn a_session_is_beaten_only_when_the_board_saw_it_later() {
        let seen = |rows: &[(&str, f64)]| -> BTreeMap<String, f64> {
            (rows.iter())
                .map(|(handle, at)| (handle.to_string(), *at))
                .collect()
        };
        let before = seen(&[("a", 1.0), ("b", 1.0), ("c", 1.0)]);
        let after = seen(&[("a", 2.0), ("b", 1.0), ("d", 9.0)]);
        assert_eq!(beaten(&before, &after, &["a", "b", "c", "d"]), 1);
        assert_eq!(beaten(&before, &after, &[]), 0);
        assert_eq!(beaten(&before, &before, &["a", "b", "c"]), 0);
    }

    #[test]
    fn the_writes_are_the_beats_the_inbox_the_say_and_the_cycle() {
        let writes: Vec<&str> = [
            Kind::Beat,
            Kind::Inbox,
            Kind::Say,
            Kind::Show,
            Kind::Cycle,
            Kind::List,
            Kind::Next,
            Kind::Pull,
        ]
        .into_iter()
        .filter(|kind| kind.writes())
        .map(Kind::name)
        .collect();
        assert_eq!(writes, ["beat", "inbox", "say", "cycle"]);
        let claim: Vec<&str> = CYCLE[0].iter().map(|word| word.at("a/b")).collect();
        assert_eq!(claim, ["claim", "a/b"]);
    }
}
