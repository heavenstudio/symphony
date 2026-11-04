//! `symphony.toml`: every default a missing key takes, what the file refuses by name, and Star
//! Foundry's committed manifest.

use std::path::Path;

use symphony::landing::Generator;
use symphony::lint::project_problems;
use symphony::project::{Entrypoint, FILE, PANES, Project, TRANSPORTS, slug};

use super::Tree;

/// A project folder called `name` inside a temporary tree.
fn folder(tree: &Tree, name: &str) -> std::path::PathBuf {
    let root = tree.path(name);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn parsed(text: &str) -> Result<Project, String> {
    Project::parse(Path::new("/projects/Night Shift"), text)
}

fn refused(text: &str) -> String {
    parsed(text).expect_err(text)
}

/// One `[[entrypoints]]` table serving `folder` at `url`, more keys appended after it.
fn entry(url: &str, folder: &str) -> String {
    format!("[[entrypoints]]\nurl = \"{url}\"\nfolder = \"{folder}\"\n")
}

#[test]
fn no_file_and_an_empty_file_are_the_plain_project_of_every_default() {
    let tree = Tree::new();
    let root = folder(&tree, "Night Shift");
    let plain = Project::plain(&root);
    assert_eq!(Project::load(&root).unwrap(), plain);
    std::fs::write(root.join(FILE), "").unwrap();
    assert_eq!(Project::load(&root).unwrap(), plain);
    assert_eq!(plain.name, "Night Shift");
    assert_eq!(plain.id, "night-shift");
    assert_eq!(plain.entrypoints, [Entrypoint::convention()]);
    assert_eq!(plain.history(), ["docs/history"]);
    assert_eq!(plain.register, "docs/DECISIONS.md");
    assert_eq!(plain.artifacts, "artifacts");
    assert_eq!(plain.default_branch, "main");
    assert!(plain.plugins.is_empty());
    assert_eq!(plain.theme, symphony::project::Theme::default());
    assert_eq!(plain.agents.transports, TRANSPORTS);
    assert_eq!(plain.agents.panes, "none");
    assert_eq!(plain.agents.worktrees, None);
    assert_eq!(plain.agents.launch, symphony::project::Launch::default());
    assert_eq!(plain.verdicts, symphony::project::Verdicts::default());
    assert_eq!(plain.board, symphony::project::Board::default());
    // Empty tables are the same project as no tables.
    assert_eq!(
        parsed("[project]\n[plugins]\n[theme]\n[agents]\n[agents.launch]\n[verdicts]\n[board]\n")
            .unwrap(),
        parsed("").unwrap()
    );
}

#[test]
fn a_folder_whose_name_makes_no_slug_is_still_a_plain_project() {
    let tree = Tree::new();
    for root in [Path::new(""), Path::new("/"), &folder(&tree, "★ ★")] {
        let plain = Project::plain(root);
        assert_eq!(
            plain.id,
            symphony::project::DEFAULT_ID,
            "{}",
            root.display()
        );
        assert_eq!(plain.entrypoints, [Entrypoint::convention()]);
    }
    assert_eq!(Project::of(&tree.path("★ ★")).id, "project");
    // a name the file gives still names the id, and one that makes no slug is refused
    assert_eq!(
        Project::parse(Path::new("/"), "[project]\nname = \"Night Shift\"\n")
            .unwrap()
            .id,
        "night-shift"
    );
    assert!(Project::parse(Path::new("/"), "[project]\nname = \"!!\"\n").is_err());
}

#[test]
fn each_key_left_out_takes_its_default_beside_those_given() {
    let project = parsed("[project]\nname = \"Deep Field\"\n").unwrap();
    assert_eq!(
        (project.name.as_str(), project.id.as_str()),
        ("Deep Field", "deep-field")
    );
    let project = parsed("[project]\nid = \"df\"\n").unwrap();
    assert_eq!(
        (project.name.as_str(), project.id.as_str()),
        ("Night Shift", "df")
    );
    // An entrypoint shows every Markdown file in its folder and passes over no history folder;
    // the register sits in the first entrypoint's folder, unless named.
    let project = parsed(&entry("/", "notes/")).unwrap();
    assert_eq!(
        project.entrypoints,
        [Entrypoint {
            url: String::new(),
            folder: "notes".into(),
            include: vec!["**".into()],
            exclude: Vec::new(),
            types: vec!["markdown".into()],
            history: Vec::new(),
        }]
    );
    assert!(project.history().is_empty());
    assert_eq!(project.register, "notes/DECISIONS.md");
    let project = parsed(&format!(
        "[project]\nregister = \"R.md\"\n{}history = [\"old\"]\n{}",
        entry("/notes/", "notes"),
        entry("/", "spec")
    ))
    .unwrap();
    assert_eq!(project.history(), ["notes/old"]);
    assert_eq!(project.register, "R.md");
    assert_eq!(
        project
            .entrypoints
            .iter()
            .map(|e| (e.url.as_str(), e.folder.as_str()))
            .collect::<Vec<_>>(),
        [("notes", "notes"), ("", "spec")],
        "in the file's order"
    );
    let project = parsed("[agents]\npanes = \"herdr\"\n").unwrap();
    assert_eq!(project.agents.panes, "herdr");
    assert_eq!(project.agents.transports, TRANSPORTS);
    let project = parsed("[agents]\ntransports = []\n").unwrap();
    assert!(project.agents.transports.is_empty());
    let project = parsed("[agents.launch]\nmodel = \"m\"\n").unwrap();
    assert_eq!(project.agents.launch.model.as_deref(), Some("m"));
    assert_eq!(project.agents.launch.default, None);
    let project = parsed("[verdicts]\ncheck = [\"make\", \"test\"]\n").unwrap();
    assert_eq!(project.verdicts.check, ["make", "test"]);
    assert!(project.verdicts.land.is_empty());
}

#[test]
fn an_unknown_table_or_key_is_refused_by_its_name() {
    for (text, name) in [
        ("[projct]\n", "projct"),
        ("[project]\nnmae = \"x\"\n", "nmae"),
        ("[plugins]\ncrate = [\"a\"]\n", "crate"),
        ("[theme]\ncolour = \"red\"\n", "colour"),
        ("[agents]\npane = \"herdr\"\n", "pane"),
        ("[agents.launch]\nreasoning = \"high\"\n", "reasoning"),
        ("[verdicts]\nlands = [\"x\"]\n", "lands"),
        (
            "[verdicts]\nland = [\"l\"]\ngenerators = [{ name = \"g\", outputs = \"o\", command = [\"x\"], every = true }]\n",
            "every",
        ),
        ("[board]\nexports = \"x\"\n", "exports"),
        // the documents folder is an entrypoint's now
        ("[project]\ndocs = \"docs\"\n", "docs"),
        ("[project]\nhistory = []\n", "history"),
        (
            "[[entrypoints]]\nurl = \"/\"\nfolder = \".\"\nfolders = [\"x\"]\n",
            "folders",
        ),
        ("[[entrypoints]]\nfolder = \"spec\"\n", "url"),
        ("[[entrypoints]]\nurl = \"/\"\n", "folder"),
    ] {
        let why = refused(text);
        assert!(why.starts_with("symphony.toml: "), "{why}");
        assert!(why.contains(name), "{text}: {why}");
    }
    assert!(refused("[project\n").starts_with("symphony.toml: "));
    assert!(refused("[project]\nname = 3\n").contains("name"));
}

#[test]
fn a_path_must_stay_inside_the_project() {
    let history = |folder: &str| format!("{}history = [\"{folder}\"]\n", entry("/docs/", "docs"));
    for (key, text) in [
        ("[[entrypoints]] /docs/ folder", entry("/docs/", "../docs")),
        ("[[entrypoints]] /docs/ folder", entry("/docs/", "/srv/docs")),
        ("[[entrypoints]] /docs/ folder", entry("/docs/", "")),
        ("[[entrypoints]] /docs/ history", history("../../old")),
        // the folder itself is not a history folder inside it
        ("[[entrypoints]] /docs/ history", history("./")),
        ("[project] register", "[project]\nregister = \"/R.md\"\n".into()),
        ("[project] artifacts", "[project]\nartifacts = \"..\"\n".into()),
        ("[theme] icon", "[theme]\nicon = \"../icon.svg\"\n".into()),
        ("[theme] stylesheet", "[theme]\nstylesheet = \"/a.css\"\n".into()),
        (
            "[verdicts] record",
            "[verdicts]\nrecord = \".git/../../x\"\n".into(),
        ),
        ("[board] export", "[board]\nexport = \"../tasks.md\"\n".into()),
        (
            "[verdicts] generators outputs",
            "[verdicts]\nland = [\"l\"]\ngenerators = [{ name = \"g\", outputs = \"/o\", command = [\"x\"] }]\n".into(),
        ),
    ] {
        let why = refused(&text);
        assert!(
            why.contains(key) && why.contains("must be a path inside the project"),
            "{text}: {why}"
        );
    }
    let project = parsed(&format!(
        "{}[verdicts]\nrecord = \".git/records\"\n",
        entry("/docs/", "./docs")
    ))
    .unwrap();
    assert_eq!(project.entrypoints[0].folder, "docs");
    assert_eq!(project.verdicts.record.as_deref(), Some(".git/records"));
    // each spelling of a folder is kept as the repository names its files
    for spelling in ["./manual", "manual/", "./manual/./", "manual//"] {
        let project = parsed(&format!(
            "{}history = [\"./past/\"]\n",
            entry("/docs/", spelling)
        ))
        .unwrap();
        assert_eq!(
            (
                project.entrypoints[0].folder.as_str(),
                project.register.as_str()
            ),
            ("manual", "manual/DECISIONS.md"),
            "{spelling}"
        );
        assert_eq!(project.history(), ["manual/past"], "{spelling}");
    }
    // the project's root is the empty folder, its register at the top
    for spelling in [".", "./", ".//."] {
        let project = parsed(&entry("/", spelling)).unwrap();
        assert_eq!(
            (
                project.entrypoints[0].folder.as_str(),
                project.register.as_str()
            ),
            ("", "DECISIONS.md"),
            "{spelling}"
        );
    }
    // a generator's outputs are spelled as changed names are, a folder keeping its slash so it
    // never matches a sibling that starts with its name
    let project = parsed(
        "[verdicts]\nland = [\"l\"]\ngenerators = [\
         { name = \"a\", outputs = \"./content/\", command = [\"x\"] },\
         { name = \"b\", outputs = \"./sim//ids.gd\", command = [\"x\"] }]\n",
    )
    .unwrap();
    assert_eq!(
        project
            .verdicts
            .generators
            .iter()
            .map(|g| g.outputs.as_str())
            .collect::<Vec<_>>(),
        ["content/", "sim/ids.gd"]
    );
}

#[test]
fn an_entrypoint_that_cannot_serve_is_refused_by_name() {
    let docs = entry("/docs/", "docs");
    for (text, says) in [
        (
            format!("{}types = [\"markdown\", \"rust\"]\n", entry("/", ".")),
            "[[entrypoints]] / type \"rust\": no renderer handles it (the renderers are markdown, media)",
        ),
        (
            format!("{docs}types = []\n"),
            "[[entrypoints]] /docs/ types is empty",
        ),
        (
            format!("{docs}include = []\n"),
            "[[entrypoints]] /docs/ include is empty",
        ),
        (
            format!("{docs}{}", entry("/docs", "notes")),
            "[[entrypoints]] /docs/ is named twice, over docs and notes",
        ),
        (
            format!("{docs}{}", entry("/notes/", "./docs/")),
            "[[entrypoints]] /notes/ reads docs, which /docs/ already serves",
        ),
        ("[[entrypoints]]\n".into(), "missing field"),
        (
            "entrypoints = []\n".into(),
            "[[entrypoints]] is empty: leave it out for the convention, docs/ at /docs/",
        ),
        // the Symphony's own routes win, so none is an entrypoint's
        (
            entry("/tasks/", "spec"),
            "[[entrypoints]] /tasks/ lies on /tasks, a route the Symphony answers",
        ),
        (
            entry("/api/v2", "spec"),
            "[[entrypoints]] /api/v2/ lies on /api, a route the Symphony answers",
        ),
        (
            entry("/artifacts/uploads/x/", "spec"),
            "lies on /artifacts/uploads,",
        ),
    ] {
        let why = refused(&text);
        assert!(why.starts_with("symphony.toml: "), "{why}");
        assert!(why.contains(says), "{text}: {why}");
    }
    for url in [
        "docs/",
        "/../x/",
        "/.hidden/",
        "/@main/",
        "/a b/",
        "/a?b/",
        "/a#b/",
        "/a%20/",
    ] {
        let why = refused(&entry(url, "spec"));
        assert!(
            why.contains(&format!(
                "[[entrypoints]] url {url:?} must be a path such as"
            )),
            "{url}: {why}"
        );
    }
    // a route under a Symphony route's name is not on it
    let project = parsed(&entry("/tasksx/", "spec")).unwrap();
    assert_eq!(project.entrypoints[0].url, "tasksx");
    let project = parsed(&entry("//guide//", "spec")).unwrap();
    assert_eq!(project.entrypoints[0].url, "guide");
}

#[test]
fn an_id_a_pane_a_branch_or_a_generator_that_cannot_work_is_refused() {
    assert!(refused("[project]\nid = \"Night Shift\"\n").contains("is not a slug"));
    assert!(refused("[project]\nid = \"\"\n").contains("is not a slug"));
    // A name with no letter or digit leaves no id.
    assert!(refused("[project]\nname = \"!!\"\n").contains("is not a slug"));
    let why = refused("[agents]\npanes = \"tmux\"\n");
    assert!(why.contains(&PANES.join(", ")), "{why}");
    assert!(refused("[project]\ndefault_branch = \" \"\n").contains("default_branch is empty"));
    assert!(
        refused("[verdicts]\nland = [\"l\"]\ngenerators = [{ name = \"g\", outputs = \"o\", command = [] }]\n")
            .contains("generator g has no command")
    );
    assert!(
        refused(
            "[verdicts]\ngenerators = [{ name = \"g\", outputs = \"o\", command = [\"x\"] }]\n"
        )
        .contains("generators need a land command")
    );
}

#[test]
fn a_name_becomes_an_id_of_lowercase_letters_digits_and_single_dashes() {
    for (name, id) in [
        ("Star Foundry", "star-foundry"),
        ("  Deep -- Field 2 ", "deep-field-2"),
        ("ÉCOLE", "cole"),
        ("a_b.c", "a-b-c"),
        ("--", ""),
    ] {
        assert_eq!(slug(name), id, "{name}");
    }
}

#[test]
fn documents_history_artifacts_and_the_record_folder_resolve_against_the_project() {
    let project = parsed(&format!(
        "{}history = [\"history\", \"old\"]\n",
        entry("/docs/", "docs")
    ))
    .unwrap();
    assert!(project.is_document_path("docs/SPEC.md"));
    assert!(!project.is_document_path("docs"), "a folder is no document");
    assert!(!project.is_document_path("docs/art.png"), "nor a picture");
    assert!(!project.is_document_path("docsy/SPEC.md"));
    assert!(!project.is_document_path("docs/history/2026.md"));
    assert!(!project.is_document_path("docs/old/x.md"));
    assert!(project.is_document_path("docs/older/x.md"));
    // a history page is still a page the owner may save
    assert!(project.is_page("docs/old/x.md") && !project.is_page("docs/old"));
    assert_eq!(project.history(), ["docs/history", "docs/old"]);
    assert_eq!(project.pathspecs(), ["docs"]);
    assert_eq!(
        project.not_a_document(),
        "path must be a markdown file under docs/"
    );
    assert_eq!(project.artifact("asks/a.json"), "artifacts/asks/a.json");
    let (root, common) = (Path::new("/w/tree"), Path::new("/w/main/.git"));
    assert_eq!(project.record_folder(root, common), None);
    let shared = parsed("[verdicts]\nrecord = \".git/sf-land/verdicts\"\n").unwrap();
    assert_eq!(
        shared.record_folder(root, common).unwrap(),
        common.join("sf-land/verdicts")
    );
    let local = parsed("[verdicts]\nrecord = \"records\"\n").unwrap();
    assert_eq!(
        local.record_folder(root, common).unwrap(),
        root.join("records")
    );
}

#[test]
fn a_cached_project_follows_its_file_and_a_broken_file_reads_as_the_plain_project() {
    let tree = Tree::new();
    let root = folder(&tree, "cached");
    assert_eq!(*Project::of(&root), Project::plain(&root));
    std::fs::write(root.join(FILE), "[project]\nname = \"Cached One\"\n").unwrap();
    assert_eq!(Project::of(&root).name, "Cached One");
    std::fs::write(root.join(FILE), "[project]\nname = \"Cached Second\"\n").unwrap();
    assert_eq!(Project::of(&root).name, "Cached Second");
    std::fs::write(root.join(FILE), "[project\n").unwrap();
    assert!(Project::load(&root).is_err());
    assert_eq!(*Project::of(&root), Project::plain(&root));
    // A file that cannot be read is refused, not taken for a missing one.
    std::fs::remove_file(root.join(FILE)).unwrap();
    std::fs::create_dir(root.join(FILE)).unwrap();
    assert!(Project::load(&root).is_err());
}

#[test]
fn the_nearest_folder_holding_a_project_file_is_the_project() {
    let tree = Tree::new();
    let root = folder(&tree, "outer");
    std::fs::create_dir_all(root.join("docs/ships")).unwrap();
    assert_eq!(Project::find(&root.join("docs/ships")), None);
    std::fs::write(root.join(FILE), "").unwrap();
    assert_eq!(Project::find(&root.join("docs/ships")), Some(root.clone()));
    assert_eq!(Project::find(&root), Some(root));
}

#[test]
fn star_foundrys_manifest_carries_the_games_values() {
    let root = super::common::repo();
    let project = Project::load(&root).unwrap();
    assert_eq!(project.name, "Star Foundry");
    assert_eq!(project.id, "star-foundry");
    // it passes no entrypoint, so the convention serves every page where it was
    assert_eq!(project.entrypoints, [Entrypoint::convention()]);
    assert_eq!(project.history(), ["docs/history"]);
    assert_eq!(project.register, "docs/DECISIONS.md");
    assert_eq!(project.default_branch, "main");
    assert_eq!(project.plugins, ["star-foundry-symphony"]);
    assert!(symphony::plugins::problems(&project).is_empty());
    assert_eq!(
        symphony::plugins::Plugins::for_project(&project)
            .list()
            .iter()
            .map(|plugin| plugin.name())
            .collect::<Vec<_>>(),
        ["star-foundry-symphony"]
    );
    assert_eq!(project.theme.package.as_deref(), Some("star-foundry"));
    let icon = project.theme.icon.as_deref().unwrap();
    assert!(root.join(icon).is_file(), "{icon}");
    assert_eq!(project.agents.transports, TRANSPORTS);
    assert_eq!(project.agents.panes, "herdr");
    assert_eq!(project.verdicts.check, ["just", "verify"]);
    assert_eq!(project.verdicts.land, ["just", "land"]);
    assert_eq!(
        project.verdicts.generators,
        [
            Generator {
                name: "content".into(),
                outputs: "content/".into(),
                command: vec!["just".into(), "content".into()],
                when_changed: false,
            },
            Generator {
                name: "content-ids".into(),
                outputs: "sim/content_ids.gd".into(),
                command: vec!["just".into(), "content-ids".into()],
                when_changed: true,
            },
            Generator {
                name: "hull-atlas".into(),
                outputs: "view/hull_atlas".into(),
                command: vec!["just".into(), "hull-atlas".into(), "build".into()],
                when_changed: true,
            },
        ]
    );
    assert_eq!(
        symphony::landing::generators(&root),
        project.verdicts.generators
    );
    assert_eq!(
        project.verdicts.record.as_deref(),
        Some(".git/sf-land/verdicts")
    );
    assert_eq!(
        project.board.export.as_deref(),
        Some("docs/history/tasks.md")
    );
    assert_eq!(
        project.board.disposable,
        ["artifacts/*", ".godot/*", ".venv/*", ".target/*"]
    );
    assert_eq!(
        project.board.on_review,
        ["tools/rs-bin.sh", "sf-land", "review"]
    );
}

#[test]
fn a_project_naming_a_plugin_this_build_lacks_is_told_and_keeps_the_ones_it_has() {
    let project =
        parsed("[plugins]\ncrates = [\"no-such-plugin\", \"star-foundry-symphony\"]\n").unwrap();
    assert_eq!(
        symphony::plugins::problems(&project),
        ["symphony.toml: [plugins] crates names no-such-plugin, which this build does not hold"]
    );
    assert_eq!(
        symphony::plugins::Plugins::for_project(&project)
            .list()
            .len(),
        1
    );
}

#[test]
fn the_lint_names_why_the_project_file_is_refused_and_fails_on_it() {
    let tree = Tree::new();
    let lint = || {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = symphony::lint::main(&tree.root, &mut out, &mut err);
        (code, String::from_utf8(err).unwrap())
    };
    assert!(project_problems(&tree.root).is_empty());
    tree.write(FILE, "[project]\nid = \"Night Shift\"\n");
    let problems = project_problems(&tree.root);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("is not a slug"), "{problems:?}");
    let (code, err) = lint();
    assert_eq!(code, 1);
    assert!(err.contains(&problems[0]), "{err}");
    assert!(err.contains("✖ the project file is refused"), "{err}");
    tree.write(FILE, "[plugins]\ncrates = [\"no-such-plugin\"]\n");
    assert_eq!(
        project_problems(&tree.root),
        ["symphony.toml: [plugins] crates names no-such-plugin, which this build does not hold"]
    );
    assert_eq!(lint().0, 1);
}

