//! Owner verdicts waiting on an agent become board work — `just verdict-tasks`.
//!
//! A card the owner rejected, or answered with a comment, waits on an agent (`just symphony-status --open`).
//! This finds every such card no live task acts on, and with `--file` puts them on the board: on the one
//! roll-up task nobody has claimed yet, or on a new `verdicts/…` task. The Symphony runs `--file` after
//! every verdict and card message it saves, so the owner's words reach the board the moment they are
//! written.
//!
//! It reads main's checkout, plus the cards an unmerged branch's worktree holds of its own
//! ([`crate::branches`]), never the whole worktree the owner saved in: a branch forked before main
//! answered a card still lists it as waiting.
//!
//! A task acts on a card when its title, brief or thread names the card's slug and was written no
//! earlier than the owner's last line on the card (a brief from before the verdict cannot be acting on
//! it), and it is not done or cancelled. The app files as its own `app` session, an agent as its
//! `TASKS_SESSION`; never as the owner.

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::board::scan;
use crate::branches::branch_cards;
use crate::register::{Decision, Who};
use crate::text::{is_space, is_word};

/// Not `review/`: `land-request` names its tasks `review/BRANCH`.
pub const PREFIX: &str = "verdicts/";
const CLOSED: [&str; 2] = ["done", "cancelled"];

const BRIEF: &str = "\
The owner's verdicts below wait on an agent and no task acts on them yet. Filed by the Symphony
when the owner saved them (`symphony verdict-tasks`); new verdicts join this task on its thread until
it is claimed.

Take every card to exactly one end state, following docs/workflow/REVIEW.md (\"What an agent does with
this\"): a board task acts on it (name it in a short dated agent line on the card, `just symphony-reply`,
and file it with `just tasks create`, the owner's words quoted in its brief, if none exists); it is
already done (say where); or a newer card supersedes it (say which). Never edit an owner line.
`just verdict-tasks` must end listing none of these cards.

Run `just verify`, commit on a branch named after this task, post the tip, and move to review.

Cards:
";

/// The board could not be read or written as asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerdictError(pub String);

/// How a board the binary cannot reach begins its words: `verdict-tasks` exits 2 on it, 1 on a refusal.
pub const UNREACHABLE: &str = "cannot reach the task board: ";

impl VerdictError {
    /// The board could not be reached at all, `why` being the binary's words.
    pub fn unreachable(why: &str) -> Self {
        Self(format!("{UNREACHABLE}{why}"))
    }

    /// Whether the board could not be reached, rather than refusing what was asked.
    pub fn is_unreachable(&self) -> bool {
        self.0.starts_with(UNREACHABLE)
    }
}

impl fmt::Display for VerdictError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for VerdictError {}

/// A task as the filing reads it: what it says, when, and whether anyone holds it.
#[derive(Clone, Debug)]
pub struct BoardTask {
    pub slug: String,
    pub title: String,
    pub body: String,
    pub status: String,
    pub created: f64,
    /// Sessions holding the executor role.
    pub executors: usize,
    /// Reservations not yet lapsed.
    pub reserved: usize,
    /// (when, text) of each message, oldest first.
    pub thread: Vec<(f64, String)>,
    /// The documents it is linked to.
    pub links: Vec<String>,
}

/// The task board, as far as the filing touches it.
pub trait Board {
    /// Seconds since the epoch.
    fn now(&self) -> f64;
    /// Every task, in the board's own order (priority, then age).
    fn tasks(&self) -> Result<Vec<BoardTask>, VerdictError>;
    /// The handle of the Symphony's live session, if it has one.
    fn app_session(&self) -> Result<Option<String>, VerdictError>;
    /// A new session of kind `app`; its handle.
    fn register_app(&self) -> Result<String, VerdictError>;
    fn say(&self, slug: &str, by: &str, text: &str) -> Result<(), VerdictError>;
    fn create(
        &self,
        slug: &str,
        title: &str,
        by: &str,
        body: &str,
        links: &[String],
    ) -> Result<(), VerdictError>;
    /// Retitle the task `slug` and link it to `links` too.
    fn amend(
        &self,
        slug: &str,
        title: &str,
        links: &[String],
        by: &str,
    ) -> Result<(), VerdictError>;
}

/// A card waiting on an agent.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenCard {
    /// Under main's checkout, or `@<tree id>/docs/…`: the page the app serves from a branch's worktree.
    pub path: String,
    pub line: usize,
    pub slug: String,
    pub state: String,
    /// The owner's last dated line on the card.
    pub since: Option<String>,
    /// The owner's words nobody has answered, oldest first.
    pub quotes: Vec<String>,
    /// The unmerged branch whose worktree holds the card; empty on main.
    pub branch: String,
}

