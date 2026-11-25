//! A test child that ends by a signal its [`Process`](super::Process) did not send: what the
//! system knew of its pid at that moment, captured once, kept beside the child's logs in the
//! scratch folder a failure keeps, and said in the failure ([`capture`]).
//!
//! And the journal of every signal this test process sends by pid ([`send`]), which that capture
//! reads first. The kernel gives a pid to a new process once the one it named has ended and been
//! reaped, and the pid space wraps within minutes on a loaded machine. A process just given a pid
//! is one that has just started, so a signal meant for the pid's old process ends it at or right
//! after its exec, before it says anything: only this journal says this process sent it. A process
//! the fixture signals by pid and does not wait for itself is an [`Owned`], signalled only while it
//! is still the process that was taken, as the product's [`Taken`] tells it (the window it leaves
//! between a read of the start and `kill(2)` is said there).

use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use symphony_board::lock::Recovered as _;
use symphony_server::sys::Taken;

/// The most lines one section of a capture keeps: its last ones.
const SECTION_LINES: usize = 60;
/// The most bytes one section of a capture keeps.
const SECTION_BYTES: usize = 12 * 1024;
/// The most bytes of one line a capture keeps.
const LINE_BYTES: usize = 400;
/// The most of one log file the search of the scratch logs reads: its end.
const LOG_TAIL: u64 = 1024 * 1024;
/// How deep below a scratch folder its logs are looked for.
const LOG_DEPTH: usize = 4;
/// How long each of the system's commands may take. Their three readers run at once, the memory
/// one running `sysctl` and then `vm_stat` and the log one a `log show` for each of its windows
/// ([`log_windows`]), so a capture takes at most twice this.
#[cfg(target_os = "macos")]
const COMMAND_PATIENCE: std::time::Duration = std::time::Duration::from_secs(30);
/// The system's log is read from this long before the child's spawn and to this long after the
/// capture.
#[cfg(any(target_os = "macos", test))]
const LOG_MARGIN: std::time::Duration = std::time::Duration::from_secs(2);
/// How long after the child's spawn, and before the capture, the system's log is read: a child that
/// lived longer has the time between left out, which would take a read past its patience.
#[cfg(any(target_os = "macos", test))]
const LOG_SPAN: std::time::Duration = std::time::Duration::from_secs(10);

/// One signal this test process sent, or held back, by pid.
struct Sent {
    at: SystemTime,
    thread: String,
    pid: i64,
    signal: i32,
    why: String,
}

/// Every signal this test process sent by pid: under `cargo test` every case of the target runs in
/// this one process, so one case's signal to a pid another case's child now has is here too.
static JOURNAL: Mutex<Vec<Sent>> = Mutex::new(Vec::new());

/// Send `signal` to process `pid`, journaled with `why` first.
pub fn send(pid: i64, signal: i32, why: &str) {
    note(pid, signal, why);
    symphony_server::sys::signal(pid, signal);
}

/// Journal `signal` to `pid`, sent another way (a child's own kill), or held back, for `why`.
pub fn note(pid: i64, signal: i32, why: &str) {
    let sent = Sent {
        at: SystemTime::now(),
        thread: std::thread::current()
            .name()
            .unwrap_or("unnamed")
            .to_owned(),
        pid,
        signal,
        why: why.to_owned(),
    };
    JOURNAL
        .lock()
        .recover()
        .push(sent);
}

/// The journal's lines for `pid`, oldest first.
fn sent_to(pid: i64) -> Vec<String> {
    JOURNAL
        .lock()
        .recover()
        .iter()
        .filter(|sent| sent.pid == pid)
        .map(|sent| {
            format!(
                "at {} ms since Unix epoch, thread {}: {} to pid {}: {}",
                millis(sent.at),
                sent.thread,
                name(sent.signal),
                sent.pid,
                sent.why
            )
        })
        .collect()
}

