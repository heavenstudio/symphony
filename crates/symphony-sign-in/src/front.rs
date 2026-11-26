//! The listener (`docs/operations/SIGN_IN_SERVICE.md`): plain HTTP/1.1 on the settings' `listen`,
//! in front of the server at `upstream`.
//!
//! - **Its own paths**, under [`PREFIX`], are answered here and never passed on: the provider's way
//!   back ([`CALLBACK`]), the visitor's own page ([`PREFIX`] itself: whom they are signed in as,
//!   and sign-out) and sign-out ([`SIGN_OUT`]).
//! - **Every other request needs a session.** With one, every `Symphony-Signed-In` the caller sent
//!   is dropped, and so is every cookie (the server reads none) and every hop-by-hop header; `Host`
//!   is the origin's, and a word signed for it afresh is added. The request goes to the server on a
//!   connection of its own, its body streamed, and the server's answer comes back streamed. A
//!   WebSocket's handshake the server answers `101` is spliced both ways; it and an event stream
//!   end at their word's expiry, and the client reconnects with a fresh word while the session
//!   holds.
//! - **Without a session nothing is passed on.** A browser's navigation is sent to sign in, its
//!   path kept to come back to; anything else, a fetch, a script, a socket, is answered `401`.
//!
//! The session cookie and the flow cookie are `HttpOnly` and `SameSite=Lax`, and for an `https://`
//! origin `Secure` and `__Host-`: a cross-site POST, fetch or socket carries neither, so it is never
//! signed, and the server refuses a signed write or socket whose `Origin` is not the origin.
//! Sign-out takes only a POST naming the origin. Where a visitor comes back to is always a path of
//! the origin, the one they asked for; no parameter names one.

use crate::PREFIX;
use crate::google::{self, Provider};
use crate::kept::{FLOW_SECONDS, Kept, SESSION_SECONDS};
use crate::settings::{self, Key, Settings};
use crate::word::{self, Who};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt as _, Empty, Full, StreamBody};
use hyper::body::{Bytes, Incoming};
use hyper::header::{self, HeaderMap, HeaderName, HeaderValue};
use hyper::upgrade::OnUpgrade;
use hyper::{Method, Request, Response, StatusCode, Uri, Version};
use hyper_util::rt::TokioIo;
use sha2::{Digest as _, Sha256};
use std::convert::Infallible;
use std::fmt;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::Instant;

/// The provider's way back, registered with it as the redirect URI.
pub(crate) const CALLBACK: &str = "/.sign-in/callback";
/// Where the visitor's own page posts to sign out.
pub(crate) const SIGN_OUT: &str = "/.sign-in/sign-out";
/// The session cookie's name, `__Host-` before it for an `https://` origin.
const SESSION: &str = "symphony-sign-in";
/// The flow cookie's name, which holds a sign-in's `state` until its way back.
const FLOW: &str = "symphony-sign-in-flow";
/// The header a fetch says its mode in.
const FETCH_MODE: &str = "sec-fetch-mode";

/// A response's body: the server's, passed on, or the service's own.
pub(crate) type Body = UnsyncBoxBody<Bytes, hyper::Error>;

/// Where what the service says goes: its standard error, or a test's buffer.
pub(crate) type Log = Arc<Mutex<dyn Write + Send>>;

/// Signs visitors in with the machine folder `machine`'s settings and key, and passes their
/// requests on, until the process ends.
///
/// # Errors
///
/// The settings or the key do not read, the server's `sign-in.toml` beside them names another key
/// or origin, or `listen` cannot be bound.
pub(crate) fn serve(machine: &Path) -> Result<(), String> {
    let settings = Settings::read(machine)?;
    let key = Key::read(machine)?;
    settings::matches_server(machine, &key, &settings.origin)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("the service's runtime did not start: {error}"))?;
    runtime.block_on(async {
        let listen = settings.listen;
        let listener = TcpListener::bind(listen)
            .await
            .map_err(|error| format!("listen {listen}: {error}"))?;
        let log: Log = Arc::new(Mutex::new(std::io::stderr()));
        let front = Arc::new(Front::new(settings, key, Provider::google(), log)?);
        front.say(format_args!(
            "listening on {listen} for {}, passing on to {}",
            front.settings.origin, front.settings.upstream
        ));
        front.listen(listener).await;
        Ok(())
    })
}

