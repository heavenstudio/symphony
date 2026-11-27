//! An attempt as a page and `tasks api` show it ([`symphony_api::AttemptView`]), the board scope
//! every host of a project's attempts reads and controls them under, and the words of the kit's
//! stop and resume modes on the wire.

use super::{Attempt, Principal, PrincipalKind, ResumeMode, Scope, StopMode};
use crate::store::Store;
use serde::Serialize;

/// The scope of project `project`'s board in `store`: the project's registered id and the board's
/// database. Every host of the board's attempts (the server, `tasks api`, a launcher) admits,
/// reads and controls them under this one, so none refuses another's as a different board's.
pub fn board_scope(project: &str, store: &Store) -> Scope {
    Scope::new(project.to_string(), store.board_id().to_string())
}

/// The snake-case word serde writes for `value`, a unit variant of the record's enums.
fn word(value: &impl Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(word)) => word,
        other => format!("{other:?}"),
    }
}

/// `principal` as a reader is told who acted: `KIND ISSUER/SUBJECT`.
pub fn principal_word(principal: &Principal) -> String {
    let kind = match principal.kind {
        PrincipalKind::Human => "human",
        PrincipalKind::Service => "service",
        PrincipalKind::Agent => "agent",
        _ => "principal",
    };
    format!("{kind} {}/{}", principal.issuer, principal.subject)
}

/// A stop mode's word on the wire.
pub fn stop_word(mode: StopMode) -> &'static str {
    match mode {
        StopMode::Pause => "pause",
        StopMode::Terminate => "terminate",
        StopMode::HardStop => "hard_stop",
        _ => "stop",
    }
}

/// The stop mode `word` names, if it names one.
pub fn stop_mode(word: &str) -> Option<StopMode> {
    [StopMode::Pause, StopMode::Terminate, StopMode::HardStop]
        .into_iter()
        .find(|mode| stop_word(*mode) == word)
}

/// A resume mode's word on the wire.
pub fn resume_word(mode: ResumeMode) -> &'static str {
    match mode {
        ResumeMode::NewResource => "new_resource",
        ResumeMode::ReuseWithNewGeneration => "reuse_with_new_generation",
        _ => "resume",
    }
}

/// The resume mode `word` names, if it names one.
pub fn resume_mode(word: &str) -> Option<ResumeMode> {
    [ResumeMode::NewResource, ResumeMode::ReuseWithNewGeneration]
        .into_iter()
        .find(|mode| resume_word(*mode) == word)
}

impl Attempt {
    /// It on the wire, offering no control: the host that shows it says which it may be asked
    /// there ([`symphony_api::AttemptControls`]). Its resource and conversation are references,
    /// kept after its handle left; no link is among them.
    pub fn wire(&self) -> symphony_api::AttemptView {
        let (identity, state) = (&self.identity, &self.state);
        symphony_api::AttemptView {
            id: self.id.clone(),
            created: self.created,
            revision: self.revision,
            current: self.current,
            lane: identity.lane.clone(),
            role: identity.role.clone(),
            handle: identity.handle.clone(),
            configuration: identity.configuration.clone(),
            predecessor: identity.predecessor.clone(),
            relation: identity.relation.as_ref().map(word),
            successor: state.successor.clone(),
            dispatch: word(&state.dispatch),
            ending: state.ending.as_ref().map(word),
            held: state.held.clone(),
            cancellation: word(&state.cancellation),
            collection: word(&state.collection),
            cleanup: word(&state.cleanup),
            resource: state.resource.as_ref().map(|bound| bound.id.clone()),
            generation: state
                .resource
                .as_ref()
                .map(|bound| bound.generation.clone()),
            conversation: state.conversation.clone(),
            initiator: principal_word(&identity.initiator),
            controls: symphony_api::AttemptControls::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_mode_reads_back_from_its_word_and_no_other_word_names_one() {
        for mode in [StopMode::Pause, StopMode::Terminate, StopMode::HardStop] {
            assert_eq!(stop_mode(stop_word(mode)), Some(mode));
        }
        for mode in [ResumeMode::NewResource, ResumeMode::ReuseWithNewGeneration] {
            assert_eq!(resume_mode(resume_word(mode)), Some(mode));
        }
        for word in ["", "Pause", "stop", "resume", "hard stop", "new-resource"] {
            assert_eq!(stop_mode(word), None, "{word}");
            assert_eq!(resume_mode(word), None, "{word}");
        }
    }

    #[test]
    fn a_principal_is_said_by_its_kind_issuer_and_subject() {
        let said = |kind, issuer: &str, subject: &str| {
            principal_word(&Principal::new(kind, issuer.into(), subject.into()))
        };
        assert_eq!(
            said(PrincipalKind::Human, "local", "owner"),
            "human local/owner"
        );
        assert_eq!(
            said(PrincipalKind::Service, "board", "api"),
            "service board/api"
        );
        assert_eq!(
            said(PrincipalKind::Agent, "local", "amber-1"),
            "agent local/amber-1"
        );
    }

    #[test]
    fn a_record_word_is_the_one_serde_writes() {
        use super::super::record::{Cleanup, Collection, Dispatch, Ending, Relation};
        assert_eq!(word(&Dispatch::Held), "held");
        assert_eq!(word(&Ending::Superseded), "superseded");
        assert_eq!(word(&Collection::NotStarted), "not_started");
        assert_eq!(word(&Cleanup::NotDue), "not_due");
        assert_eq!(word(&Relation::Resume), "resume");
    }
}
