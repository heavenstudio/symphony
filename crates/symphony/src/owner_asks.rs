//! The owner's asks ([`symphony_board::Ask`], docs/product/analysis/owner-asks.md) closed as the Symphony
//! reads their answers. An ask on a card closes when the scan that sorts the cards holds the card,
//! on the page the owner saw it on or main's, no longer waiting on an agent; one on a comment when
//! the comment is resolved, in its document or in the review ledger it was archived to; a chat when
//! its thread in the ask store ([`crate::asks`]) is answered. Each names the session that answered
//! when one is known: a chat's answer names its own, and `symphony reply --as HANDLE` records the
//! one that answered on a card or a comment ([`crate::asks::Reply`]), since the register's lines
//! say only `agent`. An ask on a task closes in the board's own write of the post that answers it,
//! and any ask by hand with `just tasks asks close`: neither is read here.
//!
//! Absence is no answer. An ask whose card, comment, document, worktree or thread is gone stays
//! open and says so ([`symphony_board::Store::mark_gone`]) until it is answered, it is found again,
//! or someone closes it by hand. A reading that could not list the worktrees, scan the worktree a
//! branch's card is on, or read what an ask is about, changes nothing.
//!
//! The shared server alone closes them, on its board clock, from main's page
//! ([`crate::dashboard::snapshot`]): its agent queue and the cards it held, main's and each unmerged
//! branch's own. A branch's ask on a card is read on main's copy only when that copy carries the
//! owner's line. A comment is read in its document, the worktree's for a branch's. An ask the owner
//! said more to after the documents were read waits for the next scan, which reads that line too.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use symphony_api::QueueItem;
use symphony_board::{Ask, Store};

use crate::asks::{AskStore, Reply, Thread};

/// How long the shared server goes at most without looking at the open asks while nothing it
/// watches changed, in seconds: an ask opened or answered some other way (the board's API, a
/// worktree that went) still closes within it.
pub const RESCAN_SECONDS: f64 = 60.0;

/// Whether `symphony verdict-asks` ([`crate::verdicts::turn`]) also opens an ask for each
/// comment already open in main's documents ([`open_comments`]), aged from when its line was
/// written; otherwise only comments saved from then on are asks. The design's second open question (docs/product/analysis/owner-asks.md,
/// Open questions), not decided: built off until the owner says.
pub const COMMENTS_OPEN_BEFORE: bool = false;

/// What one scan read: the documents' agent queue and the cards they held, the worktrees, and,
/// when an ask needs them, the ask store's threads and replies.
#[derive(Debug, Clone, Default)]
pub struct Reading {
    /// Main's checkout.
    pub root: PathBuf,
    /// The agent queue of main's page.
    pub agent: Vec<QueueItem>,
    /// Every card main's page held, waiting or not, by page (`docs/X.md`, `@<tree>/docs/X.md`) and
    /// slug, with when the owner last wrote under it ([`crate::dashboard::Snapshot::held`]).
    pub held: Arc<crate::dashboard::Held>,
    /// When the documents the queue came from began to be read, in seconds since the epoch.
    pub read_at: f64,
    /// The served worktrees by id ([`crate::branches::Tree::id`]).
    pub trees: BTreeMap<String, PathBuf>,
    /// Whether `trees` is a listing of the worktrees: one that failed says nothing of where a
    /// worktree's ask is.
    pub listed: bool,
    /// The branches of the listed worktrees whose scan failed ([`crate::branches::Scan::failed`]):
    /// what `held` lacks of them says nothing of where their cards are.
    pub unscanned: BTreeSet<String>,
    /// The ask store's threads and when they began to be read; none when it was not read.
    pub chats: Option<(Vec<Thread>, f64)>,
    /// Who replied on a card or a comment in the last day ([`crate::asks::AskStore::replies`]).
    pub replies: Vec<Reply>,
}

/// What a reading says of an open ask ([`read`]).
#[derive(Debug, Clone, PartialEq)]
pub enum Read {
    /// It cannot say: the reading began before the owner's last line, the ask names no card,
    /// comment or thread, what it is about could not be read, the worktrees could not be listed,
    /// or a branch card's worktree could not be scanned. Nothing changes.
    Unsure,
    /// It still waits on an agent where it is: a mark that it was gone comes off.
    Waiting,
    /// Answered: by the session that answered, when one is known, and where the answer is.
    Answered(Option<String>, String),
    /// What it is about is gone, in these words. No answer: the ask stays open and says so.
    Gone(String),
}

