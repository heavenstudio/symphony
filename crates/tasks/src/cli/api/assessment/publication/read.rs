//! Read-only admission and currentness. Immutable evidence is required even for a retry; mutable
//! source errors are passed into the pair-first Store query so a known receipt remains readable.

use super::*;
use std::fs;
use std::path::PathBuf;

/// The project file's key naming the folder of the project's landing assessments (`[board]
/// assessments`, [`crate::project::board_path`]).
pub const ASSESSMENTS: &str = "assessments";

#[derive(Clone, Debug)]
pub struct Source {
    pub publication: Publication,
    pub actor: String,
    pub target: String,
    pub kind: String,
}

pub struct EventEvidence {
    pub publication: String,
    pub decision: Decision,
}

pub struct Evidence {
    source: Source,
    common: PathBuf,
    cwd: PathBuf,
    target: String,
}

impl Evidence {
    /// Discover the repository from the native caller's actual cwd. No request supplies a path.
    pub fn admit(
        context: &crate::cli::Env,
        publication: &Publication,
        slug: &str,
        assessment: &str,
        record: &str,
        expected: &AssessmentState,
    ) -> Result<Self> {
        let cwd = std::env::current_dir().map_err(|error| format!("assessment cwd: {error}"))?;
        let common = crate::git::common_dir(&cwd).map_err(|error| error.to_string())?;
        let target = target(
            &common,
            context.vars.get("TASKS_SERVER").map(String::as_str),
            context.vars.get("TASKS_DATABASE").map(String::as_str),
        );
        let source = read_source(&common, &publication.id)?;
        if source.publication != *publication || source.target != target {
            return Err(
                "assessment publication does not match its immutable source or target".into(),
            );
        }
        let bindings = read_bindings(&common, &source)?;
        let binding = bindings
            .tasks
            .get(slug)
            .ok_or("assessment task has no original event binding")?;
        validate_binding(&common, &source, binding)?;
        if binding.id != assessment
            || binding.decision.record != record
            || binding.expected != *expected
        {
            return Err(
                "assessment request does not match its original event and frozen state".into(),
            );
        }
        Ok(Self {
            source,
            common,
            cwd,
            target,
        })
    }

    /// One observed snapshot, called again on every real Store transaction retry. A later ref
    /// update can race commit; callers must also check after the reply before claiming currentness.
    pub fn current(&self) -> Result<bool> {
        self.source.current(&self.cwd, &self.common, &self.target)
    }
}

