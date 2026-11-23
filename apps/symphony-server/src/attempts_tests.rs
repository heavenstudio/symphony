//! A task's execution attempts through the server's routes, on a private board, a deterministic
//! provider the server holds (`fake`, the board's own test provider) and a worker the test steps:
//! the history kept whole after a handle leaves, each control taken by the local owner through the
//! coordinator, each refusal (a forged owner, another task's or project's attempt, a stale
//! revision, a provider not held), a view resolved on demand and kept nowhere, and a hosted visitor
//! who reads and controls nothing here.

use super::*;
use crate::execution::{Provider, Providers};
use crate::front::{Shared, router};
use crate::testing::{
    Tree,
    board::{Fixture, Seed},
    repository, store_on,
};
use axum::body::{Body, to_bytes};
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{HeaderMap, Request, header};
use serde_json::{Value, json};
use std::sync::Arc;
use symphony_board::execution::{Admission, Collection, Coordinator, Grant, Input, Worked};
use symphony_plugin::execution::ContentRef;
use tower::ServiceExt;

#[expect(
    dead_code,
    reason = "the board's provider for its own tests; these use what they need of it"
)]
#[path = "../../../crates/tasks/tests/store/execution/fake.rs"]
mod fake;

use fake::{Fake, capabilities};

const TASK: &str = "tools/run";
const OTHER: &str = "tools/other";
const PROJECT: &str = "proj";
const CONFIGURATION: &str = "fake";

fn input() -> Input {
    Input::new(
        "r1".into(),
        "digest-of-r1".into(),
        "commit-abc".into(),
        ContentRef::new("manifest".into(), "manifest-digest".into()),
    )
}

/// A project `proj` served on a board where `amber` executes [`TASK`], its server holding the
/// fake provider of [`CONFIGURATION`], and the provider's own worker, which the test steps.
struct Served {
    shared: Arc<Shared>,
    runtime: tokio::runtime::Runtime,
    app: axum::Router,
    fake: Arc<Fake>,
    f: Fixture,
    _tree: Tree,
}

impl Served {
    fn new() -> Served {
        Served::with_file("[project]\nid = \"proj\"\n")
    }

    /// The project whose `symphony.toml` is `file`.
    fn with_file(file: &str) -> Served {
        let tree = Tree::new();
        let root = repository(&tree);
        std::fs::write(root.join("symphony.toml"), file).unwrap();
        let f = Fixture::new(
            Seed::default()
                .session("amber")
                .task(TASK, "in_progress")
                .role(TASK, "amber", "executor")
                .task(OTHER, "in_progress"),
        );
        let fake = Arc::new(Fake::default());
        let mut hub = Hub::new(&root, &root.join("build/client"));
        let board = f.other_database();
        hub.connect_reader = Box::new(move |_| Ok(store_on(&board)));
        hub.providers = Providers::of(vec![Provider::new(
            CONFIGURATION,
            capabilities(),
            fake.clone(),
        )]);
        hub.refresh_page("", &root).unwrap();
        let gate = crate::testing::open_gate();
        let shared = Shared::new(1, 1, &root, None, gate);
        assert!(shared.api.set(Arc::new(hub)).is_ok());
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let app = router(shared.clone());
        Served {
            shared,
            runtime,
            app,
            fake,
            f,
            _tree: tree,
        }
    }

    fn scope(&self, project: &str) -> Scope {
        board_scope(project, &self.f.store)
    }

