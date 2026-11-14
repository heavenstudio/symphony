//! Each stamp's reader at a tip and at a later commit, and which stamps a task needs at a tip.

use super::*;
use crate::stamps::Catalogue;

const TIP: &str = "4be91c2aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const LATER: &str = "7c0ffee1bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const SLUG: &str = "tools/x";
const FOLDER: &str = "artifacts/tools/x/";

fn post(id: i64, kind: &str, sender: &str, text: &str) -> Message {
    Message {
        id,
        slug: SLUG.into(),
        sender: sender.into(),
        recipient: None,
        kind: kind.into(),
        text: text.into(),
        ts: 100.0 * f64::from(u16::try_from(id).expect("a small id")),
    }
}

fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
}

fn assessed(stamps: &[&str], tip: Option<&str>) -> Needs {
    Needs {
        stamps: strings(stamps),
        tip: tip.map(str::to_string),
        by: "slate-owl".into(),
        why: "a store query and its test".into(),
    }
}

#[test]
fn a_line_names_the_tip_by_its_whole_id_or_a_word_it_starts_with() {
    assert!(names(&format!("approve: {TIP}: green"), TIP));
    assert!(names("approve 4be91c2: read it", TIP));
    assert!(names("at 4be91c2aaaa.", TIP));
    // Too short, another commit, a word that is no commit, upper case: none names it.
    for text in [
        "4be91c",
        "approve 7c0ffee1",
        "defaced",
        "4BE91C2",
        "",
        "x4be91c2",
    ] {
        assert!(!names(text, TIP), "{text:?}");
    }
    assert_eq!(named("approve: 7c0ffee1, then 4be91c2"), Some("7c0ffee1"));
    assert_eq!(named("approve: looks good"), None);
}

#[test]
fn a_task_needs_its_assessments_stamps_on_the_tip_it_read_and_the_defaults_back_elsewhere() {
    let catalogue = Catalogue::symphony();
    assert_eq!(needed(&catalogue, None, Some(TIP)), ["review"]);
    assert_eq!(needed(&catalogue, None, None), ["review"]);
    // On the tip it read, an assessment lowers what the task needs: none at all, or tests alone.
    let none = assessed(&[], Some(TIP));
    assert_eq!(
        needed(&catalogue, Some(&none), Some(TIP)),
        Vec::<String>::new()
    );
    let tests = assessed(&["tests"], Some(TIP));
    assert_eq!(needed(&catalogue, Some(&tests), Some(TIP)), ["tests"]);
    // A commit after it, a branch gone, or an assessment of the brief brings the defaults back.
    assert_eq!(
        needed(&catalogue, Some(&tests), Some(LATER)),
        ["tests", "review"]
    );
    assert_eq!(needed(&catalogue, Some(&tests), None), ["tests", "review"]);
    let brief = assessed(&["owner", "tests"], None);
    assert_eq!(
        needed(&catalogue, Some(&brief), Some(TIP)),
        ["tests", "review", "owner"]
    );
    // An assessment adds what it names wherever it stands, in the catalogue's order.
    let all = assessed(&["owner", "pictures", "review", "tests"], Some(TIP));
    assert_eq!(
        needed(&catalogue, Some(&all), Some(TIP)),
        ["tests", "review", "pictures", "owner"]
    );
    // A stamp the catalogue no longer has is none; a project's defaults are its own.
    let written = Catalogue::parse("[stamps.tests]\ndefault = true\n").unwrap();
    let gone = assessed(&["review", "pictures"], Some(TIP));
    assert_eq!(
        needed(&written, Some(&gone), Some(TIP)),
        Vec::<String>::new()
    );
    assert_eq!(needed(&written, Some(&gone), Some(LATER)), ["tests"]);
    assert_eq!(needed(&written, None, Some(TIP)), ["tests"]);
}

/// A run of `label` for `task` at `commit` as the recorder keeps it, begun at `id` thousand
/// seconds, finished with `exit` unless `state` says otherwise.
fn run(
    id: u8,
    task: &str,
    label: &str,
    phase: Phase,
    commit: &str,
    state: Step,
    exit: i32,
) -> Record {
    let started = 1000.0 * f64::from(id);
    Record {
        version: 1,
        operation_id: format!("{id:032x}"),
        parent_id: None,
        phase,
        label: label.into(),
        state,
        handle: Some(format!("agent-{id}")),
        task: Some(task.into()),
        checkout: "/checkout".into(),
        branch: Some(SLUG.into()),
        commit: Some(commit.into()),
        machine_id: None,
        host: "host".into(),
        pid: 10,
        child_pid: (state != Step::Queued).then_some(11),
        queued_at: started - 5.0,
        started_at: (state != Step::Queued).then_some(started),
        ended_at: (state == Step::Finished).then_some(started + 60.0),
        beat_at: started,
        exit_code: (state == Step::Finished).then_some(exit),
        signal: None,
        reason: String::new(),
        run: None,
        reading: None,
    }
}

