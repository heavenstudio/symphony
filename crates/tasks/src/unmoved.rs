//! A repository whose state an older build kept in the old folders of its git common dir, which
//! `tools/move-state.sh` has not moved into `symphony/` yet. This build makes none of its folders
//! where the old one still holds that state ([`refusal`]): a board started in `symphony/tasks/`
//! would be an empty one beside the board the old folder holds, and the move would then refuse to
//! put that one in its place. A moved folder leaves a one-line file at its old name, no folder.

use crate::folders;
use std::path::Path;

/// Each folder this build keeps state in, in the git common dir, by the old one an older build
/// kept the same state in, which the move renames to it.
pub const OLD: [(&str, &str); 4] = [
    (folders::TASKS, "sf-tasks"),
    (folders::SERVER, "sf-symphony"),
    (folders::MODELS, "sf-task-models"),
    (folders::RUNS, "sf-runs"),
];

/// The line refusing to start or make anything in `new`, one of [`OLD`]'s folders, in the git
/// common dir `common`, while the old folder it moves from is a folder there (or a link to one);
/// none once it moved, and none for a folder [`OLD`] does not list.
pub fn refusal(common: &Path, new: &str) -> Option<String> {
    let (_, old) = OLD.iter().find(|(folder, _)| *folder == new)?;
    let path = common.join(old);
    path.is_dir().then(|| {
        format!(
            "this repository's state is still in {}, where an older build kept it, and this build \
             keeps it in {}: move it there first, the board and the shared server stopped, with \
             `bash tools/move-state.sh {}` in a Symphony checkout, then run this again; nothing \
             was started or made meanwhile",
            path.display(),
            common.join(new).display(),
            common.display()
        )
    })
}

/// The file the board's server records its address in, in the git common dir `common`:
/// `server.json` in [`folders::TASKS`], or in the board's old folder while that is a folder there.
/// An older build's server runs from that one until the move, the live board all the while, so a
/// run naming its port is read as the live board's, which a branch's build may not move.
pub fn board_record(common: &Path) -> std::path::PathBuf {
    let folder = OLD
        .iter()
        .find(|(new, _)| *new == folders::TASKS)
        .map(|(_, old)| common.join(old))
        .filter(|old| old.is_dir())
        .unwrap_or_else(|| common.join(folders::TASKS));
    folder.join("server.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_board_record_is_read_in_the_old_folder_until_the_move() {
        let dir = common("record");
        assert_eq!(board_record(&dir), dir.join("symphony/tasks/server.json"));
        std::fs::create_dir_all(dir.join(OLD[0].1)).unwrap();
        assert_eq!(
            board_record(&dir),
            dir.join(OLD[0].1).join("server.json"),
            "the older build's board"
        );
        // Its fence once it moved.
        std::fs::remove_dir(dir.join(OLD[0].1)).unwrap();
        std::fs::write(dir.join(OLD[0].1), "moved\n").unwrap();
        assert_eq!(board_record(&dir), dir.join("symphony/tasks/server.json"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn common(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("symphony-unmoved-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_old_folder_refuses_its_new_one_naming_both_and_the_move() {
        let dir = common("folder");
        std::fs::create_dir_all(dir.join("sf-tasks/data")).unwrap();
        assert_eq!(
            refusal(&dir, "symphony/tasks"),
            Some(format!(
                "this repository's state is still in {0}/sf-tasks, where an older build kept it, \
                 and this build keeps it in {0}/symphony/tasks: move it there first, the board \
                 and the shared server stopped, with `bash tools/move-state.sh {0}` in a Symphony \
                 checkout, then run this again; nothing was started or made meanwhile",
                dir.display()
            ))
        );
        // Only the folder whose old one is there.
        for new in ["symphony/server", "symphony/models", "symphony/runs"] {
            assert_eq!(refusal(&dir, new), None, "{new}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn each_old_folder_refuses_its_own_new_one() {
        let dir = common("each");
        for (new, old) in OLD {
            std::fs::create_dir_all(dir.join(old)).unwrap();
            let said = refusal(&dir, new).unwrap_or_else(|| panic!("{old} refuses {new}"));
            assert!(
                said.contains(&format!("{}/{old},", dir.display())),
                "{said}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_fence_a_missing_folder_or_an_unlisted_one_refuses_nothing() {
        let dir = common("none");
        assert_eq!(refusal(&dir, "symphony/tasks"), None, "nothing there");
        // The file a move leaves where the folder was.
        std::fs::write(
            dir.join("sf-tasks"),
            "moved by tools/move-state.sh to symphony/tasks: run only builds of the new code on \
             this repository\n",
        )
        .unwrap();
        assert_eq!(refusal(&dir, "symphony/tasks"), None, "a fence");
        std::fs::create_dir_all(dir.join("sf-surreal/3.3.0")).unwrap();
        assert_eq!(refusal(&dir, "symphony/surreal"), None, "not listed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The day files a build writes under `symphony/` (the board's health, the agents' activity,
    /// the reports) have no old folder: they are written while the old folders still stand, and
    /// the move goes around them.
    #[test]
    fn the_day_files_folders_are_no_old_folders_own() {
        let dir = common("days");
        for (_, old) in OLD {
            std::fs::create_dir_all(dir.join(old)).unwrap();
        }
        for new in [
            crate::board_health::hours::FOLDER,
            "symphony/agents",
            "symphony/reports",
        ] {
            assert_eq!(refusal(&dir, new), None, "{new}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_link_to_an_old_folder_is_that_folder() {
        let dir = common("link");
        std::fs::create_dir_all(dir.join("elsewhere")).unwrap();
        std::os::unix::fs::symlink(dir.join("elsewhere"), dir.join("sf-runs")).unwrap();
        assert!(refusal(&dir, "symphony/runs").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
