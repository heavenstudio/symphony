//! The installation's stamp (`installed.json`): which sources the installed image was built from,
//! the hash of its bytes, and the file that hash was taken of, so a start of that same file takes the
//! hash as stamped rather than reading the whole image again: the hash is paid once per image, at
//! its install, not once per start.

use crate::cli::Serve;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

const INSTALLED: &str = "installed.json";

#[derive(Serialize, Deserialize)]
struct Installed {
    source: String,
    binary: String,
    /// The file `binary` was taken of. A stamp from before this names none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    image: Option<Image>,
}

/// Which file an image is: its device, inode, size and modification time, read through links as its
/// hash reads it. A write changes the size or the modification time, a replacement the inode. Not
/// its ctime: a link or an unlink moves that (an installation linking the image, a build moving its
/// output's name to a new file) without changing a byte.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Image {
    device: u64,
    inode: u64,
    size: u64,
    modified: (i64, i64),
}

impl Image {
    fn of(path: &Path) -> std::io::Result<Self> {
        let meta = fs::metadata(path)?;
        Ok(Self {
            device: meta.dev(),
            inode: meta.ino(),
            size: meta.len(),
            modified: (meta.mtime(), meta.mtime_nsec()),
        })
    }
}

/// `path`'s hash and the file it was taken of. `stamped` is a stamp's hash and the file it names: that
/// same file answers with the stamp's hash, unread. Any other file is read and hashed (`hash`), and
/// named only if it stood still meanwhile.
fn image_hash(
    path: &Path,
    stamped: Option<(&str, &Image)>,
    hash: impl FnOnce(&Path) -> std::io::Result<String>,
) -> std::io::Result<(String, Option<Image>)> {
    let before = Image::of(path)?;
    if let Some((binary, image)) = stamped
        && *image == before
    {
        return Ok((binary.into(), Some(before)));
    }
    let binary = hash(path)?;
    let after = Image::of(path)?;
    Ok((binary, (after == before).then_some(after)))
}

