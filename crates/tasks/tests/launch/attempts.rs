//! Local terminals as execution attempts: a launch admits its attempt before hc is started and
//! records it running; a restore is a resume linked to the attempt before it; a close is a stop;
//! a refusal or a rollback leaves the attempt ended with why; cleanup and the idle sweep say a
//! terminal they closed is gone. A record from before attempts gets none made up, and another
//! provider's attempt is never read as a terminal.

use crate::common::refused;
use crate::{Hooked, World};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::time::Instant;
use symphony_board::NewLaunch;
use symphony_board::execution::{
    Act, Adapter, Admission, Attempt, Cancellation, Coordinator, Dispatch, Ending, Grant, Relation,
    Worked,
};
use symphony_board::launch::attempts::{CONFIGURATION, profile};
use symphony_board::launch::hc::Hc;
use symphony_board::launch::screen::ON_SCREEN;
use symphony_board::launch::{Ask, Started, restore};
use symphony_board::workflow::Trigger;
use symphony_plugin::execution::{
    Authorization, Call, Capabilities, ContentRef, Input, Observation, Operation, Outcome,
    Principal, PrincipalKind, Profile, Receipt, Request, Resource, ResourceId, RuntimeState,
    StopMode, Unknown,
};

/// A provider no test here lets a call reach.
struct Silent;

impl Adapter for Silent {
    fn execute(&self, _: &Call, _: &Authorization, _: Instant) -> Outcome {
        Outcome::Unknown(Unknown::ContactLost)
    }
}

/// The local provider on this world's board, as a reader of it builds it.
fn local(w: &World) -> Profile {
    profile(&w.hc().project(), w.store())
}

/// A grant to read this world's board, and to stand as a coordinator's worker.
fn reader(w: &World) -> Grant {
    let owner = Principal::new(PrincipalKind::Human, "local".into(), "owner".into());
    Grant::from_host(
        owner,
        local(w).scope,
        &[Act::Read, Act::Work, Act::Start],
        "test",
    )
}

/// Every attempt of `slug`, oldest first.
fn attempts(w: &World, slug: &str) -> Vec<Attempt> {
    Coordinator::new(w.store(), local(w), &Silent, reader(w))
        .unwrap()
        .history(&reader(w), slug)
        .unwrap()
}

/// The one attempt of sim/jump.
fn only(w: &World) -> Attempt {
    let [attempt] = <[_; 1]>::try_from(attempts(w, "sim/jump")).expect("one attempt");
    attempt
}

/// Attempt `id`'s facts as JSON, in order.
fn facts(w: &World, id: &str) -> Vec<Value> {
    Coordinator::new(w.store(), local(w), &Silent, reader(w))
        .unwrap()
        .facts(&reader(w), id)
        .unwrap()
        .iter()
        .map(|recorded| serde_json::to_value(&recorded.fact).unwrap())
        .collect()
}

fn kinds(facts: &[Value]) -> Vec<&str> {
    facts
        .iter()
        .map(|fact| fact["kind"].as_str().unwrap())
        .collect()
}

/// `facts` from the binding of `terminal`, the terminal hc answered the start with, on. The start
/// settles on that terminal by its own answer or, where that answer came after the call's bound
/// (a loaded machine's slow hc), by the reconcile it then owes, which finds the terminal: how long
/// hc took decides which, and never what follows.
fn after_its_start<'f>(facts: &'f [Value], terminal: &str) -> &'f [Value] {
    let evidence = format!("hc run answered terminal {terminal}");
    assert_eq!(
        kinds(&facts[..4]),
        ["admitted", "intent", "sent", "answer"],
        "{facts:?}"
    );
    let settled = if facts[3]["outcome"]["Unknown"] == "Deadline" {
        assert_eq!(kinds(&facts[4..6]), ["sent", "answer"], "{facts:?}");
        assert_eq!(facts[4]["reconcile"], true, "{facts:?}");
        assert_eq!(facts[5]["outcome"]["Found"]["evidence"], evidence);
        6
    } else {
        assert_eq!(facts[3]["outcome"]["Accepted"]["evidence"], evidence);
        4
    };
    &facts[settled..]
}

/// The address of the terminal bound to `attempt`.
fn terminal_of(attempt: &Attempt) -> &str {
    &attempt.state.resource.as_ref().expect("a terminal").id
}

/// A Claude executor of sim/jump, idle in its terminal.
fn launched(w: &World) -> Started {
    let started = w
        .try_launch(&Ask {
            kind: Some("claude".into()),
            ..w.ask("sim/jump")
        })
        .unwrap();
    w.edit_terminal(&started.row.pane, |terminal| {
        terminal.insert("status".into(), json!("idle"));
    });
    started
}

fn restored(w: &World, handle: &str) -> Result<Vec<Started>, symphony_board::Error> {
    restore(
        w.store(),
        &w.hc(),
        &w.machine(),
        "sim/jump",
        Some(handle),
        None,
    )
}

