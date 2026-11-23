use super::set;
use toml::Value;

const FILE: &str = "# settings\n[agents]\npanes = \"none\" # terminals\nlaunch = { default = 'claude' }\n\n[limits] # held to\nlow_volume = 2 # slots\n\n[theme]\nname = 'x'\n";

fn edited(text: &str, path: &[&str], value: Value) -> Result<String, String> {
    let mut text = text.to_string();
    set(&mut text, path, &value)?;
    Ok(text)
}

#[test]
fn a_written_number_changes_its_span_alone_and_keeps_its_comment() {
    let after = edited(FILE, &["limits", "low_volume"], Value::Integer(5)).unwrap();
    assert_eq!(
        after,
        FILE.replace("low_volume = 2 # slots", "low_volume = 5 # slots")
    );
}

#[test]
fn a_written_text_is_quoted_and_the_rest_kept_in_order() {
    let after = edited(FILE, &["agents", "panes"], Value::String("hc".into())).unwrap();
    assert_eq!(after, FILE.replace("panes = \"none\"", "panes = \"hc\""));
}

#[test]
fn the_value_already_written_keeps_its_own_quoting() {
    let after = edited(FILE, &["theme", "name"], Value::String("x".into())).unwrap();
    assert_eq!(after, FILE);
}

#[test]
fn a_missing_key_goes_under_its_explicit_table() {
    let after = edited(FILE, &["limits", "machine_agents"], Value::Integer(3)).unwrap();
    assert_eq!(
        after,
        FILE.replace(
            "[limits] # held to\n",
            "[limits] # held to\nmachine_agents = 3\n"
        )
    );
}

#[test]
fn a_missing_key_of_an_inline_table_is_one_more_member() {
    let after = edited(
        FILE,
        &["agents", "launch", "model"],
        Value::String("m".into()),
    )
    .unwrap();
    assert_eq!(
        after,
        FILE.replace(
            "launch = { default = 'claude' }",
            "launch = { model = \"m\", default = 'claude' }"
        )
    );
    let empty = "[agents]\nlaunch = {}\n";
    let after = edited(
        empty,
        &["agents", "launch", "model"],
        Value::String("m".into()),
    )
    .unwrap();
    assert_eq!(after, "[agents]\nlaunch = {model = \"m\"}\n");
}

#[test]
fn a_table_the_file_does_not_write_is_reached_by_a_dotted_key() {
    let after = edited(
        FILE,
        &["agents", "models", "reviewer"],
        Value::String("r".into()),
    )
    .unwrap();
    assert_eq!(
        after,
        FILE.replace("[agents]\n", "[agents]\nmodels.reviewer = \"r\"\n")
    );
    let after = edited("", &["limits", "low_volume"], Value::Integer(1)).unwrap();
    assert_eq!(after, "limits.low_volume = 1\n");
    // A table no header names goes before the first header, where a key is the root's: after
    // one, it would be that table's.
    let after = edited(
        "[project]\nname = 'p'",
        &["limits", "low_volume"],
        Value::Integer(1),
    )
    .unwrap();
    assert_eq!(after, "limits.low_volume = 1\n[project]\nname = 'p'");
    assert_eq!(
        toml::from_str::<toml::Table>(&after).unwrap()["limits"]["low_volume"],
        Value::Integer(1)
    );
    // A header on a last line that does not end gets the line ended before the key.
    let after = edited("[limits]", &["limits", "low_volume"], Value::Integer(1)).unwrap();
    assert_eq!(after, "[limits]\nlow_volume = 1\n");
}

#[test]
fn a_switch_is_written_bare() {
    let after = edited(
        "[plugins]\ntiming = false\n",
        &["plugins", "timing"],
        Value::Boolean(true),
    );
    assert_eq!(after.unwrap(), "[plugins]\ntiming = true\n");
}

#[test]
fn a_file_with_windows_lines_gets_windows_lines() {
    let after = edited("[limits]\r\n", &["limits", "low_volume"], Value::Integer(4)).unwrap();
    assert_eq!(after, "[limits]\r\nlow_volume = 4\r\n");
}

#[test]
fn text_with_a_quote_and_a_delete_is_escaped_as_toml_reads_it() {
    let after = edited(
        "[agents]\n",
        &["agents", "panes"],
        Value::String("a\"b\u{7f}".into()),
    )
    .unwrap();
    assert_eq!(after, "[agents]\npanes = \"a\\\"b\\u007F\"\n");
    let read: toml::Table = toml::from_str(&after).unwrap();
    assert_eq!(read["agents"]["panes"].as_str(), Some("a\"b\u{7f}"));
}

#[test]
fn a_key_written_as_another_kind_is_refused_and_nothing_changed() {
    let mut text = FILE.to_string();
    let refused = set(
        &mut text,
        &["limits", "low_volume"],
        &Value::String("2".into()),
    );
    assert_eq!(
        refused,
        Err("symphony.toml: limits.low_volume must be a string".into())
    );
    assert_eq!(text, FILE);
}

#[test]
fn a_kind_the_edit_does_not_write_is_refused() {
    for value in [
        Value::Float(1.5),
        Value::Array(Vec::new()),
        Value::Table(toml::Table::new()),
    ] {
        let mut text = FILE.to_string();
        let refused = set(&mut text, &["limits", "low_volume"], &value).unwrap_err();
        assert!(
            refused.contains("which this edit does not write"),
            "{refused}"
        );
        assert_eq!(text, FILE);
    }
}

#[test]
fn a_path_through_a_value_that_is_no_table_is_refused() {
    let refused = edited(FILE, &["agents", "panes", "x"], Value::Integer(1)).unwrap_err();
    assert_eq!(refused, "symphony.toml: agents.panes must be a table");
}

#[test]
fn text_that_is_no_toml_is_refused_naming_the_file() {
    let refused = edited("[limits\n", &["limits", "low_volume"], Value::Integer(1)).unwrap_err();
    assert!(refused.starts_with("symphony.toml: "), "{refused}");
}
