//! A project whose entrypoint serves `spec/` at `/`: its documents are named from the root, its
//! pages list only what its patterns include and its renderers handle, its links follow the
//! mapping, and a file whose route the Symphony answers is refused by name.

use symphony::board::scan;
use symphony::docview;
use symphony::lint::project_problems;
use symphony::project::Project;
use symphony::render::Render;

use super::Tree;

const SPEC: &str = "[[entrypoints]]\nurl = \"/\"\nfolder = \"spec\"\n\
                    include = [\"**/*.md\", \"art/**\"]\nexclude = [\"drafts/**\"]\n\
                    types = [\"markdown\", \"media\"]\n";

const PNG_5X7: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x05\0\0\0\x07";

/// The project on `spec/` at `/`, with files its patterns or renderers pass over and a stray
/// `docs/` folder no entrypoint reads.
fn spec() -> Tree {
    let tree = Tree::new();
    tree.write("symphony.toml", SPEC);
    tree.write(
        "spec/DECISIONS.md",
        "# Decisions\n\n<!-- register:start -->\n### Topic\n\n- [ ] **pick** — Choose one.\n\
         <!-- register:end -->\n",
    );
    tree.write(
        "spec/index.md",
        "# Spec\n\nStart at [[START]].\n\n![ship](art/ship.png)\n",
    );
    tree.write(
        "spec/guide/START.md",
        "# Start\n\nAs **pick** says; back to [[index]] or [[DECISIONS#Topic]].\n",
    );
    tree.write(
        "spec/art/credits.txt",
        "included, but no renderer handles it",
    );
    std::fs::write(tree.path("spec/art/ship.png"), PNG_5X7).unwrap();
    tree.write("spec/guide/notes.txt", "not included");
    tree.write(
        "spec/drafts/WIP.md",
        "# Draft\n\n- [ ] **draft** — Excluded.\n",
    );
    tree.write("spec/.hidden/H.md", "# Hidden\n");
    tree.write(
        "docs/STRAY.md",
        "# Stray\n\n- [ ] **stray** — No entrypoint's.\n",
    );
    tree
}

#[test]
fn an_entrypoint_at_the_root_serves_its_folders_index_there() {
    let tree = spec();
    let project = Project::load(&tree.root).unwrap();
    assert_eq!(project.route("spec/index.md").as_deref(), Some("index.md"));
    assert_eq!(project.route("spec").as_deref(), Some(""));
    assert_eq!(project.file("index.md"), "spec/index.md");
    assert_eq!(project.file(""), "spec");
    assert_eq!(project.view().register, "DECISIONS.md");
    let render = Render::new(&tree.root);
    let doc = docview::document(&render, "index.md", "").unwrap();
    assert_eq!(
        (doc.path.as_str(), doc.title.as_str()),
        ("index.md", "Spec")
    );
    assert_eq!(
        doc.crumbs
            .iter()
            .map(|c| (c.name.as_str(), c.href.as_deref()))
            .collect::<Vec<_>>(),
        [("spec", Some("/")), ("Spec", None)]
    );
    let start = docview::document(&render, "guide/START.md", "").unwrap();
    assert_eq!(
        start
            .crumbs
            .iter()
            .map(|c| c.href.as_deref())
            .collect::<Vec<_>>(),
        [Some("/"), Some("/guide/"), None]
    );
    // what the patterns pass over, a physical name and the stray folder are no documents
    for refused in [
        "drafts/WIP.md",
        ".hidden/H.md",
        "spec/index.md",
        "docs/STRAY.md",
        "guide/notes.txt",
        "art/ship.png",
    ] {
        assert!(
            docview::document(&render, refused, "").is_err(),
            "{refused}"
        );
    }
}