/// The page a card or comment the owner saw on the worktree of `tree` is served at: main's own
/// `path`, or `@<tree id>/path`.
fn page(tree: Option<&str>, path: &str) -> String {
    match tree {
        Some(branch) => format!("@{}/{path}", tree_id(branch)),
        None => path.to_string(),
    }
}

/// A branch as one URL segment ([`crate::branches::Tree::id`]).
fn tree_id(branch: &str) -> String {
    branch.replace('/', "~")
}

/// Whether a queue item is a comment: its slug is `L` and its line.
fn is_comment(item: &QueueItem) -> bool {
    item.slug
        .strip_prefix('L')
        .is_some_and(|line| !line.is_empty() && line.bytes().all(|b| b.is_ascii_digit()))
}

/// What the open `ask` is, as `reading` reads it.
pub fn read(ask: &Ask, reading: &Reading) -> Read {
    match ask.kind.as_str() {
        "verdict" | "message" => read_card(ask, reading),
        "comment" => read_comment(ask, reading),
        "chat" => read_chat(ask, reading),
        _ => Read::Unsure,
    }
}

/// How far before the minute an ask was opened in its owner's line may be dated, in seconds: a save
/// dates the line and opens the ask in one request, which may cross a minute.
const LINE_SLACK: f64 = 60.0;

/// An ask on a card: waiting while main's page has it waiting on an agent, on the page the owner
/// saw it on; answered once the page holds it there waiting on no one; gone when it holds it
/// nowhere. A main card a branch proposes again is that branch's page's one card. A branch's card
/// that left the branch's own page is main's copy only when that copy carries the owner's line
/// (merged, or laid there from the worktree, [`crate::dashboard::Held`]): main's copy without
/// it, as when the branch's worktree was removed unmerged, says nothing of what the branch's
/// agent did, and the ask is gone.
fn read_card(ask: &Ask, reading: &Reading) -> Read {
    let (Some(path), Some(card)) = (ask.path.as_deref(), ask.card.as_deref()) else {
        return Read::Unsure;
    };
    let tree = ask.tree.as_deref();
    if ask.last >= reading.read_at
        || tree.is_some_and(|branch| !reading.listed || reading.unscanned.contains(branch))
    {
        return Read::Unsure;
    }
    let shown = page(tree, path);
    let held_at = |page: &str| reading.held.get(&(page.to_string(), card.to_string()));
    let on = |page: &str| on_page(ask, path, card, page, &reading.held);
    if reading
        .agent
        .iter()
        .any(|item| item.slug == card && on(&item.path))
    {
        return Read::Waiting;
    }
    let held = reading
        .held
        .keys()
        .filter(|(page, slug)| slug == card && on(page))
        .map(|(page, _)| page.as_str())
        .min_by_key(|page| (*page != shown, *page != path));
    let gone = |main: &str| match tree {
        Some(branch) if !reading.trees.contains_key(&tree_id(branch)) => {
            format!("the worktree of {branch} is gone, and {main}")
        }
        Some(branch) => format!("card {card} is not on {branch}'s {path}, and {main}"),
        None => format!("{path} has no card {card}"),
    };
    match held {
        Some(page) => Read::Answered(credit(ask, reading, "card", card), format!("{page}#{card}")),
        None if held_at(path).is_some() => Read::Gone(gone(&format!(
            "main's copy of card {card} does not carry the owner's line"
        ))),
        None => Read::Gone(gone(&format!("main's {path} has no card {card}"))),
    }
}

/// Whether the card `card` of the document `path` that `ask` is about is the one `page` holds:
/// main's ask on main's page or on the page of a branch that proposes the card again; a branch's on
/// that branch's own page, or on main's once its copy carries the owner's line.
fn on_page(ask: &Ask, path: &str, card: &str, page: &str, held: &crate::dashboard::Held) -> bool {
    let Some(branch) = ask.tree.as_deref() else {
        return page == path || crate::dashboard::document_path(page) == path;
    };
    let since = (ask.asked / 60.0).floor() * 60.0 - LINE_SLACK;
    page == self::page(Some(branch), path)
        || (page == path
            && held
                .get(&(page.to_string(), card.to_string()))
                .copied()
                .flatten()
                .is_some_and(|at| at >= since))
}

