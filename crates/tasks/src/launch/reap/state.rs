//! What sweeps keep between them: where (`reap.json`, [`state_path`]), the lock no two sweeps hold
//! at once ([`lock`]), and what the last one left: when each terminal was first seen idle (`idle`),
//! and why each idle one was last kept (`said`).

use crate::cli::Env;
use crate::git;
use crate::sys::FileLock;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap};
use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};

/// Where the sweeps of the board `env` reaches from `cwd` keep their state: `TASKS_REAP_STATE`; for a
/// board `TASKS_SERVER` names (a test's), a file of its own in the temp dir, never the live board's;
/// else `symphony/tasks/reap.json` in the git common dir, beside the board's server.
pub fn state_path(env: &Env, cwd: &Path) -> Result<PathBuf, String> {
    if let Some(named) = env.var("TASKS_REAP_STATE") {
        return Ok(PathBuf::from(named));
    }
    if env.var("TASKS_SERVER").is_some()
        || env
            .var("TASKS_DATABASE")
            .is_some_and(|db| db != crate::DATABASE)
    {
        let board = format!(
            "{} {}",
            env.var("TASKS_SERVER").unwrap_or("local"),
            env.var("TASKS_DATABASE").unwrap_or("")
        );
        return Ok(std::env::temp_dir()
            .join("symphony-tasks-reap")
            .join(format!("reap-{:016x}.json", fnv1a(board.as_bytes()))));
    }
    let common = git::common_dir(cwd)
        .map_err(|why| format!("not in a git repository ({why}): no task board to sweep"))?;
    Ok(common.join(crate::folders::TASKS).join("reap.json"))
}

/// FNV-1a, 64 bits: a name of its own for each test board's state file.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, b| {
        (hash ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

pub fn lock_path(state: &Path) -> PathBuf {
    let mut name = state.file_name().unwrap_or_default().to_os_string();
    name.push(".lock");
    state.with_file_name(name)
}

/// The sweeps' lock beside `state`, not waited for: None while another sweep holds it.
pub(super) fn lock(state: &Path) -> std::io::Result<Option<FileLock>> {
    if let Some(folder) = state.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let file = File::options()
        .append(true)
        .create(true)
        .open(lock_path(state))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(FileLock(file))),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

/// What the last sweep left in `state`; nothing for a state it cannot read.
pub(super) fn read_state(state: &Path) -> Map<String, Value> {
    let Ok(text) = std::fs::read_to_string(state) else {
        return Map::new();
    };
    match serde_json::from_slice::<Value>(text.as_bytes()) {
        Ok(Value::Object(read)) => read,
        _ => Map::new(),
    }
}

/// Terminal key -> when a sweep first saw it idle (the board's clock).
pub(super) fn read_seen(saved: &Map<String, Value>, now: f64) -> HashMap<String, f64> {
    let Some(Value::Object(idle)) = saved.get("idle") else {
        return HashMap::new();
    };
    let mut seen = HashMap::new();
    for (key, since) in idle {
        let Some(since) = since
            .as_f64()
            .filter(|since| since.is_finite() && *since > 0.0 && *since <= now)
        else {
            continue; // a malformed or impossible mark starts a fresh idle wait
        };
        seen.insert(key.clone(), since);
    }
    seen
}

/// Terminal -> why a sweep last said it kept it idle.
pub(super) fn read_said(saved: &Map<String, Value>) -> HashMap<String, String> {
    let Some(Value::Object(said)) = saved.get("said") else {
        return HashMap::new();
    };
    said.iter()
        .filter_map(|(terminal, why)| Some((terminal.clone(), why.as_str()?.to_string())))
        .collect()
}

/// What this sweep leaves for the next: the idle marks it keeps, and why it kept each idle one.
pub(super) fn write_state(
    state: &Path,
    idle: BTreeMap<String, f64>,
    said: &BTreeMap<String, String>,
) -> std::io::Result<()> {
    let idle: Map<String, Value> = idle
        .into_iter()
        .map(|(key, since)| (key, json!(since)))
        .collect();
    std::fs::write(state, json!({"idle": idle, "said": said}).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ended_sweep_frees_its_lock_while_a_forked_child_still_holds_the_descriptor() {
        let dir =
            std::env::temp_dir().join(format!("symphony-reap-release-{}", std::process::id()));
        let state = dir.join("reap.json");
        let held = lock(&state).unwrap().unwrap();
        // What fork gives a child until it execs: the same open file, so the same lock.
        let inherited = held.0.try_clone().unwrap();
        drop(held);
        let next = lock(&state).unwrap();
        assert!(next.is_some(), "the ended sweep still holds its lock");
        assert!(lock(&state).unwrap().is_none());
        drop((inherited, next));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
