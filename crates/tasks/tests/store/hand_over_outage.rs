//! What waited for an orchestrator that could not be reached, when a hand-over took a task it names
//! away meanwhile: each line about that work is taken back or said of the work it still hears, and
//! what older dispatchers read still counts it as said. The pane watch's BLOCKED is in
//! `blocked_panes`, whose cases run with a herdr.

use super::callbacks::Listener;
use super::common::Fixture;
use super::mixed_dispatchers::{older_fields, older_record};
use super::orchestrators::heard;
use super::quiet_callbacks::listen;
use symphony_board::health::{LIVE_SECONDS, STALE_SECONDS};
use symphony_board::{Health, NewSession, NewTask};

/// The board of one outage: `former` orchestrates `scratch/one`, `successor` is another
/// orchestrator, and `sender` speaks for the board.
struct Outage {
    fixture: Fixture,
    former: String,
    successor: String,
    sender: String,
}

impl Outage {
    fn new() -> Outage {
        let fixture = Fixture::empty();
        let former = fixture.session("orchestrator");
        let successor = fixture.session("orchestrator");
        let sender = fixture.session("worker");
        fixture.task("scratch/one");
        let one = fixture
            .store
            .take_role("scratch/one", &former, "orchestrator");
        one.unwrap();
        Outage {
            fixture,
            former,
            successor,
            sender,
        }
    }

    /// A child of `parent`.
    fn child(&self, parent: &str) -> String {
        self.fixture.session_with(NewSession {
            parent: Some(parent.to_owned()),
            ..NewSession::new("worker")
        })
    }

    fn subtask(&self, slug: &str) {
        self.fixture.task_with(NewTask {
            parent: Some("scratch/one".into()),
            ..NewTask::new(slug, "Jump departure", "owner")
        });
    }

    /// The former orchestrator's callback again, at a new listener: what waited is delivered.
    fn back(&self) -> Listener {
        let back = Listener::new(&format!("{}-back", self.fixture.store.client().database()));
        let address = format!("uds:{}", back.path.display());
        let callback = (self.fixture.store).callback(&self.former, Some(&address), false, false);
        callback.unwrap();
        back
    }
}

