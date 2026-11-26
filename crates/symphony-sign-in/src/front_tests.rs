//! The service in front of a server whose machine folder's `sign-in.toml` names its key, every word
//! it is passed verified by the server's own check (`stand_in::Server`), and Google's token
//! endpoint standing in: a visitor with no session is sent to sign in, and once back is the person
//! whose Google subject the word names, on reads, saves, uploads, the feed's stream and its socket;
//! nothing is passed on without a session; every way back but the browser's own good one is
//! refused, with no session made; sign-out ends the session; and nothing the log says names a
//! secret.

use super::*;
use crate::kept::{STALE, UNBOUND, UNKNOWN};
use crate::stand_in::{
    BACKSTOP, CLIENT_ID, CLIENT_SECRET, CODE, Google, ORIGIN, SEED, SUBJECT, Server, claims,
    folder, key, now, send, server_names, settings, text,
};
use hyper::Method;
use std::collections::BTreeMap;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

/// A request asked: its method, its path and its headers.
type Asked = (
    Method,
    &'static str,
    &'static [(&'static str, &'static str)],
);

/// Ana's sign-in, as the server's check reads the word.
const ANA: &str = "google:110248495921238986420";

/// A service in front of a stand-in server, signing visitors in at a stand-in Google.
struct Through {
    _machine: crate::stand_in::Folder,
    google: Google,
    server: Server,
    address: std::net::SocketAddr,
    front: Arc<Front>,
    origin: String,
    log: Arc<Mutex<Vec<u8>>>,
}

/// A sign-in begun: what the visitor's redirect to Google says, and the flow cookie it set.
struct Begun {
    location: String,
    query: BTreeMap<String, String>,
    /// `NAME=VALUE`, as the browser sends it back.
    cookie: String,
    set_cookie: String,
}

impl Begun {
    fn state(&self) -> &str {
        &self.query["state"]
    }
}

impl Through {
    async fn new() -> Through {
        Through::with(ORIGIN, None).await
    }

    /// For `origin`, its streams ended after `streams` where one is given.
    async fn with(origin: &str, streams: Option<Duration>) -> Through {
        let machine = folder("front");
        server_names(&machine, origin);
        let google = Google::new().await;
        let server = Server::new(&machine).await;
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut front = Front::new(
            settings(server.address, origin),
            key(),
            google.provider(),
            log.clone(),
        )
        .unwrap();
        if let Some(streams) = streams {
            front = front.streams(streams);
        }
        let (front, address) = listening(front).await;
        Through {
            _machine: machine,
            google,
            server,
            address,
            front,
            origin: origin.into(),
            log,
        }
    }

    /// The answer to `method` on `path` with `headers` and `body`.
    async fn ask(
        &self,
        method: Method,
        path: &str,
        headers: &[(&str, &str)],
        body: impl Into<Bytes>,
    ) -> Response<Bytes> {
        let mut request = Request::builder().method(method).uri(path);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        send(self.address, request.body(text(body)).unwrap()).await
    }

    async fn get(&self, path: &str, headers: &[(&str, &str)]) -> Response<Bytes> {
        self.ask(Method::GET, path, headers, "").await
    }

    /// A browser's navigation to `path` with no session, sent to Google.
    async fn begin(&self, path: &str) -> Begun {
        let response = self.get(path, &[("sec-fetch-mode", "navigate")]).await;
        assert_eq!(response.status(), StatusCode::FOUND, "{path}");
        let location = header_of(&response, header::LOCATION).unwrap();
        let (_, query) = location.split_once('?').unwrap();
        let query = form_urlencoded::parse(query.as_bytes())
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect();
        let set_cookie = cookies(&response).into_iter().next().unwrap();
        let cookie = set_cookie.split(';').next().unwrap().to_string();
        Begun {
            location,
            query,
            cookie,
            set_cookie,
        }
    }

    /// Google's way back with `query`, from a browser sending `cookie`.
    async fn back(&self, query: &str, cookie: Option<&str>) -> Response<Bytes> {
        let headers: Vec<(&str, &str)> = cookie
            .map(|cookie| ("cookie", cookie))
            .into_iter()
            .collect();
        self.get(&format!("{CALLBACK}?{query}"), &headers).await
    }

