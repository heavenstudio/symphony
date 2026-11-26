//! The service with GitHub set, alone or beside Google, in front of a server that verifies every
//! word with the server's own check (`stand_in::Server`), GitHub's token and user endpoints
//! standing in: a visitor is sent to GitHub and comes back as `github:ID`, the account's numeric id;
//! every way back but the browser's own good one, an answer naming no numeric id and an exchange
//! that fails are refused with no session; with both set, a visitor is offered two links and
//! nothing else, each to its provider for one sign-in; and nothing the log says names a secret.

use super::*;
use crate::flow::{FORGED, TO_BYTES, UNBOUND};
use crate::stand_in::{
    CLIENT_ID, CODE, Form, GITHUB_CLIENT_ID, GITHUB_CLIENT_SECRET, GITHUB_ID, GITHUB_TOKEN, GitHub,
    Google, ORIGIN, SUBJECT, Server, claims, folder, github_client, key, send, server_names,
    settings, text,
};
use std::collections::BTreeMap;

/// Ana's sign-in at GitHub, as the server's check reads the word.
const ANA: &str = "github:77001234";

/// A service in front of a stand-in server, with GitHub's client set, Google's too where asked.
struct Through {
    _machine: crate::stand_in::Folder,
    google: Google,
    github: GitHub,
    server: Server,
    address: std::net::SocketAddr,
    log: Arc<Mutex<Vec<u8>>>,
}

/// A sign-in begun: the answer to the navigation, the provider it was sent to with its query,
/// and the flow cookie, `NAME=VALUE`.
struct Begun {
    response: Response<Bytes>,
    query: BTreeMap<String, String>,
    cookie: String,
}

impl Begun {
    fn state(&self) -> &str {
        &self.query["state"]
    }
}

impl Through {
    /// With GitHub's client set, and Google's beside it where `google`.
    async fn new(google: bool) -> Through {
        Through::with(google, true).await
    }

    /// With Google's client where `google` and GitHub's where `github`.
    async fn with(google: bool, github: bool) -> Through {
        let machine = folder("front-github");
        server_names(&machine, ORIGIN);
        let (google_stand_in, github_stand_in) = (Google::new().await, GitHub::new().await);
        let server = Server::new(&machine).await;
        let mut settings = settings(server.address, ORIGIN);
        if !google {
            settings.google = None;
        }
        if github {
            settings.github = Some(github_client());
        }
        let providers = Providers {
            google: google_stand_in.provider(),
            github: github_stand_in.provider(),
        };
        let log = Arc::new(Mutex::new(Vec::new()));
        let front = Arc::new(Front::new(settings, key(), providers, log.clone()).unwrap());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(front.listen(listener));
        Through {
            _machine: machine,
            google: google_stand_in,
            github: github_stand_in,
            server,
            address,
            log,
        }
    }

    async fn get(&self, path: &str, headers: &[(&str, &str)]) -> Response<Bytes> {
        let mut request = Request::get(path);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        send(self.address, request.body(text("")).unwrap()).await
    }

    /// A browser's navigation to `path` with no session, sent to GitHub, the one provider set.
    async fn begin(&self, path: &str) -> Begun {
        let response = self.get(path, &[("sec-fetch-mode", "navigate")]).await;
        assert_eq!(response.status(), StatusCode::FOUND, "{path}");
        let location = header_of(&response, header::LOCATION);
        let (endpoint, query) = location.split_once('?').unwrap();
        assert_eq!(endpoint, "https://github.com/login/oauth/authorize");
        let query = parsed(query);
        let cookie = flow_cookie(&response);
        Begun {
            response,
            query,
            cookie,
        }
    }

    /// GitHub's way back with `query`, from a browser sending `cookie`.
    async fn back(&self, query: &str, cookie: Option<&str>) -> Response<Bytes> {
        let headers: Vec<(&str, &str)> = cookie
            .map(|cookie| ("cookie", cookie))
            .into_iter()
            .collect();
        self.get(&format!("{GITHUB_CALLBACK}?{query}"), &headers)
            .await
    }

    /// The way back for `begun` with GitHub's code, from the browser that began it.
    async fn back_for(&self, begun: &Begun) -> Response<Bytes> {
        let query = format!("code={}&state={}", enc(CODE), begun.state());
        self.back(&query, Some(&begun.cookie)).await
    }

    /// GitHub, from now, answering `begun`'s exchange with its access token, once.
    fn signs_in(&self, begun: &Begun) {
        self.github
            .signs_in(&begun.query["code_challenge"], &redirect());
    }

