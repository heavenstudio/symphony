//! Exercise the copied implementations through their actual adapters, with owned files only.

use super::shared::{machine_copy, wrapper_for};
use std::fs;
use std::io::ErrorKind;
use std::io::Write as _;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::panic::catch_unwind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

#[path = "script_board.rs"]
mod board;
#[path = "script_std.rs"]
mod standard;

struct Tmp(PathBuf);

impl Tmp {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "copied-script-{}-{}",
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

    fn source(&self, folder: &str, body: &str) -> PathBuf {
        let path = self.join(folder).join("stand-in.sh");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
        path
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn entries(root: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[test]
fn own_interpreter_files_win_and_missing_or_wrong_type_files_never_use_a_parent() {
    let tmp = Tmp::new();
    let own = tmp.join("own workspace");
    let manifest = own.join("crates/caller");
    let scripts = own.join("tools/test-support");
    let parent = tmp.join("tools/test-support");
    fs::create_dir_all(&manifest).unwrap();
    fs::create_dir_all(&scripts).unwrap();
    fs::create_dir_all(&parent).unwrap();
    assert!(!std::env::current_dir().unwrap().starts_with(&tmp.0));
    for (body, name) in [
        ("#!/bin/sh\n", "script-sh.sh"),
        ("#!/bin/bash\n", "script.sh"),
    ] {
        fs::write(parent.join(name), "hostile parent").unwrap();
        let file = scripts.join(name);
        fs::write(&file, "own interpreter").unwrap();
        assert_eq!(
            wrapper_for(&manifest, body),
            manifest.join("../../tools/test-support").join(name)
        );
        fs::remove_file(&file).unwrap();
        assert!(catch_unwind(|| wrapper_for(&manifest, body)).is_err());
        symlink("missing", &file).unwrap();
        assert!(catch_unwind(|| wrapper_for(&manifest, body)).is_err());
        fs::remove_file(&file).unwrap();
        fs::create_dir(&file).unwrap();
        assert!(catch_unwind(|| wrapper_for(&manifest, body)).is_err());
        fs::remove_dir(&file).unwrap();
        // Presence follows a link; it is not a source-provenance guarantee.
        symlink(parent.join(name), &file).unwrap();
        assert_eq!(
            fs::read(wrapper_for(&manifest, body)).unwrap(),
            b"hostile parent"
        );
        fs::remove_file(&file).unwrap();
        fs::write(&file, "mode zero remains a metadata-visible file").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0)).unwrap();
        assert!(wrapper_for(&manifest, body).is_file());
        if let Err(error) = fs::read(&file) {
            assert_eq!(error.kind(), ErrorKind::PermissionDenied);
        }
    }
    assert!(
        catch_unwind(|| wrapper_for(&tmp.join("absent/crates/caller"), "#!/bin/sh\n")).is_err()
    );
}

type Install = fn(&Path, &str, bool);

#[test]
fn all_three_adapters_preserve_interpreter_argv_zero_stdin_streams_and_nonzero_status() {
    let tmp = Tmp::new();
    for (adapter, install) in [
        ("process-cwd", super::write as Install),
        ("board", board::write as Install),
        ("std", standard::write as Install),
    ] {
        for (interpreter, header) in [("sh", "#!/bin/sh"), ("bash", "#!/bin/bash")] {
            for native in [false, true] {
                let path = tmp.join(&format!("{adapter}-{interpreter}-{native}"));
                let body = format!(
                    "{header}\nIFS= read -r line\nprintf '%s|%s|%s\\n' \"$0\" \"$1\" \"$line\"\nprintf 'owned stderr\\n' >&2\nexit 17\n"
                );
                install(&path, &body, native);
                let tracked = super::tracked_wrapper(&body);
                assert_eq!(
                    fs::read_to_string(&tracked).unwrap().lines().next(),
                    Some(header)
                );
                let alias = tmp.join(&format!("alias-{adapter}-{interpreter}-{native}"));
                symlink(path.file_name().unwrap(), &alias).unwrap();
                let mut command = symphony_board::sys::Command::new(&alias);
                command.arg("argument");
                let mut child = symphony_board::sys::spawn_piped(&mut command).unwrap();
                child
                    .stdin
                    .as_mut()
                    .unwrap()
                    .write_all(b"owned input\n")
                    .unwrap();
                let output = symphony_board::sys::wait_output(child).unwrap();
                assert_eq!(
                    output.status.code(),
                    Some(17),
                    "{adapter} {interpreter} {native}"
                );
                assert_eq!(
                    output.stdout,
                    format!("{}|argument|owned input\n", alias.display()).as_bytes()
                );
                assert_eq!(output.stderr, b"owned stderr\n");
                assert_eq!(
                    fs::symlink_metadata(&path)
                        .unwrap()
                        .file_type()
                        .is_symlink(),
                    !native
                );
            }
        }
    }
}

#[test]
fn rewriting_unlinks_a_native_or_symlink_entry_without_writing_through_shared_bytes() {
    let tmp = Tmp::new();
    let path = tmp.join("rewrite");
    let first = "#!/bin/sh\nprintf first\n";
    let tracked = super::tracked_wrapper(first);
    let bytes = fs::read(&tracked).unwrap();
    super::write(&path, first, true);
    let old = machine_copy::machine_copy(&tracked);
    let old_inode = fs::metadata(&old).unwrap().ino();
    assert_eq!(fs::metadata(&path).unwrap().ino(), old_inode);
    super::write(&path, "#!/bin/bash\nprintf second\n", false);
    assert!(
        fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::metadata(&old).unwrap().ino(), old_inode);
    assert_eq!(fs::read(&tracked).unwrap(), bytes);
    assert_eq!(fs::read(&old).unwrap(), bytes);
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(catch_unwind(|| super::write(&path, first, false)).is_err());
    assert!(path.is_dir());
}

#[test]
fn a_published_copy_keeps_content_and_inode_until_damage_requires_atomic_replacement() {
    let tmp = Tmp::new();
    let root = machine_copy::private(&tmp.join("copies"));
    let first = tmp.source("a", "#!/bin/sh\nexit 0\n");
    let second = tmp.source("b", "#!/bin/sh\nexit 0\n");
    let copy = machine_copy::published(&root, &first);
    let original = fs::metadata(&copy).unwrap().ino();
    assert_eq!(machine_copy::published(&root, &second), copy);
    assert_eq!(fs::metadata(&copy).unwrap().ino(), original);
    assert_eq!(fs::metadata(&copy).unwrap().uid(), machine_copy::uid());
    assert_eq!(fs::metadata(&copy).unwrap().mode() & 0o777, 0o755);
    for damage in ["truncated", "non-executable", "writable", "symlink"] {
        match damage {
            "truncated" => fs::write(&copy, "#!/bin/sh\n").unwrap(),
            "non-executable" => {
                fs::set_permissions(&copy, fs::Permissions::from_mode(0o644)).unwrap()
            }
            "writable" => fs::set_permissions(&copy, fs::Permissions::from_mode(0o777)).unwrap(),
            "symlink" => {
                fs::remove_file(&copy).unwrap();
                symlink(&first, &copy).unwrap();
            }
            _ => unreachable!(),
        }
        let damaged = fs::symlink_metadata(&copy).unwrap().ino();
        assert_eq!(machine_copy::published(&root, &first), copy);
        assert!(machine_copy::holds(&copy, b"#!/bin/sh\nexit 0\n"));
        assert_ne!(
            fs::symlink_metadata(&copy).unwrap().ino(),
            damaged,
            "{damage}"
        );
        assert_eq!(entries(&root).len(), 1);
    }
    fs::write(&second, "#!/bin/sh\nexit 1\n").unwrap();
    let changed = machine_copy::published(&root, &second);
    assert_ne!(changed, copy);
    assert_eq!(fs::read(&copy).unwrap(), b"#!/bin/sh\nexit 0\n");
    assert_eq!(fs::read(&changed).unwrap(), b"#!/bin/sh\nexit 1\n");
    assert_eq!(entries(&root).len(), 2);
}

#[test]
fn concurrent_handouts_prime_once_per_device_inode_before_return_and_reprime_a_replacement() {
    let tmp = Tmp::new();
    let root = machine_copy::private(&tmp.join("copies"));
    let runs = tmp.join("runs");
    let script = tmp.source("source", &format!(
        "#!/bin/sh\nprintf '%s %s\\n' \"$#\" \"${{0##*/}}\" >> '{}'\nprintf x > \"${{0%/*}}/../beside\"\n",
        runs.display()
    ));
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let (root, script, runs, barrier) = (
                root.clone(),
                script.clone(),
                runs.clone(),
                Arc::clone(&barrier),
            );
            std::thread::spawn(move || {
                barrier.wait();
                let copy = machine_copy::copy_into(&root, &script);
                (copy, fs::read_to_string(runs).unwrap())
            })
        })
        .collect();
    let handed: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    let copy = &handed[0].0;
    let line = format!("0 {}\n", copy.file_name().unwrap().to_str().unwrap());
    for (path, before_handout) in &handed {
        assert_eq!(path, copy);
        assert_eq!(before_handout, &line);
    }
    let name = copy.file_name().unwrap().to_str().unwrap().to_owned();
    assert_eq!(
        entries(&root),
        [name.clone()],
        "primer scratch must already be gone"
    );
    let inode = fs::metadata(copy).unwrap().ino();
    assert_eq!(machine_copy::copy_into(&root, &script), *copy);
    assert_eq!(fs::read_to_string(&runs).unwrap(), line);
    fs::set_permissions(copy, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(machine_copy::copy_into(&root, &script), *copy);
    assert_ne!(fs::metadata(copy).unwrap().ino(), inode);
    assert_eq!(fs::read_to_string(&runs).unwrap(), format!("{line}{line}"));
    assert_eq!(entries(&root), [name]);
}

