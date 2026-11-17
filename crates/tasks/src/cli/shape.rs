//! The board's records as JSON: the fields and their order.

use crate::json_object::object;
use crate::rows::TaskSessionRow;
use crate::stages::Stages;
use crate::{Ask, Landed, Message, Role, Session, Task};
use serde_json::Value;

/// An ask as the board holds it, with its age at `now` and whether that makes it late.
pub fn ask(a: &Ask, now: f64) -> Value {
    let (age, late) = crate::store::asks::age(a, now);
    object(vec![
        ("id", Value::from(a.id)),
        ("kind", Value::from(a.kind.as_str())),
        ("path", Value::from(a.path.as_deref())),
        ("card", Value::from(a.card.as_deref())),
        ("block", Value::from(a.block.as_deref())),
        ("tree", Value::from(a.tree.as_deref())),
        ("task", Value::from(a.task.as_deref())),
        ("thread", Value::from(a.thread.as_deref())),
        ("words", Value::from(a.words.as_str())),
        ("asked", Value::from(a.asked)),
        ("lines", Value::from(a.lines)),
        ("last", Value::from(a.last)),
        ("holder", Value::from(a.holder.as_deref())),
        ("held", Value::from(a.held)),
        (
            "before",
            Value::Array(
                a.before
                    .iter()
                    .map(|h| {
                        object(vec![
                            ("handle", Value::from(h.handle.as_str())),
                            ("since", Value::from(h.since)),
                            ("until", Value::from(h.until)),
                            ("why", Value::from(h.why.as_str())),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("open", Value::from(a.open)),
        ("answered", Value::from(a.answered)),
        ("answered_by", Value::from(a.answered_by.as_deref())),
        ("answer", Value::from(a.answer.as_deref())),
        ("closed", Value::from(a.closed)),
        ("gone", Value::from(a.gone.as_deref())),
        ("gone_since", Value::from(a.gone_since)),
        ("age", Value::from(age)),
        ("late", Value::from(late)),
    ])
}

pub fn task(t: &Task) -> Value {
    object(vec![
        ("slug", Value::from(t.slug.as_str())),
        ("title", Value::from(t.title.as_str())),
        ("body", Value::from(t.body.as_str())),
        ("status", Value::from(t.status.as_str())),
        ("priority", Value::from(t.priority)),
        ("parent", Value::from(t.parent.as_deref())),
        ("created_by", Value::from(t.created_by.as_str())),
        ("created", Value::from(t.created)),
        ("updated", Value::from(t.updated)),
        ("links", Value::from(t.links.as_slice())),
        ("depends", Value::from(t.depends.as_slice())),
        ("roles", Value::Array(t.roles.iter().map(role).collect())),
        ("instruction", Value::from(t.instruction)),
    ])
}

/// A task as `tasks list --json` lists it: with `in_status_since`, when it took the status it holds
/// ([`crate::stages::in_status_since`]), null where that was not read.
pub fn listed(t: &Task, in_status_since: Option<f64>) -> Value {
    let mut shaped = task(t);
    if let Value::Object(fields) = &mut shaped {
        fields.insert("in_status_since".into(), Value::from(in_status_since));
    }
    shaped
}

/// A landed task, with the status the landing closed it from.
pub fn landed(l: &Landed) -> Value {
    let mut shaped = task(&l.task);
    if let Value::Object(fields) = &mut shaped {
        fields.insert("from".into(), Value::from(l.from.as_str()));
    }
    shaped
}

/// An object whose keys are data, in the order given.
pub fn keyed(fields: impl Iterator<Item = (String, Value)>) -> Value {
    Value::Object(fields.collect())
}

/// Where a task's time went ([`crate::stages`]); every duration in seconds.
pub fn stages(s: &Stages) -> Value {
    object(vec![
        (
            "statuses",
            keyed(s.stages.iter().map(|stage| {
                (
                    stage.status.clone(),
                    object(vec![
                        ("first_entered", Value::from(stage.first_entered)),
                        ("seconds", Value::from(stage.seconds)),
                        ("visits", Value::from(i64::from(stage.visits))),
                    ]),
                )
            })),
        ),
        (
            "segments",
            Value::Array(
                s.segments
                    .iter()
                    .map(|segment| {
                        object(vec![
                            ("status", Value::from(segment.status.as_str())),
                            ("from", Value::from(segment.from)),
                            ("to", Value::from(segment.to)),
                            ("by", Value::from(segment.by.as_deref())),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("unclaimed_seconds", Value::from(s.unclaimed_seconds)),
        ("first_claimed", Value::from(s.first_claimed)),
        ("bounces", Value::from(i64::from(s.bounces))),
        (
            "current",
            object(vec![
                ("status", Value::from(s.current.as_str())),
                ("since", Value::from(s.current_since)),
            ]),
        ),
        ("closed", Value::from(s.closed)),
    ])
}

pub fn tasks(ts: &[Task]) -> Value {
    Value::Array(ts.iter().map(task).collect())
}

fn role(r: &Role) -> Value {
    object(vec![
        ("handle", Value::from(r.handle.as_str())),
        ("role", Value::from(r.role.as_str())),
        ("since", Value::from(r.since)),
        ("expires", Value::from(r.expires)),
        ("health", Value::from(r.health.as_str())),
    ])
}

pub fn session(s: &Session) -> Value {
    let r = &s.row;
    let affinity: Vec<&str> = r.affinity.split(',').filter(|a| !a.is_empty()).collect();
    object(vec![
        ("handle", Value::from(r.handle.as_str())),
        ("kind", Value::from(r.kind.as_str())),
        ("parent", Value::from(r.parent.as_deref())),
        ("worktree", Value::from(r.worktree.as_str())),
        ("branch", Value::from(r.branch.as_str())),
        ("model", Value::from(r.model.as_str())),
        ("pid", Value::from(r.pid)),
        ("host", Value::from(r.host.as_str())),
        ("affinity", Value::from(affinity.as_slice())),
        ("connected", Value::from(r.connected)),
        ("last_seen", Value::from(r.last_seen)),
        ("last_tool", Value::from(r.last_tool)),
        ("left_at", Value::from(r.left_at)),
        ("health", Value::from(s.health.as_str())),
    ])
}

pub fn message(m: &Message) -> Value {
    object(vec![
        ("id", Value::from(m.id)),
        ("slug", Value::from(m.slug.as_str())),
        ("sender", Value::from(m.sender.as_str())),
        ("recipient", Value::from(m.recipient.as_deref())),
        ("kind", Value::from(m.kind.as_str())),
        ("text", Value::from(m.text.as_str())),
        ("ts", Value::from(m.ts)),
    ])
}

pub fn messages(ms: &[Message]) -> Value {
    Value::Array(ms.iter().map(message).collect())
}

pub fn launch(r: &TaskSessionRow) -> Value {
    object(vec![
        ("slug", Value::from(r.slug.as_str())),
        ("handle", Value::from(r.handle.as_str())),
        ("role", Value::from(r.role.as_str())),
        ("claude_session", Value::from(r.claude_session.as_str())),
        ("worktree", Value::from(r.worktree.as_str())),
        ("branch", Value::from(r.branch.as_str())),
        ("pane", Value::from(r.pane.as_str())),
        ("model", Value::from(r.model.as_str())),
        ("started", Value::from(r.started)),
        ("closed", Value::from(r.closed)),
        ("closed_why", Value::from(r.closed_why.as_str())),
        ("restore", Value::from(r.restore.as_str())),
    ])
}

pub fn launches(rs: &[TaskSessionRow]) -> Value {
    Value::Array(rs.iter().map(launch).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn messages_preserve_text_integer_limits_and_float_values() {
        let text = "Ünï — 😀 \"quoted\" \\ \n\r\t\u{8}\u{c}\0\u{7f}";
        for (at, reference) in [
            (-0.0, "-0.0"),
            (1.0, "1.0"),
            (1e16, "1e+16"),
            (1.5e-5, "1.5e-05"),
            (2.5e-7, "2.5e-07"),
            (1791362047.125689, "1791362047.125689"),
        ] {
            let message = Message {
                id: i64::MAX,
                slug: "tools/example".into(),
                sender: "owner".into(),
                recipient: None,
                kind: "msg".into(),
                text: text.into(),
                ts: at,
            };
            let written = messages(&[message]);
            let parsed: Value = serde_json::from_str(&written.to_string()).unwrap();
            assert_eq!(parsed[0]["id"].as_i64(), Some(i64::MAX));
            assert_eq!(parsed[0]["text"], text);
            assert_eq!(parsed[0]["recipient"], Value::Null);
            let expected: f64 = serde_json::from_str(reference).unwrap();
            assert_eq!(
                parsed[0]["ts"].as_f64().unwrap().to_bits(),
                expected.to_bits()
            );
        }
        assert_eq!(messages(&[]), json!([]));
    }

    #[test]
    fn sessions_keep_optional_fields_and_filter_empty_affinities() {
        let mut record = Session {
            row: crate::rows::SessionRow::default(),
            health: crate::health::Health::Live,
        };
        record.row.affinity = ",tools,,app,".into();
        let value = session(&record);
        assert_eq!(value["affinity"], json!(["tools", "app"]));
        for key in ["parent", "pid", "last_tool", "left_at"] {
            assert_eq!(value[key], Value::Null, "{key}");
        }
        record.row.parent = Some("parent".into());
        record.row.pid = Some(i64::MAX);
        record.row.last_tool = Some(0.0);
        record.row.left_at = Some(1e16);
        let value = session(&record);
        assert_eq!(value["parent"], "parent");
        assert_eq!(value["pid"].as_i64(), Some(i64::MAX));
        assert_eq!(value["last_tool"], 0.0);
        assert_eq!(value["left_at"].as_f64(), Some(1e16));
    }

    #[test]
    fn launches_keep_null_and_closed_times() {
        let mut row = TaskSessionRow::default();
        assert_eq!(launch(&row)["closed"], Value::Null);
        row.closed = Some(123.456);
        assert_eq!(launches(&[row])[0]["closed"], 123.456);
        assert_eq!(launches(&[]), json!([]));
    }
}