#[test]
fn an_entrypoints_pages_list_only_what_its_patterns_include_and_its_renderers_handle() {
    let tree = spec();
    let render = Render::new(&tree.root);
    let entries = |rel: &str| {
        docview::folder(&render, rel, "")
            .unwrap()
            .entries
            .iter()
            .map(|e| e.path.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        entries(""),
        ["art", "guide", "DECISIONS.md", "index.md"],
        "no drafts/, .hidden/ or docs/"
    );
    assert_eq!(entries("guide"), ["guide/START.md"]);
    assert_eq!(entries("art"), ["art/ship.png"]);
    let root = render.folder("").unwrap();
    assert_eq!(
        root.entries
            .iter()
            .map(|e| (e.path.as_str(), e.docs))
            .collect::<Vec<_>>(),
        [
            ("art", 0),
            ("guide", 1),
            ("DECISIONS.md", 0),
            ("index.md", 0)
        ]
    );
    for refused in ["drafts", ".hidden", "spec", "docs"] {
        assert!(docview::folder(&render, refused, "").is_err(), "{refused}");
    }
    let docs = scan(&tree.root).unwrap();
    assert_eq!(
        docs.iter().map(|doc| doc.path.as_str()).collect::<Vec<_>>(),
        ["DECISIONS.md", "guide/START.md", "index.md"]
    );
    assert_eq!(
        docs.iter()
            .flat_map(|doc| doc.decisions.iter().map(|card| card.slug.as_str()))
            .collect::<Vec<_>>(),
        ["pick"]
    );
    assert_eq!(
        symphony::tree::index_order(&tree.root, ""),
        ["guide/START.md", "DECISIONS.md", "index.md"]
    );
    assert!(symphony::docs_audit::audit(&tree.root).errors.is_empty());
}

#[test]
fn links_cards_and_pictures_follow_the_mapping() {
    let tree = spec();
    let render = Render::new(&tree.root);
    let blocks = |rel: &str| {
        serde_json::to_string(&docview::document(&render, rel, "").unwrap().sections).unwrap()
    };
    let start = blocks("guide/START.md");
    for href in [
        "\"/DECISIONS.md#pick\"",
        "\"/index.md\"",
        "\"/DECISIONS.md#topic\"",
    ] {
        assert!(start.contains(href), "{href}: {start}");
    }
    let index = blocks("index.md");
    assert!(index.contains("\"/guide/START.md\""), "{index}");
    assert!(index.contains("\"src\":\"/art/ship.png?v="), "{index}");
    assert!(
        index.contains("\"width\":\"5\"") && index.contains("\"height\":\"7\""),
        "the picture is read from spec/: {index}"
    );
}

#[test]
fn a_file_on_a_route_the_symphony_answers_is_refused_by_name_until_it_goes() {
    let tree = spec();
    assert!(project_problems(&tree.root).is_empty());
    // an entry on a Symphony route the patterns pass over is no collision
    tree.write("spec/favicon.ico", "icon");
    tree.write("spec/tasks/plan.txt", "not included");
    assert!(Project::load(&tree.root).is_ok());
    assert_eq!(Project::of(&tree.root).entrypoints[0].folder, "spec");
    tree.write("spec/tasks/plan.md", "# Plan\n");
    let refusal = "symphony.toml: [[entrypoints]] / would serve spec/tasks/plan.md at \
                   /tasks/plan.md, where a route the Symphony answers; move or exclude it";
    assert_eq!(Project::load(&tree.root).unwrap_err(), refusal);
    assert_eq!(project_problems(&tree.root), [refusal]);
    assert_eq!(
        *Project::of(&tree.root),
        Project::plain(&tree.root),
        "a refused file reads as the plain project"
    );
    std::fs::remove_file(tree.path("spec/tasks/plan.md")).unwrap();
    assert_eq!(Project::of(&tree.root).entrypoints[0].folder, "spec");
    // excluding it is the other way out
    tree.write("spec/api/v1.md", "# V1\n");
    assert!(
        Project::load(&tree.root)
            .unwrap_err()
            .contains("would serve spec/api/v1.md at /api/v1.md")
    );
    tree.write(
        "symphony.toml",
        &SPEC.replace("[\"drafts/**\"]", "[\"drafts/**\", \"api/**\"]"),
    );
    assert!(Project::load(&tree.root).is_ok());
}

#[test]
fn a_file_under_another_entrypoints_url_is_refused_and_its_own_folder_is_not() {
    let tree = spec();
    let notes = "[[entrypoints]]\nurl = \"/notes/\"\nfolder = \"notes\"\n";
    tree.write("notes/A.md", "# A\n");
    tree.write("symphony.toml", &format!("{SPEC}{notes}"));
    let project = Project::load(&tree.root).unwrap();
    assert_eq!(project.route("notes/A.md").as_deref(), Some("notes/A.md"));
    assert_eq!(project.file("notes/A.md"), "notes/A.md");
    tree.write("spec/notes/B.md", "# B\n");
    assert_eq!(
        Project::load(&tree.root).unwrap_err(),
        "symphony.toml: [[entrypoints]] / would serve spec/notes/B.md at /notes/B.md, where the \
         entrypoint over notes answers; move or exclude it"
    );
    // a folder inside the root's that another entrypoint serves at its own name is that one's
    tree.write(
        "symphony.toml",
        &format!("{SPEC}[[entrypoints]]\nurl = \"/notes/\"\nfolder = \"spec/notes\"\n"),
    );
    let project = Project::load(&tree.root).unwrap();
    assert_eq!(
        project.route("spec/notes/B.md").as_deref(),
        Some("notes/B.md")
    );
    let docs = scan(&tree.root).unwrap();
    assert_eq!(
        docs.iter().map(|doc| doc.path.as_str()).collect::<Vec<_>>(),
        ["DECISIONS.md", "guide/START.md", "index.md", "notes/B.md"],
        "each file once, by the entrypoint holding it"
    );
}

#[test]
fn the_project_root_at_the_root_with_a_type_no_renderer_handles_is_refused_by_name() {
    let tree = Tree::new();
    tree.write("README.md", "# Orbit\n");
    tree.write("src/main.rs", "fn main() {}\n");
    tree.write(
        "symphony.toml",
        "[[entrypoints]]\nurl = \"/\"\nfolder = \".\"\ntypes = [\"markdown\", \"rust\"]\n",
    );
    let refusal = "symphony.toml: [[entrypoints]] / type \"rust\": no renderer handles it (the \
                   renderers are markdown, media)";
    assert_eq!(Project::load(&tree.root).unwrap_err(), refusal);
    assert_eq!(project_problems(&tree.root), [refusal]);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(symphony::lint::main(&tree.root, &mut out, &mut err), 1);
    assert!(String::from_utf8(err).unwrap().contains(refusal));
    // with the types it has renderers for, the root is served, its sources passed over
    tree.write(
        "symphony.toml",
        "[[entrypoints]]\nurl = \"/\"\nfolder = \".\"\n",
    );
    let project = Project::load(&tree.root).unwrap();
    assert_eq!(project.route("README.md").as_deref(), Some("README.md"));
    assert_eq!(project.register, "DECISIONS.md");
    assert_eq!(
        project
            .shown(&tree.root)
            .iter()
            .map(|(_, file)| file.as_str())
            .collect::<Vec<_>>(),
        ["README.md"]
    );
}
