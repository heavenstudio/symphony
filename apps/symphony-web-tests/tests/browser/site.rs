//! A stand-in for the Symphony server: the client's build at `/`, the board fixture at
//! `/api/board`, and a feed at `/api/feed` this test pushes events into. Served the way the real
//! server serves them: gzip (the build at the best level, the page and the API at the default),
//! the hashed build cached for good, the shell revalidated by its ETag, and each JSON read tagged
//! weak and answered `304` to a request naming its tag (apps/symphony-server/src/api/validated.rs).
//! As the server does, a build file is read and gzipped by the first request for it and kept for
//! the run, whichever site asks next: a start reads the shell alone, however many retained hashes
//! the build holds. Every request's path is logged, and each answer, so a test can say what the
//! page fetched, when, what it named and what it was sent, and the feed streams the browser holds
//! open are counted, a stream leaving the count as it closes. The board
//! names the client build served (`Board::build`), as the server reads it from the shell. A test
//! can also play a server in trouble: a board that moved, answers late or names another build, a
//! feed that ends, fails, ends as soon as it opens, or keeps its socket open and says nothing
//! (the stand-in pings every open stream every `ping_ms` its board names, as the server does,
//! until it is told to fall silent).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use flate2::Compression;
use flate2::write::GzEncoder;

#[cfg(test)]
mod tests;

/// One file as served: its bytes (gzipped when the type compresses), type and caching.
#[derive(Clone)]
struct Body {
    bytes: Arc<[u8]>,
    content_type: &'static str,
    gzipped: bool,
    cache: &'static str,
    etag: String,
    /// Header lines of its own, each ending in CRLF, sent on its `304` too: a document read's
    /// cursor ([`symphony_api::VERSION_HEADER`]).
    headers: String,
}

/// One answer the stand-in sent: to which request, naming which tag, with which status.
#[derive(Clone, Debug, PartialEq)]
pub struct Answered {
    /// The request's path and query, as asked.
    pub target: String,
    /// Its `If-None-Match`, when it named one.
    pub named: Option<String>,
    pub status: u16,
    /// A read ahead of a visit (shell/shell.ts, `ahead`): a read of the API marked as a prefetch,
    /// no page's own ([`Site::read_ahead`]).
    pub ahead: bool,
}

/// A file the site serves: the shell and the API replies, made at the start, or a build file,
/// read and gzipped by the first request for it.
enum File {
    Made(Body),
    Build {
        path: PathBuf,
        content_type: &'static str,
        cache: &'static str,
        body: OnceLock<Body>,
    },
}

impl File {
    /// The file as sent, a build file's read and gzipped now if no request has asked for it yet.
    fn body(&self) -> &Body {
        match self {
            File::Made(body) => body,
            File::Build {
                path,
                content_type,
                cache,
                body,
            } => body.get_or_init(|| {
                let bytes =
                    std::fs::read(path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
                encoded(bytes, content_type, cache, best_gzip)
            }),
        }
    }
}

/// A content by its FNV-1a (the stand-in's ETag) and length.
type Content = (u64, usize);

/// The build's gzips at the best level, by content, for the whole run.
#[derive(Default)]
struct Gzips {
    made: HashMap<Content, Arc<OnceLock<Arc<[u8]>>>>,
    /// How many times each content was gzipped, for the tests that a start gzips nothing and no
    /// content is gzipped twice.
    runs: HashMap<Content, usize>,
}

/// Every site of a run serves the same build, so a content's gzip is made once for all of them,
/// as the server makes it once for every project (apps/symphony-server/src/api/client/gzipped.rs).
static GZIPS: LazyLock<Mutex<Gzips>> = LazyLock::new(Mutex::default);

/// `bytes`, whose FNV-1a is `hash`, gzipped at the best level: made by the first call for that
/// content, which concurrent calls wait for and later calls share.
fn best_gzip(bytes: &[u8], hash: u64) -> Arc<[u8]> {
    let content = (hash, bytes.len());
    let gzip = GZIPS
        .lock()
        .unwrap()
        .made
        .entry(content)
        .or_default()
        .clone();
    gzip.get_or_init(|| {
        *GZIPS.lock().unwrap().runs.entry(content).or_default() += 1;
        gzip_at(bytes, Compression::best()).into()
    })
    .clone()
}

/// How many times the run gzipped `bytes` at the best level.
fn runs(bytes: &[u8]) -> usize {
    let content = (fnv1a(bytes), bytes.len());
    GZIPS
        .lock()
        .unwrap()
        .runs
        .get(&content)
        .copied()
        .unwrap_or(0)
}

/// One open feed stream: its number, its socket, whether it is a WebSocket, and the document
/// sections its request named (`sections`).
struct Stream {
    id: u64,
    out: TcpStream,
    socket: bool,
    sections: Vec<String>,
}

/// One thing the feed says, as the server says it in either of its forms
/// (apps/symphony-server/src/api/feed_socket.rs): a Server-Sent Event, or a WebSocket's text
/// message, its name, a line feed and its data.
#[derive(Clone, Debug)]
pub struct Said {
    name: &'static str,
    id: Option<u64>,
    data: String,
}

impl Said {
    /// The board's event `data` at version `id`.
    pub fn delta(id: u64, data: &str) -> Said {
        Said {
            name: symphony_api::FEED_EVENT,
            id: Some(id),
            data: data.to_owned(),
        }
    }

    /// The keep-alive ping.
    pub fn ping() -> Said {
        Said {
            name: symphony_api::PING_EVENT,
            id: None,
            data: String::new(),
        }
    }

    /// The server's `message` to every page.
    pub fn notice(message: &str) -> Said {
        Said {
            name: symphony_api::SERVER_EVENT,
            id: None,
            data: serde_json::json!(message).to_string(),
        }
    }

    /// As a Server-Sent Event.
    pub fn sse(&self) -> String {
        let id = self.id.map(|id| format!("id: {id}\n")).unwrap_or_default();
        if self.data.is_empty() {
            format!("event: {}\n{id}data:\n\n", self.name)
        } else {
            format!("event: {}\n{id}data: {}\n\n", self.name, self.data)
        }
    }

