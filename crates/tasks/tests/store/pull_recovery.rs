//! Historical unanswered requests stay visible while their recovery advice follows staffing now.

use crate::callbacks::Listener;
use crate::common::Fixture;
use crate::common::Seed;
use crate::quiet_callbacks::listen;
use symphony_board::NewLaunch;
use symphony_board::cli::pull::{Digest, digest};

fn look(f: &Fixture) -> Digest {
    digest(&f.store, &[], None, f.now(), &[]).unwrap()
}

fn line(digest: &Digest, post: i64) -> String {
    digest
        .lines()
        .into_iter()
        .find(|line| line.starts_with("  unanswered ") && line.contains(&format!("post #{post} —")))
        .unwrap_or_else(|| panic!("missing post {post}: {:?}", digest.lines()))
}

#[test]
fn an_unanswered_request_keeps_its_attribution_and_follows_the_current_executor() {
    for direct in [false, true] {
        let f = Fixture::new(Seed::default());
        f.task("work");
        let old = f.session("worker");
        f.store.claim("work", &old).unwrap();
        f.store
            .record_launch(&NewLaunch {
                claude_session: "old-conversation".into(),
                ..NewLaunch::new("work", &old, "executor")
            })
            .unwrap();
        let post = f
            .store
            .say(
                "work",
                "owner",
                "still needs reconciliation",
                Some(if direct { &old } else { "@executor" }),
            )
            .unwrap();
        f.advance(31.0 * 60.0);
        let original = look(&f);
        assert!(line(&original, post.id).contains(&format!("just tasks restore work {old}")));

        let next = f.session("worker");
        f.store.claim("work", &next).unwrap();
        // Neither a replacement's claim nor its messages answer the original recipient's post.
        f.store
            .say("work", &next, "working on recovery", None)
            .unwrap();
        let staffed = look(&f);
        assert_eq!(staffed.count("unanswered"), 1);
        let advice = line(&staffed, post.id);
        assert!(
            advice.contains(&format!("{old} (executor now {next}): just tasks say work")),
            "{advice}"
        );
        assert!(advice.ends_with(&format!("--to {next}")), "{advice}");
        assert!(!advice.contains("just tasks launch"), "{advice}");
        assert!(!advice.contains("just tasks restore"), "{advice}");
        f.advance(symphony_board::health::LIVE_SECONDS + 1.0);
        assert_eq!(
            line(&look(&f), post.id),
            advice,
            "a stale replacement still holds its role"
        );
        // A blocked task stays open when its executor leaves; an in-progress departure instead
        // requeues ready work, whose existing staffing advice is already actionable.
        f.store.move_to("work", "blocked", &next).unwrap();

        f.advance(31.0 * 60.0);
        let lost = look(&f);
        let advice = line(&lost, post.id);
        assert!(
            advice.ends_with("just tasks launch work --role executor"),
            "{advice}"
        );
        assert!(!advice.contains("restore work"), "{advice}");
        assert_ne!(original.entries(), staffed.entries());
        assert_ne!(staffed.entries(), lost.entries());
        f.store.leave(&next).unwrap();
        assert_eq!(
            line(&look(&f), post.id),
            advice,
            "departure keeps current recovery advice"
        );
        let retained = f
            .store
            .thread("work")
            .unwrap()
            .into_iter()
            .find(|m| m.id == post.id)
            .unwrap();
        assert_eq!(
            (
                retained.sender,
                retained.recipient,
                retained.text,
                retained.ts
            ),
            (post.sender, post.recipient, post.text, post.ts)
        );
        f.store.revive(&old).unwrap();
        f.store
            .say("work", &old, "original request answered", None)
            .unwrap();
        assert_eq!(look(&f).count("unanswered"), 0);
    }
}

#[test]
fn executor_recovery_does_not_redirect_reviewer_or_owner_questions() {
    let f = Fixture::new(Seed::default());
    f.task("work");
    let old = f.session("worker");
    f.store.claim("work", &old).unwrap();
    f.advance(31.0 * 60.0);
    let next_executor = f.session("worker");
    f.store.claim("work", &next_executor).unwrap();
    f.store.move_to("work", "review", &next_executor).unwrap();
    let reviewer = f.session("reviewer");
    f.store.take_role("work", &reviewer, "reviewer").unwrap();
    let question = f
        .store
        .say("work", "owner", "review this", Some("@reviewer"))
        .unwrap();
    let owner = f
        .store
        .say("work", &next_executor, "owner decision", Some("owner"))
        .unwrap();
    f.advance(31.0 * 60.0);
    f.store.touch(&next_executor).unwrap();
    let next_reviewer = f.session("reviewer");
    f.store
        .take_role("work", &next_reviewer, "reviewer")
        .unwrap();
    let advice = line(&look(&f), question.id);
    assert_eq!(
        advice,
        format!(
            "  unanswered work post #{} — {reviewer}: just tasks launch work --role reviewer",
            question.id
        )
    );
    assert!(
        !look(&f)
            .entries()
            .iter()
            .any(|e| e.1.contains(&format!("#{}", owner.id)))
    );
    let retained = f
        .store
        .thread("work")
        .unwrap()
        .into_iter()
        .find(|m| m.id == owner.id)
        .unwrap();
    assert_eq!(retained.recipient.as_deref(), Some("owner"));
    assert_eq!(retained.text, owner.text);
}

