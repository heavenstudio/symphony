//! The release run's children, each in a process group of its own and bounded ([`Bound`]): past the
//! run's time, or once the run is told to stop, the whole group is stopped, so no helper it started
//! (a remote's transport, a test's server) runs on. A group is told from a later one given the same
//! id by its leader's start ([`Stamp`], [`lives`]).

use std::io::{self, Write as _};
use std::path::Path;
use std::process::Output;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use symphony_board::sys::{
    Boottime, Command, Start, process_starts, spawn_output, spawn_piped, wait_output,
};

/// How long a stopped group has to end on SIGTERM before SIGKILL.
pub(crate) const GRACE: Duration = Duration::from_secs(10);
/// How often a running child is looked at.
pub(crate) const POLL: Duration = Duration::from_millis(100);

/// What a run's waits end at: its time, or sooner when it is told to stop (`heard`, the run's
/// HUP, INT or TERM).
#[derive(Clone, Copy)]
pub(crate) struct Bound<'a> {
    pub(crate) until: Instant,
    heard: Option<&'a dyn Fn() -> bool>,
}

impl<'a> Bound<'a> {
    /// At `until` alone.
    pub(crate) fn until(until: Instant) -> Bound<'a> {
        Bound { until, heard: None }
    }

    /// At `until`, or once `heard` says the run was told to stop.
    pub(crate) fn hearing(until: Instant, heard: &'a dyn Fn() -> bool) -> Bound<'a> {
        Bound {
            until,
            heard: Some(heard),
        }
    }

    /// Whether the run was told to stop.
    pub(crate) fn interrupted(&self) -> bool {
        self.heard.is_some_and(|heard| heard())
    }

    /// The same, ending `most` from now when that is sooner.
    pub(crate) fn sooner(self, most: Duration) -> Bound<'a> {
        Bound {
            until: self.until.min(Instant::now() + most),
            ..self
        }
    }

    /// The same, ending no sooner than `least` from now.
    pub(crate) fn later(self, least: Duration) -> Bound<'a> {
        Bound {
            until: self.until.max(Instant::now() + least),
            ..self
        }
    }
}

/// Why a bounded command gave no output.
#[derive(Debug)]
pub(crate) enum Failed {
    /// It could not start, or its output could not be read.
    Io(io::Error),
    /// It ran past its time, so long, and its group was killed.
    Late(Duration),
    /// The run was told to stop after it had run so long, and its group was killed.
    Interrupted(Duration),
}

/// When a process started, as the kernel stamps it: what tells a group's leader from a later
/// process given the same id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) enum Stamp {
    /// In seconds since the epoch (macOS).
    At(f64),
    /// On the clock since boot (Linux).
    AfterBoot(Boottime),
}

/// The start of the process `pid`, when it runs and can be read.
pub(crate) fn stamp(pid: i64) -> Option<Stamp> {
    match process_starts(&[pid]).pop()? {
        Start::At(at) => Some(Stamp::At(at)),
        Start::AfterBoot(boot) => Some(Stamp::AfterBoot(boot)),
        Start::Ended | Start::Unreadable => None,
    }
}

/// Whether the process group `group`, whose leader started at `start`, still runs: a member that
/// is neither exiting nor a zombie, its leader or what that left running when it ended. A group of
/// that id whose living leader started at another time is another group; one whose start cannot be
/// told counts as that group.
///
/// It is asked of the group of a check this user's release run started, unattended, every member
/// of which is this user's (a set-user-id program keeps the real user), so this user's signal
/// reaches each of them while it runs. A signal that reaches no member (EPERM) says what ESRCH
/// does, that none of the check runs: it is Darwin's answer once each member is exiting or a
/// zombie, and otherwise one of a later group of another user's, which is not the check's either.
/// Linux's signal still reaches a zombie, so a member must also be one the census reads live
/// ([`members`]); a census that cannot be read leaves the group running.
pub(crate) fn lives(group: i64, start: Option<&Stamp>) -> bool {
    let Ok(id) = libc::pid_t::try_from(group) else {
        return false;
    };
    if id <= 1 {
        return false;
    }
    // SAFETY: signal 0 sends nothing: killpg only says whether the group has a process this user
    // may signal.
    let asked = unsafe { libc::killpg(id, 0) };
    if asked != 0
        && matches!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH | libc::EPERM)
        )
    {
        return false;
    }
    let runs =
        members(id.cast_unsigned()).map_or(true, |found| found.iter().any(|member| member.live));
    if !runs {
        return false;
    }
    match (start, stamp(group)) {
        (Some(start), Some(now)) => *start == now,
        _ => true,
    }
}

/// The process group a child leading its own is.
fn group_of(pid: u32) -> libc::pid_t {
    libc::pid_t::try_from(pid).unwrap_or(libc::pid_t::MAX)
}

/// `signal` to the process group `group`, which a child of this process leads.
pub(crate) fn signal(group: libc::pid_t, signal: libc::c_int) {
    // SAFETY: killpg sends a signal and touches no memory of this process. The group is one this
    // process made for its own child, whose pid (the group's id) stays reserved while it is
    // unreaped or while any process of the group lives.
    unsafe {
        libc::killpg(group, signal);
    }
}

/// Whether this process's child `pid` has ended, left unreaped (`WNOWAIT`), so its pid still names
/// it and its group; one that cannot be waited for any more has ended.
pub(crate) fn ended(pid: u32) -> bool {
    child_ended(pid).unwrap_or(true)
}

/// Observe only an unreaped child. An error does not authorize signalling its numeric group id.
fn child_ended(pid: u32) -> io::Result<bool> {
    if pid <= 1 || libc::pid_t::try_from(pid).is_err() {
        return Err(io::Error::from_raw_os_error(libc::EINVAL));
    }
    // SAFETY: siginfo_t is plain data, for which all zeroes is a value.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    retry_interrupted(|| {
        // SAFETY: observes only the named child, writing `info` and reaping none.
        let waited = unsafe {
            libc::waitid(
                libc::P_PID,
                libc::id_t::from(pid),
                &raw mut info,
                libc::WEXITED | libc::WNOWAIT | libc::WNOHANG,
            )
        };
        if waited != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(info.si_signo == libc::SIGCHLD
            && matches!(
                info.si_code,
                libc::CLD_EXITED | libc::CLD_KILLED | libc::CLD_DUMPED
            ))
    })
}

/// Stop the group the child `pid` leads: SIGTERM, up to [`GRACE`] for its leader to end, then
/// drain whatever of it is left. The leader is left unreaped, for its sole owner to wait for.
pub(crate) fn stop(pid: u32, grace: Duration) -> io::Result<()> {
    owned_signal(pid, libc::SIGTERM)?;
    let mut clock = Wall(Instant::now());
    while !child_ended(pid)? && clock.elapsed() < grace {
        clock.pause(POLL.min(grace.saturating_sub(clock.elapsed())));
    }
    kill(pid)
}

/// Kill remaining live members before the caller reaps the leader and frees its id. A group
/// signal can miss a concurrent fork, so repeat while any live member remains, for at most
/// [`GRACE`]. Zombies cannot fork or write and need not be reaped by their inheritor first.
pub(crate) fn kill(pid: u32) -> io::Result<()> {
    drain(
        &mut || owned_signal(pid, libc::SIGKILL),
        &mut || running(pid),
        GRACE,
        &mut Wall(Instant::now()),
    )
    .map_err(|error| io::Error::new(error.kind(), format!("cannot drain group {pid}: {error}")))
}

