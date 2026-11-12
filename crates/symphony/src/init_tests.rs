//! Installer ownership, updates, retirement and failure diagnostics in private projects.

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
        recipes: "tasks *ARGS:\n    @echo tasks\n".to_string(),
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
    let recipes = tree.read("symphony.just");
    let (stamp, rest) = recipes.split_once('\n').unwrap();
    assert!(stamp.starts_with("# symphony-stamp: "), "{recipes}");
    assert_eq!(rest, "tasks *ARGS:\n    @echo tasks\n");
    for entry in ["symphony.just", "justfile", ".gitignore"] {
        assert_eq!(outcome(&report, entry), "installed", "{entry}");
    }
    assert_eq!(tree.read("justfile"), JUSTFILE);
    assert!(imports(JUSTFILE));
    assert_eq!(tree.read(".gitignore"), "/.target/\n/artifacts/\n");
}

#[test]
fn a_second_run_changes_nothing() {
    let tree = Tree::new();
    let shipped = kit(vec![skill("work", "v1")]);
    init(&tree.root, &shipped).unwrap();
    let files = [
        ".agents/skills/work/SKILL.md",
        ".claude/settings.json",
        "symphony.just",
        "justfile",
        ".gitignore",
    ];
    let before: Vec<String> = files.iter().map(|file| tree.read(file)).collect();
    let report = init(&tree.root, &shipped).unwrap();
    for entry in [
        "skill work",
        "codex rules",
        "claude settings",
        "symphony.just",
        ".gitignore",
    ] {
        assert_eq!(outcome(&report, entry), "current", "{entry}");
    }
    // The justfile init wrote is the project's from then on.
    assert_eq!(outcome(&report, "justfile"), "kept (the project's own)");
    let after: Vec<String> = files.iter().map(|file| tree.read(file)).collect();
    assert_eq!(before, after);
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

/// Where a kit holds one stamped file's text.
type ShippedText = fn(&mut Shipped) -> &mut String;

#[test]
fn codex_rules_and_recipes_follow_the_same_stamp_rule() {
    let cases: [(&str, &str, ShippedText); 2] = [
        (CODEX_RULES, "codex rules", |kit| &mut kit.codex_rules),
        (RECIPES, "symphony.just", |kit| &mut kit.recipes),
    ];
    for (file, entry, text) in cases {
        let tree = Tree::new();
        init(&tree.root, &kit(vec![])).unwrap();
        let mut newer = kit(vec![]);
        text(&mut newer).push_str("# a newer line\n");
        let report = init(&tree.root, &newer).unwrap();
        assert_eq!(outcome(&report, entry), "updated");
        assert!(tree.read(file).ends_with("# a newer line\n"), "{entry}");

        // The stamp must be the first line: one further down is the project's text.
        let own = format!("# ours\n{}", tree.read(file));
        tree.write(file, &own);
        let report = init(&tree.root, &newer).unwrap();
        assert_eq!(outcome(&report, entry), "kept (the project's own)");
        assert_eq!(tree.read(file), own);
    }
}

#[test]
fn a_project_justfile_by_any_name_just_reads_is_never_written() {
    for name in ["justfile", "Justfile", "JUSTFILE", ".justfile"] {
        let tree = Tree::new();
        tree.write(name, "build:\n    echo ours\n");
        let report = init(&tree.root, &kit(vec![])).unwrap();
        assert_eq!(outcome(&report, "justfile"), "kept (the project's own)");
        assert_eq!(tree.names(""), {
            let mut names = vec![
                ".claude".to_string(),
                ".codex".to_string(),
                ".gitignore".to_string(),
                name.to_string(),
                "symphony.just".to_string(),
            ];
            names.sort();
            names
        });
        assert_eq!(tree.read(name), "build:\n    echo ours\n");
    }
}

#[test]
fn a_justfile_imports_the_recipes_by_any_spelling_just_reads() {
    for text in [
        "import 'symphony.just'\n",
        "import? 'symphony.just'\n",
        "import \"symphony.just\"\n",
        "  import './symphony.just'   # the Symphony's\n",
        "set allow-duplicate-recipes\n\nbuild:\n    true\n\nimport 'symphony.just'",
    ] {
        assert!(imports(text), "{text:?}");
    }
    for text in [
        "",
        "# import 'symphony.just'\n",
        "import 'tools/symphony.just'\n",
        "import 'symphony.justfile'\n",
        "imports:\n    echo 'symphony.just'\n",
        "mod symphony 'symphony.just'\n",
    ] {
        assert!(!imports(text), "{text:?}");
    }
}

#[test]
fn the_ignore_file_gains_only_the_folders_it_does_not_ignore_yet() {
    for (have, want) in [
        ("", "/.target/\n/artifacts/\n"),
        ("node_modules", "node_modules\n/.target/\n/artifacts/\n"),
        (
            "artifacts/*\n!artifacts/.keep\n",
            "artifacts/*\n!artifacts/.keep\n/.target/\n",
        ),
        ("/.target/\nartifacts/**\n", "/.target/\nartifacts/**\n"),
        (".target\n/artifacts\n", ".target\n/artifacts\n"),
        // A folder of the same name deeper down, or a file beside it, is not the folder.
        (
            "docs/artifacts/\n.target.bak\n",
            "docs/artifacts/\n.target.bak\n/.target/\n/artifacts/\n",
        ),
    ] {
        let tree = Tree::new();
        if !have.is_empty() {
            tree.write(".gitignore", have);
        }
        let report = init(&tree.root, &kit(vec![])).unwrap();
        assert_eq!(tree.read(".gitignore"), want, "{have:?}");
        let said = match (have.is_empty(), have == want) {
            (true, _) => "installed",
            (false, true) => "current",
            (false, false) => "updated",
        };
        assert_eq!(outcome(&report, ".gitignore"), said, "{have:?}");
    }
}

#[test]
fn the_artifacts_folder_ignored_is_the_one_the_project_names() {
    let tree = Tree::new();
    tree.write(
        "symphony.toml",
        "[project]\nid = \"lamps\"\nartifacts = \"out/art\"\n",
    );
    init(&tree.root, &kit(vec![])).unwrap();
    assert_eq!(tree.read(".gitignore"), "/.target/\n/out/art/\n");
    // A project file that does not load leaves the default folder.
    let tree = Tree::new();
    tree.write("symphony.toml", "[project\n");
    init(&tree.root, &kit(vec![])).unwrap();
    assert_eq!(tree.read(".gitignore"), "/.target/\n/artifacts/\n");
}

#[test]
fn a_linked_ignore_file_is_the_projects_own() {
    let tree = Tree::new();
    tree.write("shared.gitignore", "x\n");
    std::os::unix::fs::symlink(tree.path("shared.gitignore"), tree.path(".gitignore")).unwrap();
    let report = init(&tree.root, &kit(vec![])).unwrap();
    assert_eq!(outcome(&report, ".gitignore"), "kept (the project's own)");
    assert_eq!(tree.read("shared.gitignore"), "x\n");
}

fn settings(tree: &Tree) -> Value {
    serde_json::from_str(&tree.read(".claude/settings.json")).unwrap()
}

/// The commands of `event` that name `hook`.
fn naming(settings: &Value, event: &str, hook: &str) -> Vec<String> {
    settings["hooks"][event]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|group| group["hooks"].as_array().unwrap())
        .filter_map(|hook| hook["command"].as_str().map(str::to_string))
        .filter(|command| command.contains(hook))
        .collect()
}

