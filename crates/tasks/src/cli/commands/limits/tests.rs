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
    )
}

#[test]
fn a_file_naming_nothing_prints_every_default_and_the_boards_own_models() {
    assert_eq!(
        shown("[project]\nname = \"x\"\n", None),
        [
            "LIMITS from /repo/symphony.toml",
            "orchestrator_slots       none    default",
            "low_volume               4       default",
            "machine_agents           none    default",
            "review_alarm_ratio       2       default",
            "review_alarm_hours       4       default",
            "review_alarm_clear_hours 1       default",
            "MODELS of codex",
            "orchestrator             -       whoever starts one chooses",
            "executor                 gpt-6.1-sol the board's own for codex",
            "reviewer                 gpt-6.1-sol the board's own for codex",
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
            "low_volume               2       [limits] low_volume, line 10",
            "machine_agents           6       [limits] machine_agents, line 11; this machine runs \
             4 now",
            "review_alarm_ratio       3       [board] review_alarm_ratio, line 15",
            "review_alarm_hours       5       [board] review_alarm_hours, line 16",
            "review_alarm_clear_hours 2       [board] review_alarm_clear_hours, line 17",
            "MODELS of claude",
            "orchestrator             fable   [agents.models] orchestrator, line 6",
            "executor                 opus    [agents] launch model, line 2",
            "reviewer                 sonnet  [agents.models] reviewer, line 5",
            "LEFT OUT [limits] wip (line 12): this build does not know it",
        ]
    );
}

#[test]
fn the_cap_an_older_file_writes_in_board_is_named_there() {
    let text = "[board]\norchestrator_slots = 8\n";
    assert_eq!(
        shown(text, None)[1],
        "orchestrator_slots       8       [board] orchestrator_slots, line 2"
    );
}

#[test]
fn a_count_that_cannot_be_made_is_said_beside_the_limit() {
    let text = "[limits]\nmachine_agents = 3\n";
    let unknown = Err(Error::Refused("hc cannot be asked".into()));
    assert_eq!(
        shown(text, Some(unknown))[3],
        "machine_agents           3       [limits] machine_agents, line 2; hc cannot be asked"
    );
}

#[test]
fn a_bad_value_shows_the_default_in_force() {
    // What is wrong was said on stderr as the store opened: here the value it holds instead.
    let text = "[limits]\nlow_volume = -1\n";
    assert_eq!(
        shown(text, None)[2],
        "low_volume               4       default"
    );
}

#[test]
fn outside_a_repository_and_with_no_models_read_it_says_so() {
    let read = Read::default();
    assert_eq!(
        lines(None, "", &read, None, None),
        [
            "LIMITS: no project file outside a repository, so every default",
            "orchestrator_slots       none    default",
            "low_volume               4       default",
            "machine_agents           none    default",
            "review_alarm_ratio       2       default",
            "review_alarm_hours       4       default",
            "review_alarm_clear_hours 1       default",
            "MODELS: the project file cannot be read for them",
        ]
    );
}
