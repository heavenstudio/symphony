use super::{Board, list, local_writes, shown, store_key};
use crate::commit::pending::{Save, State};
use serde_json::json;
use symphony_api::{
    Setting, SettingClass, SettingKind, SettingScope, SettingSource, SettingsEntry,
    SettingsSection, StoredChange,
};
use symphony_board::settings::{Change, Entry};

#[test]
fn a_settings_name_on_the_page_is_its_key_in_the_store() {
    for (at, key) in [
        ("[agents] launch model", "agents.launch.model"),
        ("[agents] launch default", "agents.launch.default"),
        ("[agents.models] reviewer", "agents.models.reviewer"),
        ("[agents] panes", "agents.panes"),
        ("[limits] low_volume", "limits.low_volume"),
    ] {
        assert_eq!(store_key(at).as_deref(), Some(key), "{at}");
    }
    // A table said whole, a device's setting and a list of tables name no key.
    for at in ["[land]", "appearance", "[[entrypoints]]", "[limits]", ""] {
        assert_eq!(store_key(at), None, "{at}");
    }
}

#[test]
fn a_stored_value_is_shown_as_the_page_shows_a_value() {
    assert_eq!(shown(&json!("opus")), "opus");
    assert_eq!(shown(&json!(3)), "3");
    assert_eq!(shown(&json!(true)), "on");
    assert_eq!(shown(&json!(false)), "off");
}

/// A local file writing `text`, as its reader reads it.
fn local(text: &str) -> symphony_board::local::Local {
    symphony_board::local::parse(std::path::Path::new("symphony.local.toml"), text).unwrap()
}

#[test]
fn the_local_file_stands_over_a_stored_key_it_writes_and_no_other() {
    let wrote = local(
        "[agents]\nlaunch = { model = 'm', effort = ' ' }\nmodels = { reviewer = 'r' }\n\n[limits]\nlow_volume = 1\n",
    );
    for (key, over) in [
        ("agents.launch.model", true),
        // Written empty is none.
        ("agents.launch.effort", false),
        ("agents.launch.default", false),
        ("agents.models.reviewer", true),
        ("agents.models.executor", false),
        ("limits.low_volume", true),
        ("limits.machine_agents", false),
        // The local file holds no panes setting.
        ("agents.panes", false),
        ("land.check", false),
    ] {
        assert_eq!(local_writes(&wrote, key), over, "{key}");
    }
}

fn setting(key: &str, scopes: Vec<SettingScope>) -> Setting {
    Setting {
        key: key.into(),
        name: key.into(),
        kind: SettingKind::Text,
        value: String::new(),
        choices: Vec::new(),
        source: SettingSource {
            scope: None,
            said: "default".into(),
            file: None,
            line: None,
            over: None,
        },
        scopes,
        class: SettingClass::Preference,
    }
}

fn sections(settings: Vec<Setting>) -> Vec<SettingsSection> {
    vec![SettingsSection {
        id: "models".into(),
        title: "Models".into(),
        about: String::new(),
        entries: vec![SettingsEntry {
            id: "models".into(),
            name: "Models".into(),
            about: String::new(),
            state: None,
            needs: Vec::new(),
            settings,
            source: None,
        }],
    }]
}

fn entry(key: &str, value: Option<serde_json::Value>, changes: Vec<Change>) -> Entry {
    Entry {
        key: key.into(),
        value,
        revision: changes.len() as u64,
        changes,
    }
}

#[test]
fn the_list_holds_each_setting_the_store_keeps_with_its_entry_and_no_other() {
    use SettingScope::{Board as Stored, Machine, Project};
    let drawn = sections(vec![
        setting("[agents.models] reviewer", vec![Project, Stored, Machine]),
        setting("[agents] panes", vec![Project, Stored]),
        setting("[limits] low_volume", vec![Project, Stored, Machine]),
        setting("[limits] machine_agents", vec![Project, Stored, Machine]),
        // Shown, never written: no scope of the store's.
        setting("[project] name", vec![Project]),
        setting("[land]", vec![Project]),
    ]);
    let change = |before, after, by: &str| Change {
        before,
        after,
        at: 0.0,
        by: by.into(),
    };
    let board = Board {
        entries: Ok(vec![
            entry(
                "agents.models.reviewer",
                Some(json!("opus")),
                vec![change(None, Some(json!("opus")), "owner")],
            ),
            // One the store refuses, however it came to be there.
            entry("agents.panes", Some(json!("tmux")), Vec::new()),
            entry(
                "limits.low_volume",
                None,
                vec![
                    change(None, Some(json!(3)), "owner"),
                    change(Some(json!(3)), None, "owner"),
                ],
            ),
            // An entry for a setting the page draws no control for is not listed.
            entry("project.name", Some(json!("x")), Vec::new()),
        ]),
        waiting: vec![Save {
            sequence: 4,
            file: "symphony.toml".into(),
            branch: "docs/owner-save-4".into(),
            shown: true,
            state: State::Waiting,
            keys: vec!["agents.models.reviewer".into()],
        }],
        running: None,
    };
    let over = local("[agents.models]\nreviewer = 'r'\n");
    let listed = list(&drawn, &board, Some(&over), "main");
    let keys: Vec<&str> = listed.iter().map(|one| one.key.as_str()).collect();
    assert_eq!(
        keys,
        [
            "agents.models.reviewer",
            "agents.panes",
            "limits.low_volume",
            "limits.machine_agents"
        ]
    );
    let reviewer = &listed[0];
    assert_eq!(reviewer.setting, "[agents.models] reviewer");
    assert_eq!(reviewer.value.as_deref(), Some("opus"));
    assert!(!reviewer.held, "the local file stands over it");
    assert_eq!(
        reviewer.waiting.as_deref(),
        Some("docs/owner-save-4: waiting for its pull request")
    );
    assert_eq!(
        reviewer.changes,
        [StoredChange {
            before: None,
            after: Some("opus".into()),
            at: symphony_board::sys::local_minute(0.0),
            by: "owner".into(),
        }]
    );
    let panes = &listed[1];
    assert!(!panes.held);
    assert!(
        panes
            .refused
            .as_deref()
            .is_some_and(|why| why.contains("tmux")),
        "{panes:?}"
    );
    let removed = &listed[2];
    assert_eq!((removed.value.clone(), removed.held), (None, false));
    assert_eq!(removed.changes.len(), 2);
    assert_eq!(removed.changes[1].after, None);
    let never = &listed[3];
    assert_eq!(
        (never.value.clone(), never.held, never.changes.len()),
        (None, false, 0)
    );
    // With no local file the reviewer's stored value is the one in force.
    let held = list(&drawn, &board, None, "main");
    assert!(held[0].held);
}

#[test]
fn a_store_that_cannot_be_read_lists_each_setting_with_no_entry_and_says_why() {
    let drawn = sections(vec![setting(
        "[limits] low_volume",
        vec![SettingScope::Project, SettingScope::Board],
    )]);
    let board = Board {
        entries: Err("connection refused".into()),
        waiting: Vec::new(),
        running: None,
    };
    let listed = list(&drawn, &board, None, "main");
    assert_eq!(listed.len(), 1);
    assert_eq!((listed[0].value.clone(), listed[0].held), (None, false));
    let stored = board.stored();
    assert!(stored.is_empty());
    assert!(
        stored.problems[0].contains("connection refused"),
        "{stored:?}"
    );
    // A board never told anything holds nothing, and says nothing.
    assert_eq!(
        Board::default().stored(),
        symphony_board::settings::Stored::default()
    );
}
