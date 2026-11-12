//! [`Home::snapshot`]: the board's store kept as it was, a copy-on-write clone of its data folder in
//! `snapshots/` beside it, named by when it was taken (`2026-11-12T07-45-00Z`, UTC), the newest
//! [`KEEP`] kept.
//!
//! A snapshot is taken only while no process serves the store, under the starters' lock, so no start
//! can begin while it is taken: never a copy of files a server is writing. It is one clone of the
//! whole folder (`clonefile` on macOS, `cp --reflink=always` on Linux), which takes no room until
//! the store moves on from it, and never a plain copy: a volume that cannot clone takes none. A
//! clone is made under a name of its own (`.taking`) and renamed once whole, so a snapshot that is
//! listed is whole; one cut short is removed by the next.
//!
//! The launcher takes one before each start of the board's server ([`Home::snapshot_before_start`]),
//! on a board outside a scratch place unless a test says otherwise: the store as the last server
//! left it, its write-ahead log not yet replayed, which is what a start that loses the log loses.

use super::{DATA, Home, LOG, scratch};
use std::fs;
use std::path::{Path, PathBuf};

/// The snapshots' folder in the board's folder.
pub const FOLDER: &str = "snapshots";
/// How many snapshots are kept: the newest.
pub const KEEP: usize = 3;
/// Where a snapshot is cloned until it is whole.
const TAKING: &str = ".taking";

/// One snapshot of the board's store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// Its name, when it was taken, in UTC: `YYYY-MM-DDTHH-MM-SSZ`, and `.N` after a second taken in
    /// the same second.
    pub name: String,
    /// Its folder, a store a server can be started on (a clone of it, never the snapshot itself).
    pub path: PathBuf,
}

/// What taking a snapshot did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Taken {
    /// The snapshot taken.
    pub snapshot: Snapshot,
    /// The older ones removed, beyond the newest [`KEEP`].
    pub removed: Vec<String>,
}

impl Home {
    /// Where its snapshots are.
    pub fn snapshots_folder(&self) -> PathBuf {
        self.dir.join(FOLDER)
    }

    /// Every snapshot of its store, the newest first: the folders in `snapshots/` named as one is.
    pub fn snapshots(&self) -> Result<Vec<Snapshot>, String> {
        let folder = self.snapshots_folder();
        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(format!("could not read {}: {e}", folder.display())),
        };
        let mut found: Vec<Snapshot> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                (named(&name) && entry.file_type().ok()?.is_dir()).then(|| Snapshot {
                    path: entry.path(),
                    name,
                })
            })
            .collect();
        found.sort_by(|a, b| order(&b.name).cmp(&order(&a.name)));
        Ok(found)
    }

    /// Take a snapshot of its store now, at `now` (seconds since the epoch), under the starters'
    /// lock: refused while a process serves the store, or when there is none.
    pub fn snapshot(&self, now: f64) -> Result<Taken, String> {
        let _held = self.lock().map_err(|e| e.to_string())?;
        self.snapshot_held(now)
    }

    /// [`Home::snapshot`], the starters' lock held by the caller.
    fn snapshot_held(&self, now: f64) -> Result<Taken, String> {
        let data = self.dir.join(DATA);
        if !data.is_dir() {
            return Err(format!(
                "the board has no store at {} to take a snapshot of",
                data.display()
            ));
        }
        if let Some(pid) = self.serving() {
            return Err(format!(
                "the board's server (pid {pid}) runs on {}: a snapshot is taken only while no \
                 process serves the store, never of files a server is writing, so none was taken; \
                 one is taken before the server's next start, and one of a store held stopped \
                 meanwhile is taken by this command",
                data.display()
            ));
        }
        let folder = self.snapshots_folder();
        fs::create_dir_all(&folder)
            .map_err(|e| format!("could not make {}: {e}", folder.display()))?;
        // Under the lock no other snapshot is being taken: a clone under way is one cut short.
        let taking = folder.join(TAKING);
        if fs::symlink_metadata(&taking).is_ok() {
            fs::remove_dir_all(&taking)
                .map_err(|e| format!("could not remove {}: {e}", taking.display()))?;
        }
        clone(&data, &taking)?;
        let name = free_name(&folder, &stamp(now));
        let path = folder.join(&name);
        fs::rename(&taking, &path).map_err(|e| {
            let _ = fs::remove_dir_all(&taking);
            format!(
                "could not move {} to {}: {e}",
                taking.display(),
                path.display()
            )
        })?;
        let mut removed = Vec::new();
        for old in self.snapshots()?.into_iter().skip(KEEP) {
            fs::remove_dir_all(&old.path)
                .map_err(|e| format!("could not remove {}: {e}", old.path.display()))?;
            removed.push(old.name);
        }
        Ok(Taken {
            snapshot: Snapshot { name, path },
            removed,
        })
    }

    /// Whether this board's servers take a snapshot before each start: one outside a scratch place
    /// does, unless a test said ([`Home::snapshotting`]).
    fn snapshots_before_start(&self) -> bool {
        self.snapshotting.unwrap_or_else(|| !scratch(&self.dir))
    }

    /// The snapshot the launcher takes before it starts the board's server, the starters' lock
    /// held: of the store as the last server left it, when there is one and this board takes them.
    /// Whatever happens is said in the server's log; a snapshot that cannot be taken never keeps the
    /// server from starting.
    pub(super) fn snapshot_before_start(&self) {
        if !self.snapshots_before_start() || !self.dir.join(DATA).is_dir() {
            return;
        }
        let said = match self.snapshot_held(crate::store::now()) {
            Ok(taken) => format!(
                "· a snapshot of the store before this start: {}",
                taken.snapshot.path.display()
            ),
            Err(why) => format!("· no snapshot of the store before this start: {why}"),
        };
        let log = self.dir.join(LOG);
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&log) {
            use std::io::Write;
            let _ = writeln!(file, "{said}");
        }
    }

    /// The process serving its store, if one does: its recorded server, or one on its data that the
    /// record does not name.
    fn serving(&self) -> Option<i64> {
        if let Some(running) = self.recorded()
            && self.serves(running.pid)
        {
            return Some(running.pid);
        }
        self.unrecorded().map(|running| running.pid)
    }
}

