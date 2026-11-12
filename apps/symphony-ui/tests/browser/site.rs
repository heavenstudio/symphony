//! A stand-in for the Symphony server: the client's build at `/`, the board fixture at
//! `/api/board`, and a feed at `/api/feed` this test pushes events into. Served the way the real
//! server serves them: gzip (the build at the best level, the page and the API at the default),
//! the hashed build cached for good, the shell revalidated by its ETag. As the server does, a
//! build file is read and gzipped by the first request for it and kept for the run, whichever
//! site asks next: a start reads the shell alone, however many retained hashes the build holds.
//! Every request's path is logged, so a test can say what the page fetched and when. The board
//! names the client build served (`Board::build`), as the server reads it from the shell. A test
//! can also play a server in trouble: a board that moved, answers late or names another build, a
//! feed that ends, fails, ends as soon as it opens, or keeps its socket open and says nothing
//! (the stand-in pings every open stream every `ping_ms` its board names, as the server does,
//! until it is told to fall silent).

use std::collections::{BTreeSet, HashMap};
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

struct Shared {
    replies: HashMap<String, Body>,
    posts: Vec<(String, serde_json::Value)>,
    /// Every request's path, in arrival order.
    log: Vec<String>,
    targets: Vec<String>,
    /// The open feed streams, written to by `push`.
    feeds: Vec<TcpStream>,
    /// The board as its route answers it now, but for its build.
    board: serde_json::Value,
    /// The client build the board names.
    build: String,
    /// How long the board's route waits before it answers.
    board_delay: Duration,
    /// How many of the next feed requests are answered 500.
    failing_feeds: usize,
    failing_routes: BTreeSet<String>,
    held_routes: BTreeSet<String>,
    /// Paths whose next request alone is held, and the requests so held, by number.
    hold_next: BTreeSet<String>,
    holding: BTreeSet<u64>,
    held_count: u64,
    /// Answers given to a path's requests in turn, the last again once the rest are given.
    turns: HashMap<String, std::collections::VecDeque<Body>>,
    /// Routes whose requests the server closes unanswered, as a dropped link does.
    dropped_routes: BTreeSet<String>,
    /// Each feed stream ends as soon as it opens.
    ending_feeds: bool,
    /// The feed's pings have stopped, its streams left open.
    silent: bool,
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
            (
                "/api/task-board",
                serde_json::json!({
                    "tasks": [{"slug": "tools/example", "title": "Example task", "body": "",
                        "status": "ready", "priority": 1, "parent": null, "created_by": "owner",
                        "created": 0.0, "updated": 0.0, "links": [], "depends": [], "roles": [],
                        "instruction": false}],
                    "statuses": ["ready"], "counts": [["ready", 1]], "signoff": [], "next": null
                }),
            ),
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
            targets: Vec::new(),
            feeds: Vec::new(),
            board: serde_json::from_str(board).expect("a board"),
            build: build_of(&shell),
            board_delay: Duration::ZERO,
            failing_feeds: 0,
            failing_routes: BTreeSet::new(),
            held_routes: BTreeSet::new(),
            hold_next: BTreeSet::new(),
            holding: BTreeSet::new(),
            held_count: 0,
            turns: HashMap::new(),
            dropped_routes: BTreeSet::new(),
            ending_feeds: false,
            silent: false,
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

    /// The paths requested so far, in order.
    pub fn requests(&self) -> Vec<String> {
        self.shared.lock().unwrap().log.clone()
    }

    pub fn targets(&self) -> Vec<String> {
        self.shared.lock().unwrap().targets.clone()
    }

    pub fn posts(&self) -> Vec<(String, serde_json::Value)> {
        self.shared.lock().unwrap().posts.clone()
    }

    pub fn set_json(&self, path: &str, value: &serde_json::Value) {
        self.set_file(path, value.to_string().into_bytes(), "application/json");
    }

    pub fn set_file(&self, path: &str, bytes: Vec<u8>, mime: &'static str) {
        self.shared
            .lock()
            .unwrap()
            .replies
            .insert(path.into(), body(bytes, mime, "no-store"));
    }

