//! [`super`]: std's channel behind ends of one boxed type, with std's behaviour and errors.

use super::*;
use std::sync::mpsc::RecvError;

#[test]
fn items_come_in_the_order_sent_from_every_clone_and_end_with_their_senders() {
    let (send, receive) = channel::<(u8, String)>();
    let other = send.clone();
    send.send((1, "a".into())).unwrap();
    other.send((2, "b".into())).unwrap();
    send.send((3, "c".into())).unwrap();
    assert_eq!(receive.recv(), Ok((1, "a".into())));
    drop((send, other));
    let rest: Vec<(u8, String)> = (&receive).into_iter().collect();
    assert_eq!(rest, [(2, "b".into()), (3, "c".into())]);
    assert_eq!(receive.recv(), Err(RecvError));
}

#[test]
fn a_send_to_a_receiver_that_went_hands_its_item_back_as_std_says_it() {
    let (send, receive) = channel::<Vec<u8>>();
    drop(receive);
    let refused = send.send(vec![1, 2]).unwrap_err();
    assert_eq!(refused.to_string(), "sending on a closed channel");
    assert_eq!(refused.0, [1, 2]);
}

#[test]
fn a_receive_with_a_deadline_says_whether_it_passed_or_the_senders_went() {
    let (send, receive) = channel::<()>();
    assert_eq!(
        receive.recv_timeout(Duration::ZERO),
        Err(RecvTimeoutError::Timeout)
    );
    send.send(()).unwrap();
    assert_eq!(receive.recv_timeout(Duration::ZERO), Ok(()));
    drop(send);
    assert_eq!(
        receive.recv_timeout(Duration::ZERO),
        Err(RecvTimeoutError::Disconnected)
    );
}

#[test]
fn try_iter_takes_what_was_sent_and_waits_for_nothing() {
    let (send, receive) = channel::<Option<u32>>();
    assert_eq!(
        receive.try_iter().count(),
        0,
        "nothing sent, nothing waited for"
    );
    for item in [Some(1), None, Some(2)] {
        send.send(item).unwrap();
    }
    assert_eq!(
        receive.try_iter().collect::<Vec<_>>(),
        [Some(1), None, Some(2)]
    );
    // Its senders still held, the next taking finds none and returns at once.
    assert_eq!(receive.try_iter().next(), None);
    send.send(Some(3)).unwrap();
    drop(send);
    assert_eq!(receive.try_iter().collect::<Vec<_>>(), [Some(3)]);
}

#[test]
fn a_receiver_taken_whole_iterates_each_item_and_so_does_one_lent() {
    let (send, receive) = channel::<u32>();
    let sender = std::thread::spawn(move || {
        for n in 0..5 {
            send.send(n).unwrap();
        }
    });
    let mut lent = Vec::new();
    for n in &receive {
        lent.push(n);
        if n == 1 {
            break;
        }
    }
    sender.join().unwrap();
    let whole: Vec<u32> = receive.into_iter().collect();
    assert_eq!((lent, whole), (vec![0, 1], vec![2, 3, 4]));
}

#[test]
fn a_bounded_channel_hands_an_item_back_when_full_or_when_its_receiver_went() {
    let (send, receive) = sync_channel::<Vec<u8>>(2);
    send.try_send(vec![1]).unwrap();
    send.try_send(vec![2]).unwrap();
    match send.try_send(vec![3]) {
        Err(TrySendError::Full(item)) => assert_eq!(item, [3]),
        other => panic!("not refused as full: {other:?}"),
    }
    assert_eq!(receive.recv(), Ok(vec![1]));
    send.try_send(vec![4]).unwrap();
    let lent: Vec<Vec<u8>> = receive.try_iter().collect();
    assert_eq!(lent, [vec![2], vec![4]]);
    drop(receive);
    match send.try_send(vec![5]) {
        Err(TrySendError::Disconnected(item)) => assert_eq!(item, [5]),
        other => panic!("not refused as disconnected: {other:?}"),
    }
}

#[test]
fn a_rendezvous_channel_is_full_while_no_receive_waits() {
    let (send, receive) = sync_channel::<u8>(0);
    assert!(matches!(send.try_send(7), Err(TrySendError::Full(7))));
    drop(send);
    assert_eq!(receive.recv(), Err(RecvError));
}

#[test]
fn the_items_a_dropped_receiver_held_are_dropped_with_it() {
    let item = std::sync::Arc::new(());
    let (send, receive) = channel::<std::sync::Arc<()>>();
    send.send(item.clone()).unwrap();
    assert_eq!(std::sync::Arc::strong_count(&item), 2);
    drop(receive);
    assert_eq!(std::sync::Arc::strong_count(&item), 1);
}

