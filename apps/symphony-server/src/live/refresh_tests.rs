//! What a refresh renders again of the documents a page follows: every one once the page's own
//! files changed or the project's settings were edited, else only one whose cards or linking tasks
//! moved. A ref, a worktree's record, a branch's edit or a scan with no file renders
//! none of main's.

use super::Hub;
use super::read_paths_tests::observe;
use crate::testing::{
    Tree,
    board::{Fixture, Seed},
    git, repository,
};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Whether the copy of `rel` the page `scope` keeps says `words`, anywhere in it.
fn kept_says(hub: &Hub, scope: &str, rel: &str, words: &str) -> bool {
    serde_json::to_string(&hub.pages.read().unwrap()[scope].documents[rel])
        .unwrap()
        .contains(words)
}

/// The last change published to `scope`'s page.
fn last_published(hub: &Hub, scope: &str) -> String {
    hub.pages.read().unwrap()[scope]
        .history
        .back()
        .map(|event| event.json.clone())
        .unwrap_or_default()
}

/// A hub over `root` following `docs/TOPIC.md` and `docs/OTHER.md` on main's page, then
/// `docs/OTHER.md` edited where no event told the hub: only a render of it reads the edit.
fn following(tree: &Tree, root: &Path) -> Hub {
    tree.write("main/docs/OTHER.md", "# Other\n\nThe first words.\n");
    let hub = Hub::new(root, &root.join("build/client"));
    hub.refresh_docs(None).unwrap();
    hub.document("", "docs/TOPIC.md").unwrap();
    hub.document("", "docs/OTHER.md").unwrap();
    tree.write("main/docs/OTHER.md", "# Other\n\nThe later words.\n");
    hub
}

#[test]
fn a_refresh_for_no_file_of_mains_renders_none_of_its_documents_and_one_for_any_renders_them_all() {
    let tree = Tree::new();
    let root = repository(&tree);
    let hub = following(&tree, &root);
    let common = root.join(".git");
    let cases: [(&str, BTreeSet<PathBuf>); 4] = [
        (
            "a branch's ref",
            BTreeSet::from([common.join("refs/heads/tools/x")]),
        ),
        (
            "a worktree's record",
            BTreeSet::from([common.join("worktrees/x/HEAD")]),
        ),
        (
            "a document of a worktree inside main's folder",
            BTreeSet::from([root.join(".claude/worktrees/x/docs/TOPIC.md")]),
        ),
        ("a scan with no file", BTreeSet::new()),
    ];
    for (case, dirty) in cases {
        hub.refresh_docs(Some(&dirty)).unwrap();
        assert!(
            kept_says(&hub, "", "docs/OTHER.md", "The first words."),
            "{case}: main's documents were rendered again"
        );
    }

    // Any file of main's documents renders every one followed: a render reads more than its own
    // file (its crumbs, its links, the cards the others hold).
    hub.refresh_docs(Some(&BTreeSet::from([root.join("docs/TOPIC.md")])))
        .unwrap();
    assert!(kept_says(&hub, "", "docs/OTHER.md", "The later words."));
    let published = last_published(&hub, "");
    assert!(published.contains("The later words."), "{published}");

    // A lost event names main's root, and a full refresh names nothing: each renders them all.
    for (case, dirty) in [
        ("a lost event", Some(BTreeSet::from([root.clone()]))),
        ("a full refresh", None),
    ] {
        let words = format!("Words after {case}.");
        tree.write("main/docs/OTHER.md", &format!("# Other\n\n{words}\n"));
        hub.refresh_docs(dirty.as_ref()).unwrap();
        assert!(kept_says(&hub, "", "docs/OTHER.md", &words), "{case}");
    }
}

