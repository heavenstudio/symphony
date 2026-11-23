//! The build that moves the live board is the one whose source the default branch commits, never
//! the branch the main checkout has out nor what its working copy holds: `main` unless the project
//! names another in `[project] default_branch`. A default branch that cannot be read moves the
//! board by no writer.

use super::{Held, Live, before, refused, this};
use crate::{git, project_file};
use symphony_board::live::SOURCE;

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
        project_file("live", "default_branch = \"topic\"\n"),
    )
    .unwrap();
    let ran = live.tasks(&live.wt, &["list"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert!(ran.out.contains("live/one"), "{}", ran.out);
    assert_eq!(live.version("tasks"), Some(this()));
}

#[test]
fn a_default_branch_that_cannot_be_read_is_moved_by_no_writer_not_even_migrate() {
    // `main` commits another source; the project names a default branch that does not exist,
    // then one whose ref names no commit.
    let live = Live::older("main-only-default-unread", Held::Other);
    let unread = |branch: &str, why: &str| {
        format!(
            "the live board is at schema {}, behind this build's {}, and the schema source its \
             default branch `{branch}` commits ({SOURCE}) cannot be read ({why}) to say whether \
             this build's schema is its own: nothing was written",
            before(),
            this()
        )
    };
    let names = |branch: &str| {
        std::fs::write(
            live.main.join("symphony.toml"),
            project_file("live", &format!("default_branch = \"{branch}\"\n")),
        )
        .unwrap();
    };
    names("nosuch");
    let why = unread(
        "nosuch",
        "there is no branch `nosuch`, which `[project] default_branch` names",
    );
    refused(&live.tasks(&live.main, &["migrate"]), &why);
    refused(&live.tasks(&live.wt, &["list"]), &why);
    std::fs::write(
        live.main.join(".git/refs/heads/topic"),
        "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef\n",
    )
    .unwrap();
    names("topic");
    let why = unread("topic", "refs/heads/topic names no commit");
    refused(&live.tasks(&live.main, &["migrate"]), &why);
    refused(&live.tasks(&live.wt, &["list"]), &why);
    assert_eq!(live.version("tasks"), Some(before()));
}