/// `signal` by its name and number.
pub fn name(signal: i32) -> String {
    let known = match signal {
        libc::SIGHUP => "SIGHUP",
        libc::SIGINT => "SIGINT",
        libc::SIGQUIT => "SIGQUIT",
        libc::SIGILL => "SIGILL",
        libc::SIGTRAP => "SIGTRAP",
        libc::SIGABRT => "SIGABRT",
        libc::SIGBUS => "SIGBUS",
        libc::SIGFPE => "SIGFPE",
        libc::SIGKILL => "SIGKILL",
        libc::SIGSEGV => "SIGSEGV",
        libc::SIGPIPE => "SIGPIPE",
        libc::SIGALRM => "SIGALRM",
        libc::SIGTERM => "SIGTERM",
        libc::SIGUSR1 => "SIGUSR1",
        libc::SIGUSR2 => "SIGUSR2",
        libc::SIGXCPU => "SIGXCPU",
        libc::SIGSYS => "SIGSYS",
        _ => return format!("signal {signal}"),
    };
    format!("{known} (signal {signal})")
}

fn millis(at: SystemTime) -> u128 {
    at.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()
}

/// A process this fixture may signal by pid though it does not wait for it itself (a server a
/// start detached, a grandchild), taken as the product takes one ([`Taken`]): a process the kernel
/// later gives the same pid is never taken for it.
#[derive(Clone, Debug)]
pub struct Owned(Taken);

/// Its pid, and whether it is still the process taken ([`Taken::alive`]).
impl std::ops::Deref for Owned {
    type Target = Taken;

    fn deref(&self) -> &Taken {
        &self.0
    }
}

impl Owned {
    /// Process `pid` as it is now.
    pub fn of(pid: i64) -> Self {
        Self(Taken::of(pid))
    }

    /// Send `signal`, journaled with `why`, while its pid still names the process taken; otherwise
    /// send nothing, and journal that.
    pub fn signal(&self, signal: i32, why: &str) {
        let pid = self.0.pid;
        if !self.0.signal(signal, || note(pid, signal, why)) {
            note(
                pid,
                signal,
                &format!("not sent, the pid no longer names the process taken ({why})"),
            );
        }
    }
}

/// A child that ended by a signal its Process did not send.
pub struct Dead<'a> {
    pub pid: u32,
    /// How it was started, as its Process reports it.
    pub command: &'a str,
    pub signal: i32,
    /// When its spawn began.
    pub spawned: SystemTime,
    /// The file the capture is kept in, beside the child's logs.
    pub kept: &'a Path,
}

/// What was known of `dead`'s pid now: the signals this test process sent it, the lines this
/// test process's scratch logs say of it (where a kill the product sent is said), and on macOS the
/// system's record of it. Written to `dead.kept` and returned.
pub fn capture(dead: &Dead) -> String {
    let at = SystemTime::now();
    let pid = dead.pid;
    let mut text = format!(
        "child {pid} ({}) ended by {}, which its Process did not send; what was known of pid {pid} \
         at {} ms since Unix epoch, kept in {}:\n",
        dead.command,
        name(dead.signal),
        millis(at),
        dead.kept.display()
    );
    section(
        &mut text,
        &format!("signals this test process sent to pid {pid}"),
        Ok(sent_to(i64::from(pid))),
        Keep::Last,
    );
    let (root, prefix) = scratch_root();
    section(
        &mut text,
        &format!("lines naming pid {pid} in this test process's scratch logs"),
        Ok(scratch_lines(&root, &prefix, pid)),
        Keep::Last,
    );
    system(&mut text, dead, at, &root.join(&prefix));
    if let Err(error) = fs::write(dead.kept, &text) {
        let _ = writeln!(text, "(not kept: {error})");
    }
    text
}

/// The folder the fixture's scratch folders are made in, and the start of this process's names.
fn scratch_root() -> (PathBuf, String) {
    let root = super::scratch_parent();
    (
        root.canonicalize().unwrap_or(root),
        format!("{}{}-", super::SCRATCH_PREFIX, std::process::id()),
    )
}