    /// The provider's worker for `configuration` under `project`'s scope.
    fn worker<'a>(&'a self, project: &str, configuration: &str) -> Coordinator<'a> {
        let scope = self.scope(project);
        let profile = Profile::new(scope.clone(), configuration.into(), capabilities());
        let worker = Principal::new(PrincipalKind::Service, "board".into(), "worker".into());
        let worker = Grant::from_host(worker, scope, &[Act::Work], "policy-1");
        Coordinator::new(&self.f.store, profile, self.fake.as_ref(), worker).unwrap()
    }

    /// An attempt of `task` admitted by its host under `project` and `configuration`, by amber.
    fn admit(&self, project: &str, configuration: &str, task: &str) -> String {
        let scope = self.scope(project);
        let starter = Grant::from_host(
            Principal::new(PrincipalKind::Service, "host".into(), "launcher".into()),
            scope,
            &[Act::Start],
            "policy-1",
        );
        let asked = Admission::new(task, "executor/amber", "executor", input()).handle("amber");
        self.worker(project, configuration)
            .admit(&starter, asked)
            .unwrap()
            .id
    }

    /// One step of the worker for `id`, which must have answered.
    fn step(&self, id: &str) -> Outcome {
        match self.worker(PROJECT, CONFIGURATION).step(id).unwrap() {
            Worked::Answered { outcome, .. } => outcome,
            other => panic!("expected an answer, got {other:?}"),
        }
    }

    /// An attempt of [`TASK`] started and observed running, its conversation bound.
    fn running(&self) -> String {
        let id = self.admit(PROJECT, CONFIGURATION, TASK);
        assert!(matches!(self.step(&id), Outcome::Accepted(_)));
        assert!(matches!(self.step(&id), Outcome::Observed(_)));
        id
    }

    /// The method, path and body asked from this machine: status, headers and JSON.
    fn ask(&self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, HeaderMap, Value) {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("Host", "127.0.0.1:1")
            .header("Content-Type", "application/json")
            .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
            .unwrap();
        let app = self.app.clone().layer(MockConnectInfo(
            "127.0.0.1:2".parse::<std::net::SocketAddr>().unwrap(),
        ));
        self.runtime.block_on(async {
            let response = app.oneshot(request).await.unwrap();
            let (status, headers) = (response.status(), response.headers().clone());
            let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let body = serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into()));
            (status, headers, body)
        })
    }

    fn history(&self, slug: &str) -> Vec<Value> {
        let (status, _, body) = self.ask("GET", &format!("/api/task-attempts?slug={slug}"), None);
        assert_eq!(status, StatusCode::OK, "{body}");
        body["attempts"].as_array().unwrap().clone()
    }

    /// Attempt `id` as the history of [`TASK`] shows it.
    fn shown(&self, id: &str) -> Value {
        self.history(TASK)
            .into_iter()
            .find(|attempt| attempt["id"] == id)
            .unwrap_or_else(|| panic!("{id} is not in the history"))
    }

    fn control(&self, body: Value) -> (StatusCode, Value) {
        let (status, _, body) = self.ask("POST", symphony_api::ATTEMPT_CONTROL_ROUTE, Some(body));
        (status, body)
    }

    /// Every fact and state the board holds of `slug`'s attempts, as text.
    fn kept(&self, slug: &str) -> String {
        let scope = self.scope(PROJECT);
        let grant = Grant::from_host(
            symphony_board::workflow::local_owner(),
            scope.clone(),
            &[Act::Read],
            "policy-1",
        );
        let reader = Reader::new(&self.f.store, scope);
        let mut kept = String::new();
        for attempt in reader.history(&grant, slug).unwrap() {
            kept.push_str(&serde_json::to_string(&attempt.state).unwrap());
            kept.push_str(&serde_json::to_string(&attempt.identity).unwrap());
            for recorded in reader.facts(&grant, &attempt.id).unwrap() {
                kept.push_str(&serde_json::to_string(&recorded.fact).unwrap());
            }
        }
        kept
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        self.shared.stop();
    }
}

fn said(body: &Value) -> &str {
    body["error"].as_str().unwrap_or_default()
}

