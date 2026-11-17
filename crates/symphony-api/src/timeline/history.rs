//! Bounded metadata for the timeline. Text remains in the task's existing thread read.

use serde::{Deserialize, Serialize};

/// Newest recorded lines considered per source. System lines count towards this bound too.
pub const TIMELINE_HISTORY_LIMIT: usize = 256;
/// Maximum source tasks read, including the task itself. Cycles and deeper ancestry stay unknown.
pub const TIMELINE_HISTORY_SOURCES: usize = 8;

/// A source's read state. Complete means every recorded line was read, not that the board recorded
/// every historical action. Unknown words fail closed as unavailable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub enum HistoryState {
    Complete,
    Partial,
    #[default]
    #[serde(other)]
    Unavailable,
}

/// One task's metadata, bounded by source count and recorded line count. A client loading it has
/// no history until its read succeeds; failure keeps its read error, never an empty complete value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct History {
    pub sources: Vec<HistorySource>,
    /// False when the source-count bound or a failed read left ancestry unexplored. Truncated
    /// ranges are reported by `sources`, even when every referenced source was visited.
    #[serde(default)]
    pub sources_complete: bool,
    pub roles: Vec<RoleSpan>,
    /// Non-system thread entries, ordered by exact timestamp then id. No text or reply inference.
    pub messages: Vec<TimelineMessage>,
}

/// Which recorded range was read. Missing bounds mean no lines were returned. `partial` means a
/// cutoff or uncertain records; `before` distinguishes a cutoff. `unavailable` means a failed read.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct HistorySource {
    pub slug: String,
    pub state: HistoryState,
    pub first_id: Option<i64>,
    pub last_id: Option<i64>,
    /// Older entries remain reachable through `/api/task-history?slug=SLUG&before=ID`.
    /// Unlike `partial`, this specifically means earlier recorded rows were left out.
    #[serde(default)]
    pub before: Option<i64>,
}

/// A recorded hold, independent of execution stages. Its boundaries retain the record's ids and
/// exact times. A clock regression or conflicting duplicate marks it uncertain; a client must
/// not draw a definite duration or resolve an individual from an uncertain span.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct RoleSpan {
    pub role: String,
    pub by: String,
    pub source: String,
    pub from: f64,
    pub from_id: i64,
    pub to: Option<f64>,
    pub to_id: Option<i64>,
    pub uncertain: bool,
}

/// The endpoint's historical resolution, never evidence of delivery, reading or causality.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub enum EndpointState {
    Known,
    Ambiguous,
    Everyone,
    Outside,
    #[default]
    #[serde(other)]
    Unknown,
}

/// A possible role at send time, with the task whose recorded assignment supplies it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct RoleEndpoint {
    pub role: String,
    pub by: String,
    pub source: String,
}

/// Candidates are preserved when several roles or holders fit; no one is chosen. An unknown
/// endpoint can retain candidates from an incomplete source without claiming they are exhaustive.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct MessageEndpoint {
    pub state: EndpointState,
    pub candidates: Vec<RoleEndpoint>,
}

/// One selectable thread entry. Its original sender, recipient, kind and timestamp remain exact.
/// `None` recipient means Everyone on task. Fetch full text by id through existing thread reads.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TimelineMessage {
    pub id: i64,
    pub at: f64,
    pub sender: String,
    pub recipient: Option<String>,
    pub kind: String,
    pub from: MessageEndpoint,
    pub to: MessageEndpoint,
    /// Unknown when incomplete records cannot establish whether the task was closed then.
    pub after_close: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TaskTimeline;

    #[test]
    fn absent_history_is_unknown_while_an_explicit_empty_complete_source_is_known_empty() {
        let old: TaskTimeline =
            serde_json::from_str(include_str!("../../fixtures/task-timeline.json")).unwrap();
        assert!(old.history.is_none());
        assert!(serde_json::to_value(old).unwrap().get("history").is_none());
        let history = History {
            sources: vec![HistorySource {
                slug: "task".into(),
                state: HistoryState::Complete,
                ..HistorySource::default()
            }],
            ..History::default()
        };
        assert_eq!(
            serde_json::from_value::<History>(serde_json::to_value(&history).unwrap()).unwrap(),
            history
        );
        assert_eq!(
            serde_json::from_str::<HistoryState>("\"future\"").unwrap(),
            HistoryState::Unavailable
        );
        assert_eq!(
            serde_json::from_str::<EndpointState>("\"future\"").unwrap(),
            EndpointState::Unknown
        );
    }

    #[test]
    fn metadata_round_trips_exact_addresses_boundaries_uncertainty_and_postclosure_extent() {
        let history = History {
            sources_complete: true,
            sources: vec![HistorySource {
                slug: "task".into(),
                state: HistoryState::Partial,
                first_id: Some(100),
                last_id: Some(101),
                before: Some(100),
            }],
            roles: vec![RoleSpan {
                role: "orchestrator".into(),
                by: "a".into(),
                source: "parent".into(),
                from: 30.0,
                from_id: 90,
                to: Some(20.0),
                to_id: Some(100),
                uncertain: true,
            }],
            messages: vec![TimelineMessage {
                id: 101,
                at: 60.0,
                sender: "outside".into(),
                recipient: Some("@orchestrator".into()),
                kind: "msg".into(),
                after_close: Some(true),
                ..TimelineMessage::default()
            }],
        };
        let timeline = TaskTimeline {
            slug: "task".into(),
            created: 1.0,
            closed: Some(40.0),
            now: 70.0,
            history: Some(history.clone()),
            ..TaskTimeline::default()
        };
        assert!(timeline.history.as_ref().unwrap().messages[0].at > timeline.closed.unwrap());
        assert_eq!(timeline.closed, Some(40.0));
        let json = serde_json::to_value(&timeline).unwrap();
        assert!(json["history"]["messages"][0].get("text").is_none());
        assert_eq!(
            serde_json::from_value::<TaskTimeline>(json)
                .unwrap()
                .history,
            Some(history)
        );
    }
}
