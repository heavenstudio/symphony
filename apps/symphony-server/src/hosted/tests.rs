use super::*;
use axum::http::{HeaderValue, Request as HttpRequest};
use futures_util::{StreamExt, stream};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tower::ServiceExt;

const ORIGIN: &str = "https://app.example:8443";
const ISSUER: &str = "https://issuer.example";
const AUDIENCE: &str = "app:immutable-id";

fn epoch() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(10_000)
}

fn config() -> Config {
    Config::new("fixture", ORIGIN, ISSUER, AUDIENCE)
        .unwrap()
        .requiring_tenant("tenant-a")
        .with_clock(Arc::new(epoch))
}

/// An in-process stand-in for a consuming verifier, not a cryptographic verifier. Only its
/// explicit assertion table is trusted. Generic admission must propagate every refusal and
/// independently validate the verified claims/transport/grant bounds it returns.
struct Fake {
    changes: watch::Sender<()>,
    outage: AtomicBool,
    policy_outage: AtomicBool,
    allowed: AtomicBool,
    reads_only: AtomicBool,
    key: AtomicUsize,
    identities: AtomicUsize,
    authorizations: AtomicUsize,
    visited: Mutex<Vec<Context>>,
    redirect: Mutex<Option<String>>,
    wrong_grant: AtomicBool,
    grant_seconds: Mutex<i64>,
    digest: Mutex<Option<[u8; 32]>>,
    notify_during_grant: AtomicBool,
    closed_policy: AtomicBool,
    revision: Mutex<String>,
}

impl Fake {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            changes: watch::Sender::new(()),
            outage: AtomicBool::new(false),
            policy_outage: AtomicBool::new(false),
            allowed: AtomicBool::new(true),
            reads_only: AtomicBool::new(false),
            key: AtomicUsize::new(1),
            identities: AtomicUsize::new(0),
            authorizations: AtomicUsize::new(0),
            visited: Mutex::default(),
            redirect: Mutex::default(),
            wrong_grant: AtomicBool::new(false),
            grant_seconds: Mutex::new(120),
            digest: Mutex::new(None),
            notify_during_grant: AtomicBool::new(false),
            closed_policy: AtomicBool::new(false),
            revision: Mutex::new("policy-1".into()),
        })
    }

    fn identity_for(&self, assertion: &str) -> Result<Identity, Refusal> {
        if self.outage.load(Ordering::SeqCst) {
            return Err(Refusal::Unavailable);
        }
        if assertion.len() > 8192
            || assertion == "forged"
            || assertion == "none"
            || assertion == "unknown-key"
            || assertion == "wrong-signature"
            || (assertion == "key-1" && self.key.load(Ordering::SeqCst) != 1)
            || (assertion == "key-2" && self.key.load(Ordering::SeqCst) != 2)
        {
            return Err(Refusal::Unauthenticated);
        }
        let mut identity = Identity {
            actor: Actor {
                issuer: ISSUER.into(),
                subject: "verified-person".into(),
                tenant: Some("tenant-a".into()),
            },
            audience: AUDIENCE.into(),
            issued_at: epoch() - Duration::from_secs(1),
            expires_at: epoch() + Duration::from_secs(300),
        };
        match assertion {
            "key-1" | "key-2" => {}
            "issuer" => identity.actor.issuer = "https://foreign.example".into(),
            "app" => identity.audience = "app:other-id".into(),
            "tenant" => identity.actor.tenant = Some("tenant-b".into()),
            "missing-tenant" => identity.actor.tenant = None,
            "empty-subject" => identity.actor.subject.clear(),
            "no-project" => identity.actor.subject = "no-project".into(),
            "expired" => identity.expires_at = epoch(),
            "future" => identity.issued_at = epoch() + Duration::from_secs(1),
            "reversed" => identity.expires_at = identity.issued_at - Duration::from_secs(1),
            "long-life" => identity.expires_at = epoch() + Duration::from_secs(900),
            _ => return Err(Refusal::Unauthenticated),
        }
        Ok(identity)
    }
}

impl Host for Fake {
    fn transport<'a>(&'a self, head: &'a Parts) -> BoxFuture<'a, Result<Transport, Refusal>> {
        Box::pin(async move {
            let mut proof = head.headers.get_all("x-fixture-transport").iter();
            if proof.next().is_some_and(|v| v == "verified") && proof.next().is_none() {
                Ok(match *self.digest.lock().unwrap() {
                    Some(body_sha256) => Transport::RequestBound { body_sha256 },
                    None => Transport::ProtectedConnection,
                })
            } else {
                Err(Refusal::Denied)
            }
        })
    }

    fn identity<'a>(&'a self, head: &'a Parts) -> BoxFuture<'a, Result<Identity, Refusal>> {
        Box::pin(async move {
            self.identities.fetch_add(1, Ordering::SeqCst);
            let mut values = head.headers.get_all("x-fixture-visitor").iter();
            let assertion = values
                .next()
                .and_then(|v| v.to_str().ok())
                .ok_or(Refusal::Unauthenticated)?;
            if values.next().is_some() {
                return Err(Refusal::Unauthenticated);
            }
            self.identity_for(assertion)
        })
    }

