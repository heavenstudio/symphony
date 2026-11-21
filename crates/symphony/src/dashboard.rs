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
use crate::{Decision, Doc, Group, ReviewError, Who, board, branches, clock, tree, triage};

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

/// How many characters of what was said the activity panel shows.
pub const EXCERPT_CHARS: usize = 160;

/// The markup that opens a line: its quote markers, a callout's (`[!note]`), then a heading's
/// marks or a list item's bullet or number and task box.
static LINE_MARKUP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^\s*(?:>\s?)*(?:\[![A-Za-z-]+\][+-]?\s*)?(?:#{1,6}\s+|(?:[-*+]|\d+[.)])\s+(?:\[[ xX?]\]\s+)?)?",
    )
    .unwrap()
});
/// A code fence's own line, quoted or not, with its language word.
static FENCE_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(?:>\s?)*(?:```|~~~)").unwrap());
/// An HTML tag: `<details>`, `</summary>`, `<br/>`.
static TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"</?[A-Za-z][A-Za-z0-9-]*(?:\s[^<>]*)?/?>").unwrap());

/// What the activity panel shows of `text`: its words without their markup, block or inline, cut
/// at a word to [`EXCERPT_CHARS`]. A code fence's lines are markup; the code inside it is words.
pub fn excerpt(text: &str) -> String {
    let lines: Vec<_> = text
        .lines()
        .filter(|line| !FENCE_LINE.is_match(line))
        .map(|line| LINE_MARKUP.replace(line, ""))
        .collect();
    cut(
        &plain(&TAG.replace_all(&lines.join("\n"), " ")),
        EXCERPT_CHARS,
    )
}

/// `text` whole when it holds at most `limit` characters; else its words that fit before an
/// ellipsis, the cut at a word's end and no dangling comma, colon or dash before the `…`.
pub(crate) fn cut(text: &str, limit: usize) -> String {
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

/// What a callout says it is about: its title, else the first line of its body, in plain words.
fn callout_title(comment: &crate::callouts::Comment) -> Option<String> {
    std::iter::once(&comment.title)
        .chain(&comment.body)
        .map(|line| plain(line))
        .find(|line| !line.is_empty())
        .map(|line| cut(&line, 110))
}

/// The home's progress in the checkout at `root`: its plugins' regions and the design documents,
/// counted as the first entrypoint's folder page counts them, the page its "browse all" opens.
/// The decisions waiting are the queue's, counted once the queue is built.
fn progress(root: &Path) -> Progress {
    let project = crate::project::Project::of(root);
    Progress {
        regions: crate::plugins::Plugins::of(root)
            .home_regions()
            .iter()
            .filter_map(|region| region.region(root))
            .collect(),
        docs: count(
            project
                .entrypoints
                .first()
                .map_or(0, |entry| tree::count_documents(root, &entry.url)),
        ),
        decisions_waiting: 0,
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
    /// The one card each page shows in place of its file's copy ([`Merged::pages`]).
    pub pages: std::sync::Arc<Pages>,
    /// Every card a page holds, waiting or not ([`Held`]): main's, and each branch proposal under
    /// its branch's page. An owner's ask on a card no page holds is not answered
    /// ([`crate::owner_asks`]).
    pub held: std::sync::Arc<Held>,
    /// The owner's dated lines under each of those cards, by slug, on every copy: what ends a
    /// task's block on one ([`crate::decided`]).
    pub said: std::sync::Arc<crate::decided::Said>,
}

/// Cards by page path and slug, each with when the owner last wrote under that page's copy, in
/// seconds since the epoch: none when no line of the owner's on it is dated. A branch's ask on a
/// card is answered on main's copy only when that copy carries the owner's line, merged or laid
/// there from the branch's worktree ([`crate::owner_asks`]).
pub type Held = BTreeMap<(String, String), Option<f64>>;

/// Every card `docs` and `pages` hold, a page's copy in place of its file's.
fn held(docs: &[Doc], pages: &Pages) -> Held {
    let cards = |path: &String, cards: &[Decision]| {
        cards
            .iter()
            .map(|card| ((path.clone(), card.slug.clone()), owner_said(card)))
            .collect::<Vec<_>>()
    };
    let mut held = BTreeMap::new();
    for (key, at) in docs
        .iter()
        .flat_map(|doc| cards(&doc.path, &doc.decisions))
        .chain(pages.iter().flat_map(|(path, shown)| cards(path, shown)))
    {
        held.insert(key, at);
    }
    held
}

/// The owner's lines under every card `docs` and `pages` hold ([`crate::decided::said`]).
fn said(docs: &[Doc], pages: &Pages) -> crate::decided::Said {
    crate::decided::said(
        docs.iter()
            .flat_map(|doc| doc.decisions.iter().map(|card| (doc.path.as_str(), card)))
            .chain(
                pages
                    .iter()
                    .flat_map(|(path, cards)| cards.iter().map(|card| (path.as_str(), card))),
            ),
    )
}

/// When the owner last wrote under `card`, by its dated lines.
fn owner_said(card: &Decision) -> Option<f64> {
    card.entries()
        .iter()
        .filter(|entry| entry.who == Who::Owner)
        .filter_map(|entry| entry.ts.as_deref().and_then(crate::verdicts::epoch))
        .reduce(f64::max)
}

/// By page path (`docs/X.md` on main, `@<tree>/docs/X.md` on a branch), the cards that page shows as
/// one card made of its copies ([`branches::one_card`]) rather than as its file holds them.
pub type Pages = BTreeMap<String, Vec<Decision>>;

/// What [`merge_decisions`] made of the branch cards.
pub(crate) struct Merged<'a> {
    /// Each branch's proposal the owner is to read, by its page and slug.
    pub proposals: BTreeMap<(String, &'a str), &'a branches::BranchCard>,
    /// The one card each page shows in place of its file's copy.
    pub pages: Pages,
}

/// Main's copy of `slug` in the page `path`, if main has the card there.
fn main_copy<'a>(docs: &'a [Doc], path: &str, slug: &str) -> Option<&'a Decision> {
    docs.iter()
        .find(|d| d.path == path)
        .and_then(|d| d.decisions.iter().find(|c| c.slug == slug))
}

/// The branch cards put into `docs`, main's copies of their slugs taken out: each branch's proposal
/// is one card with main's copy ([`branches::one_card`]), under the branch's page, unless main's
/// copy already says everything it does or closed the card after every line of it
/// ([`branches::stale`]); a finished branch's owner lines join main's copy where it is, on the same
/// terms. The same proposed version of each decision drives both triage and the owner's queue
/// order.
pub(crate) fn merge_decisions<'a>(
    docs: &mut Vec<Doc>,
    branch_cards: &'a [branches::BranchCard],
    plugins: &Plugins,
) -> Merged<'a> {
    let mut pages: Pages = BTreeMap::new();
    for owner in branch_cards.iter().filter(|b| b.on_main) {
        let Some(doc) = docs.iter_mut().find(|d| d.path == owner.doc) else {
            continue;
        };
        let Some(main) = doc.decisions.iter_mut().find(|c| c.slug == owner.card.slug) else {
            continue;
        };
        if branches::stale(main, branches::closed(plugins, main), &owner.card) {
            continue;
        }
        let one = branches::with_owner_lines(main, &owner.card);
        if !branches::same(main, &one) {
            *main = one.clone();
            let shown = pages.entry(owner.doc.clone()).or_default();
            shown.retain(|c| c.slug != one.slug);
            shown.push(one);
        }
    }
    let mut proposals = BTreeMap::new();
    let mut ones: Vec<(&branches::BranchCard, Decision, bool)> = Vec::new();
    for branch in branch_cards.iter().filter(|b| !b.on_main) {
        if proposals.contains_key(&(branch.path(), branch.card.slug.as_str())) {
            continue;
        }
        let main = main_copy(docs, &branch.doc, &branch.card.slug);
        if main.is_some_and(|main| {
            branches::stale(main, branches::closed(plugins, main), &branch.card)
        }) {
            continue; // main closed the card after the branch's copy said its last
        }
        let one = main.map_or_else(
            || branch.card.clone(),
            |main| branches::one_card(main, &branch.card),
        );
        if main.is_some_and(|main| branches::same(main, &one)) {
            continue; // main's copy already says all the branch's does
        }
        proposals.insert((branch.path(), branch.card.slug.as_str()), branch);
        ones.push((branch, one, main.is_some()));
    }
    let proposed_slugs: BTreeSet<_> = ones.iter().map(|(b, _, _)| b.card.slug.as_str()).collect();
    for doc in docs.iter_mut() {
        doc.decisions
            .retain(|card| !proposed_slugs.contains(card.slug.as_str()));
    }
    for (branch, one, on_main_too) in &ones {
        let path = branch.path();
        // Main's page shows the one card too, when it is the only proposal of its slug.
        if *on_main_too
            && ones
                .iter()
                .filter(|(other, _, _)| other.card.slug == branch.card.slug)
                .count()
                == 1
        {
            pages
                .entry(branch.doc.clone())
                .or_default()
                .push(one.clone());
        }
        pages.entry(path.clone()).or_default().push(one.clone());
        if let Some(doc) = docs.iter_mut().find(|d| d.path == path) {
            doc.decisions.push(one.clone());
        } else {
            docs.push(Doc {
                path,
                comments: Vec::new(),
                decisions: vec![one.clone()],
            });
        }
    }
    Merged { proposals, pages }
}

pub fn snapshot(
    root: &Path,
    branch_cards: &[branches::BranchCard],
    build: &str,
) -> Result<Snapshot, ReviewError> {
    // A checkout with no documents yet still has its board, branch cards and agents.
    let mut docs = board::documents_shown(root)?;
    let mut progress = progress(root);
    // Distinct proposals remain separate decisions; only main's older copy is suppressed.
    // Branch discovery already removes identical copies carried by stacked branches.
    let plugins = Plugins::of(root);
    let Merged { proposals, pages } = merge_decisions(&mut docs, branch_cards, &plugins);
    // One read of the registers: a plugin's process is asked every card at once.
    let plugins = plugins.reading(root, &docs);
    let held = held(&docs, &pages);
    let said = said(&docs, &pages);
    let task_names = if proposals.is_empty() {
        BTreeMap::new()
    } else {
        branches::task_names(root)
    };
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
        let open: Vec<_> = doc.comments.iter().filter(|c| c.open()).collect();
        // A callout is named by its own title, its document the context it is read in.
        let doc_title = if open.is_empty() {
            String::new()
        } else {
            tree::title(&root.join(project.file(&doc.path)))
        };
        for comment in open {
            queue.agent.push(QueueItem {
                href: format!("/{}#L{}", doc.path, comment.line),
                path: doc.path.clone(),
                slug: format!("L{}", comment.line),
                title: callout_title(comment).unwrap_or_else(|| doc_title.clone()),
                kind: comment.kind.clone(),
                text: doc_title.clone(),
                branch: None,
                ask: None,
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
                ask: None,
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
                // An agent's status line with nothing said (`agent · resolved`), or nothing but
                // markup, closes the reply above it: it is no reply of its own.
                let said = excerpt(&entry.text);
                if said.is_empty() {
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
                    text: said,
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
        pages: std::sync::Arc::new(pages),
        held: std::sync::Arc::new(held),
        said: std::sync::Arc::new(said),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verdicts::epoch;

    #[test]
    fn an_excerpt_is_the_words_without_their_markup() {
        assert_eq!(
            excerpt(
                "Posted for `perf/x`.\n\n    **Today.** See [[analysis/x#Why|the analysis]] and [it](x.md)."
            ),
            "Posted for perf/x. Today. See the analysis and it."
        );
        assert_eq!(excerpt(""), "");
    }

    #[test]
    fn an_excerpt_leaves_out_quote_and_callout_markers() {
        assert_eq!(
            excerpt("> [!note] The driver\n> drops below its minimum."),
            "The driver drops below its minimum."
        );
        assert_eq!(
            excerpt("> [!WARNING]- Folded\n> > Deeper."),
            "Folded Deeper."
        );
    }

    #[test]
    fn an_excerpt_leaves_out_heading_marks() {
        assert_eq!(excerpt("## Why\n\nBecause."), "Why Because.");
        assert_eq!(excerpt("> ### Quoted"), "Quoted");
    }

    #[test]
    fn an_excerpt_leaves_out_list_bullets_numbers_and_task_boxes() {
        assert_eq!(
            excerpt("- one\n* two\n+ three\n1. four\n2) five\n- [x] six\n    - [ ] seven"),
            "one two three four five six seven"
        );
    }

    #[test]
    fn an_excerpt_leaves_out_a_code_fences_lines_and_keeps_its_code() {
        assert_eq!(
            excerpt("Done.\n```rust\nfn main() {}\n```\n~~~\nx\n~~~"),
            "Done. fn main() {} x"
        );
    }

    #[test]
    fn an_excerpt_leaves_out_html_tags() {
        assert_eq!(
            excerpt("<details><summary>More</summary>\n\nThe rest.<br/>\n</details>"),
            "More The rest."
        );
    }

    #[test]
    fn an_excerpt_keeps_what_only_looks_like_markup() {
        let said = "#3 is next, at 2.5 GB: -1 left, a < b > c, see <https://x.org>.";
        assert_eq!(excerpt(said), said);
    }

    #[test]
    fn an_excerpt_of_nothing_but_markup_is_empty() {
        for said in ["**", "> ", "```\n```", "- ", "## ", "<details></details>"] {
            assert_eq!(excerpt(said), "", "{said:?}");
        }
    }

    #[test]
    fn a_long_excerpt_is_cut_at_a_word_to_its_length() {
        let said = "word ".repeat(60);
        let shown = excerpt(&format!("**{said}**"));
        assert!(shown.chars().count() <= EXCERPT_CHARS, "{shown}");
        assert!(shown.ends_with("word…"), "{shown}");
        let exact = "x".repeat(EXCERPT_CHARS);
        assert_eq!(excerpt(&exact), exact, "one that fits is whole");
    }

    fn cards(text: &str) -> Vec<Decision> {
        crate::register::parse_decisions(&text.lines().collect::<Vec<_>>())
    }

    const LAMP_A: &str = "- [-] **lamp-a** — Lamp A.\n  \
                          [2026-10-06 10:00] owner · rejected — too pale.\n  \
                          [2026-10-06 10:20] owner · note — and the base.\n  \
                          [2026-10-06 10:30] agent — Paler now.\n";
    const LAMP_B: &str = "- [ ] **lamp-b** — Lamp B.\n  [2026-10-06 09:00] agent — Posted.\n";

    #[test]
    fn a_held_card_says_when_the_owner_last_wrote_under_the_copy_its_page_shows() {
        let docs = vec![Doc {
            path: "docs/A.md".into(),
            comments: Vec::new(),
            decisions: cards(&format!("{LAMP_A}{LAMP_B}")),
        }];
        let key = |page: &str, slug: &str| (page.to_string(), slug.to_string());
        let files = held(&docs, &BTreeMap::new());
        assert_eq!(
            files[&key("docs/A.md", "lamp-a")],
            epoch("2026-10-06 10:20"),
            "the owner's last line, not the agent's after it"
        );
        assert_eq!(
            files[&key("docs/A.md", "lamp-b")],
            None,
            "no line of the owner's"
        );
        // The copy a page shows in place of its file's: main's with a branch's owner line laid on
        // it, and a branch's page's own.
        let laid = format!("{LAMP_A}  [2026-10-06 11:00] owner · rejected — still pale.\n");
        let pages = Pages::from([
            ("docs/A.md".to_string(), cards(&laid)),
            ("@pic~x/docs/A.md".to_string(), cards(LAMP_B)),
        ]);
        let shown = held(&docs, &pages);
        assert_eq!(
            shown[&key("docs/A.md", "lamp-a")],
            epoch("2026-10-06 11:00")
        );
        assert_eq!(shown[&key("@pic~x/docs/A.md", "lamp-b")], None);
        assert_eq!(shown.len(), 3);
    }
}
