//! The class a description may not lower ([`protected`], [`judged`]), the scope it may not take
//! or give as its value's source, the anchors of the plugins' entries ([`plugins`]), and the run
//! of a program's command ([`run`]): what it printed, or why not, with its time bounded.

use std::path::Path;
use std::time::Duration;

use symphony_api::{
    Setting, SettingClass, SettingKind, SettingScope, SettingSource, SettingsEntry,
    SettingsSection,
};
use symphony_board::sys;
use symphony_plugin::{Describer, Plugin};

use super::{judged, plugins, protected, run};
use crate::plugins::Plugins;
use crate::settings::{class, stored};

#[test]
fn a_key_the_local_file_refuses_is_protected_however_it_is_spelt() {
    for key in [
        "[land] delivery",
        "[land]",
        " [ land ] delivery ",
        "land.delivery",
        "[plugins.process] every",
        "[plugins] crates",
        "[stamps] needed",
        "[releases.github] command",
        "[board] on_land",
        "board.on_review",
        "[project] default_branch",
        "[project] cargo",
        // A table said whole is protected where it holds a key that is.
        "[project]",
        "[board]",
        // A table below a key refused by name takes that key's class, said whole or not.
        "[project.cargo] wrapper",
        "[board.on_land]",
        "project.cargo.wrapper",
        // A list of tables, as the page spells one, takes its table's class.
        "[[workflow_mappings]] guard",
        "[[workflow_authorities]]",
        " [[ workflow_mappings ]] guard ",
    ] {
        assert!(protected(key), "{key}");
    }
    for key in [
        "[limits] low_volume",
        "[agents] panes",
        "[agents.launch] model",
        "agents.launch.model",
        "[agents] launch model",
        "[project] name",
        "[board] port",
        // Not `[board]`'s own key, but one of a table below it, of the key `x`, said whole or not.
        "[board.x] on_land",
        "[board.x]",
        "[[entrypoints]] route",
        "[agents]",
        "[every] shade",
        "enabled",
        "",
        "[",
        "[[",
    ] {
        assert!(!protected(key), "{key}");
    }
}

#[test]
fn a_table_said_whole_is_reviewed_where_it_holds_a_reviewed_key() {
    for table in ["land", "plugins", "workflow", "project", "board"] {
        assert_eq!(class(table, ""), SettingClass::Reviewed, "{table}");
    }
    for table in ["limits", "agents", "theme", ""] {
        assert_eq!(class(table, ""), SettingClass::Preference, "{table}");
    }
    // A key of its own keeps its class, said whole or not.
    assert_eq!(class("project", "name"), SettingClass::Preference);
    assert_eq!(class("project", "cargo"), SettingClass::Reviewed);
}

fn setting(key: &str, class: SettingClass) -> Setting {
    Setting {
        key: key.into(),
        name: key.into(),
        kind: SettingKind::Text,
        value: "v".into(),
        choices: Vec::new(),
        source: SettingSource {
            scope: Some(SettingScope::Project),
            said: "a.toml, line 1".into(),
            file: Some("a.toml".into()),
            line: Some(1),
            over: None,
        },
        scopes: vec![SettingScope::Project, SettingScope::Machine],
        class,
    }
}

fn described(settings: Vec<Setting>) -> SettingsEntry {
    SettingsEntry {
        id: "its-own".into(),
        name: "A".into(),
        about: "B".into(),
        state: None,
        needs: Vec::new(),
        settings,
        source: None,
    }
}

#[test]
fn a_description_keeps_what_it_may_say_and_its_id_is_the_hosts() {
    let mine = setting("[every] shade", SettingClass::Preference);
    let raised = setting("[every] command", SettingClass::Reviewed);
    let judged = judged(
        described(vec![mine.clone(), raised.clone()]),
        "plugin-every-a".into(),
        false,
    );
    assert_eq!(judged.id, "plugin-every-a");
    assert_eq!(
        judged.settings,
        [mine, raised],
        "a describer may raise its own"
    );
}

