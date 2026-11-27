//! The feed on a WebSocket: the same events as its Server-Sent Events, from the same `?since=`,
//! each one text message ([`Said::text`]), for a request that asks to upgrade. A browser counts
//! each WebSocket apart from its six HTTP/1.1 connections a host, which a stream of Server-Sent
//! Events holds one of for as long as it is open, so a tab following the board on one leaves every
//! connection to its reads, however many tabs of the server are open (`symphony_api`'s feed
//! contract).
//!
//! A WebSocket has no same-origin rule for what it reads: a page of any site can open one to this
//! server, and the browser names that site in its `Origin`. So an upgrade whose `Origin` is not
//! `http://` and its own `Host` is refused, as such a write is (`crate::write`); the `Host` was
//! checked before the route ran, as every read's is.

use axum::extract::FromRequestParts;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::{Stream, StreamExt};

use super::{Request, error};

/// One thing the feed says: a delta at its version, a ping, or a notice of the server's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Said {
    name: &'static str,
    /// A delta's version, its Server-Sent Event's `id`.
    id: Option<u64>,
    data: String,
}

impl Said {
    /// The board's event `data` (JSON) at `version`.
    pub(super) fn delta(version: u64, data: String) -> Self {
        Self {
            name: symphony_api::FEED_EVENT,
            id: Some(version),
            data,
        }
    }

    /// The server is there: an event with no data.
    pub(super) fn ping() -> Self {
        Self {
            name: symphony_api::PING_EVENT,
            id: None,
            data: String::new(),
        }
    }

    /// The server's `message` to every page, as a JSON string.
    pub(super) fn notice(message: &str) -> Self {
        Self {
            name: symphony_api::SERVER_EVENT,
            id: None,
            data: serde_json::to_string(message).expect("a string serializes"),
        }
    }

    /// As a Server-Sent Event. A ping's data line is there with nothing on it: a browser
    /// dispatches no event that has no data field.
    pub(super) fn sse(&self) -> String {
        let id = self.id.map(|id| format!("id: {id}\n")).unwrap_or_default();
        if self.data.is_empty() {
            format!("event: {}\n{id}data:\n\n", self.name)
        } else {
            format!("event: {}\n{id}data: {}\n\n", self.name, self.data)
        }
    }

    /// As one WebSocket text message: the event's name, a line feed, then its data. No name holds
    /// a line feed, and the data is the event's JSON on one line, or nothing for a ping.
    pub(super) fn text(&self) -> String {
        format!("{}\n{}", self.name, self.data)
    }
}

/// Whether `request` asks for the feed on a WebSocket (`Upgrade: websocket`).
pub(super) fn asked(request: &Request) -> bool {
    request
        .headers()
        .get(header::UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("websocket"))
}

/// Whether a WebSocket's `Origin`, when it names one, is this server's own page: `http://` and the
/// request's `Host`, or the origin its signed word was made for ([`crate::front::own_origin`]).
fn own_origin(request: &Request) -> bool {
    // The hosted boundary already required its exact configured HTTPS Origin before any read.
    if crate::hosted::admission(request).is_some() {
        return true;
    }
    let Some(origin) = request.headers().get(header::ORIGIN) else {
        return true;
    };
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let signed = crate::front::Admitted::origin(request.extensions());
    origin
        .to_str()
        .is_ok_and(|origin| crate::front::own_origin(origin, host, signed.as_deref()))
}

/// The refusal of an upgrade from another site's page, answered before the feed reads anything.
pub(super) fn refused(request: &Request) -> Option<Response> {
    (asked(request) && !own_origin(request)).then(|| {
        error(
            StatusCode::FORBIDDEN,
            "Forbidden: a WebSocket's Origin must be this server's own page",
        )
    })
}

/// The upgrade's answer, once [`refused`] did not refuse it, the socket then carrying what `said`
/// says until it ends or the page closes it; or, when it is not a WebSocket's handshake, the
/// extractor's own refusal.
pub(super) async fn answer(
    request: Request,
    said: impl Stream<Item = Said> + Send + 'static,
) -> Response {
    let (mut parts, ()) = request.into_parts();
    match WebSocketUpgrade::from_request_parts(&mut parts, &()).await {
        Ok(upgrade) => upgrade.on_upgrade(move |socket| pump(socket, said)),
        Err(refused) => refused.into_response(),
    }
}

