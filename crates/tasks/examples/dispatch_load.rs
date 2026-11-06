//! Many board writers at once on a scratch board whose sessions hold native callbacks: how long each
//! `tasks say` takes to answer, the database server's CPU over the run, how long until every addressed
//! message reached its recipient, and whether one reached it twice or never.
//!
//!     cargo run --release -p symphony-board --example dispatch_load -- --server ADDR --database NAME \
//!         --tasks BIN [--dump FILE] [--recipients 93] [--orchestrators 5] [--writers 40] \
//!         [--surreal-pid PID] [--settle 300]
//!
//! It writes only the named non-live database on the server at ADDR (`--dump` replaces it with that
//! board first) and never the live board. Each recipient is a session of its own with a UDS listener
//! this process holds and a task it executes (orchestrates, for the first `--orchestrators`); writer
//! `i` runs `BIN say` on recipient `i % recipients`'s task, addressed to it, all at once. The writers'
//! answers say their message ids; the listeners say what reached whom. The report is one JSON object.

use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::time::{Duration, Instant};
use symphony_board::rows::Dump;
use symphony_board::rpc::{self, Client};
use symphony_board::sys::{self, Command};
use symphony_board::{DATABASE, NAMESPACE, NewSession, NewTask, Store};

type Frames = Arc<Mutex<BTreeMap<String, Vec<String>>>>;

fn arg<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
}

fn number(args: &[String], name: &str, default: usize) -> Result<usize, String> {
    arg(args, name).map_or(Ok(default), |text| {
        text.parse().map_err(|e| format!("{name} {text}: {e}"))
    })
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
                    for line in String::from_utf8_lossy(&bytes).lines() {
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
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(5)),
            }
        }
    });
}

