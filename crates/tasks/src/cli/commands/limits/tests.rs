//! What `limits` prints of a project file: each limit and model with where it came from.

use super::*;
use crate::Error;
use std::path::PathBuf;

const PATH: &str = "/repo/symphony.toml";

fn shown(text: &str, running: Option<Result<usize, Error>>) -> Vec<String> {
    let read = limits::parse(PATH, text);
    let models = Defaults::parse(PATH, text).unwrap();
    lines(
        Some(&PathBuf::from(PATH)),
        text,
        &read,
        Some(&models),
        running,
        None,
    )
}

#[test]
fn a_file_naming_nothing_prints_every_default_and_the_boards_own_models() {
    // The board's own model is longer than the value column: the column widens to it, so every
    // row's source starts at one place.
    assert_eq!(
        shown("[project]\nname = \"x\"\n", None),
        [
            "LIMITS from /repo/symphony.toml",
            "orchestrator_slots       none        default",
            "orchestrator_tasks       16          default",
            "low_volume               4           default",
            "machine_agents           none        default",
            "review_alarm_ratio       2           default",
            "review_alarm_hours       4           default",
            "review_alarm_clear_hours 1           default",
            "MODELS of codex",
            "orchestrator             -           whoever starts one chooses",
            "executor                 gpt-6.1-sol the board's own for codex",
            "reviewer                 gpt-6.1-sol the board's own for codex",
            "assessor                 gpt-6.1-sol the board's own for codex",
        ]
    );
}

