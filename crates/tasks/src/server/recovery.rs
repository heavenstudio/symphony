//! A board home's recovery authority: for each of its databases, the recovery generation the
//! board in it is bound to ([`crate::store::recovery`]), in `recovery/DATABASE.json`,
//! `{"format":1,"generation":"…"}`. No file is a board bound to none, and so is every database of
//! a home a restore just made ([`Home::revoke_recoveries`]). The folder is the home's own, beside
//! its engine's data and outside it, so neither a snapshot nor a restore of that data carries it.
//!
//! A file is published whole: written beside itself and synced, renamed into place, then the
//! folder synced, so a reader finds the generation before or the one after and never part of
//! one; a revoke removes it and syncs the folder. One that does not read as this format refuses
//! rather than read as none bound or as some generation.
//!
//! It is this machine's word alone: a board other machines write too needs an authority they all
//! read, which this file is not.

use super::{Home, ServerError, io};
use crate::store::recovery::Authority;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Write as _;
use std::path::{Path, PathBuf};

const FOLDER: &str = "recovery";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bound {
    format: u32,
    generation: String,
}

/// The authority of one database of a board home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileAuthority {
    path: PathBuf,
}

impl Home {
    /// The recovery authority of the board in `database`, a plain name, of this home.
    pub fn recovery(&self, database: &str) -> FileAuthority {
        assert!(crate::db::plain(database), "a database is a plain name");
        FileAuthority {
            path: self.dir.join(FOLDER).join(format!("{database}.json")),
        }
    }

    /// Every database of this home bound to no generation, durably: what a restore of the home's
    /// data does before that data is in place. The caller holds the starters' lock.
    pub(super) fn revoke_recoveries(&self) -> Result<(), ServerError> {
        let folder = self.dir.join(FOLDER);
        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(why) if why.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(why) => return Err(io("could not read", &folder)(why)),
        };
        for entry in entries {
            let path = entry.map_err(io("could not read", &folder))?.path();
            fs::remove_file(&path).map_err(io("could not revoke", &path))?;
        }
        sync(&folder)
    }
}

fn sync(folder: &Path) -> Result<(), ServerError> {
    File::open(folder)
        .and_then(|folder| folder.sync_all())
        .map_err(io("could not sync", folder))
}

impl FileAuthority {
    fn read(&self) -> Result<Option<String>, ServerError> {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(why) if why.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(why) => return Err(io("could not read", &self.path)(why)),
        };
        let unknown = |why: &dyn std::fmt::Display| {
            ServerError::Io(format!(
                "{} is not a known recovery binding: {why}",
                self.path.display()
            ))
        };
        let bound: Bound = serde_json::from_str(&text).map_err(|why| unknown(&why))?;
        if bound.format != 1 {
            return Err(unknown(&format_args!("format {}", bound.format)));
        }
        if bound.generation.is_empty() {
            return Err(unknown(&"an empty generation"));
        }
        Ok(Some(bound.generation))
    }

    fn remove(&self) -> Result<(), ServerError> {
        match fs::remove_file(&self.path) {
            Ok(()) => sync(self.path.parent().expect("a file in the recovery folder")),
            Err(why) if why.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(why) => Err(io("could not revoke", &self.path)(why)),
        }
    }

    fn write(&self, generation: &str) -> Result<(), ServerError> {
        let folder = self.path.parent().expect("a file in the recovery folder");
        // The one folder, never the home that holds it: a home that is gone is not made again.
        match fs::create_dir(folder) {
            Ok(()) => {}
            Err(why) if why.kind() == std::io::ErrorKind::AlreadyExists && folder.is_dir() => {}
            Err(why) => return Err(io("could not make", folder)(why)),
        }
        let temporary = self
            .path
            .with_extension(format!("json.{}", std::process::id()));
        let text = serde_json::to_vec(&Bound {
            format: 1,
            generation: generation.into(),
        })
        .expect("a binding serializes");
        File::create(&temporary)
            .and_then(|mut file| file.write_all(&text).and_then(|()| file.sync_all()))
            .map_err(io("could not write", &temporary))?;
        fs::rename(&temporary, &self.path).map_err(io("could not publish", &self.path))?;
        sync(folder)
    }
}

