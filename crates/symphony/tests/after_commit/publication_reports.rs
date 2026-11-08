//! What a stalled publication reports: the watchdog's logs, the relayed stages and thread traces,
//! and when the watchdog fires: on a case gone quiet, never on a slow case that keeps progressing.
use super::*;
use std::cell::Cell;
use std::os::unix::process::ExitStatusExt;

/// One poll of the stub cases' watchdog, on the clock each stub keeps for itself: a stub runs
/// minutes of its clock in milliseconds, so no test here waits out `DEADLINE`.
const POLL: Duration = Duration::from_millis(250);

/// A stalled stub's end: it exits cleanly once its clock passes four deadlines, so a watchdog
/// that never fires fails the test at `expect_err` instead of polling forever.
fn ends_after_four_deadlines(
    clock: &Cell<Instant>,
    spawned: Instant,
) -> impl FnMut() -> Option<std::process::ExitStatus> + '_ {
    move || (clock.get() >= spawned + DEADLINE * 4).then(|| std::process::ExitStatus::from_raw(0))
}

const FIRES_FIRST: &str = "the watchdog fires before the stub ends";

#[test]
fn the_watchdog_fails_a_stub_case_that_goes_its_deadline_without_a_stage_event() {
    let tree = common::Tree::new();
    let open = stage_line("git start [\"update-ref\"]");
    std::fs::write(tree.path("child.log"), &open).unwrap();
    let pending = Mutex::new(PendingPublications::default());
    let spawned = Instant::now();
    let clock = Cell::new(spawned);
    let mut last = spawned;
    let report = watch(
        "stalled stub",
        &tree.root,
        &pending,
        ends_after_four_deadlines(&clock, spawned),
        || clock.get(),
        || {
            let at = clock.get() + POLL;
            clock.set(at);
            // Stage events for 10 s, then only output that is not one (libtest's, a command's),
            // which does not hold the watchdog off.
            let line = if at <= spawned + Duration::from_secs(10) {
                last = at;
                open.as_str()
            } else {
                "running 1 test\n"
            };
            pending.lock().unwrap().observe(line, at);
        },
    )
    .expect_err(FIRES_FIRST);
    assert!(
        report.starts_with("stalled stub watchdog: --- child.log ---\n") && report.ends_with(&open),
        "the report gives the child log, ending at the open stage: {report}"
    );
    assert_eq!(
        clock.get() - last,
        DEADLINE,
        "it fires on the first poll a deadline after the last stage event"
    );
    assert!(
        clock.get() - spawned > DEADLINE,
        "the whole wall is not what it measures"
    );

    let pending = Mutex::new(PendingPublications::default());
    let clock = Cell::new(spawned);
    let report = watch(
        "silent stub",
        &tree.root,
        &pending,
        ends_after_four_deadlines(&clock, spawned),
        || clock.get(),
        || clock.set(clock.get() + POLL),
    )
    .expect_err(FIRES_FIRST);
    assert!(report.starts_with("silent stub watchdog: "), "{report}");
    assert_eq!(
        clock.get() - spawned,
        DEADLINE,
        "a child with no stage event at all fails a deadline after its spawn"
    );
}

#[test]
fn the_watchdog_fails_a_stub_case_whose_output_only_looks_like_stage_events() {
    let tree = common::Tree::new();
    let open = stage_line("publication start: owner comment");
    let trace = "publication pid=17 id=1 thread=ThreadId(2)/\"stub\" stage=\"wait update-ref\"";
    let noise = [
        "publication pid=\n".to_string(),
        "after_commit case=\n".into(),
        "publication pid=17 still waiting on its lock\n".into(),
        format!("{trace} queued elapsed=0.100s\n"),
        format!("{trace} finished elapsed=soon\n"),
        "publication pid=17 id=1 thread=ThreadId(2)/\"stub\" stage=\"\" finished elapsed=0.001s\n"
            .into(),
        "after_commit case=stub pid=17 +1.000s: \n".into(),
        "after_commit case=stub pid=seventeen +1.000s: git start\n".into(),
        "after_commit case=stub pid=17: git start\n".into(),
    ];
    for line in &noise {
        assert!(!stage_event(line), "not a whole stage record: {line}");
    }
    assert!(stage_event(&open));
    assert!(stage_event(&format!("{trace} started elapsed=0.000s\n")));
    std::fs::write(tree.path("child.log"), &open).unwrap();
    let pending = Mutex::new(PendingPublications::default());
    let spawned = Instant::now();
    let clock = Cell::new(spawned);
    pending.lock().unwrap().observe(&open, spawned);
    let mut lines = noise.iter().cycle();
    let report = watch(
        "noisy stub",
        &tree.root,
        &pending,
        ends_after_four_deadlines(&clock, spawned),
        || clock.get(),
        || {
            let at = clock.get() + POLL;
            clock.set(at);
            pending.lock().unwrap().observe(lines.next().unwrap(), at);
        },
    )
    .expect_err(FIRES_FIRST);
    assert!(
        report.starts_with("noisy stub watchdog: --- child.log ---\n") && report.ends_with(&open),
        "{report}"
    );
    assert_eq!(
        clock.get() - spawned,
        DEADLINE,
        "a poll of prefixed noise after the last stage event is still quiet"
    );
}

