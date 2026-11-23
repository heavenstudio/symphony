//! Whether the checkout served takes a value its default branch writes: its own project file
//! writes it, however the file spells the key.

use super::*;
use crate::test_tree::Tree;

/// A checkout whose project file is `text`, or none.
fn checkout(text: Option<&str>) -> Tree {
    let tree = Tree::new();
    if let Some(text) = text {
        tree.write(FILE, text);
    }
    tree
}

fn text(value: &str) -> toml::Value {
    toml::Value::String(value.into())
}

#[test]
fn a_checkout_takes_a_value_its_own_file_writes_however_the_key_is_spelt() {
    let panes = ["agents", "panes"];
    let reviewer = ["agents", "models", "reviewer"];
    let model = ["agents", "launch", "model"];
    for (written, at, value) in [
        ("[agents]\npanes = \"hc\" # mine\n", &panes[..], text("hc")),
        (
            "[agents.models]\nreviewer = \"opus\"\n",
            &reviewer[..],
            text("opus"),
        ),
        (
            "[agents]\nmodels.reviewer = \"opus\"\n",
            &reviewer[..],
            text("opus"),
        ),
        (
            "[agents]\nlaunch = { model = \"fable\" }\n",
            &model[..],
            text("fable"),
        ),
        (
            "[limits]\nlow_volume = 3\n",
            &["limits", "low_volume"][..],
            toml::Value::Integer(3),
        ),
    ] {
        let dir = checkout(Some(written));
        assert!(takes(dir.root.as_path(), at, &value), "{written}");
    }
}

#[test]
fn a_checkout_whose_file_writes_another_value_or_none_does_not_take_it() {
    let panes = ["agents", "panes"];
    for written in [
        Some("[agents]\npanes = \"none\" # mine\n"),
        Some("[agents]\nworktrees = \"../wt\"\n"),
        Some("[limits]\nlow_volume = 3\n"),
        // A key on the way that is no table.
        Some("agents = \"hc\"\n"),
        Some("[agents]\npanes = [\"hc\"]\n"),
        // Text that is no TOML, an empty file and no file at all.
        Some("[agents\npanes = \"hc\"\n"),
        Some(""),
        None,
    ] {
        let dir = checkout(written);
        assert!(
            !takes(dir.root.as_path(), &panes, &text("hc")),
            "{written:?}"
        );
    }
    let dir = checkout(Some("[limits]\nlow_volume = 3\n"));
    assert!(
        !takes(
            dir.root.as_path(),
            &["limits", "low_volume"],
            &toml::Value::Integer(4)
        ),
        "another number"
    );
    assert!(
        !takes(dir.root.as_path(), &[], &toml::Value::Integer(3)),
        "no key at all"
    );
}