    /// The way back for `begun` with Google's code, from the browser that began it, with the
    /// parameters Google adds and the service does not read.
    async fn back_for(&self, begun: &Begun) -> Response<Bytes> {
        self.back(
            &format!(
                "state={}&code={}&scope=openid&authuser=0&prompt=none",
                begun.state(),
                enc(CODE)
            ),
            Some(&begun.cookie),
        )
        .await
    }

    /// Google, from now, answering `begun`'s exchange with an id token of `claims`.
    fn answers(&self, begun: &Begun, claims: &serde_json::Value) {
        self.google
            .signs_in(&begun.query["code_challenge"], &self.redirect(), claims);
    }

    /// A whole sign-in from a navigation to `path`: the session cookie, `NAME=VALUE`.
    async fn sign_in(&self, path: &str) -> String {
        let begun = self.begin(path).await;
        self.answers(&begun, &claims(&begun.query["nonce"]));
        let response = self.back_for(&begun).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "{}", self.log());
        let session = cookies(&response).into_iter().next().unwrap();
        session.split(';').next().unwrap().to_string()
    }

    fn redirect(&self) -> String {
        format!("{}{CALLBACK}", self.origin)
    }

    fn log(&self) -> String {
        String::from_utf8(self.log.lock().unwrap().clone()).unwrap()
    }
}

/// `front` listening on a private loopback port, and where.
async fn listening(front: Front) -> (Arc<Front>, std::net::SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let front = Arc::new(front);
    tokio::spawn(Arc::clone(&front).listen(listener));
    (front, address)
}

/// Ana, as a session holds her.
fn ana() -> Who {
    Who {
        service: google::SERVICE,
        subject: SUBJECT.into(),
    }
}

/// The value of `name` in `response`.
fn header_of(response: &Response<Bytes>, name: HeaderName) -> Option<String> {
    response
        .headers()
        .get(name)
        .map(|value| value.to_str().unwrap().to_string())
}

/// Every `Set-Cookie` of `response`, in order.
fn cookies(response: &Response<Bytes>) -> Vec<String> {
    (response.headers().get_all(header::SET_COOKIE).iter())
        .map(|value| value.to_str().unwrap().to_string())
        .collect()
}

/// `text` form-encoded.
fn enc(text: &str) -> String {
    form_urlencoded::byte_serialize(text.as_bytes()).collect()
}

fn body(response: &Response<Bytes>) -> String {
    String::from_utf8_lossy(response.body()).into_owned()
}

/// A browser's navigation with no session is sent to Google's authorization endpoint for the
/// client, with the way back the client registered, the scope `openid` alone, and a fresh state,
/// nonce and S256 challenge, and its browser holds the state in a host-only, secure, `HttpOnly`,
/// Lax flow cookie; no session is made, and nothing reaches the server.
#[tokio::test(flavor = "multi_thread")]
async fn a_navigation_with_no_session_is_sent_to_google_with_a_fresh_state_nonce_and_challenge() {
    let through = Through::new().await;
    let begun = through.begin("/docs/TOPIC?x=1").await;
    assert!(
        begun
            .location
            .starts_with("https://accounts.google.com/o/oauth2/v2/auth?"),
        "{}",
        begun.location
    );
    let query = &begun.query;
    assert_eq!(query["response_type"], "code");
    assert_eq!(query["client_id"], CLIENT_ID);
    assert_eq!(query["redirect_uri"], format!("{ORIGIN}/.sign-in/callback"));
    assert_eq!(query["scope"], "openid");
    assert_eq!(query["code_challenge_method"], "S256");
    for name in ["state", "nonce", "code_challenge"] {
        assert_eq!(query[name].len(), 43, "{name}: {}", query[name]);
    }
    assert_eq!(
        begun.set_cookie,
        format!(
            "__Host-symphony-sign-in-flow={}; Max-Age=600; Path=/; HttpOnly; SameSite=Lax; Secure",
            begun.state()
        )
    );
    let again = through.begin("/docs/TOPIC?x=1").await;
    for name in ["state", "nonce", "code_challenge"] {
        assert_ne!(again.query[name], begun.query[name], "{name}");
    }
    assert!(through.server.heard().is_empty());
}

