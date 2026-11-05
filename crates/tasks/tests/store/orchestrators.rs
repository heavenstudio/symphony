//! One orchestrator a task: taking the role, handing it over, and what each orchestrator hears on a
//! board two of them share.

use super::callbacks::Listener;
use super::common::{Fixture, Seed, refused, session};
use super::quiet_callbacks::listen;
use std::collections::BTreeSet;
use tasks::health::{LIVE_SECONDS, STALE_SECONDS};
use tasks::{Health, NewSession, NewTask};

const EMPTY_PULL: &str = "PULL executing=0 ready_unstaffed=0 in_progress_orphaned=0 review_unreviewed=0 landed_open=0 idle=0";
const BUSY_PULL: &str = "PULL executing=1 ready_unstaffed=0 in_progress_orphaned=0 review_unreviewed=0 landed_open=0 idle=0";

/// Two orchestrators, `scratch/one` orchestrated by the first.
fn two_orchestrators() -> (Fixture, String, String) {
    let f = Fixture::empty();
    let (a, b) = (f.session("orchestrator"), f.session("orchestrator"));
    f.task("scratch/one");
    f.store
        .take_role("scratch/one", &a, "orchestrator")
        .unwrap();
    (f, a, b)
}

/// Past the lost line, with only `keep` still beating.
fn go_quiet_but(f: &Fixture, keep: &[&str]) {
    f.advance(LIVE_SECONDS + STALE_SECONDS + 5.0);
    for handle in keep {
        f.store.touch(handle).unwrap();
    }
}

#[test]
fn a_second_orchestrator_is_refused_naming_the_holder_and_the_hand_over() {
    let (f, a, b) = two_orchestrators();
    assert_eq!(
        refused(f.store.take_role("scratch/one", &b, "orchestrator")),
        format!(
            "scratch/one is orchestrated by {a} (live); {a} hands it over with: just tasks handover scratch/one {b}"
        )
    );
    f.advance(LIVE_SECONDS + 1.0);
    f.store.touch(&b).unwrap();
    assert_eq!(
        refused(f.store.take_role("scratch/one", &b, "orchestrator")),
        format!(
            "scratch/one is orchestrated by {a} (stale); {a} hands it over with: just tasks handover scratch/one {b}"
        ),
        "a stale holder still holds"
    );
    assert_eq!(
        f.store.get("scratch/one").unwrap().holders("orchestrator"),
        [a.as_str()]
    );
    // Reviewers are not limited to one.
    let (r1, r2) = (f.session("reviewer"), f.session("reviewer"));
    f.store.take_role("scratch/one", &r1, "reviewer").unwrap();
    f.store.take_role("scratch/one", &r2, "reviewer").unwrap();
    assert_eq!(
        f.store
            .get("scratch/one")
            .unwrap()
            .holders("reviewer")
            .len(),
        2
    );
}

#[test]
fn taking_the_role_over_a_lost_or_left_holder_moves_it_and_says_so() {
    let (f, a, b) = two_orchestrators();
    go_quiet_but(&f, &[&b]);
    assert_eq!(
        f.health(&a),
        Health::Left,
        "an orchestrator holds nothing claimable"
    );
    let task = f
        .store
        .take_role("scratch/one", &b, "orchestrator")
        .unwrap();
    assert_eq!(task.holders("orchestrator"), [b.as_str()]);
    assert_eq!(
        f.said("scratch/one").last().unwrap(),
        &format!("{b} took orchestrator over from {a} (left)")
    );
    // One quiet while it holds claimable work elsewhere is lost, and dropped the same way.
    f.task("scratch/work");
    f.store.claim("scratch/work", &b).unwrap();
    let c = f.session("orchestrator");
    go_quiet_but(&f, &[&c]);
    assert_eq!(f.health(&b), Health::Lost);
    f.store
        .take_role("scratch/one", &c, "orchestrator")
        .unwrap();
    assert_eq!(
        f.said("scratch/one").last().unwrap(),
        &format!("{c} took orchestrator over from {b} (lost)")
    );
}