/// The system's record of `dead`'s pid at `at`: its log over the child's life, memory, swap and
/// load, and this test's processes alive (those `ours` names: descendants of this process, and
/// processes naming its scratch folders), each reader run at once, each command under its own
/// patience.
#[cfg(target_os = "macos")]
fn system(text: &mut String, dead: &Dead, at: SystemTime, ours_named: &Path) {
    let pid = dead.pid;
    // The log command says its own arguments, the pid among them: its lines are passed over.
    let predicate =
        format!("(processID == {pid} OR eventMessage CONTAINS \"{pid}\") AND process != \"log\"");
    let windows: Vec<(String, String)> = log_windows(dead.spawned, at)
        .into_iter()
        .map(|(from, to)| (utc(from), utc(to)))
        .collect();
    let (memory, table, logs) = std::thread::scope(|scope| {
        let memory = scope.spawn(|| {
            let mut lines = run(
                "/usr/sbin/sysctl",
                &[
                    "vm.swapusage",
                    "kern.memorystatus_level",
                    "vm.loadavg",
                    "hw.memsize",
                ],
            )?;
            lines.extend(run("/usr/bin/vm_stat", &[])?);
            Ok(lines)
        });
        let table = scope.spawn(|| {
            run(
                "/bin/ps",
                &["-axww", "-o", "pid=,ppid=,pgid=,stat=,etime=,command="],
            )
        });
        let logs = scope.spawn(|| {
            let show = |(start, end): &(String, String)| {
                let args = ["show", "--style", "syslog", "--info", "--start", start];
                run("/usr/bin/log", &[&args[..], &["--end", end, "--predicate", &predicate]].concat())
            };
            windows.iter().map(show).collect::<Vec<_>>()
        });
        let joined = |handle: std::thread::ScopedJoinHandle<'_, Result<Vec<String>, String>>| {
            handle
                .join()
                .unwrap_or_else(|_| Err("its reader panicked".into()))
        };
        let logs = logs.join().unwrap_or_else(|_| {
            vec![Err("its reader panicked".to_owned()); windows.len()]
        });
        (joined(memory), joined(table), logs)
    });
    section(
        text,
        "memory, swap and load (sysctl, vm_stat)",
        memory,
        Keep::First,
    );
    let named = ours_named.display().to_string();
    section(
        text,
        &format!(
            "this test's processes alive (descendants of pid {}, and processes naming {named}…), \
             after pid {pid}'s own row",
            std::process::id()
        ),
        table.map(|lines| {
            let table = lines.join("\n");
            let own = row_of(&table, pid).map_or_else(
                || format!("pid {pid}: no row (ended and reaped)"),
                |row| format!("pid {pid}: {row}"),
            );
            std::iter::once(own)
                .chain(ours(&table, std::process::id(), &named))
                .collect()
        }),
        Keep::First,
    );
    for ((start, end), log) in windows.iter().zip(logs) {
        section(
            text,
            &format!(
                "the system log for pid {pid} from {start} to {end} (log show: the kernel's \
                 kills, jetsam, code signing at exec)"
            ),
            log.map(|lines| lines.into_iter().filter(|line| names(line, pid)).collect()),
            Keep::Last,
        );
    }
}

/// The spans of the system's log a capture at `at` reads for a child spawned at `spawned`: from
/// [`LOG_MARGIN`] before its spawn to [`LOG_SPAN`] after it, and from `LOG_SPAN` before the capture
/// to `LOG_MARGIN` after it, or one span from the first's start to the last's end when they meet.
#[cfg(any(target_os = "macos", test))]
fn log_windows(spawned: SystemTime, at: SystemTime) -> Vec<(SystemTime, SystemTime)> {
    let from = spawned.checked_sub(LOG_MARGIN).unwrap_or(spawned);
    let (first, last) = (spawned + LOG_SPAN, at.checked_sub(LOG_SPAN).unwrap_or(at));
    let to = at + LOG_MARGIN;
    if first >= last {
        vec![(from, to)]
    } else {
        vec![(from, first), (last, to)]
    }
}

#[cfg(not(target_os = "macos"))]
fn system(text: &mut String, dead: &Dead, _at: SystemTime, _ours_named: &Path) {
    let _ = writeln!(
        text,
        "· the system's record of pid {} (its log, memory and swap, this test's processes): \
         nothing captured, it is read on macOS only",
        dead.pid
    );
}

