//! `tasks settings` and the board's store of preferences as the command line holds to them: a
//! stored value in force from the next command, named by `tasks limits` with the committed value
//! it stands over, under a local file's, and gone with its entry; a refusal that names the key and
//! why and stores nothing; and each change in the entry's record.

use crate::slots::in_project;
use std::fs;

const COMMITTED: &str = "\
[agents]
panes = \"none\"
launch = { default = \"codex\", model = \"gpt\" }

[agents.models]
reviewer = \"sonnet\"

[limits]
low_volume = 3
orchestrator_slots = 2
";

const EMPTY: &str =
    "the board's store holds no setting: every value is the files' (just tasks limits)\n";

#[test]
fn a_stored_setting_holds_from_the_next_command_under_a_local_file_and_goes_with_its_entry() {
    let (c, root) = in_project("settings-order", COMMITTED);
    let orch = c.register("orchestrator");
    let before = c.ok(&orch, &["limits"]);
    assert_eq!(c.ok(&orch, &["settings", "list"]), EMPTY);
    // A limit, a role's model, the launch agent and the panes setting, each stored.
    assert_eq!(
        c.ok(&orch, &["settings", "set", "limits.low_volume", "1"]),
        "[limits] low_volume is 1 in the board's store, in force from the next read of the \
         settings unless symphony.local.toml writes it\n"
    );
    c.ok(
        &orch,
        &["settings", "set", "agents.models.reviewer", "opus"],
    );
    c.ok(
        &orch,
        &["settings", "set", "agents.launch.default", "claude"],
    );
    c.ok(&orch, &["settings", "set", "agents.panes", "hc"]);
    let stored = c.ok(&orch, &["limits"]);
    let lines: Vec<&str> = stored.lines().collect();
    assert_eq!(
        lines[1],
        "STORED over it 4 settings the board keeps (just tasks settings list)"
    );
    for line in [
        "orchestrator_slots       2       [limits] orchestrator_slots, line 10",
        "low_volume               1       the board's store [limits] low_volume, over 3 ([limits] low_volume, line 9)",
        "MODELS of claude, from the board's store [agents] launch default, over \"codex\" ([agents] launch default, line 3)",
        "executor                 gpt     [agents] launch model, line 3",
        "reviewer                 opus    the board's store [agents.models] reviewer, over \"sonnet\" ([agents.models] reviewer, line 6)",
        "assessor                 haiku   the smallest claude offers",
        "panes                    hc      the board's store [agents] panes, over \"none\" ([agents] panes, line 2)",
    ] {
        assert!(lines.contains(&line), "{line}\n{stored}");
    }
    // The stored limit is the one the board holds a command to.
    let pulled = c.ok(&orch, &["pull"]);
    assert!(pulled.contains("threshold=1 "), "{pulled}");
    // A local file's value still wins over a stored one.
    let local = root.join("symphony.local.toml");
    fs::write(
        &local,
        "[limits]\nlow_volume = 0\n\n[agents.models]\nreviewer = \"mine\"\n",
    )
    .unwrap();
    let over = c.ok(&orch, &["limits"]);
    for line in [
        "low_volume               0       symphony.local.toml [limits] low_volume, line 2",
        "reviewer                 mine    symphony.local.toml [agents.models] reviewer, line 5",
    ] {
        assert!(over.lines().any(|l| l == line), "{line}\n{over}");
    }
    fs::remove_file(&local).unwrap();
    assert_eq!(c.ok(&orch, &["limits"]), stored);
    // The value it holds already writes nothing.
    assert_eq!(
        c.ok(&orch, &["settings", "set", "limits.low_volume", "1"]),
        "[limits] low_volume is 1 in the board's store already: nothing written\n"
    );
    // Each entry removed puts the files' value back: the output of before any was stored.
    for key in [
        "limits.low_volume",
        "agents.models.reviewer",
        "agents.launch.default",
        "agents.panes",
    ] {
        let said = c.ok(&orch, &["settings", "remove", key]);
        assert!(
            said.ends_with("has no entry in the board's store now: the files' value holds\n"),
            "{said}"
        );
    }
    assert_eq!(c.ok(&orch, &["limits"]), before);
    // Its record stays, each change with its before and after and who made it.
    let listed = c.ok(&orch, &["settings", "list"]);
    let lines: Vec<&str> = listed.lines().collect();
    assert_eq!(lines.len(), 12, "{listed}");
    assert_eq!(
        lines[9],
        "[limits] low_volume  removed: the files' value holds"
    );
    assert!(
        lines[10].ends_with(&format!("  {orch}: none -> 1")),
        "{listed}"
    );
    assert!(
        lines[11].ends_with(&format!("  {orch}: 1 -> none")),
        "{listed}"
    );
    let json: serde_json::Value =
        serde_json::from_str(&c.ok(&orch, &["settings", "list", "--json"])).unwrap();
    let reviewer = &json[1];
    assert_eq!(reviewer["key"], "agents.models.reviewer");
    assert_eq!(reviewer["read"], false);
    assert_eq!(reviewer["changes"][0]["after"], "opus");
    assert_eq!(reviewer["changes"][1]["before"], "opus");
    assert_eq!(reviewer["changes"][1]["by"], orch.as_str());
}

#[test]
fn a_refused_write_names_the_key_and_why_exits_one_and_stores_nothing() {
    let (c, _) = in_project("settings-refused", COMMITTED);
    let orch = c.register("orchestrator");
    for (args, why) in [
        (
            &["settings", "set", "land.check", "false"][..],
            "✖ the board's store: [land] check is not kept in the board's store: it holds a command",
        ),
        (
            &["settings", "set", "board.on_review", "true"][..],
            "✖ the board's store: [board] on_review is not kept in the board's store",
        ),
        (
            &["settings", "set", "limits.wip", "3"][..],
            "✖ the board's store: [limits] wip is not a setting the Symphony knows",
        ),
        (
            &["settings", "set", "limits.low_volume", "few"][..],
            "✖ the board's store: [limits] low_volume is \"few\"; not a whole number",
        ),
        (
            &["settings", "set", "limits.machine_agents", "0"][..],
            "✖ the board's store: [limits] machine_agents is 0; a machine runs at least one agent",
        ),
        (
            &["settings", "set", "agents.launch.default", "gemini"][..],
            "✖ the board's store: [agents.launch] default \"gemini\" is not one of codex, claude",
        ),
        (
            &["settings", "remove", "limits.low_volume"][..],
            "✖ the board's store: [limits] low_volume has no entry: nothing to remove",
        ),
        (
            &["settings", "set", "limits.low_volume"][..],
            "✖ settings takes list, set KEY VALUE or remove KEY",
        ),
        (
            &["settings", "forget", "limits.low_volume"][..],
            "✖ settings takes list, set KEY VALUE or remove KEY",
        ),
    ] {
        let ran = c.run_as(&orch, args, "");
        assert_eq!(ran.code, 1, "{args:?}: {}{}", ran.out, ran.err);
        assert!(ran.err.starts_with(why), "{args:?}: {}", ran.err);
    }
    assert_eq!(c.ok(&orch, &["settings", "list"]), EMPTY);
    // A change says who made it: with no session, nothing runs.
    let ran = c.run_as("", &["settings", "set", "limits.low_volume", "1"], "");
    assert_eq!(ran.code, 2, "{}", ran.err);
    assert_eq!(c.ok(&orch, &["settings", "list"]), EMPTY);
}

#[test]
fn an_agents_switch_set_off_from_the_command_line_holds_and_the_defaults_is_refused() {
    let (c, _) = in_project("settings-switch", COMMITTED);
    let orch = c.register("orchestrator");
    // Codex is the committed default: its switch cannot be turned off.
    let ran = c.run_as(
        &orch,
        &["settings", "set", "agents.launch.on.codex", "off"],
        "",
    );
    assert_eq!(ran.code, 1, "{}{}", ran.out, ran.err);
    assert!(
        ran.err.starts_with(
            "✖ the board's store: [agents.launch.on] codex turns off codex, the launch default \
             (symphony.toml [agents] launch default, line 3), which cannot be off: pick another \
             default first"
        ),
        "{}",
        ran.err
    );
    assert_eq!(c.ok(&orch, &["settings", "list"]), EMPTY);
    // Claude may be off; `limits` names the switch and the store.
    assert_eq!(
        c.ok(
            &orch,
            &["settings", "set", "agents.launch.on.claude", "off"]
        ),
        "[agents.launch.on] claude is false in the board's store, in force from the next read of \
         the settings unless symphony.local.toml writes it\n"
    );
    let limits = c.ok(&orch, &["limits"]);
    for line in [
        "AGENTS a launch may start, each by its switch ([agents.launch.on])",
        "codex                    on      the launch default",
        "claude                   off     the board's store [agents.launch.on] claude",
    ] {
        assert!(limits.lines().any(|l| l == line), "{line}\n{limits}");
    }
    // A word that is no switch is refused by the switch's rule.
    let ran = c.run_as(
        &orch,
        &["settings", "set", "agents.launch.on.claude", "maybe"],
        "",
    );
    assert_eq!(ran.code, 1, "{}", ran.err);
    assert!(
        ran.err.contains("a switch is true (on) or false (off)"),
        "{}",
        ran.err
    );
}