/// Without a session nothing is passed on: a fetch, a write, a socket's handshake and a request
/// for anything but HTML are answered 401, never sent to sign in; a request for HTML from a
/// browser that sends no fetch metadata is a navigation.
#[tokio::test(flavor = "multi_thread")]
async fn anything_but_a_navigation_with_no_session_is_refused_and_never_passed_on() {
    let through = Through::new().await;
    let refused: [Asked; 5] = [
        (Method::GET, "/api/people", &[("sec-fetch-mode", "cors")]),
        (
            Method::GET,
            "/api/people",
            &[("accept", "application/json")],
        ),
        (
            Method::GET,
            "/docs/",
            &[("sec-fetch-mode", "no-cors"), ("accept", "text/html")],
        ),
        (
            Method::POST,
            "/api/decision",
            &[("sec-fetch-mode", "navigate")],
        ),
        (
            Method::GET,
            "/api/feed-socket",
            &[
                ("sec-fetch-mode", "websocket"),
                ("connection", "Upgrade"),
                ("upgrade", "websocket"),
            ],
        ),
    ];
    for (method, path, headers) in refused {
        let response = through.ask(method.clone(), path, headers, "{}").await;
        assert_eq!(
            (response.status(), body(&response).as_str()),
            (StatusCode::UNAUTHORIZED, "Unauthorized: not signed in\n"),
            "{method} {path} {headers:?}"
        );
        assert!(cookies(&response).is_empty(), "{method} {path}");
    }
    let html = through.get("/docs/", &[("accept", "text/html,*/*")]).await;
    assert_eq!(html.status(), StatusCode::FOUND);
    assert!(through.server.heard().is_empty());
}

/// Back from Google, the visitor is sent to the path they asked for, on the origin, with a
/// session cookie (host-only, secure, `HttpOnly`, Lax, twelve hours) and the flow cookie ended;
/// Google was asked once, with the code, the verifier of the challenge, the way back and the
/// client's secret. From then each read, save and upload reaches the server signed as the person
/// whose Google subject the word names, in the origin's Host, with no cookie, its body whole.
#[tokio::test(flavor = "multi_thread")]
async fn a_visitor_back_from_google_is_the_person_whose_subject_the_word_names_on_reads_saves_and_uploads()
 {
    let through = Through::new().await;
    let begun = through.begin("/docs/TOPIC?x=1").await;
    through.answers(&begun, &claims(&begun.query["nonce"]));
    let back = through.back_for(&begun).await;
    assert_eq!(back.status(), StatusCode::SEE_OTHER, "{}", through.log());
    assert_eq!(
        header_of(&back, header::LOCATION).unwrap(),
        format!("{ORIGIN}/docs/TOPIC?x=1")
    );
    let set = cookies(&back);
    let (session, flow) = (&set[0], &set[1]);
    let id = session
        .strip_prefix("__Host-symphony-sign-in=")
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    assert_eq!(id.len(), 43, "{session}");
    assert_eq!(
        *session,
        format!(
            "__Host-symphony-sign-in={id}; Max-Age=43200; Path=/; HttpOnly; SameSite=Lax; Secure"
        )
    );
    assert_eq!(
        flow,
        "__Host-symphony-sign-in-flow=; Max-Age=0; Path=/; HttpOnly; SameSite=Lax; Secure"
    );
    let forms = through.google.heard();
    assert_eq!(forms.len(), 1);
    let form: BTreeMap<_, _> = forms[0].iter().cloned().collect();
    assert_eq!(form["grant_type"], "authorization_code");
    assert_eq!(form["code"], CODE);
    assert_eq!(form["client_secret"], CLIENT_SECRET);
    assert_eq!(form["redirect_uri"], format!("{ORIGIN}/.sign-in/callback"));
    assert!(through.server.heard().is_empty());

    let cookie = format!("__Host-symphony-sign-in={id}; theme=dark");
    let read = through
        .get(
            "/api/people",
            &[("cookie", &cookie), ("sec-fetch-mode", "cors")],
        )
        .await;
    assert_eq!((read.status(), body(&read).as_str()), (StatusCode::OK, ANA));
    assert!(
        read.headers().get("x-dropped").is_none(),
        "{:?}",
        read.headers()
    );
    assert!(
        read.headers().get(header::CONNECTION).is_none(),
        "{:?}",
        read.headers()
    );
    let save = r#"{"path":"docs/TOPIC.md","slug":"one","state":"question","note":"Which one?"}"#;
    let saved = through
        .ask(
            Method::POST,
            "/api/decision",
            &[
                ("cookie", &cookie),
                ("origin", ORIGIN),
                ("content-type", "application/json"),
            ],
            save,
        )
        .await;
    assert_eq!(saved.status(), StatusCode::OK);
    let upload: Vec<u8> = (0..1_048_576_u32).map(|at| (at % 251) as u8).collect();
    let uploaded = through
        .ask(
            Method::POST,
            "/artifacts/uploads/plan.bin?name=plan",
            &[("cookie", &cookie), ("origin", ORIGIN)],
            upload.clone(),
        )
        .await;
    assert_eq!(uploaded.status(), StatusCode::OK);
    let heard = through.server.heard();
    assert_eq!(heard.len(), 3);
    for request in &heard {
        assert_eq!(request.who.as_deref(), Ok(ANA), "{}", request.path);
        assert_eq!(request.headers[header::HOST], "board.example.com");
        assert!(
            request.headers.get(header::COOKIE).is_none(),
            "{}",
            request.path
        );
    }
    assert_eq!(
        (heard[0].method.as_str(), heard[0].path.as_str()),
        ("GET", "/api/people")
    );
    assert_eq!(
        (heard[1].method.as_str(), heard[1].body.as_ref()),
        ("POST", save.as_bytes())
    );
    assert_eq!(heard[1].headers[header::ORIGIN], ORIGIN);
    assert_eq!(heard[2].body.as_ref(), upload.as_slice());
}