#[test]
fn a_tasks_history_keeps_its_attempts_and_their_references_after_the_handle_leaves() {
    let s = Served::new();
    assert!(s.history(TASK).is_empty(), "a task never run has none");
    let first = s.running();
    let shown = s.shown(&first);
    assert_eq!(shown["dispatch"], "running");
    assert_eq!(shown["handle"], "amber");
    assert_eq!(shown["resource"], "res-1");
    assert_eq!(shown["generation"], "g1");
    assert_eq!(shown["conversation"], "conversation-of-res-1");
    assert_eq!(shown["initiator"], "service host/launcher");
    assert_eq!(
        shown["controls"],
        json!({"cancel": true, "stop": ["pause", "terminate"], "resume": [], "view": true,
            "unserved": null})
    );
    // A remote attempt has no pane: nothing of it is a pane, and Go to session is as it was.
    assert!(shown.get("pane").is_none());
    let (status, _, page) = s.ask("GET", &format!("/api/task-page?slug={TASK}"), None);
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["attempts"], 1);
    assert!(page["sessions"].get("amber").is_none(), "{page}");

    // Paused, then resumed from its conversation as a new attempt of the same lane.
    let revision = shown["revision"].as_i64().unwrap();
    let (status, body) = s.control(json!({"slug": TASK, "attempt": first, "revision": revision,
        "act": "stop", "mode": "pause"}));
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(matches!(s.step(&first), Outcome::Accepted(_)));
    assert!(matches!(s.step(&first), Outcome::Observed(_)));
    let paused = s.shown(&first);
    assert_eq!(paused["dispatch"], "paused");
    assert_eq!(
        paused["controls"]["resume"],
        json!(["new_resource", "reuse_with_new_generation"])
    );
    let resume = json!({"slug": TASK, "attempt": first, "revision": paused["revision"],
        "act": "resume", "mode": "new_resource", "key": "page-key-1"});
    let (status, body) = s.control(resume.clone());
    assert_eq!(status, StatusCode::OK, "{body}");
    let attempts = body["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 2, "{body}");
    let second = attempts[1]["id"].as_str().unwrap().to_string();
    assert_eq!(attempts[1]["predecessor"], json!(first));
    assert_eq!(attempts[1]["relation"], "resume");
    assert_eq!(attempts[1]["initiator"], "human local/owner");
    assert_eq!(attempts[1]["current"], true);
    assert_eq!(attempts[0]["current"], false);
    assert_eq!(attempts[0]["successor"], json!(second));
    // The same request key, sent again at the revision read before, admits nothing more: the
    // attempt moved, so it is refused as stale, and the history holds two.
    let (status, body) = s.control(resume);
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(s.history(TASK).len(), 2);

    // The handle leaves: the board keeps every attempt and what was bound to each.
    s.f.store.leave("amber").unwrap();
    let after = s.history(TASK);
    assert_eq!(after.len(), 2);
    assert_eq!(after[0]["resource"], "res-1");
    assert_eq!(after[0]["conversation"], "conversation-of-res-1");
    assert_eq!(after[0]["handle"], "amber");
    assert_eq!(
        after[0]["controls"],
        json!({"cancel": false, "stop": [], "resume": [], "view": true, "unserved": null}),
        "a followed attempt is controlled no more, and its resource may still be viewed"
    );
    let (_, _, page) = s.ask("GET", &format!("/api/task-page?slug={TASK}"), None);
    assert_eq!(page["attempts"], 2);
}

