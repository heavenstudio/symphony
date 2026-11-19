//! Strict candidate reader and immutable delivery-workflow definitions, re-exported from the
//! board, which runs them (`symphony_board::workflow`, docs/WORKFLOW.md). Ordinary project reads
//! refuse custom activation until the all-writer storage/admission boundary is available.

use symphony_api::workflow::{Definition, Revision, Source};
pub use symphony_board::workflow::{Mapping, Validated, builtin, candidate, prerequisites};

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
pub(super) fn require_supported(
    definition: Option<&Definition>,
    mappings: &[Mapping],
) -> Result<(), String> {
    if let Some(definition) = definition {
        Validated::new(definition.clone())?.check_mappings(mappings)?;
        return Err("symphony.toml: unsupported_activation: custom workflow requires the authoritative storage/admission engine".into());
    }
    if !mappings.is_empty() {
        return Err("symphony.toml: workflow_mappings: no [workflow] to map onto".into());
    }
    Ok(())
}
