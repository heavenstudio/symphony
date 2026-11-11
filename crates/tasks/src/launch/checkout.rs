//! Preserve current feature ancestry and record ownership before checkout hooks can fail.

use super::{Ask, LaunchError, Machine, branch_exists, git};
use crate::{Error, Store, Task};
use std::collections::HashSet;
use std::path::Path;

pub(super) fn base(store: &Store, m: &Machine, ask: &Ask, task: &Task) -> Result<String, Error> {
    let mut base = "main".to_string();
    if ask.role == "executor" && !ask.detached && task.parent.is_some() {
        let mut feature = task.clone();
        let mut seen = HashSet::new();
        while let Some(parent) = feature.parent.as_deref() {
            if !seen.insert(feature.slug.clone()) {
                return Err(Error::Refused("cyclic feature ancestry".into()));
            }
            feature = store.get(parent)?;
        }
        base = crate::on_review::branch_of(&feature.slug).to_string();
        if !branch_exists(&m.root, &base) {
            return Err(Error::Refused(format!(
                "create parent feature branch {base} before launching a slice; slices do not start independently from main"
            )));
        }
    }
    Ok(base)
}

pub(super) fn make(
    machine: &Machine,
    path: &Path,
    branch: &str,
    detached: bool,
    base: &str,
    made: &mut bool,
) -> Result<(), LaunchError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| LaunchError(format!("cannot make {}: {e}", parent.display())))?;
    }
    let folder = path.to_string_lossy();
    let mut args = vec!["worktree", "add", "--no-checkout"];
    if detached {
        args.extend(["--detach", &folder, base]);
    } else if branch_exists(&machine.root, branch) {
        args.extend([&folder, branch]);
    } else {
        args.extend(["-b", branch, &folder, base]);
    }
    git(&machine.root, &args)?;
    // Only successful registration makes the checkout ours. A failing hook must not lose that
    // ownership; a failed registration on another launcher's path must not acquire it.
    *made = true;
    git(path, &["checkout", "--force"])?;
    Ok(())
}
