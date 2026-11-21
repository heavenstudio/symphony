//! Where each stamp stands at a tip, by its reader (docs/analysis/needed-stamps.md, "The default
//! catalogue"): [`tests`] from the recorder's runs, [`pictures`] from the task's thread, [`owner_at`]
//! from the thread and the cards the task's branch posted ([`super::posted`]), and [`names`], by
//! which each of them, and the task page's review gate, knows that a line names the tip. A stamp stands on the commit its evidence names alone: evidence naming another,
//! a merge of the default branch included, is stale and reads not met, naming the commit it
//! stood on. [`needed`] says which stamps a task needs at a tip, from its assessment and the
//! catalogue.
//!
//! They read and decide nothing themselves: the task page's gates (`symphony::gates`) draw them, and
//! the land goes by them ([`crate::land::needs`]), the same readers; [`reds`] names a red run for
//! the land's refusal, and [`at_tip`] and [`newest_of`] what `just tasks runs` prints of the runs
//! [`tests`] reads. A run its runner refused ([`refused`]) is no run of its command to any of them:
//! it built and ran nothing, so it is neither a red nor a green at its commit, and what the command
//! showed there before it still stands.

use super::posted::{self, Posted};
use super::{Catalogue, Needs, short};
use crate::Message;
use crate::cli::machine_work::recorded::{Phase, Record, Step};
use std::path::Path;

/// The route the owner's uploads are served under, `<sha256>.<ext>` after it, whatever the
/// project's artifacts folder is called: the Symphony's `uploads::ROUTE`.
pub const UPLOADS: &str = "/artifacts/uploads/";
/// The extensions of the pictures a page draws, as the uploads' are.
pub const PICTURES: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];
/// Who writes the owner's posts: only the Symphony, as the command line refuses to act as it.
const OWNER: &str = "owner";
/// The word an approve starts with.
const APPROVE: &str = "approve";

/// Where a stamp stands at a tip, by its reader: met or not, and what it read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reading {
    /// Whether it stands on the tip.
    pub met: bool,
    /// Who gave what it read: a handle, or `owner`.
    pub by: Option<String>,
    /// When, in the board's seconds.
    pub at: Option<f64>,
    /// The commit what it read names: the tip when met, the one it stood on when stale.
    pub commit: Option<String>,
    /// One line saying where it stands.
    pub detail: String,
    /// The thread's line that holds what it read.
    pub line: Option<i64>,
    /// The cards whose owner's approve it stood on, by name, where it stands on cards the task's
    /// branch posted rather than a line of the thread ([`owner_at`]).
    pub cards: Vec<String>,
}

/// Whether `text` names `tip`, a whole commit id: one of its words that may name a commit
/// ([`crate::land::named`]) is where the tip's id starts.
pub fn names(text: &str, tip: &str) -> bool {
    crate::land::named(text)
        .into_iter()
        .any(|word| tip.starts_with(word))
}

/// The first word of `text` that may name a commit, as written.
pub fn named(text: &str) -> Option<&str> {
    crate::land::named(text).into_iter().next()
}

/// The stamps a task with the assessment `needs` needs at `tip`, in the catalogue's order: the
/// assessment's on the tip it read; on another tip, or none (an assessment made from the brief),
/// those and the catalogue's defaults, since an assessment lowers what a task needs only on the
/// tip it read; with no assessment, the defaults. A stamp the catalogue no longer has is none.
pub fn needed(catalogue: &Catalogue, needs: Option<&Needs>, tip: Option<&str>) -> Vec<String> {
    let defaults = catalogue.defaults();
    let Some(needs) = needs else {
        return defaults;
    };
    let on_tip = needs.tip.is_some() && needs.tip.as_deref() == tip;
    let named = |name: &&str| {
        needs.stamps.iter().any(|stamp| stamp == name)
            || (!on_tip && defaults.iter().any(|stamp| stamp == name))
    };
    (catalogue.names().into_iter())
        .filter(named)
        .map(str::to_string)
        .collect()
}

