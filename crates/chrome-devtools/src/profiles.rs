//! Where a launch's profile lives: in a folder of this process's own under `/tmp`, which also holds
//! Chrome's own temporary folders, since the profile is Chrome's temporary folder too. The leash
//! kills Chrome, so Chrome removes none of them itself; they go with the profile, the process's
//! folder goes when the binary exits, and a killed run's goes at the next run.

#[path = "harness_scratch.rs"]
mod harness_scratch;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

/// Where [`root`] is. Chrome makes its singleton socket's folder in its temporary folder, here the
/// profile, and a socket's path must fit 104 bytes on macOS: the user temp folder's own 49 leave too
/// little room for the profile's path and the socket's 41 below it.
const BASE: &str = "/tmp";

/// The folder in [`BASE`] that holds every run's profiles, and only those: one per user.
fn root_name() -> String {
    // SAFETY: geteuid takes no arguments and cannot fail.
    let uid = unsafe { libc::geteuid() };
    format!("symphony-chrome-{uid}")
}

#[cfg(test)]
fn root() -> PathBuf {
    Path::new(BASE).join(root_name())
}

static OWN: Mutex<Option<PathBuf>> = Mutex::new(None);

/// This process's folder in [`root`], `<pid>-<n>`: made on first use, once the folders of runs whose
/// process is gone are swept (a killed run cannot remove its own), and removed with everything in it
/// when the binary exits.
fn own() -> io::Result<PathBuf> {
    let mut own = OWN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(own) = own.as_ref() {
        return Ok(own.clone());
    }
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = harness_scratch::harness_root(Path::new(BASE), &root_name())
        .map_err(|error| io::Error::new(error.kind(), format!("the profiles' folder: {error}")))?;
    harness_scratch::reap_dead(&root);
    let made = harness_scratch::create(&root, &NEXT).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("a profiles folder in {}: {error}", root.display()),
        )
    })?;
    // SAFETY: atexit only records a function that takes nothing, to run once at the exit.
    if unsafe { libc::atexit(remove_own) } != 0 {
        let _ = std::fs::remove_dir(&made);
        return Err(io::Error::other("the profiles' end could not be set"));
    }
    *own = Some(made.clone());
    Ok(made)
}

/// At the binary's exit: this process's profiles go.
extern "C" fn remove_own() {
    let own = OWN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(own) = own.as_ref() {
        let _ = std::fs::remove_dir_all(own);
    }
}

/// Remove the folders of runs whose process is gone, as a run's first launch does.
#[cfg(test)]
pub(crate) fn sweep() {
    harness_scratch::reap_dead(&root());
}

/// A path no launch has used, in this process's folder: a profile, with its logs beside it, or a
/// stand-in browser.
pub(crate) fn next_profile() -> io::Result<PathBuf> {
    static LAUNCHED: AtomicU32 = AtomicU32::new(0);
    Ok(own()?.join(LAUNCHED.fetch_add(1, Ordering::SeqCst).to_string()))
}

#[cfg(test)]
mod tests {
    use super::{next_profile, own, root};
    use std::io::{BufRead, BufReader};
    use std::os::unix::process::ExitStatusExt;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    /// Set on a run of this binary that plays [`a_run_of_this_binary`]: `made`, or `held`.
    const ROLE: &str = "SYMPHONY_PROFILES_ROLE";
    /// What a run prints, with its folder, once its profile is made.
    const MADE: &str = "profile made in ";

    /// A run of this binary that makes a profile and ends, or, `held`, then waits for its input's end.
    /// Run with no role, it has nothing to do.
    #[test]
    fn a_run_of_this_binary() {
        let Ok(role) = std::env::var(ROLE) else {
            return;
        };
        let profile = next_profile().unwrap();
        std::fs::create_dir(&profile).unwrap();
        std::fs::write(profile.join("file"), "a run's profile").unwrap();
        println!("{MADE}{}", own().unwrap().display());
        if role == "held" {
            // The input ends once the test that started this run has reaped it, or with that test.
            let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
        }
    }

    /// This binary as `role`, its stdout piped, read up to the folder it made: the run, and that folder.
    fn run(
        role: &str,
    ) -> (
        std::process::Child,
        BufReader<std::process::ChildStdout>,
        PathBuf,
    ) {
        let mut command: Command =
            crate::tests::suite_child("profiles::tests::a_run_of_this_binary");
        let mut child = command
            .env(ROLE, role)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut out = BufReader::new(child.stdout.take().unwrap());
        match folder_made(&mut out) {
            Ok(own) => (child, out, own),
            Err(said) => {
                let status = child.wait().unwrap();
                panic!("the {role} run ended, {status}, before its profile was made: {said}");
            }
        }
    }