impl OpenCard {
    pub fn entry(&self) -> String {
        let said = self
            .quotes
            .iter()
            .map(|q| format!("\"{}\"", collapse(q)))
            .collect::<Vec<_>>()
            .join(" / ");
        format!(
            "- {}:{} **{}** {}{}{}: {}",
            self.path,
            self.line,
            self.slug,
            self.state,
            self.since
                .as_ref()
                .map_or(String::new(), |s| format!(" ({s})")),
            if self.branch.is_empty() {
                String::new()
            } else {
                format!(" on branch {}", self.branch)
            },
            if said.is_empty() { "(no note)" } else { &said },
        )
    }
}

fn collapse(text: &str) -> String {
    text.split(is_space)
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

static STAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d{4})-(\d{1,2})-(\d{1,2})(?: (\d{1,2}):(\d{1,2}))?$").unwrap()
});

unsafe extern "C" {
    fn tzset();
}

/// A card line's local `YYYY-MM-DD HH:MM` (or a bare date) as seconds since the epoch; `None` when it
/// does not read as a time.
pub fn epoch(stamp: &str) -> Option<f64> {
    let caps = STAMP.captures(stamp)?;
    let field = |i: usize| {
        caps.get(i)
            .map_or(Some(0), |m| m.as_str().parse::<i32>().ok())
    };
    let (year, month, day, hour, minute) = (field(1)?, field(2)?, field(3)?, field(4)?, field(5)?);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if year < 1
        || !(1..=12).contains(&month)
        || !(1..=days).contains(&day)
        || hour > 23
        || minute > 59
    {
        return None;
    }
    // SAFETY: an all-zero `tm` is a valid plain-integer C struct, which mktime reads after the fields
    // below are set; tzset reads TZ into libc's own state.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = year - 1900;
    tm.tm_mon = month - 1;
    tm.tm_mday = day;
    tm.tm_hour = hour;
    tm.tm_min = minute;
    tm.tm_isdst = -1;
    // SAFETY: as above.
    let secs = unsafe {
        tzset();
        libc::mktime(&mut tm)
    };
    (secs != -1).then_some(secs as f64)
}

/// `YYYY-MM-DD-HHMM` of `secs` in this machine's time zone.
fn slug_stamp(secs: f64) -> String {
    let at = secs as libc::time_t;
    // SAFETY: an all-zero `tm` is a valid plain-integer C struct which localtime_r overwrites; tzset
    // reads TZ into libc's own state.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        tzset();
        libc::localtime_r(&at, &mut tm);
    }
    format!(
        "{:04}-{:02}-{:02}-{:02}{:02}",
        i64::from(tm.tm_year) + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min
    )
}

fn open(path: &str, card: &Decision, branch: &str) -> OpenCard {
    let since = card
        .entries()
        .into_iter()
        .rev()
        .find(|e| e.who == Who::Owner && e.ts.is_some())
        .and_then(|e| e.ts);
    OpenCard {
        path: path.to_string(),
        line: card.line,
        slug: card.slug.clone(),
        state: card.state.as_str().to_string(),
        since,
        quotes: card.unanswered(),
        branch: branch.to_string(),
    }
}

/// Every card waiting on an agent: main's (`root` is main's checkout), in document order, then the cards
/// each unmerged branch's worktree holds of its own. A worktree's copy of a card main had when it forked
/// is never one of them: main may have answered it since.
pub fn open_cards(root: &Path) -> Result<Vec<OpenCard>, VerdictError> {
    let docs = scan(root).map_err(|e| VerdictError(e.0))?;
    let plugins = crate::plugins::Plugins::of(root);
    let mut found: Vec<OpenCard> = docs
        .iter()
        .flat_map(|doc| doc.decisions.iter().map(move |card| (doc, card)))
        .filter(|(_, card)| card.open(&plugins))
        .map(|(doc, card)| open(&doc.path, card, ""))
        .collect();
    found.extend(
        branch_cards(root)
            .iter()
            .filter(|b| b.card.open(&plugins))
            .map(|b| open(&b.path(), &b.card, &b.tree.branch)),
    );
    Ok(found)
}

/// Every word any live task says, with the latest time it said it: its title and brief, then its thread.
fn named_since(tasks: &[BoardTask]) -> HashMap<String, f64> {
    let mut latest: HashMap<String, f64> = HashMap::new();
    for task in tasks
        .iter()
        .filter(|t| !CLOSED.contains(&t.status.as_str()))
    {
        let brief = format!("{}\n{}", task.title, task.body);
        let said = std::iter::once((task.created, brief.as_str()))
            .chain(task.thread.iter().map(|(at, text)| (*at, text.as_str())));
        for (at, text) in said {
            for word in text
                .split(|c: char| !(is_word(c) || c == '-'))
                .filter(|w| !w.is_empty())
            {
                match latest.get_mut(word) {
                    Some(held) if *held >= at => {}
                    Some(held) => *held = at,
                    None => {
                        latest.insert(word.to_string(), at);
                    }
                }
            }
        }
    }
    latest
}

