//! The signed word through a server of its own, every request judged before any route
//! (docs/operations/SIGNED_IN.md): a word the machine folder's key verifies, fresh and made for its
//! origin, is the person whose sign-in it names, on reads and writes alike, held to the board's
//! powers, whatever address the service reaches the server from; every other word is refused with
//! nothing written, and so is any word where no key is set. A request with no word is judged as it
//! always was.

use crate::front::{Shared, router};
use crate::testing::{Board, SignIn, Tree, open_gate, repository_holding};
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, StatusCode, header};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use symphony_board::people::ownership::Place;
use symphony_board::people::{Power, Profile, Profiles, Signin};
use tower::ServiceExt;

const PAGE: &str =
    "# Topic\n\n<!-- register:start -->\n- [ ] **one** — proposal.\n<!-- register:end -->\n";
const DECISION: &str = r#"{"path":"docs/TOPIC.md","slug":"one","state":"question","note":"Which one?","actor":"mallory"}"#;
/// Ana's subject at Google, which her profile's sign-in names.
const SUBJECT: &str = "110248495921238986420";
/// A sign-in service on another machine, outside the tailnet.
const AWAY: &str = "10.0.0.5:5000";
/// A sign-in service on this machine, which reaches the server by loopback.
const HERE: &str = "127.0.0.1:5000";
const REFUSED: &str = "Forbidden: this request's signed sign-in is not taken\n";

/// A server of its own over a project whose board gives Ana `powers`, in a group whose cards are
/// hers to decide as those powers say, and a machine folder holding her profile, signed in by
/// Google, and naming the service's key when `keyed`.
struct Served {
    _tree: Tree,
    root: PathBuf,
    shared: Arc<Shared>,
    _board: Board,
    service: SignIn,
}

impl Served {
    fn new(powers: &[Power], keyed: bool) -> Served {
        let tree = Tree::new();
        let root = repository_holding(&tree, "main", "docs/TOPIC.md", PAGE);
        let board = Board::holding(&root, &[("ana", powers)]);
        let store = Board::store(&root);
        store.add_group("staff", &[], "owner").unwrap();
        store.join_group("ana", "staff", "owner").unwrap();
        store.map_place(&Place::Default, "staff", "owner").unwrap();
        let machine = tree.root.join("machine");
        Profiles::change(&machine, |profiles| {
            profiles.people.push(Profile {
                name: "ana".into(),
                display: "Ana Lima".into(),
                signins: vec![Signin {
                    service: "google".into(),
                    subject: SUBJECT.into(),
                }],
                ..Profile::default()
            });
            Ok(())
        })
        .unwrap();
        let service = SignIn::new(1);
        if keyed {
            service.named_in(&machine);
        }
        let shared = Shared::new(1, 8123, &root, None, open_gate().with_machine(machine));
        let hub = crate::live::Hub::new(&root, &root.join("build/client"));
        assert!(shared.api.set(Arc::new(hub)).is_ok());
        Served {
            _tree: tree,
            root,
            shared,
            _board: board,
            service,
        }
    }

    /// Ana's word, fresh.
    fn ana(&self) -> String {
        self.service.word("google", SUBJECT)
    }

