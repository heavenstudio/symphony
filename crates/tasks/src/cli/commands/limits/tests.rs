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
        &Stored::default(),
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
        lines(None, "", &read, None, None, None, &Stored::default()),
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
            &Stored::default(),
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
        &Stored::default(),
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
        &Stored::default(),
    );
    assert_eq!(
        shown[11],
        "executor                 gpt-next symphony.local.toml [agents] launch model, line 2"
    );
}

/// What the store's `entries` (key and value) write, each judged.
fn stored(entries: &[(&str, serde_json::Value)]) -> Stored {
    let entries: Vec<settings::Entry> = (entries.iter())
        .map(|(key, value)| settings::Entry {
            key: (*key).into(),
            value: Some(value.clone()),
            ..settings::Entry::default()
        })
        .collect();
    Stored::of(&entries)
}

/// What `limits` prints of `text` with `stored` over it and, when given, a local file over both.
fn shown_over(text: &str, stored: &Stored, local: Option<&str>) -> Vec<String> {
    let local = local.map(|local| {
        crate::local::parse(&PathBuf::from("/repo/symphony.local.toml"), local).unwrap()
    });
    let read = limits::over(
        limits::over_stored(limits::parse(PATH, text), stored),
        Ok(local.clone()),
    );
    let mut models = Defaults::parse(PATH, text).unwrap().over(&stored.held);
    if let Some(local) = &local {
        models = models.over(local);
    }
    lines(
        Some(&PathBuf::from(PATH)),
        text,
        &read,
        Some(&models),
        None,
        local.as_ref(),
        stored,
    )
}

const COMMITTED: &str = "\
[agents]
panes = \"none\"
launch = { default = \"codex\", model = \"gpt\" }

[agents.models]
reviewer = \"sonnet\"

[limits]
low_volume = 2
";

#[test]
fn a_store_with_no_entry_in_force_prints_what_the_files_alone_do() {
    let files = shown(COMMITTED, None);
    assert_eq!(shown_over(COMMITTED, &Stored::default(), None), files);
    // An entry that is not read changes no row either.
    let refused = stored(&[("land.check", serde_json::json!(false))]);
    assert_eq!(refused.problems.len(), 1);
    assert_eq!(shown_over(COMMITTED, &refused, None), files);
}

#[test]
fn a_stored_setting_names_the_store_and_the_committed_value_it_stands_over() {
    let json = |value: &str| serde_json::json!(value);
    let held = stored(&[
        ("limits.low_volume", serde_json::json!(0)),
        ("limits.machine_agents", serde_json::json!(6)),
        ("agents.launch.default", json("claude")),
        ("agents.launch.model", json("fable")),
        ("agents.models.reviewer", json("opus")),
        ("agents.models.orchestrator", json("fable")),
        ("agents.panes", json("hc")),
    ]);
    assert_eq!(
        shown_over(COMMITTED, &held, None),
        [
            "LIMITS from /repo/symphony.toml",
            "STORED over it 7 settings the board keeps (just tasks settings list)",
            "orchestrator_slots       none    default",
            "orchestrator_tasks       16      default",
            "low_volume               0       the board's store [limits] low_volume, over 2 ([limits] low_volume, line 9)",
            "machine_agents           6       the board's store [limits] machine_agents, over its default, none",
            "review_alarm_ratio       2       default",
            "review_alarm_hours       4       default",
            "review_alarm_clear_hours 1       default",
            "MODELS of claude, from the board's store [agents] launch default, over \"codex\" ([agents] launch default, line 3)",
            "orchestrator             fable   the board's store [agents.models] orchestrator, over none written",
            "executor                 fable   the board's store [agents] launch model, over \"gpt\" ([agents] launch model, line 3)",
            "reviewer                 opus    the board's store [agents.models] reviewer, over \"sonnet\" ([agents.models] reviewer, line 6)",
            "assessor                 haiku   the smallest claude offers",
            "panes                    hc      the board's store [agents] panes, over \"none\" ([agents] panes, line 2)",
        ]
    );
    // One setting stored says so in the singular, and panes with none committed is over none.
    let one = shown_over("", &stored(&[("agents.panes", json("hc"))]), None);
    assert_eq!(
        one[1],
        "STORED over it 1 setting the board keeps (just tasks settings list)"
    );
    assert_eq!(
        one.last().unwrap(),
        "panes                    hc          the board's store [agents] panes, over none written"
    );
}

#[test]
fn a_local_files_value_wins_over_a_stored_one_and_is_named_as_before() {
    let json = |value: &str| serde_json::json!(value);
    let held = stored(&[
        ("limits.low_volume", serde_json::json!(0)),
        ("agents.launch.default", json("claude")),
        ("agents.launch.model", json("fable")),
        ("agents.models.reviewer", json("opus")),
    ]);
    let local = "[agents]\nlaunch = { default = \"codex\", model = \"mine\" }\n\n\
                 [agents.models]\nreviewer = \"local\"\n\n[limits]\nlow_volume = 9\n";
    let shown = shown_over(COMMITTED, &held, Some(local));
    assert_eq!(
        [&shown[1], &shown[2]],
        [
            "STORED over it 4 settings the board keeps (just tasks settings list)",
            "LOCAL  over it /repo/symphony.local.toml, which git does not version",
        ]
    );
    assert_eq!(
        shown[5],
        "low_volume               9       symphony.local.toml [limits] low_volume, line 8"
    );
    assert_eq!(
        shown[10],
        "MODELS of codex, from symphony.local.toml [agents] launch default, line 2"
    );
    assert_eq!(
        shown[12],
        "executor                 mine    symphony.local.toml [agents] launch model, line 2"
    );
    assert_eq!(
        shown[13],
        "reviewer                 local   symphony.local.toml [agents.models] reviewer, line 5"
    );
}
