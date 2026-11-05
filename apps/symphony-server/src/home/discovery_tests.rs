//! Port hints from a home's owned log and the listener's hint file, read bounded, with anything
//! unreadable or unannounced kept as unknown history for the recovery scan.

use super::*;
use crate::home::{FOLDER, LEGACY_FOLDER};
use std::fs;

#[test]
fn discovery_hints_include_the_owned_announcement_when_the_port_hint_lies() {
    let tree = crate::testing::Tree::new();
    let root = tree.path("root");
    let home = Home::new(
        &tree.path(FOLDER),
        Path::new("/bin/symphony-server"),
        &root,
        0,
        vec![],
    );
    assert_eq!(home.discovery_ports(), (vec![], false));
    for (bytes, expected) in [
        (b"41001\n".to_vec(), (vec![41001], false)),
        (b"0\n".to_vec(), (vec![], true)),
        (b"not a port".to_vec(), (vec![], true)),
        (b"\xff41001".to_vec(), (vec![], true)),
        (
            [vec![b'x'; 128 * 1024], b"41001".to_vec()].concat(),
            (vec![], true),
        ),
    ] {
        tree.write("root/artifacts/docs-app.port", "");
        fs::write(root.join(crate::front::PORT_FILE), bytes).unwrap();
        assert_eq!(home.discovery_ports(), expected);
    }
    tree.write("root/artifacts/docs-app.port", "41001\n");
    tree.write(&format!("{FOLDER}/{LOG}"), "→ Symphony at http://127.0.0.1:41002/  (pid 41)\n→ Symphony at http://127.0.0.1:41003/  (pid 42)\n");
    tree.write(
        &format!("{LEGACY_FOLDER}/{LOG}"),
        "→ Symphony at http://127.0.0.1:41004/  (pid 43)\n",
    );
    assert_eq!(home.discovery_ports(), (vec![41001, 41003, 41004], false));
    tree.write(&format!("{FOLDER}/{LOG}"), "→ Symphony at http://127.0.0.1:41003/  (pid 42)\n→ Symphony at http://127.0.0.1:broken/  (pid 44)\n");
    assert_eq!(home.discovery_ports(), (vec![41001, 41004], true));
}

#[test]
fn discovery_log_reads_are_bounded_lossy_and_keep_unknown_history_for_recovery() {
    let tree = crate::testing::Tree::new();
    let home = Home::new(
        &tree.path(FOLDER),
        Path::new("/bin/symphony-server"),
        &tree.path("root"),
        0,
        vec![],
    );
    let path = tree.path(&format!("{FOLDER}/{LOG}"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut bytes = vec![b'x'; 128 * 1024];
    bytes.extend_from_slice(b"\n\xff\n");
    bytes.extend_from_slice("→ Symphony at http://127.0.0.1:41011/  (pid 41)\n".as_bytes());
    fs::write(&path, &bytes).unwrap();
    let (length, tail) = read_tail(&path, DISCOVERY_LOG_BYTES).unwrap();
    assert_eq!(length, bytes.len() as u64);
    assert_eq!(tail.len(), 16386, "one invalid byte is decoded lossily");
    assert_eq!(home.discovery_ports(), (vec![41011], false));
    tree.write(
        &format!("{FOLDER}/{LOG}"),
        &format!(
            "→ Symphony at http://127.0.0.1:41011/  (pid 41)\n{}",
            "x".repeat(16384)
        ),
    );
    assert_eq!(home.discovery_ports(), (vec![], true));
}

#[test]
fn discovery_nonregular_logs_preserve_recovery_without_waiting_for_a_writer() {
    for kind in ["directory", "fifo", "symlink"] {
        let tree = crate::testing::Tree::new();
        let path = tree.path(&format!("{FOLDER}/{LOG}"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        match kind {
            "directory" => fs::create_dir(&path).unwrap(),
            "fifo" => drop(crate::testing::fifo(&path)),
            "symlink" => {
                tree.write(
                    "real.log",
                    "→ Symphony at http://127.0.0.1:41001/  (pid 41)\n",
                );
                std::os::unix::fs::symlink(tree.path("real.log"), &path).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(read_tail(&path, DISCOVERY_LOG_BYTES).is_err(), "{kind}");
        let home = Home::new(
            &tree.path(FOLDER),
            Path::new("/bin/symphony-server"),
            &tree.path("root"),
            0,
            vec![],
        );
        assert_eq!(home.discovery_ports(), (vec![], true), "{kind}");
        let hint = tree.path("root/artifacts/docs-app.port");
        fs::create_dir_all(hint.parent().unwrap()).unwrap();
        fs::rename(&path, &hint).unwrap();
        assert!(
            !path.exists(),
            "the log must not supply the recovery evidence"
        );
        assert!(read_tail(&hint, PORT_HINT_BYTES).is_err(), "{kind}");
        assert_eq!(home.discovery_ports(), (vec![], true), "{kind} port hint");
    }
}
