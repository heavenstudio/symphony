//! The board settles what the tasks a custom workflow holds need of it wherever it gives: a
//! detached dispatcher's pass recovers a lost holder, and a plain store's pass, `agent-send`'s,
//! settles nothing (`Store::giving`).

use super::common::{Fixture, Seed, held_flow, take_edge};
use symphony_board::NewTask;
use symphony_board::health::{LIVE_SECONDS, STALE_SECONDS};
use symphony_board::workflow::Host;

#[test]
fn only_a_giving_stores_pass_settles_a_held_tasks_lost_holder() {
    let f = Fixture::new(Seed::default());
    let (orch, amber) = (f.session("orchestrator"), f.session("worker"));
    f.store
        .activate_workflow(&held_flow(), &Host::local())
        .unwrap();
    f.store
        .create(&NewTask::new("sim/jump", "Jump", &orch))
        .unwrap();
    take_edge(&f.store, "sim/jump", &amber, "claim");
    // orch listens on a channel; amber goes lost.
    let _ear = crate::quiet_callbacks::listen(&f, &orch);
    f.advance(LIVE_SECONDS + STALE_SECONDS + 1.0);
    f.store.touch(&orch).unwrap();
    let at = |f: &Fixture| {
        let p = f.store.workflow_position("sim/jump").unwrap().unwrap();
        format!("{}/{}@{}", p.phase, p.state, p.revision)
    };
    // A plain store's delivering pass (agent-send's, the server's own dispatch) settles nothing.
    f.store.dispatch_callbacks().unwrap();
    assert_eq!(at(&f), "build/working@2");
    // A detached dispatcher's recovers the lost holder by its position's action, once.
    f.other().giving().dispatch_callbacks().unwrap();
    assert_eq!(at(&f), "build/available@3");
    f.other().giving().dispatch_callbacks().unwrap();
    assert_eq!(at(&f), "build/available@3");
}
