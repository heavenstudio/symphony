//! The board's records as JSON: the fields and their order.

use crate::pyjson::Py;
use crate::rows::TaskSessionRow;
use crate::stages::Stages;
use crate::{Ask, Landed, Message, Role, Session, Task};

/// An ask as the board holds it, with its age at `now` and whether that makes it late.
pub fn ask(a: &Ask, now: f64) -> Py {
    let (age, late) = crate::store::asks::age(a, now);
    Py::obj(vec![
        ("id", Py::Int(a.id)),
        ("kind", Py::str(&a.kind)),
        ("path", Py::opt_str(a.path.as_deref())),
        ("card", Py::opt_str(a.card.as_deref())),
        ("block", Py::opt_str(a.block.as_deref())),
        ("tree", Py::opt_str(a.tree.as_deref())),
        ("task", Py::opt_str(a.task.as_deref())),
        ("thread", Py::opt_str(a.thread.as_deref())),
        ("words", Py::str(&a.words)),
        ("asked", Py::Float(a.asked)),
        ("lines", Py::Int(a.lines)),
        ("last", Py::Float(a.last)),
        ("holder", Py::opt_str(a.holder.as_deref())),
        ("held", Py::opt_float(a.held)),
        (
            "before",
            Py::List(
                a.before
                    .iter()
                    .map(|h| {
                        Py::obj(vec![
                            ("handle", Py::str(&h.handle)),
                            ("since", Py::Float(h.since)),
                            ("until", Py::Float(h.until)),
                            ("why", Py::str(&h.why)),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("open", Py::Bool(a.open)),
        ("answered", Py::opt_float(a.answered)),
        ("answered_by", Py::opt_str(a.answered_by.as_deref())),
        ("answer", Py::opt_str(a.answer.as_deref())),
        ("closed", Py::Bool(a.closed)),
        ("gone", Py::opt_str(a.gone.as_deref())),
        ("gone_since", Py::opt_float(a.gone_since)),
        ("age", Py::Float(age)),
        ("late", Py::Bool(late)),
    ])
}

pub fn task(t: &Task) -> Py {
    Py::obj(vec![
        ("slug", Py::str(&t.slug)),
        ("title", Py::str(&t.title)),
        ("body", Py::str(&t.body)),
        ("status", Py::str(&t.status)),
        ("priority", Py::Int(t.priority)),
        ("parent", Py::opt_str(t.parent.as_deref())),
        ("created_by", Py::str(&t.created_by)),
        ("created", Py::Float(t.created)),
        ("updated", Py::Float(t.updated)),
        ("links", Py::strs(&t.links)),
        ("depends", Py::strs(&t.depends)),
        ("roles", Py::List(t.roles.iter().map(role).collect())),
        ("instruction", Py::Bool(t.instruction)),
    ])
}

/// A landed task, with the status the landing closed it from.
pub fn landed(l: &Landed) -> Py {
    let mut shaped = task(&l.task);
    if let Py::Obj(fields) = &mut shaped {
        fields.push(("from".into(), Py::str(&l.from)));
    }
    shaped
}

/// An object whose keys are data, in the order given.
pub fn keyed(fields: impl Iterator<Item = (String, Py)>) -> Py {
    Py::Obj(fields.collect())
}

/// Where a task's time went ([`crate::stages`]); every duration in seconds.
pub fn stages(s: &Stages) -> Py {
    Py::obj(vec![
        (
            "statuses",
            keyed(s.stages.iter().map(|stage| {
                (
                    stage.status.clone(),
                    Py::obj(vec![
                        ("first_entered", Py::Float(stage.first_entered)),
                        ("seconds", Py::Float(stage.seconds)),
                        ("visits", Py::Int(i64::from(stage.visits))),
                    ]),
                )
            })),
        ),
        (
            "segments",
            Py::List(
                s.segments
                    .iter()
                    .map(|segment| {
                        Py::obj(vec![
                            ("status", Py::str(&segment.status)),
                            ("from", Py::Float(segment.from)),
                            ("to", Py::opt_float(segment.to)),
                            ("by", Py::opt_str(segment.by.as_deref())),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("unclaimed_seconds", Py::Float(s.unclaimed_seconds)),
        ("first_claimed", Py::opt_float(s.first_claimed)),
        ("bounces", Py::Int(i64::from(s.bounces))),
        (
            "current",
            Py::obj(vec![
                ("status", Py::str(&s.current)),
                ("since", Py::Float(s.current_since)),
            ]),
        ),
        ("closed", Py::opt_float(s.closed)),
    ])
}

pub fn tasks(ts: &[Task]) -> Py {
    Py::List(ts.iter().map(task).collect())
}

fn role(r: &Role) -> Py {
    Py::obj(vec![
        ("handle", Py::str(&r.handle)),
        ("role", Py::str(&r.role)),
        ("since", Py::Float(r.since)),
        ("expires", Py::opt_float(r.expires)),
        ("health", Py::str(r.health.as_str())),
    ])
}

pub fn session(s: &Session) -> Py {
    let r = &s.row;
    let affinity: Vec<&str> = r.affinity.split(',').filter(|a| !a.is_empty()).collect();
    Py::obj(vec![
        ("handle", Py::str(&r.handle)),
        ("kind", Py::str(&r.kind)),
        ("parent", Py::opt_str(r.parent.as_deref())),
        ("worktree", Py::str(&r.worktree)),
        ("branch", Py::str(&r.branch)),
        ("model", Py::str(&r.model)),
        ("pid", r.pid.map_or(Py::Null, Py::Int)),
        ("host", Py::str(&r.host)),
        ("affinity", Py::strs(&affinity)),
        ("connected", Py::Float(r.connected)),
        ("last_seen", Py::Float(r.last_seen)),
        ("last_tool", Py::opt_float(r.last_tool)),
        ("left_at", Py::opt_float(r.left_at)),
        ("health", Py::str(s.health.as_str())),
    ])
}

pub fn message(m: &Message) -> Py {
    Py::obj(vec![
        ("id", Py::Int(m.id)),
        ("slug", Py::str(&m.slug)),
        ("sender", Py::str(&m.sender)),
        ("recipient", Py::opt_str(m.recipient.as_deref())),
        ("kind", Py::str(&m.kind)),
        ("text", Py::str(&m.text)),
        ("ts", Py::Float(m.ts)),
    ])
}

pub fn messages(ms: &[Message]) -> Py {
    Py::List(ms.iter().map(message).collect())
}

pub fn launch(r: &TaskSessionRow) -> Py {
    Py::obj(vec![
        ("slug", Py::str(&r.slug)),
        ("handle", Py::str(&r.handle)),
        ("role", Py::str(&r.role)),
        ("claude_session", Py::str(&r.claude_session)),
        ("worktree", Py::str(&r.worktree)),
        ("branch", Py::str(&r.branch)),
        ("pane", Py::str(&r.pane)),
        ("model", Py::str(&r.model)),
        ("started", Py::Float(r.started)),
        ("closed", Py::opt_float(r.closed)),
        ("closed_why", Py::str(&r.closed_why)),
        ("restore", Py::str(&r.restore)),
    ])
}

pub fn launches(rs: &[TaskSessionRow]) -> Py {
    Py::List(rs.iter().map(launch).collect())
}