/// When `run` began: started, else queued.
pub fn began(run: &Record) -> f64 {
    run.started_at.unwrap_or(run.queued_at)
}

/// Why the finished `run` is red, `None` when it is green.
pub fn red(run: &Record) -> Option<String> {
    match (run.exit_code, run.signal) {
        (Some(0), _) => None,
        (Some(code), _) => Some(format!("exit {code}")),
        (None, Some(signal)) => Some(format!("ended by signal {signal}")),
        (None, None) => Some("ended without a status".into()),
    }
}

/// The line the finished `run`'s runner refused its command line with, as its recorder read it in
/// its log ([`crate::runs::refused`]); `None` for a run that built or ran something, or whose
/// recorder could not tell.
pub fn refused(run: &Record) -> Option<&str> {
    let run = run.run.as_ref().filter(|_| run.state == Step::Finished)?;
    run.refused.as_deref()
}

/// `labels` as a line names them, the first `n` and how many more.
fn listed(labels: &[&str]) -> String {
    const SHOWN: usize = 3;
    let mut said = (labels.iter().take(SHOWN))
        .map(|label| format!("`{label}`"))
        .collect::<Vec<_>>()
        .join(", ");
    if labels.len() > SHOWN {
        said.push_str(&format!(" and {} more", labels.len() - SHOWN));
    }
    said
}

/// The task's runs among `runs` (those whose task `ours` names) that name a commit, oldest first,
/// and of them the newest at `tip` of each command, in the order each was first run there, a run
/// its runner refused ([`refused`]) never one of them. `ours` is a `dyn`, so this sort is compiled
/// once and not again for each caller's closure.
fn newest_at<'a>(
    runs: &'a [Record],
    tip: &str,
    ours: &dyn Fn(&str) -> bool,
) -> (Vec<&'a Record>, Vec<&'a Record>) {
    let mut mine: Vec<&Record> = (runs.iter())
        .filter(|run| run.commit.is_some() && run.task.as_deref().is_some_and(ours))
        .collect();
    crate::order::sort_by(&mut mine, &|a, b| began(a).total_cmp(&began(b)));
    let mut newest: Vec<&Record> = Vec::new();
    let ran = |run: &&&Record| run.commit.as_deref() == Some(tip) && refused(run).is_none();
    for run in mine.iter().filter(ran) {
        match newest.iter_mut().find(|kept| kept.label == run.label) {
            Some(kept) => *kept = run,
            None => newest.push(run),
        }
    }
    (mine, newest)
}

/// Each command whose newest finished run at `tip`, of the task's (`ours`, as [`tests`] reads
/// them), is red, with why: what keeps the `tests` stamp from standing while nothing still runs,
/// and what a run of the same command, green, at the tip clears.
pub fn reds(runs: &[Record], tip: &str, ours: impl Fn(&str) -> bool) -> Vec<(String, String)> {
    let (_, newest) = newest_at(runs, tip, &ours);
    (newest.iter())
        .filter(|run| run.state == Step::Finished)
        .filter_map(|run| red(run).map(|why| (run.label.clone(), why)))
        .collect()
}

/// One command the task ran at a tip ([`at_tip`]): its newest run there, the one [`tests`] reads,
/// and its runs there before that one, oldest first, none of them refused.
#[derive(Clone, Debug, PartialEq)]
pub struct Command<'a> {
    pub newest: &'a Record,
    pub before: Vec<&'a Record>,
}

/// What the task's runs among `runs` hold at `tip`, those whose task `ours` names, as [`tests`]
/// reads them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AtTip<'a> {
    /// Each command run at the tip, in the order each was first run there.
    pub commands: Vec<Command<'a>>,
    /// The task's runs at the tip their runner refused ([`refused`]), oldest first: no command's
    /// run there.
    pub refused: Vec<&'a Record>,
    /// The task's runs at other commits, oldest first: none of them is the tip's.
    pub elsewhere: Vec<&'a Record>,
}

