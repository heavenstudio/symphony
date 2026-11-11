//! The read models of the review board, queue, activity and pending navigation. No HTML rendering
//! is involved in a snapshot or a delta: the SPA applies the JSON to its own regions.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use symphony_api::{
    ActivityItem, Board, Card, Counts, Pending, PendingItem, Progress, Queue, QueueItem,
};

use crate::plugins::Plugins;
use crate::{Decision, Doc, Group, ReviewError, State, Who, board, branches, clock, tree, triage};

/// The register's own card markers, which a card's title and summary leave out with a plugin's.
const MARKERS: [&str; 5] = ["Replaces", "Depends", "After", "Blocks", "Gates"];
static ASK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Approve(?:,| and)[^.]*\.").unwrap());
static REF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[(?:([a-z][a-z-]*):)?([^\]|]*)(?:\|([^\]]*))?\]\]").unwrap());
static IMAGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"!\[([^\]]*)\]\([^)]*\)").unwrap());
static LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[([^\]]*)\]\([^)]*\)").unwrap());
static CUT_OFF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"!?\[[^\]]*(?:\]\([^)]*)?$").unwrap());
static SENTENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(.+?[.!?])(?:\s|$)").unwrap());

pub fn plain(text: &str) -> String {
    let refs = REF.replace_all(text, |c: &regex::Captures<'_>| {
        if c.get(1).is_some() {
            c[2].trim().to_string()
        } else {
            c.get(3)
                .map_or_else(
                    || c[2].split('#').next().unwrap_or("").trim(),
                    |m| m.as_str().trim(),
                )
                .to_string()
        }
    });
    let images = IMAGE.replace_all(&refs, "$1");
    let cut = CUT_OFF.replace_all(&images, "");
    LINK.replace_all(&cut, "$1")
        .replace(['*', '`'], "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn cut(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let short: String = text.chars().take(limit - 1).collect();
    let words = short
        .rsplit_once(' ')
        .map_or(short.as_str(), |(rest, _)| rest);
    format!("{}…", words.trim_end_matches([',', ';', ':', '—', '-']))
}

/// `text` without each marker `names` names: `*Name:*` and the words after it, up to the next `*`.
fn unmarked(text: &str, names: &[&str]) -> String {
    let mut kept = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('*') {
        let after = &rest[at + 1..];
        let marked = names
            .iter()
            .find_map(|name| after.strip_prefix(name)?.strip_prefix(":*"));
        if let Some(words) = marked {
            kept.push_str(&rest[..at]);
            rest = words.find('*').map_or("", |end| &words[end..]);
        } else {
            kept.push_str(&rest[..=at]);
            rest = after;
        }
    }
    kept.push_str(rest);
    kept
}

fn words(plugins: &Plugins, card: &Decision) -> String {
    let mut names = MARKERS.to_vec();
    names.extend(plugins.markers());
    plain(&ASK.replace_all(&unmarked(&card.text, &names), ""))
}

fn first_sentence(text: &str) -> &str {
    SENTENCE
        .captures(text)
        .and_then(|c| c.get(1))
        .map_or(text, |m| m.as_str())
}

pub fn card_title(plugins: &Plugins, card: &Decision) -> String {
    plugins.slot(card).map_or_else(
        || cut(first_sentence(&words(plugins, card)), 110),
        |slot| format!("{slot} picture"),
    )
}

pub fn card_summary(plugins: &Plugins, card: &Decision) -> String {
    cut(&words(plugins, card), 260)
}

pub fn card_rest(plugins: &Plugins, card: &Decision) -> String {
    let text = words(plugins, card);
    let rest = if plugins.slot(card).is_some() {
        text.as_str()
    } else {
        text.strip_prefix(first_sentence(&text))
            .unwrap_or("")
            .trim()
    };
    cut(rest, 260)
}

/// The home's progress in the checkout at `root`: its plugins' regions and the design documents.
fn progress(root: &Path, docs: &[Doc]) -> Progress {
    let pages: Vec<_> = docs
        .iter()
        .filter(|d| !d.path.ends_with("/README.md"))
        .collect();
    Progress {
        regions: crate::plugins::Plugins::of(root)
            .home_regions()
            .iter()
            .filter_map(|region| region.region(root))
            .collect(),
        docs: count(pages.len()),
        decisions_waiting: count(
            pages
                .iter()
                .flat_map(|d| &d.decisions)
                .filter(|c| c.state == State::Pending)
                .count(),
        ),
    }
}

