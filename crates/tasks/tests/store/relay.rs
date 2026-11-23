//! A loopback relay in front of a board's server, for the tests of a slow board, which a client
//! with a short wait reaches through it. It holds back the answer of each connection it is told to
//! hold until it ends, so that client meets its wait on it however loaded the machine is; the request
//! itself reaches the server at once, and so does each later one on that connection, its answer held
//! the same way, so a request sent again on the held stream is counted and runs like the first.
//! No other request that client sends may race its short wait
//! for the server's answer: once it holds, the relay answers a read on a connection it does not hold
//! as the server answered that same read before the hold (the rehearsal, which nobody's short wait
//! bounds), at once and without the server. A test that holds rehearses the reads its client goes on
//! to send, and writes nothing in between, so the rehearsal's answers are the board's.
//! The relay never keeps a clock: what it says of the requests waits for each connection made to it
//! before to be taken, then to say something or end, for each held one to be closed by its caller,
//! as a client closes one whose answer did not come, and for the server's answer to each request it
//! was sent ([`Relay::requests`]), not for a time.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, mpsc};
use std::time::Duration;
use symphony_board::cbor;
use symphony_board::lock::Recovered as _;
use symphony_board::rpc::{Client, Kind};

/// The wait of a client over a relay that holds an answer, which that answer outlasts.
pub(super) const WAIT: Duration = Duration::from_millis(300);

pub(super) struct Relay {
    pub(super) addr: String,
    shared: Arc<Shared>,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    before: Mutex<Option<Before>>,
    /// Told when a connection first says something or ends, when the server answers a request,
    /// when the caller of a held connection closes it, and when the relay ends.
    changed: Condvar,
}

type Before = Box<dyn FnMut(&str) + Send>;

#[derive(Default)]
struct State {
    /// The connections taken so far.
    connections: usize,
    /// Those of them that have sent a request, or ended without one.
    heard: usize,
    /// The probes made and not taken yet ([`Relay::settled`]), by the address they come from.
    probes: Vec<SocketAddr>,
    /// The first connection after the hold, and how many from it on are held.
    hold: Option<(usize, usize)>,
    /// The held connections that have sent a request and that their caller has not closed yet.
    holding: usize,
    /// Each whole request, with the connection it came on, in the order they came.
    sent: Vec<(usize, Vec<u8>)>,
    /// The requests sent to the server whose answer has not come yet.
    pending: usize,
    /// The rehearsal: each read the server answered before the hold, and its answer.
    rehearsed: HashMap<Vec<u8>, Vec<u8>>,
    /// The requests after the hold, on a connection not held, that the rehearsal did not answer.
    unrehearsed: Vec<String>,
    /// The relay has ended: what it held goes on.
    ended: bool,
}

impl Relay {
    /// A relay to the server at `server`, holding the answers of the first `held` connections it
    /// takes (none: it holds nothing yet).
    pub(super) fn to(server: &str, held: usize) -> Relay {
        Self::with_before(server, held, None)
    }

    /// As `to`, with an action immediately before forwarding each request, for a second writer to
    /// commit between this client's transactions without racing a clock.
    pub(super) fn with_before(server: &str, held: usize, before: Option<Before>) -> Relay {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let shared = Arc::new(Shared {
            before: Mutex::new(before),
            ..Shared::default()
        });
        let (server, taken) = (server.to_owned(), Arc::clone(&shared));
        std::thread::spawn(move || {
            for caller in listener.incoming().map_while(Result::ok) {
                let connection = {
                    let mut state = taken.lock();
                    let from = caller.peer_addr().ok();
                    if let Some(probe) = state.probes.iter().position(|probe| Some(*probe) == from)
                    {
                        state.probes.swap_remove(probe);
                        drop(state);
                        taken.changed.notify_all();
                        continue;
                    }
                    state.connections += 1;
                    state.connections - 1
                };
                let (server, shared) = (server.clone(), Arc::clone(&taken));
                std::thread::spawn(move || serve(caller, &server, &shared, connection));
            }
        });
        let relay = Relay { addr, shared };
        if held > 0 {
            relay.hold(held);
        }
        relay
    }

    /// From the next connection on, the answers of `held` connections are held until the relay
    /// ends, and each read on a later one is answered from the rehearsal: what the server answered
    /// through this relay before now, which must all be reads.
    pub(super) fn hold(&self, held: usize) {
        let mut state = self.settled();
        assert!(state.hold.is_none(), "a relay holds once");
        let writes: Vec<String> = state
            .sent
            .iter()
            .map(|(_, request)| statements(request))
            .filter(|sql| Kind::of(sql) != Kind::Read)
            .collect();
        assert!(
            writes.is_empty(),
            "a rehearsal writes nothing, so its answers stay the board's: {writes:?}"
        );
        state.hold = Some((state.connections, held));
    }