fn ours(task: &str) -> bool {
    task == SLUG || task == "tools/x/work"
}

use Phase::{Build, Test};
use Step::{Finished, Queued, Running};

#[test]
fn tests_are_met_by_the_newest_run_of_each_command_at_the_tip_green_with_a_test_among_them() {
    let runs = [
        run(1, SLUG, "just test-rs", Test, TIP, Finished, 101),
        run(2, SLUG, "just lint-rs", Build, TIP, Finished, 0),
        // Work under it counts, and its rerun of the test is the newest: green.
        run(3, "tools/x/work", "just test-rs", Test, TIP, Finished, 0),
        // Another task's run, and one of no task, at the same commit, count for nothing.
        run(4, "tools/y", "just test-rs", Test, TIP, Finished, 1),
    ];
    let mut unattributed = run(5, SLUG, "just test-rs", Test, TIP, Finished, 1);
    unattributed.task = None;
    let runs = [runs.as_slice(), &[unattributed]].concat();
    let reading = tests(&runs, TIP, ours);
    assert_eq!(
        reading,
        Reading {
            met: true,
            by: Some("agent-3".into()),
            at: Some(3060.0),
            commit: Some(TIP.into()),
            detail: "Green at 4be91c2: `just test-rs`, `just lint-rs`".into(),
            line: None,
        }
    );
}

#[test]
fn tests_read_at_a_later_commit_are_stale_naming_the_commit_they_last_ran_at() {
    let runs = [
        run(1, SLUG, "just test-rs", Test, TIP, Finished, 0),
        run(2, SLUG, "just lint-rs", Build, TIP, Finished, 0),
    ];
    assert!(tests(&runs, TIP, ours).met);
    let stale = tests(&runs, LATER, ours);
    assert_eq!(
        stale,
        Reading {
            met: false,
            by: Some("agent-2".into()),
            at: Some(2060.0),
            commit: Some(TIP.into()),
            detail: "No build or test at 7c0ffee: the last ran at 4be91c2".into(),
            line: None,
        }
    );
    let none = tests(&[], TIP, ours);
    assert!(!none.met);
    assert_eq!(none.detail, "No build or test recorded for it at 4be91c2");
    assert_eq!(none.commit.as_deref(), Some(TIP));
}

#[test]
fn tests_are_missing_while_a_command_is_red_still_going_or_no_test_ran() {
    let red = [
        run(1, SLUG, "just test-rs", Test, TIP, Finished, 0),
        run(2, SLUG, "just lint-rs", Build, TIP, Finished, 101),
        run(3, SLUG, "just verify", Test, TIP, Finished, 2),
    ];
    let reading = tests(&red, TIP, ours);
    assert!(!reading.met);
    assert_eq!(
        reading.detail,
        "`just lint-rs` red at 4be91c2: exit 101 (1 more red)"
    );
    let mut signalled = run(1, SLUG, "just test-rs", Test, TIP, Finished, 0);
    signalled.exit_code = None;
    signalled.signal = Some(9);
    assert_eq!(
        tests(&[signalled.clone()], TIP, ours).detail,
        "`just test-rs` red at 4be91c2: ended by signal 9"
    );
    signalled.signal = None;
    assert_eq!(
        tests(&[signalled], TIP, ours).detail,
        "`just test-rs` red at 4be91c2: ended without a status"
    );
    // A red rerun green is green; one still going or queued is not yet.
    let going = [
        run(1, SLUG, "just lint-rs", Build, TIP, Finished, 1),
        run(2, SLUG, "just lint-rs", Build, TIP, Finished, 0),
        run(3, SLUG, "just test-rs", Test, TIP, Running, 0),
        run(4, SLUG, "just client-check", Test, TIP, Queued, 0),
    ];
    let reading = tests(&going, TIP, ours);
    assert!(!reading.met);
    assert_eq!(
        reading.detail,
        "`just test-rs`, `just client-check` still going at 4be91c2"
    );
    let builds = [
        run(1, SLUG, "just lint-rs", Build, TIP, Finished, 0),
        run(2, SLUG, "just symphony-ui-dist", Build, TIP, Finished, 0),
        run(3, SLUG, "a", Build, TIP, Finished, 0),
        run(4, SLUG, "b", Build, TIP, Finished, 0),
    ];
    let reading = tests(&builds, TIP, ours);
    assert!(!reading.met);
    assert_eq!(
        reading.detail,
        "No test ran at 4be91c2: `just lint-rs`, `just symphony-ui-dist`, `a` and 1 more green, \
         no test among them"
    );
}

