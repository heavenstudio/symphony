//! What the tests stand in for, so no test reaches the network: a machine folder of a test's own;
//! Google's token endpoint and GitHub's token and user endpoints, each on a private loopback port,
//! answering as the test sets them, taking a code once and keeping every request they heard; and
//! the server behind, which verifies every word it is passed with the
//! server's own check (`symphony::signed_in`), against the `sign-in.toml` its machine folder holds,
//! and keeps who each request was from, as that check says.

use crate::front::Body;
use crate::github;
use crate::google::Provider;
use crate::settings::{Client, Key, Secret, Settings};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use futures_util::StreamExt as _;
use http_body_util::{BodyExt as _, Empty, Full, StreamBody};
use hyper::body::{Bytes, Frame, Incoming};
use hyper::header::{self, HeaderMap, HeaderValue};
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use sha2::{Digest as _, Sha256};
use std::convert::Infallible;
use std::fs;
use std::future::Future;
use std::net::SocketAddr;
use std::ops::Deref;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};

/// The client id the tests' service signs visitors in as.
pub(crate) const CLIENT_ID: &str = "1234-stand-in.apps.googleusercontent.com";
/// Its secret, which no Google issued.
pub(crate) const CLIENT_SECRET: &str = "GOCSPX-stand-in-secret-0000";
/// Ana's subject at Google.
pub(crate) const SUBJECT: &str = "110248495921238986420";
/// The code the stand-in hands back and takes once.
pub(crate) const CODE: &str = "4/0stand-in-code";
/// The client id of the tests' GitHub OAuth app.
pub(crate) const GITHUB_CLIENT_ID: &str = "Ov23liStandIn0000000";
/// Its secret, which no GitHub issued.
pub(crate) const GITHUB_CLIENT_SECRET: &str = "0123456789abcdef0123456789abcdef01234567";
/// The access token the stand-in GitHub answers a code with.
pub(crate) const GITHUB_TOKEN: &str = "gho_StandIn0000000000000000000000000000";
/// Ana's account id at GitHub.
pub(crate) const GITHUB_ID: u64 = 77_001_234;
/// The origin the tests' browsers reach the server at.
pub(crate) const ORIGIN: &str = "https://board.example.com";
/// The seed of the tests' service key.
pub(crate) const SEED: [u8; 32] = [7; 32];

/// The longest a test waits for what has already happened, or happens on its own soon: only a
/// stall reaches it.
pub(crate) const BACKSTOP: Duration = test_time::wait::FLOOR;

/// An empty folder of the test's own, removed with all it holds when it is dropped.
pub(crate) struct Folder(PathBuf);

impl Deref for Folder {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// An empty folder of the test's own, named for `name`, mode 700.
pub(crate) fn folder(name: &str) -> Folder {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "symphony-sign-in-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    Folder(dir)
}

/// `text` written to `name` in `folder`, mode `mode`.
pub(crate) fn write(folder: &Path, name: &str, text: &str, mode: u32) -> PathBuf {
    let path = folder.join(name);
    fs::write(&path, text).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
    path
}

/// The tests' service key.
pub(crate) fn key() -> Key {
    Key::from_seed(&SEED)
}

/// The machine folder `machine` naming the tests' key and `origin` in its `sign-in.toml`, as the
/// server reads it.
pub(crate) fn server_names(machine: &Path, origin: &str) {
    write(
        machine,
        crate::settings::SERVER,
        &format!("key = \"{}\"\norigin = \"{origin}\"\n", key().public()),
        0o600,
    );
}

/// The settings of a service in front of `upstream` for `origin`, listening anywhere.
pub(crate) fn settings(upstream: SocketAddr, origin: &str) -> Settings {
    Settings {
        listen: "127.0.0.1:0".parse().unwrap(),
        upstream,
        origin: origin.into(),
        google: Some(Client {
            id: CLIENT_ID.into(),
            secret: Secret::stand_in(CLIENT_SECRET),
        }),
        github: None,
    }
}

/// The tests' GitHub OAuth app.
pub(crate) fn github_client() -> Client {
    Client {
        id: GITHUB_CLIENT_ID.into(),
        secret: Secret::stand_in(GITHUB_CLIENT_SECRET),
    }
}

/// The seconds since 1970, now.
pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// An id token Google would answer with: a header, `claims` and a signature nobody checks.
pub(crate) fn id_token(claims: &serde_json::Value) -> String {
    let part = |bytes: &[u8]| URL_SAFE_NO_PAD.encode(bytes);
    format!(
        "{}.{}.{}",
        part(br#"{"alg":"RS256","kid":"stand-in","typ":"JWT"}"#),
        part(claims.to_string().as_bytes()),
        part(b"not checked")
    )
}

/// The claims of a good id token for Ana, made for [`CLIENT_ID`] with `nonce`.
pub(crate) fn claims(nonce: &str) -> serde_json::Value {
    let now = now();
    serde_json::json!({
        "iss": "https://accounts.google.com",
        "azp": CLIENT_ID,
        "aud": CLIENT_ID,
        "sub": SUBJECT,
        "nonce": nonce,
        "iat": now - 5,
        "exp": now + 3600,
    })
}

/// A server on a private loopback port answering each request with `answer`.
pub(crate) async fn serving<F, A>(answer: F) -> SocketAddr
where
    F: Fn(Request<Incoming>) -> A + Clone + Send + Sync + 'static,
    A: Future<Output = Response<Body>> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let answer = answer.clone();
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |request| {
                    let answered = answer(request);
                    async move { Ok::<_, Infallible>(answered.await) }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .with_upgrades()
                    .await;
            });
        }
    });
    address
}

