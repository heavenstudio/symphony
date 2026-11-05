//! A fixture script's machine-wide copy: one per content, kept once published, replaced only when
//! damaged, and run through a link that keeps the link's path as `$0`.

use super::super::Scratch;
use super::*;
use std::sync::{Arc, Barrier};

fn source(scratch: &Scratch, folder: &str, text: &str) -> PathBuf {
    let path = scratch.path.join(folder).join("stand-in.sh");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, text).unwrap();
    path
}

fn inode(path: &Path) -> u64 {
    fs::metadata(path).unwrap().ino()
}

/// What `root` holds besides its copies' names, sorted: no partial is ever left.
fn entries(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[test]
fn one_content_has_one_copy_that_every_checkout_links() {
    let scratch = Scratch::new("machine-copy-shared");
    let root = private(&scratch.path.join("copies"));
    // Two checkouts' sources: the same bytes under different paths, as two fresh trees hold them.
    let first = source(&scratch, "checkout-a", "#!/bin/sh\nexit 0\n");
    let second = source(&scratch, "checkout-b", "#!/bin/sh\nexit 0\n");
    let copy = copy_into(&root, &first);
    let published = inode(&copy);
    assert_eq!(copy_into(&root, &second), copy);
    assert_eq!(copy_into(&root, &first), copy);
    // Kept, never rewritten: its first exec's assessment is the one every later link shares.
    assert_eq!(inode(&copy), published);
    assert_eq!(fs::read(&copy).unwrap(), b"#!/bin/sh\nexit 0\n");
    assert_eq!(fs::metadata(&copy).unwrap().mode() & 0o777, 0o755);
    assert_ne!(inode(&copy), inode(&first));
    assert_eq!(
        entries(&root),
        [copy.file_name().unwrap().to_str().unwrap()]
    );
}

#[test]
fn a_changed_script_gets_its_own_copy_and_leaves_the_other_whole() {
    let scratch = Scratch::new("machine-copy-changed");
    let root = private(&scratch.path.join("copies"));
    let script = source(&scratch, "checkout", "#!/bin/sh\nexit 0\n");
    let old = copy_into(&root, &script);
    let old_inode = inode(&old);
    fs::write(&script, "#!/bin/sh\nexit 1\n").unwrap();
    let new = copy_into(&root, &script);
    assert_ne!(new, old);
    assert!(new.to_str().unwrap().ends_with("-stand-in.sh"), "{new:?}");
    assert_eq!(fs::read(&new).unwrap(), b"#!/bin/sh\nexit 1\n");
    assert_eq!(fs::read(&old).unwrap(), b"#!/bin/sh\nexit 0\n");
    assert_eq!(inode(&old), old_inode);
    assert_eq!(entries(&root).len(), 2);
}

/// Something done to a published copy, through its path.
type Damage = fn(&Path);

#[test]
fn a_damaged_copy_is_replaced_whole() {
    let damages: [(&str, Damage); 3] = [
        ("cut short", |copy| fs::write(copy, "#!/bin/sh\n").unwrap()),
        ("not executable", |copy| {
            fs::set_permissions(copy, fs::Permissions::from_mode(0o644)).unwrap();
        }),
        ("writable by others", |copy| {
            fs::set_permissions(copy, fs::Permissions::from_mode(0o777)).unwrap();
        }),
    ];
    for (damage, apply) in damages {
        let scratch = Scratch::new("machine-copy-damaged");
        let root = private(&scratch.path.join("copies"));
        let script = source(&scratch, "checkout", "#!/bin/sh\nexit 0\n");
        let copy = copy_into(&root, &script);
        apply(&copy);
        let damaged = inode(&copy);
        assert_eq!(copy_into(&root, &script), copy, "{damage}");
        assert!(holds(&copy, b"#!/bin/sh\nexit 0\n"), "{damage}");
        assert_ne!(
            inode(&copy),
            damaged,
            "{damage}: replaced, never written through"
        );
        assert_eq!(entries(&root).len(), 1, "{damage}");
    }
}

#[test]
fn concurrent_preparations_all_end_with_the_one_correct_copy() {
    let scratch = Scratch::new("machine-copy-concurrent");
    let root = private(&scratch.path.join("copies"));
    let script = source(&scratch, "checkout", "#!/bin/sh\nexit 0\n");
    let start = Arc::new(Barrier::new(8));
    let preparations: Vec<_> = (0..8)
        .map(|_| {
            let (root, script, start) = (root.clone(), script.clone(), Arc::clone(&start));
            std::thread::spawn(move || {
                start.wait();
                let copy = copy_into(&root, &script);
                (inode(&copy), copy)
            })
        })
        .collect();
    let copies: Vec<(u64, PathBuf)> = preparations
        .into_iter()
        .map(|preparation| preparation.join().unwrap())
        .collect();
    assert!(copies.iter().all(|copy| *copy == copies[0]), "{copies:?}");
    assert!(holds(&copies[0].1, b"#!/bin/sh\nexit 0\n"));
    assert_eq!(entries(&root).len(), 1, "{:?}", entries(&root));
}

#[test]
fn a_folder_another_user_could_write_or_a_link_is_refused() {
    let scratch = Scratch::new("machine-copy-private");
    let open = scratch.path.join("open");
    fs::create_dir(&open).unwrap();
    fs::set_permissions(&open, fs::Permissions::from_mode(0o777)).unwrap();
    let linked = scratch.path.join("linked");
    std::os::unix::fs::symlink(private(&scratch.path.join("mine")), &linked).unwrap();
    for refused in [open, linked] {
        let failure = std::panic::catch_unwind(|| private(&refused))
            .expect_err("an unsafe copies folder must be refused");
        let message = failure.downcast_ref::<String>().unwrap();
        assert!(message.contains("that no one else can write"), "{message}");
    }
}

#[test]
fn a_copy_runs_through_a_link_that_keeps_its_own_path_as_dollar_zero() {
    let scratch = Scratch::new("machine-copy-runs");
    let script = source(&scratch, "checkout", "#!/bin/sh\nprintf '%s' \"$0\"\n");
    // The machine's own folder: a copy there is assessed once per machine, never per run.
    let link = scratch.path.join("bin/stand-in");
    fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(machine_copy(&script), &link).unwrap();
    // No deadline: this may be the content's first exec on this machine.
    let ran = symphony_board::sys::output(&mut symphony_board::sys::Command::new(&link)).unwrap();
    assert!(ran.status.success(), "{ran:?}");
    assert_eq!(
        String::from_utf8(ran.stdout).unwrap(),
        link.to_str().unwrap()
    );
}
