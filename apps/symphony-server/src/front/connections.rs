//! Stop accepting, cancel unadmitted reads, and answer sockets Hyper drops without a response.
//! Protocol detection can be cancelled before HTTP/1 has a chance to send the drain's 503.

use super::Shared;
use axum::Router;
use axum::extract::connect_info::Connected;
use axum::extract::{ConnectInfo, Request};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::serve::{IncomingStream, Listener};
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

const UNAVAILABLE: &[u8] =
    b"HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";

#[derive(Clone, Debug)]
struct Connection {
    peer: SocketAddr,
    admitted: Arc<AtomicBool>,
    stopped: watch::Receiver<bool>,
}

impl Connected<IncomingStream<'_, Connections>> for Connection {
    fn connect_info(stream: IncomingStream<'_, Connections>) -> Self {
        stream.remote_addr().clone()
    }
}

/// Admission precedes the handler's first body read, so a save already reading its body keeps
/// draining normally. The inner router retains its ordinary SocketAddr connect-info contract.
async fn admit(
    ConnectInfo(connection): ConnectInfo<Connection>,
    mut request: Request,
    next: Next,
) -> Response {
    if *connection.stopped.borrow() {
        // A complete header may already be in Hyper's buffer when stop arrives. Do not start
        // reading its body merely because that last parse needs no further socket read.
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            [(header::CONNECTION, "close")],
        )
            .into_response();
    }
    connection.admitted.store(true, Ordering::SeqCst);
    request
        .extensions_mut()
        .insert(ConnectInfo(connection.peer));
    next.run(request).await
}

struct Connections {
    listener: Option<TcpListener>,
    shared: Arc<Shared>,
    accepting_stopped: watch::Sender<bool>,
    unanswered: watch::Sender<usize>,
    /// How long a rejected socket's read side is drained after its 503 ([`super::SHUTDOWN_GRACE`]
    /// outside tests).
    grace: std::time::Duration,
}

impl Connections {
    fn new(listener: TcpListener, shared: Arc<Shared>) -> Self {
        Self {
            listener: Some(listener),
            shared,
            accepting_stopped: watch::Sender::new(false),
            unanswered: watch::Sender::new(0),
            grace: super::SHUTDOWN_GRACE,
        }
    }

    async fn serve(self, app: Router) {
        let mut accepting_stopped = self.accepting_stopped.subscribe();
        let mut unanswered = self.unanswered.subscribe();
        let _ = axum::serve(
            self,
            app.layer(middleware::from_fn(admit))
                .into_make_service_with_connect_info::<Connection>(),
        )
        .with_graceful_shutdown(async move {
            // Close this generation's listener before signalling its accepted connections.
            let _ = accepting_stopped.wait_for(|stopped| *stopped).await;
        })
        .await;
        // Hyper's protocol detector drops cancelled IO without calling poll_shutdown. Those
        // sockets finish their small 503 write in an owned task, joined before this server exits.
        let _ = unanswered.wait_for(|count| *count == 0).await;
    }
}

impl Listener for Connections {
    type Io = FirstResponse;
    type Addr = Connection;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        let accepted = tokio::select! {
            biased;
            _ = self.shared.until_stopped() => {
                // Drop only this generation's descriptor: the replacement owns the shared queue.
                self.listener.take();
                self.accepting_stopped.send_replace(true);
                return std::future::pending().await;
            }
            accepted = Listener::accept(self.listener.as_mut().expect("listener is accepting")) => accepted,
        };
        self.unanswered.send_modify(|count| *count += 1);
        let admitted = Arc::new(AtomicBool::new(false));
        let stopped = self.shared.clone();
        (
            FirstResponse {
                stream: Some(accepted.0),
                unanswered: Some(Unanswered(self.unanswered.clone())),
                response_started: false,
                admitted: admitted.clone(),
                shared: self.shared.clone(),
                stopped: Box::pin(async move { stopped.until_stopped().await }),
                grace: self.grace,
            },
            Connection {
                peer: accepted.1,
                admitted,
                stopped: self.shared.stopped.subscribe(),
            },
        )
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        let peer = self
            .listener
            .as_ref()
            .ok_or_else(|| io::Error::other("Symphony listener has stopped accepting"))?
            .local_addr()?;
        Ok(Connection {
            peer,
            admitted: Arc::new(AtomicBool::new(false)),
            stopped: self.shared.stopped.subscribe(),
        })
    }
}

struct Unanswered(watch::Sender<usize>);

impl Drop for Unanswered {
    fn drop(&mut self) {
        self.0.send_modify(|count| *count -= 1);
    }
}

