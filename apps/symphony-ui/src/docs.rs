//! Document snapshots and keyed feed changes. Only a free section takes new markup; source line
//! positions always advance, so a comment submitted during an update still targets its block.

use std::collections::{BTreeMap, BTreeSet};

use symphony_api::{Block, Document, DocumentChange, Section, ThreadAuthor};

#[derive(Clone, PartialEq)]
pub struct DocumentState {
    pub shown: Document,
    pub latest: Document,
    pub removed: bool,
}

impl DocumentState {
    pub fn new(document: Document) -> Self {
        Self {
            shown: document.clone(),
            latest: document,
            removed: false,
        }
    }

    pub fn change(&mut self, delta: DocumentChange) {
        if delta.path != self.latest.path {
            return;
        }
        if delta.removed {
            self.removed = true;
            return;
        }
        let doc = &mut self.latest;
        doc.sections
            .retain(|section| !delta.deleted.contains(&section.key));
        for section in delta.sections {
            match doc.sections.iter_mut().find(|old| old.key == section.key) {
                Some(old) => *old = section,
                None => doc.sections.push(section),
            }
        }
        for position in delta.positions {
            if let Some(section) = doc.sections.iter_mut().find(|s| s.key == position.key) {
                section.start = position.start;
                section.end = position.end;
                section.part = position.part;
            }
        }
        if let Some(order) = delta.order {
            doc.sections.sort_by_key(|section| {
                order
                    .iter()
                    .position(|key| key == &section.key)
                    .unwrap_or(usize::MAX)
            });
        }
        for (slug, fragment) in delta.threads {
            match fragment {
                Some(fragment) => {
                    doc.threads.insert(slug, fragment);
                }
                None => {
                    doc.threads.remove(&slug);
                }
            }
        }
        for card in delta.cards {
            if card.removed {
                doc.cards.retain(|old| old.slug != card.key);
            }
            if let Some(value) = card.value {
                match doc.cards.iter_mut().find(|old| old.slug == card.key) {
                    Some(old) => *old = value,
                    None => doc.cards.push(value),
                }
            }
        }
        if let Some(title) = delta.title {
            doc.title = title;
        }
        if let Some(waiting) = delta.waiting {
            doc.waiting = waiting;
        }
        if let Some(crumbs) = delta.crumbs {
            doc.crumbs = crumbs;
        }
        if let Some(headings) = delta.headings {
            doc.headings = headings;
        }
        if let Some(tasks) = delta.tasks {
            doc.tasks = tasks;
        }
        if let Some(comparison) = delta.comparison {
            doc.comparison = comparison.into_iter().next();
        }
    }

    pub fn refresh(&mut self, busy: &BTreeSet<String>) {
        let mut next = self.latest.clone();
        for old in &self.shown.sections {
            if !busy.contains(&old.key) {
                continue;
            }
            if let Some(section) = next.sections.iter_mut().find(|s| s.key == old.key) {
                let (start, end, part) = (section.start, section.end, section.part);
                *section = old.clone();
                section.start = start;
                section.end = end;
                section.part = part;
            } else {
                next.sections.push(old.clone());
            }
            if let Some(slug) = old.key.strip_prefix("card:") {
                if let Some(thread) = self.shown.threads.get(slug) {
                    next.threads.insert(slug.into(), thread.clone());
                } else {
                    next.threads.remove(slug);
                }
                if let Some(card) = self.shown.cards.iter().find(|c| c.slug == slug) {
                    if let Some(target) = next.cards.iter_mut().find(|c| c.slug == slug) {
                        *target = card.clone();
                    } else {
                        next.cards.push(card.clone());
                    }
                }
            }
        }
        // Do not move a busy block under the owner's pointer when the register regroups.
        if !busy.is_empty() {
            next.sections.sort_by_key(|s| {
                self.shown
                    .sections
                    .iter()
                    .position(|old| old.key == s.key)
                    .unwrap_or(usize::MAX)
            });
        }
        if self.shown != next {
            self.shown = next;
        }
    }
}