/// The CPU seconds process `pid` has used, as `ps` says them (`[[D-]H:]M:S.ss`).
fn cpu_seconds(pid: &str) -> Option<f64> {
    let said = sys::output(Command::new("ps").args(["-o", "time=", "-p", pid])).ok()?;
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

/// The message ids each recipient received, with how often.
fn received(frames: &Frames) -> BTreeMap<String, BTreeMap<i64, usize>> {
    let frames = frames.lock().unwrap();
    frames
        .iter()
        .map(|(handle, contents)| {
            let mut ids: BTreeMap<i64, usize> = BTreeMap::new();
            for line in contents.iter().flat_map(|c| c.lines()) {
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
    sorted.sort_by(f64::total_cmp);
    let at = |q: f64| sorted[((sorted.len() - 1) as f64 * q).round() as usize];
    json!({"n": sorted.len(), "min": at(0.0), "median": at(0.5), "p90": at(0.9), "max": at(1.0)})
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let server = arg(&args, "--server").ok_or("--server ADDR is required")?;
    let database = arg(&args, "--database").ok_or("--database NAME is required")?;
    if database == DATABASE || !rpc::plain(database) {
        return Err("the measurement needs an explicit non-live database".into());
    }
    let tasks = arg(&args, "--tasks").ok_or("--tasks BIN is required")?;
    let recipients = number(&args, "--recipients", 93)?;
    let orchestrators = number(&args, "--orchestrators", 5)?;
    let writers = number(&args, "--writers", 40)?;
    let settle = number(&args, "--settle", 300)?;
    let surreal = arg(&args, "--surreal-pid");
    let store = Store::open(Client::new(server, NAMESPACE, database))?;
    if let Some(file) = arg(&args, "--dump") {
        let dump: Dump = serde_json::from_slice(&std::fs::read(file)?)?;
        let began = Instant::now();
        store.copy(&dump)?;
        eprintln!(
            "copied {} messages into {database} in {:.1} s",
            dump.messages.len(),
            began.elapsed().as_secs_f64()
        );
    }
    let host = sys::hostname();
    let frames: Frames = Arc::default();
    let stop = Arc::new(AtomicBool::new(false));
    let mut handles = Vec::new();
    let mut sockets = Vec::new();
    let run = std::process::id();
    // The setup's writes dispatch nothing (a hand-off that starts nothing yet says it did): one pass
    // after them observes every recipient once. Dispatching each registration to those before it
    // made the setup quadratic in passes, an hour on a board this size.
    let quiet =
        Store::open(Client::new(server, NAMESPACE, database))?.dispatching_by(Box::new(|| true));
    for i in 0..recipients {
        let kind = if i < orchestrators {
            "orchestrator"
        } else {
            "worker"
        };
        let mut new = NewSession::new(kind);
        new.host.clone_from(&host);
        let handle = quiet.register(&new)?.row.handle;
        let slug = format!("bench-{run}/r-{i}");
        quiet.create(&NewTask::new(&slug, "dispatch load", &handle))?;
        if kind == "orchestrator" {
            quiet.take_role(&slug, &handle, "orchestrator")?;
        } else {
            quiet.claim(&slug, &handle)?;
        }
        handles.push((handle, slug));
    }
    let mut writer_handles = Vec::new();
    for _ in 0..writers {
        let mut new = NewSession::new("worker");
        new.host.clone_from(&host);
        writer_handles.push(quiet.register(&new)?.row.handle);
    }
    let began = Instant::now();
    for (i, (handle, _)) in handles.iter().enumerate() {
        let path = PathBuf::from(format!("/tmp/sf-load-{run}-{i}.sock"));
        listen(
            &path,
            handle.clone(),
            Arc::clone(&frames),
            Arc::clone(&stop),
        );
        quiet.callback(
            handle,
            Some(&format!("uds:{}", path.display())),
            false,
            false,
        )?;
        sockets.push(path);
    }
    // The recipients' first observation, and what it queued (first PULL digests and the like), goes
    // out before the run starts.
    store.dispatch_callbacks()?;
    eprintln!(
        "registered and observed {recipients} callbacks in {:.1} s",
        began.elapsed().as_secs_f64()
    );
    frames.lock().unwrap().clear();
    let messages_before: Value = store
        .client()
        .query("RETURN count(SELECT id FROM message);", json!({}))
        .map_err(|e| e.to_string())?
        .remove(0);
    eprintln!(
        "{recipients} recipients registered on a board of {messages_before} messages; {writers} writers start"
    );
    let cpu_before = surreal.and_then(cpu_seconds);
    let barrier = Arc::new(Barrier::new(writers));
    let began = Instant::now();
    let threads: Vec<_> = writer_handles
        .iter()
        .enumerate()
        .map(|(i, writer)| {
            let (to, slug) = handles[i % recipients].clone();
            let barrier = Arc::clone(&barrier);
            let (tasks, server, database, writer) = (
                tasks.to_string(),
                server.to_string(),
                database.to_string(),
                writer.clone(),
            );
            std::thread::spawn(move || {
                let text = format!("load {i}");
                barrier.wait();
                let start = Instant::now();
                let said = sys::output(
                    Command::new(&tasks)
                        .args(["say", slug.as_str(), text.as_str(), "--to", to.as_str()])
                        .args(["--as", writer.as_str()])
                        .env("TASKS_SERVER", &server)
                        .env("TASKS_DATABASE", &database)
                        .env("TASKS_SWEEP", "off"),
                );
                let seconds = start.elapsed().as_secs_f64();
                let (ok, out) = match said {
                    Ok(said) => (
                        said.status.success(),
                        String::from_utf8_lossy(&said.stdout).to_string()
                            + &String::from_utf8_lossy(&said.stderr),
                    ),
                    Err(e) => (false, e.to_string()),
                };
                let id = out
                    .strip_prefix("sent #")
                    .and_then(|rest| rest.split_whitespace().next())
                    .and_then(|id| id.parse::<i64>().ok());
                (to, seconds, ok, id, out)
            })
        })
        .collect();
    let mut times = Vec::new();
    let mut expected: BTreeMap<String, BTreeSet<i64>> = BTreeMap::new();
    let mut failures = Vec::new();
    for thread in threads {
        let (to, seconds, ok, id, out) = thread.join().expect("a writer thread ends");
        times.push(seconds);
        match id.filter(|_| ok) {
            Some(id) => {
                expected.entry(to).or_default().insert(id);
            }
            None => failures.push(out.trim().to_string()),
        }
    }
    let writers_done = began.elapsed().as_secs_f64();
    let cpu_writers = surreal.and_then(cpu_seconds);
    // Delivery by itself first: no dispatch from here while the board's own dispatchers work.
    let complete = |frames: &Frames| {
        let got = received(frames);
        expected.iter().all(|(handle, ids)| {
            got.get(handle)
                .is_some_and(|seen| ids.iter().all(|id| seen.contains_key(id)))
        })
    };
    let deadline = Instant::now() + Duration::from_secs(settle as u64);
    while !complete(&frames) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let delivered_by_itself = complete(&frames).then(|| began.elapsed().as_secs_f64());
    if delivered_by_itself.is_none() {
        // What no dispatcher delivered is still owed: one more pass from here says whether it was lost.
        store.dispatch_callbacks()?;
        std::thread::sleep(Duration::from_secs(2));
    }
    // Let a late duplicate arrive before counting.
    std::thread::sleep(Duration::from_secs(3));
    let cpu_after = surreal.and_then(cpu_seconds);
    let got = received(&frames);
    let mut missing = Vec::new();
    let mut doubled = Vec::new();
    let mut stray = Vec::new();
    let all: BTreeSet<i64> = expected.values().flatten().copied().collect();
    for (handle, _) in &handles {
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
    stop.store(true, Ordering::Relaxed);
    for path in &sockets {
        let _ = std::fs::remove_file(path);
    }
    let report = json!({
        "database": database,
        "messages_before": messages_before,
        "recipients": recipients,
        "orchestrators": orchestrators,
        "writers": writers,
        "say_seconds": summary(&times),
        "writers_wall_seconds": writers_done,
        "failed_writers": failures,
        "delivered_all_seconds": delivered_by_itself,
        "database_cpu_seconds_while_writing": cpu_before.zip(cpu_writers).map(|(a, b)| b - a),
        "database_cpu_seconds_through_delivery": cpu_before.zip(cpu_after).map(|(a, b)| b - a),
        "missing": missing,
        "doubled": doubled,
        "stray": stray,
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