/// The task's runs among `runs` (those whose task `ours` names) at `tip`, each command's newest
/// with the runs of it there before, and its runs at other commits apart ([`AtTip`]).
pub fn at_tip<'a>(runs: &'a [Record], tip: &str, ours: impl Fn(&str) -> bool) -> AtTip<'a> {
    let (mine, newest) = newest_at(runs, tip, &ours);
    let here = |run: &&Record| run.commit.as_deref() == Some(tip);
    let commands = (newest.into_iter())
        .map(|newest| Command {
            newest,
            before: (mine.iter().copied())
                .filter(|run| here(run) && refused(run).is_none())
                .filter(|run| run.label == newest.label && !std::ptr::eq(*run, newest))
                .collect(),
        })
        .collect();
    let (at_tip, elsewhere): (Vec<&Record>, Vec<&Record>) = mine.into_iter().partition(here);
    AtTip {
        commands,
        refused: (at_tip.into_iter())
            .filter(|run| refused(run).is_some())
            .collect(),
        elsewhere,
    }
}

/// The newest run of the command `label` at `tip` among `runs`, whoever ran it, but one its runner
/// refused ([`refused`]): how the task page's check gate reads the project's `[verdicts] check`
/// there (`symphony::gates::check`).
pub fn newest_of<'a>(runs: &'a [Record], tip: &str, label: &str) -> Option<&'a Record> {
    (runs.iter())
        .filter(|run| run.label == label && run.commit.as_deref() == Some(tip))
        .filter(|run| refused(run).is_none())
        .max_by(|a, b| began(a).total_cmp(&began(b)))
}

/// The `tests` stamp at `tip`, from `runs`, the recorder's, of which those whose task `ours` names
/// are the task's (its own, or work under it): met when the newest run at the tip of each build
/// and test command the task ran there is green, and one test run at least is among them; not met
/// while one is red ([`reds`]), still going or queued, or none ran at the tip, the last run
/// elsewhere named, or the command a runner refused there ([`refused`]), which is no run of it.
pub fn tests(runs: &[Record], tip: &str, ours: impl Fn(&str) -> bool) -> Reading {
    let (mine, newest) = newest_at(runs, tip, &ours);
    let at_tip = short(tip);
    let Some(last) = newest.iter().max_by(|a, b| began(a).total_cmp(&began(b))) else {
        let here = |run: &&&Record| run.commit.as_deref() == Some(tip);
        if let Some(run) = mine.iter().rfind(here) {
            return Reading {
                by: run.handle.clone(),
                at: run.ended_at.or(Some(began(run))),
                commit: Some(tip.to_string()),
                detail: format!(
                    "No build or test ran at {at_tip}: `{}` was refused by its runner and never \
                     ran ({})",
                    run.label,
                    refused(run).unwrap_or_default()
                ),
                ..Reading::default()
            };
        }
        return match mine.iter().rfind(|run| refused(run).is_none()) {
            Some(last) => {
                let commit = last.commit.clone().unwrap_or_default();
                Reading {
                    by: last.handle.clone(),
                    at: last.ended_at.or(Some(began(last))),
                    detail: format!(
                        "No build or test at {at_tip}: the last ran at {}",
                        short(&commit)
                    ),
                    commit: Some(commit),
                    ..Reading::default()
                }
            }
            None => Reading {
                commit: Some(tip.to_string()),
                detail: format!("No build or test recorded for it at {at_tip}"),
                ..Reading::default()
            },
        };
    };
    let reading = |met: bool, detail: String| Reading {
        met,
        by: last.handle.clone(),
        at: last.ended_at.or(Some(began(last))),
        commit: Some(tip.to_string()),
        detail,
        ..Reading::default()
    };
    let reds: Vec<(&str, String)> = (newest.iter())
        .filter(|run| run.state == Step::Finished)
        .filter_map(|run| red(run).map(|why| (run.label.as_str(), why)))
        .collect();
    if let Some((label, why)) = reds.first() {
        let more = match reds.len() {
            1 => String::new(),
            n => format!(" ({} more red)", n - 1),
        };
        return reading(false, format!("`{label}` red at {at_tip}: {why}{more}"));
    }
    let going: Vec<&str> = (newest.iter())
        .filter(|run| run.state != Step::Finished)
        .map(|run| run.label.as_str())
        .collect();
    if !going.is_empty() {
        return reading(false, format!("{} still going at {at_tip}", listed(&going)));
    }
    let labels: Vec<&str> = newest.iter().map(|run| run.label.as_str()).collect();
    if !newest.iter().any(|run| run.phase == Phase::Test) {
        return reading(
            false,
            format!(
                "No test ran at {at_tip}: {} green, no test among them",
                listed(&labels)
            ),
        );
    }
    reading(true, format!("Green at {at_tip}: {}", listed(&labels)))
}

