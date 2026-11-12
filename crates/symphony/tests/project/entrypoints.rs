//! A project whose entrypoint serves `spec/` at `/`: its documents are named from the root, its
//! pages list only what its patterns include and its renderers handle, its links follow the
//! mapping, and a file whose route the Symphony answers is refused by name, however deep and
//! however late it appears. Nothing a link leads out of the folder to, or an excluded folder
//! holds, is read.

use symphony::board::scan;
use symphony::docview;
use symphony::lint::project_problems;
use symphony::project::Project;
use symphony::render::Render;

use super::Tree;

const SPEC: &str = "[[entrypoints]]\nurl = \"/\"\nfolder = \"spec\"\n\
                    include = [\"**/*.md\", \"pictures/**\"]\nexclude = [\"drafts/**\"]\n\
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
        "# Spec\n\nStart at [[START]].\n\n![lamp](pictures/lamp.png)\n",
    );
    tree.write(
        "spec/guide/START.md",
        "# Start\n\nAs **pick** says; back to [[index]] or [[DECISIONS#Topic]].\n",
    );
    tree.write(
        "spec/pictures/credits.txt",
        "included, but no renderer handles it",
    );
    std::fs::write(tree.path("spec/pictures/lamp.png"), PNG_5X7).unwrap();
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
    let doc = docview::document(&render, "index.md", "", &[]).unwrap();
    assert_eq!(
        (doc.path.as_str(), doc.title.as_str()),
        ("index.md", "Spec")
    );
    assert_eq!(
        doc.crumbs
            .iter()
            .map(|c| (c.name.as_str(), c.href.as_deref()))
            .collect::<Vec<_>>(),
        // the entrypoint's folder, with no README, by its name as a title
        [("Spec", Some("/")), ("Spec", None)]
    );
    let start = docview::document(&render, "guide/START.md", "", &[]).unwrap();
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
        "pictures/lamp.png",
    ] {
        assert!(
            docview::document(&render, refused, "", &[]).is_err(),
            "{refused}"
        );
    }
}

