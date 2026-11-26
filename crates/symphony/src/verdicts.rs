//! Owner verdicts waiting on an agent: the cards ([`open_cards`]), and the `verdicts/…` tasks the
//! Symphony filed for them before the owner's words became asks ([`to_asks`]).
//!
//! A card the owner rejected, or answered with a comment, waits on an agent (`just symphony-status --open`).
//! Its save opens an ask on the board ([`symphony_board::Ask`], [`crate::callbacks`]); no task is
//! filed for it. A verdict task still open from before is turned into asks once, by `symphony
//! verdict-asks` run in the project's main checkout ([`turn`]), never by a server's start: each
//! card it lists that still waits becomes an ask keeping the owner's time, and the task is cancelled
//! with a line naming them.
//!
//! It reads main's checkout, plus the cards live branches posted ([`crate::branches`]), never the
//! whole worktree the owner saved in: a branch forked before main answered a card still lists it as
//! waiting. The app writes as its own `app` session, never as the owner.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use symphony_board::{NewAsk, NewSession, Store};

use crate::board::scan;
use crate::branches::branch_cards;
use crate::register::{Decision, Who};
use crate::text::is_space;

/// The area the verdict tasks were filed in. Not `review/`: `land-request` names its tasks
/// `review/BRANCH`. A document's panel of related tasks leaves this area out.
pub const PREFIX: &str = symphony_api::VERDICT_TASKS;
const CLOSED: [&str; 2] = ["done", "cancelled"];

/// The board could not be read or written as asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerdictError(pub String);

/// How a board the binary cannot reach begins its words, rather than a refusal of what was asked.
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

/// The task board, as far as the app's own session needs it ([`app_actor`]).
pub trait Board {
    /// The handle of the Symphony's live session, if it has one.
    fn app_session(&self) -> Result<Option<String>, VerdictError>;
    /// A new session of kind `app`; its handle.
    fn register_app(&self) -> Result<String, VerdictError>;
}

/// The Symphony's own session on the board (kind `app`), registered once and reused: never the owner.
pub fn app_actor(board: &dyn Board) -> Result<String, VerdictError> {
    match board.app_session()? {
        Some(handle) => Ok(handle),
        None => board.register_app(),
    }
}