fn beats(settings: &Value, event: &str) -> Vec<String> {
    naming(settings, event, "tasks-beat")
}

fn guards(settings: &Value, event: &str) -> Vec<String> {
    naming(settings, event, "memory-guard")
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
    assert_eq!(beats(&after, "PreToolUse"), [HEARTBEAT.command]);
    assert_eq!(beats(&after, "PostToolUse"), [HEARTBEAT.command]);
}

#[test]
fn the_symphonys_hook_command_is_brought_current_not_duplicated() {
    let tree = Tree::new();
    tree.write(
        ".claude/settings.json",
        r#"{"hooks": {
  "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "/old/symphony hook tasks-beat # symphony-stamp: heartbeat-v0", "timeout": 3}]}],
  "PostToolUse": [{"matcher": "", "hooks": [{"type": "command", "command": "'/old path/symphony' hook tasks-beat # symphony-stamp: heartbeat-v0"}]}]
}}"#,
    );
    let report = init(&tree.root, &kit(vec![])).unwrap();
    assert_eq!(outcome(&report, "claude settings"), "updated");
    let after = settings(&tree);
    for (event, groups) in [("PreToolUse", 2), ("PostToolUse", 1)] {
        assert_eq!(beats(&after, event), [HEARTBEAT.command], "{event}");
        assert_eq!(after["hooks"][event][0]["hooks"][0]["timeout"], 10);
        // The heartbeat's one group, and the memory guard's before a tool call.
        assert_eq!(after["hooks"][event].as_array().unwrap().len(), groups);
    }
}