#[test]
fn the_watchdog_passes_a_slow_stub_case_whose_stage_events_keep_coming() {
    let tree = common::Tree::new();
    let pending = Mutex::new(PendingPublications::default());
    let spawned = Instant::now();
    let clock = Cell::new(spawned);
    let ends = spawned + DEADLINE * 3;
    let (mut next, mut id) = (spawned, 0);
    // As in the slow case that failed on 2026-11-07: one fixture stage opens a publication, and
    // then only its traces come, each one poll short of a deadline after the last.
    let status = watch(
        "slow stub",
        &tree.root,
        &pending,
        || (clock.get() >= ends).then(|| std::process::ExitStatus::from_raw(0)),
        || clock.get(),
        || {
            let at = clock.get() + POLL;
            clock.set(at);
            if at < next {
                return;
            }
            next = at + DEADLINE - POLL;
            let mut pending = pending.lock().unwrap();
            if id == 0 {
                pending.observe(&stage_line("publication start: late queued owner comment"), at);
            } else {
                let trace = format!(
                    "{PUBLICATION_TRACE}17 id={id} thread=ThreadId(2)/\"slow stub\" stage=\"wait update-ref\""
                );
                pending.observe(&format!("{trace} started elapsed=0.000s\n"), at);
                pending.observe(&format!("{trace} finished elapsed=0.300s\n"), at);
            }
            id += 1;
        },
    )
    .unwrap_or_else(|report| panic!("a progressing case passes: {report}"));
    assert!(status.success());
    assert_eq!(
        clock.get() - spawned,
        DEADLINE * 3,
        "the stub ran three deadlines of wall"
    );
    assert_eq!(
        id, 4,
        "its progress was one fixture stage and three traced stages"
    );
}

#[test]
fn watchdog_report_keeps_the_pending_publication_stage_and_non_utf8_output() {
    let tree = common::Tree::new();
    assert!(child_output(&tree.root).is_empty());
    std::fs::write(
        tree.path("child.log"),
        b"publication pid=17 id=2 thread=ThreadId(3)/\"worker\" stage=\"wait git pid=18 update-index\" started elapsed=0.000s\n",
    )
    .unwrap();
    std::fs::write(
        tree.path("filing.log"),
        b"close started\n\xffstill draining\n",
    )
    .unwrap();
    let output = watchdog_report("queued save", &tree.root);
    assert!(
        output.contains("queued save watchdog: --- child.log ---\npublication pid=17 id=2 thread=ThreadId(3)/\"worker\" stage=\"wait git pid=18 update-index\" started elapsed=0.000s\n"),
        "{output}"
    );
    assert!(
        output.contains("--- filing.log ---\nclose started\n\u{fffd}still draining\n"),
        "{output}"
    );
}