/// [`app_actor`] on the board the shared server holds open.
pub fn app_handle(store: &Store) -> Result<String, symphony_board::Error> {
    let live = store
        .sessions()?
        .into_iter()
        .find(|s| s.row.kind == "app" && s.row.left_at.is_none());
    match live {
        Some(session) => Ok(session.row.handle),
        None => Ok(store
            .register(&NewSession {
                model: "review-app".into(),
                ..NewSession::new("app")
            })?
            .row
            .handle),
    }
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
    /// The ask the card is on the board: about the card on its branch's worktree, from the owner's
    /// last line on it, the owner's unanswered words (its state when it has none).
    pub fn ask(&self) -> NewAsk {
        let doc = crate::dashboard::document_path(&self.path);
        let said = self
            .quotes
            .iter()
            .map(|q| collapse(q))
            .collect::<Vec<_>>()
            .join(" / ");
        NewAsk {
            kind: "verdict".into(),
            path: Some(doc.to_string()),
            card: Some(self.slug.clone()),
            tree: Some(self.branch.clone()).filter(|b| !b.is_empty()),
            words: if said.is_empty() {
                self.state.clone()
            } else {
                said
            },
            at: self.since.as_deref().and_then(epoch),
            ..NewAsk::default()
        }
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

/// Every card waiting on an agent as the owner reads it: main's (`root` is main's checkout), in
/// document order, then the cards live branches posted, each one card with main's copy
/// ([`crate::dashboard::merge_decisions`]). A worktree's copy of a card main had when it forked is
/// never one of them: main may have answered it since.
pub fn open_cards(root: &Path) -> Result<Vec<OpenCard>, VerdictError> {
    let mut docs = scan(root).map_err(|e| VerdictError(e.0))?;
    let plugins = crate::plugins::Plugins::of(root);
    let cards = branch_cards(root);
    let merged = crate::dashboard::merge_decisions(&mut docs, &cards, &plugins);
    let branch = |path: &str, slug: &str| {
        merged
            .proposals
            .iter()
            .find(|((at, card), _)| at == path && *card == slug)
            .map_or("", |(_, b)| b.tree.branch.as_str())
    };
    let (main, branches): (Vec<_>, Vec<_>) = docs
        .iter()
        .flat_map(|doc| doc.decisions.iter().map(move |card| (doc, card)))
        .filter(|(_, card)| card.open(&plugins))
        .partition(|(doc, card)| branch(&doc.path, &card.slug).is_empty());
    Ok(main
        .into_iter()
        .chain(branches)
        .map(|(doc, card)| open(&doc.path, card, branch(&doc.path, &card.slug)))
        .collect())
}

/// How a verdict task's thread began each post of the cards that joined it.
const MORE: &str = "More verdicts for this task:";

/// A card's line in a verdict task: its page and slug.
static LISTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^- (\S+):\d+ \*\*([^*\s]+)\*\*").unwrap());

/// Every card a verdict task lists, in its brief (`body`) or a post of more on its `thread`, as
/// (page, slug): a card listed again is one card.
fn listed<'a>(body: &'a str, thread: impl Iterator<Item = &'a str>) -> BTreeSet<(String, String)> {
    let mut listed = BTreeSet::new();
    listed.extend(
        std::iter::once(body)
            .chain(thread.filter(|text| text.starts_with(MORE)))
            .flat_map(|text| LISTED.captures_iter(text))
            .map(|found| (found[1].to_string(), found[2].to_string())),
    );
    listed
}

/// How the line that turns a verdict task into asks begins: a task whose thread has it was turned
/// already.
pub const TURNED: &str = "The owner's words here are asks now";

/// `symphony verdict-asks`: [`to_asks`] on the board `env` reaches from the checkout `root`, as
/// the app's own session, and, when [`crate::owner_asks::COMMENTS_OPEN_BEFORE`] says so, an ask for
/// each of main's comments already open ([`crate::owner_asks::open_comments`]). What it did, a line
/// each. It moves a project's live board only from that project's main checkout, as `just tasks
/// migrate` does, and is refused elsewhere before it reads; a scratch board (`TASKS_SERVER`,
/// `TASKS_DATABASE`) it moves from anywhere. A board not at this build's schema yet is refused by
/// the board's own rule, naming `just tasks migrate`. Run again, it turns nothing more.
pub fn turn(root: &Path, mut env: symphony_board::cli::Env) -> Result<Vec<String>, String> {
    env.cwd = root.to_path_buf();
    if let Some(live) = symphony_board::cli::board(&env)?.live {
        let here = symphony_board::git::checkout(root).map(|(top, _)| top);
        // One folder however it is spelled: the board's server names its data by the path it
        // was started with, git a checkout by its own (`/var` is `/private/var` on macOS).
        let at = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        match live.main() {
            Some(main) if here.as_deref().map(at) == Some(at(main)) => {}
            Some(main) => {
                return Err(format!(
                    "`symphony verdict-asks` turns the {} board's verdict tasks only from its \
                     main checkout, {}: nothing was read or written; run it there",
                    live.project,
                    main.display()
                ));
            }
            None => {
                return Err(format!(
                    "the {} board's repository has no main checkout to run \
                     `symphony verdict-asks` from: nothing was read or written",
                    live.project
                ));
            }
        }
    }
    let store = crate::taskview::connect_in(root, env)?;
    let mut done = to_asks(root, &store, || {
        app_handle(&store).map_err(|e| e.to_string())
    })?;
    if crate::owner_asks::COMMENTS_OPEN_BEFORE {
        let queue = crate::dashboard::snapshot(root, &branch_cards(root), "")
            .map_err(|e| e.0)?
            .queue;
        done.extend(crate::owner_asks::open_comments(
            root,
            &store,
            &queue.agent,
        )?);
    }
    Ok(done)
}

/// Each `verdicts/…` task still open turned into asks by the app's session (`by`, asked for only when
/// a task is turned): every card it lists that still waits on an agent becomes an ask keeping the
/// owner's time ([`OpenCard::ask`]; one open about the card already takes the line), and the task
/// is cancelled, then gets a line naming them and saying it was. A task the app may not cancel (an
/// executor holds it) stays with its holder, and its line says so and why. Once for each task: one
/// whose thread has the line is passed over. What became of each task, for the log.
pub fn to_asks(
    root: &Path,
    store: &Store,
    by: impl FnOnce() -> Result<String, String>,
) -> Result<Vec<String>, String> {
    let tasks: Vec<_> = store
        .tasks(None)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|t| t.slug.starts_with(PREFIX) && !CLOSED.contains(&t.status.as_str()))
        .collect();
    if tasks.is_empty() {
        return Ok(Vec::new());
    }
    let slugs: Vec<String> = tasks.iter().map(|t| t.slug.clone()).collect();
    let threads = store.threads(&slugs).map_err(|e| e.to_string())?;
    let turning: Vec<_> = tasks
        .iter()
        .filter(|task| {
            threads
                .get(&task.slug)
                .is_none_or(|thread| !thread.iter().any(|m| m.text.starts_with(TURNED)))
        })
        .collect();
    if turning.is_empty() {
        return Ok(Vec::new());
    }
    let by = by()?;
    let waiting = open_cards(root).map_err(|e| e.0)?;
    let mut done = Vec::new();
    for task in turning {
        let thread = threads.get(&task.slug).map_or(&[][..], Vec::as_slice);
        let cards = listed(&task.body, thread.iter().map(|m| m.text.as_str()));
        let mut asks = Vec::new();
        for card in waiting
            .iter()
            .filter(|c| cards.contains(&(c.path.clone(), c.slug.clone())))
        {
            let ask = store.open_ask(&card.ask()).map_err(|e| e.to_string())?;
            asks.push(format!("ask {} ({} in {})", ask.id, card.slug, card.path));
        }
        // The line says what became of the task, so it is posted once the cancel was tried.
        let cancelled = store.move_to(&task.slug, "cancelled", &by);
        let asked = if asks.is_empty() {
            format!("{TURNED}, and none of its cards waits on an agent")
        } else {
            format!(
                "{TURNED}: {}. `just tasks asks` lists them",
                asks.join(", ")
            )
        };
        let line = match &cancelled {
            Ok(_) if asks.is_empty() => format!("{asked}: this task is cancelled."),
            Ok(_) => format!("{asked}; this task is cancelled."),
            Err(why) => format!(
                "{asked}. This task is not cancelled: the app's session may not cancel it ({why}), \
                 so it stays with its holder, who cancels it once the asks have it."
            ),
        };
        store
            .say(&task.slug, &by, &line, None)
            .map_err(|e| e.to_string())?;
        done.push(match cancelled {
            Ok(_) => format!("{} cancelled: {} ask(s)", task.slug, asks.len()),
            Err(why) => format!(
                "{} kept by its holder ({why}): {} ask(s)",
                task.slug,
                asks.len()
            ),
        });
    }
    Ok(done)
}