/// A word the caller sent is dropped, however many: the server gets the service's own alone,
/// for the session's person, never the one the caller signed for someone else.
#[tokio::test(flavor = "multi_thread")]
async fn a_callers_own_signed_in_header_is_dropped_for_the_services_own() {
    let through = Through::new().await;
    let cookie = through.sign_in("/").await;
    let mallory = word::signed(
        &key(),
        &Who {
            service: google::SERVICE,
            subject: "mallory".into(),
        },
        ORIGIN,
        now(),
    );
    for words in [vec![mallory.as_str()], vec![mallory.as_str(), "x.y.z"]] {
        let mut headers = vec![("cookie", cookie.as_str())];
        headers.extend(words.iter().map(|word| ("symphony-signed-in", *word)));
        let read = through.get("/api/people", &headers).await;
        assert_eq!((read.status(), body(&read).as_str()), (StatusCode::OK, ANA));
    }
    let heard = through.server.heard();
    for request in heard {
        assert_eq!(request.headers.get_all(word::HEADER).iter().count(), 1);
        assert_eq!(request.who.as_deref(), Ok(ANA));
    }
    let alone = through
        .get("/api/people", &[("symphony-signed-in", mallory.as_str())])
        .await;
    assert_eq!(alone.status(), StatusCode::UNAUTHORIZED);
}

/// The feed's event stream reaches the server signed and streams back as it is sent, and it ends
/// at its word's expiry, so a session signed out keeps no feed.
#[tokio::test(flavor = "multi_thread")]
async fn the_feeds_stream_is_signed_streams_back_and_ends_at_its_words_expiry() {
    let through = Through::with(ORIGIN, Some(Duration::from_millis(200))).await;
    let cookie = through.sign_in("/").await;
    let request = Request::get("/feed")
        .header("cookie", &cookie)
        .header("accept", "text/event-stream")
        .body(text(""))
        .unwrap();
    let response = crate::stand_in::open(through.address, request).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/event-stream"
    );
    let mut body = response.into_body();
    let first = tokio::time::timeout(BACKSTOP, body.frame())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(first.into_data().unwrap(), format!("data: {ANA}\n\n"));
    let ended = tokio::time::timeout(BACKSTOP, body.frame()).await.unwrap();
    assert!(ended.is_none(), "the stream went on past its word's expiry");
}