#[test]
fn the_memory_guard_runs_before_every_write_tool_and_shell_command() {
    let tree = Tree::new();
    init(&tree.root, &kit(vec![])).unwrap();
    let after = settings(&tree);
    let pre = after["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(
        pre[1],
        json!({"matcher": "Write|Edit|MultiEdit|NotebookEdit|Bash", "hooks": [
            {"type": "command", "command": MEMORY_GUARD.command, "timeout": 10}
        ]})
    );
    assert_eq!(guards(&after, "PostToolUse"), Vec::<String>::new());
}

#[test]
fn a_stamped_memory_guard_is_brought_current_beside_the_projects_own_guards() {
    let tree = Tree::new();
    let theirs = json!({"type": "command", "command": "our-brief-guard"});
    tree.write(
        SETTINGS,
        &json!({"hooks": {"PreToolUse": [{"matcher": "Write|Edit", "hooks": [
            {"type": "command", "command": "/old/symphony hook memory-guard # symphony-stamp: memory-guard-v0"},
            theirs.clone(),
        ]}]}})
        .to_string(),
    );
    let report = init(&tree.root, &kit(vec![])).unwrap();
    assert_eq!(outcome(&report, "claude settings"), "updated");
    let after = settings(&tree);
    assert_eq!(guards(&after, "PreToolUse"), [MEMORY_GUARD.command]);
    assert_eq!(
        after["hooks"]["PreToolUse"][0],
        json!({"matcher": "Write|Edit", "hooks": [theirs]})
    );
    assert_eq!(
        outcome(&init(&tree.root, &kit(vec![])).unwrap(), "claude settings"),
        "current"
    );
}

#[test]
fn a_memory_guard_the_project_wrote_is_the_one_used_and_a_stamped_one_beside_it_goes() {
    let tree = Tree::new();
    let own = "\"$CLAUDE_PROJECT_DIR/.claude/hooks/project-hook\" memory-guard";
    tree.write(
        SETTINGS,
        &json!({"hooks": {"PreToolUse": [
            {"matcher": "Write|Edit", "hooks": [{"type": "command",
                "command": "/old/symphony hook memory-guard # symphony-stamp: memory-guard-v0"}]},
            {"matcher": "Write|Edit|MultiEdit|NotebookEdit|Bash",
                "hooks": [{"type": "command", "command": own}]}
        ]}})
        .to_string(),
    );
    let report = init(&tree.root, &kit(vec![])).unwrap();
    assert_eq!(outcome(&report, "claude settings"), "updated");
    let after = settings(&tree);
    // The stamped guard's group goes whole and none is added: the project's own guards the calls.
    assert_eq!(guards(&after, "PreToolUse"), [own]);
    assert_eq!(after["hooks"]["PreToolUse"][0]["hooks"][0]["command"], own);
    assert_eq!(beats(&after, "PreToolUse"), [HEARTBEAT.command]);
    assert_eq!(
        outcome(&init(&tree.root, &kit(vec![])).unwrap(), "claude settings"),
        "current"
    );
}

