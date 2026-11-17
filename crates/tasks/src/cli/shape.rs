//! The board's records as JSON: the fields and their order.

use crate::json_object::object;
use crate::rows::TaskSessionRow;
use crate::stages::Stages;
use crate::{Ask, Landed, Message, Role, Session, Task};
use serde_json::{Value, json};

/// An ask as the board holds it, with its age at `now` and whether that makes it late.
pub fn ask(a: &Ask, now: f64) -> Value {
    let (age, late) = crate::store::asks::age(a, now);
    object(vec![
        ("id", json!(a.id)),
        ("kind", json!(&a.kind)),
        ("path", json!(a.path.as_deref())),
        ("card", json!(a.card.as_deref())),
        ("block", json!(a.block.as_deref())),
        ("tree", json!(a.tree.as_deref())),
        ("task", json!(a.task.as_deref())),
        ("thread", json!(a.thread.as_deref())),
        ("words", json!(&a.words)),
        ("asked", json!(a.asked)),
        ("lines", json!(a.lines)),
        ("last", json!(a.last)),
        ("holder", json!(a.holder.as_deref())),
        ("held", json!(a.held)),
        (
            "before",
            Value::Array(
                a.before
                    .iter()
                    .map(|h| {
                        object(vec![
                            ("handle", json!(&h.handle)),
                            ("since", json!(h.since)),
                            ("until", json!(h.until)),
                            ("why", json!(&h.why)),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("open", json!(a.open)),
        ("answered", json!(a.answered)),
        ("answered_by", json!(a.answered_by.as_deref())),
        ("answer", json!(a.answer.as_deref())),
        ("closed", json!(a.closed)),
        ("gone", json!(a.gone.as_deref())),
        ("gone_since", json!(a.gone_since)),
        ("age", json!(age)),
        ("late", json!(late)),
    ])
}

pub fn task(t: &Task) -> Value {
    object(vec![
        ("slug", json!(&t.slug)),
        ("title", json!(&t.title)),
        ("body", json!(&t.body)),
        ("status", json!(&t.status)),
        ("priority", json!(t.priority)),
        ("parent", json!(t.parent.as_deref())),
        ("created_by", json!(&t.created_by)),
        ("created", json!(t.created)),
        ("updated", json!(t.updated)),
        ("links", json!(&t.links)),
        ("depends", json!(&t.depends)),
        ("roles", Value::Array(t.roles.iter().map(role).collect())),
        ("instruction", json!(t.instruction)),
    ])
}

/// A task as `tasks list --json` lists it: with `in_status_since`, when it took the status it holds
/// ([`crate::stages::in_status_since`]), null where that was not read.
pub fn listed(t: &Task, in_status_since: Option<f64>) -> Value {
    let mut shaped = task(t);
    if let Value::Object(fields) = &mut shaped {
        fields.insert("in_status_since".into(), json!(in_status_since));
    }
    shaped
}

/// A landed task, with the status the landing closed it from.
pub fn landed(l: &Landed) -> Value {
    let mut shaped = task(&l.task);
    if let Value::Object(fields) = &mut shaped {
        fields.insert("from".into(), json!(&l.from));
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
                        ("first_entered", json!(stage.first_entered)),
                        ("seconds", json!(stage.seconds)),
                        ("visits", json!(i64::from(stage.visits))),
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
                            ("status", json!(&segment.status)),
                            ("from", json!(segment.from)),
                            ("to", json!(segment.to)),
                            ("by", json!(segment.by.as_deref())),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("unclaimed_seconds", json!(s.unclaimed_seconds)),
        ("first_claimed", json!(s.first_claimed)),
        ("bounces", json!(i64::from(s.bounces))),
        (
            "current",
            object(vec![
                ("status", json!(&s.current)),
                ("since", json!(s.current_since)),
            ]),
        ),
        ("closed", json!(s.closed)),
    ])
}

pub fn tasks(ts: &[Task]) -> Value {
    Value::Array(ts.iter().map(task).collect())
}

fn role(r: &Role) -> Value {
    object(vec![
        ("handle", json!(&r.handle)),
        ("role", json!(&r.role)),
        ("since", json!(r.since)),
        ("expires", json!(r.expires)),
        ("health", json!(r.health.as_str())),
    ])
}

pub fn session(s: &Session) -> Value {
    let r = &s.row;
    let affinity: Vec<&str> = r.affinity.split(',').filter(|a| !a.is_empty()).collect();
    object(vec![
        ("handle", json!(&r.handle)),
        ("kind", json!(&r.kind)),
        ("parent", json!(r.parent.as_deref())),
        ("worktree", json!(&r.worktree)),
        ("branch", json!(&r.branch)),
        ("model", json!(&r.model)),
        ("pid", json!(r.pid)),
        ("host", json!(&r.host)),
        ("affinity", json!(&affinity)),
        ("connected", json!(r.connected)),
        ("last_seen", json!(r.last_seen)),
        ("last_tool", json!(r.last_tool)),
        ("left_at", json!(r.left_at)),
        ("health", json!(s.health.as_str())),
    ])
}

pub fn message(m: &Message) -> Value {
    object(vec![
        ("id", json!(m.id)),
        ("slug", json!(&m.slug)),
        ("sender", json!(&m.sender)),
        ("recipient", json!(m.recipient.as_deref())),
        ("kind", json!(&m.kind)),
        ("text", json!(&m.text)),
        ("ts", json!(m.ts)),
    ])
}

pub fn messages(ms: &[Message]) -> Value {
    Value::Array(ms.iter().map(message).collect())
}

pub fn launch(r: &TaskSessionRow) -> Value {
    object(vec![
        ("slug", json!(&r.slug)),
        ("handle", json!(&r.handle)),
        ("role", json!(&r.role)),
        ("claude_session", json!(&r.claude_session)),
        ("worktree", json!(&r.worktree)),
        ("branch", json!(&r.branch)),
        ("pane", json!(&r.pane)),
        ("model", json!(&r.model)),
        ("started", json!(r.started)),
        ("closed", json!(r.closed)),
        ("closed_why", json!(&r.closed_why)),
        ("restore", json!(&r.restore)),
    ])
}

pub fn launches(rs: &[TaskSessionRow]) -> Value {
    Value::Array(rs.iter().map(launch).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

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