pub(crate) fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// A decision's document, independent of the checkout serving its proposed version.
pub fn document_path(path: &str) -> &str {
    path.strip_prefix('@')
        .and_then(|p| p.split_once('/'))
        .map_or(path, |(_, path)| path)
}

/// All document-backed read regions from one scan of the tree.
pub struct Snapshot {
    pub board: Board,
    pub queue: Queue,
    pub pending: Pending,
    pub activity: Vec<ActivityItem>,
    pub order: std::sync::Arc<crate::postpone::View>,
}

/// The same proposed version of each decision drives both triage and the owner's queue order.
pub(crate) fn merge_decisions<'a>(
    docs: &mut Vec<Doc>,
    branch_cards: &'a [branches::BranchCard],
) -> BTreeMap<(String, &'a str), &'a branches::BranchCard> {
    let mut proposals = BTreeMap::new();
    for branch in branch_cards {
        proposals
            .entry((branch.path(), branch.card.slug.as_str()))
            .or_insert(branch);
    }
    let proposed_slugs: BTreeSet<_> = branch_cards.iter().map(|b| b.card.slug.as_str()).collect();
    for doc in docs.iter_mut() {
        doc.decisions
            .retain(|card| !proposed_slugs.contains(card.slug.as_str()));
    }
    for branch in proposals.values().copied() {
        let path = branch.path();
        if let Some(doc) = docs.iter_mut().find(|d| d.path == path) {
            doc.decisions.push(branch.card.clone());
        } else {
            docs.push(Doc {
                path,
                comments: Vec::new(),
                decisions: vec![branch.card.clone()],
            });
        }
    }
    proposals
}

pub fn snapshot(
    root: &Path,
    branch_cards: &[branches::BranchCard],
    build: &str,
) -> Result<Snapshot, ReviewError> {
    // A checkout with no documents yet still has its board, branch cards and agents.
    let mut docs = board::documents(root)?;
    let mut progress = progress(root, &docs);
    // Distinct proposals remain separate decisions; only main's older copy is suppressed.
    // Branch discovery already removes identical copies carried by stacked branches.
    let proposals = merge_decisions(&mut docs, branch_cards);
    let task_names = if branch_cards.is_empty() {
        BTreeMap::new()
    } else {
        branches::task_names(root)
    };
    let plugins = Plugins::of(root);
    let project = crate::project::Project::of(root);
    let ranked = triage::queue(&plugins, &docs, usize::MAX, "");
    let mut queue = Queue::default();
    for item in &ranked {
        let branch = proposals
            .values()
            .copied()
            .find(|b| b.path() == item.doc && b.card.slug == item.slug);
        let Some(card) = docs
            .iter()
            .find(|d| d.path == item.doc)
            .and_then(|d| d.decisions.iter().find(|c| c.slug == item.slug))
        else {
            continue;
        };
        let text = words(&plugins, card);
        let summary = if plugins.slot(card).is_some() {
            text.as_str()
        } else {
            text.strip_prefix(first_sentence(&text))
                .unwrap_or("")
                .trim()
        };
        queue.waiting.push(Card {
            postponed: false,
            slug: card.slug.clone(),
            doc: item.doc.clone(),
            doc_title: tree::title(&branch.map_or_else(
                || root.join(project.file(&item.doc)),
                |b| b.tree.path.join(project.file(&b.doc)),
            )),
            title: card_title(&plugins, card),
            summary: cut(summary, 260),
            why: item
                .reason
                .replace("design · gates nothing known", "design call"),
            posted: card.posted_at(),
            pictures: {
                let mut found = match branch {
                    Some(branch) => plugins.pictures(&branch.tree.path, &branch.doc, card),
                    None => plugins.pictures(root, &item.doc, card),
                };
                if let Some(branch) = branch {
                    for src in found.iter_mut().filter_map(|p| p.src.as_mut()) {
                        *src = crate::docview::scoped(
                            &crate::markup::target(src, &branch.doc),
                            &branch.tree.id(),
                        );
                    }
                }
                found
            },
            task: branch.map(|b| {
                task_names
                    .get(&b.tree.branch)
                    .unwrap_or(&b.tree.branch)
                    .clone()
            }),
        });
    }
    let queue_order =
        std::sync::Arc::new(crate::postpone::View::new(&plugins, &docs, &queue.waiting));
    match queue_order.waiting(root, crate::postpone::OWNER, crate::postpone::now()) {
        Ok(waiting) => queue.waiting = waiting,
        Err(error) => {
            eprintln!(
                "· could not apply postponed queue order; using mixed order without preferences: {error}"
            );
            queue.waiting = crate::postpone::mixed(queue.waiting);
        }
    }
    let today = clock::now_stamp();
    let mut answered = 0;
    let mut activity = Vec::new();
    for doc in &docs {
        for comment in doc.comments.iter().filter(|c| c.open()) {
            queue.agent.push(QueueItem {
                href: format!("/{}#L{}", doc.path, comment.line),
                path: doc.path.clone(),
                slug: format!("L{}", comment.line),
                title: tree::title(&root.join(project.file(&doc.path))),
                kind: comment.kind.clone(),
                text: comment.title.clone(),
                branch: None,
            });
        }
        for card in &doc.decisions {
            let entries = card.entries();
            let item = QueueItem {
                href: format!("/{}#{}", doc.path, card.slug),
                path: doc.path.clone(),
                slug: card.slug.clone(),
                title: card_title(&plugins, card),
                kind: card.state.as_str().to_string(),
                text: plain(
                    &entries
                        .last()
                        .map_or_else(|| card.note.join(" "), |e| e.text.clone()),
                ),
                branch: None,
            };
            if card.group(&plugins) == Group::Auto {
                queue.spot_check.push(item.clone());
            }
            if card.open(&plugins) {
                queue.agent.push(item);
            }
            let mut said = false;
            let mut answered_today = false;
            for (index, entry) in entries.iter().enumerate() {
                if entry.who == Who::Owner {
                    said = true;
                } else if said
                    && entry.verdict.is_none()
                    && entry
                        .ts
                        .as_deref()
                        .is_some_and(|ts| ts.get(..10) == today.get(..10))
                {
                    answered_today = true;
                }
                if entry.who != Who::Agent {
                    continue;
                }
                let Some(ts) = &entry.ts else {
                    continue;
                };
                activity.push(ActivityItem {
                    id: format!("{}#{}#{index}", doc.path, card.slug),
                    kind: if index == 0 { "new card" } else { "reply" }.into(),
                    path: doc.path.clone(),
                    slug: card.slug.clone(),
                    ts: ts.clone(),
                    text: entry.text.chars().take(160).collect(),
                    href: format!("/{}#L{}", doc.path, card.line),
                    answered: entries[index + 1..].iter().any(|e| e.who == Who::Owner),
                });
            }
            answered += u32::from(answered_today);
        }
    }
    let order: Vec<String> = crate::project::Project::of(root)
        .entrypoints
        .iter()
        .flat_map(|entry| tree::index_order(root, &entry.url))
        .collect();
    let mut pending = Pending {
        docs: order.clone(),
        items: Vec::new(),
    };
    for path in order {
        if let Some(doc) = docs.iter().find(|doc| doc.path == path) {
            for card in doc.decisions.iter().filter(|card| {
                queue
                    .waiting
                    .iter()
                    .any(|c| c.doc == path && c.slug == card.slug)
            }) {
                pending.items.push(PendingItem {
                    path: path.clone(),
                    href: format!("/{path}#{}", card.slug),
                    slug: card.slug.clone(),
                    branch: None,
                });
            }
        }
    }
    for branch in proposals.values().copied().filter(|b| {
        queue
            .waiting
            .iter()
            .any(|c| c.doc == b.path() && c.slug == b.card.slug)
    }) {
        let path = branch.path();
        let href = format!("/{path}#{}", branch.card.slug);
        if !pending.docs.contains(&path) {
            pending.docs.push(path.clone());
        }
        pending.items.push(PendingItem {
            path,
            slug: branch.card.slug.clone(),
            href,
            branch: Some(branch.tree.branch.clone()),
        });
    }
    activity.sort_by(|a, b| (&b.ts, &b.id).cmp(&(&a.ts, &a.id)));
    activity.truncate(40);
    let counts = Counts {
        waiting: count(queue.waiting.len()),
        docs_waiting: count(
            queue
                .waiting
                .iter()
                .map(|c| document_path(&c.doc))
                .collect::<BTreeSet<_>>()
                .len(),
        ),
        spot_check: count(queue.spot_check.len()),
        answered_today: answered,
        tasks: None,
    };
    progress.decisions_waiting = counts.waiting;
    let board = Board {
        version: 0,
        build: build.into(),
        ping_ms: symphony_api::PING_MAX_MS,
        next: queue
            .waiting
            .iter()
            .take(symphony_api::BOARD_CARDS)
            .cloned()
            .collect(),
        counts,
        progress,
    };
    Ok(Snapshot {
        board,
        queue,
        pending,
        activity,
        order: queue_order,
    })
}
