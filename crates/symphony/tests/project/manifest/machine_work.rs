//! Per-machine targets are bounded configuration advice, independent from admission policy.

use super::{parsed, refused};

#[test]
fn machine_wip_is_bounded_per_machine_advice_with_no_author_or_resource_admission_effect() {
    let project = parsed("[board.machine_wip.alpha]\nbuild = 2\ntest = 1\n[board.machine_wip.beta]\nbuild = 0\ntest = 3\n").unwrap();
    assert_eq!(project.board.machine_wip["alpha"].build, Some(2));
    assert_eq!(project.board.machine_wip["alpha"].test, Some(1));
    assert_eq!(project.board.machine_wip["beta"].build, Some(0));
    assert_eq!(project.board.machine_wip["beta"].test, Some(3));
    assert_eq!(project.board.orchestrator_slots, None);
    assert!(parsed("").unwrap().board.machine_wip.is_empty());
    for body in [
        "build = -1",
        "test = 'many'",
        "build = 4294967296",
        "limit = 2",
    ] {
        let text = format!("[board.machine_wip.alpha]\n{body}\n");
        assert!(parsed(&text).is_err(), "accepted {text}");
    }
    for id in [" ".into(), "α".repeat(129)] {
        let text = format!("[board.machine_wip.\"{id}\"]\nbuild = 1\n");
        assert!(refused(&text).contains("machine_wip"));
    }
    let too_many = (0..65)
        .map(|id| format!("[board.machine_wip.m{id}]\nbuild = 1\n"))
        .collect::<String>();
    assert!(refused(&too_many).contains("at most 64"));
}