    /// What the server answers `request` from `client`: its status and body.
    async fn answer(&self, client: &str, request: Request<Body>) -> (StatusCode, String) {
        let app = router(self.shared.clone())
            .layer(MockConnectInfo(client.parse::<SocketAddr>().unwrap()));
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    /// The card's question saved from `client`, naming `host` and, from a browser, `origin`, with
    /// `words` in the header: its payload names another actor, which no save takes.
    async fn decide(
        &self,
        client: &str,
        host: &str,
        origin: Option<&str>,
        words: &[&str],
    ) -> (StatusCode, String) {
        let mut request = Request::post("/api/decision")
            .header(header::HOST, host)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(origin) = origin {
            request = request.header(header::ORIGIN, origin);
        }
        for word in words {
            request = request.header(symphony::signed_in::HEADER, *word);
        }
        self.answer(client, request.body(Body::from(DECISION)).unwrap())
            .await
    }

    /// Ana's question saved through the service from `client`, with `word`.
    async fn decide_with(&self, client: &str, word: &str) -> (StatusCode, String) {
        self.decide(client, SignIn::HOST, Some(SignIn::ORIGIN), &[word])
            .await
    }

    /// The People page's read from `client` through the service with `word`: who it reads as.
    async fn people(&self, client: &str, word: &str) -> (StatusCode, serde_json::Value) {
        let request = Request::get(symphony_api::PEOPLE_PATH)
            .header(header::HOST, SignIn::HOST)
            .header(symphony::signed_in::HEADER, word)
            .body(Body::empty())
            .unwrap();
        let (status, body) = self.answer(client, request).await;
        (status, serde_json::from_str(&body).unwrap_or_default())
    }

    fn page(&self) -> String {
        if let Some(saves) = self.shared.saves.get() {
            saves.close();
        }
        std::fs::read_to_string(self.root.join("docs/TOPIC.md")).unwrap()
    }
}

/// Ana's word, verified, makes each request hers wherever the service runs: a read shows her the
/// board as herself, and a save is written under her short name, never the machine's person's nor
/// the one its payload names.
#[tokio::test(flavor = "multi_thread")]
async fn a_word_the_key_signed_is_the_person_whose_sign_in_it_names_on_reads_and_writes() {
    let served = Served::new(&[Power::Decide], true);
    for client in [AWAY, HERE] {
        let (status, page) = served.people(client, &served.ana()).await;
        assert_eq!(status, StatusCode::OK, "{client}: {page}");
        assert_eq!(page["you"], "ana", "{client}: {page}");
        assert_eq!(page["machine"], false, "{client}: {page}");
    }
    let (status, body) = served.decide_with(AWAY, &served.ana()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let page = served.page();
    assert!(
        page.lines()
            .any(|line| line.ends_with("] ana · question — Which one?")),
        "{page}"
    );
    assert!(
        !page.contains("owner ·") && !page.contains("mallory"),
        "{page}"
    );
}

/// Through a service on this machine too, which reaches the server by loopback: the word, not the
/// address, says who the request is from.
#[tokio::test(flavor = "multi_thread")]
async fn a_word_from_loopback_is_its_person_not_the_machines() {
    let served = Served::new(&[Power::Decide], true);
    let (status, body) = served.decide_with(HERE, &served.ana()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let page = served.page();
    assert!(page.contains("] ana · question"), "{page}");
    assert!(!page.contains("owner ·"), "{page}");
}

/// A signed person is held to the board's powers as any named person: one given speak alone is
/// refused a decision, naming the power, and nothing is written.
#[tokio::test(flavor = "multi_thread")]
async fn a_signed_person_is_refused_a_write_whose_power_the_board_does_not_give_them() {
    let served = Served::new(&[Power::Speak], true);
    let (status, body) = served.decide_with(AWAY, &served.ana()).await;
    assert_eq!(
        (status, body.as_str()),
        (
            StatusCode::FORBIDDEN,
            r#"{"error":"Forbidden: ana may not decide on this board"}"#
        )
    );
    assert_eq!(served.page(), PAGE);
}

/// Every word but a good one is refused before any route, alike, saying nothing of why, and
/// nothing is written: from a service away and from one on this machine, which a refused word
/// never makes the machine's person.
#[tokio::test(flavor = "multi_thread")]
async fn every_word_that_does_not_verify_is_refused_alike_and_nothing_is_written() {
    let served = Served::new(&[Power::Decide], true);
    let good = served.ana();
    let parts: Vec<&str> = good.split('.').collect();
    let tampered = {
        use base64::Engine as _;
        let bob = SignIn::claims("google", "bob", -5, 300).to_string();
        let part = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bob);
        format!("{}.{part}.{}", parts[0], parts[2])
    };
    let claims = |service, issued, life, origin: &str| {
        let mut claims = SignIn::claims(service, SUBJECT, issued, life);
        claims["aud"] = origin.into();
        claims
    };
    let words = [
        ("unsigned", format!("{}.{}.", parts[0], parts[1])),
        ("tampered", tampered),
        (
            "stale",
            served
                .service
                .signs(&claims("google", -400, 300, SignIn::ORIGIN)),
        ),
        (
            "not yet valid",
            served
                .service
                .signs(&claims("google", 120, 300, SignIn::ORIGIN)),
        ),
        (
            "held too long",
            served
                .service
                .signs(&claims("google", -5, 3600, SignIn::ORIGIN)),
        ),
        (
            "for another server",
            served
                .service
                .signs(&claims("google", -5, 300, "https://other.example.com")),
        ),
        ("by another key", SignIn::new(2).word("google", SUBJECT)),
        (
            "as the daemon",
            served
                .service
                .signs(&claims("tailscale", -5, 300, SignIn::ORIGIN)),
        ),
        (
            "oversized",
            format!("{good}{}", "a".repeat(symphony::signed_in::MAX_BYTES)),
        ),
        ("malformed", "not-a-word".to_string()),
    ];
    for client in [AWAY, HERE] {
        for (what, word) in &words {
            let (status, body) = served.decide_with(client, word).await;
            assert_eq!(
                (status, body.as_str()),
                (StatusCode::FORBIDDEN, REFUSED),
                "{client}: {what}"
            );
            let (status, _) = served.people(client, word).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{client}: {what}, read");
        }
        let (status, body) = served
            .decide(client, SignIn::HOST, Some(SignIn::ORIGIN), &[&good, &good])
            .await;
        assert_eq!(
            (status, body.as_str()),
            (StatusCode::FORBIDDEN, REFUSED),
            "{client}: two"
        );
    }
    assert_eq!(served.page(), PAGE);
}

/// A word that verifies and names no sign-in a profile holds is nobody's: a server of its own
/// refuses it as an address the gate refuses, as a login no profile signs in.
#[tokio::test(flavor = "multi_thread")]
async fn a_word_naming_nobody_the_profiles_hold_is_refused_as_the_gate_refuses() {
    let served = Served::new(&[Power::Decide], true);
    for word in [
        served.service.word("google", "999"),
        served.service.word("github", SUBJECT),
    ] {
        let (status, body) = served.decide_with(AWAY, &word).await;
        assert_eq!(
            (status, body.as_str()),
            (
                StatusCode::FORBIDDEN,
                "Forbidden: the Symphony answers only this machine and its tailnet\n"
            )
        );
    }
    assert_eq!(served.page(), PAGE);
}

/// Where no key is set, a word is refused, from loopback too, rather than taken as the machine's
/// person; a request with none is judged by its address as it always was.
#[tokio::test(flavor = "multi_thread")]
async fn where_no_key_is_set_a_word_is_refused_and_a_request_without_one_is_as_before() {
    let served = Served::new(&[Power::Decide], false);
    for client in [HERE, AWAY] {
        let (status, body) = served.decide_with(client, &served.ana()).await;
        assert_eq!(
            (status, body.as_str()),
            (StatusCode::FORBIDDEN, REFUSED),
            "{client}"
        );
    }
    assert_eq!(served.page(), PAGE);
    let (status, body) = served
        .decide(AWAY, "127.0.0.1:8123", Some("http://127.0.0.1:8123"), &[])
        .await;
    assert_eq!(
        (status, body.as_str()),
        (
            StatusCode::FORBIDDEN,
            "Forbidden: the Symphony answers only this machine and its tailnet\n"
        )
    );
    let (status, body) = served
        .decide(HERE, "127.0.0.1:8123", Some("http://127.0.0.1:8123"), &[])
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(served.page().contains("] owner · question"));
}

/// The service's origin is a name a signed request alone may give: one with no word naming it in
/// its Host is refused as a Host that names no name of this machine, from loopback too, and a
/// signed request's write from another site's page is refused by its Origin.
#[tokio::test(flavor = "multi_thread")]
async fn only_a_signed_request_may_name_the_services_origin_and_only_its_own_page_writes() {
    let served = Served::new(&[Power::Decide], true);
    let (status, body) = served
        .decide(HERE, SignIn::HOST, Some(SignIn::ORIGIN), &[])
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let request = Request::get(symphony_api::PEOPLE_PATH)
        .header(header::HOST, SignIn::HOST)
        .body(Body::empty())
        .unwrap();
    let (status, body) = served.answer(HERE, request).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    for origin in [
        "https://evil.example.com",
        "http://board.example.com",
        "null",
    ] {
        let (status, _) = served
            .decide(AWAY, SignIn::HOST, Some(origin), &[&served.ana()])
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{origin}");
    }
    assert_eq!(served.page(), PAGE);
    // A service that names the server by loopback in its Host passes the browser's Origin on.
    let (status, body) = served
        .decide(
            HERE,
            "127.0.0.1:8123",
            Some(SignIn::ORIGIN),
            &[&served.ana()],
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // And a client that is no browser sends no Origin.
    let (status, body) = served
        .decide(AWAY, SignIn::HOST, None, &[&served.ana()])
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// A request's Host names a signed visitor's origin by its host and port, as a browser sends them,
/// in any case; and the page a signed request's Origin may name is that origin, alone under the
/// service's name, and beside a page of this server's own under one of this machine's names.
#[test]
fn the_service_is_named_by_its_origins_host_and_its_page_by_that_origin() {
    use crate::front::{names_origin, own_origin};
    assert!(names_origin(SignIn::ORIGIN, "board.example.com"));
    assert!(names_origin(SignIn::ORIGIN, "Board.Example.COM"));
    assert!(names_origin("http://board.lan:8080", "board.lan:8080"));
    for other in [
        "board.example.com:443",
        "other.example.com",
        "",
        "https://board.example.com",
    ] {
        assert!(!names_origin(SignIn::ORIGIN, other), "{other}");
    }
    let signed = Some(SignIn::ORIGIN);
    assert!(own_origin(SignIn::ORIGIN, SignIn::HOST, signed));
    assert!(own_origin(
        "HTTPS://BOARD.example.com",
        SignIn::HOST,
        signed
    ));
    assert!(!own_origin(
        "http://board.example.com",
        SignIn::HOST,
        signed
    ));
    // With no word it is today's rule, Origin against Host: such a Host is refused before it.
    assert!(own_origin("http://board.example.com", SignIn::HOST, None));
    assert!(own_origin(SignIn::ORIGIN, "127.0.0.1:8123", signed));
    assert!(own_origin(
        "http://127.0.0.1:8123",
        "127.0.0.1:8123",
        signed
    ));
    assert!(own_origin("http://127.0.0.1:8123", "127.0.0.1:8123", None));
    assert!(!own_origin(SignIn::ORIGIN, "127.0.0.1:8123", None));
    assert!(!own_origin(
        "https://evil.example.com",
        "127.0.0.1:8123",
        signed
    ));
    assert!(!own_origin("http://", "", None));
}

/// A signed visitor whose service runs on this machine reaches the server by loopback, and is not
/// this machine's for Go to session: only the person at the machine's request may show a pane here.
#[tokio::test(flavor = "multi_thread")]
async fn a_signed_visitor_from_loopback_is_not_this_machine_for_a_pane() {
    let served = Served::new(&[Power::Decide], true);
    let asked = |machine: bool, origin: Option<String>| {
        let mut request = crate::testing::request("/api/agents");
        request.extensions_mut().insert(axum::extract::ConnectInfo(
            HERE.parse::<SocketAddr>().unwrap(),
        ));
        request.extensions_mut().insert(crate::front::Admitted {
            actor: symphony_board::workflow::local_actor(),
            machine,
            origin,
        });
        request
    };
    let signed = asked(false, Some(SignIn::ORIGIN.into()));
    assert!(!crate::session_go::local(&served.shared, &signed).await);
    let machine = asked(true, None);
    assert!(crate::session_go::local(&served.shared, &machine).await);
}
