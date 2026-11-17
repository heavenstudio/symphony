//! Strict candidate reader and immutable delivery-workflow definitions. Nothing here activates
//! a graph or admits a task operation. Ordinary project reads refuse custom activation until
//! the all-writer storage/admission boundary is available (docs/WORKFLOW.md).

use sha2::{Digest, Sha256};
use symphony_api::workflow::{Definition, FORMAT, Revision, Source};

mod builtin;
mod validate;
pub use builtin::builtin;

/// A validated, normalized definition. Deserialization alone never constructs this wrapper.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Validated(Definition);

impl Validated {
    pub fn new(definition: Definition) -> Result<Self, String> {
        validate::validate(&definition)?;
        Ok(Self(normalize(definition)))
    }

    pub fn definition(&self) -> &Definition {
        &self.0
    }

    /// Content identity includes format, labels/order and committed source metadata. The
    /// caller supplies metadata from its Git reader; this function does not attest provenance.
    pub fn revision(&self, source: Source) -> Result<Revision, String> {
        for (name, id) in [
            ("commit", &source.commit),
            ("project_blob", &source.project_blob),
        ] {
            if !matches!(id.len(), 40 | 64)
                || !id
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            {
                return Err(format!(
                    "workflow source {name}: expected a lowercase Git object ID"
                ));
            }
        }
        let bytes = serde_json::to_vec(&(FORMAT, &self.0, &source)).map_err(|e| e.to_string())?;
        Ok(Revision {
            format: FORMAT,
            digest: Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            source,
        })
    }

    /// Refuse a stale or substituted revision, including a different source for equal content.
    pub fn check_revision(&self, revision: &Revision) -> Result<(), String> {
        if self.revision(revision.source.clone())? != *revision {
            return Err("stale_workflow: definition or source does not match revision".into());
        }
        Ok(())
    }
}

/// Parse only the candidate `[workflow]` in a project file; other project tables belong to
/// `Project::parse`. Omission selects built-in v1. Unknown workflow fields/variants refuse.
pub fn candidate(text: &str) -> Result<Validated, String> {
    #[derive(serde::Deserialize)]
    struct File {
        workflow: Option<Definition>,
    }
    let file: File = toml::from_str(text).map_err(|e| format!("workflow: {e}"))?;
    Validated::new(file.workflow.unwrap_or_else(builtin))
}

/// Read the project file from an actual commit in a repository root, never from its index or
/// working tree. The file must be committed and a regular blob; this does not activate it.
pub fn committed(root: &std::path::Path, reference: &str) -> Result<(Validated, Revision), String> {
    let read = || -> Result<_, String> {
        let repo = gix::open(root).map_err(|e| e.to_string())?;
        let commit = repo
            .rev_parse_single(reference)
            .map_err(|e| e.to_string())?
            .object()
            .map_err(|e| e.to_string())?
            .peel_to_commit()
            .map_err(|e| e.to_string())?;
        let tree = commit.tree().map_err(|e| e.to_string())?;
        let entry = tree
            .lookup_entry_by_path(super::FILE)
            .map_err(|e| e.to_string())?
            .ok_or("committed symphony.toml is missing")?;
        if !matches!(
            entry.mode().kind(),
            gix::object::tree::EntryKind::Blob | gix::object::tree::EntryKind::BlobExecutable
        ) {
            return Err("committed symphony.toml must be a regular blob".into());
        }
        let blob = repo.find_blob(entry.id()).map_err(|e| e.to_string())?;
        let text = std::str::from_utf8(&blob.data).map_err(|e| e.to_string())?;
        let definition = candidate(text)?;
        let revision = definition.revision(Source {
            commit: commit.id.to_string(),
            project_blob: blob.id.to_string(),
        })?;
        Ok((definition, revision))
    };
    read().map_err(|e| format!("workflow committed source: {e}"))
}

/// Runtime reads may use only the omitted configuration in this contract-only release.
/// A custom candidate equal to the default is still explicit custom configuration.
pub(super) fn require_supported(definition: Option<&Definition>) -> Result<(), String> {
    if let Some(definition) = definition {
        validate::validate(definition)?;
        return Err("symphony.toml: unsupported_activation: custom workflow requires the authoritative storage/admission engine".into());
    }
    Ok(())
}

/// Host support is checked separately from syntax. Declaring a capability never supplies it.
/// These are adapter names, not membership or authorization decisions.
pub fn prerequisites(
    definition: &Validated,
    capabilities: &[&str],
    adapters: &[&str],
) -> Result<(), String> {
    for capability in &definition.0.capabilities {
        if !capabilities.contains(&capability.as_str()) {
            return Err(format!("unsupported capability: {capability}"));
        }
    }
    for adapter in &definition.0.adapters {
        if !adapters.contains(&adapter.as_str()) {
            return Err(format!("unsupported adapter: {adapter}"));
        }
    }
    Ok(())
}

fn normalize(mut d: Definition) -> Definition {
    d.phases.sort_by(|a, b| a.id.cmp(&b.id));
    d.states.sort_by(|a, b| a.id.cmp(&b.id));
    d.positions.sort_by(|a, b| a.at.cmp(&b.at));
    d.edges.sort_by(|a, b| a.id.cmp(&b.id));
    d.guards.sort_by(|a, b| a.id.cmp(&b.id));
    d.capabilities.sort();
    d.adapters.sort();
    d.task_kinds.sort();
    for edge in &mut d.edges {
        edge.inputs.sort();
        edge.guards.sort();
        // Authority is an unordered OR; effects remain in application order.
        edge.authority.sort();
    }
    d
}
