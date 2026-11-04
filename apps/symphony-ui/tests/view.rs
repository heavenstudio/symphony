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
    assert!(hero.contains("North · frigate picture"), "{hero}");
    assert!(
        hero.contains("Three candidates for the Lancer-class frigate"),
        "{hero}"
    );
    assert!(
        hero.contains(r#"href="/docs/lamps/north.md""#),
        "the document chip: {hero}"
    );
    assert!(hero.contains("North lamps"), "{hero}");
    assert!(hero.contains("picture · gates 4 lamps"), "{hero}");
    assert!(
        hero.contains("waiting 2 h"),
        "posted 07:42, now 10:12: {hero}"
    );
    assert!(
        hero.contains(r#"href="/docs/lamps/north.md#north-frigate-picture""#),
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
            hero.contains(&format!("/docs/pictures/lamps/north/{src}.png")),
            "{src}: {hero}"
        );
    }
}

#[test]
fn a_lamp_not_drawn_yet_shows_that_its_picture_is_coming() {
    let mut board = board("board.json");
    board.next.rotate_left(4); // the East card, naming three lamps, comes first
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
        "east-lamp-silhouettes",
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
        next.contains(r#"src="/docs/pictures/lamps/machine/carrier-a.png""#),
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
fn progress_shows_lamp_picture_per_maker_and_the_design_docs() {
    let html = render(board("board.json"));
    let progress = region(&html, r#"class="progress panel card""#);
    assert!(
        progress.contains("43%"),
        "69 of 161 lamps approved: {progress}"
    );
    assert!(progress.contains("69 of 161 lamps approved"), "{progress}");
    assert_eq!(count(progress, r#"class="bar""#), 6, "{progress}");
    assert!(
        progress.contains(r#"href="/docs/lamps/south.md""#),
        "{progress}"
    );
    assert!(progress.contains("Alien south"), "{progress}");
    assert!(
        progress.contains("width: 33.3%"),
        "south: 9 of 27 approved: {progress}"
    );
    assert!(
        progress.contains("width: 11.1%"),
        "south: 3 of 27 waiting: {progress}"
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
        !html.contains(r#"class="pics""#),
        "no pictures without a card: {html}"
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
    assert!(hero.contains("href=\"/@design~policy/docs/POLICY.md#north-frigate-picture\""));
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

fn region_of(id: &str, totals: &[u32]) -> symphony_api::Region {
    symphony_api::Region {
        id: id.into(),
        title: format!("{id} title"),
        note: format!("{id} note"),
        bars: totals
            .iter()
            .enumerate()
            .map(|(index, &total)| symphony_api::ProgressBar {
                key: format!("{id}-{index}"),
                name: format!("bar {index}"),
                page: format!("docs/{id}.md"),
                title: String::new(),
                done: total / 2,
                waiting: 0,
                total,
            })
            .collect(),
        legend: ["done".into(), "waiting".into(), "left".into()],
    }
}

#[test]
fn each_plugin_region_draws_in_its_order_and_one_with_nothing_to_count_is_left_out() {
    let mut regions = board("board.json");
    regions.progress.regions = vec![
        region_of("pictures", &[4, 2]),
        region_of("empty", &[0, 0]),
        region_of("bare", &[]),
        region_of("audio", &[10]),
    ];
    let html = render(regions);
    let progress = region(&html, r#"class="progress panel card""#);
    assert_eq!(count(progress, "data-region="), 2, "{progress}");
    let picture = progress
        .find(r#"data-region="pictures""#)
        .expect("the picture region");
    let audio = progress
        .find(r#"data-region="audio""#)
        .expect("the audio region");
    assert!(picture < audio, "{progress}");
    assert!(
        !progress.contains("empty title") && !progress.contains("bare title"),
        "{progress}"
    );
    assert!(progress.contains("<h2>pictures title</h2>"), "{progress}");
    assert!(
        progress.contains("50%<small>audio note</small>"),
        "{progress}"
    );
    assert!(
        progress.contains(r#"href="/docs/pictures.md""#),
        "{progress}"
    );
    let mut none = board("board.json");
    none.progress.regions.clear();
    let html = render(none);
    assert_eq!(count(&html, "data-region="), 0, "{html}");
    assert!(
        html.contains("browse all 64"),
        "the docs summary stays: {html}"
    );
}

/// The home's row of page links.
fn links(html: &str) -> &str {
    let start = html.find(r#"class="dash-links""#).expect("the links");
    let rest = &html[start..];
    &rest[..rest.find("</p>").expect("their end")]
}

#[test]
fn the_home_links_the_projects_own_pages_and_its_register() {
    let html = render(board("board.json"));
    let links = links(&html);
    assert!(
        !links.contains("/lamps"),
        "a plain project adds no page: {links}"
    );
    assert!(links.contains(r#"href="/docs/DECISIONS.md""#), "{links}");
    let now = Moment::parse("2026-10-08 10:12").unwrap();
    let project = symphony_api::ProjectView {
        name: "Orbit".into(),
        register: "notes/register.md".into(),
        nav: vec![
            symphony_api::NavItem {
                label: "⬡ lamps".into(),
                title: "Lamp models".into(),
                href: "/lamps".into(),
                document: "docs/lamps/LAMPS.md".into(),
            },
            symphony_api::NavItem {
                label: "♪ cues".into(),
                title: "Audio cues".into(),
                href: "/cues".into(),
                document: "docs/audio/CUES.md".into(),
            },
        ],
        entrypoints: vec!["notes".into()],
    };
    let board = board("board.json");
    let html = dioxus_ssr::render_element(rsx! { BoardView { board, now, project } });
    let links = self::links(&html);
    let lamps = links
        .find(r#"<a class="pill" href="/lamps">Lamp models</a>"#)
        .expect("the lamps pill");
    let cues = links
        .find(r#"<a class="pill" href="/cues">Audio cues</a>"#)
        .expect("the cues pill");
    let register = links
        .find(r#"<a class="pill" href="/notes/register.md">Decision register</a>"#)
        .expect("the register pill");
    assert!(
        links.contains(r#"<a class="pill" href="/notes/">All docs</a>"#),
        "all docs open the first entrypoint: {links}"
    );
    assert!(
        links.find("All docs").unwrap() < lamps && lamps < cues && cues < register,
        "{links}"
    );
}