/// The service, as its listener answers each request.
pub(crate) struct Front {
    settings: Settings,
    key: Key,
    provider: Provider,
    kept: Mutex<Kept>,
    log: Log,
    /// The origin's host and port, which every request passed on names in its `Host`.
    host: HeaderValue,
    /// Whether the origin is `https://`, and so the cookies `Secure` and `__Host-`.
    secure: bool,
    /// How long a stream passed on stays open: its word's lifetime.
    streams: Duration,
}

impl Front {
    /// The service of `settings`, signing with `key`, signing visitors in at `provider` and
    /// saying what it does to `log`.
    ///
    /// # Errors
    ///
    /// The origin's host is no header value.
    pub(crate) fn new(
        settings: Settings,
        key: Key,
        provider: Provider,
        log: Log,
    ) -> Result<Front, String> {
        let (secure, authority) = match settings.origin.strip_prefix("https://") {
            Some(authority) => (true, authority),
            None => (false, settings.origin.trim_start_matches("http://")),
        };
        let host = HeaderValue::from_str(authority)
            .map_err(|error| format!("the origin's host is no header value: {error}"))?;
        Ok(Front {
            settings,
            key,
            provider,
            kept: Mutex::new(Kept::default()),
            log,
            host,
            secure,
            streams: Duration::from_secs(word::LIFETIME),
        })
    }

    /// The same, with its streams ended after `streams` rather than their word's lifetime.
    #[cfg(test)]
    pub(crate) fn streams(mut self, streams: Duration) -> Front {
        self.streams = streams;
        self
    }

    /// Answers every connection `listener` accepts, each on a task of its own.
    pub(crate) async fn listen(self: Arc<Self>, listener: TcpListener) {
        loop {
            let stream = match listener.accept().await {
                Ok((stream, _)) => stream,
                Err(error) => {
                    self.say(format_args!("a connection was not accepted: {error}"));
                    continue;
                }
            };
            let front = Arc::clone(&self);
            tokio::spawn(async move {
                let answer = hyper::service::service_fn(move |request| {
                    let front = Arc::clone(&front);
                    async move { Ok::<_, Infallible>(front.answer(request).await) }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), answer)
                    .with_upgrades()
                    .await;
            });
        }
    }

    /// The answer to `request`.
    async fn answer(&self, request: Request<Incoming>) -> Response<Body> {
        let path = request.uri().path().to_string();
        if path == PREFIX {
            return self.page(&request);
        }
        if path == CALLBACK {
            return self.callback(&request).await;
        }
        if path == SIGN_OUT {
            return self.sign_out(&request);
        }
        if path.starts_with(PREFIX) || path == PREFIX.trim_end_matches('/') {
            return plain(StatusCode::NOT_FOUND, "Not Found\n");
        }
        match self.session(request.headers()) {
            Some(who) => self.pass(request, &who).await,
            None => self.unsigned(&request),
        }
    }

    /// The visitor's own page: whom they are signed in as, and sign-out.
    fn page(&self, request: &Request<Incoming>) -> Response<Body> {
        if !matches!(*request.method(), Method::GET | Method::HEAD) {
            return not_allowed("GET, HEAD");
        }
        let Some(who) = self.session(request.headers()) else {
            return self.unsigned(request);
        };
        html(
            StatusCode::OK,
            "Signed in",
            &format!(
                "<p>Signed in as <code>{}</code></p>\n\
                 <form method=\"post\" action=\"{SIGN_OUT}\"><button>Sign out</button></form>",
                escaped(&who.signin())
            ),
        )
    }