/// The caller alone owns and reaps this child throughout the operation. A number that no longer
/// names an unreaped child authorizes no signal.
fn owned_signal(pid: u32, how: libc::c_int) -> io::Result<()> {
    child_ended(pid)?;
    retry_interrupted(|| {
        // SAFETY: waitid above established the caller's unreaped child; its id stays reserved.
        if unsafe { libc::killpg(group_of(pid), how) } == 0 {
            return Ok(());
        }
        unreached(io::Error::last_os_error())
    })
}

/// A group signal that reached no member, which leaves the group to the census that follows
/// every signal ([`drain`]): ESRCH, no process left in it, and EPERM, Darwin's answer once each
/// member is a zombie or exiting. An exiting member is counted out of the signal as its exit
/// begins, some milliseconds before its status reads as a zombie (longer on a loaded machine), so
/// in between the census still reads it live: the drain looks again, and fails at its deadline
/// on a member that stays live, as one the signal may not reach would. Neither answer ever means
/// the cleanup succeeded. Every other refusal stands.
fn unreached(error: io::Error) -> io::Result<()> {
    match error.raw_os_error() {
        Some(libc::ESRCH | libc::EPERM) => Ok(()),
        _ => Err(error),
    }
}

fn retry_interrupted<T>(mut operation: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    loop {
        match operation() {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            answer => return answer,
        }
    }
}

/// The signal and census are separate: after every signal a new member can appear. No member
/// pid from a census is ever signalled individually, since it could be recycled before the kill.
fn drain(
    kill: &mut dyn FnMut() -> io::Result<()>,
    running: &mut dyn FnMut() -> io::Result<bool>,
    most: Duration,
    clock: &mut dyn Clock,
) -> io::Result<()> {
    loop {
        kill()?;
        if !running()? {
            return Ok(());
        }
        let left = most.saturating_sub(clock.elapsed());
        if left.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "live members remain after SIGKILL",
            ));
        }
        clock.pause(POLL.min(left));
    }
}

#[cfg(target_os = "macos")]
fn member_pids(group: u32) -> io::Result<Vec<i32>> {
    // sys/proc_info.h PROC_PGRP_ONLY. List only this group, including its zombies.
    const PROC_PGRP_ONLY: u32 = 2;
    let mut pids = vec![0i32; 64];
    loop {
        let size = i32::try_from(pids.len() * size_of::<i32>()).map_err(io::Error::other)?;
        let bytes = libproc_count(&mut || {
            // SAFETY: the buffer holds the supplied byte count, and libproc only writes pids.
            unsafe { libc::proc_listpids(PROC_PGRP_ONLY, group, pids.as_mut_ptr().cast(), size) }
        })?;
        let count = bytes / size_of::<i32>();
        if count == pids.len() {
            pids.resize(pids.len() * 2, 0);
            continue;
        }
        pids.truncate(count);
        break;
    }
    pids.retain(|pid| *pid > 0);
    Ok(pids)
}

/// libproc returns zero both for an empty list and for a syscall failure. Reset errno at each
/// attempt, before the native call, so stale errors cannot hide a genuinely empty group.
#[cfg(target_os = "macos")]
fn libproc_count(call: &mut dyn FnMut() -> libc::c_int) -> io::Result<usize> {
    retry_interrupted(|| {
        // SAFETY: __error returns this thread's live errno slot.
        let errno = unsafe { libc::__error() };
        // SAFETY: the slot belongs to this thread and is writable for one c_int.
        unsafe { errno.write(0) };
        let bytes = call();
        // SAFETY: the native call ran on this thread, whose errno slot remains valid.
        let error = unsafe { errno.read() };
        if bytes == 0 && error != 0 {
            Err(io::Error::from_raw_os_error(error))
        } else {
            usize::try_from(bytes).map_err(io::Error::other)
        }
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Member {
    pid: u32,
    start: (u64, u64),
    live: bool,
}

impl Member {
    /// An incomplete observation needs another look, never an assumption of successful cleanup.
    fn uncertain(pid: u32) -> Self {
        Self {
            pid,
            start: (0, 0),
            live: true,
        }
    }
}

fn running(group: u32) -> io::Result<bool> {
    // A kernel census can cease listing a dying leader before waitid reports its exit. Keep
    // that wait inside the bounded drain too, so the caller's final reap cannot block on it.
    if !child_ended(group)? {
        return Ok(true);
    }
    changing(&mut || members(group))
}

/// A parent can fork and end between a membership list and its status read. A second census
/// must contain only the same zombies before cleanup can succeed. Starts distinguish recycled
/// member pids; none is used as a target for a signal.
fn changing(census: &mut dyn FnMut() -> io::Result<Vec<Member>>) -> io::Result<bool> {
    let before = census()?;
    if before.iter().any(|member| member.live) {
        return Ok(true);
    }
    Ok(census()?
        .iter()
        .any(|member| member.live || !before.contains(member)))
}

#[cfg(target_os = "macos")]
fn members(group: u32) -> io::Result<Vec<Member>> {
    let pids = member_pids(group)?;
    let mut found = Vec::new();
    for pid in pids {
        // SAFETY: proc_bsdinfo is plain output data for which all zeroes is valid.
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = i32::try_from(size_of::<libc::proc_bsdinfo>()).unwrap();
        let read = retry_interrupted(|| {
            // SAFETY: libproc receives exactly the size of the output buffer. The nonzero arg
            // includes zombies (XNU proc_pidinfo), so an exiting parent cannot masquerade as
            // absent just because the default query excludes zombies.
            let read = unsafe {
                libc::proc_pidinfo(
                    pid,
                    libc::PROC_PIDTBSDINFO,
                    1,
                    std::ptr::from_mut(&mut info).cast(),
                    size,
                )
            };
            if read == size {
                return Ok(true);
            }
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                Ok(false)
            } else {
                Err(error)
            }
        })?;
        if read && info.pbi_pgid == group {
            found.push(Member {
                pid: info.pbi_pid,
                start: (info.pbi_start_tvsec, info.pbi_start_tvusec),
                live: info.pbi_status != 5,
            });
        } else {
            // It left the group or was reaped after the list. It could have forked first; make
            // the drain look again rather than calling that incomplete census empty.
            found.push(Member::uncertain(pid as u32));
        }
    }
    Ok(found)
}

#[cfg(target_os = "linux")]
fn members(group: u32) -> io::Result<Vec<Member>> {
    linux_members(Path::new("/proc"), group, &mut process_group)
}

#[cfg(any(target_os = "linux", test))]
fn process_group(pid: u32) -> io::Result<Option<u32>> {
    if pid == 0 {
        return Err(io::Error::from_raw_os_error(libc::EINVAL));
    }
    let pid = libc::pid_t::try_from(pid).map_err(io::Error::other)?;
    retry_interrupted(|| {
        // SAFETY: getpgid only observes this positive process id; it sends no signal.
        let group = unsafe { libc::getpgid(pid) };
        if group >= 0 {
            return Ok(Some(group as u32));
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(None)
        } else {
            Err(error)
        }
    })
}

#[cfg(any(target_os = "linux", test))]
fn linux_members(
    root: &Path,
    group: u32,
    group_of: &mut dyn FnMut(u32) -> io::Result<Option<u32>>,
) -> io::Result<Vec<Member>> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        match group_of(pid)? {
            Some(current) if current != group => continue,
            None => {
                found.push(Member::uncertain(pid));
                continue;
            }
            Some(_) => {}
        }
        let text = match std::fs::read_to_string(entry.path().join("stat")) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                found.push(Member::uncertain(pid));
                continue;
            }
            Err(error) => return Err(error),
        };
        found.push(stat_member(pid, &text, group)?.unwrap_or_else(|| Member::uncertain(pid)));
    }
    Ok(found)
}