/// Whether `text` shows a picture: a file with a picture's extension ([`PICTURES`]) under
/// `folder` (the task's artifacts folder, `artifacts/SLUG/`, named where a path starts: the text's
/// start, a `/` or a character no file name holds) or an upload ([`UPLOADS`]).
pub fn shows_picture(text: &str, folder: &str) -> bool {
    let in_name = |c: char| c.is_alphanumeric() || "-_.".contains(c);
    [folder, UPLOADS].into_iter().any(|prefix| {
        text.match_indices(prefix).any(|(at, _)| {
            if !prefix.starts_with('/') && text[..at].chars().next_back().is_some_and(in_name) {
                return false;
            }
            let path = text[at + prefix.len()..]
                .split(|c: char| c.is_whitespace() || "()[]<>`\"',".contains(c))
                .next()
                .unwrap_or_default()
                .trim_end_matches(['.', ':', ';', '!', '?']);
            path.rsplit_once('.').is_some_and(|(name, extension)| {
                !name.is_empty() && PICTURES.contains(&extension.to_ascii_lowercase().as_str())
            })
        })
    })
}

/// The task `slug`'s artifacts folder under the project's, `artifacts`: where its pictures go.
pub fn folder(artifacts: &str, slug: &str) -> String {
    format!("{}/{slug}/", artifacts.trim_end_matches('/'))
}

/// The `pictures` stamp at `tip`, from the task's `thread`: met by a post naming the tip and
/// showing at least one picture under `folder` ([`folder`]) or uploaded ([`shows_picture`]); not
/// met while the newest such post names another commit or none, or there is none.
pub fn pictures(thread: &[Message], tip: &str, folder: &str) -> Reading {
    let shown =
        (thread.iter()).filter(|post| post.kind != "system" && shows_picture(&post.text, folder));
    let at_tip = short(tip);
    let Some((post, reading)) = newest(shown, tip, |post| post.sender.clone()) else {
        return Reading {
            commit: Some(tip.to_string()),
            detail: format!("No picture posted at {at_tip}"),
            ..Reading::default()
        };
    };
    let detail = match (reading.met, reading.commit.as_deref()) {
        (true, _) => format!("Pictures at {at_tip}, posted by {}", post.sender),
        (false, Some(commit)) => format!("Pictures posted at {}, none at {at_tip}", short(commit)),
        (false, None) => format!("Pictures posted naming no commit, none at {at_tip}"),
    };
    Reading { detail, ..reading }
}

