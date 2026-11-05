//! What a stalled publication reports: the watchdog's logs, the relayed stages and thread traces.
use super::*;

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
    assert_eq!(quiet_publication_output(&input), "normal output\n");
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
