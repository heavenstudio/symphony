//! The release run's children, each in a process group of its own and bounded ([`Bound`]): past the
//! run's time, or once the run is told to stop, the whole group is stopped, so no helper it started
//! (a remote's transport, a test's server) runs on. A group is told from a later one given the same
//! id by its leader's start ([`Stamp`], [`lives`]).

use std::io;
use std::path::Path;
use std::process::Output;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use symphony_board::sys::{Boottime, Command, Start, process_starts, spawn_output, wait_output};

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

/// Whether the process group `group`, whose leader started at `start`, still has a process: its
/// leader, or what that left running when it ended. A group of that id whose living leader started
/// at another time is another group; one whose start cannot be told counts as that group.
pub(crate) fn lives(group: i64, start: Option<&Stamp>) -> bool {
    let Ok(id) = libc::pid_t::try_from(group) else {
        return false;
    };
    if id <= 1 {
        return false;
    }
    // SAFETY: signal 0 sends nothing: killpg only says whether the group has a process.
    let asked = unsafe { libc::killpg(id, 0) };
    if asked != 0 && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
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
    // SAFETY: siginfo_t is plain data, for which all zeroes is a value.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // SAFETY: reads whether one child of this process ended into `info`, waiting for and reaping
    // none.
    let waited = unsafe {
        libc::waitid(
            libc::P_PID,
            libc::id_t::from(pid),
            &raw mut info,
            libc::WEXITED | libc::WNOWAIT | libc::WNOHANG,
        )
    };
    waited != 0 || info.si_signo == libc::SIGCHLD
}

/// Stop the group the child `pid` leads: SIGTERM, up to [`GRACE`] for its leader to end, then
/// SIGKILL to whatever of it is left. The leader is left unreaped, for its owner to wait for.
pub(crate) fn stop(pid: u32, grace: Duration) {
    let group = group_of(pid);
    signal(group, libc::SIGTERM);
    let until = Instant::now() + grace;
    while !ended(pid) && Instant::now() < until {
        std::thread::sleep(POLL);
    }
    signal(group, libc::SIGKILL);
}

