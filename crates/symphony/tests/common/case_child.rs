// A case child's output as its relays carry it: saved whole to its log, quiet traces kept off
// the live stream, its open publication stages and its progress recorded for the watchdog,
// which this file holds too. `commit_fixture.rs` includes it, so the two share one scope.

#[allow(dead_code)]
fn relay_output(
    mut reader: impl Read,
    mut saved: impl Write,
    mut live: impl Write,
) -> std::io::Result<()> {
    let mut buffer = [0; 8192];
    loop {
        let count = match reader.read(&mut buffer) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            return Ok(());
        }
        saved.write_all(&buffer[..count])?;
        live.write_all(&buffer[..count])?;
    }
}

// Reporting only: the existing watchdog and target budgets remain unchanged.
const PUBLICATION_REPORT_AFTER: Duration = Duration::from_secs(5);

#[allow(dead_code)]
struct PublicationProgress {
    line: String,
    began: Instant,
    reported: bool,
}

/// Whether `line` is a whole stage record, the only output that is a case child's progress:
/// a `stage` line naming its stage, or a publication trace whose stage started or finished. A
/// line that only starts like one (a bare prefix, a diagnostic naming a pid) is not.
#[allow(dead_code)]
fn stage_event(line: &str) -> bool {
    let line = line.trim_end_matches('\n');
    let seconds = |text: &str| text.strip_suffix('s').is_some_and(|s| s.parse::<f64>().is_ok());
    if let Some(rest) = line.strip_prefix(STAGE_EVENT) {
        let Some((case, rest)) = rest.split_once(" pid=") else {
            return false;
        };
        let Some((pid, rest)) = rest.split_once(" +") else {
            return false;
        };
        let Some((elapsed, name)) = rest.split_once(": ") else {
            return false;
        };
        return !case.is_empty()
            && pid.parse::<u32>().is_ok()
            && seconds(elapsed)
            && !name.trim().is_empty();
    }
    let Some(rest) = line.strip_prefix(PUBLICATION_TRACE) else {
        return false;
    };
    let Some((pid, rest)) = rest.split_once(" id=") else {
        return false;
    };
    let Some((id, rest)) = rest.split_once(" thread=") else {
        return false;
    };
    let Some((thread, rest)) = rest.split_once(" stage=\"") else {
        return false;
    };
    let Some((stage, state)) = rest.rsplit_once("\" ") else {
        return false;
    };
    let elapsed = state
        .strip_prefix("started elapsed=")
        .or_else(|| state.strip_prefix("finished elapsed="));
    pid.parse::<u32>().is_ok()
        && id.parse::<u64>().is_ok()
        && !thread.is_empty()
        && !stage.is_empty()
        && elapsed.is_some_and(seconds)
}

/// What the relays have seen of a child: its open publication stages, each thread's last trace,
/// and when its last stage event (`stage_event`) arrived.
#[allow(dead_code)]
#[derive(Default)]
struct PendingPublications(
    std::collections::BTreeMap<String, (Instant, String, bool)>,
    std::collections::BTreeMap<String, PublicationProgress>,
    Option<Instant>,
);

#[allow(dead_code)]
impl PendingPublications {
    fn thread_key(line: &str) -> String {
        let header = line.split_once(" stage=").map_or(line, |(header, _)| header);
        let (process, rest) = header.split_once(" id=").unwrap_or((header, ""));
        let thread = rest.split_once(" thread=").map_or("", |(_, thread)| thread);
        format!("{process} thread={thread}")
    }

    fn observe(&mut self, line: &str, now: Instant) -> bool {
        if stage_event(line) {
            self.2 = Some(now);
        }
        if !line.starts_with(PUBLICATION_TRACE) {
            return false;
        }
        let key = if let Some((key, _)) = line.rsplit_once(" started elapsed=") {
            self.0.insert(key.into(), (now, line.into(), false));
            key
        } else if let Some((key, _)) = line.rsplit_once(" finished elapsed=") {
            self.0.remove(key);
            key
        } else {
            return true;
        };
        self.1.insert(Self::thread_key(key), PublicationProgress {
            line: line.into(),
            began: now,
            reported: false,
        });
        true
    }

    fn report(began: Instant, line: &str, last: &str, now: Instant) -> String {
        format!(
            "after_commit publication still open after {:.3}s: {} | last trace: {}\n",
            now.duration_since(began).as_secs_f64(),
            line.trim_end(),
            last.trim_end()
        )
    }

    fn overdue(&mut self, now: Instant) -> String {
        let mut output = String::new();
        let mut just_reported = std::collections::BTreeSet::new();
        for (began, line, reported) in self.0.values_mut() {
            if !*reported && now.duration_since(*began) >= PUBLICATION_REPORT_AFTER {
                *reported = true;
                let thread = Self::thread_key(line);
                output.push_str(&Self::report(*began, line, &self.1[&thread].line, now));
                just_reported.insert(thread);
            }
        }
        // A new trace line starts a new quiet period, even when an enclosing
        // publication has already been reported. Preserve the last finished
        // stage so a gap between stages is located before the target timeout.
        for (thread, progress) in &mut self.1 {
            if !progress.reported && now.duration_since(progress.began) >= PUBLICATION_REPORT_AFTER {
                progress.reported = true;
                if !just_reported.contains(thread) {
                    for (began, line, _) in self.0.values() {
                        if Self::thread_key(line) == *thread {
                            output.push_str(&Self::report(*began, line, &progress.line, now));
                        }
                    }
                }
            }
        }
        output
    }
}

