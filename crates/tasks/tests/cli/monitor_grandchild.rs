//! Grandchild completion reaches the root through the public command boundary.

use super::{Monitor, Watch};

#[test]
fn a_grandchilds_messages_and_status_reach_the_root_orchestrator() {
    let m = Monitor::new("grandchild");
    let (orch, exe) = (m.register("orchestrator"), m.register("worker"));
    m.ok(&orch, &["create", "a", "Top"]);
    m.ok(&orch, &["role", "a", "orchestrator"]);
    m.ok(&orch, &["create", "a/b", "Middle", "--parent", "a"]);
    m.ok(&orch, &["create", "a/b/c", "Bottom", "--parent", "a/b"]);
    m.watch(&orch, Watch::polls(4), &|m, n| {
        let step: &[&str] = match n {
            0 => &["claim", "a/b/c"],
            1 => &["say", "a/b/c", "measuring now"],
            2 => &["done", "a/b/c", "integrated into parent feature"],
            _ => return,
        };
        m.ok(&exe, step);
    });
    assert_eq!(
        m.starting(&["ROLE ", "STATUS "]),
        ["STATUS a/b/c ready → in_progress"]
    );
    assert_eq!(
        m.starting(&["VERDICT "]),
        [format!(
            "VERDICT a/b/c from {exe} — done: integrated into parent feature"
        )]
    );
    assert_eq!(m.board().get("a/b/c").unwrap().status, "done");
    assert!(m.starting(&["MSG "]).is_empty());
}
