//! `[limits]` and `[agents.models]`: settings the board reads from the file itself, each known
//! here by name, so a misspelt one is refused where it stands, and a `[limits]` value the board
//! cannot read refused by a load and the lint with its line, while the running project reads on.

use super::{folder, parsed, refused};
use crate::Tree;
use symphony::lint::project_problems;
use symphony::project::{FILE, Models, Project};

#[test]
fn no_table_is_no_model_and_empty_tables_are_the_same_project_as_none() {
    assert_eq!(parsed("").unwrap().agents.models, Models::default());
    assert_eq!(
        parsed("[agents.models]\n[limits]\n").unwrap(),
        parsed("").unwrap()
    );
}

#[test]
fn each_roles_model_and_each_limit_is_a_setting() {
    let project = parsed(
        "[agents.models]\norchestrator = \"fable\"\nexecutor = \"opus\"\nreviewer = \"sonnet\"\n\
         assessor = \"haiku\"\n\n\
         [limits]\norchestrator_slots = 5\nlow_volume = 0\nmachine_agents = 6\n",
    )
    .unwrap();
    assert_eq!(
        project.agents.models,
        Models {
            orchestrator: Some("fable".into()),
            executor: Some("opus".into()),
            reviewer: Some("sonnet".into()),
            assessor: Some("haiku".into()),
        }
    );
    // An `[agents]` table writing its launch inline beside them, as this repository's does.
    let project = parsed(
        "[agents]\nlaunch = { default = \"claude\", model = \"opus\" }\n\n[agents.models]\nreviewer = \"sonnet\"\n",
    )
    .unwrap();
    assert_eq!(project.agents.launch.model.as_deref(), Some("opus"));
    assert_eq!(project.agents.models.reviewer.as_deref(), Some("sonnet"));
}

#[test]
fn a_key_neither_table_knows_is_refused_by_its_name_and_line() {
    assert_eq!(
        refused("[limits]\nlow_volume = 2\nwip = 3\n"),
        "symphony.toml: [limits] wip (line 3) is not a setting the Symphony knows"
    );
    assert_eq!(
        refused("[agents.models]\nplanner = \"opus\"\n"),
        "symphony.toml: [agents.models] planner (line 2) is not a setting the Symphony knows"
    );
    assert_eq!(
        refused("limits.staff = 1\n"),
        "symphony.toml: [limits] staff (line 1) is not a setting the Symphony knows"
    );
}

#[test]
fn a_limit_the_board_cannot_read_is_refused_with_its_line_as_the_board_says_it() {
    for (text, said) in [
        (
            "[limits]\nlow_volume = -1\n",
            "symphony.toml: [limits] low_volume (line 2) is -1; it is at least 0; it takes its default, 4",
        ),
        (
            "[limits]\n\nmachine_agents = 0\n",
            "symphony.toml: [limits] machine_agents (line 3) is 0; a machine runs at least one agent; no limit on the agents this machine runs applies",
        ),
        (
            "[limits]\norchestrator_slots = \"8\"\n",
            "symphony.toml: [limits] orchestrator_slots (line 2) is \"8\"; not a whole number; no cap on an orchestrator's open tasks applies",
        ),
        (
            "[limits]\norchestrator_slots = 2\n\n[board]\norchestrator_slots = 9\n",
            "symphony.toml: [board] orchestrator_slots (line 5) is 9, left out: [limits] orchestrator_slots (line 2) is 2, which holds",
        ),
    ] {
        assert_eq!(refused(text), said, "{text}");
    }
    // The same cap written in both tables, as a project stages the move, is no problem.
    let staged = "[limits]\norchestrator_slots = 8\n\n[board]\norchestrator_slots = 8\n";
    assert_eq!(parsed(staged).unwrap().board.orchestrator_slots, Some(8));
}

#[test]
fn a_model_that_is_no_text_is_refused_where_it_stands() {
    let why = refused("[agents.models]\nexecutor = 4\n");
    assert!(why.starts_with("symphony.toml: "), "{why}");
    assert!(why.contains("line 2") && why.contains("executor"), "{why}");
}

#[test]
fn the_lint_names_a_bad_limit_and_the_running_project_reads_the_rest() {
    let tree = Tree::new();
    let root = folder(&tree, "Lamp Works");
    std::fs::write(
        root.join(FILE),
        "[project]\nname = \"Lamp\"\n\n[limits]\nlow_volume = \"few\"\nmachine_agents = 4\n",
    )
    .unwrap();
    let said = "symphony.toml: [limits] low_volume (line 5) is \"few\"; not a whole number; it takes its default, 4";
    assert_eq!(project_problems(&root), [said]);
    assert_eq!(Project::load(&root).unwrap_err(), said);
    // Its name is the file's, not the folder's: the rest of the file is read.
    assert_eq!(Project::of(&root).name, "Lamp");
}