#[test]
fn a_launch_admits_its_attempt_before_hc_is_started_and_records_it_running() {
    let w = World::new();
    let before = RefCell::new(None);
    let hc = Hooked {
        instead: Box::new(|args| {
            if args[0] == "run" {
                *before.borrow_mut() = Some(only(&w));
            }
            None
        }),
        ..Hooked::new(&w)
    };
    let ask = Ask {
        kind: Some("claude".into()),
        ..w.ask("sim/jump")
    };
    let row = symphony_board::launch::launch(w.store(), &hc, &w.machine(), &ask)
        .unwrap()
        .row;
    // When hc was asked for the terminal, the attempt stood admitted, its start recorded as sent.
    let admitted = before.take().expect("hc run was called");
    assert_eq!(admitted.state.dispatch, Dispatch::Pending);
    let owed = admitted.state.pending.expect("a start owed");
    assert_eq!((owed.kind, owed.sent), (Operation::Start, true));
    assert_eq!(admitted.state.resource, None);

    let attempt = only(&w);
    assert_eq!(attempt.id, admitted.id);
    let identity = &attempt.identity;
    assert_eq!(identity.configuration, CONFIGURATION);
    assert_eq!(identity.task, "sim/jump");
    assert_eq!(identity.role, "executor");
    assert_eq!(identity.handle.as_deref(), Some(row.handle.as_str()));
    assert_eq!(identity.lane, format!("executor/{}", row.handle));
    assert_eq!(
        (identity.predecessor.as_deref(), identity.relation),
        (None, None)
    );
    assert_eq!(identity.initiator.subject, w.orch);
    assert_eq!(identity.initiator.kind, PrincipalKind::Agent);
    assert_eq!(identity.capacity, None);
    // The commit its checkout is at, and the task as the board held it.
    assert_eq!(
        identity.input.repository_revision,
        crate::git(&w.wt.join("sim-jump"), &["rev-parse", "HEAD"])
    );
    assert_eq!(identity.input.manifest.reference, "task:sim/jump");
    // Running in the terminal hc made, in the conversation the launch chose, and current.
    assert_eq!(attempt.state.dispatch, Dispatch::Running);
    assert_eq!(terminal_of(&attempt), row.pane);
    assert_eq!(
        attempt.state.conversation.as_deref(),
        Some(row.claude_session.as_str())
    );
    assert_eq!(attempt.state.pending, None);
    assert!(attempt.current);
    assert_eq!(
        w.store().live_attempts("sim/jump").unwrap(),
        std::slice::from_ref(&attempt.id)
    );
    let facts = facts(&w, &attempt.id);
    assert_eq!(
        kinds(&facts),
        [
            "admitted", "intent", "sent", "answer", "bound", "answer", "bound"
        ],
        "{facts:?}"
    );
    assert_eq!(
        facts[3]["outcome"]["Accepted"]["evidence"],
        format!("hc run answered terminal {}", row.pane)
    );
    // No look beyond the launch's own: hc's receipt of the first message is the observation.
    assert_eq!(w.calls_of(&["get", &row.pane]).len(), 1);
    // The launch record on the task is written beside it.
    let [record] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
    assert_eq!(record, row);
}

#[test]
fn a_codex_launch_binds_no_conversation_it_does_not_know_and_a_reviewer_has_its_own_lane() {
    let w = World::new();
    let executor = w.launch("sim/jump").row;
    let executors = only(&w);
    assert_eq!(executors.state.dispatch, Dispatch::Running);
    assert_eq!(executors.state.conversation, None);
    assert_eq!(kinds(&facts(&w, &executors.id)).last(), Some(&"answer"));
    w.store().claim("sim/jump", &executor.handle).unwrap();
    w.store()
        .move_to("sim/jump", "review", &executor.handle)
        .unwrap();
    let reviewer = w.review("sim/jump").row;
    let all = attempts(&w, "sim/jump");
    assert_eq!(all.len(), 2);
    assert_eq!(all[1].identity.role, "reviewer");
    assert_eq!(
        all[1].identity.lane,
        format!("reviewer/{}", reviewer.handle)
    );
    assert_eq!(terminal_of(&all[1]), reviewer.pane);
    // Each holds its own lane: both are current.
    assert!(
        all.iter()
            .all(|attempt| attempt.current && attempt.state.live())
    );
}

#[test]
fn an_hc_that_refuses_the_run_is_a_rejected_start_with_its_reason() {
    let mut w = World::new();
    w.set("FAKE_HC_FAIL", "run");
    let problem = refused(w.try_launch(&w.ask("sim/jump")));
    let attempt = only(&w);
    assert_eq!(attempt.state.dispatch, Dispatch::Terminal);
    assert_eq!(attempt.state.ending, Some(Ending::Rejected));
    assert_eq!(attempt.state.resource, None);
    assert_eq!(attempt.state.cancellation, Cancellation::None);
    let facts = facts(&w, &attempt.id);
    assert_eq!(
        kinds(&facts),
        ["admitted", "intent", "sent", "answer", "ended"]
    );
    assert_eq!(facts[3]["outcome"]["Rejected"]["Provider"], problem);
    assert!(w.store().live_attempts("sim/jump").unwrap().is_empty());
    assert_eq!(w.calls_of(&["run"]).len(), 1);
}

#[test]
fn a_first_message_refusal_cancels_the_attempt_by_closing_its_terminal_with_the_reason() {
    let mut w = World::new();
    w.set("FAKE_HC_DEAF", "1");
    let problem = refused(w.try_launch(&w.ask("sim/jump")));
    let attempt = only(&w);
    let terminal = terminal_of(&attempt).to_string();
    assert_eq!(
        problem,
        format!(
            "the agent in terminal {terminal} did not start working on its first message (its \
             status when it went in: idle)"
        )
    );
    assert_eq!(attempt.state.dispatch, Dispatch::Terminal);
    assert_eq!(attempt.state.ending, Some(Ending::Cancelled));
    assert_eq!(attempt.state.cancellation, Cancellation::Confirmed);
    assert_eq!(
        (&attempt.state.pending, &attempt.state.cancel),
        (&None, &None)
    );
    let facts = facts(&w, &attempt.id);
    let ending = after_its_start(&facts, &terminal);
    assert_eq!(
        kinds(ending),
        [
            "bound", "asked", "intent", "sent", "answer", "answer", "ended"
        ],
        "{facts:?}"
    );
    // The cancel was sent as the terminate that closed the terminal, the refusal its evidence.
    assert_eq!(ending[1]["cancel"], true);
    assert_eq!(ending[2]["operation"], "Stop");
    assert_eq!(
        ending[4]["outcome"]["Accepted"]["evidence"],
        format!("terminal {terminal} closed: {problem}")
    );
    assert_eq!(ending[5]["outcome"]["Observed"]["state"], "Destroyed");
    // The terminal was closed once, by that stop, and nothing else of the launch is left.
    assert!(!w.has_terminal(&terminal));
    assert_eq!(w.calls_of(&["kill", &terminal]).len(), 1);
    assert_eq!(w.calls_of(&["rm", &terminal]).len(), 1);
    assert_eq!(w.store().task_sessions("sim/jump").unwrap(), vec![]);
    assert!(w.store().live_attempts("sim/jump").unwrap().is_empty());
}

