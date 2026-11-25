//! A sweep request is appended before trying the lock. Contenders never wait or reach the board.
//! The holder releases the lock before checking for requests made during its pass, so a request
//! racing its exit is either served by it or by the caller that acquired the released lock.

use super::trigger;
use crate::sys;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const REQUESTS: &str = "sweep.requests";
const SERVED: &str = "sweep.served";

fn at<T>(path: &Path, result: std::io::Result<T>) -> Result<T, String> {
    result.map_err(|e| format!("{}: {e}", path.display()))
}

/// Full and task-scoped passes may inspect together, but only one removes a given tree.
pub(super) fn tree_lock(path: &Path) -> Option<sys::FileLock> {
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(super::git::admin(path)?.join("symphony-sweep-remove.lock"))
        .ok()?;
    file.try_lock().ok()?;
    Some(sys::FileLock(file))
}

pub struct Pass {
    common: PathBuf,
    held: sys::FileLock,
    end: u64,
    pub tasks: Vec<String>,
}

/// Record one request, then try the lock without waiting. No board is needed for either step.
pub fn request(common: &Path, tasks: &[&str]) -> Result<Option<Pass>, String> {
    let dir = trigger::folder(common);
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut line = serde_json::to_vec(tasks).map_err(|e| e.to_string())?;
    line.push(b'\n');
    OpenOptions::new()
        .append(true)
        .create(true)
        .open(dir.join(REQUESTS))
        .and_then(|mut file| file.write_all(&line))
        .map_err(|e| format!("cannot request a sweep in {}: {e}", dir.display()))?;
    take(common)
}

fn take(common: &Path) -> Result<Option<Pass>, String> {
    let dir = trigger::folder(common);
    let Some(held) = trigger::hold(common)? else {
        return Ok(None);
    };
    let requested = dir.join(REQUESTS);
    let mut requests = at(&requested, File::open(&requested))?;
    let end = at(&requested, requests.metadata())?.len();
    let served = fs::read_to_string(dir.join(SERVED))
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|n| *n <= end)
        .unwrap_or(0);
    if served == end {
        return Ok(None);
    }
    at(&requested, requests.seek(SeekFrom::Start(served)))?;
    let mut text = String::new();
    at(
        &requested,
        requests.take(end - served).read_to_string(&mut text),
    )?;
    let mut tasks = std::collections::BTreeSet::new();
    for line in text.lines() {
        tasks.extend(
            serde_json::from_str::<Vec<String>>(line)
                .map_err(|e| format!("{}: {e}", dir.join(REQUESTS).display()))?,
        );
    }
    Ok(Some(Pass {
        common: common.to_path_buf(),
        held,
        end,
        tasks: tasks.into_iter().collect(),
    }))
}

impl Pass {
    /// Publish completion, release, and take another pass only when newer requests stand.
    pub fn next(self) -> Result<Option<Self>, String> {
        let dir = trigger::folder(&self.common);
        let served = dir.join(SERVED);
        at(&served, fs::write(&served, self.end.to_string()))?;
        let Self {
            common, held, end, ..
        } = self;
        drop(held);
        let requested = dir.join(REQUESTS);
        if at(&requested, fs::metadata(&requested))?.len() == end {
            return Ok(None);
        }
        take(&common)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_interrupted_pass_retains_its_tasks_and_deduplicates_new_requests() {
        let dir =
            std::env::temp_dir().join(format!("symphony-sweep-interrupted-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let interrupted = request(&dir, &["tools/old"]).unwrap().unwrap();
        assert!(
            request(&dir, &["tools/old", "tools/new"])
                .unwrap()
                .is_none()
        );
        drop(interrupted);
        let resumed = request(&dir, &[]).unwrap().unwrap();
        assert_eq!(resumed.tasks, ["tools/new", "tools/old"]);
        assert!(resumed.next().unwrap().is_none());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ten_contenders_return_before_release_and_make_exactly_one_more_pass() {
        let dir =
            std::env::temp_dir().join(format!("symphony-sweep-coalesce-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let first = request(&dir, &[]).unwrap().unwrap();
        let (tell, told) = crate::test_channel::channel();
        let threads: Vec<_> = (0..10)
            .map(|n| {
                let common = dir.clone();
                let tell = tell.clone();
                std::thread::spawn(move || {
                    assert!(
                        request(&common, &[&format!("tools/{n}")])
                            .unwrap()
                            .is_none()
                    );
                    tell.send(()).unwrap();
                })
            })
            .collect();
        for _ in 0..10 {
            told.recv_timeout(test_time::backstop(std::time::Duration::from_secs(10)))
                .expect("each contender returns while the first pass still holds the lock");
        }
        for thread in threads {
            thread.join().unwrap();
        }
        let second = first.next().unwrap().unwrap();
        assert_eq!(second.tasks.len(), 10);
        assert!(second.next().unwrap().is_none());
        assert!(
            take(&dir).unwrap().is_none(),
            "completed requests need no pass"
        );
        let third = request(&dir, &[]).unwrap().unwrap();
        assert!(
            third.tasks.is_empty(),
            "completed requests are not read again: {:?}",
            third.tasks
        );
        assert!(third.next().unwrap().is_none());
        fs::remove_dir_all(dir).unwrap();
    }
}