/// Tell each card of the agent queue the owner's open ask it is on the board, the oldest when
/// several are about it: how long it has waited and who holds it, from the ask itself. A comment's
/// row, and a card no open ask is about, is told none.
pub fn note(agent: &mut [QueueItem], open: &[Ask], held: &crate::dashboard::Held) {
    for item in agent.iter_mut() {
        item.ask = open
            .iter()
            .filter(|ask| ask.open && matches!(ask.kind.as_str(), "verdict" | "message"))
            .filter(|ask| {
                !is_comment(item)
                    && ask.card.as_deref() == Some(item.slug.as_str())
                    && ask
                        .path
                        .as_deref()
                        .is_some_and(|path| on_page(ask, path, &item.slug, &item.path, held))
            })
            .min_by(|a, b| a.asked.total_cmp(&b.asked).then(a.id.cmp(&b.id)))
            .map(|ask| symphony_api::QueueAsk {
                id: ask.id,
                asked: ask.asked.floor() as i64,
                holder: ask.holder.clone(),
                held: ask.held.map(|held| held.floor() as i64),
            });
    }
}

/// An ask on a comment, read in its document, the worktree's for a branch's.
fn read_comment(ask: &Ask, reading: &Reading) -> Read {
    let (Some(path), Some(block)) = (ask.path.as_deref(), ask.block.as_deref()) else {
        return Read::Unsure;
    };
    let tree = ask.tree.as_deref();
    if ask.last >= reading.read_at || (tree.is_some() && !reading.listed) {
        return Read::Unsure;
    }
    let checkout = match tree {
        None => reading.root.as_path(),
        Some(branch) => match reading.trees.get(&tree_id(branch)) {
            Some(checkout) => checkout.as_path(),
            None => return Read::Gone(format!("the worktree of {branch} is gone")),
        },
    };
    let shown = page(tree, path);
    match comment_in(&reading.root, checkout, path, block) {
        None => Read::Unsure,
        Some(Comment::Open) => Read::Waiting,
        Some(Comment::Resolved) => Read::Answered(
            credit(ask, reading, "comment", block),
            format!("{shown}: {block}"),
        ),
        Some(Comment::Missing(why)) => Read::Gone(why),
    }
}

/// An ask on a chat, read in the ask store's threads.
fn read_chat(ask: &Ask, reading: &Reading) -> Read {
    let Some((threads, read_at)) = reading.chats.as_ref() else {
        return Read::Unsure;
    };
    let Some(id) = ask.thread.as_deref().and_then(|id| id.parse::<i64>().ok()) else {
        return Read::Unsure;
    };
    if ask.last >= *read_at {
        return Read::Unsure;
    }
    match threads.iter().find(|thread| thread.id == id) {
        None => Read::Gone(format!("chat {id} is gone from the ask store")),
        Some(thread) if thread.status == "answered" => Read::Answered(
            thread
                .answers
                .last()
                .map(|a| a.by.clone())
                .filter(|by| !by.is_empty() && by != "agent"),
            format!("ask {id}'s answer"),
        ),
        Some(_) => Read::Waiting,
    }
}

/// The session that answered `ask` on its card or comment (`kind` `card` or `comment`, `target`
/// its slug or title): the last one `symphony reply --as` recorded there since the owner's last
/// line, in the worktree the owner saw it on first, else in any other.
fn credit(ask: &Ask, reading: &Reading, kind: &str, target: &str) -> Option<String> {
    let path = ask.path.as_deref()?;
    let tree = ask.tree.as_deref().unwrap_or("");
    let since: Vec<&Reply> = reading
        .replies
        .iter()
        .filter(|r| {
            r.kind == kind
                && r.path == path
                && r.target.trim() == target.trim()
                && r.at >= ask.last
                && !r.by.trim().is_empty()
        })
        .collect();
    since
        .iter()
        .rev()
        .find(|r| r.tree == tree)
        .or_else(|| since.last())
        .map(|r| r.by.clone())
}

/// How a comment stands in its document.
#[derive(Debug, Clone, PartialEq)]
enum Comment {
    Open,
    Resolved,
    /// Not there, in these words.
    Missing(String),
}

