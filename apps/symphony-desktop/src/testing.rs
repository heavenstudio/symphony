//! What the unit tests share: a temporary folder, and project lists made to order.

use std::collections::BTreeMap;

use symphony_api::{ListedCard, ListedProject, ProjectList, TaskAsks, TaskVerdict};

#[expect(
    clippy::new_without_default,
    reason = "a tree is made fresh with new(), never by default"
)]
#[path = "../../../crates/symphony/tests/common/tree.rs"]
mod tree;

pub use tree::Tree;

/// A project `id`, named `id works`, with `cards` waiting, the first titled `next card of ID`, and
/// a task board asking the owner nothing, which its server writes.
pub fn project(id: &str, cards: u32) -> ListedProject {
    ListedProject {
        id: id.into(),
        name: format!("{id} works"),
        refused: None,
        unwell: None,
        version: Some(1),
        waiting: Some(cards),
        next: (cards > 0).then(|| ListedCard {
            title: format!("next card of {id}"),
            href: format!("/{id}/docs/{id}.md#next"),
        }),
        asks: Some(TaskAsks {
            questions: BTreeMap::from([("owner".to_owned(), 0)]),
            verdicts: BTreeMap::new(),
        }),
        writes: Some(true),
    }
}

/// `project` with `questions` asked of the owner on its tasks.
pub fn asking(mut project: ListedProject, questions: u32) -> ListedProject {
    if let Some(asks) = project.asks.as_mut() {
        asks.questions.insert("owner".into(), questions);
    }
    project
}

/// `project` with the newest verdict, line `id`, by `reviewer` on `task`.
pub fn judged(mut project: ListedProject, id: i64, task: &str, verdict: &str) -> ListedProject {
    if let Some(asks) = project.asks.as_mut() {
        asks.verdicts.insert(
            "owner".into(),
            TaskVerdict {
                id,
                task: task.into(),
                verdict: verdict.into(),
                reviewer: "slate-wren".into(),
                at: 1_763_213_045.25,
            },
        );
    }
    project
}

/// The list of a server whose installation is `installation`, serving `projects`.
pub fn listing(installation: Option<&str>, projects: Vec<ListedProject>) -> ProjectList {
    ProjectList {
        installation: installation.map(str::to_owned),
        pin: 1,
        projects,
    }
}
