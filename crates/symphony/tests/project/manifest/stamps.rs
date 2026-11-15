//! `[stamps]`: the catalogue of stamps a project's land can require, which the board reads itself
//! from the main checkout's committed file (`symphony_board::stamps`). Each stamp the Symphony has a
//! reader for is known here by its name, so another is refused by name and line as any key the
//! Symphony does not know; a catalogue the board would refuse (an empty one, a key no stamp has) is
//! refused by a load and the lint as the board says it, while the running project reads on.

use super::{folder, parsed, refused};
use crate::Tree;
use symphony::lint::project_problems;
use symphony::project::{FILE, Project};

#[test]
fn a_catalogue_of_stamps_with_readers_is_a_setting_and_changes_nothing_else() {
    let written = "[stamps.tests]\ndefault = true\n\n[stamps.review]\napplies = \"every change\"\n";
    assert_eq!(parsed(written).unwrap(), parsed("").unwrap());
    assert_eq!(
        parsed("stamps = { owner = {} }\n").unwrap(),
        parsed("").unwrap()
    );
}

#[test]
fn a_stamp_with_no_reader_is_no_setting_and_a_catalogue_the_board_cannot_read_is_refused_as_it_says_it()
 {
    assert_eq!(
        refused("[stamps.review]\n[stamps.lint]\n"),
        "symphony.toml: [stamps] lint (line 2) is not a setting the Symphony knows"
    );
    assert_eq!(
        refused("[stamps]\n"),
        "symphony.toml: [stamps] names no stamp: leave the table out for the Symphony's own \
         four, or write each stamp the project's land can require"
    );
    let why = refused("[stamps.review]\nneeded = true\n");
    assert!(why.starts_with("symphony.toml: "), "{why}");
    assert!(why.contains("unknown field `needed`"), "{why}");
}

#[test]
fn the_lint_names_a_bad_catalogue_and_the_running_project_reads_the_rest() {
    let tree = Tree::new();
    let root = folder(&tree, "Lamp Works");
    std::fs::write(root.join(FILE), "[project]\nname = \"Lamp\"\n\n[stamps]\n").unwrap();
    let said = "symphony.toml: [stamps] names no stamp: leave the table out for the Symphony's own \
                four, or write each stamp the project's land can require";
    assert_eq!(project_problems(&root), [said]);
    assert_eq!(Project::load(&root).unwrap_err(), said);
    assert_eq!(Project::of(&root).name, "Lamp");
    // A stamp with no reader is left out by name as any key the Symphony does not know.
    std::fs::write(
        root.join(FILE),
        "[project]\nname = \"Lamp\"\n\n[stamps.security]\ndefault = true\n",
    )
    .unwrap();
    assert_eq!(
        project_problems(&root),
        ["symphony.toml: [stamps] security (line 4) is not a setting the Symphony knows"]
    );
    assert_eq!(Project::of(&root).name, "Lamp");
}
