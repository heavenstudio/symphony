//! Opt-in, single-project admission for a trusted consuming host.
//!
//! [`router`] does not install a token verifier, trust forwarding headers or enable hosting in
//! the CLI. The host must independently prove proxy transport, verify the visitor's signature
//! and claims, supply current project grants and validate session/project-bound CSRF. A visitor
//! assertion alone is never transport proof. Errors are deliberately finite and nonsecret.
//!
//! Native read-only surfaces use the admitted project. Mutations and plugin routes go only to
//! [`Host::dispatch`], with verified context, never to the saves of the person the local gate
//! admits (`crate::write`), which refuse a hosted request: a write's actor
//! (`symphony_plugin::Actor`) is made from the verified [`Actor`] by the host that dispatches it,
//! never by the local saves. Worktree selection and machine operations are excluded. No machine
//! registry is consulted.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{Method, StatusCode, header, request::Parts};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use futures_util::future::BoxFuture;
use tokio::sync::watch;

use crate::front::Shared;

mod canonical;
mod json;
mod lease;
mod projection;
use lease::Admission;
pub(crate) use lease::admission;
pub(crate) use projection::{project_agents, project_feed, project_page};

/// A principal from a verified assertion; neither a display name nor a payload actor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Actor {
    pub issuer: String,
    pub subject: String,
    pub tenant: Option<String>,
}

/// The identity result of signature, algorithm, issuer, audience and key verification.
/// The host rejects missing/duplicate assertions and pins its key source in trusted config.
#[derive(Clone)]
pub struct Identity {
    pub actor: Actor,
    pub audience: String,
    pub issued_at: SystemTime,
    pub expires_at: SystemTime,
}

/// Independent transport evidence verified by the trusted host.
pub enum Transport {
    /// Effective network/IAM policy proves this connection came exclusively from the proxy.
    ProtectedConnection,
    /// A cryptographic proxy attestation binds method, full request target, this body digest,
    /// expiry and replay policy. Symphony compares the actual bytes before any dispatch.
    RequestBound { body_sha256: [u8; 32] },
}

/// Distinct grants; reading a project confers no mutation, feed or media permission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Read,
    Media,
    Feed,
    /// The exact canonical mutation route, never an implicit owner/control capability.
    Mutation(String),
    /// An exact plugin route, dispatched only through the context-aware host.
    Plugin(String),
}

/// The server-resolved target. The first profile has one configured project and no worktrees.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub project: String,
    pub route: String,
    pub action: Action,
}

/// A bounded grant for this exact actor and target. Revisions change on every policy change.
#[derive(Clone)]
pub struct Grant {
    pub actor: Actor,
    pub target: Target,
    pub revision: String,
    pub valid_until: SystemTime,
}

/// Authority passed to actor-aware dispatch. Credential/assertion headers are never included.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Context {
    pub actor: Actor,
    pub target: Target,
    pub policy_revision: String,
    pub valid_until: SystemTime,
}

/// Bounded and canonicalized input to a host's save/plugin/provider integration.
pub struct Input {
    pub query: BTreeMap<String, String>,
    pub json: Option<serde_json::Value>,
}

/// Nonsecret failures; no token, claim, filesystem path or dependency error is reflected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Unauthenticated,
    Denied,
    Missing,
    Invalid,
    Unavailable,
    TooLarge,
}

impl Refusal {
    fn response(self) -> Response {
        let (status, message) = match self {
            Self::Unauthenticated => (StatusCode::UNAUTHORIZED, "authentication required"),
            Self::Denied => (StatusCode::FORBIDDEN, "operation denied"),
            Self::Missing => (StatusCode::NOT_FOUND, "not found"),
            Self::Invalid => (StatusCode::BAD_REQUEST, "invalid request"),
            Self::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "service unavailable"),
            Self::TooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "request too large"),
        };
        (status, axum::Json(serde_json::json!({"error": message}))).into_response()
    }
}

