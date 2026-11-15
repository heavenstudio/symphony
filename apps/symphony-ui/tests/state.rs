//! What the chat's panel takes of what the page heard: its activity toasts, and the asks' readers'
//! news. Every page follows the feed in the web client (apps/symphony-web).

use symphony_ui::state::{AsksNews, Toasts};

fn activity(id: &str) -> symphony_api::ActivityItem {
    symphony_api::ActivityItem {
        id: id.into(),
        kind: "question".into(),
        path: "docs/PROBE.md".into(),
        slug: format!("Unread {id}"),
        ts: "2026-11-02T06:00:00Z".into(),
        text: "A real activity notification".into(),
        href: "/docs/PROBE.md".into(),
        answered: false,
    }
}

fn up(toasts: &Toasts) -> Vec<&str> {
    toasts.items().map(|item| item.id.as_str()).collect()
}

#[test]
fn an_earlier_batch_expiring_leaves_a_later_batch_up() {
    let mut toasts = Toasts::default();
    let first = toasts.arrive(vec![activity("a")]);
    let second = toasts.arrive(vec![activity("b"), activity("c")]);
    assert_ne!(first, second);
    toasts.expire(first);
    assert_eq!(
        up(&toasts),
        ["b", "c"],
        "the later batch keeps its own time"
    );
    toasts.expire(first);
    assert_eq!(
        up(&toasts),
        ["b", "c"],
        "an expiry that already ran changes nothing"
    );
    toasts.expire(second);
    assert!(up(&toasts).is_empty());
}

#[test]
fn a_dismissed_batch_expiring_takes_down_no_later_toast() {
    let mut toasts = Toasts::default();
    let first = toasts.arrive(vec![activity("a"), activity("b")]);
    toasts.dismiss("a");
    toasts.dismiss("b");
    toasts.arrive(vec![activity("c"), activity("d")]);
    toasts.expire(first);
    assert_eq!(up(&toasts), ["c", "d"]);
}

#[test]
fn at_most_three_toasts_are_up_and_the_newest_stay() {
    let mut toasts = Toasts::default();
    toasts.arrive(vec![activity("a"), activity("b")]);
    let second = toasts.arrive(vec![activity("c"), activity("d")]);
    assert_eq!(up(&toasts), ["b", "c", "d"]);
    toasts.arrive((0..4).map(|n| activity(&format!("e{n}"))).collect());
    assert_eq!(
        up(&toasts),
        ["e1", "e2", "e3"],
        "one batch past the cap keeps its newest"
    );
    toasts.expire(second);
    assert_eq!(up(&toasts), ["e1", "e2", "e3"]);
}

#[test]
fn dismissing_takes_down_that_toast_alone() {
    let mut toasts = Toasts::default();
    toasts.arrive(vec![activity("a"), activity("b")]);
    toasts.dismiss("a");
    assert_eq!(up(&toasts), ["b"]);
    toasts.dismiss("missing");
    assert_eq!(up(&toasts), ["b"]);
}

#[test]
fn ask_news_concerns_the_reader_of_a_moved_thread_and_every_reader_after_a_reconnect() {
    let moved = AsksNews {
        threads: vec![(7, "docs/A.md".into()), (9, "docs/B.md".into())],
        ..AsksNews::default()
    };
    assert!(moved.concerns(Some(7), None), "the chat on thread 7");
    assert!(
        !moved.concerns(Some(8), None),
        "a chat on another thread reads nothing"
    );
    assert!(
        moved.concerns(None, Some("docs/B.md")),
        "a document with a moved thread"
    );
    assert!(
        !moved.concerns(None, Some("docs/C.md")),
        "a document none of whose threads moved"
    );
    assert!(
        !AsksNews::default().concerns(Some(7), Some("docs/A.md")),
        "news of nothing concerns nobody"
    );
    for everyone in [
        AsksNews {
            everything: true,
            ..AsksNews::default()
        },
        AsksNews {
            recipient: true,
            ..AsksNews::default()
        },
    ] {
        assert!(everyone.concerns(Some(8), None), "{everyone:?}");
        assert!(everyone.concerns(None, Some("docs/C.md")), "{everyone:?}");
    }
}