#[test]
fn retaking_a_role_drops_a_left_double_and_keeps_a_live_one() {
    let mut orch = session("solar");
    orch.kind = "orchestrator".into();
    let mut gone = session("teal");
    gone.kind = "orchestrator".into();
    gone.last_seen -= LIVE_SECONDS + STALE_SECONDS + 5.0;
    let mut other = session("cobalt");
    other.kind = "orchestrator".into();
    let f = Fixture::new(
        Seed::default()
            .session_row(orch)
            .session_row(gone)
            .session_row(other)
            .task("scratch/gate", "ready")
            .role("scratch/gate", "solar", "orchestrator")
            .role("scratch/gate", "teal", "orchestrator")
            .task("scratch/both", "ready")
            .role("scratch/both", "solar", "orchestrator")
            .role("scratch/both", "cobalt", "orchestrator"),
    );
    let task = f
        .store
        .take_role("scratch/gate", "solar", "orchestrator")
        .unwrap();
    assert_eq!(task.holders("orchestrator"), ["solar"]);
    assert_eq!(
        f.said("scratch/gate").last().unwrap(),
        "solar took orchestrator over from teal (left)"
    );
    // Both still there: nothing to settle by taking it, only by a hand-over or a release.
    let task = f
        .store
        .take_role("scratch/both", "solar", "orchestrator")
        .unwrap();
    let mut held = task.holders("orchestrator");
    held.sort_unstable();
    assert_eq!(held, ["cobalt", "solar"]);
    assert!(f.said("scratch/both").is_empty());
    let task = f
        .store
        .hand_over("scratch/both", "cobalt", "solar")
        .unwrap();
    assert_eq!(task.holders("orchestrator"), ["solar"]);
}

#[test]
fn a_hand_over_moves_the_role_in_one_write_naming_the_new_orchestrator() {
    let (f, a, b) = two_orchestrators();
    let task = f.store.hand_over("scratch/one", &a, &b).unwrap();
    assert_eq!(task.holders("orchestrator"), [b.as_str()]);
    assert_eq!(
        f.said("scratch/one").last().unwrap(),
        &format!("{a} handed orchestrator over to {b}; address {b} or @orchestrator")
    );
    assert_eq!(
        refused(f.store.hand_over("scratch/one", &a, &b)),
        format!("{a} does not orchestrate scratch/one")
    );
    assert_eq!(
        refused(f.store.hand_over("scratch/one", &b, &b)),
        format!("{b} already orchestrates scratch/one")
    );
    assert_eq!(
        refused(f.store.hand_over("scratch/one", &b, "nobody")),
        "no session 'nobody' to hand scratch/one to"
    );
    f.store.leave(&a).unwrap();
    assert_eq!(
        refused(f.store.hand_over("scratch/one", &b, &a)),
        format!("{a} has left; hand scratch/one to a session still here")
    );
    f.store.move_to("scratch/one", "cancelled", &b).unwrap();
    assert_eq!(
        refused(f.store.hand_over("scratch/one", &b, "nobody")),
        "scratch/one is cancelled"
    );
}

/// What reached each listener since the last look, through a barrier `sender` sends.
fn heard(f: &Fixture, sender: &str, who: &[(&Listener, &str)]) -> Vec<Vec<String>> {
    f.store.dispatch_callbacks_snapshot().unwrap();
    who.iter()
        .map(|(listener, handle)| listener.drain(f, sender, handle))
        .collect()
}

#[test]
fn a_hand_over_sends_one_role_event_to_each_side() {
    let (f, a, b) = two_orchestrators();
    let w = f.session("worker");
    f.store.claim("scratch/one", &w).unwrap();
    let (la, lb) = (listen(&f, &a), listen(&f, &b));
    assert_eq!(
        (la.text(), lb.text()),
        (BUSY_PULL.into(), EMPTY_PULL.into())
    );
    f.store.hand_over("scratch/one", &a, &b).unwrap();
    let role = format!("ROLE scratch/one {b} orchestrator, in_progress (from {a})");
    assert_eq!(
        heard(&f, &w, &[(&la, &a), (&lb, &b)]),
        [vec![role.clone()], vec![role]]
    );
    assert_eq!(
        heard(&f, &w, &[(&la, &a), (&lb, &b)]),
        [Vec::<String>::new(), vec![]]
    );
}