/// A body of `text`.
pub(crate) fn text(text: impl Into<Bytes>) -> Body {
    Full::new(text.into())
        .map_err(|never: Infallible| match never {})
        .boxed_unsync()
}

/// An answer of `status` and `body`.
pub(crate) fn answer(status: StatusCode, body: impl Into<Bytes>) -> Response<Body> {
    let mut response = Response::new(text(body));
    *response.status_mut() = status;
    response
}

/// A form as the token endpoint heard it: each name and value, in order.
pub(crate) type Form = Vec<(String, String)>;

/// What the token endpoint is to take, set by a test once it has read the sign-in's redirect.
#[derive(Clone)]
pub(crate) struct Expect {
    /// The S256 challenge the visitor was sent with, which the verifier must answer.
    pub(crate) challenge: String,
    /// The redirect URI the visitor was sent with, which the exchange must name again.
    pub(crate) redirect: String,
    /// What it answers a form that is right: its status and body.
    pub(crate) answer: (u16, String),
    /// Whether its code was taken: a code is taken once, and refused after.
    pub(crate) taken: bool,
}

/// Google's token endpoint, standing in.
pub(crate) struct Google {
    /// The endpoint's URL, as Google's is `https://oauth2.googleapis.com/token`.
    pub(crate) token: String,
    expect: Arc<Mutex<Option<Expect>>>,
    heard: Arc<Mutex<Vec<Form>>>,
}

impl Google {
    /// A stand-in that refuses every form until it is told what to take.
    pub(crate) async fn new() -> Google {
        let expect: Arc<Mutex<Option<Expect>>> = Arc::default();
        let heard: Arc<Mutex<Vec<Form>>> = Arc::default();
        let (taken, kept) = (expect.clone(), heard.clone());
        let address = serving(move |request: Request<Incoming>| {
            let (taken, kept) = (taken.clone(), kept.clone());
            async move {
                let body = request.into_body().collect().await.unwrap().to_bytes();
                let form: Form = form_urlencoded::parse(&body)
                    .map(|(name, value)| (name.into_owned(), value.into_owned()))
                    .collect();
                kept.lock().unwrap().push(form.clone());
                let field = |name: &str| {
                    form.iter()
                        .find(|(named, _)| named == name)
                        .map(|(_, value)| value.as_str())
                };
                let mut expected = taken.lock().unwrap();
                let Some(expect) = expected.as_mut() else {
                    return answer(StatusCode::BAD_REQUEST, r#"{"error":"invalid_request"}"#);
                };
                let verified = field("code_verifier").is_some_and(|verifier| {
                    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())) == expect.challenge
                });
                let right = field("grant_type") == Some("authorization_code")
                    && field("code") == Some(CODE)
                    && field("client_id") == Some(CLIENT_ID)
                    && field("client_secret") == Some(CLIENT_SECRET)
                    && field("redirect_uri") == Some(expect.redirect.as_str())
                    && verified
                    && !expect.taken;
                if !right {
                    return answer(StatusCode::BAD_REQUEST, r#"{"error":"invalid_grant"}"#);
                }
                expect.taken = true;
                let (status, body) = expect.answer.clone();
                answer(StatusCode::from_u16(status).unwrap(), body)
            }
        })
        .await;
        Google {
            token: format!("http://{address}/token"),
            expect,
            heard,
        }
    }

