//! What a worktree or a build folder holds on disk, as `du` counts it: every file's allocated
//! blocks, a file with several links once, no link followed. A copy-on-write clone counts in full,
//! as `du` counts it, so removing one may give the disk back less than it says; `df` says what the
//! disk holds.

use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// A gigabyte, a megabyte and a kilobyte, as the board's disk warning counts them (10^9 bytes).
const UNITS: [(f64, &str); 3] = [(1e9, "GB"), (1e6, "MB"), (1e3, "KB")];

/// The bytes `path` holds on disk, a folder with all it holds; 0 when nothing there can be read.
/// What cannot be read inside (a folder the user may not list) is left out, never an error.
pub fn bytes(path: &Path) -> u64 {
    let mut seen = BTreeSet::new();
    walk(path, &mut seen)
}

fn walk(path: &Path, seen: &mut BTreeSet<(u64, u64)>) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    // A file with other links counts once, wherever its other names are (the `rs-bin` hard links).
    if !meta.is_dir() && meta.nlink() > 1 && !seen.insert((meta.dev(), meta.ino())) {
        return 0;
    }
    let mut total = meta.blocks().saturating_mul(512);
    if meta.is_dir()
        && let Ok(entries) = fs::read_dir(path)
    {
        for entry in entries.flatten() {
            total = total.saturating_add(walk(&entry.path(), seen));
        }
    }
    total
}

/// `bytes` said in the largest unit it reaches, one decimal: `12.3 GB`, `4.0 MB`, `512 B`.
pub fn shown(bytes: u64) -> String {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a size is said to one decimal of its unit"
    )]
    let value = bytes as f64;
    UNITS.iter().find(|(unit, _)| value >= *unit).map_or_else(
        || format!("{bytes} B"),
        |(unit, name)| format!("{:.1} {name}", value / unit),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("symphony-sweep-size-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_size_is_said_in_the_largest_unit_it_reaches() {
        assert_eq!(shown(0), "0 B");
        assert_eq!(shown(999), "999 B");
        assert_eq!(shown(1000), "1.0 KB");
        assert_eq!(shown(4_000_000), "4.0 MB");
        assert_eq!(shown(12_345_000_000), "12.3 GB");
        assert_eq!(shown(2_000_000_000_000), "2000.0 GB");
    }

    #[test]
    fn a_folder_counts_what_it_holds_a_hard_link_once_and_no_link_followed() {
        let dir = scratch("walk");
        let tree = dir.join("tree");
        fs::create_dir_all(tree.join("deep/deeper")).unwrap();
        fs::write(tree.join("deep/deeper/a"), vec![7u8; 64 * 1024]).unwrap();
        let one = bytes(&tree.join("deep/deeper/a"));
        assert!(one >= 64 * 1024, "{one}");
        let alone = bytes(&tree);
        assert!(alone >= one, "{alone} {one}");
        // A second name for the same file adds nothing; a link to a big folder outside adds only
        // the link itself.
        fs::hard_link(tree.join("deep/deeper/a"), tree.join("b")).unwrap();
        let outside = dir.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("big"), vec![1u8; 512 * 1024]).unwrap();
        symlink(&outside, tree.join("link")).unwrap();
        let linked = bytes(&tree);
        assert!(linked < alone + 64 * 1024, "{linked} {alone}");
        assert_eq!(bytes(&dir.join("missing")), 0);
        fs::remove_dir_all(&dir).unwrap();
    }
}