#[test]
fn each_orchestrator_hears_its_own_tasks_and_the_unclaimed_ones_only() {
    let f = Fixture::empty();
    let (a, b) = (f.session("orchestrator"), f.session("orchestrator"));
    // b launched a's worker: the work is a's all the same.
    let wa = f.session_with(NewSession {
        parent: Some(b.clone()),
        ..NewSession::new("worker")
    });
    let wb = f.session("worker");
    for (slug, orch, worker) in [("scratch/a", &a, &wa), ("scratch/b", &b, &wb)] {
        f.task(slug);
        f.store.take_role(slug, orch, "orchestrator").unwrap();
        f.store.claim(slug, worker).unwrap();
    }
    let (la, lb) = (listen(&f, &a), listen(&f, &b));
    assert_eq!((la.text(), lb.text()), (BUSY_PULL.into(), BUSY_PULL.into()));
    let ears = [(&la, a.as_str()), (&lb, b.as_str())];
    let none = Vec::<String>::new;

    let post = f
        .store
        .say("scratch/a", &wa, "need a hand", Some("@orchestrator"))
        .unwrap();
    assert_eq!(
        heard(&f, &wb, &ears),
        [
            vec![format!(
                "MSG #{} scratch/a from {wa} — need a hand",
                post.id
            )],
            none()
        ]
    );

    f.store.move_to("scratch/b", "blocked", &wb).unwrap();
    assert_eq!(
        heard(&f, &wa, &ears),
        [
            none(),
            vec!["STATUS scratch/b in_progress → blocked".to_string()]
        ]
    );

    go_quiet_but(&f, &[&a, &b, &wb]);
    assert_eq!(f.health(&wa), Health::Lost);
    let heard_now = heard(&f, &wb, &ears);
    assert_eq!(
        heard_now[0][..2],
        [
            format!("LOST {wa} — its work is claimable: scratch/a"),
            "PULL executing=0 ready_unstaffed=0 in_progress_orphaned=1 review_unreviewed=0 landed_open=0 idle=0".to_string(),
        ]
    );
    assert_eq!(
        heard_now[1],
        none(),
        "neither its parent nor another task's orchestrator"
    );

    f.task_with(NewTask::new("scratch/free", "Free work", "owner"));
    f.task_with(NewTask {
        parent: Some("scratch/b".into()),
        ..NewTask::new("scratch/b/sub", "B's sub", "owner")
    });
    let lines = heard(&f, &wb, &ears);
    let news = |lines: &[String]| -> BTreeSet<String> {
        lines
            .iter()
            .filter(|l| l.starts_with("NEW "))
            .cloned()
            .collect()
    };
    assert_eq!(
        news(&lines[0]),
        BTreeSet::from(["NEW scratch/free ready — Free work".to_string()])
    );
    assert_eq!(
        news(&lines[1]),
        BTreeSet::from([
            "NEW scratch/b/sub ready — B's sub".to_string(),
            "NEW scratch/free ready — Free work".to_string(),
        ])
    );
}

#[test]
fn a_post_to_at_orchestrator_reaches_the_nearest_orchestrator_only() {
    let (f, a, b) = two_orchestrators();
    f.task_with(NewTask {
        parent: Some("scratch/one".into()),
        ..NewTask::new("scratch/one/b", "B's part", "owner")
    });
    f.task_with(NewTask {
        parent: Some("scratch/one".into()),
        ..NewTask::new("scratch/one/a", "A's part", "owner")
    });
    f.store
        .take_role("scratch/one/b", &b, "orchestrator")
        .unwrap();
    let w = f.session("worker");
    let to_b = f
        .store
        .say("scratch/one/b", &w, "for b", Some("@orchestrator"))
        .unwrap();
    let to_a = f
        .store
        .say("scratch/one/a", &w, "for a", Some("@orchestrator"))
        .unwrap();
    let ids = |handle: &str| -> Vec<i64> {
        f.store
            .inbox(handle)
            .unwrap()
            .iter()
            .map(|m| m.id)
            .collect()
    };
    assert_eq!((ids(&a), ids(&b)), (vec![to_a.id], vec![to_b.id]));
    // A post by handle still reaches whoever it names.
    let named = f
        .store
        .say("scratch/one/b", &w, "to a by name", Some(&a))
        .unwrap();
    assert_eq!(ids(&a), [to_a.id, named.id]);
}

#[test]
fn each_digest_lists_its_own_and_the_unclaimed_tasks() {
    let f = Fixture::empty();
    let (a, b) = (f.session("orchestrator"), f.session("orchestrator"));
    let idle_b = f.session_with(NewSession {
        parent: Some(b.clone()),
        ..NewSession::new("worker")
    });
    let idle_free = f.session("worker");
    for (slug, orch) in [
        ("scratch/a", Some(&a)),
        ("scratch/b", Some(&b)),
        ("scratch/free", None),
    ] {
        f.task(slug);
        if let Some(orch) = orch {
            f.store.take_role(slug, orch, "orchestrator").unwrap();
        }
    }
    let idle = [idle_b.clone(), idle_free.clone()];
    let digest = tasks::cli::pull::digest(&f.store, &idle, None, f.now(), &[]).unwrap();
    let board = f.store.board().unwrap();
    let entries = |handle: &str| -> BTreeSet<(String, String)> {
        digest.clone().only_for(&board, handle).entries()
    };
    let entry = |name: &str, what: &str| (name.to_string(), what.to_string());
    assert_eq!(
        entries(&a),
        BTreeSet::from([
            entry("ready_unstaffed", "scratch/a"),
            entry("ready_unstaffed", "scratch/free"),
            entry("idle", &idle_free),
        ])
    );
    assert_eq!(
        entries(&b),
        BTreeSet::from([
            entry("ready_unstaffed", "scratch/b"),
            entry("ready_unstaffed", "scratch/free"),
            entry("idle", &idle_b),
            entry("idle", &idle_free),
        ])
    );
    assert_eq!(digest.entries().len(), 5, "the whole board, unscoped");
}