#[test]
fn a_cancel_is_the_local_owners_intent_through_the_coordinator() {
    let s = Served::new();
    let id = s.running();
    let revision = s.shown(&id)["revision"].clone();
    let (status, body) =
        s.control(json!({"slug": TASK, "attempt": id, "revision": revision, "act": "cancel"}));
    assert_eq!(status, StatusCode::OK, "{body}");
    let attempt = &body["attempts"][0];
    assert_eq!(attempt["cancellation"], "requested");
    assert_eq!(attempt["controls"]["cancel"], false, "asked once");
    let kept = s.kept(TASK);
    assert!(
        kept.contains(r#""kind":"asked","initiator":{"kind":"Human","issuer":"local","subject":"owner"},"cancel":true"#),
        "{kept}"
    );
    // The provider's worker sends it.
    assert!(s.fake.calls().iter().all(|op| *op != Operation::Cancel));
    assert!(matches!(s.step(&id), Outcome::Accepted(_)));
    assert!(s.fake.calls().contains(&Operation::Cancel));
}

#[test]
fn a_resource_whose_cleanup_has_begun_is_offered_and_resumed_only_on_a_new_one() {
    let s = Served::new();
    let id = s.running();
    let revision = s.shown(&id)["revision"].clone();
    let (status, body) = s.control(json!({"slug": TASK, "attempt": id, "revision": revision,
        "act": "stop", "mode": "terminate"}));
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(matches!(s.step(&id), Outcome::Accepted(_)));
    assert!(matches!(s.step(&id), Outcome::Observed(_)));
    let ended = s.shown(&id);
    assert_eq!(ended["ending"], "stopped", "{ended}");
    assert_eq!(
        ended["controls"]["resume"],
        json!(["new_resource", "reuse_with_new_generation"]),
        "its resource not yet cleaned, either way"
    );

    // Its evidence collected and its resource's cleanup begun, as a project's worker does.
    let worker = s.worker(PROJECT, CONFIGURATION);
    let cleaner = Grant::from_host(
        Principal::new(PrincipalKind::Service, "board".into(), "cleaner".into()),
        s.scope(PROJECT),
        &[Act::Collect, Act::Cleanup],
        "policy-1",
    );
    worker.begin_collection(&cleaner, &id).unwrap();
    worker
        .end_collection(&cleaner, &id, Collection::None, Some("it wrote nothing"))
        .unwrap();
    worker.begin_cleanup(&cleaner, &id).unwrap();
    let cleaning = s.shown(&id);
    assert_ne!(cleaning["cleanup"], "not_due", "{cleaning}");
    assert_eq!(
        cleaning["controls"]["resume"],
        json!(["new_resource"]),
        "a resource being cleaned is not reused"
    );

    // A reuse asked all the same is the coordinator's refusal, and writes nothing.
    let before = s.kept(TASK);
    let (status, body) = s.control(json!({"slug": TASK, "attempt": id,
        "revision": cleaning["revision"], "act": "resume", "mode": "reuse_with_new_generation"}));
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(said(&body).contains("cleanup has begun"), "{body}");
    assert_eq!(s.kept(TASK), before, "nothing written");
    let (status, body) = s.control(json!({"slug": TASK, "attempt": id,
        "revision": cleaning["revision"], "act": "resume", "mode": "new_resource"}));
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["attempts"][1]["predecessor"], json!(id));
}

#[test]
fn a_control_is_refused_for_a_forged_owner_another_task_or_project_and_a_stale_read() {
    let s = Served::new();
    let id = s.running();
    let revision = s.shown(&id)["revision"].as_i64().unwrap();
    let before = s.kept(TASK);
    let cancel = |more: Value| {
        let mut asked = json!({"slug": TASK, "attempt": id, "revision": revision, "act": "cancel"});
        for (key, value) in more.as_object().unwrap() {
            asked[key] = value.clone();
        }
        s.control(asked)
    };
    // Who asks is never a field of the request.
    for forged in [
        json!({"owner": "owner"}),
        json!({"principal": {"kind": "Human", "issuer": "local", "subject": "owner"}}),
        json!({"grant": "all"}),
    ] {
        let (status, body) = cancel(forged.clone());
        assert_eq!(status, StatusCode::BAD_REQUEST, "{forged}: {body}");
        assert!(said(&body).contains("unknown field"), "{body}");
    }
    // Another task's slug names no attempt of this one.
    let (status, body) = cancel(json!({"slug": OTHER}));
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    // No such attempt, no such task, no slug.
    let (status, body) = cancel(json!({"attempt": "0123456789abcdef0123456789abcdef"}));
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = cancel(json!({"slug": "tools/none"}));
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = cancel(json!({"slug": ""}));
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    // A revision the attempt has moved past is stale.
    let (status, body) = cancel(json!({"revision": revision - 1}));
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(said(&body).contains("moved since it was read"), "{body}");
    // A control's words: its act, and its mode where it takes one.
    for (more, why) in [
        (json!({"act": "delete"}), "no control"),
        (json!({"mode": "pause"}), "takes no mode"),
        (json!({"act": "stop"}), "names its mode"),
        (json!({"act": "stop", "mode": "explode"}), "names its mode"),
        (json!({"act": "resume", "mode": "pause"}), "names its mode"),
    ] {
        let (status, body) = cancel(more.clone());
        assert_eq!(status, StatusCode::BAD_REQUEST, "{more}: {body}");
        assert!(said(&body).contains(why), "{more}: {body}");
    }
    // A stop the provider does not offer, and a resume of a running attempt, are the
    // coordinator's refusals.
    let (status, body) = cancel(json!({"act": "stop", "mode": "hard_stop"}));
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let (status, body) = cancel(json!({"act": "resume", "mode": "new_resource"}));
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(s.kept(TASK), before, "no refused control wrote anything");
    // An attempt this board holds for another project is no attempt of this one, named under
    // either task's slug.
    let foreign = s.admit("another-project", CONFIGURATION, OTHER);
    for slug in [TASK, OTHER] {
        let (status, body) = cancel(json!({"slug": slug, "attempt": foreign}));
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert!(
            said(&body).contains("another project's or board's attempt"),
            "{body}"
        );
    }
    // The history of a task holding another project's attempt is refused whole, never mixed.
    let (status, _, body) = s.ask("GET", &format!("/api/task-attempts?slug={OTHER}"), None);
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(s.kept(TASK), before);
}