    /// The provider whose token endpoint is this stand-in, reached through `curl`.
    pub(crate) fn provider(&self) -> Provider {
        Provider::stand_in(&self.token, "curl")
    }

    /// From now, the exchange of [`CODE`] for the sign-in sent with `challenge` to come back to
    /// `redirect` is answered `status` and `body`, once.
    pub(crate) fn answers(&self, challenge: &str, redirect: &str, status: u16, body: &str) {
        *self.expect.lock().unwrap() = Some(Expect {
            challenge: challenge.into(),
            redirect: redirect.into(),
            answer: (status, body.into()),
            taken: false,
        });
    }

    /// From now, the exchange for the sign-in sent with `challenge` is answered with an id token
    /// bearing `claims`.
    pub(crate) fn signs_in(&self, challenge: &str, redirect: &str, claims: &serde_json::Value) {
        let body = serde_json::json!({
            "access_token": "ya29.stand-in",
            "expires_in": 3599,
            "token_type": "Bearer",
            "scope": "openid",
            "id_token": id_token(claims),
        });
        self.answers(challenge, redirect, 200, &body.to_string());
    }

    /// Every form it heard, in order.
    pub(crate) fn heard(&self) -> Vec<Form> {
        self.heard.lock().unwrap().clone()
    }
}

/// GitHub's token and user endpoints, standing in on one loopback port: the token endpoint takes
/// [`CODE`] once for the sign-in it is told of, with the app's id and secret, the way back and the
/// verifier of its challenge, and answers [`GITHUB_TOKEN`], as GitHub does `200` with an `error`
/// where it takes no code; the user endpoint answers whom [`GITHUB_TOKEN`] is for, Ana's
/// [`GITHUB_ID`] unless a test sets another answer, and `401` for any other token.
pub(crate) struct GitHub {
    /// The token endpoint's URL, as GitHub's is `https://github.com/login/oauth/access_token`.
    pub(crate) token: String,
    /// The user endpoint's URL, as GitHub's is `https://api.github.com/user`.
    pub(crate) user: String,
    expect: Arc<Mutex<Option<Expect>>>,
    says: Arc<Mutex<(u16, String)>>,
    heard: Arc<Mutex<Vec<Form>>>,
    asked: Arc<Mutex<Vec<HeaderMap>>>,
}

