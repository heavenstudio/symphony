//! Inventory boundary cases use real metadata, without launching native status processes.

use super::*;
use std::fs::{File, FileTimes};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Tree(PathBuf);

impl Tree {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "sf-inv-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("admin")).unwrap();
        let tree = Self(path.join("tree"));
        fs::create_dir_all(tree.0.join("sim/deep")).unwrap();
        fs::write(
            tree.0.join(".git"),
            format!("gitdir: {}\n", path.join("admin").display()),
        )
        .unwrap();
        fs::write(tree.0.join("sim/deep/source"), "original").unwrap();
        tree
    }

    fn capture(&self, ignored: &[&str], tracked: &[&str]) -> Result<Inventory, Unproven> {
        let mut capture = Capture {
            root: &self.0,
            began: SystemTime::now(),
            ignored: ignored.iter().map(PathBuf::from).collect(),
            entries: BTreeMap::new(),
        };
        capture.walk(Path::new(""))?;
        for path in tracked {
            capture.tracked(Path::new(path))?;
        }
        Ok(Inventory {
            version: VERSION,
            entries: capture.entries.into_iter().collect(),
        })
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.0.parent().unwrap());
    }
}

#[test]
fn native_index_records_require_lossless_complete_stage_zero_paths() {
    let oid = "1".repeat(40);
    let record = |mode: &str, stage: &str, name: &str| format!("{mode} {oid} {stage}\t{name}\0");
    let bytes = format!(
        "{}{}{}",
        record("100644", "0", "sim/a\tb"),
        record("100755", "0", "program"),
        record("120000", "0", "link")
    );
    assert_eq!(tracked_paths(bytes.as_bytes()).unwrap().len(), 3);
    assert!(tracked_paths(b"").unwrap().is_empty());
    for bad in [
        record("160000", "0", "module"),
        record("100644", "1", "conflict"),
        record("100644", "0", "../escape"),
        record("100644", "0", "/absolute"),
        record("100644", "0", ""),
        format!(
            "{}{}",
            record("100644", "0", "same"),
            record("100644", "0", "same")
        ),
        "100644 short 0\tfile\0".into(),
    ] {
        assert_eq!(
            tracked_paths(bad.as_bytes()),
            Err(Unproven::Unknown),
            "{bad:?}"
        );
    }
    // Each control is otherwise well formed, so another parser rejection cannot mask the
    // missing terminator, lossy decoding or byte-bound regression it is meant to catch.
    let valid = record("100644", "0", "file");
    assert_eq!(
        tracked_paths(&valid.as_bytes()[..valid.len() - 1]),
        Err(Unproven::Unknown)
    );
    let mut non_utf8 = record("100644", "0", "name").into_bytes();
    non_utf8.insert(non_utf8.len() - 1, 0xff);
    assert_eq!(tracked_paths(&non_utf8), Err(Unproven::Unknown));
    let oversized = record("100644", "0", &"p".repeat(MAX_LIST_BYTES));
    assert_eq!(tracked_paths(oversized.as_bytes()), Err(Unproven::Unknown));
    let overflow: String = (0..=MAX_ENTRIES)
        .map(|n| record("100644", "0", &format!("p{n}")))
        .collect();
    assert_eq!(tracked_paths(overflow.as_bytes()), Err(Unproven::Unknown));
}

#[test]
fn nested_create_delete_rename_mode_and_restored_mtime_invalidate_the_inventory() {
    for change in ["create", "delete", "rename", "mode", "content"] {
        let tree = Tree::new();
        let proof = tree.capture(&[], &[]).unwrap();
        assert!(proof.matches(&tree.0), "unchanged {change}");
        let file = tree.0.join("sim/deep/source");
        let before = fs::metadata(&file).unwrap();
        match change {
            "create" => fs::write(tree.0.join("sim/deep/new"), "new source").unwrap(),
            "delete" => fs::remove_file(&file).unwrap(),
            "rename" => fs::rename(&file, tree.0.join("sim/deep/renamed")).unwrap(),
            "mode" => fs::set_permissions(
                &file,
                fs::Permissions::from_mode(before.permissions().mode() ^ 0o111),
            )
            .unwrap(),
            _ => {
                fs::write(&file, "modified").unwrap();
                File::open(&file)
                    .unwrap()
                    .set_times(FileTimes::new().set_modified(before.modified().unwrap()))
                    .unwrap();
                assert_eq!(fs::metadata(&file).unwrap().len(), before.len());
                assert_eq!(
                    fs::metadata(&file).unwrap().modified().unwrap(),
                    before.modified().unwrap()
                );
            }
        }
        assert!(!proof.matches(&tree.0), "missed {change}");
    }
}