    /// How many feed streams are open now.
    pub fn feeds(&self) -> usize {
        self.shared.lock().unwrap().feeds.len()
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

    /// Fail the actual snapshot route as well as, or instead of, its event stream.
    pub fn fail_route(&self, path: &str, fail: bool) {
        let mut shared = self.shared.lock().unwrap();
        if fail {
            shared.failing_routes.insert(path.to_owned());
        } else {
            shared.failing_routes.remove(path);
        }
    }

    /// Hold a real request until the test releases it, without a timing sleep.
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

    /// Close a route's requests without an answer, so the page's read of it fails.
    pub fn drop_route(&self, path: &str, drop: bool) {
        let mut shared = self.shared.lock().unwrap();
        if drop {
            shared.dropped_routes.insert(path.to_owned());
        } else {
            shared.dropped_routes.remove(path);
        }
    }

    pub fn server_notice(&self, message: &str) {
        self.send(&format!(
            "event: {}\ndata: {}\n\n",
            symphony_api::SERVER_EVENT,
            serde_json::json!(message)
        ));
    }

    /// End every open feed stream, as a server that stops does.
    pub fn drop_feeds(&self) {
        for stream in self.shared.lock().unwrap().feeds.drain(..) {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }

    /// Send one feed event to every open stream; the streams that are gone are dropped.
    pub fn push(&self, id: u64, data: &str) {
        self.send(&format!(
            "event: {}\nid: {id}\ndata: {data}\n\n",
            symphony_api::FEED_EVENT
        ));
    }

    fn send(&self, event: &str) {
        send(&self.shared, event);
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

/// Write `event` to every open feed stream; the streams that are gone are dropped.
fn send(shared: &Mutex<Shared>, event: &str) {
    let mut shared = shared.lock().unwrap();
    shared.feeds.retain_mut(|stream| {
        stream
            .write_all(event.as_bytes())
            .and_then(|_| stream.flush())
            .is_ok()
    });
}

/// The keep-alive ping, as the stand-in and the server (apps/symphony-server/src/api.rs) write it.
pub fn ping() -> String {
    format!("event: {}\ndata:\n\n", symphony_api::PING_EVENT)
}

/// Ping every open stream each `ping_ms` the board now names, until the site stops or falls
/// silent.
fn ping_every_ping_ms(shared: &Mutex<Shared>, stop: &AtomicBool) {
    let ping = ping();
    let mut last = Instant::now();
    while !stop.load(Ordering::SeqCst) {
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
        let cache = if relative.starts_with("assets/") || relative.starts_with("wasm/") {
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
        "wasm" => "application/wasm",
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

/// `bytes` as sent, gzipped by `gzip` (given them and their FNV-1a) when the type compresses.
fn encoded(
    bytes: Vec<u8>,
    content_type: &'static str,
    cache: &'static str,
    gzip: impl FnOnce(&[u8], u64) -> Arc<[u8]>,
) -> Body {
    let hash = fnv1a(&bytes);
    let gzipped = compresses(content_type);
    Body {
        bytes: if gzipped {
            gzip(&bytes, hash)
        } else {
            bytes.into()
        },
        content_type,
        gzipped,
        cache,
        etag: format!("\"{hash:016x}\""),
    }
}

fn gzip_at(bytes: &[u8], level: Compression) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), level);
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
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
        let original = target.split('?').next().unwrap_or("/").to_string();
        let path = match original.strip_prefix("/@").and_then(|s| s.split_once('/')) {
            Some((_, rest)) => format!("/{rest}"),
            None => original,
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
            shared.log.push(path.clone());
            shared.targets.push(target.clone());
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
                || ticket.is_some_and(|ticket| pending.holding.contains(&ticket))
            {
                pending = released.wait(pending).unwrap();
            }
            if pending.dropped_routes.contains(&path) {
                let _ = out.shutdown(Shutdown::Both);
                return;
            }
        }
        if shared.lock().unwrap().failing_routes.contains(&path) {
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
            let mut shared = shared.lock().unwrap();
            if shared.failing_feeds > 0 {
                shared.failing_feeds -= 1;
                let _ = out.write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                return;
            }
            let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\n\r\n: open\n\n";
            if out.write_all(head.as_bytes()).is_ok() {
                if shared.ending_feeds {
                    let _ = out.flush();
                    let _ = out.shutdown(Shutdown::Both);
                } else {
                    shared.feeds.push(out);
                }
            }
            return;
        }
        let board;
        let overridden = turn.or_else(|| {
            let shared = shared.lock().unwrap();
            shared
                .replies
                .get(&target)
                .or_else(|| shared.replies.get(&path))
                .cloned()
        });
        let found = if overridden.is_some() {
            overridden.as_ref()
        } else if path == symphony_api::BOARD_PATH {
            let delay = shared.lock().unwrap().board_delay;
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
        } else {
            files
                .get(&path)
                .or_else(|| {
                    // A route of the app, not a file: the shell, as the server's fallback serves it.
                    let last = path.rsplit('/').next().unwrap_or("");
                    (!path.starts_with("/api/")
                        && (!last.contains('.')
                            || path.starts_with("/docs/") && path.ends_with(".md")))
                    .then(|| files.get("/"))
                    .flatten()
                })
                .map(File::body)
        };
        let written = match found {
            None => out.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n"),
            Some(body) if headers.get("if-none-match") == Some(&body.etag) => out.write_all(
                format!(
                    "HTTP/1.1 304 Not Modified\r\nETag: {}\r\nCache-Control: {}\r\n\r\n",
                    body.etag, body.cache
                )
                .as_bytes(),
            ),
            Some(body) => {
                let encoding = if body.gzipped {
                    "Content-Encoding: gzip\r\nVary: Accept-Encoding\r\n"
                } else {
                    ""
                };
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: {}\r\nETag: {}\r\n{encoding}\r\n",
                    body.content_type,
                    body.bytes.len(),
                    body.cache,
                    body.etag
                );
                out.write_all(head.as_bytes())
                    .and_then(|_| out.write_all(&body.bytes))
            }
        };
        if written.and_then(|_| out.flush()).is_err() {
            return;
        }
    }
}
