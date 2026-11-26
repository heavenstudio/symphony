//! Many board writers at once on a scratch board whose sessions hold native callbacks: how long each
//! `tasks say` takes to answer, the database server's CPU over the run, how long until every addressed
//! message reached its recipient, and whether one reached it twice or never.
//!
//!     cargo run --release -p symphony-board --example dispatch_load -- --server ADDR --database NAME \
//!         --tasks BIN [--dump FILE] [--recipients 93] [--orchestrators 5] [--writers 40] \
//!         [--server-pid PID] [--settle 300]
//!
//! It writes only the named non-live database on the server at ADDR (`--dump` replaces it with that
//! board first) and never the live board. Each recipient is a session of its own with a UDS listener
//! this process holds and a task it executes (orchestrates, for the first `--orchestrators`); writer
//! `i` runs `BIN say` on recipient `i % recipients`'s task, addressed to it, all at once. The writers'
//! answers say their message ids; the listeners say what reached whom. The report is one JSON object,
//! with the machine's load averages before and after the writers: a say waits on the processors as
//! much as on the board.

use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::time::{Duration, Instant};
use symphony_board::rows::Dump;
use symphony_board::rpc::Client;
use symphony_board::sys;
use symphony_board::{DATABASE, NAMESPACE, NewSession, NewTask, Store};

type Frames = Arc<Mutex<BTreeMap<String, Vec<String>>>>;
type Failed = Box<dyn std::error::Error>;

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
struct Options {
    server: String,
    database: String,
    tasks: String,
    dump: Option<String>,
    recipients: usize,
    orchestrators: usize,
    writers: usize,
    settle: usize,
    /// The database server's process, whose CPU the report counts.
    server_pid: Option<String>,
}

impl Options {
    fn parse(args: &[String]) -> Result<Options, Failed> {
        let database = arg(args, "--database").ok_or("--database NAME is required")?;
        if database == DATABASE || !symphony_board::db::plain(database) {
            return Err("the measurement needs an explicit non-live database".into());
        }
        Ok(Options {
            server: arg(args, "--server")
                .ok_or("--server ADDR is required")?
                .to_string(),
            database: database.to_string(),
            tasks: arg(args, "--tasks")
                .ok_or("--tasks BIN is required")?
                .to_string(),
            dump: arg(args, "--dump").map(String::from),
            recipients: number(args, "--recipients", 93)?,
            orchestrators: number(args, "--orchestrators", 5)?,
            writers: number(args, "--writers", 40)?,
            settle: number(args, "--settle", 300)?,
            server_pid: arg(args, "--server-pid").map(String::from),
        })
    }

    fn store(&self) -> Result<Store, Failed> {
        Ok(Store::open(Client::new(
            &self.server,
            NAMESPACE,
            &self.database,
        ))?)
    }

    /// The database server's CPU seconds so far, when its pid was given.
    fn cpu(&self) -> Option<f64> {
        self.server_pid.as_deref().and_then(cpu_seconds)
    }
}

/// One socket per recipient, read a connection at a time to its end: each frame's content, in order.
fn listen(path: &Path, handle: String, frames: Frames, stop: Arc<AtomicBool>) {
    let listener = UnixListener::bind(path).expect("the listener's socket binds");
    listener.set_nonblocking(true).expect("nonblocking accept");
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                    let mut bytes = Vec::new();
                    let _ = stream.read_to_end(&mut bytes);
                    let sent = String::from_utf8_lossy(&bytes);
                    for line in sent.lines() {
                        let content = serde_json::from_str::<Value>(line)
                            .ok()
                            .and_then(|v| v["message"]["content"].as_str().map(String::from));
                        if let Some(content) = content {
                            frames
                                .lock()
                                .unwrap()
                                .entry(handle.clone())
                                .or_default()
                                .push(content);
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    #[expect(
                        clippy::disallowed_methods,
                        reason = "no-timed-waits baseline: a load example run by hand paces itself by lengths of time"
                    )]
                    std::thread::sleep(Duration::from_millis(5));
                }
                #[expect(
                    clippy::disallowed_methods,
                    reason = "no-timed-waits baseline: a load example run by hand paces itself by lengths of time"
                )]
                Err(_) => std::thread::sleep(Duration::from_millis(5)),
            }
        }
    });
}