    fn log(&self) -> String {
        String::from_utf8(self.log.lock().unwrap().clone()).unwrap()
    }
}

/// GitHub's way back, as the app registers it.
fn redirect() -> String {
    format!("{ORIGIN}{GITHUB_CALLBACK}")
}

fn header_of(response: &Response<Bytes>, name: HeaderName) -> String {
    response.headers()[name].to_str().unwrap().to_string()
}

/// Every `Set-Cookie` of `response`, in order.
fn cookies(response: &Response<Bytes>) -> Vec<String> {
    (response.headers().get_all(header::SET_COOKIE).iter())
        .map(|value| value.to_str().unwrap().to_string())
        .collect()
}

/// The flow cookie `response` sets, `NAME=VALUE`.
fn flow_cookie(response: &Response<Bytes>) -> String {
    let set = cookies(response);
    assert_eq!(set.len(), 1, "{set:?}");
    let cookie = set[0].split(';').next().unwrap().to_string();
    assert!(
        cookie.starts_with("__Host-symphony-sign-in-flow="),
        "{cookie}"
    );
    cookie
}

/// The session cookie `response` sets, `NAME=VALUE`, where it sets one.
fn session_cookie(response: &Response<Bytes>) -> Option<String> {
    (cookies(response).into_iter())
        .find(|cookie| cookie.starts_with("__Host-symphony-sign-in="))
        .map(|cookie| cookie.split(';').next().unwrap().to_string())
}

/// A query's names and values.
fn parsed(query: &str) -> BTreeMap<String, String> {
    form_urlencoded::parse(query.as_bytes())
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

/// `text` form-encoded.
fn enc(text: &str) -> String {
    form_urlencoded::byte_serialize(text.as_bytes()).collect()
}

fn body(response: &Response<Bytes>) -> String {
    String::from_utf8_lossy(response.body()).into_owned()
}

/// With GitHub alone set, a browser's navigation with no session is sent to GitHub's authorization
/// endpoint for the app, with GitHub's way back, a fresh state and S256 challenge, no scope and no
/// nonce, and its browser holds the sign-in in a host-only, secure, `HttpOnly`, Lax flow cookie;
/// nothing reaches the server, and Google's way back is not one of the service's paths.
#[tokio::test(flavor = "multi_thread")]
async fn with_github_alone_a_navigation_is_sent_to_github_with_no_scope() {
    let through = Through::new(false).await;
    let begun = through.begin("/docs/TOPIC?x=1").await;
    let names: Vec<&str> = begun.query.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        [
            "client_id",
            "code_challenge",
            "code_challenge_method",
            "redirect_uri",
            "state"
        ]
    );
    assert_eq!(begun.query["client_id"], GITHUB_CLIENT_ID);
    assert_eq!(begun.query["redirect_uri"], redirect());
    assert_eq!(begun.query["code_challenge_method"], "S256");
    for name in ["state", "code_challenge"] {
        assert_eq!(begun.query[name].len(), 43, "{name}");
    }
    let set = cookies(&begun.response);
    assert_eq!(
        set[0],
        format!(
            "{}; Max-Age=600; Path=/; HttpOnly; SameSite=Lax; Secure",
            begun.cookie
        )
    );
    assert_eq!(begun.response.headers()[header::CACHE_CONTROL], "no-store");
    for path in [CALLBACK, "/.sign-in/github"] {
        let query = format!("{path}?state={}&code={}", begun.state(), enc(CODE));
        let response = through
            .get(&query, &[("cookie", begun.cookie.as_str())])
            .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
    assert!(through.server.heard().is_empty());
}

/// Back from GitHub, the visitor is sent to the path they asked for, with a session; GitHub was
/// asked once for the code, with the verifier of the challenge, the way back and the app's secret,
/// and once whose its access token is, the token as a bearer. From then each request reaches the
/// server signed as `github:` and the account's numeric id, which the visitor's own page says.
#[tokio::test(flavor = "multi_thread")]
async fn a_visitor_back_from_github_is_passed_on_as_github_and_their_numeric_id() {
    let through = Through::new(false).await;
    let begun = through.begin("/docs/TOPIC?x=1").await;
    through.signs_in(&begun);
    let back = through.back_for(&begun).await;
    assert_eq!(back.status(), StatusCode::SEE_OTHER, "{}", through.log());
    assert_eq!(
        header_of(&back, header::LOCATION),
        format!("{ORIGIN}/docs/TOPIC?x=1")
    );
    let session = session_cookie(&back).unwrap();
    assert_eq!(
        cookies(&back)[1],
        "__Host-symphony-sign-in-flow=; Max-Age=0; Path=/; HttpOnly; SameSite=Lax; Secure"
    );
    let forms = through.github.heard();
    assert_eq!(forms.len(), 1);
    let form: BTreeMap<_, _> = forms[0].iter().cloned().collect();
    assert_eq!(form["code"], CODE);
    assert_eq!(form["client_id"], GITHUB_CLIENT_ID);
    assert_eq!(form["client_secret"], GITHUB_CLIENT_SECRET);
    assert_eq!(form["redirect_uri"], redirect());
    let asked = through.github.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0][header::AUTHORIZATION],
        format!("Bearer {GITHUB_TOKEN}")
    );
    assert!(through.server.heard().is_empty());

    let read = through
        .get(
            "/api/people",
            &[("cookie", session.as_str()), ("sec-fetch-mode", "cors")],
        )
        .await;
    assert_eq!((read.status(), body(&read).as_str()), (StatusCode::OK, ANA));
    let heard = through.server.heard();
    assert_eq!(heard.len(), 1);
    assert_eq!(heard[0].who.as_deref(), Ok(ANA));
    assert_eq!(ANA, format!("github:{GITHUB_ID}"));
    assert!(heard[0].headers.get(header::COOKIE).is_none());
    let page = through.get(PREFIX, &[("cookie", session.as_str())]).await;
    assert!(
        body(&page).contains(&format!("<p>Signed in as <code>{ANA}</code></p>")),
        "{}",
        body(&page)
    );
}

