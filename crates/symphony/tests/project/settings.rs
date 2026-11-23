//! The Settings page's read (`symphony::settings::page`): each value in force with where it was
//! written, for a project with no file, with a project file, the board's store over it and a
//! local file over both, and a project file or a store that does not load said as such; the
//! class of every key of the reference, which the read takes from the local settings file's
//! lists; and every key of the reference on the page, and no other.

use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;

use super::reference::{REFERENCE, listed, read};
use crate::Tree;
use crate::board::HELD_FLOW;
use serde_json::json;
use symphony::commit::pending::{Save, State};
use symphony::settings::described::GITHUB;
use symphony::settings::stored::Board;
use symphony::settings::{Machine, class, page, page_with, scopes};
use symphony_api::{Setting, SettingClass, SettingKind, SettingScope, SettingsEntry, SettingsPage};
use symphony_board::settings::Entry;

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
    // Its state is its program's to say, and this machine has none.
    let github = entry(&page, "github");
    let state = github.state.as_ref().unwrap();
    assert_eq!((state.word.as_str(), state.on), ("Unknown", None));
    assert_eq!(
        state.why,
        "Unknown: its state is its program's to say, and this machine has no symphony-github: \
         nothing is exchanged from here."
    );
    assert!(
        github.settings.is_empty(),
        "the page reads no file of its own"
    );
    let workflow = setting(&page, "workflow", "[workflow]");
    assert_eq!(workflow.value, "The board's own, built-in v1.");
    assert_eq!(workflow.source.said, "default: no [workflow] written");
    assert_eq!(workflow.class, SettingClass::Reviewed);
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

/// A board whose store holds each of `entries`, a key and its value, and nothing waits.
fn holding(entries: &[(&str, serde_json::Value)]) -> Board {
    Board {
        entries: Ok(entries
            .iter()
            .map(|(key, value)| Entry {
                key: (*key).into(),
                value: Some(value.clone()),
                revision: 1,
                changes: Vec::new(),
            })
            .collect()),
        waiting: Vec::new(),
    }
}

/// A setting's value, its source's scope, where it is said and what it stands over.
fn from(setting: &Setting) -> (&str, Option<SettingScope>, &str, Option<&str>) {
    let source = &setting.source;
    (
        &setting.value,
        source.scope,
        &source.said,
        source.over.as_deref(),
    )
}

#[test]
fn the_board_s_store_holds_over_the_project_file_and_the_local_file_over_the_store() {
    let tree = Tree::new();
    tree.write("symphony.toml", PROJECT);
    tree.write(
        "symphony.local.toml",
        "[agents.models]\nexecutor = \"fable\"\n\n[limits]\nlow_volume = 1\n",
    );
    let board = holding(&[
        ("agents.panes", json!("none")),
        ("agents.launch.model", json!("sonnet")),
        ("agents.launch.effort", json!("high")),
        ("agents.models.reviewer", json!("fable")),
        ("agents.models.executor", json!("opus")),
        ("limits.low_volume", json!(3)),
        ("limits.machine_agents", json!(2)),
    ]);
    let page = page_with(&tree.root, &bare(), &board);
    assert_eq!(page.problems, Vec::<String>::new());
    let store = Some(SettingScope::Board);
    // The store's value, named as its own, over the committed one and where it is written.
    assert_eq!(
        from(setting(&page, "herdr", "[agents] panes")),
        (
            "none",
            store,
            "the board's store [agents] panes",
            Some("\"hc\" (symphony.toml [agents] panes, line 5)")
        )
    );
    // Herdr's state follows the panes in force.
    assert_eq!(
        entry(&page, "herdr").state.as_ref().unwrap().on,
        Some(false)
    );
    assert_eq!(
        from(setting(&page, "launch", "[agents] launch model")),
        (
            "sonnet",
            store,
            "the board's store [agents] launch model",
            Some("\"opus\" (symphony.toml [agents] launch model, line 6)")
        )
    );
    // One the project file does not write stands over none.
    assert_eq!(
        from(setting(&page, "launch", "[agents] launch effort")),
        (
            "high",
            store,
            "the board's store [agents] launch effort",
            Some("none written")
        )
    );
    assert_eq!(
        from(setting(&page, "models", "[agents.models] reviewer")),
        (
            "fable",
            store,
            "the board's store [agents.models] reviewer",
            Some("\"opus\" (symphony.toml [agents.models] reviewer, line 9)")
        )
    );
    assert_eq!(
        from(setting(&page, "limits", "[limits] machine_agents")),
        (
            "2",
            store,
            "the board's store [limits] machine_agents",
            Some("its default, no limit")
        )
    );
    // The local file's value holds over the store's, which it names.
    let machine = Some(SettingScope::Machine);
    assert_eq!(
        from(setting(&page, "models", "[agents.models] executor")),
        (
            "fable",
            machine,
            "symphony.local.toml [agents.models] executor, line 2",
            Some("\"opus\" (the board's store [agents.models] executor)")
        )
    );
    assert_eq!(
        from(setting(&page, "limits", "[limits] low_volume")),
        (
            "1",
            machine,
            "symphony.local.toml [limits] low_volume, line 5",
            Some("3 (the board's store [limits] low_volume)")
        )
    );
    // What the page writes: each setting the store keeps, in force or under the local file's.
    let held: Vec<(&str, Option<&str>, bool)> = (page.stored.iter())
        .map(|item| (item.key.as_str(), item.value.as_deref(), item.held))
        .collect();
    for expected in [
        ("agents.panes", Some("none"), true),
        ("agents.launch.model", Some("sonnet"), true),
        ("agents.launch.default", None, false),
        ("agents.models.executor", Some("opus"), false),
        ("limits.low_volume", Some("3"), false),
        ("limits.orchestrator_slots", None, false),
    ] {
        assert!(held.contains(&expected), "{expected:?} in {held:?}");
    }
    // A setting the store does not keep is not written from the page.
    assert!(
        page.stored
            .iter()
            .all(|item| !item.key.starts_with("land."))
    );
    assert!(page.stored.iter().all(|item| item.key != "project.name"));
}