/// What `said` says, one message each, then a close when it ends. The page says nothing on it: a
/// close, a failure or anything but a ping or a pong from it ends the feed.
async fn pump(mut socket: WebSocket, said: impl Stream<Item = Said> + Send + 'static) {
    let mut said = std::pin::pin!(said);
    loop {
        tokio::select! {
            next = said.next() => match next {
                Some(next) => {
                    if socket.send(Message::Text(next.text().into())).await.is_err() {
                        return;
                    }
                }
                None => {
                    let _ = socket.send(Message::Close(None)).await;
                    return;
                }
            },
            heard = socket.recv() => match heard {
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                _ => return,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use futures_util::stream::BoxStream;
    use std::net::{SocketAddr, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// The wall-clock backstop of a wait here, which only a stall reaches.
    const BACKSTOP: Duration = test_time::backstop(Duration::from_secs(10));

    /// A server on a private port answering its one request with [`answer`], saying what `said`
    /// says.
    async fn serving(said: impl Stream<Item = Said> + Send + 'static) -> SocketAddr {
        let said: Arc<Mutex<Option<BoxStream<'static, Said>>>> =
            Arc::new(Mutex::new(Some(said.boxed())));
        let app = axum::Router::new().fallback(move |request: axum::extract::Request| {
            let said = said.lock().unwrap().take().expect("one feed a server");
            async move { answer(request.map(|_| ()), said).await }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        address
    }

    /// A page's socket on the feed at `address`, its handshake done.
    fn page(address: SocketAddr) -> tungstenite::WebSocket<TcpStream> {
        let stream = TcpStream::connect(address).unwrap();
        stream.set_read_timeout(Some(BACKSTOP)).unwrap();
        let (socket, answered) =
            tungstenite::client(format!("ws://{address}/api/feed?since=0"), stream).unwrap();
        assert_eq!(answered.status(), StatusCode::SWITCHING_PROTOCOLS);
        socket
    }

    /// The next message the page hears.
    fn heard(socket: &mut tungstenite::WebSocket<TcpStream>) -> tungstenite::Message {
        socket.read().unwrap()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_socket_says_each_event_as_one_message_then_closes() {
        let address = serving(futures_util::stream::iter([
            Said::notice("Serving"),
            Said::delta(3, r#"{"a":1}"#.into()),
            Said::ping(),
        ]))
        .await;
        let heard = tokio::task::spawn_blocking(move || {
            let mut socket = page(address);
            std::iter::from_fn(|| match heard(&mut socket) {
                tungstenite::Message::Text(text) => Some(text.to_string()),
                tungstenite::Message::Close(_) => None,
                other => panic!("not a feed's message: {other:?}"),
            })
            .collect::<Vec<_>>()
        })
        .await
        .unwrap();
        assert_eq!(heard, ["server\n\"Serving\"", "delta\n{\"a\":1}", "ping\n"]);
    }

    /// A feed told what to say by the test, followed by a page that heard its first event and then
    /// did `then` on its socket: the feed's teller (closed once the server dropped the feed) and the
    /// page's socket.
    async fn followed(
        then: fn(&mut tungstenite::WebSocket<TcpStream>),
    ) -> (
        tokio::sync::mpsc::Sender<Said>,
        tungstenite::WebSocket<TcpStream>,
    ) {
        let (tell, told) = tokio::sync::mpsc::channel::<Said>(1);
        let said = futures_util::stream::unfold(told, |mut told| async move {
            told.recv().await.map(|said| (said, told))
        });
        let address = serving(said).await;
        tell.send(Said::ping()).await.unwrap();
        let socket = tokio::task::spawn_blocking(move || {
            let mut socket = page(address);
            assert_eq!(heard(&mut socket), tungstenite::Message::text("ping\n"));
            then(&mut socket);
            socket
        })
        .await
        .unwrap();
        (tell, socket)
    }

    /// Whether the server dropped the feed `tell` tells, waited for up to the backstop.
    async fn dropped(tell: &tokio::sync::mpsc::Sender<Said>) -> bool {
        tokio::time::timeout(BACKSTOP, tell.closed()).await.is_ok()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_ping_of_the_pages_is_answered_and_the_feed_goes_on() {
        let (tell, mut socket) = followed(|socket| {
            socket
                .send(tungstenite::Message::Ping(vec![7].into()))
                .unwrap();
            assert_eq!(heard(socket), tungstenite::Message::Pong(vec![7].into()));
        })
        .await;
        tell.send(Said::notice("still here")).await.unwrap();
        let next = tokio::task::spawn_blocking(move || heard(&mut socket))
            .await
            .unwrap();
        assert_eq!(next, tungstenite::Message::text("server\n\"still here\""));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_page_closing_its_socket_ends_the_feed() {
        let (tell, _socket) = followed(|socket| {
            socket.close(None).unwrap();
            socket.flush().unwrap();
        })
        .await;
        assert!(dropped(&tell).await, "the feed outlived its page's close");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_page_that_says_anything_ends_the_feed() {
        let (tell, _socket) = followed(|socket| {
            socket.send(tungstenite::Message::text("since=0")).unwrap();
        })
        .await;
        assert!(dropped(&tell).await, "the feed outlived what its page said");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_page_gone_ends_the_feed() {
        let (tell, socket) = followed(|_| {}).await;
        drop(socket);
        assert!(
            dropped(&tell).await,
            "the feed outlived its page's connection"
        );
    }

    fn request(headers: &[(header::HeaderName, &str)]) -> Request {
        let mut request = Request::new(());
        for (name, value) in headers {
            request
                .headers_mut()
                .append(name, HeaderValue::from_str(value).unwrap());
        }
        request
    }

    #[test]
    fn server_sent_events_keep_their_bytes() {
        assert_eq!(
            Said::delta(7, r#"{"kind":"card"}"#.into()).sse(),
            "event: delta\nid: 7\ndata: {\"kind\":\"card\"}\n\n"
        );
        assert_eq!(Said::ping().sse(), "event: ping\ndata:\n\n");
        assert_eq!(
            Said::notice("Rebuilt: \"x\"\nreload").sse(),
            "event: server\ndata: \"Rebuilt: \\\"x\\\"\\nreload\"\n\n"
        );
        // A delta of version 0 still names its id.
        assert_eq!(
            Said::delta(0, "{}".into()).sse(),
            "event: delta\nid: 0\ndata: {}\n\n"
        );
    }

    #[test]
    fn a_message_is_the_event_name_a_line_feed_and_its_data() {
        assert_eq!(
            Said::delta(7, "{\"a\":1}".into()).text(),
            "delta\n{\"a\":1}"
        );
        assert_eq!(Said::ping().text(), "ping\n");
        assert_eq!(
            Said::notice("line\nbreak").text(),
            "server\n\"line\\nbreak\""
        );
        // The data never holds a line feed of its own: a notice's is escaped in its JSON.
        assert!(
            !Said::notice("a\nb")
                .text()
                .split_once('\n')
                .unwrap()
                .1
                .contains('\n')
        );
    }

    #[test]
    fn the_names_are_the_wire_events() {
        assert_eq!(Said::delta(1, String::new()).name, symphony_api::FEED_EVENT);
        assert_eq!(Said::ping().name, symphony_api::PING_EVENT);
        assert_eq!(Said::notice("").name, symphony_api::SERVER_EVENT);
    }

    #[test]
    fn only_a_websocket_upgrade_is_asked() {
        assert!(asked(&request(&[(header::UPGRADE, "websocket")])));
        assert!(asked(&request(&[(header::UPGRADE, " WebSocket ")])));
        assert!(!asked(&request(&[])));
        assert!(!asked(&request(&[(header::UPGRADE, "h2c")])));
        assert!(!asked(&request(&[(header::CONNECTION, "upgrade")])));
    }

    #[test]
    fn the_origin_is_this_servers_own_page_or_none() {
        let host = (header::HOST, "127.0.0.1:8765");
        assert!(own_origin(&request(std::slice::from_ref(&host))));
        assert!(own_origin(&request(&[
            host.clone(),
            (header::ORIGIN, "http://127.0.0.1:8765")
        ])));
        assert!(own_origin(&request(&[
            (header::HOST, "Machine.tailnet.ts.net:8765"),
            (header::ORIGIN, "http://machine.TAILNET.ts.net:8765")
        ])));
        for other in [
            "https://example.com",
            "http://127.0.0.1:8766",
            "https://127.0.0.1:8765",
            "http://127.0.0.1",
            "null",
            "",
        ] {
            assert!(
                !own_origin(&request(&[host.clone(), (header::ORIGIN, other)])),
                "{other}"
            );
        }
        // An Origin with no Host to match is another site's.
        assert!(!own_origin(&request(&[(header::ORIGIN, "http://")])));
        // So is one that is not text: no page of this server names itself so.
        let mut opaque = request(std::slice::from_ref(&host));
        opaque.headers_mut().insert(
            header::ORIGIN,
            HeaderValue::from_bytes(b"http://127.0.0.1:8765\xff").unwrap(),
        );
        assert!(!own_origin(&opaque));
    }

    /// A signed visitor's socket from the page of the origin their word was made for is theirs,
    /// and only that page is, under the service's name; under a name of this machine's the page
    /// of either is.
    #[test]
    fn a_signed_visitors_socket_is_from_the_origin_their_word_was_made_for() {
        let signed = |headers: &[(header::HeaderName, &str)]| {
            let mut request = request(headers);
            request
                .extensions_mut()
                .insert(crate::front::Admitted::Named {
                    actor: symphony_board::workflow::local_actor(),
                    origin: Some("https://board.example.com".into()),
                });
            own_origin(&request)
        };
        let service = (header::HOST, "board.example.com");
        assert!(signed(&[
            service.clone(),
            (header::ORIGIN, "https://board.example.com")
        ]));
        for other in [
            "http://board.example.com",
            "https://example.com",
            "https://board.example.com:8443",
        ] {
            assert!(
                !signed(&[service.clone(), (header::ORIGIN, other)]),
                "{other}"
            );
        }
        let loopback = (header::HOST, "127.0.0.1:8765");
        assert!(signed(&[
            loopback.clone(),
            (header::ORIGIN, "https://board.example.com")
        ]));
        assert!(signed(&[
            loopback.clone(),
            (header::ORIGIN, "http://127.0.0.1:8765")
        ]));
        assert!(!signed(&[
            loopback,
            (header::ORIGIN, "https://example.com")
        ]));
        // Without a word, the service's page is another site's.
        assert!(!own_origin(&request(&[
            service,
            (header::ORIGIN, "https://board.example.com")
        ])));
    }

    #[tokio::test]
    async fn only_another_sites_upgrade_is_refused() {
        let host = (header::HOST, "127.0.0.1:8765");
        let upgrade = (header::UPGRADE, "websocket");
        let foreign = (header::ORIGIN, "https://example.com");
        let refusal = refused(&request(&[host.clone(), foreign.clone(), upgrade.clone()]))
            .expect("another site's upgrade is refused");
        assert_eq!(refusal.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(refusal.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()["error"],
            "Forbidden: a WebSocket's Origin must be this server's own page"
        );
        // Its own page's upgrade, and any request for Server-Sent Events, whatever its Origin (a
        // browser lets no other page read those), are the feed's to answer.
        let own = (header::ORIGIN, "http://127.0.0.1:8765");
        assert!(refused(&request(&[host.clone(), own, upgrade.clone()])).is_none());
        assert!(refused(&request(&[host.clone(), upgrade])).is_none());
        assert!(refused(&request(&[host, foreign])).is_none());
    }

    #[tokio::test]
    async fn a_request_that_cannot_upgrade_gets_the_extractors_answer() {
        // A handshake's headers on a request no connection can hand over: no upgrade to take.
        let answered = answer(
            request(&[
                (header::HOST, "127.0.0.1:8765"),
                (header::UPGRADE, "websocket"),
                (header::CONNECTION, "upgrade"),
                (header::SEC_WEBSOCKET_VERSION, "13"),
                (header::SEC_WEBSOCKET_KEY, "dGhlIHNhbXBsZSBub25jZQ=="),
            ]),
            futures_util::stream::empty(),
        )
        .await;
        assert_eq!(answered.status(), StatusCode::UPGRADE_REQUIRED);
        // Not a handshake at all: no version.
        let answered = answer(
            request(&[
                (header::UPGRADE, "websocket"),
                (header::CONNECTION, "upgrade"),
            ]),
            futures_util::stream::empty(),
        )
        .await;
        assert!(answered.status().is_client_error(), "{}", answered.status());
    }
}
