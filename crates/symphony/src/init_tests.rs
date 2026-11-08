use super::*;
use crate::test_tree::Tree;

fn skill(name: &str, body: &str) -> Skill {
    Skill {
        name: name.to_string(),
        files: vec![
            (
                "SKILL.md".to_string(),
                format!("---\nname: {name}\n---\n\n{body}\n"),
            ),
            ("notes.md".to_string(), format!("notes for {body}\n")),
        ],
    }
}

fn kit(skills: Vec<Skill>) -> Shipped {
    Shipped {
        skills,
        codex_rules: "prefix_rule(pattern=[\"just\"], decision=\"allow\")\n".to_string(),
    }
}

fn outcome(report: &Report, entry: &str) -> String {
    let (_, outcome) = report
        .iter()
        .find(|(name, _)| name == entry)
        .unwrap_or_else(|| panic!("no {entry} in {report:?}"));
    outcome.word().to_string()
}

#[test]
fn a_fresh_project_gets_every_file_stamped_and_linked() {
    let tree = Tree::new();
    let report = init(&tree.root, &kit(vec![skill("work", "v1")])).unwrap();
    assert_eq!(outcome(&report, "skill work"), "installed");
    assert_eq!(outcome(&report, "codex rules"), "installed");
    assert_eq!(outcome(&report, "claude settings"), "installed");
    let text = tree.read(".agents/skills/work/SKILL.md");
    let rest = text.strip_prefix("---\nname: work\n---\n").unwrap();
    assert!(rest.starts_with("<!-- symphony-stamp: "), "{text}");
    assert!(
        rest.lines()
            .next()
            .unwrap()
            .ends_with(" files=SKILL.md,notes.md -->")
    );
    assert_eq!(tree.read(".agents/skills/work/notes.md"), "notes for v1\n");
    let link = std::fs::read_link(tree.path(".claude/skills/work")).unwrap();
    assert_eq!(link, PathBuf::from("../../.agents/skills/work"));
    assert!(
        tree.read(".codex/rules/symphony.rules")
            .starts_with("# symphony-stamp: ")
    );
}

#[test]
fn a_second_run_changes_nothing() {
    let tree = Tree::new();
    let shipped = kit(vec![skill("work", "v1")]);
    init(&tree.root, &shipped).unwrap();
    let before = (
        tree.read(".agents/skills/work/SKILL.md"),
        tree.read(".claude/settings.json"),
    );
    let report = init(&tree.root, &shipped).unwrap();
    for entry in ["skill work", "codex rules", "claude settings"] {
        assert_eq!(outcome(&report, entry), "current", "{entry}");
    }
    assert_eq!(
        before,
        (
            tree.read(".agents/skills/work/SKILL.md"),
            tree.read(".claude/settings.json")
        )
    );
}

#[test]
fn a_stamped_copy_out_of_date_is_updated_whole() {
    let tree = Tree::new();
    init(&tree.root, &kit(vec![skill("work", "v1")])).unwrap();
    let report = init(&tree.root, &kit(vec![skill("work", "v2")])).unwrap();
    assert_eq!(outcome(&report, "skill work"), "updated");
    assert!(tree.read(".agents/skills/work/SKILL.md").contains("v2"));
    assert_eq!(tree.read(".agents/skills/work/notes.md"), "notes for v2\n");
}

#[test]
fn a_stamped_file_edited_in_place_is_brought_back() {
    let tree = Tree::new();
    let shipped = kit(vec![skill("work", "v1")]);
    init(&tree.root, &shipped).unwrap();
    tree.write(".agents/skills/work/notes.md", "scribbled\n");
    let report = init(&tree.root, &shipped).unwrap();
    assert_eq!(outcome(&report, "skill work"), "updated");
    assert_eq!(tree.read(".agents/skills/work/notes.md"), "notes for v1\n");
}

#[test]
fn a_copy_without_the_stamp_is_the_projects_own_and_wins() {
    let tree = Tree::new();
    let own = "---\nname: work\n---\nour way\n";
    tree.write(".agents/skills/work/SKILL.md", own);
    tree.write(".agents/skills/work/notes.md", "ours too\n");
    let shipped = kit(vec![skill("work", "v1")]);
    for _ in 0..2 {
        let report = init(&tree.root, &shipped).unwrap();
        assert_eq!(outcome(&report, "skill work"), "kept (the project's own)");
        assert_eq!(tree.read(".agents/skills/work/SKILL.md"), own);
        assert_eq!(tree.read(".agents/skills/work/notes.md"), "ours too\n");
    }
}

#[test]
fn an_own_copy_with_no_link_gets_the_link_so_claude_loads_it() {
    let tree = Tree::new();
    tree.write(".agents/skills/work/SKILL.md", "own\n");
    init(&tree.root, &kit(vec![skill("work", "v1")])).unwrap();
    assert!(tree.path(".claude/skills/work").join("SKILL.md").exists());
}