fn heard(listener: &Listener, f: &Fixture, barrier: &str, orch: &str) -> Vec<String> {
    listener
        .drain(f, barrier, orch)
        .into_iter()
        .filter(|l| l.starts_with("unanswered "))
        .collect()
}

#[test]
fn replacement_advice_is_delivered_once_per_action_to_the_current_orchestrator() {
    let f = Fixture::new(Seed::default());
    f.task("work");
    let former = f.session("orchestrator");
    let successor = f.session("orchestrator");
    let old = f.session("worker");
    let barrier = f.session("worker");
    f.store.take_role("work", &former, "orchestrator").unwrap();
    f.store.claim("work", &old).unwrap();
    let first = listen(&f, &former);
    let second = listen(&f, &successor);
    first.drain(&f, &barrier, &former);
    second.drain(&f, &barrier, &successor);
    let post = f
        .store
        .say("work", "owner", "continue", Some("@executor"))
        .unwrap();
    f.advance(31.0 * 60.0);
    for handle in [&former, &successor] {
        f.elsewhere().touch(handle).unwrap();
    }
    f.other().dispatch_callbacks().unwrap();
    let before = heard(&first, &f, &barrier, &former);
    assert_eq!(before.len(), 1, "{before:?}");
    assert!(
        before[0].contains(&format!("post #{} — {old}: just tasks launch", post.id)),
        "{before:?}"
    );

    let next = f.session("worker");
    f.store.touch(&successor).unwrap();
    // Write elsewhere so the queued advice is re-evaluated only after ownership moves.
    f.elsewhere().claim("work", &next).unwrap();
    f.elsewhere()
        .hand_over("work", &former, &successor)
        .unwrap();
    f.other().dispatch_callbacks().unwrap();
    assert!(heard(&first, &f, &barrier, &former).is_empty());
    let followup = heard(&second, &f, &barrier, &successor);
    assert_eq!(followup.len(), 1, "{followup:?}");
    assert!(
        followup[0].ends_with(&format!("--to {next}")),
        "{followup:?}"
    );
    for _ in 0..2 {
        f.other().dispatch_callbacks().unwrap();
        assert!(heard(&second, &f, &barrier, &successor).is_empty());
    }
    f.advance(31.0 * 60.0);
    for handle in [&former, &successor] {
        f.elsewhere().touch(handle).unwrap();
    }
    f.other().dispatch_callbacks().unwrap();
    let recovery = heard(&second, &f, &barrier, &successor);
    assert_eq!(recovery.len(), 1, "{recovery:?}");
    assert!(
        recovery[0].ends_with("just tasks launch work --role executor"),
        "{recovery:?}"
    );
    for _ in 0..2 {
        f.other().dispatch_callbacks().unwrap();
        assert!(heard(&second, &f, &barrier, &successor).is_empty());
    }
}

#[test]
fn queued_launch_advice_is_replaced_before_delivery_after_executor_recovery() {
    use crate::hand_over_outage::Outage;
    use crate::mixed_dispatchers::{older_fields, older_record};
    use crate::pending_pull::waiting_pull;

    let outage = Outage::on();
    let f = &outage.fixture;
    let old = f.session("worker");
    f.store.claim("scratch/one", &old).unwrap();
    outage.cut();
    let post = outage
        .elsewhere
        .say("scratch/one", "owner", "continue", Some("@executor"))
        .unwrap();
    outage.pass(31.0 * 60.0, &[]);
    f.store.dispatch_callbacks_snapshot().unwrap();
    let before = waiting_pull(&outage).expect("recovery waits out the outage");
    assert!(
        before
            .text
            .contains("just tasks launch scratch/one --role executor"),
        "{before:?}"
    );
    let next = f.session("worker");
    outage.elsewhere.claim("scratch/one", &next).unwrap();
    for _ in 0..2 {
        f.other().dispatch_callbacks_snapshot().unwrap();
    }
    let after = waiting_pull(&outage).expect("follow-up waits instead");
    assert_ne!(before.key, after.key);
    assert!(
        after.text.contains(&format!("post #{}", post.id)),
        "{after:?}"
    );
    assert!(after.text.contains(&format!("--to {next}")), "{after:?}");
    assert!(!after.text.contains("just tasks launch"), "{after:?}");
    let back = outage.back();
    let delivered = heard(&back, f, &outage.sender, &outage.former);
    assert_eq!(delivered.len(), 1, "{delivered:?}");
    assert!(
        delivered[0].ends_with(&format!("--to {next}")),
        "{delivered:?}"
    );
    assert_eq!(
        older_fields(f, &outage.former),
        older_record(f, &outage.former)
    );
    outage.pass(601.0, &[&next]);
    for _ in 0..2 {
        f.other().dispatch_callbacks().unwrap();
        assert!(heard(&back, f, &outage.sender, &outage.former).is_empty());
    }
}