    fn authorize<'a>(
        &'a self,
        actor: &'a Actor,
        target: &'a Target,
    ) -> BoxFuture<'a, Result<Grant, Refusal>> {
        Box::pin(async move {
            self.authorizations.fetch_add(1, Ordering::SeqCst);
            if actor.subject == "no-project" {
                return Err(Refusal::Missing);
            }
            if self.notify_during_grant.swap(false, Ordering::SeqCst) {
                self.changes.send_replace(());
            }
            if self.policy_outage.load(Ordering::SeqCst) {
                return Err(Refusal::Unavailable);
            }
            if !self.allowed.load(Ordering::SeqCst)
                || (self.reads_only.load(Ordering::SeqCst) && target.action != Action::Read)
            {
                return Err(Refusal::Denied);
            }
            let seconds = *self.grant_seconds.lock().unwrap();
            let valid_until = if seconds >= 0 {
                epoch() + Duration::from_secs(seconds.unsigned_abs())
            } else {
                epoch() - Duration::from_secs(seconds.unsigned_abs())
            };
            let mut target = target.clone();
            if self.wrong_grant.load(Ordering::SeqCst) {
                target.project = "other".into();
            }
            Ok(Grant {
                actor: actor.clone(),
                target,
                revision: self.revision.lock().unwrap().clone(),
                valid_until,
            })
        })
    }

    fn changes(&self) -> watch::Receiver<()> {
        if self.closed_policy.load(Ordering::SeqCst) {
            let (_, closed) = watch::channel(());
            closed
        } else {
            self.changes.subscribe()
        }
    }

    fn csrf<'a>(
        &'a self,
        head: &'a Parts,
        context: &'a Context,
    ) -> BoxFuture<'a, Result<(), Refusal>> {
        Box::pin(async move {
            if head
                .headers
                .get("cookie")
                .is_some_and(|v| v == "__Host-session=fixture")
                && head
                    .headers
                    .get("x-symphony-csrf")
                    .is_some_and(|v| v == "session-project-secret")
                && context.target.project == "fixture"
            {
                Ok(())
            } else {
                Err(Refusal::Denied)
            }
        })
    }

    fn dispatch<'a>(
        &'a self,
        context: &'a Context,
        _input: Input,
    ) -> BoxFuture<'a, Result<Response, Refusal>> {
        Box::pin(async move {
            self.visited.lock().unwrap().push(context.clone());
            if let Some(location) = self.redirect.lock().unwrap().clone() {
                return Ok((
                    StatusCode::TEMPORARY_REDIRECT,
                    [(header::LOCATION, location)],
                )
                    .into_response());
            }
            Ok("dispatched".into_response())
        })
    }
}

fn boundary(fake: &Arc<Fake>) -> Boundary {
    Boundary {
        config: config(),
        host: fake.clone(),
    }
}

fn harness(fake: &Arc<Fake>) -> Router {
    Router::new()
        .fallback(|request: Request| async move {
            assert!(admission(&request).is_some());
            for name in [
                "cookie",
                "authorization",
                "x-fixture-visitor",
                "x-fixture-transport",
                "forwarded",
                "x-forwarded-host",
            ] {
                assert!(!request.headers().contains_key(name), "{name}");
            }
            "private project bytes"
        })
        .layer(middleware::from_fn_with_state(boundary(fake), admit))
}

pub(crate) fn request(path: &str) -> Request {
    HttpRequest::builder()
        .uri(path)
        .header("host", "app.example:8443")
        .header("x-fixture-transport", "verified")
        .header("x-fixture-visitor", "key-1")
        .body(Body::empty())
        .unwrap()
}

pub(crate) fn native_host(project: &str) -> (Config, Arc<dyn Host>) {
    let mut configured = config();
    configured.project = project.into();
    (configured, Fake::new())
}

pub(crate) const PRIVATE_DIAGNOSTICS: &[&str] = &[
    "foreign-machine-id",
    "foreign-host-label",
    "foreign-producer-id",
    "/private/foreign-checkout",
    "private-machine-recipe",
    "unassigned-private-operation",
    "inventory-reader-secret",
    "/private/inventory-reader",
    "private-guidance",
];

pub(crate) fn diagnostic_work() -> symphony_api::MachineWork {
    use symphony_api::{
        MachineObservation, MachineWork, WorkCounts, WorkFreshness, WorkGuidance, WorkKind,
        WorkOperation, WorkState,
    };
    let operation = WorkOperation {
        machine_id: Some("foreign-machine-id".into()),
        operation_id: Some("foreign-operation".into()),
        parent_id: None,
        handle: Some("worker".into()),
        task: Some("tools/visible".into()),
        checkout: "/private/foreign-checkout".into(),
        recipe: "private-machine-recipe".into(),
        kind: WorkKind::Build,
        state: WorkState::Running,
        resource: "private-machine-resource".into(),
        reason: "inventory-reader-secret: /private/inventory-reader failed".into(),
        phase_since: None,
        under: Vec::new(),
    };
    let mut unassigned = operation.clone();
    unassigned.task = None;
    unassigned.operation_id = Some("unassigned-private-operation".into());
    MachineWork {
        freshness: WorkFreshness::Fresh,
        machines: vec![MachineObservation {
            machine_id: "foreign-machine-id".into(),
            label: "foreign-host-label".into(),
            producer_id: "foreign-producer-id".into(),
            sequence: 42,
            age_ms: Some(1),
            freshness: WorkFreshness::Fresh,
            complete: true,
            reason: "inventory-reader-secret: /private/inventory-reader failed".into(),
            counts: Some(WorkCounts {
                running_builds: 1,
                ..WorkCounts::default()
            }),
            operations: vec![operation],
        }],
        unassigned: vec![unassigned],
        guidance: std::collections::BTreeMap::from([(
            "private-guidance".into(),
            WorkGuidance {
                build: Some(4),
                test: Some(6),
            },
        )]),
    }
}

pub(crate) fn assert_no_diagnostics(text: &str) {
    for private in PRIVATE_DIAGNOSTICS {
        assert!(!text.contains(private), "leaked {private}");
    }
}

fn post(path: &str, json: &str) -> Request {
    let mut request = request(path);
    *request.method_mut() = Method::POST;
    for (key, value) in [
        ("origin", ORIGIN),
        ("content-type", "application/json"),
        ("x-symphony-csrf", "session-project-secret"),
        ("cookie", "__Host-session=fixture"),
    ] {
        request.headers_mut().insert(key, value.parse().unwrap());
    }
    *request.body_mut() = Body::from(json.to_string());
    request
}

fn unread_body() -> Body {
    Body::from_stream(stream::poll_fn(
        |_| -> std::task::Poll<Option<Result<Vec<u8>, std::io::Error>>> {
            panic!("refusal must happen before reading body")
        },
    ))
}

