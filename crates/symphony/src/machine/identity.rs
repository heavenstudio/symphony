//! A private installation identity in the existing machine folder, atomically published once.
//! It is neither hostname nor authentication. Existing unreadable/corrupt identities never rotate.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const FILE: &str = "machine-id";

pub fn installation_id() -> Result<String, String> {
    in_folder(&super::dir()?).map_err(|error| format!("machine identity unknown: {error}"))
}

fn read(path: &Path) -> io::Result<String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("identity is not a regular file"));
    }
    let mut text = String::new();
    file.take(128).read_to_string(&mut text)?;
    let id = text
        .strip_prefix("1\t")
        .and_then(|text| text.strip_suffix('\n'))
        .filter(|id| id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit()));
    id.map(str::to_owned)
        .ok_or_else(|| io::Error::other("identity has invalid version or contents"))
}

struct Pending(PathBuf);
impl Drop for Pending {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn in_folder(folder: &Path) -> io::Result<String> {
    let path = folder.join(FILE);
    match read(&path) {
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
    read(&path)
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
