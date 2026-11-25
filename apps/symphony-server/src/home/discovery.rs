//! The ports an auto-port start looks at before it reads the process table: the record's, the
//! owned log's last announcement and the bound listener's hint, each read bounded.

use super::{Home, LOG, announced_port};
use std::fs::OpenOptions;
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

const PORT_HINT_BYTES: u64 = 16;
const DISCOVERY_LOG_BYTES: u64 = 16 * 1024;
/// What every announcement line starts its URL with.
const ANNOUNCED: &str = "→ Symphony at http://127.0.0.1:";

/// What one piece of startup history says about the port a server of this folder may hold.
enum Hint {
    /// Nothing was written: no history.
    Absent,
    Port(u16),
    /// Written but unreadable, unannounced or malformed: history whose port is unknown.
    Unknown,
}

impl Home {
    /// Port hints select a lookup; only the process command and its health identify this server.
    /// Unreadable or unannounced startup history retains the census recovery path.
    pub(super) fn discovery_ports(&self) -> (Vec<u16>, bool) {
        let mut ports = vec![self.port];
        ports.extend(self.recorded().map(|running| running.port));
        let mut unknown = false;
        if ports.contains(&0) {
            let folders = [Some(self.dir.as_path()), self.legacy.as_deref()];
            let logs = folders.into_iter().flatten().map(logged_port);
            // A bound listener publishes its hint before warming or writing its announcement.
            for hint in logs.chain([hinted_port(&self.root)]) {
                match hint {
                    Hint::Absent => {}
                    Hint::Port(port) => ports.push(port),
                    Hint::Unknown => unknown = true,
                }
            }
        }
        ports.retain(|&port| port != 0);
        symphony_board::order::sort_by(&mut ports, &Ord::cmp);
        ports.dedup();
        (ports, unknown)
    }
}

/// The port of the last announcement in `folder`'s owned log, read from its bounded tail.
fn logged_port(folder: &Path) -> Hint {
    let said = match read_tail(&folder.join(LOG), DISCOVERY_LOG_BYTES) {
        Ok((_, said)) => said,
        Err(error) if error.kind() == ErrorKind::NotFound => return Hint::Absent,
        Err(_) => return Hint::Unknown,
    };
    let Some(line) = said.lines().rev().find(|line| line.contains(ANNOUNCED)) else {
        return Hint::Unknown;
    };
    let pid = line
        .trim_end()
        .rsplit_once("(pid ")
        .and_then(|(_, pid)| pid.strip_suffix(')'))
        .and_then(|pid| pid.parse().ok());
    match pid.and_then(|pid| announced_port(line, pid)) {
        Some(port) if port != 0 => Hint::Port(port),
        _ => Hint::Unknown,
    }
}

/// The port the listener's hint file under `root` names, read no further than a port's length.
fn hinted_port(root: &Path) -> Hint {
    match read_tail(&crate::front::port_file(root), PORT_HINT_BYTES) {
        Ok((length, said)) if length <= PORT_HINT_BYTES => match said.trim().parse::<u16>() {
            Ok(port) if port != 0 => Hint::Port(port),
            _ => Hint::Unknown,
        },
        Err(error) if error.kind() == ErrorKind::NotFound => Hint::Absent,
        _ => Hint::Unknown,
    }
}

/// An owned regular file's bounded snapshot tail, preserving damaged or split UTF-8 lossily.
fn read_tail(path: &Path, limit: u64) -> std::io::Result<(u64, String)> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    let from = metadata.len().saturating_sub(limit);
    file.seek(SeekFrom::Start(from))?;
    let mut bytes = Vec::new();
    file.take(metadata.len() - from).read_to_end(&mut bytes)?;
    Ok((metadata.len(), String::from_utf8_lossy(&bytes).into_owned()))
}

#[cfg(test)]
#[path = "discovery_tests.rs"]
mod tests;
