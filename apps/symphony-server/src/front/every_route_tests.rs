//! Every route the server has takes a request for who a gate marked it as ([`Admitted`]), and for
//! no one else: every save (the app's, `symphony::save::ROUTES`, and the server's own,
//! `crate::write::ROUTES`) and every read (the router's own `/api/front` and `crate::api::READS`).
//! A hosted request is asked of local code as hosted admission marks it, from this very machine,
//! where the local gate takes a request for the person at it; a request no gate marked is handed
//! to the routes past every gate. A route added to either table is asked here too.

use crate::front::{Admitted, Shared, router};
use crate::testing::board::{Fixture, Seed};
use crate::testing::{Tree, repository_holding, store_on};
use axum::body::{Body, to_bytes};
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, StatusCode, header};
use std::net::SocketAddr;
use std::sync::Arc;
use tower::ServiceExt;

/// This machine, where an adapter running beside the server sends from.
const HERE: &str = "127.0.0.1:5000";
/// An address the local gate refuses.
const AWAY: &str = "203.0.113.9:5000";
const HOST: &str = "127.0.0.1:8123";
const PAGE: &str =
    "# Topic\n\n<!-- register:start -->\n- [ ] **one** — A question.\n<!-- register:end -->\n";
/// A value for each argument a read requires, so that each read reaches its own answer.
const ARGUMENTS: &str = "slug=tools/visible&path=docs/TOPIC.md&handle=worker&since=0&run=r1\
                         &slugs=%5B%22tools%2Fvisible%22%5D";
/// A save's body with what any save asks, so that none is refused for its body first.
const SAVE: &str = r#"{"path":"docs/TOPIC.md","slug":"one","state":"approved","text":"Mine?"}"#;

/// A status, and the body unless the answer is a stream, which never ends.
type Answer = (StatusCode, Option<Vec<u8>>);

/// The project of one document, served by a server of its own on port 8123 behind the open gate.
fn served(tree: &Tree) -> (std::path::PathBuf, Arc<Shared>) {
    let root = repository_holding(tree, "main", "docs/TOPIC.md", PAGE);
    let shared = Shared::new(1, 8123, &root, None, crate::testing::open_gate());
    (root, shared)
}

/// What `shared`'s router answers `request` sent from `peer`.
async fn sent(shared: &Arc<Shared>, peer: &str, request: Request<Body>) -> Answer {
    let app = router(shared.clone()).layer(MockConnectInfo(peer.parse::<SocketAddr>().unwrap()));
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let stream = (response.headers().get(header::CONTENT_TYPE))
        .is_some_and(|kind| kind.as_bytes().starts_with(b"text/event-stream"));
    if stream {
        return (status, None);
    }
    (status, Some(bytes(response).await))
}

/// A read of `route` with every argument, marked `mark` as a gate would have, or marked by none.
fn read(route: &str, mark: Option<Admitted>) -> Request<Body> {
    let mut request = Request::get(format!("{route}?{ARGUMENTS}"))
        .header(header::HOST, HOST)
        .body(Body::empty())
        .unwrap();
    if let Some(mark) = mark {
        request.extensions_mut().insert(mark);
    }
    request
}

/// A save to `route` from this server's own page, of [`SAVE`].
fn save(route: &str) -> Request<Body> {
    Request::post(route)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("http://{HOST}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(SAVE))
        .unwrap()
}

/// Every save route: the app's and the server's own.
fn saves() -> impl Iterator<Item = &'static str> {
    (symphony::save::ROUTES.iter())
        .chain(&crate::write::ROUTES)
        .copied()
}

/// The bytes `response` carries.
async fn bytes(response: axum::response::Response) -> Vec<u8> {
    to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec()
}

/// Every save, the upload's, the agent focus's and an attempt's included, refuses a hosted visitor
/// before anything else, as its host's to dispatch, and nothing is written: none is taken for the
/// person at the machine, who holds every power.
#[tokio::test(flavor = "multi_thread")]
async fn every_save_refuses_a_hosted_visitor_first_and_writes_nothing() {
    let tree = Tree::new();
    let (root, shared) = served(&tree);
    let refused = serde_json::json!({ "error": crate::write::HOSTED_SAVE });
    for route in saves() {
        let mut request = save(route);
        (request.extensions_mut()).insert(crate::hosted::tests::admitted().await);
        let (status, said) = sent(&shared, HERE, request).await;
        let said: serde_json::Value = serde_json::from_slice(&said.unwrap()).unwrap();
        assert_eq!(
            (status, said),
            (StatusCode::FORBIDDEN, refused.clone()),
            "{route}"
        );
    }
    assert!(shared.saves.get().is_none(), "no save was started");
    let kept = std::fs::read_to_string(root.join("docs/TOPIC.md")).unwrap();
    assert_eq!(kept, PAGE);
    shared.stop();
}