#[test]
fn a_rollback_whose_terminal_will_not_close_leaves_the_attempt_live_and_says_so() {
    let mut w = World::new();
    w.set("FAKE_HC_DEAF", "1");
    let hc = Hooked {
        instead: Box::new(|args| {
            (args[0] == "kill").then(|| {
                Err(symphony_board::launch::LaunchError(
                    "hc kill: refused".into(),
                ))
            })
        }),
        ..Hooked::new(&w)
    };
    let launched = symphony_board::launch::launch(w.store(), &hc, &w.machine(), &w.ask("sim/jump"));
    let problem = refused(launched);
    let attempt = only(&w);
    let terminal = terminal_of(&attempt).to_string();
    assert!(
        problem.ends_with(&format!(
            "; launch cleanup incomplete: terminal {terminal}: hc kill: refused"
        )),
        "{problem}"
    );
    // Asked to cease, and not known to have: it reads live, as its terminal is.
    assert!(w.has_terminal(&terminal));
    assert!(attempt.state.live());
    assert_eq!(attempt.state.cancellation, Cancellation::Unconfirmed);
    let facts = facts(&w, &attempt.id);
    let refusal = facts.last().unwrap();
    assert_eq!(
        refusal["outcome"]["Rejected"]["Provider"],
        "hc kill: refused"
    );
    // Its worktree stays, the terminal may still use it.
    assert!(w.wt.join("sim-jump").exists());
}

#[test]
fn a_close_is_a_stop_asked_by_the_closer_and_ends_the_attempt_stopped() {
    let w = World::new();
    let row = launched(&w).row;
    w.close(&row.handle).unwrap();
    let attempt = only(&w);
    assert_eq!(attempt.state.dispatch, Dispatch::Terminal);
    assert_eq!(attempt.state.ending, Some(Ending::Stopped));
    assert_eq!(attempt.state.cancellation, Cancellation::None);
    assert_eq!(attempt.state.stop, None);
    let facts = facts(&w, &attempt.id);
    let tail = &facts[facts.len() - 6..];
    assert_eq!(
        kinds(tail),
        ["asked", "intent", "sent", "answer", "answer", "ended"],
        "{facts:?}"
    );
    assert_eq!(tail[0]["cancel"], false);
    assert_eq!(tail[0]["mode"], "Terminate");
    assert_eq!(tail[0]["initiator"]["subject"], w.orch);
    assert_eq!(
        tail[3]["outcome"]["Accepted"]["evidence"],
        format!("terminal {} closed by {}", row.pane, w.orch)
    );
    assert!(!w.has_terminal(&row.pane));
    assert_eq!(w.calls_of(&["kill", &row.pane]).len(), 1);
    assert!(w.store().live_attempts("sim/jump").unwrap().is_empty());
}

#[test]
fn a_close_that_keeps_its_terminal_leaves_the_attempt_running_with_the_stop_refused() {
    let w = World::new();
    let row = launched(&w).row;
    w.shows(json!({"agent": row.pane}));
    assert_eq!(
        refused(w.close(&row.handle)),
        format!("terminal {} stays: {ON_SCREEN}", row.pane)
    );
    assert!(w.has_terminal(&row.pane));
    assert_eq!(only(&w).state.dispatch, Dispatch::Running);
    // Out of sight at the first look and on screen at the last: the stop was sent and refused.
    let looks = std::cell::Cell::new(0);
    w.shows(json!({}));
    let hc = Hooked {
        after: Box::new(|args| {
            if args[0] == "get" {
                looks.set(looks.get() + 1);
                if looks.get() == 2 {
                    w.shows(json!({"agent": row.pane}));
                }
            }
        }),
        ..Hooked::new(&w)
    };
    assert_eq!(
        refused(w.close_through(&hc, &row.handle)),
        format!("terminal {} stays: {ON_SCREEN}", row.pane)
    );
    let attempt = only(&w);
    assert_eq!(attempt.state.dispatch, Dispatch::Running);
    assert_eq!(
        (&attempt.state.stop, &attempt.state.pending),
        (&None, &None)
    );
    let facts = facts(&w, &attempt.id);
    assert_eq!(
        facts.last().unwrap()["outcome"]["Rejected"]["Provider"],
        ON_SCREEN
    );
    w.untouched();
    // Out of sight again, the same attempt is stopped.
    w.shows(json!({}));
    w.close(&row.handle).unwrap();
    assert_eq!(only(&w).state.ending, Some(Ending::Stopped));
}

