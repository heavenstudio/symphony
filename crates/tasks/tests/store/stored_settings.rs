//! The board's store of preferences (`symphony_board::settings`, `Store::store_setting`): a value
//! judged when it is written, kept with every change of it, read after the committed project file
//! and before the local file, and never read for a setting the store may not hold, however its
//! row came to be there.

use crate::common::{Fixture, HOST, NOW, Seed, database, refused};
use serde_json::json;
use std::path::{Path, PathBuf};
use symphony_board::launch::Defaults;
use symphony_board::limits::Watched;
use symphony_board::settings::{Entry, Stored, value_of};
use symphony_board::{Database, Opening, Schema, Store};

fn board() -> Fixture {
    Fixture::new(Seed::default().session("amber-heron"))
}

/// No project file: the launch default a switch is judged against is the board's own, codex.
fn no_file() -> &'static Path {
    Path::new("")
}

/// `value` stored for `key` by amber-heron: the entry, and whether it changed.
fn set(store: &Store, key: &str, value: &str) -> (Entry, bool) {
    store
        .store_setting(key, &value_of(key, value), "amber-heron", no_file())
        .unwrap()
}

/// Why storing `value` for `key` is refused.
fn refusal(store: &Store, key: &str, value: &str) -> String {
    refused(store.store_setting(key, &value_of(key, value), "amber-heron", no_file()))
}

/// A scratch main checkout: its folder, holding `committed` and, when given, `local`.
struct Checkout(PathBuf);