/// Every read answers a hosted visitor or refuses them, and never as the person at the machine: a
/// read that answers the person at the machine and refuses a person the board named under the
/// machine's own name refuses a hosted visitor too. Where the request came from changes nothing it
/// is answered, and no read fails for a hosted visitor that answers the person at the machine.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_read_answers_a_hosted_visitor_as_one_or_refuses_never_as_the_machine() {
    let tree = Tree::new();
    let (root, shared) = served(&tree);
    let board = Fixture::new(
        Seed::default()
            .session("worker")
            .task("tools/visible", "in_progress")
            .role("tools/visible", "worker", "executor"),
    );
    let mut hub = crate::live::Hub::new(&root, &root);
    let client = board.other_database();
    hub.connect_reader = Box::new(move |_| Ok(store_on(&client)));
    hub.refresh_page("", &root).unwrap();
    assert!(shared.api.set(Arc::new(hub)).is_ok());
    let person = (shared.gate)
        .member(&shared.root, &crate::gate::Visitor::Machine)
        .unwrap();
    let hosted = async || Some(crate::hosted::tests::admitted().await);
    let reads = std::iter::once(symphony_api::FRONT_PATH).chain(crate::api::READS.iter().copied());
    for route in reads {
        let machine = sent(&shared, HERE, read(route, None)).await;
        let named = Some(Admitted::Named {
            actor: person.clone(),
            origin: None,
        });
        let named = sent(&shared, HERE, read(route, named)).await;
        let here = sent(&shared, HERE, read(route, hosted().await)).await;
        let again = sent(&shared, HERE, read(route, hosted().await)).await;
        let away = sent(&shared, AWAY, read(route, hosted().await)).await;
        assert!(
            !here.0.is_server_error() || machine.0 == here.0,
            "{route}: {} to a hosted visitor, {} to the person at the machine",
            here.0,
            machine.0
        );
        assert_eq!(here.0, away.0, "{route}: where it came from");
        if here == again {
            assert_eq!(here, away, "{route}: where it came from");
        }
        if named.0.is_client_error() && !machine.0.is_client_error() {
            assert!(
                here.0.is_client_error(),
                "{route}: {} to a hosted visitor, as to the person at the machine",
                here.0
            );
        }
    }
    shared.stop();
}

/// A request no gate marked is no one's: handed to every read and every save past every gate, it
/// has no actor and is not the machine's, and each refuses it before anything else, the hub and
/// the saves never started for it.
#[tokio::test]
async fn a_request_no_gate_marked_is_refused_by_every_read_and_every_save() {
    let tree = Tree::new();
    let (_root, shared) = served(&tree);
    let refused = bytes(crate::front::unmarked()).await;
    for route in crate::api::READS {
        let request = Request::get(format!("{route}?{ARGUMENTS}"))
            .header(header::HOST, HOST)
            .body(())
            .unwrap();
        let response = crate::api::answer(&shared, request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{route}");
        assert_eq!(bytes(response).await, refused, "{route}");
    }
    for route in saves() {
        let response = crate::write::answer(&shared, save(route)).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{route}");
        assert_eq!(bytes(response).await, refused, "{route}");
    }
    assert!(shared.api.get().is_none(), "no hub was started");
    assert!(shared.saves.get().is_none(), "no save was started");
    shared.stop();
}

/// The marks say who: the person at the machine alone is the machine's, a named person and the
/// person a sign-in service names keep their actor and the origin their word was made for, and a
/// hosted visitor and a request no gate marked have no actor and are not the machine's.
#[tokio::test]
async fn each_mark_says_its_actor_whether_it_is_the_machines_and_its_origin() {
    let actor = symphony_board::workflow::local_actor();
    let signed = "https://board.example.com".to_string();
    let marks = [
        (Some(Admitted::Machine(actor.clone())), true, true, None),
        (
            Some(Admitted::Named {
                actor: actor.clone(),
                origin: None,
            }),
            true,
            false,
            None,
        ),
        (
            Some(Admitted::Named {
                actor: actor.clone(),
                origin: Some(signed.clone()),
            }),
            true,
            false,
            Some(signed),
        ),
        (
            Some(crate::hosted::tests::admitted().await),
            false,
            false,
            None,
        ),
        (None, false, false, None),
    ];
    for (mark, acts, machine, origin) in marks {
        let mut request = Request::get("/").body(()).unwrap();
        let hosted = matches!(mark, Some(Admitted::Hosted(_)));
        if let Some(mark) = mark {
            request.extensions_mut().insert(mark);
        }
        let said = (
            Admitted::actor(request.extensions()),
            Admitted::machine(request.extensions()),
            Admitted::origin(request.extensions()),
            crate::hosted::admission(&request).is_some(),
        );
        assert_eq!(
            said,
            (acts.then(|| actor.clone()), machine, origin, hosted),
            "{hosted} {machine}"
        );
    }
}

/// The local gate marks the person at the machine as theirs and anyone else a board added as
/// named, a signed visitor with the origin their word was made for: never as a hosted visitor.
#[test]
fn a_visitor_is_marked_the_machines_or_named_by_the_gate() {
    use crate::gate::Visitor;
    let actor = symphony_board::workflow::local_actor();
    let machine = Admitted::of(&Visitor::Machine, actor.clone());
    assert!(matches!(machine, Admitted::Machine(ref marked) if *marked == actor));
    let node = Admitted::of(&Visitor::Node("tagged".into()), actor.clone());
    assert!(matches!(node, Admitted::Named { origin: None, .. }));
    let signed = symphony::signed_in::SignedIn {
        service: "google".into(),
        subject: "1".into(),
        display: None,
        origin: "https://board.example.com".into(),
    };
    let signed = Admitted::of(&Visitor::Signed(signed), actor);
    assert!(matches!(
        signed,
        Admitted::Named { origin: Some(ref origin), .. } if origin == "https://board.example.com"
    ));
}