#[test]
fn a_provider_not_held_here_still_takes_a_cancel_but_no_resume_or_view() {
    let s = Served::new();
    let id = s.admit(PROJECT, "elsewhere", TASK);
    // Its own worker, elsewhere, started it and saw it running.
    let elsewhere = s.worker(PROJECT, "elsewhere");
    assert!(matches!(
        elsewhere.step(&id).unwrap(),
        Worked::Answered { .. }
    ));
    assert!(matches!(
        elsewhere.step(&id).unwrap(),
        Worked::Answered { .. }
    ));
    let shown = s.shown(&id);
    assert_eq!(shown["configuration"], "elsewhere");
    assert_eq!(shown["controls"]["view"], false);
    assert_eq!(
        shown["controls"]["unserved"],
        "No resume or view here: its provider elsewhere is not one this server holds"
    );
    let (status, _, body) = s.ask(
        "POST",
        symphony_api::ATTEMPT_VIEW_ROUTE,
        Some(json!({"slug": TASK, "attempt": id})),
    );
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        said(&body).contains("elsewhere is not one this server holds"),
        "{body}"
    );
    let (status, body) = s.control(json!({"slug": TASK, "attempt": id,
        "revision": shown["revision"], "act": "resume", "mode": "new_resource"}));
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        said(&body).contains("a resume goes through its provider"),
        "{body}"
    );
    // A cancel is intent its own worker sends: recorded here all the same.
    let (status, body) = s.control(json!({"slug": TASK, "attempt": id,
        "revision": shown["revision"], "act": "cancel"}));
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["attempts"][0]["cancellation"], "requested");
    let calls = s.fake.calls().len();
    assert!(matches!(
        elsewhere.step(&id).unwrap(),
        Worked::Answered { .. }
    ));
    assert_eq!(s.fake.calls()[calls], Operation::Cancel);
}

#[test]
fn a_view_is_resolved_on_demand_for_the_local_owner_and_kept_nowhere() {
    let s = Served::new();
    let id = s.admit(PROJECT, CONFIGURATION, TASK);
    // Nothing bound yet: there is nothing to view.
    let view = json!({"slug": TASK, "attempt": id});
    let (status, _, body) = s.ask("POST", symphony_api::ATTEMPT_VIEW_ROUTE, Some(view.clone()));
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(matches!(s.step(&id), Outcome::Accepted(_)));
    let (status, headers, body) =
        s.ask("POST", symphony_api::ATTEMPT_VIEW_ROUTE, Some(view.clone()));
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({"url": "https://provider.invalid/res-1", "valid_for_ms": 10_000})
    );
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert!(s.fake.calls().contains(&Operation::ExternalLink));
    // Neither the board nor the history read keeps it.
    assert!(!s.kept(TASK).contains("provider.invalid"));
    let (_, _, read) = s.ask("GET", &format!("/api/task-attempts?slug={TASK}"), None);
    assert!(!read.to_string().contains("provider.invalid"), "{read}");
    // Read with GET it is no route; asked for another task's slug, or with a forged field, refused.
    let (status, _, _) = s.ask(
        "GET",
        &format!(
            "{}?slug={TASK}&attempt={id}",
            symphony_api::ATTEMPT_VIEW_ROUTE
        ),
        None,
    );
    assert_ne!(status, StatusCode::OK);
    let (status, _, body) = s.ask(
        "POST",
        symphony_api::ATTEMPT_VIEW_ROUTE,
        Some(json!({"slug": OTHER, "attempt": id})),
    );
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (status, _, body) = s.ask(
        "POST",
        symphony_api::ATTEMPT_VIEW_ROUTE,
        Some(json!({"slug": TASK, "attempt": id, "owner": "owner"})),
    );
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