#[test]
fn a_heartbeat_hook_the_project_wrote_is_the_one_used() {
    let tree = Tree::new();
    let own = r#"{"hooks": {"PostToolUse": [{"matcher": "", "hooks": [{"type": "command", "command": "project-hook tasks-beat"}]}]}}"#;
    tree.write(".claude/settings.json", own);
    init(&tree.root, &kit(vec![])).unwrap();
    let after = settings(&tree);
    assert_eq!(beats(&after, "PostToolUse"), ["project-hook tasks-beat"]);
    assert_eq!(beats(&after, "PreToolUse"), [HEARTBEAT.command]);
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
    let (code, out, err) = run(&["--root", root]);
    assert_eq!(code, 0);
    assert!(out.contains("skill work: installed\n"), "{out}");
    assert!(out.contains("claude settings: installed\n"), "{out}");
    assert!(
        out.contains(".gitignore: installed\n\nNext:\n- write symphony.toml to name the project"),
        "{out}"
    );
    assert_eq!(
        err,
        format!(
            "symphony init: {}\n",
            heartbeat_warning(&tree.root).unwrap()
        )
    );
    let (code, out, _) = run(&["--root", root]);
    assert_eq!((code, out.contains("skill work: current\n")), (0, true));
    assert_eq!(run(&["--help"]).0, 0);
    let (code, _, err) = run(&["stray"]);
    assert_eq!(code, 2);
    assert!(err.contains("unrecognized arguments: stray"), "{err}");
    assert_eq!(run(&["--unknown"]).0, 2);
    assert_eq!(run(&["--root"]).0, 2);
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
        assert!(!is_stamped(text), "{} includes a stamp", skill.name);
    }
}

#[test]
fn removed_managed_support_files_retire_but_project_data_and_own_links_survive() {
    let tree = Tree::new();
    init(&tree.root, &kit(vec![skill("work", "v1")])).unwrap();
    tree.write(".agents/skills/work/taste.md", "project data\n");
    let mut next = skill("work", "v2");
    next.files.retain(|(name, _)| name != "notes.md");
    init(&tree.root, &kit(vec![next])).unwrap();
    assert!(!tree.path(".agents/skills/work/notes.md").exists());
    assert_eq!(tree.read(".agents/skills/work/taste.md"), "project data\n");
    std::fs::remove_file(tree.path(".claude/skills/work")).unwrap();
    std::os::unix::fs::symlink("../../own", tree.path(".claude/skills/work")).unwrap();
    init(&tree.root, &kit(vec![])).unwrap();
    assert_eq!(
        std::fs::read_link(tree.path(".claude/skills/work")).unwrap(),
        PathBuf::from("../../own")
    );
    assert_eq!(tree.read(".agents/skills/work/taste.md"), "project data\n");
}

#[test]
fn all_builtin_skills_leave_a_projects_own_copies_in_use_byte_for_byte() {
    let tree = Tree::new();
    let shipped = Shipped::builtin();
    assert_eq!(shipped.skills.len(), 7);
    for skill in &shipped.skills {
        tree.write(
            &format!(".agents/skills/{}/SKILL.md", skill.name),
            "own\n\nexact bytes\n",
        );
    }
    for _ in 0..2 {
        let report = init(&tree.root, &shipped).unwrap();
        for skill in &shipped.skills {
            assert_eq!(
                outcome(&report, &format!("skill {}", skill.name)),
                "kept (the project's own)"
            );
            assert_eq!(
                tree.read(&format!(".claude/skills/{}/SKILL.md", skill.name)),
                "own\n\nexact bytes\n"
            );
        }
    }
}

#[test]
fn a_project_hook_wins_even_when_a_managed_hook_precedes_it_and_prompt_hooks_stay() {
    let tree = Tree::new();
    let hooks = json!({"hooks":{"PostToolUse":[{"matcher":"", "hooks":[
        {"type":"command", "command":"/old/symphony hook tasks-beat # symphony-stamp: heartbeat-v0"},
        {"type":"command", "command":"our-hook tasks-beat"},
        {"type":"prompt", "prompt":"keep this prompt"}
    ]}]}});
    tree.write(SETTINGS, &hooks.to_string());
    init(&tree.root, &kit(vec![])).unwrap();
    let after = settings(&tree);
    assert_eq!(beats(&after, "PostToolUse"), ["our-hook tasks-beat"]);
    assert_eq!(
        after["hooks"]["PostToolUse"][0]["hooks"][1],
        hooks["hooks"]["PostToolUse"][0]["hooks"][2]
    );
    assert_eq!(
        outcome(&init(&tree.root, &kit(vec![])).unwrap(), "claude settings"),
        "current"
    );
}

