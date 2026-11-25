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
//! recipient was told changes only when it acknowledges a PULL ([`Account`]): until then, an
//! entry it hears and was not told stays out of what it was told, whoever writes the record.

use super::callback_state::{Awaiting, Entry, State};
use super::queued_lines::Queued;
use crate::cli::pull::Said;
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
    let none = Entries::new();
    let withheld = line.pull_withheld.as_ref().unwrap_or(&none);
    let recorded = line.pull_entries.iter().flatten();
    let mut told = Entries::new();
    told.extend(recorded.filter(|entry| !withheld.contains(entry)).cloned());
    told
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
        Some(told) => {
            let mut new = Entries::new();
            new.extend(heard.difference(told).cloned());
            new
        }
        None => heard.clone(),
    };
    if acknowledged.is_some() && new.is_empty() {
        return match waiting.is_empty() {
            true => PullPlan::Unchanged,
            false => PullPlan::Dropped,
        };
    }
    let mut pending: Entries = BTreeSet::new();
    pending.extend(waiting.iter().flat_map(|line| tells(line)));
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
    /// `pull_entries`: what it was told, and each entry it does not hear (`unheard`), as an older
    /// dispatcher would have told it them; nothing before its first acknowledgement.
    pub(super) recorded: Option<Entries>,
    pub(super) unheard: Entries,
    /// The entries it hears and was not told, left out of `pull_entries` while the PULL telling
    /// them waits (`pull_queued` keeps older dispatchers from sending them), so one found there
    /// later came with a PULL someone acknowledged.
    pub(super) untold: Entries,
    /// `pull_withheld`: the entries it does not hear and was not told, from its first PULL on. An
    /// older dispatcher's acknowledgement records every entry of the PULL as told, the first one
    /// too, and leaves this as it was.
    pub(super) withheld: Entries,
    /// `pull_awaiting`, and the record of the PULL left waiting.
    pub(super) awaiting: Option<Awaiting>,
}

/// What the recipient in `state` was told of the whole digest's `entries`: none before its first
/// PULL. Ordinary entries are forgotten when they leave the digest, so one that comes back is new
/// (the recipient may have made it leave, by staffing the work, and not know it is back), while
/// acknowledged unanswered posts retain their identity across departures and returns.
pub(super) fn told(state: &State, entries: &Entries) -> Option<Entries> {
    state.acknowledged().map(|told| {
        let mut kept = Entries::new();
        kept.extend(
            (told.into_iter()).filter(|entry| entry.0 == "unanswered" || entries.contains(entry)),
        );
        kept
    })
}

/// What the last PULL this generation recorded said ([`State::said`]), while it is still the
/// baseline of what the recipient in `state`, which hears `heard`, was told. A dispatcher that keeps
/// `pull_awaiting` but not `pull_said` records the entries a PULL told when it acknowledges one
/// and leaves `pull_said` as it was. So an entry the recipient hears, recorded as told and not
/// withheld, that the baseline does not name was told by a PULL acknowledged since: there is no
/// baseline, and the whole digest is said. Unanswered posts are not read, since one told stays
/// told across a departure the baseline does not keep.
pub(super) fn baseline<'s>(state: &'s State, heard: &Entries) -> Option<&'s Said> {
    let said = state.said()?;
    let recorded = state.pull_entries.as_ref()?;
    let since = (recorded - &state.pull_withheld).into_iter().find(|entry| {
        entry.0 != "unanswered" && heard.contains(entry) && !said.entries.contains(entry)
    });
    since.is_none().then_some(said)
}