#[test]
fn a_restore_is_a_resume_linked_to_the_attempt_before_it() {
    let w = World::new();
    let row = launched(&w).row;
    w.close(&row.handle).unwrap();
    let [again] = <[_; 1]>::try_from(restored(&w, &row.handle).unwrap()).unwrap();
    let [first, second] = <[_; 2]>::try_from(attempts(&w, "sim/jump")).unwrap();
    assert_eq!(
        second.identity.predecessor.as_deref(),
        Some(first.id.as_str())
    );
    assert_eq!(second.identity.relation, Some(Relation::Resume));
    assert_eq!(
        second.identity.runtime_epoch,
        first.identity.runtime_epoch + 1
    );
    assert_eq!(second.identity.lane, first.identity.lane);
    assert_eq!(second.identity.input, first.identity.input);
    assert_eq!(second.identity.initiator.kind, PrincipalKind::Service);
    assert_eq!(first.state.successor.as_deref(), Some(second.id.as_str()));
    assert_eq!(first.state.ending, Some(Ending::Stopped));
    assert!(!first.current && second.current);
    // A new terminal, never the old one's address, in the recorded conversation.
    assert_eq!(second.state.dispatch, Dispatch::Running);
    assert_eq!(terminal_of(&second), again.row.pane);
    assert_ne!(terminal_of(&second), terminal_of(&first));
    assert_eq!(
        second.state.conversation.as_deref(),
        Some(row.claude_session.as_str())
    );
    assert_eq!(facts(&w, &second.id)[1]["operation"], "Resume");
    assert_eq!(kinds(&facts(&w, &first.id)).last(), Some(&"followed"));
    assert_eq!(w.store().live_attempts("sim/jump").unwrap(), [second.id]);
}

#[test]
fn a_restore_observes_gone_the_attempt_of_a_terminal_closed_outside_the_board() {
    let w = World::new();
    let row = launched(&w).row;
    w.hc().close(&row.pane).unwrap();
    restored(&w, &row.handle).unwrap();
    let [first, second] = <[_; 2]>::try_from(attempts(&w, "sim/jump")).unwrap();
    assert_eq!(first.state.ending, Some(Ending::Stopped));
    let facts = facts(&w, &first.id);
    let seen = &facts[facts.len() - 3];
    assert_eq!(
        seen["outcome"]["Observed"]["state"], "Destroyed",
        "{facts:?}"
    );
    assert_eq!(second.identity.relation, Some(Relation::Resume));
    assert_eq!(second.state.dispatch, Dispatch::Running);
}

#[test]
fn a_record_from_before_attempts_is_restored_as_a_plain_start_and_none_is_made_up() {
    let w = World::new();
    let handle = w.f.session("worker");
    let pane = w.add_terminal(true, Some(&handle), json!({}));
    w.store()
        .record_launch(&NewLaunch {
            claude_session: "an-old-conversation".into(),
            worktree: w.wt.join("sim-jump").display().to_string(),
            pane: pane.clone(),
            model: "sonnet".into(),
            ..NewLaunch::new("sim/jump", &handle, "executor")
        })
        .unwrap();
    // Its close ends no attempt: none stands for the past launch.
    w.close(&handle).unwrap();
    assert!(!w.has_terminal(&pane));
    assert_eq!(attempts(&w, "sim/jump"), vec![]);
    let [again] = <[_; 1]>::try_from(restored(&w, &handle).unwrap()).unwrap();
    let attempt = only(&w);
    assert_eq!(
        (
            attempt.identity.predecessor.as_deref(),
            attempt.identity.relation
        ),
        (None, None)
    );
    assert_eq!(attempt.identity.runtime_epoch, 0);
    assert_eq!(attempt.state.dispatch, Dispatch::Running);
    assert_eq!(terminal_of(&attempt), again.row.pane);
    assert_eq!(
        attempt.state.conversation.as_deref(),
        Some("an-old-conversation")
    );
    assert_eq!(facts(&w, &attempt.id)[1]["operation"], "Start");
    // The record is the one it had, reopened.
    let [record] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
    assert_eq!(record.claude_session, "an-old-conversation");
}

#[test]
fn an_unconfirmed_restore_cancels_its_resume_and_the_next_restore_follows_that_one() {
    let mut w = World::new();
    let row = launched(&w).row;
    w.close(&row.handle).unwrap();
    w.set("FAKE_HC_DEAF", "1");
    let problem = refused(restored(&w, &row.handle));
    let [first, failed] = <[_; 2]>::try_from(attempts(&w, "sim/jump")).unwrap();
    assert_eq!(
        failed.identity.predecessor.as_deref(),
        Some(first.id.as_str())
    );
    assert_eq!(failed.state.ending, Some(Ending::Cancelled));
    assert_eq!(failed.state.cancellation, Cancellation::Confirmed);
    let killed = terminal_of(&failed).to_string();
    assert!(!w.has_terminal(&killed));
    assert_eq!(w.calls_of(&["kill", &killed]).len(), 1);
    let facts = facts(&w, &failed.id);
    assert_eq!(
        facts
            .iter()
            .filter_map(|fact| fact["outcome"]["Accepted"]["evidence"].as_str())
            .next_back(),
        Some(format!("terminal {killed} closed: {problem}").as_str()),
        "{facts:?}"
    );
    w.env.vars.remove("FAKE_HC_DEAF");
    restored(&w, &row.handle).unwrap();
    let all = attempts(&w, "sim/jump");
    assert_eq!(all.len(), 3);
    assert_eq!(
        all[2].identity.predecessor.as_deref(),
        Some(failed.id.as_str())
    );
    assert_eq!(all[2].identity.runtime_epoch, 2);
    assert_eq!(all[2].state.dispatch, Dispatch::Running);
}

#[test]
fn a_restore_whose_run_hc_refuses_rejects_its_resume_and_the_next_starts_plainly() {
    let mut w = World::new();
    let row = launched(&w).row;
    w.close(&row.handle).unwrap();
    w.set("FAKE_HC_FAIL", "run");
    refused(restored(&w, &row.handle));
    let all = attempts(&w, "sim/jump");
    assert_eq!(all[1].state.ending, Some(Ending::Rejected));
    assert_eq!(all[1].state.resource, None);
    assert_eq!(
        w.calls_of(&["kill"]).len(),
        1,
        "only the close killed anything"
    );
    // No terminal was ever its: nothing to resume from, so the next restore is a start.
    w.env.vars.remove("FAKE_HC_FAIL");
    restored(&w, &row.handle).unwrap();
    let all = attempts(&w, "sim/jump");
    assert_eq!(all.len(), 3);
    assert_eq!(
        (
            all[2].identity.predecessor.as_deref(),
            all[2].identity.relation
        ),
        (None, None)
    );
    assert_eq!(all[2].state.dispatch, Dispatch::Running);
}