#[test]
fn what_waited_out_an_outage_says_nothing_of_the_work_a_hand_over_took_away() {
    for keeps_work in [false, true] {
        let outage = Outage::new();
        let Outage {
            fixture,
            former,
            successor,
            sender,
        } = &outage;
        // The former orchestrator's child, and the successor's working on the former's task.
        let (lost, stale) = (outage.child(former), outage.child(successor));
        outage.subtask("scratch/one/st");
        outage.subtask("scratch/one/stale");
        fixture.store.claim("scratch/one", &lost).unwrap();
        fixture.store.claim("scratch/one/stale", &stale).unwrap();
        if keeps_work {
            fixture.task("scratch/kept");
            let kept = fixture
                .store
                .take_role("scratch/kept", former, "orchestrator");
            kept.unwrap();
            fixture.store.claim("scratch/kept", &lost).unwrap();
        }
        let gone = listen(fixture, former);
        assert!(gone.text().starts_with("PULL "));
        drop(gone);
        // While it cannot be reached, its children go quiet, and its task gains a subtask, a
        // status move, a reviewer and a post; written on another host, they wait for one dispatch.
        let elsewhere = fixture.elsewhere();
        fixture.advance(STALE_SECONDS);
        for handle in [former, successor, sender, &stale] {
            elsewhere.touch(handle).unwrap();
        }
        fixture.advance(LIVE_SECONDS + 5.0);
        for handle in [former, successor, sender] {
            elsewhere.touch(handle).unwrap();
        }
        assert_eq!(
            (fixture.health(&lost), fixture.health(&stale)),
            (Health::Lost, Health::Stale)
        );
        elsewhere
            .create(&NewTask {
                parent: Some("scratch/one".into()),
                ..NewTask::new("scratch/one/new", "Jump departure", "owner")
            })
            .unwrap();
        (elsewhere.move_to("scratch/one/st", "backlog", sender)).unwrap();
        let reviewer = elsewhere.register(&NewSession::new("reviewer")).unwrap();
        let reviewer = reviewer.row.handle;
        (elsewhere.take_role("scratch/one", &reviewer, "reviewer")).unwrap();
        let post = elsewhere
            .say(
                "scratch/one",
                sender,
                "for the orchestrator",
                Some("@orchestrator"),
            )
            .unwrap();
        fixture.store.dispatch_callbacks_snapshot().unwrap();
        let waiting: Vec<String> = (fixture.store.callback_inbox(former).unwrap().into_iter())
            .map(|(_, text)| text)
            .collect();
        let mut kinds: Vec<&str> = (waiting.iter())
            .map(|text| text.split(' ').next().unwrap())
            .collect();
        kinds.sort_unstable();
        let held = if keeps_work {
            "scratch/kept, scratch/one"
        } else {
            "scratch/one"
        };
        let lost_line = format!("LOST {lost} — its work is claimable: {held}");
        assert_eq!(
            kinds,
            ["LOST", "NEW", "PULL", "ROLE", "STALE", "STATUS"],
            "{waiting:?}"
        );
        assert!(waiting.contains(&lost_line), "{waiting:?}");

        fixture
            .store
            .hand_over("scratch/one", former, successor)
            .unwrap();
        let back = outage.back();
        let mut told = Vec::new();
        if keeps_work {
            told.push(format!("LOST {lost} — its work is claimable: scratch/kept"));
        }
        // A post was said to whoever held the role then; the hand-over is said to both sides.
        told.push(format!(
            "MSG #{} scratch/one from {sender} — for the orchestrator",
            post.id
        ));
        told.push(format!(
            "ROLE scratch/one {successor} orchestrator, in_progress (from {former})"
        ));
        if keeps_work {
            // The PULL is built again of the work it still hears.
            told.extend([
                "PULL executing=0 ready_unstaffed=0 in_progress_orphaned=1 review_unreviewed=0 landed_open=0 idle=0".to_string(),
                "in_progress_orphaned p2 scratch/kept — Jump departure".to_string(),
            ]);
        }
        assert_eq!(back.drain(fixture, sender, former), told);
        // What older dispatchers read still records what it was told, so they never say it.
        assert_eq!(older_fields(fixture, former), older_record(fixture, former));
        // Nor is any of it said at the next periodic look, while the successor holds the task.
        fixture.advance(601.0);
        for handle in [former, successor, sender] {
            fixture.store.touch(handle).unwrap();
        }
        assert_eq!(
            heard(fixture, sender, &[(&back, former)]),
            [Vec::<String>::new()]
        );
    }
}

#[test]
fn a_child_quiet_on_work_handed_away_meanwhile_is_not_said_to_the_orchestrator_it_left() {
    for (quiet, kind) in [
        (LIVE_SECONDS + 1.0, "STALE"),
        (LIVE_SECONDS + STALE_SECONDS + 5.0, "LOST"),
    ] {
        let outage = Outage::new();
        let Outage {
            fixture,
            former,
            successor,
            sender,
        } = &outage;
        let child = outage.child(former);
        fixture.store.claim("scratch/one", &child).unwrap();
        let gone = listen(fixture, former);
        assert!(gone.text().starts_with("PULL "));
        drop(gone);
        let elsewhere = fixture.elsewhere();
        fixture.advance(quiet);
        for handle in [former, successor, sender] {
            elsewhere.touch(handle).unwrap();
        }
        fixture.store.dispatch_callbacks_snapshot().unwrap();
        let notice = format!("{kind} {child} ");
        let waiting = fixture.store.callback_inbox(former).unwrap();
        assert!(
            (waiting.iter()).any(|(_, text)| text.starts_with(&notice)),
            "{kind}: {waiting:?}"
        );
        fixture
            .store
            .hand_over("scratch/one", former, successor)
            .unwrap();
        let told = outage.back().drain(fixture, sender, former);
        let moved =
            format!("ROLE scratch/one {successor} orchestrator, in_progress (from {former})");
        assert!(told.contains(&moved), "{kind}: {told:?}");
        assert!(
            !(told.iter()).any(|line| line.starts_with(&notice)),
            "{kind}: {told:?}"
        );
    }
}
