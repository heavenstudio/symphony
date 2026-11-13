//! Filesystem identity must authorize the exact path published by the PID reader.

use super::*;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::symlink;

#[test]
fn caret_names_require_the_same_file_identity_and_preserve_valid_neighbors() {
    let root = std::env::temp_dir().join(format!("symphony-cwd-name-{}", std::process::id()));
    let literal = root.join("name^A");
    let control = root.join("name\x01");
    std::fs::create_dir_all(&literal).unwrap();
    std::fs::create_dir_all(&control).unwrap();
    let literal = literal.canonicalize().unwrap();
    let control = control.canonicalize().unwrap();
    let identity = std::fs::metadata(&literal).unwrap();
    let other = std::fs::metadata(&control).unwrap();
    assert_ne!(identity.ino(), other.ino());
    let name = format!("n{}\n", literal.display());
    let device = format!("D0x{:x}\n", identity.dev());
    let inode = format!("i{}\n", identity.ino());
    let fields = format!("{name}{device}{inode}");
    for valid in [&fields, &format!("{inode}{device}{name}")] {
        let scan = parse(format!("p1\nfcwd\n{valid}").as_bytes(), true);
        assert_eq!(
            scan,
            PidCwds {
                listed: vec![(1, literal.clone())],
                complete: true
            }
        );
    }
    for invalid in [
        name.clone(),
        format!("{name}{device}"),
        format!("{name}{inode}"),
        format!("{name}{device}i{}\n", other.ino()),
        format!("{name}D0x{:x}\n{inode}", identity.dev() + 1),
        format!("{fields}{device}"),
        format!("{fields}{inode}"),
        format!("{name}D0xgg\n{inode}"),
        format!("{name}D0x\n{inode}"),
        format!("{name}{device}i\n"),
        format!("{name}{device}i+1\n"),
        format!("{name}{device}i18446744073709551616\n"),
        format!("{name}D0x10000000000000000\n{inode}"),
        format!("n{}/missing^A\n{device}{inode}", root.display()),
    ] {
        let rows = format!("p9\nfcwd\nn/before\np1\nfcwd\n{invalid}p2\nfcwd\nn/after\n");
        assert_eq!(
            parse(rows.as_bytes(), true),
            PidCwds {
                listed: vec![(9, "/before".into()), (2, "/after".into())],
                complete: false,
            },
            "{invalid:?}"
        );
    }
    // A prior valid row's identity cannot authorize the next ambiguous file's name.
    let rows = format!("p1\nfcwd\n{device}{inode}n/plain\nfcwd\n{name}");
    assert_eq!(
        parse(rows.as_bytes(), true),
        PidCwds {
            listed: vec![(1, "/plain".into())],
            complete: false,
        }
    );
    let file = root.join("file^A");
    std::fs::write(&file, "not a directory").unwrap();
    let identity = std::fs::metadata(&file).unwrap();
    let rows = format!(
        "p1\nfcwd\nD0x{:x}\ni{}\nn{}\n",
        identity.dev(),
        identity.ino(),
        file.display()
    );
    assert_eq!(
        parse(rows.as_bytes(), true),
        PidCwds {
            listed: vec![],
            complete: false
        }
    );
    std::fs::remove_dir_all(root).unwrap();
}

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "symphony-cwd-canonical-{}-{name}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    root.canonicalize().unwrap()
}

fn row(candidate: &Path, actual: &Path) -> String {
    let identity = std::fs::metadata(actual).unwrap();
    // Only the test producer's name field is escaped, independently of expected filesystem paths.
    let name = candidate.to_str().unwrap().replace('\\', "\\\\");
    format!(
        "p7\nfcwd\nD0x{:x}\ni{}\nn{name}\n",
        identity.dev(),
        identity.ino()
    )
}

fn surrounded(fields: &str) -> String {
    format!("p1\nfcwd\nn/before\n{fields}p9\nfcwd\nn/after\n")
}

fn assert_unresolved(fields: &str) {
    assert_eq!(
        parse(surrounded(fields).as_bytes(), true),
        PidCwds {
            listed: vec![(1, "/before".into()), (9, "/after".into())],
            complete: false,
        }
    );
}

#[test]
fn symlink_rows_publish_the_verified_canonical_directory_without_decoding_again() {
    let root = scratch("links");
    for placement in ["leaf", "ancestor"] {
        let actual_parent = root.join(format!("{placement}-physical\x01\\x41"));
        let alias_parent = root.join(format!("{placement}^A"));
        let component = if placement == "leaf" { "" } else { "nested" };
        let actual = actual_parent.join(component);
        std::fs::create_dir_all(&actual).unwrap();
        symlink(&actual_parent, &alias_parent).unwrap();
        let alias = alias_parent.join(component);
        assert_ne!(alias, actual);
        assert_eq!(alias.canonicalize().unwrap(), actual);
        let fields = row(&alias, &actual);
        assert_eq!(
            parse(surrounded(&fields).as_bytes(), true),
            PidCwds {
                listed: vec![
                    (1, "/before".into()),
                    (7, actual.clone()),
                    (9, "/after".into())
                ],
                complete: true,
            },
            "{placement}"
        );

        let unrelated = root.join(format!("{placement}-unrelated"));
        std::fs::create_dir_all(&unrelated).unwrap();
        assert_ne!(
            std::fs::metadata(&actual).unwrap().ino(),
            std::fs::metadata(&unrelated).unwrap().ino()
        );
        assert_unresolved(&row(&alias, &unrelated));
        assert_unresolved(&format!("p7\nfcwd\nn{}\n", alias.display()));
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn vanished_canonical_identity_preserves_neighbors_and_uncertainty() {
    let root = scratch("unusable");
    let actual = root.join("vanishing");
    let alias = root.join("vanishing^A");
    std::fs::create_dir_all(&actual).unwrap();
    symlink(&actual, &alias).unwrap();
    let fields = row(&alias, &actual);
    std::fs::remove_dir(&actual).unwrap();
    assert_unresolved(&fields);

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn canonical_filesystem_bytes_are_validated_without_printable_name_decoding() {
    for path in [
        "/work/control\x01",
        "/work/literal\\x00",
        "/work/\\x2e\\x2e",
    ] {
        assert!(Cwd::usable_path(Path::new(path)), "{path:?}");
    }
    for path in ["relative/path", "/work/../other", "/work/\0invalid"] {
        assert!(!Cwd::usable_path(Path::new(path)), "{path:?}");
    }
    // APFS rejects these names at creation. The value boundary is still fail-closed on bytes
    // a different filesystem may return, without assuming this host can create such a directory.
    let opaque = PathBuf::from(OsString::from_vec(b"/work/opaque-\xff".to_vec()));
    assert!(!Cwd::usable_path(&opaque), "{opaque:?}");
}