    /// The folder a run says it made its profile in, or, if its output ends first, all it said.
    fn folder_made(out: &mut impl BufRead) -> Result<PathBuf, String> {
        let mut said = String::new();
        loop {
            if let Some(own) = said.lines().find_map(|line| line.strip_prefix(MADE)) {
                return Ok(PathBuf::from(own));
            }
            if out.read_line(&mut said).unwrap() == 0 {
                return Err(said);
            }
        }
    }

    #[test]
    fn a_profile_is_in_this_processs_own_folder_in_the_root() {
        let profile = next_profile().unwrap();
        let own = profile.parent().unwrap();
        assert_eq!(own.parent(), Some(root().as_path()));
        let name = own.file_name().unwrap().to_str().unwrap();
        let (pid, n) = name.split_once('-').unwrap();
        assert_eq!(pid, std::process::id().to_string());
        assert!(n.parse::<usize>().is_ok(), "{name}");
    }

    #[test]
    fn a_runs_profiles_go_at_its_exit_and_a_killed_runs_at_the_next_run() {
        let (mut held, _out, killed) = run("held");
        // Kept until the held run is reaped: `wait` closes a child's input before it waits, and on
        // macOS a killed process's thread blocked on that input can wake to its end and remove the
        // profiles at the exit before the kill has ended the process.
        let input = held.stdin.take();
        held.kill().unwrap();
        assert_eq!(held.wait().unwrap().signal(), Some(libc::SIGKILL));
        drop(input);
        assert!(
            killed.join("0/file").is_file(),
            "the kill left no profile, proving nothing"
        );
        let (mut next, mut out, made) = run("made");
        let mut rest = String::new();
        std::io::Read::read_to_string(&mut out, &mut rest).unwrap();
        assert!(next.wait().unwrap().success(), "{rest}");
        assert!(rest.contains("1 passed"), "{rest}");
        assert!(!made.exists(), "the run's profiles outlived its exit");
        assert!(
            !killed.exists(),
            "the killed run's profiles outlived the next run"
        );
    }

    #[test]
    fn chromes_temporary_folders_are_in_its_profile_and_go_with_it() {
        let Some(binary) = crate::find() else {
            eprintln!("no Chrome: real browser capability unavailable");
            return;
        };
        let chrome = crate::Chrome::launch(&binary, &crate::tests::leash()).unwrap();
        let profile = chrome.owner.profile.clone();
        // Chrome's singleton socket, in a folder Chrome made in its temporary folder at startup.
        let socket = std::fs::read_link(profile.join("SingletonSocket")).unwrap();
        assert!(
            socket.starts_with(&profile),
            "{} is not in the profile {}",
            socket.display(),
            profile.display()
        );
        drop(chrome);
        assert!(!profile.exists(), "the profile outlived its browser");
        assert!(
            !socket.exists(),
            "Chrome's singleton folder outlived its browser"
        );
    }

    /// Set on the run of [`nothing_chrome_starts_works_in_its_launchers_folder_or_outlives_it`]
    /// that launches Chrome.
    const LAUNCHES: &str = "SYMPHONY_CHROME_LEFTOVERS_RUN";
    /// What that run prints once Chrome has drawn a page: its profile, and its leash's id.
    const PROFILE_IS: &str = "chrome's profile: ";
    const LEASH_IS: &str = "chrome's leash: ";

    /// A process lsof lists with its working folder.
    #[derive(Clone, Debug, PartialEq)]
    struct Working {
        pid: u32,
        command: String,
        folder: PathBuf,
    }

    /// Every process lsof lists, each with its working folder.
    fn working() -> Vec<Working> {
        let listed = Command::new("lsof")
            .args(["-n", "-P", "-w", "-F", "pcn", "-d", "cwd"])
            .output()
            .unwrap();
        let (mut pid, mut command) = (0, String::new());
        let mut found = Vec::new();
        for line in String::from_utf8_lossy(&listed.stdout).lines() {
            match line.split_at_checked(1) {
                Some(("p", value)) => pid = value.parse().unwrap_or(0),
                Some(("c", value)) => value.clone_into(&mut command),
                Some(("n", value)) if pid > 0 => found.push(Working {
                    pid,
                    command: command.clone(),
                    folder: PathBuf::from(value),
                }),
                _ => {}
            }
        }
        assert!(
            found.iter().any(|seen| seen.pid == std::process::id()),
            "lsof did not list this process: {}",
            String::from_utf8_lossy(&listed.stderr)
        );
        found
    }

