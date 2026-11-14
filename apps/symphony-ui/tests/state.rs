//! Following the feed: a lost feed is tried again after a wait; another client build loads the
//! page again; the panel's toasts and the asks' readers take what it heard. Home's board applies
//! each event in the web client (apps/symphony-web/src/home-board.test.ts).

use symphony_ui::state::{
    AsksNews, Backoff, Ended, FewMap, Toasts, keeps_panel_baseline, reload_for,
};

#[test]
fn a_client_loads_the_page_again_for_another_build_than_its_own_once() {
    let (own, served) = ("/assets/a.js", "/assets/b.js");
    assert!(reload_for(Some(own), served, None));
    assert!(!reload_for(Some(own), own, None), "its own build");
    assert!(
        !reload_for(Some(own), served, Some(served)),
        "it loaded again for that build already: a shell named another way never loops"
    );
    assert!(
        reload_for(Some(own), "/assets/c.js", Some(served)),
        "a build after the one it loaded again for"
    );
    assert!(
        !reload_for(None, served, None),
        "a client that cannot name its own build never loads again"
    );
}

#[test]
fn a_lost_feed_waits_one_second_doubling_to_thirty_until_a_stream_holds() {
    let mut backoff = Backoff::default();
    let short = 500.0;
    let waits: Vec<i32> = (0..7).map(|_| backoff.after(short)).collect();
    assert_eq!(
        waits,
        [1_000, 2_000, 4_000, 8_000, 16_000, 30_000, 30_000],
        "a stream that ends as soon as it opens does not reset the wait"
    );
    assert_eq!(
        backoff.after(30_000.0),
        1_000,
        "a stream that held 30 s starts the waits again"
    );
    assert_eq!(backoff.after(0.0), 2_000);
}

#[test]
fn a_stream_closed_as_its_page_went_away_waits_none_and_starts_the_waits_again() {
    let mut backoff = Backoff::default();
    let lost: Vec<i32> = (0..4).map(|_| backoff.wait(Ended::Lost(500.0))).collect();
    assert_eq!(
        lost,
        [1_000, 2_000, 4_000, 8_000],
        "a lost stream backs off"
    );
    assert_eq!(
        backoff.wait(Ended::Away),
        0,
        "a page back from away reads again at once"
    );
    assert_eq!(
        backoff.wait(Ended::Lost(0.0)),
        1_000,
        "the next loss waits from the first wait again"
    );
    assert_eq!(backoff.wait(Ended::Lost(0.0)), 2_000);
    assert_eq!(backoff.wait(Ended::Away), 0);
    assert_eq!(backoff.wait(Ended::Away), 0, "every return reads at once");
    assert_eq!(
        backoff.wait(Ended::Lost(30_000.0)),
        1_000,
        "a stream that held 30 s still starts the waits again"
    );
}

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
fn the_panel_baseline_holds_from_an_arrival_until_the_panel_is_ready() {
    let none = None::<&[&str]>;
    let (empty, one, two) = (Some(&[][..]), Some(&["a"][..]), Some(&["a", "b"][..]));
    assert!(
        !keeps_panel_baseline(none, empty, false),
        "no baseline yet: the first is taken"
    );
    assert!(
        !keeps_panel_baseline(empty, none, false),
        "nothing known before the first read"
    );
    assert!(
        !keeps_panel_baseline(one, one, false),
        "no arrival since the baseline"
    );
    assert!(
        !keeps_panel_baseline(two, one, false),
        "an id going away is no arrival"
    );
    assert!(
        keeps_panel_baseline(empty, one, false),
        "an announced arrival keeps the ids from before it while the panel loads"
    );
    assert!(keeps_panel_baseline(one, two, false));
    assert!(
        !keeps_panel_baseline(empty, one, true),
        "a ready panel follows the activity itself"
    );
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

#[test]
fn a_few_map_keeps_one_value_per_key_in_key_order() {
    let mut map = FewMap::<String, u64>::default();
    assert!(map.is_empty());
    assert_eq!(map.first_key(), None);
    assert_eq!(map.insert("ready".into(), 1), None);
    assert_eq!(map.insert("done".into(), 2), None);
    assert_eq!(map.insert("review".into(), 3), None);
    assert_eq!(
        map.insert("ready".into(), 4),
        Some(1),
        "a key holds one value"
    );
    assert_eq!(map.first_key().map(String::as_str), Some("done"));
    assert_eq!(map.get("ready"), Some(&4));
    assert!(map.contains_key("review"));
    assert!(!map.contains_key("blocked"));
    *map.or_default("blocked".into()) += 5;
    *map.or_default("blocked".into()) += 1;
    assert_eq!(map.get("blocked"), Some(&6));
    assert_eq!(map.first_key().map(String::as_str), Some("blocked"));
    if let Some(value) = map.get_mut("done") {
        *value = 7;
    }
    assert_eq!(map.remove("done"), Some(7));
    assert_eq!(map.remove("done"), None);
    assert_eq!(map.get_mut("done"), None);
    let mut other = FewMap::default();
    for (key, value) in [("review", 3), ("blocked", 6), ("ready", 4)] {
        other.insert(key.to_string(), value);
    }
    assert_eq!(map, other, "equal whatever order the keys came in");
    map.clear();
    assert!(map.is_empty());
    assert_eq!(map.get("ready"), None);
}