#[test]
fn a_store_that_cannot_be_read_or_an_entry_it_refuses_is_said_and_the_files_hold() {
    let tree = Tree::new();
    tree.write("symphony.toml", PROJECT);
    let down = Board {
        entries: Err("connection refused".into()),
        waiting: Vec::new(),
    };
    let page = page_with(&tree.root, &bare(), &down);
    assert_eq!(
        page.problems,
        [
            "the board's store cannot be read (connection refused); none of it is read, the \
          files' settings hold"
        ]
    );
    let panes = setting(&page, "herdr", "[agents] panes");
    assert_eq!(
        (panes.value.as_str(), panes.source.scope),
        ("hc", Some(SettingScope::Project))
    );
    // An entry the store does not read is said, and the file's value holds.
    let refused = holding(&[
        ("agents.panes", json!("tmux")),
        ("limits.low_volume", json!(3)),
    ]);
    let page = page_with(&tree.root, &bare(), &refused);
    assert_eq!(page.problems.len(), 1, "{:?}", page.problems);
    assert!(page.problems[0].contains("tmux"), "{:?}", page.problems);
    let panes = setting(&page, "herdr", "[agents] panes");
    assert_eq!(
        (panes.value.as_str(), panes.source.scope),
        ("hc", Some(SettingScope::Project))
    );
    let item = page
        .stored
        .iter()
        .find(|item| item.key == "agents.panes")
        .unwrap();
    assert!(!item.held);
    assert!(
        item.refused
            .as_deref()
            .is_some_and(|why| why.contains("tmux")),
        "{item:?}"
    );
    let low = setting(&page, "limits", "[limits] low_volume");
    assert_eq!(
        (low.value.as_str(), low.source.scope),
        ("3", Some(SettingScope::Board))
    );
}

#[test]
fn a_commit_waiting_for_its_delivery_is_said_beside_its_setting() {
    let tree = Tree::new();
    tree.write("symphony.toml", PROJECT);
    let board = Board {
        waiting: vec![Save {
            sequence: 2,
            file: "symphony.toml".into(),
            branch: "docs/owner-save-2".into(),
            shown: true,
            state: State::Held("its task was cancelled".into()),
            by: String::new(),
            keys: vec!["limits.low_volume".into()],
        }],
        ..holding(&[("limits.low_volume", json!(3))])
    };
    let page = page_with(&tree.root, &bare(), &board);
    let item = page
        .stored
        .iter()
        .find(|item| item.key == "limits.low_volume")
        .unwrap();
    assert_eq!(
        item.waiting.as_deref(),
        Some("docs/owner-save-2: kept, not delivered: its task was cancelled")
    );
    let other = page
        .stored
        .iter()
        .find(|item| item.key == "limits.machine_agents")
        .unwrap();
    assert_eq!(other.waiting, None);
}