#[test]
#[should_panic(expected = "carries only the type its ends were made for")]
fn an_item_of_another_type_is_refused_as_a_mistake() {
    let _: u8 = unboxed(Box::new("not a byte"));
}

/// The module compiled by the build's own rustc as a crate of its own, after it a receiver of
/// `General` (a fn type) returned as one of `received` (line 6) and a sender and a bounded sender
/// of `Static` (a fn type `General` is a subtype of) as ones of `sent` (lines 7 and 8), in
/// `ends.rs`: whether it compiled, and each error the compiler pointed at a line, by its code,
/// that line's file (`ends.rs`, or the whole path of a file outside the scratch folder) and the
/// line. The scratch folder goes whether rustc started or not.
fn compiled(name: &str, received: &str, sent: &str) -> (bool, Vec<(String, String, u64)>) {
    let dir = std::env::temp_dir().join(format!(
        "symphony-app-channel-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let module = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/channel.rs");
    let ends = dir.join("ends.rs");
    let source = format!(
        "#[path = {module:?}]\nmod channel;\nuse channel::{{Receiver, Sender, SyncSender}};\n\
         type Static = fn(&'static str);\ntype General = for<'a> fn(&'a str);\n\
         fn received(end: Receiver<General>) -> Receiver<{received}> {{ end }}\n\
         fn sent(end: Sender<Static>) -> Sender<{sent}> {{ end }}\n\
         fn synced(end: SyncSender<Static>) -> SyncSender<{sent}> {{ end }}\n"
    );
    std::fs::write(&ends, source).unwrap();
    let rustc = std::path::Path::new(env!("CARGO")).with_file_name("rustc");
    let output = std::process::Command::new(&rustc)
        .args(["--edition", "2024", "--crate-type", "lib"])
        .args(["--crate-name", "ends", "--emit", "metadata"])
        .args(["--error-format", "json", "--cap-lints", "allow"])
        .arg("--out-dir")
        .args([&dir, &ends])
        .output();
    std::fs::remove_dir_all(&dir).unwrap();
    let output = output.unwrap_or_else(|error| panic!("{}: {error}", rustc.display()));
    let errors = String::from_utf8_lossy(&output.stderr)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|said| said["level"] == "error")
        .filter_map(|said| {
            let spans = said["spans"].as_array()?;
            let primary = spans.iter().find(|span| span["is_primary"] == true)?;
            let code = said["code"]["code"].as_str().unwrap_or_default();
            let file = std::path::Path::new(primary["file_name"].as_str()?);
            let file = file
                .strip_prefix(&dir)
                .unwrap_or(file)
                .display()
                .to_string();
            Some((code.to_string(), file, primary["line_start"].as_u64()?))
        })
        .collect();
    (output.status.success(), errors)
}

#[test]
fn no_end_is_taken_as_a_channel_of_another_type() {
    // Each end at its own type compiles: the module is read whole, so what is refused below is the
    // other type alone.
    assert_eq!(compiled("own", "General", "Static"), (true, vec![]));
    let mismatched = |line| ("E0308".to_string(), "ends.rs".to_string(), line);
    assert_eq!(
        compiled("other", "Static", "General"),
        (false, vec![mismatched(6), mismatched(7), mismatched(8)]),
        "a receiver taken as one of a supertype, a sender and a bounded sender as ones of a subtype"
    );
}

/// Clippy reads the nearest clippy.toml alone (or `.clippy.toml`), so a crate's own replaces the
/// workspace's: each, this crate's among them, which refuses std's channel past [`channel`] and
/// [`sync_channel`], holds the workspace's settings whole before its own. The board's and the
/// server's lib tests each hold their own crate's as well, so an edit to one alone is tested by
/// its own crate's run; this holds symphony-app's, and finds a crate's file no test of its own
/// holds yet.
#[test]
fn every_crates_clippy_settings_start_with_the_workspaces() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let workspace = std::fs::read_to_string(root.join("clippy.toml")).unwrap();
    let mut held = Vec::new();
    for group in ["crates", "apps"] {
        for package in std::fs::read_dir(root.join(group)).unwrap() {
            let package = package.unwrap().path();
            for name in ["clippy.toml", ".clippy.toml"] {
                let Ok(own) = std::fs::read_to_string(package.join(name)) else {
                    continue;
                };
                let file = format!("{group}/{}/{name}", package.file_name().unwrap().display());
                assert!(
                    own.starts_with(&workspace),
                    "{file} must start with the workspace's clippy.toml whole"
                );
                held.push(file);
            }
        }
    }
    for own in [
        "apps/symphony-server/clippy.toml",
        "crates/symphony/clippy.toml",
        "crates/tasks/clippy.toml",
    ] {
        assert!(
            held.iter().any(|file| file == own),
            "{own} not found: {held:?}"
        );
    }
}