/// Trusted adapter implemented by the consuming host, never by a visitor or a project document.
/// Async implementations bound all external calls and fail closed on key or policy outages.
pub trait Host: Send + Sync + 'static {
    /// Independent, verified transport evidence from the connection/deployment or a request-bound
    /// cryptographic proxy attestation. Forwarded headers and visitor identity are insufficient.
    fn transport<'a>(&'a self, head: &'a Parts) -> BoxFuture<'a, Result<Transport, Refusal>>;
    /// Verify exactly one signed assertion using protected issuer/app/key configuration. Reject
    /// forged, expired, wrong-key/issuer/app/tenant assertions; never decode as an outage fallback.
    fn identity<'a>(&'a self, head: &'a Parts) -> BoxFuture<'a, Result<Identity, Refusal>>;
    /// Current membership/action permission. Return `Missing` for unknown or ungranted projects,
    /// and `Denied` only for an action within an admitted project. No cached allow may outlive
    /// its declared validity.
    fn authorize<'a>(
        &'a self,
        actor: &'a Actor,
        target: &'a Target,
    ) -> BoxFuture<'a, Result<Grant, Refusal>>;
    /// Subscribe before authorization, so revocation between subscribe and check cannot be lost.
    /// Send on revocation/policy loss; closing the channel also ends all in-flight authorization.
    /// External revocation must reach this channel or have a separately accepted bounded contract.
    fn changes(&self) -> watch::Receiver<()>;
    /// Validate an unguessable CSRF token against the authenticated session AND project. A
    /// browser session binding uses a host-only Secure cookie, separate from proxy login cookies.
    /// Origin, fetch metadata and content type are also checked by Symphony before body reads.
    fn csrf<'a>(
        &'a self,
        _head: &'a Parts,
        _context: &'a Context,
    ) -> BoxFuture<'a, Result<(), Refusal>> {
        Box::pin(async { Err(Refusal::Denied) })
    }
    /// Context-aware integration only. Do not pass these operations to the local gate's saves or
    /// to actorless plugin APIs. Resolve domain-specific references and recheck permission before state/provider I/O;
    /// the request is cancelled at expiry/revocation, including while its response is streaming.
    fn dispatch<'a>(
        &'a self,
        _context: &'a Context,
        _input: Input,
    ) -> BoxFuture<'a, Result<Response, Refusal>> {
        Box::pin(async { Err(Refusal::Denied) })
    }
}

/// Protected deployment configuration, supplied by the host rather than discovered from HTTP.
#[derive(Clone)]
pub struct Config {
    project: String,
    origin: String,
    authority: String,
    issuer: String,
    audience: String,
    tenant: Option<String>,
    clock: Arc<dyn Fn() -> SystemTime + Send + Sync>,
}

impl Config {
    /// One exact HTTPS origin (no path, wildcard, userinfo or implicit alias), verified issuer
    /// and immutable app audience. An invalid configuration is refused before any router exists.
    pub fn new(project: &str, origin: &str, issuer: &str, audience: &str) -> Result<Self, Refusal> {
        let authority = canonical::authority(origin)?;
        if project.is_empty() || issuer.is_empty() || audience.is_empty() {
            return Err(Refusal::Invalid);
        }
        Ok(Self {
            project: project.into(),
            origin: origin.into(),
            authority,
            issuer: issuer.into(),
            audience: audience.into(),
            tenant: None,
            clock: Arc::new(SystemTime::now),
        })
    }

    /// Require this verified tenant in addition to the host's actor/project grant.
    #[must_use]
    pub fn requiring_tenant(mut self, tenant: &str) -> Self {
        self.tenant = Some(tenant.into());
        self
    }

    /// An injected wall clock; expiry also has a monotonic deadline so clock rollback cannot
    /// prolong a response. Tests advance the runtime clock without waiting in real time.
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn Fn() -> SystemTime + Send + Sync>) -> Self {
        self.clock = clock;
        self
    }
}

#[derive(Clone)]
struct Boundary {
    config: Config,
    host: Arc<dyn Host>,
}