#[test]
fn a_protected_key_called_a_preference_is_reviewed_in_the_projects_scope_alone() {
    let judged = judged(
        described(vec![setting("[land] delivery", SettingClass::Preference)]),
        "a".into(),
        false,
    );
    let [delivery] = judged.settings.as_slice() else {
        panic!("{judged:?}");
    };
    assert_eq!(delivery.class, SettingClass::Reviewed);
    assert_eq!(delivery.scopes, [SettingScope::Project]);
    assert_eq!(delivery.value, "v", "nothing else of it changes");
}

#[test]
fn no_description_is_written_in_the_boards_store_whatever_its_describer_says() {
    let scopes = vec![
        SettingScope::Project,
        SettingScope::Board,
        SettingScope::Machine,
    ];
    let shade = Setting {
        scopes: scopes.clone(),
        ..setting("[every] shade", SettingClass::Preference)
    };
    // The store keeps this key, from the Symphony's own entry, never from a description.
    let limit = Setting {
        scopes: scopes.clone(),
        ..setting("[limits] low_volume", SettingClass::Preference)
    };
    let delivery = Setting {
        scopes,
        ..setting("[land] delivery", SettingClass::Preference)
    };
    let judged = judged(described(vec![shade, limit, delivery]), "a".into(), false);
    let scopes: Vec<_> = (judged.settings.iter()).map(|s| s.scopes.clone()).collect();
    assert_eq!(
        scopes,
        [
            vec![SettingScope::Project, SettingScope::Machine],
            vec![SettingScope::Project, SettingScope::Machine],
            vec![SettingScope::Project],
        ]
    );
}

#[test]
fn no_description_says_its_value_comes_from_the_boards_store_whatever_its_describer_says() {
    let from = |key: &str, scope: Option<SettingScope>| {
        let mut one = setting(key, SettingClass::Preference);
        one.source.scope = scope;
        one
    };
    let settings = vec![
        from("[every] shade", Some(SettingScope::Board)),
        // Reviewed, its source is judged all the same.
        from("[land] delivery", Some(SettingScope::Board)),
        from("[every] tint", Some(SettingScope::Machine)),
        from("[every] hue", Some(SettingScope::Project)),
        from("[every] tone", None),
    ];
    for all in [false, true] {
        let judged = judged(described(settings.clone()), "a".into(), all);
        let sources: Vec<_> = (judged.settings.iter()).map(|s| s.source.scope).collect();
        assert_eq!(
            sources,
            [
                None,
                None,
                Some(SettingScope::Machine),
                Some(SettingScope::Project),
                None
            ],
            "every setting reviewed: {all}"
        );
        let shade = &judged.settings[0].source;
        assert_eq!(
            (shade.said.as_str(), shade.file.as_deref(), shade.line),
            ("a.toml, line 1", Some("a.toml"), Some(1)),
            "its describer's words beside the tag stay"
        );
    }
}

#[test]
fn a_setting_given_the_boards_scope_alone_is_left_with_none_and_drawn_with_no_control() {
    let alone = Setting {
        scopes: vec![SettingScope::Board],
        ..setting("[every] shade", SettingClass::Preference)
    };
    let judged = judged(described(vec![alone.clone()]), "plugin-every-a".into(), false);
    let [shade] = judged.settings.as_slice() else {
        panic!("{judged:?}");
    };
    assert_eq!(
        shade,
        &Setting {
            scopes: Vec::new(),
            ..alone
        },
        "a preference still, its value and its source as said"
    );
    // The page draws a control only for a setting the store's list names, and names none for it.
    let page = [SettingsSection {
        id: "plugins".into(),
        title: "Plugins".into(),
        about: String::new(),
        entries: vec![judged],
    }];
    assert!(stored::list(&page, &stored::Board::default(), None, "main").is_empty());
}

