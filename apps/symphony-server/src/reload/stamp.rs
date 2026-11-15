//! The installation's stamp (`installed.json`): which sources the installed image was built from,
//! the hash of its bytes, and the file that hash was taken of, so a start of that same file takes the
//! hash as stamped rather than reading the whole image again: the hash is paid once per image, at
//! its install, not once per start. A generation that runs the installed image knows it by the
//! same two ([`RunningImage`]), so a build that leaves its bytes installed is stamped, not handed
//! over to.

use crate::cli::Serve;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const INSTALLED: &str = "installed.json";
/// The installed image, in its install folder.
const IMAGE: &str = "bin/symphony-server";

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

/// The installed server's source, when this process is the installed image, that image holds the
/// bytes the stamp hashed, and the installed client is the stamped one. `lease` is the home's
/// server lock, which this start holds (`reload::setup`).
///
/// The hash decides: an upgrade or a manual replacement with other bytes never passes, and cargo
/// installing the same bytes again does. The image's identity ([`Image`]) only spares the read, as
/// the file the stamp hashed answers unread ([`stamped_source`]). Its ctime is left out on purpose:
/// the after_reload fixture installs a hard link of the build output, and every other test
/// process's link of that output and every rebuild's unlink of its name move the ctime of the
/// installed file without changing a byte.
pub(super) fn installed_source(serve: &Serve, lease: &File) -> Option<String> {
    let home = serve.installed()?;
    installed_image(serve)?;
    let source = stamped_source(home, lease)?;
    // A current server stamp alone cannot certify an older (or manually replaced) client.
    // Transferred generations bypass this initial-install lookup: their parent just published
    // the bundle, even if writing its optional recovery stamp failed.
    (symphony::reload::installed_client_source(home)?
        == symphony::reload::client_fingerprint(&serve.root).ok()?)
    .then_some(source)
}

/// `serve`'s installed image, when this process is it.
fn installed_image(serve: &Serve) -> Option<PathBuf> {
    let binary = serve.installed()?.join(IMAGE);
    (std::env::current_exe().ok()?.canonicalize().ok()? == binary.canonicalize().ok()?)
        .then_some(binary)
}

/// `home`'s stamp, when it reads.
fn read_stamp(home: &Path) -> Option<Installed> {
    serde_json::from_slice(&fs::read(home.join(INSTALLED)).ok()?).ok()
}

/// The installed image this generation runs: the file it started from, by that file's identity
/// then, and the hash of its bytes once taken ([`RunningImage::hash`]). A build that leaves those
/// bytes installed needs no hand-off ([`RunningImage::kept`]).
///
/// The hash decides, not the file: `cargo install --force` writes an unchanged build as a new file
/// of the same size and modification time, so a build that changed nothing still installs another
/// inode, and identity alone would never see it as this image. The identity only spares a read, as
/// in the stamp. It is taken as the start begins, after the exec, so an install between the two
/// would be taken for the image this process runs; the start's own check of the stamp
/// ([`installed_source`]) reads the same file the same way.
#[derive(Debug)]
pub(super) struct RunningImage {
    image: Image,
    hash: Option<String>,
}

impl RunningImage {
    /// `serve`'s installed image, when this process is it.
    pub(super) fn of(serve: &Serve) -> Option<Self> {
        Self::at(&installed_image(serve)?)
    }

    /// The file at `path`, as the image this process started from.
    pub(super) fn at(path: &Path) -> Option<Self> {
        Some(Self {
            image: Image::of(path).ok()?,
            hash: None,
        })
    }

    /// Before a build into `home`: this image's hash, taken once, while `home`'s installed image is
    /// still the file this process started from and stands still while it is read; unread when
    /// the stamp names that file. Once the installed image is another file, this one's bytes are
    /// read nowhere and stay unknown, and no build is kept.
    pub(super) fn hash(&mut self, home: &Path) {
        let path = home.join(IMAGE);
        if self.hash.is_some() || Image::of(&path).ok().as_ref() != Some(&self.image) {
            return;
        }
        let stamp = read_stamp(home);
        let stamped = stamp
            .as_ref()
            .and_then(|stamp| Some((stamp.binary.as_str(), stamp.image.as_ref()?)));
        if let Ok((binary, Some(image))) = image_hash(&path, stamped, binary_fingerprint)
            && image == self.image
        {
            self.hash = Some(binary);
        }
    }

    /// After a build of `source` into `home`: whether its installed image still holds this image's
    /// bytes, the same file (unread) or another of the same bytes (read once). It is then stamped
    /// as built from `source` with that hash, under the lease this generation holds
    /// ([`write_stamp`]), so the next start of it builds nothing: `Some` with how the stamp's write
    /// went.
    pub(super) fn kept(&self, home: &Path, source: &str) -> Option<std::io::Result<()>> {
        let hash = self.hash.as_deref()?;
        let (binary, image) = image_hash(
            &home.join(IMAGE),
            Some((hash, &self.image)),
            binary_fingerprint,
        )
        .ok()?;
        (binary == hash).then(|| {
            write_stamp(
                home,
                &Installed {
                    source: source.into(),
                    binary,
                    image,
                },
            )
        })
    }
}