#[test]
fn running_test_evidence_keeps_the_current_executor_followup_actionable() {
    let f = Fixture::new(Seed::default());
    f.task("work");
    let old = f.session("worker");
    f.store.claim("work", &old).unwrap();
    let post = f
        .store
        .say("work", "owner", "continue", Some(&old))
        .unwrap();
    f.advance(31.0 * 60.0);
    let next = f.session("worker");
    f.store.claim("work", &next).unwrap();
    f.advance(31.0 * 60.0);
    let lost = look(&f);
    assert!(line(&lost, post.id).ends_with("just tasks launch work --role executor"));
    let running = digest(&f.store, &[], None, f.now(), std::slice::from_ref(&next)).unwrap();
    assert!(line(&running, post.id).ends_with(&format!("--to {next}")));
    assert_ne!(lost.entries(), running.entries());
    let unrelated = digest(&f.store, &[], None, f.now(), &["another-worker".into()]).unwrap();
    assert_eq!(line(&lost, post.id), line(&unrelated, post.id));
}

#[test]
fn a_released_live_successor_cannot_suppress_recovery_even_when_running_other_work() {
    for direct in [false, true] {
        let f = Fixture::new(Seed::default());
        f.task("work");
        let old = f.session("worker");
        f.store.claim("work", &old).unwrap();
        let post = f
            .store
            .say(
                "work",
                "owner",
                "continue",
                Some(if direct { &old } else { "@executor" }),
            )
            .unwrap();
        f.advance(31.0 * 60.0);
        let next = f.session("worker");
        f.store.claim("work", &next).unwrap();
        f.store.move_to("work", "blocked", &next).unwrap();
        let followup = look(&f);
        assert!(line(&followup, post.id).ends_with(&format!("--to {next}")));
        f.store.release("work", &next, Some("executor")).unwrap();
        f.task("other");
        f.store.claim("other", &next).unwrap();
        assert!(f.store.get("work").unwrap().holders("executor").is_empty());
        assert_eq!(
            f.store.session(&next).unwrap().health,
            symphony_board::Health::Live
        );
        let released = look(&f);
        let advice = line(&released, post.id);
        assert!(
            advice.contains(&format!("{old} (last executor {next})")),
            "{advice}"
        );
        assert!(
            advice.ends_with("just tasks launch work --role executor"),
            "{advice}"
        );
        assert_ne!(followup.entries(), released.entries());
        let running_elsewhere =
            digest(&f.store, &[], None, f.now(), std::slice::from_ref(&next)).unwrap();
        assert_eq!(line(&running_elsewhere, post.id), advice);
        assert_eq!(running_elsewhere.entries(), released.entries());
        let retained = f
            .store
            .thread("work")
            .unwrap()
            .into_iter()
            .find(|p| p.id == post.id)
            .unwrap();
        assert_eq!(
            (
                retained.sender,
                retained.recipient,
                retained.text,
                retained.ts
            ),
            (post.sender, post.recipient, post.text, post.ts)
        );
    }
}

#[test]
fn releasing_a_live_successor_delivers_recovery_after_its_followup_once() {
    let f = Fixture::new(Seed::default());
    f.task("work");
    let old = f.session("worker");
    f.store.claim("work", &old).unwrap();
    let post = f
        .store
        .say("work", "owner", "continue", Some(&old))
        .unwrap();
    f.advance(31.0 * 60.0);
    let next = f.session("worker");
    f.store.claim("work", &next).unwrap();
    f.store.move_to("work", "blocked", &next).unwrap();
    let orch = f.session("orchestrator");
    let barrier = f.session("worker");
    f.store.take_role("work", &orch, "orchestrator").unwrap();
    let listener = listen(&f, &orch);
    let followup = heard(&listener, &f, &barrier, &orch);
    assert_eq!(followup.len(), 1, "{followup:?}");
    assert!(followup[0].contains(&format!("post #{} —", post.id)));
    assert!(
        followup[0].ends_with(&format!("--to {next}")),
        "{followup:?}"
    );
    f.elsewhere()
        .release("work", &next, Some("executor"))
        .unwrap();
    assert_eq!(
        f.store.session(&next).unwrap().health,
        symphony_board::Health::Live
    );
    f.other().dispatch_callbacks().unwrap();
    let recovered = heard(&listener, &f, &barrier, &orch);
    assert_eq!(recovered.len(), 1, "{recovered:?}");
    assert!(recovered[0].contains(&format!("post #{} —", post.id)));
    assert!(
        recovered[0].ends_with("just tasks launch work --role executor"),
        "{recovered:?}"
    );
    for _ in 0..2 {
        f.other().dispatch_callbacks().unwrap();
        assert!(heard(&listener, &f, &barrier, &orch).is_empty());
    }
}
