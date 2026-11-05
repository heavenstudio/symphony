//! A project's plugin with one board action, `approve to land`, which the child server holds: it
//! offers `fixture-patch` on work in review, records each call in `land.calls`, and answers an
//! approval as the checkout's files say.

use std::io::Write;
use std::path::Path;

use symphony_plugin::vocabulary::TaskOffer;
use symphony_plugin::{BoardAction, Plugin, Submission, Submitted, TaskRef};

/// The plugin's name, as a project's `[plugins] crates` lists it.
pub const NAME: &str = "fixture-approving";

/// The plugins a child server holds: [`Approving`] alone.
pub static PLUGINS: &[&dyn Plugin] = &[&APPROVING];

static APPROVING: Approving = Approving;

/// What [`Approving`] answers an approval with, and the slugs it releases (JSON), in the checkout.
pub const REFUSAL: &str = "approve.refusal";
pub const RELEASED: &str = "approve.released";

/// The plugin: an approval is answered as [`super::Fixture::approving`] set.
struct Approving;

impl Plugin for Approving {
    fn name(&self) -> &str {
        NAME
    }

    fn board_actions(&self) -> Vec<&dyn BoardAction> {
        vec![self]
    }
}

impl BoardAction for Approving {
    fn name(&self) -> &str {
        "approve to land"
    }

    fn offer(&self, root: &Path, task: &TaskRef<'_>) -> Result<TaskOffer, String> {
        if task.status != "review" {
            return Ok(TaskOffer::default());
        }
        called(root, &format!("offer {}", task.branch));
        Ok(TaskOffer {
            action: BoardAction::name(self).into(),
            label: "Approve to land".into(),
            branch: task.branch.into(),
            payload: Some("fixture-patch".into()),
        })
    }

    fn submit(
        &self,
        root: &Path,
        task: &TaskRef<'_>,
        submission: &Submission,
    ) -> Result<Submitted, String> {
        called(
            root,
            &format!("approve {} {}", task.branch, submission.payload),
        );
        let refusal = std::fs::read_to_string(root.join(REFUSAL)).unwrap_or_default();
        if !refusal.is_empty() {
            return Err(refusal);
        }
        called(root, &format!("release {}", task.branch));
        let released = std::fs::read_to_string(root.join(RELEASED)).unwrap_or_default();
        Ok(Submitted {
            said: format!("approved {} to land", task.branch),
            released: serde_json::from_str(&released).unwrap_or_default(),
        })
    }
}

fn called(root: &Path, call: &str) {
    let mut calls = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("land.calls"))
        .unwrap();
    writeln!(calls, "{call}").unwrap();
}
