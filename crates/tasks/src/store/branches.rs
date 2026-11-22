//! Recorded executor branches shared by board snapshots, cap reads and landing queries.

use super::postgres::branches as pg;
use super::statement::both;
use super::surreal::branches as sql;
use super::{Error, Store, Task};
use serde::Deserialize;
use serde_json::json;

#[derive(Clone, Deserialize)]
pub(super) struct Recorded {
    slug: String,
    branch: String,
    handle: String,
    started: f64,
}

pub(super) fn latest<'a>(records: &'a [Recorded], slug: &str) -> Option<&'a str> {
    records
        .iter()
        .filter(|r| r.slug == slug)
        .max_by(|a, b| {
            a.started
                .total_cmp(&b.started)
                .then(a.handle.cmp(&b.handle))
        })
        .map(|r| r.branch.as_str())
}

impl Store {
    /// Tasks belonging to a branch, primary task first followed by its review mirrors. This is
    /// the same slug/recorded-branch pairing the slot ledger uses. Finished tasks are retained so
    /// callers can apply their own lifecycle policy without losing ancestry.
    pub fn tasks_for_branch(&self, branch: &str) -> Result<Vec<Task>, Error> {
        let candidates: Vec<String> = self.retrying(|| {
            self.read_together(
                both(sql::on_branch(), pg::on_branch()),
                json!({"branch": branch, "slot_wanted": [branch]}),
            )
        })?;
        let tasks = self.tasks_by_slug(&candidates)?;
        let primary = crate::slots::branch_task(
            branch,
            tasks
                .iter()
                .map(|t| (t.slug.as_str(), t.branch.as_deref(), t.parent.as_deref())),
        );
        let mut found: Vec<Task> = tasks
            .iter()
            .filter(|t| Some(t.slug.as_str()) == primary)
            .cloned()
            .collect();
        let mut number = 1;
        loop {
            let slug = if number == 1 {
                format!("review/{branch}")
            } else {
                format!("review-{number}/{branch}")
            };
            let mirrors = self.tasks_by_slug(&[slug])?;
            if mirrors.is_empty() {
                break;
            }
            for mirror in mirrors {
                if !found.iter().any(|task| task.slug == mirror.slug) {
                    found.push(mirror);
                }
            }
            number += 1;
        }
        Ok(found)
    }
}