/// Admitted exchanges keep Hyper's normal lifecycle. Unadmitted shutdown responses linger through
/// peer EOF; protocol detection can also drop the socket before writing any response at all.
struct FirstResponse {
    stream: Option<TcpStream>,
    unanswered: Option<Unanswered>,
    response_started: bool,
    admitted: Arc<AtomicBool>,
    shared: Arc<Shared>,
    stopped: Pin<Box<dyn Future<Output = ()> + Send>>,
    /// Its [`Connections`]' read-side drain grace.
    grace: std::time::Duration,
}

impl FirstResponse {
    fn answered(&mut self) {
        self.response_started = true;
        // A late header can reach middleware entirely from Hyper's buffer. Its 503 still
        // needs the read-side drain, even though Hyper has already written that response.
        if self.admitted.load(Ordering::SeqCst) || !self.shared.stopping() {
            self.unanswered.take();
        }
    }
}

impl Drop for FirstResponse {
    fn drop(&mut self) {
        if self.shared.stopping()
            && let Some(unanswered) = self.unanswered.take()
            && let Some(mut stream) = self.stream.take()
        {
            let needs_response = !self.response_started;
            let grace = self.grace;
            crate::spawn::task(async move {
                if needs_response {
                    let _ = stream.write_all(UNAVAILABLE).await;
                }
                // FIN lets the peer finish receiving the 503 and close its write side. Keep
                // reading through that EOF: a pre-write try_read misses bytes still in flight,
                // and closing with unread input can reset the socket and destroy the response.
                let _ = stream.shutdown().await;
                let _ = tokio::time::timeout(grace, async {
                    let mut discard = [0; 4096];
                    loop {
                        match stream.read(&mut discard).await {
                            Ok(0) | Err(_) => break,
                            Ok(_) => {}
                        }
                    }
                })
                .await;
                drop(unanswered);
            });
        }
    }
}

impl AsyncRead for FirstResponse {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if !self.admitted.load(Ordering::SeqCst)
            && (self.shared.stopping() || self.stopped.as_mut().poll(cx).is_ready())
        {
            // A partial header, like a socket that sent nothing, cannot pin a draining process.
            // An IO cancellation avoids fabricating a malformed-request error; Drop sends 503.
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "Symphony server draining an unadmitted request",
            )));
        }
        Pin::new(self.stream.as_mut().expect("live connection stream")).poll_read(cx, buf)
    }
}

impl AsyncWrite for FirstResponse {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let written =
            Pin::new(self.stream.as_mut().expect("live connection stream")).poll_write(cx, buf);
        if matches!(written, Poll::Ready(Ok(count)) if count > 0) {
            self.answered();
        }
        written
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(self.stream.as_mut().expect("live connection stream")).poll_flush(cx)
    }

    fn is_write_vectored(&self) -> bool {
        self.stream
            .as_ref()
            .expect("live connection stream")
            .is_write_vectored()
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let written = Pin::new(self.stream.as_mut().expect("live connection stream"))
            .poll_write_vectored(cx, bufs);
        if matches!(written, Poll::Ready(Ok(count)) if count > 0) {
            self.answered();
        }
        written
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(self.stream.as_mut().expect("live connection stream")).poll_shutdown(cx)
    }
}

