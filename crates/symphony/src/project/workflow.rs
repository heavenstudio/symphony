//! Strict candidate reader and immutable delivery-workflow definitions, re-exported from the
//! board, which runs them (`symphony_board::workflow`, docs/WORKFLOW.md). Ordinary project reads
//! accept a custom workflow the local host supports, which the board activates once the project's
//! default branch commits it, and refuse one it does not, naming what it needs.

use symphony_api::workflow::{Revision, Source};
use symphony_board::workflow::written;
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

/// A custom workflow is valid, its mappings map onto it, and the local host supports all it
/// needs (`symphony_board::workflow::activate::supported`): a capability its grants name, and
/// nothing this release cannot run yet. Refused as `unsupported_activation`, naming what, so the
/// project learns before its default branch commits a workflow the board would not activate. The
/// file `text` is read by the board's own reader ([`written`]), as the board reads it when the
/// branch commits it: a field it refuses is named, without TOML's line.
pub(super) fn require_supported(text: &str) -> Result<(), String> {
    let (definition, _) = written(text).map_err(|why| format!("{}: {why}", super::FILE))?;
    if let Some(validated) = definition {
        let host = symphony_board::workflow::Host::local();
        return symphony_board::workflow::activate::supported(&validated, &host)
            .map_err(|why| format!("symphony.toml: unsupported_activation: {why}"));
    }
    Ok(())
}