async fn bytes(response: Response) -> String {
    String::from_utf8(
        axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

#[test]
fn origin_configuration_is_exact_https_and_never_request_derived() {
    for origin in [
        "http://app.example",
        "https://",
        "https://a.example/",
        "https://a.example/path",
        "https://user@a.example",
        "https://*.example",
        "https://a.example:0",
        "https://a.example:65536",
        "https://a.example:",
        "https://a.example?x",
        "https://a.example#x",
        "https://a.example.",
        "https://a.example\\x",
        "https://a.example%2f.evil",
    ] {
        assert!(
            Config::new("fixture", origin, ISSUER, AUDIENCE).is_err(),
            "{origin}"
        );
    }
    for origin in [ORIGIN, "https://app.example", "https://[::1]:8443"] {
        assert!(
            Config::new("fixture", origin, ISSUER, AUDIENCE).is_ok(),
            "{origin}"
        );
    }
    assert!(Config::new("", ORIGIN, ISSUER, AUDIENCE).is_err());
}

#[tokio::test]
async fn transport_proof_is_independent_of_identity_and_forwarding_headers() {
    let fake = Fake::new();
    let mut req = request("/");
    req.headers_mut().remove("x-fixture-transport");
    req.headers_mut().insert(
        "forwarded",
        "host=app.example:8443;proto=https".parse().unwrap(),
    );
    req.headers_mut()
        .insert("x-forwarded-host", "app.example:8443".parse().unwrap());
    *req.body_mut() = unread_body();
    assert_eq!(
        harness(&fake).oneshot(req).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(fake.identities.load(Ordering::SeqCst), 0);
    assert_eq!(fake.authorizations.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn assertion_refusals_never_read_bodies_or_reach_authorization() {
    let fake = Fake::new();
    for token in [
        None,
        Some("forged"),
        Some("none"),
        Some("wrong-signature"),
        Some("unknown-key"),
        Some("issuer"),
        Some("app"),
        Some("tenant"),
        Some("missing-tenant"),
        Some("empty-subject"),
        Some("expired"),
        Some("future"),
        Some("reversed"),
        Some("long-life"),
    ] {
        let mut req = request("/");
        req.headers_mut().remove("x-fixture-visitor");
        if let Some(token) = token {
            req.headers_mut()
                .insert("x-fixture-visitor", token.parse().unwrap());
        }
        *req.body_mut() = unread_body();
        let response = harness(&fake).oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{token:?}");
        assert_eq!(
            bytes(response).await,
            r#"{"error":"authentication required"}"#
        );
    }
    let mut req = request("/");
    req.headers_mut()
        .append("x-fixture-visitor", "key-1".parse().unwrap());
    assert_eq!(
        harness(&fake).oneshot(req).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(fake.authorizations.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn key_rotation_and_key_outage_never_reuse_a_previous_allow() {
    let fake = Fake::new();
    assert_eq!(
        harness(&fake).oneshot(request("/")).await.unwrap().status(),
        StatusCode::OK
    );
    fake.key.store(2, Ordering::SeqCst);
    assert_eq!(
        harness(&fake).oneshot(request("/")).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    let mut rotated = request("/");
    rotated
        .headers_mut()
        .insert("x-fixture-visitor", "key-2".parse().unwrap());
    assert_eq!(
        harness(&fake).oneshot(rotated).await.unwrap().status(),
        StatusCode::OK
    );
    fake.outage.store(true, Ordering::SeqCst);
    assert_eq!(
        harness(&fake).oneshot(request("/")).await.unwrap().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn authority_origin_and_browser_checks_happen_before_body_access() {
    let fake = Fake::new();
    for (name, value, status) in [
        ("host", "foreign.example", StatusCode::FORBIDDEN),
        ("host", "user@app.example:8443", StatusCode::FORBIDDEN),
        ("origin", "https://foreign.example", StatusCode::FORBIDDEN),
        ("origin", "null", StatusCode::FORBIDDEN),
        ("sec-fetch-site", "cross-site", StatusCode::FORBIDDEN),
        ("content-type", "text/plain", StatusCode::FORBIDDEN),
        ("x-symphony-csrf", "forged", StatusCode::FORBIDDEN),
        ("cookie", "__Host-session=other", StatusCode::FORBIDDEN),
    ] {
        let mut req = post("/api/comment", "{}");
        req.headers_mut().insert(name, value.parse().unwrap());
        *req.body_mut() = unread_body();
        assert_eq!(
            harness(&fake).oneshot(req).await.unwrap().status(),
            status,
            "{name}: {value}"
        );
    }
    for name in ["origin", "x-symphony-csrf", "cookie"] {
        let mut req = post("/api/comment", "{}");
        req.headers_mut().remove(name);
        *req.body_mut() = unread_body();
        assert_eq!(
            harness(&fake).oneshot(req).await.unwrap().status(),
            StatusCode::FORBIDDEN,
            "{name}"
        );
    }
    for name in ["host", "origin", "content-type", "x-symphony-csrf"] {
        let mut req = post("/api/comment", "{}");
        let value = req.headers()[name].clone();
        req.headers_mut().append(name, value);
        assert_eq!(
            harness(&fake).oneshot(req).await.unwrap().status(),
            StatusCode::BAD_REQUEST,
            "{name}"
        );
    }
    assert!(fake.visited.lock().unwrap().is_empty());
}

#[tokio::test]
async fn route_query_and_body_aliases_cannot_choose_another_scope() {
    let fake = Fake::new();
    for path in [
        "/@other/api/document",
        "/%40other/api/document",
        "/api%2ffront",
        "/api/%2e%2e/front",
        "/%252e%252e/secret",
        "/docs/../secret",
        "/docs//secret",
        "/.git/config",
        "/docs/%5csecret",
        "/docs/%zz",
        "/docs/%ff",
        "/docs/%00",
        "/api/document?project=other",
        "/api/document?pro%6aect=other",
        "/api/document?scope=other",
        "/api/document?path=%40other/docs/A.md",
        "/api/document?path=%2540other/docs/A.md",
        "/api/document?path=docs/../../other",
        "/api/document?path=a&path=b",
        "/api/feed?since=other:3",
        "/api/front",
        "/api/projects",
        "/api/machine-work",
        "/api/schema-updates",
        "/api/settings",
        "/api/setting",
        "/api/agent-focus",
        "/api/agent-screen",
        "/api/task-offer",
    ] {
        let req = request(path);
        let status = harness(&fake).oneshot(req).await.unwrap().status();
        assert!(
            matches!(status, StatusCode::NOT_FOUND | StatusCode::BAD_REQUEST),
            "{path}: {status}"
        );
    }
    for json in [
        r#"{"project":"other"}"#,
        r#"{"project_id":"other"}"#,
        r#"{"scope":"other"}"#,
        r#"{"path":"@other/docs/A.md"}"#,
        r#"{"path":"../other"}"#,
        r#"{"path":"a%2fb"}"#,
        r#"{"path":"/etc/passwd"}"#,
        r#"{"actor":"owner"}"#,
        r#"{"tenant":"tenant-b"}"#,
        r#"{"path":{}}"#,
        "[]",
    ] {
        assert!(
            !harness(&fake)
                .oneshot(post("/api/comment", json))
                .await
                .unwrap()
                .status()
                .is_success(),
            "{json}"
        );
    }
    // A hosted visitor never writes a setting: the Settings page's write is not served there.
    let write = r#"{"key":"limits.low_volume","action":"store","value":1}"#;
    let status = harness(&fake)
        .oneshot(post(symphony_api::SETTING_WRITE_PATH, write))
        .await
        .unwrap()
        .status();
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(fake.visited.lock().unwrap().is_empty());
}

#[tokio::test]
async fn each_action_needs_its_own_grant_before_reading_a_body() {
    let fake = Fake::new();
    fake.reads_only.store(true, Ordering::SeqCst);
    assert_eq!(
        harness(&fake).oneshot(request("/")).await.unwrap().status(),
        StatusCode::OK
    );
    for path in [
        "/api/comment",
        "/api/task-action",
        "/api/upload",
        "/x/plugin/action",
    ] {
        let mut req = post(path, "{}");
        *req.body_mut() = unread_body();
        assert_eq!(
            harness(&fake).oneshot(req).await.unwrap().status(),
            StatusCode::FORBIDDEN,
            "{path}"
        );
    }
    for path in [
        "/artifacts/uploads/digest.png",
        "/api/feed?since=0",
        "/x/plugin/data",
    ] {
        assert_eq!(
            harness(&fake)
                .oneshot(request(path))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN,
            "{path}"
        );
    }
    assert!(fake.visited.lock().unwrap().is_empty());
}

#[tokio::test]
async fn context_aware_dispatch_gets_verified_actor_project_action_and_revision() {
    let fake = Fake::new();
    let response = harness(&fake)
        .oneshot(post(
            "/api/comment",
            r#"{"path":"docs/A.md","text":"hello"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(bytes(response).await, "dispatched");
    let visits = fake.visited.lock().unwrap();
    assert_eq!(visits.len(), 1);
    assert_eq!(visits[0].actor.subject, "verified-person");
    assert_eq!(visits[0].actor.tenant.as_deref(), Some("tenant-a"));
    assert_eq!(visits[0].target.project, "fixture");
    assert_eq!(
        visits[0].target.action,
        Action::Mutation("/api/comment".into())
    );
    assert_eq!(visits[0].policy_revision, "policy-1");
}

#[tokio::test]
async fn malformed_oversized_and_incomplete_bodies_never_dispatch() {
    let fake = Fake::new();
    for (body, expected) in [
        ("{", StatusCode::BAD_REQUEST),
        ("", StatusCode::BAD_REQUEST),
    ] {
        assert_eq!(
            harness(&fake)
                .oneshot(post("/api/comment", body))
                .await
                .unwrap()
                .status(),
            expected
        );
    }
    let large = "a".repeat(2 * 1024 * 1024 + 1);
    assert_eq!(
        harness(&fake)
            .oneshot(post("/api/comment", &large))
            .await
            .unwrap()
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert!(fake.visited.lock().unwrap().is_empty());
}

#[tokio::test]
async fn wrong_expired_or_unavailable_grants_fail_closed() {
    let fake = Fake::new();
    fake.wrong_grant.store(true, Ordering::SeqCst);
    assert_eq!(
        harness(&fake).oneshot(request("/")).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    fake.wrong_grant.store(false, Ordering::SeqCst);
    for seconds in [0, -1] {
        *fake.grant_seconds.lock().unwrap() = seconds;
        assert_eq!(
            harness(&fake).oneshot(request("/")).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
    *fake.grant_seconds.lock().unwrap() = 120;
    fake.policy_outage.store(true, Ordering::SeqCst);
    assert_eq!(
        harness(&fake).oneshot(request("/")).await.unwrap().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn redirects_are_relative_or_exactly_on_the_configured_origin() {
    let fake = Fake::new();
    for location in [
        "https://foreign.example/path",
        "//foreign.example/path",
        "https://app.example:8443.evil/path",
        "/\\foreign.example/path",
    ] {
        *fake.redirect.lock().unwrap() = Some(location.into());
        assert_eq!(
            harness(&fake)
                .oneshot(post("/api/comment", "{}"))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN,
            "{location}"
        );
    }
    for location in ["/docs/A.md", "https://app.example:8443/docs/A.md"] {
        *fake.redirect.lock().unwrap() = Some(location.into());
        assert_eq!(
            harness(&fake)
                .oneshot(post("/api/comment", "{}"))
                .await
                .unwrap()
                .status(),
            StatusCode::TEMPORARY_REDIRECT
        );
    }
}

/// An admission the fixture host verified, for a test that hands it to a native route as hosted
/// admission would have let the request in.
pub(crate) async fn admitted() -> Admission {
    lease(&Fake::new()).await
}

async fn lease(fake: &Arc<Fake>) -> Admission {
    Admission::new(
        boundary(fake),
        fake.identity_for("key-1").unwrap(),
        Target {
            project: "fixture".into(),
            route: "/api/feed".into(),
            action: Action::Feed,
        },
    )
    .await
    .unwrap()
}

#[tokio::test(start_paused = true)]
async fn idle_streams_end_at_grant_expiry_without_a_message_or_wall_clock_wait() {
    let fake = Fake::new();
    let lease = lease(&fake).await;
    let mut stream = Box::pin(lease.guard(stream::pending::<u8>()));
    tokio::time::advance(Duration::from_secs(120)).await;
    assert_eq!(stream.next().await, None);
    assert_eq!(lease.check().await, Err(Refusal::Denied));
}

#[tokio::test(start_paused = true)]
async fn identity_expiry_bounds_a_longer_policy_grant_even_with_a_frozen_wall_clock() {
    let fake = Fake::new();
    *fake.grant_seconds.lock().unwrap() = 1000;
    let lease = lease(&fake).await;
    assert_eq!(
        lease.context.valid_until,
        epoch() + Duration::from_secs(300)
    );
    tokio::time::advance(Duration::from_secs(300)).await;
    assert_eq!(lease.check().await, Err(Refusal::Denied));
}

#[tokio::test]
async fn policy_events_and_loss_cancel_idle_streams_and_pending_response_bytes() {
    let fake = Fake::new();
    let admitted = lease(&fake).await;
    let mut guarded = Box::pin(admitted.guard(stream::iter([1, 2])));
    assert_eq!(guarded.next().await, Some(1));
    fake.allowed.store(false, Ordering::SeqCst);
    fake.changes.send_replace(());
    assert_eq!(guarded.next().await, None);
    assert_eq!(admitted.check().await, Err(Refusal::Denied));

    let fake = Fake::new();
    let lease = lease(&fake).await;
    let mut idle = Box::pin(lease.guard(stream::pending::<u8>()));
    fake.changes.send_replace(());
    assert_eq!(idle.next().await, None);
}

#[tokio::test]
async fn policy_outage_before_a_chunk_and_reconnect_never_uses_cached_allow() {
    let fake = Fake::new();
    let lease = lease(&fake).await;
    let mut guarded = Box::pin(lease.guard(stream::iter([1, 2])));
    assert_eq!(guarded.next().await, Some(1));
    fake.policy_outage.store(true, Ordering::SeqCst);
    assert_eq!(guarded.next().await, None);
    assert_eq!(
        harness(&fake).oneshot(request("/")).await.unwrap().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn feed_origin_is_required_even_without_a_websocket_upgrade() {
    let fake = Fake::new();
    assert_eq!(
        harness(&fake)
            .oneshot(request("/api/feed?since=0"))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let mut req = request("/api/feed?since=0");
    req.headers_mut().insert("origin", ORIGIN.parse().unwrap());
    assert_eq!(
        harness(&fake).oneshot(req).await.unwrap().status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn native_router_keeps_local_gate_and_hosted_requests_have_no_machine_privilege() {
    let tree = crate::testing::Tree::new();
    tree.write("symphony.toml", "[project]\nid = \"fixture\"\n");
    let shared = Shared::new(
        1,
        1,
        &tree.root,
        None,
        crate::gate::Gate::new(
            Arc::new(|_| panic!("hosted admission must not ask tailnet")),
            Arc::new(std::time::Instant::now),
        ),
    );
    let fake = Fake::new();
    let app = router(shared.clone(), config(), fake.clone()).unwrap();
    let mut req = request("/");
    req.extensions_mut().insert(axum::extract::ConnectInfo(
        "203.0.113.9:1234".parse::<std::net::SocketAddr>().unwrap(),
    ));
    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    assert!(!bytes(response).await.is_empty());
    assert!(
        shared.api.get().is_none(),
        "shell must not open project board state"
    );

    for path in ["/api/front", "/api/machine-work"] {
        let mut req = request(path);
        req.extensions_mut().insert(axum::extract::ConnectInfo(
            "127.0.0.1:1234".parse::<std::net::SocketAddr>().unwrap(),
        ));
        assert_eq!(
            router(shared.clone(), config(), fake.clone())
                .unwrap()
                .oneshot(req)
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
        assert!(
            shared.api.get().is_none(),
            "{path} must be refused before a native hub/state read"
        );
    }
    let mut req = request("/");
    req.extensions_mut().insert(axum::extract::ConnectInfo(
        "203.0.113.9:1234".parse::<std::net::SocketAddr>().unwrap(),
    ));
    assert_eq!(
        crate::front::router(shared.clone())
            .oneshot(req)
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );

    let mut req = request("/api/agents");
    req.extensions_mut().insert(axum::extract::ConnectInfo(
        "127.0.0.1:1234".parse::<std::net::SocketAddr>().unwrap(),
    ));
    req.extensions_mut().insert(lease(&fake).await);
    assert!(!crate::session_go::local(&shared, &req).await);
    assert!(
        router(
            shared.clone(),
            Config::new("other", ORIGIN, ISSUER, AUDIENCE).unwrap(),
            fake
        )
        .is_err()
    );
    shared.stop();
}

/// The next actions are the local owner's: a hosted visitor's read of them is refused before a
/// hub starts, whatever task it names; the workflow's definition stays a native read.
#[tokio::test]
async fn next_actions_are_the_local_owners_never_a_hosted_visitors() {
    let tree = crate::testing::Tree::new();
    tree.write("symphony.toml", "[project]\nid = \"fixture\"\n");
    let shared = Shared::new(
        1,
        1,
        &tree.root,
        None,
        crate::gate::Gate::new(
            Arc::new(|_| panic!("hosted admission must not ask tailnet")),
            Arc::new(std::time::Instant::now),
        ),
    );
    let fake = Fake::new();
    // A task's guards are read with its actions, and are the local owner's as they are.
    for uri in [
        "/api/workflow-actions?slug=tools/x",
        "/api/workflow-guards?slug=tools/x",
    ] {
        let mut req = axum::http::Request::builder().uri(uri).body(()).unwrap();
        req.extensions_mut().insert(lease(&fake).await);
        let response = crate::api::answer(&shared, req).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{uri}");
    }
    assert!(shared.api.get().is_none(), "no hub was started");
    shared.stop();
}

#[tokio::test]
async fn cache_head_and_range_reads_still_require_current_media_authority() {
    let fake = Fake::new();
    for method in [Method::GET, Method::HEAD] {
        let mut req = request("/artifacts/uploads/digest.png");
        *req.method_mut() = method;
        req.headers_mut()
            .insert("range", "bytes=0-3".parse().unwrap());
        req.headers_mut()
            .insert("if-none-match", "*".parse().unwrap());
        fake.allowed.store(false, Ordering::SeqCst);
        let response = harness(&fake).oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "private, no-store"
        );
    }
}

#[tokio::test]
async fn native_and_adapter_errors_cannot_leak_diagnostics() {
    let fake = Fake::new();
    for status in [
        StatusCode::BAD_REQUEST,
        StatusCode::UNAUTHORIZED,
        StatusCode::FORBIDDEN,
        StatusCode::NOT_FOUND,
        StatusCode::METHOD_NOT_ALLOWED,
        StatusCode::CONFLICT,
        StatusCode::PAYLOAD_TOO_LARGE,
        StatusCode::INTERNAL_SERVER_ERROR,
        StatusCode::SERVICE_UNAVAILABLE,
    ] {
        let app = Router::new()
            .fallback(
                move || async move { (status, "secret-token /private/path database://secret") },
            )
            .layer(middleware::from_fn_with_state(boundary(&fake), admit));
        let response = app.oneshot(request("/")).await.unwrap();
        assert_eq!(response.status(), status);
        let text = bytes(response).await;
        assert!(!text.contains("secret"));
        assert!(!text.contains("/private"));
    }
    let app = Router::new()
        .fallback(|| async {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "secret-token /private/path database://secret",
            )
        })
        .layer(middleware::from_fn_with_state(boundary(&fake), admit));
    let response = app.oneshot(request("/")).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(bytes(response).await, r#"{"error":"service unavailable"}"#);
}

#[tokio::test]
async fn harmless_encodings_resolve_once_and_credentials_stop_at_the_boundary() {
    let fake = Fake::new();
    let mut req = request("/docs/A%20B.md");
    req.headers_mut()
        .insert("authorization", HeaderValue::from_static("Bearer secret"));
    req.headers_mut()
        .insert("cookie", HeaderValue::from_static("secret=session"));
    assert_eq!(
        harness(&fake).oneshot(req).await.unwrap().status(),
        StatusCode::OK
    );
    let mut req = request("/x/plugin/data");
    *req.method_mut() = Method::PUT;
    assert_eq!(
        harness(&fake).oneshot(req).await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn duplicate_and_nested_payload_targets_are_rejected_before_dispatch() {
    let fake = Fake::new();
    for json in [
        r#"{"project":"other","project":"fixture"}"#,
        r#"{"context":{"project":"other"}}"#,
        r#"{"references":[{"scope":"other"}]}"#,
        r#"{"path":"a","\u0070ath":"b"}"#,
    ] {
        assert!(
            !harness(&fake)
                .oneshot(post("/api/comment", json))
                .await
                .unwrap()
                .status()
                .is_success()
        );
    }
    assert!(fake.visited.lock().unwrap().is_empty());
}

#[tokio::test]
async fn real_upload_conditional_and_head_reads_are_private_and_revocation_precedes_304() {
    let tree = crate::testing::Tree::new();
    tree.write("symphony.toml", "[project]\nid = \"fixture\"\n");
    let upload = symphony::uploads::store(&tree.root, "evidence.txt", b"project evidence").unwrap();
    let path = format!("/artifacts/uploads/{}.txt", upload.sha);
    let shared = Shared::new(1, 1, &tree.root, None, crate::testing::open_gate());
    let fake = Fake::new();
    let app = router(shared.clone(), config(), fake.clone())
        .unwrap()
        .layer(axum::extract::connect_info::MockConnectInfo(
            "127.0.0.1:1234".parse::<std::net::SocketAddr>().unwrap(),
        ));
    let response = app.clone().oneshot(request(&path)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let etag = response.headers()[header::ETAG].clone();
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    assert_eq!(bytes(response).await, "project evidence");
    let mut req = request(&path);
    req.headers_mut()
        .insert(header::IF_NONE_MATCH, etag.clone());
    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    let mut req = request(&path);
    *req.method_mut() = Method::HEAD;
    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(bytes(response).await.is_empty());
    fake.allowed.store(false, Ordering::SeqCst);
    let mut req = request(&path);
    req.headers_mut().insert(header::IF_NONE_MATCH, etag);
    assert_eq!(
        app.oneshot(req).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    shared.stop();
}

fn feed_fixture(fake: &Arc<Fake>) -> (crate::testing::Tree, Arc<crate::live::Hub>, Router) {
    let tree = crate::testing::Tree::new();
    tree.write("symphony.toml", "[project]\nid = \"fixture\"\n");
    tree.write(
        "docs/README.md",
        "# Document\n\nSection body at the final diagnostic cursor.\n",
    );
    let hub = Arc::new(crate::live::Hub::new(&tree.root, &tree.root));
    hub.refresh_page("", &tree.root).unwrap();
    // Follow the document before the machine-only change, as an opened outline does. Sections
    // must then name the hub's latest cursor, even though their content has not changed.
    hub.document("", "docs/README.md").unwrap();
    {
        let mut pages = hub.pages.write().unwrap();
        let page = pages.get_mut("").unwrap();
        page.board.version = 8;
        page.history.clear();
        let work = diagnostic_work();
        let probes = serde_json::json!({"panes": "inventory-reader-secret", "slots": "/private/inventory-reader"});
        let ordinary = serde_json::json!({"region": "task", "op": "upsert", "key": "tools/visible",
            "data": {"slug": "tools/visible", "title": "ordinary-project-data"}});
        page.history.push_back(crate::live::Event {
            version: 7,
            json: serde_json::json!({"changes":[ordinary, {"region":"machine-work","data":work},
                {"region":"agent-probes","data":probes}],"version":7})
            .to_string(),
        });
        page.history.push_back(crate::live::Event {
            version: 8,
            json: serde_json::json!({"changes":[{"region":"machine-work","data":work},
                {"region":"agent-probes","data":probes}],"version":8})
            .to_string(),
        });
    }
    hub.server_notice
        .send_replace("secret diagnostic /private/path".into());
    let shared = Shared::new(1, 1, &tree.root, None, crate::testing::open_gate());
    assert!(shared.api.set(hub.clone()).is_ok());
    let app = router(shared, config(), fake.clone()).unwrap().layer(
        axum::extract::connect_info::MockConnectInfo(
            "127.0.0.1:1234".parse::<std::net::SocketAddr>().unwrap(),
        ),
    );
    (tree, hub, app)
}

async fn native_sections_at_final_cursor(app: &Router) -> symphony_api::DocumentSections {
    let outline = app
        .clone()
        .oneshot(request("/api/document?path=docs/README.md&view=outline"))
        .await
        .unwrap();
    assert_eq!(outline.status(), StatusCode::OK);
    let outline: symphony_api::DocumentOutline =
        serde_json::from_str(&bytes(outline).await).unwrap();
    // The renderer emits one section per block. Fetch the paragraph announced by the outline,
    // as the client requests the missing block near its viewport.
    let paragraph = outline
        .document
        .sections
        .last()
        .expect("the fixture document has a paragraph");
    let keys: String = serde_json::to_string(&[&paragraph.key])
        .unwrap()
        .bytes()
        .map(|byte| format!("%{byte:02X}"))
        .collect();
    let response = app
        .clone()
        .oneshot(request(&format!(
            "/api/section?path=docs/README.md&format=blocks&keys={keys}"
        )))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[symphony_api::VERSION_HEADER], "8");
    let body = bytes(response).await;
    assert_no_diagnostics(&body);
    let parts: symphony_api::DocumentSections = serde_json::from_str(&body).unwrap();
    assert_eq!(parts.version, 8);
    assert_eq!(parts.change.path, "docs/README.md");
    assert_eq!(parts.change.sections.len(), 1);
    assert_eq!(
        parts.change.sections[0].blocks,
        vec![symphony_api::Block::Paragraph {
            spans: vec![symphony_api::Span::Text {
                text: "Section body at the final diagnostic cursor.".into(),
            }],
        }]
    );
    parts
}

#[tokio::test]
async fn native_sse_keeps_the_final_diagnostic_cursor_for_sections_and_ends_at_revocation() {
    let fake = Fake::new();
    let (_tree, hub, app) = feed_fixture(&fake);
    let mut req = request("/api/feed?since=6");
    req.headers_mut()
        .insert(header::ORIGIN, ORIGIN.parse().unwrap());
    let mut stale = request("/api/feed?since=99");
    stale
        .headers_mut()
        .insert(header::ORIGIN, ORIGIN.parse().unwrap());
    assert_eq!(
        app.clone().oneshot(stale).await.unwrap().status(),
        StatusCode::CONFLICT
    );
    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-accel-buffering"], "no");
    let mut stream = response.into_body().into_data_stream();
    let event = stream.next().await.unwrap().unwrap();
    let event = std::str::from_utf8(&event).unwrap();
    assert!(event.contains("id: 7"));
    assert!(!event.contains("secret"));
    assert!(event.contains("ordinary-project-data"));
    assert_no_diagnostics(event);
    let sections = native_sections_at_final_cursor(&app).await;
    let next = tokio::time::timeout(test_time::backstop(Duration::from_secs(10)), stream.next())
        .await
        .unwrap_or_else(|_| panic!("hosted SSE cursor stalled; {}", test_time::Load::now()))
        .unwrap()
        .unwrap();
    let next = std::str::from_utf8(&next).unwrap();
    assert!(next.contains("id: 8"), "{next}");
    assert_no_diagnostics(next);
    let data = next
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .unwrap();
    let delta: symphony_api::Delta = serde_json::from_str(data).unwrap();
    assert_eq!(delta.version, sections.version, "sections can be displayed");
    assert!(delta.changes.is_empty());
    assert_eq!(
        hub.pages.read().unwrap()[""].board.version,
        sections.version
    );
    assert_eq!(
        hub.pages.read().unwrap()[""]
            .history
            .back()
            .unwrap()
            .version,
        8
    );
    fake.changes.send_replace(());
    assert!(stream.next().await.is_none());
    let mut req = request("/api/feed?since=other:7");
    req.headers_mut()
        .insert(header::ORIGIN, ORIGIN.parse().unwrap());
    assert_eq!(
        harness(&fake).oneshot(req).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    hub.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_websocket_keeps_the_final_diagnostic_cursor_for_sections_and_closes_on_revocation()
{
    use tungstenite::client::IntoClientRequest;
    let fake = Fake::new();
    let (_tree, hub, app) = feed_fixture(&fake);
    let sections = native_sections_at_final_cursor(&app).await;
    assert_eq!(
        hub.pages.read().unwrap()[""].board.version,
        sections.version
    );
    assert_eq!(
        hub.pages.read().unwrap()[""]
            .history
            .back()
            .unwrap()
            .version,
        8
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let revoke = fake.clone();
    tokio::task::spawn_blocking(move || {
        let stream = std::net::TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(test_time::backstop(Duration::from_secs(10))))
            .unwrap();
        let mut request = format!("ws://{address}/api/feed?since=6")
            .into_client_request()
            .unwrap();
        for (key, value) in [
            ("host", "app.example:8443"),
            ("origin", ORIGIN),
            ("x-fixture-transport", "verified"),
            ("x-fixture-visitor", "key-1"),
        ] {
            request.headers_mut().insert(key, value.parse().unwrap());
        }
        let (mut socket, response) = tungstenite::client(request, stream).unwrap();
        assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
        let first = socket.read().unwrap().into_text().unwrap();
        assert!(first.contains("\"version\":7"));
        assert!(!first.contains("secret"));
        assert!(first.contains("ordinary-project-data"));
        assert_no_diagnostics(&first);
        let next = socket.read().unwrap().into_text().unwrap();
        assert_no_diagnostics(&next);
        let (name, data) = next.split_once('\n').unwrap();
        assert_eq!(name, symphony_api::FEED_EVENT);
        let delta: symphony_api::Delta = serde_json::from_str(data).unwrap();
        assert_eq!(delta.version, sections.version, "sections can be displayed");
        assert!(delta.changes.is_empty());
        revoke.changes.send_replace(());
        assert!(matches!(
            socket.read().unwrap(),
            tungstenite::Message::Close(_)
        ));
        let _ = socket.close(None);
    })
    .await
    .unwrap();
    stop.send(()).unwrap();
    server.await.unwrap();
    hub.stop();
}

#[tokio::test]
async fn request_bound_transport_checks_actual_body_digest_for_reads_and_writes() {
    use sha2::{Digest, Sha256};
    let fake = Fake::new();
    *fake.digest.lock().unwrap() = Some(Sha256::digest(b"{}").into());
    assert_eq!(
        harness(&fake)
            .oneshot(post("/api/comment", "{}"))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        harness(&fake)
            .oneshot(post("/api/comment", r#"{"text":"altered"}"#))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(fake.visited.lock().unwrap().len(), 1);
    assert_eq!(
        harness(&fake).oneshot(request("/")).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    *fake.digest.lock().unwrap() = Some(Sha256::digest(b"").into());
    assert_eq!(
        harness(&fake).oneshot(request("/")).await.unwrap().status(),
        StatusCode::OK
    );
    assert_eq!(
        harness(&fake)
            .oneshot(request("/x/plugin/data"))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}

#[tokio::test(start_paused = true)]
async fn grant_expiry_cancels_a_pending_body_before_dispatch() {
    let fake = Fake::new();
    let mut req = post("/api/comment", "{}");
    *req.body_mut() = Body::from_stream(stream::pending::<Result<Vec<u8>, std::io::Error>>());
    let response = harness(&fake).oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(fake.visited.lock().unwrap().is_empty());
}

#[tokio::test]
async fn smaller_grant_bounds_and_response_errors_do_not_keep_an_old_allow() {
    let fake = Fake::new();
    let admitted = lease(&fake).await;
    *fake.grant_seconds.lock().unwrap() = 10;
    assert_eq!(admitted.check().await, Err(Refusal::Denied));
    let mut req = post("/api/comment", "{}");
    *req.body_mut() = Body::from_stream(stream::once(async {
        Err::<Vec<u8>, _>(std::io::Error::other("private error"))
    }));
    assert_eq!(
        harness(&fake).oneshot(req).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn closed_policy_and_changes_during_initial_authorization_fail_closed() {
    let fake = Fake::new();
    fake.closed_policy.store(true, Ordering::SeqCst);
    assert_eq!(
        harness(&fake).oneshot(request("/")).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    fake.closed_policy.store(false, Ordering::SeqCst);
    fake.notify_during_grant.store(true, Ordering::SeqCst);
    assert_eq!(
        harness(&fake).oneshot(request("/")).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let admitted = lease(&fake).await;
    *fake.revision.lock().unwrap() = "policy-2".into();
    assert_eq!(admitted.check().await, Err(Refusal::Denied));
}

#[tokio::test]
async fn a_wall_clock_already_past_expiry_cannot_deliver_even_an_idle_stream() {
    let fake = Fake::new();
    let now = Arc::new(Mutex::new(epoch()));
    let clock = now.clone();
    let boundary = Boundary {
        config: config().with_clock(Arc::new(move || *clock.lock().unwrap())),
        host: fake.clone(),
    };
    let admitted = Admission::new(
        boundary,
        fake.identity_for("key-1").unwrap(),
        Target {
            project: "fixture".into(),
            route: "/api/feed".into(),
            action: Action::Feed,
        },
    )
    .await
    .unwrap();
    *now.lock().unwrap() = epoch() + Duration::from_secs(301);
    assert_eq!(admitted.check().await, Err(Refusal::Denied));
    assert!(
        Box::pin(admitted.guard(stream::pending::<u8>()))
            .next()
            .await
            .is_none()
    );
}

struct Unintegrated(Arc<Fake>);

impl Host for Unintegrated {
    fn transport<'a>(&'a self, head: &'a Parts) -> BoxFuture<'a, Result<Transport, Refusal>> {
        self.0.transport(head)
    }
    fn identity<'a>(&'a self, head: &'a Parts) -> BoxFuture<'a, Result<Identity, Refusal>> {
        self.0.identity(head)
    }
    fn authorize<'a>(
        &'a self,
        actor: &'a Actor,
        target: &'a Target,
    ) -> BoxFuture<'a, Result<Grant, Refusal>> {
        self.0.authorize(actor, target)
    }
    fn changes(&self) -> watch::Receiver<()> {
        self.0.changes()
    }
}

#[tokio::test]
async fn a_host_without_csrf_or_actor_aware_dispatch_never_gains_legacy_mutation_access() {
    async fn legacy() -> Response {
        panic!("legacy handlers must not be called");
    }
    let fake = Fake::new();
    let app = Router::new()
        .fallback(legacy)
        .layer(middleware::from_fn_with_state(
            Boundary {
                config: config(),
                host: Arc::new(Unintegrated(fake)),
            },
            admit,
        ));
    let mut req = post("/api/comment", "{}");
    *req.body_mut() = unread_body();
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.oneshot(request("/x/plugin/data"))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn unknown_and_ungranted_projects_have_the_same_nonrevealing_miss() {
    let fake = Fake::new();
    let unknown = harness(&fake)
        .oneshot(request("/api/document?project=other"))
        .await
        .unwrap();
    let mut req = request("/api/document?project=fixture");
    req.headers_mut()
        .insert("x-fixture-visitor", "no-project".parse().unwrap());
    let ungranted = harness(&fake).oneshot(req).await.unwrap();
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    assert_eq!(ungranted.status(), unknown.status());
    assert_eq!(bytes(unknown).await, bytes(ungranted).await);
}

#[tokio::test]
async fn absolute_request_authorities_cannot_disagree_with_trusted_https_configuration() {
    let fake = Fake::new();
    for uri in ["https://foreign.example/", "http://app.example:8443/"] {
        assert_eq!(
            harness(&fake).oneshot(request(uri)).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        harness(&fake)
            .oneshot(request("https://app.example:8443/"))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let mut req = request("/");
    req.headers_mut()
        .insert(header::HOST, HeaderValue::from_bytes(b"\xff").unwrap());
    assert_eq!(
        harness(&fake).oneshot(req).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn oversize_and_nontext_assertions_are_refused_without_authorizing() {
    let fake = Fake::new();
    for value in [
        HeaderValue::from_str(&"x".repeat(8193)).unwrap(),
        HeaderValue::from_bytes(b"\xff").unwrap(),
    ] {
        let mut req = request("/");
        req.headers_mut().insert("x-fixture-visitor", value);
        *req.body_mut() = unread_body();
        assert_eq!(
            harness(&fake).oneshot(req).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(fake.authorizations.load(Ordering::SeqCst), 0);
}