impl Authority for FileAuthority {
    fn bound(&self) -> Result<Option<String>, String> {
        self.read().map_err(|why| why.to_string())
    }

    fn revoke(&self) -> Result<(), String> {
        self.remove().map_err(|why| why.to_string())
    }

    fn publish(&self, generation: &str) -> Result<(), String> {
        self.write(generation).map_err(|why| why.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(name: &str) -> (PathBuf, Home) {
        let dir = std::env::temp_dir().join(format!("recovery-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        (dir.clone(), Home::new(&dir, Path::new("/no/surreal")))
    }

    #[test]
    fn a_binding_is_none_until_published_whole_and_none_again_once_revoked() {
        let (dir, home) = home("round");
        let tasks = home.recovery("tasks");
        assert_eq!(tasks.bound(), Ok(None));
        tasks.revoke().unwrap();
        tasks.publish("one").unwrap();
        assert_eq!(tasks.bound(), Ok(Some("one".into())));
        assert_eq!(
            fs::read_to_string(dir.join("recovery/tasks.json")).unwrap(),
            r#"{"format":1,"generation":"one"}"#
        );
        tasks.publish("two").unwrap();
        assert_eq!(home.recovery("tasks").bound(), Ok(Some("two".into())));
        assert_eq!(home.recovery("other").bound(), Ok(None), "each database");
        let left: Vec<_> = fs::read_dir(dir.join("recovery")).unwrap().collect();
        assert_eq!(left.len(), 1, "no temporary file stays");
        tasks.revoke().unwrap();
        assert_eq!(tasks.bound(), Ok(None));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_publish_cut_short_before_its_rename_leaves_the_binding_as_it_was() {
        let (dir, home) = home("cut");
        let tasks = home.recovery("tasks");
        tasks.publish("kept").unwrap();
        fs::write(dir.join("recovery/tasks.json.1"), r#"{"format":1,"gener"#).unwrap();
        assert_eq!(tasks.bound(), Ok(Some("kept".into())));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_file_that_is_not_a_binding_refuses_and_reads_as_neither_bound_nor_unbound() {
        let (dir, home) = home("unknown");
        let tasks = home.recovery("tasks");
        fs::create_dir_all(dir.join("recovery")).unwrap();
        for text in [
            "",
            "{",
            r#"{"format":2,"generation":"g"}"#,
            r#"{"format":1,"generation":""}"#,
            r#"{"format":1}"#,
            r#"{"format":1,"generation":"g","state":"bound"}"#,
            r#"["g"]"#,
        ] {
            fs::write(dir.join("recovery/tasks.json"), text).unwrap();
            let refused = tasks.bound().unwrap_err();
            assert!(
                refused.contains("is not a known recovery binding"),
                "{text}: {refused}"
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_restore_revokes_every_database_and_a_home_with_none_bound_has_nothing_to_revoke() {
        let (dir, home) = home("all");
        home.revoke_recoveries().unwrap();
        assert!(
            !dir.join("recovery").exists(),
            "nothing is made to revoke nothing"
        );
        home.recovery("tasks").publish("one").unwrap();
        home.recovery("scratch").publish("two").unwrap();
        home.revoke_recoveries().unwrap();
        assert_eq!(home.recovery("tasks").bound(), Ok(None));
        assert_eq!(home.recovery("scratch").bound(), Ok(None));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_binding_that_cannot_be_written_or_removed_is_an_error_naming_its_file() {
        let (dir, home) = home("blocked");
        fs::write(dir.join("recovery"), "not a folder").unwrap();
        let tasks = home.recovery("tasks");
        assert!(tasks.publish("one").unwrap_err().contains("recovery"));
        assert!(
            tasks.bound().is_err(),
            "a path through a file reads as an error"
        );
        assert!(home.revoke_recoveries().is_err());
        fs::remove_file(dir.join("recovery")).unwrap();
        fs::create_dir_all(dir.join("recovery/tasks.json")).unwrap();
        assert!(
            tasks.revoke().is_err(),
            "a folder in the file's place is not removed"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[should_panic(expected = "a database is a plain name")]
    fn a_database_that_is_not_a_plain_name_names_no_file() {
        home("plain").1.recovery("../tasks");
    }
}
