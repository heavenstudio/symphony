//! Bounded key ranges on each backend, including exclusive continuation and clipped event text.
use crate::common::{Fixture, NOW, Seed, refused};
use symphony_board::THREAD_WINDOW_LIMIT;
use symphony_board::rows::MessageRow;

#[test]
fn thread_window_caps_rows_continues_exclusively_and_omits_post_bodies() {
    let mut seed = Seed::default()
        .task("window", "ready")
        .task("other", "ready");
    seed.dump.messages = (0..THREAD_WINDOW_LIMIT + 3)
        .map(|n| MessageRow {
            id: n as i64 + 1,
            slug: "window".into(),
            sender: "worker".into(),
            recipient: Some("@orchestrator".into()),
            kind: "msg".into(),
            text: format!("body {n}"),
            ts: NOW,
        })
        .collect();
    seed.dump.messages.push(MessageRow {
        id: THREAD_WINDOW_LIMIT as i64 + 4,
        slug: "other".into(),
        sender: "other-worker".into(),
        kind: "system".into(),
        text: "another thread".into(),
        ts: NOW,
        ..MessageRow::default()
    });
    let f = Fixture::new(seed);
    let empty = f.store.thread_window("window", Some(0)).unwrap();
    assert!(empty.lines.is_empty() && !empty.more && !empty.uncertain);
    assert_eq!(empty.status, "ready");
    assert_eq!(empty.parent, None);
    assert!(refused(f.store.thread_window("missing", None)).contains("no task"));
    let mut all = f.store.thread("window").unwrap();
    for line in &mut all {
        line.text.clear();
    }
    let page = f.store.thread_window("window", None).unwrap();
    assert_eq!(page.lines.len(), THREAD_WINDOW_LIMIT);
    assert!(page.more && !page.uncertain);
    let first = page.lines[0].id;
    assert_eq!(page.lines, all[all.len() - THREAD_WINDOW_LIMIT..]);
    let older = f.store.thread_window("window", Some(first)).unwrap();
    assert!(!older.more);
    assert!(older.lines.iter().all(|line| line.id < first));
    assert_eq!(older.lines.len() + page.lines.len(), all.len());
    assert_eq!(older.lines, all[..all.len() - THREAD_WINDOW_LIMIT]);
    let exact = f
        .store
        .thread_window("window", Some(THREAD_WINDOW_LIMIT as i64 + 1))
        .unwrap();
    assert!(!exact.more && !exact.uncertain);
    assert_eq!(exact.lines, all[..THREAD_WINDOW_LIMIT]);
}

#[test]
fn thread_window_clips_system_text_marks_uncertainty_and_keeps_verdict_words() {
    let mut seed = Seed::default().task("window", "review");
    seed.dump.messages = [
        ("system", "o joined as orchestrator".into()),
        ("system", "é".repeat(2050)),
        ("verdict", "approve: details that stay on the thread".into()),
    ]
    .into_iter()
    .enumerate()
    .map(|(n, (kind, text))| MessageRow {
        id: n as i64 + 1,
        slug: "window".into(),
        sender: "worker".into(),
        kind: kind.into(),
        text,
        ts: NOW,
        ..MessageRow::default()
    })
    .collect();
    let f = Fixture::new(seed);
    let page = f.store.thread_window("window", None).unwrap();
    assert!(!page.more && page.uncertain);
    assert_eq!(page.status, "review");
    assert_eq!(page.lines[0].text, "o joined as orchestrator");
    assert_eq!(page.lines[1].text.chars().count(), 2048);
    assert_eq!(page.lines[2].text, "approve: details");
}
