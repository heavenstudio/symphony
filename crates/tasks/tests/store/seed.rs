//! A board to seed through the one-way copy, built row by row in the dump's shape.

use super::NOW;
use symphony_board::rows::{Dump, LinkRow, RoleRow, SessionRow, TaskRow};

/// A board to seed, in the dump's shape: sessions seen at `NOW`, tasks in the order given.
#[derive(Default, Clone)]
pub struct Seed {
    pub dump: Dump,
}

impl Seed {
    pub fn session(self, handle: &str) -> Seed {
        self.session_row(session(handle))
    }

    pub fn session_row(mut self, row: SessionRow) -> Seed {
        self.dump.sessions.push(row);
        self
    }

    pub fn task(self, slug: &str, status: &str) -> Seed {
        let row = TaskRow {
            id: self.dump.tasks.len() as i64 + 1,
            ..task(slug, status)
        };
        self.task_row(row)
    }

    pub fn task_row(mut self, row: TaskRow) -> Seed {
        self.dump.tasks.push(row);
        self
    }

    pub fn role(mut self, slug: &str, handle: &str, role: &str) -> Seed {
        let since = NOW + self.dump.roles.len() as f64;
        self.dump.roles.push(RoleRow {
            slug: slug.into(),
            handle: handle.into(),
            role: role.into(),
            since,
            expires: None,
        });
        self
    }

    pub fn reserved(mut self, slug: &str, handle: &str, expires: Option<f64>) -> Seed {
        self.dump.roles.push(RoleRow {
            slug: slug.into(),
            handle: handle.into(),
            role: "reserved".into(),
            since: NOW,
            expires,
        });
        self
    }

    pub fn link(mut self, slug: &str, kind: &str, target: &str) -> Seed {
        self.dump.task_links.push(LinkRow {
            slug: slug.into(),
            kind: kind.into(),
            target: target.into(),
        });
        self
    }
}

pub fn session(handle: &str) -> SessionRow {
    SessionRow {
        handle: handle.into(),
        kind: "worker".into(),
        connected: NOW,
        last_seen: NOW,
        ..Default::default()
    }
}

pub fn task(slug: &str, status: &str) -> TaskRow {
    TaskRow {
        id: 1,
        slug: slug.into(),
        title: format!("Task {slug}"),
        status: status.into(),
        priority: 2,
        created_by: "owner".into(),
        created: NOW,
        updated: NOW,
        ..Default::default()
    }
}