impl GitHub {
    /// A stand-in that takes no code until it is told what to take.
    pub(crate) async fn new() -> GitHub {
        let expect: Arc<Mutex<Option<Expect>>> = Arc::default();
        let says = Arc::new(Mutex::new((
            200,
            serde_json::json!({"login": "ana", "id": GITHUB_ID, "type": "User"}).to_string(),
        )));
        let heard: Arc<Mutex<Vec<Form>>> = Arc::default();
        let asked: Arc<Mutex<Vec<HeaderMap>>> = Arc::default();
        let kept = (expect.clone(), says.clone(), heard.clone(), asked.clone());
        let address = serving(move |request: Request<Incoming>| {
            let (expect, says, heard, asked) = kept.clone();
            async move {
                if request.uri().path() == "/user" {
                    asked.lock().unwrap().push(request.headers().clone());
                    let bearer = format!("Bearer {GITHUB_TOKEN}");
                    if request.method() != hyper::Method::GET
                        || request.headers().get(header::AUTHORIZATION)
                            != Some(&HeaderValue::from_str(&bearer).unwrap())
                    {
                        return answer(StatusCode::UNAUTHORIZED, r#"{"message":"Bad credentials"}"#);
                    }
                    let (status, body) = says.lock().unwrap().clone();
                    return answer(StatusCode::from_u16(status).unwrap(), body);
                }
                let body = request.into_body().collect().await.unwrap().to_bytes();
                let form: Form = form_urlencoded::parse(&body)
                    .map(|(name, value)| (name.into_owned(), value.into_owned()))
                    .collect();
                heard.lock().unwrap().push(form.clone());
                let field = |name: &str| {
                    form.iter()
                        .find(|(named, _)| named == name)
                        .map(|(_, value)| value.as_str())
                };
                let refused = r#"{"error":"bad_verification_code","error_description":"The code passed is incorrect or expired."}"#;
                let mut expected = expect.lock().unwrap();
                let Some(expect) = expected.as_mut() else {
                    return answer(StatusCode::OK, refused);
                };
                let verified = field("code_verifier").is_some_and(|verifier| {
                    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())) == expect.challenge
                });
                let right = field("code") == Some(CODE)
                    && field("client_id") == Some(GITHUB_CLIENT_ID)
                    && field("client_secret") == Some(GITHUB_CLIENT_SECRET)
                    && field("redirect_uri") == Some(expect.redirect.as_str())
                    && verified
                    && !expect.taken;
                if !right {
                    return answer(StatusCode::OK, refused);
                }
                expect.taken = true;
                let (status, body) = expect.answer.clone();
                answer(StatusCode::from_u16(status).unwrap(), body)
            }
        })
        .await;
        GitHub {
            token: format!("http://{address}/login/oauth/access_token"),
            user: format!("http://{address}/user"),
            expect,
            says,
            heard,
            asked,
        }
    }

    /// The provider whose endpoints are this stand-in's, reached through `curl`.
    pub(crate) fn provider(&self) -> github::Provider {
        github::Provider::stand_in(&self.token, &self.user, "curl")
    }

    /// From now, the exchange of [`CODE`] for the sign-in sent with `challenge` to come back to
    /// `redirect` is answered `status` and `body`, once.
    pub(crate) fn answers(&self, challenge: &str, redirect: &str, status: u16, body: &str) {
        *self.expect.lock().unwrap() = Some(Expect {
            challenge: challenge.into(),
            redirect: redirect.into(),
            answer: (status, body.into()),
            taken: false,
        });
    }

    /// From now, the exchange for the sign-in sent with `challenge` is answered with
    /// [`GITHUB_TOKEN`], as GitHub answers, once.
    pub(crate) fn signs_in(&self, challenge: &str, redirect: &str) {
        let body = serde_json::json!({
            "access_token": GITHUB_TOKEN,
            "token_type": "bearer",
            "scope": "",
        });
        self.answers(challenge, redirect, 200, &body.to_string());
    }

    /// From now, the user endpoint answers [`GITHUB_TOKEN`]'s asking with `status` and `body`.
    pub(crate) fn says(&self, status: u16, body: &str) {
        *self.says.lock().unwrap() = (status, body.into());
    }

    /// Every form the token endpoint heard, in order.
    pub(crate) fn heard(&self) -> Vec<Form> {
        self.heard.lock().unwrap().clone()
    }

    /// The headers of every request the user endpoint heard, in order.
    pub(crate) fn asked(&self) -> Vec<HeaderMap> {
        self.asked.lock().unwrap().clone()
    }
}

/// One request the server behind heard.
#[derive(Clone, Debug)]
pub(crate) struct Heard {
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) headers: HeaderMap,
    pub(crate) body: Bytes,
    /// Who the server's own check says the request is from, `SERVICE:SUBJECT`, or why it refused
    /// its word.
    pub(crate) who: Result<String, String>,
}

/// The server behind, standing in: every request's word verified by the server's own check, and
/// answered `200` saying who it is from, with each `X-Answer-Cache-Control` it was sent as its
/// `Cache-Control`; `/feed` an event stream that sends who it is from and
/// stays open; and `/socket`, a WebSocket's handshake, answered `101`, its connection sending who
/// it is from and echoing what it is sent.
pub(crate) struct Server {
    pub(crate) address: SocketAddr,
    heard: Arc<Mutex<Vec<Heard>>>,
}

