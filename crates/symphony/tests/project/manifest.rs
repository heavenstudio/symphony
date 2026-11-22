//! `symphony.toml`: every default a missing key takes, what the file refuses by name, and the
//! consuming project's committed manifest.

use std::path::Path;

use symphony::lint::project_problems;
use symphony::project::{Entrypoint, FILE, PANES, Project, slug};

use super::Tree;
mod limits;
mod machine_work;
mod retired;
mod settings_check;
mod stamps;

/// A project folder called `name` inside a temporary tree.
fn folder(tree: &Tree, name: &str) -> std::path::PathBuf {
    let root = tree.path(name);
    std::fs::create_dir_all(&root).unwrap();
    root
}

pub(super) fn parsed(text: &str) -> Result<Project, String> {
    Project::parse(Path::new("/projects/Night Shift"), text)
}

pub(super) fn refused(text: &str) -> String {
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
    // An entrypoint shows every Markdown and HTML document in its folder and passes over no
    // history folder; the register sits in the first entrypoint's folder, unless named.
    let project = parsed(&entry("/", "notes/")).unwrap();
    assert_eq!(
        project.entrypoints,
        [Entrypoint {
            url: String::new(),
            folder: "notes".into(),
            include: vec!["**".into()],
            exclude: Vec::new(),
            types: vec![symphony::entrypoint::MARKDOWN, symphony::entrypoint::HTML],
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
    let project = parsed("[agents]\npanes = \"hc\"\n").unwrap();
    assert_eq!(project.agents.panes, "hc");
    let project = parsed("[agents.launch]\nmodel = \"m\"\n").unwrap();
    assert_eq!(project.agents.launch.model.as_deref(), Some("m"));
    assert_eq!(project.agents.launch.default, None);
    let project = parsed("[verdicts]\ncheck = [\"make\", \"test\"]\n").unwrap();
    assert_eq!(project.verdicts.check, ["make", "test"]);
    assert!(project.verdicts.land.is_empty());
    // The release candidate's bounds: 3 red reruns and 24 hours, 0 in either holding none.
    let releases = parsed("").unwrap().releases;
    assert_eq!(
        (releases.candidate_reruns, releases.candidate_hours),
        (3, 24)
    );
    assert!(releases.holds_candidates());
    let releases = parsed("[releases]\ncandidate_hours = 6\n")
        .unwrap()
        .releases;
    assert_eq!(
        (releases.candidate_reruns, releases.candidate_hours),
        (3, 6)
    );
    for off in ["candidate_reruns = 0", "candidate_hours = 0"] {
        let releases = parsed(&format!("[releases]\n{off}\n")).unwrap().releases;
        assert!(!releases.holds_candidates(), "{off}");
    }
    assert!(refused("[releases]\ncandidate_hours = -1\n").contains("candidate_hours"));
    // A green release publishes on crates.io only on the project's word.
    assert!(!parsed("").unwrap().releases.publish);
    assert!(
        !parsed("[releases]\nrefresh = true\n")
            .unwrap()
            .releases
            .publish
    );
    assert!(
        parsed("[releases]\npublish = true\n")
            .unwrap()
            .releases
            .publish
    );
    assert!(refused("[releases]\npublish = \"yes\"\n").contains("publish"));
    // Its GitHub Release is asked of the command the project names, and of none unsaid.
    assert!(parsed("").unwrap().releases.github.is_empty());
    assert!(
        (parsed("[releases]\ngithub = []\n").unwrap().releases.github).is_empty(),
        "an empty command is none"
    );
    assert_eq!(
        parsed("[releases]\ngithub = [\"symphony-github\", \"release\"]\n")
            .unwrap()
            .releases
            .github,
        ["symphony-github", "release"]
    );
    assert!(refused("[releases]\ngithub = [\" \", \"release\"]\n").contains("names no command"));
    assert!(refused("[releases]\ngithub = \"symphony-github\"\n").contains("github"));
    // The command that verifies a release's commit on GitHub's machines, none unless said.
    assert!(parsed("").unwrap().releases.hosted.is_empty());
    assert!((parsed("[releases]\nhosted = []\n").unwrap().releases.hosted).is_empty());
    assert_eq!(
        parsed("[releases]\nhosted = [\"symphony-github\", \"verify\"]\n")
            .unwrap()
            .releases
            .hosted,
        ["symphony-github", "verify"]
    );
    let unnamed = refused("[releases]\nhosted = [\"\", \"verify\"]\n");
    assert!(
        unnamed
            .contains("[releases] hosted names no command: leave it out to verify a release here"),
        "{unnamed}"
    );
    assert!(refused("[releases]\nhosted = true\n").contains("hosted"));
    // Its clock: 30 minutes unless said, and none while no command is named or the minutes are 0.
    let named = "[releases]\nhosted = [\"symphony-github\", \"verify\"]\n";
    let releases = parsed("").unwrap().releases;
    assert_eq!(releases.hosted_every_minutes, 30);
    assert_eq!(releases.hosted_clock(), None, "no command, no clock");
    assert_eq!(parsed(named).unwrap().releases.hosted_clock(), Some(30));
    let set = |minutes: &str| {
        parsed(&format!("{named}hosted_every_minutes = {minutes}\n"))
            .unwrap()
            .releases
            .hosted_clock()
    };
    assert_eq!(set("25"), Some(25));
    assert_eq!(set("0"), None, "0 starts none by the clock");
    assert_eq!(
        parsed("[releases]\nhosted_every_minutes = 25\n")
            .unwrap()
            .releases
            .hosted_clock(),
        None
    );
    for wrong in ["-1", "\"often\"", "2.5"] {
        let refusal = refused(&format!("[releases]\nhosted_every_minutes = {wrong}\n"));
        assert!(
            refusal.contains("hosted_every_minutes"),
            "{wrong}: {refusal}"
        );
    }
    // Who bypasses the rules is verified again by the command the project names, and by none
    // unsaid.
    assert!(parsed("").unwrap().releases.rulesets.is_empty());
    assert!(
        (parsed("[releases]\nrulesets = []\n")
            .unwrap()
            .releases
            .rulesets)
            .is_empty(),
        "an empty command is none"
    );
    assert_eq!(
        parsed("[releases]\nrulesets = [\"symphony-github\", \"rulesets\", \"--verify\"]\n")
            .unwrap()
            .releases
            .rulesets,
        ["symphony-github", "rulesets", "--verify"]
    );
    let blank = refused("[releases]\nrulesets = [\"\", \"rulesets\"]\n");
    assert!(
        blank.contains("[releases] rulesets names no command"),
        "{blank}"
    );
    assert!(refused("[releases]\nrulesets = \"symphony-github\"\n").contains("rulesets"));
}

#[test]
fn an_unknown_table_or_key_is_refused_by_its_name() {
    for (text, name) in [
        ("[projct]\n", "projct"),
        ("[project]\nnmae = \"x\"\n", "nmae"),
        ("[plugins]\ncrate = [\"a\"]\n", "crate"),
        ("[theme]\ncolour = \"red\"\n", "colour"),
        ("[agents]\npane = \"hc\"\n", "pane"),
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
        ("[board] guide", "[board]\nguide = \"../TASKS.md\"\n".into()),
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
            "[[entrypoints]] / type \"rust\": no renderer handles it (the renderers are markdown, html, media)",
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
            entry("/reports/", "spec"),
            "[[entrypoints]] /reports/ lies on /reports, a route the Symphony answers",
        ),
        (
            entry("/runs/", "spec"),
            "[[entrypoints]] /runs/ lies on /runs, a route the Symphony answers",
        ),
        (
            entry("/settings/", "spec"),
            "[[entrypoints]] /settings/ lies on /settings, a route the Symphony answers",
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
fn the_land_table_names_what_the_regenerator_makes_and_whether_a_land_pushes_and_checks() {
    let plain = parsed("").unwrap();
    assert!(
        plain.land.push,
        "a land pushes unless the project says not to"
    );
    assert!(plain.land.generators.is_empty());
    let project = parsed(
        "[land]\npush = false\ngenerators = [\n  { name = \"the client\", outputs = \"client/public/\", \
         command = [\"just\", \"client-update\"] },\n  { name = \"the index\", outputs = \"./index.txt\", \
         command = [\"x\"], when_changed = true },\n]\n",
    )
    .unwrap();
    assert!(!project.land.push);
    let generators: Vec<(&str, &str, bool)> = project
        .land
        .generators
        .iter()
        .map(|g| (g.name.as_str(), g.outputs.as_str(), g.when_changed))
        .collect();
    assert_eq!(
        generators,
        [
            ("the client", "client/public/", false),
            ("the index", "index.txt", true),
        ]
    );
    // A land generator needs no `[verdicts] land`: it is the default land's own.
    assert!(project.verdicts.land.is_empty());
    assert!(
        refused("[land]\ngenerators = [{ name = \"g\", outputs = \"o\", command = [] }]\n")
            .contains("[land] generator g has no command")
    );
    assert!(
        refused("[land]\ngenerators = [{ name = \"g\", outputs = \"../o\", command = [\"x\"] }]\n")
            .contains("[land] generators outputs")
    );
    for (text, name) in [
        ("[land]\npussh = true\n", "[land] pussh"),
        (
            "[land]\ngenerators = [{ name = \"g\", outputs = \"o\", command = [\"x\"], when = 1 }]\n",
            "when",
        ),
    ] {
        let why = refused(text);
        assert!(why.contains(name), "{text}: {why}");
    }
    assert!(refused("[land]\npush = \"no\"\n").contains("push"));
    // How long each command a land runs may take: an hour unless the project says.
    assert_eq!(
        plain.land.timeout_seconds,
        symphony_board::land::TIMEOUT.as_secs()
    );
    assert_eq!(
        parsed("[land]\ntimeout_seconds = 900\n")
            .unwrap()
            .land
            .timeout_seconds,
        900
    );
    assert!(
        refused("[land]\ntimeout_seconds = 0\n")
            .contains("[land] timeout_seconds is 0, which no generator or check runs in")
    );
    assert!(refused("[land]\ntimeout_seconds = \"1h\"\n").contains("timeout_seconds"));
    // How often the delivery watcher reads the host, and how long a host is given to merge: keys
    // the project file knows, which the board reads (`symphony_board::land::Settings`), neither 0.
    assert!(parsed("[land]\nwatch_seconds = 10\nmerge_within_minutes = 45\n").is_ok());
    assert!(
        refused("[land]\nwatch_seconds = 0\n")
            .contains("[land] watch_seconds is 0: the watcher rests between its rounds")
    );
    assert!(
        refused("[land]\nmerge_within_minutes = 0\n")
            .contains("[land] merge_within_minutes is 0: a host is given time to merge")
    );
    assert!(refused("[land]\nwatch_seconds = \"30s\"\n").contains("watch_seconds"));
    // Whether a land runs `[verdicts] check`: on unless the project turns it off.
    assert!(plain.land.check);
    assert!(!parsed("[land]\ncheck = false\n").unwrap().land.check);
    assert!(refused("[land]\ncheck = \"no\"\n").contains("check"));
    // What holds the project's tests and checks, which the land reads for a red's fix: a key the
    // project file knows, each entry a path inside the project.
    assert!(parsed("[land]\ncheck_paths = [\"tests/\", \"/symphony.toml [verdicts]\"]\n").is_ok());
    assert!(
        refused("[land]\ncheck_paths = [\"../x\"]\n")
            .contains("[land] check_paths names \"../x\", which is no path inside the project")
    );
    // The command that delivers a task by a hosted pull request: a key the project file knows, a
    // list of words, and never one naming no command.
    assert!(parsed("[land]\ndelivery = [\"symphony-github\", \"deliver\"]\n").is_ok());
    assert!(refused("[land]\ndelivery = \"gh\"\n").contains("delivery"));
    assert!(
        refused("[land]\ndelivery = [\"\"]\n")
            .contains("[land] delivery names no command: leave it out to land here")
    );
    // The upstream an import is of: read whole, and refused where git could misread it.
    let upstream =
        "[land.upstream]\nurl = \"https://github.com/up/stream.git\"\nbranch = \"main\"\n";
    assert!(parsed(upstream).is_ok());
    assert!(refused("[land.upstream]\nurl = \"u\"\n").contains("branch"));
    assert!(refused(&format!("{upstream}token = \"x\"\n")).contains("token"));
    assert!(
        refused("[land.upstream]\nurl = \"--upload-pack=x\"\nbranch = \"main\"\n")
            .contains("[land.upstream] url is \"--upload-pack=x\"")
    );
    // A generator's key, which the regenerator runs before it, named in `[land] keys`: a key
    // inside the generator's own table would make an older Symphony refuse the whole file.
    let generator =
        "[land]\ngenerators = [{ name = \"g\", outputs = \"o/\", command = [\"x\"] }]\n";
    let keyed = parsed(&format!("{generator}keys = {{ g = [\"k\"] }}\n")).unwrap();
    assert_eq!(
        keyed.land.keys,
        std::collections::BTreeMap::from([("g".to_string(), vec!["k".to_string()])])
    );
    let unkeyed = parsed(generator).unwrap();
    assert!(unkeyed.land.keys.is_empty());
    // Beside the generators, never one of their fields: a consumer's code builds one by its fields.
    assert_eq!(keyed.land.generators, unkeyed.land.generators);
    assert!(
        refused(&format!("{generator}keys = {{ h = [\"k\"] }}\n"))
            .contains("[land] keys names \"h\", which no [land] generator is")
    );
    // A key names one generator: two of one name are refused, keyed or not.
    let twice = "[land]\ngenerators = [{ name = \"g\", outputs = \"o/\", command = [\"x\"] }, { \
                 name = \"g\", outputs = \"p/\", command = [\"x\"] }]\n";
    for text in [
        twice.to_string(),
        format!("{twice}keys = {{ g = [\"k\"] }}\n"),
    ] {
        assert!(
            refused(&text).contains(
                "[land] generators names \"g\" twice: give each generator a name of its own"
            ),
            "{text}"
        );
    }
    assert!(
        refused(
            "[land]\ngenerators = [{ name = \"g\", outputs = \"o/\", command = [\"x\"], key = \
             [\"k\"] }]\n"
        )
        .contains("key")
    );
    // The land reads the same table the same way.
    for text in [
        "",
        "[land]\ntimeout_seconds = 900\n",
        "[land]\npush = false\n",
        "[verdicts]\ncheck = [\"c\"]\n",
        "[verdicts]\ncheck = [\"c\"]\n[land]\ncheck = false\n",
    ] {
        let land = symphony_board::land::Settings::parse(text).unwrap();
        let project = parsed(text).unwrap();
        assert_eq!(
            (land.timeout.as_secs(), land.push),
            (project.land.timeout_seconds, project.land.push),
            "{text}"
        );
        let checked = if project.land.check {
            project.verdicts.check.clone()
        } else {
            Vec::new()
        };
        assert_eq!(land.check, checked, "{text}");
    }
}

#[test]
fn a_name_becomes_an_id_of_lowercase_letters_digits_and_single_dashes() {
    for (name, id) in [
        ("Lamp Works", "lamp-works"),
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
    assert!(
        !project.is_document_path("docs/picture.png"),
        "nor a picture"
    );
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
    let shared = parsed("[verdicts]\nrecord = \".git/land/verdicts\"\n").unwrap();
    assert_eq!(
        shared.record_folder(root, common).unwrap(),
        common.join("land/verdicts")
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
    std::fs::create_dir_all(root.join("docs/lamps")).unwrap();
    assert_eq!(Project::find(&root.join("docs/lamps")), None);
    std::fs::write(root.join(FILE), "").unwrap();
    assert_eq!(Project::find(&root.join("docs/lamps")), Some(root.clone()));
    assert_eq!(Project::find(&root), Some(root));
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

#[test]
fn a_table_or_key_this_build_does_not_know_is_left_out_of_the_running_project_and_the_rest_read() {
    let tree = Tree::new();
    let root = folder(&tree, "Night Shift");
    std::fs::write(
        root.join(FILE),
        "[project]\nname = \"Lamp Works\"\n\n[board]\nslot_locks = \"locks\"\n\
         on_land = [\"z\"]\non_review = [\"x\"]\nguide = \"docs/TASKS.md\"\n\
         starts = [\"y\"]\n\n[theme]\ncolour = \"red\"\n\n[hooks]\nbeat = 1\n",
    )
    .unwrap();
    // loading and linting refuse them all by name, where they stand
    let refusal = "symphony.toml: [hooks] (line 14), [theme] colour (line 12), [board] starts (line 9) are not settings the Symphony knows";
    assert_eq!(Project::load(&root).unwrap_err(), refusal);
    assert_eq!(project_problems(&root), [refusal]);
    // the running project leaves them out and keeps every setting it knows
    let project = Project::of(&root);
    assert_eq!(project.name, "Lamp Works");
    assert_eq!(project.board.slot_locks.as_deref(), Some("locks"));
    assert_eq!(project.board.on_land, ["z"]);
    assert_eq!(project.board.on_review, ["x"]);
    assert_eq!(project.board.guide(), "docs/TASKS.md");
    assert_ne!(*project, Project::plain(&root));
    // one is named alone, and before any other refusal
    assert_eq!(
        refused("[board]\nexports = \"x\"\n"),
        "symphony.toml: [board] exports (line 2) is not a setting the Symphony knows"
    );
    // an entrypoint's table stays strict: a misspelt key there refuses the file where it is read,
    // and the running project is the plain one
    let why = refused("[[entrypoints]]\nurl = \"/\"\nfolder = \".\"\nfolders = [\"x\"]\n");
    assert!(why.contains("line 4") && why.contains("folders"), "{why}");
    let strict = folder(&tree, "Strict");
    std::fs::write(
        strict.join(FILE),
        "[project]\nname = \"Lamp Works\"\n\n[[entrypoints]]\nurl = \"/\"\nfolder = \".\"\nfolders = [\"x\"]\n",
    )
    .unwrap();
    assert_eq!(*Project::of(&strict), Project::plain(&strict));
    assert_eq!(
        refused("[agents]\npanes = \"tmux\"\npane = \"herdr\"\n"),
        "symphony.toml: [agents] pane (line 3) is not a setting the Symphony knows"
    );
    // a dotted key and a dotted table header lead to their tables too
    assert_eq!(
        refused("board.starts = [1]\n"),
        "symphony.toml: [board] starts (line 1) is not a setting the Symphony knows"
    );
    assert_eq!(
        refused("[agents.launch]\nreasoning = \"high\"\n"),
        "symphony.toml: [agents.launch] reasoning (line 2) is not a setting the Symphony knows"
    );
}
