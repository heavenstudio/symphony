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
        recipes: Some("tasks *ARGS:\n    @echo tasks\n".to_string()),
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
    assert_eq!(
        tree.read(".gitignore"),
        "/.target/\n/artifacts/\n/symphony.local.toml\n"
    );
}

#[test]
fn the_work_skill_installs_the_owner_answer_commit_instruction_with_a_current_stamp() {
    let tree = Tree::new();
    let shipped = Shipped::builtin();
    init(&tree.root, &shipped).unwrap();
    let text = tree.read(".agents/skills/work/finish.md");
    assert!(text.contains("commit them on\ntheir own cards in that branch"));
    assert!(text.contains("another worktree is committed on that worktree's own branch and\ncard"));
    let skill = tree.read(".agents/skills/work/SKILL.md");
    assert!(!skill_differs_from_stamp(&tree.path(".agents/skills/work"), &skill).unwrap());
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
        // The justfile init wrote, as it wrote it: the project's own once it changes it.
        "justfile",
    ] {
        assert_eq!(outcome(&report, entry), "current", "{entry}");
    }
    let after: Vec<String> = files.iter().map(|file| tree.read(file)).collect();
    assert_eq!(before, after);
    tree.write("justfile", &format!("{JUSTFILE}\nbuild:\n    true\n"));
    let report = init(&tree.root, &shipped).unwrap();
    assert_eq!(outcome(&report, "justfile"), "kept (the project's own)");
}