/// A socket's handshake reaches the server signed, with what a WebSocket's handshake carries
/// (its origin, key and version); answered 101, the two connections are spliced both ways, and
/// the socket ends at its word's expiry.
#[tokio::test(flavor = "multi_thread")]
async fn a_socket_is_signed_spliced_both_ways_and_ends_at_its_words_expiry() {
    let through = Through::with(ORIGIN, Some(Duration::from_millis(300))).await;
    let cookie = through.sign_in("/").await;
    let mut socket = TcpStream::connect(through.address).await.unwrap();
    let handshake = format!(
        "GET /socket HTTP/1.1\r\nHost: board.example.com\r\nCookie: {cookie}\r\n\
         Origin: {ORIGIN}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\
         Sec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
    );
    socket.write_all(handshake.as_bytes()).await.unwrap();
    let mut read = Vec::new();
    let mut buffer = [0; 1024];
    let wanted = format!("{ANA}\n");
    tokio::time::timeout(BACKSTOP, async {
        while !String::from_utf8_lossy(&read).ends_with(&wanted) {
            let got = socket.read(&mut buffer).await.unwrap();
            assert_ne!(got, 0, "{}", String::from_utf8_lossy(&read));
            read.extend_from_slice(&buffer[..got]);
        }
    })
    .await
    .unwrap();
    let said = String::from_utf8_lossy(&read).into_owned();
    assert!(said.starts_with("HTTP/1.1 101 "), "{said}");
    assert!(
        said.to_ascii_lowercase().contains("upgrade: websocket"),
        "{said}"
    );
    socket.write_all(b"ping").await.unwrap();
    let mut echoed = [0; 4];
    tokio::time::timeout(BACKSTOP, socket.read_exact(&mut echoed))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&echoed, b"ping");
    let end = tokio::time::timeout(BACKSTOP, socket.read(&mut buffer))
        .await
        .unwrap();
    assert!(matches!(end, Ok(0) | Err(_)), "{end:?}");
    let heard = through.server.heard();
    let handshake = &heard[0];
    assert_eq!(handshake.who.as_deref(), Ok(ANA));
    assert_eq!(handshake.headers[header::ORIGIN], ORIGIN);
    assert_eq!(
        handshake.headers["sec-websocket-key"],
        "dGhlIHNhbXBsZSBub25jZQ=="
    );
    assert_eq!(handshake.headers[header::UPGRADE], "websocket");
}

/// Each way back but the browser's own good one is refused with a page that offers to begin
/// again, and no session is made: a forged state, a replay, a way back from a browser that did not
/// begin it or began another, Google's own refusal, and an id token with another nonce, for
/// another client or expired. The log says why, and nothing reaches the server.
#[tokio::test(flavor = "multi_thread")]
async fn every_way_back_but_the_browsers_own_good_one_is_refused_with_no_session() {
    let through = Through::new().await;
    let refused = |response: &Response<Bytes>, why: &str, log: &str| {
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{why}");
        assert!(body(response).contains("Sign in again"), "{why}");
        assert!(
            !cookies(response)
                .iter()
                .any(|cookie| cookie.starts_with("__Host-symphony-sign-in=")),
            "{why}"
        );
        assert!(
            log.contains(&format!("refused a sign-in: {why}")),
            "{why}: {log}"
        );
    };

    let forged = "x".repeat(43);
    let response = through
        .back(
            &format!("state={forged}&code={}", enc(CODE)),
            Some(&format!("__Host-symphony-sign-in-flow={forged}")),
        )
        .await;
    refused(&response, UNKNOWN, &through.log());

    let begun = through.begin("/").await;
    through.answers(&begun, &claims(&begun.query["nonce"]));
    let query = format!("state={}&code={}", begun.state(), enc(CODE));
    refused(&through.back(&query, None).await, UNBOUND, &through.log());
    let other = through.begin("/").await;
    refused(
        &through.back(&query, Some(&other.cookie)).await,
        UNBOUND,
        &through.log(),
    );
    assert_eq!(
        through.back_for(&begun).await.status(),
        StatusCode::SEE_OTHER
    );
    refused(&through.back_for(&begun).await, UNKNOWN, &through.log());
    assert_eq!(
        through.google.heard().len(),
        1,
        "a replay is never exchanged"
    );

    let begun = through.begin("/").await;
    let denied = format!("state={}&error=access_denied", begun.state());
    refused(
        &through.back(&denied, Some(&begun.cookie)).await,
        "Google did not sign the visitor in",
        &through.log(),
    );
    let begun = through.begin("/").await;
    refused(
        &through
            .back(&format!("state={}", begun.state()), Some(&begun.cookie))
            .await,
        "its query names no code",
        &through.log(),
    );
    refused(
        &through.back(&format!("code={}", enc(CODE)), None).await,
        "its query names no state",
        &through.log(),
    );
    let begun = through.begin("/").await;
    let twice = format!("state={0}&state={0}&code={1}", begun.state(), enc(CODE));
    refused(
        &through.back(&twice, Some(&begun.cookie)).await,
        "its query names a parameter twice",
        &through.log(),
    );

    for (change, why) in [
        (("nonce", serde_json::json!("another")), google::NONCE),
        (
            ("aud", serde_json::json!("another-client")),
            google::AUDIENCE,
        ),
        (("exp", serde_json::json!(now() - 10)), google::EXPIRED),
        (
            ("iss", serde_json::json!("https://evil.example")),
            google::ISSUER,
        ),
    ] {
        let begun = through.begin("/").await;
        let mut claims = claims(&begun.query["nonce"]);
        claims[change.0] = change.1;
        if change.0 == "aud" {
            claims["azp"] = serde_json::json!("another-client");
        }
        through.answers(&begun, &claims);
        refused(&through.back_for(&begun).await, why, &through.log());
    }
    assert!(through.server.heard().is_empty());
}

