//! The registry: an entry read from a project's file; a project added again, moved or refused;
//! and the file written whole under its lock.

use super::*;
use crate::test_tree::Tree;

fn project(tree: &Tree, folder: &str, manifest: Option<&str>) -> PathBuf {
    tree.write(&format!("{folder}/docs/README.md"), "# Docs\n");
    if let Some(manifest) = manifest {
        tree.write(&format!("{folder}/{FILE}"), manifest);
    }
    tree.path(folder).canonicalize().unwrap()
}

#[test]
fn an_entry_reads_the_project_file_and_a_root_without_one_is_a_plain_project() {
    let tree = Tree::new();
    let named = project(
        &tree,
        "named",
        Some("[project]\nname = \"Named One\"\nid = \"named-one\"\n[plugins]\ncrates = [\"p\"]\n"),
    );
    let entry = Entry::of(&named).unwrap();
    assert_eq!(entry.id, "named-one");
    assert_eq!(entry.name, "Named One");
    assert_eq!(entry.plugins, ["p"]);
    assert_eq!(entry.manifest, Some(named.join(FILE)));
    assert_eq!((entry.pin, entry.schema), (PIN, symphony_board::SCHEMA));
    assert!(entry.holds());
    let plain = project(&tree, "Plain Folder", None);
    let entry = Entry::of(&plain).unwrap();
    assert_eq!(
        (entry.id.as_str(), entry.name.as_str()),
        ("plain-folder", "Plain Folder")
    );
    assert_eq!(entry.manifest, None);
    assert!(entry.plugins.is_empty());
    let broken = project(&tree, "broken", Some("[project]\nnope = 1\n"));
    let why = Entry::of(&broken).unwrap_err();
    assert!(why.contains("nope"), "{why}");
    assert!(Entry::of(&tree.path("missing")).is_err());
    let file = tree.write("a-file", "");
    assert!(Entry::of(&file).unwrap_err().contains("not a folder"));
}

#[test]
fn adding_a_project_twice_changes_nothing_and_a_changed_file_updates_it_in_place() {
    let tree = Tree::new();
    let one = project(&tree, "one", Some("[project]\nid = \"one\"\n"));
    let two = project(&tree, "two", Some("[project]\nid = \"two\"\n"));
    let mut registry = Registry::default();
    assert_eq!(registry.add(Entry::of(&one).unwrap()), Ok(Added::New));
    assert_eq!(registry.add(Entry::of(&two).unwrap()), Ok(Added::New));
    assert_eq!(registry.add(Entry::of(&one).unwrap()), Ok(Added::Same));
    tree.write(
        "one/symphony.toml",
        "[project]\nid = \"one\"\n[plugins]\ncrates = [\"p\"]\n",
    );
    assert_eq!(registry.add(Entry::of(&one).unwrap()), Ok(Added::Updated));
    let ids: Vec<&str> = registry.projects.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["one", "two"], "an update keeps the project's place");
    assert_eq!(registry.get("one").unwrap().plugins, ["p"]);
    // A new id for the same root replaces its entry; the old id is gone.
    tree.write("one/symphony.toml", "[project]\nid = \"uno\"\n");
    assert_eq!(registry.add(Entry::of(&one).unwrap()), Ok(Added::Updated));
    assert!(registry.get("one").is_none());
    assert_eq!(registry.get("uno").unwrap().root, one);
    assert_eq!(registry.projects.len(), 2);
}

#[test]
fn a_moved_root_takes_its_id_along_and_a_root_still_holding_it_keeps_it() {
    let tree = Tree::new();
    let old = project(&tree, "old", Some("[project]\nid = \"game\"\n"));
    let mut registry = Registry::default();
    registry.add(Entry::of(&old).unwrap()).unwrap();
    let copy = project(&tree, "copy", Some("[project]\nid = \"game\"\n"));
    let why = registry.add(Entry::of(&copy).unwrap()).unwrap_err();
    assert!(
        why.contains("already") && why.contains("another [project] id"),
        "{why}"
    );
    assert_eq!(registry.get("game").unwrap().root, old);
    std::fs::remove_dir_all(&old).unwrap();
    assert_eq!(
        registry.add(Entry::of(&copy).unwrap()),
        Ok(Added::Moved(old.clone()))
    );
    assert_eq!(registry.get("game").unwrap().root, copy);
    assert_eq!(registry.projects.len(), 1);
    // A root that now names another id has left the old one too.
    let third = project(&tree, "third", Some("[project]\nid = \"game\"\n"));
    tree.write("copy/symphony.toml", "[project]\nid = \"renamed\"\n");
    assert_eq!(
        registry.add(Entry::of(&third).unwrap()),
        Ok(Added::Moved(copy))
    );
}

