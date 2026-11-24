//! The owner's offer on a task: each board action decides which work it offers on, whatever the
//! task's status. Each case has a scratch database of its own, never the live board.

use std::path::Path;

use symphony::plugins::Plugins;
use symphony::taskview::owner_offer;
use symphony_plugin::vocabulary::TaskOffer;
use symphony_plugin::{BoardAction, Plugin, Submission, Submitted, TaskRef};

use super::board::{Fixture, Seed};

struct Desk;
struct PickUp;

static DESK: Desk = Desk;

impl Plugin for Desk {
    fn name(&self) -> &str {
        "desk"
    }
    fn board_actions(&self) -> Vec<&dyn BoardAction> {
        vec![&PickUp]
    }
}

/// Offers to pick up work nobody started, and nothing else.
impl BoardAction for PickUp {
    fn name(&self) -> &str {
        "pick up"
    }
    fn offer(&self, _: &Path, task: &TaskRef<'_>) -> Result<TaskOffer, String> {
        if task.status != "ready" {
            return Ok(TaskOffer::default());
        }
        Ok(TaskOffer {
            action: self.name().into(),
            label: "Pick up".into(),
            branch: task.branch.into(),
            payload: Some(task.slug.into()),
        })
    }
    fn submit(&self, _: &Path, _: &TaskRef<'_>, _: &Submission) -> Result<Submitted, String> {
        Err("nothing submits in these cases".into())
    }
}

/// No action offers anything: the offer names the task's branch alone.
fn none(branch: &str) -> Result<TaskOffer, String> {
    Ok(TaskOffer {
        branch: branch.into(),
        ..TaskOffer::default()
    })
}

#[test]
fn an_action_offers_on_the_work_it_chooses_and_a_plain_project_offers_nothing() {
    let f = Fixture::new(
        Seed::default()
            .task("sim/jump", "ready")
            .task("sim/dash", "in_progress")
            .task("sim/roll", "review"),
    );
    let root = Path::new("/nowhere");
    let desk = Plugins::new(vec![&DESK]);
    assert_eq!(
        owner_offer(&f.store, &desk, "sim/jump", root, None),
        Ok(TaskOffer {
            action: "pick up".into(),
            label: "Pick up".into(),
            branch: "sim/jump".into(),
            payload: Some("sim/jump".into()),
        })
    );
    assert_eq!(
        owner_offer(&f.store, &desk, "sim/dash", root, None),
        none("sim/dash")
    );
    assert_eq!(
        owner_offer(&f.store, &desk, "sim/roll", root, None),
        none("sim/roll")
    );
    for slug in ["sim/jump", "sim/dash", "sim/roll"] {
        assert_eq!(
            owner_offer(&f.store, &Plugins::default(), slug, root, None),
            none(slug)
        );
    }
    assert!(owner_offer(&f.store, &desk, "sim/none", root, None).is_err());
}