#[allow(dead_code)]
fn relay_publication_output(
    reader: impl Read,
    mut saved: impl Write,
    mut live: impl Write,
    pending: &Mutex<PendingPublications>,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            return Ok(());
        }
        saved.write_all(&line)?;
        let trace = pending.lock().unwrap().observe(&String::from_utf8_lossy(&line), Instant::now());
        if !trace {
            live.write_all(&line)?;
        }
    }
}

#[allow(dead_code)]
fn quiet_publication_output(output: &str) -> String {
    output.split_inclusive('\n').filter(|line| !line.starts_with(PUBLICATION_TRACE)).collect()
}

/// The case child's watchdog. It measures the quiet since the child's last stage event, as the
/// relays record them, never the case's whole wall: a case a loaded host slows passes while its
/// stages keep coming, and one that goes `DEADLINE` without a stage event is a hang, failed with
/// its watchdog report, whose child logs end at the open stage. `now` and `wait` are the clock
/// and one poll's pause, so a test can run the loop on a clock of its own; a case runs its looks
/// ([`watch_look`]) by `test_time::until` instead.
///
/// Its limit: it catches a child that stops, never one that spins. A save that
/// `retry_conflicts` (src/commit.rs) retries every 250 ms, while another worker hangs holding an
/// older backlog, traces whole lock and process stages on each attempt, so its case never goes
/// quiet. That case is left to the target's own budget, and only the 5 s still-open report
/// names its open publication on the live stream. A wall bound per thread or per stage would
/// catch it, but would also fail a slow case that is still advancing on a loaded host: the
/// failure this watchdog measures quiet to avoid.
#[allow(dead_code)]
fn watch(
    name: &str,
    home: &Path,
    pending: &Mutex<PendingPublications>,
    mut exited: impl FnMut() -> Option<std::process::ExitStatus>,
    now: impl Fn() -> Instant,
    mut wait: impl FnMut(),
) -> Result<std::process::ExitStatus, String> {
    let spawned = now();
    loop {
        if let Some(outcome) = watch_look(name, home, pending, &mut exited, spawned, now()) {
            return outcome;
        }
        // Only this watchdog polls process completion. The cases' ordering assertions use
        // sockets or worker joins, with no elapsed-time or sleep-based success criterion.
        wait();
    }
}

/// One look of the watchdog ([`watch`]) at `at`, for a child spawned at `spawned`: its status
/// once it has exited, its watchdog report once it has gone `DEADLINE` without a stage event, and
/// none while it runs on.
#[allow(dead_code)]
fn watch_look(
    name: &str,
    home: &Path,
    pending: &Mutex<PendingPublications>,
    exited: &mut impl FnMut() -> Option<std::process::ExitStatus>,
    spawned: Instant,
    at: Instant,
) -> Option<Result<std::process::ExitStatus, String>> {
    // Full detail stays in child.log; an open stage reaches the live stream
    // before the target timeout, without flooding successful suite output.
    let (overdue, progress) = {
        let mut pending = pending.lock().unwrap();
        (pending.overdue(at), pending.2.map_or(spawned, |last| last.max(spawned)))
    };
    let _ = std::io::stderr().write_all(overdue.as_bytes());
    if let Some(status) = exited() {
        return Some(Ok(status));
    }
    (at.saturating_duration_since(progress) >= DEADLINE).then(|| Err(watchdog_report(name, home)))
}

#[allow(dead_code)]
struct OwnedChild(symphony_board::sys::Child, Vec<std::thread::JoinHandle<std::io::Result<()>>>, Arc<Mutex<PendingPublications>>);
impl OwnedChild {
    fn logged(command: &mut symphony_board::sys::Command, path: &Path) -> Self {
        let log = std::fs::File::create(path).unwrap();
        let saved = log.try_clone().unwrap();
        // The relays end with the case's output, never later because another case's child kept it.
        let mut child = symphony_board::sys::spawn_output(command).unwrap();
        let error = child.stderr.take().unwrap();
        let output = child.stdout.take().unwrap();
        let pending = Arc::new(Mutex::new(PendingPublications::default()));
        let out_pending = Arc::clone(&pending);
        let err_pending = Arc::clone(&pending);
        Self(
            child,
            vec![
                std::thread::spawn(move || relay_publication_output(output, saved, std::io::stderr(), &out_pending)),
                std::thread::spawn(move || relay_publication_output(error, log, std::io::stderr(), &err_pending)),
            ],
            pending,
        )
    }

    fn finish_output(&mut self) {
        for relay in self.1.drain(..) {
            relay.join().unwrap().unwrap();
        }
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
