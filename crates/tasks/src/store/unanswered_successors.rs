//! Current executor eligibility, independent of historical post attribution. Recovery reserves
//! against the exact task/session/native inputs it judged; a changed observation consumes no try.
//! New registrations share the existing session-revision fence, including absent callback rows.
//! Existing native writers change the callback row itself: SurrealDB advances its schemaless
//! `recovery_revision` (absent means zero; older readers ignore and writers preserve it), whereas
//! PostgreSQL locks that row. No session field or schema change is needed.

use super::{Error, Health, Pending, Store, pg, recovery_error};
use crate::health;
use crate::rows::SessionRow;
use crate::store::statement::Statement;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(crate) fn active(
    role: &str,
    original: &str,
    current: &str,
    health: Health,
    verifying: &BTreeSet<String>,
) -> bool {
    role == "executor" && original != current && (health.holds() || verifying.contains(current))
}

#[derive(Deserialize)]
struct Inputs {
    holders: Vec<(String, Option<SessionRow>, Option<String>, f64)>,
}

pub(super) struct Observation {
    pub value: Value,
    inputs: Inputs,
}

impl Observation {
    pub(super) fn active(&self, store: &Store, pending: &Pending, busy: &BTreeSet<String>) -> bool {
        let now = store.now();
        self.inputs
            .holders
            .iter()
            .any(|(handle, session, address, until)| {
                let health = session.as_ref().map_or(Health::Lost, |session| {
                    let health = health::health(session, now, &store.host, &*store.pid_alive, true);
                    if address.is_some() && session.left_at.is_none() && *until >= now {
                        Health::Live
                    } else {
                        super::super::native_health(
                            session,
                            health,
                            now,
                            address
                                .as_deref()
                                .filter(|a| a.starts_with(crate::launch::CODEX))
                                .map(|_| *until),
                        )
                    }
                });
                active(&pending.role, &pending.handle, handle, health, busy)
            })
    }
}

pub(super) fn read(store: &Store, pending: &Pending) -> Result<Option<Observation>, Error> {
    if pending.role != "executor" {
        return Ok(None);
    }
    let value: Value = store
        .bounded_read(
            Statement::from(pg::successors()),
            json!({"slug":pending.slug}),
        )
        .map_err(recovery_error)?;
    let inputs = crate::objects::read(&value).map_err(|e| Error::Db(e.to_string()))?;
    Ok(Some(Observation { value, inputs }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_active_different_executor_blocks_recovery() {
        for health in [
            Health::Live,
            Health::Stale,
            Health::Unknown,
            Health::Lost,
            Health::Left,
        ] {
            for busy in [BTreeSet::new(), BTreeSet::from(["next".into()])] {
                assert_eq!(
                    active("executor", "old", "next", health, &busy),
                    health.holds() || busy.contains("next")
                );
                assert!(!active("executor", "next", "next", health, &busy));
                assert!(!active("reviewer", "old", "next", health, &busy));
            }
        }
    }
}