/// A way back that is not the browser's own good one is refused with a page that offers to begin
/// again, and no session is made: a state that is not the browser's sign-in's, no flow cookie, one
/// the service did not sign, a replay, which GitHub refuses as it takes a code once, and GitHub's
/// own refusal. Nothing reaches the server.
#[tokio::test(flavor = "multi_thread")]
async fn a_way_back_from_github_not_the_browsers_own_is_refused_with_no_session() {
    let through = Through::new(false).await;
    let begun = through.begin("/").await;
    through.signs_in(&begun);
    let other = through.begin("/").await;
    let refusals = [
        (
            format!("state={}&code={}", other.state(), enc(CODE)),
            Some(begun.cookie.clone()),
            UNBOUND,
        ),
        (
            format!("state={}&code={}", begun.state(), enc(CODE)),
            None,
            UNBOUND,
        ),
        (
            format!("state={}&code={}", begun.state(), enc(CODE)),
            Some(format!("__Host-symphony-sign-in-flow={}", begun.state())),
            FORGED,
        ),
        (
            format!("state={}&error=access_denied", begun.state()),
            Some(begun.cookie.clone()),
            "GitHub did not sign the visitor in",
        ),
    ];
    for (query, cookie, why) in refusals {
        let response = through.back(&query, cookie.as_deref()).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{why}");
        assert!(body(&response).contains("Sign in again"), "{why}");
        assert_eq!(session_cookie(&response), None, "{why}");
        assert!(
            through.log().contains(&format!("refused a sign-in: {why}")),
            "{why}: {}",
            through.log()
        );
    }
    assert!(
        through.github.heard().is_empty(),
        "a refusal is never exchanged"
    );

    let good = through.back_for(&begun).await;
    assert_eq!(good.status(), StatusCode::SEE_OTHER);
    let replay = through.back_for(&begun).await;
    assert_eq!(replay.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(session_cookie(&replay), None);
    assert!(
        through.log().contains(
            "a sign-in was not answered: GitHub's token endpoint answered 200 with no access \
             token: bad_verification_code"
        ),
        "{}",
        through.log()
    );
    assert_eq!(through.github.asked().len(), 1, "a replay asks no user");
    assert!(through.server.heard().is_empty());
}

/// GitHub's answer of whose the token is, with no id or one that is not a whole number above 0, is
/// refused, and no session is made: never the login in its place.
#[tokio::test(flavor = "multi_thread")]
async fn an_answer_naming_no_numeric_id_is_refused_with_no_session() {
    let through = Through::new(false).await;
    for said in [
        r#"{"login":"ana"}"#,
        r#"{"login":"ana","id":"77001234"}"#,
        r#"{"login":"ana","id":-1}"#,
        r#"{"login":"ana","id":1.5}"#,
        "not json",
    ] {
        through.github.says(200, said);
        let begun = through.begin("/").await;
        through.signs_in(&begun);
        let response = through.back_for(&begun).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{said}");
        assert!(
            body(&response).contains("This sign-in is not taken."),
            "{said}"
        );
        assert_eq!(session_cookie(&response), None, "{said}");
        assert!(
            through
                .log()
                .ends_with(&format!("refused a sign-in: {}\n", github::NO_ID)),
            "{said}: {}",
            through.log()
        );
    }
    assert!(through.server.heard().is_empty());
}

/// A code GitHub does not exchange, or a token it does not say whose it is, is said as GitHub not
/// answering the sign-in, and no session is made: its error's code, or its status, in the log.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_exchange_or_user_answer_is_said_as_github_not_answering() {
    let through = Through::new(false).await;
    let unexchanged = through.begin("/").await;
    let response = through.back_for(&unexchanged).await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert!(
        body(&response).contains("<p>GitHub did not answer the sign-in.</p>"),
        "{}",
        body(&response)
    );
    assert_eq!(session_cookie(&response), None);
    assert!(
        through.log().contains(
            "GitHub's token endpoint answered 200 with no access token: bad_verification_code"
        ),
        "{}",
        through.log()
    );

    let failing = through.begin("/").await;
    through.github.answers(
        &failing.query["code_challenge"],
        &redirect(),
        500,
        "Internal Server Error",
    );
    let response = through.back_for(&failing).await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert!(
        through
            .log()
            .contains("GitHub's token endpoint answered 500 with no access token\n"),
        "{}",
        through.log()
    );

    through.github.says(401, r#"{"message":"Bad credentials"}"#);
    let unknown = through.begin("/").await;
    through.signs_in(&unknown);
    let response = through.back_for(&unknown).await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(session_cookie(&response), None);
    assert!(
        through
            .log()
            .ends_with("a sign-in was not answered: GitHub's user endpoint answered 401\n"),
        "{}",
        through.log()
    );
    assert!(through.server.heard().is_empty());
}

/// With both set, a navigation with no session is answered a page of two links and nothing else,
/// Google's and GitHub's, each to its provider for the one sign-in its flow cookie holds, framed by
/// no site and kept by no cache; following either comes back signed in by it.
#[tokio::test(flavor = "multi_thread")]
async fn with_both_set_a_visitor_is_offered_two_links_and_nothing_else() {
    let through = Through::new(true).await;
    let mut signed = Vec::new();
    for by in [By::GitHub, By::Google] {
        let page = through
            .get("/docs/TOPIC?x=1", &[("sec-fetch-mode", "navigate")])
            .await;
        assert_eq!(page.status(), StatusCode::OK);
        assert_eq!(page.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(
            page.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8"
        );
        assert!(
            (page.headers()[header::CONTENT_SECURITY_POLICY]
                .to_str()
                .unwrap())
            .starts_with("default-src 'none'; style-src 'sha256-")
        );
        let cookie = flow_cookie(&page);
        let html = body(&page);
        let (_, links) = html.split_once("</style>\n").unwrap();
        let hrefs: Vec<String> = (links.split("<a href=\"").skip(1))
            .map(|link| link.split_once('"').unwrap().0.replace("&amp;", "&"))
            .collect();
        assert_eq!(
            links,
            format!(
                "<p><a href=\"{}\">Sign in with Google</a></p>\n\
                 <p><a href=\"{}\">Sign in with GitHub</a></p>\n",
                hrefs[0].replace('&', "&amp;"),
                hrefs[1].replace('&', "&amp;")
            )
        );
        let (google_to, google) = hrefs[0].split_once('?').unwrap();
        let (github_to, github) = hrefs[1].split_once('?').unwrap();
        assert_eq!(google_to, "https://accounts.google.com/o/oauth2/v2/auth");
        assert_eq!(github_to, "https://github.com/login/oauth/authorize");
        let (google, github) = (parsed(google), parsed(github));
        assert_eq!(google["client_id"], CLIENT_ID);
        assert_eq!(google["redirect_uri"], format!("{ORIGIN}{CALLBACK}"));
        assert_eq!(google["scope"], "openid");
        assert_eq!(github["client_id"], GITHUB_CLIENT_ID);
        assert_eq!(github["redirect_uri"], redirect());
        assert!(!github.contains_key("scope") && !github.contains_key("nonce"));
        for name in ["state", "code_challenge"] {
            assert_eq!(google[name], github[name], "{name}: one sign-in");
        }
        let state = &github["state"];
        let back = match by {
            By::GitHub => {
                through
                    .github
                    .signs_in(&github["code_challenge"], &redirect());
                let query = format!("code={}&state={state}", enc(CODE));
                through.back(&query, Some(&cookie)).await
            }
            By::Google => {
                through.google.signs_in(
                    &google["code_challenge"],
                    &format!("{ORIGIN}{CALLBACK}"),
                    &claims(&google["nonce"]),
                );
                let query = format!("{CALLBACK}?state={state}&code={}", enc(CODE));
                through.get(&query, &[("cookie", cookie.as_str())]).await
            }
        };
        assert_eq!(back.status(), StatusCode::SEE_OTHER, "{}", through.log());
        assert_eq!(
            header_of(&back, header::LOCATION),
            format!("{ORIGIN}/docs/TOPIC?x=1")
        );
        let session = session_cookie(&back).unwrap();
        let read = through
            .get(
                "/api/people",
                &[("cookie", session.as_str()), ("sec-fetch-mode", "cors")],
            )
            .await;
        signed.push(body(&read));
    }
    assert_eq!(signed, [ANA.to_string(), format!("google:{SUBJECT}")]);
}

/// With Google alone set, GitHub's way back is not one of the service's paths, and nothing of
/// GitHub is reached.
#[tokio::test(flavor = "multi_thread")]
async fn with_google_alone_githubs_way_back_is_not_found() {
    let through = Through::with(true, false).await;
    let page = through.get("/", &[("sec-fetch-mode", "navigate")]).await;
    assert_eq!(page.status(), StatusCode::FOUND);
    let location = header_of(&page, header::LOCATION);
    assert!(
        location.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"),
        "{location}"
    );
    let cookie = flow_cookie(&page);
    let state = parsed(location.split_once('?').unwrap().1)["state"].clone();
    let query = format!("{GITHUB_CALLBACK}?state={state}&code={}", enc(CODE));
    let response = through.get(&query, &[("cookie", cookie.as_str())]).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(through.github.heard(), Vec::<Form>::new());
}

/// A path to come back to past its bound comes back to `/`, so the flow cookie a browser keeps
/// holds it; one within it comes back whole.
#[tokio::test(flavor = "multi_thread")]
async fn a_way_back_past_its_bound_comes_back_to_the_root() {
    let through = Through::new(false).await;
    let within = format!("/{}", "a".repeat(TO_BYTES - 1));
    for (asked, to) in [
        (within.clone(), within.as_str()),
        (format!("{within}a"), "/"),
    ] {
        let begun = through.begin(&asked).await;
        assert!(begun.cookie.len() < 3200, "{}", begun.cookie.len());
        through.signs_in(&begun);
        let back = through.back_for(&begun).await;
        assert_eq!(header_of(&back, header::LOCATION), format!("{ORIGIN}{to}"));
    }
}

/// What the log says over GitHub sign-ins, a replay, every refusal and a failed exchange names
/// no secret: not the app's secret, the code, a state, a verifier, a flow cookie, the access
/// token, a session's id or a word.
#[tokio::test(flavor = "multi_thread")]
async fn the_log_names_no_github_secret_code_or_token() {
    let through = Through::new(false).await;
    let begun = through.begin("/").await;
    through.signs_in(&begun);
    let back = through.back_for(&begun).await;
    let session = session_cookie(&back).unwrap();
    through
        .get("/api/people", &[("cookie", session.as_str())])
        .await;
    through.back_for(&begun).await;
    let unbound = format!("state={}&code={}", begun.state(), enc(CODE));
    through.back(&unbound, None).await;
    through.github.says(200, r#"{"login":"ana"}"#);
    let other = through.begin("/").await;
    through.signs_in(&other);
    through.back_for(&other).await;
    let log = through.log();
    assert!(log.contains("refused a sign-in"), "{log}");
    assert!(log.contains("was not answered"), "{log}");
    let mut secrets = vec![
        GITHUB_CLIENT_SECRET.to_string(),
        GITHUB_TOKEN.to_string(),
        CODE.to_string(),
        enc(CODE),
        session.split_once('=').unwrap().1.to_string(),
    ];
    for begun in [&begun, &other] {
        secrets.extend(["state", "code_challenge"].map(|name| begun.query[name].clone()));
        secrets.push(begun.cookie.split_once('=').unwrap().1.to_string());
    }
    secrets.extend(
        (through.github.heard().iter())
            .flat_map(|form| form.iter().filter(|(name, _)| name == "code_verifier"))
            .map(|(_, verifier)| verifier.clone()),
    );
    secrets.extend(
        (through.server.heard().iter())
            .flat_map(|request| request.headers.get_all(word::HEADER).iter())
            .map(|word| word.to_str().unwrap().to_string()),
    );
    assert!(secrets.len() > 10, "{secrets:?}");
    for secret in &secrets {
        assert!(
            !log.contains(secret.as_str()),
            "the log names {secret}: {log}"
        );
    }
}
