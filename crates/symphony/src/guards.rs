//! The guards of a task a custom workflow holds, as its page reads them
//! ([`symphony_api::workflow::WORKFLOW_GUARDS_PATH`], docs/WORKFLOW.md, "The app"): the board's own
//! records of what it accepted from each approval authority and what it admitted
//! (`symphony_board::Store::workflow_guards`), with the two things only this host knows: whether a
//! plugin the project selects answers for an authority, so the owner's take asks it
//! (`crate::save`), and the page the project file names for an authority's ledger
//! (`[[workflow_authorities]]`'s `page`, docs/PROJECT_FILE.md), filled in for each guard of this
//! task. It shows and links; nothing here approves, grants or decides a guard.

use std::path::Path;

use symphony_api::workflow::TaskGuards;
use symphony_board::workflow::{authority_pages, page_for};

/// `slug`'s guards on `store`, the board of the project at `root`.
pub fn of(
    root: &Path,
    store: &symphony_board::Store,
    slug: &str,
) -> Result<TaskGuards, symphony_board::Error> {
    let read = store.workflow_guards(slug)?;
    if read.guards.is_empty() {
        return Ok(read);
    }
    // The file as the checkout holds it: a page is a link alone, which a file that does not read
    // leaves out.
    let file = std::fs::read_to_string(root.join(crate::project::FILE)).unwrap_or_default();
    Ok(filled(
        read,
        &crate::plugins::Plugins::of(root),
        &file,
        slug,
    ))
}

/// `read`, the board's records of `slug`'s guards, with what this host knows: whether one of
/// `plugins` answers for each authority, and each guard's page as the project file `file` names
/// its authority's.
pub(crate) fn filled(
    mut read: TaskGuards,
    plugins: &crate::plugins::Plugins,
    file: &str,
    slug: &str,
) -> TaskGuards {
    for authority in &mut read.authorities {
        authority.asked_here = plugins.authority(&authority.adapter).is_some();
    }
    let pages = authority_pages(file);
    for need in &mut read.guards {
        need.page = (pages.get(&need.adapter)).map(|page| page_for(page, slug, &need.guard));
    }
    read
}
