//! The build that moves the live board is the one whose source the default branch commits, never
//! the branch the main checkout has out nor what its working copy holds: `main` unless the project
//! names another in `[project] default_branch`.

use super::{Held, Live, before, refused, this};
use crate::git;

#[test]
fn a_branch_the_main_checkout_has_out_is_no_default_branch_unless_the_project_names_it() {
    // `main` commits another source than this build's.
    let live = Live::older("main-only-default-branch", Held::Other);
    let why = live.another_source();
    // The main checkout has a branch out that commits this build's source, which its working copy
    // holds too.
    git(&live.main, &["checkout", "-qb", "topic"]);
    live.hold(Held::Own);
    for cwd in [&live.main, &live.wt] {
        refused(&live.tasks(cwd, &["list"]), &why);
    }
    refused(&live.tasks(&live.main, &["migrate"]), &why);
    assert_eq!(live.version("tasks"), Some(before()));

    // A project whose default branch is `topic`, and says so, has this build for its own.
    std::fs::write(
        live.main.join("symphony.toml"),
        "[project]\nid = \"live\"\ndefault_branch = \"topic\"\n",
    )
    .unwrap();
    let ran = live.tasks(&live.wt, &["list"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert!(ran.out.contains("live/one"), "{}", ran.out);
    assert_eq!(live.version("tasks"), Some(this()));
}
