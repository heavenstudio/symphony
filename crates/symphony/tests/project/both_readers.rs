//! The keys of `symphony.toml` that the board reads itself, beside the Symphony's load, are each
//! judged by one rule in the board's crate, which both readers call. Fed each key's edge values,
//! the Symphony's load (`Project::parse`) and the board's own reader give the same answer, or the
//! same refusal naming the key. Where the board reads a value it refuses as unwritten (an id), it
//! reads what it reads with none written.

use crate::Tree;
use symphony::project::{FILE, Project};
use symphony_board::{land, launch, limits, live};

/// A main checkout, as the board finds one, holding the project file `text`, and what the
/// Symphony's load makes of it.
fn checkout(text: &str) -> (Tree, Result<Project, String>) {
    let tree = Tree::checkout();
    tree.write(FILE, text);
    let loaded = Project::parse(&tree.root, text);
    (tree, loaded)
}

/// `refusal`, the Symphony's load's, without the file's name it starts with.
fn said(refusal: &str) -> &str {
    refusal
        .strip_prefix(&format!("{FILE}: "))
        .unwrap_or_else(|| panic!("{refusal:?} names no file"))
}

#[test]
fn a_project_id_is_a_slug_to_both_and_the_board_reads_another_as_unwritten() {
    for (id, taken) in [
        ("lamp-shop", true),
        ("x9", true),
        ("", false),
        (" ", false),
        ("Lamp Shop", false),
        ("lamp_shop", false),
        ("-lamp", false),
        ("lamp--shop", false),
    ] {
        let (tree, loaded) = checkout(&format!("[project]\nid = {id:?}\n"));
        let board = symphony_board::project::find(&tree.root, None).unwrap().id;
        if taken {
            assert_eq!(symphony_board::project::id(id), Ok(id));
            assert_eq!(loaded.unwrap().id, id);
            assert_eq!(board, id);
        } else {
            let refusal = symphony_board::project::id(id).unwrap_err();
            assert!(refusal.starts_with("[project] id "), "{refusal}");
            assert_eq!(said(&loaded.unwrap_err()), refusal);
            tree.write(FILE, "");
            let unwritten = symphony_board::project::find(&tree.root, None).unwrap().id;
            assert_eq!(board, unwritten, "{id:?} is read as unwritten");
        }
    }
}

#[test]
fn a_default_branch_is_read_as_written_by_the_load_the_land_and_the_live_board_or_refused_by_all() {
    let (tree, loaded) = checkout("");
    assert_eq!(loaded.unwrap().default_branch, "main");
    assert_eq!(land::Settings::parse("").unwrap().default_branch, "main");
    assert_eq!(live::default_branch(Some(&tree.root)), Ok(None));
    for (branch, taken) in [
        ("trunk", true),
        ("release/2", true),
        (" dev ", true),
        ("", false),
        ("  ", false),
        ("\t", false),
    ] {
        let text = format!("[project]\ndefault_branch = {branch:?}\n");
        let (tree, loaded) = checkout(&text);
        let landed = land::Settings::parse(&text).map(|settings| settings.default_branch);
        let guarded = live::default_branch(Some(&tree.root));
        if taken {
            assert_eq!(loaded.unwrap().default_branch, branch);
            assert_eq!(landed.as_deref(), Ok(branch));
            assert_eq!(guarded, Ok(Some(branch.to_string())));
        } else {
            let refusal = "[project] default_branch is empty";
            assert_eq!(said(&loaded.unwrap_err()), refusal, "{branch:?}");
            assert_eq!(said(&landed.unwrap_err()), refusal, "{branch:?}");
            assert_eq!(guarded, Err(refusal.to_string()), "{branch:?}");
        }
    }
}

#[test]
fn a_generator_with_no_program_is_refused_by_the_load_and_the_land_alike() {
    for (command, taken) in [
        (r#"["just", "gen"]"#, true),
        (r#"["tools/gen.sh", ""]"#, true),
        (r#"[" "]"#, true),
        ("[]", false),
        (r#"[""]"#, false),
        (r#"["", "gen"]"#, false),
    ] {
        let text = format!(
            "[land]\ngenerators = [{{ name = \"g\", outputs = \"gen/\", command = {command} }}]\n"
        );
        let (_tree, loaded) = checkout(&text);
        let loaded = loaded.map(|project| project.land.generators[0].command.clone());
        let landed =
            land::Settings::parse(&text).map(|settings| settings.generators[0].command.clone());
        assert_eq!(loaded, landed, "{command}");
        if taken {
            assert!(loaded.is_ok(), "{command}");
        } else {
            assert_eq!(
                said(&loaded.unwrap_err()),
                "[land] generator g has no command",
                "{command}"
            );
        }
    }
    // The Symphony judges a `[verdicts]` generator by the same rule, named by its table.
    let (_tree, loaded) = checkout(
        "[verdicts]\nland = [\"l\"]\ngenerators = [{ name = \"g\", outputs = \"o\", command = [\"\"] }]\n",
    );
    assert_eq!(
        said(&loaded.unwrap_err()),
        "[verdicts] generator g has no command"
    );
}

#[test]
fn the_launch_agent_is_one_a_launch_starts_to_both_and_a_blank_one_none() {
    for (agent, taken) in [
        ("codex", Some(Some("codex"))),
        ("claude", Some(Some("claude"))),
        ("", Some(None)),
        ("  ", Some(None)),
        ("gemini", None),
        ("Claude", None),
    ] {
        let (tree, loaded) = checkout(&format!("[agents.launch]\ndefault = {agent:?}\n"));
        let board = launch::Defaults::configured(&tree.root).map(|defaults| defaults.agent);
        if let Some(taken) = taken {
            assert_eq!(loaded.unwrap().agents.launch.default.as_deref(), taken);
            assert_eq!(board, Ok(taken));
        } else {
            let refusal = format!("[agents] launch default {agent:?} is not one of codex, claude");
            assert_eq!(said(&loaded.unwrap_err()), refusal);
            let file = tree.path(FILE).display().to_string();
            assert_eq!(board, Err(format!("{file}: {refusal}")));
        }
    }
}

#[test]
fn a_cap_on_an_orchestrators_slots_is_one_rule_in_either_table_for_both() {
    for table in ["board", "limits"] {
        for (value, cap) in [
            ("8", Some(8)),
            ("4294967295", Some(4_294_967_295)),
            ("4294967296", None),
            ("0", None),
            ("-1", None),
            ("\"8\"", None),
        ] {
            let text = format!("[{table}]\norchestrator_slots = {value}\n");
            let (_tree, loaded) = checkout(&text);
            let board = limits::parse(FILE, &text);
            assert_eq!(board.limits.orchestrator_slots, cap, "{text}");
            match (loaded, cap) {
                (Ok(project), Some(cap)) => {
                    assert_eq!(board.problems, Vec::<String>::new(), "{text}");
                    let held = (project.board.orchestrator_slots)
                        .map(|held| usize::try_from(held).unwrap());
                    assert_eq!(held, (table == "board").then_some(cap), "{text}");
                }
                (Err(refusal), None) => {
                    assert_eq!(board.problems, [refusal], "{text}");
                    assert!(
                        board.problems[0]
                            .starts_with(&format!("{FILE}: [{table}] orchestrator_slots (line 2)")),
                        "{text}"
                    );
                }
                (loaded, cap) => panic!("{text}: the Symphony read {loaded:?}, the board {cap:?}"),
            }
        }
    }
}
