//! A task's execution attempts on the wire
//! ([`docs/engineering/EXECUTION_ATTEMPTS.md`](../../../docs/engineering/EXECUTION_ATTEMPTS.md#on-a-tasks-page)): its history as [`TASK_ATTEMPTS_PATH`] answers it, every attempt kept after its handle left, with
//! the resource and conversation bound to it as references, never as links; the controls a page
//! posts to [`ATTEMPT_CONTROL_ROUTE`]; and a view of an attempt's resource, resolved on demand at
//! [`ATTEMPT_VIEW_ROUTE`] for the one who asks and kept nowhere.
//!
//! Each state is the board's word for it (`symphony_board::execution`), in snake case: a word a
//! newer server adds is drawn as it is said.

use serde::{Deserialize, Serialize};

/// A task's attempts, `?slug=`: [`TaskAttempts`].
pub const TASK_ATTEMPTS_PATH: &str = "/api/task-attempts";
/// Cancel, stop or resume an attempt: an [`AttemptControl`], answered with the task's
/// [`TaskAttempts`] as they are after it.
pub const ATTEMPT_CONTROL_ROUTE: &str = "/api/attempt-control";
/// A view of an attempt's resource: an [`AttemptViewAsk`], answered with an [`AttemptLink`].
pub const ATTEMPT_VIEW_ROUTE: &str = "/api/attempt-view";

/// A task's execution attempts, oldest first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TaskAttempts {
    pub attempts: Vec<AttemptView>,
}

/// One attempt as a reader sees it: who ran it in which lane, what it follows, where each of its
/// dimensions stands, and what it was bound to.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct AttemptView {
    /// Its UUID, 32 hex digits.
    pub id: String,
    /// When it was admitted, in ms since the epoch.
    pub created: i64,
    /// The revision a control names, refused as stale once it moved.
    pub revision: i64,
    /// Whether it holds its lane: a later attempt of the lane supersedes it.
    pub current: bool,
    pub lane: String,
    pub role: String,
    #[serde(default)]
    pub handle: Option<String>,
    /// The provider configuration it ran under.
    pub configuration: String,
    /// The attempt it follows, and how: `retry` or `resume`.
    #[serde(default)]
    pub predecessor: Option<String>,
    #[serde(default)]
    pub relation: Option<String>,
    /// The attempt that followed it.
    #[serde(default)]
    pub successor: Option<String>,
    /// `pending`, `starting`, `unknown`, `running`, `paused`, `held` or `terminal`.
    pub dispatch: String,
    /// How a terminal attempt ended: `succeeded`, `failed`, `cancelled`, `stopped`, `rejected` or
    /// `superseded`.
    #[serde(default)]
    pub ending: Option<String>,
    /// Why it waits on a person.
    #[serde(default)]
    pub held: Option<String>,
    /// `none`, `requested`, `confirmed` or `unconfirmed`.
    pub cancellation: String,
    /// `not_started`, `collecting`, `complete`, `partial`, `failed` or `none`.
    pub collection: String,
    /// `not_due`, `running`, `complete`, `failed` or `unknown`.
    pub cleanup: String,
    /// The provider's opaque id of the resource bound to it, and its generation: a reference that
    /// outlives the handle, never an address or a credential.
    #[serde(default)]
    pub resource: Option<String>,
    #[serde(default)]
    pub generation: Option<String>,
    /// The runtime conversation bound to it.
    #[serde(default)]
    pub conversation: Option<String>,
    /// Who asked for it: `KIND ISSUER/SUBJECT`.
    pub initiator: String,
    /// What the one who read it may ask of it here.
    #[serde(default)]
    pub controls: AttemptControls,
}

/// What a reader may ask of an attempt, as the server that answered judged it: offered here, and
/// decided again when asked.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct AttemptControls {
    pub cancel: bool,
    /// The stop modes it may be asked: `pause`, `terminate`, `hard_stop`.
    #[serde(default)]
    pub stop: Vec<String>,
    /// The modes a resume of it may take: `new_resource`, `reuse_with_new_generation`.
    #[serde(default)]
    pub resume: Vec<String>,
    /// Whether a view of its resource may be asked.
    #[serde(default)]
    pub view: bool,
    /// Why a control its state allows is not offered here, as a sentence the page shows: its
    /// provider is not one this server holds, or it is a local terminal its launcher steps.
    #[serde(default)]
    pub unserved: Option<String>,
}

/// A control of one attempt of task `slug`, as the page that read it at `revision` asks it.
///
/// Its fields are all it takes: who asks is the one the server authenticated, never a field of
/// the request, so a request with any other field is refused.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct AttemptControl {
    pub slug: String,
    pub attempt: String,
    pub revision: i64,
    /// `cancel`, `stop` or `resume`.
    pub act: String,
    /// The stop's or the resume's mode, one of the attempt's [`AttemptControls`].
    #[serde(default)]
    pub mode: Option<String>,
    /// A resume's request key: the same key admits the same attempt, however often it is sent.
    #[serde(default)]
    pub key: Option<String>,
}

/// A view asked of one attempt of task `slug`; its fields are all it takes, as an
/// [`AttemptControl`]'s.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct AttemptViewAsk {
    pub slug: String,
    pub attempt: String,
}

/// A view of an attempt's resource, resolved now for the one who asked: opened at once and kept
/// nowhere, as it lasts `valid_for_ms` at most and may carry the provider's credential.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct AttemptLink {
    pub url: String,
    pub valid_for_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_control_takes_its_own_fields_alone() {
        let asked = json!({"slug": "tools/x", "attempt": "ab", "revision": 3, "act": "cancel"});
        let control: AttemptControl = serde_json::from_value(asked).unwrap();
        assert_eq!((control.mode, control.key), (None, None));
        for forged in [
            json!({"slug": "tools/x", "attempt": "ab", "revision": 3, "act": "cancel", "owner": "owner"}),
            json!({"slug": "tools/x", "attempt": "ab", "revision": 3, "act": "cancel",
                   "principal": {"kind": "Human", "issuer": "local", "subject": "owner"}}),
        ] {
            let refused = serde_json::from_value::<AttemptControl>(forged).unwrap_err();
            assert!(refused.to_string().contains("unknown field"), "{refused}");
        }
        let view = json!({"slug": "tools/x", "attempt": "ab", "grant": "all"});
        assert!(serde_json::from_value::<AttemptViewAsk>(view).is_err());
    }

    #[test]
    fn an_attempt_read_without_its_controls_offers_none() {
        let read = json!({
            "id": "ab", "created": 1, "revision": 2, "current": true, "lane": "executor/a",
            "role": "executor", "configuration": "cloud", "dispatch": "running",
            "cancellation": "none", "collection": "not_started", "cleanup": "not_due",
            "initiator": "human local/owner",
        });
        let attempt: AttemptView = serde_json::from_value(read).unwrap();
        assert_eq!(attempt.controls, AttemptControls::default());
        assert_eq!(attempt.resource, None);
    }
}
