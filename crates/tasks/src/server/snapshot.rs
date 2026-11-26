//! What a snapshot of the board's PostgreSQL is named and copied by
//! ([`super::Postgres::snapshot`]): its name, when it was taken (`2026-11-12T07-45-00Z`, UTC, and
//! `.N` after a second taken in the same second), how snapshots are ordered by it, and the copy of
//! a folder whole, a copy-on-write clone where the volume clones (`clonefile` on macOS,
//! `cp --reflink=always` on Linux), else a plain copy that keeps each file's times. A restore's
//! switch names the folder it keeps the replaced cluster in by the same stamp
//! ([`super::moving`]).

use std::path::Path;

#[cfg(test)]
thread_local! {
    /// Why a clone made on this thread fails, while a test says one does: a volume that cannot
    /// clone, so the plain copy's path runs where every volume a test runs on clones.
    pub(super) static CLONE_FAILS: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
}

/// Clone the folder `from` whole to `to`, which must not exist, copy-on-write: one `clonefile` on
/// macOS, `cp -R --reflink=always` on Linux; why not, when it cannot.
pub(super) fn clone(from: &Path, to: &Path) -> Result<(), String> {
    #[cfg(test)]
    if let Some(why) = CLONE_FAILS.get() {
        return Err(why.into());
    }
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

/// Copy the folder `from` whole to `to`, which must not exist, byte by byte, keeping each file's
/// times.
pub(super) fn copy(from: &Path, to: &Path) -> Result<(), String> {
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

/// Whether `name` is a snapshot's: `YYYY-MM-DDTHH-MM-SSZ`, then `.N` or nothing.
pub(super) fn named(name: &str) -> bool {
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
pub(super) fn order(name: &str) -> (&str, u64) {
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
    use super::{CLONE_FAILS, civil, clone, copy, named, order, stamp};
    use std::fs;

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
        crate::order::sort_by(&mut names, &|a, b| order(b).cmp(&order(a)));
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

    /// A clone and a plain copy each hold the folder whole, a plain copy with each file's times;
    /// a clone made on a thread a test makes fail says why, and makes nothing.
    #[test]
    fn a_folder_is_cloned_or_copied_whole_and_a_failed_clone_says_why() {
        let dir = std::env::temp_dir().join(format!("snapshot-copies-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let from = dir.join("from");
        fs::create_dir_all(from.join("inner")).unwrap();
        fs::write(from.join("file"), b"rows").unwrap();
        fs::write(from.join("inner/log"), b"a log").unwrap();
        let copied = dir.join("copied");
        copy(&from, &copied).unwrap();
        // Where the volume clones (APFS, btrfs, XFS); one that cannot is refused, saying why.
        let cloned = dir.join("cloned");
        let clones = clone(&from, &cloned).is_ok();
        for to in [&copied, &cloned] {
            if to == &cloned && !clones {
                continue;
            }
            for file in ["file", "inner/log"] {
                assert_eq!(
                    fs::read(to.join(file)).unwrap(),
                    fs::read(from.join(file)).unwrap()
                );
                assert_eq!(
                    fs::metadata(to.join(file)).unwrap().modified().unwrap(),
                    fs::metadata(from.join(file)).unwrap().modified().unwrap(),
                    "{file}: its time"
                );
            }
        }
        CLONE_FAILS.set(Some("this volume cannot clone"));
        let refused = clone(&from, &dir.join("refused"));
        CLONE_FAILS.set(None);
        assert_eq!(refused, Err("this volume cannot clone".to_string()));
        assert!(!dir.join("refused").exists());
        assert!(copy(&dir.join("missing"), &dir.join("none")).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