#[test]
fn an_existing_link_or_folder_in_claude_skills_is_left_alone() {
    let tree = Tree::new();
    tree.write(".claude/skills/work/SKILL.md", "the project's loose copy\n");
    init(&tree.root, &kit(vec![skill("work", "v1")])).unwrap();
    assert_eq!(
        tree.read(".claude/skills/work/SKILL.md"),
        "the project's loose copy\n"
    );
}

#[test]
fn a_project_skill_the_symphony_never_shipped_is_untouched() {
    let tree = Tree::new();
    tree.write(".agents/skills/shader-work/SKILL.md", "ours\n");
    let report = init(&tree.root, &kit(vec![])).unwrap();
    assert_eq!(tree.read(".agents/skills/shader-work/SKILL.md"), "ours\n");
    assert!(report.iter().all(|(name, _)| !name.contains("shader")));
}

#[test]
fn a_skill_no_longer_shipped_is_removed_with_its_link_when_the_symphony_wrote_it() {
    let tree = Tree::new();
    init(
        &tree.root,
        &kit(vec![skill("work", "v1"), skill("old", "x")]),
    )
    .unwrap();
    assert!(tree.path(".claude/skills/old").exists());
    let report = init(&tree.root, &kit(vec![skill("work", "v1")])).unwrap();
    assert_eq!(outcome(&report, "skill old"), "removed");
    assert!(!tree.path(".agents/skills/old/SKILL.md").exists());
    assert!(tree.path(".claude/skills/old").symlink_metadata().is_err());
    assert!(tree.path(".agents/skills/work/SKILL.md").exists());
}

#[test]
fn a_retired_skill_keeps_the_projects_files_beside_it() {
    let tree = Tree::new();
    init(&tree.root, &kit(vec![skill("old", "x")])).unwrap();
    tree.write(".agents/skills/old/taste.md", "ours\n");
    init(&tree.root, &kit(vec![])).unwrap();
    assert_eq!(tree.read(".agents/skills/old/taste.md"), "ours\n");
}

#[test]
fn a_retired_skill_the_project_took_over_is_not_removed() {
    let tree = Tree::new();
    tree.write(".agents/skills/old/SKILL.md", "ours\n");
    let report = init(&tree.root, &kit(vec![])).unwrap();
    assert!(report.iter().all(|(name, _)| !name.contains("old")));
    assert_eq!(tree.read(".agents/skills/old/SKILL.md"), "ours\n");
}

#[test]
fn a_stamped_skill_with_a_missing_file_gets_it_back() {
    let tree = Tree::new();
    let shipped = kit(vec![skill("work", "v1")]);
    init(&tree.root, &shipped).unwrap();
    std::fs::remove_file(tree.path(".agents/skills/work/notes.md")).unwrap();
    let report = init(&tree.root, &shipped).unwrap();
    assert_eq!(outcome(&report, "skill work"), "updated");
    assert!(tree.path(".agents/skills/work/notes.md").exists());
}

#[test]
fn a_skill_folder_without_its_skill_file_is_installed() {
    let tree = Tree::new();
    tree.write(".agents/skills/work/leftover.md", "x\n");
    let report = init(&tree.root, &kit(vec![skill("work", "v1")])).unwrap();
    assert_eq!(outcome(&report, "skill work"), "installed");
}

#[test]
fn codex_rules_follow_the_same_stamp_rule() {
    let tree = Tree::new();
    init(&tree.root, &kit(vec![])).unwrap();
    let mut newer = kit(vec![]);
    newer
        .codex_rules
        .push_str("prefix_rule(pattern=[\"x\"], decision=\"allow\")\n");
    let report = init(&tree.root, &newer).unwrap();
    assert_eq!(outcome(&report, "codex rules"), "updated");
    assert!(tree.read(".codex/rules/symphony.rules").contains("\"x\""));

    tree.write(".codex/rules/symphony.rules", "# ours\n");
    let report = init(&tree.root, &newer).unwrap();
    assert_eq!(outcome(&report, "codex rules"), "kept (the project's own)");
    assert_eq!(tree.read(".codex/rules/symphony.rules"), "# ours\n");
}

fn settings(tree: &Tree) -> Value {
    serde_json::from_str(&tree.read(".claude/settings.json")).unwrap()
}

fn beats(settings: &Value, event: &str) -> Vec<String> {
    settings["hooks"][event]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|group| group["hooks"].as_array().unwrap())
        .map(|hook| hook["command"].as_str().unwrap().to_string())
        .filter(|command| command.contains("tasks-beat"))
        .collect()
}