fn binary_fingerprint(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let size = file.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        hash.update(&buffer[..size]);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// A stamp belongs to executable bytes, not their inode or mtime: cargo may install an unchanged
/// binary again. A manual replacement or an upgrade invalidates the old stamp safely. The bytes are
/// read again only when the file is not the one the stamp hashed ([`image_hash`]).
pub(super) fn installed_source(serve: &Serve) -> Option<String> {
    let home = serve.home.as_ref()?;
    let binary = home.join("bin/symphony-server");
    if std::env::current_exe().ok()?.canonicalize().ok()? != binary.canonicalize().ok()? {
        return None;
    }
    let installed: Installed =
        serde_json::from_slice(&fs::read(home.join(INSTALLED)).ok()?).ok()?;
    let stamped = installed
        .image
        .as_ref()
        .map(|image| (installed.binary.as_str(), image));
    if installed.binary != image_hash(&binary, stamped, binary_fingerprint).ok()?.0 {
        return None;
    }
    // A current server stamp alone cannot certify an older (or manually replaced) client.
    // Transferred generations bypass this initial-install lookup: their parent just published
    // the bundle, even if writing its optional recovery stamp failed.
    (symphony::reload::installed_client_source(home)?
        == symphony::reload::client_fingerprint(&serve.root).ok()?)
    .then_some(installed.source)
}

pub(super) fn stamp_install(home: &Path, source: &str) -> std::io::Result<()> {
    let (binary, image) = image_hash(&home.join("bin/symphony-server"), None, binary_fingerprint)?;
    let installed = Installed {
        source: source.into(),
        binary,
        image,
    };
    let bytes = serde_json::to_vec(&installed).map_err(std::io::Error::other)?;
    let next = home.join(format!("{INSTALLED}.{}", std::process::id()));
    let result = fs::write(&next, bytes).and_then(|()| fs::rename(&next, home.join(INSTALLED)));
    if result.is_err() {
        let _ = fs::remove_file(next);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{INSTALLED, Image, Installed, binary_fingerprint, image_hash, stamp_install};
    use crate::testing::Tree;
    use std::cell::Cell;
    use std::path::Path;

    #[test]
    fn an_image_is_hashed_once_and_again_only_once_it_is_another_file() {
        let tree = Tree::new();
        let image = tree.write("bin/symphony-server", "first image");
        let hashes = Cell::new(0);
        let hash = |path: &Path| {
            hashes.set(hashes.get() + 1);
            binary_fingerprint(path)
        };
        let (first, named) = image_hash(&image, None, hash).unwrap();
        assert_eq!(first, binary_fingerprint(&image).unwrap());
        let named = named.expect("a file that stood still is named");
        assert_eq!(named, Image::of(&image).unwrap());
        assert_eq!(hashes.get(), 1);

        // The stamped file answers with the stamp's hash, unread, again and again.
        for _ in 0..2 {
            assert_eq!(
                image_hash(&image, Some((&first, &named)), hash).unwrap(),
                (first.clone(), Some(named.clone()))
            );
        }
        assert_eq!(hashes.get(), 1);
        // A link made to it and an unlink of another of its names leave it the same file, as an
        // installation linking the image and a build moving its output's name to a new file do.
        let other_name = tree.path("target/symphony-server");
        std::fs::create_dir_all(other_name.parent().unwrap()).unwrap();
        std::fs::hard_link(&image, &other_name).unwrap();
        std::fs::remove_file(&other_name).unwrap();
        assert_eq!(
            image_hash(&image, Some((&first, &named)), hash).unwrap().0,
            first
        );
        assert_eq!(hashes.get(), 1);

        // Written in place at the same length: its modification time moved, so it is read again,
        // and answers with its new bytes' hash, never the stamp's.
        std::fs::write(&image, "other image").unwrap();
        let (rewritten, renamed) = image_hash(&image, Some((&first, &named)), hash).unwrap();
        assert_eq!(hashes.get(), 2);
        assert_eq!(rewritten, binary_fingerprint(&image).unwrap());
        assert_ne!(rewritten, first);
        assert_ne!(renamed, Some(named.clone()));

        // Replaced whole by the first bytes again (an unchanged install): another file, read again,
        // and its hash is the stamp's, so the stamp still holds.
        let next = tree.write("bin/next", "first image");
        std::fs::rename(&next, &image).unwrap();
        let (replaced, renamed) = image_hash(&image, Some((&first, &named)), hash).unwrap();
        assert_eq!(hashes.get(), 3);
        assert_eq!(replaced, first);
        assert_ne!(renamed, Some(named));
        let missing = image_hash(&tree.path("bin/missing"), None, hash);
        assert!(missing.is_err(), "{missing:?}");
        assert_eq!(hashes.get(), 3);
    }

    #[test]
    fn an_install_stamp_names_the_image_it_hashed_and_an_older_stamp_still_reads() {
        let tree = Tree::new();
        let home = tree.path("home");
        let image = tree.write("home/bin/symphony-server", "installed image");
        stamp_install(&home, "source fingerprint").unwrap();
        let written = std::fs::read(home.join(INSTALLED)).unwrap();
        let stamp: Installed = serde_json::from_slice(&written).unwrap();
        assert_eq!(stamp.source, "source fingerprint");
        assert_eq!(stamp.binary, binary_fingerprint(&image).unwrap());
        assert_eq!(stamp.image, Some(Image::of(&image).unwrap()));
        let partial = home.join(format!("{INSTALLED}.{}", std::process::id()));
        assert!(!partial.exists(), "{}", partial.display());
        // A stamp from before this names no image: it still reads, so a start hashes the image as
        // it did (`image_hash` with nothing stamped), and one written without an image omits it.
        let older: Installed = serde_json::from_str(r#"{"source":"s","binary":"b"}"#).unwrap();
        assert_eq!((older.source.as_str(), older.binary.as_str()), ("s", "b"));
        assert_eq!(older.image, None);
        let without = serde_json::to_value(Installed {
            image: None,
            ..older
        })
        .unwrap();
        assert_eq!(without, serde_json::json!({"source":"s","binary":"b"}));
    }
}