#[test]
fn each_written_setting_names_its_table_key_and_line() {
    let text = "\
[agents]
launch = { default = \"claude\", model = \"opus\" }

[agents.models]
reviewer = \"sonnet\"
orchestrator = \"fable\"

[limits]
orchestrator_slots = 5
orchestrator_tasks = 12
low_volume = 2
machine_agents = 6
wip = 3

[board]
review_alarm_ratio = 3
review_alarm_hours = 5
review_alarm_clear_hours = 2
";
    assert_eq!(
        shown(text, Some(Ok(4))),
        [
            "LIMITS from /repo/symphony.toml",
            "orchestrator_slots       5       [limits] orchestrator_slots, line 9",
            "orchestrator_tasks       12      [limits] orchestrator_tasks, line 10",
            "low_volume               2       [limits] low_volume, line 11",
            "machine_agents           6       [limits] machine_agents, line 12; this machine runs \
             4 now",
            "review_alarm_ratio       3       [board] review_alarm_ratio, line 16",
            "review_alarm_hours       5       [board] review_alarm_hours, line 17",
            "review_alarm_clear_hours 2       [board] review_alarm_clear_hours, line 18",
            "MODELS of claude",
            "orchestrator             fable   [agents.models] orchestrator, line 6",
            "executor                 opus    [agents] launch model, line 2",
            "reviewer                 sonnet  [agents.models] reviewer, line 5",
            "assessor                 haiku   the smallest claude offers",
            "LEFT OUT [limits] wip (line 13): this build does not know it",
        ]
    );
}

#[test]
fn the_cap_an_older_file_writes_in_board_is_named_there() {
    let text = "[board]\norchestrator_slots = 8\n";
    assert_eq!(
        shown(text, None)[1],
        "orchestrator_slots       8           [board] orchestrator_slots, line 2"
    );
}

#[test]
fn a_count_that_cannot_be_made_is_said_beside_the_limit() {
    let text = "[limits]\nmachine_agents = 3\n";
    let unknown = Err(Error::Refused("hc cannot be asked".into()));
    assert_eq!(
        shown(text, Some(unknown))[4],
        "machine_agents           3           [limits] machine_agents, line 2; hc cannot be asked"
    );
}

#[test]
fn a_bad_value_shows_the_default_in_force() {
    // What is wrong was said on stderr as the store opened: here the value it holds instead.
    let text = "[limits]\nlow_volume = -1\n";
    assert_eq!(
        shown(text, None)[3],
        "low_volume               4           default"
    );
}

#[test]
fn outside_a_repository_and_with_no_models_read_it_says_so() {
    let read = Read::default();
    assert_eq!(
        lines(None, "", &read, None, None, None),
        [
            "LIMITS: no project file outside a repository, so every default",
            "orchestrator_slots       none    default",
            "orchestrator_tasks       16      default",
            "low_volume               4       default",
            "machine_agents           none    default",
            "review_alarm_ratio       2       default",
            "review_alarm_hours       4       default",
            "review_alarm_clear_hours 1       default",
            "MODELS: the project file cannot be read for them",
        ]
    );
}

#[test]
fn a_setting_the_local_file_overrides_names_that_file_and_its_line() {
    let text = "\
[agents]
launch = { default = \"codex\", model = \"gpt\" }

[agents.models]
executor = \"opus\"
reviewer = \"sonnet\"

[limits]
machine_agents = 9
low_volume = 2
";
    let local = crate::local::parse(
        &PathBuf::from("/repo/symphony.local.toml"),
        "# mine\n[agents.launch]\ndefault = \"claude\"\nmodel = \"fable\"\n\n[agents.models]\n\
         executor = \"fable\"\nassessor = \"\"\n\n[limits]\nmachine_agents = 4\n",
    )
    .unwrap();
    let read = limits::over(limits::parse(PATH, text), Ok(Some(local.clone())));
    let models = Defaults::parse(PATH, text).unwrap().over(&local);
    assert_eq!(
        lines(
            Some(&PathBuf::from(PATH)),
            text,
            &read,
            Some(&models),
            None,
            Some(&local),
        ),
        [
            "LIMITS from /repo/symphony.toml",
            "LOCAL  over it /repo/symphony.local.toml, which git does not version",
            "orchestrator_slots       none    default",
            "orchestrator_tasks       16      default",
            "low_volume               2       [limits] low_volume, line 10",
            "machine_agents           4       symphony.local.toml [limits] machine_agents, line 11",
            "review_alarm_ratio       2       default",
            "review_alarm_hours       4       default",
            "review_alarm_clear_hours 1       default",
            "MODELS of claude, from symphony.local.toml [agents] launch default, line 3",
            "orchestrator             -       whoever starts one chooses",
            "executor                 fable   symphony.local.toml [agents.models] executor, line 7",
            "reviewer                 sonnet  [agents.models] reviewer, line 6",
            // Written empty locally, so none is written: the smallest its agent offers.
            "assessor                 haiku   the smallest claude offers",
        ]
    );
    // A local file that writes none of what is printed names itself and no row.
    let quiet = crate::local::parse(&PathBuf::from("/repo/symphony.local.toml"), "").unwrap();
    let shown = lines(
        Some(&PathBuf::from(PATH)),
        text,
        &limits::parse(PATH, text),
        Some(&Defaults::parse(PATH, text).unwrap()),
        None,
        Some(&quiet),
    );
    assert_eq!(shown[9], "MODELS of codex");
    assert_eq!(
        shown[11],
        "executor                 opus    [agents.models] executor, line 5"
    );
    assert_eq!(
        shown[13],
        "assessor                 gpt     [agents] launch model, line 2"
    );
    // The launch's model, written locally, is where a role without one takes it from.
    let launch = crate::local::parse(
        &PathBuf::from("/repo/symphony.local.toml"),
        "[agents]\nlaunch = { model = \"gpt-next\" }\n",
    )
    .unwrap();
    let shown = lines(
        Some(&PathBuf::from(PATH)),
        "",
        &Read::default(),
        Some(&Defaults::default().over(&launch)),
        None,
        Some(&launch),
    );
    assert_eq!(
        shown[11],
        "executor                 gpt-next symphony.local.toml [agents] launch model, line 2"
    );
}