/// The served project's routes under hosted admission, its visitor verified by the fixture host.
fn hosted(s: &Served) -> axum::Router {
    let project = symphony::project::Project::of(&s.shared.root).id.clone();
    assert_eq!(project, PROJECT);
    let (config, host) = crate::hosted::tests::native_host(&project);
    crate::hosted::router(s.shared.clone(), config, host)
        .unwrap()
        .layer(MockConnectInfo(
            "127.0.0.1:1234".parse::<std::net::SocketAddr>().unwrap(),
        ))
}

/// [`TASK`]'s history as `hosted` answers it.
fn hosted_history(s: &Served, hosted: &axum::Router) -> (StatusCode, Value) {
    let read = crate::hosted::tests::request(&format!("/api/task-attempts?slug={TASK}"));
    s.runtime.block_on(async {
        let response = hosted.clone().oneshot(read).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice::<Value>(&bytes).unwrap())
    })
}

#[test]
fn a_hosted_visitor_reads_the_history_with_no_control_and_posts_none_here() {
    let s = Served::new();
    let id = s.running();
    let hosted = hosted(&s);
    let (status, body) = hosted_history(&s, &hosted);
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["attempts"][0]["resource"], "res-1",
        "a provider's reference is shown"
    );
    assert_eq!(body["attempts"][0]["id"], json!(id));
    assert_eq!(
        body["attempts"][0]["controls"],
        serde_json::to_value(AttemptControls::default()).unwrap(),
        "a hosted visitor's controls are its host's"
    );
    // A post is its host's dispatch, never the native control: without the host's CSRF it is
    // refused, and nothing was asked of the attempt.
    let mut post = crate::hosted::tests::request(symphony_api::ATTEMPT_CONTROL_ROUTE);
    *post.method_mut() = axum::http::Method::POST;
    *post.body_mut() = Body::from(
        json!({"slug": TASK, "attempt": id, "revision": 0, "act": "cancel", "owner": "owner"})
            .to_string(),
    );
    let status = s
        .runtime
        .block_on(async { hosted.oneshot(post).await.unwrap().status() });
    assert!(status.is_client_error(), "{status}");
    assert_eq!(s.shown(&id)["cancellation"], "none");
}

#[test]
fn a_local_terminals_attempt_takes_no_control_here_and_a_hosted_read_hides_its_terminal() {
    let s = Served::new();
    let local = symphony_board::launch::attempts::CONFIGURATION;
    let id = s.admit(PROJECT, local, TASK);
    let launcher = s.worker(PROJECT, local);
    for _ in 0..2 {
        assert!(matches!(
            launcher.step(&id).unwrap(),
            Worked::Answered { .. }
        ));
    }
    let shown = s.shown(&id);
    assert_eq!(shown["dispatch"], "running");
    assert_eq!(shown["resource"], "res-1");
    assert_eq!(
        shown["controls"],
        json!({"cancel": false, "stop": [], "resume": [], "view": false,
            "unserved": "No control here: a local terminal, stepped by its launcher's own commands (close, restore)"})
    );
    let before = s.kept(TASK);
    for act in [
        json!({"act": "cancel"}),
        json!({"act": "stop", "mode": "terminate"}),
        json!({"act": "resume", "mode": "new_resource"}),
    ] {
        let mut asked = json!({"slug": TASK, "attempt": id, "revision": shown["revision"]});
        asked
            .as_object_mut()
            .unwrap()
            .extend(act.as_object().unwrap().clone());
        let (status, body) = s.control(asked);
        assert_eq!(status, StatusCode::CONFLICT, "{act}: {body}");
        assert!(said(&body).contains("a local terminal"), "{body}");
    }
    let (status, _, body) = s.ask(
        "POST",
        symphony_api::ATTEMPT_VIEW_ROUTE,
        Some(json!({"slug": TASK, "attempt": id})),
    );
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(s.kept(TASK), before, "nothing was asked of the terminal");
    // Its address and conversation are this machine's: a hosted read leaves them out.
    let (status, body) = hosted_history(&s, &hosted(&s));
    assert_eq!(status, StatusCode::OK, "{body}");
    let read = &body["attempts"][0];
    assert_eq!(read["id"], json!(id));
    assert_eq!(
        (
            &read["resource"],
            &read["generation"],
            &read["conversation"]
        ),
        (&Value::Null, &Value::Null, &Value::Null),
        "{read}"
    );
    assert!(!body.to_string().contains("res-1"), "{body}");
}