    /// A client of `ns` and `db` over this relay: waiting [`WAIT`] once the relay holds an answer,
    /// which then comes late; before that, as long as a client waits, since nothing is late.
    pub(super) fn client(&self, ns: &str, db: &str) -> Client {
        let client = Client::new(&self.addr, ns, db);
        if self.shared.lock().hold.is_some() {
            client.waiting(WAIT)
        } else {
            client
        }
    }

    /// Each request the relay was sent since the hold (all of them, with none), by the connection
    /// it came on, counted from the hold's first, then as they came on it: that connection, and
    /// the request's statements. Once every connection taken has said something or ended, every
    /// held one has been closed by its caller, so what it sent there again before giving up is
    /// among these, and the server has answered each request the relay sent it, so what a request
    /// did there is done.
    pub(super) fn requests(&self) -> Vec<(usize, String)> {
        let state = self.settled();
        assert!(
            state.unrehearsed.is_empty(),
            "a connection not held sent what the rehearsal did not answer, so its answer raced the \
             held client's wait: {:?}",
            state.unrehearsed
        );
        let first = state.hold.map_or(0, |(first, _)| first);
        let mut sent: Vec<(usize, String)> = state
            .sent
            .iter()
            .filter(|(connection, _)| *connection >= first)
            .map(|(connection, request)| (connection - first, statements(request)))
            .collect();
        sent.sort_by_key(|(connection, _)| *connection);
        sent
    }

    /// The state once every connection made to the relay before now is taken and settled
    /// ([`Shared::settled`]). The listener's queue gives connections in the order they were made,
    /// so a probe made now, which the relay takes as no connection, is taken after each of them.
    /// The probe is said before the relay can take it, and stays open until it is taken.
    fn settled(&self) -> MutexGuard<'_, State> {
        let (probe, from) = {
            let mut state = self.shared.lock();
            let probe = TcpStream::connect(&self.addr).unwrap();
            let from = probe.local_addr().unwrap();
            state.probes.push(from);
            (probe, from)
        };
        drop(self.shared.until(|state| !state.probes.contains(&from)));
        drop(probe);
        self.shared.settled()
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.shared.lock().ended = true;
        self.shared.changed.notify_all();
    }
}

impl Shared {
    /// The state, even after a test's assertion failed holding it, so the relay still ends.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().recover()
    }

    fn until(&self, done: impl Fn(&State) -> bool) -> MutexGuard<'_, State> {
        self.changed
            .wait_while(self.lock(), |state| !done(state))
            .recover()
    }

    /// The state once every connection taken has said something or ended, every held one has been
    /// closed by its caller, and the server has answered every request it was sent.
    fn settled(&self) -> MutexGuard<'_, State> {
        self.until(|state| {
            state.heard == state.connections && state.holding == 0 && state.pending == 0
        })
    }

    /// A connection taken ended without a request.
    fn silent(&self) {
        self.lock().heard += 1;
        self.changed.notify_all();
    }

    /// The caller of a held connection closed it.
    fn closed(&self) {
        self.lock().holding -= 1;
        self.changed.notify_all();
    }

    /// `request`, taken on `connection`: whether its answer is held, and the rehearsal's answer
    /// to it when it is not sent to the server.
    fn took(&self, connection: usize, request: &[u8]) -> (bool, Option<Vec<u8>>) {
        let mut state = self.lock();
        let opening = !state.sent.iter().any(|(on, _)| *on == connection);
        if opening {
            state.heard += 1;
        }
        state.sent.push((connection, request.to_vec()));
        let (held, rehearsed) = match state.hold {
            Some((first, held)) if connection >= first => {
                if connection - first < held {
                    if opening {
                        state.holding += 1;
                    }
                    (true, None)
                } else if let Some(answer) = state.rehearsed.get(request) {
                    (false, Some(answer.clone()))
                } else {
                    state.unrehearsed.push(statements(request));
                    (false, None)
                }
            }
            _ => (false, None),
        };
        if rehearsed.is_none() {
            state.pending += 1;
        }
        drop(state);
        self.changed.notify_all();
        (held, rehearsed)
    }

    /// The server's `answer` to `request` came, or none will; a read's answer before the hold is
    /// the rehearsal's.
    fn answered(&self, request: &[u8], answer: Option<&[u8]>) {
        let mut state = self.lock();
        state.pending -= 1;
        if let Some(answer) = answer
            && state.hold.is_none()
            && Kind::of(&statements(request)) == Kind::Read
        {
            state.rehearsed.insert(request.to_vec(), answer.to_vec());
        }
        drop(state);
        self.changed.notify_all();
    }
}