/// The comment titled `block` in the document `path` of the checkout `checkout` (main's is
/// `root`): open, resolved (there, or archived resolved to the review ledger of the checkout's
/// first history folder), or missing; none when the document cannot be read for any reason but
/// its absence.
fn comment_in(root: &Path, checkout: &Path, path: &str, block: &str) -> Option<Comment> {
    let project = crate::project::Project::of(root);
    let file = project.file(path);
    let text = match std::fs::read_to_string(checkout.join(&file)) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Some(Comment::Missing(format!("{path} is gone")));
        }
        Err(_) => return None,
    };
    let lines: Vec<&str> = text.lines().collect();
    let titled: Vec<_> = crate::callouts::parse_comments(&lines)
        .into_iter()
        .filter(|c| c.kind != "like" && c.title.trim() == block.trim())
        .collect();
    Some(if titled.iter().any(|c| c.open()) {
        Comment::Open
    } else if !titled.is_empty() || archived(checkout, &project, path, &file, block) {
        Comment::Resolved
    } else {
        Comment::Missing(format!("{path} has no comment {block:?}"))
    })
}

/// Whether the review ledger under `checkout` holds the comment titled `block` of the document
/// `path` (its file `file`), resolved: the tidy archives an answered comment there
/// ([`crate::callouts::archive_callouts`]).
fn archived(
    checkout: &Path,
    project: &crate::project::Project,
    path: &str,
    file: &str,
    block: &str,
) -> bool {
    let Some(history) = project.history().into_iter().next() else {
        return false;
    };
    let Ok(ledger) = std::fs::read_to_string(checkout.join(history).join(crate::board::LEDGER))
    else {
        return false;
    };
    ledger.split("\n## ").any(|entry| {
        let mut lines = entry.lines();
        let head = lines.next().unwrap_or("").trim_start_matches("## ");
        let named = head
            .rsplit_once(" · ")
            .is_some_and(|(_, doc)| doc.trim() == file || doc.trim() == path);
        let rest: Vec<&str> = lines.collect();
        named
            && crate::callouts::parse_comments(&rest)
                .iter()
                .any(|c| c.title.trim() == block.trim() && c.resolution.is_some())
    })
}

/// The ask store's threads of the checkout `root` (`ASKS_DB` when set), and when they began to be
/// read.
pub fn chats(root: &Path) -> Result<(Vec<Thread>, f64), String> {
    let at = now();
    let threads = ask_store(root)
        .and_then(|store| store.all())
        .map_err(|e| e.to_string())?;
    Ok((threads, at))
}

/// Who replied on a card or a comment of the checkout `root` in the last day, as `symphony reply
/// --as` recorded it.
pub fn replies(root: &Path) -> Result<Vec<Reply>, String> {
    ask_store(root)
        .and_then(|store| store.replies())
        .map_err(|e| e.to_string())
}

fn ask_store(root: &Path) -> Result<AskStore, crate::asks::AskError> {
    AskStore::open(&crate::asks::default_path(
        root,
        std::env::var("ASKS_DB").ok().as_deref(),
    ))
}

/// Where `symphony reply` wrote, as an ask names it: the checkout holding `file`, the worktree's
/// branch (empty on the main checkout), and the document's route. None outside a git checkout.
pub fn reply_place(file: &Path) -> Option<(PathBuf, String, String)> {
    let file = std::fs::canonicalize(file).ok()?;
    let dir = file.parent()?;
    let said = crate::branches::git(
        dir,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--show-toplevel",
            "--git-dir",
            "--git-common-dir",
        ],
    )?;
    let mut lines = said.lines();
    let (top, git_dir, common) = (lines.next()?, lines.next()?, lines.next()?);
    let canonical =
        |path: &str| std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path));
    let top = canonical(top);
    let tree = if canonical(git_dir) == canonical(common) {
        String::new()
    } else {
        crate::branches::git(&top, &["rev-parse", "--abbrev-ref", "HEAD"])?
            .trim()
            .to_string()
    };
    let rel = file.strip_prefix(&top).ok()?.to_string_lossy().into_owned();
    let route = crate::project::Project::route_in(&top, &rel).unwrap_or(rel);
    Some((top, tree, route))
}

/// What the ask store's file and its write-ahead log were last changed at and hold: a chat answered
/// or a save journalled changes it.
type Stamp = Vec<Option<(std::time::SystemTime, u64)>>;

fn stamp(root: &Path) -> Stamp {
    let path = crate::asks::default_path(root, std::env::var("ASKS_DB").ok().as_deref());
    let mut wal = path.as_os_str().to_os_string();
    wal.push("-wal");
    [path, PathBuf::from(wal)]
        .iter()
        .map(|file| {
            let meta = std::fs::metadata(file).ok()?;
            Some((meta.modified().ok()?, meta.len()))
        })
        .collect()
}