#[test]
fn cleanup_and_finish_say_the_terminal_they_closed_is_gone() {
    let w = World::new();
    let (_, terminal) = w.worker("sim/jump", Some("done"));
    assert_eq!(only(&w).state.dispatch, Dispatch::Running);
    assert_eq!(w.finish("sim/jump").len(), 1);
    assert!(!w.has_terminal(&terminal));
    let attempt = only(&w);
    assert_eq!(attempt.state.ending, Some(Ending::Stopped));
    let facts = facts(&w, &attempt.id);
    let seen = &facts[facts.len() - 2];
    assert_eq!(
        seen["outcome"]["Observed"]["state"], "Destroyed",
        "{facts:?}"
    );
    assert_eq!(seen["worker"]["subject"], "cleanup");

    // A terminal that went outside the board: cleanup records it gone, and its attempt with it.
    w.task("sim/dock", None);
    let row = w.launch("sim/dock").row;
    w.hc().close(&row.pane).unwrap();
    w.clean(false, &[]);
    let [dock] = <[_; 1]>::try_from(attempts(&w, "sim/dock")).unwrap();
    assert_eq!(dock.state.ending, Some(Ending::Stopped));
}

#[test]
fn a_cleanup_that_keeps_a_terminal_leaves_its_attempt_running() {
    let w = World::new();
    let (_, terminal) = w.worker("sim/jump", Some("done"));
    w.run_under(w.pid_of(&terminal), &["just verify"]);
    assert_eq!(w.finish("sim/jump"), Vec::<String>::new());
    assert_eq!(only(&w).state.dispatch, Dispatch::Running);
    // A dry run closes nothing and says nothing of the attempt.
    w.run_under(w.pid_of(&terminal), &[]);
    w.clean(true, &[]);
    assert!(w.has_terminal(&terminal));
    assert_eq!(only(&w).state.dispatch, Dispatch::Running);
}

#[test]
fn the_idle_sweep_says_an_ended_terminal_it_removed_is_gone() {
    let w = World::new();
    let row = launched(&w).row;
    w.store().claim("sim/jump", &row.handle).unwrap();
    w.edit_terminal(&row.pane, |terminal| {
        terminal.insert("state".into(), json!("finished"));
        terminal.insert("status".into(), Value::Null);
    });
    let swept = w.sweep_through(&w.hc());
    assert_eq!(swept.closed.len(), 1, "{swept:?}");
    assert!(!w.has_terminal(&row.pane));
    assert_eq!(only(&w).state.ending, Some(Ending::Stopped));
}

/// Another provider: its one resource, `res-1`, is no terminal.
struct Remote;

impl Adapter for Remote {
    fn execute(&self, call: &Call, _: &Authorization, _: Instant) -> Outcome {
        let resource = Resource::new(
            call.identity.attempt.clone(),
            call.identity.configuration.clone(),
            ResourceId::new("res-1".into()),
            "g1".into(),
        );
        match &call.request {
            Request::Start { .. } => Outcome::Accepted(Receipt::new(
                call.identity.clone(),
                resource,
                "accepted".into(),
            )),
            Request::Observe { resource, .. } => {
                Outcome::Observed(Observation::new(resource.clone(), RuntimeState::Running))
            }
            _ => Outcome::Unsupported(call.request.operation()),
        }
    }
}

/// An attempt of `handle` on sim/jump under the provider `remote`, running in `res-1`.
fn remote_attempt(w: &World, handle: &str, scope_board: Option<&str>) -> (Profile, Attempt) {
    let mut capabilities = Capabilities::default();
    capabilities.operations = vec![Operation::Start, Operation::Observe, Operation::Stop];
    capabilities.stop_modes = vec![StopMode::Terminate];
    let mut scope = local(w).scope;
    if let Some(board) = scope_board {
        scope.board = board.to_string();
    }
    let remote = Profile::new(scope.clone(), "remote".into(), capabilities);
    let owner = Principal::new(PrincipalKind::Human, "local".into(), "owner".into());
    let grant = Grant::from_host(owner, scope, &[Act::Read, Act::Work, Act::Start], "test");
    let coordinator = Coordinator::new(w.store(), remote.clone(), &Remote, grant.clone()).unwrap();
    let input = Input::new(
        "r1".into(),
        "digest".into(),
        "commit".into(),
        ContentRef::new("manifest".into(), "manifest-digest".into()),
    );
    let asked = Admission::new("sim/jump", "remote", "executor", input).handle(handle);
    let attempt = coordinator.admit(&grant, asked).unwrap();
    assert!(matches!(
        coordinator.step(&attempt.id).unwrap(),
        Worked::Answered { .. }
    ));
    let attempt = coordinator.attempt(&grant, &attempt.id).unwrap();
    assert_eq!(terminal_of(&attempt), "res-1");
    (remote, attempt)
}