#[test]
fn an_entrypoints_pages_list_only_what_its_patterns_include_and_its_renderers_handle() {
    let tree = spec();
    let render = Render::new(&tree.root);
    let entries = |rel: &str| {
        docview::folder(&render, rel, "", &[])
            .unwrap()
            .entries
            .iter()
            .map(|e| e.path.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        entries(""),
        ["guide", "pictures", "DECISIONS.md", "index.md"],
        "no drafts/, .hidden/ or docs/"
    );
    assert_eq!(entries("guide"), ["guide/START.md"]);
    assert_eq!(entries("pictures"), ["pictures/lamp.png"]);
    let root = render.folder("").unwrap();
    assert_eq!(
        root.entries
            .iter()
            .map(|e| (e.path.as_str(), e.docs))
            .collect::<Vec<_>>(),
        [
            ("guide", 1),
            ("pictures", 0),
            ("DECISIONS.md", 0),
            ("index.md", 0)
        ]
    );
    for refused in ["drafts", ".hidden", "spec", "docs"] {
        assert!(
            docview::folder(&render, refused, "", &[]).is_err(),
            "{refused}"
        );
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
        serde_json::to_string(&docview::document(&render, rel, "", &[]).unwrap().sections).unwrap()
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
    assert!(index.contains("\"src\":\"/pictures/lamp.png?v="), "{index}");
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
    let project = Project::load(&tree.root).unwrap();
    // the route is the Symphony's: no entrypoint maps it to a file of its folder
    assert!(project.serving("tasks/plan.txt").is_none());
    assert_eq!(project.file("tasks/plan.txt"), "tasks/plan.txt");
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

#[test]
fn a_folder_linked_out_of_the_project_is_refused_and_a_linked_file_opens_where_it_is_listed() {
    let outside = Tree::new();
    outside.write(
        "SECRET.md",
        "# Secret\n\n- [ ] **secret** — Not the project's.\n",
    );
    let tree = Tree::new();
    tree.write("symphony.toml", SPEC);
    std::os::unix::fs::symlink(&outside.root, tree.path("spec")).unwrap();
    let refusal = "symphony.toml: [[entrypoints]] / folder spec lies outside the project once its \
                   links are resolved";
    assert_eq!(Project::load(&tree.root).unwrap_err(), refusal);
    assert_eq!(project_problems(&tree.root), [refusal]);
    assert_eq!(*Project::of(&tree.root), Project::plain(&tree.root));
    // in the folder, a link to a folder (even one inside it), out of the project, into a dot folder
    // or to an upload is passed over; one to a file elsewhere in the project is listed, and opens
    let tree = spec();
    tree.write("OTHER.md", "# Other\n");
    tree.write(".private/KEY.md", "# Key\n");
    tree.write("artifacts/uploads/0123abcd.md", "# Upload\n");
    let link = |target: std::path::PathBuf, name: &str| {
        std::os::unix::fs::symlink(target, tree.path(name)).unwrap();
    };
    link(outside.path("SECRET.md"), "spec/guide/LEAK.md");
    link(outside.root.clone(), "spec/linked");
    link(tree.path("spec/guide"), "spec/alias");
    link(tree.path("spec/index.md"), "spec/guide/ALIAS.md");
    link(tree.path("OTHER.md"), "spec/guide/OTHER.md");
    link(tree.path(".private/KEY.md"), "spec/guide/KEY.md");
    link(
        tree.path("artifacts/uploads/0123abcd.md"),
        "spec/guide/UPLOAD.md",
    );
    assert!(Project::load(&tree.root).is_ok());
    let docs = scan(&tree.root).unwrap();
    assert_eq!(
        docs.iter().map(|doc| doc.path.as_str()).collect::<Vec<_>>(),
        [
            "DECISIONS.md",
            "guide/ALIAS.md",
            "guide/OTHER.md",
            "guide/START.md",
            "index.md"
        ]
    );
    let render = Render::new(&tree.root);
    let listed: Vec<String> = docview::folder(&render, "guide", "", &[])
        .unwrap()
        .entries
        .iter()
        .map(|e| e.path.clone())
        .collect();
    assert_eq!(
        listed,
        ["guide/OTHER.md", "guide/ALIAS.md", "guide/START.md"],
        "by title: Other, Spec (the alias), Start"
    );
    for opens in &listed {
        assert!(
            docview::document(&render, opens, "", &[]).is_ok(),
            "{opens}"
        );
    }
    for refused in [
        "guide/LEAK.md",
        "guide/KEY.md",
        "guide/UPLOAD.md",
        "linked/SECRET.md",
        "alias/START.md",
    ] {
        assert!(
            docview::document(&render, refused, "", &[]).is_err(),
            "{refused}"
        );
        assert!(
            docview::target(&tree.root, refused, true).is_err(),
            "nor saved: {refused}"
        );
    }
    for refused in ["linked", "alias"] {
        assert!(
            docview::folder(&render, refused, "", &[]).is_err(),
            "{refused}"
        );
    }
}

#[test]
fn a_colliding_file_appearing_deep_in_a_folder_already_read_is_refused_at_once() {
    let tree = spec();
    tree.write("spec/tasks/deep/notes.txt", "not included");
    assert_eq!(Project::of(&tree.root).entrypoints[0].folder, "spec");
    tree.write("spec/tasks/deep/LEAK.md", "# Leak\n");
    assert_eq!(
        *Project::of(&tree.root),
        Project::plain(&tree.root),
        "the cached project is read again"
    );
    assert!(
        project_problems(&tree.root)[0]
            .contains("would serve spec/tasks/deep/LEAK.md at /tasks/deep/LEAK.md")
    );
    std::fs::remove_file(tree.path("spec/tasks/deep/LEAK.md")).unwrap();
    assert_eq!(Project::of(&tree.root).entrypoints[0].folder, "spec");
    // a new folder under one already read holds the file
    tree.write("spec/tasks/deep/deeper/LEAK.md", "# Leak\n");
    assert_eq!(*Project::of(&tree.root), Project::plain(&tree.root));
}

#[test]
fn an_excluded_folder_takes_everything_under_it_from_every_reader() {
    let tree = Tree::new();
    tree.write(
        "symphony.toml",
        "[[entrypoints]]\nurl = \"/\"\nfolder = \"spec\"\nexclude = [\"drafts\"]\n",
    );
    tree.write("spec/index.md", "# Spec\n");
    tree.write(
        "spec/drafts/PRIVATE.md",
        "# Private\n\n- [ ] **private** — Excluded.\n",
    );
    tree.write("spec/drafts/deep/D.md", "# Deep\n");
    tree.write("spec/draftsx/K.md", "# Kept\n");
    let project = Project::load(&tree.root).unwrap();
    assert!(project.is_page("spec/index.md") && project.is_page("spec/draftsx/K.md"));
    let render = Render::new(&tree.root);
    for refused in ["drafts/PRIVATE.md", "drafts/deep/D.md"] {
        assert!(!project.is_page(&project.file(refused)), "{refused}");
        assert!(
            docview::document(&render, refused, "", &[]).is_err(),
            "{refused}"
        );
        assert!(
            docview::target(&tree.root, refused, true).is_err(),
            "the save target too: {refused}"
        );
    }
    assert!(docview::folder(&render, "drafts/deep", "", &[]).is_err());
    assert_eq!(
        scan(&tree.root)
            .unwrap()
            .iter()
            .map(|doc| doc.path.as_str())
            .collect::<Vec<_>>(),
        ["draftsx/K.md", "index.md"]
    );
}

#[test]
fn uploads_are_the_symphonys_store_never_a_collision_nor_documents() {
    let tree = Tree::new();
    let root_types =
        "[[entrypoints]]\nurl = \"/\"\nfolder = \".\"\ntypes = [\"markdown\", \"media\"]\n";
    tree.write("symphony.toml", root_types);
    tree.write("README.md", "# Orbit\n");
    tree.write("artifacts/notes.md", "# Notes\n");
    let register =
        "# D\n\n<!-- register:start -->\n### T\n\n- [x] **done** — plain.\n<!-- register:end -->\n";
    tree.write("artifacts/uploads/5ca1ab1e.md", register);
    std::fs::write(tree.path("artifacts/uploads/0123abcd.png"), PNG_5X7).unwrap();
    // a link to an upload, or into a dot folder, is no way in either
    tree.write(".private/KEY.md", "# Key\n");
    for (target, name) in [
        (".private/KEY.md", "KEY.md"),
        ("artifacts/uploads/5ca1ab1e.md", "PASTED.md"),
    ] {
        std::os::unix::fs::symlink(tree.path(target), tree.path(name)).unwrap();
    }
    let project = Project::load(&tree.root).unwrap();
    assert!(project_problems(&tree.root).is_empty());
    assert_eq!(
        project.file("artifacts/uploads/0123abcd.png"),
        "artifacts/uploads/0123abcd.png"
    );
    // no walk reads an upload: it is no document, card, register or entry of a folder page
    assert!(!project.is_page("artifacts/uploads/5ca1ab1e.md"));
    assert_eq!(
        scan(&tree.root)
            .unwrap()
            .iter()
            .map(|doc| doc.path.as_str())
            .collect::<Vec<_>>(),
        ["README.md", "artifacts/notes.md"]
    );
    assert!(symphony::board::register_files(&tree.root).is_empty());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(
        symphony::board::tidy(&tree.root, Some("2026-09-26 08:00"), &mut out, &mut err),
        0,
        "{}",
        String::from_utf8_lossy(&err)
    );
    assert_eq!(tree.read("artifacts/uploads/5ca1ab1e.md"), register);
    let render = Render::new(&tree.root);
    assert_eq!(
        docview::folder(&render, "artifacts", "", &[])
            .unwrap()
            .entries
            .iter()
            .map(|e| e.path.as_str())
            .collect::<Vec<_>>(),
        ["artifacts/notes.md"]
    );
    assert!(docview::folder(&render, "artifacts/uploads", "", &[]).is_err());
    assert!(docview::document(&render, "artifacts/uploads/5ca1ab1e.md", "", &[]).is_err());
    for refused in ["KEY.md", "PASTED.md"] {
        assert!(
            docview::document(&render, refused, "", &[]).is_err(),
            "{refused}"
        );
        assert!(
            docview::target(&tree.root, refused, true).is_err(),
            "nor saved: {refused}"
        );
    }
    // nor does an entrypoint read the uploads folder
    tree.write(
        "symphony.toml",
        &format!(
            "{root_types}[[entrypoints]]\nurl = \"/pasted/\"\nfolder = \"artifacts/uploads\"\n"
        ),
    );
    assert_eq!(
        Project::load(&tree.root).unwrap_err(),
        "symphony.toml: [[entrypoints]] /pasted/ reads artifacts/uploads, the Symphony's uploads, \
         which no entrypoint shows"
    );
    // elsewhere than the project's uploads folder, a file on that route would be hidden
    tree.write(
        "symphony.toml",
        &format!("[project]\nartifacts = \"out\"\n{root_types}"),
    );
    assert!(
        Project::load(&tree.root).unwrap_err().contains(
            "would serve artifacts/uploads/0123abcd.png at /artifacts/uploads/0123abcd.png"
        )
    );
    let tree = spec();
    tree.write("spec/artifacts/uploads/notes.md", "# Not the uploads\n");
    assert!(
        Project::load(&tree.root)
            .unwrap_err()
            .contains("would serve spec/artifacts/uploads/notes.md at /artifacts/uploads/notes.md")
    );
}

#[test]
fn an_entrypoint_folder_linked_into_the_uploads_is_refused_and_one_above_them_never_enters() {
    let tree = Tree::new();
    tree.write("symphony.toml", SPEC);
    tree.write("artifacts/uploads/0123abcd.md", "# Upload\n");
    tree.write("artifacts/index.md", "# Spec\n");
    std::os::unix::fs::symlink(tree.path("artifacts/uploads"), tree.path("spec")).unwrap();
    let refusal = "symphony.toml: [[entrypoints]] / folder spec lies in artifacts/uploads once its \
                   links are resolved: the Symphony's uploads, which no entrypoint shows";
    assert_eq!(Project::load(&tree.root).unwrap_err(), refusal);
    assert_eq!(project_problems(&tree.root), [refusal]);
    assert_eq!(*Project::of(&tree.root), Project::plain(&tree.root));
    assert!(docview::target(&tree.root, "0123abcd.md", true).is_err());
    // through a folder above the uploads, the walk never enters them, nor does anything open there
    std::fs::remove_file(tree.path("spec")).unwrap();
    std::os::unix::fs::symlink(tree.path("artifacts"), tree.path("spec")).unwrap();
    assert!(Project::load(&tree.root).is_ok());
    assert_eq!(Project::of(&tree.root).entrypoints[0].folder, "spec");
    let render = Render::new(&tree.root);
    assert_eq!(
        docview::folder(&render, "", "", &[])
            .unwrap()
            .entries
            .iter()
            .map(|e| e.path.as_str())
            .collect::<Vec<_>>(),
        ["index.md"]
    );
    assert_eq!(
        scan(&tree.root)
            .unwrap()
            .iter()
            .map(|doc| doc.path.as_str())
            .collect::<Vec<_>>(),
        ["index.md"]
    );
    assert!(docview::folder(&render, "uploads", "", &[]).is_err());
    assert!(docview::target(&tree.root, "uploads/0123abcd.md", true).is_err());
}

#[test]
fn a_dotted_or_markdown_named_entrypoint_and_folder_are_folder_pages_on_both_sides() {
    for (url, folder) in [("v1.0", "guide.v2"), ("guide.md", "old.md")] {
        let tree = Tree::new();
        tree.write(
            "symphony.toml",
            &format!("[[entrypoints]]\nurl = \"/{url}/\"\nfolder = \"spec\"\n"),
        );
        tree.write(&format!("spec/{folder}/A.md"), "# A\n");
        let view = Project::load(&tree.root).unwrap().view();
        let render = Render::new(&tree.root);
        let inner = format!("{url}/{folder}");
        // the client asks for the root by its URL and for a folder by its slash
        for (route, page) in [
            (url.to_string(), url.to_string()),
            (inner.clone(), format!("{inner}/")),
        ] {
            assert!(docview::folder(&render, &route, "", &[]).is_ok(), "{route}");
            assert!(
                docview::document(&render, &route, "", &[]).is_err(),
                "{route}"
            );
            assert!(view.is_folder_route(&page), "{page}");
        }
        let document = format!("{inner}/A.md");
        assert!(docview::document(&render, &document, "", &[]).is_ok());
        assert!(view.is_document_route(&document) && !view.is_folder_route(&document));
    }
}
