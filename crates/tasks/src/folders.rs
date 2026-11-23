//! The folders this build keeps a repository's state in, in its git common dir, each named here
//! once and every one in the one folder of Symphony's there, `symphony/`
//! (docs/engineering/analysis/own-names.md#symphony-own-names). The scripts that cannot read this file
//! (`tools/rs-bin.sh`) spell theirs alike.

use std::path::{Path, PathBuf};

/// The board: its server's data, lock and log, the heartbeat's keys, the land's state and the
/// sweeps', the dispatcher's and the staffing's records.
pub const TASKS: &str = "symphony/tasks";
/// The shared server's install, the record of one started before the machine folder, and the
/// verdict journals and their locks.
pub const SERVER: &str = "symphony/server";
/// The recorded runs' day files and their logs.
pub const RUNS: &str = "symphony/runs";
/// The release run's records, logs and lock (`just release`, in the Symphony's own repository
/// alone): a checkout whose repository keeps it is the Symphony's.
pub const RELEASE: &str = "symphony/release";
/// What a program that exchanges the board's tasks with another system keeps between its runs
/// (its mappings, its journal), a folder for each such program ([`integration`]). Nothing here
/// makes, reads or sweeps it: the program does.
pub const INTEGRATIONS: &str = "symphony/integrations";

/// The folder the integration `name` keeps its state in, for the repository whose common dir is
/// `common`: the same from every checkout and worktree of it, and there after any of them is
/// removed. It is named, never made.
///
/// # Errors
///
/// When `name` is not one to sixty-four lowercase letters, digits and hyphens, a letter or a
/// digit first: it is one folder's name, and never a path.
pub fn integration(common: &Path, name: &str) -> Result<PathBuf, String> {
    let plain = (1..=64).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !plain {
        return Err(format!(
            "an integration's name is one to sixty-four lowercase letters, digits and hyphens, a \
             letter or a digit first, not {name:?}"
        ));
    }
    Ok(common.join(INTEGRATIONS).join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_folder_is_its_own_in_the_one_folder_of_symphonys() {
        let all = [TASKS, SERVER, RUNS, RELEASE, INTEGRATIONS];
        for folder in all {
            let (top, name) = folder.split_once('/').expect("a folder under symphony/");
            assert_eq!(top, "symphony", "{folder}");
            assert!(!name.is_empty() && !name.contains('/'), "{folder}");
        }
        let mut names = all.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), all.len(), "each folder once");
    }

    #[test]
    fn an_integrations_folder_is_one_plain_name_under_the_common_dir() {
        let common = Path::new("/repo/.git");
        assert_eq!(
            integration(common, "github").unwrap(),
            Path::new("/repo/.git/symphony/integrations/github")
        );
        let longest = "a".repeat(64);
        for name in ["g", "2-way", "issue-exchange", longest.as_str()] {
            assert_eq!(
                integration(common, name).unwrap(),
                common.join(INTEGRATIONS).join(name)
            );
        }
        let long = "a".repeat(65);
        for name in [
            "",
            ".",
            "..",
            "a/b",
            "../tasks",
            "-a",
            "A",
            "a b",
            "a.b",
            "é",
            long.as_str(),
        ] {
            let why = integration(common, name).unwrap_err();
            assert!(why.contains("an integration's name is"), "{name}: {why}");
        }
    }
}