#[test]
fn publication_relay_keeps_fast_stages_private_and_reports_each_pending_stage_once() {
    let began = Instant::now();
    let first = "publication pid=17 id=1 thread=ThreadId(3)/\"worker\" stage=\"wait git pid=18 update-index\"";
    let second = "publication pid=17 id=2 thread=ThreadId(4)/\"worker\" stage=\"wait git pid=18 update-index\"";
    let mut pending = PendingPublications::default();
    assert!(pending.observe(&format!("{first} started elapsed=0.000s\n"), began));
    assert!(
        pending
            .overdue(began + PUBLICATION_REPORT_AFTER - Duration::from_nanos(1))
            .is_empty()
    );
    assert!(pending.observe(&format!("{first} finished elapsed=0.001s\n"), began));
    assert!(pending.overdue(began + PUBLICATION_REPORT_AFTER).is_empty());
    for key in [first, second] {
        pending.observe(&format!("{key} started elapsed=0.000s\n"), began);
    }
    let report = pending.overdue(began + PUBLICATION_REPORT_AFTER);
    assert!(
        report.contains(first) && report.contains(second),
        "{report}"
    );
    assert_eq!(report.lines().count(), 2);
    assert!(
        pending
            .overdue(began + PUBLICATION_REPORT_AFTER * 2)
            .is_empty()
    );
    pending.observe(&format!("{first} finished elapsed=6.000s\n"), began);
    assert_eq!(pending.0.len(), 1);
    pending.observe(&format!("{second} finished elapsed=6.000s\n"), began);
    assert!(pending.0.is_empty());

    let input =
        format!("normal output\n{first} started elapsed=0.000s\n{first} finished elapsed=0.001s\n");
    let mut bytes = input.as_bytes().to_vec();
    bytes.extend_from_slice(b"non utf8 \xff\n");
    let (mut saved, mut live) = (Vec::new(), Vec::new());
    let pending = Mutex::new(PendingPublications::default());
    relay_publication_output(bytes.as_slice(), &mut saved, &mut live, &pending).unwrap();
    assert_eq!(saved, bytes, "the failure log retains every byte");
    assert_eq!(live, b"normal output\nnon utf8 \xff\n");
    assert!(pending.lock().unwrap().0.is_empty());
    assert!(
        pending.lock().unwrap().2.is_some(),
        "a relayed trace is the child's progress"
    );
    assert_eq!(quiet_publication_output(&input), "normal output\n");
    let quiet = Mutex::new(PendingPublications::default());
    relay_publication_output(
        &b"normal output\nnon utf8 \xff\n"[..],
        Vec::new(),
        Vec::new(),
        &quiet,
    )
    .unwrap();
    assert!(
        quiet.lock().unwrap().2.is_none(),
        "other output is no stage event"
    );
}

#[test]
fn publication_reports_include_the_last_thread_trace_and_repeat_for_a_new_quiet_period() {
    let began = Instant::now();
    let period = PUBLICATION_REPORT_AFTER;
    let outer = "publication pid=17 id=1 thread=ThreadId(3)/\"worker\" stage=\"commit verdicts\"";
    let inner = "publication pid=17 id=2 thread=ThreadId(3)/\"worker\" stage=\"wait update-ref\"";
    let other = "publication pid=17 id=3 thread=ThreadId(4)/\"worker\" stage=\"other publication\"";
    let mut pending = PendingPublications::default();
    let start = format!("{outer} started elapsed=0.000s\n");
    pending.observe(&start, began);
    let first = pending.overdue(began + period);
    assert!(
        first.contains(&format!("last trace: {}", start.trim_end())),
        "{first}"
    );
    assert_eq!(
        first.lines().count(),
        1,
        "initial and quiet reports must not duplicate"
    );
    assert!(pending.overdue(began + period * 2).is_empty());

    let next = began + period * 2;
    pending.observe(&format!("{inner} started elapsed=0.000s\n"), next);
    let finished = format!("{inner} finished elapsed=0.001s\n");
    pending.observe(&finished, next);
    pending.observe(
        &format!("{other} started elapsed=0.000s\n"),
        next + period / 2,
    );
    assert!(
        pending
            .overdue(next + period - Duration::from_nanos(1))
            .is_empty()
    );
    let gap = pending.overdue(next + period);
    assert!(gap.contains(outer), "{gap}");
    assert!(
        gap.contains(&format!("last trace: {}", finished.trim_end())),
        "{gap}"
    );
    assert_eq!(
        gap.lines().count(),
        1,
        "the other worker has not gone quiet yet"
    );
    assert!(pending.overdue(next + period).is_empty());
    let other_report = pending.overdue(next + period + period / 2);
    assert!(other_report.contains(other), "{other_report}");
    assert!(!other_report.contains(outer), "{other_report}");

    pending.observe(
        &format!("{outer} finished elapsed=20.000s\n"),
        next + period * 2,
    );
    assert!(
        pending.overdue(next + period * 3).is_empty(),
        "finished threads must not report an idle gap"
    );
}