    /// As the server's WebSocket frame: one text frame, unmasked, its length in the shortest field
    /// that holds it (RFC 6455, 5.2).
    pub fn frame(&self) -> Vec<u8> {
        let text = format!("{}\n{}", self.name, self.data);
        let mut frame = vec![0x81];
        match text.len() {
            length @ 0..=125 => frame.push(length as u8),
            length @ 126..=65_535 => {
                frame.push(126);
                frame.extend_from_slice(&(length as u16).to_be_bytes());
            }
            length => {
                frame.push(127);
                frame.extend_from_slice(&(length as u64).to_be_bytes());
            }
        }
        frame.extend_from_slice(text.as_bytes());
        frame
    }
}

/// A WebSocket's close frame from the server, with no status: the answer to a page's close.
const CLOSE: [u8; 2] = [0x88, 0x00];

struct Shared {
    replies: HashMap<String, Body>,
    posts: Vec<(String, serde_json::Value)>,
    /// Every request's path but the reads ahead's, in arrival order.
    log: Vec<String>,
    /// When each path of [`Shared::log`] was first asked for: as its request was read, before its
    /// answer.
    first_asked: HashMap<String, Instant>,
    /// Every answer but the feed's, in the order sent.
    answers: Vec<Answered>,
    /// Every request's path and query but the reads ahead's, in arrival order.
    targets: Vec<String>,
    /// The reads ahead's paths and queries, in arrival order ([`Answered::ahead`]).
    ahead: Vec<String>,
    /// The open feed streams, by the number each was given, written to by `push`. A stream the
    /// page closes leaves at once ([`hold_feed`]).
    feeds: Vec<Stream>,
    /// How many feed streams were opened.
    opened_feeds: u64,
    /// The server's notice, which each stream opened says first ([`Site::server_notice`]); none
    /// until a test says one.
    notice: Option<Said>,
    /// The board as its route answers it now, but for its build.
    board: serde_json::Value,
    /// The client build the board names.
    build: String,
    /// How long the board's route waits before it answers.
    board_delay: Duration,
    /// How many of the next feed requests are answered 500.
    failing_feeds: usize,
    failing_routes: BTreeSet<String>,
    /// Paths or targets answered 404, as the server answers a read of what it does not have, or 400,
    /// as it refuses a write, each with its status and the server's error.
    missing_routes: BTreeMap<String, (u16, String)>,
    held_routes: BTreeSet<String>,
    /// Paths whose next request alone is held, and the requests so held, by number.
    hold_next: BTreeSet<String>,
    holding: BTreeSet<u64>,
    held_count: u64,
    /// Answers given to a path's requests in turn, the last again once the rest are given.
    turns: HashMap<String, std::collections::VecDeque<Body>>,
    /// Routes whose requests the server closes unanswered, as a dropped link does.
    dropped_routes: BTreeSet<String>,
    /// Routes whose answers send their head and half their body, then hold the rest.
    stalled_bodies: BTreeSet<String>,
    /// The routes answered as a server built before every JSON read named the feed's cursor
    /// answers them ([`Site::names_no_cursor`]).
    no_cursor: BTreeSet<String>,
    /// Each feed stream ends as soon as it opens.
    ending_feeds: bool,
    /// The feed's pings have stopped, its streams left open.
    silent: bool,
    /// Build files the server no longer holds, answered 404, as a server whose published client
    /// dropped their build.
    retired: BTreeSet<String>,
    /// Documents read as the server reads them, by their path ([`Site::set_document`]).
    documents: HashMap<String, serde_json::Value>,
    /// HTML documents by their route ([`Site::set_html_page`]).
    html_pages: HashMap<String, Body>,
}

/// One ordinary card for generic page tests, in the stage their view opens.
/// Cases exercising a specific lifecycle stage name that stage explicitly.
pub fn example_listing(status: &str) -> serde_json::Value {
    serde_json::json!({
        "tasks": [{"slug":"tools/example","title":"Example task","body":"","status":status,
            "priority":1,"parent":null,"created_by":"owner","created":0.0,"updated":0.0,
            "links":[],"depends":[],"roles":[],"instruction":false}],
        "statuses":[status],"counts":[[status,1]],"signoff":[],"next":null
    })
}

pub struct Site {
    pub addr: SocketAddr,
    files: Arc<HashMap<String, File>>,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    released: Arc<Condvar>,
    accept: Option<JoinHandle<()>>,
    pinger: Option<JoinHandle<()>>,
}

impl Site {
    /// Serve `dist` at `/` and `board` at the board route, naming the build served.
    pub fn start(dist: &Path, board: &str) -> Site {
        let mut files = HashMap::new();
        add_tree(&mut files, dist, "/");
        // First visits exercise loaded views, including their API data, rather than an error
        // page that never reaches its rows. This is isolated fixture data, never the board DB.
        for (path, value) in [
            ("/api/activity", serde_json::json!({"items": []})),
            ("/api/pending", serde_json::json!({"items": [], "docs": []})),
            ("/api/task-board", example_listing("review")),
            (
                "/api/agent-page",
                serde_json::json!({
                    "view": {"generated": 0.0, "probes": {"panes": "ok", "slots": "ok"}, "agents": []},
                    "tasks": [], "total": 0, "next": null
                }),
            ),
            (
                "/api/folder",
                serde_json::json!({
                    "path": "docs", "title": "Documents", "crumbs": [], "docs": 0,
                    "intro": null, "entries": []
                }),
            ),
            (
                "/api/queue",
                serde_json::json!({
                    "waiting": [], "spot_check": [], "agent": [], "branches": []
                }),
            ),
            (
                "/api/machine-work",
                serde_json::json!({"freshness": "unknown", "machines": [], "unassigned": []}),
            ),
            (
                symphony_api::REPORTS_PATH,
                serde_json::json!({
                    "grain": "day", "days": 14, "from": 1_799_884_800.0,
                    "to": 1_800_003_600.0, "buckets": [], "first": null
                }),
            ),
            (
                symphony_api::RUNS_PATH,
                serde_json::json!({"runs": [], "more": false, "pipelines": [], "unread": []}),
            ),
            (
                symphony_api::SETTINGS_PATH,
                serde_json::json!({"files": [], "sections": []}),
            ),
        ] {
            files.insert(
                path.into(),
                File::Made(body(
                    value.to_string().into_bytes(),
                    "application/json",
                    "no-store",
                )),
            );
        }
        let shell = std::fs::read_to_string(dist.join("index.html")).expect("the build's shell");
        // The server gzips the page per request, at the default level.
        let page = body(
            crate::common::served_shell(&shell).into_bytes(),
            content_type("index.html"),
            "no-cache",
        );
        files.insert("/".into(), File::Made(page.clone()));
        files.insert("/index.html".into(), File::Made(page));
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
        let addr = listener.local_addr().unwrap();
        let files = Arc::new(files);
        let shared = Arc::new(Mutex::new(Shared {
            replies: HashMap::new(),
            posts: Vec::new(),
            log: Vec::new(),
            first_asked: HashMap::new(),
            answers: Vec::new(),
            targets: Vec::new(),
            ahead: Vec::new(),
            feeds: Vec::new(),
            opened_feeds: 0,
            notice: None,
            board: serde_json::from_str(board).expect("a board"),
            build: build_of(&shell),
            board_delay: Duration::ZERO,
            failing_feeds: 0,
            failing_routes: BTreeSet::new(),
            missing_routes: BTreeMap::new(),
            held_routes: BTreeSet::new(),
            hold_next: BTreeSet::new(),
            holding: BTreeSet::new(),
            held_count: 0,
            turns: HashMap::new(),
            dropped_routes: BTreeSet::new(),
            stalled_bodies: BTreeSet::new(),
            no_cursor: BTreeSet::new(),
            ending_feeds: false,
            silent: false,
            retired: BTreeSet::new(),
            documents: HashMap::new(),
            html_pages: HashMap::new(),
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let released = Arc::new(Condvar::new());
        let accept = {
            let released = released.clone();
            let (files, shared, stop) = (files.clone(), shared.clone(), stop.clone());
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let (files, shared, released) =
                        (files.clone(), shared.clone(), released.clone());
                    std::thread::spawn(move || serve(stream, &files, &shared, &released));
                }
            })
        };
        let pinger = {
            let (shared, stop) = (shared.clone(), stop.clone());
            std::thread::spawn(move || ping_every_ping_ms(&shared, &stop))
        };
        Site {
            addr,
            files,
            shared,
            stop,
            released,
            accept: Some(accept),
            pinger: Some(pinger),
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    /// The paths the pages requested so far, in order: their own, not their reads ahead.
    pub fn requests(&self) -> Vec<String> {
        self.shared.lock().unwrap().log.clone()
    }

    /// When the pages first asked for `path`, their own request and not a read ahead, as the site
    /// read it and before it answered: none while they have not.
    pub fn first_asked(&self, path: &str) -> Option<Instant> {
        self.shared.lock().unwrap().first_asked.get(path).copied()
    }

    /// [`Site::requests`] with their queries.
    pub fn targets(&self) -> Vec<String> {
        self.shared.lock().unwrap().targets.clone()
    }

    /// The answers sent so far to the pages' own requests for `path` (any query, any checkout's),
    /// in order.
    pub fn answers(&self, path: &str) -> Vec<Answered> {
        self.shared
            .lock()
            .unwrap()
            .answers
            .iter()
            .filter(|answer| !answer.ahead && route_of(&answer.target) == path)
            .cloned()
            .collect()
    }

    /// The reads ahead of a visit asked so far, their paths and queries in order, answered or not.
    pub fn asked_ahead(&self) -> Vec<String> {
        self.shared.lock().unwrap().ahead.clone()
    }

    /// The answers sent so far to reads ahead of a visit, in order.
    pub fn read_ahead(&self) -> Vec<Answered> {
        self.shared
            .lock()
            .unwrap()
            .answers
            .iter()
            .filter(|answer| answer.ahead)
            .cloned()
            .collect()
    }

    pub fn posts(&self) -> Vec<(String, serde_json::Value)> {
        self.shared.lock().unwrap().posts.clone()
    }

    pub fn set_json(&self, path: &str, value: &serde_json::Value) {
        self.set_file(path, value.to_string().into_bytes(), "application/json");
    }

    /// Serve `document` (as `/api/document` without a view sends it, its bodies and threads in it)
    /// at its path as the server does: its outline, the sections a read's `keys` name, each
    /// tagged without the feed's cursor and naming it in a header ([`document_read`]). Its
    /// `anchors`, when it has them, are its outline's ([`outline_of`]). A reply
    /// [`Site::set_json`] gave a read's route or target wins over it. Each section carries the
    /// kinds of its first and last blocks as the server's do ([`with_ends`]).
    pub fn set_document(&self, document: &serde_json::Value) {
        let path = document["path"].as_str().expect("a document's path");
        self.shared
            .lock()
            .unwrap()
            .documents
            .insert(path.to_string(), with_ends(document));
    }

    /// Serve `html` as the HTML document at the route `path`, as the server does: the route is the
    /// app's page, whatever headers ask for it, and the file's address (the route with the file's
    /// query, `symphony_api::file_address`), which its frame loads, the file. The stand-in
    /// sends no policy of its own with the file, so what a page sees of it is its frame's sandbox
    /// alone (the server's policy on it is its own test, apps/symphony-server/src/media_files.rs).
    pub fn set_html_page(&self, path: &str, html: &str) {
        self.shared.lock().unwrap().html_pages.insert(
            path.into(),
            body(html.as_bytes().to_vec(), content_type(path), "no-cache"),
        );
    }

    pub fn set_file(&self, path: &str, bytes: Vec<u8>, mime: &'static str) {
        self.shared
            .lock()
            .unwrap()
            .replies
            .insert(path.into(), body(bytes, mime, "no-store"));
    }

    /// How many feed streams are open now: opened, and neither closed by the page nor ended here.
    pub fn feeds(&self) -> usize {
        self.shared.lock().unwrap().feeds.len()
    }

    /// How many feed streams were opened so far, closed since or not: while no feed fails or ends
    /// on opening, each one [`Site::feed_requests`] counts once it is open, and so in reach of
    /// [`Site::drop_feeds`].
    pub fn opened_feeds(&self) -> usize {
        self.shared.lock().unwrap().opened_feeds as usize
    }

    /// How many times the feed was asked for, answered or not.
    pub fn feed_requests(&self) -> usize {
        let shared = self.shared.lock().unwrap();
        shared
            .log
            .iter()
            .filter(|path| *path == symphony_api::FEED_PATH)
            .count()
    }

    /// How many times the board was asked for.
    pub fn board_requests(&self) -> usize {
        let shared = self.shared.lock().unwrap();
        shared
            .log
            .iter()
            .filter(|path| *path == symphony_api::BOARD_PATH)
            .count()
    }

    /// Answer the board as `board` from now on, naming the build served.
    pub fn set_board(&self, board: &str) {
        self.shared.lock().unwrap().board = serde_json::from_str(board).expect("a board");
    }

    /// Name `build` in the board from now on, as a server that was deployed again does (this one
    /// still serves the same files).
    pub fn deploy(&self, build: &str) {
        self.shared.lock().unwrap().build = build.to_owned();
    }

    /// Answer the build file at `path` 404 from now on, as a server whose published client no
    /// longer keeps the build it belongs to.
    pub fn retire(&self, path: &str) {
        self.shared.lock().unwrap().retired.insert(path.to_owned());
    }

    /// Answer the build file at `path` again, after [`Site::retire`]: a load that failed and works
    /// later.
    pub fn restore(&self, path: &str) {
        self.shared.lock().unwrap().retired.remove(path);
    }

    /// End each feed stream as soon as it opens, from now on.
    pub fn end_feeds_on_open(&self) {
        self.shared.lock().unwrap().ending_feeds = true;
    }

    /// Stop pinging the feed, its streams left open, as a server whose link goes quiet does (a
    /// phone's network change, for one).
    pub fn fall_silent(&self) {
        self.shared.lock().unwrap().silent = true;
    }

    /// Answer the board `delay` after it is asked for, from now on.
    pub fn delay_board(&self, delay: Duration) {
        self.shared.lock().unwrap().board_delay = delay;
    }

    /// Answer the next `count` feed requests 500, as a server that is up but failing does.
    pub fn fail_feeds(&self, count: usize) {
        self.shared.lock().unwrap().failing_feeds = count;
    }

    /// Fail the actual snapshot route as well as, or instead of, its event stream. `path` is a
    /// route (every query of it) or one target, its query included.
    pub fn fail_route(&self, path: &str, fail: bool) {
        let mut shared = self.shared.lock().unwrap();
        if fail {
            shared.failing_routes.insert(path.to_owned());
        } else {
            shared.failing_routes.remove(path);
        }
    }

    /// Answer a route 404 with the server's JSON error, `{"error": ERROR}`, as the server answers
    /// a folder, document or record it does not have; none answers it again. `path` is a route
    /// (every query of it) or one target, its query included.
    pub fn miss_route(&self, path: &str, error: Option<&str>) {
        let mut shared = self.shared.lock().unwrap();
        match error {
            Some(error) => shared
                .missing_routes
                .insert(path.to_owned(), (404, error.to_owned())),
            None => shared.missing_routes.remove(path),
        };
    }

    /// Answer a route 400 with the server's JSON error, `{"error": ERROR}`, as the server refuses a
    /// write it read and would not take (an operation a workflow refuses, its code first); none
    /// answers it again. `path` is a route (every query of it) or one target, its query included.
    pub fn refuse_route(&self, path: &str, error: Option<&str>) {
        let mut shared = self.shared.lock().unwrap();
        match error {
            Some(error) => shared
                .missing_routes
                .insert(path.to_owned(), (400, error.to_owned())),
            None => shared.missing_routes.remove(path),
        };
    }

    /// Hold a real request until the test releases it, without a timing sleep. `path` is a route
    /// (every query of it) or one target, its query included.
    pub fn hold_route(&self, path: &str, hold: bool) {
        let mut shared = self.shared.lock().unwrap();
        if hold {
            shared.held_routes.insert(path.to_owned());
        } else {
            shared.held_routes.remove(path);
            self.released.notify_all();
        }
    }

    /// Hold the next request to `path` alone until [`Site::release_held`]; the ones after it are
    /// answered as they come.
    pub fn hold_next(&self, path: &str) {
        self.shared
            .lock()
            .unwrap()
            .hold_next
            .insert(path.to_owned());
    }

    /// Let every request [`Site::hold_next`] held go.
    pub fn release_held(&self) {
        self.shared.lock().unwrap().holding.clear();
        self.released.notify_all();
    }

    /// Answer the requests to `path` in turn from now on: the first with `answers[0]`, the next with
    /// `answers[1]`, the last again once the rest are given. A request's answer is its turn's, taken
    /// when it arrives, however long it is held.
    pub fn answer_in_turn(&self, path: &str, answers: &[serde_json::Value]) {
        let bodies = answers
            .iter()
            .map(|answer| {
                body(
                    answer.to_string().into_bytes(),
                    "application/json",
                    "no-store",
                )
            })
            .collect();
        self.shared
            .lock()
            .unwrap()
            .turns
            .insert(path.to_owned(), bodies);
    }

    /// Answer a route's requests with their head and half their body and hold the rest until
    /// `stall` is false again, as a link that stops mid-answer does.
    pub fn stall_body(&self, path: &str, stall: bool) {
        let mut shared = self.shared.lock().unwrap();
        if stall {
            shared.stalled_bodies.insert(path.to_owned());
        } else {
            shared.stalled_bodies.remove(path);
            self.released.notify_all();
        }
    }

    /// Answer a route's requests as an older server does: the feed's cursor in no header of its
    /// answers, a `304` among them, where the stand-in otherwise names it on every JSON read of
    /// the API as the server does.
    pub fn names_no_cursor(&self, path: &str) {
        self.shared
            .lock()
            .unwrap()
            .no_cursor
            .insert(path.to_owned());
    }

    /// Close a route's requests without an answer, so the page's read of it fails.
    pub fn drop_route(&self, path: &str, drop: bool) {
        let mut shared = self.shared.lock().unwrap();
        if drop {
            shared.dropped_routes.insert(path.to_owned());
        } else {
            shared.dropped_routes.remove(path);
        }
    }

    /// The server's `message` to every page, an empty one clearing it: said on every open feed
    /// stream, and first on each stream opened after, as the server says its notice
    /// (apps/symphony-server/src/api.rs, `feed`). A page that opens its feed again, as a document
    /// does once its sections near the screen, hears it on the new stream.
    pub fn server_notice(&self, message: &str) {
        let said = Said::notice(message);
        let mut shared = self.shared.lock().unwrap();
        say(&mut shared, &said);
        shared.notice = Some(said);
    }

    /// End every open feed stream, as a server that stops does.
    pub fn drop_feeds(&self) {
        for stream in self.shared.lock().unwrap().feeds.drain(..) {
            let _ = stream.out.shutdown(Shutdown::Both);
        }
    }

    /// Send one feed event to every open stream; the streams that are gone are dropped.
    pub fn push(&self, id: u64, data: &str) {
        self.send(&Said::delta(id, data));
    }

    /// How many of the open feed streams are on a WebSocket.
    pub fn socket_feeds(&self) -> usize {
        let shared = self.shared.lock().unwrap();
        shared.feeds.iter().filter(|stream| stream.socket).count()
    }

    /// Whether an open feed stream follows the document section `key`, its request naming it
    /// among its `sections`. A document page opens its feed before its sections come near the
    /// screen and opens it again naming them: the server sends the stream opened again what the
    /// page missed since its cursor (`since`), and the stand-in sends nothing, so an event pushed
    /// between the two streams never reaches the page. A test pushes a document's events once the
    /// stream naming its section is open.
    pub fn following(&self, key: &str) -> bool {
        let shared = self.shared.lock().unwrap();
        shared
            .feeds
            .iter()
            .any(|stream| stream.sections.iter().any(|section| section == key))
    }

    fn send(&self, said: &Said) {
        send(&self.shared, said);
    }

    /// The size every file of the build is sent at (gzipped when its type compresses), by path.
    /// It gzips each file no request has asked for yet, the retained hashes among them, on a
    /// thread per core.
    pub fn build_sizes(&self) -> Vec<(String, usize)> {
        let files: Vec<(&String, &File)> = self
            .files
            .iter()
            .filter(|(path, _)| !path.ends_with('/') && !path.starts_with("/api/"))
            .collect();
        let next = AtomicUsize::new(0);
        let threads = std::thread::available_parallelism().map_or(1, usize::from);
        let mut sizes: Vec<(String, usize)> = std::thread::scope(|scope| {
            let sizing: Vec<_> = (0..threads)
                .map(|_| {
                    scope.spawn(|| {
                        let mut sized = Vec::new();
                        while let Some((path, file)) =
                            files.get(next.fetch_add(1, Ordering::Relaxed))
                        {
                            sized.push(((*path).clone(), file.body().bytes.len()));
                        }
                        sized
                    })
                })
                .collect();
            sizing
                .into_iter()
                .flat_map(|thread| thread.join().unwrap())
                .collect()
        });
        sizes.sort();
        sizes
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.shared.lock().unwrap().held_routes.clear();
        self.shared.lock().unwrap().holding.clear();
        self.shared.lock().unwrap().stalled_bodies.clear();
        self.released.notify_all();
        let _ = TcpStream::connect(self.addr); // wakes the accept loop so it sees the stop
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
        if let Some(pinger) = self.pinger.take() {
            let _ = pinger.join();
        }
        self.drop_feeds();
    }
}

/// Say `said` on every open feed stream, in its form; the streams that are gone are dropped.
fn send(shared: &Mutex<Shared>, said: &Said) {
    say(&mut shared.lock().unwrap(), said);
}

/// [`send`], under the lock already held.
fn say(shared: &mut Shared, said: &Said) {
    let (sse, frame) = (said.sse(), said.frame());
    shared
        .feeds
        .retain_mut(|stream| tell(&mut stream.out, stream.socket, &sse, &frame));
}

/// Write one thing said to a stream, as its Server-Sent Event (`sse`) or, on a WebSocket, its
/// frame: whether the stream took it.
fn tell(out: &mut TcpStream, socket: bool, sse: &str, frame: &[u8]) -> bool {
    let bytes = if socket { frame } else { sse.as_bytes() };
    out.write_all(bytes).and_then(|_| out.flush()).is_ok()
}

/// Keep feed stream `id` open until the page closes it, then count it no more: a page sends
/// nothing on its stream after the request, so the first read that ends is the close, and nothing
/// on its WebSocket but its close frame, which is answered with the server's.
fn hold_feed(mut reader: impl Read, shared: &Mutex<Shared>, id: u64) {
    let mut sent = [0; 64];
    let closed = loop {
        match reader.read(&mut sent) {
            Ok(0) | Err(_) => break false,
            Ok(_) => {
                let socket = shared
                    .lock()
                    .unwrap()
                    .feeds
                    .iter()
                    .any(|open| open.id == id && open.socket);
                if socket {
                    break true;
                }
            }
        }
    };
    let mut shared = shared.lock().unwrap();
    if let Some(at) = shared.feeds.iter().position(|open| open.id == id) {
        let mut stream = shared.feeds.remove(at);
        if closed {
            let _ = stream.out.write_all(&CLOSE);
            let _ = stream.out.shutdown(Shutdown::Both);
        }
    }
}

/// The keep-alive ping, as the stand-in and the server (apps/symphony-server/src/api.rs) say it.
pub fn ping() -> Said {
    Said::ping()
}

/// Whether a request's `headers` ask for a WebSocket.
fn asks_socket(headers: &HashMap<String, String>) -> bool {
    headers
        .get("upgrade")
        .is_some_and(|upgrade| upgrade.eq_ignore_ascii_case("websocket"))
}

/// The head that answers a WebSocket handshake whose key is `key`.
pub fn socket_head(key: &str) -> String {
    format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
        tungstenite::handshake::derive_accept_key(key.as_bytes())
    )
}

/// Ping every open stream each `ping_ms` the board now names, until the site stops or falls
/// silent.
fn ping_every_ping_ms(shared: &Mutex<Shared>, stop: &AtomicBool) {
    let ping = ping();
    let mut last = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        #[expect(
            clippy::disallowed_methods,
            reason = "no-timed-waits baseline: the stand-in server looks each 50 ms whether a feed's ping is due, where a timer per feed is the helper"
        )]
        std::thread::sleep(Duration::from_millis(50));
        let every = {
            let shared = shared.lock().unwrap();
            (!shared.silent).then(|| shared.board["ping_ms"].as_u64().unwrap_or(1_000))
        };
        if every.is_some_and(|every| last.elapsed() >= Duration::from_millis(every)) {
            send(shared, &ping);
            last = Instant::now();
        }
    }
}

