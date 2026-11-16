//! The statements of `store/waits.rs`.

use super::logged;
use super::write::ACT;
use crate::alarm::{ASSESSED, FIXED, WAITS};
use crate::red::FIELDS;

/// Task `$slug`'s status and version, as one row; none for a slug no task has.
pub(in crate::store) const ASSESSMENT_STATE: &str =
    "SELECT slug, status, version FROM type::record('task', $slug);";

/// What task `$slug`'s landing waits on, `$record` of the assessment `$assessment`, written as
/// `$patch` with its line (`$actor`, `$set`) unless that pair was committed before or stands: the
/// pair standing is kept among the task's assessments first. `acting`, the actor is a session,
/// checked and its activity recorded. Refuses with `$missing`, `$source_error` when given,
/// `$source_changed` unless `$source_current`, and `$changed` when `$checked` and the task is no
/// longer at `$expected_status` and `$expected_version`.
pub(in crate::store) fn record(acting: bool) -> String {
    let act = if acting { ACT } else { "" };
    format!(
        "IF !record::exists(type::record('task', $slug)) {{ THROW $missing }};
LET $board = type::record('task', $slug).fields.{FIELDS};
LET $seen = $board.assessments[$assessment][$record];
IF $seen = NONE AND !($board.{WAITS} = $record AND $board.{ASSESSED}.id = $assessment) {{
{act}
IF $source_error != NONE {{ THROW $source_error }};
IF !$source_current {{ THROW $source_changed }};
IF $checked AND (type::record('task', $slug).status != $expected_status OR type::record('task', $slug).version != $expected_version) {{ THROW $changed }};
IF $board.{ASSESSED}.id != NONE AND $board.{WAITS} != NONE {{
    UPDATE type::record('task', $slug) SET
        fields.{FIELDS}.assessments[$board.{ASSESSED}.id][$board.{WAITS}] = {{
            at: $board.{ASSESSED}.at, sid: $board.{ASSESSED}.sid
        }} RETURN NONE;
}};
UPDATE type::record('task', $slug) MERGE {{ fields: $patch }} RETURN NONE;
UPDATE type::record('task', $slug) SET version += 1 RETURN NONE;
LET $said = $actor + $set + $record;
{}
UPDATE type::record('task', $slug) SET
    fields.{FIELDS}.{ASSESSED} = {{ id: $assessment, at: $ts, sid: $sid }},
    fields.{FIELDS}.assessments[$assessment][$record] = {{ at: $ts, sid: $sid }} RETURN NONE;
}};",
        logged("$slug", "$actor", "'system'", "$said", "NONE")
    )
}

/// Where the rework the record `$written` of the tip `$judged` asked for ended, kept on task
/// `$slug` as `$patch` unless one is kept for that record, answering what is kept after.
pub(in crate::store) fn fixed() -> String {
    let kept = format!("fields.{FIELDS}.{FIXED}[$judged][$written]");
    format!(
        "IF record::exists(type::record('task', $slug)) {{
    UPDATE type::record('task', $slug) MERGE {{ fields: $patch }} WHERE {kept} = NONE RETURN NONE;
}};
RETURN (SELECT VALUE {kept} FROM ONLY type::record('task', $slug));"
    )
}
