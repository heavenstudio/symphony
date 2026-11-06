//! What an orchestrator's PULL stands for, and what becomes of one still waiting, at each dispatch
//! that builds its digest again. One rule ([`pull_plan`]) over three sets of digest entries: those
//! the recipient hears now, those the PULLs it acknowledged told it ([`State::acknowledged`]), and
//! those the PULL waiting for it tells:
//!
//! - nothing it hears beyond what it acknowledged: the PULL waiting is taken back;
//! - an entry beyond both: a new PULL, with a key and a retry budget of its own;
//! - otherwise the PULL waiting keeps its key and attempts, and says the digest as it is now.
//!
//! Whatever the plan, the queued and event records become the digest now, every entry of the
//! whole board's as dispatchers built before one orchestrator per task keep them, and what the
//! recipient was told changes only when it acknowledges a PULL ([`Account`]).

use super::callback_state::{Awaiting, Entry, State};
use super::queued_lines::Queued;
use std::collections::BTreeSet;

type Entries = BTreeSet<Entry>;

/// What a dispatch does with the PULLs waiting for an orchestrator.
#[derive(Debug, PartialEq)]
pub(super) enum PullPlan {
    /// None waits, and none is due.
    Unchanged,
    /// Take back each one waiting.
    Dropped,
    /// Keep the one with this key, and its attempts, saying the digest now; take back any other.
    Kept(String),
    /// Queue a new one in place of any waiting.
    Queued,
}

/// The entries a waiting PULL tells: every one it records but those it withholds. One an older
/// dispatcher wrote withholds none.
fn tells(line: &Queued) -> Entries {
    let withheld = line.pull_withheld.clone().unwrap_or_default();
    let recorded = line.pull_entries.iter().flatten();
    recorded
        .filter(|entry| !withheld.contains(entry))
        .cloned()
        .collect()
}

/// What becomes of the PULLs `queued` for a recipient that hears the entries `heard` of the
/// digest now, and acknowledged PULLs telling it `acknowledged`: none before its first PULL, which
/// is due even when empty.
pub(super) fn pull_plan(
    queued: &[Queued],
    heard: &Entries,
    acknowledged: Option<&Entries>,
) -> PullPlan {
    let waiting: Vec<&Queued> = queued.iter().filter(|line| line.is_pull()).collect();
    let new: Entries = match acknowledged {
        Some(told) => heard.difference(told).cloned().collect(),
        None => heard.clone(),
    };
    if acknowledged.is_some() && new.is_empty() {
        return match waiting.is_empty() {
            true => PullPlan::Unchanged,
            false => PullPlan::Dropped,
        };
    }
    let pending: Entries = waiting.iter().flat_map(|line| tells(line)).collect();
    match waiting.iter().max_by_key(|line| line.sequence) {
        Some(line) if new.is_subset(&pending) => PullPlan::Kept(line.key.clone()),
        _ => PullPlan::Queued,
    }
}

/// What a dispatch that built the digest again writes of it.
#[derive(Debug, PartialEq)]
pub(super) struct Account {
    pub(super) plan: PullPlan,
    /// `pull_queued`: every entry of the digest, while a PULL waits.
    pub(super) queued: Option<Entries>,
    /// `pull_entries`, which takes in each entry the recipient does not hear (`unheard`), as an
    /// older dispatcher would have told it them: nothing before its first acknowledgement.
    pub(super) recorded: Option<Entries>,
    pub(super) unheard: Entries,
    /// `pull_withheld`: of the entries recorded, those it was not told.
    pub(super) withheld: Entries,
    /// `pull_awaiting`, and the record of the PULL left waiting.
    pub(super) awaiting: Option<Awaiting>,
}

impl Account {
    /// The account of the recipient in `state`, with `queued` waiting for it, that hears `heard`
    /// of the whole digest's `entries`. What left the digest is forgotten, so it pushes once more
    /// if it returns.
    pub(super) fn of(state: &State, queued: &[Queued], entries: &Entries, heard: &Entries) -> Self {
        let acknowledged = state.acknowledged().map(|told| &told & entries);
        let told = acknowledged.clone().unwrap_or_default();
        let unheard = entries - heard;
        let recorded =
            (state.pull_entries.as_ref()).map(|recorded| &(recorded & entries) | &unheard);
        let withheld = (recorded.as_ref()).map_or_else(Entries::new, |recorded| recorded - &told);
        let plan = pull_plan(queued, heard, acknowledged.as_ref());
        let waits = matches!(plan, PullPlan::Kept(_) | PullPlan::Queued);
        let awaiting = waits.then(|| Awaiting {
            entries: entries.clone(),
            withheld: &unheard - &told,
        });
        Account {
            plan,
            queued: waits.then(|| entries.clone()),
            recorded,
            unheard,
            withheld,
            awaiting,
        }
    }