/// A stamp's evidence among `posts`, a thread's oldest first, at `tip`: the newest post naming the
/// tip, met; else the newest of them, not met, its commit the one it names (none when it names
/// none); `None` with no post. `by` says who gave it; the caller words the detail.
fn newest<'a>(
    mut posts: impl DoubleEndedIterator<Item = &'a Message> + Clone,
    tip: &str,
    by: impl Fn(&Message) -> String,
) -> Option<(&'a Message, Reading)> {
    let (post, met) = match posts.clone().rfind(|post| names(&post.text, tip)) {
        Some(post) => (post, true),
        None => (posts.next_back()?, false),
    };
    let commit = if met { Some(tip) } else { named(&post.text) };
    let reading = Reading {
        met,
        by: Some(by(post)),
        at: Some(post.ts),
        commit: commit.map(str::to_string),
        detail: String::new(),
        line: Some(post.id),
        cards: Vec::new(),
    };
    Some((post, reading))
}

/// Whether `post` is the owner's approve: a line of the owner's whose first word is `approve`.
pub fn owner_approves(post: &Message) -> bool {
    let first = (post.text.trim_start())
        .split(|c: char| c.is_whitespace() || c == ':')
        .next()
        .unwrap_or_default();
    post.sender == OWNER && post.kind != "system" && first.eq_ignore_ascii_case(APPROVE)
}

/// The `owner` stamp at `tip`, the one reader every caller goes by: met by the owner's approve
/// naming the tip on the task's `thread` ([`owner`]), else by the owner's approve of every card the
/// task's branch posted, with nothing but the owner's lines and merges of the default branch after
/// it, read from git in the main checkout `main` ([`posted::read`]).
pub fn owner_at(thread: &[Message], tip: &str, main: &Path) -> Reading {
    owner_with(thread, tip, || posted::read(main, tip))
}

/// [`owner_at`], what the cards the branch posted say asked of `posted` only where the thread
/// names no approve of the tip: for a reader that keeps it for a tip ([`posted::read`] asks git).
/// Met by the cards, it says so, naming them; where a commit after their approve keeps them from
/// standing, the thread's reading says which.
pub fn owner_with(thread: &[Message], tip: &str, posted: impl FnOnce() -> Posted) -> Reading {
    let reading = owner(thread, tip);
    if reading.met {
        return reading;
    }
    match posted() {
        Posted::Nothing => reading,
        Posted::Approved { cards } => Reading {
            met: true,
            by: Some(OWNER.into()),
            at: None,
            commit: Some(tip.to_string()),
            detail: format!("Approved by the owner on {}, at {}", on(&cards), short(tip)),
            line: None,
            cards,
        },
        Posted::Changed { cards, commit } => Reading {
            detail: format!(
                "{}; the owner's approve of {} came before {}, which changes more than the \
                 owner's lines",
                reading.detail,
                on(&cards),
                short(&commit)
            ),
            ..reading
        },
    }
}

/// `cards` as a reading names them: `card NAME`, `cards NAME, OTHER`.
pub fn on(cards: &[String]) -> String {
    match cards {
        [card] => format!("card {card}"),
        _ => format!("cards {}", cards.join(", ")),
    }
}

/// The `owner` stamp at `tip` from the task's `thread` alone: met by the owner's approve naming the
/// tip ([`owner_approves`]), which its page posts (`approve FULL_SHA`) or the owner writes; not met
/// while the newest names another commit or none, or there is none. [`owner_at`] reads the cards the
/// task posted beside it.
pub fn owner(thread: &[Message], tip: &str) -> Reading {
    let approves = thread.iter().filter(|post| owner_approves(post));
    let at_tip = short(tip);
    let Some((_, reading)) = newest(approves, tip, |_| OWNER.into()) else {
        return Reading {
            commit: Some(tip.to_string()),
            detail: format!("Waits on the owner's approve of {at_tip}"),
            ..Reading::default()
        };
    };
    let detail = match (reading.met, reading.commit.as_deref()) {
        (true, _) => format!("Approved by the owner at {at_tip}"),
        (false, Some(commit)) => {
            format!("The owner approved {}, not the tip {at_tip}", short(commit))
        }
        (false, None) => format!("The owner approved naming no commit, not the tip {at_tip}"),
    };
    Reading { detail, ..reading }
}

#[cfg(test)]
mod tests;