/// A sign-in under way is taken only within its ten minutes: one begun longer ago is refused.
#[tokio::test(flavor = "multi_thread")]
async fn a_way_back_past_its_sign_ins_ten_minutes_is_refused() {
    let through = Through::new().await;
    let flow = (through.front.kept())
        .begin("/".into(), now() - FLOW_SECONDS)
        .unwrap();
    let cookie = format!("__Host-symphony-sign-in-flow={}", flow.state);
    let response = through
        .back(
            &format!("state={}&code={}", flow.state, enc(CODE)),
            Some(&cookie),
        )
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(through.log().contains(STALE), "{}", through.log());
}

/// A code Google does not take is said as Google not answering the sign-in, its error's code in
/// the log, and no session is made.
#[tokio::test(flavor = "multi_thread")]
async fn a_code_google_refuses_makes_no_session() {
    let through = Through::new().await;
    let begun = through.begin("/").await;
    through.google.answers(
        &begun.query["code_challenge"],
        &through.redirect(),
        400,
        r#"{"error":"invalid_grant","error_description":"Bad Request"}"#,
    );
    let response = through.back_for(&begun).await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(cookies(&response), Vec::<String>::new());
    assert!(
        through
            .log()
            .contains("Google's token endpoint answered 400 with no id token: invalid_grant"),
        "{}",
        through.log()
    );
}