impl Checkout {
    fn new(db: &Database, committed: &str, local: Option<&str>) -> Checkout {
        let root = std::env::temp_dir().join(format!(
            "stored-settings-{}-{}",
            std::process::id(),
            db.database()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("symphony.toml"), committed).unwrap();
        if let Some(local) = local {
            std::fs::write(root.join("symphony.local.toml"), local).unwrap();
        }
        Checkout(root)
    }

    fn file(&self) -> PathBuf {
        self.0.join("symphony.toml")
    }
}

impl Drop for Checkout {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_board_never_told_anything_holds_no_entry() {
    let f = board();
    assert_eq!(f.store.stored_entries().unwrap(), []);
    assert_eq!(f.store.stored(), Stored::default());
}

#[test]
fn a_stored_value_is_kept_with_every_change_of_it_and_a_removed_entry_keeps_its_record() {
    let f = board();
    let (first, changed) = set(&f.store, "limits.low_volume", "2");
    assert!(changed);
    assert_eq!((first.value.clone(), first.revision), (Some(json!(2)), 1));
    f.advance(60.0);
    // Another session's process writes the same setting: both changes stay in its record.
    let other = f.other();
    let (second, _) = other
        .store_setting(
            "limits.low_volume",
            &value_of("limits.low_volume", "0"),
            "teal-ibis",
            no_file(),
        )
        .unwrap();
    assert_eq!(second.revision, 2);
    // The value it holds already writes nothing and records nothing.
    let (same, changed) = set(&f.store, "limits.low_volume", "0");
    assert!(!changed);
    assert_eq!(same, second);
    f.advance(60.0);
    let removed = f
        .store
        .remove_setting("limits.low_volume", "amber-heron")
        .unwrap();
    assert_eq!(removed.value, None);
    let record: Vec<_> = (removed.changes.iter())
        .map(|change| {
            (
                change.before.clone(),
                change.after.clone(),
                change.at,
                change.by.as_str(),
            )
        })
        .collect();
    assert_eq!(
        record,
        [
            (None, Some(json!(2)), NOW, "amber-heron"),
            (Some(json!(2)), Some(json!(0)), NOW + 60.0, "teal-ibis"),
            (Some(json!(0)), None, NOW + 120.0, "amber-heron"),
        ]
    );
    assert_eq!(f.store.stored_entries().unwrap(), [removed]);
    assert!(
        f.store.stored().is_empty(),
        "a removed entry writes nothing"
    );
    // Nothing is left to remove, and a setting never stored has nothing either.
    for key in ["limits.low_volume", "limits.machine_agents"] {
        let why = refused(f.store.remove_setting(key, "amber-heron"));
        assert!(
            why.ends_with("has no entry: nothing to remove, the files' value holds"),
            "{why}"
        );
    }
    // It is stored again from none, its record going on.
    let (again, _) = set(&f.store, "limits.low_volume", "5");
    assert_eq!((again.revision, again.changes.len()), (4, 4));
    assert_eq!(again.changes[3].before, None);
}

#[test]
fn an_entry_is_removed_for_a_commit_only_while_it_holds_the_value_committed() {
    let f = board();
    set(&f.store, "limits.low_volume", "2");
    // Stored again since the commit of 2: the newer value stays, nothing written.
    set(&f.store, "limits.low_volume", "3");
    let kept = f
        .store
        .remove_setting_holding("limits.low_volume", &json!(2), "owner");
    assert!(!kept.unwrap(), "a newer value stays");
    let entries = f.store.stored_entries().unwrap();
    assert_eq!(
        (entries[0].value.clone(), entries[0].revision),
        (Some(json!(3)), 2)
    );
    // Holding the value committed: removed, as owner's change, its record kept.
    f.advance(60.0);
    let removed = f
        .store
        .remove_setting_holding("limits.low_volume", &json!(3), "owner");
    assert!(removed.unwrap(), "the value committed is removed");
    let entries = f.store.stored_entries().unwrap();
    let last = entries[0].changes.last().unwrap();
    assert_eq!(
        (
            entries[0].value.clone(),
            last.before.clone(),
            last.after.clone(),
            last.by.as_str()
        ),
        (None, Some(json!(3)), None, "owner")
    );
    assert!(f.store.stored().is_empty());
    // Removed already, or never stored: nothing to do, and no refusal.
    for key in ["limits.low_volume", "limits.machine_agents"] {
        assert!(
            !f.store
                .remove_setting_holding(key, &json!(3), "owner")
                .unwrap(),
            "{key}"
        );
    }
    assert_eq!(f.store.stored_entries().unwrap().len(), 1);
    // Who removes it is said, as for any change.
    let why = refused(
        f.store
            .remove_setting_holding("limits.low_volume", &json!(3), " "),
    );
    assert!(why.contains("says who made it"), "{why}");
}

#[test]
fn each_preference_is_stored_and_read_in_the_local_files_shape() {
    let f = board();
    for (key, value) in [
        ("agents.launch.default", "claude"),
        ("agents.launch.model", "fable"),
        ("agents.launch.effort", "high"),
        ("agents.models.reviewer", "opus"),
        ("agents.models.assessor", "haiku"),
        ("agents.panes", "hc"),
        ("limits.orchestrator_slots", "3"),
        ("limits.orchestrator_tasks", "7"),
        ("limits.low_volume", "0"),
        ("limits.machine_agents", "4"),
    ] {
        set(&f.store, key, value);
    }
    let stored = f.store.stored();
    assert_eq!(stored.problems, [] as [String; 0]);
    assert_eq!(stored.keys.len(), 10);
    let held = &stored.held;
    assert_eq!(
        held.launch,
        ["claude", "fable", "high"].map(|v| Some(v.to_string()))
    );
    assert_eq!(held.model_of("reviewer"), Some("opus"));
    assert_eq!(held.model_of("assessor"), Some("haiku"));
    assert_eq!(held.model_of("executor"), None);
    assert_eq!(held.panes.as_deref(), Some("hc"));
    let limits: Vec<_> = held.limits.iter().map(|(key, n)| (*key, *n)).collect();
    assert_eq!(
        limits,
        [
            ("low_volume", 0),
            ("machine_agents", 4),
            ("orchestrator_slots", 3),
            ("orchestrator_tasks", 7),
        ]
    );
}

#[test]
fn a_refused_write_names_the_key_and_why_and_stores_nothing() {
    let f = board();
    for (key, value, why) in [
        // A setting that holds a command or decides what lands, by the local file's lists.
        (
            "land.check",
            "false",
            "the board's store: [land] check is not kept in the board's store: it holds a command",
        ),
        (
            "board.on_land",
            "x",
            "the board's store: [board] on_land is not kept in the board's store: it holds a command",
        ),
        (
            "project.cargo",
            "x",
            "the board's store: [project] cargo is not kept in the board's store",
        ),
        (
            "stamps.review",
            "x",
            "the board's store: [stamps] review is not kept in the board's store",
        ),
        // One the store does not keep, and one nobody knows.
        (
            "board.review_alarm_ratio",
            "3",
            "the board's store: [board] review_alarm_ratio is not a setting the board's store keeps",
        ),
        (
            "theme.accent",
            "red",
            "the board's store: [theme] is not a setting the board's store keeps",
        ),
        (
            "agents.worktrees",
            "x",
            "the board's store: [agents] worktrees is not a setting the board's store keeps",
        ),
        (
            "limits.wip",
            "3",
            "the board's store: [limits] wip is not a setting the Symphony knows",
        ),
        (
            "agents.models.janitor",
            "x",
            "the board's store: [agents.models] janitor is not a setting the Symphony knows",
        ),
        (
            "limits",
            "3",
            "the board's store: \"limits\" names no setting",
        ),
        // A value of the wrong type, and one out of its range.
        (
            "limits.low_volume",
            "many",
            "the board's store: [limits] low_volume is \"many\"; not a whole number",
        ),
        (
            "limits.low_volume",
            "-1",
            "the board's store: [limits] low_volume is -1; it is at least 0",
        ),
        (
            "limits.orchestrator_slots",
            "0",
            "the board's store: [limits] orchestrator_slots is 0; an orchestrator needs at least one slot",
        ),
        (
            "agents.launch.default",
            "gemini",
            "the board's store: [agents.launch] default \"gemini\" is not one of",
        ),
        (
            "agents.panes",
            "tmux",
            "the board's store: [agents] panes \"tmux\" is not one of hc, none",
        ),
    ] {
        let said = refusal(&f.store, key, value);
        assert!(said.starts_with(why), "{key}: {said}");
        assert!(said.ends_with(": nothing was written"), "{said}");
    }
    let why = refused(f.store.store_setting(
        "limits.low_volume",
        &value_of("limits.low_volume", "1"),
        " ",
        no_file(),
    ));
    assert!(why.contains("a change says who made it"), "{why}");
    assert_eq!(f.store.stored_entries().unwrap(), []);
}

#[test]
fn an_entry_the_store_may_not_hold_written_straight_into_the_table_is_never_read() {
    let f = board();
    set(&f.store, "limits.low_volume", "1");
    let raw = f.store.raw();
    for (key, value) in [
        ("land.check", json!(false)),
        ("board.on_land", json!("rm -rf /")),
        ("limits.machine_agents", json!("many")),
        ("limits.orchestrator_tasks", json!({"a": 1})),
    ] {
        raw.row("setting", key)
            .create(json!({"key": key, "value": value, "revision": 1, "changes": []}))
            .unwrap();
    }
    let stored = f.store.stored();
    assert_eq!(stored.keys, ["limits.low_volume"]);
    assert_eq!(stored.held.limits.len(), 1);
    assert_eq!(stored.problems.len(), 4, "{:?}", stored.problems);
    for (problem, starts) in stored.problems.iter().zip([
        "the board's store: [board] on_land is not kept in the board's store",
        "the board's store: [land] check is not kept in the board's store",
        "the board's store: [limits] machine_agents is \"many\"; not a whole number",
        "the board's store: [limits] orchestrator_tasks is of no kind a setting holds",
    ]) {
        assert!(problem.starts_with(starts), "{problem}");
        assert!(
            problem.ends_with("its entry is not read, the files' value holds"),
            "{problem}"
        );
    }
    // Such an entry is removed as any other.
    f.store.remove_setting("land.check", "amber-heron").unwrap();
    assert_eq!(f.store.stored().problems.len(), 3);
}

#[test]
fn a_limit_is_read_from_the_default_the_file_the_store_then_the_local_file() {
    let f = board();
    let committed = "[limits]\nlow_volume = 2\norchestrator_slots = 5\n";
    let checkout = Checkout::new(&database(), committed, None);
    let (watched, _) = Watched::new(checkout.file());
    let store = f.other().watching(watched);
    // The default, then the file.
    let limits = store.limits();
    assert_eq!((limits.orchestrator_tasks, limits.low_volume), (16, 2));
    assert_eq!(
        (limits.orchestrator_slots, limits.machine_agents),
        (Some(5), None)
    );
    // The store over both, from the next read of a store kept open, by another's write.
    set(&f.store, "limits.low_volume", "0");
    set(&f.store, "limits.orchestrator_tasks", "9");
    set(&f.store, "limits.machine_agents", "6");
    let limits = store.limits();
    assert_eq!((limits.orchestrator_tasks, limits.low_volume), (9, 0));
    assert_eq!(
        (limits.orchestrator_slots, limits.machine_agents),
        (Some(5), Some(6))
    );
    let read = store.limits_read().unwrap();
    assert_eq!(
        read.written["low_volume"].said(),
        "the board's store [limits] low_volume"
    );
    assert_eq!(read.under["low_volume"], "2 ([limits] low_volume, line 2)");
    assert_eq!(read.under["orchestrator_tasks"], "its default, 16");
    assert_eq!(read.under["machine_agents"], "its default, none");
    assert_eq!(
        read.written["orchestrator_slots"].said(),
        "[limits] orchestrator_slots, line 3"
    );
    assert!(!read.under.contains_key("orchestrator_slots"));
    // The local file over the store.
    std::fs::write(
        checkout.0.join("symphony.local.toml"),
        "[limits]\nlow_volume = 7\n",
    )
    .unwrap();
    let read = store.limits_read().unwrap();
    assert_eq!(
        (read.limits.low_volume, read.limits.orchestrator_tasks),
        (7, 9)
    );
    assert_eq!(
        read.written["low_volume"].said(),
        "symphony.local.toml [limits] low_volume, line 2"
    );
    assert!(!read.under.contains_key("low_volume"));
    // A removed entry puts the file's value back, and the default's.
    std::fs::remove_file(checkout.0.join("symphony.local.toml")).unwrap();
    f.store
        .remove_setting("limits.low_volume", "amber-heron")
        .unwrap();
    f.store
        .remove_setting("limits.orchestrator_tasks", "amber-heron")
        .unwrap();
    let read = store.limits_read().unwrap();
    assert_eq!(
        (read.limits.low_volume, read.limits.orchestrator_tasks),
        (2, 16)
    );
    assert_eq!(
        read.written["low_volume"].said(),
        "[limits] low_volume, line 2"
    );
    assert_eq!(
        read.under.keys().copied().collect::<Vec<_>>(),
        ["machine_agents"]
    );
    // Limits set on a store hold whatever the board keeps.
    let set_here = f.other().capping(Some(1));
    assert_eq!(set_here.limits().machine_agents, None);
}

#[test]
fn a_launch_takes_the_stored_agent_and_models_over_the_file_and_under_the_local_file() {
    let f = board();
    let committed = "[agents]\nlaunch = { default = \"codex\", model = \"gpt\" }\n\n\
                     [agents.models]\nreviewer = \"committed\"\nexecutor = \"committed\"\n";
    let local = "[agents.models]\nexecutor = \"mine\"\n";
    let checkout = Checkout::new(&database(), committed, Some(local));
    let models = |store: &Store| {
        let defaults = Defaults::configured_with(&checkout.0, &store.stored()).unwrap();
        ["orchestrator", "executor", "reviewer", "assessor"]
            .map(|role| defaults.model_of(role))
            .map(|(agent, model, _)| format!("{agent}:{}", model.unwrap_or_default()))
    };
    assert_eq!(
        models(&f.store),
        ["codex:", "codex:mine", "codex:committed", "codex:gpt"]
    );
    set(&f.store, "agents.launch.default", "claude");
    set(&f.store, "agents.launch.model", "fable");
    set(&f.store, "agents.models.reviewer", "stored");
    set(&f.store, "agents.models.executor", "stored");
    // The assessor's is the smallest the stored agent offers; the local file's executor holds.
    assert_eq!(
        models(&f.store),
        ["claude:", "claude:mine", "claude:stored", "claude:haiku"]
    );
    let resolved = Defaults::configured_with(&checkout.0, &f.store.stored())
        .unwrap()
        .resolve_as(Some("orchestrator"), None, None);
    assert_eq!(
        (resolved.agent, resolved.model.as_str()),
        ("claude", "fable")
    );
    f.store
        .remove_setting("agents.launch.default", "amber-heron")
        .unwrap();
    f.store
        .remove_setting("agents.models.reviewer", "amber-heron")
        .unwrap();
    assert_eq!(
        models(&f.store),
        ["codex:", "codex:mine", "codex:committed", "codex:fable"]
    );
}

#[test]
fn a_switch_turning_off_the_launch_default_is_refused_and_any_other_kept() {
    let f = board();
    let off = |file: &Path, agent: &str| {
        let key = format!("agents.launch.on.{agent}");
        f.store
            .store_setting(&key, &value_of(&key, "off"), "amber-heron", file)
    };
    // No project file: codex, the board's own default, cannot be turned off.
    assert_eq!(
        refused(off(no_file(), "codex")),
        "the board's store: [agents.launch.on] codex turns off codex, the launch default (the \
         board's own), which cannot be off: pick another default first: nothing was written"
    );
    assert_eq!(f.store.stored_entries().unwrap(), []);
    // Under a project file naming claude, claude cannot be off and codex can.
    let checkout = Checkout::new(&database(), "[agents.launch]\ndefault = \"claude\"\n", None);
    let why = refused(off(&checkout.file(), "claude"));
    assert!(
        why.contains("the launch default (symphony.toml [agents] launch default, line 2)"),
        "{why}"
    );
    let (entry, changed) = off(&checkout.file(), "codex").unwrap();
    assert!(changed);
    assert_eq!(entry.value, Some(json!(false)));
    let defaults = Defaults::configured_with(&checkout.0, &f.store.stored()).unwrap();
    assert_eq!(
        defaults.off("codex").map(|switch| switch.said()).as_deref(),
        Some("the board's store [agents.launch.on] codex")
    );
    // On again is the switch alone.
    set(&f.store, "agents.launch.on.codex", "on");
    assert_eq!(
        Defaults::configured_with(&checkout.0, &f.store.stored())
            .unwrap()
            .off("codex"),
        None
    );
    // The store's default stands over the project file's: with codex stored as the default,
    // codex cannot be off, and claude can.
    set(&f.store, "agents.launch.default", "codex");
    let why = refused(off(&checkout.file(), "codex"));
    assert!(
        why.contains("the launch default (the board's store [agents] launch default)"),
        "{why}"
    );
    off(&checkout.file(), "claude").unwrap();
    // A default naming an agent turned off is taken: the default wins, its switch does not hold.
    set(&f.store, "agents.launch.default", "claude");
    let defaults = Defaults::configured_with(&checkout.0, &f.store.stored()).unwrap();
    assert_eq!(defaults.off("claude"), None);
    assert!(
        defaults
            .overruled()
            .unwrap()
            .contains("which does not hold: claude is the launch default")
    );
    // A value that is no switch is refused by the switch's own rule.
    let why = refusal(&f.store, "agents.launch.on.codex", "maybe");
    assert!(
        why.starts_with(
            "the board's store: [agents.launch.on] codex is \"maybe\"; a switch is true"
        ),
        "{why}"
    );
}

#[test]
fn a_default_committed_after_an_older_stored_switch_wins_and_the_switch_does_not_hold() {
    let f = board();
    let checkout = Checkout::new(&database(), "[agents.launch]\ndefault = \"claude\"\n", None);
    f.store
        .store_setting(
            "agents.launch.on.codex",
            &toml::Value::Boolean(false),
            "amber-heron",
            &checkout.file(),
        )
        .unwrap();
    // Later, the project commits codex as its default.
    std::fs::write(checkout.file(), "[agents.launch]\ndefault = \"codex\"\n").unwrap();
    let defaults = Defaults::configured_with(&checkout.0, &f.store.stored()).unwrap();
    assert_eq!(defaults.default_agent(), "codex");
    assert_eq!(defaults.off("codex"), None, "the default starts");
    assert_eq!(
        defaults.overruled().as_deref(),
        Some(
            "[agents.launch.on] codex turns off codex (the board's store [agents.launch.on] \
             codex), which does not hold: codex is the launch default, which wins; pick \
             another default to turn it off"
        )
    );
    // The entry itself is read and listed as it was: only the merge overrules it.
    assert!(f.store.stored().holds("agents.launch.on.codex"));
}

#[test]
fn a_board_without_the_step_reads_no_entry_and_refuses_a_write_before_anything_is_sent() {
    let db = database();
    let open = |how: &Opening| {
        Store::opening_on(
            db.another(),
            Box::new(|| NOW),
            Box::new(|_| false),
            HOST,
            how,
        )
        .unwrap()
    };
    // A board made by a build of the contract before the step, which a reader never moves.
    open(&Opening::write().at(Schema::of_this_contract(0)));
    let reader = open(&Opening::read());
    assert_eq!(reader.additions().unwrap(), 0);
    assert_eq!(reader.stored_entries().unwrap(), []);
    assert_eq!(reader.stored(), Stored::default());
    let why = refusal(&reader, "limits.low_volume", "1");
    assert!(
        why.starts_with("the board's store is not on this board yet"),
        "{why}"
    );
    let why = refused(reader.remove_setting("limits.low_volume", "amber-heron"));
    assert!(
        why.starts_with("the board's store is not on this board yet"),
        "{why}"
    );
    // The build that may move it adds its steps, the store's among them, and the store is there.
    let writer = open(&Opening::write());
    assert_eq!(
        writer.additions().unwrap(),
        symphony_board::schema::ADDITIONS
    );
    assert!(set(&writer, "limits.low_volume", "1").1);
    assert_eq!(reader.stored().keys, ["limits.low_volume"]);
}
