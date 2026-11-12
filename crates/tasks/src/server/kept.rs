//! The store kept before a private replay of its log ([`Home::replay`]): `data/` cloned whole
//! beside it, `data.before-replay-YYYY-MM-DDTHH-MM-SSZ` (UTC, with `.N` after one kept in the same
//! second), before the replay's server starts, so before anything can remove a log segment.
//!
//! A store that went back (a start that split its log's replay ended before its next flush: an
//! older build's kill, or a crash) holds a segment its manifest counts as written and never
//! applied. Any server on it skips that segment, and its clean stop removes it: the replay's stop
//! would, before the board's server came up. The clone keeps it, byte for byte, for a recovery
//! (docs/TASKS.md, Where it lives).
//!
//! It is one clone (`clonefile` on macOS, `cp --reflink=always` on Linux), which takes no room
//! until the store moves on from it; a volume that cannot clone gets a plain copy, said as one. A
//! store that can be neither cloned nor copied is not replayed and the start refused, the reason
//! said. A clone is made under `.partial` and renamed once whole, so one listed is whole. The board
//! never removes one nor overwrites one: each is the owner's, removed once the board reads as it
//! should, and `just tasks database` lists them. A replay runs only on a log a server left without
//! its clean close, so they are few; a start whose replay failed (a server that ended at once)
//! leaves the store as a kept one holds it, and the next keeps none more ([`Kept::Already`]).

use super::{DATA, Home, ServerError};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The start of the name of a store kept before a replay, in the board's folder.
pub const KEPT: &str = "data.before-replay-";
/// What a clone under way is named after, until it is whole.
const PARTIAL: &str = ".partial";

/// Where the store is kept before a replay, its folder beside `data/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kept {
    /// Cloned now, copy-on-write.
    Cloned(PathBuf),
    /// Copied now, byte by byte, as the volume cannot clone.
    Copied(PathBuf),
    /// Kept by an earlier start: its log and manifest are the store's now, file for file.
    Already(PathBuf),
}

impl Kept {
    /// Its folder.
    pub fn path(&self) -> &Path {
        match self {
            Kept::Cloned(path) | Kept::Copied(path) | Kept::Already(path) => path,
        }
    }
}

impl Home {
    /// The folders of every store kept before a replay, the oldest first, and of every one cut
    /// short (`.partial`), which a start ended in the middle of its clone or copy left.
    pub fn kept(&self) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return (Vec::new(), Vec::new());
        };
        let mut found: Vec<(String, PathBuf)> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                name.starts_with(KEPT).then(|| (name, entry.path()))
            })
            .collect();
        found.sort();
        let (partial, whole): (Vec<_>, Vec<_>) = found
            .into_iter()
            .partition(|(name, _)| name.ends_with(PARTIAL));
        (
            whole.into_iter().map(|(_, path)| path).collect(),
            partial.into_iter().map(|(_, path)| path).collect(),
        )
    }

    /// Keep the store as it is now, at `now` (seconds since the epoch): `data/` cloned whole to a
    /// folder of its own beside it, never one that exists; none when a kept store holds its log and
    /// manifest as they are, file for file ([`log_and_manifest`]). Under the starters' lock, while
    /// no server runs on it.
    pub(super) fn keep(&self, now: f64) -> Result<Kept, ServerError> {
        let data = self.dir.join(DATA);
        if let Some(now) = log_and_manifest(&data)
            && let Some(kept) = self
                .kept()
                .0
                .into_iter()
                .rev()
                .find(|kept| log_and_manifest(kept).as_ref() == Some(&now))
        {
            return Ok(Kept::Already(kept));
        }
        let name = free_name(&self.dir, &format!("{KEPT}{}", stamp(now)));
        let path = self.dir.join(&name);
        let partial = self.dir.join(format!("{name}{PARTIAL}"));
        let cloned = match clone(&data, &partial) {
            Ok(()) => true,
            Err(why) => {
                let _ = fs::remove_dir_all(&partial);
                copy(&data, &partial).map_err(|again| {
                    let _ = fs::remove_dir_all(&partial);
                    ServerError::Io(format!(
                        "the board's store at {} could not be kept before the replay of its log: \
                         it could not be cloned ({why}) nor copied ({again}), so it was not \
                         replayed and no server was started on it; make room, or clone it by hand, \
                         and run the board's command again",
                        data.display()
                    ))
                })?;
                false
            }
        };
        fs::rename(&partial, &path).map_err(|problem| {
            ServerError::Io(format!(
                "the board's store, kept before the replay of its log at {}, could not be named \
                 {}: {problem}; it was not replayed and no server was started on it",
                partial.display(),
                path.display()
            ))
        })?;
        Ok(if cloned {
            Kept::Cloned(path)
        } else {
            Kept::Copied(path)
        })
    }
}