    /// The answer to `request` with no session: a navigation sent to sign in, to come back to its
    /// own path; anything else refused.
    fn unsigned(&self, request: &Request<Incoming>) -> Response<Body> {
        if !navigation(request) {
            return plain(StatusCode::UNAUTHORIZED, "Unauthorized: not signed in\n");
        }
        let to = (request.uri().path_and_query())
            .map(hyper::http::uri::PathAndQuery::as_str)
            .filter(|asked| asked.starts_with('/'))
            .unwrap_or("/");
        let flow = match self.kept().begin(to.into(), now()) {
            Ok(flow) => flow,
            Err(error) => {
                self.say(format_args!("a sign-in did not begin: {error}"));
                return plain(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal Server Error: the sign-in did not begin\n",
                );
            }
        };
        let to = self
            .provider
            .authorize(&self.settings.google, &self.redirect(), &flow);
        let mut response = empty(StatusCode::FOUND);
        let headers = response.headers_mut();
        insert(headers, header::LOCATION, &to);
        append(
            headers,
            header::SET_COOKIE,
            &self.cookie(FLOW, &flow.state, FLOW_SECONDS),
        );
        response
    }

    /// The provider's way back: taken once, from the browser whose sign-in it is, its code
    /// exchanged and its id token checked, and a session made; refused otherwise.
    async fn callback(&self, request: &Request<Incoming>) -> Response<Body> {
        if request.method() != Method::GET {
            return not_allowed("GET");
        }
        let mut asked: [Option<String>; 3] = [None, None, None];
        let query = request.uri().query().unwrap_or_default();
        for (name, value) in form_urlencoded::parse(query.as_bytes()) {
            let Some(at) = ["state", "code", "error"]
                .iter()
                .position(|known| *known == name)
            else {
                continue;
            };
            if let Some(slot) = asked.get_mut(at) {
                if slot.is_some() {
                    return self.refuse("its query names a parameter twice");
                }
                *slot = Some(value.into_owned());
            }
        }
        let [state, code, error] = asked;
        let Some(state) = state else {
            return self.refuse("its query names no state");
        };
        let cookie = cookie(request.headers(), &self.name(FLOW));
        let flow = match self.kept().take(&state, cookie.as_deref(), now()) {
            Ok(flow) => flow,
            Err(why) => return self.refuse(why),
        };
        if error.is_some() {
            return self.refuse("Google did not sign the visitor in");
        }
        let Some(code) = code else {
            return self.refuse("its query names no code");
        };
        let (provider, client, redirect) = (
            self.provider.clone(),
            self.settings.google.clone(),
            self.redirect(),
        );
        let verifier = flow.verifier.clone();
        let exchanged = tokio::task::spawn_blocking(move || {
            provider.exchange(&client, &redirect, &code, &verifier)
        })
        .await;
        let token = match exchanged {
            Ok(Ok(token)) => token,
            Ok(Err(why)) => return self.unexchanged(format_args!("{why}")),
            Err(error) => return self.unexchanged(format_args!("its exchange ended: {error}")),
        };
        let subject = match google::subject(&token, &self.settings.google.id, &flow.nonce, now()) {
            Ok(subject) => subject,
            Err(why) => return self.refuse(why),
        };
        let who = Who {
            service: google::SERVICE,
            subject,
        };
        let id = match self.kept().open(who, now()) {
            Ok(id) => id,
            Err(error) => {
                self.say(format_args!("a session was not made: {error}"));
                return plain(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal Server Error: the session was not made\n",
                );
            }
        };
        let mut response = empty(StatusCode::SEE_OTHER);
        let headers = response.headers_mut();
        insert(
            headers,
            header::LOCATION,
            &format!("{}{}", self.settings.origin, flow.to),
        );
        append(
            headers,
            header::SET_COOKIE,
            &self.cookie(SESSION, &id, SESSION_SECONDS),
        );
        append(headers, header::SET_COOKIE, &self.cookie(FLOW, "", 0));
        response
    }

    /// A way back refused for `why`, said in the log and on a page that offers to begin again.
    fn refuse(&self, why: &str) -> Response<Body> {
        self.say(format_args!("refused a sign-in: {why}"));
        html(
            StatusCode::BAD_REQUEST,
            "Sign-in refused",
            "<p>This sign-in is not taken.</p>\n<p><a href=\"/\">Sign in again</a></p>",
        )
    }