pub(super) async fn serve(listener: TcpListener, app: Router, shared: Arc<Shared>) {
    Connections::new(listener, shared).serve(app).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::http::StatusCode;
    use std::path::Path;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Each test waits on its events; only a stall reaches this wall-clock backstop.
    const BACKSTOP: Duration = test_time::backstop(Duration::from_secs(15));

    /// `work` to its end, or a failure that says it stalled and the load average then.
    async fn within<T>(work: impl std::future::Future<Output = T>) -> T {
        tokio::time::timeout(BACKSTOP, work)
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "stalled: the test's wall-clock backstop of {BACKSTOP:?} ran out; {}",
                    test_time::Load::now()
                )
            })
    }

    async fn connections() -> (Connections, SocketAddr, Arc<Shared>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        assert_ne!(address.port(), 8765);
        let shared = Shared::new(
            1,
            address.port(),
            Path::new("/uninitialized-connection-fixture"),
            None,
            crate::testing::open_gate(),
        );
        (Connections::new(listener, shared.clone()), address, shared)
    }

    #[tokio::test]
    async fn accepted_sockets_get_their_first_response_even_when_stop_precedes_the_request() {
        within(async {
            let cases = [
                "",
                "P",
                "GET /api/board HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n",
                "invalid",
            ].into_iter().map(|prefix| async move {
                let (connections, address, shared) = connections().await;
                let mut unanswered = connections.unanswered.subscribe();
                let mut accepting_stopped = connections.accepting_stopped.subscribe();
                let app = super::super::router(shared.clone());
                let serving = tokio::spawn(connections.serve(app));
                let mut client = TcpStream::connect(address).await.unwrap();
                client.write_all(prefix.as_bytes()).await.unwrap();
                unanswered.wait_for(|count| *count == 1).await.unwrap();
                shared.stop();
                accepting_stopped
                    .wait_for(|stopped| *stopped)
                    .await
                    .unwrap();
                // The client neither completes its header nor closes. Joining first proves
                // shutdown cannot depend on any further client action.
                serving.await.unwrap();
                let mut response = String::new();
                client.read_to_string(&mut response).await.unwrap();
                assert_eq!(response, "HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nContent-Length: 0\r\n\r\n", "prefix {prefix:?}");
                assert_eq!(*unanswered.borrow(), 0);
            });
            // Each idle peer deliberately leaves its write side open. Exercise the existing
            // shutdown bound concurrently rather than multiplying that bound by the case count.
            futures_util::future::join_all(cases).await;
        })
        .await;
    }

    #[tokio::test]
    async fn a_peer_closing_without_a_response_releases_the_drain() {
        within(async {
            let (connections, address, shared) = connections().await;
            let mut unanswered = connections.unanswered.subscribe();
            let mut accepting_stopped = connections.accepting_stopped.subscribe();
            let serving = tokio::spawn(connections.serve(super::super::router(shared.clone())));
            let client = TcpStream::connect(address).await.unwrap();
            unanswered.wait_for(|count| *count == 1).await.unwrap();
            shared.stop();
            accepting_stopped
                .wait_for(|stopped| *stopped)
                .await
                .unwrap();
            drop(client);
            serving.await.unwrap();
            assert_eq!(*unanswered.borrow(), 0);
        })
        .await;
    }

    #[tokio::test]
    async fn an_answered_keepalive_connection_does_not_hold_the_drain() {
        within(async {
            let (connections, address, shared) = connections().await;
            let mut unanswered = connections.unanswered.subscribe();
            let app = Router::new().fallback(|| async { StatusCode::NO_CONTENT });
            let serving = tokio::spawn(connections.serve(app));
            let mut client = TcpStream::connect(address).await.unwrap();
            unanswered.wait_for(|count| *count == 1).await.unwrap();
            client
                .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .await
                .unwrap();
            let mut response = Vec::new();
            while !response.ends_with(b"\r\n\r\n") {
                response.push(client.read_u8().await.unwrap());
            }
            assert!(response.starts_with(b"HTTP/1.1 204 "));
            unanswered.wait_for(|count| *count == 0).await.unwrap();
            shared.stop();
            serving.await.unwrap();
            assert_eq!(
                client.read_u8().await.unwrap_err().kind(),
                io::ErrorKind::UnexpectedEof
            );
        })
        .await;
    }

    #[tokio::test]
    async fn only_nonempty_writes_release_an_unanswered_socket() {
        within(async {
            let (mut connections, address, _) = connections().await;
            assert_eq!(connections.local_addr().unwrap().peer, address);
            let mut client = TcpStream::connect(address).await.unwrap();
            let (mut stream, peer) = connections.accept().await;
            assert_eq!(peer.peer, client.local_addr().unwrap());
            assert_eq!(stream.write(b"").await.unwrap(), 0);
            assert_eq!(
                *connections.unanswered.borrow(),
                1,
                "no response was written"
            );
            let response = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
            stream.write_all(response).await.unwrap();
            assert_eq!(*connections.unanswered.borrow(), 0);
            drop(stream);
            assert_eq!(
                *connections.unanswered.borrow(),
                0,
                "drop releases a socket only once"
            );
            let mut received = Vec::new();
            client.read_to_end(&mut received).await.unwrap();
            assert_eq!(received, response);
        })
        .await;
    }

    #[tokio::test]
    async fn an_admitted_body_finishes_after_stop() {
        within(async {
            let (connections, address, shared) = connections().await;
            let mut accepting_stopped = connections.accepting_stopped.subscribe();
            let app = Router::new().fallback(|request: Request| async {
                axum::body::to_bytes(request.into_body(), 16).await.unwrap()
            });
            let serving = tokio::spawn(connections.serve(app));
            let mut client = TcpStream::connect(address).await.unwrap();
            client.write_all(b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 7\r\nExpect: 100-continue\r\nConnection: close\r\n\r\n").await.unwrap();
            let mut continued = [0; 25];
            client.read_exact(&mut continued).await.unwrap();
            assert_eq!(&continued, b"HTTP/1.1 100 Continue\r\n\r\n");
            shared.stop();
            accepting_stopped.wait_for(|stopped| *stopped).await.unwrap();
            assert!(!serving.is_finished(), "an admitted body is still in flight");
            client.write_all(b"payload").await.unwrap();
            let mut response = String::new();
            client.read_to_string(&mut response).await.unwrap();
            assert!(response.starts_with("HTTP/1.1 200 "), "{response}");
            assert!(response.ends_with("\r\n\r\npayload"), "{response}");
            serving.await.unwrap();
        }).await;
    }

    #[tokio::test]
    async fn malformed_requests_keep_their_parser_response() {
        within(async {
            let (connections, address, shared) = connections().await;
            let serving = tokio::spawn(connections.serve(super::super::router(shared.clone())));
            let mut client = TcpStream::connect(address).await.unwrap();
            client.write_all(b"invalid request\r\n\r\n").await.unwrap();
            let mut response = String::new();
            client.read_to_string(&mut response).await.unwrap();
            assert!(response.starts_with("HTTP/1.1 400 "), "{response}");
            shared.stop();
            serving.await.unwrap();
        })
        .await;
    }

    #[tokio::test]
    async fn headers_buffered_before_stop_cannot_admit_a_late_body() {
        use tower::ServiceExt;

        async fn late_handler() -> StatusCode {
            panic!("a late body handler must not run");
        }

        let (_, address, shared) = connections().await;
        let admitted = Arc::new(AtomicBool::new(false));
        let connection = Connection {
            peer: address,
            admitted: admitted.clone(),
            stopped: shared.stopped.subscribe(),
        };
        let app = Router::new()
            .fallback(late_handler)
            .layer(middleware::from_fn(admit))
            .layer(axum::Extension(ConnectInfo(connection)));
        shared.stop();
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()[header::CONNECTION], "close");
        assert!(!admitted.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn a_cancelled_unread_request_gets_503_without_a_connection_reset() {
        within(async {
            for read_prefix in [false, true] {
            let (mut connections, address, shared) = connections().await;
            let mut unanswered = connections.unanswered.subscribe();
            let mut client = TcpStream::connect(address).await.unwrap();
            let (mut stream, _) = connections.accept().await;
            client
                .write_all(
                    b"GET /api/front HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            if read_prefix {
                let mut prefix = [0; 4];
                stream.read_exact(&mut prefix).await.unwrap();
                assert_eq!(&prefix, b"GET ");
            }
            // Cover both an unread socket and an unadmitted read that has already consumed
            // part of a header. Stop wins even while more input is immediately available.
            shared.stop();
            if read_prefix {
                assert_eq!(stream.read_u8().await.unwrap_err().kind(), io::ErrorKind::ConnectionAborted);
            }
            drop(stream);
            unanswered.wait_for(|count| *count == 0).await.unwrap();
            let mut response = String::new();
            client.read_to_string(&mut response).await.unwrap();
            assert_eq!(
                response,
                "HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
            );
            }
        })
        .await;
    }

    #[tokio::test]
    async fn rejected_sockets_drain_bytes_sent_after_the_503_before_closing() {
        within(async {
            for middleware_answered in [false, true] {
                let (mut connections, address, shared) = connections().await;
                // The drain must outlast the test's own steps however loaded the machine is: only
                // the client's EOF below ends it, never the grace.
                connections.grace = BACKSTOP * 2;
                let mut unanswered = connections.unanswered.subscribe();
                let mut client = TcpStream::connect(address).await.unwrap();
                let (mut stream, _) = connections.accept().await;
                let expected = b"HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";
                shared.stop();
                if middleware_answered {
                    stream.write_all(expected).await.unwrap();
                }
                drop(stream);
                let mut response = vec![0; expected.len()];
                client.read_exact(&mut response).await.unwrap();
                assert_eq!(response, expected);
                // Send input the pre-write read could never have seen, keeping our write
                // side open so the server cannot have reached peer EOF yet.
                client
                    .write_all(b"GET /api/front HTTP/1.1\r\nHost: localhost\r\n\r\n")
                    .await
                    .unwrap();
                let mut tail = Vec::new();
                client.read_to_end(&mut tail).await.unwrap();
                assert!(tail.is_empty());
                // Server FIN is the rendezvous: before client EOF, the drain must still
                // own the socket. Checking only after our EOF misses a removed drain.
                assert_eq!(
                    *unanswered.borrow(),
                    1,
                    "drain ended before client EOF; middleware_answered={middleware_answered}"
                );
                client.shutdown().await.unwrap();
                unanswered.wait_for(|count| *count == 0).await.unwrap();
            }
        })
        .await;
    }
}
