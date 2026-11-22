//! A task's execution attempts through the server's routes
//! ([`docs/EXECUTION_ATTEMPTS.md`](../../../docs/EXECUTION_ATTEMPTS.md#on-a-tasks-page)): their
//! history ([`symphony_api::TASK_ATTEMPTS_PATH`]), a cancel, a stop or a resume of one
//! ([`symphony_api::ATTEMPT_CONTROL_ROUTE`]) and a view of its resource
//! ([`symphony_api::ATTEMPT_VIEW_ROUTE`]).
//!
//! Who asks is the one the server authenticated, never a field of the request: on this machine's
//! server the local owner ([`symphony_board::workflow::local_owner`], as a workflow edge is
//! taken), and under hosted admission the principal and policy revision the host verified, which
//! reads alone here (its controls are the host's dispatch). Each grant is on the one task the
//! route names, under the board scope of the project the server serves
//! ([`symphony_board::execution::board_scope`]), so an attempt of another task or project is
//! refused however its id came. A control names the revision of the attempt the page read and is
//! refused as stale once it moved.
//!
//! A cancel or a stop is intent the coordinator records for the worker that sends the attempt's
//! operations, wherever it runs: it needs no provider here. A local terminal's attempt
//! (`local-hc`, the board's launcher) takes none: its launcher's own commands alone step it, so an
//! intent recorded here would wait unsent; and a hosted read leaves out its terminal's address
//! and conversation, this machine's own. A resume admits another attempt under
//! its provider's own profile and capacity, and a view is that provider's I/O now, so both go only
//! through a provider this server holds ([`crate::execution`]). A view's address is answered to
//! the one who asked, `no-store`, and kept nowhere: not in the board, a log, a cache or a thread.

use crate::execution::{Provider, Providers, Served};
use crate::live::Hub;
use axum::http::StatusCode;
use serde_json::Value;
use std::time::Instant;
use symphony_api::{AttemptControl, AttemptControls, AttemptLink, AttemptViewAsk, TaskAttempts};
use symphony_board::Store;
use symphony_board::execution::{
    Act, Adapter, Attempt, Continuation, Coordinator, ExternalLink, Grant, Operation, Outcome,
    Principal, PrincipalKind, Profile, Reader, Refusal, ResumeMode, Scope, board_scope,
    resume_mode, resume_word, stop_mode, stop_word,
};
use symphony_plugin::execution::{Authorization, Call, Unknown};

/// The policy the server's own grants are made under.
pub(crate) const POLICY: &str = "symphony-server/1";

/// A refused request: its status and why.
pub(crate) type Failed = (StatusCode, String);

/// Who asks, as the server authenticated them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Asker {
    /// The local owner, at this machine's board.
    Local,
    /// A visitor hosted admission verified: its principal and the policy that admitted it.
    Hosted {
        principal: Principal,
        policy: String,
    },
}

impl Asker {
    /// The asker of a request hosted admission admitted for `actor` under `policy`.
    pub(crate) fn hosted(actor: &crate::hosted::Actor, policy: &str) -> Asker {
        Asker::Hosted {
            principal: Principal::new(
                PrincipalKind::Human,
                actor.issuer.clone(),
                actor.subject.clone(),
            ),
            policy: policy.to_string(),
        }
    }

    /// Its grant of `acts` on `task` alone, in `scope`.
    fn grant(&self, scope: &Scope, task: &str, acts: &[Act]) -> Grant {
        let (principal, policy) = match self {
            Asker::Local => (symphony_board::workflow::local_owner(), POLICY.to_string()),
            Asker::Hosted { principal, policy } => (principal.clone(), policy.clone()),
        };
        Grant::from_host(principal, scope.clone(), acts, policy).on_tasks(&[task])
    }
}

/// The board scope of the project `hub` serves, on `store`: the project's id as the board's own
/// commands name it ([`symphony_board::project::find`], as the launcher and `tasks api` do: its
/// file's id, else the machine's registry's, else its main checkout's folder), so an attempt a
/// launch or a host admitted for this project is this project's here. The served project's own
/// id where its folder is in no repository, whose board holds no attempt.
fn scope_of(hub: &Hub, store: &Store) -> Scope {
    let id = symphony_board::project::find(&hub.root, hub.machine.as_deref()).map_or_else(
        |_| symphony::project::Project::of(&hub.root).id.clone(),
        |project| project.id,
    );
    board_scope(&id, store)
}

