//! Versioned filesystem evidence for assessment/2. The producer writes this contract before
//! transport, in the folder the project's `[board] assessments` names; native admission reads the
//! same immutable binding before echoing its witness. Checksums detect damaged local evidence, not
//! hostile rewriting or an independent board write. Source freshness is observed once per
//! transaction attempt, never serialized with Git refs.

use crate::AssessmentState;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

#[path = "publication/model.rs"]
mod model;
#[path = "publication/read.rs"]
mod read;
pub use model::{Decision, validate_source};
pub use read::{
    ASSESSMENTS, Evidence, Source, home, read_bindings, read_event, read_fence, read_source,
};
pub type Result<T> = std::result::Result<T, String>;

/// The protocol of a landing submission, its generation pointers and verification markers.
pub const SUBMISSION: &str = "symphony-land-submission/1";
/// The protocol of an assessment event's decision.
pub const EVENT: &str = "symphony-land-assessment/1";
/// The protocol of a submission's task-to-event bindings, which [`Bindings::new`] writes.
pub const BINDING: &str = "symphony-land-binding/1";

/// Exact source identity echoed only after its immutable task/event binding is validated.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Publication {
    pub id: String,
    pub branch: String,
    pub tip: String,
    #[serde(deserialize_with = "required_option")]
    pub verification: Option<String>,
}

fn required_option<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<String>, D::Error> {
    Option::<String>::deserialize(d)
}

/// One task's original event and fixed expected state. Entries are append-only: retries reopen
/// this entry, even when the derived tasks reference has disappeared after a lost reply.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub id: String,
    pub decision: Decision,
    pub expected: AssessmentState,
    pub signature: String,
}

/// Authoritative task map, initialized empty by the producer before publishing its source.
/// Missing/corrupt primary evidence never means that a task has not been prepared.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bindings {
    pub protocol: String,
    pub publication: Publication,
    pub actor: String,
    pub target: String,
    pub kind: String,
    pub tasks: BTreeMap<String, Binding>,
}

impl Bindings {
    pub fn new(source: &Source) -> Self {
        Self {
            protocol: BINDING.into(),
            publication: source.publication.clone(),
            actor: source.actor.clone(),
            target: source.target.clone(),
            kind: source.kind.clone(),
            tasks: BTreeMap::new(),
        }
    }

    pub fn value(&self) -> Result<Value> {
        serde_json::to_value(self)
            .map(seal)
            .map_err(|error| error.to_string())
    }
}

pub fn valid_id(id: &str) -> bool {
    id.starts_with("occ-")
        && id.len() > 4
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

pub fn key(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

pub fn fingerprint(value: &Value) -> String {
    let mut value = value.clone();
    value.sort_all_objects();
    Sha256::digest(value.to_string().as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn seal(mut value: Value) -> Value {
    value["sha256"] = json!(fingerprint(&value));
    value
}

pub fn unseal(mut value: Value) -> Result<Value> {
    let checksum = value
        .as_object_mut()
        .and_then(|object| object.remove("sha256"));
    if checksum.as_ref() != Some(&json!(fingerprint(&value))) {
        return Err("assessment evidence checksum does not match its payload".into());
    }
    Ok(value)
}

pub fn target(common: &Path, server: Option<&str>, database: Option<&str>) -> String {
    let mut hash = Sha256::new();
    hash.update(common.as_os_str().as_encoded_bytes());
    for (name, value) in [("TASKS_SERVER", server), ("TASKS_DATABASE", database)] {
        hash.update(name.as_bytes());
        hash.update([u8::from(value.is_some())]);
        if let Some(value) = value {
            hash.update(value.len().to_le_bytes());
            hash.update(value.as_bytes());
        }
    }
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn fence(value: Value, task: &str) -> Result<(AssessmentState, String)> {
    let value = unseal(value)?;
    let expected: AssessmentState = serde_json::from_value(value["expected"].clone())
        .map_err(|error| format!("assessment fence: {error}"))?;
    let signature = value["signature"]
        .as_str()
        .ok_or("assessment fence has no input identity")?;
    if value["protocol"] != "assessment/1"
        || expected.slug != task
        || expected.version < 0
        || !crate::STATUSES.contains(&expected.status.as_str())
        || signature.len() != 64
        || !signature.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("assessment fence does not match its task state or input identity".into());
    }
    Ok((expected, signature.into()))
}
