//! Owned sweep/filter controls and supplied-value evidence for the actual removal decision.

use super::{create, dead_owners, harness_root, reap_dead};
use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::{MetadataExt, PermissionsExt, chown, symlink};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Tmp(PathBuf);

impl Tmp {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "copied-scratch-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path.canonicalize().unwrap()),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                Err(error) => panic!("{}: {error}", path.display()),
            }
        }
    }
    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_real_owned_harness_preserves_collisions_and_never_writes_through_a_link() {
    let tmp = Tmp::new();
    let root = harness_root(&tmp.0, "harness").unwrap();
    assert_eq!(harness_root(&tmp.0, "harness").unwrap(), root);
    assert_eq!(
        fs::metadata(&root).unwrap().uid(),
        fs::metadata(&tmp.0).unwrap().uid()
    );
    let collision = root.join(format!("{}-0", std::process::id()));
    fs::create_dir(&collision).unwrap();
    fs::write(collision.join("held"), "live collision").unwrap();
    let next = AtomicUsize::new(0);
    assert_eq!(
        create(&root, &next).unwrap(),
        root.join(format!("{}-1", std::process::id()))
    );
    assert_eq!(
        fs::read_to_string(collision.join("held")).unwrap(),
        "live collision"
    );
    let linked = tmp.join("linked");
    symlink(&root, &linked).unwrap();
    assert!(harness_root(&tmp.0, "linked").is_err());
    let regular = tmp.join("file");
    fs::write(&regular, "not a directory").unwrap();
    assert!(harness_root(&tmp.0, "file").is_err());
    assert_eq!(fs::read_to_string(regular).unwrap(), "not a directory");
    assert_eq!(
        create(&tmp.join("absent"), &AtomicUsize::new(0))
            .unwrap_err()
            .kind(),
        ErrorKind::NotFound
    );
}

#[test]
fn a_real_foreign_owner_is_refused_by_the_uid_predicate() {
    let tmp = Tmp::new();
    let own_uid = fs::metadata(&tmp.0).unwrap().uid();
    let foreign = if own_uid == 0 {
        let path = tmp.join("foreign");
        fs::create_dir(&path).unwrap();
        chown(&path, Some(1), None).unwrap();
        path
    } else {
        // Only query an existing real foreign directory. mkdir returns AlreadyExists; the
        // actual harness guard must refuse before any scratch creation or sweep there.
        fs::canonicalize("/tmp").unwrap()
    };
    let before = fs::symlink_metadata(&foreign).unwrap();
    assert!(before.is_dir());
    assert_ne!(
        before.uid(),
        own_uid,
        "arm the actual foreign-owner predicate"
    );
    let error = harness_root(
        foreign.parent().unwrap(),
        foreign.file_name().unwrap().to_str().unwrap(),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("is not a directory of uid"),
        "{error}"
    );
    assert_eq!(fs::symlink_metadata(&foreign).unwrap().ino(), before.ino());
}

#[test]
fn supplied_query_values_remove_only_minus_one_with_esrch() {
    let tmp = Tmp::new();
    for (name, result, errno, removed) in [
        ("esrch", -1, Some(libc::ESRCH), true),
        ("success-stale-errno", 0, Some(libc::ESRCH), false),
        ("success", 0, None, false),
        ("permission", -1, Some(libc::EPERM), false),
        ("other-error", -1, Some(libc::EIO), false),
        ("no-errno", -1, None, false),
        ("other-return", 1, Some(libc::ESRCH), false),
    ] {
        let path = tmp.join(name);
        fs::create_dir(&path).unwrap();
        fs::write(path.join("held"), "owned decision input").unwrap();
        let inode = fs::metadata(&path).unwrap().ino();
        dead_owners::remove_observed_dead(&path, result, errno);
        if removed {
            assert_eq!(fs::metadata(&path).unwrap_err().kind(), ErrorKind::NotFound);
        } else {
            assert_eq!(fs::metadata(&path).unwrap().ino(), inode, "{name}");
            assert_eq!(
                fs::read_to_string(path.join("held")).unwrap(),
                "owned decision input",
                "{name}"
            );
        }
    }
}

#[test]
fn the_real_sweep_preserves_live_uncertain_and_ineligible_entries() {
    let tmp = Tmp::new();
    let root = harness_root(&tmp.0, "harness").unwrap();
    let pid = std::process::id();
    let sibling = root.join(format!("other-{pid}-0"));
    fs::create_dir(&sibling).unwrap();
    let live = root.join(format!("mine-{}-2", std::process::id()));
    fs::create_dir(&live).unwrap();
    // PID1 is either live or inaccessible to this uid; neither result authorizes removal.
    let uncertain = root.join("mine-1-7");
    fs::create_dir(&uncertain).unwrap();
    let linked = root.join(format!("mine-{pid}-3"));
    symlink(&sibling, &linked).unwrap();
    let regular = root.join(format!("mine-{pid}-4"));
    fs::write(&regular, "a regular file is never a scratch directory").unwrap();
    for name in [
        "mine-0-1",
        "mine--1-2",
        "mine-2147483648-0",
        "mine-2-x",
        "mine-2-",
        "unrelated",
    ] {
        fs::create_dir(root.join(name)).unwrap();
    }
    dead_owners::reap_dead_scratch(&root, "mine-");
    for kept in [&sibling, &live, &uncertain, &linked] {
        assert!(kept.exists(), "{}", kept.display());
    }
    assert!(
        fs::symlink_metadata(&linked)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read_to_string(regular).unwrap(),
        "a regular file is never a scratch directory"
    );
    for name in [
        "mine-0-1",
        "mine--1-2",
        "mine-2147483648-0",
        "mine-2-x",
        "mine-2-",
        "unrelated",
    ] {
        assert!(root.join(name).is_dir(), "{name}");
    }
    let unprefixed = root.join(format!("{pid}-9"));
    fs::create_dir(&unprefixed).unwrap();
    reap_dead(&root);
    assert!(unprefixed.is_dir(), "the wrapper preserves this live owner");
    assert!(sibling.exists());
    reap_dead(&tmp.join("absent-root"));
}

#[test]
fn unreadable_scratch_preserves_entries_and_recovery_uses_the_same_harness() {
    let tmp = Tmp::new();
    let root = harness_root(&tmp.0, "harness").unwrap();
    let live = root.join(format!("{}-0", std::process::id()));
    fs::create_dir(&live).unwrap();
    fs::write(live.join("held"), "live owner").unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0)).unwrap();
    reap_dead(&root);
    let created = create(&root, &AtomicUsize::new(4));
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    if let Err(error) = created {
        assert_eq!(error.kind(), ErrorKind::PermissionDenied);
    }
    assert_eq!(fs::read_to_string(live.join("held")).unwrap(), "live owner");
    assert_eq!(harness_root(&tmp.0, "harness").unwrap(), root);
}
