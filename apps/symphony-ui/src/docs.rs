//! Document snapshots and keyed feed changes. Only a free section takes new markup; source line
//! positions always advance, so a comment submitted during an update still targets its block.

use std::collections::{BTreeMap, BTreeSet};

use symphony_api::{Block, Document, DocumentCard, DocumentChange, Section, ThreadAuthor};

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
/// the owner's more than `before`, the lines its latest copy had when the save was sent
/// ([`released_from`]), and more than a release of it still waiting asks for.
pub fn release(released: &mut BTreeMap<String, usize>, key: String, before: usize) {
    let lines = released.get(&key).map_or(before, |&had| before.max(had)) + 1;
    released.insert(key, lines);
}

/// The lines of the owner's a card save's release waits past, if any. A save the server wrote now
/// waits past `before`, the card's lines when it was sent. A save the server answered as a `repeat`
/// of a tap it had written writes no line: it waits past the card's lines when that tap was first
/// sent, if it was this page's save that no reply answered (`retried`); else it releases nothing,
/// since `before` may already count the line the tap did write, and a release waiting for a line
/// that never comes would keep the card from holding its place for good.
pub fn released_from(repeat: bool, before: usize, retried: Option<&Unanswered>) -> Option<usize> {
    if repeat {
        retried.map(|retried| retried.before)
    } else {
        Some(before)
    }
}

/// What a save in a card says: its verdict (`message` for a message) and its note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Words {
    pub action: String,
    pub note: String,
}

impl Words {
    pub fn new(action: &str, note: &str) -> Self {
        Self {
            action: action.into(),
            note: note.into(),
        }
    }
}

/// A save in a card that no reply answered (`send` gave up waiting, or the server's answer could
/// not be read): the server may have written it. The words sent under its tap, in the order last
/// sent (a retry under the tap may say other words, and the server holds whichever reached it
/// first, if any), the tap, and the lines of the owner's the card had when it was first sent.
#[derive(Clone, Debug, PartialEq)]
pub struct Unanswered {
    pub sent: Vec<Words>,
    pub tap: Option<String>,
    pub before: usize,
}

impl Unanswered {
    /// The card `key` in `latest` shows a line of the owner's more than it had when this was first
    /// sent: the save landed, and the feed, which brings every change, is its answer.
    pub fn answered(&self, latest: &Document, key: &str) -> bool {
        owner_lines(latest, key) > self.before
    }

    /// The card's unanswered save once `words`, sent under `tap` while the card had `before` lines
    /// of the owner's, went unanswered too, after `earlier`. A retry under the earlier save's tap
    /// adds its words to the earlier ones and still waits past the lines of the first sending, since
    /// one tap writes at most one line, which may be an earlier one's; any other save starts anew.
    pub fn after(earlier: Option<Self>, words: Words, tap: Option<&str>, before: usize) -> Self {
        let (mut sent, before) = earlier
            .filter(|earlier| earlier.retried_by(tap))
            .map_or((Vec::new(), before), |earlier| {
                (earlier.sent, earlier.before)
            });
        sent.retain(|old| *old != words);
        sent.push(words);
        Self {
            sent,
            tap: tap.map(String::from),
            before,
        }
    }

    /// Whether a save sent under `tap` retried this one: the server writes one verdict line for a
    /// tap, so a reply to it answers this save too.
    pub fn retried_by(&self, tap: Option<&str>) -> bool {
        self.tap.is_some() && self.tap.as_deref() == tap
    }

    /// The words last sent under the tap.
    pub fn last(&self) -> Option<&Words> {
        self.sent.last()
    }

    /// The words the server holds, once the card `key`'s latest copy has its line ([`answered`]):
    /// the only words sent, else the one sending whose verdict and plain words the card's newest
    /// line of the owner's shows. `None` when the page cannot tell which: words with markup, or
    /// two sendings that read alike.
    ///
    /// [`answered`]: Self::answered
    pub fn held(&self, latest: &Document, key: &str) -> Option<Words> {
        if let [only] = self.sent.as_slice() {
            return Some(only.clone());
        }
        let (verdict, text) = newest_owner_line(latest, key)?;
        let mut matching = self.sent.iter().filter(|words| {
            Some(words.action.as_str()) == verdict.as_deref() && collapse(&words.note) == text
        });
        let found = matching.next()?;
        matching.next().is_none().then(|| found.clone())
    }
}