impl Account {
    /// The account of the recipient in `state`, with `queued` waiting for it, that hears `heard`
    /// of the whole digest's `entries`, told of them as [`told`] says.
    pub(super) fn of(state: &State, queued: &[Queued], entries: &Entries, heard: &Entries) -> Self {
        let acknowledged = told(state, entries);
        let none = Entries::new();
        let told = acknowledged.as_ref().unwrap_or(&none);
        let unheard = entries - heard;
        let recorded = (state.pull_entries.as_ref()).map(|_| &(told & entries) | &unheard);
        let withheld = &unheard - told;
        let plan = pull_plan(queued, heard, acknowledged.as_ref());
        let waits = matches!(plan, PullPlan::Kept(_) | PullPlan::Queued);
        let awaiting = waits.then(|| Awaiting {
            entries: entries.clone(),
            withheld: withheld.clone(),
        });
        Account {
            plan,
            queued: waits.then(|| entries.clone()),
            recorded,
            untold: heard - told,
            unheard,
            withheld,
            awaiting,
        }
    }

    /// Legacy entries count as told on upgrade. A recipient condition unknown to that dispatcher
    /// still needs its first acknowledgement.
    pub(super) fn upgrade(state: &State, entries: &Entries, heard: &Entries) -> Self {
        let unheard = entries - heard;
        let mut untold: Entries = BTreeSet::new();
        untold.extend(
            heard
                .iter()
                .filter(|entry| entry.0 == "low_volume")
                .cloned(),
        );
        let first_condition = !untold.is_empty();
        Account {
            plan: if first_condition {
                PullPlan::Queued
            } else {
                PullPlan::Unchanged
            },
            queued: if first_condition {
                Some(entries.clone())
            } else {
                state.pull_queued.clone()
            },
            recorded: Some(entries - &untold),
            untold,
            withheld: unheard.clone(),
            awaiting: first_condition.then(|| Awaiting {
                entries: entries.clone(),
                withheld: unheard.clone(),
            }),
            unheard,
        }
    }

