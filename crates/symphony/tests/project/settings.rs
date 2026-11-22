//! The Settings page's read (`symphony::settings::page`): each value in force with where it was
//! written, for a project with no file, with a project file and with a local file over it, and a
//! project file that does not load said as such; and the class of every key of the reference,
//! which the read takes from the local settings file's lists.

use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use super::reference::{REFERENCE, listed, read};
use crate::Tree;
use symphony::settings::{Machine, class, page, scopes};
use symphony_api::{Setting, SettingClass, SettingKind, SettingScope, SettingsEntry, SettingsPage};

/// A machine with nothing on its search path.
fn bare() -> Machine {
    Machine::path(None)
}

fn entry<'a>(page: &'a SettingsPage, id: &str) -> &'a SettingsEntry {
    page.sections
        .iter()
        .flat_map(|section| &section.entries)
        .find(|entry| entry.id == id)
        .unwrap_or_else(|| panic!("no entry {id}"))
}

fn setting<'a>(page: &'a SettingsPage, id: &str, key: &str) -> &'a Setting {
    entry(page, id)
        .settings
        .iter()
        .find(|setting| setting.key == key)
        .unwrap_or_else(|| panic!("no setting {key} in {id}"))
}

/// Every setting of the page.
fn settings(page: &SettingsPage) -> Vec<&Setting> {
    page.sections
        .iter()
        .flat_map(|section| &section.entries)
        .flat_map(|entry| &entry.settings)
        .collect()
}

/// `git` with `args` in `root`.
fn git(root: &Path, args: &[&str]) {
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_COMMON_DIR"),
    )
    .unwrap();
    assert!(done.status.success(), "{args:?}: {done:?}");
}

#[test]
fn a_project_with_no_file_shows_every_default_and_says_none_is_written() {
    let tree = Tree::new();
    let page = page(&tree.root, &bare());
    assert_eq!(page.problems, Vec::<String>::new());
    assert_eq!(
        page.files
            .iter()
            .map(|file| (file.name.as_str(), file.found))
            .collect::<Vec<_>>(),
        [("symphony.toml", false), ("symphony.local.toml", false)]
    );
    for setting in settings(&page) {
        if setting.source.scope == Some(SettingScope::Device) {
            continue;
        }
        assert_eq!(setting.source.scope, None, "{}", setting.key);
        assert!(setting.source.said.starts_with("default"), "{setting:?}");
        assert_eq!((&setting.source.file, setting.source.line), (&None, None));
    }
    let panes = setting(&page, "herdr", "[agents] panes");
    assert_eq!(
        (panes.value.as_str(), panes.kind),
        ("none", SettingKind::Choice)
    );
    assert_eq!(panes.choices, ["hc", "none"]);
    assert_eq!(
        entry(&page, "herdr").state.as_ref().unwrap().on,
        Some(false)
    );
    let slots = setting(&page, "limits", "[limits] orchestrator_slots");
    assert_eq!(
        (slots.value.as_str(), slots.source.said.as_str()),
        ("no cap", "default: no cap")
    );
    assert_eq!(setting(&page, "limits", "[limits] low_volume").value, "4");
    let agent = setting(&page, "launch", "[agents] launch default");
    assert_eq!(agent.value, "codex");
    assert_eq!(agent.source.said, "default: the board's own");
    let assessor = setting(&page, "models", "[agents.models] assessor");
    assert_eq!(assessor.value, "gpt-6.1-sol");
    let orchestrator = setting(&page, "models", "[agents.models] orchestrator");
    assert_eq!(orchestrator.value, "not set");
    assert_eq!(
        orchestrator.source.said,
        "default: whoever starts one chooses"
    );
    let stamps = setting(&page, "stamps", "[stamps]");
    assert_eq!(
        stamps.value,
        "The Symphony's four: tests, review, pictures, owner."
    );
    assert_eq!(stamps.source.said, "default: no [stamps] written");
    let github = entry(&page, "github");
    assert_eq!(github.state.as_ref().unwrap().on, Some(false));
    assert_eq!(
        github.state.as_ref().unwrap().why,
        "Off: no symphony-github.toml on main."
    );
    let workflow = entry(&page, "workflow");
    assert!(workflow.settings.is_empty(), "the workflow claims no class");
    assert_eq!(workflow.state.as_ref().unwrap().word, "Built in");
    assert_eq!(
        workflow.source.as_ref().unwrap().said,
        "default: no [workflow] written"
    );
}

