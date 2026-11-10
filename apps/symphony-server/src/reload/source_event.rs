//! Build input events for either the standalone workspace or its consuming host.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub(super) fn source_event(root: &Path, directories: &BTreeSet<PathBuf>, path: &Path) -> bool {
    if path.starts_with(root.join("symphony/apps/symphony-ui"))
        || path.starts_with(root.join("apps/symphony-ui"))
        || path.starts_with(root.join("apps/symphony-server/client/public"))
        || path.starts_with(root.join("symphony/apps/symphony-server/client/public"))
        || [
            "tools/rs-bin.sh",
            "tools/cargo.sh",
            "tools/test-rs.sh",
            "tools/check_lock.sh",
            "symphony/tools/rs-bin.sh",
            "symphony/tools/check_lock.sh",
        ]
        .iter()
        .any(|name| path == root.join(name))
    {
        return true;
    }
    // A referenced crate may not have a manifest yet, so it is absent even from a nonempty graph.
    // Manifest events rediscover that graph; an unrelated manifest leaves its fingerprint unchanged.
    // A wholly unreadable graph also needs to recover from other source corrections.
    if [
        "apps",
        "crates",
        "tools",
        "symphony/apps",
        "symphony/crates",
        "symphony/tools",
    ]
    .iter()
    .any(|group| path.starts_with(root.join(group)))
        && (directories.is_empty() || path.file_name().is_some_and(|name| name == "Cargo.toml"))
    {
        return true;
    }
    if path.starts_with(root.join(".cargo")) || path.starts_with(root.join("symphony/.cargo")) {
        return true;
    }
    if (path.parent() == Some(root) || path.parent() == Some(root.join("symphony").as_path()))
        && matches!(
            path.file_name().and_then(|n| n.to_str()),
            Some(
                "Cargo.toml"
                    | "Cargo.lock"
                    | "rust-toolchain.toml"
                    | "justfile"
                    | "apps"
                    | "crates"
                    | "tools"
            )
        )
    {
        return true;
    }
    directories.iter().any(|dir| {
        path == dir
            || path.starts_with(dir.join("src"))
            || (path.parent() == Some(dir)
                && matches!(
                    path.file_name().and_then(|n| n.to_str()),
                    Some("Cargo.toml" | "build.rs")
                ))
    })
}
