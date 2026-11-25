use super::{BUILDS, KEEP, publish_keeping};
use crate::test_tree::Tree;
use serde_json::{Value, json};

/// The files of the build `id`: its shell, a module of its own and the style sheet every build
/// shares, whose bytes are the same in each.
fn files(id: &str) -> Vec<String> {
    vec![
        format!("symphony-shell-{id}.js"),
        format!("board-{id}.js"),
        "shared.css".into(),
    ]
}

fn build(id: &str) -> Value {
    json!({"shell": format!("symphony-shell-{id}.js"), "files": files(id)})
}

/// A client folder at `rel` holding the builds `ids`, newest first, its page loading the first;
/// named in its `builds.json` when `listed`.
fn folder(tree: &Tree, rel: &str, ids: &[&str], listed: bool) {
    tree.write(
        &format!("{rel}/index.html"),
        &format!(
            "<script type=\"module\" src=\"/assets/symphony-shell-{}.js\"></script>",
            ids[0]
        ),
    );
    for id in ids {
        for name in files(id) {
            tree.write(&format!("{rel}/assets/{name}"), &format!("bytes of {name}"));
        }
    }
    if listed {
        let builds: Vec<Value> = ids.iter().map(|id| build(id)).collect();
        tree.write(
            &format!("{rel}/{BUILDS}"),
            &json!({ "builds": builds }).to_string(),
        );
    }
}

/// The tree's `stage` published over its `public` as `symphony-build publish-client` publishes it,
/// or why it was refused.
fn publish(tree: &Tree) -> Result<(), String> {
    publish_keeping(&tree.path("stage"), &tree.path("public")).map_err(|error| error.to_string())
}