/// Explicitly serve one project's read surfaces and actor-aware integrations under hosted
/// admission. Does not register projects, run a server, change the local gate or prove deployment
/// readiness. Only use with independently demonstrated transport and membership verification.
pub fn router(shared: Arc<Shared>, config: Config, host: Arc<dyn Host>) -> Result<Router, Refusal> {
    if !shared.base.is_empty()
        || symphony::project::Project::load(&shared.root)
            .map_err(|_| Refusal::Invalid)?
            .id
            != config.project
    {
        return Err(Refusal::Invalid);
    }
    Ok(
        crate::front::router(shared).layer(middleware::from_fn_with_state(
            Boundary { config, host },
            admit,
        )),
    )
}

async fn admit(State(boundary): State<Boundary>, request: Request, next: Next) -> Response {
    let head = request.method() == Method::HEAD;
    let response = match admitted(boundary, request, next).await {
        Ok(response) => response,
        Err(refused) => refused.response(),
    };
    let mut response = private(response);
    if head {
        *response.body_mut() = Body::empty();
    }
    response
}

async fn admitted(boundary: Boundary, request: Request, next: Next) -> Result<Response, Refusal> {
    let (head, body) = request.into_parts();
    canonical::headers(&boundary.config, &head)?;
    let transport = boundary.host.transport(&head).await?;
    let expected_digest = match transport {
        Transport::ProtectedConnection => None,
        Transport::RequestBound { body_sha256 } => Some(body_sha256),
    };
    let identity = boundary.host.identity(&head).await?;
    let now = (boundary.config.clock)();
    if identity.actor.issuer != boundary.config.issuer
        || identity.audience != boundary.config.audience
        || identity.actor.subject.is_empty()
        || identity.issued_at > now
        || identity.expires_at <= now
        || identity
            .expires_at
            .duration_since(identity.issued_at)
            .ok()
            .is_none_or(|life| life > Duration::from_secs(15 * 60))
        || boundary
            .config
            .tenant
            .as_ref()
            .is_some_and(|tenant| identity.actor.tenant.as_ref() != Some(tenant))
    {
        return Err(Refusal::Unauthenticated);
    }
    let (target, query) = canonical::target(&boundary.config, &head)?;
    let admission = Admission::new(boundary.clone(), identity, target).await?;
    let mutation = head.method == Method::POST;
    if mutation {
        canonical::browser(&boundary.config, &head, true)?;
        boundary.host.csrf(&head, &admission.context).await?;
    } else if admission.context.target.action == Action::Feed {
        canonical::browser(&boundary.config, &head, false)?;
    }
    let dispatch = mutation || matches!(admission.context.target.action, Action::Plugin(_));
    let operation = async {
        if dispatch {
            let json = if mutation {
                let bytes = json::collect(body, expected_digest).await?;
                let json = json::parse(&bytes)?;
                canonical::payload(&boundary.config, &json)?;
                Some(json)
            } else {
                if expected_digest.is_some() {
                    json::collect(body, expected_digest).await?;
                }
                None
            };
            admission.check().await?;
            boundary
                .host
                .dispatch(&admission.context, Input { query, json })
                .await
        } else {
            let body = if expected_digest.is_some() {
                json::collect(body, expected_digest).await?;
                Body::empty()
            } else {
                body
            };
            admission.check().await?;
            let mut request = Request::from_parts(head, body);
            canonical::strip_credentials(request.headers_mut());
            request.extensions_mut().insert(admission.clone());
            Ok(next.run(request).await)
        }
    };
    let response = tokio::select! {
        biased;
        () = admission.ended() => return Err(Refusal::Denied),
        response = operation => response?,
    };
    admission.check().await?;
    let response = canonical::response(&boundary.config, response)?;
    let (parts, body) = response.into_parts();
    Ok(Response::from_parts(
        parts,
        Body::from_stream(admission.guard(body.into_data_stream())),
    ))
}

fn private(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("private, no-store"),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        header::HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        header::HeaderValue::from_static("no-referrer"),
    );
    response
}

#[cfg(test)]
pub(crate) mod tests;