/// A refusal of the coordinator as an answer.
fn refused(refusal: Refusal) -> Failed {
    let status = match &refusal {
        Refusal::Unauthorized(_) => StatusCode::FORBIDDEN,
        Refusal::Scope | Refusal::Unknown(_) => StatusCode::NOT_FOUND,
        Refusal::Unavailable(_) => StatusCode::BAD_GATEWAY,
        Refusal::Board(error) => return crate::api::unavailable(&error.to_string(), ""),
        // Busy, full, a conflict, a state that does not hold, and any refusal a newer board adds:
        // what was asked does not hold for the attempt now.
        _ => StatusCode::CONFLICT,
    };
    (status, refusal.to_string())
}

/// `work` on the board the hub reads, given `slug`'s task on it: a board that failed it is
/// dropped, to be opened again by the next read, as the task pages' reads drop it.
fn on_board<T>(
    hub: &Hub,
    slug: &str,
    work: impl FnOnce(&Store) -> Result<T, Failed>,
) -> Result<T, Failed> {
    if slug.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "name the task's slug".into()));
    }
    let store = hub.reader().map_err(|e| crate::api::unavailable(&e, ""))?;
    let done = store
        .get(slug)
        .map_err(crate::api::task_failure)
        .and_then(|_| work(&store));
    hub.read_on(&store, done)
}

/// What `attempt` may be asked here, by the local owner: its controls as its state, its pinned
/// capabilities and the providers this server holds allow. The coordinator decides each again.
pub(crate) fn controls(attempt: &Attempt, providers: &Providers) -> AttemptControls {
    if local(attempt) {
        return AttemptControls {
            unserved: Some(LOCAL.into()),
            ..AttemptControls::default()
        };
    }
    let (identity, state) = (&attempt.identity, &attempt.state);
    let pinned = &identity.capabilities;
    let provider = providers.of_configuration(&identity.configuration);
    let live = state.live() && state.successor.is_none();
    let cancel = live && state.cancellation == symphony_board::execution::Cancellation::None;
    let stop = if live
        && state.dispatch != symphony_board::execution::Dispatch::Held
        && state.resource.is_some()
        && state.stop.is_none()
        && pinned.operations.contains(&Operation::Stop)
    {
        pinned
            .stop_modes
            .iter()
            .map(|m| stop_word(*m).into())
            .collect()
    } else {
        Vec::new()
    };
    // A resume on a new resource asks least of it; each mode is offered while it may take it, so
    // a reuse goes once its resource's cleanup has begun.
    let continues = state.conversation.is_some() || state.checkpoint.is_some();
    let resumable = continues && attempt.resumable(ResumeMode::NewResource).is_ok();
    let resume = match provider {
        Some(provider) if resumable => {
            let offered = provider.capabilities();
            if offered.operations.contains(&Operation::Resume) {
                offered
                    .resume_modes
                    .iter()
                    .filter(|m| attempt.resumable(**m).is_ok())
                    .map(|m| resume_word(*m).into())
                    .collect()
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    };
    let viewable = state.resource.is_some() && pinned.external_view;
    AttemptControls {
        cancel,
        stop,
        resume,
        view: viewable && provider.is_some(),
        unserved: (provider.is_none() && (resumable || viewable)).then(|| {
            format!(
                "No resume or view here: its provider {} is not one this server holds",
                identity.configuration
            )
        }),
    }
}

/// Why a local terminal's attempt takes no control here.
const LOCAL: &str =
    "No control here: a local terminal, stepped by its launcher's own commands (close, restore)";

/// Whether `attempt` is a terminal of this machine's launcher
/// ([`symphony_board::launch::attempts::CONFIGURATION`]): stepped by the launcher's own commands
/// alone, with no worker between them, so a control recorded here would wait unsent, and its
/// resource and conversation are this machine's.
fn local(attempt: &Attempt) -> bool {
    attempt.identity.configuration == symphony_board::launch::attempts::CONFIGURATION
}

/// `slug`'s attempts, read for `asker`: the local owner is told what each may be asked here; a
/// hosted visitor, whose controls are the host's, none.
pub(crate) fn read(hub: &Hub, slug: &str, asker: &Asker) -> Result<TaskAttempts, Failed> {
    on_board(hub, slug, |store| history(hub, store, slug, asker))
}

fn history(hub: &Hub, store: &Store, slug: &str, asker: &Asker) -> Result<TaskAttempts, Failed> {
    let scope = scope_of(hub, store);
    let grant = asker.grant(&scope, slug, &[Act::Read]);
    let attempts = Reader::new(store, scope)
        .history(&grant, slug)
        .map_err(refused)?;
    Ok(TaskAttempts {
        attempts: attempts
            .iter()
            .map(|attempt| {
                let mut wired = attempt.wire();
                if *asker == Asker::Local {
                    wired.controls = controls(attempt, &hub.providers);
                } else if local(attempt) {
                    // This machine's terminal address and conversation, which a hosted page
                    // leaves out as it leaves out the sessions.
                    (wired.resource, wired.generation, wired.conversation) = (None, None, None);
                }
                wired
            })
            .collect(),
    })
}

/// The provider no control here reaches: a cancel and a stop are recorded, never sent, here.
struct Unreached;

impl Adapter for Unreached {
    fn execute(&self, _: &Call, _: &Authorization, _: Instant) -> Outcome {
        Outcome::Unknown(Unknown::Other(
            "the server records controls; the provider's worker sends them".into(),
        ))
    }
}

/// The grant the server makes its coordinators with: it only makes them, and steps no attempt.
fn worker(scope: &Scope) -> Grant {
    Grant::from_host(
        symphony_board::workflow::board_service("symphony-server"),
        scope.clone(),
        &[Act::Work],
        POLICY,
    )
}

/// A coordinator of `provider`'s configuration, under its own profile and capacity.
fn provided<'a>(
    store: &'a Store,
    scope: &Scope,
    provider: &'a Provider,
) -> Result<Coordinator<'a>, Refusal> {
    let profile = Profile::new(
        scope.clone(),
        provider.configuration().to_string(),
        provider.capabilities().clone(),
    );
    let made = Coordinator::new(store, profile, provider.adapter(), worker(scope))?;
    Ok(match provider.capacity() {
        Some((capacity, limit)) => made.with_capacity(capacity, limit),
        None => made,
    })
}

