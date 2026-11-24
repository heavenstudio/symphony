//! One hand to a fake program in a temporary folder: the message on its stdin alone, its status
//! read as the format says, its words repeated with the address and the link hidden, a program that
//! does not end ended with its whole group, and the server's stop ending one under way.

use super::*;
use crate::testing::Tree;
use std::path::PathBuf;
use std::sync::Arc;

const ADDRESS: &str = "U024BE7LH";
const LINK: &str = "http://studio.tailnet.ts.net:8765/symphony/task/app/x";
const LINE: &str = "{\"notice\":1,\"id\":\"n-1\",\"kind\":\"verdict\",\"project\":\"symphony\",\
    \"link\":\"http://studio.tailnet.ts.net:8765/symphony/task/app/x\",\
    \"to\":{\"way\":\"slack\",\"address\":\"U024BE7LH\"}}";

/// A fake program `name` in `tree`, its sh `body` run with the folder it is in as `$here`.
fn program(tree: &Tree, name: &str, body: &str) -> PathBuf {
    let path = tree.path(name);
    let body = format!("#!/bin/sh\nhere=\"${{0%/*}}\"\n{body}\n");
    crate::fixture_script::write(&path, &body, false);
    path
}

/// The server's own wait for a program's end.
fn ends(pid: u32) -> io::Result<bool> {
    symphony_board::sys::gone(i64::from(pid), MOST)
}

fn handed(program: &Path) -> Handed {
    hand(program, LINE, &[ADDRESS, LINK], &Running::default(), &ends)
}

#[test]
fn a_sent_notice_reaches_its_program_on_stdin_alone() {
    let tree = Tree::new();
    let sends = program(
        &tree,
        "sends",
        "cat > \"$here/got\"\nprintf '%s|' \"$@\" > \"$here/args\"\nenv > \"$here/env\"\nexit 0",
    );
    assert_eq!(handed(&sends), Handed::Sent);
    assert_eq!(tree.read("got"), format!("{LINE}\n"), "one line and its newline");
    assert_eq!(tree.read("args"), "|", "no argument");
    // No part of the message is in its environment; the environment itself is not printed.
    let env = tree.read("env");
    for part in [ADDRESS, LINK, "\"notice\":1"] {
        assert!(!env.contains(part), "{part} is in the program's environment");
    }
}

#[test]
fn a_75_is_handed_again_with_its_words_the_address_and_the_link_hidden() {
    let tree = Tree::new();
    let later = program(
        &tree,
        "later",
        &format!("cat >/dev/null\necho \"ratelimited for {ADDRESS}\" >&2\necho \"at {LINK}\" >&2\nexit 75"),
    );
    assert_eq!(
        handed(&later),
        Handed::Later("ratelimited for <hidden> / at <hidden>".into())
    );
    let quiet = program(&tree, "quiet", "exit 75");
    assert_eq!(
        handed(&quiet),
        Handed::Later("its program said nothing on stderr".into())
    );
}

#[test]
fn any_other_status_is_never_sent_with_its_status_and_words() {
    let tree = Tree::new();
    let refused = program(
        &tree,
        "refused",
        &format!("cat >/dev/null\necho \"channel_not_found: {ADDRESS}\" >&2\nexit 1"),
    );
    assert_eq!(
        handed(&refused),
        Handed::Never("exit 1: channel_not_found: <hidden>".into())
    );
    let three = program(&tree, "three", "exit 3");
    assert_eq!(handed(&three), Handed::Never("exit 3".into()));
    let killed = program(&tree, "killed", "kill -TERM $$");
    let Handed::Never(why) = handed(&killed) else {
        panic!("a signal's end is never sent");
    };
    assert!(why.starts_with("it ended with no exit status"), "{why}");
}

#[test]
fn a_program_that_does_not_start_is_never_sent() {
    let tree = Tree::new();
    let Handed::Never(why) = handed(&tree.path("absent")) else {
        panic!("a program that is not there fails the same on every hand");
    };
    assert!(why.starts_with("its program did not start: "), "{why}");
    tree.write("plain", "not a program");
    assert!(matches!(handed(&tree.path("plain")), Handed::Never(_)));
}

#[test]
fn a_program_that_does_not_end_is_ended_with_its_whole_group() {
    let tree = Tree::new();
    // It starts a child of its own, says so, and never ends: nor does its child.
    let stuck = program(
        &tree,
        "stuck",
        "sleep 600 &\necho $! > \"$here/child.tmp\"\nmv \"$here/child.tmp\" \"$here/child\"\nexec sleep 600",
    );
    let child = tree.path("child");
    // Its bound passes once its child is running: the end a deadline gives, without a clock.
    let bound = |_pid: u32| {
        test_time::until_true("the stuck program's child", MOST, || child.is_file())
            .unwrap_or_else(|stall| panic!("{stall}"));
        Ok(false)
    };
    let Handed::Later(why) = hand(&stuck, LINE, &[ADDRESS, LINK], &Running::default(), &bound)
    else {
        panic!("a program ended at its bound is handed again");
    };
    assert_eq!(why, "it did not end within 60 s, so it and its group were ended");
    let pid: i64 = tree.read("child").trim().parse().unwrap();
    test_time::until_true("the stuck program's child ended", MOST, || {
        !symphony_board::sys::pid_alive(pid)
    })
    .unwrap_or_else(|stall| panic!("{stall}"));
}

#[test]
fn the_servers_stop_ends_a_hand_under_way_and_starts_none_after() {
    let tree = Tree::new();
    let stuck = program(
        &tree,
        "stuck",
        "echo $$ > \"$here/started.tmp\"\nmv \"$here/started.tmp\" \"$here/started\"\nexec sleep 600",
    );
    let running = Arc::new(Running::default());
    let hand_under_way = {
        let (running, stuck) = (running.clone(), stuck.clone());
        std::thread::spawn(move || hand(&stuck, LINE, &[], &running, &ends))
    };
    let started = tree.path("started");
    test_time::until_true("the program started", MOST, || started.is_file())
        .unwrap_or_else(|stall| panic!("{stall}"));
    running.stop();
    let handed = crate::testing::joined(hand_under_way, "the hand the stop cut", MOST);
    assert_eq!(handed, Handed::Cut);
    std::fs::remove_file(&started).unwrap();
    assert_eq!(hand(&stuck, LINE, &[], &running, &ends), Handed::Cut);
    assert!(!started.exists(), "no program starts once the server stops");
}

#[test]
fn what_a_program_says_is_one_line_cut_short_with_every_hidden_word_replaced() {
    assert_eq!(hidden_in(b"  a  \n\n b U1 \n", &["U1", ""]), "a / b <hidden>");
    let long = "x".repeat(SAID + 20);
    let said = hidden_in(long.as_bytes(), &[]);
    assert_eq!(said.chars().count(), SAID + 1);
    assert!(said.ends_with('…'));
    assert_eq!(hidden_in(b"\xff ok", &[]), "\u{fffd} ok");
}
