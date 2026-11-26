//! `agent-send` follows no workflow: its message moves no task and its program links none of the
//! workflow, so a workflow the default branch commits is activated on the live board by the next
//! `tasks` write, never by a message (docs/workflow/WORKFLOW.md, "Activation and mappings").

use super::{Held, Live, SEND};
use crate::common::NOW;
use crate::{git, project_file};
use std::path::Path;
use symphony_board::{Opening, Store};

#[test]
fn a_message_activates_no_committed_workflow_and_the_next_tasks_write_does() {
    let live = Live::new("main-only-unfollowed", Held::Own);
    let mut handles = Vec::new();
    for _ in 0..2 {
        let ran = live.tasks(&live.wt, &["register", "--kind", "worker"]);
        assert_eq!(ran.code, 0, "{}", ran.err);
        handles.push(ran.out.trim().to_string());
    }
    let active = || {
        Store::unchecked_on(
            live.database("tasks"),
            Box::new(|| NOW),
            Box::new(|_| true),
            "",
            &Opening::write(),
        )
        .workflow_active()
        .unwrap()
    };
    assert_eq!(active(), None);
    // The default branch commits a workflow once the board is made.
    std::fs::write(
        live.main.join("symphony.toml"),
        project_file("live", crate::workflow::FLOW),
    )
    .unwrap();
    git(&live.main, &["commit", "-qam", "a workflow"]);

    // The message reaches the board, which opened to write and found the recipient with no
    // channel to send on: it followed nothing.
    let (to, from) = (&handles[0], &handles[1]);
    let ran = live.run(
        Path::new(SEND),
        &live.wt,
        &[],
        &[to, "hello", "--as", from],
        "",
    );
    assert!(
        ran.err
            .starts_with(&format!("✖ {to} has no native channel")),
        "{}{}",
        ran.out,
        ran.err
    );
    assert_eq!(active(), None, "agent-send activated the workflow");

    let ran = live.tasks(&live.wt, &["list"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(active().map(|a| a.generation), Some(1));
}
