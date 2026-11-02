//! Document tree, summary and page data: folder entries, sections, headings and
//! crumbs. Route redirects and the to-top link are tested with the server.

#[path = "../common/mod.rs"]
mod common;

mod document;
mod document_cache;
mod folder;
mod summary;

use common::Tree;

/// The project every folder case reads.
pub fn docs() -> Tree {
    let tree = Tree::new();
    tree.write("docs/README.md", "# Star Foundry docs\n\nStart here.\n");
    tree.write(
        "docs/SPEC.md",
        "---\nstatus: approved\n---\n# The game\n\nWhat the game is, in one place.\n",
    );
    tree.write("docs/data.json", "{}");
    tree.write(
        "docs/ships/README.md",
        "# Ships\n\nEvery hull, by faction.\n",
    );
    tree.write(
        "docs/ships/empire.md",
        "---\nstatus: review\n---\n# Empire ships\n\nEvery Empire hull.\n\n- [ ] **x** — a card\n",
    );
    tree.write("docs/ships/old/notes.md", "No heading here, just a line.\n");
    std::fs::create_dir_all(tree.path("docs/empty")).unwrap();
    std::fs::create_dir_all(tree.path("artifacts")).unwrap();
    tree
}
