//! Plan native PULL delivery without losing previously announced unanswered-post identities.

use super::{State, event_line};
use crate::cli::{Env, probes, pull};
use crate::{Error, Store};
use std::collections::BTreeSet;

type Entries = BTreeSet<(String, String)>;

pub(super) struct Plan {
    pub queued: Option<Entries>,
    pub entries: Entries,
    pub current: Entries,
    pub event: Option<(String, Entries)>,
}

impl Plan {
    pub(super) fn unchanged(state: &State) -> Self {
        Self {
            queued: state.pull_queued.clone(),
            entries: state.pull_entries.clone().unwrap_or_default(),
            current: Entries::new(),
            event: None,
        }
    }

    pub(super) fn observe(
        store: &Store,
        state: &State,
        env: &Env,
        idle: &[String],
        busy: &[String],
        legacy_upgrade: bool,
    ) -> Result<Self, Error> {
        let root = probes::repo_root(&env.cwd);
        let landed = |task: &crate::Task| {
            root.as_ref()
                .is_some_and(|root| probes::landed(root, &task.slug))
        };
        let digest = pull::digest(store, idle, Some(&landed), store.now(), busy)?;
        let entries = digest.entries();
        let mut plan = Self::unchanged(state);
        plan.current = entries.clone();
        plan.entries
            .retain(|entry| entry.0 == "unanswered" || entries.contains(entry));
        if let Some(queued) = &mut plan.queued {
            queued.retain(|entry| entry.0 == "unanswered" || entries.contains(entry));
        }
        if legacy_upgrade {
            plan.entries = entries;
        } else if (state.pull_entries.is_none() || !entries.is_subset(&plan.entries))
            && plan
                .queued
                .as_ref()
                .is_none_or(|queued| !entries.is_subset(queued))
        {
            let text = digest
                .event_lines_since(&plan.entries)
                .iter()
                .map(|line| event_line(line))
                .collect::<Vec<_>>()
                .join("\n");
            plan.entries.extend(entries);
            plan.queued = Some(plan.entries.clone());
            plan.event = Some((text, plan.entries.clone()));
        }
        Ok(plan)
    }
}