#[test]
fn an_edit_to_the_projects_settings_loading_or_not_renders_mains_documents_again() {
    let tree = Tree::new();
    let root = repository(&tree);
    let hub = following(&tree, &root);
    // The loop's read: what the hub was asked to read since the last.
    let read = |hub: &Hub| {
        let dirty = std::mem::take(&mut *hub.dirty.lock().unwrap());
        hub.refresh_docs(Some(&dirty)).unwrap();
    };
    hub.tell_project();
    read(&hub);
    assert!(
        kept_says(&hub, "", "docs/OTHER.md", "The first words."),
        "settings left as they were ask for no render"
    );

    // An entrypoint added: the settings still load, and its documents' links and routes are
    // others.
    tree.write("main/notes/README.md", "# Notes\n");
    std::fs::write(
        root.join(symphony::project::FILE),
        "[[entrypoints]]\nurl = \"/docs/\"\nfolder = \"docs\"\n\n[[entrypoints]]\nurl = \"/notes/\"\nfolder = \"notes\"\n",
    )
    .unwrap();
    assert_eq!(symphony::project::Project::problem(&root), None);
    hub.tell_project();
    read(&hub);
    assert!(kept_says(&hub, "", "docs/OTHER.md", "The later words."));

    // Served with the defaults from now on: its renderers are others.
    tree.write(
        "main/docs/OTHER.md",
        "# Other\n\nWords under the defaults.\n",
    );
    std::fs::write(
        root.join(symphony::project::FILE),
        "[[entrypoints]]\nurl = \"/docs/\"\nfolder = \"docs\"\ntypes = [\"none\"]\n",
    )
    .unwrap();
    hub.tell_project();
    read(&hub);
    assert!(kept_says(
        &hub,
        "",
        "docs/OTHER.md",
        "Words under the defaults."
    ));
}

#[test]
fn a_followed_document_whose_one_card_moved_is_rendered_again_and_no_other() {
    let tree = Tree::new();
    let root = repository(&tree);
    let register = |lines: &str| {
        format!(
            "# Topic\n\n<!-- register:start -->\n- [ ] **initial-card** — Initial question.\n{lines}<!-- register:end -->\n"
        )
    };
    tree.write("main/docs/TOPIC.md", &register(""));
    git(&root, &["commit", "-qam", "Register the fixture decisions"]);
    let branch = tree.path("proposal");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-b",
            "design/policy",
            branch.to_str().unwrap(),
        ],
    );
    let hub = following(&tree, &root);
    let answer = "The branch's answer.";
    assert!(!kept_says(&hub, "", "docs/TOPIC.md", answer));

    // The branch posts a line on main's card: main's page shows the one card, with no file of
    // main's changed.
    tree.write(
        "proposal/docs/TOPIC.md",
        &register(&format!("  [2026-11-11 10:00] agent — {answer}\n")),
    );
    git(&branch, &["commit", "-qam", "Answer on the branch"]);
    let branch = branch.canonicalize().unwrap();
    hub.refresh_docs(Some(&BTreeSet::from([branch.join("docs/TOPIC.md")])))
        .unwrap();
    assert!(
        kept_says(&hub, "", "docs/TOPIC.md", answer),
        "main's copy shows the one card"
    );
    let published = last_published(&hub, "");
    assert!(published.contains(answer), "{published}");
    assert!(
        kept_says(&hub, "", "docs/OTHER.md", "The first words."),
        "a document whose cards stayed is not rendered again"
    );
}

#[test]
fn a_followed_document_whose_linking_task_moved_is_rendered_again_and_no_other() {
    let tree = Tree::new();
    let root = repository(&tree);
    let f = Fixture::new(Seed::default().task("tools/topic", "ready").link(
        "tools/topic",
        "doc",
        "docs/TOPIC.md",
    ));
    let hub = following(&tree, &root);
    observe(&hub, &f.store, &[]);
    hub.refresh_docs(Some(&BTreeSet::new())).unwrap();
    let status = |hub: &Hub| {
        hub.pages.read().unwrap()[""].documents["docs/TOPIC.md"]
            .tasks
            .iter()
            .map(|task| (task.slug.clone(), task.status.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        status(&hub),
        [("tools/topic".to_string(), "ready".to_string())],
        "the task the board read since links it"
    );

    f.store
        .raw()
        .table("task")
        .matching(serde_json::json!({"slug": "tools/topic"}))
        .set(serde_json::json!({"status": "in_progress"}))
        .unwrap();
    observe(&hub, &f.store, &[]);
    hub.refresh_docs(Some(&BTreeSet::new())).unwrap();
    assert_eq!(
        status(&hub),
        [("tools/topic".to_string(), "in_progress".to_string())]
    );
    let published = last_published(&hub, "");
    assert!(
        published.contains("\"document\"") && published.contains("in_progress"),
        "{published}"
    );
    assert!(
        kept_says(&hub, "", "docs/OTHER.md", "The first words."),
        "a document no task links is not rendered again"
    );
}