/// Each request `caller` sends on `connection` until it closes the connection, answered by the
/// server or by the rehearsal at once; on a held connection every request still goes to the server
/// as it comes, and the answers only once the relay has ended ([`release`]).
fn serve(caller: TcpStream, server: &str, shared: &Arc<Shared>, connection: usize) {
    let mut asked = BufReader::new(caller.try_clone().unwrap());
    let mut caller = caller;
    let (mut upstream, mut heard, mut held_answers) = (None, false, None);
    while let Some(request) = message(&mut asked) {
        heard = true;
        let (held, rehearsed) = shared.took(connection, &request);
        let answer = rehearsed.or_else(|| {
            if let Some(before) = shared.before.lock().unwrap().as_mut() {
                before(&statements(&request));
            }
            let answer = forward(&mut upstream, server, &request);
            shared.answered(&request, answer.as_deref());
            answer
        });
        if held {
            let to = held_answers.get_or_insert_with(|| release(&caller, shared));
            drop(to.send(answer));
            continue;
        }
        let Some(answer) = answer else {
            break;
        };
        if caller.write_all(&answer).is_err() {
            break;
        }
    }
    if !heard {
        shared.silent();
    }
    if held_answers.is_some() {
        shared.closed();
    }
}

/// Where the answers held on `caller`'s connection go: to it once the relay has ended, in the order
/// they came, up to the first the server did not give, which ends the connection.
fn release(caller: &TcpStream, shared: &Arc<Shared>) -> mpsc::Sender<Option<Vec<u8>>> {
    let (held, answers) = mpsc::channel::<Option<Vec<u8>>>();
    let (mut caller, shared) = (caller.try_clone().unwrap(), Arc::clone(shared));
    std::thread::spawn(move || {
        drop(shared.until(|state| state.ended));
        for answer in answers.iter().map_while(std::convert::identity) {
            if caller.write_all(&answer).is_err() {
                break;
            }
        }
        drop(caller.shutdown(Shutdown::Both));
    });
    held
}

/// `request` sent to the server on the connection `upstream` keeps, made if there is none: its
/// answer, or none when the server closed the connection first.
pub(super) fn forward(
    upstream: &mut Option<BufReader<TcpStream>>,
    server: &str,
    request: &[u8],
) -> Option<Vec<u8>> {
    if upstream.is_none() {
        *upstream = Some(BufReader::new(TcpStream::connect(server).ok()?));
    }
    let to = upstream.as_mut()?;
    let answer = to
        .get_mut()
        .write_all(request)
        .ok()
        .and_then(|()| message(to));
    if answer
        .as_deref()
        .is_none_or(|answer| head(answer).lines().any(|line| line == "connection: close"))
    {
        *upstream = None;
    }
    answer
}

/// The head of `message`, in lower case.
fn head(message: &[u8]) -> String {
    let end = message
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(message.len());
    String::from_utf8_lossy(&message[..end]).to_ascii_lowercase()
}

/// One whole HTTP message off `from`: its head, and its body by the length the head gives; none at
/// the end of the connection, or for a message that gives no length (the client's requests and the
/// server's answers all do), which ends the connection rather than a relay thread, whose end
/// would leave [`Relay::requests`] waiting.
pub(super) fn message(from: &mut impl BufRead) -> Option<Vec<u8>> {
    let mut raw = Vec::new();
    loop {
        let start = raw.len();
        if from.read_until(b'\n', &mut raw).ok()? == 0 {
            return None;
        }
        if raw[start..] == *b"\r\n" {
            break;
        }
    }
    let length: usize = head(&raw)
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))?
        .trim()
        .parse()
        .ok()?;
    let start = raw.len();
    raw.resize(start + length, 0);
    from.read_exact(&mut raw[start..]).ok()?;
    Some(raw)
}

/// The statements of `request`, one whole RPC request.
pub(super) fn statements(request: &[u8]) -> String {
    let body = request
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map_or(request, |end| &request[end + 4..]);
    let call = cbor::decode(body).unwrap();
    call["params"][0].as_str().unwrap().to_owned()
}