    /// The account of a legacy observer's upgrade: every entry it hears counts as told, and it is
    /// said no PULL.
    pub(super) fn upgrade(state: &State, entries: &Entries, heard: &Entries) -> Self {
        let unheard = entries - heard;
        Account {
            plan: PullPlan::Unchanged,
            queued: state.pull_queued.clone(),
            recorded: Some(entries.clone()),
            withheld: unheard.clone(),
            unheard,
            awaiting: None,
        }
    }

    /// Whether writing this account, with `text` the PULL of the digest now, changes nothing.
    pub(super) fn changes_nothing(&self, state: &State, queued: &[Queued], text: &str) -> bool {
        let none = Entries::new();
        let recorded = self.recorded.as_ref().unwrap_or(&none);
        let kept = |key: &str, line: &Queued| {
            let awaiting = self.awaiting.as_ref();
            line.key == key
                && line.text == text
                && line.pull_entries.as_ref() == awaiting.map(|a| &a.entries)
                && line.pull_withheld.as_ref() == awaiting.map(|a| &a.withheld)
        };
        let waiting: Vec<&Queued> = queued.iter().filter(|line| line.is_pull()).collect();
        self.queued == state.pull_queued
            && recorded == state.pull_entries.as_ref().unwrap_or(&none)
            && self.withheld == state.pull_withheld
            && self.awaiting == state.pull_awaiting
            && match (&self.plan, waiting.as_slice()) {
                (PullPlan::Unchanged, _) => true,
                (PullPlan::Kept(key), [line]) => kept(key, line),
                _ => false,
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(slugs: &[&str]) -> Entries {
        let entry = |slug: &&str| ("ready_unstaffed".to_string(), slug.to_string());
        slugs.iter().map(entry).collect()
    }

    /// A PULL waiting, as `pull_plan` reads it: the entries it records, and those it withholds
    /// (none recorded when an older dispatcher wrote it).
    fn waiting(key: &str, recorded: &[&str], withheld: Option<&[&str]>) -> Queued {
        Queued {
            key: key.into(),
            text: "PULL".into(),
            pull_entries: Some(entries(recorded)),
            pull_withheld: withheld.map(entries),
            ..Queued::default()
        }
    }

    /// One judgement of the PULL of the digest now.
    #[derive(Debug)]
    struct Plan {
        /// The lines waiting for the recipient.
        queued: Vec<Queued>,
        /// What it hears of the digest now.
        heard: &'static [&'static str],
        /// What the PULLs it acknowledged told it, if it acknowledged one.
        acknowledged: Option<&'static [&'static str]>,
        expected: PullPlan,
    }