/// Clone the folder `from` whole to `to`, which must not exist, copy-on-write: one `clonefile` on
/// macOS, `cp -R --reflink=always` on Linux. Never a plain copy: a volume that cannot clone refuses.
fn clone(from: &Path, to: &Path) -> Result<(), String> {
    let refused = |why: &dyn std::fmt::Display| {
        format!(
            "could not clone {} to {} copy-on-write ({why}), and a snapshot is never a plain copy",
            from.display(),
            to.display()
        )
    };
    #[cfg(target_os = "macos")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        /// `clonefile`'s `CLONE_NOFOLLOW`: a link is cloned as itself.
        const CLONE_NOFOLLOW: u32 = 1;
        let (Ok(src), Ok(dst)) = (
            CString::new(from.as_os_str().as_bytes()),
            CString::new(to.as_os_str().as_bytes()),
        ) else {
            return Err(refused(&"a path holds a NUL"));
        };
        // SAFETY: both are NUL-terminated paths that outlive the call, which reads only them.
        if unsafe { libc::clonefile(src.as_ptr(), dst.as_ptr(), CLONE_NOFOLLOW) } != 0 {
            return Err(refused(&std::io::Error::last_os_error()));
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let out = crate::sys::output(
            crate::sys::command("cp")
                .args([
                    "-R",
                    "--no-dereference",
                    "--preserve=all",
                    "--reflink=always",
                ])
                .arg(from)
                .arg(to),
        )
        .map_err(|e| refused(&e))?;
        if out.status.success() {
            return Ok(());
        }
        let _ = fs::remove_dir_all(to);
        Err(refused(&String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// `base`, or `base.N` with the first `N` from 1 that names nothing in `folder`.
fn free_name(folder: &Path, base: &str) -> String {
    if fs::symlink_metadata(folder.join(base)).is_err() {
        return base.to_string();
    }
    (1..)
        .map(|n| format!("{base}.{n}"))
        .find(|name| fs::symlink_metadata(folder.join(name)).is_err())
        .unwrap_or_else(|| base.to_string())
}

/// Whether `name` is a snapshot's: `YYYY-MM-DDTHH-MM-SSZ`, then `.N` or nothing.
fn named(name: &str) -> bool {
    const SHAPE: &[u8; 20] = b"0000-00-00T00-00-00Z";
    let bytes = name.as_bytes();
    let (stamp, after) = bytes.split_at(bytes.len().min(SHAPE.len()));
    let shape = stamp.len() == SHAPE.len()
        && stamp.iter().zip(SHAPE).all(|(byte, want)| match want {
            b'0' => byte.is_ascii_digit(),
            want => byte == want,
        });
    shape
        && (after.is_empty()
            || after
                .strip_prefix(b".")
                .is_some_and(|n| !n.is_empty() && n.iter().all(u8::is_ascii_digit)))
}

/// How snapshots are ordered by date: by their time, then by the count after it.
fn order(name: &str) -> (&str, u64) {
    match name.split_once('.') {
        Some((stamp, n)) => (stamp, n.parse().unwrap_or(0)),
        None => (name, 0),
    }
}

/// `seconds` since the epoch as a snapshot's name: its UTC time, `YYYY-MM-DDTHH-MM-SSZ`.
pub fn stamp(seconds: f64) -> String {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a time in seconds is far inside i64"
    )]
    let seconds = seconds.floor() as i64;
    let (days, day) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    let (year, month, date) = civil(days);
    format!(
        "{year:04}-{month:02}-{date:02}T{:02}-{:02}-{:02}Z",
        day / 3600,
        day % 3600 / 60,
        day % 60
    )
}

/// The proleptic Gregorian date `days` after 1970-01-01: year, month, day.
fn civil(days: i64) -> (i64, i64, i64) {
    // Howard Hinnant's `civil_from_days`, over eras of 400 years from 0000-03-01.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let date = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, date)
}

#[cfg(test)]
mod tests {
    use super::{civil, free_name, named, order, stamp};

    #[test]
    fn a_snapshot_is_named_by_its_utc_second() {
        assert_eq!(stamp(0.0), "1970-01-01T00-00-00Z");
        assert_eq!(stamp(59.9), "1970-01-01T00-00-59Z", "the second floored");
        assert_eq!(stamp(1_762_933_512.0), "2025-11-12T07-45-12Z");
        // 2026-11-12 08:45 in Rome, the board's loss.
        assert_eq!(stamp(1_794_469_500.0), "2026-11-12T07-45-00Z");
        assert_eq!(stamp(951_782_400.0), "2000-02-29T00-00-00Z", "a leap day");
        assert_eq!(
            stamp(4_107_542_399.0),
            "2100-02-28T23-59-59Z",
            "2100 is no leap year"
        );
        assert_eq!(stamp(-1.0), "1969-12-31T23-59-59Z");
        assert_eq!(civil(-719_468), (0, 3, 1));
    }

    #[test]
    fn only_a_name_of_that_shape_is_a_snapshot_and_they_order_by_date_then_count() {
        for good in [
            "2026-11-12T07-45-00Z",
            "2026-11-12T07-45-00Z.1",
            "2026-11-12T07-45-00Z.12",
        ] {
            assert!(named(good), "{good}");
        }
        for bad in [
            "",
            ".taking",
            "data",
            "2026-11-12T07-45-00",
            "2026-11-12T07:45:00Z",
            "2026-11-12 07-45-00Z",
            "2026-11-12T07-45-00Z.",
            "2026-11-12T07-45-00Z.x",
            "2026-11-12T07-45-00Z-old",
            "x026-11-12T07-45-00Z",
            "2026-1é-12T07-45-00Z",
            "2026-11-12T07-45-00é",
        ] {
            assert!(!named(bad), "{bad}");
        }
        let mut names = vec![
            "2026-11-12T07-45-00Z.10",
            "2026-11-12T07-45-00Z",
            "2026-11-12T07-45-00Z.2",
            "2026-11-11T23-59-59Z",
        ];
        names.sort_by(|a, b| order(b).cmp(&order(a)));
        assert_eq!(
            names,
            [
                "2026-11-12T07-45-00Z.10",
                "2026-11-12T07-45-00Z.2",
                "2026-11-12T07-45-00Z",
                "2026-11-11T23-59-59Z"
            ]
        );
    }

    #[test]
    fn a_second_snapshot_in_the_same_second_takes_the_next_count() {
        let folder = std::env::temp_dir().join(format!("snapshot-names-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let base = "2026-11-12T07-45-00Z";
        assert_eq!(free_name(&folder, base), base);
        std::fs::create_dir(folder.join(base)).unwrap();
        assert_eq!(free_name(&folder, base), format!("{base}.1"));
        std::fs::create_dir(folder.join(format!("{base}.1"))).unwrap();
        assert_eq!(free_name(&folder, base), format!("{base}.2"));
        let _ = std::fs::remove_dir_all(&folder);
    }
}
