use super::{Changed, changes, message, task};
use crate::commit::Edit;
use toml::Value;

const BASE: &str = "# the project\n[agents]\npanes = \"none\"\n\n[limits] # held to\nlow_volume = 2\n\n[land]\ncheck = true\n";

fn edit(base: &str, new: &str) -> Edit {
    Edit {
        rel: "symphony.toml".into(),
        base: base.into(),
        new: new.into(),
    }
}

fn changed(base: &str, new: &str) -> Result<Vec<Changed>, String> {
    changes(&edit(base, new)).expect("an edit of the project file")
}

#[test]
fn an_edit_of_any_other_file_is_no_settings_edit() {
    let other = Edit {
        rel: "docs/a.md".into(),
        ..edit(BASE, BASE)
    };
    assert!(changes(&other).is_none());
    let nested = Edit {
        rel: "sub/symphony.toml".into(),
        ..edit(BASE, BASE)
    };
    assert!(changes(&nested).is_none());
}

#[test]
fn each_preference_it_changes_is_said_with_what_the_file_wrote_before() {
    let new = BASE
        .replace("low_volume = 2", "low_volume = 3")
        .replace("panes = \"none\"", "panes = \"hc\"")
        + "[agents.models]\nreviewer = \"opus\"\n";
    assert_eq!(
        changed(BASE, &new),
        Ok(vec![
            Changed {
                key: "agents.models.reviewer".into(),
                before: None,
                after: Value::String("opus".into()),
            },
            Changed {
                key: "agents.panes".into(),
                before: Some(Value::String("none".into())),
                after: Value::String("hc".into()),
            },
            Changed {
                key: "limits.low_volume".into(),
                before: Some(Value::Integer(2)),
                after: Value::Integer(3),
            },
        ])
    );
}

#[test]
fn a_key_reached_by_an_inline_table_or_a_dotted_key_is_the_same_setting() {
    let base = "[agents]\nlaunch = { default = 'claude' }\n";
    let new = "[agents]\nlaunch = { default = 'codex' }\n";
    let found = changed(base, new).unwrap();
    assert_eq!(found[0].key, "agents.launch.default");
    let found = changed("", "agents.launch.model = \"m\"\n").unwrap();
    assert_eq!(found[0].key, "agents.launch.model");
}

#[test]
fn comments_and_order_alone_change_no_setting() {
    let new = BASE.replace("# held to", "# what the board holds to");
    assert_eq!(changed(BASE, &new), Ok(Vec::new()));
}

#[test]
fn a_setting_that_holds_a_command_or_decides_what_lands_is_refused_in_the_stores_words() {
    for (from, to) in [
        ("check = true", "check = false"),
        ("[land]\n", "[land]\ndelivery = [\"x\"]\n"),
        (
            "check = true\n",
            "check = true\n\n[board]\non_land = [\"rm\"]\n",
        ),
    ] {
        let why = changed(BASE, &BASE.replace(from, to)).unwrap_err();
        assert!(
            why.starts_with("a settings edit of symphony.toml changes preferences alone: "),
            "{to}: {why}"
        );
        assert!(why.contains("holds a command or a program"), "{to}: {why}");
    }
}

#[test]
fn a_key_the_store_does_not_keep_or_a_value_it_refuses_is_refused() {
    for (to, said) in [
        (
            "[project]\nname = 'x'\n",
            "is not a setting the board's store keeps",
        ),
        ("[limits]\nlow_volume = -1\n", "low_volume"),
        ("[agents]\npanes = \"tmux\"\n", "is not one of hc, none"),
        ("[agents.launch]\ndefault = \"vim\"\n", "default"),
    ] {
        let why = changed("", to).unwrap_err();
        assert!(why.contains(said), "{to}: {why}");
    }
}

#[test]
fn a_key_taken_out_is_refused() {
    let why = changed(BASE, &BASE.replace("low_volume = 2\n", "")).unwrap_err();
    assert_eq!(
        why,
        "a settings edit of symphony.toml removes nothing, and this one removes limits.low_volume"
    );
}

#[test]
fn text_that_is_no_toml_on_either_side_is_refused() {
    let why = changed("[limits\n", BASE).unwrap_err();
    assert!(why.starts_with("symphony.toml before the edit: "), "{why}");
    let why = changed(BASE, "[limits\n").unwrap_err();
    assert!(why.starts_with("symphony.toml after the edit: "), "{why}");
}

#[test]
fn the_message_names_each_key_the_value_before_and_after() {
    let changes = changed(
        BASE,
        &(BASE.replace("low_volume = 2", "low_volume = 3")
            + "[agents.models]\nreviewer = \"opus\"\n"),
    )
    .unwrap();
    assert_eq!(
        message(&changes),
        "settings: [agents.models] reviewer from none written to \"opus\"; [limits] low_volume \
         from 2 to 3"
    );
    let launch = changed("", "[agents.launch]\nmodel = \"m\"\n").unwrap();
    assert_eq!(
        message(&launch),
        "settings: [agents] launch model from none written to \"m\""
    );
}

#[test]
fn its_task_names_the_change_its_branch_and_its_commit() {
    let changes = changed(BASE, &BASE.replace("low_volume = 2", "low_volume = 3")).unwrap();
    let (title, body) = task(&changes, "docs/owner-save-7", "abc123", "main");
    assert_eq!(
        title,
        "Deliver the owner's settings change: [limits] low_volume from 2 to 3"
    );
    for said in [
        "the branch docs/owner-save-7 holds it",
        "the one commit abc123 of symphony.toml",
        "not on main",
        "its entry is removed once the delivery's receipt proves the commit",
    ] {
        assert!(body.contains(said), "{said}: {body}");
    }
}
