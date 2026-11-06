//! One recipient's observation as its callback state keeps it ([`State`]), for every generation of
//! dispatcher at once. Older ones read and write only the record in its first meaning; newer ones
//! keep what they learned since in fields older ones ignore, and read those over that record, so a
//! line either generation said is never said again by the other.

use crate::cli::attach;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

/// A digest entry: its list, and the slug or handle it names.
pub(super) type Entry = (String, String);

/// Dispatchers built before one orchestrator per task read and write `seen`, `statuses`,
/// `pull_entries` and `pull_queued` in their whole-board meaning: the legacy look
/// ([`attach::legacy_seen`]), every open task's status and every digest entry. A newer one keeps
/// those as they would, and what its orchestrator hears in fields they ignore, `narrowing` and
/// `pull_withheld`, read over them ([`State::heard`], [`State::told`]); otherwise each writer's
/// diff repeats the other's lines.
#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct State {
    pub(super) seen: Option<attach::Seen>,
    pub(super) narrowing: Option<attach::Narrowing>,
    pub(super) statuses: BTreeMap<String, String>,
    pub(super) pull: f64,
    pub(super) pull_entries: Option<BTreeSet<Entry>>,
    pub(super) pull_queued: Option<BTreeSet<Entry>>,
    /// The entries the heard digest left out of the whole one when last computed. They are
    /// recorded in `pull_entries` too, so an older dispatcher never sends them.
    pub(super) pull_withheld: BTreeSet<Entry>,
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

    /// Of the entries `recorded` as delivered or queued, those the recipient was told: all but the
    /// ones withheld from it when they were recorded.
    pub(super) fn told(&self, recorded: &BTreeSet<Entry>) -> BTreeSet<Entry> {
        recorded.difference(&self.pull_withheld).cloned().collect()
    }
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
        let entry = |slug: &str| ("ready_unstaffed".to_string(), slug.to_string());
        let mut state: State = serde_json::from_value(json!({
            "seen": look(&["t/mine", "t/theirs", "t/said"]), "narrowing": narrowing,
            "pull_withheld": [entry("t/theirs")],
        }))
        .unwrap();
        // An older dispatcher said NEW t/said since: it is told; t/theirs never is.
        let heard = state.heard().unwrap();
        assert_eq!(heard.ready.keys().collect::<Vec<_>>(), ["t/mine", "t/said"]);
        let recorded = BTreeSet::from([entry("t/mine"), entry("t/theirs"), entry("t/said")]);
        assert_eq!(
            state.told(&recorded),
            BTreeSet::from([entry("t/mine"), entry("t/said")])
        );
        // A record no newer dispatcher narrowed is its own baseline, with nothing withheld.
        state.narrowing = None;
        state.pull_withheld.clear();
        assert_eq!(state.heard(), state.seen);
        assert_eq!(state.told(&recorded), recorded);
        assert_eq!(State::default().heard(), None);
    }
}