/// When the shared server's clock looks at the open asks: when the documents were read again, when
/// the ask store changed, and [`RESCAN_SECONDS`] after it last looked.
#[derive(Debug, Default)]
pub struct Due {
    read_at: Option<f64>,
    store: Option<Stamp>,
    at: f64,
}

impl Due {
    /// Whether a scan is due at `now` (seconds since the epoch) of main's checkout `root`, whose
    /// documents were read at `read_at`; one that is due counts as made.
    pub fn now(&mut self, root: &Path, read_at: f64, now: f64) -> bool {
        let store = stamp(root);
        let due = self.read_at != Some(read_at)
            || self.store.as_ref() != Some(&store)
            || now - self.at >= RESCAN_SECONDS;
        if due {
            *self = Due {
                read_at: Some(read_at),
                store: Some(store),
                at: now,
            };
        }
        due
    }
}

/// Now, in seconds since the epoch.
pub fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |since| since.as_secs_f64())
}

/// When line `line` (1-based) of `file` under the checkout `root` was written: its commit's author
/// time, now for a line not committed yet; none when git cannot say.
fn written(root: &Path, file: &str, line: usize) -> Option<f64> {
    let range = format!("{line},{line}");
    crate::branches::git(root, &["blame", "--porcelain", "-L", &range, "--", file])?
        .lines()
        .find_map(|said| said.strip_prefix("author-time "))
        .and_then(|time| time.trim().parse().ok())
}

/// An ask for each of main's open comments in the agent queue `agent` that no ask, open or
/// answered, is about yet, aged from when its line was written ([`COMMENTS_OPEN_BEFORE`]). A line
/// for each it opened, for the log.
pub fn open_comments(
    root: &Path,
    store: &Store,
    agent: &[QueueItem],
) -> Result<Vec<String>, String> {
    let known: BTreeSet<String> = store
        .asks(Some(0.0))
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|ask| ask.about)
        .collect();
    let project = crate::project::Project::of(root);
    let mut opened = Vec::new();
    for item in agent
        .iter()
        .filter(|item| is_comment(item) && !item.text.trim().is_empty())
    {
        let line = item.slug[1..].parse().unwrap_or(0);
        let new = symphony_board::NewAsk {
            kind: "comment".into(),
            path: Some(item.path.clone()),
            block: Some(item.text.clone()),
            words: item.text.clone(),
            at: written(root, &project.file(&item.path), line),
            ..symphony_board::NewAsk::default()
        };
        if known.contains(&new.about().map_err(|e| e.to_string())?) {
            continue;
        }
        let ask = store.open_ask(&new).map_err(|e| e.to_string())?;
        opened.push(format!(
            "ask {} opened for the comment in {}: {}",
            ask.id, item.path, item.text
        ));
    }
    Ok(opened)
}

/// What each of the `open` asks becomes on the board as `reading` reads it: answered, marked gone
/// and left open, or its gone mark taken off once it is found where it was. A line for each change,
/// and for each the board refused, for the log.
pub fn keep(store: &Store, open: &[Ask], reading: &Reading) -> Vec<String> {
    let mut said = Vec::new();
    for ask in open.iter().filter(|ask| ask.open) {
        let id = ask.id;
        match read(ask, reading) {
            Read::Unsure => {}
            Read::Waiting if ask.gone.is_some() => said.push(match store.mark_gone(id, None) {
                Ok(_) => format!("ask {id} waits where it was again"),
                Err(why) => format!("ask {id} still marked gone: {why}"),
            }),
            Read::Waiting => {}
            Read::Gone(why) if ask.gone.as_deref() != Some(why.as_str()) => {
                said.push(match store.mark_gone(id, Some(&why)) {
                    Ok(_) => format!("ask {id} left open, gone: {why}"),
                    Err(error) => format!("ask {id} not marked gone ({why}): {error}"),
                });
            }
            Read::Gone(_) => {}
            Read::Answered(by, answer) => {
                said.push(match store.answer_ask(id, by.as_deref(), &answer) {
                    Ok(_) => match by {
                        Some(by) => format!("ask {id} answered by {by}: {answer}"),
                        None => format!("ask {id} answered: {answer}"),
                    },
                    Err(why) => format!("ask {id} left open: {why}"),
                });
            }
        }
    }
    said
}

#[cfg(test)]
#[path = "owner_asks_tests.rs"]
mod tests;