    /// The processes of `all` working in `folder` (a real path) or below it.
    fn inside(all: &[Working], folder: &std::path::Path) -> Vec<Working> {
        (all.iter())
            .filter(|seen| seen.folder.starts_with(folder))
            .cloned()
            .collect()
    }

    /// Chrome works in its private profile, never in the folder its launcher works in, and once the
    /// launcher's run has ended nothing Chrome started is left: none of the processes that worked in
    /// its profile while it ran, its crash handler among them (it leaves the browser's group, so the
    /// leash's kill never reaches it, and ends by itself once the browser has), and nothing in
    /// either folder. A run of this binary is the launcher, working in a folder of its own.
    #[test]
    fn nothing_chrome_starts_works_in_its_launchers_folder_or_outlives_it() {
        const TEST: &str =
            "profiles::tests::nothing_chrome_starts_works_in_its_launchers_folder_or_outlives_it";
        let Some(binary) = crate::find() else {
            eprintln!("no Chrome: real browser capability unavailable");
            return;
        };
        if std::env::var_os(LAUNCHES).is_some() {
            let mut chrome = crate::Chrome::launch(&binary, &crate::tests::leash()).unwrap();
            chrome
                .open("data:text/html,<title>leftovers</title><p>a page drawn")
                .unwrap();
            println!("{PROFILE_IS}{}", chrome.profile().display());
            println!("{LEASH_IS}{}", chrome.leash_id());
            // The input ends once the test that started this run has looked at it.
            let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
            return;
        }
        let folder = next_profile().unwrap();
        std::fs::create_dir(&folder).unwrap();
        let folder = folder.canonicalize().unwrap();
        let mut launcher = crate::tests::suite_child(TEST)
            .env(LAUNCHES, "1")
            .current_dir(&folder)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut out = BufReader::new(launcher.stdout.take().unwrap());
        let mut said = String::new();
        let (profile, leash) = loop {
            let profile = said.lines().find_map(|line| line.strip_prefix(PROFILE_IS));
            let leash = said.lines().find_map(|line| line.strip_prefix(LEASH_IS));
            if let (Some(profile), Some(leash)) = (profile, leash) {
                break (PathBuf::from(profile), leash.parse::<u32>().unwrap());
            }
            if out.read_line(&mut said).unwrap() == 0 {
                let status = launcher.wait().unwrap();
                panic!("the launcher ended, {status}, before its Chrome drew a page: {said}");
            }
        };
        let profile = profile.canonicalize().unwrap();
        let running = working();
        let in_folder: Vec<u32> = inside(&running, &folder).iter().map(|s| s.pid).collect();
        let mut expected = vec![launcher.id(), leash];
        expected.sort_unstable();
        let mut seen = in_folder.clone();
        seen.sort_unstable();
        assert_eq!(
            seen,
            expected,
            "only the launcher and its leash work in the launcher's folder: {:?}",
            inside(&running, &folder)
        );
        let chromes = inside(&running, &profile);
        assert!(
            !chromes.is_empty(),
            "Chrome does not work in its profile {}",
            profile.display()
        );
        eprintln!("working in the profile while Chrome ran: {chromes:?}");
        drop(launcher.stdin.take());
        let mut rest = String::new();
        std::io::Read::read_to_string(&mut out, &mut rest).unwrap();
        let status = launcher.wait().unwrap();
        assert!(status.success(), "{status}: {said}{rest}");
        assert!(rest.contains("1 passed"), "{rest}");
        // A crash handler ends once its clients have: a moment after the leash's kill, never by it.
        let ended = std::time::Instant::now();
        let left = loop {
            let now = working();
            let left: Vec<Working> = (now.iter())
                .filter(|seen| {
                    chromes
                        .iter()
                        .any(|chrome| chrome.pid == seen.pid && chrome.command == seen.command)
                        || seen.folder.starts_with(&folder)
                        || seen.folder.starts_with(&profile)
                })
                .cloned()
                .collect();
            if left.is_empty() || ended.elapsed() > std::time::Duration::from_secs(60) {
                break left;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        };
        assert!(
            left.is_empty(),
            "left {:?} after the launcher's run ended: {left:?}",
            ended.elapsed()
        );
        eprintln!(
            "nothing Chrome started was left {:?} after the run ended",
            ended.elapsed()
        );
    }
}
