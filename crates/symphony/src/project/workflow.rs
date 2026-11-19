//! Strict candidate reader and immutable delivery-workflow definitions, re-exported from the
//! board, which runs them (`symphony_board::workflow`, docs/WORKFLOW.md). Ordinary project reads
//! accept a custom workflow the local host supports, which the board activates once the project's
//! default branch commits it, and refuse one it does not, naming what it needs.

use symphony_api::workflow::{Definition, Revision, Source};
pub use symphony_board::workflow::{
    Attester, Mapping, Validated, builtin, candidate, prerequisites,
};

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

/// A custom workflow is valid, its mappings map onto it, its approval authorities are ones it
/// declares, and the local host supports all it needs
/// (`symphony_board::workflow::activate::supported_with`): a capability its grants name, an
/// authority committed for each guard, no route around a guard, and nothing this release cannot
/// run yet. Refused as `unsupported_activation`, naming what, so the project learns before its
/// default branch commits a workflow the board would not activate. An authority's attestations
/// name the project by its committed `[project] id` (`project`), which it then needs.
pub(super) fn require_supported(
    definition: Option<&Definition>,
    mappings: &[Mapping],
    authorities: &[Attester],
    project: Option<&str>,
) -> Result<(), String> {
    if let Some(definition) = definition {
        let validated = Validated::new(definition.clone())?;
        validated.check_mappings(mappings)?;
        symphony_board::workflow::guard::check_attesters(&validated, authorities)
            .map_err(|why| format!("symphony.toml: {why}"))?;
        if !authorities.is_empty() && project.is_none_or(|id| id.trim().is_empty()) {
            return Err(
                "symphony.toml: workflow_authorities: an attestation names the project it is \
                 for, and [project] id names none"
                    .into(),
            );
        }
        let host = symphony_board::workflow::Host::local();
        return symphony_board::workflow::activate::supported_with(&validated, &host, authorities)
            .map_err(|why| format!("symphony.toml: unsupported_activation: {why}"));
    }
    if !mappings.is_empty() {
        return Err("symphony.toml: workflow_mappings: no [workflow] to map onto".into());
    }
    if !authorities.is_empty() {
        return Err("symphony.toml: workflow_authorities: no [workflow] to guard".into());
    }
    Ok(())
}