/// What a run says of a stamped copy it wrote over an edit to, the project's own going to `own`.
fn replaced(own: &str) -> String {
    format!("replaced: it differed from its stamp, so an edit to it is gone; {own}")
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
fn a_stamped_file_edited_in_place_is_brought_back_saying_the_edit_is_gone() {
    let shipped = kit(vec![skill("work", "v1")]);
    let newer = kit(vec![skill("work", "v2")]);
    // An edit to any file of the skill, its SKILL.md below or above the stamp among them, by the
    // same build or a newer one.
    for (file, edit, kit, want) in [
        ("notes.md", "scribbled\n", &shipped, "notes for v1\n"),
        ("notes.md", "scribbled\n", &newer, "notes for v2\n"),
        ("SKILL.md", "a line of ours\n", &shipped, "v1"),
        ("SKILL.md", "a line of ours\n", &newer, "v2"),
    ] {
        let tree = Tree::new();
        init(&tree.root, &shipped).unwrap();
        let path = format!(".agents/skills/work/{file}");
        let text = tree.read(&path);
        let edited = if file == "SKILL.md" {
            text.replacen("---\n", &format!("---\n{edit}"), 1)
        } else {
            edit.to_string()
        };
        tree.write(&path, &edited);
        let report = init(&tree.root, kit).unwrap();
        assert_eq!(
            outcome(&report, "skill work"),
            replaced(OWN_SKILL),
            "{file}"
        );
        assert!(tree.read(&path).contains(want), "{file}");
        assert!(!tree.read(&path).contains(edit), "{file}");
    }
}

#[test]
fn a_stamped_skill_whose_stamp_names_no_digest_differs_from_it() {
    let tree = Tree::new();
    let folder = tree.path("work");
    assert!(skill_differs_from_stamp(&folder, "---\nname: work\n---\nno stamp\n").unwrap());
    assert!(
        skill_differs_from_stamp(&folder, "<!-- symphony-stamp: abc -->\nno files named\n")
            .unwrap()
    );
    let skill = skill("work", "v1");
    let text = stamped_skill(&skill, &skill.files[0].1);
    tree.write("work/SKILL.md", &text);
    tree.write("work/notes.md", &skill.files[1].1);
    assert!(!skill_differs_from_stamp(&folder, &text).unwrap());
    let unnamed = text.replace(" files=SKILL.md,notes.md", " files=SKILL.md");
    assert!(skill_differs_from_stamp(&folder, &unnamed).unwrap());
}

#[test]
fn a_first_line_stamp_holds_the_digest_of_the_text_below_it() {
    let text = "tasks *ARGS:\n    @echo tasks\n";
    let stamped = stamped_rules(text);
    assert!(!differs_from_stamp(&stamped));
    assert!(differs_from_stamp(&format!("{stamped}ours:\n    true\n")));
    assert!(differs_from_stamp(&stamped.replacen("tasks", "task", 1)));
    assert!(differs_from_stamp(&format!("# {MARK}\n{text}")));
    assert!(differs_from_stamp(&format!(
        "# {MARK} 0123456789abcdef\n{text}"
    )));
    // A stamp on its own, with or without its line's end: the empty text below it, as written.
    assert!(!differs_from_stamp(&stamped_rules("")));
    assert!(!differs_from_stamp(stamped_rules("").trim_end()));
    assert!(differs_from_stamp(""));
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
    // A file gone is a difference from the stamp too, which the run undoes.
    assert_eq!(outcome(&report, "skill work"), replaced(OWN_SKILL));
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
    let cases: [(&str, &str, ShippedText, &str); 2] = [
        (
            CODEX_RULES,
            "codex rules",
            |kit| &mut kit.codex_rules,
            OWN_RULES,
        ),
        (
            RECIPES,
            "symphony.just",
            |kit| kit.recipes.as_mut().unwrap(),
            OWN_RECIPES,
        ),
    ];
    for (file, entry, text, own) in cases {
        let tree = Tree::new();
        init(&tree.root, &kit(vec![])).unwrap();
        let mut newer = kit(vec![]);
        text(&mut newer).push_str("# a newer line\n");
        let report = init(&tree.root, &newer).unwrap();
        assert_eq!(outcome(&report, entry), "updated");
        assert!(tree.read(file).ends_with("# a newer line\n"), "{entry}");

        // An edit below the stamp (a recipe added) is written over, by this build or a newer one,
        // and the run says so, and where the project's own goes.
        for shipped in [&newer, &kit(vec![])] {
            let edited = format!("{}ours:\n    true\n", tree.read(file));
            tree.write(file, &edited);
            let report = init(&tree.root, shipped).unwrap();
            assert_eq!(outcome(&report, entry), replaced(own), "{entry}");
            assert!(!tree.read(file).contains("ours:"), "{entry}");
        }
        init(&tree.root, &newer).unwrap();

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
        ("", "/.target/\n/artifacts/\n/symphony.local.toml\n"),
        (
            "node_modules",
            "node_modules\n/.target/\n/artifacts/\n/symphony.local.toml\n",
        ),
        (
            "artifacts/*\n!artifacts/.keep\n",
            "artifacts/*\n!artifacts/.keep\n/.target/\n/symphony.local.toml\n",
        ),
        (
            "/.target/\nartifacts/**\n",
            "/.target/\nartifacts/**\n/symphony.local.toml\n",
        ),
        // The local settings file ignored already, however the line spells it, gains no line.
        (
            ".target\n/artifacts\nsymphony.local.toml\n",
            ".target\n/artifacts\nsymphony.local.toml\n",
        ),
        (
            "/.target/\n/artifacts/\n/symphony.local.toml\n",
            "/.target/\n/artifacts/\n/symphony.local.toml\n",
        ),
        // A folder of the same name deeper down, or a file beside it, is not the folder.
        (
            "docs/artifacts/\n.target.bak\nsymphony.local.toml.bak\n",
            "docs/artifacts/\n.target.bak\nsymphony.local.toml.bak\n/.target/\n/artifacts/\n\
             /symphony.local.toml\n",
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
        // Run again, it adds no line twice.
        let report = init(&tree.root, &kit(vec![])).unwrap();
        assert_eq!(tree.read(".gitignore"), want, "{have:?}, again");
        assert_eq!(outcome(&report, ".gitignore"), "current", "{have:?}");
    }
}

#[test]
fn git_ignores_the_local_settings_file_after_init_whatever_folder_rule_names_it() {
    for have in [
        "",
        // A folder's rule of the file's name ignores no regular file: the file's own line is added.
        "/symphony.local.toml/\n",
        "/symphony.local.toml/*\n",
        "symphony.local.toml/**\n",
        "symphony.local.toml\n",
        "/symphony.local.toml\n",
    ] {
        let tree = Tree::new();
        let git = |args: &[&str]| {
            symphony_board::sys::output(
                symphony_board::sys::git()
                    .arg("-C")
                    .arg(&tree.root)
                    .args(args),
            )
            .unwrap()
            .status
            .success()
        };
        assert!(git(&["init", "-q"]));
        if !have.is_empty() {
            tree.write(".gitignore", have);
        }
        init(&tree.root, &kit(vec![])).unwrap();
        let first = tree.read(".gitignore");
        // Run again, it is current and adds no second line.
        let report = init(&tree.root, &kit(vec![])).unwrap();
        assert_eq!(outcome(&report, ".gitignore"), "current", "{have:?}");
        assert_eq!(tree.read(".gitignore"), first, "{have:?}");
        let lines = first
            .lines()
            .filter(|line| line.trim_start_matches('/') == "symphony.local.toml")
            .count();
        assert_eq!(lines, 1, "{have:?}: {first}");
        tree.write("symphony.local.toml", "[limits]\nlow_volume = 2\n");
        assert!(
            git(&["check-ignore", "--quiet", "symphony.local.toml"]),
            "{have:?}: git does not ignore the regular file under\n{first}"
        );
    }
}

#[test]
fn the_artifacts_folder_ignored_is_the_one_the_project_names() {
    let tree = Tree::new();
    tree.write(
        "symphony.toml",
        "[project]\nid = \"lamps\"\nartifacts = \"out/pictures\"\n",
    );
    init(&tree.root, &kit(vec![])).unwrap();
    assert_eq!(
        tree.read(".gitignore"),
        "/.target/\n/out/pictures/\n/symphony.local.toml\n"
    );
    // A project file that does not load leaves the default folder.
    let tree = Tree::new();
    tree.write("symphony.toml", "[project\n");
    init(&tree.root, &kit(vec![])).unwrap();
    assert_eq!(
        tree.read(".gitignore"),
        "/.target/\n/artifacts/\n/symphony.local.toml\n"
    );
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
    // The project's ask rule is its own, and init adds none.
    assert_eq!(after["permissions"]["ask"], json!(["Bash(just bless:*)"]));
    init(&tree.root, &kit(vec![])).unwrap();
    assert_eq!(settings(&tree), after);
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
    assert_eq!(allow.last(), Some(&"Bash(just land:*)"));
    let pre = after["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre[0]["hooks"][0]["command"], "guard");
    assert_eq!(beats(&after, "PreToolUse"), [HEARTBEAT.command]);
    assert_eq!(beats(&after, "PostToolUse"), [HEARTBEAT.command]);
}

#[test]
fn a_retired_allow_rule_is_taken_out_and_every_other_entry_stays_in_its_place() {
    // A rule made up for the test, among the project's own: one that only starts like it, one
    // that is not a rule at all, and a rule left as it is.
    let retired = ["Bash(just lamp-retired:*)"];
    let mut allow = vec![
        json!("Bash(just tasks:*)"),
        json!("Bash(just lamp-retired:*)"),
        json!("Bash(own:*)"),
        json!("Bash(just lamp-retired:*)"),
        json!("Bash(just lamp-retired status:*)"),
        json!(3),
    ];
    assert!(retire(&mut allow, &retired));
    assert_eq!(
        allow,
        [
            json!("Bash(just tasks:*)"),
            json!("Bash(own:*)"),
            json!("Bash(just lamp-retired status:*)"),
            json!(3),
        ]
    );
    assert!(!retire(&mut allow, &retired), "none left to take");
    assert!(!retire(&mut allow, &[]));
    assert_eq!(allow.len(), 4);
    // What this init retires it never writes.
    for rule in RETIRED.iter().chain(&RETIRED_ASK) {
        assert!(!ALLOW.contains(rule), "{rule} is retired and wanted");
    }
}

#[test]
fn a_fresh_project_allows_just_land_and_is_asked_before_nothing_init_wrote() {
    let tree = Tree::new();
    init(&tree.root, &kit(vec![])).unwrap();
    let after = settings(&tree);
    assert_eq!(
        after["permissions"],
        json!({"allow": ALLOW}),
        "the allow rules alone, `just land` among them, and no ask list"
    );
    assert!(ALLOW.contains(&"Bash(just land:*)"));
    // The server recipes stay asked first: no rule allows them.
    for server in ["symphony", "symphony-cli"] {
        assert!(
            ALLOW
                .iter()
                .all(|rule| !rule.contains(&format!("just {server}:"))
                    && !rule.contains(&format!("just {server} "))),
            "{server}"
        );
    }
}

/// The allow rules a landed init wrote while it asked before a land: no `just land`, and the
/// env-prefixed board rule, retired since.
const ALLOWED_BEFORE_A_LAND: [&str; 6] = [
    "Bash(just tasks:*)",
    "Bash(TASKS_SESSION=* just tasks *)",
    "Bash(just agent-send:*)",
    "Bash(just symphony-status:*)",
    "Bash(just symphony-reply:*)",
    "Bash(just symphony-ask:*)",
];

/// The settings of a project a landed init set up while it asked before a land: the ask rules it
/// wrote, after the project's own and before a rule the project added since.
fn asked_before_a_land(own_before: &[&str], own_after: &[&str]) -> Value {
    let ask: Vec<&str> = own_before
        .iter()
        .chain(&RETIRED_ASK)
        .chain(own_after)
        .copied()
        .collect();
    json!({
        "permissions": {"allow": ALLOWED_BEFORE_A_LAND, "ask": ask, "deny": ["Edit(~/.secrets/**)"]},
        "hooks": {}
    })
}

/// Each allow rule is a recipe's words and then one `*`, at its end: Claude Code's `*` matches any
/// text, spaces included, and only the words before the first one bound what a rule passes
/// (code.claude.com/docs/en/permissions, "Wildcard patterns"). A `*` before or among them, as the
/// retired `Bash(TASKS_SESSION=* just tasks *)` had, passes any program.
#[test]
fn every_allow_rule_is_a_recipes_words_with_its_one_wildcard_last() {
    for rule in ALLOW {
        let words = rule
            .strip_prefix("Bash(just ")
            .and_then(|rest| rest.strip_suffix(":*)"))
            .unwrap_or_else(|| panic!("{rule} is not `Bash(just RECIPE:*)`"));
        assert!(
            !words.is_empty() && words.bytes().all(|b| b.is_ascii_lowercase() || b == b'-'),
            "{rule}"
        );
    }
    assert!(RETIRED.contains(&"Bash(TASKS_SESSION=* just tasks *)"));
}

/// A project an init set up before carries the env-prefixed board rule: a run takes it out, leaves
/// the project's own rules where they were (one that only looks like it among them), and a second
/// run changes nothing.
#[test]
fn the_env_prefixed_board_rule_goes_and_the_projects_own_rules_stay_in_their_place() {
    let tree = Tree::new();
    let mut allow = vec!["Bash(just:*)", "Bash(TASKS_SESSION=lamp-1 just tasks list)"];
    allow.extend(ALLOWED_BEFORE_A_LAND);
    allow.push("Bash(just land:*)");
    allow.push("Bash(rm:*)");
    tree.write(
        SETTINGS,
        &json!({"permissions": {"allow": allow, "deny": ["Edit(~/.secrets/**)"]}}).to_string(),
    );
    let report = init(&tree.root, &kit(vec![])).unwrap();
    assert_eq!(outcome(&report, "claude settings"), "updated");
    let after = settings(&tree);
    let mut want = vec!["Bash(just:*)", "Bash(TASKS_SESSION=lamp-1 just tasks list)"];
    want.extend(ALLOW.iter().filter(|rule| **rule != "Bash(just land:*)"));
    want.extend(["Bash(just land:*)", "Bash(rm:*)"]);
    assert_eq!(after["permissions"]["allow"], json!(want));
    assert_eq!(after["permissions"]["deny"], json!(["Edit(~/.secrets/**)"]));
    let report = init(&tree.root, &kit(vec![])).unwrap();
    assert_eq!(outcome(&report, "claude settings"), "current");
    assert_eq!(settings(&tree), after);
}

#[test]
fn the_ask_rules_a_landed_init_wrote_go_and_the_projects_own_stay_in_their_place() {
    // The first named the marker the `land` recipe sets, which stays: `tasks land` run by itself
    // still refuses (`symphony_board::land::admit`).
    assert_eq!(
        RETIRED_ASK[0],
        format!("Bash(*{}*)", symphony_board::land::RECIPE).as_str()
    );
    let tree = Tree::new();
    tree.write(
        SETTINGS,
        &asked_before_a_land(&["Bash(just bless:*)"], &["Bash(rm:*)"]).to_string(),
    );
    let report = init(&tree.root, &kit(vec![])).unwrap();
    assert_eq!(outcome(&report, "claude settings"), "updated");
    let after = settings(&tree);
    assert_eq!(
        after["permissions"]["ask"],
        json!(["Bash(just bless:*)", "Bash(rm:*)"])
    );
    // The permissions keep their order: the ask list stays where it was.
    let keys: Vec<&String> = after["permissions"].as_object().unwrap().keys().collect();
    assert_eq!(keys, ["allow", "ask", "deny"]);
    assert_eq!(after["permissions"]["allow"], json!(ALLOW));
    // A second run finds nothing left to take or add.
    let report = init(&tree.root, &kit(vec![])).unwrap();
    assert_eq!(outcome(&report, "claude settings"), "current");
    assert_eq!(settings(&tree), after);
}

#[test]
fn settings_current_but_for_the_retired_ask_rules_are_written_back_to_what_a_fresh_init_writes() {
    // Every allow rule and hook init writes is there already: taking the ask rules out is the one
    // change, and it alone has the file written.
    let tree = Tree::new();
    init(&tree.root, &kit(vec![])).unwrap();
    let fresh = tree.read(SETTINGS);
    let mut asked: Value = serde_json::from_str(&fresh).unwrap();
    asked["permissions"]["ask"] = json!(RETIRED_ASK);
    let mut text = serde_json::to_string_pretty(&asked).unwrap();
    text.push('\n');
    tree.write(SETTINGS, &text);
    let report = init(&tree.root, &kit(vec![])).unwrap();
    assert_eq!(outcome(&report, "claude settings"), "updated");
    assert_eq!(tree.read(SETTINGS), fresh);
    let report = init(&tree.root, &kit(vec![])).unwrap();
    assert_eq!(outcome(&report, "claude settings"), "current");
    assert_eq!(tree.read(SETTINGS), fresh);
}

#[test]
fn an_ask_list_of_only_the_retired_rules_goes_with_them_and_the_rest_keep_their_order() {
    let tree = Tree::new();
    tree.write(SETTINGS, &asked_before_a_land(&[], &[]).to_string());
    init(&tree.root, &kit(vec![])).unwrap();
    let after = settings(&tree);
    let keys: Vec<&String> = after["permissions"].as_object().unwrap().keys().collect();
    assert_eq!(keys, ["allow", "deny"]);
    assert_eq!(after["permissions"]["deny"], json!(["Edit(~/.secrets/**)"]));
    assert_eq!(
        outcome(&init(&tree.root, &kit(vec![])).unwrap(), "claude settings"),
        "current"
    );

    // An ask list that was empty before, or one holding only a rule like a retired one, is the
    // project's: left as it is.
    for ask in [json!([]), json!(["Bash(just tasks land:*)", 3])] {
        let tree = Tree::new();
        let have = json!({"permissions": {"allow": ALLOW, "ask": ask}, "hooks": {}});
        tree.write(SETTINGS, &have.to_string());
        init(&tree.root, &kit(vec![])).unwrap();
        assert_eq!(settings(&tree)["permissions"]["ask"], ask, "{ask}");
    }
}

#[test]
fn an_ask_that_is_not_a_list_is_refused_and_left() {
    let tree = Tree::new();
    let text = json!({"permissions": {"ask": "Bash(*SYMPHONY_LAND*)"}}).to_string();
    tree.write(SETTINGS, &text);
    let error = init(&tree.root, &kit(vec![])).unwrap_err();
    assert!(error.to_string().contains(SETTINGS), "{error}");
    assert_eq!(tree.read(SETTINGS), text);
}

#[test]
fn a_projects_own_allow_rules_for_the_server_recipes_stay() {
    let tree = Tree::new();
    // No landed init wrote these: a copy a run finds is the project's choice.
    tree.write(
        SETTINGS,
        &json!({"permissions": {
            "allow": ["Bash(just symphony:*)", "Bash(just symphony-cli:*)"],
            "ask": ["Bash(just symphony:*)"]
        }})
        .to_string(),
    );
    init(&tree.root, &kit(vec![])).unwrap();
    let after = settings(&tree);
    let mut want = vec![
        json!("Bash(just symphony:*)"),
        json!("Bash(just symphony-cli:*)"),
    ];
    want.extend(ALLOW.iter().map(|rule| json!(rule)));
    assert_eq!(after["permissions"]["allow"], json!(want));
    assert_eq!(after["permissions"]["ask"][0], "Bash(just symphony:*)");
    assert_eq!(
        outcome(&init(&tree.root, &kit(vec![])).unwrap(), "claude settings"),
        "current"
    );
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
    let tree = Tree::checkout();
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
    assert!(out.ends_with(&format!("\n\n{TRUST}\n")), "{out}");
    // As `main` reads them: this process's machine folder and search path.
    let warned = heartbeat_warning(
        &tree.root,
        &builds::Beyond {
            machine: crate::machine::dir().ok(),
            path: std::env::var_os("PATH"),
        },
    )
    .map_or_else(String::new, |warning| format!("symphony init: {warning}\n"));
    assert_eq!(err, warned);
    let (code, out, _) = run(&["--root", root]);
    assert_eq!((code, out.contains("skill work: current\n")), (0, true));
    let (code, out, _) = run(&["--help"]);
    assert_eq!(code, 0);
    assert!(out.contains("--root ROOT  the project's root, a folder that is there"));
    // A root that is not a folder is refused, and nothing is made.
    for gone in [tree.path("gone"), tree.path("gone/deeper")] {
        let (code, out, err) = run(&["--root", gone.to_str().unwrap()]);
        assert_eq!((code, out.as_str()), (2, ""), "{err}");
        assert_eq!(
            err,
            format!(
                "{USAGE}\nsymphony init: error: no folder at {}: --root names the project's \
                 folder, which init never makes\n",
                gone.display()
            )
        );
    }
    assert!(!tree.path("gone").exists());
    tree.write("a file", "not a folder\n");
    let (code, _, err) = run(&["--root", tree.path("a file").to_str().unwrap()]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("no folder at "), "{err}");
    assert_eq!(tree.read("a file"), "not a folder\n");
    // Relative, it is read from the current folder.
    let (code, _, err) = run(&["--root", "no such folder here"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("/no such folder here: --root names"), "{err}");
    // A folder in no git repository is refused, and nothing is made in it: the board lives in the
    // repository's common dir, and every board command there would refuse it.
    let outside = Tree::new();
    let (code, out, err) = run(&["--root", outside.root.to_str().unwrap()]);
    assert_eq!((code, out.as_str()), (2, ""), "{err}");
    let real = outside.root.canonicalize().unwrap();
    assert!(
        err.starts_with(&format!(
            "{USAGE}\nsymphony init: error: {} is not inside a git repository (",
            real.display()
        )) && err.ends_with(
            "): the board lives in the project's git repository, so init sets up only a folder in \
             a checkout of one (`git init` makes it one)\n"
        ),
        "{err}"
    );
    assert_eq!(outside.names(""), Vec::<String>::new());
    // A git dir is in no checkout, though git answers a common dir there: a checkout's `.git` and
    // a bare repository are refused, and nothing is made among git's own files.
    let bare = Tree::new();
    bare.write("HEAD", "ref: refs/heads/main\n");
    bare.write("objects/.keep", "");
    bare.write("refs/.keep", "");
    let listing = |dir: &Path| {
        let mut names: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        names
    };
    for git_dir in [tree.path(".git"), bare.root.clone()] {
        let before = listing(&git_dir);
        let (code, out, err) = run(&["--root", git_dir.to_str().unwrap()]);
        assert_eq!((code, out.as_str()), (2, ""), "{err}");
        let real = git_dir.canonicalize().unwrap();
        assert_eq!(
            err,
            format!(
                "{USAGE}\nsymphony init: error: {} is in the git dir {}, in no checkout of it: \
                 init sets up only a folder in a git checkout, never among git's own files\n",
                real.display(),
                real.display()
            )
        );
        assert_eq!(listing(&git_dir), before, "{git_dir:?}: nothing made");
    }
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

/// The page a project writes its standing instructions for a role in, beside that role's skill.
const PROJECT_PAGE: &str = "project.md";

/// A project's standing instructions for a role are its own page beside the role's skill: each
/// role's skill names the page, and so does the orchestrator's, which says where an instruction
/// goes; no other skill names it, backticked or bare, and none ships one; and a run neither writes
/// over the project's, counts it as an edit to the skill, nor removes it.
#[test]
fn a_projects_page_beside_a_role_skill_is_named_by_the_skill_and_left_as_written() {
    let shipped = Shipped::builtin();
    let roles = ["work", "review-task", "assess-task"];
    for skill in &shipped.skills {
        let name = skill.name.as_str();
        let names = |text: &str| text.contains(PROJECT_PAGE);
        if roles.contains(&name) || name == "task-orchestrator" {
            assert!(names(&skill.files[0].1), "{name} does not name the page");
        } else {
            assert!(
                skill.files.iter().all(|(_, text)| !names(text)),
                "{name} names the page"
            );
        }
        assert!(
            skill.files.iter().all(|(file, _)| file != PROJECT_PAGE),
            "{} ships the project's page",
            skill.name
        );
    }
    let tree = Tree::new();
    init(&tree.root, &shipped).unwrap();
    for role in roles {
        tree.write(&format!(".agents/skills/{role}/{PROJECT_PAGE}"), "ours\n");
    }
    let report = init(&tree.root, &shipped).unwrap();
    for role in roles {
        assert_eq!(outcome(&report, &format!("skill {role}")), "current");
        let page = format!(".agents/skills/{role}/{PROJECT_PAGE}");
        assert_eq!(tree.read(&page), "ours\n");
        assert_eq!(
            tree.read(&format!(".claude/skills/{role}/{PROJECT_PAGE}")),
            "ours\n"
        );
    }
    // The skills retired, the page stays where the project wrote it.
    init(&tree.root, &kit(vec![])).unwrap();
    for role in roles {
        assert!(
            !tree
                .path(&format!(".agents/skills/{role}/SKILL.md"))
                .exists()
        );
        assert_eq!(
            tree.read(&format!(".agents/skills/{role}/{PROJECT_PAGE}")),
            "ours\n"
        );
    }
}

/// Where a page link in a shipped skill's file leads, as `(skill, file, anchor)`: a file beside it,
/// `../SKILL/FILE` in another skill, or `#anchor` in itself; none for a link of no such shape.
fn link_target<'a>(
    skill: &'a str,
    file: &'a str,
    target: &'a str,
) -> Option<(&'a str, &'a str, Option<&'a str>)> {
    let (place, anchor) = match target.split_once('#') {
        Some((place, anchor)) => (place, Some(anchor)),
        None => (target, None),
    };
    let (skill, file) = match place.strip_prefix("../") {
        Some(other) => other.split_once('/')?,
        None if place.is_empty() => (skill, file),
        None => (skill, place),
    };
    Some((skill, file, anchor))
}

/// An agent reads a skill's other files only through its pages: each link in a shipped file leads
/// to a file a shipped skill carries, and to a heading there when it names one, and each file a
/// skill carries beside its `SKILL.md` is named by another of its files.
#[test]
fn every_page_a_shipped_skill_links_is_shipped_and_every_page_it_ships_is_named() {
    let shipped = Shipped::builtin();
    let text = |skill: &str, file: &str| {
        let skill = shipped.skills.iter().find(|one| one.name == skill)?;
        let (_, body) = skill.files.iter().find(|(name, _)| name == file)?;
        Some(body.clone())
    };
    let link = regex::Regex::new(r"\]\(([^)\s]+)\)").unwrap();
    let mut links = 0;
    for skill in &shipped.skills {
        for (file, body) in &skill.files {
            for found in link.captures_iter(body) {
                let target = &found[1];
                if target.contains("://") {
                    continue;
                }
                let at = format!("{}/{file} links {target}", skill.name);
                let (owner, page, anchor) =
                    link_target(&skill.name, file, target).unwrap_or_else(|| panic!("{at}"));
                let page = text(owner, page)
                    .unwrap_or_else(|| panic!("{at}, which no shipped skill carries"));
                if let Some(anchor) = anchor {
                    let headings = page.lines().filter_map(|line| {
                        let words = line.trim_start_matches('#');
                        (words.len() < line.len() && words.starts_with(' ')).then_some(words)
                    });
                    let anchors: Vec<String> = headings.map(crate::edit::heading_anchor).collect();
                    assert!(
                        anchors.iter().any(|one| one == anchor),
                        "{at}, a heading it lacks: {anchors:?}"
                    );
                }
                links += 1;
            }
        }
        for (file, _) in skill.files.iter().skip(1) {
            let named = skill
                .files
                .iter()
                .any(|(other, body)| other != file && body.contains(file.as_str()));
            assert!(
                named,
                "{}/{file} is shipped and no page names it",
                skill.name
            );
        }
    }
    // The walk reads links: the work skill's own router leads to each of its stage pages.
    assert!(links >= 3, "{links} links read");
    let router = text("work", "SKILL.md").unwrap();
    for stage in ["start.md", "build.md", "finish.md"] {
        assert!(router.contains(&format!("]({stage})")), "{stage}");
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
    assert_eq!(shipped.skills.len(), 9);
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
/// `path` and then `tools/`, which holds `just`.
fn left(tree: &Tree, path: &str) -> Vec<String> {
    left_beside(tree, path, None)
}

/// [`left`], with `beside` the folder of the `symphony` running init.
fn left_beside(tree: &Tree, path: &str, beside: Option<&Path>) -> Vec<String> {
    let folder = tree.path("machine");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::create_dir_all(tree.path("Lamp Shop")).unwrap();
    program(tree, "tools/just");
    let path = std::ffi::OsString::from(format!("{path}:{}", tree.path("tools").display()));
    steps(
        &tree.path("Lamp Shop"),
        &Machine {
            folder: Some(&folder),
            path: Some(&path),
            surreal: None,
            beside,
        },
    )
}

/// The link step for no build anywhere, and the build step after it.
const NO_BUILDS: [&str; 2] = [
    "link the project's builds at .target/bin/symphony, .target/bin/tasks, \
     .target/bin/agent-send and .target/bin/symphony-server, or put them on PATH: the heartbeat, \
     the memory guard, `just symphony-cli`, `just tasks`, `just agent-send` and `just symphony` \
     run them and build nothing",
    "build `symphony`, `tasks`, `agent-send` and `symphony-server` to link in a checkout of the \
     Symphony: `cargo build --locked --release --bin symphony --bin tasks --bin agent-send --bin \
     symphony-server` there leaves them in its .target/release/",
];

#[test]
fn a_fresh_project_is_told_every_step_it_has_left() {
    let tree = Tree::new();
    std::fs::create_dir_all(tree.path("Lamp Shop")).unwrap();
    init(&tree.path("Lamp Shop"), &kit(vec![])).unwrap();
    assert_eq!(
        left(&tree, ""),
        [
            "write symphony.toml to name the project (until then it is `lamp-shop`, after its \
             folder) and to set its agents, verdicts and board: \
             https://github.com/heavenstudio/symphony/blob/main/docs/PROJECT_FILE.md lists its \
             tables and keys",
            "make docs/ for the documents the Symphony serves at /lamp-shop/docs/, or name the \
             folders it serves with [[entrypoints]] in symphony.toml",
            NO_BUILDS[0],
            NO_BUILDS[1],
            "register the project with the machine's Symphony: `just symphony-cli project add` \
             (a start of `just symphony` registers it too)",
        ]
    );
}

#[test]
fn every_checkout_layout_is_a_checkout_and_a_git_dir_is_none() {
    use crate::test_layouts::{git, others, repository};
    let tree = Tree::new();
    let (main, linked) = repository(&tree.path("lantern"));
    let found = others(&tree.root);
    // A worktree of the bare repository placed inside it has a `.git` of its own.
    let inside = found.bare.join("inside");
    git(
        &found.bare,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "inside",
            inside.to_str().unwrap(),
        ],
    );
    let in_git_dir_here = |root: &Path| {
        let common = symphony_board::git::common_dir(root).unwrap();
        in_git_dir(root, &common)
    };
    for checkout in [
        &main,
        &linked,
        &main.join("docs"),
        &found.submodule,
        &found.separate,
        &found.bare_worktree,
        &inside,
    ] {
        assert!(!in_git_dir_here(checkout), "{checkout:?} is a checkout");
    }
    for git_dir in [
        main.join(".git"),
        main.join(".git/refs"),
        tree.path("outer/.git/modules/sub"),
        tree.path("sep.gitdir"),
        found.bare.clone(),
        found.bare.join("refs"),
    ] {
        assert!(in_git_dir_here(&git_dir), "{git_dir:?} is in a git dir");
    }
    assert!(!in_git_dir(&tree.path("gone"), &main.join(".git")));
}

#[test]
fn a_checkout_is_told_to_make_its_board_until_its_store_is_there() {
    let tree = Tree::new();
    tree.write("Lamp Shop/.git/HEAD", "ref: refs/heads/main\n");
    // After the PostgreSQL a new board is made on, before the registration the Symphony reads it
    // by; and no SurrealDB is asked for.
    let steps = left(&tree, "");
    let at = |start: &str| {
        steps
            .iter()
            .position(|step| step.starts_with(start))
            .unwrap_or_else(|| panic!("{start}: {steps:?}"))
    };
    assert_eq!(steps[at("make the board")], MAKE_BOARD);
    assert!(MAKE_BOARD.contains("on PostgreSQL"), "{MAKE_BOARD}");
    assert_eq!(at("have PostgreSQL") + 1, at("make the board"), "{steps:?}");
    assert_eq!(at("make the board") + 1, at("register "), "{steps:?}");
    assert!(
        steps.iter().all(|step| !step.contains("SurrealDB")),
        "{steps:?}"
    );
    // A store a server left: a board, as far as the folder tells with nothing running, on
    // SurrealDB, whose server is asked for in place of PostgreSQL.
    tree.write("Lamp Shop/.git/symphony/tasks/data/manifest", "");
    let steps = left(&tree, "");
    assert!(!steps.contains(&MAKE_BOARD.to_string()), "{steps:?}");
    assert!(
        steps.iter().any(|step| step.starts_with("fetch SurrealDB")),
        "{steps:?}"
    );
    assert!(
        steps.iter().all(|step| !step.contains("PostgreSQL")),
        "{steps:?}"
    );
    // A stopped PostgreSQL board is made too: its cluster is there, its server is not.
    std::fs::remove_dir_all(tree.path("Lamp Shop/.git/symphony/tasks/data")).unwrap();
    tree.write(
        "Lamp Shop/.git/symphony/tasks/backend.json",
        r#"{"format":1,"engine":"postgresql"}"#,
    );
    tree.write(
        "Lamp Shop/.git/symphony/tasks/postgresql/PG_VERSION",
        "18\n",
    );
    let steps = left(&tree, "");
    assert!(!steps.contains(&MAKE_BOARD.to_string()), "{steps:?}");
    assert!(
        steps.iter().any(|step| step.starts_with("have PostgreSQL")),
        "{steps:?}"
    );
    // Outside a checkout no board is asked for.
    let outside = Tree::new();
    assert!(!unmade_board(&outside.root, None));
    assert_eq!(board_engine(&outside.root, None), None);
}

#[test]
fn a_board_on_postgresql_is_told_the_setup_of_postgresql_until_discovery_finds_it() {
    let tree = Tree::new();
    tree.write("Lamp Shop/.git/HEAD", "ref: refs/heads/main\n");
    let asked = |path: &str| {
        left(&tree, path)
            .into_iter()
            .filter(|step| step.contains("PostgreSQL") && step != MAKE_BOARD)
            .collect::<Vec<_>>()
    };
    let told = asked("");
    assert_eq!(told.len(), 1, "{told:?}");
    let why = symphony_board::server::postgres::discover_with_path(
        &tree.path("Lamp Shop"),
        Some(std::ffi::OsStr::new(
            &tree.path("tools").display().to_string(),
        )),
    )
    .unwrap_err();
    assert_eq!(told[0], postgresql_step(&why));
    assert!(
        told[0].starts_with("have PostgreSQL 18 or later for the board"),
        "{told:?}"
    );
    // The setup line, as the board's own refusal says it.
    assert!(told[0].contains("brew install postgresql@18"), "{told:?}");
    assert!(told[0].contains("[board] postgresql"), "{told:?}");
    // Found on the search path, it is asked for no more, and the board is still to be made.
    for tool in ["initdb", "pg_ctl", "psql", "postgres"] {
        program(&tree, &format!("pg/{tool}"));
    }
    let release = |said: &str| {
        tree.write(
            "pg/postgres",
            &format!("#!/bin/sh\necho 'postgres (PostgreSQL) {said}'\n"),
        )
    };
    let programs = tree.path("pg").display().to_string();
    release("18.7");
    assert_eq!(asked(&programs), Vec::<String>::new());
    assert!(left(&tree, &programs).contains(&MAKE_BOARD.to_string()));
    // An older PostgreSQL is no PostgreSQL for the board: its refusal is the step.
    release("17.2");
    let told = asked(&programs);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].contains("is PostgreSQL 17.2; the board runs on 18 or later"),
        "{told:?}"
    );
}

#[test]
fn a_machine_without_just_is_told_to_install_it() {
    let tree = Tree::new();
    let folder = tree.path("machine");
    std::fs::create_dir_all(&folder).unwrap();
    let path = std::ffi::OsString::from("/nowhere");
    let told = |path: &std::ffi::OsString| {
        steps(
            &tree.root,
            &Machine {
                folder: Some(&folder),
                path: Some(path),
                surreal: None,
                beside: None,
            },
        )
        .into_iter()
        .filter(|step| step.contains("`just`"))
        .collect::<Vec<_>>()
    };
    assert_eq!(
        told(&path),
        [
            "install `just` (https://just.systems) and put it on PATH: the skills, the agents and \
             the owner run the board and the Symphony by its recipes (`just tasks`, `just \
             symphony`)"
        ]
    );
    program(&tree, "tools/just");
    let path = std::ffi::OsString::from(format!("/nowhere:{}", tree.path("tools").display()));
    assert_eq!(told(&path), Vec::<String>::new());
}

#[test]
fn each_build_is_told_with_what_runs_it_and_every_one_the_recipes_run_is_there() {
    assert_eq!(
        runners("symphony", &[]),
        ["the heartbeat", "the memory guard", "`just symphony-cli`"]
    );
    assert_eq!(runners("tasks", &[]), ["the heartbeat", "`just tasks`"]);
    assert_eq!(runners("symphony-server", &[]), ["`just symphony`"]);
    assert_eq!(runners("elsewhere", &[]), Vec::<String>::new());
    // A recipe of the project's own replaces the Symphony's, which runs the build no more.
    let own = ["tasks", "symphony", "symphony-cli"];
    assert_eq!(runners("tasks", &own), ["the heartbeat"]);
    assert_eq!(
        runners("symphony", &own),
        ["the heartbeat", "the memory guard"]
    );
    assert_eq!(runners("symphony-server", &own), Vec::<String>::new());
    for (recipe, binary) in RUNS {
        assert!(BUILDS.contains(&binary), "{recipe} runs {binary}");
    }
    assert_eq!(BUILDS.len(), RUNS.len());
    for name in HEARTBEAT_BUILDS {
        assert!(BUILDS.contains(&name), "{name}");
    }
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
    assert_eq!(
        heartbeat_warning(&tree.path("Lamp Shop"), &builds::Beyond::default()),
        None
    );
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
fn builds_on_the_search_path_serve_the_recipes_and_the_hooks_alike() {
    let tree = Tree::new();
    tree.write("Lamp Shop/symphony.toml", "[project]\nid = \"lamps\"\n");
    tree.write("Lamp Shop/docs/README.md", "# Lamps\n");
    init(&tree.path("Lamp Shop"), &kit(vec![])).unwrap();
    for name in BUILDS {
        program(&tree, &format!("it's here/{name}"));
    }
    let folder = tree.path("it's here");
    let path = format!("/nowhere:{}", folder.display());
    let steps = left(&tree, &path);
    assert_eq!(steps.len(), 1, "{steps:?}");
    assert!(steps[0].starts_with("register the project"), "{steps:?}");
    let path = std::ffi::OsString::from(path);
    assert_eq!(
        heartbeat_warning(&tree.path("Lamp Shop"), &builds::Beyond::path(Some(&path))),
        None
    );
}

#[test]
fn builds_beside_the_running_symphony_are_told_how_to_link_and_the_rest_how_to_build() {
    let tree = Tree::new();
    tree.write("Lamp Shop/symphony.toml", "[project]\nid = \"lamps\"\n");
    tree.write("Lamp Shop/docs/README.md", "# Lamps\n");
    init(&tree.path("Lamp Shop"), &kit(vec![])).unwrap();
    // A Symphony checkout's release folder, its name quoted as a shell word, holding two builds
    // and a `tasks` that cannot run.
    let release = tree.path("it's built/.target/release");
    for name in ["symphony", "symphony-server"] {
        program(&tree, &format!("it's built/.target/release/{name}"));
    }
    tree.write("it's built/.target/release/tasks", "not runnable");
    let quoted = |name: &str| {
        release
            .join(name)
            .display()
            .to_string()
            .replace('\'', "'\\''")
    };
    let steps = left_beside(&tree, "", Some(&release));
    assert_eq!(
        steps[..2],
        [
            format!(
                "{} (`mkdir -p .target/bin && ln -s '{}' .target/bin/symphony && ln -s '{}' \
                 .target/bin/symphony-server`)",
                NO_BUILDS[0],
                quoted("symphony"),
                quoted("symphony-server")
            ),
            "build `tasks` and `agent-send` to link in a checkout of the Symphony: `cargo build \
             --locked --release --bin tasks --bin agent-send` there leaves them in its \
             .target/release/"
                .to_string(),
        ]
    );
    // All of them there, none is left to build.
    for name in ["tasks", "agent-send"] {
        program(&tree, &format!("it's built/.target/release/{name}"));
    }
    let steps = left_beside(&tree, "", Some(&release));
    assert!(steps[0].starts_with(NO_BUILDS[0]), "{steps:?}");
    assert!(steps[1].starts_with("register the project"), "{steps:?}");
}

#[test]
fn one_build_left_to_link_is_told_in_the_singular() {
    let tree = Tree::new();
    tree.write("Lamp Shop/symphony.toml", "[project]\nid = \"lamps\"\n");
    tree.write("Lamp Shop/docs/README.md", "# Lamps\n");
    init(&tree.path("Lamp Shop"), &kit(vec![])).unwrap();
    for name in ["symphony", "tasks", "agent-send"] {
        program(&tree, &format!("Lamp Shop/.target/bin/{name}"));
    }
    let steps = left(&tree, "");
    assert_eq!(
        steps[..2],
        [
            "link the project's build at .target/bin/symphony-server, or put it on PATH: \
             `just symphony` runs it and builds nothing",
            "build `symphony-server` to link in a checkout of the Symphony: `cargo build \
             --locked --release --bin symphony-server` there leaves it in its .target/release/",
        ]
    );
    std::fs::remove_file(tree.path("Lamp Shop/.target/bin/symphony")).unwrap();
    std::fs::remove_file(tree.path("Lamp Shop/.target/bin/agent-send")).unwrap();
    let steps = left(&tree, "");
    assert!(
        steps[0].starts_with(
            "link the project's builds at .target/bin/symphony, .target/bin/agent-send and \
             .target/bin/symphony-server, or put them on PATH: the heartbeat, the memory guard, \
             `just symphony-cli`, `just agent-send` and `just symphony` run them and build nothing"
        ),
        "{steps:?}"
    );
}

#[test]
fn a_linked_worktree_finds_its_main_checkouts_builds_as_the_hooks_do() {
    let tree = Tree::new();
    tree.write("main/.git/HEAD", "ref: refs/heads/main\n");
    tree.write("main/.git/worktrees/lamp/HEAD", "ref: refs/heads/lamp\n");
    let main = tree.path("main").canonicalize().unwrap();
    tree.write(
        "lamp/.git",
        &format!("gitdir: {}\n", main.join(".git/worktrees/lamp").display()),
    );
    tree.write("main/.git/worktrees/lamp/commondir", "../..\n");
    for name in BUILDS {
        program(&tree, &format!("main/.target/bin/{name}"));
    }
    let found = |root: &Path| {
        steps(
            root,
            &Machine {
                folder: None,
                path: None,
                surreal: Some(Path::new("/bin/sh")),
                beside: None,
            },
        )
        .into_iter()
        .filter(|step| step.contains(".target/bin/"))
        .collect::<Vec<_>>()
    };
    assert_eq!(found(&tree.path("lamp")), Vec::<String>::new());
    assert_eq!(
        heartbeat_warning(&tree.path("lamp"), &builds::Beyond::default()),
        None
    );
    std::fs::remove_file(tree.path("main/.target/bin/tasks")).unwrap();
    assert_eq!(
        heartbeat_warning(&tree.path("lamp"), &builds::Beyond::default()).unwrap(),
        "warning: tasks has no build at .target/bin/ or on PATH: no tool call counts as a beat \
         until it does (see Next)"
    );
}

#[test]
fn a_justfiles_own_recipes_are_its_heads_never_its_settings_assignments_or_bodies() {
    let text = "\
set allow-duplicate-recipes
import 'symphony.just'
version := \"1:2\"
alias t := tasks
# fmt: a comment
fmt:
    tools/cargo.sh fmt --all
test-rs *ARGS:
    tools/cargo.sh test \"$@\"
client-check OUT=\"build/verified-client\":
    true
symphony: (fmt)
    run: indented, a body
[private]
_lint:
    true
Upper:
    true
nocolon
";
    assert_eq!(
        defined(text),
        ["fmt", "test-rs", "client-check", "symphony"]
    );
    assert_eq!(defined(""), Vec::<&str>::new());
}

#[test]
fn a_build_only_recipes_the_project_replaced_would_run_is_not_asked_for() {
    let tree = Tree::new();
    tree.write("Lamp Shop/symphony.toml", "[project]\nid = \"lamps\"\n");
    tree.write("Lamp Shop/docs/README.md", "# Lamps\n");
    // The project runs its own server and board by recipes of its own: `just symphony` and
    // `just tasks` never run the Symphony's builds, though the heartbeat still runs `tasks`.
    tree.write(
        "Lamp Shop/justfile",
        "import 'symphony.just'\n\nsymphony *ARGS:\n    cargo run --bin symphony-server\n\n\
         tasks *ARGS:\n    cargo run --bin tasks\n",
    );
    init(&tree.path("Lamp Shop"), &kit(vec![])).unwrap();
    program(&tree, "Lamp Shop/.target/bin/symphony");
    program(&tree, "Lamp Shop/.target/bin/agent-send");
    let steps = left(&tree, "");
    assert_eq!(
        steps[..2],
        [
            "link the project's build at .target/bin/tasks, or put it on PATH: the heartbeat \
             runs it and builds nothing",
            "build `tasks` to link in a checkout of the Symphony: `cargo build --locked --release \
             --bin tasks` there leaves it in its .target/release/",
        ]
    );
    assert!(steps[2].starts_with("register the project"), "{steps:?}");
}

#[test]
fn a_justfile_of_the_projects_without_the_import_is_told_how_to_take_the_recipes() {
    let tree = Tree::new();
    tree.write("Lamp Shop/Justfile", "build:\n    true\n");
    init(&tree.path("Lamp Shop"), &kit(vec![])).unwrap();
    let step = "import the recipes in Justfile: add `import 'symphony.just'`; a recipe of its \
                own of the same name (`land`, say) replaces the Symphony's";
    assert!(left(&tree, "").contains(&step.to_string()));
    tree.write("Lamp Shop/Justfile", "import 'symphony.just'\n");
    assert!(
        left(&tree, "")
            .iter()
            .all(|step| !step.contains("import") && !step.contains("allow-duplicate")),
        "{:?}",
        left(&tree, "")
    );
}

/// The shipped recipes' setting, as the kits below carry it.
fn setting_kit() -> Shipped {
    Shipped {
        recipes: Some(format!("{SETTING}\ntasks *ARGS:\n    @echo tasks\n")),
        ..kit(vec![])
    }
}

/// What init wrote as the recipes, its stamp line left out.
fn written_recipes(tree: &Tree) -> String {
    let text = tree.read("symphony.just");
    text.split_once('\n').unwrap().1.to_string()
}

#[test]
fn a_justfile_setting_what_the_recipes_set_gets_them_with_the_setting_left_to_it() {
    let tree = Tree::new();
    let shipped = setting_kit();
    for text in [
        "set allow-duplicate-recipes\nimport 'symphony.just'\n",
        "set allow-duplicate-recipes := true # ours\nimport 'symphony.just'\n",
        "  set   allow-duplicate-recipes\nimport './symphony.just'\n",
    ] {
        tree.write("Justfile", text);
        let report = init(&tree.root, &shipped).unwrap();
        assert_eq!(
            written_recipes(&tree),
            format!("{SETTING_LEFT}\ntasks *ARGS:\n    @echo tasks\n"),
            "{text:?}"
        );
        assert!(!sets_duplicates(&written_recipes(&tree)));
        assert_ne!(
            outcome(&report, "symphony.just"),
            "kept (the project's own)"
        );
        // Its justfile is its own, never written.
        assert_eq!(tree.read("Justfile"), text);
    }
    // Once the project takes its setting out, the recipes make it again.
    tree.write("Justfile", "import 'symphony.just'\n");
    let report = init(&tree.root, &shipped).unwrap();
    assert_eq!(outcome(&report, "symphony.just"), "updated");
    assert_eq!(
        written_recipes(&tree),
        format!("{SETTING}\ntasks *ARGS:\n    @echo tasks\n")
    );
    // Nothing is left for the owner to do about the setting.
    assert!(
        left(&tree, "")
            .iter()
            .all(|step| !step.contains("allow-duplicate")),
        "{:?}",
        left(&tree, "")
    );
}

#[test]
fn a_setting_in_a_file_the_justfile_imports_counts_but_not_in_the_recipes() {
    let tree = Tree::new();
    let shipped = setting_kit();
    tree.write("settings.just", "set allow-duplicate-recipes\n");
    tree.write(
        "justfile",
        "import 'settings.just'\nimport? 'missing.just'\nimport 'symphony.just'\n",
    );
    init(&tree.root, &shipped).unwrap();
    assert!(written_recipes(&tree).starts_with(SETTING_LEFT));
    // The recipes' own setting is not the project's: a second run writes the same.
    tree.write("settings.just", "set positional-arguments\n");
    init(&tree.root, &shipped).unwrap();
    assert!(written_recipes(&tree).starts_with(SETTING));
    init(&tree.root, &shipped).unwrap();
    assert!(written_recipes(&tree).starts_with(SETTING));
    // No justfile: the recipes set it, and the justfile init writes does not.
    let fresh = Tree::new();
    init(&fresh.root, &shipped).unwrap();
    assert!(written_recipes(&fresh).starts_with(SETTING));
    assert!(!sets_duplicates(&fresh.read("justfile")));
}

#[test]
fn the_justfile_init_wrote_setting_it_takes_the_recipes_that_set_it() {
    let tree = Tree::new();
    tree.write("justfile", JUSTFILE_SETTING_ITSELF);
    let report = init(&tree.root, &setting_kit()).unwrap();
    assert_eq!(outcome(&report, "justfile"), "updated");
    assert_eq!(tree.read("justfile"), JUSTFILE);
    assert!(written_recipes(&tree).starts_with(SETTING));
}

#[test]
fn the_shipped_recipes_make_the_setting_on_one_line_init_can_leave_out() {
    let shipped = Shipped::builtin();
    let recipes = shipped.recipes.as_deref().unwrap();
    assert_eq!(recipes.matches(SETTING).count(), 1);
    assert!(recipes.lines().any(|line| format!("{line}\n") == SETTING));
    let left = recipes.replacen(SETTING, SETTING_LEFT, 1);
    assert!(!sets_duplicates(&left));
    assert!(SETTING_LEFT.starts_with('#'));
}

#[test]
fn only_the_setting_itself_counts_as_setting_duplicates() {
    for text in [
        "set allow-duplicate-recipes\n",
        "set allow-duplicate-recipes := true\n",
        "\tset allow-duplicate-recipes # why\n",
    ] {
        assert!(sets_duplicates(text), "{text:?}");
    }
    for text in [
        "",
        "# set allow-duplicate-recipes\n",
        "set allow-duplicate-variables\n",
        "set allow-duplicate-recipes-please\n",
        "setallow-duplicate-recipes\n",
        "reset allow-duplicate-recipes\n",
        "build:\n    echo set allow-duplicate-recipes\n",
    ] {
        assert!(!sets_duplicates(text), "{text:?}");
    }
}

#[test]
fn the_justfile_init_wrote_with_the_setting_is_brought_to_the_import_alone() {
    let tree = Tree::new();
    tree.write("justfile", JUSTFILE_SETTING_ITSELF);
    let report = init(&tree.root, &kit(vec![])).unwrap();
    assert_eq!(outcome(&report, "justfile"), "updated");
    assert_eq!(tree.read("justfile"), JUSTFILE);
    assert!(!sets_duplicates(JUSTFILE));
    let report = init(&tree.root, &kit(vec![])).unwrap();
    assert_eq!(outcome(&report, "justfile"), "current");
    // The same text the project changed by a line is its own.
    let own = format!("{JUSTFILE_SETTING_ITSELF}\nbuild:\n    true\n");
    tree.write("justfile", &own);
    init(&tree.root, &kit(vec![])).unwrap();
    assert_eq!(tree.read("justfile"), own);
}

#[test]
fn the_shipped_recipes_set_what_lets_the_projects_own_replace_them() {
    let shipped = Shipped::builtin();
    let recipes = shipped.recipes.as_deref().unwrap();
    assert!(sets_duplicates(recipes));
    assert!(!sets_duplicates(JUSTFILE));
    assert!(imports(JUSTFILE));
    let land = recipes
        .lines()
        .skip_while(|line| !line.starts_with("land "))
        .nth(1)
        .unwrap();
    assert!(land.ends_with(" tasks land \"$@\""), "{land}");
    // It alone sets the marker `tasks land` runs by: `just tasks land` is refused.
    assert!(
        land.starts_with(&format!("    @{}=1 ", symphony_board::land::RECIPE)),
        "{land}"
    );
}

#[test]
fn retiring_the_recipes_removes_the_stamped_copy_and_the_justfile_init_wrote() {
    let tree = Tree::new();
    init(&tree.root, &kit(vec![])).unwrap();
    let retired = Shipped {
        recipes: None,
        ..kit(vec![])
    };
    let report = init(&tree.root, &retired).unwrap();
    assert_eq!(outcome(&report, "symphony.just"), "removed");
    assert_eq!(outcome(&report, "justfile"), "removed");
    assert!(!tree.path("symphony.just").exists());
    assert!(!tree.path("justfile").exists());
    // Nothing is left to retire a second time.
    let report = init(&tree.root, &retired).unwrap();
    assert!(
        report
            .iter()
            .all(|(name, _)| name != "symphony.just" && name != "justfile"),
        "{report:?}"
    );
}

#[test]
fn retiring_the_recipes_takes_only_the_stamped_import_out_of_a_justfile_grown_since() {
    let tree = Tree::new();
    init(&tree.root, &kit(vec![])).unwrap();
    let grown = format!("{JUSTFILE}\nland *ARGS:\n    tools/land.sh \"$@\"\n");
    tree.write("justfile", &grown);
    let retired = Shipped {
        recipes: None,
        ..kit(vec![])
    };
    let report = init(&tree.root, &retired).unwrap();
    assert_eq!(outcome(&report, "justfile"), "updated");
    let kept = tree.read("justfile");
    assert!(!kept.contains("import"), "{kept}");
    assert!(
        kept.ends_with("\nland *ARGS:\n    tools/land.sh \"$@\"\n"),
        "{kept}"
    );
    // An import of the project's own, with no stamp, stays.
    tree.write("symphony.just", "# symphony-stamp: 0\n");
    tree.write("justfile", "import 'symphony.just'\n");
    let report = init(&tree.root, &retired).unwrap();
    assert_eq!(outcome(&report, "symphony.just"), "removed");
    assert!(report.iter().all(|(name, _)| name != "justfile"));
    assert_eq!(tree.read("justfile"), "import 'symphony.just'\n");
}

#[test]
fn retiring_the_recipes_keeps_a_copy_the_project_made_its_own_and_its_import() {
    let retired = Shipped {
        recipes: None,
        ..kit(vec![])
    };
    let tree = Tree::new();
    init(&tree.root, &kit(vec![])).unwrap();
    tree.write("symphony.just", "# ours\nland:\n    true\n");
    let report = init(&tree.root, &retired).unwrap();
    assert_eq!(
        outcome(&report, "symphony.just"),
        "kept (the project's own)"
    );
    assert_eq!(tree.read("justfile"), JUSTFILE);
    assert_eq!(tree.read("symphony.just"), "# ours\nland:\n    true\n");

    let linked = Tree::new();
    init(&linked.root, &kit(vec![])).unwrap();
    linked.write("shared/recipes.just", "# symphony-stamp: 0\n");
    std::fs::remove_file(linked.path("symphony.just")).unwrap();
    std::os::unix::fs::symlink("shared/recipes.just", linked.path("symphony.just")).unwrap();
    let report = init(&linked.root, &retired).unwrap();
    assert_eq!(
        outcome(&report, "symphony.just"),
        "kept (the project's own)"
    );
    assert_eq!(linked.read("justfile"), JUSTFILE);
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
fn the_warning_names_the_missing_builds_and_what_goes_without_them() {
    let tree = Tree::new();
    assert_eq!(
        heartbeat_warning(&tree.root, &builds::Beyond::default()).unwrap(),
        "warning: symphony and tasks have no build at .target/bin/ or on PATH: no tool call \
         counts as a beat and the memory guard refuses nothing until they do (see Next)"
    );
    program(&tree, ".target/bin/symphony");
    assert_eq!(
        heartbeat_warning(&tree.root, &builds::Beyond::default()).unwrap(),
        "warning: tasks has no build at .target/bin/ or on PATH: no tool call counts as a beat \
         until it does (see Next)"
    );
    std::fs::remove_file(tree.path(".target/bin/symphony")).unwrap();
    program(&tree, ".target/bin/tasks");
    assert_eq!(
        heartbeat_warning(&tree.root, &builds::Beyond::default()).unwrap(),
        "warning: symphony has no build at .target/bin/ or on PATH: no tool call counts as a \
         beat and the memory guard refuses nothing until it does (see Next)"
    );
    // On the search path it counts, as the hooks find it there.
    program(&tree, "bin/symphony");
    let path = std::ffi::OsString::from(format!("/nowhere:{}", tree.path("bin").display()));
    assert_eq!(
        heartbeat_warning(&tree.root, &builds::Beyond::path(Some(&path))),
        None
    );
}

#[test]
fn a_release_installed_on_the_machine_is_the_beats_tasks_and_never_the_hooks_symphony() {
    // A project with no build of its own, and a release holding both binaries: its recipes run
    // the release, but the hooks' shell finds `symphony` by the links and the search path alone.
    let tree = Tree::new();
    program(&tree, "machine/releases/release-1/bin/symphony");
    program(&tree, "machine/releases/release-1/bin/tasks");
    std::os::unix::fs::symlink("releases/release-1", tree.path("machine/current")).unwrap();
    let project = tree.path("lamp");
    std::fs::create_dir_all(&project).unwrap();
    let released = |path: Option<&std::ffi::OsStr>| builds::Beyond {
        machine: Some(tree.path("machine")),
        path: path.map(std::ffi::OsStr::to_os_string),
    };
    assert_eq!(
        heartbeat_warning(&project, &released(None)).unwrap(),
        "warning: symphony has no build at .target/bin/ or on PATH: no tool call counts as a \
         beat and the memory guard refuses nothing until it does (see Next)"
    );
    // The recipes do find it there: no step asks for a link.
    assert_eq!(
        unfound(&project, &["symphony", "tasks"], &released(None)),
        [] as [&str; 0]
    );
    // With `symphony` where the hooks look, the beat's `tasks` is the release's: nothing to warn.
    program(&tree, "bin/symphony");
    let path = tree.path("bin").into_os_string();
    assert_eq!(heartbeat_warning(&project, &released(Some(&path))), None);
    program(&tree, "lamp/.target/bin/symphony");
    assert_eq!(heartbeat_warning(&project, &released(None)), None);
    // Without the release, the same project lacks `tasks` as well.
    assert_eq!(
        heartbeat_warning(&project, &builds::Beyond::default()).unwrap(),
        "warning: tasks has no build at .target/bin/ or on PATH: no tool call counts as a beat \
         until it does (see Next)"
    );
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
            beside: None,
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
                beside: None,
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
    for line in shipped.recipes.as_deref().unwrap().lines() {
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
    // A board whose store is SurrealDB's, as a board made before PostgreSQL was a new board's.
    tree.write("Lamp Shop/.git/symphony/tasks/data/manifest", "");
    let root = tree.path("Lamp Shop");
    let pinned = root
        .canonicalize()
        .unwrap()
        .join(".git/symphony/surreal")
        .join(symphony_board::server::SURREAL_VERSION)
        .join("surreal");
    let asked = |named: Option<&Path>| {
        let found = steps(
            &root,
            &Machine {
                folder: None,
                path: None,
                surreal: named,
                beside: None,
            },
        );
        let mut asked = found.into_iter().filter(|step| step.contains("SurrealDB"));
        let step = asked.next();
        assert_eq!(asked.next(), None);
        step
    };
    let version = symphony_board::server::SURREAL_VERSION;
    let fetch = symphony_board::server::surreal_fetch(
        &pinned,
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
    .expect("a machine the tests run on has a pinned build");
    let fetched = format!(
        "fetch SurrealDB {version}, which the board's server runs from {}: `{fetch}` (or name \
         another with SYMPHONY_SURREAL)",
        pinned.display()
    );
    assert_eq!(asked(None), Some(fetched.clone()));
    let named = tree.path("elsewhere/surreal");
    assert_eq!(
        asked(Some(&named)),
        Some(format!(
            "put SurrealDB {version} at {}, where SYMPHONY_SURREAL names it: the board's server \
             runs it",
            named.display()
        ))
    );
    program(&tree, "elsewhere/surreal");
    assert_eq!(asked(Some(&named)), None);
    tree.write(
        &format!("Lamp Shop/.git/symphony/surreal/{version}/surreal"),
        "not runnable",
    );
    assert_eq!(asked(None), Some(fetched));
    // A machine no build is pinned for is told to put one there.
    assert_eq!(
        surreal_step(&pinned, false, None),
        format!(
            "put SurrealDB {version}'s release binary for this machine at {}, where the board's \
             server runs it, or name one with SYMPHONY_SURREAL",
            pinned.display()
        )
    );
    program(
        &tree,
        &format!("Lamp Shop/.git/symphony/surreal/{version}/surreal"),
    );
    assert_eq!(asked(None), None);
    let outside = Tree::new();
    assert_eq!(missing_surreal(&outside.root, None), None);
}
