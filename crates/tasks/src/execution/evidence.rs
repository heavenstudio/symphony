//! What the host reports once an attempt has run, and what may count of it: its artifacts'
//! collection, its resource's cleanup, and [`Coordinator::eligibility`]. The project's storage
//! collects and the project's hook cleans; the board records what each says, once, by the
//! attempt's current generation and runtime epoch.

use super::coordinator::{to_value, written};
use super::keys;
use super::record::{Artifact, Cleanup, Collection, Dispatch, Ending, Fact, Row, State};
use super::rules::Applied;
use super::{Act, Attempt, Cancellation, Collected, Coordinator, Eligibility, Grant, Refusal};
use crate::store::execution::{Decision, Write};
use serde_json::json;

/// The longest artifact name kept.
const NAME_LIMIT: usize = 512;

/// Whether `name` names an artifact within its attempt's manifest: a relative path, each part a
/// plain name, that leads nowhere above it.
pub(crate) fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= NAME_LIMIT
        && !name.starts_with('/')
        && !name.contains(['\\', '\0'])
        && name
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

impl Coordinator<'_> {
    /// Attempt `id`'s collection begun: it has ended or paused, so what it made can be gathered.
    pub fn begin_collection(&self, grant: &Grant, id: &str) -> Result<Attempt, Refusal> {
        self.collecting(grant, id, |row| match row.state.collection {
            Collection::Collecting => Ok(None),
            Collection::NotStarted
                if matches!(row.state.dispatch, Dispatch::Terminal | Dispatch::Paused) =>
            {
                Ok(Some((Collection::Collecting, None)))
            }
            Collection::NotStarted => Err(Refusal::Invalid(
                "it has neither ended nor paused: nothing is collected from a running attempt"
                    .into(),
            )),
            ended => Err(Refusal::Invalid(format!(
                "its collection has ended already, as {ended:?}"
            ))),
        })
    }

    /// Artifact `artifact` collected for attempt `id`, while its collection is under way: kept
    /// when the attempt's current resource generation and runtime epoch made it, a stale fact
    /// alone when any other did. Reporting it again changes nothing; another artifact of the
    /// same name is refused.
    pub fn record_artifact(
        &self,
        grant: &Grant,
        id: &str,
        artifact: Artifact,
    ) -> Result<Collected, Refusal> {
        let row = self.row(id)?;
        grant.allows(&self.profile().scope, Some(&row.task), Act::Collect)?;
        if !plain_name(&artifact.name) {
            return Err(Refusal::Invalid(format!(
                "artifact name {:?} is not a relative path within its manifest",
                artifact.name
            )));
        }
        if artifact.reference.is_empty() || artifact.digest.is_empty() {
            return Err(Refusal::Invalid(
                "an artifact names where it is kept and its digest".into(),
            ));
        }
        let (_, answer) = self.store().execution_update(id, |value| {
            let row = Row::of(value)?;
            if row.state.collection != Collection::Collecting {
                let refusal = Refusal::Invalid("its collection is not under way".into());
                return Ok(Decision::Nothing(Err(refusal)));
            }
            let current = row
                .state
                .resource
                .as_ref()
                .is_some_and(|binding| binding.generation == artifact.generation)
                && row.identity.runtime_epoch == artifact.runtime_epoch;
            let fact = |stale: bool| {
                to_value(&Fact::Artifact {
                    name: artifact.name.clone(),
                    digest: artifact.digest.clone(),
                    stale,
                })
            };
            if !current {
                let write = Write {
                    facts: vec![fact(true)],
                    ..Write::default()
                };
                return Ok(Decision::write(write, Ok(Collected::Stale)));
            }
            let kept = self.store().execution_artifacts(id)?;
            if let Some(known) = kept
                .iter()
                .find(|kept| kept["name"] == artifact.name.as_str())
            {
                let answer = if known["artifact"] == to_value(&artifact) {
                    Ok(Collected::Already)
                } else {
                    Err(Refusal::Conflict(format!(
                        "artifact {} was kept already with other content",
                        artifact.name
                    )))
                };
                return Ok(Decision::Nothing(answer));
            }
            let write = Write {
                facts: vec![fact(false)],
                artifacts: vec![json!({"name": artifact.name, "artifact": artifact})],
                ..Write::default()
            };
            Ok(Decision::write(write, Ok(Collected::Kept)))
        })?;
        answer
    }

    /// Attempt `id`'s collection ended as `collection` (complete, partial, failed or none), with
    /// `evidence`: none needs it, and complete needs an artifact kept.
    pub fn end_collection(
        &self,
        grant: &Grant,
        id: &str,
        collection: Collection,
        evidence: Option<&str>,
    ) -> Result<Attempt, Refusal> {
        if !collection.settled() {
            return Err(Refusal::Invalid(format!(
                "a collection ends complete, partial, failed or none, not {collection:?}"
            )));
        }
        let evidence = evidence.filter(|evidence| !evidence.trim().is_empty());
        if collection == Collection::None && evidence.is_none() {
            return Err(Refusal::Invalid(
                "an attempt that made nothing to collect says how that is known".into(),
            ));
        }
        self.collecting(grant, id, |row| match row.state.collection {
            Collection::Collecting => {
                if collection == Collection::Complete
                    && self.store().execution_artifacts(id)?.is_empty()
                {
                    return Err(Refusal::Invalid(
                        "no artifact was kept: end it as none, with the evidence".into(),
                    ));
                }
                Ok(Some((collection, evidence.map(str::to_string))))
            }
            ended if ended == collection => Ok(None),
            other => Err(Refusal::Invalid(format!(
                "its collection is {other:?}, not under way"
            ))),
        })
    }

    /// Attempt `id`'s collection moved as `next` decides on its row: to a state with evidence,
    /// or nowhere.
    fn collecting(
        &self,
        grant: &Grant,
        id: &str,
        next: impl Fn(&Row) -> Result<Option<(Collection, Option<String>)>, Refusal>,
    ) -> Result<Attempt, Refusal> {
        let row = self.row(id)?;
        grant.allows(&self.profile().scope, Some(&row.task), Act::Collect)?;
        let (_, answer) = self.store().execution_update(id, |value| {
            let row = Row::of(value)?;
            Ok(match next(&row) {
                Err(refusal) => Decision::Nothing(Err(refusal)),
                Ok(None) => Decision::Nothing(Ok(())),
                Ok(Some((collection, evidence))) => {
                    let mut applied = Applied::from_state(row.state.clone());
                    applied.state.collection = collection;
                    applied.facts.push(Fact::Collection {
                        collection,
                        by: grant.principal().clone(),
                        evidence,
                    });
                    applied.due_in = row.due.map(|_| 0);
                    Decision::write(written(&applied, None, false), Ok(()))
                }
            })
        })?;
        answer?;
        self.view(id)
    }

    /// Attempt `id`'s cleanup begun, once it has ended and its collection has: the key the
    /// project's hook cleans under, the same while that cleanup is under way. A resource handed
    /// to a resume is refused while that resume holds it or may still take it.
    pub fn begin_cleanup(&self, grant: &Grant, id: &str) -> Result<String, Refusal> {
        let row = self.row(id)?;
        grant.allows(&self.profile().scope, Some(&row.task), Act::Cleanup)?;
        let (_, answer) = self.store().execution_update(id, |value| {
            let row = Row::of(value)?;
            let state = &row.state;
            let refuse = |why: &str| Ok(Decision::Nothing(Err(Refusal::Invalid(why.into()))));
            if state.resource.is_none() {
                return refuse("no resource was bound to it: there is nothing to clean");
            }
            if state.dispatch != Dispatch::Terminal || !state.collection.settled() {
                return refuse("its resource is cleaned once it has ended and its collection has");
            }
            match (state.cleanup, &state.cleanup_key) {
                (Cleanup::Running, Some(key)) => return Ok(Decision::Nothing(Ok(key.clone()))),
                (Cleanup::Complete, _) => return refuse("its resource is cleaned up already"),
                _ => {}
            }
            if let Some(resume) = &state.resource_handed_to
                && self.resume_holds(resume, state)?
            {
                return refuse(&format!(
                    "its resource was handed to attempt {resume}, a resume that reuses it: its \
                     cleanup is that attempt's"
                ));
            }
            let identity = &row.identity;
            let number = state.cleanups + 1;
            let key = keys::cleanup(
                &identity.project,
                &identity.board,
                &identity.configuration,
                &row.attempt,
                number,
            );
            let mut applied = Applied::from_state(state.clone());
            applied.state.cleanup = Cleanup::Running;
            applied.state.cleanup_key = Some(key.clone());
            applied.state.cleanups = number;
            // A resume handed the resource ended without taking it: it is this attempt's again.
            applied.state.resource_handed_to = None;
            applied.facts.push(Fact::Cleanup {
                key: key.clone(),
                cleanup: Cleanup::Running,
                by: grant.principal().clone(),
                evidence: None,
            });
            Ok(Decision::write(written(&applied, None, false), Ok(key)))
        })?;
        answer
    }

    /// Whether attempt `resume`, which `state`'s resource was handed to, holds that resource or
    /// may still take it: it bound it, or it is live. Once it ended without it, it never will, so
    /// a read of it unfenced can only answer yes too early, never no.
    fn resume_holds(&self, resume: &str, state: &State) -> Result<bool, crate::Error> {
        let Some(value) = self.store().execution_attempt(resume)? else {
            return Ok(true);
        };
        let after = Row::of(&value)?;
        let id = |state: &State| state.resource.as_ref().map(|binding| binding.id.clone());
        Ok(after.state.live() || (id(&after.state).is_some() && id(&after.state) == id(state)))
    }

    /// Attempt `id`'s cleanup under `key` ended as `cleanup` (complete, failed or unknown), with
    /// `evidence`. A key not the one under way is refused, so a late report of an earlier cleanup
    /// changes nothing.
    pub fn end_cleanup(
        &self,
        grant: &Grant,
        id: &str,
        key: &str,
        cleanup: Cleanup,
        evidence: Option<&str>,
    ) -> Result<Attempt, Refusal> {
        if !matches!(
            cleanup,
            Cleanup::Complete | Cleanup::Failed | Cleanup::Unknown
        ) {
            return Err(Refusal::Invalid(format!(
                "a cleanup ends complete, failed or unknown, not {cleanup:?}"
            )));
        }
        let row = self.row(id)?;
        grant.allows(&self.profile().scope, Some(&row.task), Act::Cleanup)?;
        let (_, answer) = self.store().execution_update(id, |value| {
            let row = Row::of(value)?;
            let state = &row.state;
            if state.cleanup_key.as_deref() != Some(key) {
                let refusal = Refusal::Conflict(format!("cleanup {key} is not the one under way"));
                return Ok(Decision::Nothing(Err(refusal)));
            }
            match state.cleanup {
                Cleanup::Running => {}
                ended if ended == cleanup => return Ok(Decision::Nothing(Ok(()))),
                ended => {
                    let refusal =
                        Refusal::Conflict(format!("cleanup {key} ended already, as {ended:?}"));
                    return Ok(Decision::Nothing(Err(refusal)));
                }
            }
            let mut applied = Applied::from_state(state.clone());
            applied.state.cleanup = cleanup;
            applied.facts.push(Fact::Cleanup {
                key: key.to_string(),
                cleanup,
                by: grant.principal().clone(),
                evidence: evidence.map(str::to_string),
            });
            Ok(Decision::write(written(&applied, None, false), Ok(())))
        })?;
        answer?;
        self.view(id)
    }

    /// Attempt `id`'s kept artifacts, by name.
    pub fn artifacts(&self, grant: &Grant, id: &str) -> Result<Vec<Artifact>, Refusal> {
        let row = self.row(id)?;
        grant.allows(&self.profile().scope, Some(&row.task), Act::Read)?;
        self.store()
            .execution_artifacts(id)?
            .into_iter()
            .map(|value| {
                crate::objects::read(&value["artifact"]).map_err(|e| {
                    Refusal::Board(crate::Error::Db(format!(
                        "an execution artifact not in the board's shape: {e}"
                    )))
                })
            })
            .collect()
    }

    /// Whether attempt `id`'s result may be put forward as evidence for the task at revision
    /// `task_revision`, and every reason it may not. Eligible is all it says: it completes no
    /// task, stamps nothing and moves no workflow.
    pub fn eligibility(
        &self,
        grant: &Grant,
        id: &str,
        task_revision: &str,
    ) -> Result<Eligibility, Refusal> {
        let attempt = self.attempt(grant, id)?;
        let state = &attempt.state;
        let mut reasons = Vec::new();
        if !attempt.current {
            reasons.push("a later attempt holds its lane".to_string());
        }
        if state.dispatch != Dispatch::Terminal || state.ending != Some(Ending::Succeeded) {
            reasons.push(format!(
                "it has not succeeded: {:?}{}",
                state.dispatch,
                state
                    .ending
                    .map_or(String::new(), |ending| format!(", {ending:?}"))
            ));
        }
        if state.cancellation != Cancellation::None {
            reasons.push(format!(
                "a cancel was asked for it ({:?})",
                state.cancellation
            ));
        }
        if state.collection != Collection::Complete {
            reasons.push(format!(
                "its artifacts are not all collected: {:?}",
                state.collection
            ));
        }
        if attempt.identity.input.task_revision != task_revision {
            reasons.push(format!(
                "it ran on task revision {}, not {task_revision}",
                attempt.identity.input.task_revision
            ));
        }
        for artifact in self.artifacts(grant, id)? {
            if !artifact.verified {
                reasons.push(format!("artifact {} is not verified", artifact.name));
            }
        }
        Ok(Eligibility {
            eligible: reasons.is_empty(),
            reasons,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::plain_name;

    #[test]
    fn an_artifact_name_is_a_relative_path_within_its_manifest() {
        for kept in [
            "report.json",
            "logs/run.txt",
            "a/b/c.d",
            "..a/b..",
            ".hidden",
        ] {
            assert!(plain_name(kept), "{kept}");
        }
        let long = "a".repeat(513);
        for refused in [
            "",
            "/etc/passwd",
            "../up",
            "a/../../b",
            "a/./b",
            "a//b",
            "a/",
            "a\\b",
            "a\0b",
            long.as_str(),
        ] {
            assert!(!plain_name(refused), "{refused:?}");
        }
    }
}