/// The open cards no live task has named since the owner's last line on them.
pub fn untasked(root: &Path, board: &dyn Board) -> Result<Vec<OpenCard>, VerdictError> {
    let named = named_since(&board.tasks()?);
    let mut waiting = Vec::new();
    for card in open_cards(root)? {
        let since = match card.since.as_deref() {
            None => f64::NEG_INFINITY,
            Some(stamp) => epoch(stamp).ok_or_else(|| {
                VerdictError(format!(
                    "card {} ({}:{}): the owner's last line is dated {stamp}, which is not a time",
                    card.slug, card.path, card.line
                ))
            })?,
        };
        if named.get(&card.slug).copied().unwrap_or(f64::NEG_INFINITY) < since {
            waiting.push(card);
        }
    }
    Ok(waiting)
}

/// The Symphony's own session on the board (kind `app`), registered once and reused: never the owner.
pub fn app_actor(board: &dyn Board) -> Result<String, VerdictError> {
    match board.app_session()? {
        Some(handle) => Ok(handle),
        None => board.register_app(),
    }
}

/// The roll-up task nobody has claimed or been given yet, if there is one.
fn roll_up(tasks: &[BoardTask]) -> Option<&BoardTask> {
    tasks.iter().find(|t| {
        t.status == "ready" && t.slug.starts_with(PREFIX) && t.executors == 0 && t.reserved == 0
    })
}

/// How a roll-up's thread begins each post of the cards that joined it.
const MORE: &str = "More verdicts for this task:";

/// A card's line in a roll-up ([`OpenCard::entry`]): its page and slug.
static LISTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^- (\S+):\d+ \*\*([^*\s]+)\*\*").unwrap());

/// Every card a roll-up lists, in its brief or a post of more, as (page, slug): a card listed again
/// is one card.
fn listed(task: &BoardTask) -> BTreeSet<(String, String)> {
    std::iter::once(task.body.as_str())
        .chain(
            task.thread
                .iter()
                .map(|(_, text)| text.as_str())
                .filter(|text| text.starts_with(MORE)),
        )
        .flat_map(|text| LISTED.captures_iter(text))
        .map(|found| (found[1].to_string(), found[2].to_string()))
        .collect()
}

fn title(cards: usize) -> String {
    format!(
        "Answer {cards} owner verdict{} waiting on an agent",
        if cards == 1 { "" } else { "s" }
    )
}

/// Each page `cards` are on, once.
fn pages(cards: &[OpenCard]) -> Vec<String> {
    cards
        .iter()
        .map(|c| c.path.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn new_slug(board: &dyn Board, tasks: &[BoardTask]) -> String {
    let base = format!("{PREFIX}{}", slug_stamp(board.now()));
    let taken: BTreeSet<&str> = tasks.iter().map(|t| t.slug.as_str()).collect();
    let (mut slug, mut n) = (base.clone(), 1);
    while taken.contains(slug.as_str()) {
        n += 1;
        slug = format!("{base}-{n}");
    }
    slug
}

/// Put every untasked open card on the board as `by`; the task's slug, or `None` when none was waiting.
pub fn file(root: &Path, board: &dyn Board, by: &str) -> Result<Option<String>, VerdictError> {
    if by == "owner" {
        return Err(VerdictError(
            "the owner's words are quoted, never posted as the owner: file as a session".into(),
        ));
    }
    let cards = untasked(root, board)?;
    if cards.is_empty() {
        return Ok(None);
    }
    let listing = cards
        .iter()
        .map(OpenCard::entry)
        .collect::<Vec<_>>()
        .join("\n");
    let tasks = board.tasks()?;
    if let Some(task) = roll_up(&tasks) {
        board.say(&task.slug, by, &format!("{MORE}\n{listing}"))?;
        // Its title counts every card it lists, and it links every page they are on.
        let mut all = listed(task);
        all.extend(cards.iter().map(|c| (c.path.clone(), c.slug.clone())));
        let links: Vec<String> = pages(&cards)
            .into_iter()
            .filter(|page| !task.links.contains(page))
            .collect();
        let title = title(all.len());
        if title != task.title || !links.is_empty() {
            board.amend(&task.slug, &title, &links, by)?;
        }
        return Ok(Some(task.slug.clone()));
    }
    let slug = new_slug(board, &tasks);
    board.create(
        &slug,
        &title(cards.len()),
        by,
        &format!("{BRIEF}{listing}\n"),
        &pages(&cards),
    )?;
    Ok(Some(slug))
}