impl Server {
    /// A server whose machine folder is `machine`.
    pub(crate) async fn new(machine: &Path) -> Server {
        let heard: Arc<Mutex<Vec<Heard>>> = Arc::default();
        let (kept, machine) = (heard.clone(), machine.to_path_buf());
        let address = serving(move |mut request: Request<Incoming>| {
            let (kept, machine) = (kept.clone(), machine.clone());
            async move {
                let who = checked(request.headers(), &machine);
                let upgrade = (request.headers().contains_key(header::UPGRADE))
                    .then(|| hyper::upgrade::on(&mut request));
                let (parts, body) = request.into_parts();
                let path = parts.uri.path().to_string();
                let body = if path == "/feed" || upgrade.is_some() {
                    Bytes::new()
                } else {
                    body.collect().await.unwrap().to_bytes()
                };
                kept.lock().unwrap().push(Heard {
                    method: parts.method.to_string(),
                    path: path.clone(),
                    headers: parts.headers.clone(),
                    body: body.clone(),
                    who: who.clone(),
                });
                let said = who.unwrap_or_else(|why| format!("refused: {why}"));
                if let (Some(upgrade), "/socket") = (upgrade, path.as_str()) {
                    tokio::spawn(async move {
                        let mut socket = TokioIo::new(upgrade.await.unwrap());
                        socket
                            .write_all(format!("{said}\n").as_bytes())
                            .await
                            .unwrap();
                        let mut buffer = [0; 1024];
                        loop {
                            match socket.read(&mut buffer).await {
                                Ok(0) | Err(_) => return,
                                Ok(read) => {
                                    if socket.write_all(&buffer[..read]).await.is_err() {
                                        return;
                                    }
                                }
                            }
                        }
                    });
                    let mut response = Response::new(
                        Empty::new()
                            .map_err(|never: Infallible| match never {})
                            .boxed_unsync(),
                    );
                    *response.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
                    let headers = response.headers_mut();
                    headers.insert(header::UPGRADE, HeaderValue::from_static("websocket"));
                    headers.insert(header::CONNECTION, HeaderValue::from_static("Upgrade"));
                    headers.insert("sec-websocket-accept", HeaderValue::from_static("stand-in"));
                    return response;
                }
                if path == "/feed" {
                    let first = format!("data: {said}\n\n");
                    let frames = futures_util::stream::once(async move {
                        Ok::<_, hyper::Error>(Frame::data(Bytes::from(first)))
                    })
                    .chain(futures_util::stream::pending());
                    let mut response = Response::new(StreamBody::new(frames).boxed_unsync());
                    response.headers_mut().insert(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static("text/event-stream"),
                    );
                    return response;
                }
                let mut response = answer(StatusCode::OK, said);
                for control in parts.headers.get_all("x-answer-cache-control") {
                    response
                        .headers_mut()
                        .append(header::CACHE_CONTROL, control.clone());
                }
                response.headers_mut().insert(
                    header::CONNECTION,
                    HeaderValue::from_static("keep-alive, x-dropped"),
                );
                response
                    .headers_mut()
                    .insert("x-dropped", HeaderValue::from_static("hop"));
                response
            }
        })
        .await;
        Server { address, heard }
    }

    /// Every request it heard, in order.
    pub(crate) fn heard(&self) -> Vec<Heard> {
        self.heard.lock().unwrap().clone()
    }
}

/// Who the word among `headers` is from, by the server's own check against `machine`'s
/// `sign-in.toml`: `SERVICE:SUBJECT`, or why it is refused.
fn checked(headers: &HeaderMap, machine: &Path) -> Result<String, String> {
    use symphony::signed_in::{self, Settings};
    let values: Vec<&[u8]> = (headers.get_all(signed_in::HEADER).iter())
        .map(HeaderValue::as_bytes)
        .collect();
    let word = signed_in::carried(&values)
        .ok_or("no word")?
        .map_err(|refused| format!("{refused:?}"))?;
    let settings = Settings::read(machine)?.ok_or("no sign-in.toml")?;
    let signed = signed_in::verify(word, &settings, SystemTime::now())
        .map_err(|refused| format!("{refused:?}"))?;
    Ok(format!("{}:{}", signed.service, signed.subject))
}

/// The answer to `request` sent to `address` on a connection of its own, its body read whole.
pub(crate) async fn send(address: SocketAddr, request: Request<Body>) -> Response<Bytes> {
    let response = open(address, request).await;
    let (parts, body) = response.into_parts();
    let body = tokio::time::timeout(BACKSTOP, body.collect())
        .await
        .unwrap()
        .unwrap()
        .to_bytes();
    Response::from_parts(parts, body)
}

/// The answer to `request` sent to `address` on a connection of its own, its body still to read.
pub(crate) async fn open(address: SocketAddr, request: Request<Body>) -> Response<Incoming> {
    let stream = TcpStream::connect(address).await.unwrap();
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = connection.with_upgrades().await;
    });
    tokio::time::timeout(BACKSTOP, sender.send_request(request))
        .await
        .unwrap()
        .unwrap()
}