/// The sections whose shown markup and place stay as they are ([`DocumentState::refresh`]): those
/// the owner is `engaged` in (a field, an open form or excerpt), but for a card whose own save is
/// `released`; a section holding a closed draft only once it is gone from `latest`, so the draft can
/// still be copied; and the sections still `loading` their bodies.
pub fn held(
    engaged: &BTreeSet<String>,
    drafts: &BTreeSet<String>,
    released: &BTreeMap<String, usize>,
    loading: impl IntoIterator<Item = String>,
    latest: &Document,
) -> BTreeSet<String> {
    let mut held: BTreeSet<String> = engaged
        .iter()
        .filter(|key| !released.contains_key(*key))
        .cloned()
        .collect();
    held.extend(
        drafts
            .iter()
            .filter(|key| !latest.sections.iter().any(|s| &s.key == *key))
            .cloned(),
    );
    held.extend(loading);
    held
}

/// How many lines of the owner's the card `key` (`card:SLUG`) shows in `document`'s thread.
pub fn owner_lines(document: &Document, key: &str) -> usize {
    let Some(thread) = key
        .strip_prefix("card:")
        .and_then(|slug| document.threads.get(slug))
    else {
        return 0;
    };
    thread
        .blocks
        .iter()
        .map(|block| match block {
            Block::Thread { entries } => entries
                .iter()
                .filter(|entry| entry.who == ThreadAuthor::Owner)
                .count(),
            _ => 0,
        })
        .sum()
}

/// The owner's save in the card `key` went through: the card is released until it shows one line of
/// the owner's more than `before`, the lines its latest copy had when the save was sent, and more
/// than a release of it still waiting asks for. A save the server answered as a `repeat` of one it
/// had written (a retried tap whose first reply was lost) writes no line, and `before` may already
/// count the one it did write, so it releases nothing: a release that waited for a line that never
/// comes would keep the card from holding its place for good.
pub fn release(released: &mut BTreeMap<String, usize>, key: String, before: usize, repeat: bool) {
    if repeat {
        return;
    }
    let lines = released.get(&key).map_or(before, |&had| before.max(had)) + 1;
    released.insert(key, lines);
}

/// The cards the owner just saved in, by how many lines of the owner's each must show: the owner's
/// answer shows at once, whatever holds the card. A release ends once the feed has brought the
/// save's own change and the card shows it, or once the card is gone; a reply an agent wrote before
/// it, shown on the way, ends nothing.
pub fn settle(released: &mut BTreeMap<String, usize>, shown: &Document) {
    released.retain(|key, lines| {
        shown.sections.iter().any(|s| &s.key == key) && owner_lines(shown, key) < *lines
    });
}

pub fn fragment(section: &Section) -> symphony_api::Fragment {
    symphony_api::Fragment {
        blocks: section.blocks.clone(),
        html: String::new(),
        media: Vec::new(),
    }
}

/// A section keeps its keyed parent when the register regroups. Only the contiguous part's
/// visible edges change, so a focused editor never remounts to acquire its card's border.
#[cfg(any(all(target_arch = "wasm32", feature = "page-docs"), test))]
pub(crate) fn part_edges(sections: &[Section], index: usize) -> (bool, bool) {
    let Some(section) = sections.get(index) else {
        return (false, false);
    };
    let previous = index.checked_sub(1).and_then(|i| sections.get(i));
    let next = sections.get(index + 1);
    (
        previous.is_none_or(|s| s.part != section.part),
        next.is_none_or(|s| s.part != section.part),
    )
}

/// Client pages and API reads and writes keep the selected worktree's scope.
pub fn scoped(scope: &str, route: &str) -> String {
    if scope.is_empty() {
        route.into()
    } else {
        format!("/@{scope}{route}")
    }
}

