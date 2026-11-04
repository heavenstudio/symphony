//! A board field set through the Symphony: the plugin declaring it validates the value before the
//! board stores what it returns. Each case has a scratch database of its own, never the live board.

use serde_json::json;
use symphony::plugins::Plugins;
use symphony::taskview::set_field;
use symphony_plugin::vocabulary::FieldValue;
use symphony_plugin::{BoardField, FieldKind, Plugin};

use super::board::{Fixture, Seed};

struct Game;
struct Effort;
struct Risk;

static GAME: Game = Game;

impl Plugin for Game {
    fn name(&self) -> &str {
        "game"
    }
    fn board_fields(&self) -> Vec<&dyn BoardField> {
        vec![&Effort, &Risk]
    }
}

impl BoardField for Effort {
    fn name(&self) -> &str {
        "effort"
    }
    fn kind(&self) -> FieldKind {
        FieldKind::Number
    }
}

impl BoardField for Risk {
    fn name(&self) -> &str {
        "risk"
    }
    fn kind(&self) -> FieldKind {
        FieldKind::Choice(vec!["low".into(), "high".into()])
    }
}

fn board() -> Fixture {
    let f = Fixture::new(Seed::default().session("brisk-finch"));
    f.task("sim/jump");
    f
}

#[test]
fn a_declared_field_is_validated_then_stored_under_its_plugin() {
    let f = board();
    let plugins = Plugins::new(vec![&GAME]);
    assert_eq!(
        set_field(
            &f.store,
            &plugins,
            "sim/jump",
            "effort",
            &json!(" 3 "),
            "brisk-finch"
        ),
        Ok(("game".into(), FieldValue::Number(3.0)))
    );
    assert_eq!(
        set_field(
            &f.store,
            &plugins,
            "sim/jump",
            "game.risk",
            &json!("high"),
            "brisk-finch"
        ),
        Ok(("game".into(), FieldValue::Text("high".into())))
    );
    let stored = f.store.get("sim/jump").unwrap().fields;
    assert_eq!(stored.keys().collect::<Vec<_>>(), ["game"]);
    assert_eq!(stored["game"]["effort"].as_f64(), Some(3.0));
    assert_eq!(stored["game"]["risk"], json!("high"));
}

#[test]
fn a_field_no_plugin_declares_or_a_value_its_kind_refuses_writes_nothing() {
    let f = board();
    let plugins = Plugins::new(vec![&GAME]);
    for (name, value, why) in [
        (
            "weight",
            json!(1),
            "no plugin of this project declares a board field \"weight\"",
        ),
        (
            "other.effort",
            json!(1),
            "no plugin of this project declares a board field \"other.effort\"",
        ),
        ("effort", json!("lots"), "\"lots\" is not a number"),
        ("risk", json!("mid"), "\"mid\" is not one of low, high"),
        (
            "effort",
            json!(true),
            "a field value is a number or text, not true",
        ),
    ] {
        assert_eq!(
            set_field(&f.store, &plugins, "sim/jump", name, &value, "brisk-finch"),
            Err(why.to_string()),
            "{name}"
        );
    }
    // A plain project declares no field at all.
    assert!(
        set_field(
            &f.store,
            &Plugins::default(),
            "sim/jump",
            "effort",
            &json!(1),
            "owner"
        )
        .is_err()
    );
    assert!(f.store.get("sim/jump").unwrap().fields.is_empty());
}

#[test]
fn a_task_the_board_lacks_is_refused_after_the_value_is_checked() {
    let f = board();
    let plugins = Plugins::new(vec![&GAME]);
    let why = set_field(
        &f.store,
        &plugins,
        "sim/none",
        "effort",
        &json!(2),
        "brisk-finch",
    )
    .unwrap_err();
    assert!(why.contains("no task 'sim/none'"), "{why}");
}