#[test]
fn an_id_on_a_route_the_server_answers_before_any_project_is_refused() {
    let tree = Tree::new();
    let mut registry = Registry::default();
    for (folder, id) in [("routes", "tasks"), ("uploads", "artifacts")] {
        let root = project(&tree, folder, Some(&format!("[project]\nid = \"{id}\"\n")));
        let why = registry.add(Entry::of(&root).unwrap()).unwrap_err();
        assert!(why.contains("a route the Symphony answers"), "{why}");
    }
    let game = project(&tree, "game", Some("[project]\nid = \"game\"\n"));
    registry.add(Entry::of(&game).unwrap()).unwrap();
    // Today's /docs/ links of the game redirect to it: no project may take /docs/.
    let docs = project(&tree, "docs-project", Some("[project]\nid = \"docs\"\n"));
    let why = registry.add(Entry::of(&docs).unwrap()).unwrap_err();
    assert!(why.contains("pages answer at the server's root"), "{why}");
    // An entrypoint at `/` answers the names its folder holds.
    let site = project(
        &tree,
        "site",
        Some("[project]\nid = \"site\"\n[[entrypoints]]\nurl = \"/\"\nfolder = \"pages\"\n"),
    );
    tree.write("site/pages/guide/index.md", "# Guide\n");
    registry.add(Entry::of(&site).unwrap()).unwrap();
    let guide = project(&tree, "guide", Some("[project]\nid = \"guide\"\n"));
    let why = registry.add(Entry::of(&guide).unwrap()).unwrap_err();
    assert!(why.contains("site's pages answer"), "{why}");
    assert_eq!(registry.projects.len(), 2);
}

#[test]
fn the_registry_is_written_whole_and_one_that_does_not_read_is_never_written_over() {
    let tree = Tree::new();
    let dir = tree.path("machine");
    assert_eq!(Registry::read(&dir), Ok(Registry::default()));
    let one = project(&tree, "one", None);
    let added = Registry::change(&dir, |registry| registry.add(Entry::of(&one).unwrap()));
    assert_eq!(added, Ok(Added::New));
    let read = Registry::read(&dir).unwrap();
    assert_eq!(read.projects.len(), 1);
    assert_eq!(
        tree.names("machine"),
        [PROJECTS, LOCK],
        "no temporary file is left"
    );
    let failed: Result<(), String> = Registry::change(&dir, |registry| {
        registry.projects.clear();
        Err("refused".into())
    });
    assert_eq!(failed, Err("refused".into()));
    assert_eq!(
        Registry::read(&dir).unwrap(),
        read,
        "a failed change writes nothing"
    );
    tree.write("machine/projects.json", "{not json");
    let why = Registry::change(&dir, |_| Ok(())).unwrap_err();
    assert!(why.contains(PROJECTS), "{why}");
    assert_eq!(tree.read("machine/projects.json"), "{not json");
    // A registry from before a field reads that field's default.
    tree.write(
        "machine/projects.json",
        &format!(
            r#"{{"projects":[{{"id":"one","name":"one","root":{:?}}}]}}"#,
            one
        ),
    );
    let old = Registry::read(&dir).unwrap();
    assert_eq!(
        (old.projects[0].pin, old.projects[0].manifest.clone()),
        (0, None)
    );
}

#[test]
fn a_malformed_registry_is_refused_in_the_words_and_at_the_place_serde_json_from_str_gives() {
    // The registry is read through the crate's one JSON reader, of bytes (`from_slice`): for any
    // text, the same error, words and line and column, as `from_str` of that text.
    let tree = Tree::new();
    let dir = tree.path("machine");
    let path = dir.join(PROJECTS);
    let malformed = [
        "",
        "  \n\t",
        "{\"projects\": [",
        "{\"projects\": 3}",
        "{\"projects\": []} and more",
        "{\"projects\": [{\"id\": \"a\\q\"}]}",
        "{\n  \"projects\": [\n    {\"id\": 1}\n  ]\n}",
        "{\"projects\": [], \"projects\": []}",
        "{\"projects\": [{}]}",
        "{\"projects\": [{\"id\": \"\\ud800\"}]}",
        "{\"projects\": 1e400}",
        "\u{feff}{\"projects\": []}",
        "{\"projects\": [{\"id\": \"a\", \"name\": \"b\", \"root\": \"c\", \"pin\": -1}]}",
    ];
    for text in malformed {
        tree.write("machine/projects.json", text);
        let before = serde_json::from_str::<Registry>(text).unwrap_err();
        assert_eq!(
            Registry::read(&dir),
            Err(format!("{}: {before}", path.display())),
            "{text:?}"
        );
    }
    let deep = serde_json::from_str::<Registry>(malformed[6]).unwrap_err();
    assert_eq!(deep.line(), 3, "one error is past the first line");
}