#[cfg(any(target_os = "linux", test))]
fn stat_member(pid: u32, stat: &str, group: u32) -> io::Result<Option<Member>> {
    let fields: Vec<_> = stat
        .rsplit_once(')')
        .map_or("", |(_, fields)| fields)
        .split_whitespace()
        .collect();
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid /proc process stat");
    let pgrp = fields
        .get(2)
        .and_then(|field| field.parse::<u32>().ok())
        .ok_or_else(invalid)?;
    if pgrp != group {
        return Ok(None);
    }
    let start = fields
        .get(19)
        .and_then(|field| field.parse().ok())
        .ok_or_else(invalid)?;
    Ok(Some(Member {
        pid,
        start: (start, 0),
        live: !matches!(fields.first(), Some(&"Z" | &"X" | &"x")),
    }))
}

/// `command` run in a process group of its own, stdin null and its output read, within `bound`: a
/// command still running when it ends has its whole group killed.
pub(crate) fn output_until(command: &mut Command, bound: Bound<'_>) -> Result<Output, Failed> {
    output_fed_until(command, None, bound)
}

/// [`output_until`], `input` written to the command's stdin, then closed, by a thread of its own,
/// so a command that answers as it reads (`git cat-file --batch-check`) is read meanwhile and
/// neither waits on the other. A command that ends before reading it all is answered by its own
/// status, and the writer then ends at the broken pipe.
pub(crate) fn fed_until(
    command: &mut Command,
    input: Vec<u8>,
    bound: Bound<'_>,
) -> Result<Output, Failed> {
    output_fed_until(command, Some(input), bound)
}

fn output_fed_until(
    command: &mut Command,
    input: Option<Vec<u8>>,
    bound: Bound<'_>,
) -> Result<Output, Failed> {
    let started = Instant::now();
    command.process_group(0);
    let mut child = match input {
        None => spawn_output(command),
        Some(_) => spawn_piped(command),
    }
    .map_err(Failed::Io)?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }
    let pid = child.id();
    let (tell, heard) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tell.send(wait_output(child));
    });
    loop {
        let left = bound.until.saturating_duration_since(Instant::now());
        let stopped = match heard.recv_timeout(left.min(POLL)) {
            Ok(output) => return output.map_err(Failed::Io),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(Failed::Io(io::Error::other(
                    "its waiter ended without a word",
                )));
            }
            Err(mpsc::RecvTimeoutError::Timeout) if bound.interrupted() => {
                Failed::Interrupted(started.elapsed())
            }
            Err(mpsc::RecvTimeoutError::Timeout) if left.is_zero() => {
                Failed::Late(started.elapsed())
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
        };
        // Its output is read to its end before it is reaped, so a command still writing is
        // unreaped and its group is its own.
        signal(group_of(pid), libc::SIGKILL);
        let _ = heard.recv();
        return Err(stopped);
    }
}

/// The processes that hold `file` open, by `lsof` within a minute: none when no process does.
pub(crate) fn holders(file: &Path) -> Result<Vec<u32>, String> {
    holders_within(file, Bound::until(Instant::now() + Duration::from_secs(60)))
}

/// [`holders`], lsof stopped when `bound` ends.
fn holders_within(file: &Path, bound: Bound<'_>) -> Result<Vec<u32>, String> {
    let done = output_until(
        symphony::children::command("lsof")
            .args(["-w", "-t", "--"])
            .arg(file),
        bound,
    )
    .map_err(|failed| match failed {
        Failed::Io(e) => format!("cannot run lsof: {e}"),
        Failed::Late(took) | Failed::Interrupted(took) => {
            format!("lsof did not end within {} s", took.as_secs())
        }
    })?;
    let pids: Vec<u32> = String::from_utf8_lossy(&done.stdout)
        .split_whitespace()
        .filter_map(|word| word.parse().ok())
        .collect();
    if pids.is_empty() && done.status.code() != Some(1) && !done.status.success() {
        return Err(format!(
            "lsof failed on {}: {}",
            file.display(),
            String::from_utf8_lossy(&done.stderr).trim()
        ));
    }
    Ok(pids)
}

/// A process working in a folder, or holding a file inside it open: its id, its command's name and
/// where in the folder.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Inside {
    pub(crate) pid: u32,
    pub(crate) command: String,
    /// `working there` (in the folder itself) or `working in REL`, for its working directory, else
    /// the first file inside it holds open.
    pub(crate) what: String,
}

impl Inside {
    /// The process and what it does there, in a few words.
    pub(crate) fn said(&self) -> String {
        format!("process {} ({}), {}", self.pid, self.command, self.what)
    }

    /// The folder it works in, relative to the tree it was found in (empty for the tree itself),
    /// as [`inside_of`] said it; none for a process found by a file it holds open.
    pub(crate) fn working_in(&self) -> Option<&str> {
        if self.what == WORKING_THERE {
            return Some("");
        }
        self.what.strip_prefix(WORKING_IN)
    }
}

/// What [`inside_of`] says of a process working in the tree itself, and before a folder inside it.
const WORKING_THERE: &str = "working there";
const WORKING_IN: &str = "working in ";

/// How long what a look finds in a folder has to leave it before it counts: what a check's killed
/// group still runs leaves within it. The board's recorder of a check is never among what a look
/// finds, however long it takes to write down how the check ended: it works at the root, out of
/// the checkout, and holds no file of it open (docs/workflow/MACHINE_WORK.md).
pub(crate) const SETTLE: Duration = Duration::from_secs(5);
/// How long a look waits before it lists again what it found.
const AGAIN: Duration = Duration::from_secs(1);

/// The processes but this one that `lsof` sees working in each of `trees` (real paths) or holding
/// a file inside it open, within `bound` and each listing within a minute, a list for each: as
/// listed once none is found, or [`SETTLE`] after the first listing found one, so a process on its
/// way out is not counted. Each listing is one of every process, read by path: a search of a tree
/// (`lsof +D`) reads every file of its build folder first.
pub(crate) fn inside(trees: &[&Path], bound: Bound<'_>) -> Result<Vec<Vec<Inside>>, String> {
    let mut list = || {
        listing(
            &mut symphony::children::command("lsof"),
            bound.sooner(Duration::from_secs(60)),
        )
    };
    settled(
        &mut list,
        trees,
        std::process::id(),
        SETTLE,
        &mut Wall(Instant::now()),
    )
}

/// The time a look settles by: how long since it began, and a pause before it lists again.
trait Clock {
    fn elapsed(&self) -> Duration;
    fn pause(&mut self, pause: Duration);
}

/// The machine's clock, from the instant a look began.
struct Wall(Instant);

impl Clock for Wall {
    fn elapsed(&self) -> Duration {
        self.0.elapsed()
    }