/// `command` run in a process group of its own, stdin null and its output read, within `bound`: a
/// command still running when it ends has its whole group killed.
pub(crate) fn output_until(command: &mut Command, bound: Bound<'_>) -> Result<Output, Failed> {
    let started = Instant::now();
    command.process_group(0);
    let child = spawn_output(command).map_err(Failed::Io)?;
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

/// The processes that hold `file` open, by `lsof`: none when no process does.
pub(crate) fn holders(file: &Path) -> Result<Vec<u32>, String> {
    let done = output_until(
        crate::children::command("lsof")
            .args(["-w", "-t", "--"])
            .arg(file),
        Bound::until(Instant::now() + Duration::from_secs(60)),
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
/// group still runs, or the board's recorder writing down how the check ended, leaves within it.
pub(crate) const SETTLE: Duration = Duration::from_secs(5);
/// How long a look waits before it lists again what it found.
const AGAIN: Duration = Duration::from_secs(1);

/// The processes but this one that `lsof` sees working in each of `trees` (real paths) or holding
/// a file inside it open, within `bound`, a list for each: as listed once none is found, or
/// [`SETTLE`] after the first listing found one, so a process on its way out is not counted. Each
/// listing is one of every process, read by path: a search of a tree (`lsof +D`) reads every file
/// of its build folder first.
pub(crate) fn inside(trees: &[&Path], bound: Bound<'_>) -> Result<Vec<Vec<Inside>>, String> {
    let mut list = || listing(&mut crate::children::command("lsof"), bound);
    settled(&mut list, trees, std::process::id(), SETTLE)
}

/// A listing of every process's working directory and open files by `lsof`, a command for lsof to
/// which this adds the arguments, within `bound`: never an empty one.
fn listing(lsof: &mut Command, bound: Bound<'_>) -> Result<String, String> {
    let done = output_until(
        lsof.args(["-n", "-P", "-w", "-F", "pcfn"]),
        bound.sooner(Duration::from_secs(60)),
    )
    .map_err(|failed| match failed {
        Failed::Io(e) => format!("cannot run lsof: {e}"),
        Failed::Late(took) => format!("lsof did not end within {} s", took.as_secs()),
        Failed::Interrupted(took) => format!(
            "lsof was stopped after {} s: the run was told to stop",
            took.as_secs()
        ),
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
/// every [`AGAIN`] while something is found, until nothing is or `settle` has passed since the
/// first, and the last listing's.
fn settled(
    list: &mut dyn FnMut() -> Result<String, String>,
    trees: &[&Path],
    me: u32,
    settle: Duration,
) -> Result<Vec<Vec<Inside>>, String> {
    let first = Instant::now();
    loop {
        let listing = list()?;
        let found: Vec<Vec<Inside>> = (trees.iter())
            .map(|tree| inside_of(&listing, tree, me))
            .collect();
        let left = settle.saturating_sub(first.elapsed());
        if found.iter().all(Vec::is_empty) || left.is_zero() {
            return Ok(found);
        }
        std::thread::sleep(AGAIN.min(left));
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
    use super::*;
    use crate::children::command;
    use crate::test_tree::Tree;

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
                       p11\ncchrome_crashpad_handler\nfcwd\nn/wt/release/apps/symphony-ui\n\
                       p12\ncvim\nf7r\nn/wt/release/apps/symphony-ui/src/a.rs\n\
                       p13\ncsh\nfcwd\nn/wt/release/working in/x\n";
        let found = inside_of(listing, Path::new("/wt/release"), 1);
        let folders: Vec<Option<&str>> = found.iter().map(Inside::working_in).collect();
        assert_eq!(
            folders,
            [
                Some(""),
                Some("apps/symphony-ui"),
                None,
                Some("working in/x")
            ]
        );
    }

    #[test]
    fn a_listing_that_cannot_be_had_is_why_and_never_an_empty_one() {
        let soon = || Bound::until(Instant::now() + Duration::from_secs(30));
        let refused = listing(&mut command("/no/such/lsof"), soon()).unwrap_err();
        assert!(refused.starts_with("cannot run lsof: "), "{refused}");
        let mut silent = command("sh");
        silent.args(["-c", "echo 'lsof: no listing' >&2; exit 1", "lsof"]);
        assert_eq!(
            listing(&mut silent, soon()).unwrap_err(),
            "lsof listed no process: lsof: no listing"
        );
        let mut slow = command("sh");
        slow.args(["-c", "sleep 30", "lsof"]);
        let late = Bound::until(Instant::now() + Duration::from_millis(300));
        assert_eq!(
            listing(&mut slow, late).unwrap_err(),
            "lsof did not end within 0 s"
        );
        let mut told = command("sh");
        told.args(["-c", "sleep 30", "lsof"]);
        let heard = || true;
        let stopping = Bound::hearing(Instant::now() + Duration::from_secs(30), &heard);
        assert_eq!(
            listing(&mut told, stopping).unwrap_err(),
            "lsof was stopped after 0 s: the run was told to stop"
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
        let found = settled(&mut listings(&[Ok(nothing)], &calls), &trees, 1, SETTLE).unwrap();
        assert_eq!(found, [vec![], vec![]]);
        assert_eq!(calls.get(), 1);

        // Found, then gone at the next listing, a second later: nothing counts.
        let calls = std::cell::Cell::new(0);
        let started = Instant::now();
        let gone = [Ok(both), Ok(nothing)];
        let found = settled(&mut listings(&gone, &calls), &trees, 1, SETTLE).unwrap();
        assert_eq!(found, [vec![], vec![]]);
        assert_eq!(calls.get(), 2);
        assert!(started.elapsed() >= AGAIN, "{:?}", started.elapsed());

        // What stays through the settling time counts, by the last listing, in its own folder.
        let calls = std::cell::Cell::new(0);
        let settle = Duration::from_millis(1500);
        let stays = [Ok(both), Ok(other_only)];
        let found = settled(&mut listings(&stays, &calls), &trees, 1, settle).unwrap();
        assert_eq!(found, [vec![], vec![cargo()]]);
        assert_eq!(calls.get(), 3, "at 0, 1 and 1.5 s");

        // With no time to settle, the first listing is what counts.
        let calls = std::cell::Cell::new(0);
        let found = settled(&mut listings(&stays, &calls), &trees, 1, Duration::ZERO).unwrap();
        assert_eq!(found[0].len(), 1);
        assert_eq!(calls.get(), 1);

        // A listing that fails while it settles is why, never what an earlier one found.
        let calls = std::cell::Cell::new(0);
        let failing = [Ok(both), Err("lsof did not end within 60 s")];
        assert_eq!(
            settled(&mut listings(&failing, &calls), &trees, 1, SETTLE).unwrap_err(),
            "lsof did not end within 60 s"
        );
    }

    #[test]
    fn the_real_listing_sees_a_process_working_in_the_tree_and_not_this_one() {
        let tree = Tree::new();
        let dir = tree.root.canonicalize().unwrap();
        let soon = || Bound::until(Instant::now() + Duration::from_secs(60));
        assert_eq!(inside(&[&dir], soon()).unwrap(), [vec![]]);
        let mut sleeper = command("sleep")
            .arg("60")
            .current_dir(&dir)
            .spawn()
            .unwrap();
        let listed = listing(&mut command("lsof"), soon());
        sleeper.kill().unwrap();
        sleeper.wait().unwrap();
        assert_eq!(
            inside_of(&listed.unwrap(), &dir, std::process::id()),
            [Inside {
                pid: sleeper.id(),
                command: "sleep".into(),
                what: "working there".into()
            }]
        );
    }

    fn alive(pid: u32) -> bool {
        symphony_board::sys::pid_alive(i64::from(pid))
    }

    #[test]
    fn a_command_within_its_time_answers_what_it_printed() {
        let done = output_until(
            command("sh").args(["-c", "echo out; echo err >&2; exit 3"]),
            Bound::until(Instant::now() + Duration::from_secs(30)),
        )
        .unwrap();
        assert_eq!(done.status.code(), Some(3));
        assert_eq!(done.stdout, b"out\n");
        assert_eq!(done.stderr, b"err\n");
        let missing = output_until(
            &mut command("/no/such/program"),
            Bound::until(Instant::now() + Duration::from_secs(30)),
        );
        assert!(matches!(missing, Err(Failed::Io(_))), "{missing:?}");
    }

    #[test]
    fn a_command_past_its_time_is_stopped_with_what_it_started() {
        let tree = Tree::new();
        let pid = tree.path("helper.pid");
        let script = format!("sleep 30 & echo $! > '{}'; wait", pid.display());
        let started = Instant::now();
        let late = output_until(
            command("sh").args(["-c", &script]),
            Bound::until(Instant::now() + Duration::from_millis(500)),
        );
        assert!(matches!(late, Err(Failed::Late(_))), "{late:?}");
        assert!(started.elapsed() < Duration::from_secs(20));
        let helper: u32 = std::fs::read_to_string(&pid)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        while alive(helper) && Instant::now() < until {
            std::thread::sleep(POLL);
        }
        assert!(!alive(helper), "the helper went with the command's group");
    }

    #[test]
    fn a_command_is_stopped_with_its_group_once_the_run_is_told_to_stop() {
        let started = Instant::now();
        let heard = move || started.elapsed() >= Duration::from_millis(300);
        let stopped = output_until(
            command("sh").args(["-c", "sleep 30"]),
            Bound::hearing(Instant::now() + Duration::from_secs(60), &heard),
        );
        assert!(
            matches!(stopped, Err(Failed::Interrupted(_))),
            "{stopped:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(20));
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
        let script = format!("sleep 30 & echo $! > '{}'; sleep 1", left.display());
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
        let helper: i64 = std::fs::read_to_string(&left)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        while lives(group, start.as_ref()) && Instant::now() < until {
            std::thread::sleep(POLL);
        }
        assert!(
            !lives(group, start.as_ref()),
            "helper {helper} went with it"
        );
        for nothing in [0, 1, -5, i64::MAX] {
            assert!(!lives(nothing, None), "{nothing}");
        }
    }

    #[test]
    fn a_stopped_group_ends_on_term_and_its_leader_is_left_to_be_reaped() {
        let mut child = command("sh")
            .args(["-c", "sleep 30"])
            .process_group(0)
            .spawn()
            .unwrap();
        assert!(!ended(child.id()));
        stop(child.id(), GRACE);
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
        let mut child = command("sh")
            .args(["-c", "trap '' TERM; while :; do sleep 1; done"])
            .process_group(0)
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_millis(200));
        stop(child.id(), Duration::from_millis(300));
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
        assert_eq!(holders(&file), Ok(Vec::new()));
        let mut holder = command("sleep")
            .arg("30")
            .stdin(std::fs::File::open(&file).unwrap())
            .spawn()
            .unwrap();
        let held = holders(&file).unwrap();
        assert_eq!(held, [holder.id()]);
        holder.kill().unwrap();
        holder.wait().unwrap();
        assert_eq!(holders(&tree.path("none")), Ok(Vec::new()));
    }
}
