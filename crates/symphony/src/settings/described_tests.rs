//! The class a description may not lower ([`protected`], [`judged`]), and the run of a program's
//! command ([`run`]): what it printed, or why not, with its time bounded.

use std::time::Duration;

use symphony_api::{
    Setting, SettingClass, SettingKind, SettingScope, SettingSource, SettingsEntry,
};
use symphony_board::sys;

use super::{judged, protected, run};

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
    ] {
        assert!(protected(key), "{key}");
    }
    for key in [
        "[limits] low_volume",
        "[agents] panes",
        "[agents.launch] model",
        "[project] name",
        "[board] port",
        // Not `[board]`'s own key, but one of a table below it.
        "[board.x] on_land",
        "[every] shade",
        "enabled",
        "",
        "[",
    ] {
        assert!(!protected(key), "{key}");
    }
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