    fn pause(&mut self, pause: Duration) {
        #[expect(
            clippy::disallowed_methods,
            reason = "no-timed-waits baseline: the machine's pause behind the clock its tests replace"
        )]
        std::thread::sleep(pause);
    }
}

/// A listing of every process's working directory and open files by `lsof`, a command for lsof to
/// which this adds the arguments, within `bound`: never an empty one.
fn listing(lsof: &mut Command, bound: Bound<'_>) -> Result<String, String> {
    let done =
        output_until(lsof.args(["-n", "-P", "-w", "-F", "pcfn"]), bound).map_err(|failed| {
            match failed {
                Failed::Io(e) => format!("cannot run lsof: {e}"),
                Failed::Late(took) => format!("lsof did not end within {} s", took.as_secs()),
                Failed::Interrupted(took) => format!(
                    "lsof was stopped after {} s: the run was told to stop",
                    took.as_secs()
                ),
            }
        })?;
    let listing = String::from_utf8_lossy(&done.stdout).into_owned();
    if listing.trim().is_empty() {
        return Err(format!(
            "lsof listed no process: {}",
            String::from_utf8_lossy(&done.stderr).trim()
        ));
    }
    Ok(listing)
}

/// What the listings `list` makes say works in each of `trees`, but the process `me`: listed again
/// every [`AGAIN`] while something is found, until nothing is or `settle` has passed on `clock`
/// since the first, and the last listing's.
fn settled(
    list: &mut dyn FnMut() -> Result<String, String>,
    trees: &[&Path],
    me: u32,
    settle: Duration,
    clock: &mut dyn Clock,
) -> Result<Vec<Vec<Inside>>, String> {
    loop {
        let listing = list()?;
        let found: Vec<Vec<Inside>> = (trees.iter())
            .map(|tree| inside_of(&listing, tree, me))
            .collect();
        let left = settle.saturating_sub(clock.elapsed());
        if found.iter().all(Vec::is_empty) || left.is_zero() {
            return Ok(found);
        }
        clock.pause(AGAIN.min(left));
    }
}