/// The stamp's source, when `home`'s installed image holds the bytes the stamp hashed. Another
/// file of those bytes (cargo installing them again, an install by hand, a stamp from before
/// identities) is read once and recorded in the stamp, so the next start takes it as stamped
/// rather than reading it again until a reload stamps. Writing here keeps the stamp's rule, as
/// `lease`, the home's server lock, is held ([`write_stamp`]); a file that moved while it was read
/// is not recorded ([`image_hash`]).
fn stamped_source(home: &Path, _lease: &File) -> Option<String> {
    let installed = read_stamp(home)?;
    let stamped = installed
        .image
        .as_ref()
        .map(|image| (installed.binary.as_str(), image));
    let (binary, image) = image_hash(&home.join(IMAGE), stamped, binary_fingerprint).ok()?;
    if binary != installed.binary {
        return None;
    }
    if image.is_none() || image == installed.image {
        return Some(installed.source);
    }
    let restamped = Installed { image, ..installed };
    if let Err(why) = write_stamp(home, &restamped) {
        eprintln!("· Symphony: could not record the installed image in its stamp: {why}");
    }
    Some(restamped.source)
}

pub(super) fn stamp_install(home: &Path, source: &str) -> std::io::Result<()> {
    let (binary, image) = image_hash(&home.join(IMAGE), None, binary_fingerprint)?;
    write_stamp(
        home,
        &Installed {
            source: source.into(),
            binary,
            image,
        },
    )
}