/// The verdict and the words, as plain text with its spaces collapsed, of the newest line of the
/// owner's on the card `key` in `document`; `None` without one, or when its words hold more than
/// plain text, emphasis, code and links.
fn newest_owner_line(document: &Document, key: &str) -> Option<(Option<String>, String)> {
    let thread = document.threads.get(key.strip_prefix("card:")?)?;
    let entry = thread
        .blocks
        .iter()
        .flat_map(|block| match block {
            Block::Thread { entries } => entries.as_slice(),
            _ => &[],
        })
        .rfind(|entry| entry.who == ThreadAuthor::Owner)?;
    let mut text = String::new();
    for block in &entry.blocks {
        let Block::Paragraph { spans } = block else {
            return None;
        };
        text.push(' ');
        plain(spans, &mut text)?;
    }
    Some((entry.verdict.clone(), collapse(&text)))
}

/// `spans` as plain text onto `text`; `None` for anything but text, emphasis, code and links.
fn plain(spans: &[symphony_api::Span], text: &mut String) -> Option<()> {
    use symphony_api::Span;
    for span in spans {
        match span {
            Span::Text { text: words } | Span::Code { text: words } => text.push_str(words),
            Span::Emphasis { spans }
            | Span::Strong { spans }
            | Span::Link { spans, .. }
            | Span::WikiLink { spans, .. } => plain(spans, text)?,
            Span::LineBreak => text.push(' '),
            _ => return None,
        }
    }
    Some(())
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What the editor says once the server has `action` (a verdict, `message` or `comment`).
pub fn saved_text(action: &str) -> String {
    match action {
        "message" => "Message sent".into(),
        "comment" => "Comment saved".into(),
        _ => format!("Verdict saved: {action}"),
    }
}

/// How a card's answered save leaves its editor: what it says, whether the box is emptied and
/// whether the form closes.
#[derive(Debug, PartialEq, Eq)]
pub struct Ending {
    pub status: String,
    pub clear: bool,
    pub close: bool,
}

/// How a card's save ends once answered, by `held`, the words the server holds under its tap
/// (`None` when the page cannot tell), against `last`, the words last sent under it, and `boxed`,
/// what the box holds now. Only words the server holds are cleared or said to be saved. Words
/// sent and not held stay as the card's draft with the form closed, the save being answered; words
/// the owner has written since sending stay as they are, the form open.
pub fn ending(held: Option<&Words>, last: &Words, boxed: &str) -> Ending {
    let unchanged = boxed == last.note;
    match held {
        Some(held) if unchanged && held == last => Ending {
            status: saved_text(&held.action),
            clear: true,
            close: true,
        },
        Some(held) => Ending {
            status: if unchanged {
                format!(
                    "{}; your change was not sent: send it again to add it",
                    saved_text(&held.action)
                )
            } else {
                saved_text(&held.action)
            },
            clear: false,
            close: unchanged,
        },
        None => Ending {
            status:
                "Verdict saved; the card shows its words: send yours again if they are not there"
                    .into(),
            clear: false,
            close: unchanged,
        },
    }
}

/// The tap a verdict in a card goes under, if not a new one: the tap of the card's save that no
/// reply answered, whatever the words now say, so a retry after "no answer" never writes a second
/// verdict line; else the one `stored` for the card (`ID\nSIGNATURE`, kept until a reply answers it)
/// when it was for this same `signature`, a retry across a reload.
pub fn tap(
    unanswered: Option<&Unanswered>,
    stored: Option<&str>,
    signature: &str,
) -> Option<String> {
    if let Some(tap) = unanswered.and_then(|unanswered| unanswered.tap.clone()) {
        return Some(tap);
    }
    let (id, sent) = stored?.split_once('\n')?;
    (sent == signature && !id.is_empty()).then(|| id.to_string())
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

/// A section's body as a page draws it: the section with its blocks and, for a card, its card and
/// thread, as `/api/section` answers them and this device keeps them
/// ([`crate::cache::document::SECTIONS`], which the page writes as the server sent them).
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
pub struct Body {
    pub section: Section,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<DocumentCard>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<symphony_api::Fragment>,
}

/// The bodies `document` holds of the sections `keys` names: each one drawn, with its card and
/// thread.
pub fn bodies(document: &Document, keys: &BTreeSet<String>) -> Vec<Body> {
    document
        .sections
        .iter()
        .filter(|section| keys.contains(&section.key) && !section.blocks.is_empty())
        .map(|section| {
            let slug = section.key.strip_prefix("card:");
            Body {
                section: section.clone(),
                card: slug
                    .and_then(|slug| document.cards.iter().find(|card| card.slug == slug))
                    .cloned(),
                thread: slug.and_then(|slug| document.threads.get(slug)).cloned(),
            }
        })
        .collect()
}

/// `bodies` drawn into `document`, an outline's, whose sections have no blocks: the section of the
/// same key takes its body's blocks and keeps its place from the outline, and a card's takes its
/// thread and the card's whole text, its state the outline's. A body whose section the outline no
/// longer names is left out.
pub fn draw_bodies(document: &mut Document, bodies: Vec<Body>) {
    for body in bodies {
        let key = body.section.key;
        let Some(section) = document.sections.iter_mut().find(|s| s.key == key) else {
            continue;
        };
        section.blocks = body.section.blocks;
        let Some(slug) = key.strip_prefix("card:") else {
            continue;
        };
        if let Some((kept, card)) = body
            .card
            .zip(document.cards.iter_mut().find(|card| card.slug == slug))
        {
            card.text = kept.text;
        }
        match body.thread {
            Some(thread) => {
                document.threads.insert(slug.to_string(), thread);
            }
            None => {
                document.threads.remove(slug);
            }
        }
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

    fn card(slug: &str, state: &str, text: &str) -> DocumentCard {
        DocumentCard {
            slug: slug.into(),
            state: state.into(),
            text: text.into(),
            group: "decide".into(),
            open: true,
        }
    }

    /// The document as an outline holds it: its sections' places and hashes with no blocks, no
    /// threads, and its cards' text cut short.
    fn outlined(mut doc: Document) -> Document {
        for section in &mut doc.sections {
            section.blocks.clear();
        }
        doc.threads.clear();
        for card in &mut doc.cards {
            card.text = card.text.chars().take(3).collect();
        }
        doc
    }

    #[test]
    fn a_documents_bodies_are_its_drawn_sections_with_their_cards_and_threads() {
        let mut doc = thread(
            document(&[("intro", "i1"), ("card:alpha", "a1"), ("card:bravo", "b1")]),
            "alpha",
            1,
            1,
        );
        doc.cards = vec![
            card("alpha", "pending", "Alpha's whole text"),
            card("bravo", "approved", "Bravo"),
        ];
        doc.sections[2].blocks.clear();
        let taken = bodies(&doc, &keys(&["intro", "card:alpha", "card:bravo", "gone"]));
        // Bravo has no body drawn yet, and no section is called gone.
        assert_eq!(
            taken
                .iter()
                .map(|b| b.section.key.as_str())
                .collect::<Vec<_>>(),
            ["intro", "card:alpha"]
        );
        assert_eq!((&taken[0].card, &taken[0].thread), (&None, &None));
        assert_eq!(taken[1].card.as_ref().unwrap().text, "Alpha's whole text");
        assert_eq!(taken[1].thread.as_ref(), doc.threads.get("alpha"));
        assert!(bodies(&doc, &BTreeSet::new()).is_empty());
    }

    #[test]
    fn bodies_drawn_into_an_outline_keep_its_places_and_states_and_bring_their_threads() {
        let mut full = thread(
            document(&[("intro", "i1"), ("card:alpha", "a1"), ("card:bravo", "b1")]),
            "alpha",
            2,
            0,
        );
        full.cards = vec![
            card("alpha", "pending", "Alpha's whole text"),
            card("bravo", "pending", "Bravo"),
        ];
        let kept = bodies(&full, &keys(&["intro", "card:alpha", "card:bravo"]));
        // Since they were kept the sections moved, alpha was answered and intro went.
        let mut outline = outlined(full.clone());
        outline.sections.remove(0);
        outline.sections[0].start = 40;
        outline.cards[0].state = "approved".into();
        let mut drawn = outline.clone();
        draw_bodies(&mut drawn, kept);
        assert_eq!(
            drawn
                .sections
                .iter()
                .map(|s| (s.key.as_str(), s.start))
                .collect::<Vec<_>>(),
            [("card:alpha", 40), ("card:bravo", 0)],
            "the outline's sections in its places, the body it no longer names left out"
        );
        assert!(drawn.sections.iter().all(|s| !s.blocks.is_empty()));
        assert_eq!(drawn.cards[0].state, "approved", "the outline's state");
        assert_eq!(drawn.cards[0].text, "Alpha's whole text", "the kept text");
        assert_eq!(drawn.threads.get("alpha"), full.threads.get("alpha"));
        assert!(!drawn.threads.contains_key("bravo"));
        // A body with no thread takes away one the document held.
        let mut held = drawn.clone();
        held.threads
            .insert("bravo".into(), full.threads["alpha"].clone());
        draw_bodies(&mut held, bodies(&drawn, &keys(&["card:bravo"])));
        assert!(!held.threads.contains_key("bravo"));
        // A card body without its card keeps the outline's text.
        let mut bare = outline.clone();
        let mut alpha = bodies(&drawn, &keys(&["card:alpha"]));
        alpha[0].card = None;
        draw_bodies(&mut bare, alpha);
        assert_eq!(bare.cards[0].text, outline.cards[0].text);
        // Nothing drawn leaves the outline as it was.
        let mut untouched = outline.clone();
        draw_bodies(&mut untouched, Vec::new());
        assert_eq!(untouched, outline);
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

        release(&mut released, "card:alpha".into(), before);
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
        release(&mut released, "card:alpha".into(), before);
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
        release(&mut released, "card:alpha".into(), 0);
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
        release(&mut released, "card:alpha".into(), 1);
        release(&mut released, "card:alpha".into(), 1);
        assert_eq!(released.get("card:alpha"), Some(&3));
        settle(&mut released, &thread(document(&BEFORE), "alpha", 2, 0));
        assert_eq!(released.len(), 1, "the first answer alone shows");
        release(&mut released, "card:alpha".into(), 3);
        assert_eq!(
            released.get("card:alpha"),
            Some(&4),
            "a save after the first two arrived waits for its own"
        );
        settle(&mut released, &thread(document(&BEFORE), "alpha", 4, 0));
        assert!(released.is_empty());
    }

    /// A retried tap whose first post landed but whose reply was lost, across a reload: the server
    /// answers it as a repeat and writes no line. It releases nothing, and leaves a release still
    /// waiting as it was; the card shown with the first post's line ends nothing it did not start.
    #[test]
    fn a_tap_the_server_answers_as_a_repeat_releases_nothing() {
        let mut released = BTreeMap::new();
        assert_eq!(released_from(true, 2, None), None);
        let held = super::held(
            &keys(&["card:alpha"]),
            &BTreeSet::new(),
            &released,
            [],
            &thread(document(&BEFORE), "alpha", 2, 0),
        );
        assert_eq!(held, keys(&["card:alpha"]), "an open form still holds it");

        release(&mut released, "card:bravo".into(), 0);
        if let Some(before) = released_from(true, 1, None) {
            release(&mut released, "card:bravo".into(), before);
        }
        assert_eq!(
            released,
            BTreeMap::from([("card:bravo".to_string(), 1)]),
            "a repeat leaves another save's release as it was"
        );
        assert_eq!(
            released_from(false, 1, None),
            Some(1),
            "a save written now waits past the lines it was sent beside"
        );
    }

    fn unanswered(action: &str, note: &str, tap: Option<&str>, before: usize) -> Unanswered {
        Unanswered {
            sent: vec![Words::new(action, note)],
            tap: tap.map(String::from),
            before,
        }
    }

    /// `doc` with the card `slug`'s thread holding `lines`, each the owner's verdict and its words.
    fn answered(
        mut doc: Document,
        slug: &str,
        lines: &[(&str, Vec<symphony_api::Span>)],
    ) -> Document {
        let entries = lines
            .iter()
            .map(|(verdict, spans)| symphony_api::ThreadEntry {
                who: ThreadAuthor::Owner,
                timestamp: None,
                verdict: Some(verdict.to_string()),
                blocks: vec![Block::Paragraph {
                    spans: spans.clone(),
                }],
            })
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

    fn words(text: &str) -> Vec<symphony_api::Span> {
        vec![symphony_api::Span::Text { text: text.into() }]
    }

    /// A save `send` gave up on, while the owner's form holds the card: the feed's line of the
    /// owner's is its answer. An agent's reply is not; the owner's line releases the card, which
    /// then shows it, regrouped, and the release ends.
    #[test]
    fn the_feeds_line_of_the_owners_answers_a_save_no_reply_answered_and_releases_its_card() {
        let mut state = DocumentState::new(thread(document(&BEFORE), "alpha", 0, 1));
        let sent = unanswered(
            "approved",
            "Keep it",
            Some("ui-1"),
            owner_lines(&state.latest, "card:alpha"),
        );
        let engaged = keys(&["card:alpha"]);
        let none = BTreeSet::new();
        let mut released = BTreeMap::new();

        state.latest = thread(document(&BEFORE), "alpha", 0, 2);
        assert!(
            !sent.answered(&state.latest, "card:alpha"),
            "an agent's reply is not the owner's answer"
        );
        state.latest = thread(document(&AFTER), "alpha", 1, 2);
        assert!(!sent.answered(&state.latest, "card:bravo"), "another card");
        assert!(sent.answered(&state.latest, "card:alpha"));
        let held = super::held(&engaged, &none, &released, [], &state.latest);
        state.refresh(&held);
        assert_eq!(
            shown(&state),
            [
                ("needs", "n2"),
                ("card:alpha", "a1"),
                ("card:bravo", "b1"),
                ("waiting", "w1")
            ],
            "unconfirmed, the open form holds the card as it was"
        );

        release(&mut released, "card:alpha".into(), sent.before);
        let held = super::held(&engaged, &none, &released, [], &state.latest);
        assert!(held.is_empty(), "{held:?}");
        state.refresh(&held);
        settle(&mut released, &state.shown);
        assert_eq!(shown(&state), AFTER, "the answer shows, regrouped");
        assert!(released.is_empty(), "the card shows the owner's line");
    }

    /// While a card's save is unanswered, a retry goes under its tap whatever the words now say: the
    /// server writes one verdict line for a tap. Without one, only the same verdict and words take
    /// the stored tap; anything else is a new answer.
    #[test]
    fn a_retry_after_no_answer_takes_the_unanswered_tap_whatever_its_words() {
        let sent = unanswered("approved", "Keep it", Some("ui-1"), 0);
        let stored = Some("ui-1\napproved\nKeep it");
        assert_eq!(
            tap(Some(&sent), stored, "rejected\nOn second thought"),
            Some("ui-1".into())
        );
        assert_eq!(
            tap(Some(&sent), None, "approved\nKeep it"),
            Some("ui-1".into()),
            "the page's own record, whatever storage kept"
        );
        assert_eq!(
            tap(None, stored, "approved\nKeep it"),
            Some("ui-1".into()),
            "the same answer after a reload"
        );
        assert_eq!(tap(None, stored, "approved\nKeep it all"), None);
        assert_eq!(tap(None, None, "approved\nKeep it"), None);
        assert_eq!(tap(None, Some(""), ""), None, "a cleared tap");
        assert_eq!(tap(None, Some("ui-1"), "ui-1"), None, "no signature");
        assert_eq!(
            tap(
                Some(&unanswered("message", "Hi", None, 0)),
                stored,
                "rejected\nNo"
            ),
            None,
            "an unanswered message has no tap to retry"
        );
    }

    /// A retry under the unanswered tap that is unanswered too keeps every sending's words, the
    /// latest last, and still waits past the first sending's lines: only one line comes for the
    /// tap, maybe the first one's, already counted by the retry.
    #[test]
    fn an_unanswered_retry_keeps_every_sendings_words_and_the_first_sendings_lines() {
        let first = unanswered("approved", "Keep it", Some("ui-1"), 1);
        let retry = Unanswered::after(
            Some(first.clone()),
            Words::new("rejected", "No"),
            Some("ui-1"),
            2,
        );
        assert_eq!(
            retry,
            Unanswered {
                sent: vec![
                    Words::new("approved", "Keep it"),
                    Words::new("rejected", "No")
                ],
                tap: Some("ui-1".into()),
                before: 1
            }
        );
        assert_eq!(retry.last(), Some(&Words::new("rejected", "No")));
        assert!(retry.retried_by(Some("ui-1")));
        assert!(!retry.retried_by(Some("ui-2")));
        assert!(!retry.retried_by(None));
        let again = Unanswered::after(
            Some(retry),
            Words::new("approved", "Keep it"),
            Some("ui-1"),
            3,
        );
        assert_eq!(
            again.sent,
            [
                Words::new("rejected", "No"),
                Words::new("approved", "Keep it")
            ],
            "the first words sent again are the latest, not twice"
        );

        let other = Unanswered::after(Some(first), Words::new("message", "Hi"), None, 2);
        assert_eq!(
            other,
            unanswered("message", "Hi", None, 2),
            "a save under no tap of the first waits past its own"
        );
        assert!(!other.retried_by(None), "no tap retries nothing");
        assert_eq!(
            Unanswered::after(None, Words::new("approved", "Keep it"), Some("ui-3"), 4),
            unanswered("approved", "Keep it", Some("ui-3"), 4)
        );
    }

    /// The server answers a retry under the unanswered tap as a repeat: the release waits past the
    /// first sending's lines.
    #[test]
    fn a_repeat_of_an_unanswered_tap_releases_from_its_first_sending() {
        let first = unanswered("approved", "Keep it", Some("ui-1"), 1);
        assert_eq!(released_from(true, 2, Some(&first)), Some(1));
        assert_eq!(released_from(false, 2, Some(&first)), Some(2));

        let mut released = BTreeMap::new();
        release(&mut released, "card:alpha".into(), 1);
        settle(&mut released, &thread(document(&BEFORE), "alpha", 2, 0));
        assert!(released.is_empty(), "the first save's line ends it");
    }

    /// Which words the feed's line answers: the only words sent, whatever the line shows; else the
    /// sending whose verdict and plain words the newest line of the owner's shows, and none when no
    /// sending or two read so, or the line holds more than plain words.
    #[test]
    fn the_feeds_line_tells_which_words_under_a_tap_the_server_holds() {
        let keep = Words::new("approved", "Keep it");
        let why = Words::new("approved", "Keep it, and say why");
        let one = unanswered("approved", "Keep it", Some("ui-1"), 0);
        let latest = answered(document(&AFTER), "alpha", &[("approved", words("Other"))]);
        assert_eq!(one.held(&latest, "card:alpha"), Some(keep.clone()));

        let two = Unanswered::after(Some(one), why.clone(), Some("ui-1"), 0);
        for (line, held) in [
            (words("Keep it"), Some(&keep)),
            (words("  Keep\n it "), Some(&keep)),
            (words("Keep it, and say why"), Some(&why)),
            (
                vec![
                    symphony_api::Span::Strong {
                        spans: words("Keep"),
                    },
                    symphony_api::Span::LineBreak,
                    symphony_api::Span::Code { text: "it".into() },
                ],
                Some(&keep),
            ),
            (words("Neither"), None),
            (
                vec![symphony_api::Span::Component {
                    kind: "status".into(),
                    blocks: Vec::new(),
                }],
                None,
            ),
        ] {
            let latest = answered(
                document(&AFTER),
                "alpha",
                &[
                    ("rejected", words("An old answer")),
                    ("approved", line.clone()),
                ],
            );
            assert_eq!(two.held(&latest, "card:alpha").as_ref(), held, "{line:?}");
        }
        let latest = answered(document(&AFTER), "alpha", &[("rejected", words("Keep it"))]);
        assert_eq!(two.held(&latest, "card:alpha"), None, "another verdict");
        assert_eq!(two.held(&document(&AFTER), "card:alpha"), None, "no line");
        let alike = Unanswered::after(
            Some(two),
            Words::new("approved", "Keep  it"),
            Some("ui-1"),
            0,
        );
        let latest = answered(document(&AFTER), "alpha", &[("approved", words("Keep it"))]);
        assert_eq!(alike.held(&latest, "card:alpha"), None, "two read alike");
    }

    /// Only the words the server holds are cleared and said to be saved. Words sent and not held
    /// stay as the draft, the form closed; words written since stay in the open form; when the page
    /// cannot tell, nothing is said to be saved but the verdict.
    #[test]
    fn an_answered_save_clears_only_the_words_the_server_holds() {
        let keep = Words::new("approved", "Keep it");
        let why = Words::new("approved", "Keep it, and say why");
        let not_sent = "Verdict saved: approved; your change was not sent: send it again to add it";
        let cases = [
            (
                Some(&keep),
                &keep,
                "Keep it",
                "Verdict saved: approved",
                true,
                true,
            ),
            (
                Some(&keep),
                &keep,
                "Keep it, and more",
                "Verdict saved: approved",
                false,
                false,
            ),
            (
                Some(&keep),
                &why,
                "Keep it, and say why",
                not_sent,
                false,
                true,
            ),
            (
                Some(&keep),
                &why,
                "Something else",
                "Verdict saved: approved",
                false,
                false,
            ),
            (
                Some(&Words::new("rejected", "Keep it, and say why")),
                &why,
                "Keep it, and say why",
                "Verdict saved: rejected; your change was not sent: send it again to add it",
                false,
                true,
            ),
            (
                None,
                &why,
                "Keep it, and say why",
                "Verdict saved; the card shows its words: send yours again if they are not there",
                false,
                true,
            ),
            (
                None,
                &why,
                "Something else",
                "Verdict saved; the card shows its words: send yours again if they are not there",
                false,
                false,
            ),
        ];
        for (held, last, boxed, status, clear, close) in cases {
            assert_eq!(
                ending(held, last, boxed),
                Ending {
                    status: status.into(),
                    clear,
                    close
                },
                "{held:?} {last:?} {boxed:?}"
            );
        }
        let message = Words::new("message", "Hi");
        assert_eq!(
            ending(Some(&message), &message, "Hi").status,
            "Message sent"
        );
        assert_eq!(saved_text("comment"), "Comment saved");
    }

    /// Two sendings under one tap, then the feed: "Keep it" goes unanswered but lands; the owner
    /// sends other words under its tap, unanswered too; the feed brings the first words' line. The
    /// page says the first verdict was saved and keeps the other words as the draft.
    #[test]
    fn two_unanswered_sendings_then_the_feeds_line_keep_the_words_not_written() {
        let first = unanswered("approved", "Keep it", Some("ui-1"), 0);
        let sent = Unanswered::after(
            Some(first),
            Words::new("approved", "Keep it, and say why"),
            Some("ui-1"),
            0,
        );
        let latest = answered(document(&AFTER), "alpha", &[("approved", words("Keep it"))]);
        assert!(sent.answered(&latest, "card:alpha"));
        let held = sent.held(&latest, "card:alpha");
        assert_eq!(held, Some(Words::new("approved", "Keep it")));
        assert_eq!(
            ending(held.as_ref(), sent.last().unwrap(), "Keep it, and say why"),
            Ending {
                status:
                    "Verdict saved: approved; your change was not sent: send it again to add it"
                        .into(),
                clear: false,
                close: true
            }
        );
    }

    /// Two sendings under one tap, then a repeat: the other words once more, which the server
    /// answers as a repeat of the tap with the words it holds. They are the first words, so the
    /// other words stay as the draft.
    #[test]
    fn a_repeat_answering_other_words_than_the_tap_holds_keeps_them() {
        let first = unanswered("approved", "Keep it", Some("ui-1"), 0);
        let why = Words::new("approved", "Keep it, and say why");
        let sent = Unanswered::after(Some(first), why.clone(), Some("ui-1"), 0);
        assert_eq!(
            tap(
                Some(&sent),
                Some("ui-1\napproved\nKeep it, and say why"),
                "approved\nKeep it, and say why"
            ),
            Some("ui-1".into()),
            "the third sending goes under the same tap"
        );
        let held = Words::new("approved", "Keep it");
        assert_eq!(
            ending(Some(&held), &why, "Keep it, and say why"),
            Ending {
                status:
                    "Verdict saved: approved; your change was not sent: send it again to add it"
                        .into(),
                clear: false,
                close: true
            }
        );
        assert!(
            ending(Some(&why), &why, "Keep it, and say why").clear,
            "the tap holds these words: they are saved"
        );
    }

    /// A draft resumed after a reload: the other words were sent under the first tap and the page
    /// reloaded before their reply. The stored record takes the resumed words under that tap; the
    /// repeat answers with the first words, so the resumed words stay as the draft.
    #[test]
    fn a_reloaded_draft_under_the_first_tap_is_kept_when_the_tap_holds_other_words() {
        let stored = "ui-1\napproved\nKeep it, and say why";
        let why = Words::new("approved", "Keep it, and say why");
        assert_eq!(
            tap(None, Some(stored), "approved\nKeep it, and say why"),
            Some("ui-1".into())
        );
        let held = Words::new("approved", "Keep it");
        let end = ending(Some(&held), &why, "Keep it, and say why");
        assert!(!end.clear && end.close, "{end:?}");
        assert!(end.status.contains("your change was not sent"), "{end:?}");
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