/// The store at `store`'s log and manifest, file by file: each file in its `wal/` and `manifest/`,
/// by its path there, with its length and when it was last written, which a clone and a copy keep.
/// Two stores alike in it hold the same log under the same manifest. None for a store without
/// either folder, or one that cannot be read, which is like no other.
fn log_and_manifest(store: &Path) -> Option<Vec<(PathBuf, u64, SystemTime)>> {
    let mut found = Vec::new();
    for part in ["wal", "manifest"] {
        for entry in fs::read_dir(store.join(part)).ok()? {
            let entry = entry.ok()?;
            let meta = entry.metadata().ok()?;
            if meta.is_file() {
                found.push((
                    Path::new(part).join(entry.file_name()),
                    meta.len(),
                    meta.modified().ok()?,
                ));
            }
        }
    }
    found.sort();
    Some(found)
}

/// `base`, or `base.N` with the first `N` from 1 that names nothing in `folder`, nor a clone of it
/// under way.
fn free_name(folder: &Path, base: &str) -> String {
    let free = |name: &str| {
        fs::symlink_metadata(folder.join(name)).is_err()
            && fs::symlink_metadata(folder.join(format!("{name}{PARTIAL}"))).is_err()
    };
    if free(base) {
        return base.to_string();
    }
    (1..)
        .map(|n| format!("{base}.{n}"))
        .find(|name| free(name))
        .expect("some count names nothing")
}

/// Clone the folder `from` whole to `to`, which must not exist, copy-on-write: one `clonefile` on
/// macOS, `cp -R --reflink=always` on Linux.
fn clone(from: &Path, to: &Path) -> Result<(), String> {
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
            return Err("a path holds a NUL".into());
        };
        // SAFETY: both are NUL-terminated paths that outlive the call, which reads only them.
        if unsafe { libc::clonefile(src.as_ptr(), dst.as_ptr(), CLONE_NOFOLLOW) } != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        cp(
            &[
                "-R",
                "--no-dereference",
                "--preserve=all",
                "--reflink=always",
            ],
            from,
            to,
        )
    }
}

/// Copy the folder `from` whole to `to`, which must not exist, byte by byte.
fn copy(from: &Path, to: &Path) -> Result<(), String> {
    if cfg!(target_os = "macos") {
        cp(&["-R", "-p"], from, to)
    } else {
        cp(&["-R", "--no-dereference", "--preserve=all"], from, to)
    }
}

