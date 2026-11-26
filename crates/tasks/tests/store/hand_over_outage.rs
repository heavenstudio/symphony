//! What waited for an orchestrator that could not be reached, when a hand-over took a task it names
//! away meanwhile: each line is said only if a dispatch would tell it the line now, a LOST of the
//! work it is still told of, and what older dispatchers read still counts it as said. The pane
//! watch's BLOCKED is in `blocked_panes`, whose cases run with an hc; a PULL waiting while the
//! digest changes beneath it, in `pending_pull`.

use super::callbacks::Listener;
use super::common::{Fixture, HOST};
use super::mixed_dispatchers::{older_fields, older_record};
use super::orchestrators::heard;
use super::quiet_callbacks::listen;
use serde_json::json;
use symphony_board::health::{LIVE_SECONDS, STALE_SECONDS};
use symphony_board::{Health, NewSession, NewTask, Store};

/// The board of one outage: `former` orchestrates `scratch/one`, `successor` is another
/// orchestrator, and `sender` speaks for the board.
pub(super) struct Outage {
    pub(super) fixture: Fixture,
    /// Writes on another host, which wait for the next dispatch here.
    pub(super) elsewhere: Store,
    pub(super) former: String,
    pub(super) successor: String,
    pub(super) sender: String,
}

/// What the first case's outage left: the former orchestrator's lost child, and the post to
/// `@orchestrator` said meanwhile.
struct Missed {
    lost: String,
    post: i64,
}

