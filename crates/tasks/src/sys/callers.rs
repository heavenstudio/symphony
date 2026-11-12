//! The processes this one runs under, each by the name the kernel gives it and its parent
//! ([`callers`]), and the folder a process works in ([`folder_of`]): libproc on macOS, `/proc` on
//! Linux. Neither reads another process's environment, which macOS keeps from its readers.

use std::path::PathBuf;

/// At most this many callers are read: more than any chain a shell, a build and a test make.
const MOST: usize = 64;

/// A process this one runs under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caller {
    pub pid: u32,
    /// The kernel's name for it: its program's file name, cut to the kernel's length (15 or 16
    /// bytes).
    pub name: String,
    /// The process it runs under.
    pub parent: u32,
}

/// The processes this one runs under, nearest first: its parent, that one's, and so on up to and not
/// including the first process (pid 1), ending at one the kernel cannot say (or one already named).
pub fn callers() -> Vec<Caller> {
    let mut chain: Vec<Caller> = Vec::new();
    let Some((mut at, _)) = process(std::process::id()) else {
        return chain;
    };
    while at > 1 && chain.len() < MOST && !chain.iter().any(|caller| caller.pid == at) {
        let Some((parent, name)) = process(at) else {
            break;
        };
        chain.push(Caller {
            pid: at,
            name,
            parent,
        });
        at = parent;
    }
    chain
}

/// The parent and name of `pid`, as libproc's `proc_bsdinfo` gives them: none for a pid that names
/// no process (or a reused one) or when libproc cannot be read.
#[cfg(target_os = "macos")]
fn process(pid: u32) -> Option<(u32, String)> {
    let wanted = std::ffi::c_int::try_from(pid).ok()?;
    let size = std::ffi::c_int::try_from(size_of::<libc::proc_bsdinfo>()).ok()?;
    // SAFETY: proc_bsdinfo holds integers and byte arrays alone, for which all zeros is a value.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    // SAFETY: libproc receives a writable proc_bsdinfo buffer of exactly the size it is told, and
    // keeps no pointer past the call; a short read is checked before any field is.
    let read = unsafe {
        libc::proc_pidinfo(
            wanted,
            libc::PROC_PIDTBSDINFO,
            0,
            (&raw mut info).cast(),
            size,
        )
    };
    (read == size && info.pbi_pid == pid).then(|| {
        let name = info.pbi_comm.map(|byte| byte.to_ne_bytes()[0]);
        (info.pbi_ppid, ended(&name))
    })
}

/// The parent and name of `pid`, as `/proc/PID/stat` gives them: none for a pid that names no
/// process or a file that cannot be read.
#[cfg(target_os = "linux")]
fn process(pid: u32) -> Option<(u32, String)> {
    stat_process(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

/// No process can be read on a system with no reader.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn process(_pid: u32) -> Option<(u32, String)> {
    None
}

/// The folder `pid` works in, as libproc's `proc_vnodepathinfo` gives it: none when it cannot be
/// read.
#[cfg(target_os = "macos")]
pub fn folder_of(pid: u32) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let wanted = std::ffi::c_int::try_from(pid).ok()?;
    let size = std::ffi::c_int::try_from(size_of::<libc::proc_vnodepathinfo>()).ok()?;
    // SAFETY: proc_vnodepathinfo holds integers and byte arrays alone, for which all zeros is a
    // value.
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    // SAFETY: libproc receives a writable proc_vnodepathinfo buffer of exactly the size it is told,
    // and keeps no pointer past the call; a short read is checked before any field is.
    let read = unsafe {
        libc::proc_pidinfo(
            wanted,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            (&raw mut info).cast(),
            size,
        )
    };
    if read != size {
        return None;
    }
    let path: Vec<u8> = info
        .pvi_cdir
        .vip_path
        .iter()
        .flatten()
        .map(|byte| byte.to_ne_bytes()[0])
        .take_while(|byte| *byte != 0)
        .collect();
    (!path.is_empty()).then(|| std::ffi::OsString::from_vec(path).into())
}

/// The folder `pid` works in, as `/proc/PID/cwd` links to it: none when it cannot be read.
#[cfg(target_os = "linux")]
pub fn folder_of(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

/// No folder can be read on a system with no reader.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn folder_of(_pid: u32) -> Option<PathBuf> {
    None
}

/// A name in a buffer the kernel fills: up to its first NUL, or the whole buffer without one.
#[cfg(any(target_os = "macos", test))]
fn ended(name: &[u8]) -> String {
    let end = name
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(name.len());
    String::from_utf8_lossy(&name[..end]).into_owned()
}

/// The parent and name a `/proc/PID/stat` line gives: the name in parentheses, which may hold
/// either and spaces, then the state, then the parent.
#[cfg(any(target_os = "linux", test))]
fn stat_process(stat: &str) -> Option<(u32, String)> {
    let (head, fields) = stat.rsplit_once(')')?;
    let (_, name) = head.split_once('(')?;
    let parent = fields.split_whitespace().nth(1)?.parse().ok()?;
    Some((parent, name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_ends_at_its_first_nul_or_at_the_buffer_s_end() {
        assert_eq!(ended(b"cargo\0\0\0junk"), "cargo");
        assert_eq!(ended(b"sixteen-letters!"), "sixteen-letters!");
        assert_eq!(ended(b"\0cargo"), "");
        assert_eq!(ended(b""), "");
    }

    #[test]
    fn a_stat_line_names_the_parent_after_the_name_and_the_state() {
        assert_eq!(
            stat_process("42 (a) b) c) S 7 42 42 0 -1"),
            Some((7, "a) b) c".to_string()))
        );
        assert_eq!(
            stat_process("42 (cargo) S 1"),
            Some((1, "cargo".to_string()))
        );
        assert_eq!(stat_process("42 (sh) S"), None);
        assert_eq!(stat_process("42 no name"), None);
        assert_eq!(stat_process("42 sh) S 7"), None);
        assert_eq!(stat_process("42 (sh) S x"), None);
    }

    #[test]
    fn a_child_runs_under_this_process_by_its_program_s_name_in_the_folder_it_was_started_in() {
        let folder = std::env::temp_dir().canonicalize().unwrap();
        let mut child = crate::sys::command("sleep")
            .arg("30")
            .current_dir(&folder)
            .spawn()
            .unwrap();
        let pid = child.id();
        let (read, at) = (process(pid), folder_of(pid));
        child.kill().unwrap();
        child.wait().unwrap();
        if cfg!(any(target_os = "macos", target_os = "linux")) {
            assert_eq!(read, Some((std::process::id(), "sleep".to_string())));
            assert_eq!(at.map(|at| at.canonicalize().unwrap()), Some(folder));
        }
        // A reaped child names no process.
        assert_eq!(process(pid), None);
        assert_eq!(folder_of(pid), None);
        // This process's own callers start at its parent, each under the next, and never hold pid
        // 1 or itself.
        let callers = callers();
        if cfg!(any(target_os = "macos", target_os = "linux")) {
            // SAFETY: getppid cannot fail.
            let parent = unsafe { libc::getppid() };
            assert_eq!(
                callers.first().map(|caller| caller.pid),
                u32::try_from(parent).ok()
            );
        }
        for pair in callers.windows(2) {
            assert_eq!(pair[0].parent, pair[1].pid, "{callers:?}");
        }
        assert!(
            callers
                .iter()
                .all(|caller| caller.pid > 1 && caller.pid != std::process::id()),
            "{callers:?}"
        );
    }
}