    /// A way back whose code was not exchanged, for `why`.
    fn unexchanged(&self, why: fmt::Arguments<'_>) -> Response<Body> {
        self.say(format_args!("a sign-in's code was not exchanged: {why}"));
        html(
            StatusCode::BAD_GATEWAY,
            "Sign-in failed",
            "<p>Google did not answer the sign-in.</p>\n<p><a href=\"/\">Sign in again</a></p>",
        )
    }

    /// Sign-out: a POST naming the origin ends the browser's session.
    fn sign_out(&self, request: &Request<Incoming>) -> Response<Body> {
        if request.method() != Method::POST {
            return not_allowed("POST");
        }
        let origin = request.headers().get(header::ORIGIN);
        if origin.map(HeaderValue::as_bytes) != Some(self.settings.origin.as_bytes()) {
            self.say(format_args!(
                "refused a sign-out that names another origin or none"
            ));
            return plain(
                StatusCode::FORBIDDEN,
                "Forbidden: a sign-out from another site is not taken\n",
            );
        }
        if let Some(id) = cookie(request.headers(), &self.name(SESSION)) {
            self.kept().close(&id);
        }
        let mut response = html(
            StatusCode::OK,
            "Signed out",
            "<p>Signed out</p>\n<p><a href=\"/\">Sign in</a></p>",
        );
        append(
            response.headers_mut(),
            header::SET_COOKIE,
            &self.cookie(SESSION, "", 0),
        );
        response
    }

    /// `request` passed on to the server as `who`, and the server's answer.
    async fn pass(&self, mut request: Request<Incoming>, who: &Who) -> Response<Body> {
        let upgrade = upgrading(request.headers()).then(|| hyper::upgrade::on(&mut request));
        let deadline = Instant::now() + self.streams;
        let Ok(word) =
            HeaderValue::from_str(&word::signed(&self.key, who, &self.settings.origin, now()))
        else {
            return plain(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error: the request was not signed\n",
            );
        };
        let target = (request.uri().path_and_query())
            .map(hyper::http::uri::PathAndQuery::as_str)
            .filter(|asked| asked.starts_with('/'))
            .unwrap_or("/")
            .parse::<Uri>()
            .unwrap_or_default();
        *request.uri_mut() = target;
        *request.version_mut() = Version::HTTP_11;
        let headers = request.headers_mut();
        hop_by_hop(headers, upgrade.is_some());
        headers.remove(word::HEADER);
        headers.remove(header::COOKIE);
        headers.insert(header::HOST, self.host.clone());
        headers.insert(word::HEADER, word);
        let mut response = match self.send(request).await {
            Ok(response) => response,
            Err(why) => {
                self.say(format_args!("{why}"));
                return plain(
                    StatusCode::BAD_GATEWAY,
                    "Bad Gateway: the server behind does not answer\n",
                );
            }
        };
        if response.status() == StatusCode::SWITCHING_PROTOCOLS {
            if let Some(client) = upgrade {
                tokio::spawn(splice(client, hyper::upgrade::on(&mut response), deadline));
            }
            let (parts, _) = response.into_parts();
            return Response::from_parts(parts, Empty::new().map_err(never).boxed_unsync());
        }
        hop_by_hop(response.headers_mut(), false);
        let stream = (response.headers().get(header::CONTENT_TYPE))
            .is_some_and(|kind| kind.as_bytes().starts_with(b"text/event-stream"));
        response.map(|body| {
            if stream {
                until(body, deadline)
            } else {
                body.boxed_unsync()
            }
        })
    }