#[test]
fn the_scope_is_the_projects_id_as_the_launcher_names_it_whatever_the_file_leaves_out() {
    // A file that names the project but no id: its pages are served under the name's slug, while
    // the board's commands name it by its main checkout's folder, and so do its attempts.
    let s = Served::with_file("[project]\nname = \"Proj Name\"\n");
    let root = s.shared.root.clone();
    assert_eq!(symphony::project::Project::of(&root).id, "proj-name");
    let launched = symphony_board::project::find(&root, None).unwrap().id;
    assert_eq!(launched, "main", "the repository's folder");
    let id = s.admit(&launched, CONFIGURATION, TASK);
    assert_eq!(s.history(TASK)[0]["id"], json!(id));
    // One admitted under the served id is another project's here.
    let other = s.admit("proj-name", CONFIGURATION, OTHER);
    let (status, _, body) = s.ask("GET", &format!("/api/task-attempts?slug={OTHER}"), None);
    assert_eq!(status, StatusCode::NOT_FOUND, "{other}: {body}");
}

#[test]
fn a_hosted_askers_grant_is_its_verified_principal_on_the_one_task() {
    let actor = crate::hosted::Actor {
        issuer: "https://issuer.example".into(),
        subject: "visitor-1".into(),
        tenant: None,
    };
    let asker = Asker::hosted(&actor, "policy-7");
    let scope = Scope::new("proj".into(), "board".into());
    let grant = asker.grant(&scope, TASK, &[Act::Read]);
    assert_eq!(grant.policy_revision(), "policy-7");
    assert_eq!(
        grant.principal(),
        &Principal::new(
            PrincipalKind::Human,
            "https://issuer.example".into(),
            "visitor-1".into()
        )
    );
    let local = Asker::Local.grant(&scope, TASK, &[Act::Cancel]);
    assert_eq!(local.principal(), &symphony_board::workflow::local_owner());
    assert_eq!(local.policy_revision(), POLICY);
}

#[test]
fn each_refusal_of_the_coordinator_is_answered_with_its_status() {
    let status = |refusal| refused(refusal).0;
    assert_eq!(
        status(Refusal::Unauthorized("no".into())),
        StatusCode::FORBIDDEN
    );
    assert_eq!(status(Refusal::Scope), StatusCode::NOT_FOUND);
    assert_eq!(status(Refusal::Unknown("ab".into())), StatusCode::NOT_FOUND);
    assert_eq!(status(Refusal::Busy(None)), StatusCode::CONFLICT);
    assert_eq!(status(Refusal::Full(Vec::new())), StatusCode::CONFLICT);
    assert_eq!(status(Refusal::Conflict("k".into())), StatusCode::CONFLICT);
    assert_eq!(status(Refusal::Invalid("x".into())), StatusCode::CONFLICT);
    assert_eq!(
        status(Refusal::Unavailable("down".into())),
        StatusCode::BAD_GATEWAY
    );
    let board = refused(Refusal::Board(symphony_board::Error::Db("lost".into())));
    assert_eq!(board.0, StatusCode::SERVICE_UNAVAILABLE, "{board:?}");
}