/// A describer of the id it holds: one whose id starts `broken` cannot say.
struct Says(&'static str);

impl Describer for Says {
    fn id(&self) -> &str {
        self.0
    }
    fn describe(&self, _: &Path) -> Result<SettingsEntry, String> {
        if self.0.starts_with("broken") {
            return Err("its file does not read".into());
        }
        Ok(described(Vec::new()))
    }
}

/// A plugin of a name and its describers.
struct Describes(&'static str, &'static [Says]);

impl Plugin for Describes {
    fn name(&self) -> &str {
        self.0
    }
    fn describers(&self) -> Vec<&dyn Describer> {
        (self.1.iter()).map(|d| -> &dyn Describer { d }).collect()
    }
}

#[test]
fn each_plugin_entry_has_an_anchor_no_earlier_one_took() {
    static X: Describes = Describes(
        "x",
        &[Says("y-z"), Says("y-z"), Says("broken"), Says("broken")],
    );
    static X_Y: Describes = Describes("x-y", &[Says("z"), Says("z-2")]);
    let entries = plugins(&Plugins::new(vec![&X, &X_Y]), Path::new("/p"));
    let ids: Vec<&str> = (entries.iter()).map(|e| e.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "plugin-x-y-z",
            // Two describers of one id.
            "plugin-x-y-z-2",
            // One that cannot say is counted on as well.
            "plugin-x-broken",
            "plugin-x-broken-2",
            // Plugin `x-y`'s `z` meets plugin `x`'s `y-z`, and `z-2` the count given before it.
            "plugin-x-y-z-3",
            "plugin-x-y-z-2-2",
        ]
    );
    assert_eq!(
        entries[1].name, "A",
        "a counted entry is described all the same"
    );
    assert!(
        (entries[3].state.as_ref()).is_some_and(|s| s.why.contains("its file does not read")),
        "{:?}",
        entries[3]
    );
}

#[test]
fn every_setting_of_a_program_whose_settings_decide_what_lands_is_reviewed() {
    let judged = judged(
        described(vec![
            setting("enabled", SettingClass::Preference),
            setting("[limits] low_volume", SettingClass::Preference),
        ]),
        "github".into(),
        true,
    );
    assert!(
        (judged.settings.iter())
            .all(|s| s.class == SettingClass::Reviewed && s.scopes == [SettingScope::Project]),
        "{judged:?}"
    );
}

/// `sh -c SCRIPT`, said as `sh`.
fn shell(script: &str, within: Duration) -> Result<Vec<u8>, String> {
    run(sys::command("sh").args(["-c", script]), "sh", within)
}

#[test]
fn a_command_that_exits_0_answers_what_it_printed() {
    let answer = shell("echo '{}'; echo said >&2", Duration::from_secs(60));
    assert_eq!(answer, Ok(b"{}\n".to_vec()));
}

#[test]
fn a_command_that_fails_says_its_code_and_its_first_line_on_stderr() {
    assert_eq!(
        shell(
            "echo out; printf '\\n  no file\\nmore\\n' >&2; exit 3",
            Duration::from_secs(60)
        ),
        Err("sh exited 3: no file".into())
    );
    assert_eq!(
        shell("exit 4", Duration::from_secs(60)),
        Err("sh exited 4".into())
    );
    assert_eq!(
        shell("kill -9 $$", Duration::from_secs(60)),
        Err("sh was killed".into())
    );
}

#[test]
fn a_program_that_does_not_start_says_so() {
    let gone = run(
        &mut sys::command("/nonexistent/symphony-github"),
        "symphony-github describe",
        Duration::from_secs(60),
    )
    .unwrap_err();
    assert!(
        gone.starts_with("symphony-github describe did not start: "),
        "{gone}"
    );
}

#[test]
fn a_command_that_gives_no_answer_in_its_time_is_killed_and_said() {
    // It would sleep far past the bound; the bound is what ends the wait.
    let answer = shell("exec sleep 600", Duration::from_millis(200));
    assert_eq!(answer, Err("sh gave no answer in 200 ms".into()));
    assert_eq!(
        shell("exec sleep 600", Duration::from_secs(1)).unwrap_err(),
        "sh gave no answer in 1 s"
    );
}