#[test]
fn a_remote_attempts_resource_is_never_given_to_hc_as_a_terminal() {
    let w = World::new();
    let row = launched(&w).row;
    let (remote, before) = remote_attempt(&w, &row.handle, None);
    w.close(&row.handle).unwrap();
    restored(&w, &row.handle).unwrap();
    let again = w.store().task_sessions("sim/jump").unwrap()[0].clone();
    w.edit_terminal(&again.pane, |terminal| {
        terminal.insert("status".into(), json!("idle"));
    });
    w.close(&row.handle).unwrap();
    w.clean(false, &[]);
    let named: Vec<Vec<String>> = w
        .calls()
        .into_iter()
        .filter(|call| call.iter().any(|word| word.contains("res-1")))
        .collect();
    assert_eq!(named, Vec::<Vec<String>>::new());
    // The local attempts ended and followed each other; the remote one is left untouched.
    let all = attempts(&w, "sim/jump");
    let local: Vec<&Attempt> = all
        .iter()
        .filter(|attempt| attempt.identity.configuration == CONFIGURATION)
        .collect();
    assert_eq!(local.len(), 2);
    assert_eq!(
        local[1].identity.predecessor.as_deref(),
        Some(local[0].id.as_str())
    );
    assert!(local.iter().all(|attempt| !attempt.state.live()));
    let grant = Grant::from_host(
        Principal::new(PrincipalKind::Human, "local".into(), "owner".into()),
        remote.scope.clone(),
        &[Act::Read, Act::Work],
        "test",
    );
    let after = Coordinator::new(w.store(), remote, &Remote, grant.clone())
        .unwrap()
        .attempt(&grant, &before.id)
        .unwrap();
    assert_eq!(after, before);
}

#[test]
fn a_task_holding_another_boards_attempt_still_launches_closes_and_restores() {
    let w = World::new();
    let row = launched(&w).row;
    remote_attempt(&w, &row.handle, Some("another-board"));
    // Its history cannot be read as this board's, and the local operations go on: the close
    // stops the live attempt, which it reads by itself, and the restore starts plainly, since
    // what ended there cannot be listed.
    let live = || w.store().live_attempts("sim/jump").unwrap().len();
    assert_eq!(live(), 2);
    w.close(&row.handle).unwrap();
    assert!(!w.has_terminal(&row.pane));
    assert_eq!(live(), 1);
    let [again] = <[_; 1]>::try_from(restored(&w, &row.handle).unwrap()).unwrap();
    assert!(w.has_terminal(&again.row.pane));
    assert_eq!(live(), 2);
    // Still running, it is the one a second close stops.
    w.edit_terminal(&again.row.pane, |terminal| {
        terminal.insert("status".into(), json!("idle"));
    });
    w.close(&row.handle).unwrap();
    assert_eq!(live(), 1);
}

/// hc as a launch that died with its start in flight left it: the call made, no answer heard.
struct Lost;

impl Adapter for Lost {
    fn execute(&self, _: &Call, _: &Authorization, _: Instant) -> Outcome {
        Outcome::Unknown(Unknown::ContactLost)
    }
}

#[test]
fn a_restore_settles_a_start_that_never_answered_as_cancelled_and_starts_plainly() {
    let w = World::new();
    let handle = w.f.session("worker");
    w.store()
        .record_launch(&NewLaunch {
            claude_session: "its-conversation".into(),
            worktree: w.wt.join("sim-jump").display().to_string(),
            pane: "t-never".into(),
            model: "sonnet".into(),
            ..NewLaunch::new("sim/jump", &handle, "executor")
        })
        .unwrap();
    let grant = reader(&w);
    let lost = Coordinator::new(w.store(), local(&w), &Lost, grant.clone()).unwrap();
    let input = Input::new(
        "r1".into(),
        "digest".into(),
        "commit".into(),
        ContentRef::new("task:sim/jump".into(), "brief".into()),
    );
    let lane = format!("executor/{handle}");
    let asked = Admission::new("sim/jump", &lane, "executor", input).handle(&handle);
    let unsettled = lost.admit(&grant, asked).unwrap();
    lost.step(&unsettled.id).unwrap();
    assert_eq!(only(&w).state.dispatch, Dispatch::Unknown);

    // No terminal runs as the handle: the start is reconciled absent, cancelled, and never sent
    // again; with no terminal ever its own, the restore is a start in the same lane.
    let [again] = <[_; 1]>::try_from(restored(&w, &handle).unwrap()).unwrap();
    let [first, second] = <[_; 2]>::try_from(attempts(&w, "sim/jump")).unwrap();
    assert_eq!(first.id, unsettled.id);
    assert_eq!(first.state.ending, Some(Ending::Cancelled));
    assert_eq!(first.state.cancellation, Cancellation::Confirmed);
    assert_eq!(first.state.resource, None);
    let facts = facts(&w, &first.id);
    let absent = &facts[facts.len() - 2];
    assert_eq!(
        absent["outcome"]["Absent"]["evidence"],
        format!("no terminal of this board runs as {handle}"),
        "{facts:?}"
    );
    assert_eq!(second.identity.lane, lane);
    assert_eq!(
        (
            second.identity.predecessor.as_deref(),
            second.identity.relation
        ),
        (None, None)
    );
    assert_eq!(second.state.dispatch, Dispatch::Running);
    assert_eq!(terminal_of(&second), again.row.pane);
    assert_eq!(w.calls_of(&["run"]).len(), 1);
}

#[test]
fn a_restore_is_refused_while_its_last_attempt_is_held_and_starts_no_terminal() {
    let w = World::new();
    let handle = w.f.session("worker");
    w.store()
        .record_launch(&NewLaunch {
            claude_session: "its-conversation".into(),
            worktree: w.wt.join("sim-jump").display().to_string(),
            pane: "t-never".into(),
            model: "sonnet".into(),
            ..NewLaunch::new("sim/jump", &handle, "executor")
        })
        .unwrap();
    let grant = reader(&w);
    let lost = Coordinator::new(w.store(), local(&w), &Lost, grant.clone()).unwrap();
    let input = Input::new(
        "r1".into(),
        "digest".into(),
        "commit".into(),
        ContentRef::new("task:sim/jump".into(), "brief".into()),
    );
    let lane = format!("executor/{handle}");
    let asked = Admission::new("sim/jump", &lane, "executor", input).handle(&handle);
    let unsettled = lost.admit(&grant, asked).unwrap();
    // Answer after answer settles nothing, until recovery gives it to a person.
    let mut steps = 0;
    while only(&w).state.dispatch != Dispatch::Held {
        lost.step(&unsettled.id).unwrap();
        steps += 1;
        assert!(steps < 20, "never held: {:?}", only(&w).state);
    }
    let why = only(&w).state.held.expect("why it is held");
    assert_eq!(
        refused(restored(&w, &handle)),
        format!(
            "{handle}'s execution attempt {} is not known to have ended ({why}), so the restore \
             starts no terminal",
            unsettled.id
        )
    );
    assert_eq!(w.calls_of(&["run"]), Vec::<Vec<String>>::new());
    let held = only(&w);
    assert_eq!(held.state.dispatch, Dispatch::Held);
    assert_eq!(held.state.cancellation, Cancellation::Requested);
}

