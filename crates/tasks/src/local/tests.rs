//! What a local settings file may hold, what it is refused, and how each reader takes it.

use super::*;
use crate::launch::Defaults;
use crate::limits::{LOW, MACHINE_AGENTS, SLOTS, TASKS};

const PATH: &str = "/r/symphony.local.toml";

fn parsed(text: &str) -> Result<Local, Vec<String>> {
    parse(Path::new(PATH), text)
}

fn problems(text: &str) -> Vec<String> {
    parsed(text).expect_err(text)
}

/// A scratch main checkout's folder holding `committed` and, when given, `local`.
fn folder(name: &str, committed: Option<&str>, local: Option<&str>) -> PathBuf {
    let root = std::env::temp_dir().join(format!("symphony-local-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    if let Some(text) = committed {
        std::fs::write(root.join(crate::project::FILE), text).unwrap();
    }
    if let Some(text) = local {
        std::fs::write(root.join(FILE), text).unwrap();
    }
    root
}

#[test]
fn an_empty_file_writes_nothing() {
    for text in [
        "",
        "# mine\n",
        "[limits]\n",
        "[agents]\n",
        "[agents.models]\n",
    ] {
        let local = parsed(text).expect(text);
        assert_eq!(local.launch, [None, None, None], "{text}");
        assert_eq!(local.models, [None, None, None, None], "{text}");
        assert!(local.limits.is_empty(), "{text}");
    }
}

#[test]
fn each_preference_is_read_with_its_line() {
    let local = parsed(
        "[agents]\nlaunch = { default = \"claude\", model = \"fable\", effort = \"high\" }\n\n\
         [agents.models]\norchestrator = \"o\"\nexecutor = \"e\"\nreviewer = \"r\"\n\
         assessor = \"a\"\n\n[limits]\norchestrator_slots = 3\norchestrator_tasks = 7\n\
         low_volume = 0\nmachine_agents = 4\n",
    )
    .unwrap();
    assert_eq!(
        local.launch,
        ["claude", "fable", "high"].map(|v| Some(v.to_string()))
    );
    assert_eq!(
        local.models,
        ["o", "e", "r", "a"].map(|v| Some(v.to_string()))
    );
    for (role, model) in ROLES.iter().zip(["o", "e", "r", "a"]) {
        assert_eq!(local.model_of(role), Some(model));
    }
    assert_eq!(local.model_of("owner"), None, "no such role");
    assert_eq!(
        local.limits,
        BTreeMap::from([(SLOTS, 3), (TASKS, 7), (LOW, 0), (MACHINE_AGENTS, 4)])
    );
    assert_eq!(local.line(&["agents", "models"], "reviewer"), Some(7));
    assert_eq!(
        local
            .written("[limits] machine_agents", &["limits"], MACHINE_AGENTS)
            .said(),
        "symphony.local.toml [limits] machine_agents, line 14"
    );
    assert_eq!(
        local
            .written("[agents] launch model", &["agents", "launch"], "model")
            .said(),
        "symphony.local.toml [agents] launch model, line 2"
    );
}

#[test]
fn a_setting_that_runs_a_command_or_decides_what_lands_is_refused_by_name_with_the_reason() {
    for (text, name) in [
        ("[land]\ncheck = false\n", "[land] check (line 2)"),
        ("[land]\npush = false\n", "[land] push (line 2)"),
        (
            "[verdicts]\ncheck = [\"true\"]\n",
            "[verdicts] check (line 2)",
        ),
        ("[board]\non_land = [\"x\"]\n", "[board] on_land (line 2)"),
        (
            "[board]\non_review = [\"x\"]\n",
            "[board] on_review (line 2)",
        ),
        (
            "[board]\nmachine_work_reader = [\"x\"]\n",
            "[board] machine_work_reader (line 2)",
        ),
        (
            "[board]\npostgresql = \"/x\"\n",
            "[board] postgresql (line 2)",
        ),
        (
            "[board]\nassessments = \"a\"\n",
            "[board] assessments (line 2)",
        ),
        ("[project]\ncargo = [\"c\"]\n", "[project] cargo (line 2)"),
        (
            "[project]\ndefault_branch = \"x\"\n",
            "[project] default_branch (line 2)",
        ),
        ("[plugins]\ncrates = [\"p\"]\n", "[plugins] crates (line 2)"),
        ("[plugins.process]\np = [\"run\"]\n", "[plugins] process"),
        (
            "[releases]\npublish = true\n",
            "[releases] publish (line 2)",
        ),
        ("[stamps.review]\nstale = 1\n", "[stamps] review"),
        ("\n[land]\n", "[land] (line 2)"),
    ] {
        let said = problems(text);
        assert_eq!(said.len(), 1, "{text}: {said:?}");
        assert!(
            said[0].starts_with(&format!("{PATH}: {name}")),
            "{text}: {said:?}"
        );
        assert!(
            said[0].ends_with(
                "is not read from a local file: it holds a command or a program the Symphony \
                 runs, or decides what lands, and a file no commit records must not change that; \
                 it stays in symphony.toml"
            ),
            "{text}: {said:?}"
        );
    }
}

#[test]
fn any_other_table_or_key_is_refused_as_none_a_local_file_overrides() {
    for (text, name) in [
        ("[theme]\naccent = \"red\"\n", "[theme] (line 1)"),
        ("[limts]\nlow_volume = 2\n", "[limts] (line 1)"),
        ("[project]\nname = \"x\"\n", "[project] name (line 2)"),
        ("[project]\n", "[project] (line 1)"),
        (
            "[board]\nreview_alarm_ratio = 3\n",
            "[board] review_alarm_ratio (line 2)",
        ),
        (
            "[board]\norchestrator_slots = 3\n",
            "[board] orchestrator_slots (line 2)",
        ),
        ("[agents]\npanes = \"hc\"\n", "[agents] panes (line 2)"),
        (
            "[agents]\nworktrees = \"../wt\"\n",
            "[agents] worktrees (line 2)",
        ),
    ] {
        assert_eq!(
            problems(text),
            [format!(
                "{PATH}: {name} is not a setting a local file overrides: it holds [agents] \
                 launch, [agents.models] and [limits] alone"
            )],
            "{text}"
        );
    }
    // An array of tables is no table of settings.
    assert_eq!(
        problems("[[entrypoints]]\nurl = \"/d/\"\nfolder = \"d\"\n"),
        [format!(
            "{PATH}: entrypoints (line 1) is array; a local file holds tables of settings"
        )]
    );
}

#[test]
fn an_unknown_key_of_a_table_it_may_hold_is_refused_in_the_project_files_words() {
    for (text, name) in [
        ("[limits]\nwip = 3\n", "[limits] wip (line 2)"),
        (
            "[agents.models]\nowner = \"x\"\n",
            "[agents.models] owner (line 2)",
        ),
        (
            "[agents]\nlaunch = { modle = \"x\" }\n",
            "[agents.launch] modle (line 2)",
        ),
    ] {
        assert_eq!(
            problems(text),
            [format!(
                "{PATH}: {name} is not a setting the Symphony knows"
            )],
            "{text}"
        );
    }
}

#[test]
fn a_value_of_the_wrong_type_or_range_is_said_with_its_line() {
    for (text, said) in [
        (
            "[limits]\n\nlow_volume = \"two\"\n",
            "[limits] low_volume (line 3) is \"two\"; not a whole number",
        ),
        (
            "[limits]\nmachine_agents = 0\n",
            "[limits] machine_agents (line 2) is 0; a machine runs at least one agent",
        ),
        (
            "[limits]\norchestrator_slots = 0\n",
            "[limits] orchestrator_slots (line 2) is 0; an orchestrator needs at least one slot",
        ),
        (
            "[limits]\norchestrator_slots = 4294967296\n",
            "[limits] orchestrator_slots (line 2) is 4294967296; past what it can be",
        ),
        (
            "[limits]\norchestrator_tasks = -1\n",
            "[limits] orchestrator_tasks (line 2) is -1; an orchestrator holds at least one task",
        ),
        (
            "[limits]\nlow_volume = -1\n",
            "[limits] low_volume (line 2) is -1; it is at least 0",
        ),
        (
            "[agents.models]\nexecutor = 4\n",
            "[agents.models] executor (line 2) is 4; not text",
        ),
        (
            "[agents.launch]\neffort = true\n",
            "[agents.launch] effort (line 2) is true; not text",
        ),
        (
            "[agents]\nlaunch = \"claude\"\n",
            "[agents] launch (line 2) is \"claude\"; not a table",
        ),
        (
            "[agents]\nmodels = 3\n",
            "[agents] models (line 2) is 3; not a table",
        ),
        (
            "limits = 3\n",
            "limits (line 1) is 3; a local file holds tables of settings",
        ),
        (
            "[agents.launch]\ndefault = \"gemini\"\n",
            "[agents.launch] default (line 2) \"gemini\" is not one of codex, claude",
        ),
    ] {
        assert_eq!(problems(text), [format!("{PATH}: {said}")], "{text}");
    }
}

#[test]
fn text_that_is_no_toml_is_said_with_its_line_and_every_problem_is_said() {
    let said = problems("[limits\n");
    assert_eq!(said.len(), 1);
    assert!(
        said[0].starts_with(&format!("{PATH}: TOML parse error at line 1")),
        "{said:?}"
    );
    let said = problems("[limits]\nwip = 1\nlow_volume = \"x\"\n[land]\ncheck = false\n");
    assert_eq!(said.len(), 3, "{said:?}");
}

#[test]
fn one_refused_key_leaves_the_whole_file_out_of_every_reader() {
    let committed = "[agents.models]\nexecutor = \"opus\"\n\n[limits]\nmachine_agents = 9\n";
    let local = "[agents.models]\nexecutor = \"fable\"\n\n[limits]\nmachine_agents = 4\n\n\
                 [land]\ncheck = false\n";
    let root = folder("refused", Some(committed), Some(local));
    let file = root.join(crate::project::FILE);
    let read = crate::limits::read_at(&file);
    assert_eq!(read.limits.machine_agents, Some(9), "the committed value");
    assert_eq!(
        read.written[MACHINE_AGENTS].said(),
        "[limits] machine_agents, line 5"
    );
    assert_eq!(
        read.problems,
        [format!(
            "{}: [land] check (line 8) is not read from a local file: it holds a command or a \
             program the Symphony runs, or decides what lands, and a file no commit records must \
             not change that; it stays in symphony.toml; none of symphony.local.toml is read, \
             the committed settings hold",
            root.join(FILE).display()
        )]
    );
    let defaults = Defaults::configured(&root).unwrap();
    assert_eq!(defaults.models.executor.as_deref(), Some("opus"));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_local_key_replaces_the_committed_one_and_one_left_out_keeps_it() {
    let committed = "[agents]\nlaunch = { default = \"codex\", model = \"gpt\", effort = \"low\" }\n\n\
                     [agents.models]\nexecutor = \"opus\"\nreviewer = \"sonnet\"\n\n\
                     [limits]\nmachine_agents = 9\nlow_volume = 2\n\n\
                     [board]\norchestrator_slots = 5\n";
    let root = folder("over", Some(committed), None);
    let file = root.join(crate::project::FILE);
    let said = file.display().to_string();
    let before = (
        crate::limits::read_at(&file),
        Defaults::configured(&root).unwrap(),
    );
    assert_eq!(before.0, crate::limits::parse(&said, committed));
    assert_eq!(
        before.1,
        Defaults::parse(&said, committed).unwrap(),
        "no local file reads exactly as the project file alone"
    );

    std::fs::write(
        root.join(FILE),
        "# mine\n[agents.models]\nexecutor = \"fable\"\n\n[limits]\nmachine_agents = 4\n\
         orchestrator_slots = 2\norchestrator_tasks = 30\n",
    )
    .unwrap();
    let read = crate::limits::read_at(&file);
    assert_eq!(read.problems, Vec::<String>::new());
    assert_eq!(read.limits.orchestrator_tasks, 30);
    assert_eq!(
        read.written[TASKS].said(),
        "symphony.local.toml [limits] orchestrator_tasks, line 8"
    );
    assert_eq!(read.limits.machine_agents, Some(4));
    assert_eq!(
        read.limits.orchestrator_slots,
        Some(2),
        "over [board]'s older cap too"
    );
    assert_eq!(read.limits.low_volume, 2, "left out locally: the committed");
    assert_eq!(
        read.written[MACHINE_AGENTS].said(),
        "symphony.local.toml [limits] machine_agents, line 6"
    );
    assert_eq!(
        read.written[SLOTS].said(),
        "symphony.local.toml [limits] orchestrator_slots, line 7"
    );
    assert_eq!(read.written[LOW].said(), "[limits] low_volume, line 10");
    let defaults = Defaults::configured(&root).unwrap();
    assert_eq!(defaults.models.executor.as_deref(), Some("fable"));
    assert_eq!(defaults.models.reviewer.as_deref(), Some("sonnet"));
    assert_eq!(defaults.agent, Some("codex"));
    assert_eq!(
        defaults.resolve_as(Some("executor"), None, None).model,
        "fable"
    );

    // The line deleted, the committed model is back, with nothing restarted.
    std::fs::write(root.join(FILE), "# mine\n[agents.models]\n").unwrap();
    assert_eq!(Defaults::configured(&root).unwrap(), before.1);
    assert_eq!(crate::limits::read_at(&file), before.0);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        committed,
        "the project file is never written"
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_launch_agent_its_model_and_effort_are_overridden_key_by_key() {
    let committed =
        "[agents]\nlaunch = { default = \"codex\", model = \"gpt\", effort = \"low\" }\n";
    let root = folder(
        "launch",
        Some(committed),
        Some("[agents.launch]\ndefault = \"claude\"\nmodel = \"fable\"\n"),
    );
    let defaults = Defaults::configured(&root).unwrap();
    assert_eq!(defaults.agent, Some("claude"));
    assert_eq!(defaults.model.as_deref(), Some("fable"));
    assert_eq!(defaults.effort.as_deref(), Some("low"), "left out: kept");
    assert_eq!(
        defaults.resolve(None, None).said(),
        "claude on fable (effort low)"
    );

    // A value written empty is none, as in the project file: the board's own stands.
    std::fs::write(
        root.join(FILE),
        "[agents.launch]\ndefault = \"\"\nmodel = \"\"\neffort = \" \"\n",
    )
    .unwrap();
    assert_eq!(Defaults::configured(&root).unwrap(), Defaults::default());
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_local_file_holds_over_no_project_file_and_one_that_cannot_be_read_is_said() {
    let root = folder(
        "alone",
        None,
        Some("[limits]\nlow_volume = 1\n\n[agents.models]\nreviewer = \"r\"\n"),
    );
    let file = root.join(crate::project::FILE);
    assert_eq!(crate::limits::read_at(&file).limits.low_volume, 1);
    assert_eq!(
        Defaults::configured(&root)
            .unwrap()
            .models
            .reviewer
            .as_deref(),
        Some("r")
    );
    std::fs::remove_file(root.join(FILE)).unwrap();
    assert_eq!(beside(&file), Ok(None), "no file is no override");
    std::fs::create_dir(root.join(FILE)).unwrap();
    let said = beside(&file).unwrap_err();
    assert!(
        said[0].starts_with(&format!("cannot read {}: ", root.join(FILE).display())),
        "{said:?}"
    );
    assert_eq!(crate::limits::read_at(&file).problems.len(), 1);
    assert_eq!(Defaults::configured(&root).unwrap(), Defaults::default());
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_watched_project_file_is_read_again_when_the_local_file_changes() {
    let root = folder("watched", Some("[limits]\nmachine_agents = 9\n"), None);
    let (watched, read) = crate::limits::Watched::new(root.join(crate::project::FILE));
    assert_eq!(read.limits.machine_agents, Some(9));
    std::fs::write(root.join(FILE), "[limits]\nmachine_agents = 4\n").unwrap();
    assert_eq!(watched.now().machine_agents, Some(4), "it appeared");
    std::fs::write(root.join(FILE), "[limits]\nmachine_agents = 12\n").unwrap();
    assert_eq!(watched.now().machine_agents, Some(12), "its length changed");
    std::fs::remove_file(root.join(FILE)).unwrap();
    assert_eq!(watched.now().machine_agents, Some(9), "gone: the committed");
    std::fs::remove_dir_all(&root).unwrap();
}