/// A coordinator for `attempt`'s configuration on `store`: its provider's, when this server holds
/// it, else the profile the attempt pinned, with an adapter nothing reaches.
fn coordinator<'a>(
    store: &'a Store,
    scope: &Scope,
    attempt: &Attempt,
    provider: Option<&'a Provider>,
) -> Result<Coordinator<'a>, Failed> {
    let made = match provider {
        Some(provider) => provided(store, scope, provider),
        None => {
            let identity = &attempt.identity;
            let mut profile = Profile::new(
                scope.clone(),
                identity.configuration.clone(),
                identity.capabilities.clone(),
            );
            profile.contract = identity.contract;
            Coordinator::new(store, profile, &Unreached, worker(scope))
        }
    };
    made.map_err(refused)
}

/// What goes through a provider alone, held by [`Providers::of`]: a binary that installs no
/// provider links neither, nor the admission a resume makes.
pub(crate) const SERVED: Served = Served {
    resume: resumed,
    view: linked,
};

/// A resume of attempt `id` through `provider`.
fn resumed(
    store: &Store,
    scope: &Scope,
    provider: &Provider,
    grant: &Grant,
    id: &str,
    continuation: Continuation,
    key: Option<&str>,
) -> Result<(), Refusal> {
    provided(store, scope, provider)?
        .resume(grant, id, continuation, key)
        .map(|_| ())
}

/// A view of attempt `id`'s resource through `provider`, resolved now.
fn linked(
    store: &Store,
    scope: &Scope,
    provider: &Provider,
    grant: &Grant,
    id: &str,
) -> Result<ExternalLink, Refusal> {
    provided(store, scope, provider)?.external_link(grant, id)
}

/// Why a provider is needed and not held.
fn unserved(attempt: &Attempt, what: &str) -> Failed {
    (
        StatusCode::CONFLICT,
        format!(
            "{what} goes through its provider, and {} is not one this server holds",
            attempt.identity.configuration
        ),
    )
}