/// `cp OPTIONS FROM TO`, its refusal what it said.
fn cp(options: &[&str], from: &Path, to: &Path) -> Result<(), String> {
    let out = crate::sys::output(crate::sys::command("cp").args(options).arg(from).arg(to))
        .map_err(|problem| format!("cp: {problem}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// `seconds` since the epoch as a kept store's name says it: its UTC time, `YYYY-MM-DDTHH-MM-SSZ`.
pub(super) fn stamp(seconds: f64) -> String {
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

/// The proleptic Gregorian date `days` after 1970-01-01: year, month, day (Howard Hinnant's
/// `civil_from_days`, over eras of 400 years from 0000-03-01).
fn civil(days: i64) -> (i64, i64, i64) {
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
    use super::{KEPT, Kept, PARTIAL, civil, free_name, stamp};
    use crate::server::Home;
    use std::fs;

    /// A store is kept whole, and kept again only once its log or manifest moved: a start whose
    /// replay failed, leaving the store as it was, keeps none more. A store with no manifest is
    /// like no other, so it is kept each time.
    #[test]
    fn a_store_is_kept_whole_and_again_only_once_its_log_or_manifest_moved() {
        let dir = std::env::temp_dir().join(format!("kept-store-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("data/wal")).unwrap();
        fs::create_dir_all(dir.join("data/manifest")).unwrap();
        fs::create_dir_all(dir.join("data/sstables")).unwrap();
        fs::write(dir.join("data/wal/00000000000000000017.wal"), b"a log").unwrap();
        fs::write(
            dir.join("data/manifest/00000000000000000000.manifest"),
            b"m",
        )
        .unwrap();
        fs::write(dir.join("data/sstables/1.sst"), b"rows").unwrap();
        let home = Home::new(&dir, &dir.join("no-surreal"));
        let at = 1_794_469_500.0;
        // A clone where the volume clones (macOS's APFS), a copy where it cannot.
        let first = home.keep(at).unwrap();
        let (Kept::Cloned(path) | Kept::Copied(path)) = &first else {
            panic!("{first:?}");
        };
        assert_eq!(matches!(first, Kept::Cloned(_)), cfg!(target_os = "macos"));
        assert_eq!(path, &dir.join(format!("{KEPT}2026-11-12T07-45-00Z")));
        for (file, bytes) in [
            ("wal/00000000000000000017.wal", &b"a log"[..]),
            ("manifest/00000000000000000000.manifest", b"m"),
            ("sstables/1.sst", b"rows"),
        ] {
            assert_eq!(fs::read(path.join(file)).unwrap(), bytes, "{file}");
        }
        assert_eq!(home.keep(at + 60.0).unwrap(), Kept::Already(path.clone()));
        assert_eq!(home.kept(), (vec![path.clone()], vec![]));

        fs::write(dir.join("data/wal/00000000000000000018.wal"), b"more").unwrap();
        let second = home.keep(at).unwrap();
        assert!(!matches!(second, Kept::Already(_)), "{second:?}");
        assert_eq!(
            second.path(),
            dir.join(format!("{KEPT}2026-11-12T07-45-00Z.1"))
        );
        assert!(
            !path.join("wal/00000000000000000018.wal").exists(),
            "a kept store took a later write of the store's"
        );
        assert_eq!(home.kept().0.len(), 2);

        fs::remove_dir_all(dir.join("data/manifest")).unwrap();
        assert!(!matches!(home.keep(at).unwrap(), Kept::Already(_)));
        assert!(!matches!(home.keep(at).unwrap(), Kept::Already(_)));
        assert_eq!(home.kept().0.len(), 4);
        let _ = fs::remove_dir_all(&dir);
    }

    /// A store that is not there cannot be kept, and the refusal says the replay and start are
    /// not made.
    #[test]
    fn a_store_that_cannot_be_kept_refuses_the_replay() {
        let dir = std::env::temp_dir().join(format!("kept-none-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let home = Home::new(&dir, &dir.join("no-surreal"));
        let refused = home.keep(0.0).unwrap_err().to_string();
        assert!(
            refused.starts_with(&format!(
                "the board's store at {} could not be kept before the replay of its log: it \
                 could not be cloned (",
                dir.join("data").display()
            )),
            "{refused}"
        );
        assert!(
            refused.ends_with(
                "), so it was not replayed and no server was started on it; make room, or clone \
                 it by hand, and run the board's command again"
            ),
            "{refused}"
        );
        assert_eq!(home.kept(), (vec![], vec![]), "nothing cut short is left");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A kept store is named by its UTC second.
    #[test]
    fn a_kept_store_is_named_by_its_utc_second() {
        assert_eq!(stamp(0.0), "1970-01-01T00-00-00Z");
        assert_eq!(stamp(59.9), "1970-01-01T00-00-59Z", "the second floored");
        // 2026-11-12 08:45 in Rome, the board's loss.
        assert_eq!(stamp(1_794_469_500.0), "2026-11-12T07-45-00Z");
        assert_eq!(stamp(951_782_400.0), "2000-02-29T00-00-00Z", "a leap day");
        assert_eq!(stamp(-1.0), "1969-12-31T23-59-59Z");
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(-719_468), (0, 3, 1));
        assert_eq!(civil(59), (1970, 3, 1));
    }

    /// A name is never one that exists, nor one a clone under way holds.
    #[test]
    fn a_kept_store_never_takes_a_name_in_use() {
        let dir = std::env::temp_dir().join(format!("kept-names-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let base = format!("{KEPT}2026-11-12T07-45-00Z");
        assert_eq!(free_name(&dir, &base), base);
        fs::create_dir(dir.join(&base)).unwrap();
        assert_eq!(free_name(&dir, &base), format!("{base}.1"));
        fs::create_dir(dir.join(format!("{base}.1{PARTIAL}"))).unwrap();
        assert_eq!(free_name(&dir, &base), format!("{base}.2"));
        fs::write(dir.join(format!("{base}.2")), "a file").unwrap();
        assert_eq!(free_name(&dir, &base), format!("{base}.3"));
        let _ = fs::remove_dir_all(&dir);
    }
}