    /// The server's answer to `request`, on a connection of its own.
    async fn send(&self, request: Request<Incoming>) -> Result<Response<Incoming>, String> {
        let upstream = self.settings.upstream;
        let unreached = |error: &dyn fmt::Display| {
            format!("the server at {upstream} did not answer a request passed on: {error}")
        };
        let stream = TcpStream::connect(upstream)
            .await
            .map_err(|error| unreached(&error))?;
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|error| unreached(&error))?;
        tokio::spawn(async move {
            let _ = connection.with_upgrades().await;
        });
        sender
            .send_request(request)
            .await
            .map_err(|error| unreached(&error))
    }

    /// Who the session the browser's cookie in `headers` names is, now.
    fn session(&self, headers: &HeaderMap) -> Option<Who> {
        let id = cookie(headers, &self.name(SESSION))?;
        self.kept().who(&id, now())
    }

    /// The provider's way back, as it is registered.
    fn redirect(&self) -> String {
        format!("{}{CALLBACK}", self.settings.origin)
    }

    /// The cookie `name` as this origin names it.
    fn name(&self, name: &str) -> String {
        if self.secure {
            format!("__Host-{name}")
        } else {
            name.into()
        }
    }

    /// The `Set-Cookie` of the cookie `name` holding `value` for `seconds`; none ends it.
    fn cookie(&self, name: &str, value: &str, seconds: u64) -> String {
        format!(
            "{}={value}; Max-Age={seconds}; Path=/; HttpOnly; SameSite=Lax{}",
            self.name(name),
            if self.secure { "; Secure" } else { "" }
        )
    }

    fn kept(&self) -> MutexGuard<'_, Kept> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// One line of the log. Nothing said names a secret, a code, a token, a cookie or a word.
    fn say(&self, line: fmt::Arguments<'_>) {
        let mut log = self.log.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = writeln!(log, "symphony-sign-in: {line}");
    }
}

/// Splices a WebSocket's two upgraded connections, the browser's and the server's, until either
/// ends or `deadline` passes.
async fn splice(client: OnUpgrade, server: OnUpgrade, deadline: Instant) {
    let (Ok(client), Ok(server)) = tokio::join!(client, server) else {
        return;
    };
    let (mut client, mut server) = (TokioIo::new(client), TokioIo::new(server));
    let _ = tokio::time::timeout_at(
        deadline,
        tokio::io::copy_bidirectional(&mut client, &mut server),
    )
    .await;
}

/// `body` passed on frame by frame, ended when `deadline` passes.
fn until(body: Incoming, deadline: Instant) -> Body {
    let frames = futures_util::stream::unfold(Some(body), move |body| async move {
        let mut body = body?;
        match tokio::time::timeout_at(deadline, body.frame()).await {
            Ok(Some(frame)) => Some((frame, Some(body))),
            Ok(None) | Err(_) => None,
        }
    });
    StreamBody::new(frames).boxed_unsync()
}

/// Whether `headers` ask for an upgrade: an `Upgrade`, and `upgrade` among `Connection`'s.
fn upgrading(headers: &HeaderMap) -> bool {
    headers.contains_key(header::UPGRADE)
        && (headers.get_all(header::CONNECTION).iter())
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(','))
            .any(|name| name.trim().eq_ignore_ascii_case("upgrade"))
}

/// Drops from `headers` what is said of one connection alone: each header `Connection` names and
/// the standard ones, but for an `upgrade` its `Upgrade`, with `Connection: upgrade` again.
fn hop_by_hop(headers: &mut HeaderMap, upgrade: bool) {
    let named: Vec<HeaderName> = (headers.get_all(header::CONNECTION).iter())
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|name| HeaderName::from_bytes(name.trim().as_bytes()).ok())
        .collect();
    for name in named {
        if !(upgrade && name == header::UPGRADE) {
            headers.remove(name);
        }
    }
    for name in [
        header::CONNECTION,
        HeaderName::from_static("keep-alive"),
        header::PROXY_AUTHENTICATE,
        header::PROXY_AUTHORIZATION,
        HeaderName::from_static("proxy-connection"),
        header::TE,
        header::TRAILER,
        header::TRANSFER_ENCODING,
    ] {
        headers.remove(name);
    }
    if upgrade {
        headers.insert(header::CONNECTION, HeaderValue::from_static("upgrade"));
    } else {
        headers.remove(header::UPGRADE);
    }
}

/// Whether `request` is a browser's navigation, which may be sent to sign in: a GET or a HEAD that
/// says its fetch mode is `navigate`, or, from a browser that sends no fetch metadata, asks for
/// HTML.
fn navigation(request: &Request<Incoming>) -> bool {
    let headers = request.headers();
    matches!(*request.method(), Method::GET | Method::HEAD)
        && !upgrading(headers)
        && match headers.get(FETCH_MODE) {
            Some(mode) => mode.as_bytes() == b"navigate",
            None => (headers.get(header::ACCEPT))
                .and_then(|accept| accept.to_str().ok())
                .is_some_and(|accept| accept.contains("text/html")),
        }
}

