//! A full session cannot become a process-waiting subagent before its callback registers.

use super::*;

#[test]
fn proven_full_sessions_resist_takeover_and_same_key_downgrade_in_the_health_view() {
    for key in ["different-subagent", "full-session"] {
        let c = Case::new();
        let tree = linked_worktree(&format!("full-provenance-{key}"));
        let handle = c.main_session();
        c.f.store.claim("sim/jump", &handle).unwrap();
        assert!(c.f.store.bind_agent("full-session", &handle, true).unwrap());
        let before = c.f.store.agent_bindings().unwrap().remove(&handle).unwrap();
        assert_eq!(before.agent, "full-session");
        assert!(!before.in_process, "{before:?}");
        let accepted = c.f.store.bind_agent(key, &handle, false).unwrap();
        assert!(!accepted, "{key}: takeover/downgrade was accepted");
        c.call_reading(key, &tree, Some(&since_boot(100.0)));
        c.processes_in(vec![booted(&tree, 100.0)]);
        c.past_the_lost_line();
        let after = c.f.store.agent_bindings().unwrap().remove(&handle).unwrap();
        let a = c.agent(&handle);
        assert_eq!(after, before, "{key}: {a:?}");
        assert_eq!(a.worktree, "/main", "{key}: {a:?}");
        assert_eq!(
            (a.state, a.health, a.detail),
            ("gone", Health::Lost, None),
            "{key}"
        );
        assert_eq!(
            c.listed.get(),
            0,
            "a full session has no inferred process ownership"
        );
    }
}