    /// Whether writing this account, with `text` the PULL of the digest now, which says `said`,
    /// changes nothing.
    pub(super) fn changes_nothing(
        &self,
        state: &State,
        queued: &[Queued],
        text: &str,
        said: Option<&Said>,
    ) -> bool {
        let none = Entries::new();
        let recorded = self.recorded.as_ref().unwrap_or(&none);
        let kept = |key: &str, line: &Queued| {
            let awaiting = self.awaiting.as_ref();
            line.key == key
                && line.text == text
                && line.pull_said.as_ref() == said
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

    #[test]
    fn what_was_said_stays_the_baseline_until_a_pull_it_does_not_name_was_acknowledged() {
        let post = ("unanswered".to_string(), "t/old:12".to_string());
        // The last PULL this generation recorded named t/a and the post; t/w was withheld.
        let said = Said {
            entries: &entries(&["t/a"]) | &Entries::from([post.clone()]),
            ..Said::default()
        };
        let state = |recorded: &[&str]| State {
            pull_entries: Some(&entries(recorded) | &Entries::from([post.clone()])),
            pull_withheld: entries(&["t/w"]),
            pull_unanswered: Entries::from([post.clone()]),
            pull_said: Some(said.clone()),
            ..State::default()
        };
        let heard = |slugs: &[&str]| &entries(slugs) | &Entries::from([post.clone()]);
        // As this generation recorded it, t/a now handed away: the baseline stands.
        let ours = state(&["t/a", "t/w"]);
        assert_eq!(baseline(&ours, &heard(&["t/a"])), Some(&said));
        assert_eq!(baseline(&ours, &heard(&[])), Some(&said));
        // Withheld work handed to it is not what a PULL told: the baseline stands.
        assert_eq!(baseline(&ours, &heard(&["t/a", "t/w"])), Some(&said));
        // A PULL acknowledged since, by a dispatcher that does not record what it said, told t/b,
        // which the recipient hears: no baseline.
        let since = state(&["t/a", "t/b", "t/w"]);
        assert_eq!(baseline(&since, &heard(&["t/a", "t/b"])), None);
        // Told t/b, which left the recipient's digest since: nothing shows it.
        assert_eq!(baseline(&since, &heard(&["t/a"])), Some(&said));
        // Nothing recorded, or an older acknowledgement since: no baseline either way.
        let none = State {
            pull_entries: None,
            ..ours
        };
        assert_eq!(baseline(&none, &heard(&["t/a"])), None);
        assert_eq!(baseline(&State::default(), &Entries::new()), None);
    }

    #[test]
    fn coherent_volume_crossings_clear_old_receipts_and_acknowledge_new_warnings_once() {
        let low = Entries::from([("low_volume".into(), "keen".into())]);
        let peer = Entries::from([("low_volume".into(), "peer".into())]);
        // The paired digest snapshot cases establish these global/scoped sets for old3/new4
        // and old4/new3. The account must use the same observation for pruning and delivery.
        for (retained_low, fresh_low) in [(true, false), (false, true)] {
            let told = if retained_low {
                &peer | &low
            } else {
                peer.clone()
            };
            let state = State {
                pull_entries: Some(told),
                pull_withheld: peer.clone(),
                ..State::default()
            };
            let heard = if fresh_low {
                low.clone()
            } else {
                Entries::new()
            };
            let all = &peer | &heard;
            let account = Account::of(&state, &[], &all, &heard);
            assert_eq!(account.untold, heard);
            assert_eq!(account.recorded, Some(peer.clone()));
            let acknowledged = if fresh_low {
                assert_eq!(account.plan, PullPlan::Queued);
                let awaiting = account.awaiting.clone().expect("the new condition waits");
                assert_eq!(awaiting.entries, all);
                assert_eq!(awaiting.withheld, peer);
                let pending = Queued {
                    key: "pull-coherent".into(),
                    pull_entries: Some(awaiting.entries.clone()),
                    pull_withheld: Some(awaiting.withheld.clone()),
                    ..Queued::default()
                };
                let persisted = State {
                    pull_entries: account.recorded,
                    pull_queued: account.queued,
                    pull_withheld: account.withheld,
                    pull_awaiting: Some(awaiting.clone()),
                    ..State::default()
                };
                assert_eq!(
                    Account::of(&persisted, &[pending], &all, &heard).plan,
                    PullPlan::Kept("pull-coherent".into())
                );
                State {
                    pull_entries: Some(awaiting.entries),
                    pull_withheld: awaiting.withheld.clone(),
                    pull_awaiting: Some(Awaiting {
                        entries: all.clone(),
                        withheld: awaiting.withheld,
                    }),
                    ..State::default()
                }
            } else {
                assert_eq!(account.plan, PullPlan::Unchanged);
                State {
                    pull_entries: account.recorded,
                    pull_withheld: account.withheld,
                    ..State::default()
                }
            };
            assert_eq!(acknowledged.acknowledged(), Some(heard.clone()));
            assert_eq!(
                Account::of(&acknowledged, &[], &all, &heard).plan,
                PullPlan::Unchanged
            );
            // A later real return queues once after a clear; a real clear forgets the old receipt.
            let later = if fresh_low {
                Entries::new()
            } else {
                low.clone()
            };
            let changed = Account::of(&acknowledged, &[], &(&peer | &later), &later);
            assert_eq!(
                changed.plan,
                if fresh_low {
                    PullPlan::Unchanged
                } else {
                    PullPlan::Queued
                }
            );
            assert_eq!(changed.recorded, Some(peer.clone()));
        }
    }

    #[test]
    fn an_upgrade_does_not_acknowledge_a_warning_the_legacy_observer_could_not_say() {
        let mut all = entries(&["t/ready", "t/elsewhere"]);
        let low = Entries::from([("low_volume".into(), "keen".into())]);
        all.extend(low.clone());
        let heard = &entries(&["t/ready"]) | &low;
        let account = Account::upgrade(&State::default(), &all, &heard);
        assert_eq!(account.plan, PullPlan::Queued);
        assert_eq!(account.untold, low);
        assert_eq!(account.recorded, Some(entries(&["t/ready", "t/elsewhere"])));
        assert_eq!(
            account.awaiting,
            Some(Awaiting {
                entries: all.clone(),
                withheld: entries(&["t/elsewhere"])
            })
        );
        let state = State {
            pull_entries: account.recorded.clone(),
            pull_queued: account.queued.clone(),
            pull_awaiting: account.awaiting.clone(),
            pull_withheld: account.withheld.clone(),
            ..State::default()
        };
        let pending = Queued {
            key: "pull-1".into(),
            pull_entries: Some(all.clone()),
            pull_withheld: Some(account.withheld),
            ..Queued::default()
        };
        assert_eq!(
            Account::of(&state, &[pending], &all, &heard).plan,
            PullPlan::Kept("pull-1".into())
        );
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
        let heard = entries(&["t/a", "t/b"]);
        let account = Account::of(&state, &[], &all, &heard);
        let awaiting = Awaiting {
            entries: all.clone(),
            withheld: entries(&["t/c"]),
        };
        // It records t/c, withheld, and leaves t/b out until a PULL naming it is acknowledged;
        // the PULL waiting, queued as the whole digest, keeps older dispatchers from sending it.
        let expected = Account {
            plan: PullPlan::Queued,
            queued: Some(all.clone()),
            recorded: Some(entries(&["t/a", "t/c"])),
            untold: entries(&["t/b"]),
            unheard: entries(&["t/c"]),
            withheld: entries(&["t/c"]),
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
        let pull = || Queued {
            key: "pull-2".into(),
            text: "PULL".into(),
            pull_entries: Some(all.clone()),
            pull_withheld: Some(entries(&["t/c"])),
            ..Queued::default()
        };
        let again = Account::of(&state, &[pull()], &all, &heard);
        assert_eq!(again.plan, PullPlan::Kept("pull-2".into()));
        assert_eq!(again.recorded, expected.recorded);
        assert_eq!(again.withheld, expected.withheld);
        assert_eq!(again.awaiting, expected.awaiting);
        // An older dispatcher acknowledged it, then queued its own for work the recipient does not
        // hear: that PULL says nothing new to it, and is taken back.
        let older = Queued {
            key: "pull-7".into(),
            pull_withheld: None,
            ..pull()
        };
        let acknowledged = State {
            pull_entries: Some(all.clone()),
            ..state
        };
        let after = Account::of(&acknowledged, &[older], &all, &heard);
        assert_eq!(after.plan, PullPlan::Dropped);
        assert_eq!(after.recorded, Some(all.clone()));
        // An upgrade counts what it hears as told, and leaves what is queued.
        let upgrade = Account::upgrade(&acknowledged, &all, &entries(&["t/a"]));
        assert_eq!(upgrade.withheld, entries(&["t/b", "t/c"]));
        assert_eq!(upgrade.plan, PullPlan::Unchanged);
        assert_eq!(upgrade.queued, Some(all));
    }

    #[test]
    fn the_first_pull_withholds_what_the_recipient_does_not_hear_whoever_acknowledges_it() {
        // Before any acknowledgement nothing is recorded as told, and t/c is withheld.
        let all = entries(&["t/a", "t/b", "t/c"]);
        let heard = entries(&["t/a", "t/b"]);
        let first = Account::of(&State::default(), &[], &all, &heard);
        assert_eq!(first.plan, PullPlan::Queued);
        assert_eq!(first.recorded, None);
        assert_eq!(first.withheld, entries(&["t/c"]));
        let awaiting = first.awaiting.clone().expect("the first PULL waits");
        assert_eq!(awaiting.withheld, first.withheld);
        // An older dispatcher acknowledges it: it records every entry of that PULL, t/c too, and
        // leaves what this generation wrote. Once t/c is the recipient's, it is new.
        let acknowledged = State {
            pull_entries: Some(all.clone()),
            pull_withheld: first.withheld,
            pull_awaiting: first.awaiting,
            ..State::default()
        };
        assert_eq!(acknowledged.acknowledged(), Some(heard));
        let after = Account::of(&acknowledged, &[], &all, &all);
        assert_eq!(after.plan, PullPlan::Queued);
        assert_eq!(after.untold, entries(&["t/c"]));
    }
}
