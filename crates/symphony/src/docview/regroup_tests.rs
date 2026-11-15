//! A page that shows a card in another group than its file copy draws it in that group, each
//! section at the file's lines ([`drawn`], [`place`], [`super::super::document_showing`]).

use super::{drawn, place};
use crate::Decision;
use crate::plugins::Plugins;
use crate::register::parse_decisions;
use crate::render::Render;
use crate::test_tree::Tree;
use crate::text::splitlines;

/// A register of two pending cards under one group and topic, with a note after it.
const FILE: &str = "# Decisions\n\n<!-- register:start -->\n## Needs your decision\n\n### Topic\n\n- [ ] **alpha** — First question.\n- [ ] **bravo** — Second question.\n<!-- register:end -->\n\nA note after the register.\n";
/// Alpha, approved with words no agent has answered: it waits on an agent.
const ANSWERED: &str =
    "- [x] **alpha** — First question.\n  [2026-11-14 10:36] owner · approved — Keep it\n";
/// Alpha with an agent's line under it: still the owner's to decide.
const REPLIED: &str = "- [ ] **alpha** — First question.\n  [2026-11-14 10:36] agent — A detail.\n";

fn cards(text: &str) -> Vec<Decision> {
    parse_decisions(&splitlines(text))
}

/// The drawn text's lines, each with the file's line it stands for.
fn lines_and_places(text: &str, shown: &str) -> Option<Vec<(String, Option<usize>)>> {
    let (drawn, places) = drawn(text, &cards(text), &cards(shown), &Plugins::default())?;
    let lines = splitlines(&drawn);
    assert_eq!(lines.len(), places.len(), "a place for every line");
    Some(lines.into_iter().map(str::to_string).zip(places).collect())
}

#[test]
fn a_card_shown_answered_is_drawn_in_its_group_each_line_at_the_files_lines() {
    let drawn = lines_and_places(FILE, ANSWERED).expect("alpha regroups");
    let at = |line: &str| -> Vec<Option<usize>> {
        drawn
            .iter()
            .filter(|(text, _)| text == line)
            .map(|(_, place)| *place)
            .collect()
    };
    let order: Vec<&str> = drawn
        .iter()
        .map(|(text, _)| text.as_str())
        .filter(|text| text.starts_with("##") || text.starts_with("- ["))
        .collect();
    assert_eq!(
        order,
        [
            "## Needs your decision",
            "### Topic",
            "- [ ] **bravo** — Second question.",
            "## Waiting on an agent",
            "### Topic",
            "- [x] **alpha** — First question.",
        ]
    );
    assert_eq!(at("- [x] **alpha** — First question."), [Some(8)]);
    assert_eq!(
        at("  [2026-11-14 10:36] owner · approved — Keep it"),
        [Some(8)],
        "a line past the file's copy stands for its last"
    );
    assert_eq!(at("- [ ] **bravo** — Second question."), [Some(9)]);
    assert_eq!(at("## Needs your decision"), [Some(4)]);
    assert_eq!(
        at("## Waiting on an agent"),
        [None],
        "only the regroup wrote it"
    );
    assert_eq!(at("### Topic"), [Some(6), None]);
    assert_eq!(at("<!-- register:start -->"), [Some(3)]);
    assert_eq!(at("<!-- register:end -->"), [Some(10)]);
    assert_eq!(at("A note after the register."), [Some(12)]);
}

#[test]
fn a_card_shown_in_its_file_copys_group_draws_the_file_as_it_is() {
    assert_eq!(lines_and_places(FILE, REPLIED), None);
    assert_eq!(lines_and_places(FILE, ""), None, "no card shown");
    assert_eq!(
        lines_and_places(
            FILE,
            "- [x] **other** — Not on this page.\n  [2026-11-14 10:36] owner · approved — Keep it\n"
        ),
        None,
        "a card the file lacks"
    );
}

#[test]
fn a_register_a_save_would_refuse_is_drawn_as_it_is() {
    let foreign = FILE.replace(
        "### Topic\n",
        "### Topic\n\nA paragraph inside the register.\n",
    );
    assert_eq!(lines_and_places(&foreign, ANSWERED), None);
}

