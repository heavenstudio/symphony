//! The machine's free disk, which the board warns of when it runs low: a full disk fails every build
//! on the machine and can corrupt the board's database. `tasks status` reads it at its run, for the
//! disk holding the board's record, and the Symphony server at each tick, for the disk holding the
//! project, on the pages' notice ([`Low`]). The board warns under `SYMPHONY_LOW_DISK_GB`
//! gigabytes (10^9 bytes) free, else [`THRESHOLD_GB`]; 0 never warns.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The setting naming the threshold, in gigabytes.
pub const VAR: &str = symphony_env::LOW_DISK_GB.name;
/// The threshold when [`VAR`] names none, in gigabytes.
pub const THRESHOLD_GB: f64 = 20.0;
/// A gigabyte, as the threshold and the warnings count it.
const GB: f64 = 1e9;

/// The threshold `vars` names, in gigabytes: [`VAR`], else [`THRESHOLD_GB`], a blank value naming
/// none; refused when it is not a number of gigabytes, 0 or more.
pub fn threshold(vars: &BTreeMap<String, String>) -> Result<f64, String> {
    let named = symphony_env::LOW_DISK_GB
        .get(|name| vars.get(name).map(|text| text.trim()))
        .filter(|text| !text.is_empty());
    let gb = match named {
        Some(text) => text
            .parse::<f64>()
            .map_err(|_| format!("{VAR}={text:?} is not a number of gigabytes"))?,
        None => THRESHOLD_GB,
    };
    if gb >= 0.0 && gb.is_finite() {
        Ok(gb)
    } else {
        Err(format!(
            "{VAR}={gb} is not a number of gigabytes, 0 or more"
        ))
    }
}

/// The free bytes of the disk holding `path`, read at the nearest folder of it that exists, and
/// that folder; none when it cannot be read.
pub fn free(path: &Path) -> Option<(PathBuf, u64)> {
    let at = path.ancestors().find(|p| p.exists())?;
    Some((at.to_path_buf(), crate::sys::free_bytes(at)?))
}

/// The disk holding `path` with less free than the threshold.
#[derive(Debug, Clone, PartialEq)]
pub struct Low {
    /// Where it was read: the folder asked about, or the nearest one above it that exists.
    pub path: PathBuf,
    /// Its free bytes, those a process of this user may write.
    pub free: u64,
    /// The threshold it is under, in gigabytes.
    pub threshold_gb: f64,
}

impl Low {
    /// Whether `free` bytes is under `threshold_gb`: the disk holding `path` running low; none when
    /// it is not.
    pub fn of(path: &Path, free: u64, threshold_gb: f64) -> Option<Low> {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a disk's bytes are compared in gigabytes"
        )]
        let under = (free as f64) < threshold_gb * GB;
        under.then(|| Low {
            path: path.to_path_buf(),
            free,
            threshold_gb,
        })
    }

    /// The disk holding `path` read now, against `threshold_gb` ([`free`]); none while it is not
    /// low, or when it cannot be read.
    pub fn check(path: &Path, threshold_gb: f64) -> Option<Low> {
        let (at, free) = free(path)?;
        Low::of(&at, free, threshold_gb)
    }

    /// Its free space in gigabytes, one decimal.
    fn free_gb(&self) -> String {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a disk's bytes are said in gigabytes"
        )]
        let gb = self.free as f64 / GB;
        format!("{gb:.1} GB")
    }

    /// What `tasks status` warns of it.
    pub fn warning(&self) -> String {
        format!(
            "the disk holding {} has {} free, under the {} GB the board warns at ({VAR}): a full disk fails every build on the machine and can corrupt the board's database; `just tasks sweep` clears the build output of finished worktrees",
            self.path.display(),
            self.free_gb(),
            self.threshold_gb
        )
    }

    /// What every page of the project says of it, on the server's notice: no figure that moves
    /// while it stays low, so the pages are told when it starts and when it ends, not at every
    /// build's write.
    pub fn notice(&self) -> String {
        format!(
            "The disk has under {} GB free: a full disk fails every build and can corrupt the task board. `just tasks status` says how much.",
            self.threshold_gb
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(value: Option<&str>) -> BTreeMap<String, String> {
        value
            .map(|v| (VAR.to_string(), v.to_string()))
            .into_iter()
            .collect()
    }

    #[test]
    fn the_threshold_is_the_setting_else_twenty_gigabytes() {
        assert_eq!(threshold(&vars(None)), Ok(THRESHOLD_GB));
        for blank in ["", "  "] {
            assert_eq!(threshold(&vars(Some(blank))), Ok(THRESHOLD_GB));
        }
        assert_eq!(threshold(&vars(Some(" 35.5 "))), Ok(35.5));
        assert_eq!(threshold(&vars(Some("0"))), Ok(0.0));
        assert_eq!(
            threshold(&vars(Some("lots"))),
            Err("SYMPHONY_LOW_DISK_GB=\"lots\" is not a number of gigabytes".into())
        );
        for wrong in ["-1", "NaN", "inf"] {
            assert!(threshold(&vars(Some(wrong))).is_err(), "{wrong}");
        }
    }

    #[test]
    fn a_disk_is_low_only_under_the_threshold() {
        let at = Path::new("/repo/.git/sf-tasks");
        assert_eq!(
            Low::of(at, 20_000_000_000, 20.0),
            None,
            "at it is not under"
        );
        assert_eq!(Low::of(at, 0, 0.0), None, "0 never warns, a full disk too");
        let low = Low::of(at, 19_999_999_999, 20.0).unwrap();
        assert_eq!(
            low,
            Low {
                path: at.into(),
                free: 19_999_999_999,
                threshold_gb: 20.0
            }
        );
        assert_eq!(
            Low::of(at, 12_345_678_901, 20.0).unwrap().warning(),
            "the disk holding /repo/.git/sf-tasks has 12.3 GB free, under the 20 GB the board warns at (SYMPHONY_LOW_DISK_GB): a full disk fails every build on the machine and can corrupt the board's database; `just tasks sweep` clears the build output of finished worktrees"
        );
        assert_eq!(
            Low::of(at, 4_960_000_000, 7.5).unwrap().warning(),
            "the disk holding /repo/.git/sf-tasks has 5.0 GB free, under the 7.5 GB the board warns at (SYMPHONY_LOW_DISK_GB): a full disk fails every build on the machine and can corrupt the board's database; `just tasks sweep` clears the build output of finished worktrees"
        );
        let notice = "The disk has under 7.5 GB free: a full disk fails every build and can corrupt the task board. `just tasks status` says how much.";
        assert_eq!(Low::of(at, 4_960_000_000, 7.5).unwrap().notice(), notice);
        assert_eq!(
            Low::of(at, 1_000, 7.5).unwrap().notice(),
            notice,
            "the same while it stays low"
        );
    }

    #[test]
    fn the_disk_is_read_at_the_nearest_folder_that_exists() {
        let dir = std::env::temp_dir();
        let missing = dir.join(format!("sf-disk-missing-{}/a/b", std::process::id()));
        let free = crate::sys::free_bytes(&dir).expect("the temporary folder's disk is read");
        assert!(free > 0, "{free}");
        // Over any disk there is: low; at 0: never.
        let low = Low::check(&missing, 1e12).expect("under a thousand terabytes");
        assert_eq!(
            (low.path.as_path(), low.threshold_gb),
            (dir.as_path(), 1e12)
        );
        assert!(low.free > 0);
        assert_eq!(Low::check(&missing, 0.0), None);
    }
}