/// The lines `program` with `args` prints within [`COMMAND_PATIENCE`]: its output's, then its
/// error stream's, then its status when it failed.
#[cfg(target_os = "macos")]
fn run(program: &str, args: &[&str]) -> Result<Vec<String>, String> {
    use std::os::unix::process::ExitStatusExt;
    let mut command = symphony_board::sys::Command::new(program);
    command.args(args);
    let out = symphony_server::sys::output_within(command, COMMAND_PATIENCE)
        .map_err(|why| format!("{program}: {why}"))?;
    let mut lines: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_owned)
        .collect();
    lines.extend(
        String::from_utf8_lossy(&out.stderr)
            .lines()
            .map(|line| format!("{program} said: {line}")),
    );
    if !out.status.success() {
        lines.push(match out.status.signal() {
            Some(signal) => format!("{program} ended by {}", name(signal)),
            None => format!("{program} exited {}", out.status.code().unwrap_or(-1)),
        });
    }
    Ok(lines)
}

/// Which of a section's lines it keeps when they are more than its bounds hold.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Keep {
    /// Its first: a table whose first rows matter most.
    First,
    /// Its last: a log, whose end is nearest the end it explains.
    Last,
}

/// `title` and its lines, as many of them as [`SECTION_LINES`] and [`SECTION_BYTES`] hold, those
/// `keep` says, or why there are none.
fn section(text: &mut String, title: &str, lines: Result<Vec<String>, String>, keep: Keep) {
    let _ = match lines {
        Ok(lines) if lines.is_empty() => writeln!(text, "· {title}: none"),
        Ok(lines) => {
            let _ = writeln!(text, "· {title}:");
            text.push_str(&bounded(&lines, SECTION_LINES, SECTION_BYTES, keep));
            Ok(())
        }
        Err(why) => writeln!(text, "· {title}: unavailable: {why}"),
    };
}

/// The first or the last of `lines`, as `keep` says, that fit `most` lines and `bytes` bytes,
/// each indented and cut to [`LINE_BYTES`], with a line saying how many others were left out.
fn bounded(lines: &[String], most: usize, bytes: usize, keep: Keep) -> String {
    let ordered: Box<dyn Iterator<Item = &String>> = match keep {
        Keep::First => Box::new(lines.iter()),
        Keep::Last => Box::new(lines.iter().rev()),
    };
    let mut kept: Vec<String> = Vec::new();
    let mut size = 0;
    for line in ordered {
        let line = format!("  {}\n", clipped(line, LINE_BYTES));
        if kept.len() == most || size + line.len() > bytes {
            break;
        }
        size += line.len();
        kept.push(line);
    }
    let left = lines.len() - kept.len();
    let mut text = String::new();
    match keep {
        Keep::First => {
            text.extend(kept);
            if left > 0 {
                let _ = writeln!(text, "  … {left} later lines left out");
            }
        }
        Keep::Last => {
            if left > 0 {
                let _ = writeln!(text, "  … {left} earlier lines left out");
            }
            text.extend(kept.into_iter().rev());
        }
    }
    text
}

/// `line` within `most` bytes, cut at a character's boundary and marked when cut.
fn clipped(line: &str, most: usize) -> String {
    if line.len() <= most {
        return line.to_owned();
    }
    let mut end = most;
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", line.get(..end).unwrap_or_default())
}

/// Whether `line` names `pid`: its digits, with no digit on either side. An ASCII digit is one
/// byte, and no byte of a longer character is one.
fn names(line: &str, pid: u32) -> bool {
    let digits = pid.to_string();
    let bytes = line.as_bytes();
    line.match_indices(&digits).any(|(at, _)| {
        let before = at.checked_sub(1).and_then(|before| bytes.get(before));
        let after = bytes.get(at + digits.len());
        !before.is_some_and(u8::is_ascii_digit) && !after.is_some_and(u8::is_ascii_digit)
    })
}

