//! One recipient's observation as its callback state keeps it ([`State`]), for every generation of
//! dispatcher at once. Older ones read and write only the record in its first meaning; newer ones
//! keep what they learned since in fields older ones ignore, and read those over that record, so a
//! line either generation said is never said again by the other.

use crate::cli::attach;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A digest entry: its list, and the slug or handle it names.
pub(super) type Entry = (String, String);

/// Dispatchers built before one orchestrator per task read and write `seen`, `statuses`,
/// `pull_entries` and `pull_queued` in their whole-board meaning: the legacy look
/// ([`attach::legacy_seen`]), every open task's status and every digest entry. A newer one keeps
/// those as they would, and what its orchestrator hears and was told in fields they ignore,
/// `narrowing`, `pull_withheld` and `pull_awaiting`, read over them ([`State::heard`],
/// [`State::acknowledged`]); otherwise each writer's diff repeats the other's lines.
#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct State {
    pub(super) seen: Option<attach::Seen>,
    pub(super) narrowing: Option<attach::Narrowing>,
    pub(super) statuses: BTreeMap<String, String>,
    pub(super) pull: f64,
    pub(super) pull_entries: Option<BTreeSet<Entry>>,
    pub(super) pull_queued: Option<BTreeSet<Entry>>,
    /// Of `pull_entries`, those no PULL the recipient acknowledged told it: withheld from it, or
    /// heard since and not yet told. Recorded there all the same, so an older dispatcher never
    /// sends them.
    pub(super) pull_withheld: BTreeSet<Entry>,
    /// The PULL this generation left queued, as its acknowledgement records it.
    pub(super) pull_awaiting: Option<Awaiting>,
    // Legacy dispatchers replace Seen wholesale. They leave these stream fields untouched.
    pub(super) message_cursor: Option<i64>,
    pub(super) delivery_version: Option<u32>,
    pub(super) revision: u64,
    pub(super) snapshot_sequence: u64,
    pub(super) inbox_cursor: Option<i64>,
    pub(super) unknown_children: BTreeSet<String>,
    pub(super) registration_revision: u64,
    pub(super) observed_registration: u64,
}

impl State {
    pub(super) fn restore_unknown(&mut self) {
        if let Some(seen) = &mut self.seen {
            for child in &self.unknown_children {
                if let Some(health) = seen.children.get_mut(child)
                    && *health != crate::Health::Live
                {
                    *health = crate::Health::Unknown;
                }
            }
        }
    }

    /// What the recipient was told, as it hears it: the legacy record as it stands, narrowed as a
    /// newer dispatcher last narrowed it. The lines an older one said since are in that record. A
    /// record no newer dispatcher narrowed is its own baseline.
    pub(super) fn heard(&self) -> Option<attach::Seen> {
        let narrowed = |legacy: &attach::Seen| {
            (self.narrowing.as_ref()).map_or_else(|| legacy.clone(), |n| n.over(legacy))
        };
        self.seen.as_ref().map(narrowed)
    }

    /// The digest entries the PULLs the recipient acknowledged told it; none before the first.
    /// Every generation's acknowledgement records the PULL's entries in `pull_entries` and ends
    /// `pull_queued`; only this one's records what it did not tell. Once an older one acknowledged
    /// a PULL this one left awaiting, that PULL's untold entries are those; once it acknowledged
    /// one of its own, which names every entry it records (and holds one the awaited PULL did not,
    /// or it would not have been queued in its place), none are.
    pub(super) fn acknowledged(&self) -> Option<BTreeSet<Entry>> {
        let recorded = self.pull_entries.as_ref()?;
        let untold = match &self.pull_awaiting {
            Some(awaited) if self.pull_queued.is_none() => {
                (recorded.is_subset(&awaited.entries)).then_some(&awaited.withheld)
            }
            _ => Some(&self.pull_withheld),
        };
        Some(match untold {
            Some(untold) => recorded.difference(untold).cloned().collect(),
            None => recorded.clone(),
        })
    }
}

