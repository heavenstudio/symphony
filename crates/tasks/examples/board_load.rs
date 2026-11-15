//! The board under the work its agents give it, on a scratch board whose server this process
//! starts as the product starts it ([`Home`]) and which ends with it: what each command costs
//! (its wall, the `tasks` process's CPU), what the store's server took for the whole run (its CPU,
//! its resident memory, its bytes on disk), and how many commands failed.
//!
//!     cargo run -p symphony-board --example board_load -- --home DIR --surreal BIN --tasks BIN \
//!         [--database load] [--dump FILE] [--sessions 30] [--calls 40] [--seed 1]
//!
//! DIR is a scratch place ([`scratch`]) that exists; the board is kept in `DIR/board`, which a
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
use std::sync::{Arc, Barrier, Mutex};
use std::time::{Duration, Instant};
use symphony_board::rows::Dump;
use symphony_board::rpc::{self, Client};
use symphony_board::server::{Home, Running, scratch};
use symphony_board::sys;
use symphony_board::{DATABASE, NAMESPACE, NewSession, NewTask, Store};

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

/// The resident kilobytes of `root` and every process under it, or none when `root` is not among
/// `rows`. Memory two of them share is counted for each: a server that answers from processes of
/// its own over shared buffers reads higher than it holds.
fn tree_rss(rows: &[(i64, i64, u64)], root: i64) -> Option<u64> {
    let mut total = rows.iter().find(|row| row.0 == root)?.2;
    let mut parents = vec![root];
    while let Some(parent) = parents.pop() {
        for (pid, _, rss) in rows.iter().filter(|row| row.1 == parent && row.0 != root) {
            total += rss;
            parents.push(*pid);
        }
    }
    Some(total)
}