/// `installed` as `home`'s stamp, whole or not at all. Every writer holds the home's server lock:
/// a reload's [`stamp_install`] under the lease its replacement inherits, a kept build's
/// [`RunningImage::kept`] under the lease its generation holds, and a fresh start's
/// [`stamped_source`] under the lease it took. So no two write at once, and nothing writes between
/// a start's read of the stamp and its write-back.
fn write_stamp(home: &Path, installed: &Installed) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(installed).map_err(std::io::Error::other)?;
    let next = home.join(format!("{INSTALLED}.{}", std::process::id()));
    let result = fs::write(&next, bytes).and_then(|()| fs::rename(&next, home.join(INSTALLED)));
    if result.is_err() {
        let _ = fs::remove_file(next);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{
        INSTALLED, Image, Installed, RunningImage, binary_fingerprint, image_hash, stamp_install,
        stamped_source, write_stamp,
    };
    use crate::testing::Tree;
    use std::cell::Cell;
    use std::fs::{self, File};
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

    /// A file written while it is read keeps the hash of what was read, but is not named: its
    /// identity after the write would vouch for bytes nobody hashed.
    #[test]
    fn an_image_written_while_it_is_hashed_is_not_named() {
        let tree = Tree::new();
        let first = binary_fingerprint(&tree.write("first", "first image")).unwrap();
        for written in ["other image", "a longer image than the first"] {
            let image = tree.write("bin/symphony-server", "first image");
            let (binary, named) = image_hash(&image, None, |path| {
                let read = binary_fingerprint(path);
                fs::write(path, written).unwrap();
                read
            })
            .unwrap();
            assert_eq!(binary, first, "{written}");
            assert_eq!(named, None, "{written}");
        }
    }

    /// A start under the home's lease records another file of the stamped bytes in the stamp, which
    /// the next start takes unread, and leaves the stamp alone otherwise.
    #[test]
    fn a_start_records_another_file_of_the_stamped_bytes_and_only_that() {
        let tree = Tree::new();
        let home = tree.path("home");
        let image = tree.write("home/bin/symphony-server", "installed image");
        let lease = File::create(home.join("server.lock")).unwrap();
        lease.try_lock().unwrap();
        let stamp = home.join(INSTALLED);
        let read = || serde_json::from_slice::<Installed>(&fs::read(&stamp).unwrap()).unwrap();
        stamp_install(&home, "source").unwrap();

        // The stamped file: its source, and the stamp as it was.
        let written = fs::read(&stamp).unwrap();
        assert_eq!(stamped_source(&home, &lease).as_deref(), Some("source"));
        assert_eq!(fs::read(&stamp).unwrap(), written);

        // The same bytes installed again, as another file: its source, and that file recorded.
        let reinstall = |text: &str| {
            fs::rename(tree.write("home/bin/next", text), &image).unwrap();
            Image::of(&image).unwrap()
        };
        let again = reinstall("installed image");
        assert_ne!(read().image.as_ref(), Some(&again));
        assert_eq!(stamped_source(&home, &lease).as_deref(), Some("source"));
        let restamped = read();
        assert_eq!(
            (restamped.binary, restamped.image),
            (binary_fingerprint(&image).unwrap(), Some(again.clone()))
        );
        let partial = home.join(format!("{INSTALLED}.{}", std::process::id()));
        assert!(!partial.exists(), "{}", partial.display());

        // A stamp from before identities: recorded the same way.
        let binary = binary_fingerprint(&image).unwrap();
        fs::write(
            &stamp,
            serde_json::json!({"source": "older", "binary": binary}).to_string(),
        )
        .unwrap();
        assert_eq!(stamped_source(&home, &lease).as_deref(), Some("older"));
        assert_eq!(read().image, Some(again));

        // Other bytes: no source, and the stamp kept for the reload that builds.
        reinstall("upgraded image");
        let kept = fs::read(&stamp).unwrap();
        assert_eq!(stamped_source(&home, &lease), None);
        assert_eq!(fs::read(&stamp).unwrap(), kept);
    }

    /// A generation's image is kept by its bytes: a build that leaves them installed, in the same
    /// file or another, is kept and stamped with the hash taken before it; one that installs other
    /// bytes is not, and leaves the stamp for its hand-off.
    #[test]
    fn a_running_image_is_kept_by_its_bytes_and_stamped_with_the_hash_it_took() {
        let tree = Tree::new();
        let home = tree.path("home");
        let image = tree.write("home/bin/symphony-server", "running image");
        let read = || {
            serde_json::from_slice::<Installed>(&fs::read(home.join(INSTALLED)).unwrap()).unwrap()
        };
        let hash = binary_fingerprint(&image).unwrap();
        let mut running = RunningImage::at(&image).unwrap();
        // Nothing is kept before its bytes are known.
        assert!(running.kept(&home, "unknown").is_none());
        assert!(!home.join(INSTALLED).exists());
        running.hash(&home);
        assert_eq!(running.hash.as_deref(), Some(hash.as_str()));

        // Left as it was: kept, the stamp naming that file.
        running.kept(&home, "first").unwrap().unwrap();
        let stamp = read();
        assert_eq!(
            (stamp.source.as_str(), stamp.binary.as_str(), stamp.image),
            ("first", hash.as_str(), Some(Image::of(&image).unwrap()))
        );

        // The same bytes as another file, as `cargo install --force` installs a build that changed
        // nothing: kept, the stamp naming the new file.
        let reinstall = |text: &str| {
            fs::rename(tree.write("home/bin/next", text), &image).unwrap();
            Image::of(&image).unwrap()
        };
        let again = reinstall("running image");
        assert_ne!(Some(&again), read().image.as_ref());
        running.kept(&home, "second").unwrap().unwrap();
        let stamp = read();
        assert_eq!(
            (stamp.source.as_str(), stamp.binary.as_str(), stamp.image),
            ("second", hash.as_str(), Some(again))
        );
        let partial = home.join(format!("{INSTALLED}.{}", std::process::id()));
        assert!(!partial.exists(), "{}", partial.display());

        // Other bytes: not kept, and the stamp as it was.
        reinstall("upgraded image");
        let kept = fs::read(home.join(INSTALLED)).unwrap();
        assert!(running.kept(&home, "third").is_none());
        assert_eq!(fs::read(home.join(INSTALLED)).unwrap(), kept);
        // A build that installs this image's bytes again keeps it again.
        reinstall("running image");
        running.kept(&home, "fourth").unwrap().unwrap();
        assert_eq!(read().source, "fourth");
    }

    /// A generation's bytes are hashed only while its file is installed, and once: unread when the
    /// stamp names that file; never once another file is installed in its place, even of the same
    /// bytes, as its own are then read nowhere. A missing image runs no generation.
    #[test]
    fn a_running_image_is_hashed_only_while_it_is_the_installed_file() {
        let tree = Tree::new();
        let home = tree.path("home");
        let image = tree.write("home/bin/symphony-server", "running image");
        // A stamp naming the file with a hash its bytes do not have: taken as stamped, unread.
        let named = Image::of(&image).unwrap();
        let stamped = Installed {
            source: "stamped source".into(),
            binary: "stamped hash".into(),
            image: Some(named.clone()),
        };
        write_stamp(&home, &stamped).unwrap();
        let mut running = RunningImage::at(&image).unwrap();
        running.hash(&home);
        assert_eq!(running.hash.as_deref(), Some("stamped hash"));
        // Once taken it holds, whatever the stamp says after.
        write_stamp(
            &home,
            &Installed {
                binary: "another hash".into(),
                ..stamped
            },
        )
        .unwrap();
        running.hash(&home);
        assert_eq!(running.hash.as_deref(), Some("stamped hash"));

        // No stamp: the bytes are read.
        let mut unstamped = RunningImage::at(&image).unwrap();
        fs::remove_file(home.join(INSTALLED)).unwrap();
        unstamped.hash(&home);
        assert_eq!(unstamped.hash, Some(binary_fingerprint(&image).unwrap()));

        // Another file installed before the build: its bytes are not this generation's to vouch
        // for.
        let mut replaced = RunningImage::at(&image).unwrap();
        fs::rename(tree.write("home/bin/next", "running image"), &image).unwrap();
        replaced.hash(&home);
        assert_eq!(replaced.hash, None);
        assert!(replaced.kept(&home, "source").is_none());
        assert!(!home.join(INSTALLED).exists());

        assert!(RunningImage::at(&tree.path("home/bin/missing")).is_none());
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
