//! A task's execution attempts through the server's routes
//! ([docs/EXECUTION_ATTEMPTS.md](../../../docs/EXECUTION_ATTEMPTS.md#on-a-tasks-page)): their
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
//! operations, wherever it runs: it needs no provider here. A resume admits another attempt under
//! its provider's own profile and capacity, and a view is that provider's I/O now, so both go only
//! through a provider this server holds ([`crate::execution`]). A view's address is answered to
//! the one who asked, `no-store`, and kept nowhere: not in the board, a log, a cache or a thread.

use crate::execution::{Provider, Providers};
use crate::live::Hub;
use axum::http::StatusCode;
use serde_json::Value;
use std::time::Instant;
use symphony_api::{AttemptControl, AttemptControls, AttemptLink, AttemptViewAsk, TaskAttempts};
use symphony_board::Store;
use symphony_board::execution::{
    Act, Adapter, Attempt, Continuation, Coordinator, Grant, Operation, Outcome, Principal,
    PrincipalKind, Profile, Reader, Refusal, Scope, board_scope, resume_mode, resume_word,
    stop_mode, stop_word,
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
    Hosted { principal: Principal, policy: String },
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

/// The board scope of the project `hub` serves, on `store`.
fn scope_of(hub: &Hub, store: &Store) -> Scope {
    board_scope(&symphony::project::Project::of(&hub.root).id, store)
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
    let store = hub
        .reader()
        .map_err(|e| crate::api::unavailable(&e, ""))?;
    let done = store
        .get(slug)
        .map_err(crate::api::task_failure)
        .and_then(|_| work(&store));
    if let Err((status, _)) = &done
        && [StatusCode::SERVICE_UNAVAILABLE, StatusCode::GATEWAY_TIMEOUT].contains(status)
    {
        hub.reader_failed(&store);
    }
    done
}

/// What `attempt` may be asked here, by the local owner: its controls as its state, its pinned
/// capabilities and the providers this server holds allow. The coordinator decides each again.
pub(crate) fn controls(attempt: &Attempt, providers: &Providers) -> AttemptControls {
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
        pinned.stop_modes.iter().map(|m| stop_word(*m).into()).collect()
    } else {
        Vec::new()
    };
    let resumable = attempt.resumable().is_ok()
        && (state.conversation.is_some() || state.checkpoint.is_some());
    let viewable = state.resource.is_some() && pinned.external_view;
    let resume = match provider {
        Some(provider) if resumable => {
            let offered = provider.capabilities();
            if offered.operations.contains(&Operation::Resume) {
                offered.resume_modes.iter().map(|m| resume_word(*m).into()).collect()
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    };
    AttemptControls {
        cancel,
        stop,
        resume,
        view: viewable && provider.is_some(),
        unserved: (provider.is_none() && (resumable || viewable)).then(|| {
            format!(
                "its provider {} is not one this server holds",
                identity.configuration
            )
        }),
    }
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

/// A coordinator for `attempt`'s configuration on `store`: its provider's, when this server holds
/// it, else the profile the attempt pinned, with an adapter nothing reaches. The server's worker
/// grant only makes it; the server steps no attempt.
fn coordinator<'a>(
    store: &'a Store,
    scope: &Scope,
    attempt: &Attempt,
    provider: Option<&'a Provider>,
) -> Result<Coordinator<'a>, Failed> {
    let identity = &attempt.identity;
    let worker = Grant::from_host(
        symphony_board::workflow::board_service("symphony-server"),
        scope.clone(),
        &[Act::Work],
        POLICY,
    );
    let made = match provider {
        Some(provider) => {
            let profile = Profile::new(
                scope.clone(),
                provider.configuration().to_string(),
                provider.capabilities().clone(),
            );
            Coordinator::new(store, profile, provider.adapter(), worker).map(|c| {
                match provider.capacity() {
                    Some((capacity, limit)) => c.with_capacity(capacity, limit),
                    None => c,
                }
            })
        }
        None => {
            let mut profile = Profile::new(
                scope.clone(),
                identity.configuration.clone(),
                identity.capabilities.clone(),
            );
            profile.contract = identity.contract;
            Coordinator::new(store, profile, &Unreached, worker)
        }
    };
    made.map_err(refused)
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
            if provider.is_none() {
                return Err(unserved(&attempt, "a resume"));
            }
            let continuation = Continuation {
                checkpoint: attempt.state.checkpoint.clone(),
                conversation: attempt.state.conversation.clone(),
                mode,
            };
            let grant = asker.grant(&scope, &asked.slug, &[Act::Resume]);
            coordinator(store, &scope, &attempt, provider)?
                .resume(&grant, &attempt.id, continuation, asked.key.as_deref())
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
    let Some(provider) = hub
        .providers
        .of_configuration(&attempt.identity.configuration)
    else {
        return Err(unserved(&attempt, "a view"));
    };
    let grant = asker.grant(&scope, &asked.slug, &[Act::Link]);
    let link = coordinator(store, &scope, &attempt, Some(provider))?
        .external_link(&grant, &attempt.id)
        .map_err(refused)?;
    Ok(AttemptLink {
        url: link.url,
        valid_for_ms: link.valid_for_ms,
    })
}

#[cfg(test)]
#[path = "attempts_tests.rs"]
mod tests;
