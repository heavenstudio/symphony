//! Where a bound subagent's tool calls run, as its binding keeps it from each hook event's `cwd`:
//! the checkout, when it came there, and the earliest time a call of its there read.

use super::beats::{Clocked, inbox};
use super::{bash, tool};
use crate::common::NOW;
use crate::{repository, scratch};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use symphony_board::Binding;

/// A repository's main checkout and a linked worktree of it on `tools/x`, with a folder inside.
fn checkouts(name: &str) -> (PathBuf, PathBuf) {
    let dir = scratch(name);
    let main = repository(&dir.join("repo"));
    let tree = dir.join("wt");
    crate::git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "tools/x",
            tree.to_str().unwrap(),
        ],
    );
    std::fs::create_dir_all(tree.join("crates/tasks")).unwrap();
    (main, tree)
}

/// What `h`'s binding keeps.
fn binding(b: &Clocked, h: &str) -> Binding {
    b.board().agent_bindings().unwrap().remove(h).unwrap()
}

/// Subagent `agent-1`'s binding, in `top` on `branch`, come at `came`, its calls there read from
/// `from` on.
fn placed(top: &Path, branch: &str, came: f64, from: f64) -> Binding {
    Binding {
        agent: "agent-1".into(),
        in_process: true,
        worktree: top.to_str().unwrap().into(),
        branch: branch.into(),
        placed_at: came,
        stay_from: from,
    }
}

/// `event` with its `cwd`.
fn from(mut event: Value, cwd: &Path) -> Value {
    event["cwd"] = json!(cwd.to_str().unwrap());
    event
}

/// Subagent `agent-1`'s background `cargo build`, about to run.
fn background_build() -> Value {
    bash("tools/rs-bin.sh sf-slots cargo build")
        .agent("agent-1")
        .with("run_in_background", json!(true))
        .0
}

#[test]
fn a_subagents_board_command_records_the_checkout_it_runs_in_on_its_binding() {
    let b = Clocked::new();
    let (_, tree) = checkouts("beat-place");
    let h = b.register("worker");
    let bind = bash(&inbox(&h)).agent("agent-1").0;
    b.beat(&from(bind, &tree.join("crates/tasks")));
    assert_eq!(binding(&b, &h), placed(&tree, "tools/x", NOW, NOW));
    // A call in no checkout moves nothing; one in the same checkout keeps when it came.
    b.pass(30.0);
    b.beat(&from(tool("Read", "agent-1"), Path::new("/")));
    b.beat(&from(tool("Read", "agent-1"), &tree));
    assert_eq!(binding(&b, &h), placed(&tree, "tools/x", NOW, NOW));
}

/// A subagent bound in one checkout that starts its background build in another is looked for
/// there: the build's own call records it, whatever the command (the adversarial review's probe).
#[test]
fn a_later_tool_call_of_a_bound_subagent_moves_its_checkout_and_when_it_came() {
    let b = Clocked::new();
    let (main, tree) = checkouts("beat-moved");
    let h = b.register("worker");
    b.beat(&from(bash(&inbox(&h)).agent("agent-1").0, &main));
    assert_eq!(binding(&b, &h).worktree, main.to_str().unwrap());
    b.pass(30.0);
    b.beat(&from(background_build(), &tree.join("crates/tasks")));
    let came = NOW + 30.0;
    assert_eq!(binding(&b, &h), placed(&tree, "tools/x", came, came));
    // Any tool's call says it too: a Read back in the main checkout moves it back.
    b.pass(30.0);
    b.beat(&from(tool("Read", "agent-1"), &main));
    let back = binding(&b, &h);
    assert_eq!(
        (back.worktree.as_str(), back.placed_at),
        (main.to_str().unwrap(), NOW + 60.0)
    );
}

/// The adversarial review's clock probe at the hook: the clock goes back 5 s between the board
/// command that placed the subagent and its background build there. When it came stays; the build's
/// call keeps the earliest time read there, which the agents view reads as a doubt, never absence.
#[test]
fn a_call_after_the_clock_went_back_keeps_the_earliest_time_read_there() {
    let b = Clocked::new();
    let (_, tree) = checkouts("beat-clock-back");
    let h = b.register("worker");
    b.beat(&from(bash(&inbox(&h)).agent("agent-1").0, &tree));
    b.pass(-5.0);
    let build = background_build();
    b.beat(&from(build.clone(), &tree));
    b.beat(&from(crate::beat::Event(build).ended().0, &tree));
    assert_eq!(binding(&b, &h), placed(&tree, "tools/x", NOW, NOW - 5.0));
    let seen = b.board().session(&h).unwrap().row.last_tool;
    assert_eq!(seen, Some(NOW), "its last call stays the latest time read");
}

#[test]
fn a_full_sessions_board_command_records_no_checkout() {
    let b = Clocked::new();
    let (main, _) = checkouts("beat-full-place");
    let h = b.register("worker");
    b.beat(&from(bash(&inbox(&h)).session("s-9").0, &main));
    let mut later = tool("Read", "s-9");
    later.as_object_mut().unwrap().remove("agent_id");
    later["session_id"] = json!("s-9");
    b.beat(&from(later, &main));
    let full = Binding {
        agent: "s-9".into(),
        ..Binding::default()
    };
    assert_eq!(binding(&b, &h), full);
}