/// The CPU seconds process `pid` has used, as `ps` says them (`[[D-]H:]M:S.ss`).
fn cpu_seconds(pid: &str) -> Option<f64> {
    let said = sys::output(sys::command("ps").args(["-o", "time=", "-p", pid])).ok()?;
    let text = String::from_utf8_lossy(&said.stdout).trim().to_string();
    let (days, rest) = text
        .split_once('-')
        .map_or((0.0, text.as_str()), |(d, r)| (d.parse().unwrap_or(0.0), r));
    let seconds = rest
        .split(':')
        .try_fold(0.0, |total, part| {
            part.parse::<f64>().map(|p| total * 60.0 + p)
        })
        .ok()?;
    Some(days * 86_400.0 + seconds)
}

/// The machine's load averages over 1, 5 and 15 minutes, as `/proc/loadavg` or `sysctl vm.loadavg`
/// says them: the writers share the processors with whatever else runs, and a say's time says as
/// much about that as about the board.
fn load_average() -> Option<String> {
    let said = std::fs::read_to_string("/proc/loadavg").ok().or_else(|| {
        let said = sys::output(sys::command("sysctl").args(["-n", "vm.loadavg"])).ok()?;
        Some(String::from_utf8_lossy(&said.stdout).into_owned())
    })?;
    let numbers: Vec<&str> = (said.split_whitespace())
        .filter(|word| word.parse::<f64>().is_ok())
        .take(3)
        .collect();
    (numbers.len() == 3).then(|| numbers.join(" "))
}

/// The message ids each recipient received, with how often.
fn received(frames: &Frames) -> BTreeMap<String, BTreeMap<i64, usize>> {
    let frames = frames.lock().unwrap();
    frames
        .iter()
        .map(|(handle, contents)| {
            let mut ids: BTreeMap<i64, usize> = BTreeMap::new();
            let lines = contents.iter().flat_map(|content| content.lines());
            for line in lines {
                let id = line
                    .strip_prefix("MSG #")
                    .and_then(|rest| rest.split_whitespace().next())
                    .and_then(|id| id.parse().ok());
                if let Some(id) = id {
                    *ids.entry(id).or_default() += 1;
                }
            }
            (handle.clone(), ids)
        })
        .collect()
}

fn summary(times: &[f64]) -> Value {
    let mut sorted = times.to_vec();
    symphony_board::order::sort_by(&mut sorted, &f64::total_cmp);
    let at = |q: f64| sorted[((sorted.len() - 1) as f64 * q).round() as usize];
    json!({"n": sorted.len(), "min": at(0.0), "median": at(0.5), "p90": at(0.9), "max": at(1.0)})
}

/// A recipient: its session and the task it works on.
#[derive(Clone)]
struct Recipient {
    handle: String,
    slug: String,
}

/// The board the writers write on: its recipients, listening, and the writers' sessions.
struct Board {
    recipients: Vec<Recipient>,
    writers: Vec<String>,
    frames: Frames,
    sockets: Vec<PathBuf>,
    stop: Arc<AtomicBool>,
}