impl Source {
    pub fn current(&self, cwd: &Path, common: &Path, actual_target: &str) -> Result<bool> {
        if self.target != actual_target {
            return Err("assessment target changed; evidence remains pending".into());
        }
        let publication = &self.publication;
        let path = home(common)?.join("latest").join(key(&publication.branch));
        if !path.exists() {
            return Err(format!(
                "assessment generation pointer is missing: {}",
                path.display()
            ));
        }
        let latest = read(&path)?;
        let id = latest["id"]
            .as_str()
            .filter(|id| valid_id(id))
            .ok_or("assessment generation pointer has no valid identity")?;
        if !is(&latest, SUBMISSION) || latest["branch"] != publication.branch {
            return Err("assessment generation pointer is incompatible".into());
        }
        if id != publication.id {
            return Ok(false);
        }
        let tip = crate::sweep::git::rev(cwd, &format!("refs/heads/{}", publication.branch))
            .ok_or("assessment branch is unreadable")?;
        if tip != publication.tip {
            return Ok(false);
        }
        if let Some(run) = &publication.verification {
            let marker = read(
                &home(common)?
                    .join("verifications")
                    .join(key(&publication.branch)),
            )
            .map_err(|error| {
                format!("verification assessment generation is unreadable: {error}")
            })?;
            if !is(&marker, SUBMISSION) || !marker["run"].is_string() || !marker["tip"].is_string()
            {
                return Err("verification assessment generation has an unknown protocol or malformed marker".into());
            }
            if marker["run"] != *run || marker["tip"] != publication.tip {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

pub fn read_source(common: &Path, id: &str) -> Result<Source> {
    if !valid_id(id) {
        return Err(format!("invalid assessment submission identity {id:?}"));
    }
    let value = read(
        &home(common)?
            .join("submissions")
            .join(id)
            .join("submission.json"),
    )?;
    let string = |name: &str| {
        value[name]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| format!("assessment submission has no string {name}"))
    };
    let publication: Publication = serde_json::from_value(json!({
        "id": id, "branch": value["branch"], "tip": value["tip"],
        "verification": value.get("verification").ok_or("assessment source has no verification field")?,
    })).map_err(|error| format!("assessment source: {error}"))?;
    let actor = string("actor")?;
    let kind = value["outcome"]["kind"]
        .as_str()
        .ok_or("assessment source has no kind")?;
    validate_source(
        &publication.branch,
        &publication.tip,
        &actor,
        publication.verification.as_deref(),
    )?;
    if !is(&value, SUBMISSION)
        || value["id"] != id
        || !matches!(kind, "assessment" | "landing" | "verification")
    {
        return Err("assessment submission is incompatible".into());
    }
    Ok(Source {
        publication,
        actor,
        kind: kind.into(),
        target: string("target")?,
    })
}

pub fn read_bindings(common: &Path, source: &Source) -> Result<Bindings> {
    let path = home(common)?
        .join("submissions")
        .join(&source.publication.id)
        .join("binding.json");
    let bindings: Bindings = serde_json::from_value(unseal(read(&path)?)?)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if bindings.protocol != BINDING
        || bindings.publication != source.publication
        || bindings.actor != source.actor
        || bindings.target != source.target
        || bindings.kind != source.kind
    {
        return Err("assessment task binding does not match its source".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for (task, binding) in &bindings.tasks {
        if task != &binding.decision.task || !ids.insert(&binding.id) {
            return Err("assessment task binding is repeated or mismatched".into());
        }
        validate_binding(common, source, binding)?;
    }
    Ok(bindings)
}

fn validate_binding(common: &Path, source: &Source, binding: &Binding) -> Result<()> {
    let event = read_event(common, &binding.id)?;
    let (expected, signature) = read_fence(common, &binding.id, &binding.decision.task)?;
    let decision = &binding.decision;
    if event.publication != source.publication.id
        || event.decision != *decision
        || decision.branch != source.publication.branch
        || decision.tip != source.publication.tip
        || decision.actor != source.actor
        || decision.verification != source.publication.verification
        || binding.expected != expected
        || binding.signature != signature
    {
        return Err("assessment event does not match its immutable publication binding".into());
    }
    Ok(())
}

pub fn read_event(common: &Path, id: &str) -> Result<EventEvidence> {
    if !valid_id(id) {
        return Err(format!("invalid assessment identity {id:?}"));
    }
    let value = read(&home(common)?.join("events").join(id).join("decision.json"))?;
    let publication = value["publication"]
        .as_str()
        .filter(|id| valid_id(id))
        .ok_or("assessment event has no publication binding")?;
    if !is(&value, EVENT) || value["id"] != id {
        return Err("assessment event journal is incompatible".into());
    }
    Ok(EventEvidence {
        publication: publication.into(),
        decision: Decision::read(&value["decision"])?,
    })
}

pub fn read_fence(common: &Path, id: &str, task: &str) -> Result<(AssessmentState, String)> {
    if !valid_id(id) {
        return Err(format!("invalid assessment event identity {id:?}"));
    }
    let path = home(common)?.join("events").join(id).join("fence.json");
    read(&path)
        .map_err(|error| format!("pending assessment fence is unreadable: {error}"))
        .and_then(|value| fence(value, task))
        .map_err(|error| format!("assessment fence {}: {error}", path.display()))
}

/// The folder of the landing assessments of the repository whose common dir is `common`, as its
/// main checkout's project file names it ([`ASSESSMENTS`]); a project that names none has none, and
/// its assessments are refused.
pub fn home(common: &Path) -> Result<PathBuf> {
    crate::project::board_path(common, ASSESSMENTS).ok_or_else(|| {
        format!(
            "no landing assessments: the project names no folder for them ([board] {ASSESSMENTS} in {})",
            crate::project::FILE
        )
    })
}

/// Whether the evidence `value` is written by the protocol `name`.
fn is(value: &Value, name: &str) -> bool {
    value["protocol"] == name
}

fn read(path: &Path) -> Result<Value> {
    let bytes =
        fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))
}

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;