/// A route's breadcrumbs, and whether they sit under the tasks page. A document or folder sits
/// under its folders, each crumb but the page's own a link to its folder; a task's slug is one
/// name under the tasks page, which its other crumb links to, since the parts of a slug are not
/// tasks of their own.
pub fn crumbs(path: &str) -> (Vec<&str>, bool) {
    let path = path.trim_end_matches('/');
    if let Some(slug) = path.strip_prefix("task/") {
        (vec!["tasks", slug], true)
    } else if path.is_empty() {
        (vec!["board"], false)
    } else {
        (path.split('/').collect(), false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_task_sits_under_the_tasks_page_and_a_document_under_its_folders() {
        assert_eq!(
            crumbs("task/product/polish-sweep/client-source"),
            (vec!["tasks", "product/polish-sweep/client-source"], true),
            "no crumb names a part of the slug as a task of its own"
        );
        assert_eq!(crumbs("task/solo/"), (vec!["tasks", "solo"], true));
        assert_eq!(
            crumbs("docs/workflow/REVIEW.md"),
            (vec!["docs", "workflow", "REVIEW.md"], false)
        );
        assert_eq!(crumbs("docs/lamps/"), (vec!["docs", "lamps"], false));
        assert_eq!(crumbs(""), (vec!["board"], false));
        assert_eq!(crumbs("tasks"), (vec!["tasks"], false));
        assert_eq!(
            crumbs("taskforce/notes.md"),
            (vec!["taskforce", "notes.md"], false),
            "only the task route's own prefix makes a slug"
        );
    }

    #[test]
    fn document_comparison_metadata_updates_and_clears_on_the_feed() {
        let doc: Document = serde_json::from_value(serde_json::json!({
            "version":0,"path":"@design~policy/docs/POLICY.md","title":"Policy",
            "waiting":[],"crumbs":[],"headings":[],"sections":[],"cards":[],"threads":{},"tasks":[]
        }))
        .unwrap();
        let mut state = DocumentState::new(doc);
        let comparison = symphony_api::DocumentComparison {
            task: "design/policy".into(),
        };
        let delta = DocumentChange {
            path: state.latest.path.clone(),
            comparison: Some(vec![comparison.clone()]),
            ..Default::default()
        };
        state.change(delta);
        state.refresh(&BTreeSet::new());
        assert_eq!(state.shown.comparison, Some(comparison));
        state.change(DocumentChange {
            path: state.latest.path.clone(),
            comparison: Some(Vec::new()),
            ..Default::default()
        });
        state.refresh(&BTreeSet::new());
        assert_eq!(state.shown.comparison, None);
    }

    #[test]
    fn part_edges_follow_contiguous_runs_including_absent_and_repeated_parts() {
        let sections: Vec<_> = [None, None, Some(0), Some(0), None]
            .into_iter()
            .map(|part| Section {
                start: 0,
                end: 0,
                key: String::new(),
                hash: String::new(),
                part,
                blocks: Vec::new(),
                html: String::new(),
                media: Vec::new(),
                comparison: None,
            })
            .collect();
        let edges: Vec<_> = (0..sections.len())
            .map(|index| part_edges(&sections, index))
            .collect();
        assert_eq!(
            edges,
            [
                (true, false),
                (false, true),
                (true, false),
                (false, true),
                (true, true)
            ]
        );
        assert_eq!(part_edges(&[], 0), (false, false));
        assert_eq!(part_edges(&sections, usize::MAX), (false, false));
    }

    fn section(key: &str, hash: &str) -> Section {
        Section {
            start: 0,
            end: 0,
            key: key.into(),
            hash: hash.into(),
            part: None,
            blocks: vec![symphony_api::Block::Paragraph { spans: Vec::new() }],
            html: String::new(),
            media: Vec::new(),
            comparison: None,
        }
    }

    fn document(sections: &[(&str, &str)]) -> Document {
        let mut doc: Document = serde_json::from_value(serde_json::json!({
            "version":0,"path":"docs/DECISIONS.md","title":"Decisions",
            "waiting":[],"crumbs":[],"headings":[],"sections":[],"cards":[],"threads":{},"tasks":[]
        }))
        .unwrap();
        doc.sections = sections.iter().map(|(k, h)| section(k, h)).collect();
        doc
    }

    fn shown(state: &DocumentState) -> Vec<(&str, &str)> {
        state
            .shown
            .sections
            .iter()
            .map(|s| (s.key.as_str(), s.hash.as_str()))
            .collect()
    }

    fn keys(keys: &[&str]) -> BTreeSet<String> {
        keys.iter().map(|k| k.to_string()).collect()
    }

    /// `doc` with the card `slug`'s thread: the owner's lines and the agents', in turn.
    fn thread(mut doc: Document, slug: &str, owner: usize, agent: usize) -> Document {
        let entry = |who| symphony_api::ThreadEntry {
            who,
            timestamp: None,
            verdict: None,
            blocks: Vec::new(),
        };
        let entries = std::iter::repeat_n(ThreadAuthor::Owner, owner)
            .chain(std::iter::repeat_n(ThreadAuthor::Agent, agent))
            .map(entry)
            .collect();
        doc.threads.insert(
            slug.into(),
            symphony_api::Fragment {
                blocks: vec![Block::Thread { entries }],
                html: String::new(),
                media: Vec::new(),
            },
        );
        doc
    }

    /// Alpha is answered and moves from the first group to the second, its excerpt open.
    const BEFORE: [(&str, &str); 3] = [("needs", "n1"), ("card:alpha", "a1"), ("card:bravo", "b1")];
    const AFTER: [(&str, &str); 4] = [
        ("needs", "n2"),
        ("card:bravo", "b1"),
        ("waiting", "w1"),
        ("card:alpha", "a2"),
    ];

    #[test]
    fn an_engaged_card_keeps_its_markup_and_the_page_its_order_until_its_own_save_releases_it() {
        let mut state = DocumentState::new(thread(document(&BEFORE), "alpha", 0, 1));
        let before = owner_lines(&state.latest, "card:alpha");
        state.latest = thread(document(&AFTER), "alpha", 1, 1);
        let engaged = keys(&["card:alpha"]);
        let none = BTreeSet::new();
        let mut released = BTreeMap::new();
        let held = super::held(&engaged, &none, &released, [], &state.latest);
        assert_eq!(held, engaged);
        state.refresh(&held);
        assert_eq!(
            shown(&state),
            [
                ("needs", "n2"),
                ("card:alpha", "a1"),
                ("card:bravo", "b1"),
                ("waiting", "w1")
            ],
            "another reader's open excerpt holds the card and the order"
        );

        release(&mut released, "card:alpha".into(), before, false);
        let held = super::held(&engaged, &none, &released, [], &state.latest);
        assert!(held.is_empty(), "{held:?}");
        state.refresh(&held);
        settle(&mut released, &state.shown);
        assert_eq!(
            shown(&state),
            AFTER,
            "the owner's own answer shows, regrouped"
        );
        assert!(
            released.is_empty(),
            "the release ends once the card shows the owner's line"
        );
    }

    /// An agent's reply lands while the owner reads an excerpt beside the card, held there; the owner
    /// saves an answer. The release shows the reply at once, but the answer is still to come: the
    /// release lasts until the feed brings the save's own change, then holds the card again.
    #[test]
    fn a_reply_shown_before_the_owners_own_change_arrives_keeps_the_release() {
        let mut state = DocumentState::new(thread(document(&BEFORE), "alpha", 1, 0));
        let engaged = keys(&["card:alpha"]);
        let none = BTreeSet::new();
        let mut released = BTreeMap::new();
        let tick = |state: &mut DocumentState, released: &mut BTreeMap<String, usize>| {
            let held = super::held(&engaged, &none, released, [], &state.latest);
            state.refresh(&held);
            settle(released, &state.shown);
        };
        state.latest = thread(document(&[("card:alpha", "a2")]), "alpha", 1, 1);
        tick(&mut state, &mut released);
        assert_eq!(
            shown(&state),
            [("card:alpha", "a1")],
            "the excerpt holds the reply"
        );

        let before = owner_lines(&state.latest, "card:alpha");
        release(&mut released, "card:alpha".into(), before, false);
        tick(&mut state, &mut released);
        assert_eq!(shown(&state), [("card:alpha", "a2")]);
        assert_eq!(
            released.get("card:alpha"),
            Some(&2),
            "the reply is not the owner's answer: the release waits for it"
        );

        state.latest = thread(document(&[("card:alpha", "a3")]), "alpha", 2, 1);
        tick(&mut state, &mut released);
        assert_eq!(
            shown(&state),
            [("card:alpha", "a3")],
            "the owner's answer shows"
        );
        assert!(released.is_empty());

        state.latest = thread(document(&[("card:alpha", "a4")]), "alpha", 2, 2);
        tick(&mut state, &mut released);
        assert_eq!(
            shown(&state),
            [("card:alpha", "a3")],
            "the excerpt holds the card again"
        );
    }

    #[test]
    fn a_release_lasts_until_its_card_shows_the_owners_line_or_is_gone() {
        let mut released = BTreeMap::new();
        release(&mut released, "card:alpha".into(), 0, false);
        settle(&mut released, &document(&BEFORE));
        assert_eq!(released.len(), 1, "the feed has not brought the answer yet");
        settle(&mut released, &thread(document(&BEFORE), "bravo", 1, 0));
        assert_eq!(released.len(), 1, "another card's line is not this one's");
        settle(&mut released, &document(&[("needs", "n1")]));
        assert!(released.is_empty(), "the card is gone");
    }

    /// The owner saves twice before the first save's change arrives: the release waits for both.
    #[test]
    fn a_second_save_before_the_first_shows_waits_for_both() {
        let mut released = BTreeMap::new();
        release(&mut released, "card:alpha".into(), 1, false);
        release(&mut released, "card:alpha".into(), 1, false);
        assert_eq!(released.get("card:alpha"), Some(&3));
        settle(&mut released, &thread(document(&BEFORE), "alpha", 2, 0));
        assert_eq!(released.len(), 1, "the first answer alone shows");
        release(&mut released, "card:alpha".into(), 3, false);
        assert_eq!(
            released.get("card:alpha"),
            Some(&4),
            "a save after the first two arrived waits for its own"
        );
        settle(&mut released, &thread(document(&BEFORE), "alpha", 4, 0));
        assert!(released.is_empty());
    }

    /// A retried tap whose first post landed but whose reply was lost: the server answers it as a
    /// repeat and writes no line. It releases nothing, and leaves a release still waiting as it was;
    /// the card shown with the first post's line ends nothing it did not start.
    #[test]
    fn a_tap_the_server_answers_as_a_repeat_releases_nothing() {
        let mut released = BTreeMap::new();
        release(&mut released, "card:alpha".into(), 2, true);
        assert!(released.is_empty(), "{released:?}");
        let held = super::held(
            &keys(&["card:alpha"]),
            &BTreeSet::new(),
            &released,
            [],
            &thread(document(&BEFORE), "alpha", 2, 0),
        );
        assert_eq!(held, keys(&["card:alpha"]), "an open form still holds it");

        release(&mut released, "card:bravo".into(), 0, false);
        release(&mut released, "card:bravo".into(), 1, true);
        assert_eq!(
            released,
            BTreeMap::from([("card:bravo".to_string(), 1)]),
            "a repeat leaves another save's release as it was"
        );
    }

    #[test]
    fn a_card_still_loading_its_body_stays_held_though_released() {
        let released = BTreeMap::from([("card:alpha".to_string(), 1)]);
        let held = super::held(
            &keys(&["card:alpha"]),
            &BTreeSet::new(),
            &released,
            ["card:alpha".to_string()],
            &document(&AFTER),
        );
        assert_eq!(held, keys(&["card:alpha"]));
    }

    #[test]
    fn a_closed_draft_holds_its_card_only_once_the_card_is_gone() {
        let drafts = keys(&["card:bravo"]);
        let none = BTreeSet::new();
        let released = BTreeMap::new();
        let mut state = DocumentState::new(document(&BEFORE));
        state.latest = document(&AFTER);
        let held = super::held(&none, &drafts, &released, [], &state.latest);
        assert!(
            held.is_empty(),
            "a draft elsewhere lets the register regroup"
        );
        state.refresh(&held);
        assert_eq!(shown(&state), AFTER);

        state.latest = document(&[("needs", "n3"), ("waiting", "w1"), ("card:alpha", "a2")]);
        let held = super::held(&none, &drafts, &released, [], &state.latest);
        assert_eq!(held, drafts);
        state.refresh(&held);
        assert_eq!(
            shown(&state),
            [
                ("needs", "n3"),
                ("card:bravo", "b1"),
                ("waiting", "w1"),
                ("card:alpha", "a2")
            ],
            "a removed card with a draft stays where it was, for the draft to be copied"
        );
    }
}