/// The client build a shell serves: the path of its module script, as the server reads it.
fn build_of(shell: &str) -> String {
    let script = shell
        .split("<script")
        .find(|tag| tag.contains("type=\"module\""))
        .expect("the shell's module script");
    let src = script.split("src=\"").nth(1).expect("its src");
    src[..src.find('"').unwrap()].to_owned()
}

/// Every build file under `root` but the shell, at `prefix` and its path there, read when first
/// asked for.
fn add_tree(files: &mut HashMap<String, File>, root: &Path, prefix: &str) {
    for path in walk(root) {
        let relative = path
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if relative == "index.html" {
            continue;
        }
        // A hashed build file never changes under its name; anything else is revalidated.
        let cache = if relative.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        let file = File::Build {
            content_type: content_type(&relative),
            cache,
            path,
            body: OnceLock::new(),
        };
        files.insert(format!("{prefix}{relative}"), file);
    }
}

fn walk(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in
            std::fs::read_dir(&dir).unwrap_or_else(|err| panic!("{}: {err}", dir.display()))
        {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}

fn compresses(content_type: &str) -> bool {
    !content_type.starts_with("image/png") && content_type != "image/x-icon"
}

/// A reply the server compresses per request, at the encoder's default level: the page and the API.
fn body(bytes: Vec<u8>, content_type: &'static str, cache: &'static str) -> Body {
    encoded(bytes, content_type, cache, |bytes, _| {
        gzip_at(bytes, Compression::default()).into()
    })
}

/// `bytes` as sent, gzipped by `gzip` (given them and their FNV-1a) when the type compresses. A
/// JSON read the browser may not keep is tagged weak, as the server tags it: the tag names the
/// JSON, not its encoding.
fn encoded(
    bytes: Vec<u8>,
    content_type: &'static str,
    cache: &'static str,
    gzip: impl FnOnce(&[u8], u64) -> Arc<[u8]>,
) -> Body {
    let hash = fnv1a(&bytes);
    let gzipped = compresses(content_type);
    let weak = if content_type == "application/json" && cache == "no-store" {
        "W/"
    } else {
        ""
    };
    Body {
        bytes: if gzipped {
            gzip(&bytes, hash)
        } else {
            bytes.into()
        },
        content_type,
        gzipped,
        cache,
        etag: format!("{weak}\"{hash:016x}\""),
        headers: String::new(),
    }
}

/// A document read's answer, as the server sends it (apps/symphony-server/src/api.rs,
/// `validated::without_cursor`): tagged by its JSON with the feed's cursor `null`, so the same
/// content answers `304` at any cursor, and the cursor named in a header of its own, on the `304`
/// too.
fn document_body(value: &serde_json::Value) -> Body {
    let cursor = value
        .get("document")
        .unwrap_or(value)
        .get("version")
        .and_then(serde_json::Value::as_u64);
    let mut bare = value.clone();
    if bare.get("document").is_some() {
        bare["document"]["version"] = serde_json::Value::Null;
    } else if bare.get("version").is_some() {
        bare["version"] = serde_json::Value::Null;
    }
    let mut body = body(
        value.to_string().into_bytes(),
        "application/json",
        "no-store",
    );
    body.etag = format!("W/\"{:016x}\"", fnv1a(bare.to_string().as_bytes()));
    if let Some(cursor) = cursor {
        body.headers = format!("{}: {cursor}\r\n", symphony_api::VERSION_HEADER);
    }
    body
}

/// A query parameter of a request's target, percent-decoded.
pub fn query(target: &str, name: &str) -> Option<String> {
    let (_, query) = target.split_once('?')?;
    let value = query
        .split('&')
        .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))?;
    let mut bytes = Vec::with_capacity(value.len());
    let mut rest = value.as_bytes();
    while let Some((&byte, after)) = rest.split_first() {
        let hex = after
            .get(..2)
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (byte, hex) {
            (b'%', Some(decoded)) => {
                bytes.push(decoded);
                rest = &after[2..];
            }
            _ => {
                bytes.push(byte);
                rest = after;
            }
        }
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// The section keys a `/api/section` request asked for, as its `keys` query names them.
pub fn asked_keys(target: &str) -> Vec<String> {
    query_list(target, "keys")
}

/// The list of keys a request's query parameter `name` holds as a JSON array, as a page names
/// sections: none where it names none or no list.
fn query_list(target: &str, name: &str) -> Vec<String> {
    query(target, name)
        .and_then(|keys| serde_json::from_str(&keys).ok())
        .unwrap_or_default()
}

/// `document`'s outline (`/api/document?view=outline`): no bodies or threads, each card's thread
/// named by a hash of it, and the ids each section holds as the fixture names them (its
/// `anchors`, by section key; none where it names none).
pub fn outline_of(document: &serde_json::Value) -> serde_json::Value {
    let mut outline = with_ends(document);
    let anchors = outline
        .as_object_mut()
        .and_then(|whole| whole.remove("anchors"))
        .unwrap_or_else(|| serde_json::json!({}));
    for section in outline["sections"].as_array_mut().into_iter().flatten() {
        section["blocks"] = serde_json::json!([]);
    }
    let threads = outline["threads"].take();
    outline["threads"] = serde_json::json!({});
    let hashes: serde_json::Map<String, serde_json::Value> = threads
        .as_object()
        .into_iter()
        .flatten()
        .map(|(slug, thread)| {
            let hash = format!("{:016x}", fnv1a(thread.to_string().as_bytes()));
            (slug.clone(), hash.into())
        })
        .collect();
    serde_json::json!({"document": outline, "anchors": anchors, "thread_hashes": hashes})
}

/// `document` with each section's `ends` as the server gives it (`symphony_api::Section::ends_of`)
/// where the fixture names none: a fixture's `"ends": null` stands for an outline kept from before
/// the outline carried them.
fn with_ends(document: &serde_json::Value) -> serde_json::Value {
    let mut document = document.clone();
    for section in document["sections"].as_array_mut().into_iter().flatten() {
        if section.get("ends").is_some() {
            continue;
        }
        // A fixture's blocks the wire cannot read (a page's own test of them) draw no edges.
        let blocks = serde_json::from_value::<Vec<symphony_api::Block>>(section["blocks"].clone());
        if let Some(ends) = blocks
            .ok()
            .and_then(|blocks| symphony_api::Section::ends_of(&blocks))
        {
            section["ends"] = serde_json::json!(ends);
        }
    }
    document
}

/// What a page reads ahead of `document` (`/api/document?view=first`), as the server cuts it
/// (`DocumentOutline::first`): its leading sections whole within `symphony_api::AHEAD_BYTES`.
pub fn first_of(document: &serde_json::Value) -> serde_json::Value {
    let whole: symphony_api::Document =
        serde_json::from_value(document.clone()).expect("a whole document");
    let outline: symphony_api::DocumentOutline =
        serde_json::from_value(outline_of(document)).expect("its outline");
    let part = outline
        .first(&whole, |part| {
            serde_json::to_vec(part).is_ok_and(|json| json.len() <= symphony_api::AHEAD_BYTES)
        })
        .expect("a fixture's head within the cap");
    serde_json::to_value(part).unwrap()
}

/// The answer the server gives `target`, a read of `/api/document` or `/api/section` of
/// `document` (a whole document, as `/api/document` without a view sends it): its outline
/// ([`outline_of`]), its first part ([`first_of`]), the sections its `keys` name with their cards
/// and threads, or the document whole.
fn document_read(document: &serde_json::Value, route: &str, target: &str) -> Body {
    let value = if route == "/api/section" {
        let keys = asked_keys(target);
        let asked = |key: &str| keys.iter().any(|asked| asked == key);
        let sections: Vec<&serde_json::Value> = document["sections"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|section| asked(section["key"].as_str().unwrap_or_default()))
            .collect();
        let cards: Vec<&serde_json::Value> = document["cards"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|card| {
                asked(&format!(
                    "card:{}",
                    card["slug"].as_str().unwrap_or_default()
                ))
            })
            .collect();
        let threads: serde_json::Map<String, serde_json::Value> = cards
            .iter()
            .map(|card| {
                let slug = card["slug"].as_str().unwrap_or_default();
                (slug.to_string(), document["threads"][slug].clone())
            })
            .collect();
        let cards: Vec<serde_json::Value> = cards
            .into_iter()
            .map(|card| serde_json::json!({"key": card["slug"], "value": card}))
            .collect();
        serde_json::json!({
            "version": document["version"],
            "change": {"path": document["path"], "sections": sections, "cards": cards, "threads": threads}
        })
    } else {
        match query(target, "view").as_deref() {
            Some("outline") => outline_of(document),
            Some("first") => first_of(document),
            _ => document.clone(),
        }
    };
    document_body(&value)
}

fn gzip_at(bytes: &[u8], level: Compression) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), level);
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

