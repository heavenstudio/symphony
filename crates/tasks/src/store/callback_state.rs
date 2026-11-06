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
    /// The entries the recipient does not hear and no PULL it acknowledged told it: withheld from
    /// it, and recorded in `pull_entries` all the same, so an older dispatcher never sends them.
    /// Kept from its first PULL on, since an older one's acknowledgement records every entry of
    /// the PULL, the first's too. An entry it hears and was not told is left out of
    /// `pull_entries` instead, while the PULL telling it waits.
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

    /// The digest entries the PULLs the recipient acknowledged told it, or fewer, never more; none
    /// before the first. Every generation's acknowledgement records the PULL's entries in
    /// `pull_entries` and ends `pull_queued`; only this one's records what it withheld. An older
    /// write only ever takes entries out of `pull_entries`, and this one leaves out each entry the
    /// recipient hears and was not told: one there was in the record this one wrote, or came with
    /// a PULL someone acknowledged since, which told it. So all but what was withheld count as
    /// told, whoever acknowledged and whatever was queued after (`revision` is not read). Once the
    /// record names an entry the PULL this one left awaiting did not, the PULL acknowledged was an
    /// older one's own, which told every entry. Otherwise an older one's own PULL of the same
    /// entries, which also told what was withheld, reads as the awaited one: work withheld from
    /// the recipient is told again once a hand-over makes it its own, and never lost.
    pub(super) fn acknowledged(&self) -> Option<BTreeSet<Entry>> {
        let recorded = self.pull_entries.as_ref()?;
        let older_own = (self.pull_awaiting.as_ref())
            .is_some_and(|awaited| !recorded.is_subset(&awaited.entries));
        Some(match older_own {
            true => recorded.clone(),
            false => recorded - &self.pull_withheld,
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
        /// Whether `pull_queued` is set: a PULL of either generation waits.
        queued: bool,
        /// The entries this generation's `pull_withheld` keeps untold.
        withheld: &'static [&'static str],
        /// The PULL it left awaiting: its entries, and those it does not tell.
        awaiting: Option<(&'static [&'static str], &'static [&'static str])>,
        told: &'static [&'static str],
    }

    #[test]
    fn what_was_told_changes_only_with_an_acknowledgement_and_never_takes_in_what_may_be_untold() {
        // Told t/a, the recipient hears t/b too, and t/c is withheld from it: the PULL awaited
        // tells t/b, which this generation's record leaves out until a PULL naming it is
        // acknowledged.
        let awaited: Option<(&[&str], &[&str])> = Some((&["t/a", "t/b", "t/c"], &["t/c"]));
        let cases = [
            // Nothing acknowledged since: t/b stays untold.
            Acknowledgement {
                recorded: &["t/a", "t/c"],
                queued: true,
                withheld: &["t/c"],
                awaiting: awaited,
                told: &["t/a"],
            },
            // An older dispatcher acknowledged the PULL awaited: it told all but what it withheld.
            // Its own PULL, queued for t/c's departure and return, records the same entries:
            // t/c stays untold too, to be told again once it is the recipient's.
            Acknowledgement {
                recorded: &["t/a", "t/b", "t/c"],
                queued: false,
                withheld: &["t/c"],
                awaiting: awaited,
                told: &["t/a", "t/b"],
            },
            // It acknowledged the PULL awaited, then queued its own for work the recipient does
            // not hear: t/b was told all the same, and that PULL says nothing new.
            Acknowledgement {
                recorded: &["t/a", "t/b", "t/c"],
                queued: true,
                withheld: &["t/c"],
                awaiting: awaited,
                told: &["t/a", "t/b"],
            },
            // It took the PULL awaited back unsaid for one of its own: t/b is still untold.
            Acknowledgement {
                recorded: &["t/a", "t/c"],
                queued: true,
                withheld: &["t/c"],
                awaiting: awaited,
                told: &["t/a"],
            },
            // An entry departed before it acknowledged: the awaited PULL still told the rest.
            Acknowledgement {
                recorded: &["t/a", "t/b"],
                queued: false,
                withheld: &["t/c"],
                awaiting: awaited,
                told: &["t/a", "t/b"],
            },
            // Its own PULL, queued in the awaited one's place for an entry it lacked, named all,
            // whatever it queued after.
            Acknowledgement {
                recorded: &["t/a", "t/b", "t/c", "t/d"],
                queued: false,
                withheld: &["t/c"],
                awaiting: awaited,
                told: &["t/a", "t/b", "t/c", "t/d"],
            },
            Acknowledgement {
                recorded: &["t/a", "t/b", "t/c", "t/d"],
                queued: true,
                withheld: &["t/c"],
                awaiting: awaited,
                told: &["t/a", "t/b", "t/c", "t/d"],
            },
            // This generation's acknowledgement took the awaited PULL's untold entries already,
            // and an older dispatcher's observation since changes nothing.
            Acknowledgement {
                recorded: &["t/a", "t/b", "t/c"],
                queued: false,
                withheld: &["t/c"],
                awaiting: None,
                told: &["t/a", "t/b"],
            },
            // An older one's own PULL, acknowledged after it, may have told t/c: it stays untold.
            Acknowledgement {
                recorded: &["t/a", "t/b", "t/c", "t/d"],
                queued: false,
                withheld: &["t/c"],
                awaiting: None,
                told: &["t/a", "t/b", "t/d"],
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