#[test]
fn a_picture_is_a_file_under_the_tasks_artifacts_folder_or_an_upload() {
    for shown in [
        "artifacts/tools/x/wide.png",
        "Before and after at 4be91c2: artifacts/tools/x/phone-dark.PNG, artifacts/tools/x/b.jpg.",
        "/Users/me/wt/artifacts/tools/x/shots/a.webp",
        "![wide](/artifacts/uploads/0123abcd.png)",
        "http://127.0.0.1:8765/artifacts/uploads/0123abcd.jpg",
        "`artifacts/tools/x/a.jpeg`",
        "(artifacts/tools/x/a.gif)",
    ] {
        assert!(shows_picture(shown, FOLDER), "{shown:?}");
    }
    for not in [
        "artifacts/tools/x/",
        "artifacts/tools/x/notes.md",
        "artifacts/tools/x/.png",
        "artifacts/tools/y/wide.png",
        "artifacts/tools/x-2/wide.png",
        "my-artifacts/tools/x/wide.png",
        "[report](/artifacts/uploads/0123abcd.pdf)",
        "wide.png",
    ] {
        assert!(!shows_picture(not, FOLDER), "{not:?}");
    }
    assert_eq!(folder("artifacts", SLUG), FOLDER);
    assert_eq!(folder("out/", SLUG), "out/tools/x/");
}

#[test]
fn pictures_are_met_by_a_post_naming_the_tip_and_stale_at_a_later_commit() {
    let thread = [
        post(
            1,
            "msg",
            "teal-merlin",
            "Wide and phone at 4be91c2: artifacts/tools/x/wide.png",
        ),
        // The board's own line and a post with no picture count for nothing.
        post(
            2,
            "system",
            "board",
            "artifacts/tools/x/wide.png at 7c0ffee1",
        ),
        post(3, "msg", "teal-merlin", "tip 7c0ffee1: tests green"),
    ];
    let met = pictures(&thread, TIP, FOLDER);
    assert_eq!(
        met,
        Reading {
            met: true,
            by: Some("teal-merlin".into()),
            at: Some(100.0),
            commit: Some(TIP.into()),
            detail: "Pictures at 4be91c2, posted by teal-merlin".into(),
            line: Some(1),
        }
    );
    let stale = pictures(&thread, LATER, FOLDER);
    assert!(!stale.met);
    assert_eq!(stale.commit.as_deref(), Some("4be91c2"));
    assert_eq!(stale.detail, "Pictures posted at 4be91c2, none at 7c0ffee");
    assert_eq!(stale.line, Some(1));
    // An upload, named by its 64 hex digits, which name no commit, stands on none.
    let upload = format!("![a]({UPLOADS}{}.png)", "ab12".repeat(16));
    let unnamed = [post(4, "msg", "owner", &upload)];
    let reading = pictures(&unnamed, TIP, FOLDER);
    assert!(!reading.met);
    assert_eq!(reading.commit, None);
    assert_eq!(
        reading.detail,
        "Pictures posted naming no commit, none at 4be91c2"
    );
    let none = pictures(&[], TIP, FOLDER);
    assert_eq!(
        (none.met, none.detail.as_str()),
        (false, "No picture posted at 4be91c2")
    );
}

#[test]
fn the_owners_stamp_is_the_owners_approve_naming_the_tip_and_stale_at_a_later_commit() {
    let thread = [
        // Another's approve, a word in the middle and the board's record are no owner's approve.
        post(1, "msg", "slate-owl", &format!("approve {TIP}")),
        post(2, "msg", "owner", "I approve 4be91c2"),
        post(3, "system", "owner", "approve 4be91c2"),
        post(4, "msg", "owner", &format!("approve {TIP}")),
    ];
    assert!(!owner_approves(&thread[0]) && !owner_approves(&thread[1]));
    assert!(!owner_approves(&thread[2]) && owner_approves(&thread[3]));
    assert!(owner_approves(&post(
        5,
        "msg",
        "owner",
        "Approve: 4be91c2, looks right"
    )));
    assert_eq!(
        owner(&thread, TIP),
        Reading {
            met: true,
            by: Some("owner".into()),
            at: Some(400.0),
            commit: Some(TIP.into()),
            detail: "Approved by the owner at 4be91c2".into(),
            line: Some(4),
        }
    );
    let stale = owner(&thread, LATER);
    assert!(!stale.met);
    assert_eq!(stale.commit.as_deref(), Some(TIP));
    assert_eq!(
        stale.detail,
        "The owner approved 4be91c2, not the tip 7c0ffee"
    );
    let unnamed = owner(&[post(6, "msg", "owner", "approve")], TIP);
    assert_eq!(
        (unnamed.met, unnamed.detail.as_str()),
        (
            false,
            "The owner approved naming no commit, not the tip 4be91c2"
        )
    );
    let none = owner(&thread[..3], TIP);
    assert_eq!(
        (none.met, none.by, none.detail.as_str()),
        (false, None, "Waits on the owner's approve of 4be91c2")
    );
}
