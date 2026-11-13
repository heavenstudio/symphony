//! The capture's parts: which lines name a pid, how a section is bounded, which processes are the
//! test's, how a time is given to `log show`, and an owned process taken for no other.

use super::super::{Scratch, write};
use super::*;
use std::time::Duration;

#[test]
fn a_line_names_a_pid_only_as_a_whole_number() {
    for line in [
        "2435",
        "memorystatus: killing_specific_process pid 2435 [symphony-server] (vm-pageshortage)",
        "CODE SIGNING: cs_invalid_page(0x1): p=2435[symphony-server] final status 0x1",
        "localhost launchd[1]: [pid/2435 [symphony-server]:] exited due to SIGKILL",
        "symphony-server[2435]: said",
        "12 then 2435",
    ] {
        assert!(names(line, 2435), "{line}");
    }
    for line in [
        "",
        "pid 12435",
        "pid 24350",
        "1763021243512 ms since Unix epoch",
        "pid 243",
    ] {
        assert!(!names(line, 2435), "{line}");
    }
    // A longer number holding it does not hide a later whole one.
    assert!(names("124351 and 2435", 2435));
}

#[test]
fn a_section_keeps_its_first_or_last_lines_within_its_bounds_and_says_how_many_it_left_out() {
    let lines: Vec<String> = (1..=5).map(|n| format!("line {n}")).collect();
    for keep in [Keep::First, Keep::Last] {
        assert_eq!(
            bounded(&lines, 10, 1024, keep),
            "  line 1\n  line 2\n  line 3\n  line 4\n  line 5\n"
        );
    }
    assert_eq!(
        bounded(&lines, 2, 1024, Keep::Last),
        "  … 3 earlier lines left out\n  line 4\n  line 5\n"
    );
    assert_eq!(
        bounded(&lines, 2, 1024, Keep::First),
        "  line 1\n  line 2\n  … 3 later lines left out\n"
    );
    // Each kept line costs its indent and its newline: "  line 5\n" is 9 bytes.
    assert_eq!(
        bounded(&lines, 10, 18, Keep::Last),
        "  … 3 earlier lines left out\n  line 4\n  line 5\n"
    );
    assert_eq!(
        bounded(&lines, 10, 17, Keep::First),
        "  line 1\n  … 4 later lines left out\n"
    );
    assert_eq!(
        bounded(&lines, 10, 8, Keep::Last),
        "  … 5 earlier lines left out\n"
    );
    let long = "é".repeat(LINE_BYTES);
    let kept = bounded(std::slice::from_ref(&long), 1, SECTION_BYTES, Keep::Last);
    assert!(kept.ends_with("…\n"), "{kept}");
    assert!(kept.len() <= LINE_BYTES + "  …\n".len(), "{}", kept.len());

    let mut text = String::new();
    section(&mut text, "empty", Ok(Vec::new()), Keep::Last);
    section(
        &mut text,
        "failed",
        Err("no answer within 30 s".into()),
        Keep::Last,
    );
    section(&mut text, "some", Ok(vec!["a".into()]), Keep::First);
    assert_eq!(
        text,
        "· empty: none\n· failed: unavailable: no answer within 30 s\n· some:\n  a\n"
    );
}

#[test]
fn the_tests_processes_are_it_its_descendants_and_those_naming_its_scratch() {
    let table = "\
          1     0     1 Ss   10-00:00:00 /sbin/launchd
         99   100   100 S       00:10 /path/to/symphony-server --port 0
        100     1   100 S       01:00 /path/to/after_lifecycle-hash
        101   100   100 S       00:10 /path/to/symphony-server
        102   101   102 Ss      00:01 /bin/ps -axww
        200     1   200 Ss      00:20 /path/to/symphony-server serve --home /root/after-lifecycle-100-3/machine
        201   200   201 Ss      00:01 /usr/bin/git status
        300     1   300 Ss      00:20 /path/to/symphony-server serve --home /root/after-lifecycle-1000-3/machine
        400     1   400 S       00:20 /usr/bin/other
        not a row
";
    assert_eq!(
        ours(table, 100, "/root/after-lifecycle-100-"),
        [
            "100     1   100 S       01:00 /path/to/after_lifecycle-hash",
            "99   100   100 S       00:10 /path/to/symphony-server --port 0",
            "101   100   100 S       00:10 /path/to/symphony-server",
            "102   101   102 Ss      00:01 /bin/ps -axww",
            "200     1   200 Ss      00:20 /path/to/symphony-server serve --home /root/after-lifecycle-100-3/machine",
            "201   200   201 Ss      00:01 /usr/bin/git status",
        ]
    );
    assert_eq!(
        row_of(table, 400).as_deref(),
        Some("400     1   400 S       00:20 /usr/bin/other")
    );
    assert_eq!(row_of(table, 401), None);
}