#[test]
fn a_close_of_a_terminal_that_is_gone_observes_its_attempt_gone() {
    let w = World::new();
    let row = launched(&w).row;
    w.hc().close(&row.pane).unwrap();
    let before = w.calls().len();
    let closure = w.close(&row.handle).unwrap();
    assert_eq!(closure.why, symphony_board::launch::GONE);
    let attempt = only(&w);
    assert_eq!(attempt.state.ending, Some(Ending::Stopped));
    assert_eq!(attempt.state.cancellation, Cancellation::None);
    // No stop was asked of a terminal that is not there, and nothing was killed.
    let facts = facts(&w, &attempt.id);
    assert!(!kinds(&facts).contains(&"asked"), "{facts:?}");
    let after: Vec<Vec<String>> = w.calls().split_off(before);
    assert!(
        after
            .iter()
            .all(|call| call[0] != "kill" && call[0] != "rm"),
        "{after:?}"
    );
}

#[test]
fn a_launch_in_a_folder_that_is_no_checkout_pins_no_commit() {
    let w = World::new();
    let plain = w.dir().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    w.try_launch(&Ask {
        worktree: Some(plain),
        ..w.ask("sim/jump")
    })
    .unwrap();
    let attempt = only(&w);
    assert_eq!(attempt.identity.input.repository_revision, "unversioned");
    assert_eq!(attempt.state.dispatch, Dispatch::Running);
}

/// An hc whose `run` answers only after the step that sent the start has lost its lease, as an
/// answer slower than the lease does: the lease of the task's live attempt is expired once hc has
/// answered each run.
fn outlasting_its_lease(w: &World) -> Hooked<'_> {
    Hooked {
        after: Box::new(|args| {
            if args[0] != "run" {
                return;
            }
            for id in w.store().live_attempts("sim/jump").unwrap() {
                let row = w.store().raw().row("execution_attempt", id);
                let mut lease = row.field("lease").unwrap();
                assert!(lease.is_object(), "the step holds a lease: {lease}");
                lease["expires"] = json!(0);
                row.set(json!({ "lease": lease })).unwrap();
            }
        }),
        ..Hooked::new(w)
    }
}

/// The outcomes of `facts`' answers, by name, with whether each came after its lease.
fn answers(facts: &[Value]) -> Vec<(String, bool)> {
    facts
        .iter()
        .filter(|fact| fact["kind"] == "answer")
        .map(|fact| {
            let outcome = fact["outcome"].as_object().expect("an outcome");
            let name = outcome.keys().next().expect("its name").clone();
            (name, fact["stale"] == true)
        })
        .collect()
}

#[test]
fn a_terminal_hc_answered_after_the_steps_lease_is_found_and_the_launch_goes_on() {
    let w = World::new();
    let hc = outlasting_its_lease(&w);
    let ask = Ask {
        kind: Some("claude".into()),
        ..w.ask("sim/jump")
    };
    let row = symphony_board::launch::launch(w.store(), &hc, &w.machine(), &ask)
        .unwrap()
        .row;
    // The terminal hc made is the attempt's, and nothing closed it or asked for another.
    let attempt = only(&w);
    assert_eq!(attempt.state.dispatch, Dispatch::Running);
    assert_eq!(terminal_of(&attempt), row.pane);
    assert_eq!(attempt.state.pending, None);
    assert_eq!(w.calls_of(&["run"]).len(), 1);
    assert_eq!(w.calls_of(&["close"]), Vec::<Vec<String>>::new());
    // The receipt that came after the lease changed nothing; the reconcile found the terminal.
    let facts = facts(&w, &attempt.id);
    let answers = answers(&facts);
    assert_eq!(
        answers[..2],
        [("Accepted".to_string(), true), ("Found".to_string(), false)],
        "{facts:?}"
    );
    let found = facts
        .iter()
        .find(|fact| fact["outcome"]["Found"].is_object())
        .expect("a found answer");
    assert_eq!(
        found["outcome"]["Found"]["evidence"],
        format!("hc run answered terminal {}", row.pane)
    );
    let [record] = <[_; 1]>::try_from(w.store().task_sessions("sim/jump").unwrap()).unwrap();
    assert_eq!(record, row);
}

#[test]
fn a_restores_terminal_answered_after_the_steps_lease_is_found_as_its_resume() {
    let w = World::new();
    let row = launched(&w).row;
    w.close(&row.handle).unwrap();
    let hc = outlasting_its_lease(&w);
    let restored = restore(
        w.store(),
        &hc,
        &w.machine(),
        "sim/jump",
        Some(&row.handle),
        None,
    );
    let [again] = <[_; 1]>::try_from(restored.unwrap()).unwrap();
    let [first, second] = <[_; 2]>::try_from(attempts(&w, "sim/jump")).unwrap();
    assert_eq!(second.identity.relation, Some(Relation::Resume));
    assert_eq!(second.state.dispatch, Dispatch::Running);
    assert_eq!(terminal_of(&second), again.row.pane);
    assert_ne!(terminal_of(&second), terminal_of(&first));
    assert_eq!(w.calls_of(&["run"]).len(), 2);
    assert_eq!(
        w.calls_of(&["close", &again.row.pane]),
        Vec::<Vec<String>>::new()
    );
    let facts = facts(&w, &second.id);
    assert_eq!(
        answers(&facts)[..2],
        [("Accepted".to_string(), true), ("Found".to_string(), false)],
        "{facts:?}"
    );
}