/// A request's own fields, refused with any other: who asks is never one of them.
fn fields<T: serde::de::DeserializeOwned>(payload: Value) -> Result<T, Failed> {
    serde_json::from_value(payload).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

/// `payload`'s control, taken by the local owner: answered with the task's attempts after it.
pub(crate) fn control(hub: &Hub, payload: Value) -> Result<TaskAttempts, Failed> {
    let asked: AttemptControl = fields(payload)?;
    on_board(hub, &asked.slug, |store| controlled(hub, store, &asked))
}

fn controlled(hub: &Hub, store: &Store, asked: &AttemptControl) -> Result<TaskAttempts, Failed> {
    let scope = scope_of(hub, store);
    let asker = Asker::Local;
    let read = asker.grant(&scope, &asked.slug, &[Act::Read]);
    let attempt = Reader::new(store, scope.clone())
        .attempt(&read, &asked.attempt)
        .map_err(refused)?;
    if attempt.revision != asked.revision {
        return Err((
            StatusCode::CONFLICT,
            format!(
                "attempt {} moved since it was read (revision {}, now {}): read it again",
                attempt.id, asked.revision, attempt.revision
            ),
        ));
    }
    if local(&attempt) {
        return Err((StatusCode::CONFLICT, LOCAL.into()));
    }
    let provider = hub
        .providers
        .of_configuration(&attempt.identity.configuration);
    let mode = asked.mode.as_deref();
    let bad = |why: String| (StatusCode::BAD_REQUEST, why);
    match asked.act.as_str() {
        "cancel" => {
            if mode.is_some() {
                return Err(bad("a cancel takes no mode".into()));
            }
            let grant = asker.grant(&scope, &asked.slug, &[Act::Cancel]);
            coordinator(store, &scope, &attempt, provider)?
                .cancel(&grant, &attempt.id)
                .map_err(refused)?;
        }
        "stop" => {
            let mode = mode
                .and_then(stop_mode)
                .ok_or_else(|| bad(format!("a stop names its mode, not {mode:?}")))?;
            let grant = asker.grant(&scope, &asked.slug, &[Act::Stop]);
            coordinator(store, &scope, &attempt, provider)?
                .stop(&grant, &attempt.id, mode)
                .map_err(refused)?;
        }
        "resume" => {
            let mode = mode
                .and_then(resume_mode)
                .ok_or_else(|| bad(format!("a resume names its mode, not {mode:?}")))?;
            let Some((provider, served)) = hub.providers.serving(&attempt.identity.configuration)
            else {
                return Err(unserved(&attempt, "a resume"));
            };
            let continuation = Continuation {
                checkpoint: attempt.state.checkpoint.clone(),
                conversation: attempt.state.conversation.clone(),
                mode,
            };
            let grant = asker.grant(&scope, &asked.slug, &[Act::Resume]);
            let key = asked.key.as_deref();
            (served.resume)(
                store,
                &scope,
                provider,
                &grant,
                &attempt.id,
                continuation,
                key,
            )
            .map_err(refused)?;
        }
        other => return Err(bad(format!("no control {other:?}: cancel, stop or resume"))),
    }
    history(hub, store, &asked.slug, &asker)
}

/// A view of `payload`'s attempt, resolved now for the local owner through its provider.
pub(crate) fn view(hub: &Hub, payload: Value) -> Result<AttemptLink, Failed> {
    let asked: AttemptViewAsk = fields(payload)?;
    on_board(hub, &asked.slug, |store| viewed(hub, store, &asked))
}

fn viewed(hub: &Hub, store: &Store, asked: &AttemptViewAsk) -> Result<AttemptLink, Failed> {
    let scope = scope_of(hub, store);
    let asker = Asker::Local;
    let read = asker.grant(&scope, &asked.slug, &[Act::Read]);
    let attempt = Reader::new(store, scope.clone())
        .attempt(&read, &asked.attempt)
        .map_err(refused)?;
    if local(&attempt) {
        return Err((StatusCode::CONFLICT, LOCAL.into()));
    }
    let Some((provider, served)) = hub.providers.serving(&attempt.identity.configuration) else {
        return Err(unserved(&attempt, "a view"));
    };
    let grant = asker.grant(&scope, &asked.slug, &[Act::Link]);
    let link = (served.view)(store, &scope, provider, &grant, &attempt.id).map_err(refused)?;
    Ok(AttemptLink {
        url: link.url,
        valid_for_ms: link.valid_for_ms,
    })
}

#[cfg(test)]
#[path = "attempts_tests.rs"]
mod tests;
