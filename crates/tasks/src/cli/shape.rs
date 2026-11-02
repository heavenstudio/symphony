//! The board's records as JSON: the fields and their order.

use crate::pyjson::Py;
use crate::rows::TaskSessionRow;
use crate::{Message, Role, Session, Task};

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