#[test]
fn the_page_inlines_the_projects_view_as_json_no_value_can_close_or_comment_out() {
    let plain = parsed("").unwrap().view();
    assert_eq!(plain.name, "Night Shift");
    assert_eq!(plain.register, "docs/DECISIONS.md");
    assert!(plain.nav.is_empty(), "a plain project has no plugin pages");
    let star_foundry = Project::of(&super::common::repo()).view();
    assert_eq!(star_foundry.name, "Star Foundry");
    assert_eq!(
        star_foundry
            .nav
            .iter()
            .map(|item| (item.href.as_str(), item.document.as_str()))
            .collect::<Vec<_>>(),
        [("/hulls", "docs/ships/HULLS.md")]
    );
    let hostile = symphony_api::ProjectView {
        name: "</script><!--<script>alert(1)</script>".into(),
        ..plain
    };
    let element = Project::element(&hostile);
    let open = format!(
        "<script type=\"application/json\" id=\"{}\">",
        symphony_api::PROJECT_ELEMENT
    );
    let json = element
        .strip_prefix(&open)
        .and_then(|rest| rest.strip_suffix("</script>"))
        .unwrap_or_else(|| panic!("{element}"));
    assert!(!json.contains('<'), "{json}");
    assert_eq!(
        serde_json::from_str::<symphony_api::ProjectView>(json).unwrap(),
        hostile
    );
}
