//! A writer's line: what the opening's read answers, when a line is written again, which writers a
//! migrate leaves behind and the words that name them.

use super::*;
use serde_json::json;

const NOW: f64 = 1_800_000_000.0;
const DAY: f64 = 24.0 * 3600.0;

fn line(program: &str, host: &str, schema: i64, seen: f64) -> Writer {
    Writer {
        program: program.into(),
        path: format!("/work/{program}"),
        host: host.into(),
        schema,
        seen,
    }
}

#[test]
fn the_opening_read_says_the_version_and_whether_the_table_and_the_line_are_there() {
    let kept = json!({"schema": 18, "seen": NOW});
    assert_eq!(
        opened(&[Value::Null, json!([18, kept, 0])]),
        (
            Some(18),
            Line::Held(Some(Kept {
                schema: 18,
                seen: NOW
            })),
            0
        )
    );
    assert_eq!(
        opened(&[Value::Null, json!([18, null, 2])]),
        (Some(18), Line::Held(None), 2)
    );
    assert_eq!(
        opened(&[Value::Null, json!([17, false, 0])]),
        (Some(17), Line::NoTable, 0)
    );
    assert_eq!(
        opened(&[Value::Null, json!([null, false, 0])]),
        (None, Line::NoTable, 0)
    );
    // A line that does not read as one is written again, never taken for a fresh one.
    assert_eq!(
        opened(&[Value::Null, json!([18, {"seen": "yesterday"}, 0])]),
        (Some(18), Line::Held(None), 0)
    );
    // A board's count of additive steps that is missing or not a number is none.
    assert_eq!(
        opened(&[json!([18, null])]),
        (Some(18), Line::Held(None), 0)
    );
    assert_eq!(
        opened(&[json!([18, null, "2"])]),
        (Some(18), Line::Held(None), 0)
    );
    assert_eq!(opened(&[]), (None, Line::NoTable, 0));
}

#[test]
fn a_line_is_written_when_missing_at_another_schema_or_an_hour_old_and_never_without_its_table() {
    let at = |schema, seen| {
        Line::Held(Some(Kept {
            schema,
            seen: NOW - seen,
        }))
    };
    assert!(due(&Line::Held(None), 18, NOW));
    assert!(due(&at(17, 0.0), 18, NOW), "rebuilt since");
    assert!(due(&at(19, 0.0), 18, NOW), "another build at that path");
    assert!(due(&at(18, EVERY), 18, NOW));
    assert!(due(&at(18, EVERY + 1.0), 18, NOW));
    assert!(!due(&at(18, EVERY - 1.0), 18, NOW));
    assert!(!due(&at(18, 0.0), 18, NOW));
    // A line from a clock ahead of this one is fresh to it.
    assert!(!due(&at(18, -60.0), 18, NOW));
    assert!(!due(&Line::NoTable, 18, NOW));
}

#[test]
fn a_migrate_leaves_behind_only_writers_seen_in_the_last_week_behind_its_target_but_itself() {
    let migrate = Program::named("tasks", "/work/tasks");
    let lines = vec![
        line("tasks", "mini", 18, NOW), // the migrate itself, rebuilt since
        line("tasks", "studio", 18, NOW - DAY), // the same path on another host
        line("gate-review", "mini", 18, NOW - 3.0 * DAY),
        line("old-gate", "mini", 17, NOW - RECENT - 1.0), // not seen in the last week
        line("edge", "mini", 18, NOW - RECENT),           // seen a week ago to the second
        line("rebuilt", "mini", 19, NOW),                 // at the target already
        line("ahead", "mini", 20, NOW),                   // ahead of it
    ];
    let behind = behind(lines, 19, NOW, &migrate, "mini");
    let named: Vec<(&str, &str)> = behind
        .iter()
        .map(|w| (w.program.as_str(), w.host.as_str()))
        .collect();
    assert_eq!(
        named,
        [
            ("edge", "mini"),
            ("gate-review", "mini"),
            ("tasks", "studio")
        ]
    );
}

#[test]
fn the_refusal_names_each_writer_behind_its_schema_where_it_is_and_when_it_was_seen() {
    let one = [line("gate-review", "mini", 18, NOW - 3.0 * 3600.0)];
    assert_eq!(
        left_behind("the alpha board", 19, &one, NOW),
        "the alpha board has a writer seen in the last week behind schema 19, the one this \
         migrate moves it to, and each would be refused the board from then on: gate-review at \
         schema 18 (/work/gate-review on mini, last seen 3 h ago). Nothing was moved: rebuild \
         each at schema 19 and run it once, which records it, then `just tasks migrate` again; \
         or `just tasks migrate --leave-behind` moves it now and leaves them refused"
    );
    let two = [
        line("gate-review", "mini", 18, NOW - 120.0),
        line("tasks", "studio", 17, NOW - 30.0),
    ];
    let said = left_behind("this board", 19, &two, NOW);
    assert!(
        said.starts_with("this board has 2 writers seen in the last week behind schema 19"),
        "{said}"
    );
    assert!(
        said.contains(
            ": gate-review at schema 18 (/work/gate-review on mini, last seen 2 min ago); tasks \
             at schema 17 (/work/tasks on studio, last seen 30 s ago). Nothing was moved"
        ),
        "{said}"
    );
}

#[test]
fn a_program_is_named_by_its_file_name_and_this_process_by_its_executable() {
    assert_eq!(
        Program::at(Path::new("/work/alpha/.target/debug/gate-review")),
        Program::named("gate-review", "/work/alpha/.target/debug/gate-review")
    );
    assert_eq!(Program::at(Path::new("/")), Program::named("/", "/"));
    let this = Program::this();
    let exe = std::env::current_exe().unwrap().canonicalize().unwrap();
    assert_eq!(this.path, exe.to_string_lossy());
    assert_eq!(
        this.name,
        exe.file_name().unwrap().to_string_lossy().into_owned()
    );
    assert_eq!(Program::this(), this, "the same each time");
}
