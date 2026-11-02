//! The board follows the feed: one event changes the regions it names, in place; a lost feed is
//! tried again after a wait; another client build loads the page again.

mod common;

use common::{board, delta};
use symphony_api::{Change, Delta};
use symphony_ui::state::{Backoff, Notice, Region, apply, reload_for};

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
    assert_eq!(board.progress.ship_art[0].approved, 22);
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