    #[test]
    fn a_pull_waiting_is_taken_back_kept_or_replaced_by_what_is_new_beyond_it() {
        let line = || Queued {
            key: "event-1-0".into(),
            text: "NEW t/a ready — T".into(),
            ..Queued::default()
        };
        let kept = || PullPlan::Kept("pull-2".into());
        let cases = [
            // The first PULL is due even when empty, and nothing beyond it replaces it.
            Plan {
                queued: vec![],
                heard: &[],
                acknowledged: None,
                expected: PullPlan::Queued,
            },
            Plan {
                queued: vec![waiting("pull-2", &[], Some(&[]))],
                heard: &[],
                acknowledged: None,
                expected: kept(),
            },
            // Nothing beyond what was acknowledged: no PULL, or the one waiting taken back.
            Plan {
                queued: vec![line()],
                heard: &["t/a"],
                acknowledged: Some(&["t/a", "t/gone"]),
                expected: PullPlan::Unchanged,
            },
            Plan {
                queued: vec![waiting("pull-2", &["t/a", "t/b"], Some(&[]))],
                heard: &["t/a"],
                acknowledged: Some(&["t/a"]),
                expected: PullPlan::Dropped,
            },
            // Some of what it tells departed, or was handed away: it keeps its key and attempts.
            Plan {
                queued: vec![line(), waiting("pull-2", &["t/a", "t/b", "t/c"], Some(&[]))],
                heard: &["t/b"],
                acknowledged: Some(&["t/a"]),
                expected: kept(),
            },
            // Work it withheld becomes the recipient's: that is new.
            Plan {
                queued: vec![waiting("pull-2", &["t/a", "t/b", "t/c"], Some(&["t/c"]))],
                heard: &["t/b", "t/c"],
                acknowledged: Some(&["t/a"]),
                expected: PullPlan::Queued,
            },
            // An entry beyond both, shown or hidden: a new PULL.
            Plan {
                queued: vec![waiting("pull-2", &["t/a", "t/b"], Some(&[]))],
                heard: &["t/a", "t/b", "t/d"],
                acknowledged: Some(&["t/a"]),
                expected: PullPlan::Queued,
            },
            Plan {
                queued: vec![],
                heard: &["t/a", "t/d"],
                acknowledged: Some(&["t/a"]),
                expected: PullPlan::Queued,
            },
            // An older dispatcher's PULL tells every entry it records.
            Plan {
                queued: vec![waiting("pull-2", &["t/a", "t/b", "t/c"], None)],
                heard: &["t/b", "t/c"],
                acknowledged: Some(&["t/a"]),
                expected: kept(),
            },
            // Of two waiting, the latest is kept.
            Plan {
                queued: vec![
                    waiting("pull-1", &["t/b"], Some(&[])),
                    Queued {
                        sequence: 9,
                        ..waiting("pull-2", &["t/b"], Some(&[]))
                    },
                ],
                heard: &["t/b"],
                acknowledged: Some(&["t/a"]),
                expected: kept(),
            },
        ];
        for case in cases {
            let acknowledged = case.acknowledged.map(entries);
            let plan = pull_plan(&case.queued, &entries(case.heard), acknowledged.as_ref());
            assert_eq!(plan, case.expected, "{case:?}");
        }
    }

    #[test]
    fn an_account_records_the_whole_digest_and_takes_in_what_was_told_only_when_acknowledged() {
        // Told t/a; t/b was withheld from it and is its own now; t/gone left the digest.
        let state = State {
            pull_entries: Some(entries(&["t/a", "t/b", "t/gone"])),
            pull_withheld: entries(&["t/b"]),
            ..State::default()
        };
        let all = entries(&["t/a", "t/b", "t/c"]);
        let account = Account::of(&state, &[], &all, &entries(&["t/a", "t/b"]));
        let awaiting = Awaiting {
            entries: all.clone(),
            withheld: entries(&["t/c"]),
        };
        let expected = Account {
            plan: PullPlan::Queued,
            queued: Some(all.clone()),
            recorded: Some(all.clone()),
            unheard: entries(&["t/c"]),
            withheld: entries(&["t/b", "t/c"]),
            awaiting: Some(awaiting),
        };
        assert_eq!(account, expected);
        // Before the PULL is acknowledged, t/b stays untold, so the PULL waiting stays.
        let state = State {
            pull_entries: account.recorded.clone(),
            pull_queued: account.queued.clone(),
            pull_withheld: account.withheld.clone(),
            pull_awaiting: account.awaiting.clone(),
            ..State::default()
        };
        let pull = Queued {
            key: "pull-2".into(),
            text: "PULL".into(),
            pull_entries: Some(all.clone()),
            pull_withheld: Some(entries(&["t/c"])),
            ..Queued::default()
        };
        let again = Account::of(&state, &[pull], &all, &entries(&["t/a", "t/b"]));
        assert_eq!(again.plan, PullPlan::Kept("pull-2".into()));
        assert_eq!(
            (again.withheld, again.awaiting),
            (expected.withheld, expected.awaiting)
        );
        // An upgrade counts what it hears as told, and leaves what is queued.
        let upgrade = Account::upgrade(&state, &all, &entries(&["t/a"]));
        assert_eq!(upgrade.withheld, entries(&["t/b", "t/c"]));
        assert_eq!(
            (upgrade.plan, upgrade.queued),
            (PullPlan::Unchanged, Some(all))
        );
    }
}
