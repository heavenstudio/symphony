//! hc as an execution provider: each launch, restore and close of a local terminal is recorded as
//! an execution attempt ([`crate::execution`]), through the coordinator's public API alone, so a
//! local run and a remote one share one history
//! ([docs/engineering/EXECUTION_ATTEMPTS.md](../../../../docs/engineering/EXECUTION_ATTEMPTS.md#local-terminals)).
//!
//! The provider is the configuration [`CONFIGURATION`], built here: no project setting names it
//! and nothing remote is read. Its resource is the terminal's own address, as hc gave it. A launch
//! admits its attempt before hc is started; its start is `hc run` alone, and the first message,
//! which may wait a minute, stays the launcher's step. A terminal hc answered is the attempt's
//! however long the answer took: one that came after the call's bound or the step's lease settles
//! the start by the reconcile it then owes, answered with that terminal. A launch or a restore
//! that fails after that cancels its attempt, the cancel sent as the terminate that closes the
//! terminal, its reason in the receipt. A restore admits a resume of the attempt its handle ran
//! last, from the recorded conversation; a record no attempt was made for (a launch from before
//! attempts) gets a plain start, and none is made up for the past. A close asks a terminate, whose
//! kill is the close's own last look.
//!
//! The kit's [`Adapter`] is `Send + Sync`, and the launcher's hc and its machine are neither, so
//! the adapter here is a channel ([`Bridge`]): the coordinator's step runs on a scoped thread, and
//! the thread that launches answers its one call with the hc it was handed ([`Local::step`]).
//! What the launcher already read of a terminal answers an observation without another hc call: a
//! first message hc confirmed is a running terminal, and a terminal the close just killed is gone.
//!
//! A lane is the role and the handle: who may execute or review a task is the store's rule, and
//! a restore follows its own handle's attempt on the task it restores. An attempt of another
//! configuration is never read as a terminal: only this configuration's resources are addresses.

use super::hc::Hc;
use super::{LaunchError, git};
use crate::execution::{
    Act, Adapter, Admission, Attempt, Continuation, Coordinator, Grant, Pacing, Refusal, Worked,
    board_scope,
};
use crate::{Error, Store, Task};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};
use symphony_plugin::execution::{
    Authorization, Call, Capabilities, ContentRef, ConversationId, Input, Observation, Operation,
    Outcome, Principal, Profile, Receipt, Rejection, Request, Resource, ResourceId, ResumeMode,
    RuntimeState, StopMode, Unknown,
};

/// The provider configuration of this machine's hc terminals.
pub const CONFIGURATION: &str = "local-hc";
/// The policy the local grants are made under: the board's own rules for its command line.
const POLICY: &str = "local-hc/1";
/// A terminal is never reused: each has the one generation.
const GENERATION: &str = "1";
/// How many steps an attempt is given to end: a reconcile, a stop and an observation, with room.
const ROUNDS: usize = 5;

/// What hc can do for an attempt: start a terminal, say whether a start that never answered left
/// one, observe it, terminate it and resume a conversation in a new one. It pauses nothing and
/// gives no view; its focus and its screen are local.
pub fn capabilities() -> Capabilities {
    let mut capabilities = Capabilities::default();
    capabilities.operations = vec![
        Operation::Start,
        Operation::Reconcile,
        Operation::Observe,
        Operation::Stop,
        Operation::Resume,
    ];
    capabilities.stop_modes = vec![StopMode::Terminate];
    capabilities.resume_modes = vec![ResumeMode::NewResource];
    capabilities.authoritative_absence = true;
    capabilities.local_focus = true;
    capabilities.local_screen = true;
    capabilities
}

/// The local provider's profile on `store`'s board, the board of the project whose id is
/// `project` ([`board_scope`]): what a reader of this board's local attempts builds its
/// [`Coordinator`] from. A launch names its project by [`Hc::project`].
pub fn profile(project: &str, store: &Store) -> Profile {
    Profile::new(
        board_scope(project, store),
        CONFIGURATION.to_string(),
        capabilities(),
    )
}

