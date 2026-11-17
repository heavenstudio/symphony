//! The board's records as JSON: the fields and their order.

use crate::json_object::{object, value};
use crate::rows::TaskSessionRow;
use crate::stages::Stages;
use crate::{Ask, Landed, Message, Role, Session, Task};
use serde_json::Value;

/// An ask as the board holds it, with its age at `now` and whether that makes it late.
pub fn ask(a: &Ask, now: f64) -> Value {
    let (age, late) = crate::store::asks::age(a, now);
    object(vec![
        ("id", value(a.id)),
        ("kind", value(a.kind.as_str())),
        ("path", value(a.path.as_deref())),
        ("card", value(a.card.as_deref())),
        ("block", value(a.block.as_deref())),
        ("tree", value(a.tree.as_deref())),
        ("task", value(a.task.as_deref())),
        ("thread", value(a.thread.as_deref())),
        ("words", value(a.words.as_str())),
        ("asked", value(a.asked)),
        ("lines", value(a.lines)),
        ("last", value(a.last)),
        ("holder", value(a.holder.as_deref())),
        ("held", value(a.held)),
        (
            "before",
            Value::Array(
                a.before
                    .iter()
                    .map(|h| {
                        object(vec![
                            ("handle", value(h.handle.as_str())),
                            ("since", value(h.since)),
                            ("until", value(h.until)),
                            ("why", value(h.why.as_str())),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("open", value(a.open)),
        ("answered", value(a.answered)),
        ("answered_by", value(a.answered_by.as_deref())),
        ("answer", value(a.answer.as_deref())),
        ("closed", value(a.closed)),
        ("gone", value(a.gone.as_deref())),
        ("gone_since", value(a.gone_since)),
        ("age", value(age)),
        ("late", value(late)),
    ])
}

pub fn task(t: &Task) -> Value {
    object(vec![
        ("slug", value(t.slug.as_str())),
        ("title", value(t.title.as_str())),
        ("body", value(t.body.as_str())),
        ("status", value(t.status.as_str())),
        ("priority", value(t.priority)),
        ("parent", value(t.parent.as_deref())),
        ("created_by", value(t.created_by.as_str())),
        ("created", value(t.created)),
        ("updated", value(t.updated)),
        ("links", value(t.links.as_slice())),
        ("depends", value(t.depends.as_slice())),
        ("roles", Value::Array(t.roles.iter().map(role).collect())),
        ("instruction", value(t.instruction)),
    ])
}

/// A task as `tasks list --json` lists it: with `in_status_since`, when it took the status it holds
/// ([`crate::stages::in_status_since`]), null where that was not read.
pub fn listed(t: &Task, in_status_since: Option<f64>) -> Value {
    let mut shaped = task(t);
    if let Value::Object(fields) = &mut shaped {
        fields.insert("in_status_since".into(), value(in_status_since));
    }
    shaped
}

/// A landed task, with the status the landing closed it from.
pub fn landed(l: &Landed) -> Value {
    let mut shaped = task(&l.task);
    if let Value::Object(fields) = &mut shaped {
        fields.insert("from".into(), value(l.from.as_str()));
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
                        ("first_entered", value(stage.first_entered)),
                        ("seconds", value(stage.seconds)),
                        ("visits", value(i64::from(stage.visits))),
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
                            ("status", value(segment.status.as_str())),
                            ("from", value(segment.from)),
                            ("to", value(segment.to)),
                            ("by", value(segment.by.as_deref())),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("unclaimed_seconds", value(s.unclaimed_seconds)),
        ("first_claimed", value(s.first_claimed)),
        ("bounces", value(i64::from(s.bounces))),
        (
            "current",
            object(vec![
                ("status", value(s.current.as_str())),
                ("since", value(s.current_since)),
            ]),
        ),
        ("closed", value(s.closed)),
    ])
}

pub fn tasks(ts: &[Task]) -> Value {
    Value::Array(ts.iter().map(task).collect())
}

fn role(r: &Role) -> Value {
    object(vec![
        ("handle", value(r.handle.as_str())),
        ("role", value(r.role.as_str())),
        ("since", value(r.since)),
        ("expires", value(r.expires)),
        ("health", value(r.health.as_str())),
    ])
}

pub fn session(s: &Session) -> Value {
    let r = &s.row;
    let affinity: Vec<&str> = r.affinity.split(',').filter(|a| !a.is_empty()).collect();
    object(vec![
        ("handle", value(r.handle.as_str())),
        ("kind", value(r.kind.as_str())),
        ("parent", value(r.parent.as_deref())),
        ("worktree", value(r.worktree.as_str())),
        ("branch", value(r.branch.as_str())),
        ("model", value(r.model.as_str())),
        ("pid", value(r.pid)),
        ("host", value(r.host.as_str())),
        ("affinity", value(affinity.as_slice())),
        ("connected", value(r.connected)),
        ("last_seen", value(r.last_seen)),
        ("last_tool", value(r.last_tool)),
        ("left_at", value(r.left_at)),
        ("health", value(s.health.as_str())),
    ])
}

pub fn message(m: &Message) -> Value {
    object(vec![
        ("id", value(m.id)),
        ("slug", value(m.slug.as_str())),
        ("sender", value(m.sender.as_str())),
        ("recipient", value(m.recipient.as_deref())),
        ("kind", value(m.kind.as_str())),
        ("text", value(m.text.as_str())),
        ("ts", value(m.ts)),
    ])
}

pub fn messages(ms: &[Message]) -> Value {
    Value::Array(ms.iter().map(message).collect())
}

pub fn launch(r: &TaskSessionRow) -> Value {
    object(vec![
        ("slug", value(r.slug.as_str())),
        ("handle", value(r.handle.as_str())),
        ("role", value(r.role.as_str())),
        ("claude_session", value(r.claude_session.as_str())),
        ("worktree", value(r.worktree.as_str())),
        ("branch", value(r.branch.as_str())),
        ("pane", value(r.pane.as_str())),
        ("model", value(r.model.as_str())),
        ("started", value(r.started)),
        ("closed", value(r.closed)),
        ("closed_why", value(r.closed_why.as_str())),
        ("restore", value(r.restore.as_str())),
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
