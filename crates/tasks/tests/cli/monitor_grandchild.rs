//! Grandchild completion reaches the root through the public command boundary.

use super::{Monitor, Watch};

#[test]
fn a_grandchilds_messages_and_status_reach_the_root_orchestrator() {
    let m = Monitor::new("grandchild");
    let (orch, exe) = (m.register("orchestrator"), m.register("worker"));
    m.ok(&orch, &["create", "x/a", "Top"]);
    m.ok(&orch, &["role", "x/a", "orchestrator"]);
    m.ok(&orch, &["create", "x/a/b", "Middle", "--parent", "x/a"]);
    m.ok(&orch, &["create", "x/a/b/c", "Bottom", "--parent", "x/a/b"]);
    m.watch(&orch, Watch::polls(4), &|m, n| {
        let step: &[&str] = match n {
            0 => &["claim", "x/a/b/c"],
            1 => &["say", "x/a/b/c", "measuring now"],
            2 => &["done", "x/a/b/c", "integrated into parent feature"],
            _ => return,
        };
        m.ok(&exe, step);
    });
    assert_eq!(
        m.starting(&["ROLE ", "STATUS "]),
        ["STATUS x/a/b/c ready → in_progress"]
    );
    assert_eq!(
        m.starting(&["VERDICT "]),
        [format!(
            "VERDICT x/a/b/c from {exe} — done: integrated into parent feature"
        )]
    );
    assert_eq!(m.board().get("x/a/b/c").unwrap().status, "done");
    assert!(m.starting(&["MSG "]).is_empty());
}