#[test]
fn output_cuts_never_hide_tracked_files_or_follow_a_symlink_parent() {
    let tree = Tree::new();
    for path in [".target", ".godot"] {
        fs::create_dir_all(tree.0.join(path)).unwrap();
        fs::write(tree.0.join(path).join("tracked"), "original").unwrap();
        fs::write(tree.0.join(path).join("cache"), "generated").unwrap();
    }
    let proof = tree
        .capture(&[".godot"], &[".target/tracked", ".godot/tracked"])
        .unwrap();
    assert!(proof.matches(&tree.0));
    assert!(
        proof
            .entries
            .iter()
            .all(|(path, _)| !path.ends_with("cache"))
    );
    fs::write(tree.0.join(".godot/tracked"), "modified").unwrap();
    assert!(!proof.matches(&tree.0));
    let link = tree.0.join("link");
    symlink(tree.0.join("sim"), &link).unwrap();
    assert!(matches!(
        tree.capture(&[], &["link/deep/source"]),
        Err(Unproven::Changed)
    ));
    let proof = tree.capture(&[], &[]).unwrap();
    fs::remove_file(&link).unwrap();
    symlink("missing", &link).unwrap();
    assert!(!proof.matches(&tree.0));
}

#[test]
fn collection_changes_unsupported_admin_and_entry_bounds_decline_proof() {
    let tree = Tree::new();
    let mut capture = Capture {
        root: &tree.0,
        began: SystemTime::UNIX_EPOCH + Duration::from_secs(1),
        ignored: BTreeSet::new(),
        entries: BTreeMap::new(),
    };
    assert_eq!(capture.walk(Path::new("")), Err(Unproven::Changed));
    assert_eq!(
        capture.metadata(Path::new("missing")).unwrap_err(),
        Unproven::Changed
    );
    let proof = tree.capture(&[], &[]).unwrap();
    for marker in ["sharedindex.probe", ".sf-sweep-lock-probe"] {
        let path = tree.0.parent().unwrap().join("admin").join(marker);
        fs::write(&path, "private marker").unwrap();
        assert!(!proof.matches(&tree.0), "{marker}");
        fs::remove_file(path).unwrap();
        assert!(proof.matches(&tree.0));
    }
    fs::create_dir(tree.0.join("nested")).unwrap();
    fs::create_dir(tree.0.join("nested/.git")).unwrap();
    assert!(matches!(tree.capture(&[], &[]), Err(Unproven::Unknown)));
    let mut oversized = proof;
    oversized.version += 1;
    assert!(!oversized.matches(&tree.0));
    oversized.version = VERSION;
    let entry = oversized.entries[0].clone();
    oversized.entries = vec![entry; MAX_ENTRIES + 1];
    assert!(!oversized.matches(&tree.0));
}

#[test]
fn unreadable_directories_special_files_and_capture_overflow_are_unproven() {
    let tree = Tree::new();
    let directory = tree.0.join("sim/deep");
    let permissions = fs::metadata(&directory).unwrap().permissions();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o000)).unwrap();
    let result = tree.capture(&[], &[]);
    fs::set_permissions(&directory, permissions).unwrap();
    assert!(matches!(result, Err(Unproven::Unknown)), "{result:?}");
    let _socket = std::os::unix::net::UnixListener::bind(tree.0.join("sim/socket")).unwrap();
    assert!(matches!(tree.capture(&[], &[]), Err(Unproven::Unknown)));
    let value = entry_stamp(&tree.0).unwrap();
    let mut capture = Capture {
        root: &tree.0,
        began: SystemTime::now(),
        ignored: BTreeSet::new(),
        entries: (0..MAX_ENTRIES)
            .map(|number| (PathBuf::from(format!("entry-{number}")), value.clone()))
            .collect(),
    };
    assert_eq!(
        capture.record(Path::new("one-more"), fs::metadata(&tree.0).unwrap()),
        Err(Unproven::Unknown)
    );
}

#[test]
fn source_sized_inventory_roundtrip_measures_the_actual_metadata_validation_cost() {
    let tree = Tree::new();
    for directory in 0..512 {
        let path = tree.0.join(format!("sources/{directory}"));
        fs::create_dir_all(&path).unwrap();
        for file in 0..8 {
            fs::write(path.join(file.to_string()), "source").unwrap();
        }
    }
    let began = std::time::Instant::now();
    let proof = tree.capture(&[], &[]).unwrap();
    let cold = began.elapsed();
    assert!(
        proof.entries.len() >= 4608,
        "{} captured entries",
        proof.entries.len()
    );
    let bytes = serde_json::to_vec(&proof).unwrap();
    let began = std::time::Instant::now();
    for _ in 0..4 {
        let saved: Inventory = serde_json::from_slice(&bytes).unwrap();
        assert!(saved.matches(&tree.0));
    }
    eprintln!(
        "METADATA ONLY: {} entries, {} bytes, collect {cold:?}, deserialize+validate mean {:?}",
        proof.entries.len(),
        bytes.len(),
        began.elapsed() / 4
    );
}