/// Whether an `If-None-Match` value names `etag`, by weak comparison, as the server reads it: any
/// of its comma-separated tags with or without `W/`, or `*`.
fn names(asked: &str, etag: &str) -> bool {
    let opaque = |tag: &str| tag.trim().trim_start_matches("W/").to_string();
    asked
        .split(',')
        .any(|tag| tag.trim() == "*" || opaque(tag) == opaque(etag))
}

/// A request's path without its query and its checkout (`/@tree/api/board` is `/api/board`).
fn route_of(target: &str) -> String {
    let original = target.split('?').next().unwrap_or("/").to_string();
    match original.strip_prefix("/@").and_then(|s| s.split_once('/')) {
        Some((_, rest)) => format!("/{rest}"),
        None => original,
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Answer the requests of one connection until it closes, or hand it to the feed.
fn serve(
    stream: TcpStream,
    files: &HashMap<String, File>,
    shared: &Mutex<Shared>,
    released: &Condvar,
) {
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(stream) => stream,
        Err(_) => return,
    });
    let mut out = stream;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let target = line.split_whitespace().nth(1).unwrap_or("/").to_string();
        let mut headers = HashMap::new();
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header).unwrap_or(0) == 0 {
                return;
            }
            let header = header.trim_end();
            if header.is_empty() {
                break;
            }
            if let Some((name, value)) = header.split_once(':') {
                headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
            }
        }
        let path = route_of(&target);
        // An HTML document's file, asked for at its file's address; its route is the app's shell.
        let file_asked = target
            .split_once('?')
            .is_some_and(|(_, query)| symphony_api::asks_for_file(query));
        let named = headers.get("if-none-match").cloned();
        // A read of the API marked as a prefetch is a read ahead of a visit (shell/shell.ts);
        // the shell's warmed modules are prefetches too, but no reads of the API.
        let ahead = path.starts_with("/api/")
            && headers
                .get("purpose")
                .is_some_and(|purpose| purpose.starts_with("prefetch"));
        let answered = |status: u16| {
            shared.lock().unwrap().answers.push(Answered {
                target: target.clone(),
                named: named.clone(),
                status,
                ahead,
            });
        };
        let length = headers
            .get("content-length")
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        let mut bytes = vec![0; length];
        if reader.read_exact(&mut bytes).is_err() {
            return;
        }
        let (turn, ticket) = {
            let mut shared = shared.lock().unwrap();
            if ahead {
                shared.ahead.push(target.clone());
            } else {
                shared
                    .first_asked
                    .entry(path.clone())
                    .or_insert_with(Instant::now);
                shared.log.push(path.clone());
                shared.targets.push(target.clone());
            }
            if line.starts_with("POST ") {
                shared.posts.push((
                    path.clone(),
                    serde_json::from_slice(&bytes).expect("client JSON post"),
                ));
            }
            let turn = shared.turns.get_mut(&path).and_then(|turns| {
                if turns.len() > 1 {
                    turns.pop_front()
                } else {
                    turns.front().cloned()
                }
            });
            let ticket = shared.hold_next.remove(&path).then(|| {
                shared.held_count += 1;
                let ticket = shared.held_count;
                shared.holding.insert(ticket);
                ticket
            });
            (turn, ticket)
        };
        {
            let mut pending = shared.lock().unwrap();
            while pending.held_routes.contains(&path)
                || pending.held_routes.contains(&target)
                || ticket.is_some_and(|ticket| pending.holding.contains(&ticket))
            {
                pending = released.wait(pending).unwrap();
            }
            if pending.dropped_routes.contains(&path) {
                let _ = out.shutdown(Shutdown::Both);
                return;
            }
        }
        let missing = {
            let shared = shared.lock().unwrap();
            shared
                .missing_routes
                .get(&path)
                .or_else(|| shared.missing_routes.get(&target))
                .cloned()
        };
        if let Some((status, error)) = missing {
            answered(status);
            let reason = if status == 400 {
                "Bad Request"
            } else {
                "Not Found"
            };
            let error = serde_json::json!({ "error": error }).to_string();
            let _ = write!(
                out,
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{error}",
                error.len()
            );
            return;
        }
        let failing = {
            let shared = shared.lock().unwrap();
            shared.failing_routes.contains(&path) || shared.failing_routes.contains(&target)
        };
        if failing {
            answered(500);
            let error = br#"{"error":"Board snapshot unavailable"}"#;
            let _ = write!(
                out,
                "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                error.len()
            );
            let _ = out.write_all(error);
            return;
        }
        if path == symphony_api::FEED_PATH {
            // Every page follows the feed on a WebSocket. Any other request gets the events, as
            // the server sends them, so a page that took the event stream again shows in
            // `socket_feeds` (tabs.rs).
            let socket = asks_socket(&headers);
            let held = {
                let mut shared = shared.lock().unwrap();
                if shared.failing_feeds > 0 {
                    shared.failing_feeds -= 1;
                    let _ = out.write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    return;
                }
                let head = if socket {
                    socket_head(headers.get("sec-websocket-key").map_or("", String::as_str))
                } else {
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\n\r\n: open\n\n".to_owned()
                };
                if out.write_all(head.as_bytes()).is_err() {
                    return;
                }
                if shared.ending_feeds {
                    let _ = out.flush();
                    let _ = out.shutdown(Shutdown::Both);
                    return;
                }
                // The server's notice first, as the server says it on every stream it opens.
                if let Some(notice) = &shared.notice
                    && !tell(&mut out, socket, &notice.sse(), &notice.frame())
                {
                    return;
                }
                shared.opened_feeds += 1;
                let id = shared.opened_feeds;
                shared.feeds.push(Stream {
                    id,
                    out,
                    socket,
                    sections: query_list(&target, "sections"),
                });
                id
            };
            hold_feed(reader, shared, held);
            return;
        }
        // As the server (apps/symphony-server/src/api.rs, `answer`): every JSON read of the API
        // names the feed's cursor it was read at, which is read before its answer. A route a
        // test answers as an older server does names none (`Site::names_no_cursor`).
        let cursor = path
            .starts_with("/api/")
            .then(|| {
                let shared = shared.lock().unwrap();
                (!shared.no_cursor.contains(&path))
                    .then(|| shared.board["version"].as_u64())
                    .flatten()
            })
            .flatten();
        let own_headers = |body: &Body| match cursor {
            Some(cursor) if body.headers.is_empty() && body.content_type == "application/json" => {
                format!("{}: {cursor}\r\n", symphony_api::VERSION_HEADER)
            }
            _ => body.headers.clone(),
        };
        let board;
        let html_page;
        let overridden = turn.or_else(|| {
            let shared = shared.lock().unwrap();
            shared
                .replies
                .get(&target)
                .or_else(|| shared.replies.get(&path))
                .cloned()
                .or_else(|| {
                    let read = matches!(path.as_str(), "/api/document" | "/api/section")
                        .then(|| query(&target, "path"))
                        .flatten()?;
                    let document = shared.documents.get(&read)?;
                    Some(document_read(document, &path, &target))
                })
        });
        let found = if overridden.is_some() {
            overridden.as_ref()
        } else if path == symphony_api::BOARD_PATH {
            let delay = shared.lock().unwrap().board_delay;
            #[expect(
                clippy::disallowed_methods,
                reason = "no-timed-waits baseline: the stand-in's board answer made slow by a length of time, not held until its test releases it"
            )]
            std::thread::sleep(delay);
            let named = {
                let shared = shared.lock().unwrap();
                let mut named = shared.board.clone();
                named["build"] = shared.build.clone().into();
                named
            };
            board = body(
                named.to_string().into_bytes(),
                "application/json",
                "no-store",
            );
            Some(&board)
        } else if let Some(page) = file_asked
            .then(|| shared.lock().unwrap().html_pages.get(&path).cloned())
            .flatten()
        {
            html_page = page;
            Some(&html_page)
        } else if shared.lock().unwrap().retired.contains(&path) {
            None
        } else {
            files
                .get(&path)
                .or_else(|| {
                    // A route of the app, not a file: the shell, as the server's fallback serves it.
                    let last = path.rsplit('/').next().unwrap_or("");
                    (!path.starts_with("/api/")
                        && (!last.contains('.')
                            || path.starts_with("/docs/")
                                && (path.ends_with(".md")
                                    || !file_asked && symphony_api::is_html(&path))))
                    .then(|| files.get("/"))
                    .flatten()
                })
                .map(File::body)
        };
        let written = match found {
            None => {
                answered(404);
                out.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")
            }
            Some(body)
                if named
                    .as_deref()
                    .is_some_and(|named| names(named, &body.etag)) =>
            {
                answered(304);
                out.write_all(
                    format!(
                        "HTTP/1.1 304 Not Modified\r\nETag: {}\r\nCache-Control: {}\r\n{}\r\n",
                        body.etag,
                        body.cache,
                        own_headers(body)
                    )
                    .as_bytes(),
                )
            }
            Some(body) => {
                answered(200);
                let encoding = if body.gzipped {
                    "Content-Encoding: gzip\r\nVary: Accept-Encoding\r\n"
                } else {
                    ""
                };
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: {}\r\nETag: {}\r\n{}{encoding}\r\n",
                    body.content_type,
                    body.bytes.len(),
                    body.cache,
                    body.etag,
                    own_headers(body)
                );
                let half = if shared.lock().unwrap().stalled_bodies.contains(&path) {
                    body.bytes.len() / 2
                } else {
                    body.bytes.len()
                };
                let sent = out
                    .write_all(head.as_bytes())
                    .and_then(|_| out.write_all(&body.bytes[..half]))
                    .and_then(|_| out.flush());
                if half < body.bytes.len() && sent.is_ok() {
                    let mut pending = shared.lock().unwrap();
                    while pending.stalled_bodies.contains(&path) {
                        pending = released.wait(pending).unwrap();
                    }
                }
                sent.and_then(|_| out.write_all(&body.bytes[half..]))
            }
        };
        if written.and_then(|_| out.flush()).is_err() {
            return;
        }
    }
}
