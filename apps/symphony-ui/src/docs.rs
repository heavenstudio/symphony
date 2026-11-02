//! Document snapshots and keyed feed changes. Only a free section takes new markup; source line
//! positions always advance, so a comment submitted during an update still targets its block.

use std::collections::BTreeSet;

use symphony_api::{Document, DocumentChange, Section};

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
