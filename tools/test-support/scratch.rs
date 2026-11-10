//! Dead-owner cleanup shared by integration-test harnesses. Each caller owns its scratch prefix and
//! decides when to sweep; a reused pid or any uncertainty preserves the folder.

use std::fs;
use std::io;
use std::path::Path;

/// Drop cannot run after SIGKILL. A later run reclaims only matching directories with known-dead owners.
pub(super) fn reap_dead_scratch(root: &Path, prefix: &str) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name();
        let Some((pid, counter)) = name
            .to_str()
            .and_then(|name| name.strip_prefix(prefix))
            .and_then(|name| name.split_once('-'))
        else {
            continue;
        };
        if pid.is_empty()
            || !pid.bytes().all(|byte| byte.is_ascii_digit())
            || counter.is_empty()
            || !counter.bytes().all(|byte| byte.is_ascii_digit())
        {
            continue;
        }
        let Ok(pid) = pid.parse::<libc::pid_t>() else {
            continue;
        };
        if pid <= 0 || !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        // SAFETY: a positive pid and signal 0 only query existence; no signal is sent. Permission
        // errors and any other uncertainty preserve the folder, as does pid reuse by a live process.
        if unsafe { libc::kill(pid, 0) } == -1
            && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}