/// The lane of `handle` in `role`.
fn lane(role: &str, handle: &str) -> String {
    format!("{role}/{handle}")
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A refusal as the launcher says it.
fn refused(refusal: Refusal) -> Error {
    match refusal {
        Refusal::Board(error) => error,
        other => Error::Refused(other.to_string()),
    }
}

/// The terminal at `address` as `call`'s attempt names it.
fn resource(call: &Call, address: &str) -> Resource {
    Resource::new(
        call.identity.attempt.clone(),
        call.identity.configuration.clone(),
        ResourceId::new(address.to_string()),
        GENERATION.to_string(),
    )
}

fn receipt(call: &Call, resource: Resource, evidence: &str) -> Outcome {
    Outcome::Accepted(Receipt::new(
        call.identity.clone(),
        resource,
        evidence.to_string(),
    ))
}

/// The adapter no call reaches: what admits, reads and asks, and sends nothing.
struct Unreached;

impl Adapter for Unreached {
    fn execute(&self, _: &Call, _: &Authorization, _: Instant) -> Outcome {
        Outcome::Unknown(Unknown::Other(
            "the local provider answers only its own step".into(),
        ))
    }
}

/// The adapter a step's worker thread calls: it hands the call to the thread that launches and
/// waits for its answer. A side that is gone is lost contact, which the coordinator reconciles.
struct Bridge {
    calls: Mutex<Option<Sender<Call>>>,
    answers: Mutex<Receiver<Outcome>>,
}

impl Bridge {
    /// No more calls: the thread that answers stops waiting for one.
    fn hang_up(&self) {
        if let Ok(mut calls) = self.calls.lock() {
            calls.take();
        }
    }
}

impl Adapter for Bridge {
    fn execute(&self, call: &Call, _: &Authorization, _: Instant) -> Outcome {
        let lost = || Outcome::Unknown(Unknown::ContactLost);
        let sent = match self.calls.lock() {
            Ok(calls) => calls
                .as_ref()
                .is_some_and(|calls| calls.send(call.clone()).is_ok()),
            Err(_) => false,
        };
        if !sent {
            return lost();
        }
        match self.answers.lock() {
            Ok(answers) => answers.recv().unwrap_or_else(|_| lost()),
            Err(_) => lost(),
        }
    }
}

/// Hangs the bridge up when the worker's step is over, however it ended.
struct HangUp<'a>(&'a Bridge);

impl Drop for HangUp<'_> {
    fn drop(&mut self) {
        self.0.hang_up();
    }
}

/// How ending an attempt went: what closing its terminal answered, when the close was made, and
/// what the board said of the attempt's own writes.
pub(super) struct Ended {
    pub(super) closed: Option<Result<(), String>>,
    pub(super) board: Result<(), Error>,
}

/// This board's local attempts, for one principal: the session or the board service that acts.
pub(super) struct Local<'a> {
    store: &'a Store,
    profile: Profile,
    grant: Grant,
}