/// A PULL left queued, as its acknowledgement records it: every entry of the whole digest, and of
/// those, the ones it does not tell (withheld from its recipient and not told before).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(super) struct Awaiting {
    pub(super) entries: BTreeSet<Entry>,
    pub(super) withheld: BTreeSet<Entry>,
}

/// Older dispatchers deserialize Seen's health enum. Keep their existing values in that record,
/// and retain the richer observation in a field they ignore, so mixed binary versions still read it.
/// The value must match their heartbeat observation, or each writer's diff repeats the other's notice.
pub(super) fn stored_seen(
    seen: &attach::Seen,
    legacy_health: impl Fn(&str) -> crate::Health,
) -> (attach::Seen, BTreeSet<String>) {
    let mut stored = seen.clone();
    let mut unknown = BTreeSet::new();
    for (child, health) in &mut stored.children {
        if *health == crate::Health::Unknown {
            unknown.insert(child.clone());
            *health = legacy_health(child);
        }
    }
    (stored, unknown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unknown_observations_remain_readable_by_legacy_dispatchers_and_restore_without_repeats() {
        #[derive(Debug, PartialEq, Deserialize)]
        enum LegacyHealth {
            Live,
            Stale,
            Lost,
            Left,
        }
        #[derive(Deserialize)]
        struct LegacySeen {
            children: BTreeMap<String, LegacyHealth>,
        }
        let seen: attach::Seen = serde_json::from_value(json!({
            "roles":[], "statuses":{}, "ready":{},
            "children":{"stale-unknown":"Unknown", "lost-unknown":"Unknown", "left-unknown":"Unknown", "active":"Live", "stale":"Stale"}
        }))
        .unwrap();
        let (stored, unknown) = stored_seen(&seen, |child| match child {
            "stale-unknown" => crate::Health::Stale,
            "lost-unknown" => crate::Health::Lost,
            "left-unknown" => crate::Health::Left,
            other => panic!("unexpected unknown child: {other}"),
        });
        let legacy: LegacySeen =
            serde_json::from_value(serde_json::to_value(&stored).unwrap()).unwrap();
        assert_eq!(legacy.children["stale-unknown"], LegacyHealth::Stale);
        assert_eq!(legacy.children["lost-unknown"], LegacyHealth::Lost);
        assert_eq!(legacy.children["left-unknown"], LegacyHealth::Left);
        assert_eq!(legacy.children["active"], LegacyHealth::Live);
        assert_eq!(
            unknown,
            BTreeSet::from([
                "stale-unknown".to_string(),
                "lost-unknown".to_string(),
                "left-unknown".to_string(),
            ])
        );

        let mut state: State =
            serde_json::from_value(json!({"seen":stored, "unknown_children":unknown})).unwrap();
        state.restore_unknown();
        assert_eq!(
            state.seen,
            Some(seen),
            "the next dispatcher must see the same observation, not emit unknown again"
        );
        state
            .seen
            .as_mut()
            .unwrap()
            .children
            .insert("lost-unknown".into(), crate::Health::Live);
        state.restore_unknown();
        assert_eq!(
            state.seen.as_ref().unwrap().children["lost-unknown"],
            crate::Health::Live,
            "an older dispatcher's fresh evidence wins over stale metadata"
        );
        let mut empty = State::default();
        empty.restore_unknown();
        assert!(empty.seen.is_none());
    }

    #[test]
    fn an_older_writers_record_is_read_through_the_last_narrowing_and_withheld_entries_untold() {
        let look = |ready: &[&str]| -> attach::Seen {
            let ready: BTreeMap<&str, &str> = ready.iter().map(|slug| (*slug, "title")).collect();
            serde_json::from_value(json!({"roles":[], "statuses":{}, "ready":ready, "children":{}}))
                .unwrap()
        };
        // The newer dispatcher heard t/mine of the whole board's t/mine and t/theirs.
        let narrowing =
            attach::Narrowing::between(&look(&["t/mine", "t/theirs"]), &look(&["t/mine"]));
        let recorded = entries(&["t/mine", "t/theirs", "t/said"]);
        let mut state: State = serde_json::from_value(json!({
            "seen": look(&["t/mine", "t/theirs", "t/said"]), "narrowing": narrowing,
            "pull_entries": recorded, "pull_withheld": entries(&["t/theirs"]),
        }))
        .unwrap();
        // An older dispatcher said NEW t/said since: it is told; t/theirs never is.
        let heard = state.heard().unwrap();
        assert_eq!(heard.ready.keys().collect::<Vec<_>>(), ["t/mine", "t/said"]);
        let told = state.acknowledged();
        assert_eq!(told, Some(entries(&["t/mine", "t/said"])));
        // A record no newer dispatcher narrowed is its own baseline, with nothing withheld.
        state.narrowing = None;
        state.pull_withheld.clear();
        assert_eq!(state.heard(), state.seen);
        assert_eq!(state.acknowledged(), Some(recorded));
        assert_eq!(State::default().heard(), None);
        assert_eq!(State::default().acknowledged(), None);
    }

    fn entries(slugs: &[&str]) -> BTreeSet<Entry> {
        let entry = |slug: &&str| ("ready_unstaffed".to_string(), slug.to_string());
        slugs.iter().map(entry).collect()
    }

    /// The record an acknowledgement left, beside the PULL this generation left awaiting.
    #[derive(Debug)]
    struct Acknowledgement {
        /// `pull_entries` as the acknowledgement recorded them.
        recorded: &'static [&'static str],
        /// Whether `pull_queued` is still set: no acknowledgement since this generation's write.
        queued: bool,
        /// The entries this generation's `pull_withheld` keeps untold.
        withheld: &'static [&'static str],
        /// The PULL it left awaiting: its entries, and those it does not tell.
        awaiting: Option<(&'static [&'static str], &'static [&'static str])>,
        told: &'static [&'static str],
    }

    #[test]
    fn what_was_told_changes_only_with_an_acknowledgement_whoever_records_it() {
        let cases = [
            // Nothing acknowledged since: the untold entries stay untold, whatever is heard now.
            Acknowledgement {
                recorded: &["t/a", "t/b", "t/c"],
                queued: true,
                withheld: &["t/b", "t/c"],
                awaiting: Some((&["t/a", "t/b", "t/c"], &["t/c"])),
                told: &["t/a"],
            },
            // An older dispatcher acknowledged the PULL awaited: it told all but what it withheld.
            Acknowledgement {
                recorded: &["t/a", "t/b", "t/c"],
                queued: false,
                withheld: &["t/b", "t/c"],
                awaiting: Some((&["t/a", "t/b", "t/c"], &["t/c"])),
                told: &["t/a", "t/b"],
            },
            // An entry departed before it acknowledged: the awaited PULL still told the rest.
            Acknowledgement {
                recorded: &["t/a", "t/b"],
                queued: false,
                withheld: &["t/b", "t/c"],
                awaiting: Some((&["t/a", "t/b", "t/c"], &["t/c"])),
                told: &["t/a", "t/b"],
            },
            // Its own PULL, queued in the awaited one's place for an entry it lacked, named all.
            Acknowledgement {
                recorded: &["t/a", "t/b", "t/c", "t/d"],
                queued: false,
                withheld: &["t/b", "t/c"],
                awaiting: Some((&["t/a", "t/b", "t/c"], &["t/c"])),
                told: &["t/a", "t/b", "t/c", "t/d"],
            },
            // This generation's acknowledgement took the awaited PULL's untold entries already.
            Acknowledgement {
                recorded: &["t/a", "t/b", "t/c"],
                queued: false,
                withheld: &["t/c"],
                awaiting: None,
                told: &["t/a", "t/b"],
            },
        ];
        for case in cases {
            let awaiting = case.awaiting.map(|(all, withheld)| Awaiting {
                entries: entries(all),
                withheld: entries(withheld),
            });
            let state = State {
                pull_entries: Some(entries(case.recorded)),
                pull_queued: case.queued.then(|| entries(case.recorded)),
                pull_withheld: entries(case.withheld),
                pull_awaiting: awaiting,
                ..State::default()
            };
            let told = state.acknowledged();
            assert_eq!(told, Some(entries(case.told)), "{case:?}");
        }
    }
}