/// The server's resident memory now, in MiB, with every process under it.
fn server_mib(pid: i64) -> Option<f64> {
    let said = sys::output(sys::command("ps").args(["-axo", "pid=,ppid=,rss="])).ok()?;
    let rows = processes(&String::from_utf8_lossy(&said.stdout));
    tree_rss(&rows, pid).map(|kilobytes| kilobytes as f64 / 1024.0)
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

/// What the command line asks for.
#[derive(Debug)]
struct Options {
    home: PathBuf,
    surreal: PathBuf,
    tasks: PathBuf,
    database: String,
    dump: Option<PathBuf>,
    sessions: usize,
    calls: usize,
    seed: u64,
}

impl Options {
    fn parse(args: &[String]) -> Result<Options, String> {
        let needed = |name: &str| arg(args, name).ok_or(format!("{name} is required"));
        let home = PathBuf::from(needed("--home")?);
        if !scratch(&home) {
            return Err(format!(
                "--home {} is not a scratch place: the temporary folder, or a checkout's artifacts/",
                home.display()
            ));
        }
        let database = arg(args, "--database").unwrap_or("load");
        if database == DATABASE || !rpc::plain(database) {
            return Err(format!(
                "--database {database}: a plain name, never the live board's"
            ));
        }
        let sessions = number(args, "--sessions", 30)?;
        if sessions == 0 {
            return Err("--sessions 0: nothing would run".into());
        }
        Ok(Options {
            home,
            surreal: needed("--surreal")?.into(),
            tasks: needed("--tasks")?.into(),
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

/// What every command of the run is started with.
struct Runner {
    tasks: PathBuf,
    /// The whole environment of each command ([`Runner::vars`]).
    vars: BTreeMap<String, String>,
    /// The folder the commands run in: no repository, an empty process table and no home.
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
            ("TASKS_SERVER", server.to_string()),
            ("TASKS_DATABASE", database.to_string()),
            ("TASKS_DISPATCH", "inline".to_string()),
            ("TASKS_SWEEP", "off".to_string()),
            ("TASKS_PS_TABLE", format!("{folder}/ps")),
            ("HERDR_BIN", "/usr/bin/false".to_string()),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect()
    }

    /// `tasks ARGS` with `input` on its stdin, read to its end and reaped: its wall and the CPU it
    /// took with every child it reaped.
    fn run(&self, kind: Kind, args: &[&str], input: &str) -> Ran {
        let mut command = sys::command_with(&self.tasks, &self.vars);
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

/// One session's tool calls, back to back, each command's end counted on `ended` and the
/// server's memory read into `memory` once in [`SAMPLE_EVERY`] of them.
fn work(
    runner: &Runner,
    session: &Session,
    plan: &[Option<Kind>],
    ended: &AtomicUsize,
    memory: &Mutex<Vec<f64>>,
    server: i64,
) -> Vec<Ran> {
    let mut ran = Vec::new();
    let mut turn = 0;
    let count = |ran: &mut Vec<Ran>, one: Ran| {
        ran.push(one);
        if (ended.fetch_add(1, Ordering::Relaxed)).is_multiple_of(SAMPLE_EVERY)
            && let Some(mib) = server_mib(server)
        {
            memory.lock().unwrap().push(mib);
        }
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

/// The server's resident memory in MiB: as the run found it, right after its start (`opened`),
/// the samples' median and peak, and once every command had ended.
fn memory(opened: Option<f64>, samples: &[f64], after: Option<f64>) -> Value {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    json!({
        "opened": opened.map(rounded),
        "median": quantile(&sorted, 0.5).map(rounded),
        "peak": quantile(&sorted, 1.0).map(rounded),
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

fn main() -> Result<(), Failed> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = Options::parse(&args)?;
    let home = Home::new(&options.home.join("board"), &options.surreal);
    let open = |running: &Running| -> Result<Store, Failed> {
        Ok(Store::open(Client::new(
            &running.addr,
            NAMESPACE,
            &options.database,
        ))?)
    };
    // The board is made on one start of the server and measured on the next, as a board's server
    // is found after a restart: what the copy left in its memory is not the run's.
    let making = home.ensure().map_err(|why| why.to_string())?;
    let mut board = json!({"tasks": 0, "messages": 0, "sessions": 0});
    if let Some(file) = &options.dump {
        let dump = Dump::from_json(&std::fs::read_to_string(file)?)?;
        board = json!({"tasks": dump.tasks.len(), "messages": dump.messages.len(),
            "sessions": dump.sessions.len()});
        open(&making)?.copy(&dump)?;
        eprintln!("board_load: copied {board}");
    }
    let sessions = set_up(&options, open(&making)?)?;
    home.stop().map_err(|why| why.to_string())?;
    let running = home.ensure().map_err(|why| why.to_string())?;
    let server = u32::try_from(running.pid)?;
    let store = || open(&running);
    let folder = options.home.join("run");
    std::fs::create_dir_all(&folder)?;
    std::fs::write(folder.join("ps"), "")?;
    let runner = Runner {
        tasks: options.tasks.clone(),
        vars: Runner::vars(
            &running.addr,
            &options.database,
            &folder,
            &std::env::var("PATH").unwrap_or_default(),
        ),
        folder,
    };
    let (ended, samples) = (AtomicUsize::new(0), Mutex::new(Vec::new()));
    let barrier = Arc::new(Barrier::new(sessions.len()));
    let handles: Vec<&str> = (sessions.iter())
        .map(|session| session.handle.as_str())
        .collect();
    let seen_before = last_seen(&store()?)?;
    let load_before = test_time::wait::load();
    let (bytes_before, opened) = (bytes_under(home.dir()), server_mib(running.pid));
    let cpu_before = test_time::cpu::of(server)?;
    let began = Instant::now();
    let ran: Vec<Ran> = std::thread::scope(|scope| {
        let threads: Vec<_> = (sessions.iter().enumerate())
            .map(|(n, session)| {
                let plan = schedule(options.seed, n, options.calls);
                let (runner, barrier) = (&runner, Arc::clone(&barrier));
                let (ended, samples) = (&ended, &samples);
                scope.spawn(move || {
                    barrier.wait();
                    work(runner, session, &plan, ended, samples, running.pid)
                })
            })
            .collect();
        (threads.into_iter())
            .flat_map(|thread| thread.join().expect("a session's thread ends"))
            .collect()
    });
    let wall = began.elapsed().as_secs_f64();
    let cpu_after = test_time::cpu::of(server)?;
    let beaten = beaten(&seen_before, &last_seen(&store()?)?, &handles);
    let done = |ran: &&Ran| ran.failed.is_none();
    let writes = ran
        .iter()
        .filter(done)
        .filter(|one| one.kind.writes())
        .count();
    let tasks_cpu: f64 = ran.iter().map(|one| one.cpu.as_secs_f64()).sum();
    let report = json!({
        "server": {"store": "SurrealDB", "version": running.version},
        "board": board,
        "sessions": options.sessions,
        "calls": options.calls,
        "seed": options.seed,
        "commands": by_kind(&ran),
        "failures": failures(&ran),
        "wall_seconds": rounded(wall),
        "commands_per_second": rounded(ran.iter().filter(done).count() as f64 / wall),
        "sessions_beaten": beaten,
        "writes": writes,
        "writes_per_second": rounded(writes as f64 / wall),
        "tasks_cpu_seconds": rounded(tasks_cpu),
        "server_cpu_seconds": rounded((cpu_after.saturating_sub(cpu_before)).as_secs_f64()),
        "server_rss_mib": memory(opened, &samples.lock().unwrap(),
            server_mib(running.pid)),
        "store_bytes": {"before": bytes_before, "after": bytes_under(home.dir())},
        "load_average": {"before": load_before, "after": test_time::wait::load()},
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    home.stop().map_err(|why| why.to_string())?;
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
        // A process table that names a root as its own parent ends.
        assert_eq!(tree_rss(&[(1, 1, 7), (2, 1, 3)], 1), Some(10));
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
        let home = std::env::temp_dir().join("board-load-options");
        let home = home.to_str().unwrap();
        let given = |more: &[&str]| {
            let mut args = strings(&["--home", home, "--surreal", "/s", "--tasks", "/t"]);
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
            "--surreal",
            "/s",
            "--tasks",
            "/t",
        ]);
        assert!(
            Options::parse(&elsewhere)
                .unwrap_err()
                .contains("not a scratch place")
        );
        let missing = strings(&["--home", home, "--tasks", "/t"]);
        assert_eq!(
            Options::parse(&missing).unwrap_err(),
            "--surreal is required"
        );
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
    }

    #[test]
    fn a_command_runs_on_the_named_board_alone_with_nothing_of_this_process_but_its_path() {
        let vars = Runner::vars("127.0.0.1:9", "load", Path::new("/tmp/run"), "/bin");
        let names: Vec<&str> = vars.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            [
                "HERDR_BIN",
                "HOME",
                "PATH",
                "TASKS_DATABASE",
                "TASKS_DISPATCH",
                "TASKS_PS_TABLE",
                "TASKS_SERVER",
                "TASKS_SWEEP"
            ]
        );
        assert_eq!(vars["TASKS_SERVER"], "127.0.0.1:9");
        assert_eq!(vars["TASKS_DATABASE"], "load");
        assert_eq!(vars["TASKS_DISPATCH"], "inline");
        assert_eq!(vars["TASKS_SWEEP"], "off");
        assert_eq!(vars["TASKS_PS_TABLE"], "/tmp/run/ps");
        assert_eq!(vars["HOME"], "/tmp/run");
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
