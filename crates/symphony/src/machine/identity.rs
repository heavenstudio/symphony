//! A private installation identity in the existing machine folder, atomically published once.
//! It is neither hostname nor authentication. Existing unreadable/corrupt identities never rotate.
//! The board's recorder reads the same file ([`symphony_board::project::machine_id`]), never
//! making it.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use symphony_board::project::{MACHINE_ID as FILE, machine_id as read};

pub fn installation_id() -> Result<String, String> {
    in_folder(&super::dir()?).map_err(|error| format!("machine identity unknown: {error}"))
}

struct Pending(PathBuf);
impl Drop for Pending {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn in_folder(folder: &Path) -> io::Result<String> {
    let path = folder.join(FILE);
    match read(folder) {
        Ok(id) => return Ok(id),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    fs::create_dir_all(folder)?;
    let mut random = [0; 16];
    File::open("/dev/urandom")?.read_exact(&mut random)?;
    let id: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    let pending = Pending(folder.join(format!("{FILE}.{id}.pending")));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&pending.0)?;
    file.write_all(format!("1\t{id}\n").as_bytes())?;
    drop(file);
    // Link a fully written inode without overwriting a winner; all contenders read that winner.
    match fs::hard_link(&pending.0, &path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    read(folder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_tree::Tree;

    #[test]
    fn concurrent_initialization_reads_one_complete_winner_and_cleans_owned_temporaries() {
        let tree = Tree::new();
        let folder = tree.path("machine");
        let barrier = std::sync::Barrier::new(8);
        let ids = std::thread::scope(|scope| {
            (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        in_folder(&folder).unwrap()
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|thread| thread.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(ids.len(), 8);
        assert!(ids.iter().all(|id| id == &ids[0]), "{ids:?}");
        assert_eq!(in_folder(&folder).unwrap(), ids[0]);
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 1);
        let other = in_folder(&tree.path("other-machine")).unwrap();
        assert_ne!(other, ids[0], "independent installations reused identity");
    }

    #[test]
    fn existing_invalid_or_nonregular_identity_fails_without_rotation_or_following_links() {
        let tree = Tree::new();
        for (index, text) in [
            "",
            "1\tshort\n",
            "2\t11111111111111111111111111111111\n",
            "1\t11111111111111111111111111111111\nextra",
        ]
        .iter()
        .enumerate()
        {
            let folder = tree.path(&format!("machine-{index}"));
            fs::create_dir(&folder).unwrap();
            fs::write(folder.join(FILE), text).unwrap();
            assert!(in_folder(&folder).is_err(), "accepted {text:?}");
            assert_eq!(fs::read_to_string(folder.join(FILE)).unwrap(), *text);
            assert_eq!(fs::read_dir(folder).unwrap().count(), 1);
        }
        let folder = tree.path("symlink");
        fs::create_dir(&folder).unwrap();
        let target = tree.write("foreign-id", "1\t11111111111111111111111111111111\n");
        std::os::unix::fs::symlink(&target, folder.join(FILE)).unwrap();
        assert!(in_folder(&folder).is_err());
        assert_eq!(
            fs::read_to_string(target).unwrap(),
            "1\t11111111111111111111111111111111\n"
        );
        let folder = tree.path("directory");
        fs::create_dir_all(folder.join(FILE)).unwrap();
        assert!(in_folder(&folder).is_err());
    }
}