#[test]
fn file_and_hook_shape_errors_name_the_path_and_leave_existing_bytes() {
    for file in [
        ".agents/skills/work/SKILL.md",
        CODEX_RULES,
        SETTINGS,
        RECIPES,
        IGNORE,
    ] {
        let tree = Tree::new();
        std::fs::create_dir_all(tree.path(file)).unwrap();
        let error = init(&tree.root, &kit(vec![skill("work", "v1")])).unwrap_err();
        assert!(error.to_string().contains(file), "{error}");
        assert!(tree.path(file).is_dir());
    }
    for event in [
        json!(3),
        json!([{"hooks":3}]),
        json!([{"hooks":[{"type":"command","command":3}]}]),
    ] {
        let tree = Tree::new();
        let text = json!({"hooks":{"PostToolUse":event}}).to_string();
        tree.write(SETTINGS, &text);
        let error = init(&tree.root, &kit(vec![])).unwrap_err();
        assert!(error.to_string().contains("hooks.PostToolUse"), "{error}");
        assert_eq!(tree.read(SETTINGS), text);
    }
}

#[test]
fn managed_retirement_refuses_an_unremovable_file_and_can_be_retried() {
    let tree = Tree::new();
    init(&tree.root, &kit(vec![skill("old", "v1")])).unwrap();
    let before = tree.read(".agents/skills/old/SKILL.md");
    std::fs::remove_file(tree.path(".agents/skills/old/notes.md")).unwrap();
    std::fs::create_dir(tree.path(".agents/skills/old/notes.md")).unwrap();
    let error = init(&tree.root, &kit(vec![])).unwrap_err();
    assert!(error.to_string().contains("old/notes.md"), "{error}");
    assert_eq!(tree.read(".agents/skills/old/SKILL.md"), before);
    std::fs::remove_dir(tree.path(".agents/skills/old/notes.md")).unwrap();
    init(&tree.root, &kit(vec![])).unwrap();
    assert!(!tree.path(".agents/skills/old").exists());
}

#[test]
fn a_stale_managed_matcher_is_repaired_without_changing_its_adjacent_guard() {
    let tree = Tree::new();
    tree.write(
        SETTINGS,
        &json!({"hooks":{"PreToolUse":[{"matcher":"Read", "hooks":[
            {"type":"command","command":"our-read-guard"},
            {"type":"command","command":"/old/symphony hook tasks-beat # symphony-stamp: heartbeat-v0"}
        ]}]}})
        .to_string(),
    );
    init(&tree.root, &kit(vec![])).unwrap();
    let after = settings(&tree);
    assert_eq!(
        after["hooks"]["PreToolUse"][0],
        json!({"matcher":"Read", "hooks":[{"type":"command","command":"our-read-guard"}]})
    );
    assert_eq!(after["hooks"]["PreToolUse"][1]["matcher"], "Bash");
    assert_eq!(beats(&after, "PreToolUse"), [HEARTBEAT.command]);
    assert_eq!(
        outcome(&init(&tree.root, &kit(vec![])).unwrap(), "claude settings"),
        "current"
    );
}

#[test]
fn symlinked_project_skills_and_rules_win_and_managed_support_links_are_not_overwritten() {
    let tree = Tree::new();
    init(&tree.root, &kit(vec![skill("work", "v1")])).unwrap();
    let before = tree.read(".agents/skills/work/SKILL.md");
    std::fs::rename(tree.path(".agents/skills/work"), tree.path("own-work")).unwrap();
    std::os::unix::fs::symlink(tree.path("own-work"), tree.path(".agents/skills/work")).unwrap();
    std::fs::rename(tree.path(CODEX_RULES), tree.path("own.rules")).unwrap();
    std::os::unix::fs::symlink(tree.path("own.rules"), tree.path(CODEX_RULES)).unwrap();
    let rules = tree.read("own.rules");
    init(&tree.root, &kit(vec![skill("work", "v2")])).unwrap();
    init(&tree.root, &kit(vec![])).unwrap();
    assert_eq!(tree.read("own-work/SKILL.md"), before);
    assert_eq!(tree.read("own.rules"), rules);

    init(&tree.root, &kit(vec![skill("other", "v1")])).unwrap();
    std::fs::remove_file(tree.path(".agents/skills/other/notes.md")).unwrap();
    tree.write("own-notes", "project notes\n");
    std::os::unix::fs::symlink(
        tree.path("own-notes"),
        tree.path(".agents/skills/other/notes.md"),
    )
    .unwrap();
    let error = init(&tree.root, &kit(vec![skill("other", "v2")])).unwrap_err();
    assert!(error.to_string().contains("other/notes.md"), "{error}");
    assert_eq!(tree.read("own-notes"), "project notes\n");
}