/// What a listing of `lsof -F pcfn` says works in `tree` or holds a file inside it, but the process
/// `me`: one entry a process, its working directory before the files it holds.
fn inside_of(listing: &str, tree: &Path, me: u32) -> Vec<Inside> {
    let tree = tree.to_string_lossy();
    let mut found: Vec<Inside> = Vec::new();
    let (mut pid, mut command, mut fd) = (0, String::new(), String::new());
    for line in listing.lines() {
        let mut chars = line.chars();
        let field = chars.next();
        let value = chars.as_str();
        match field {
            Some('p') => {
                pid = value.parse().unwrap_or(0);
                command.clear();
                fd.clear();
            }
            Some('c') => value.clone_into(&mut command),
            Some('f') => value.clone_into(&mut fd),
            Some('n') => {
                let Some(rest) = value.strip_prefix(&*tree) else {
                    continue;
                };
                if !(rest.is_empty() || rest.starts_with('/')) || pid == 0 || pid == me {
                    continue;
                }
                let inner = rest.trim_start_matches('/');
                let what = match (fd == "cwd", inner.is_empty()) {
                    (true, true) => WORKING_THERE.to_string(),
                    (true, false) => format!("{WORKING_IN}{inner}"),
                    (false, true) => "holding . open".to_string(),
                    (false, false) => format!("holding {inner} open"),
                };
                match found.iter_mut().find(|seen| seen.pid == pid) {
                    Some(seen) if fd == "cwd" => seen.what = what,
                    Some(_) => {}
                    None => found.push(Inside {
                        pid,
                        command: command.clone(),
                        what,
                    }),
                }
            }
            _ => {}
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::super::waits::{far, gone, pid_written, untimed, written_pid};
    use super::*;
    use crate::test_tree::Tree;
    use symphony::children::command;
    use symphony_board::sys::{Child, Stdio};

    /// Each fixture process remains our unreaped child even when an assertion panics.
    struct Owned(Child);

    impl Drop for Owned {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// A clock that moves only by the pauses taken on it.
    #[derive(Default)]
    struct Paused(Duration);

    impl Clock for Paused {
        fn elapsed(&self) -> Duration {
            self.0
        }

        fn pause(&mut self, pause: Duration) {
            self.0 += pause;
        }
    }

    #[test]
    fn a_drain_retries_live_members_and_reports_errors_or_its_deadline() {
        let calls = std::cell::Cell::new(0);
        let mut kill = || {
            calls.set(calls.get() + 1);
            Ok(())
        };
        let mut clock = Paused::default();
        drain(&mut kill, &mut || Ok(calls.get() < 3), GRACE, &mut clock).unwrap();
        assert_eq!(calls.get(), 3);
        assert_eq!(clock.elapsed(), POLL * 2);
        let error = drain(&mut kill, &mut || Ok(true), Duration::ZERO, &mut clock).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        let before = calls.get();
        let error = drain(
            &mut || Err(io::Error::from_raw_os_error(libc::EPERM)),
            &mut || panic!("a failed signal must not be called clean"),
            GRACE,
            &mut clock,
        )
        .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(libc::EPERM));
        assert_eq!(calls.get(), before);
        let error = drain(
            &mut kill,
            &mut || Err(io::Error::other("census refused")),
            GRACE,
            &mut clock,
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "census refused");
        // Even at its deadline a completed drain succeeds, without a timer assertion.
        drain(&mut kill, &mut || Ok(false), Duration::ZERO, &mut clock).unwrap();
    }

    #[test]
    fn a_signal_that_reached_no_member_leaves_the_group_to_the_census() {
        let refused = |code| unreached(io::Error::from_raw_os_error(code));
        refused(libc::ESRCH).unwrap();
        // Darwin's answer while the only members left are exiting, which the census reads live.
        refused(libc::EPERM).unwrap();
        for stands in [libc::EINVAL, libc::EINTR] {
            assert_eq!(refused(stands).unwrap_err().raw_os_error(), Some(stands));
        }
        // The census decides: a member that leaves is drained, one that stays live is an error
        // at the deadline, never a successful cleanup.
        let looks = std::cell::Cell::new(0);
        let mut clock = Paused::default();
        drain(
            &mut || refused(libc::EPERM),
            &mut || {
                looks.set(looks.get() + 1);
                Ok(looks.get() < 3)
            },
            GRACE,
            &mut clock,
        )
        .unwrap();
        assert_eq!((looks.get(), clock.elapsed()), (3, POLL * 2));
        let error = drain(
            &mut || refused(libc::EPERM),
            &mut || Ok(true),
            GRACE,
            &mut clock,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn a_census_confirms_only_the_same_zombies_and_never_assumes_an_error_is_empty() {
        let zombie = Member {
            pid: 10,
            start: (100, 0),
            live: false,
        };
        let live = Member {
            live: true,
            ..zombie.clone()
        };
        let cases = [
            (vec![], vec![], false),
            (vec![zombie.clone()], vec![zombie.clone()], false),
            (vec![zombie.clone()], vec![], false),
            (vec![live.clone()], vec![], true),
            (vec![zombie.clone()], vec![live], true),
            (
                vec![zombie.clone()],
                vec![Member {
                    pid: 11,
                    ..zombie.clone()
                }],
                true,
            ),
            (
                vec![zombie.clone()],
                vec![Member {
                    start: (101, 0),
                    ..zombie
                }],
                true,
            ),
        ];
        for (first, second, expected) in cases {
            let mut looks = [first, second].into_iter();
            assert_eq!(
                changing(&mut || Ok(looks.next().unwrap())).unwrap(),
                expected
            );
        }
        assert!(changing(&mut || Err(io::Error::other("unreadable"))).is_err());
        let mut looks = [Ok(vec![]), Err(io::Error::other("second census failed"))].into_iter();
        assert!(changing(&mut || looks.next().unwrap()).is_err());
    }

    #[test]
    fn interrupted_observations_retry_and_other_errors_stand() {
        let mut answers = [Err(io::Error::from_raw_os_error(libc::EINTR)), Ok(42)].into_iter();
        assert_eq!(retry_interrupted(|| answers.next().unwrap()).unwrap(), 42);
        let error = retry_interrupted(|| Err::<(), _>(io::Error::from_raw_os_error(libc::EACCES)))
            .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(libc::EACCES));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn libproc_zero_results_retry_interruptions_preserve_errors_and_reset_stale_errno() {
        fn set_errno(value: libc::c_int) {
            // SAFETY: __error returns this thread's live errno slot.
            let errno = unsafe { libc::__error() };
            // SAFETY: this thread owns the writable slot for one c_int.
            unsafe { errno.write(value) };
        }
        fn errno() -> libc::c_int {
            // SAFETY: __error returns this thread's live errno slot.
            let errno = unsafe { libc::__error() };
            // SAFETY: the slot remains valid and readable on this thread.
            unsafe { errno.read() }
        }

        set_errno(libc::ENOMEM);
        let mut attempts = 0;
        let empty = libproc_count(&mut || {
            assert_eq!(errno(), 0, "errno is reset before every native attempt");
            attempts += 1;
            if attempts == 1 {
                set_errno(libc::EINTR);
            }
            0
        })
        .unwrap();
        assert_eq!((empty, attempts), (0, 2));
        for refused in [libc::ENOMEM, libc::EPERM] {
            let mut attempts = 0;
            let error = libproc_count(&mut || {
                assert_eq!(errno(), 0);
                attempts += 1;
                set_errno(refused);
                0
            })
            .unwrap_err();
            assert_eq!(error.raw_os_error(), Some(refused));
            assert_eq!(attempts, 1, "a non-retryable failure is not retried");
        }
        set_errno(libc::EINTR);
        assert_eq!(
            libproc_count(&mut || {
                assert_eq!(errno(), 0);
                0
            })
            .unwrap(),
            0
        );
        assert_eq!(
            libproc_count(&mut || {
                set_errno(libc::EIO);
                4
            })
            .unwrap(),
            4
        );
        assert!(libproc_count(&mut || -1).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_native_libproc_error_is_not_an_empty_group() {
        let mut byte = 0u8;
        let error = libproc_count(&mut || {
            // SAFETY: the output buffer has the supplied one-byte size. libproc rejects it as
            // too small for a pid before writing anything; this call only observes processes.
            unsafe {
                libc::proc_listpids(
                    2,
                    std::process::id(),
                    std::ptr::from_mut(&mut byte).cast(),
                    1,
                )
            }
        })
        .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(libc::ENOMEM));
        // A negative numeric group cannot name a live process group. The same boundary must
        // accept its genuine empty success even after the preceding call set errno to ENOMEM.
        assert!(member_pids(u32::MAX).unwrap().is_empty());
    }

    #[test]
    fn linux_member_status_reads_past_names_and_excludes_zombies_and_other_groups() {
        let stat = |state| {
            format!("10 (helper (with spaces)) {state} 1 10 10 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 123 0")
        };
        for state in ["R", "S", "T", "D"] {
            let member = stat_member(10, &stat(state), 10).unwrap().unwrap();
            assert_eq!(
                member,
                Member {
                    pid: 10,
                    start: (123, 0),
                    live: true
                }
            );
        }
        for state in ["Z", "X", "x"] {
            assert!(!stat_member(10, &stat(state), 10).unwrap().unwrap().live);
        }
        assert!(stat_member(10, &stat("S"), 11).unwrap().is_none());
        for bad in [
            "",
            "10 (name) S 1 invalid",
            "10 (name) Z 1 10",
            "10 (name) S 1 10 0",
        ] {
            assert_eq!(
                stat_member(10, bad, 10).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
    }

    #[test]
    fn a_linux_census_rechecks_disappearing_or_changed_members_and_preserves_errors() {
        let tree = Tree::new();
        let stat = |group| format!("7 (helper) Z 1 {group} 10 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 123 0");
        tree.write("self", "not a numeric process directory");
        tree.write("7/stat", &stat(10));
        tree.write("8/stat", "another group's stat is not read");
        tree.write("9/placeholder", "the stat disappeared after getpgid");
        tree.write("10/stat", &stat(11));
        tree.write("11/stat", "the process disappeared before getpgid");
        let mut group_of = |pid| {
            Ok(match pid {
                8 => Some(11),
                11 => None,
                _ => Some(10),
            })
        };
        let mut found = linux_members(&tree.root, 10, &mut group_of).unwrap();
        found.sort_by_key(|member| member.pid);
        assert_eq!(
            found,
            [
                Member {
                    pid: 7,
                    start: (123, 0),
                    live: false
                },
                Member::uncertain(9),
                Member::uncertain(10),
                Member::uncertain(11)
            ]
        );
        let error = linux_members(&tree.root, 10, &mut |_| {
            Err(io::Error::other("getpgid refused"))
        })
        .unwrap_err();
        assert_eq!(error.to_string(), "getpgid refused");
        assert!(linux_members(&tree.path("missing"), 10, &mut group_of).is_err());
        tree.write("7/stat", "malformed");
        assert_eq!(
            linux_members(&tree.root, 10, &mut group_of)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        let unreadable = Tree::new();
        unreadable.write(
            "7/stat/directory",
            "a stat that is a directory cannot be read",
        );
        assert!(linux_members(&unreadable.root, 10, &mut group_of).is_err());
    }

    #[test]
    fn a_native_group_observation_reads_only_positive_pids() {
        let child = Owned(
            command("sleep")
                .arg("1000")
                .process_group(0)
                .spawn()
                .unwrap(),
        );
        assert_eq!(process_group(child.0.id()).unwrap(), Some(child.0.id()));
        assert!(process_group(0).is_err());
        assert!(process_group(u32::MAX).is_err());
    }

    #[test]
    fn a_reaped_or_unowned_leader_never_authorizes_a_group_signal() {
        // A live owned group stands in for a recycled numeric id. waitid must refuse it when
        // called by a different parent, and the original group must survive that attempt.
        const CASE: &str = "SYMPHONY_RELEASE_UNOWNED_GROUP";
        if let Ok(pid) = std::env::var(CASE) {
            let pid = pid.parse().unwrap();
            assert_eq!(
                child_ended(pid).unwrap_err().raw_os_error(),
                Some(libc::ECHILD)
            );
            let error = kill(pid).unwrap_err();
            assert!(error.to_string().contains("cannot drain group"));
            return;
        }
        let mut child = Owned(
            command("sleep")
                .arg("1000")
                .process_group(0)
                .spawn()
                .unwrap(),
        );
        let name = format!(
            "{}::a_reaped_or_unowned_leader_never_authorizes_a_group_signal",
            module_path!().split_once("::").unwrap().1
        );
        let done = output_until(
            command(std::env::current_exe().unwrap())
                .args(["--exact", &name])
                .env(CASE, child.0.id().to_string()),
            far(),
        )
        .unwrap();
        assert!(
            done.status.success(),
            "{}",
            String::from_utf8_lossy(&done.stdout)
        );
        assert!(
            !ended(child.0.id()),
            "an unowned numeric id received no signal"
        );
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        assert!(kill(child.0.id()).is_err());
        for invalid in [0, 1, u32::MAX] {
            assert!(stop(invalid, Duration::ZERO).is_err());
        }
    }

    #[test]
    fn a_member_appearing_after_the_first_kill_is_drained_before_the_leader_is_reaped() {
        use std::io::{Read, Write};
        const CASE: &str = "SYMPHONY_RELEASE_JOIN_GROUP";
        if let Ok(group) = std::env::var(CASE) {
            let group: libc::pid_t = group.parse().unwrap();
            std::io::stdin().read_exact(&mut [0]).unwrap();
            // SAFETY: this fixture changes only its own group, to its sibling's owned group.
            assert_eq!(unsafe { libc::setpgid(0, group) }, 0);
            std::fs::write(
                std::env::var_os("SYMPHONY_RELEASE_JOINED").unwrap(),
                "joined",
            )
            .unwrap();
            std::io::stdin().read_exact(&mut [0]).unwrap();
            panic!("the fixture must be killed, not released twice");
        }
        let tree = Tree::new();
        let joined = tree.path("joined");
        let mut leader = Owned(
            command("sleep")
                .arg("1000")
                .process_group(0)
                .spawn()
                .unwrap(),
        );
        let pid = leader.0.id();
        let name = format!(
            "{}::a_member_appearing_after_the_first_kill_is_drained_before_the_leader_is_reaped",
            module_path!().split_once("::").unwrap().1
        );
        let mut helper = Owned(
            command(std::env::current_exe().unwrap())
                .args(["--exact", &name])
                .env(CASE, pid.to_string())
                .env("SYMPHONY_RELEASE_JOINED", &joined)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let helper_pid = helper.0.id();
        let mut calls = 0;
        drain(
            &mut || {
                owned_signal(pid, libc::SIGKILL)?;
                calls += 1;
                if calls == 1 {
                    // Barrier models a member missed by the first signal's membership snapshot.
                    helper.0.stdin.as_mut().unwrap().write_all(b"x").unwrap();
                    test_time::until_true(
                        "the late member joined the owned group",
                        test_time::FLOOR,
                        || joined.exists(),
                    )
                    .unwrap_or_else(|stall| panic!("{stall}"));
                    assert!(running(pid).unwrap());
                } else {
                    // Native scheduling is bounded by a condition's stall backstop, separately
                    // from the injected clock used for the drain's product deadline.
                    test_time::until_true(
                        "the killed leader and late member exited",
                        test_time::FLOOR,
                        || child_ended(pid).unwrap() && child_ended(helper_pid).unwrap(),
                    )
                    .unwrap_or_else(|stall| panic!("{stall}"));
                }
                Ok(())
            },
            &mut || running(pid),
            GRACE,
            &mut Paused::default(),
        )
        .unwrap();
        assert!(calls >= 2, "the late member needs another group signal");
        assert!(
            child_ended(pid).unwrap(),
            "the leader remains an unreaped child"
        );
        assert!(!running(pid).unwrap());
        for child in [&mut leader.0, &mut helper.0] {
            assert_eq!(
                std::os::unix::process::ExitStatusExt::signal(&child.wait().unwrap()),
                Some(libc::SIGKILL)
            );
        }
    }

    #[test]
    fn a_stopped_child_is_live_and_a_zombie_only_group_drains_without_reaping() {
        let mut child = Owned(
            command("sleep")
                .arg("1000")
                .process_group(0)
                .spawn()
                .unwrap(),
        );
        let pid = child.0.id();
        owned_signal(pid, libc::SIGSTOP).unwrap();
        test_time::until_true("the owned child stopped", test_time::FLOOR, || {
            // waitid may report CLD_STOPPED on Darwin even when only WEXITED was requested.
            // SAFETY: siginfo_t is plain output data for which all zeroes is valid.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            // SAFETY: observes only our unreaped child and writes exactly one siginfo_t.
            let read = unsafe {
                libc::waitid(
                    libc::P_PID,
                    pid,
                    &raw mut info,
                    libc::WSTOPPED | libc::WNOWAIT | libc::WNOHANG,
                )
            };
            read == 0 && info.si_code == libc::CLD_STOPPED
        })
        .unwrap_or_else(|stall| panic!("{stall}"));
        assert!(!ended(pid));
        assert!(running(pid).unwrap());
        kill(pid).unwrap();
        assert!(child_ended(pid).unwrap());
        assert!(!running(pid).unwrap(), "its unreaped zombie runs nothing");
        assert_eq!(
            members(pid)
                .unwrap()
                .iter()
                .filter(|member| member.pid == pid)
                .map(|member| member.live)
                .collect::<Vec<_>>(),
            [false],
            "the native census includes the unreaped leader as a zombie"
        );
        kill(pid).unwrap();
        child.0.wait().unwrap();
    }

    #[test]
    fn a_listing_names_each_process_working_in_the_tree_or_holding_a_file_inside_it_once() {
        let listing = "p10\nczsh\nfcwd\nn/wt/release\nftxt\nn/bin/zsh\n\
                       p11\nccargo\nf3r\nn/wt/release/.target/debug/a.rlib\nf4w\nn/wt/release/src/b.rs\n\
                       fcwd\nn/wt/release/crates\n\
                       p12\ncperl\nfcwd\nn/\nf5w\nn/wt/release-other/x\nf6r\nn/wt/releasex/y\n\
                       p13\ncsh\nfcwd\nn/wt/release\n\
                       p14\ncvim\nf7r\nn/wt/release\n\
                       garbage\n\n";
        assert_eq!(
            inside_of(listing, Path::new("/wt/release"), 13),
            [
                Inside {
                    pid: 10,
                    command: "zsh".into(),
                    what: "working there".into()
                },
                Inside {
                    pid: 11,
                    command: "cargo".into(),
                    what: "working in crates".into()
                },
                Inside {
                    pid: 14,
                    command: "vim".into(),
                    what: "holding . open".into()
                },
            ],
            "a folder beside it of a longer name is not inside, nor is this process"
        );
        assert_eq!(
            inside_of(listing, Path::new("/wt/release-other"), 1),
            [Inside {
                pid: 12,
                command: "perl".into(),
                what: "holding x open".into()
            }]
        );
        assert_eq!(
            inside_of(
                "p11\nccargo\nf3r\nn/wt/release/src/b.rs\n",
                Path::new("/wt/release"),
                1
            )[0]
            .said(),
            "process 11 (cargo), holding src/b.rs open"
        );
        assert!(inside_of(listing, Path::new("/elsewhere"), 1).is_empty());
    }

    #[test]
    fn a_process_found_by_its_working_folder_says_that_folder_and_one_found_by_a_file_none() {
        let listing = "p10\nczsh\nfcwd\nn/wt/release\n\
                       p11\ncchrome_crashpad_handler\nfcwd\nn/wt/release/apps/symphony-web\n\
                       p12\ncvim\nf7r\nn/wt/release/apps/symphony-web/src/a.rs\n\
                       p13\ncsh\nfcwd\nn/wt/release/working in/x\n";
        let found = inside_of(listing, Path::new("/wt/release"), 1);
        let folders: Vec<Option<&str>> = found.iter().map(Inside::working_in).collect();
        assert_eq!(
            folders,
            [
                Some(""),
                Some("apps/symphony-web"),
                None,
                Some("working in/x")
            ]
        );
    }

    #[test]
    fn a_listing_that_cannot_be_had_is_why_and_never_an_empty_one() {
        let soon = far;
        let refused = listing(&mut command("/no/such/lsof"), soon()).unwrap_err();
        assert!(refused.starts_with("cannot run lsof: "), "{refused}");
        let mut silent = command("sh");
        silent.args(["-c", "echo 'lsof: no listing' >&2; exit 1", "lsof"]);
        assert_eq!(
            listing(&mut silent, soon()).unwrap_err(),
            "lsof listed no process: lsof: no listing"
        );
        // Its time up before it starts, the first look stops it.
        let mut slow = command("sh");
        slow.args(["-c", "sleep 30", "lsof"]);
        let late = Bound::until(Instant::now());
        assert_eq!(
            untimed(&listing(&mut slow, late).unwrap_err()),
            "lsof did not end within N s"
        );
        let mut told = command("sh");
        told.args(["-c", "sleep 30", "lsof"]);
        let heard = || true;
        let stopping = Bound::hearing(far().until, &heard);
        assert_eq!(
            untimed(&listing(&mut told, stopping).unwrap_err()),
            "lsof was stopped after N s: the run was told to stop"
        );
        // One that could not read every process still says what it read.
        let mut partial = command("sh");
        partial.args([
            "-c",
            "printf 'p7\\ncsh\\nfcwd\\nn/wt/release\\n'; exit 1",
            "lsof",
        ]);
        assert_eq!(
            listing(&mut partial, soon()).unwrap(),
            "p7\ncsh\nfcwd\nn/wt/release\n"
        );
    }

    /// A stand-in for lsof answering `listings` in turn, the last again once they run out, and
    /// counting the calls in `calls`.
    fn listings<'a>(
        listings: &'a [Result<&'a str, &'a str>],
        calls: &'a std::cell::Cell<usize>,
    ) -> impl FnMut() -> Result<String, String> + 'a {
        move || {
            let next = listings[calls.get().min(listings.len() - 1)];
            calls.set(calls.get() + 1);
            next.map(str::to_string).map_err(str::to_string)
        }
    }

    #[test]
    fn what_leaves_a_folder_while_the_look_settles_is_not_counted_and_what_stays_is() {
        let trees = [Path::new("/wt/release"), Path::new("/wt/release-other")];
        let both = "p10\ncsh\nfcwd\nn/wt/release\np11\nccargo\nfcwd\nn/wt/release-other/x\n";
        let other_only = "p11\nccargo\nfcwd\nn/wt/release-other/x\n";
        let nothing = "p1\nclaunchd\nfcwd\nn/\n";
        let cargo = || Inside {
            pid: 11,
            command: "cargo".into(),
            what: "working in x".into(),
        };

        // Nothing found: one listing.
        let calls = std::cell::Cell::new(0);
        let found = settled(
            &mut listings(&[Ok(nothing)], &calls),
            &trees,
            1,
            SETTLE,
            &mut Paused::default(),
        )
        .unwrap();
        assert_eq!(found, [vec![], vec![]]);
        assert_eq!(calls.get(), 1);

        // Found, then gone at the next listing, a second later: nothing counts.
        let calls = std::cell::Cell::new(0);
        let mut clock = Paused::default();
        let gone = [Ok(both), Ok(nothing)];
        let found = settled(&mut listings(&gone, &calls), &trees, 1, SETTLE, &mut clock).unwrap();
        assert_eq!(found, [vec![], vec![]]);
        assert_eq!(calls.get(), 2);
        assert_eq!(clock.elapsed(), AGAIN);

        // What stays through the settling time counts, by the last listing, in its own folder.
        let calls = std::cell::Cell::new(0);
        let mut clock = Paused::default();
        let settle = Duration::from_millis(1500);
        let stays = [Ok(both), Ok(other_only)];
        let found = settled(&mut listings(&stays, &calls), &trees, 1, settle, &mut clock).unwrap();
        assert_eq!(found, [vec![], vec![cargo()]]);
        assert_eq!(calls.get(), 3, "at 0, 1 and 1.5 s");
        assert_eq!(clock.elapsed(), settle);

        // With no time to settle, the first listing is what counts.
        let calls = std::cell::Cell::new(0);
        let found = settled(
            &mut listings(&stays, &calls),
            &trees,
            1,
            Duration::ZERO,
            &mut Paused::default(),
        )
        .unwrap();
        assert_eq!(found[0].len(), 1);
        assert_eq!(calls.get(), 1);

        // A listing that fails while it settles is why, never what an earlier one found.
        let calls = std::cell::Cell::new(0);
        let failing = [Ok(both), Err("lsof did not end within 60 s")];
        let failed = settled(
            &mut listings(&failing, &calls),
            &trees,
            1,
            SETTLE,
            &mut Paused::default(),
        );
        assert_eq!(failed.unwrap_err(), "lsof did not end within 60 s");
    }

    #[test]
    fn the_real_listing_sees_a_process_working_in_the_tree_and_not_this_one() {
        let tree = Tree::new();
        let dir = tree.root.canonicalize().unwrap();
        let me = std::process::id();
        let mut real = || listing(&mut command("lsof"), far());
        let found = settled(&mut real, &[&dir], me, SETTLE, &mut Paused::default());
        assert_eq!(found.unwrap(), [vec![]]);
        let mut sleeper = command("sleep")
            .arg("1000")
            .current_dir(&dir)
            .spawn()
            .unwrap();
        let listed = real();
        sleeper.kill().unwrap();
        sleeper.wait().unwrap();
        assert_eq!(
            inside_of(&listed.unwrap(), &dir, me),
            [Inside {
                pid: sleeper.id(),
                command: "sleep".into(),
                what: "working there".into()
            }]
        );
    }

    #[test]
    fn a_command_within_its_time_answers_what_it_printed() {
        let done = output_until(
            command("sh").args(["-c", "echo out; echo err >&2; exit 3"]),
            far(),
        )
        .unwrap();
        assert_eq!(done.status.code(), Some(3));
        assert_eq!(done.stdout, b"out\n");
        assert_eq!(done.stderr, b"err\n");
        let missing = output_until(&mut command("/no/such/program"), far());
        assert!(matches!(missing, Err(Failed::Io(_))), "{missing:?}");
    }

    /// More than a pipe holds either way: what it is fed is read while what it prints is, and one
    /// that ends without reading what it is fed is answered by its own status.
    #[test]
    fn a_fed_command_reads_its_input_while_it_answers_and_may_end_without_it() {
        let input: Vec<u8> = (0..200_000).map(|at| b'a' + (at % 26) as u8).collect();
        let done = fed_until(&mut command("cat"), input.clone(), far()).unwrap();
        assert!(done.status.success());
        assert_eq!(done.stdout, input);
        let done = fed_until(
            command("sh").args(["-c", "echo early; exit 4"]),
            input,
            far(),
        )
        .unwrap();
        assert_eq!(done.status.code(), Some(4));
        assert_eq!(done.stdout, b"early\n");
        let missing = fed_until(&mut command("/no/such/program"), Vec::new(), far());
        assert!(matches!(missing, Err(Failed::Io(_))), "{missing:?}");
    }

    /// A shell that starts a helper which outlives any wait of a test, writes its pid in `pid` and
    /// waits for it.
    fn with_helper(pid: &Path) -> String {
        format!("sleep 1000 & echo $! > '{}'; wait", pid.display())
    }

    #[test]
    fn a_command_past_its_time_is_stopped_with_what_it_started() {
        let tree = Tree::new();
        let pid = tree.path("helper.pid");
        // Its time is up before it starts. The run is never told to stop, but asked whether it was,
        // it answers once the helper runs, so the time stops a command whose helper runs.
        let heard = || {
            written_pid(&pid);
            false
        };
        let late = output_until(
            command("sh").args(["-c", &with_helper(&pid)]),
            Bound::hearing(Instant::now(), &heard),
        );
        assert!(matches!(late, Err(Failed::Late(_))), "{late:?}");
        gone(written_pid(&pid), "the helper of the command's group");
    }

    #[test]
    fn a_command_is_stopped_with_its_group_once_the_run_is_told_to_stop() {
        let tree = Tree::new();
        let pid = tree.path("helper.pid");
        // The run is told to stop once the helper runs.
        let heard = || pid_written(&pid);
        let stopped = output_until(
            command("sh").args(["-c", &with_helper(&pid)]),
            Bound::hearing(far().until, &heard),
        );
        assert!(
            matches!(stopped, Err(Failed::Interrupted(_))),
            "{stopped:?}"
        );
        gone(written_pid(&pid), "the helper of the command's group");
        let quiet = || false;
        let bound = Bound::hearing(Instant::now() + Duration::from_secs(60), &quiet);
        assert!(!bound.interrupted());
        assert!(!Bound::until(Instant::now()).interrupted());
        let soon = Instant::now() + Duration::from_secs(1);
        assert!(Bound::until(soon).sooner(Duration::from_secs(60)).until == soon);
        assert!(Bound::until(soon).later(Duration::from_secs(60)).until > soon);
    }

    #[test]
    fn a_group_lives_while_any_of_it_runs_and_not_as_a_later_group_of_its_id() {
        let tree = Tree::new();
        let left = tree.path("left.pid");
        let script = format!("sleep 1000 & echo $! > '{}'; sleep 1", left.display());
        let mut leader = command("sh")
            .args(["-c", &script])
            .process_group(0)
            .spawn()
            .unwrap();
        let group = i64::from(leader.id());
        let start = stamp(group);
        assert!(start.is_some(), "a running process's start is read");
        assert!(lives(group, start.as_ref()));
        assert!(
            lives(group, None),
            "a group whose start is unknown counts as it"
        );
        let other = match start {
            Some(Stamp::At(at)) => Stamp::At(at - 1.0),
            _ => Stamp::At(0.0),
        };
        assert!(
            !lives(group, Some(&other)),
            "a leader that started at another time leads another group"
        );
        // Its leader gone, what it left in the group is the group still.
        leader.wait().unwrap();
        assert!(lives(group, start.as_ref()));
        signal(libc::pid_t::try_from(group).unwrap(), libc::SIGKILL);
        let helper = written_pid(&left);
        test_time::until_true(
            format!("the group {group} gone with its helper {helper}"),
            test_time::FLOOR,
            || !lives(group, start.as_ref()),
        )
        .unwrap_or_else(|stall| panic!("{stall}"));
        for nothing in [0, 1, -5, i64::MAX] {
            assert!(!lives(nothing, None), "{nothing}");
        }
    }

    /// Our child `pid`'s exit, waited for and left unreaped: a zombie, its id still its group's.
    fn exited(pid: u32) -> libc::c_int {
        // SAFETY: siginfo_t is plain output data for which all zeroes is valid.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        retry_interrupted(|| {
            // SAFETY: waits for our unreaped child alone, writing one siginfo_t and reaping none.
            let waited = unsafe {
                libc::waitid(
                    libc::P_PID,
                    libc::id_t::from(pid),
                    &raw mut info,
                    libc::WEXITED | libc::WNOWAIT,
                )
            };
            if waited == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        })
        .unwrap();
        info.si_code
    }

    #[test]
    fn a_group_whose_every_member_is_exiting_or_a_zombie_does_not_live() {
        let mut child = Owned(
            command("sleep")
                .arg("1000")
                .process_group(0)
                .spawn()
                .unwrap(),
        );
        let pid = child.0.id();
        let group = i64::from(pid);
        let start = stamp(group);
        assert!(start.is_some(), "a running process's start is read");
        assert!(lives(group, start.as_ref()));
        signal(group_of(pid), libc::SIGKILL);
        assert_eq!(exited(pid), libc::CLD_KILLED);
        // Darwin's killpg reaches no member of it, Linux's still reaches its zombie: either way
        // nothing of it runs, whatever start was recorded for it.
        assert!(
            !lives(group, start.as_ref()),
            "its killed leader, left unreaped, runs nothing"
        );
        assert!(!lives(group, None), "nor when its start is unknown");
        child.0.wait().unwrap();
        assert!(!lives(group, start.as_ref()), "nor once it is reaped");
    }

    #[test]
    fn a_stopped_group_ends_on_term_and_its_leader_is_left_to_be_reaped() {
        let mut child = command("sh")
            .args(["-c", "sleep 1000"])
            .process_group(0)
            .spawn()
            .unwrap();
        assert!(!ended(child.id()));
        // Its grace is a wait for its leader to end, which only a stall outlasts.
        stop(child.id(), test_time::backstop(GRACE)).unwrap();
        assert!(ended(child.id()));
        let status = child.wait().unwrap();
        assert_eq!(
            std::os::unix::process::ExitStatusExt::signal(&status),
            Some(libc::SIGTERM)
        );
        assert!(ended(child.id()), "a reaped child has ended too");
    }

    #[test]
    fn a_group_that_ignores_term_is_killed_after_the_grace() {
        let tree = Tree::new();
        let ignoring = tree.path("ignoring");
        let script = format!(
            "trap '' TERM; : > '{}'; while :; do sleep 1; done",
            ignoring.display()
        );
        let mut child = command("sh")
            .args(["-c", &script])
            .process_group(0)
            .spawn()
            .unwrap();
        test_time::until_true("the group ignoring TERM", test_time::FLOOR, || {
            ignoring.exists()
        })
        .unwrap_or_else(|stall| panic!("{stall}"));
        stop(child.id(), Duration::from_millis(300)).unwrap();
        let status = child.wait().unwrap();
        assert_eq!(
            std::os::unix::process::ExitStatusExt::signal(&status),
            Some(libc::SIGKILL)
        );
    }

    #[test]
    fn the_holders_of_a_file_are_the_processes_that_hold_it_open() {
        let tree = Tree::new();
        let file = tree.write("index.lock", "");
        assert_eq!(holders_within(&file, far()), Ok(Vec::new()));
        let mut holder = command("sleep")
            .arg("1000")
            .stdin(std::fs::File::open(&file).unwrap())
            .spawn()
            .unwrap();
        let held = holders_within(&file, far()).unwrap();
        assert_eq!(held, [holder.id()]);
        holder.kill().unwrap();
        holder.wait().unwrap();
        assert_eq!(holders_within(&tree.path("none"), far()), Ok(Vec::new()));
    }
}