/// The value of the cookie `name` among `headers`' cookies: the first one, where several are sent.
fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    (headers.get_all(header::COOKIE).iter())
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(named, _)| *named == name)
        .map(|(_, value)| value.to_string())
}

/// The seconds since 1970, now.
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// An answer of `status` with no body, kept out of every cache, whose redirect names no page it
/// came from.
fn empty(status: StatusCode) -> Response<Body> {
    let mut response = Response::new(Empty::new().map_err(never).boxed_unsync());
    *response.status_mut() = status;
    insert(response.headers_mut(), header::CACHE_CONTROL, "no-store");
    insert(
        response.headers_mut(),
        header::REFERRER_POLICY,
        "no-referrer",
    );
    response
}

/// An answer of `status` whose body is the text `text`.
fn plain(status: StatusCode, text: &'static str) -> Response<Body> {
    let mut response = empty(status);
    *response.body_mut() = Full::new(Bytes::from_static(text.as_bytes()))
        .map_err(never)
        .boxed_unsync();
    insert(
        response.headers_mut(),
        header::CONTENT_TYPE,
        "text/plain; charset=utf-8",
    );
    response
}

/// A refusal of a method `allow` does not name.
fn not_allowed(allow: &str) -> Response<Body> {
    let mut response = plain(StatusCode::METHOD_NOT_ALLOWED, "Method Not Allowed\n");
    insert(response.headers_mut(), header::ALLOW, allow);
    response
}

/// The pages' one style: the system's font, and a button a finger can press. The page takes the
/// visitor's light or dark scheme from its `color-scheme`.
const STYLE: &str = "body{margin:0;padding:24px 16px;font:16px/1.5 system-ui,sans-serif}\
                     code{overflow-wrap:anywhere}\
                     button{min-height:44px;padding:0 20px;font:inherit}";

/// The pages' policy: they load nothing and are framed by no site, and the one style they apply
/// is [`STYLE`], allowed by its digest.
static POLICY: LazyLock<String> = LazyLock::new(|| {
    format!(
        "default-src 'none'; style-src 'sha256-{}'; form-action 'self'; frame-ancestors 'none'; \
         base-uri 'none'",
        STANDARD.encode(Sha256::digest(STYLE.as_bytes()))
    )
});

/// One of the service's pages: `title`, and `body`, HTML that loads nothing, framed by no site.
fn html(status: StatusCode, title: &str, body: &str) -> Response<Body> {
    let mut response = empty(status);
    *response.body_mut() = Full::new(Bytes::from(format!(
        "<!doctype html>\n<html lang=\"en\">\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width\">\n\
         <meta name=\"color-scheme\" content=\"light dark\">\n<title>{title}</title>\n\
         <style>{STYLE}</style>\n{body}\n"
    )))
    .map_err(never)
    .boxed_unsync();
    let headers = response.headers_mut();
    insert(headers, header::CONTENT_TYPE, "text/html; charset=utf-8");
    insert(headers, header::CONTENT_SECURITY_POLICY, &POLICY);
    insert(headers, header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    // A page's own requests name its origin and no path or query, as the refusal page's address
    // holds a code; a browser names it in the sign-out's `Origin` too, which `no-referrer` makes
    // `null`.
    insert(headers, header::REFERRER_POLICY, "strict-origin");
    response
}

/// `text` as HTML text.
fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Sets `name` to `value` in `headers`, where `value` is a header value; the service's own values
/// all are.
fn insert(headers: &mut HeaderMap, name: HeaderName, value: &str) {
    if let Ok(value) = HeaderValue::from_str(value) {
        headers.insert(name, value);
    }
}

/// Adds `value` to `name` in `headers`, as [`insert`].
fn append(headers: &mut HeaderMap, name: HeaderName, value: &str) {
    if let Ok(value) = HeaderValue::from_str(value) {
        headers.append(name, value);
    }
}

/// The error of a body that cannot fail.
fn never(never: Infallible) -> hyper::Error {
    match never {}
}

#[cfg(test)]
#[path = "front_tests.rs"]
mod tests;