#[test]
fn settings_keep_the_projects_rules_and_hooks_and_add_the_missing() {
    let tree = Tree::new();
    tree.write(
        ".claude/settings.json",
        r#"{
  "$comment": "ours",
  "permissions": {"allow": ["Bash(just:*)", "Bash(just tasks:*)"], "ask": ["Bash(just bless:*)"]},
  "hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "guard"}]}]}
}"#,
    );
    init(&tree.root, &kit(vec![])).unwrap();
    let after = settings(&tree);
    assert_eq!(after["$comment"], "ours");
    assert_eq!(after["permissions"]["ask"][0], "Bash(just bless:*)");
    let allow: Vec<&str> = after["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .map(|rule| rule.as_str().unwrap())
        .collect();
    assert_eq!(&allow[..2], ["Bash(just:*)", "Bash(just tasks:*)"]);
    assert_eq!(
        allow
            .iter()
            .filter(|rule| **rule == "Bash(just tasks:*)")
            .count(),
        1
    );
    assert!(allow.contains(&"Bash(just symphony-ask:*)"));
    let pre = after["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre[0]["hooks"][0]["command"], "guard");
    assert_eq!(beats(&after, "PreToolUse"), [HOOK_COMMAND]);
    assert_eq!(beats(&after, "PostToolUse"), [HOOK_COMMAND]);
}

#[test]
fn the_symphonys_hook_command_is_brought_current_not_duplicated() {
    let tree = Tree::new();
    tree.write(
        ".claude/settings.json",
        r#"{"hooks": {
  "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "/old/symphony\" hook tasks-beat", "timeout": 3}]}],
  "PostToolUse": [{"matcher": "", "hooks": [{"type": "command", "command": "/old/symphony\" hook tasks-beat"}]}]
}}"#,
    );
    let report = init(&tree.root, &kit(vec![])).unwrap();
    assert_eq!(outcome(&report, "claude settings"), "updated");
    let after = settings(&tree);
    for event in ["PreToolUse", "PostToolUse"] {
        assert_eq!(beats(&after, event), [HOOK_COMMAND], "{event}");
        assert_eq!(after["hooks"][event][0]["hooks"][0]["timeout"], 10);
        assert_eq!(after["hooks"][event].as_array().unwrap().len(), 1);
    }
}

#[test]
fn a_heartbeat_hook_the_project_wrote_is_the_one_used() {
    let tree = Tree::new();
    let own = r#"{"hooks": {"PostToolUse": [{"matcher": "", "hooks": [{"type": "command", "command": "sf-hook tasks-beat"}]}]}}"#;
    tree.write(".claude/settings.json", own);
    init(&tree.root, &kit(vec![])).unwrap();
    let after = settings(&tree);
    assert_eq!(beats(&after, "PostToolUse"), ["sf-hook tasks-beat"]);
    assert_eq!(beats(&after, "PreToolUse"), [HOOK_COMMAND]);
}

#[test]
fn settings_that_are_not_json_or_not_an_object_are_refused_and_left() {
    for text in [
        "{not json",
        "[]",
        r#"{"permissions": []}"#,
        r#"{"hooks": 3}"#,
    ] {
        let tree = Tree::new();
        tree.write(".claude/settings.json", text);
        let error = init(&tree.root, &kit(vec![])).unwrap_err();
        assert!(
            error.to_string().contains("settings.json"),
            "{text}: {error}"
        );
        assert_eq!(tree.read(".claude/settings.json"), text);
    }
}

#[test]
fn the_command_installs_and_reports_and_refuses_what_it_does_not_know() {
    let tree = Tree::new();
    let run = |args: &[&str]| {
        let args: Vec<String> = args.iter().map(ToString::to_string).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = main(&args, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    };
    let root = tree.root.to_str().unwrap();
    let (code, out, _) = run(&["--root", root]);
    assert_eq!(code, 0);
    assert!(out.contains("skill work: installed\n"), "{out}");
    assert!(out.contains("claude settings: installed\n"), "{out}");
    let (code, out, _) = run(&["--root", root]);
    assert_eq!((code, out.contains("skill work: current\n")), (0, true));
    assert_eq!(run(&["--help"]).0, 0);
    let (code, _, err) = run(&["stray"]);
    assert_eq!(code, 2);
    assert!(err.contains("unrecognized arguments: stray"), "{err}");
    tree.write(".claude/settings.json", "{not json");
    let (code, _, err) = run(&["--root", root]);
    assert_eq!(code, 1);
    assert!(err.starts_with("symphony init: "), "{err}");
}

#[test]
fn every_shipped_skill_is_a_skill_with_front_matter_and_no_stamp_of_its_own() {
    for skill in Shipped::builtin().skills {
        let text = &skill.files[0].1;
        assert!(text.starts_with("---\nname: "), "{}", skill.name);
        assert!(text.contains(&format!("name: {}\n", skill.name)));
        assert!(!is_stamped(text), "{} ships a stamp", skill.name);
    }
}