/// A runnable stand-in at `rel`.
fn program(tree: &Tree, rel: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = tree.write(rel, "#!/bin/sh\n");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// The steps left at `tree`'s `Lamp Shop/`, with the machine folder `machine/` and the search path
/// `path`.
fn left(tree: &Tree, path: &str) -> Vec<String> {
    let folder = tree.path("machine");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::create_dir_all(tree.path("Lamp Shop")).unwrap();
    let path = std::ffi::OsString::from(path);
    steps(
        &tree.path("Lamp Shop"),
        &Machine {
            folder: Some(&folder),
            path: Some(&path),
            surreal: None,
        },
    )
}

#[test]
fn a_fresh_project_is_told_every_step_it_has_left() {
    let tree = Tree::new();
    std::fs::create_dir_all(tree.path("Lamp Shop")).unwrap();
    init(&tree.path("Lamp Shop"), &kit(vec![])).unwrap();
    assert_eq!(
        left(&tree, ""),
        [
            "write symphony.toml to name the project (until then it is `lamp-shop`, after its \
             folder) and to set its agents, verdicts and board",
            "make docs/ for the documents the Symphony serves at /lamp-shop/docs/, or name the \
             folders it serves with [[entrypoints]] in symphony.toml",
            "link the project's builds at .target/bin/symphony and .target/bin/tasks: the \
             heartbeat and the memory guard run them and build nothing",
            "link the project's builds at .target/bin/agent-send and .target/bin/symphony-server, \
             or put them on PATH: `just agent-send` and `just symphony` run them",
            "register the project with the machine's Symphony: `just symphony-cli project add` \
             (a start of `just symphony` registers it too)",
        ]
    );
}

#[test]
fn a_project_with_everything_in_place_has_nothing_left() {
    let tree = Tree::new();
    tree.write("Lamp Shop/symphony.toml", "[project]\nid = \"lamps\"\n");
    tree.write("Lamp Shop/docs/README.md", "# Lamps\n");
    init(&tree.path("Lamp Shop"), &kit(vec![])).unwrap();
    for name in ["symphony", "tasks", "agent-send", "symphony-server"] {
        program(&tree, &format!("Lamp Shop/.target/bin/{name}"));
    }
    let entry = crate::machine::Entry::of(&tree.path("Lamp Shop")).unwrap();
    crate::machine::Registry {
        projects: vec![entry],
    }
    .write(&tree.path("machine"))
    .unwrap();
    assert_eq!(left(&tree, ""), Vec::<String>::new());
    assert_eq!(heartbeat_warning(&tree.path("Lamp Shop")), None);
    assert_eq!(
        next(&[]),
        "Next: nothing; agents can run the board (`just tasks register`)."
    );
    assert_eq!(
        next(&["one".to_string(), "two".to_string()]),
        "Next:\n- one\n- two"
    );
}

#[test]
fn builds_on_the_search_path_serve_the_recipes_but_the_heartbeat_needs_links() {
    let tree = Tree::new();
    tree.write("Lamp Shop/symphony.toml", "[project]\nid = \"lamps\"\n");
    tree.write("Lamp Shop/docs/README.md", "# Lamps\n");
    init(&tree.path("Lamp Shop"), &kit(vec![])).unwrap();
    for name in ["symphony", "tasks", "agent-send", "symphony-server"] {
        program(&tree, &format!("it's here/{name}"));
    }
    let folder = tree.path("it's here");
    let quoted = |name: &str| {
        folder
            .join(name)
            .display()
            .to_string()
            .replace('\'', "'\\''")
    };
    let steps = left(&tree, &format!("/nowhere:{}", folder.display()));
    assert_eq!(
        steps[0],
        format!(
            "link the project's builds at .target/bin/symphony and .target/bin/tasks: the \
             heartbeat and the memory guard run them and build nothing (`mkdir -p .target/bin && \
             ln -s '{}' .target/bin/symphony && ln -s '{}' .target/bin/tasks`)",
            quoted("symphony"),
            quoted("tasks")
        )
    );
    assert!(steps[1].starts_with("register the project"), "{steps:?}");
    assert_eq!(steps.len(), 2, "{steps:?}");
}

#[test]
fn one_build_left_to_link_is_told_in_the_singular() {
    let tree = Tree::new();
    tree.write("Lamp Shop/symphony.toml", "[project]\nid = \"lamps\"\n");
    tree.write("Lamp Shop/docs/README.md", "# Lamps\n");
    init(&tree.path("Lamp Shop"), &kit(vec![])).unwrap();
    for name in ["symphony", "agent-send"] {
        program(&tree, &format!("Lamp Shop/.target/bin/{name}"));
    }
    let steps = left(&tree, "");
    assert_eq!(
        steps[..2],
        [
            "link the project's build at .target/bin/tasks: the heartbeat and the memory guard \
             run it and build nothing",
            "link the project's build at .target/bin/symphony-server, or put it on PATH: \
             `just symphony` runs it",
        ]
    );
}

#[test]
fn a_justfile_of_the_projects_without_the_import_is_told_how_to_take_the_recipes() {
    let tree = Tree::new();
    tree.write("Lamp Shop/Justfile", "build:\n    true\n");
    init(&tree.path("Lamp Shop"), &kit(vec![])).unwrap();
    let step = "import the recipes in Justfile: add `import 'symphony.just'`, with \
                `set allow-duplicate-recipes` so a recipe of its own of the same name wins";
    assert!(left(&tree, "").contains(&step.to_string()));
    tree.write(
        "Lamp Shop/Justfile",
        "set allow-duplicate-recipes\nimport 'symphony.just'\n",
    );
    assert!(left(&tree, "").iter().all(|step| !step.contains("import")));
}

#[test]
fn a_project_file_that_does_not_load_is_the_first_step_and_the_rest_still_say() {
    let tree = Tree::new();
    tree.write(
        "Lamp Shop/symphony.toml",
        "[project]\nid = \"Not A Slug\"\n",
    );
    let steps = left(&tree, "");
    assert!(
        steps[0].starts_with("fix symphony.toml: ") && steps[0].contains("is not a slug"),
        "{steps:?}"
    );
    assert!(
        steps
            .iter()
            .any(|step| step.starts_with("link the project's builds at .target/bin/symphony")),
        "{steps:?}"
    );
}

#[test]
fn an_entrypoint_at_the_root_or_elsewhere_names_its_own_folder_and_route() {
    let tree = Tree::new();
    tree.write(
        "Lamp Shop/symphony.toml",
        "[project]\nid = \"lamps\"\n\n[[entrypoints]]\nurl = \"/\"\nfolder = \".\"\n\n\
         [[entrypoints]]\nurl = \"/notes/\"\nfolder = \"design/notes\"\n",
    );
    let steps = left(&tree, "");
    let made: Vec<&String> = steps.iter().filter(|s| s.starts_with("make ")).collect();
    assert_eq!(
        made,
        [
            "make design/notes/ for the documents the Symphony serves at /lamps/notes/, or name \
          the folders it serves with [[entrypoints]] in symphony.toml"
        ],
        "{steps:?}"
    );
}

#[test]
fn the_warning_names_the_missing_links_and_what_goes_without_them() {
    let tree = Tree::new();
    assert_eq!(
        heartbeat_warning(&tree.root).unwrap(),
        "warning: .target/bin/symphony and .target/bin/tasks are missing: no tool call counts as \
         a beat and the memory guard refuses nothing until the project's builds are linked there \
         (see Next)"
    );
    program(&tree, ".target/bin/symphony");
    assert_eq!(
        heartbeat_warning(&tree.root).unwrap(),
        "warning: .target/bin/tasks is missing: no tool call counts as a beat until the \
         project's builds are linked there (see Next)"
    );
    program(&tree, ".target/bin/tasks");
    assert_eq!(heartbeat_warning(&tree.root), None);
}

#[test]
fn a_registry_that_cannot_be_read_holds_nothing_and_no_machine_folder_asks_nothing() {
    let tree = Tree::new();
    tree.write("machine/projects.json", "{not json");
    assert!(!registered(&tree.root, &tree.path("machine")));
    let steps = steps(
        &tree.root,
        &Machine {
            folder: None,
            path: None,
            surreal: None,
        },
    );
    assert!(
        steps.iter().all(|step| !step.starts_with("register")),
        "no machine folder, no registry to ask: {steps:?}"
    );
}

#[test]
fn no_root_or_a_relative_one_names_the_project_after_the_folder_it_resolves_to() {
    let tree = Tree::new();
    std::fs::create_dir_all(tree.path("Lamp Shop/docs")).unwrap();
    let shop = tree.path("Lamp Shop").canonicalize().unwrap();
    let absolute = shop.display().to_string();
    for given in [
        None,
        Some("."),
        Some("./"),
        Some("../Lamp Shop"),
        Some(&*absolute),
    ] {
        let root = project_root(given, &tree.path("Lamp Shop"));
        assert_eq!(root, shop, "{given:?}");
        let steps = steps(
            &root,
            &Machine {
                folder: None,
                path: None,
                surreal: None,
            },
        );
        assert!(
            steps[0].contains("until then it is `lamp-shop`, after its folder"),
            "{given:?}: {steps:?}"
        );
    }
    // A folder that is not there is left as named, and the run refuses it.
    assert_eq!(
        project_root(Some("gone"), &tree.root),
        tree.root.join("gone")
    );
}

#[test]
fn the_binaries_init_looks_for_are_the_ones_the_installed_recipes_run() {
    let shipped = Shipped::builtin();
    let mut ran = Vec::new();
    let mut recipe = None;
    for line in shipped.recipes.lines() {
        if line.starts_with(|c: char| c.is_ascii_lowercase()) {
            recipe = line.split([' ', ':']).next();
        } else if let Some((_, rest)) = line.split_once("{{ _symphony_run }} ") {
            let binary = rest.split_whitespace().next().unwrap();
            ran.push((recipe.unwrap(), binary));
        }
    }
    ran.sort();
    let mut runs = RUNS.to_vec();
    runs.sort();
    assert_eq!(ran, runs);
}

#[test]
fn a_checkout_is_told_where_its_boards_surrealdb_goes_until_one_is_there() {
    let tree = Tree::new();
    tree.write("Lamp Shop/.git/HEAD", "ref: refs/heads/main\n");
    let root = tree.path("Lamp Shop");
    let pinned = root
        .canonicalize()
        .unwrap()
        .join(".git/symphony/surreal")
        .join(symphony_board::server::SURREAL_VERSION)
        .join("surreal");
    let surreal_step = |named: Option<&Path>| {
        let found = steps(
            &root,
            &Machine {
                folder: None,
                path: None,
                surreal: named,
            },
        );
        let mut asked = found.into_iter().filter(|step| step.contains("SurrealDB"));
        let step = asked.next();
        assert_eq!(asked.next(), None);
        step
    };
    let told = |at: &Path| {
        format!(
            "put SurrealDB's v{} release binary at {}, or name one with SYMPHONY_SURREAL: the board's \
             server runs it",
            symphony_board::server::SURREAL_VERSION,
            at.display()
        )
    };
    assert_eq!(surreal_step(None), Some(told(&pinned)));
    let named = tree.path("elsewhere/surreal");
    assert_eq!(surreal_step(Some(&named)), Some(told(&named)));
    program(&tree, "elsewhere/surreal");
    assert_eq!(surreal_step(Some(&named)), None);
    tree.write(
        &format!(
            "Lamp Shop/.git/symphony/surreal/{}/surreal",
            symphony_board::server::SURREAL_VERSION
        ),
        "not runnable",
    );
    assert_eq!(surreal_step(None), Some(told(&pinned)));
    program(
        &tree,
        &format!(
            "Lamp Shop/.git/symphony/surreal/{}/surreal",
            symphony_board::server::SURREAL_VERSION
        ),
    );
    assert_eq!(surreal_step(None), None);
    let outside = Tree::new();
    assert_eq!(missing_surreal(&outside.root, None), None);
}
