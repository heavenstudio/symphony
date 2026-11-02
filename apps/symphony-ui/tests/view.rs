//! The board view, rendered to HTML on the host by dioxus-ssr from the wire fixtures: what the
//! owner sees on `/`, region by region.

mod common;

use common::board;
use dioxus::prelude::*;
use symphony_api::Board;
use symphony_ui::view::BoardView;
use symphony_ui::words::Moment;

fn render(board: Board) -> String {
    let now = Moment::parse("2026-10-08 10:12").unwrap();
    dioxus_ssr::render_element(rsx! { BoardView { board, now } })
}

/// The text between `<tag class="name"` and the end of that element's first closing tag of the
/// same name: good enough for the flat regions the board draws.
fn region<'a>(html: &'a str, marker: &str) -> &'a str {
    let start = html
        .find(marker)
        .unwrap_or_else(|| panic!("no {marker} in {html}"));
    let rest = &html[start..];
    let end = rest.find("</section>").unwrap_or(rest.len());
    &rest[..end]
}

fn count(html: &str, needle: &str) -> usize {
    html.matches(needle).count()
}

#[test]
fn the_hero_is_the_next_card_with_a_button_straight_to_it() {
    let html = render(board("board.json"));
    let hero = region(&html, r#"class="hero card""#);
    assert!(hero.contains("Next up · 1 of 37"), "{hero}");
    assert!(hero.contains("Republic · frigate picture"), "{hero}");
    assert!(
        hero.contains("Three candidates for the Lancer-class frigate"),
        "{hero}"
    );
    assert!(
        hero.contains(r#"href="/docs/ships/republic.md""#),
        "the document chip: {hero}"
    );
    assert!(hero.contains("Republic ships"), "{hero}");
    assert!(hero.contains("art · gates 4 hulls"), "{hero}");
    assert!(
        hero.contains("waiting 2 h"),
        "posted 07:42, now 10:12: {hero}"
    );
    assert!(
        hero.contains(r#"href="/docs/ships/republic.md#republic-frigate-picture""#),
        "{hero}"
    );
    assert!(hero.contains("Open this card"), "{hero}");
    assert_eq!(
        count(hero, "<figure"),
        3,
        "one figure per candidate: {hero}"
    );
    for (letter, src) in [("A", "frigate-a"), ("B", "frigate-b"), ("C", "frigate-c")] {
        assert!(
            hero.contains(&format!("<figcaption>{letter}</figcaption>")),
            "{letter}: {hero}"
        );
        assert!(
            hero.contains(&format!("/docs/art/ships/republic/{src}.png")),
            "{src}: {hero}"
        );
    }
}

#[test]
fn a_hull_not_drawn_yet_shows_that_its_picture_is_coming() {
    let mut board = board("board.json");
    board.next.rotate_left(4); // the Empire card, naming three hulls, comes first
    let html = render(board);
    let hero = region(&html, r#"class="hero card""#);
    assert_eq!(count(hero, "<figure"), 3, "{hero}");
    assert_eq!(count(hero, "picture coming"), 1, "the cruiser: {hero}");
    assert_eq!(
        count(hero, r#"class="waiting""#),
        1,
        "the destroyer waits on review: {hero}"
    );
    assert!(
        !hero.contains("hero-summary"),
        "no summary, no paragraph: {hero}"
    );
}

#[test]
fn then_come_the_four_after_it_in_the_queues_order() {
    let html = render(board("board.json"));
    let next = region(&html, r#"class="next card""#);
    assert_eq!(count(next, r#"class="nextcard""#), 4, "{next}");
    let order: Vec<usize> = [
        "mining-yield-curve",
        "machine-carrier-picture",
        "dock-menu-order",
        "empire-hull-silhouettes",
    ]
    .iter()
    .map(|slug| {
        next.find(&format!("#{slug}\""))
            .unwrap_or_else(|| panic!("{slug}: {next}"))
    })
    .collect();
    assert!(order.is_sorted(), "in the queue's order: {order:?}");
    assert!(next.contains("Mining · design · gates 2 cards"), "{next}");
    assert!(
        next.contains(r#"src="/docs/art/ships/machine/carrier-a.png""#),
        "the first picture is the thumbnail: {next}"
    );
    assert!(
        next.contains(r#"href="/queue""#) && next.contains("show all 37"),
        "{next}"
    );
}

#[test]
fn the_counts_link_to_where_their_detail_lives() {
    let html = render(board("board.json"));
    for (href, value, label) in [
        ("/queue#you", "37", "waiting on you"),
        ("/queue#spot", "4", "auto-approved to spot-check"),
        ("#activity", "6", "answered by agents today"),
        ("/tasks", "12 · 5", "tasks in progress · in review"),
        ("/agents", "14", "agents live"),
    ] {
        let tile = format!(r#"href="{href}"><b>{value}</b><span>{label}</span>"#);
        assert!(html.contains(&tile), "{tile}\nin {html}");
    }
    assert!(html.contains("across 9 documents"), "{html}");
}

#[test]
fn without_a_task_board_its_counts_are_dashes() {
    let mut board = board("board.json");
    board.counts.tasks = None;
    let html = render(board);
    assert!(html.contains(r#"href="/tasks"><b>–</b>"#), "{html}");
    assert!(html.contains(r#"href="/agents"><b>–</b>"#), "{html}");
}

#[test]
fn progress_shows_ship_art_per_faction_and_the_design_docs() {
    let html = render(board("board.json"));
    let progress = region(&html, r#"class="progress panel card""#);
    assert!(
        progress.contains("43%"),
        "69 of 161 hulls approved: {progress}"
    );
    assert!(progress.contains("69 of 161 hulls approved"), "{progress}");
    assert_eq!(count(progress, r#"class="bar""#), 6, "{progress}");
    assert!(
        progress.contains(r#"href="/docs/ships/coalition.md""#),
        "{progress}"
    );
    assert!(progress.contains("Alien coalition"), "{progress}");
    assert!(
        progress.contains("width: 33.3%"),
        "coalition: 9 of 27 approved: {progress}"
    );
    assert!(
        progress.contains("width: 11.1%"),
        "coalition: 3 of 27 waiting: {progress}"
    );
    assert!(progress.contains("browse all 64"), "{progress}");
    assert!(progress.contains("41 decisions waiting"), "{progress}");
}

#[test]
fn the_head_greets_by_the_hour_and_says_how_much_waits() {
    let html = render(board("board.json"));
    assert!(html.contains("<h1>Good morning</h1>"), "{html}");
    assert!(
        html.contains("37 cards wait on you across 9 documents. Start with the one below."),
        "{html}"
    );
}

#[test]
fn an_empty_queue_says_nothing_waits_and_points_at_the_spot_checks() {
    let html = render(board("board-empty.json"));
    assert!(html.contains("Your queue is empty."), "{html}");
    assert!(html.contains("Nothing waits on you."), "{html}");
    assert!(
        html.contains(r#"href="/queue#spot""#) && html.contains("2 auto-approved cards"),
        "{html}"
    );
    assert!(
        !html.contains(r#"class="next card""#),
        "no list after an empty hero: {html}"
    );
    assert!(
        !html.contains(r#"class="art""#),
        "no ship art without hulls: {html}"
    );
}

#[test]
fn a_branch_decision_uses_the_ordinary_hero_and_links_its_task() {
    let mut board = board("board.json");
    board.next[0].doc = "@design~policy/docs/POLICY.md".into();
    board.next[0].task = Some("design/policy".into());
    let html = render(board);
    let hero = region(&html, r#"class="hero card""#);
    assert!(hero.contains("href=\"/task/design/policy\""));
    assert!(hero.contains("href=\"/@design~policy/docs/POLICY.md#republic-frigate-picture\""));
    assert!(hero.contains("Next up · 1 of 37"));
}

#[test]
fn comparisons_label_each_kind_and_link_the_task() {
    use symphony_api::{Difference, DocumentComparison, SectionComparison};
    use symphony_ui::doc_components::{ComparisonNote, SectionDifference};
    let comparison = DocumentComparison {
        task: "design/policy".into(),
    };
    let html = dioxus_ssr::render_element(rsx! { ComparisonNote { comparison } });
    assert!(html.contains("href=\"/task/design/policy\""));
    for (kind, label) in [
        (Difference::Added, "Added"),
        (Difference::Changed, "Changed"),
        (Difference::Removed, "Removed"),
    ] {
        let comparison = Some(SectionComparison {
            kind,
            before: Vec::new(),
        });
        let html = dioxus_ssr::render_element(
            rsx! { SectionDifference { comparison, p { "Current prose" } } },
        );
        assert!(html.contains(&format!(">{label}</p>")));
        assert!(html.contains("Current prose"));
    }
    let html = dioxus_ssr::render_element(
        rsx! { SectionDifference { comparison: None, p { "Unchanged prose" } } },
    );
    assert!(html.contains("Unchanged prose"));
    assert!(!html.contains("data-difference"));
}
#[path = "view/queue.rs"]
mod queue;
