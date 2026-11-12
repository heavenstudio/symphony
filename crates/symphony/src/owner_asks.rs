//! The owner's asks ([`symphony_board::Ask`], docs/analysis/owner-asks.md) closed as the Symphony
//! reads their answers. An ask on a card closes when the scan that sorts the cards no longer has the
//! card waiting on an agent, one on a comment when the comment is resolved, a chat when its thread in
//! the ask store ([`crate::asks`]) is answered. An ask on a task closes in the board's own write of
//! the post that answers it, and any ask by hand with `just tasks asks close`: neither is read here.
//!
//! The shared server alone closes them, on its board clock, from the agent queue of main's page
//! ([`crate::dashboard::snapshot`]): main's cards and each unmerged branch's own, and main's
//! comments. A branch's comments are not in that queue, so an ask on one reads the worktree's
//! document itself. An ask the owner said more to after the documents were read waits for the
//! next scan, which reads that line too.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use symphony_api::QueueItem;
use symphony_board::{Ask, Store};

use crate::asks::{AskStore, Thread};

/// How long the shared server goes at most without looking at the open asks while nothing it
/// watches changed, in seconds: an ask opened or answered some other way (the board's API, a
/// worktree that went) still closes within it.
pub const RESCAN_SECONDS: f64 = 60.0;

/// Whether `symphony verdicts-to-asks` ([`crate::verdicts::turn`]) also opens an ask for each
/// comment already open in main's documents ([`open_comments`]), aged from when its line was
/// written; otherwise only comments saved from then on are asks. The design's second open question (docs/analysis/owner-asks.md,
/// Open questions), not decided: built off until the owner says.
pub const COMMENTS_OPEN_BEFORE: bool = false;

/// What one scan read: the documents' agent queue and, when a chat ask is open, the ask store.
#[derive(Debug, Clone, Default)]
pub struct Reading {
    /// Main's checkout.
    pub root: PathBuf,
    /// The agent queue of main's page.
    pub agent: Vec<QueueItem>,
    /// When the documents the queue came from began to be read, in seconds since the epoch.
    pub read_at: f64,
    /// The served worktrees by id ([`crate::branches::Tree::id`]).
    pub trees: BTreeMap<String, PathBuf>,
    /// The ask store's threads and when they began to be read; none when it was not read.
    pub chats: Option<(Vec<Thread>, f64)>,
}

/// The page a card or comment the owner saw on the worktree of `tree` is served at: main's own
/// `path`, or `@<tree id>/path`.
fn page(tree: Option<&str>, path: &str) -> String {
    match tree {
        Some(branch) => format!("@{}/{path}", branch.replace('/', "~")),
        None => path.to_string(),
    }
}

/// Whether a queue item is a comment: its slug is `L` and its line.
fn is_comment(item: &QueueItem) -> bool {
    item.slug
        .strip_prefix('L')
        .is_some_and(|line| !line.is_empty() && line.bytes().all(|b| b.is_ascii_digit()))
}

/// Whether an open comment titled `block` is in the document `path` under the worktree `tree`;
/// none when the document cannot be read for any reason but its absence.
fn comment_open_in(root: &Path, tree: &Path, path: &str, block: &str) -> Option<bool> {
    let file = tree.join(crate::project::Project::of(root).file(path));
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(false),
        Err(_) => return None,
    };
    let lines: Vec<&str> = text.lines().collect();
    Some(
        crate::callouts::parse_comments(&lines)
            .iter()
            .any(|c| c.open() && c.title.trim() == block.trim()),
    )
}

/// How the open `ask` was answered, as `reading` says: the session that answered, when it names
/// one, and where the answer is. None while it still waits, or when `reading` cannot say.
pub fn answer(ask: &Ask, reading: &Reading) -> Option<(Option<String>, String)> {
    match ask.kind.as_str() {
        "verdict" | "message" => {
            let (path, card) = (ask.path.as_deref()?, ask.card.as_deref()?);
            if ask.last >= reading.read_at {
                return None;
            }
            let shown = page(ask.tree.as_deref(), path);
            // A branch's card that left the branch's own (the branch merged, or main took the same
            // card) waits as main's copy does.
            let waiting = |page: &str| {
                reading
                    .agent
                    .iter()
                    .any(|item| item.path == page && item.slug == card)
            };
            (!waiting(&shown) && !waiting(path)).then(|| (None, format!("{shown}#{card}")))
        }
        "comment" => {
            let (path, block) = (ask.path.as_deref()?, ask.block.as_deref()?);
            if ask.last >= reading.read_at {
                return None;
            }
            let tree = ask
                .tree
                .as_deref()
                .and_then(|branch| reading.trees.get(&branch.replace('/', "~")));
            let open = match tree {
                Some(tree) => comment_open_in(&reading.root, tree, path, block)?,
                None => reading.agent.iter().any(|item| {
                    item.path == path && is_comment(item) && item.text.trim() == block.trim()
                }),
            };
            (!open).then(|| {
                (
                    None,
                    format!("{}: {block}", page(ask.tree.as_deref(), path)),
                )
            })
        }
        "chat" => {
            let (threads, read_at) = reading.chats.as_ref()?;
            let id: i64 = ask.thread.as_deref()?.parse().ok()?;
            if ask.last >= *read_at {
                return None;
            }
            match threads.iter().find(|thread| thread.id == id) {
                None => Some((None, format!("ask {id}, gone from the ask store"))),
                Some(thread) if thread.status == "answered" => Some((
                    thread
                        .answers
                        .last()
                        .map(|a| a.by.clone())
                        .filter(|by| !by.is_empty() && by != "agent"),
                    format!("ask {id}'s answer"),
                )),
                Some(_) => None,
            }
        }
        _ => None,
    }
}

/// The ask store's threads of the checkout `root` (`ASKS_DB` when set), and when they began to be
/// read.
pub fn chats(root: &Path) -> Result<(Vec<Thread>, f64), String> {
    let at = now();
    let path = crate::asks::default_path(root, std::env::var("ASKS_DB").ok().as_deref());
    let threads = AskStore::open(&path)
        .and_then(|store| store.all())
        .map_err(|e| e.to_string())?;
    Ok((threads, at))
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
    let known: std::collections::BTreeSet<String> = store
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

/// Every one of the `open` asks `reading` reads an answer for, answered on the board. A line for
/// each it closed, and for each the board refused, for the log.
pub fn close_answered(store: &Store, open: &[Ask], reading: &Reading) -> Vec<String> {
    open.iter()
        .filter(|ask| ask.open)
        .filter_map(|ask| {
            let (by, answer) = self::answer(ask, reading)?;
            Some(match store.answer_ask(ask.id, by.as_deref(), &answer) {
                Ok(_) => format!("ask {} answered: {answer}", ask.id),
                Err(why) => format!("ask {} left open: {why}", ask.id),
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "owner_asks_tests.rs"]
mod tests;
