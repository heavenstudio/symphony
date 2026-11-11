//! Immutable decision payload shared by the producer and native admission validator.

use serde_json::{Value, json};
type Result<T> = std::result::Result<T, String>;

/// The immutable evidence a production decision submits. Board state and a confirmed receipt
/// belong beside this payload, so neither a retry nor a delayed reply can rewrite its decision.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub branch: String,
    pub task: String,
    pub tip: String,
    pub actor: String,
    pub record: String,
    pub changes_requested: bool,
    /// A verification completion names its original run, never whichever run stands later.
    #[serde(deserialize_with = "super::required_option")]
    pub verification: Option<String>,
}

impl Decision {
    pub fn validate(&self) -> Result<()> {
        validate_source(
            &self.branch,
            &self.tip,
            &self.actor,
            self.verification.as_deref(),
        )?;
        text("task", &self.task)?;
        let waits = crate::alarm::Waits::parse(&self.record)
            .ok_or_else(|| format!("invalid assessment record: {:?}", self.record))?;
        if waits.tip != self.tip || self.record.chars().any(char::is_control) {
            return Err(format!(
                "assessment record does not judge tip {}: {:?}",
                self.tip, self.record
            ));
        }
        if waits.changes() != self.changes_requested {
            return Err("assessment changes signal disagrees with its record".into());
        }
        Ok(())
    }

    pub fn value(&self) -> Value {
        json!({
            "branch": self.branch, "task": self.task, "tip": self.tip,
            "actor": self.actor, "record": self.record, "verification": self.verification,
            "changes_requested": self.changes_requested,
        })
    }

    pub fn read(value: &Value) -> Result<Self> {
        if !value["changes_requested"].is_boolean() {
            return Err(format!(
                "assessment decision has no boolean changes_requested: {}",
                value["changes_requested"]
            ));
        }
        let decision: Self = serde_json::from_value(value.clone())
            .map_err(|error| format!("assessment decision: {error}"))?;
        decision.validate()?;
        Ok(decision)
    }
}

pub fn validate_source(
    branch: &str,
    tip: &str,
    actor: &str,
    verification: Option<&str>,
) -> Result<()> {
    text("branch", branch)?;
    text("actor", actor)?;
    if !matches!(tip.len(), 40 | 64) || !tip.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("assessment tip is not a full commit: {tip:?}"));
    }
    if let Some(run) = verification {
        text("verification", run)?;
    }
    Ok(())
}

fn text(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        return Err(format!("assessment {name} is invalid: {value:?}"));
    }
    Ok(())
}