/// Sign-out takes a POST naming the origin, ends the session and clears its cookie, so the next
/// request with it is refused; one from another site, or naming none, is refused and the session
/// holds; any other method is not allowed.
#[tokio::test(flavor = "multi_thread")]
async fn sign_out_from_the_origin_ends_the_session() {
    let through = Through::new().await;
    let cookie = through.sign_in("/").await;
    let fetch = [("cookie", cookie.as_str()), ("sec-fetch-mode", "cors")];
    for origin in [Some("https://evil.example"), None] {
        let mut headers = vec![("cookie", cookie.as_str())];
        headers.extend(origin.map(|origin| ("origin", origin)));
        let response = through.ask(Method::POST, SIGN_OUT, &headers, "").await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{origin:?}");
        assert_eq!(
            through.get("/api/people", &fetch).await.status(),
            StatusCode::OK
        );
    }
    let get = through.get(SIGN_OUT, &[("cookie", cookie.as_str())]).await;
    assert_eq!(get.status(), StatusCode::METHOD_NOT_ALLOWED);
    let out = through
        .ask(
            Method::POST,
            SIGN_OUT,
            &[("cookie", cookie.as_str()), ("origin", ORIGIN)],
            "",
        )
        .await;
    assert_eq!(out.status(), StatusCode::OK);
    assert_eq!(
        cookies(&out),
        ["__Host-symphony-sign-in=; Max-Age=0; Path=/; HttpOnly; SameSite=Lax; Secure"]
    );
    let after = through.get("/api/people", &fetch).await;
    assert_eq!(after.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(through.server.heard().len(), 2);
}

/// The visitor's own page says whom they are signed in as, the sign-in a profile names, and
/// offers sign-out, loading nothing and framed by no site; without a session a navigation to it
/// is sent to sign in. The service's own paths never reach the server.
#[tokio::test(flavor = "multi_thread")]
async fn the_visitors_own_page_says_whom_they_are_signed_in_as_and_offers_sign_out() {
    let through = Through::new().await;
    let cookie = through.sign_in("/").await;
    let page = through.get(PREFIX, &[("cookie", cookie.as_str())]).await;
    assert_eq!(page.status(), StatusCode::OK);
    let html = body(&page);
    assert!(
        html.contains(&format!("<p>Signed in as <code>{ANA}</code></p>")),
        "{html}"
    );
    assert!(
        html.contains(
            r#"<form method="post" action="/.sign-in/sign-out"><button>Sign out</button></form>"#
        ),
        "{html}"
    );
    assert!(
        page.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .contains("default-src 'none'")
    );
    assert_eq!(page.headers()[header::CACHE_CONTROL], "no-store");
    let unsigned = through.get(PREFIX, &[("sec-fetch-mode", "navigate")]).await;
    assert_eq!(unsigned.status(), StatusCode::FOUND);
    for path in ["/.sign-in/other", "/.sign-in"] {
        let response = through.get(path, &[("cookie", cookie.as_str())]).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
    for path in [PREFIX, CALLBACK] {
        let post = through
            .ask(Method::POST, path, &[("cookie", cookie.as_str())], "")
            .await;
        assert_eq!(post.status(), StatusCode::METHOD_NOT_ALLOWED, "{path}");
    }
    assert!(through.server.heard().is_empty());
}

/// Where a visitor comes back to is the path they asked for, on the origin, whatever a request's
/// target or query names: an absolute target's host is dropped, and a parameter naming another
/// site is part of the path, never followed.
#[tokio::test(flavor = "multi_thread")]
async fn a_visitor_comes_back_to_a_path_of_the_origin_never_a_url_a_caller_names() {
    let through = Through::new().await;
    let mut socket = TcpStream::connect(through.address).await.unwrap();
    socket
        .write_all(
            b"GET http://evil.example/next?to=https://evil.example HTTP/1.1\r\n\
              Host: evil.example\r\nSec-Fetch-Mode: navigate\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
    let mut said = String::new();
    tokio::time::timeout(BACKSTOP, socket.read_to_string(&mut said))
        .await
        .unwrap()
        .unwrap();
    let location = (said.lines())
        .find_map(|line| line.strip_prefix("location: "))
        .unwrap();
    let state = form_urlencoded::parse(location.split_once('?').unwrap().1.as_bytes())
        .find(|(name, _)| name == "state")
        .unwrap()
        .1
        .into_owned();
    let nonce = form_urlencoded::parse(location.split_once('?').unwrap().1.as_bytes())
        .find(|(name, _)| name == "nonce")
        .unwrap()
        .1
        .into_owned();
    let challenge = form_urlencoded::parse(location.split_once('?').unwrap().1.as_bytes())
        .find(|(name, _)| name == "code_challenge")
        .unwrap()
        .1
        .into_owned();
    through
        .google
        .signs_in(&challenge, &through.redirect(), &claims(&nonce));
    let back = through
        .back(
            &format!(
                "state={state}&code={}&redirect_uri=https://evil.example&to=//evil.example",
                enc(CODE)
            ),
            Some(&format!("__Host-symphony-sign-in-flow={state}")),
        )
        .await;
    assert_eq!(
        header_of(&back, header::LOCATION).unwrap(),
        format!("{ORIGIN}/next?to=https://evil.example")
    );
}

/// For an `http://` origin, which Google allows on this machine's loopback alone, the cookies
/// drop the `__Host-` prefix and `Secure`, which a browser takes only over HTTPS.
#[tokio::test(flavor = "multi_thread")]
async fn an_http_origins_cookies_are_neither_host_prefixed_nor_secure() {
    let origin = "http://localhost:8766";
    let through = Through::with(origin, None).await;
    let begun = through.begin("/").await;
    assert_eq!(
        begun.set_cookie,
        format!(
            "symphony-sign-in-flow={}; Max-Age=600; Path=/; HttpOnly; SameSite=Lax",
            begun.state()
        )
    );
    assert_eq!(
        begun.query["redirect_uri"],
        "http://localhost:8766/.sign-in/callback"
    );
    through.answers(&begun, &claims(&begun.query["nonce"]));
    let back = through.back_for(&begun).await;
    assert!(
        cookies(&back)[0].starts_with("symphony-sign-in="),
        "{:?}",
        cookies(&back)
    );
    let cookie = cookies(&back)[0].split(';').next().unwrap().to_string();
    let read = through
        .get("/api/people", &[("cookie", cookie.as_str())])
        .await;
    assert_eq!(body(&read), ANA);
    assert_eq!(
        through.server.heard()[0].headers[header::HOST],
        "localhost:8766"
    );
}

/// A server behind that does not answer is said to the visitor as a gateway that does not, and
/// in the log by its address.
#[tokio::test(flavor = "multi_thread")]
async fn a_server_behind_that_does_not_answer_is_a_bad_gateway() {
    let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = closed.local_addr().unwrap();
    drop(closed);
    let log = Arc::new(Mutex::new(Vec::new()));
    let google = Google::new().await;
    let front = Front::new(
        settings(address, ORIGIN),
        key(),
        google.provider(),
        log.clone(),
    )
    .unwrap();
    let token = front.kept().open(ana(), now()).unwrap();
    let (_front, at) = listening(front).await;
    let request = Request::get("/api/people")
        .header("cookie", format!("__Host-symphony-sign-in={token}"))
        .body(text(""))
        .unwrap();
    let response = send(at, request).await;
    assert_eq!(
        (response.status(), body(&response).as_str()),
        (
            StatusCode::BAD_GATEWAY,
            "Bad Gateway: the server behind does not answer\n"
        )
    );
    let said = String::from_utf8(log.lock().unwrap().clone()).unwrap();
    assert!(
        said.contains(&format!("the server at {address} did not answer")),
        "{said}"
    );
}

/// What the log says over a whole sign-in, a passed-on read, every refusal and sign-out names
/// no secret: not the client secret, the key, the code, a state, a nonce, a verifier, a session's
/// id, an id token or a word.
#[tokio::test(flavor = "multi_thread")]
async fn the_log_names_no_secret_code_token_cookie_or_word() {
    let through = Through::new().await;
    let begun = through.begin("/").await;
    let claims = claims(&begun.query["nonce"]);
    through.answers(&begun, &claims);
    let back = through.back_for(&begun).await;
    let session = cookies(&back)[0].split(';').next().unwrap().to_string();
    let id = session.split_once('=').unwrap().1.to_string();
    through
        .get("/api/people", &[("cookie", session.as_str())])
        .await;
    through.back_for(&begun).await;
    let other = through.begin("/").await;
    through
        .back(&format!("state={}&code={}", other.state(), enc(CODE)), None)
        .await;
    through.google.answers(
        &other.query["code_challenge"],
        &through.redirect(),
        400,
        r#"{"error":"invalid_grant"}"#,
    );
    through.back_for(&other).await;
    through
        .ask(
            Method::POST,
            SIGN_OUT,
            &[("cookie", session.as_str()), ("origin", ORIGIN)],
            "",
        )
        .await;
    let log = through.log();
    assert!(log.contains("refused a sign-in"), "{log}");
    let words: Vec<String> = (through.server.heard().iter())
        .flat_map(|request| request.headers.get_all(word::HEADER).iter())
        .map(|word| word.to_str().unwrap().to_string())
        .collect();
    assert_ne!(words, Vec::<String>::new());
    let seed = settings::hex(&SEED);
    let mut secrets = vec![
        CLIENT_SECRET.to_string(),
        seed,
        CODE.to_string(),
        enc(CODE),
        id,
        crate::stand_in::id_token(&claims),
    ];
    for begun in [&begun, &other] {
        secrets.extend(["state", "nonce", "code_challenge"].map(|name| begun.query[name].clone()));
    }
    secrets.extend(words);
    for secret in &secrets {
        assert!(
            !log.contains(secret.as_str()),
            "the log names {secret}: {log}"
        );
    }
}
