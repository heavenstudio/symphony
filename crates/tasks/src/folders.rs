//! The folders this build keeps a repository's state in, in its git common dir, each named here
//! once and every one in the one folder of Symphony's there, `symphony/` (DECISIONS:
//! symphony-own-names). The scripts that cannot read this file (`tools/surreal.sh`,
//! `tools/rs-bin.sh`) spell theirs alike.

/// The board: its server's data, record, lock and log, the heartbeat's keys, the land's state and
/// the sweeps', the dispatcher's and the staffing's records.
pub const TASKS: &str = "symphony/tasks";
/// The pinned SurrealDB `tools/surreal.sh` fetches, a folder for each version.
pub const SURREAL: &str = "symphony/surreal";
/// The shared server's install, the record of one started before the machine folder, and the
/// verdict journals and their locks.
pub const SERVER: &str = "symphony/server";
/// The recorded runs' day files and their logs.
pub const RUNS: &str = "symphony/runs";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_folder_is_its_own_in_the_one_folder_of_symphonys() {
        let all = [TASKS, SURREAL, SERVER, RUNS];
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
}