#[test]
fn a_time_is_given_to_log_show_in_utc() {
    for (seconds, said) in [
        (0, "1970-01-01 00:00:00+0000"),
        (951_782_400, "2000-02-29 00:00:00+0000"),
        (1_704_067_199, "2023-12-31 23:59:59+0000"),
        (1_794_558_000, "2026-11-13 08:20:00+0000"),
        (4_107_542_400, "2100-03-01 00:00:00+0000"),
    ] {
        assert_eq!(utc(UNIX_EPOCH + Duration::from_secs(seconds)), said);
    }
}

#[test]
fn a_signal_is_named_with_its_number() {
    assert_eq!(name(libc::SIGKILL), "SIGKILL (signal 9)");
    assert_eq!(name(libc::SIGTERM), "SIGTERM (signal 15)");
    assert_eq!(name(64), "signal 64");
}

#[test]
fn an_owned_process_is_signalled_only_while_its_pid_names_the_process_taken() {
    let mut child = symphony_board::sys::Command::new("/bin/sleep")
        .arg("60")
        .spawn()
        .unwrap();
    let pid = i64::from(child.id());
    let owned = Owned::of(pid);
    assert!(owned.alive());
    // The same pid with another start is what a pid the kernel gave a new process looks like.
    let other = Owned {
        pid,
        start: Identity::Started(Start::At(1.0)),
    };
    assert!(!other.alive());
    other.signal(libc::SIGKILL, "a recycled pid");
    assert!(
        child.try_wait().unwrap().is_none(),
        "a process taken for another was signalled"
    );
    owned.signal(libc::SIGKILL, "the case ends it");
    assert_eq!(
        std::os::unix::process::ExitStatusExt::signal(&child.wait().unwrap()),
        Some(libc::SIGKILL)
    );
    assert!(!owned.alive(), "a reaped child is gone");
    owned.signal(libc::SIGTERM, "after its end");
    let journal = sent_to(pid);
    let tails: Vec<&str> = journal
        .iter()
        .map(|line| line.split_once(": ").unwrap().1)
        .collect();
    assert_eq!(
        tails,
        [
            format!(
                "SIGKILL (signal 9) to pid {pid}: not sent, the pid no longer names the process \
                 taken (a recycled pid)"
            ),
            format!("SIGKILL (signal 9) to pid {pid}: the case ends it"),
            format!(
                "SIGTERM (signal 15) to pid {pid}: not sent, the pid no longer names the process \
                 taken (after its end)"
            ),
        ]
    );
    assert!(
        journal.iter().all(|line| line.starts_with("at ")
            && line.contains(" ms since Unix epoch, thread ")),
        "{journal:?}"
    );
}

#[test]
fn a_process_taken_once_it_had_ended_is_gone_and_one_of_unknown_start_is_known_by_its_pid() {
    for pid in [0, -1] {
        let gone = Owned::of(pid);
        assert_eq!(gone.start, Identity::Gone, "{pid}");
        assert!(!gone.alive());
    }
    // Only the pid is known: alive while some process has it.
    let this = Owned {
        pid: i64::from(std::process::id()),
        start: Identity::Unknown,
    };
    assert!(this.alive());
    let started = Owned::of(i64::from(std::process::id()));
    assert!(
        matches!(started.start, Identity::Started(_)),
        "{started:?}"
    );
    assert!(started.alive());
}

#[test]
fn the_scratch_logs_are_searched_for_a_pid_in_this_processs_folders_alone() {
    let scratch = Scratch::new("scratch-log-search");
    let root = scratch.path.join("root");
    let prefix = "after-lifecycle-7-";
    let mine = root.join(format!("{prefix}1"));
    write(
        &mine.join("starter-3.err"),
        "· Symphony: SIGKILL to the server, pid 2435: it did not end\nother pid 24350\n",
    );
    write(&mine.join("machine/server.log"), "group 2435 killed\n");
    write(&mine.join("deep/a/b/c/x.log"), "deep enough 2435\n");
    write(&mine.join("deep/a/b/c/d/x.log"), "too deep 2435\n");
    write(&mine.join("main/.git/logs/HEAD.log"), "git 2435\n");
    write(&mine.join("notes.txt"), "not a log 2435\n");
    write(
        &root.join("after-lifecycle-70-1/starter.err"),
        "another process's 2435\n",
    );
    assert_eq!(
        scratch_lines(&root, prefix, 2435),
        [
            format!("{prefix}1/deep/a/b/c/x.log: deep enough 2435"),
            format!("{prefix}1/machine/server.log: group 2435 killed"),
            format!(
                "{prefix}1/starter-3.err: · Symphony: SIGKILL to the server, pid 2435: it did \
                 not end"
            ),
        ]
    );
    assert!(scratch_lines(&root.join("none"), prefix, 2435).is_empty());
}