#[test]
fn every_copy_of_a_slug_keeps_its_own_lines() {
    // Two registers, each with an alpha; both shown answered by the one card.
    let second = "\n## Later\n\n<!-- register:start -->\n## Needs your decision\n\n### Other\n\n- [ ] **alpha** — First question.\n<!-- register:end -->\n";
    let text = format!("{FILE}{second}");
    let drawn = lines_and_places(&text, ANSWERED).expect("both regroup");
    let alphas: Vec<Option<usize>> = drawn
        .iter()
        .filter(|(line, _)| line.starts_with("- [x] **alpha**"))
        .map(|(_, place)| *place)
        .collect();
    let file_lines: Vec<usize> = splitlines(&text)
        .iter()
        .enumerate()
        .filter(|(_, line)| line.starts_with("- [ ] **alpha**"))
        .map(|(at, _)| at + 1)
        .collect();
    assert_eq!(alphas, file_lines.into_iter().map(Some).collect::<Vec<_>>());
}

#[test]
fn a_section_stands_for_the_first_to_the_last_file_line_it_holds() {
    let places = [Some(3), None, Some(4), Some(4), None];
    assert_eq!(place(&places, 1, 4), (3, 4));
    assert_eq!(place(&places, 3, 3), (4, 4));
    assert_eq!(
        place(&places, 2, 2),
        (0, 0),
        "a heading only the regroup wrote"
    );
    assert_eq!(place(&places, 5, 9), (0, 0), "past the text");
    assert_eq!(place(&places, 0, 1), (3, 3));
}

/// The page `docs/D.md` of a tree holding `text`, `shown` its one cards: each section's key, the
/// group heading above it and its place.
fn page(text: &str, shown: &str) -> (Vec<(String, usize, usize)>, symphony_api::Document) {
    let tree = Tree::new();
    tree.write("docs/D.md", text);
    let render = Render::new(&tree.root);
    let doc = super::super::document_showing(&render, "docs/D.md", "", &[], &cards(shown)).unwrap();
    let placed = doc
        .sections
        .iter()
        .map(|section| (section.key.clone(), section.start, section.end))
        .collect();
    (placed, doc)
}

#[test]
fn a_page_draws_a_card_shown_answered_under_its_group_at_the_files_place() {
    let (placed, doc) = page(FILE, ANSWERED);
    let (file, _) = page(FILE, "");
    let keys: Vec<&str> = placed.iter().map(|(key, _, _)| key.as_str()).collect();
    let bravo = keys.iter().position(|key| *key == "card:bravo").unwrap();
    let alpha = keys.iter().position(|key| *key == "card:alpha").unwrap();
    let waiting = doc
        .sections
        .iter()
        .position(|section| section.html.contains("Waiting on an agent"))
        .expect("the group the answer puts alpha in");
    assert!(bravo < waiting && waiting < alpha, "{keys:?}");
    assert_eq!(placed[waiting].1, 0, "the file has no such heading");
    for key in ["card:alpha", "card:bravo"] {
        let place = |placed: &[(String, usize, usize)]| {
            placed
                .iter()
                .find(|(k, _, _)| k == key)
                .map(|(_, start, end)| (*start, *end))
        };
        assert_eq!(place(&placed), place(&file), "{key} keeps the file's lines");
    }
    // Its key follows the headings drawn above it, as on the page the answer was saved on.
    let last = |placed: &[(String, usize, usize)]| placed.last().map(|(_, s, e)| (*s, *e));
    assert_eq!(
        last(&placed),
        last(&file),
        "a section after the register too"
    );
    let alpha_card = doc.cards.iter().find(|card| card.slug == "alpha").unwrap();
    assert_eq!(
        (alpha_card.state.as_str(), alpha_card.group.as_str()),
        ("approved", "agent")
    );
    assert!(doc.threads["alpha"].html.contains("Keep it"));
    assert!(
        doc.sections
            .iter()
            .any(|section| section.html.contains("Needs your decision (1)")),
        "the group's count is the cards drawn under it"
    );
}

#[test]
fn a_page_draws_a_card_shown_in_its_file_copys_group_in_place() {
    let (placed, doc) = page(FILE, REPLIED);
    let (file, _) = page(FILE, "");
    assert_eq!(placed, file, "the file's sections, at the file's lines");
    assert!(doc.threads["alpha"].html.contains("A detail."));
}