impl<'a> Local<'a> {
    /// The local provider on `store`'s board, as `hc` names it, acting for `principal`.
    pub(super) fn new(store: &'a Store, hc: &dyn Hc, principal: Principal) -> Local<'a> {
        let profile = profile(&hc.project(), store);
        let acts = [
            Act::Start,
            Act::Cancel,
            Act::Stop,
            Act::Resume,
            Act::Read,
            Act::Work,
        ];
        Local {
            store,
            grant: Grant::from_host(principal, profile.scope.clone(), &acts, POLICY),
            profile,
        }
    }

    fn coordinator<'b>(&'b self, adapter: &'b dyn Adapter) -> Result<Coordinator<'b>, Error> {
        let pacing = Pacing {
            call: Duration::from_secs(30),
            lease: Duration::from_secs(60),
            ..Pacing::default()
        };
        Coordinator::new(
            self.store,
            self.profile.clone(),
            adapter,
            self.grant.clone(),
        )
        .and_then(|coordinator| coordinator.with_pacing(pacing))
        .map_err(refused)
    }

    fn read(&self, id: &str) -> Result<Attempt, Error> {
        self.coordinator(&Unreached)?
            .attempt(&self.grant, id)
            .map_err(refused)
    }

    /// Whether `attempt` is one of this provider's, run as `handle`.
    fn ours(attempt: &Attempt, handle: &str) -> bool {
        attempt.identity.configuration == CONFIGURATION
            && attempt.identity.handle.as_deref() == Some(handle)
    }

    /// The attempts of `slug` that may still run and that this board can read: one a host
    /// admitted under another name for this board is left out, never refused for.
    fn live(&self, slug: &str) -> Result<Vec<Attempt>, Error> {
        let coordinator = self.coordinator(&Unreached)?;
        let mut live = Vec::new();
        for id in self.store.live_attempts(slug)? {
            match coordinator.attempt(&self.grant, &id) {
                Ok(attempt) => live.push(attempt),
                Err(Refusal::Scope) => {}
                Err(refusal) => return Err(refused(refusal)),
            }
        }
        Ok(live)
    }

    /// The attempt `handle` ran last on `slug`, if it ran one: its live one, else the last in the
    /// task's history. A history that holds another board's attempts cannot be read, and then
    /// reads as none of ours having ended there.
    pub(super) fn latest(&self, slug: &str, handle: &str) -> Result<Option<Attempt>, Error> {
        let mut live = self.live(slug)?;
        live.retain(|attempt| Local::ours(attempt, handle));
        if let Some(attempt) = live.pop() {
            return Ok(Some(attempt));
        }
        let mut all = match self.coordinator(&Unreached)?.history(&self.grant, slug) {
            Ok(all) => all,
            Err(Refusal::Scope) => Vec::new(),
            Err(refusal) => return Err(refused(refusal)),
        };
        all.retain(|attempt| Local::ours(attempt, handle));
        Ok(all.pop())
    }

    /// The live attempt of `handle`, on one of `slugs`, whose terminal is `address`.
    pub(super) fn bound_to(
        &self,
        slugs: &[&str],
        handle: &str,
        address: &str,
    ) -> Result<Option<Attempt>, Error> {
        let mut seen: Vec<&str> = Vec::new();
        for slug in slugs {
            if seen.contains(slug) {
                continue;
            }
            seen.push(slug);
            let bound = self.live(slug)?.into_iter().find(|attempt| {
                Local::ours(attempt, handle)
                    && attempt
                        .state
                        .resource
                        .as_ref()
                        .is_some_and(|bound| bound.id == address)
            });
            if bound.is_some() {
                return Ok(bound);
            }
        }
        Ok(None)
    }

    /// The task's input as a local run takes it: the task as the board holds it now, the commit
    /// `checkout` is at and the first message `text`.
    fn input(&self, task: &Task, checkout: &str, text: &str) -> Input {
        let revision = format!("{}", task.updated);
        let brief = sha256(task.body.as_bytes());
        let commit = git(Path::new(checkout), &["rev-parse", "HEAD"])
            .ok()
            .filter(|commit| !commit.is_empty())
            .unwrap_or_else(|| "unversioned".to_string());
        let pinned = serde_json::json!([task.slug, revision, brief, commit, text]);
        Input::new(
            revision,
            sha256(pinned.to_string().as_bytes()),
            commit,
            ContentRef::new(format!("task:{}", task.slug), brief),
        )
    }

    /// Admit the attempt of `handle` running `task` as `role` in `checkout`, told `text`: its
    /// start owed, nothing sent.
    pub(super) fn admit(
        &self,
        task: &Task,
        role: &str,
        handle: &str,
        checkout: &str,
        text: &str,
    ) -> Result<Attempt, Error> {
        let input = self.input(task, checkout, text);
        let admission = Admission::new(&task.slug, &lane(role, handle), role, input).handle(handle);
        self.coordinator(&Unreached)?
            .admit(&self.grant, admission)
            .map_err(refused)
    }

    /// Admit the attempt that restores `handle` on `task` in `checkout`, told `text`, where no
    /// terminal runs as it: a resume of the attempt it ran last on `task`, from `conversation`,
    /// once that one is recorded ended; a plain start where it ran none there, or one that never
    /// had a terminal. Refused while its last attempt cannot be said to have ended.
    pub(super) fn follow(
        &self,
        task: &Task,
        role: &str,
        handle: &str,
        checkout: &str,
        text: &str,
        conversation: &str,
    ) -> Result<Attempt, Error> {
        let Some(before) = self.latest(&task.slug, handle)? else {
            return self.admit(task, role, handle, checkout, text);
        };
        let gone = format!("no terminal of this board runs as {handle}");
        self.end(&before.id, false, &gone, &mut || Ok(())).board?;
        let before = self.read(&before.id)?;
        if before.state.live() {
            let why = before.state.held.clone().unwrap_or_else(|| gone.clone());
            return Err(Error::Refused(format!(
                "{handle}'s execution attempt {} is not known to have ended ({why}), so the \
                 restore starts no terminal",
                before.id
            )));
        }
        if before.state.resource.is_none() {
            return self.admit(task, role, handle, checkout, text);
        }
        let continuation = Continuation {
            checkpoint: None,
            conversation: Some(conversation.to_string()),
            mode: ResumeMode::NewResource,
        };
        self.coordinator(&Unreached)?
            .resume(&self.grant, &before.id, continuation, None)
            .map_err(refused)
    }

    /// One step of attempt `id`'s worker, its call (if it makes one) answered by `serve` on this
    /// thread: the coordinator records the operation as sent, then `serve` runs, then its answer
    /// is recorded.
    pub(super) fn step(
        &self,
        id: &str,
        serve: &mut dyn FnMut(&Call) -> Outcome,
    ) -> Result<Worked, Error> {
        let (calls, asked) = mpsc::channel();
        let (answer, answers) = mpsc::channel();
        let bridge = Bridge {
            calls: Mutex::new(Some(calls)),
            answers: Mutex::new(answers),
        };
        let coordinator = self.coordinator(&bridge)?;
        let worked = std::thread::scope(|scope| {
            // Dropped with this closure, so a `serve` that panics leaves no worker waiting.
            let answer = answer;
            let worker = scope.spawn(|| {
                let _over = HangUp(&bridge);
                coordinator.step(id)
            });
            for call in &asked {
                if answer.send(serve(&call)).is_err() {
                    break;
                }
            }
            worker.join()
        });
        match worked {
            Ok(worked) => worked.map_err(refused),
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    /// Send attempt `id`'s start (or resume), which `run` makes: the address of the terminal hc
    /// started. An hc that refused is a rejected start, and the attempt has ended; a terminal the
    /// attempt did not take is the caller's to close.
    pub(super) fn start(
        &self,
        id: &str,
        run: &mut dyn FnMut() -> Result<String, LaunchError>,
    ) -> Result<String, Error> {
        let mut made = None;
        let worked = self.step(id, &mut |call| match &call.request {
            Request::Start { .. } | Request::Resume { .. } => {
                let ran = run();
                let outcome = match &ran {
                    Ok(address) => receipt(
                        call,
                        resource(call, address),
                        &format!("hc run answered terminal {address}"),
                    ),
                    Err(problem) => Outcome::Rejected(Rejection::Provider(problem.0.clone())),
                };
                made = Some(ran);
                outcome
            }
            _ => Outcome::Unknown(Unknown::Other("it owes no start".into())),
        })?;
        let accepted = matches!(
            &worked,
            Worked::Answered {
                outcome: Outcome::Accepted(_),
                ..
            }
        );
        match made {
            Some(Err(problem)) => Err(problem.into()),
            Some(Ok(address)) if accepted || self.found(id, &address)? => Ok(address),
            Some(Ok(address)) => Err(Error::Refused(format!(
                "terminal {address} started, but its execution attempt {id} did not take it"
            ))),
            None => Err(Error::Refused(format!(
                "execution attempt {id} sent no start, so no terminal is started"
            ))),
        }
    }

    /// Settle attempt `id`'s start from the terminal `address` hc answered, where the step that
    /// sent it recorded no receipt (the answer came after the call's bound, or after its lease):
    /// the reconcile an unsettled start owes is answered with the terminal found. Whether the
    /// attempt is bound to it.
    fn found(&self, id: &str, address: &str) -> Result<bool, Error> {
        let bound = |attempt: &Attempt| {
            attempt.state.pending.is_none()
                && attempt
                    .state
                    .resource
                    .as_ref()
                    .is_some_and(|bound| bound.id == address)
        };
        for _ in 0..ROUNDS {
            let attempt = self.read(id)?;
            if bound(&attempt) {
                return Ok(true);
            }
            if !attempt.state.live() {
                return Ok(false);
            }
            let worked = self.step(id, &mut |call| match &call.request {
                Request::Reconcile { .. } | Request::ReconcileResume { .. } => {
                    Outcome::Found(Receipt::new(
                        call.identity.clone(),
                        resource(call, address),
                        format!("hc run answered terminal {address}"),
                    ))
                }
                _ => Outcome::Unknown(Unknown::Other("its start is unsettled".into())),
            })?;
            if !matches!(worked, Worked::Answered { .. } | Worked::Fenced { .. }) {
                break;
            }
        }
        Ok(bound(&self.read(id)?))
    }

    /// Record attempt `id` running, as hc's receipt of its first message said, in `conversation`
    /// when it is known.
    pub(super) fn running(&self, id: &str, conversation: Option<&str>) -> Result<(), Error> {
        let conversation = conversation.filter(|conversation| !conversation.is_empty());
        self.step(id, &mut |call| match &call.request {
            Request::Observe { resource, .. } => {
                let mut seen = Observation::new(resource.clone(), RuntimeState::Running);
                seen.conversation = conversation.map(|c| ConversationId::new(c.to_string()));
                Outcome::Observed(seen)
            }
            _ => Outcome::Unknown(Unknown::Other("it has no terminal to observe".into())),
        })?;
        Ok(())
    }

    /// Ask attempt `id`'s terminal to terminate, then end it ([`Local::end`]): what a close does.
    /// An attempt with nothing to stop (ended, or never started) is ended as it stands.
    pub(super) fn stop(
        &self,
        id: &str,
        why: &str,
        close: &mut dyn FnMut() -> Result<(), String>,
    ) -> Ended {
        let asked = self.coordinator(&Unreached).and_then(|coordinator| {
            match coordinator.stop(&self.grant, id, StopMode::Terminate) {
                Ok(_) | Err(Refusal::Invalid(_)) => Ok(()),
                Err(refusal) => Err(refused(refusal)),
            }
        });
        match asked {
            Ok(()) => self.end(id, false, why, close),
            Err(problem) => Ended {
                closed: None,
                board: Err(problem),
            },
        }
    }

    /// End attempt `id`, whose terminal `close` closes (called at most once; a terminal that is
    /// gone already closes at once): cancelled when `cancel`, or when its start never settled.
    /// The close is the answer to the stop the attempt owes, `why` its receipt's evidence; a start
    /// that never settled is reconciled as absent once the terminal is closed; and the closed
    /// terminal is observed gone. A close that fails leaves the attempt live, as its terminal is.
    pub(super) fn end(
        &self,
        id: &str,
        cancel: bool,
        why: &str,
        close: &mut dyn FnMut() -> Result<(), String>,
    ) -> Ended {
        let mut closed: Option<Result<(), String>> = None;
        let board = (|| {
            let attempt = self.read(id)?;
            if !attempt.state.live() {
                return Ok(());
            }
            let starting = attempt.state.pending.as_ref().is_some_and(|pending| {
                matches!(pending.kind, Operation::Start | Operation::Resume)
            });
            if cancel || starting {
                self.coordinator(&Unreached)?
                    .cancel(&self.grant, id)
                    .map_err(refused)?;
            }
            for _ in 0..ROUNDS {
                if matches!(closed, Some(Err(_))) || !self.read(id)?.state.live() {
                    break;
                }
                let worked = self.step(id, &mut |call| {
                    let result = closed.get_or_insert_with(&mut *close).clone();
                    match (&call.request, result) {
                        (Request::Stop { resource, .. }, Ok(())) => {
                            receipt(call, resource.clone(), why)
                        }
                        (Request::Stop { .. }, Err(problem)) => {
                            Outcome::Rejected(Rejection::Provider(problem))
                        }
                        (Request::Reconcile { .. } | Request::ReconcileResume { .. }, Ok(())) => {
                            Outcome::Absent {
                                evidence: why.to_string(),
                            }
                        }
                        (Request::Observe { resource, .. }, Ok(())) => Outcome::Observed(
                            Observation::new(resource.clone(), RuntimeState::Destroyed),
                        ),
                        (_, Err(problem)) => Outcome::Unknown(Unknown::Other(problem)),
                        (_, Ok(())) => Outcome::Rejected(Rejection::Provider(why.to_string())),
                    }
                })?;
                if !matches!(worked, Worked::Answered { .. }) {
                    break;
                }
            }
            Ok(())
        })();
        Ended { closed, board }
    }

    /// End attempt `id` of a launch or a restore that failed for `why`, closing `terminal` (none
    /// when hc started none): whether the terminal closed, and what the board refused of the
    /// attempt. The terminal is closed whatever the board says.
    pub(super) fn abandon(
        &self,
        id: &str,
        hc: &dyn Hc,
        terminal: &str,
        why: &str,
    ) -> (Result<(), String>, Result<(), Error>) {
        let mut close = || {
            if terminal.is_empty() {
                return Ok(());
            }
            hc.close(terminal).map_err(|problem| problem.0)
        };
        let evidence = if terminal.is_empty() {
            format!("hc started no terminal: {why}")
        } else {
            format!("terminal {terminal} closed: {why}")
        };
        let ended = self.end(id, true, &evidence, &mut close);
        let closed = ended.closed.unwrap_or_else(close);
        (closed, ended.board)
    }
}

/// The terminal `address`, which ran `handle`, is closed: the board killed it, removed it once its
/// program had ended, or found it gone (`why`). Its execution attempt, if one still reads live, is
/// observed gone. What cleanup and the idle sweep say of a terminal they closed; a close on an
/// orchestrator's word asks a stop first ([`Local::stop`]).
pub(super) fn closed(
    store: &Store,
    hc: &dyn Hc,
    handle: &str,
    address: &str,
    why: &str,
) -> Result<(), Error> {
    let local = Local::new(store, hc, crate::workflow::board_service("cleanup"));
    let records = store.sessions_of(handle)?;
    let slugs: Vec<&str> = records.iter().map(|record| record.slug.as_str()).collect();
    if let Some(attempt) = local.bound_to(&slugs, handle, address)? {
        let evidence = format!("terminal {address}: {why}");
        local
            .end(&attempt.id, false, &evidence, &mut || Ok(()))
            .board?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hc_advertises_what_it_does_and_no_more() {
        let offered = capabilities();
        let stop = |mode| Request::Stop {
            resource: Resource::new(
                symphony_plugin::execution::AttemptRef::new(
                    symphony_plugin::execution::Scope::new("p".into(), "b".into()),
                    "t".into(),
                    symphony_plugin::execution::AttemptId::new([0; 16]),
                ),
                CONFIGURATION.into(),
                ResourceId::new("t1".into()),
                GENERATION.into(),
            ),
            mode,
        };
        assert!(offered.supports(&stop(StopMode::Terminate)));
        assert!(!offered.supports(&stop(StopMode::Pause)));
        assert!(!offered.operations.contains(&Operation::Cancel));
        assert!(!offered.operations.contains(&Operation::ExternalLink));
        assert!(offered.authoritative_absence && offered.local_focus && offered.local_screen);
        assert!(!offered.external_view);
    }

    #[test]
    fn a_bridge_with_no_one_answering_is_lost_contact() {
        let (calls, asked) = mpsc::channel::<Call>();
        let (answer, answers) = mpsc::channel::<Outcome>();
        let bridge = Bridge {
            calls: Mutex::new(Some(calls)),
            answers: Mutex::new(answers),
        };
        let attempt = symphony_plugin::execution::AttemptRef::new(
            symphony_plugin::execution::Scope::new("p".into(), "b".into()),
            "t".into(),
            symphony_plugin::execution::AttemptId::new([0; 16]),
        );
        let identity = symphony_plugin::execution::OperationIdentity::new(
            attempt.clone(),
            CONFIGURATION.into(),
            "digest".into(),
            "key".into(),
            Operation::Reconcile,
            1,
        );
        let principal = crate::workflow::board_service("test");
        let call = Call::new(
            identity,
            principal.clone(),
            Request::Reconcile { resource: None },
        );
        let authorization =
            Authorization::from_host(principal, attempt, Operation::Reconcile, POLICY.into());
        let lost = Outcome::Unknown(Unknown::ContactLost);
        // The answering side went away after it took the call.
        drop(answer);
        assert_eq!(
            bridge.execute(&call, &authorization, Instant::now()),
            lost.clone()
        );
        assert_eq!(asked.try_iter().count(), 1);
        // Hung up, or the receiving side gone: nothing is sent.
        bridge.hang_up();
        assert_eq!(bridge.execute(&call, &authorization, Instant::now()), lost);
        assert_eq!(asked.try_iter().count(), 0);
    }
}