impl Drop for Board {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for path in &self.sockets {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// The recipients and the writers, registered on the board, each recipient's callback a listener of
/// this process's, and every recipient's first observation delivered.
fn set_up(options: &Options, store: &Store) -> Result<Board, Failed> {
    let host = sys::hostname();
    let run = std::process::id();
    // The setup's writes deliver nothing (a hand-off that starts nothing yet says it did): one pass
    // after them delivers what they queued, before the run starts.
    let quiet = options.store()?.dispatching_by(Box::new(|| true));
    let session = |kind: &str| -> Result<String, Failed> {
        let mut new = NewSession::new(kind);
        new.host.clone_from(&host);
        Ok(quiet.register(&new)?.row.handle)
    };
    let mut recipients = Vec::new();
    for n in 0..options.recipients {
        let orchestrates = n < options.orchestrators;
        let handle = session(if orchestrates {
            "orchestrator"
        } else {
            "worker"
        })?;
        let slug = format!("bench-{run}/r-{n}");
        quiet.create(&NewTask::new(&slug, "dispatch load", &handle))?;
        if orchestrates {
            quiet.take_role(&slug, &handle, "orchestrator")?;
        } else {
            quiet.claim(&slug, &handle)?;
        }
        recipients.push(Recipient { handle, slug });
    }
    let writers = (0..options.writers)
        .map(|_| session("worker"))
        .collect::<Result<Vec<_>, _>>()?;
    let began = Instant::now();
    let mut board = Board {
        recipients,
        writers,
        frames: Arc::default(),
        sockets: Vec::new(),
        stop: Arc::new(AtomicBool::new(false)),
    };
    let listening: Vec<(usize, String)> = (board.recipients.iter().enumerate())
        .map(|(n, recipient)| (n, recipient.handle.clone()))
        .collect();
    for (n, handle) in listening {
        let path = PathBuf::from(format!("/tmp/symphony-load-{run}-{n}.sock"));
        listen(
            &path,
            handle.clone(),
            Arc::clone(&board.frames),
            Arc::clone(&board.stop),
        );
        quiet.callback(
            &handle,
            Some(&format!("uds:{}", path.display())),
            false,
            false,
        )?;
        board.sockets.push(path);
    }
    store.dispatch_callbacks()?;
    eprintln!(
        "registered and observed {} callbacks in {:.1} s",
        options.recipients,
        began.elapsed().as_secs_f64()
    );
    board.frames.lock().unwrap().clear();
    Ok(board)
}

/// What one writer's `tasks say` came to.
struct Said {
    to: String,
    seconds: f64,
    /// The message's id, when the command answered with one.
    id: Option<i64>,
    out: String,
}

/// Each writer's `tasks say` to its recipient, all started at once.
fn write_all(options: &Options, board: &Board) -> Vec<Said> {
    let barrier = Arc::new(Barrier::new(board.writers.len()));
    let threads: Vec<_> = (board.writers.iter().enumerate())
        .map(|(n, writer)| {
            let to = board.recipients[n % board.recipients.len()].clone();
            let barrier = Arc::clone(&barrier);
            let mut command = sys::command(&options.tasks);
            command
                .args(["say", &to.slug, &format!("load {n}"), "--to", &to.handle])
                .args(["--as", writer])
                .env("TASKS_SERVER", &options.server)
                .env("TASKS_DATABASE", &options.database)
                .env("TASKS_SWEEP", "off");
            std::thread::spawn(move || {
                barrier.wait();
                let start = Instant::now();
                let said = sys::output(&mut command);
                let seconds = start.elapsed().as_secs_f64();
                let (ok, out) = match said {
                    Ok(said) => (
                        said.status.success(),
                        String::from_utf8_lossy(&said.stdout).to_string()
                            + &String::from_utf8_lossy(&said.stderr),
                    ),
                    Err(e) => (false, e.to_string()),
                };
                let id = (out.strip_prefix("sent #"))
                    .and_then(|rest| rest.split_whitespace().next())
                    .and_then(|id| id.parse::<i64>().ok())
                    .filter(|_| ok);
                Said {
                    to: to.handle,
                    seconds,
                    id,
                    out,
                }
            })
        })
        .collect();
    threads
        .into_iter()
        .map(|thread| thread.join().expect("a writer thread ends"))
        .collect()
}

/// Which recipient missed a message meant for it, heard one twice, or heard another's.
fn tally(board: &Board, expected: &BTreeMap<String, BTreeSet<i64>>) -> Value {
    let got = received(&board.frames);
    let mut missing = Vec::new();
    let mut doubled = Vec::new();
    let mut stray = Vec::new();
    let all: BTreeSet<i64> = expected.values().flatten().copied().collect();
    for recipient in &board.recipients {
        let handle = &recipient.handle;
        let mine = expected.get(handle).cloned().unwrap_or_default();
        let seen = got.get(handle).cloned().unwrap_or_default();
        missing.extend(
            mine.iter()
                .filter(|id| !seen.contains_key(id))
                .map(|id| json!([handle, id])),
        );
        doubled.extend(
            seen.iter()
                .filter(|(_, n)| **n > 1)
                .map(|(id, n)| json!([handle, id, n])),
        );
        stray.extend(
            seen.keys()
                .filter(|id| all.contains(id) && !mine.contains(id))
                .map(|id| json!([handle, id])),
        );
    }
    json!({"missing": missing, "doubled": doubled, "stray": stray})
}

fn main() -> Result<(), Failed> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = Options::parse(&args)?;
    let store = options.store()?;
    if let Some(file) = &options.dump {
        let dump: Dump = serde_json::from_slice(&std::fs::read(file)?)?;
        let began = Instant::now();
        store.copy(&dump)?;
        eprintln!(
            "copied {} messages into {} in {:.1} s",
            dump.messages.len(),
            options.database,
            began.elapsed().as_secs_f64()
        );
    }
    let board = set_up(&options, &store)?;
    let messages_before = (store.raw().table("message").count()).map_err(|e| e.to_string())?;
    let messages_before: Value = json!(messages_before);
    eprintln!(
        "{} recipients registered on a board of {messages_before} messages; {} writers start",
        options.recipients, options.writers
    );
    let cpu_before = options.cpu();
    let load_before = load_average();
    let began = Instant::now();
    let said = write_all(&options, &board);
    let writers_done = began.elapsed().as_secs_f64();
    let cpu_writers = options.cpu();
    let load_writers = load_average();
    let times: Vec<f64> = said.iter().map(|said| said.seconds).collect();
    let mut expected: BTreeMap<String, BTreeSet<i64>> = BTreeMap::new();
    let mut failures = Vec::new();
    for said in said {
        match said.id {
            Some(id) => {
                expected.entry(said.to).or_default().insert(id);
            }
            None => failures.push(said.out.trim().to_string()),
        }
    }
    // Delivery by itself first: no dispatch from here while the board's own dispatchers work.
    let complete = || {
        let got = received(&board.frames);
        expected.iter().all(|(handle, ids)| {
            got.get(handle)
                .is_some_and(|seen| ids.iter().all(|id| seen.contains_key(id)))
        })
    };
    let deadline = Instant::now() + Duration::from_secs(options.settle as u64);
    while !complete() && Instant::now() < deadline {
        #[expect(
            clippy::disallowed_methods,
            reason = "no-timed-waits baseline: a load example run by hand paces itself by lengths of time"
        )]
        std::thread::sleep(Duration::from_millis(50));
    }
    let delivered_by_itself = complete().then(|| began.elapsed().as_secs_f64());
    if delivered_by_itself.is_none() {
        // What no dispatcher delivered is still owed: one more pass from here says whether it was lost.
        store.dispatch_callbacks()?;
        #[expect(
            clippy::disallowed_methods,
            reason = "no-timed-waits baseline: a load example run by hand paces itself by lengths of time"
        )]
        std::thread::sleep(Duration::from_secs(2));
    }
    // Let a late duplicate arrive before counting.
    #[expect(
        clippy::disallowed_methods,
        reason = "no-timed-waits baseline: a load example run by hand paces itself by lengths of time"
    )]
    std::thread::sleep(Duration::from_secs(3));
    let cpu_after = options.cpu();
    let mut report = json!({
        "database": options.database,
        "messages_before": messages_before,
        "recipients": options.recipients,
        "orchestrators": options.orchestrators,
        "writers": options.writers,
        "say_seconds": summary(&times),
        "writers_wall_seconds": writers_done,
        "failed_writers": failures,
        "delivered_all_seconds": delivered_by_itself,
        "database_cpu_seconds_while_writing": cpu_before.zip(cpu_writers).map(|(a, b)| b - a),
        "database_cpu_seconds_through_delivery": cpu_before.zip(cpu_after).map(|(a, b)| b - a),
        "load_average_before_writers": load_before,
        "load_average_after_writers": load_writers,
    });
    if let (Some(report), Value::Object(tally)) = (report.as_object_mut(), tally(&board, &expected))
    {
        report.extend(tally);
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