#[test]
fn stale_prime_folders_are_replaced_but_foreign_writable_or_linked_copy_roots_refuse() {
    let tmp = Tmp::new();
    let root = machine_copy::private(&tmp.join("copies"));
    let stale = root.join(format!(".prime.{}.{}", std::process::id(), usize::MAX));
    fs::create_dir_all(stale.join("bin")).unwrap();
    fs::write(stale.join("beside"), "left by a killed primer").unwrap();
    symlink("/absent", stale.join("bin/old")).unwrap();
    assert_eq!(machine_copy::prime_folder(&root, usize::MAX), stale);
    assert_eq!(entries(&stale), ["bin"]);
    assert!(entries(&stale.join("bin")).is_empty());
    let open = tmp.join("open");
    fs::create_dir(&open).unwrap();
    fs::set_permissions(&open, fs::Permissions::from_mode(0o777)).unwrap();
    let linked = tmp.join("linked");
    symlink(&root, &linked).unwrap();
    for refused in [open, linked] {
        let error = catch_unwind(|| machine_copy::private(&refused)).unwrap_err();
        assert!(
            error
                .downcast_ref::<String>()
                .unwrap()
                .contains("that no one else can write")
        );
    }
    assert!(catch_unwind(|| machine_copy::published(&root, &tmp.join("absent-source"))).is_err());
}