impl Outage {
    pub(super) fn on() -> Outage {
        let fixture = Fixture::new(crate::common::Seed::default());
        let former = fixture.session("orchestrator");
        let successor = fixture.session("orchestrator");
        let sender = fixture.session("worker");
        fixture.task("scratch/one");
        let one = fixture
            .store
            .take_role("scratch/one", &former, "orchestrator");
        one.unwrap();
        Outage {
            elsewhere: fixture.elsewhere(),
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

    /// The former orchestrator's callback, heard once and then gone.
    pub(super) fn cut(&self) {
        let gone = listen(&self.fixture, &self.former);
        let first = gone.text();
        assert!(first.starts_with("PULL "), "{first}");
    }

    /// `seconds` pass in which only the orchestrators, the sender and `beating` show a sign.
    pub(super) fn pass(&self, seconds: f64, beating: &[&str]) {
        self.fixture.advance(seconds);
        let always = [&self.former, &self.successor, &self.sender].map(String::as_str);
        let signs = always.iter().chain(beating);
        for handle in signs {
            self.elsewhere.touch(handle).unwrap();
        }
    }

    /// The lines one dispatch here left waiting for the former orchestrator.
    fn waiting(&self) -> Vec<String> {
        self.fixture.store.dispatch_callbacks_snapshot().unwrap();
        let inbox = self.fixture.store.callback_inbox(&self.former).unwrap();
        inbox.into_iter().map(|(_, text)| text).collect()
    }

    /// The first case's outage. The former's child, `lost`, executes `scratch/one` (and
    /// `scratch/kept`, the former's too, when `keeps_work`); the successor's child executes
    /// `scratch/one/stale`. While the former cannot be reached they go lost and stale, and its task
    /// gains a subtask, a status move, a reviewer and a post.
    fn missed(&self, keeps_work: bool) -> Missed {
        let Outage {
            fixture,
            elsewhere,
            former,
            successor,
            sender,
        } = self;
        let (lost, stale) = (self.child(former), self.child(successor));
        self.subtask("scratch/one/st");
        self.subtask("scratch/one/stale");
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
        self.cut();
        self.pass(STALE_SECONDS, &[&stale]);
        self.pass(LIVE_SECONDS + 5.0, &[]);
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
        let post = elsewhere.say(
            "scratch/one",
            sender,
            "for the orchestrator",
            Some("@orchestrator"),
        );
        Missed {
            lost,
            post: post.unwrap().id,
        }
    }

    /// The former orchestrator's callback again, at a new listener: what waited is delivered.
    pub(super) fn back(&self) -> Listener {
        let back = Listener::new(&format!(
            "{}-back",
            self.fixture.store.database().database()
        ));
        let address = format!("uds:{}", back.path.display());
        let callback = (self.fixture.store).callback(&self.former, Some(&address), false, false);
        callback.unwrap();
        back
    }
}

#[test]
fn what_waited_out_an_outage_says_nothing_of_the_work_a_hand_over_took_away() {
    for keeps_work in [false, true] {
        let outage = Outage::on();
        let Outage {
            fixture,
            former,
            successor,
            sender,
            ..
        } = &outage;
        let Missed { lost, post } = outage.missed(keeps_work);
        let waiting = outage.waiting();
        let mut kinds: Vec<&str> = (waiting.iter())
            .map(|text| text.split(' ').next().unwrap())
            .collect();
        symphony_board::order::sort_by(&mut kinds, &Ord::cmp);
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
            // The PULL, in its place, says what changed in the work it still hears: of the three
            // running tasks it heard, two are the successor's now and the third lost its executor,
            // and scratch/one/st went back to the backlog.
            told.extend([
                format!("LOST {lost} — its work is claimable: scratch/kept"),
                "PULL executing=0 (was 3) ready_unstaffed=0 (was 1) in_progress_orphaned=1 (was 0)"
                    .to_string(),
                "in_progress_orphaned p2 scratch/kept — Jump departure".to_string(),
                "resolved ready_unstaffed scratch/one/st".to_string(),
            ]);
        }
        // A post was said to whoever held the role then; the hand-over is its own write, which it
        // is not told.
        told.push(format!(
            "MSG #{post} scratch/one from {sender} — for the orchestrator"
        ));
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

/// One child going quiet on `scratch/one` while the former orchestrator cannot be reached.
struct Quiet {
    /// Whether it is the former orchestrator's child, else the successor's.
    formers: bool,
    /// How long it shows no sign.
    seconds: f64,
    /// The notice that waits: STALE or LOST.
    notice: &'static str,
    /// Whether its claim is let go of after the hand-over.
    released: bool,
    /// Whether the former orchestrator hears the notice at last.
    said: bool,
}

#[test]
fn a_child_quiet_meanwhile_is_said_after_a_hand_over_only_to_an_orchestrator_told_of_it_now() {
    let stale = LIVE_SECONDS + 1.0;
    let lost = LIVE_SECONDS + STALE_SECONDS + 5.0;
    let cases = [
        // Its work is the successor's now.
        Quiet {
            formers: true,
            seconds: stale,
            notice: "STALE",
            released: false,
            said: false,
        },
        Quiet {
            formers: true,
            seconds: lost,
            notice: "LOST",
            released: false,
            said: false,
        },
        // Holding nothing, a child is its parent's to hear of, and nobody else's.
        Quiet {
            formers: false,
            seconds: stale,
            notice: "STALE",
            released: true,
            said: false,
        },
        Quiet {
            formers: true,
            seconds: stale,
            notice: "STALE",
            released: true,
            said: true,
        },
    ];
    for Quiet {
        formers,
        seconds,
        notice,
        released,
        said,
    } in cases
    {
        let outage = Outage::on();
        let Outage {
            fixture,
            elsewhere,
            former,
            successor,
            sender,
        } = &outage;
        let child = outage.child(if formers { former } else { successor });
        fixture.store.claim("scratch/one", &child).unwrap();
        outage.cut();
        outage.pass(seconds, &[]);
        let notice = format!("{notice} {child} ");
        let case = format!("{notice}(the former's: {formers}, released: {released})");
        let waiting = outage.waiting();
        let queued = (waiting.iter()).any(|text| text.starts_with(&notice));
        assert!(queued, "{case}: {waiting:?}");
        (elsewhere.hand_over("scratch/one", former, successor)).unwrap();
        if released {
            (elsewhere.release("scratch/one", &child, None)).unwrap();
        }
        let told = outage.back().drain(fixture, sender, former);
        let status = if released { "ready" } else { "in_progress" };
        // Its own hand-over is not said to it.
        let moved = format!("ROLE scratch/one {successor} orchestrator, {status} (from {former})");
        assert!(!told.contains(&moved), "{case}: {told:?}");
        let heard = (told.iter()).any(|line| line.starts_with(&notice));
        assert_eq!(heard, said, "{case}: {told:?}");
    }
}

/// The head of a PULL of one ready task nobody staffs, come up since the last PULL the former
/// orchestrator heard, which counted none.
pub(super) const FREE: &str = "PULL ready_unstaffed=1 (was 0)";

/// What the lines waiting for the former orchestrator after its hand-over of `scratch/one` come
/// to, with `scratch/free` (nobody's) made during the outage: its NEW and the PULL of it (`head`),
/// which keeps its place; its own move is not said to it.
fn after_hand_over(head: &str) -> Vec<String> {
    vec![
        "NEW scratch/free ready — Jump departure".to_string(),
        head.to_string(),
        "ready_unstaffed p2 scratch/free — Jump departure".to_string(),
    ]
}

/// The former orchestrator's queued PULL as a dispatcher built before one orchestrator per task
/// writes it: the whole board's digest, and every entry of it.
fn as_an_older_dispatcher_wrote_it(outage: &Outage) {
    let Outage {
        fixture, former, ..
    } = outage;
    let whole = symphony_board::cli::pull::digest(&fixture.store, &[], None, fixture.now(), &[]);
    let whole = whole.unwrap();
    let lines = (whole.event_lines().into_iter())
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "));
    let text = lines.collect::<Vec<_>>().join("\n");
    assert!(text.contains("scratch/theirs"), "{text}");
    let raw = fixture.store.raw();
    let rows = raw
        .table("callback_event")
        .matching(json!({"handle":former}))
        .get()
        .unwrap();
    for row in rows
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["delivered"] != true && r["key"].as_str().unwrap().starts_with("pull-"))
    {
        raw.row("callback_event", json!([former, row["key"]]))
            .set(json!({"text":text,"pull_entries":whole.entries(),"pull_withheld":null}))
            .unwrap();
    }
}

#[test]
fn a_pull_waiting_out_an_outage_is_said_as_the_digest_is_at_delivery_whoever_wrote_it() {
    for older in [false, true] {
        let outage = Outage::on();
        let Outage {
            fixture,
            elsewhere,
            former,
            successor,
            sender,
        } = &outage;
        // The successor's ready work, which only an older dispatcher's PULL names.
        fixture.task("scratch/theirs");
        let theirs = fixture
            .store
            .take_role("scratch/theirs", successor, "orchestrator");
        theirs.unwrap();
        let child = outage.child(former);
        fixture.store.claim("scratch/one", &child).unwrap();
        outage.cut();
        // While it cannot be reached, work nobody orchestrates comes up: its PULL counts its own
        // running task beside it, as the last PULL it heard did (`executing` has not moved).
        let free = NewTask::new("scratch/free", "Jump departure", "owner");
        elsewhere.create(&free).unwrap();
        let waiting = outage.waiting();
        let counted = format!("{FREE}\nready_unstaffed p2 scratch/free — Jump departure");
        assert!(waiting.contains(&counted), "{waiting:?}");
        if older {
            as_an_older_dispatcher_wrote_it(&outage);
        }
        (elsewhere.hand_over("scratch/one", former, successor)).unwrap();
        let told = outage.back().drain(fixture, sender, former);
        // The running task is the successor's now: the count it heard moves.
        let head = "PULL executing=0 (was 1) ready_unstaffed=1 (was 0)";
        assert_eq!(told, after_hand_over(head), "older: {older}");
        assert_eq!(older_fields(fixture, former), older_record(fixture, former));
    }
}

#[test]
fn a_pull_waiting_out_an_outage_names_no_session_whose_work_a_hand_over_took_away() {
    let outage = Outage::on();
    let Outage {
        fixture,
        elsewhere,
        former,
        successor,
        sender,
    } = &outage;
    // The former's child reviews its task.
    let reviewer = outage.child(former);
    fixture.store.claim("scratch/one", sender).unwrap();
    (fixture.store.move_to("scratch/one", "review", sender)).unwrap();
    let reviews = fixture
        .store
        .take_role("scratch/one", &reviewer, "reviewer");
    reviews.unwrap();
    outage.cut();
    // Its presence goes unconfirmed, as a native callback's sweep overdue leaves it, while it
    // shows no sign: its liveness is unknown, and its work stays held.
    (fixture.store.raw().row("callback_state", reviewer.as_str()))
        .put(
            json!({"handle":reviewer, "address":"codex:abcdef01", "host":HOST,
            "confirmed_until":fixture.now() - 1.0, "lease":fixture.now() + 1_000_000.0}),
        )
        .unwrap();
    (fixture.store.raw().row("session", reviewer.as_str()))
        .set(json!({"last_seen":fixture.now() - 500.0}))
        .unwrap();
    assert_eq!(fixture.health(&reviewer), Health::Unknown);
    let free = NewTask::new("scratch/free", "Jump departure", "owner");
    elsewhere.create(&free).unwrap();
    let waiting = outage.waiting();
    let held = format!("work remains held: {reviewer}");
    let named = (waiting.iter()).any(|text| text.starts_with("PULL ") && text.contains(&held));
    assert!(named, "{waiting:?}");
    // The review is the successor's, and the reviewer shows a sign again.
    (elsewhere.hand_over("scratch/one", former, successor)).unwrap();
    elsewhere.touch(&reviewer).unwrap();
    assert_eq!(fixture.health(&reviewer), Health::Live);
    let told = outage.back().drain(fixture, sender, former);
    assert_eq!(told, after_hand_over(FREE));
}