/// The shells `rel`'s `builds.json` names, newest first, and the bound it names.
fn listed(tree: &Tree, rel: &str) -> (Vec<String>, u64) {
    let builds: Value = serde_json::from_str(&tree.read(&format!("{rel}/{BUILDS}"))).unwrap();
    let shells = builds["builds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|build| build["shell"].as_str().unwrap().to_owned())
        .collect();
    (shells, builds["keep"].as_u64().unwrap_or(0))
}

fn shells(ids: &[&str]) -> Vec<String> {
    ids.iter()
        .map(|id| format!("symphony-shell-{id}.js"))
        .collect()
}

/// Every file of the builds `ids`, sorted, as a folder's names are.
fn names(ids: &[&str]) -> Vec<String> {
    let mut names: Vec<String> = ids.iter().flat_map(|id| files(id)).collect();
    symphony_board::order::sort_by(&mut names, &Ord::cmp);
    names.dedup();
    names
}

#[test]
fn a_publication_keeps_its_build_and_the_two_published_before_it() {
    assert_eq!(KEEP, 3);
    let tree = Tree::new();
    folder(&tree, "public", &["c", "b", "a"], true);
    // The stage holds its build, a copy of the oldest build's files and a file no build names.
    folder(&tree, "stage", &["d"], true);
    for name in files("a") {
        tree.write(&format!("stage/assets/{name}"), &format!("bytes of {name}"));
    }
    tree.write(
        "stage/assets/stray-0123456789abcdef.js",
        "named by no build",
    );
    publish(&tree).unwrap();
    assert_eq!(listed(&tree, "public"), (shells(&["d", "c", "b"]), 3));
    // The builds kept are whole, the ones the stage lacked copied from the folder published.
    assert_eq!(tree.names("public/assets"), names(&["d", "c", "b"]));
    for name in names(&["d", "c", "b"]) {
        assert_eq!(
            tree.read(&format!("public/assets/{name}")),
            format!("bytes of {name}")
        );
    }
    assert!(
        tree.read("public/index.html")
            .contains("symphony-shell-d.js")
    );
    // The folder published before is exchanged out whole, every build it held kept for a restore.
    assert_eq!(listed(&tree, "stage"), (shells(&["c", "b", "a"]), 0));
    assert_eq!(tree.names("stage/assets"), names(&["c", "b", "a"]));
    // The next publication keeps the three newest again and drops the oldest of them.
    std::fs::remove_dir_all(tree.path("stage")).unwrap();
    folder(&tree, "stage", &["e"], true);
    publish(&tree).unwrap();
    assert_eq!(listed(&tree, "public"), (shells(&["e", "d", "c"]), 3));
    assert_eq!(tree.names("public/assets"), names(&["e", "d", "c"]));
}

#[test]
fn publishing_the_newest_build_again_keeps_the_same_builds() {
    let tree = Tree::new();
    folder(&tree, "public", &["c", "b", "a"], true);
    folder(&tree, "stage", &["c"], true);
    publish(&tree).unwrap();
    assert_eq!(listed(&tree, "public"), (shells(&["c", "b", "a"]), 3));
    assert_eq!(tree.names("public/assets"), names(&["c", "b", "a"]));
}

#[test]
fn a_first_publication_or_one_over_a_folder_naming_no_builds_keeps_its_build_alone() {
    let tree = Tree::new();
    folder(&tree, "stage", &["b"], true);
    tree.write(
        "stage/assets/stray-0123456789abcdef.js",
        "named by no build",
    );
    publish(&tree).unwrap();
    assert!(!tree.path("stage").exists());
    assert_eq!(listed(&tree, "public"), (shells(&["b"]), 3));
    assert_eq!(tree.names("public/assets"), names(&["b"]));
    // A folder published with no `builds.json` names no build to keep: its files all go.
    std::fs::remove_dir_all(tree.path("public")).unwrap();
    folder(&tree, "public", &["b", "a"], false);
    folder(&tree, "stage", &["c"], true);
    publish(&tree).unwrap();
    assert_eq!(listed(&tree, "public"), (shells(&["c"]), 3));
    assert_eq!(tree.names("public/assets"), names(&["c"]));
}

/// A way a publication cannot go through: what it is, how it spoils the folders and what the
/// refusal ends with.
type Refusal = (&'static str, fn(&Tree), String);

#[test]
fn a_stage_or_a_build_that_cannot_be_kept_whole_refuses_and_leaves_the_published_folder() {
    let cases: [Refusal; 9] = [
        (
            "a stage naming no build",
            |tree| std::fs::remove_file(tree.path("stage/builds.json")).unwrap(),
            "builds.json names no build".into(),
        ),
        (
            "a page that loads another build",
            |tree| {
                tree.write(
                    "stage/index.html",
                    "<script src=\"/assets/symphony-shell-z.js\">",
                );
            },
            "index.html does not load symphony-shell-d.js, the build builds.json names".into(),
        ),
        (
            "a file outside assets/",
            |tree| {
                tree.write(
                    "stage/builds.json",
                    &json!({"builds": [{"shell": "symphony-shell-d.js",
                        "files": ["symphony-shell-d.js", "../index.html"]}]})
                    .to_string(),
                );
            },
            "the build symphony-shell-d.js names \"../index.html\", not a file of assets/".into(),
        ),
        (
            "a file in a folder of assets/",
            |tree| {
                tree.write(
                    "stage/builds.json",
                    &json!({"builds": [{"shell": "symphony-shell-d.js",
                        "files": ["symphony-shell-d.js", "chunks/old.js"]}]})
                    .to_string(),
                );
            },
            "the build symphony-shell-d.js names \"chunks/old.js\", not a file of assets/".into(),
        ),
        (
            "a build not listing its shell",
            |tree| {
                tree.write(
                    "stage/builds.json",
                    &json!({"builds": [{"shell": "symphony-shell-d.js", "files": ["board-d.js"]}]})
                        .to_string(),
                );
            },
            "the build symphony-shell-d.js does not list its shell".into(),
        ),
        (
            "the stage's own build missing a file",
            |tree| std::fs::remove_file(tree.path("stage/assets/board-d.js")).unwrap(),
            "the build symphony-shell-d.js lacks assets/board-d.js".into(),
        ),
        (
            "a kept build missing a file in both folders",
            |tree| std::fs::remove_file(tree.path("public/assets/board-b.js")).unwrap(),
            "the build symphony-shell-b.js lacks assets/board-b.js".into(),
        ),
        (
            "one name with two contents",
            |tree| {
                tree.write("stage/assets/shared.css", "other bytes under the same name");
            },
            "immutable asset collision: assets/shared.css differs from the one published".into(),
        ),
        (
            "a folder among the assets",
            |tree| std::fs::create_dir(tree.path("stage/assets/chunks")).unwrap(),
            "client assets must be regular files".into(),
        ),
    ];
    for (case, spoil, said) in cases {
        let tree = Tree::new();
        folder(&tree, "public", &["c", "b", "a"], true);
        folder(&tree, "stage", &["d"], true);
        spoil(&tree);
        let before = tree.names("public/assets");
        let err = publish(&tree).unwrap_err();
        assert!(err.ends_with(&said), "{case}: {err}");
        assert_eq!(
            listed(&tree, "public"),
            (shells(&["c", "b", "a"]), 0),
            "{case}"
        );
        assert_eq!(tree.names("public/assets"), before, "{case}");
    }
}

#[test]
fn a_link_among_the_stage_assets_refuses() {
    let tree = Tree::new();
    folder(&tree, "stage", &["d"], true);
    std::os::unix::fs::symlink(tree.path("outside.js"), tree.path("stage/assets/link.js")).unwrap();
    let err = publish(&tree).unwrap_err();
    assert!(
        err.ends_with("link.js: client assets must be regular files"),
        "{err}"
    );
    assert!(!tree.path("public").exists());
}