/// Every line naming `pid` in the logs (`.out`, `.err`, `.log`) of the scratch folders under
/// `root` whose names start with `prefix`, each after its path below `root`; `.git` folders are
/// passed over, and only the last [`LOG_TAIL`] bytes of a file are read.
fn scratch_lines(root: &Path, prefix: &str, pid: u32) -> Vec<String> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return found;
    };
    let mut folders: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(prefix))
        })
        .map(|entry| entry.path())
        .collect();
    symphony_board::order::sort_by(&mut folders, &Ord::cmp);
    for folder in folders {
        logs_naming(&folder, root, LOG_DEPTH, pid, &mut found);
    }
    found
}

fn logs_naming(dir: &Path, root: &Path, depth: usize, pid: u32, found: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    symphony_board::order::sort_by_key(&mut entries, &fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        // The entry's own type: a link is followed nowhere, and a FIFO is never opened.
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if depth > 0 && entry.file_name() != ".git" {
                logs_naming(&path, root, depth - 1, pid, found);
            }
        } else if kind.is_file()
            && matches!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("out" | "err" | "log")
            )
        {
            let shown = path.strip_prefix(root).unwrap_or(&path).display().to_string();
            found.extend(
                tail(&path)
                    .lines()
                    .filter(|line| names(line, pid))
                    .map(|line| format!("{shown}: {line}")),
            );
        }
    }
}

/// The last [`LOG_TAIL`] bytes of the file at `path`, lossily; nothing when it cannot be read.
fn tail(path: &Path) -> String {
    let mut bytes = Vec::new();
    if let Ok(mut file) = File::open(path)
        && let Ok(meta) = file.metadata()
        && file
            .seek(SeekFrom::Start(meta.len().saturating_sub(LOG_TAIL)))
            .is_ok()
    {
        let _ = file.take(LOG_TAIL).read_to_end(&mut bytes);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The rows of `table` (ps's `pid ppid …` rows) of processes that are `test`, name `named` in
/// their command line, or descend from one that does: `test`'s first, then the table's order.
#[cfg(any(target_os = "macos", test))]
fn ours(table: &str, test: u32, named: &str) -> Vec<String> {
    let mut rows: Vec<(u32, u32, &str)> = table.lines().filter_map(row).collect();
    symphony_board::order::sort_by_key(&mut rows, &|(pid, _, _)| *pid != test);
    let mut chosen: std::collections::HashSet<u32> = rows
        .iter()
        .filter(|(pid, _, line)| *pid == test || line.contains(named))
        .map(|(pid, _, _)| *pid)
        .collect();
    loop {
        let before = chosen.len();
        for (pid, parent, _) in &rows {
            if chosen.contains(parent) {
                chosen.insert(*pid);
            }
        }
        if chosen.len() == before {
            break;
        }
    }
    rows.iter()
        .filter(|(pid, _, _)| chosen.contains(pid))
        .map(|(_, _, line)| (*line).to_owned())
        .collect()
}

/// The row of `table` for `pid`.
#[cfg(any(target_os = "macos", test))]
fn row_of(table: &str, pid: u32) -> Option<String> {
    table
        .lines()
        .filter_map(row)
        .find(|(each, _, _)| *each == pid)
        .map(|(_, _, line)| line.to_owned())
}

/// A ps row's pid and parent, and the row trimmed.
#[cfg(any(target_os = "macos", test))]
fn row(line: &str) -> Option<(u32, u32, &str)> {
    let line = line.trim();
    let mut words = line.split_whitespace();
    let pid = words.next()?.parse().ok()?;
    let parent = words.next()?.parse().ok()?;
    Some((pid, parent, line))
}

/// `at` as `log show` reads a time: `YYYY-MM-DD HH:MM:SS+0000`, in UTC.
#[cfg(any(target_os = "macos", test))]
fn utc(at: SystemTime) -> String {
    let seconds = at.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let (year, month, day) = civil(i64::try_from(seconds / 86_400).unwrap_or(0));
    let time = seconds % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}+0000",
        time / 3600,
        time % 3600 / 60,
        time % 60
    )
}

/// The proleptic Gregorian date `days` after 1970-01-01 (Howard Hinnant's `civil_from_days`).
#[cfg(any(target_os = "macos", test))]
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let of_era = z.rem_euclid(146_097);
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * of_year + 2) / 153;
    let day = of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
#[path = "killed_tests.rs"]
mod tests;