const PROJECT: &str = "\
[project]
name = \"Lamp Shop\"

[agents]
panes = \"hc\"
launch = { default = \"claude\", model = \"opus\" }

[agents.models]
reviewer = \"opus\"
executor = \"sonnet\"

[land]
check = false

[board]
orchestrator_slots = 8
on_land = [\"tools/after\", \"--due\"]
review_alarm_hours = 6

[limits]
low_volume = 2
";

#[test]
fn a_project_file_gives_each_value_it_writes_with_its_table_key_and_line() {
    let tree = Tree::new();
    tree.write("symphony.toml", PROJECT);
    let page = page(&tree.root, &bare());
    assert_eq!(page.problems, Vec::<String>::new());
    assert!(page.files[0].found);
    let said = |id: &str, key: &str| {
        let setting = setting(&page, id, key);
        (setting.value.clone(), setting.source.said.clone())
    };
    let told = |value: &str, from: &str| (value.to_string(), from.to_string());
    assert_eq!(
        said("identity", "[project] name"),
        told("Lamp Shop", "symphony.toml [project] name, line 2")
    );
    assert_eq!(
        said("herdr", "[agents] panes"),
        told("hc", "symphony.toml [agents] panes, line 5")
    );
    assert_eq!(entry(&page, "herdr").state.as_ref().unwrap().on, Some(true));
    assert_eq!(
        said("launch", "[agents] launch default"),
        told("claude", "symphony.toml [agents] launch default, line 6")
    );
    assert_eq!(
        said("launch", "[agents] launch model"),
        told("opus", "symphony.toml [agents] launch model, line 6")
    );
    assert_eq!(
        said("models", "[agents.models] executor"),
        told("sonnet", "symphony.toml [agents.models] executor, line 10")
    );
    // No model written for the role: where the board takes it from.
    assert_eq!(
        said("models", "[agents.models] assessor"),
        told("haiku", "default: the smallest claude offers")
    );
    // The cap an older build reads in `[board]` is the one in force, named where it stands.
    assert_eq!(
        said("limits", "[limits] orchestrator_slots"),
        told("8", "symphony.toml [board] orchestrator_slots, line 16")
    );
    assert_eq!(
        said("limits", "[limits] low_volume"),
        told("2", "symphony.toml [limits] low_volume, line 21")
    );
    assert_eq!(
        said("alarm", "[board] review_alarm_hours"),
        told("6", "symphony.toml [board] review_alarm_hours, line 18")
    );
    assert_eq!(
        said("alarm", "[board] review_alarm_ratio"),
        told("2", "default")
    );
    let low = setting(&page, "limits", "[limits] low_volume");
    assert_eq!(low.source.scope, Some(SettingScope::Project));
    assert_eq!(low.source.file.as_deref(), Some("symphony.toml"));
    assert_eq!(low.source.line, Some(21));
    assert_eq!(low.source.over, None);
    assert_eq!(
        entry(&page, "launch").about,
        "A launch whose caller names nothing starts claude on opus."
    );
}

#[test]
fn a_reviewed_setting_names_its_file_and_line_and_is_never_a_machines() {
    let tree = Tree::new();
    tree.write("symphony.toml", PROJECT);
    let page = page(&tree.root, &bare());
    let after = setting(&page, "commands", "[board] on_land");
    assert_eq!(after.value, "tools/after --due");
    assert_eq!(after.class, SettingClass::Reviewed);
    assert_eq!(after.source.said, "symphony.toml [board] on_land, line 17");
    assert_eq!(after.scopes, [SettingScope::Project]);
    let land = setting(&page, "landing", "[land]");
    assert_eq!(land.kind, SettingKind::Sentence);
    assert_eq!(land.class, SettingClass::Reviewed);
    assert_eq!(
        land.value,
        "No check; pushes main; no generator; merged here."
    );
    assert_eq!(land.source.said, "symphony.toml [land], line 12");
    assert_eq!(land.source.line, Some(12));
    // One no file writes is reviewed all the same, and says it is the default.
    let review = setting(&page, "commands", "[board] on_review");
    assert_eq!(
        (review.value.as_str(), review.class),
        ("none", SettingClass::Reviewed)
    );
    assert_eq!(review.source.said, "default");
    for setting in settings(&page) {
        if setting.class == SettingClass::Reviewed {
            assert_eq!(setting.scopes, [SettingScope::Project], "{}", setting.key);
        }
    }
}

#[test]
fn a_local_file_holds_over_the_project_file_and_names_what_it_overrides() {
    let tree = Tree::new();
    tree.write("symphony.toml", PROJECT);
    tree.write(
        "symphony.local.toml",
        "[agents.models]\nexecutor = \"fable\"\norchestrator = \"fable\"\n\n[limits]\nlow_volume = 1\nmachine_agents = 4\n",
    );
    let page = page(&tree.root, &bare());
    assert_eq!(page.problems, Vec::<String>::new());
    assert!(page.files[1].found);
    let executor = setting(&page, "models", "[agents.models] executor");
    assert_eq!(executor.value, "fable");
    assert_eq!(executor.source.scope, Some(SettingScope::Machine));
    assert_eq!(
        executor.source.said,
        "symphony.local.toml [agents.models] executor, line 2"
    );
    assert_eq!(executor.source.file.as_deref(), Some("symphony.local.toml"));
    assert_eq!(
        executor.source.over.as_deref(),
        Some("symphony.toml [agents.models] executor, line 10")
    );
    // One the project file does not write overrides nothing.
    let orchestrator = setting(&page, "models", "[agents.models] orchestrator");
    assert_eq!(
        (orchestrator.value.as_str(), &orchestrator.source.over),
        ("fable", &None)
    );
    // One the local file leaves alone keeps the committed one.
    let reviewer = setting(&page, "models", "[agents.models] reviewer");
    assert_eq!(
        reviewer.source.said,
        "symphony.toml [agents.models] reviewer, line 9"
    );
    let low = setting(&page, "limits", "[limits] low_volume");
    assert_eq!(low.value, "1");
    assert_eq!(
        low.source.said,
        "symphony.local.toml [limits] low_volume, line 6"
    );
    assert_eq!(
        low.source.over.as_deref(),
        Some("symphony.toml [limits] low_volume, line 21")
    );
    let agents = setting(&page, "limits", "[limits] machine_agents");
    assert_eq!((agents.value.as_str(), &agents.source.over), ("4", &None));
    assert_eq!(agents.source.scope, Some(SettingScope::Machine));
}

#[test]
fn a_local_file_that_is_refused_is_said_and_none_of_it_holds() {
    let tree = Tree::new();
    tree.write("symphony.toml", PROJECT);
    tree.write(
        "symphony.local.toml",
        "[limits]\nlow_volume = 1\n\n[land]\ncheck = true\n",
    );
    let page = page(&tree.root, &bare());
    assert_eq!(page.problems.len(), 1, "{:?}", page.problems);
    assert!(
        page.problems[0].contains("[land] check (line 5)"),
        "{:?}",
        page.problems
    );
    assert!(page.problems[0].contains("none of symphony.local.toml is read"));
    let low = setting(&page, "limits", "[limits] low_volume");
    assert_eq!(low.value, "2");
    assert_eq!(low.source.scope, Some(SettingScope::Project));
}

#[test]
fn a_project_file_that_does_not_load_is_said_and_every_default_is_shown() {
    let tree = Tree::new();
    tree.write(
        "symphony.toml",
        "[project]\nname = \"Lamp\"\n\n[agents]\npanes = \"tmux\"\n",
    );
    let page = page(&tree.root, &bare());
    assert_eq!(page.problems.len(), 1, "{:?}", page.problems);
    assert!(
        page.problems[0].starts_with("symphony.toml: "),
        "{:?}",
        page.problems
    );
    assert!(
        page.problems[0].ends_with("; the project runs on every default until it loads"),
        "{:?}",
        page.problems
    );
    assert!(page.files[0].found, "the file is there, and does not load");
    // What the file writes is not in force, so it is not shown as if it were.
    let name = setting(&page, "identity", "[project] name");
    assert_ne!(name.value, "Lamp");
    assert_eq!(name.source.said, "default");
    assert_eq!(setting(&page, "herdr", "[agents] panes").value, "none");
}

#[test]
fn a_project_file_that_is_no_toml_is_said_for_the_board_s_settings_too() {
    let tree = Tree::new();
    tree.write("symphony.toml", "[limits\nlow_volume = 2\n");
    let page = page(&tree.root, &bare());
    assert!(page.problems.len() >= 2, "{:?}", page.problems);
    assert!(
        page.problems
            .iter()
            .any(|p| p.contains("every limit takes its default")),
        "{:?}",
        page.problems
    );
    assert_eq!(setting(&page, "limits", "[limits] low_volume").value, "4");
    assert_eq!(
        setting(&page, "launch", "[agents] launch default").value,
        "codex"
    );
}

#[test]
fn an_agent_says_whether_this_machine_has_its_program() {
    let tree = Tree::new();
    tree.write("symphony.toml", PROJECT);
    let claude = tree.write("bin/claude", "#!/bin/sh\n");
    std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
    // A file that cannot run is no program.
    tree.write("bin/codex", "");
    let machine = Machine::path(Some(tree.path("bin").into_os_string()));
    let page = page(&tree.root, &machine);
    let claude = entry(&page, "claude");
    assert!(claude.needs[0].met);
    assert!(claude.needs[0].said.starts_with("Found on this machine: "));
    assert!(
        claude
            .state
            .as_ref()
            .unwrap()
            .why
            .starts_with("The launch default")
    );
    let codex = entry(&page, "codex");
    assert!(!codex.needs[0].met);
    assert_eq!(
        codex.needs[0].said,
        "Not found on this machine: a launch of it here would fail."
    );
    assert_eq!(codex.state.as_ref().unwrap().on, Some(true));
    assert!(
        codex
            .state
            .as_ref()
            .unwrap()
            .why
            .starts_with("A launch starts it when")
    );
    assert!(!entry(&page, "herdr").needs[0].met);
    assert!(!entry(&page, "github").needs[0].met);
}

#[test]
fn github_is_read_from_what_the_default_branch_commits_never_the_working_file() {
    let tree = Tree::new();
    git(&tree.root, &["init", "-q", "-b", "main"]);
    tree.write("a.txt", "a");
    git(&tree.root, &["add", "a.txt"]);
    git(&tree.root, &["commit", "-q", "-m", "a"]);
    let text = "# The exchange.\nenabled = true\nrepository = \"lamp/shop\"\nissues = \"import\"\n";
    tree.write("symphony-github.toml", text);
    let uncommitted = page(&tree.root, &bare());
    let github = entry(&uncommitted, "github");
    assert_eq!(
        github.state.as_ref().unwrap().on,
        Some(false),
        "a working file enables nothing"
    );
    git(&tree.root, &["add", "symphony-github.toml"]);
    git(&tree.root, &["commit", "-q", "-m", "on"]);
    let committed = page(&tree.root, &bare());
    let github = entry(&committed, "github");
    // The file is one its program refuses (enabled with no token file, import with no labels),
    // which runs as off: the page says what the file writes and claims the exchange on for none.
    let state = github.state.as_ref().unwrap();
    assert_eq!(
        (state.word.as_str(), state.on),
        ("Enabled in its file", None)
    );
    assert_eq!(
        state.why,
        "symphony-github.toml on main writes enabled = true; repository lamp/shop, issues import, \
         pull requests off, releases off. Whether the exchange runs is its program's to say: a \
         file it refuses, or a token it lacks, leaves it off (docs/operations/GITHUB_SETUP.md)."
    );
    assert!(!state.why.contains("On"), "{state:?}");
    assert_eq!(github.needs.len(), 1, "the page claims no token or App");
    let enabled = &github.settings[0];
    assert_eq!(enabled.class, SettingClass::Reviewed);
    assert_eq!(enabled.value, "on");
    assert_eq!(
        enabled.source.said,
        "symphony-github.toml on main, enabled, line 2"
    );
    assert_eq!(enabled.source.line, Some(2));
    // One committed that does not read is off, and says why.
    tree.write("symphony-github.toml", "enabled = \n");
    git(&tree.root, &["commit", "-q", "-am", "broken"]);
    let broken = page(&tree.root, &bare());
    let state = entry(&broken, "github").state.clone().unwrap();
    assert_eq!(state.on, Some(false));
    assert!(
        state
            .why
            .starts_with("Off: symphony-github.toml on main does not read ("),
        "{state:?}"
    );
}

/// The branch is the one the main checkout's HEAD commits in its project file, as the program
/// reads it: a working edit of the project file names none, and a file that leaves `enabled` out
/// or false is off.
#[test]
fn github_reads_the_branch_head_commits_and_a_file_that_does_not_enable_is_off() {
    let tree = Tree::new();
    git(&tree.root, &["init", "-q", "-b", "main"]);
    tree.write("symphony.toml", "[project]\ndefault_branch = \"trunk\"\n");
    tree.write(
        "symphony-github.toml",
        "enabled = true\nrepository = \"lamp/shop\"\n",
    );
    git(&tree.root, &["add", "."]);
    git(&tree.root, &["commit", "-q", "-m", "on main"]);
    // HEAD names trunk, which no branch is: nothing is committed there.
    let none = page(&tree.root, &bare());
    let state = entry(&none, "github").state.clone().unwrap();
    assert_eq!(state.on, Some(false));
    assert_eq!(state.why, "Off: no symphony-github.toml on trunk.");
    git(&tree.root, &["branch", "trunk"]);
    let trunk = page(&tree.root, &bare());
    let github = entry(&trunk, "github");
    assert_eq!(github.state.as_ref().unwrap().on, None);
    assert_eq!(
        github.settings[0].source.said,
        "symphony-github.toml on trunk, enabled, line 1"
    );
    // A working edit of the project file names no branch until it is committed.
    tree.write("symphony.toml", "[project]\ndefault_branch = \"main\"\n");
    let edited = page(&tree.root, &bare());
    assert!(
        entry(&edited, "github").settings[0]
            .source
            .said
            .contains("on trunk"),
    );
    git(&tree.root, &["checkout", "-q", "--", "symphony.toml"]);
    // Committed with the exchange not enabled: off, and said with what it writes.
    tree.write(
        "symphony-github.toml",
        "enabled = false\nrepository = \"lamp/shop\"\n",
    );
    git(&tree.root, &["commit", "-q", "-am", "off"]);
    git(&tree.root, &["branch", "-f", "trunk"]);
    let off = page(&tree.root, &bare());
    let state = entry(&off, "github").state.clone().unwrap();
    assert_eq!((state.word.as_str(), state.on), ("Off", Some(false)));
    assert_eq!(
        state.why,
        "Off: symphony-github.toml on trunk does not enable it; repository lamp/shop, issues \
         off, pull requests off, releases off."
    );
    // A project file HEAD commits that does not read names no branch: off, and said.
    tree.write("symphony.toml", "[project\n");
    git(&tree.root, &["commit", "-q", "-am", "broken"]);
    let broken = page(&tree.root, &bare());
    let state = entry(&broken, "github").state.clone().unwrap();
    assert_eq!(state.on, Some(false));
    assert!(
        state
            .why
            .starts_with("Off: symphony-github.toml does not read (symphony.toml at HEAD: "),
        "{state:?}"
    );
}

#[test]
fn plugins_and_documents_say_what_the_project_selects_and_serves() {
    let tree = Tree::new();
    tree.write(
        "symphony.toml",
        "[[entrypoints]]\nurl = \"/guide/\"\nfolder = \"guide\"\n",
    );
    tree.write("guide/a.md", "# A\n");
    let page = page(&tree.root, &bare());
    assert_eq!(page.problems, Vec::<String>::new());
    let served = setting(&page, "documents", "[[entrypoints]]");
    assert_eq!(served.value, "Serves guide/ at /guide.");
    assert_eq!(served.source.said, "symphony.toml [[entrypoints]], line 1");
    let plugins = entry(&page, "plugins");
    assert_eq!(plugins.state.as_ref().unwrap().word, "None selected");
    assert!(
        plugins
            .settings
            .iter()
            .all(|s| s.class == SettingClass::Reviewed)
    );
}

/// The top-level table a heading of the reference names, and the nested table's key when it
/// names one of a table's own (`[board.external_postgresql]` is `board`'s `external_postgresql`).
fn top(heading: &str) -> (&str, Option<&str>) {
    let inner = heading.trim_matches(|c| c == '[' || c == ']');
    let mut parts = inner.split('.');
    (parts.next().unwrap(), parts.next())
}

#[test]
fn every_key_of_the_reference_has_the_class_the_local_file_s_lists_give_it() {
    // What the reference says is never read from a local file, as holding a command or deciding
    // what lands: the expectation, written out, against the reader's own lists.
    let tables = ["plugins", "verdicts", "land", "releases", "stamps"];
    let keys = [
        ("project", "cargo"),
        ("project", "default_branch"),
        ("board", "on_review"),
        ("board", "on_land"),
        ("board", "machine_work_reader"),
        ("board", "postgresql"),
        ("board", "external_postgresql"),
        ("board", "assessments"),
    ];
    let mut reviewed = BTreeSet::new();
    let mut seen = 0;
    for (heading, key) in listed(&read(REFERENCE)) {
        let (table, nested) = top(&heading);
        // A nested table's keys take the class of the key that holds the table.
        let key = nested.unwrap_or(&key);
        let expected = tables.contains(&table) || keys.contains(&(table, key));
        let class = class(table, key);
        assert_eq!(
            class == SettingClass::Reviewed,
            expected,
            "{heading} {key} is {class:?}"
        );
        if expected {
            reviewed.insert((table.to_string(), key.to_string()));
        }
        seen += 1;
    }
    assert!(seen > 100, "the reference lists its keys: {seen}");
    for (table, key) in keys {
        assert!(
            reviewed.contains(&(table.to_string(), key.to_string())),
            "{table} {key}"
        );
    }
    // The workflow's tables are in neither list: the page shows them with no class.
    assert_eq!(class("workflow", "format"), SettingClass::Preference);
}

#[test]
fn every_setting_the_page_shows_has_the_class_and_the_scopes_of_its_key() {
    let tree = Tree::new();
    tree.write("symphony.toml", PROJECT);
    let page = page(&tree.root, &bare());
    let mut shown = BTreeSet::new();
    for setting in settings(&page) {
        let Some((table, key)) = setting
            .key
            .strip_prefix('[')
            .and_then(|k| k.trim_start_matches('[').split_once(']'))
        else {
            // GitHub's own file and the device's are no key of the project file.
            assert!(
                ["enabled", "appearance"].contains(&setting.key.as_str()),
                "{}",
                setting.key
            );
            continue;
        };
        let mut path: Vec<&str> = table.split('.').collect();
        let mut key = key.trim_start_matches(']').trim();
        // `[agents] launch model`, as `just tasks limits` spells it.
        if let Some(of) = key.strip_prefix("launch ") {
            path.push("launch");
            key = of;
        }
        assert_eq!(setting.class, class(path[0], key), "{}", setting.key);
        assert_eq!(setting.scopes, scopes(&path, key), "{}", setting.key);
        shown.insert((path[0].to_string(), key.to_string()));
        if key.is_empty() {
            shown.insert((path[0].to_string(), "*".to_string()));
        }
    }
    // Every key of a top-level table of the reference is on the page: by itself, or in its
    // table's sentence.
    let elsewhere = [
        // The cap is shown once, under `[limits]`, from whichever table writes it.
        ("board", "orchestrator_slots"),
        // The two tables of `[agents]`, shown key by key.
        ("agents", "launch"),
        ("agents", "models"),
        // The workflow is an entry with no setting.
        ("workflow", "*"),
        ("workflow_mappings", "*"),
        ("workflow_authorities", "*"),
    ];
    for (heading, key) in listed(&read(REFERENCE)) {
        let (table, nested) = top(&heading);
        let key = nested.unwrap_or(&key).to_string();
        let table = table.trim_matches(|c| c == '[' || c == ']');
        let whole = shown.contains(&(table.to_string(), "*".to_string()));
        let named = shown.contains(&(table.to_string(), key.clone()));
        let left = elsewhere
            .iter()
            .any(|(t, k)| *t == table && (*k == "*" || *k == key));
        assert!(whole || named || left, "{heading} {key} is not on the page");
    }
}

#[test]
fn only_what_a_local_file_may_hold_is_a_machine_s() {
    let both = [SettingScope::Project, SettingScope::Machine];
    assert_eq!(scopes(&["limits"], "machine_agents"), both);
    assert_eq!(scopes(&["agents", "launch"], "effort"), both);
    assert_eq!(scopes(&["agents", "models"], "assessor"), both);
    for (path, key) in [
        (&["agents"][..], "panes"),
        (&["board"][..], "on_land"),
        (&["limits"][..], "misspelt"),
        (&["agents", "models"][..], "writer"),
        (&["agents", "launch"][..], "panes"),
        (&[][..], "workflow"),
    ] {
        assert_eq!(scopes(path, key), [SettingScope::Project], "{path:?} {key}");
    }
}

/// A landing that checks, delivers by a command and names several generators is said whole, as
/// are the owner's saves and a catalogue of stamps the project writes.
#[test]
fn a_landing_with_a_check_generators_and_a_delivery_and_the_projects_stamps_are_said() {
    let tree = Tree::new();
    tree.write(
        "symphony.toml",
        r#"[verdicts]
check = ["just", "check"]
land = ["just", "land-generated"]
generators = [{ name = "index", outputs = "out/", command = ["make", "index"] }]

[land]
push = false
delivery = ["tools/deliver", "--hosted"]
generators = [
  { name = "client", outputs = "dist/", command = ["make", "client"] },
  { name = "site", outputs = "site/", command = ["make", "site"] },
]

[stamps.tests]
[stamps.review]
"#,
    );
    let page = page(&tree.root, &bare());
    assert_eq!(page.problems, [""; 0]);
    let land = setting(&page, "landing", "[land]");
    assert_eq!(
        land.value,
        "Runs the check; does not push main; 2 generators, client, site; delivered by `tools/deliver --hosted`."
    );
    assert_eq!(land.source.said, "symphony.toml [land], line 6");
    let verdicts = setting(&page, "landing", "[verdicts]");
    assert_eq!(
        verdicts.value,
        "A generated branch is checked by `just check`; one generator, index."
    );
    assert_eq!(verdicts.source.said, "symphony.toml [verdicts], line 1");
    let stamps = setting(&page, "stamps", "[stamps]");
    assert_eq!(stamps.value, "The project's: tests, review.");
    assert_eq!(stamps.class, SettingClass::Reviewed);
}
