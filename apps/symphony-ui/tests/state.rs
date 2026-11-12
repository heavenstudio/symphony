//! The board follows the feed: one event changes the regions it names, in place; a lost feed is
//! tried again after a wait; another client build loads the page again.

mod common;

use common::{board, delta};
use symphony_api::{Change, Delta};
use symphony_ui::state::{
    Backoff, Ended, FewMap, Notice, Region, Toasts, apply, keeps_panel_baseline, reload_for,
};

#[test]
fn a_card_event_changes_that_card_and_nothing_else() {
    let mut board = board("board.json");
    let before = board.clone();
    let outcome = apply(&mut board, &delta("delta-card.json"));
    assert_eq!(
        outcome.moved,
        [Region::Card(3)],
        "the fourth card moved, nothing else"
    );
    assert_eq!(
        board.next[3].title,
        "The dock menu lists repair before trade, and refuel before both."
    );
    assert_eq!(
        board.version, 4183,
        "the board is now at the event's version"
    );
    for index in [0, 1, 2, 4] {
        assert_eq!(
            board.next[index], before.next[index],
            "card {index} untouched"
        );
    }
    assert_eq!(
        (board.counts, board.progress),
        (before.counts, before.progress)
    );
    assert_eq!(outcome.notice, None);
    assert_eq!(board.build, before.build);
}

#[test]
fn a_card_not_on_the_board_moves_nothing_but_the_version() {
    let mut board = board("board.json");
    let mut card = board.next[0].clone();
    card.slug = "somewhere-past-the-fifth".into();
    let before = board.clone();
    let outcome = apply(
        &mut board,
        &Delta {
            version: 4183,
            changes: vec![Change::Card(card)],
        },
    );
    assert!(outcome.moved.is_empty());
    assert_eq!(board.next, before.next);
    assert_eq!(board.version, 4183);
}

#[test]
fn a_branch_card_update_does_not_replace_mains_card_with_the_same_slug() {
    let mut board = board("board.json");
    let main = board.next[0].clone();
    let mut branch = main.clone();
    branch.doc = format!("@design~policy/{}", main.doc);
    branch.task = Some("design/policy".into());
    board.next.insert(1, branch.clone());
    branch.title = "Revised branch proposal".into();
    let outcome = apply(
        &mut board,
        &Delta {
            version: 4190,
            changes: vec![Change::Card(branch.clone())],
        },
    );
    assert_eq!(outcome.moved, [Region::Card(1)]);
    assert_eq!(board.next[0], main);
    assert_eq!(board.next[1], branch);
}

#[test]
fn queue_counts_progress_and_client_each_move_their_region() {
    let mut board = board("board.json");
    let outcome = apply(&mut board, &delta("delta-regions.json"));
    assert_eq!(
        outcome.moved,
        [Region::Queue, Region::Counts, Region::Progress]
    );
    assert_eq!(board.next.len(), 1);
    assert_eq!(board.next[0].slug, "mining-yield-curve");
    assert_eq!(board.counts.waiting, 36);
    assert_eq!(board.progress.regions[0].bars[0].done, 22);
    assert_eq!(
        board.build, "/assets/symphony-ui-dxh3f9a1c0123456789.js",
        "the client region names the build served now"
    );
    assert_eq!(outcome.notice, None, "the queue shrank: nothing new waits");
}

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

#[test]
fn an_event_at_or_before_the_boards_version_is_ignored() {
    let mut board = board("board.json");
    let before = board.clone();
    let mut stale = delta("delta-card.json");
    stale.version = before.version;
    let outcome = apply(&mut board, &stale);
    assert!(outcome.moved.is_empty());
    assert_eq!(board, before, "a replayed event changes nothing");
}

#[test]
fn more_cards_waiting_raise_one_notice_naming_the_next_card() {
    let mut board = board("board.json");
    let mut counts = board.counts.clone();
    counts.waiting += 2;
    let mut first = board.next[1].clone();
    first.slug = "new-card".into();
    first.title = "A new card waits.".into();
    let mut queue = board.next.clone();
    queue.insert(0, first);
    queue.truncate(5);
    let outcome = apply(
        &mut board,
        &Delta {
            version: 4190,
            changes: vec![Change::Queue(queue), Change::Counts(counts)],
        },
    );
    assert_eq!(
        outcome.notice,
        Some(Notice {
            title: "39 cards wait on you".into(),
            body: "A new card waits.".into()
        })
    );
}

#[test]
fn the_first_card_waiting_says_card_not_cards() {
    let mut board = board("board-empty.json");
    let card = board_card();
    let mut counts = board.counts.clone();
    counts.waiting = 1;
    counts.docs_waiting = 1;
    let outcome = apply(
        &mut board,
        &Delta {
            version: 2,
            changes: vec![Change::Counts(counts), Change::Queue(vec![card])],
        },
    );
    assert_eq!(
        outcome.notice.map(|notice| notice.title),
        Some("1 card waits on you".to_string()),
        "the notice reads the queue after the whole event, whatever the order of its regions"
    );
}

#[test]
fn a_region_this_client_does_not_know_moves_nothing() {
    let mut board = board("board.json");
    let before = board.clone();
    let outcome = apply(
        &mut board,
        &Delta {
            version: 4183,
            changes: vec![Change::Other],
        },
    );
    assert!(outcome.moved.is_empty());
    assert_eq!(board.next, before.next);
}

fn board_card() -> symphony_api::Card {
    board("board.json").next[1].clone()
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