/// `[agents] launch` written in both files: the local file's agent and model hold over the
/// committed ones and name them, and an effort it writes empty is none, so the default holds.
#[test]
fn a_local_launch_holds_over_the_committed_one_key_by_key() {
    let tree = Tree::new();
    tree.write(
        "symphony.toml",
        "[agents]\nlaunch = { default = \"claude\", model = \"opus\", effort = \"high\" }\n",
    );
    tree.write(
        "symphony.local.toml",
        "[agents.launch]\ndefault = \"codex\"\nmodel = \"gpt-x\"\neffort = \"\"\n",
    );
    let page = page(&tree.root, &bare());
    assert_eq!(page.problems, Vec::<String>::new());
    for (key, value, line) in [("default", "codex", 2), ("model", "gpt-x", 3)] {
        let launch = setting(&page, "launch", &format!("[agents] launch {key}"));
        assert_eq!(launch.value, value, "{key}");
        assert_eq!(launch.source.scope, Some(SettingScope::Machine), "{key}");
        assert_eq!(
            launch.source.said,
            format!("symphony.local.toml [agents] launch {key}, line {line}")
        );
        assert_eq!(launch.source.line, Some(line));
        assert_eq!(
            launch.source.over,
            Some(format!("symphony.toml [agents] launch {key}, line 2"))
        );
        assert_eq!(
            launch.scopes,
            [
                SettingScope::Project,
                SettingScope::Board,
                SettingScope::Machine
            ]
        );
    }
    let effort = setting(&page, "launch", "[agents] launch effort");
    assert_eq!(
        (effort.value.as_str(), effort.source.said.as_str()),
        ("xhigh", "default: the board's own for codex")
    );
    assert_eq!((effort.source.scope, &effort.source.over), (None, &None));
    assert_eq!(
        entry(&page, "launch").about,
        "A launch whose caller names nothing starts codex on gpt-x (effort xhigh)."
    );
    // A role with no model of its own runs on the launch's, the local file's.
    let executor = setting(&page, "models", "[agents.models] executor");
    assert_eq!(
        (executor.value.as_str(), executor.source.said.as_str()),
        ("gpt-x", "default: [agents] launch model")
    );
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

/// A machine whose search path is `bin` of `tree`, holding a `symphony-github` that runs
/// `script` for `describe` and refuses any other words.
fn github(tree: &Tree, script: &str) -> Machine {
    let program = tree.write(
        "bin/symphony-github",
        &format!("#!/bin/sh\n[ \"$*\" = describe ] || exit 2\n{script}\n"),
    );
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    Machine::path(Some(tree.path("bin").into_os_string()))
}

/// What a program answering for the exchange prints: its entry, its id another than the page's,
/// its state saying the folder it ran in, a need of its own and a setting it calls a preference of
/// the machine's.
const ANSWER: &str = r#"printf '{"id":"elsewhere","name":"GitHub","about":"Its own words.","state":{"word":"On","on":true,"why":"Enabled, run in %s."},"needs":[{"what":"the token file","met":false,"said":"Not found."}],"settings":[{"key":"enabled","name":"Whether it runs","kind":"switch","value":"on","source":{"scope":"project","said":"symphony-github.toml on main enabled, line 1","file":"symphony-github.toml","line":1},"scopes":["project","machine"],"class":"preference"}]}\n' "$(pwd -P)""#;

#[test]
fn github_is_its_programs_own_answer_run_in_the_main_checkout_and_judged() {
    let tree = Tree::new();
    tree.write("symphony.toml", PROJECT);
    let machine = github(&tree, ANSWER);
    let page = page(&tree.root, &machine);
    let github = entry(&page, "github");
    assert_eq!(github.name, "GitHub");
    assert_eq!(github.about, "Its own words.");
    let state = github.state.as_ref().unwrap();
    assert_eq!((state.word.as_str(), state.on), ("On", Some(true)));
    assert_eq!(
        state.why,
        format!(
            "Enabled, run in {}.",
            tree.root.canonicalize().unwrap().display()
        )
    );
    // The program the page ran is its first need, then the ones it says.
    let needs: Vec<(&str, bool)> = (github.needs.iter())
        .map(|need| (need.what.as_str(), need.met))
        .collect();
    assert_eq!(
        needs,
        [
            ("the symphony-github program", true),
            ("the token file", false)
        ]
    );
    assert_eq!(
        github.needs[0].said,
        format!(
            "Found on this machine: {}.",
            tree.path("bin/symphony-github").display()
        )
    );
    // Its settings decide what lands: reviewed, whatever it says, and the project's alone.
    let [enabled] = github.settings.as_slice() else {
        panic!("{:?}", github.settings);
    };
    assert_eq!(enabled.class, SettingClass::Reviewed);
    assert_eq!(enabled.scopes, [SettingScope::Project]);
    assert_eq!(enabled.value, "on");
    assert_eq!(enabled.source.line, Some(1));
}

#[test]
fn github_absent_failing_or_answering_what_does_not_read_is_said_and_the_page_still_drawn() {
    let tree = Tree::new();
    tree.write("symphony.toml", PROJECT);
    let said = |machine: &Machine| {
        let page = page(&tree.root, machine);
        assert_eq!(page.sections.len(), 5, "every section is drawn");
        let github = entry(&page, "github").clone();
        assert_eq!(github.needs[0].what, "the symphony-github program");
        let state = github.state.unwrap();
        assert_eq!((state.word.as_str(), state.on), ("Unknown", None));
        (github.needs[0].met, state.why)
    };
    let (met, why) = said(&bare());
    assert!(!met);
    assert!(why.contains("this machine has no symphony-github"), "{why}");
    let failing = github(&tree, "echo half; echo 'no settings here' >&2; exit 3");
    assert_eq!(
        said(&failing),
        (
            true,
            "Unknown: its program could not say: symphony-github describe exited 3: no settings \
             here."
                .into()
        )
    );
    let garbled = github(&tree, "echo 'not an entry'");
    let (met, why) = said(&garbled);
    assert!(met);
    assert!(
        why.starts_with(
            "Unknown: its program could not say: symphony-github describe printed what does not \
             read as an entry: "
        ),
        "{why}"
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

/// A project's own workflow is said as one, with the line of its table, and with its mappings and
/// its approval authorities when it writes them; all three tables are reviewed.
#[test]
fn a_custom_workflow_is_said_with_its_line_mappings_and_authorities() {
    let mapping = "\n[[workflow_mappings]]\nfrom = { phase = \"ready\", state = \"available\" }\n\
                   to = { phase = \"build\", state = \"available\" }\n";
    let tree = Tree::new();
    tree.write("symphony.toml", &format!("{HELD_FLOW}{mapping}"));
    let custom = page(&tree.root, &bare());
    assert_eq!(custom.problems, Vec::<String>::new());
    let workflow = setting(&custom, "workflow", "[workflow]");
    assert_eq!(
        workflow.value,
        "The project's own, with mappings, which the board activates once the default branch \
         commits it."
    );
    assert_eq!(workflow.kind, SettingKind::Sentence);
    assert_eq!(workflow.class, SettingClass::Reviewed);
    assert_eq!(workflow.scopes, [SettingScope::Project]);
    assert_eq!(workflow.source.said, "symphony.toml [workflow], line 2");
    assert_eq!(workflow.source.line, Some(2));
    // Guarded, with the authority that attests its guard committed for the project.
    let flow = HELD_FLOW
        .replacen("adapters = []", "adapters = [\"approvals\"]", 1)
        .replacen(
            "guards = []",
            "guards = [{ id = \"approved\", adapter = \"approvals\" }]",
            1,
        )
        .replacen(
            "dependencies = \"none\", guards = []",
            "dependencies = \"none\", guards = [\"approved\"]",
            1,
        );
    let authority = "\n[[workflow_authorities]]\nadapter = \"approvals\"\nissuer = \"ledger\"\n\
                     contract = 1\nkey = \"ed25519:\
                     5866666666666666666666666666666666666666666666666666666666666666\"\n";
    tree.write(
        "symphony.toml",
        &format!("[project]\nid = \"demo\"\n{flow}{mapping}{authority}"),
    );
    let guarded = page(&tree.root, &bare());
    assert_eq!(guarded.problems, Vec::<String>::new());
    let workflow = setting(&guarded, "workflow", "[workflow]");
    assert_eq!(
        workflow.value,
        "The project's own, with mappings and approval authorities, which the board activates \
         once the default branch commits it."
    );
    assert_eq!(workflow.source.said, "symphony.toml [workflow], line 4");
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
    let tables = [
        "plugins",
        "verdicts",
        "land",
        "releases",
        "stamps",
        "workflow",
        "workflow_mappings",
        "workflow_authorities",
    ];
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
    // Each of the workflow's three tables is listed with its keys, and each is reviewed.
    for table in &tables[5..] {
        assert!(
            reviewed.iter().any(|(listed, _)| listed == table),
            "{table}"
        );
    }
}

/// Whether `entry` is one the Symphony says itself, not a describer's (`settings::described`): the
/// GitHub exchange's, or a plugin's (`plugin-NAME-ID`).
fn symphony_s(entry: &SettingsEntry) -> bool {
    entry.id != GITHUB.id && !entry.id.starts_with("plugin-")
}

/// The table a setting of the page is written in, as the path to it, and its key there, empty for
/// a table said whole (`[stamps]`, `[[entrypoints]]`); none for the device's, the one setting no
/// file writes. `[agents] launch model` is `[agents.launch]`'s `model`, as `just tasks limits`
/// spells it.
fn written_as(setting: &Setting) -> Option<(Vec<&str>, &str)> {
    let Some((table, key)) = setting
        .key
        .strip_prefix('[')
        .and_then(|k| k.trim_start_matches('[').split_once(']'))
    else {
        assert_eq!(setting.key, "appearance", "no key of the project file");
        return None;
    };
    let mut path: Vec<&str> = table.split('.').collect();
    let mut key = key.trim_start_matches(']').trim();
    if let Some(of) = key.strip_prefix("launch ") {
        path.push("launch");
        key = of;
    }
    Some((path, key))
}

#[test]
fn every_setting_the_page_shows_has_the_class_and_the_scopes_of_its_key() {
    let tree = Tree::new();
    tree.write("symphony.toml", PROJECT);
    let page = page(&tree.root, &bare());
    let mut shown = BTreeSet::new();
    for setting in settings(&page) {
        let Some((path, key)) = written_as(setting) else {
            continue;
        };
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
        // The workflow's mappings and authorities are said in its sentence.
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
fn every_setting_the_page_shows_is_one_the_project_file_reads() {
    // The reference lists every key the project file reads and no other (`reference.rs`), so a
    // key it does not list, a removed one among them, is one the file refuses: a row of it would
    // show a setting no file can write. The Symphony's own entries alone: a describer's are its
    // program's to name, so they are left out here.
    let tree = Tree::new();
    tree.write("symphony.toml", PROJECT);
    let page = page(&tree.root, &bare());
    let reference = listed(&read(REFERENCE));
    let table = |heading: &str| heading.trim_matches(|c| c == '[' || c == ']').to_string();
    let keys: BTreeSet<(String, &str)> = reference
        .iter()
        .map(|(heading, key)| (table(heading), key.as_str()))
        .collect();
    let (own, described): (Vec<&SettingsEntry>, Vec<_>) = (page.sections.iter())
        .flat_map(|section| &section.entries)
        .partition(|entry| symphony_s(entry));
    // The GitHub exchange's entry is on every page, said in its program's place on this bare
    // machine: what is left out is an entry the page has.
    let ids: Vec<&str> = described.iter().map(|entry| entry.id.as_str()).collect();
    assert_eq!(ids, [GITHUB.id]);
    let mut seen = 0;
    for setting in own.iter().flat_map(|entry| &entry.settings) {
        let Some((path, key)) = written_as(setting) else {
            continue;
        };
        let path = path.join(".");
        let known = if key.is_empty() {
            keys.iter().any(|(listed, _)| *listed == path)
        } else {
            keys.contains(&(path, key))
        };
        assert!(
            known,
            "{} is no setting the project file reads",
            setting.key
        );
        seen += 1;
    }
    assert!(seen > 50, "the page shows its settings: {seen}");
}

#[test]
fn only_what_a_local_file_may_hold_is_a_machine_s_and_the_board_keeps_panes_besides() {
    let all = [
        SettingScope::Project,
        SettingScope::Board,
        SettingScope::Machine,
    ];
    assert_eq!(scopes(&["limits"], "machine_agents"), all);
    assert_eq!(scopes(&["agents", "launch"], "effort"), all);
    assert_eq!(scopes(&["agents", "models"], "assessor"), all);
    // The board's store keeps Herdr's panes, which no local file writes.
    assert_eq!(
        scopes(&["agents"], "panes"),
        [SettingScope::Project, SettingScope::Board]
    );
    for (path, key) in [
        (&["agents"][..], "worktrees"),
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