#[test]
fn an_attempt_started_from_a_linked_worktree_is_read_under_the_main_checkouts_scope() {
    let w = World::new();
    std::fs::write(w.repo.join("symphony.toml"), "[project]\nid = \"lamp\"\n").unwrap();
    crate::git(&w.repo, &["add", "symphony.toml"]);
    crate::git(&w.repo, &["commit", "-q", "-m", "the project's file"]);
    let executor = w.launch("sim/jump").row;
    w.store().claim("sim/jump", &executor.handle).unwrap();
    w.store()
        .move_to("sim/jump", "review", &executor.handle)
        .unwrap();
    // The reviewer is launched by a command run in the executor's linked worktree.
    let linked = symphony_board::cli::Env {
        cwd: w.wt.join("sim-jump"),
        vars: w.env.vars.clone(),
    };
    let from_worktree = symphony_board::launch::hc::Binary::of(&linked);
    let ask = Ask {
        role: "reviewer".into(),
        kind: Some("reviewer".into()),
        ..w.ask("sim/jump")
    };
    symphony_board::launch::launch(w.store(), &from_worktree, &w.machine(), &ask).unwrap();

    // The project's id is its file's, not a folder's name, from either checkout.
    assert_eq!(w.hc().project(), "lamp");
    assert_eq!(from_worktree.project(), "lamp");
    let main = symphony_board::execution::board_scope("lamp", w.store());
    assert_eq!(main.project, "lamp");
    assert_eq!(main.board, w.store().database().database());
    assert_eq!(profile(&from_worktree.project(), w.store()).scope, main);
    // One history under the main checkout's scope holds both, each admitted under it.
    let [first, second] = <[_; 2]>::try_from(attempts(&w, "sim/jump")).unwrap();
    assert_eq!(
        (first.identity.role.as_str(), second.identity.role.as_str()),
        ("executor", "reviewer")
    );
    for attempt in [&first, &second] {
        assert_eq!(
            (&attempt.identity.project, &attempt.identity.board),
            (&main.project, &main.board)
        );
    }
}

/// The execution tests' workflow (`tests/store/execution/attempts.toml`) active on `w`'s board,
/// sim/jump placed at build/available, where any ending of an attempt waits on a person: the
/// local terminals proposed to it as every attempt is.
fn under_workflow(w: &World) {
    const STAY: &str =
        r#"{ manual = true, slot = "retain", release_role = false, release_reservation = false }"#;
    let text = include_str!("../store/execution/attempts.toml")
        .replace("@FAILURE@", STAY)
        .replace(
            r#"legacy = "ready", satisfies_dependencies = false, completion = "none" }"#,
            &format!(
                r#"legacy = "ready", satisfies_dependencies = false, completion = "none", recovery = {{ release = {STAY}, lost_holder = {STAY}, attempt_failure = {STAY}, cancellation_acknowledged = {STAY} }} }}"#
            ),
        );
    let (definition, mappings) = symphony_board::workflow::written(&text).unwrap();
    let definition = definition.unwrap();
    let revision = (definition.revision(symphony_api::workflow::Source {
        commit: "1".repeat(40),
        project_blob: "2".repeat(40),
    }))
    .unwrap();
    let custom = symphony_board::workflow::Custom {
        definition,
        revision,
        mappings,
        authorities: Vec::new(),
        project: None,
    };
    let host = symphony_board::workflow::Host::local();
    w.store().activate_workflow(&custom, &host).unwrap();
    let placed = w.store().workflow_position("sim/jump").unwrap().unwrap();
    assert_eq!(
        (placed.phase.as_str(), placed.state.as_str()),
        ("build", "available")
    );
}

/// Where the workflow holds sim/jump: its pair, its revision and the trigger it waits on a person
/// for, if it does; and its last transition's key.
fn held(w: &World) -> (String, u64, Option<Trigger>, Option<String>) {
    let at = w.store().workflow_position("sim/jump").unwrap().unwrap();
    let last = w.store().workflow_transitions("sim/jump").unwrap().pop();
    (
        format!("{}/{}", at.phase, at.state),
        at.revision,
        at.manual.map(|m| m.trigger),
        last.and_then(|t| t.key),
    )
}

#[test]
fn under_a_custom_workflow_a_refused_run_or_a_rollback_is_proposed_from_where_the_launch_admitted_it()
 {
    // A run hc refuses is a failure of the attempt, answered by the position's action.
    let mut w = World::new();
    under_workflow(&w);
    let (_, placed, _, _) = held(&w);
    w.set("FAKE_HC_FAIL", "run");
    refused(w.try_launch(&w.ask("sim/jump")));
    let attempt = only(&w);
    assert_eq!(attempt.state.ending, Some(Ending::Rejected));
    assert_eq!(
        held(&w),
        (
            "build/available".into(),
            placed + 1,
            Some(Trigger::AttemptFailure),
            Some(format!("attempt/{}", attempt.id))
        )
    );

    // A launch rolled back cancels its attempt, a cancel that took.
    let mut w = World::new();
    under_workflow(&w);
    w.set("FAKE_HC_DEAF", "1");
    refused(w.try_launch(&w.ask("sim/jump")));
    let attempt = only(&w);
    assert_eq!(attempt.state.cancellation, Cancellation::Confirmed);
    assert_eq!(
        held(&w),
        (
            "build/available".into(),
            placed + 1,
            Some(Trigger::CancellationAcknowledged),
            Some(format!("attempt/{}", attempt.id))
        )
    );
}
