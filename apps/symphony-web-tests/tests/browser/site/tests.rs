//! A start reads the shell and no build file; a build file is gzipped at the best level by the
//! first request for it, exactly as the server gzips it, and kept for the run, whichever site of
//! the run asks for it next.

use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;

use super::{Site, fnv1a, outline_of, runs};

/// A build folder of its own under the system's temporary folder: a shell naming `script` and
/// each (path, bytes) of `files`, removed on drop.
struct Dist(PathBuf);

impl Dist {
    fn new(script: &str, files: &[(&str, &[u8])]) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "symphony-browser-site-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(
            root.join("index.html"),
            format!(
                "<!DOCTYPE html><html><head></head><body><script type=\"module\" src=\"{script}\"></script></body></html>"
            ),
        )
        .unwrap();
        for (path, bytes) in files {
            std::fs::write(root.join(path), bytes).unwrap();
        }
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dist {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Text no other test of the run serves, so its gzips are this test's alone: `lines` lines naming
/// `name`, this process and a count.
fn unique(name: &str, lines: usize) -> Vec<u8> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    (0..lines)
        .map(|line| {
            format!(
                "{name} {} {n} line {line} of a build file\n",
                std::process::id()
            )
        })
        .collect::<String>()
        .into_bytes()
}

fn best_gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

/// One GET of `path` with `headers`: the response's head and body.
fn get(addr: SocketAddr, path: &str, headers: &str) -> (String, Vec<u8>) {
    let mut stream = TcpStream::connect(addr).unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: site\r\nAccept-Encoding: gzip\r\n{headers}\r\n"
    )
    .unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    let end = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("a response head");
    let head = String::from_utf8(bytes[..end].to_vec()).unwrap();
    (head, bytes[end + 4..].to_vec())
}

fn board() -> String {
    crate::common::fixture("board.json")
}

#[test]
fn a_start_gzips_nothing_and_each_build_file_is_gzipped_once_for_the_run_by_its_first_request() {
    let (script, wasm, picture) = (
        unique("script", 400),
        unique("wasm", 4_000),
        unique("png", 50),
    );
    let dist = Dist::new(
        "/assets/app.js",
        &[
            ("assets/app.js", &script),
            ("assets/app_bg.wasm", &wasm),
            ("assets/mark.png", &picture),
        ],
    );
    let site = Site::start(dist.path(), &board());
    assert_eq!(
        (runs(&script), runs(&wasm)),
        (0, 0),
        "a start gzips no build file"
    );

    let (head, body) = get(site.addr, "/assets/app.js", "");
    assert!(head.starts_with("HTTP/1.1 200 OK"), "{head}");
    assert!(head.contains("Content-Encoding: gzip"), "{head}");
    assert!(head.contains("Content-Type: text/javascript"), "{head}");
    assert!(
        head.contains("Cache-Control: public, max-age=31536000, immutable"),
        "{head}"
    );
    assert_eq!(
        body,
        best_gzip(&script),
        "the server's bytes: the best level"
    );
    let mut decoded = Vec::new();
    GzDecoder::new(&body[..]).read_to_end(&mut decoded).unwrap();
    assert_eq!(decoded, script);
    assert_eq!(
        (runs(&script), runs(&wasm)),
        (1, 0),
        "only the file asked for"
    );

    let etag = format!("\"{:016x}\"", fnv1a(&script));
    assert!(head.contains(&format!("ETag: {etag}")), "{head}");
    let (again, body) = get(site.addr, "/assets/app.js", "");
    assert!(again.starts_with("HTTP/1.1 200 OK"), "{again}");
    assert_eq!(body, best_gzip(&script));
    let (unchanged, body) = get(
        site.addr,
        "/assets/app.js",
        &format!("If-None-Match: {etag}\r\n"),
    );
    assert!(
        unchanged.starts_with("HTTP/1.1 304 Not Modified"),
        "{unchanged}"
    );
    assert!(body.is_empty());
    assert_eq!(runs(&script), 1, "later requests send the kept gzip");

    // Another site of the run, as the next test starts one, over the same build.
    let next = Site::start(dist.path(), &board());
    let (_, body) = get(next.addr, "/assets/app.js", "");
    assert_eq!(body, best_gzip(&script));
    let (_, body) = get(next.addr, "/assets/app_bg.wasm", "");
    assert_eq!(body, best_gzip(&wasm));
    assert_eq!(
        (runs(&script), runs(&wasm)),
        (1, 1),
        "each content gzipped once in the run, whichever site asked"
    );

    let (head, body) = get(next.addr, "/assets/mark.png", "");
    assert!(head.starts_with("HTTP/1.1 200 OK"), "{head}");
    assert!(!head.contains("Content-Encoding"), "{head}");
    assert_eq!(body, picture, "a picture goes as it is");
    assert_eq!(runs(&picture), 0);
}

/// A route failed or held fails or holds every query of it; one target, only that query.
#[test]
fn a_failure_or_a_hold_names_a_route_or_one_of_its_targets() {
    let dist = Dist::new("/assets/shell.js", &[]);
    let site = Site::start(dist.path(), &board());
    let (column, counts) = ("/api/task-board?status=build", "/api/task-board?counts=1");
    site.set_json("/api/task-board", &serde_json::json!({"tasks": []}));
    let status = |path: &str| get(site.addr, path, "").0[9..12].to_string();
    site.fail_route(column, true);
    assert_eq!(
        (status(column), status(counts)),
        ("500".into(), "200".into())
    );
    site.fail_route(column, false);
    site.fail_route("/api/task-board", true);
    assert_eq!(
        (status(column), status(counts)),
        ("500".into(), "500".into())
    );
    site.fail_route("/api/task-board", false);

    let answered = |site: &Site| {
        site.answers("/api/task-board")
            .iter()
            .filter(|answer| answer.target == column)
            .count()
    };
    let asked = |site: &Site| {
        site.targets()
            .iter()
            .filter(|target| *target == column)
            .count()
    };
    let (before, sent) = (answered(&site), asked(&site));
    site.hold_route(column, true);
    let addr = site.addr;
    let held = std::thread::spawn(move || get(addr, column, "").0[9..12].to_string());
    test_time::until_true(
        "the held request arriving",
        test_time::backstop(std::time::Duration::from_secs(10)),
        || asked(&site) != sent,
    )
    .expect("the held request never arrived");
    assert_eq!(status(counts), "200", "another query of the route answers");
    assert_eq!(answered(&site), before, "the held target waits");
    site.hold_route(column, false);
    assert_eq!(held.join().unwrap(), "200");
}

/// The cache tests' helpers (`cache_pages.rs`) count, slice and hold by the reads they are given,
/// in the order given: a read never asked counts none, and one outside the reads is not held.
#[test]
fn the_cache_helpers_count_slice_and_hold_the_reads_they_are_given() {
    use crate::cache_pages::{answered, hold, since};
    let dist = Dist::new("/assets/shell.js", &[]);
    let site = Site::start(dist.path(), &board());
    site.set_json("/api/queue", &serde_json::json!({"cards": []}));
    site.set_json("/api/asks", &serde_json::json!({"asks": []}));
    let reads = ["/api/queue", "/api/asks", "/api/never-asked"];
    let status = |path: &str| get(site.addr, path, "").0[9..12].to_string();
    assert_eq!(answered(&site, &reads), [0, 0, 0]);
    assert_eq!(answered(&site, &[]), [] as [usize; 0]);
    assert_eq!(status("/api/asks"), "200");
    let before = answered(&site, &reads);
    assert_eq!(before, [0, 1, 0]);
    assert_eq!(since(&site, &reads, &before, "/api/asks"), []);
    assert_eq!(status("/api/asks?card=one"), "200");
    assert_eq!(status("/api/queue"), "200");
    let targets = |read: &str| -> Vec<String> {
        since(&site, &reads, &before, read)
            .into_iter()
            .map(|answer| answer.target)
            .collect()
    };
    assert_eq!(targets("/api/asks"), ["/api/asks?card=one"]);
    assert_eq!(targets("/api/queue"), ["/api/queue"]);
    assert_eq!(targets("/api/never-asked"), [] as [&str; 0]);
    assert_eq!(answered(&site, &reads), [1, 2, 0]);
    assert_eq!(
        answered(&site, &["/api/asks", "/api/queue"]),
        [2, 1],
        "counted in the order the reads are given"
    );

    hold(&site, &["/api/queue"], true);
    let (addr, asked) = (site.addr, site.targets().len());
    let held = std::thread::spawn(move || get(addr, "/api/queue", "").0[9..12].to_string());
    test_time::until_true(
        "the held request arriving",
        test_time::backstop(std::time::Duration::from_secs(10)),
        || site.targets().len() != asked,
    )
    .expect("the held request never arrived");
    assert_eq!(status("/api/asks"), "200", "a read not named is not held");
    assert_eq!(answered(&site, &reads), [1, 3, 0], "the held read waits");
    hold(&site, &["/api/queue"], false);
    assert_eq!(held.join().unwrap(), "200");
    assert_eq!(answered(&site, &reads), [2, 3, 0]);
}

/// Asking for the answers to a read that is none of the reads counted is the test's mistake.
#[test]
#[should_panic(expected = "/api/asks is none of the reads counted, [\"/api/queue\"]")]
fn the_answers_since_a_count_are_of_a_read_that_was_counted() {
    let dist = Dist::new("/assets/shell.js", &[]);
    let site = Site::start(dist.path(), &board());
    let reads = ["/api/queue"];
    let before = crate::cache_pages::answered(&site, &reads);
    crate::cache_pages::since(&site, &reads, &before, "/api/asks");
}

/// A retired build file answers 404 until it is restored, and only it: the others stay served.
#[test]
fn a_retired_build_file_is_gone_until_it_is_restored() {
    let (chunk, other) = (unique("retired chunk", 10), unique("kept chunk", 10));
    let dist = Dist::new(
        "/assets/shell.js",
        &[("assets/chunk.wasm", &chunk), ("assets/other.wasm", &other)],
    );
    let site = Site::start(dist.path(), &board());
    let status = |path: &str| get(site.addr, path, "").0[9..12].to_string();
    let (retired, kept) = ("/assets/chunk.wasm", "/assets/other.wasm");
    assert_eq!(
        (status(retired), status(kept)),
        ("200".into(), "200".into())
    );
    site.retire(retired);
    assert_eq!(
        (status(retired), status(kept)),
        ("404".into(), "200".into())
    );
    site.restore(retired);
    assert_eq!(
        (status(retired), status(kept)),
        ("200".into(), "200".into())
    );
}

/// A path's first request is timed as the site reads it, before its answer; a later one, any query
/// of it, or a read ahead moves nothing.
#[test]
fn a_paths_first_request_is_timed_as_it_is_read() {
    let dist = Dist::new("/assets/shell.js", &[]);
    let site = Site::start(dist.path(), &board());
    site.set_json("/api/read", &serde_json::json!({}));
    assert_eq!(site.first_asked("/tasks"), None);
    let before = Instant::now();
    get(site.addr, "/tasks", "");
    let answered = Instant::now();
    let first = site.first_asked("/tasks").expect("asked for");
    assert!(before <= first && first <= answered, "{first:?}");
    get(site.addr, "/tasks?view=list", "");
    get(site.addr, "/tasks", "");
    assert_eq!(site.first_asked("/tasks"), Some(first), "the first stays");
    get(site.addr, "/api/read", "Purpose: prefetch\r\n");
    assert_eq!(
        site.first_asked("/api/read"),
        None,
        "a read ahead is no page's"
    );
    let later = Instant::now();
    get(site.addr, "/api/read", "");
    assert!(
        site.first_asked("/api/read")
            .is_some_and(|read| read >= later)
    );
}

#[test]
fn the_shell_and_the_routes_it_answers_for_are_served_from_the_start() {
    let script = unique("shell script", 10);
    let dist = Dist::new("/assets/shell.js", &[("assets/shell.js", &script)]);
    let site = Site::start(dist.path(), &board());
    for path in ["/", "/index.html", "/tasks", "/assets/", "/docs/README.md"] {
        let (head, body) = get(site.addr, path, "");
        assert!(head.starts_with("HTTP/1.1 200 OK"), "{path}: {head}");
        assert!(head.contains("Cache-Control: no-cache"), "{path}: {head}");
        let mut page = String::new();
        GzDecoder::new(&body[..]).read_to_string(&mut page).unwrap();
        assert!(page.contains("src=\"/assets/shell.js\""), "{path}: {page}");
    }
    for path in ["/assets/missing.js", "/api/nothing"] {
        let (head, _) = get(site.addr, path, "");
        assert!(head.starts_with("HTTP/1.1 404 Not Found"), "{path}: {head}");
    }
    assert_eq!(runs(&script), 0, "no request named the script");
}

#[test]
fn the_build_sizes_are_each_files_gzip_as_sent_made_once_for_the_run() {
    let (script, style, picture) = (
        unique("sized script", 300),
        unique("sized style", 200),
        unique("sized png", 20),
    );
    let dist = Dist::new(
        "/assets/app.js",
        &[
            ("assets/app.js", &script),
            ("assets/app.css", &style),
            ("assets/mark.png", &picture),
        ],
    );
    let site = Site::start(dist.path(), &board());
    let sizes = site.build_sizes();
    let size = |path: &str| {
        sizes
            .iter()
            .find(|(named, _)| named == path)
            .unwrap_or_else(|| panic!("{path} in {sizes:?}"))
            .1
    };
    assert_eq!(size("/assets/app.js"), best_gzip(&script).len());
    assert_eq!(size("/assets/app.css"), best_gzip(&style).len());
    assert_eq!(size("/assets/mark.png"), picture.len());
    assert!(sizes.iter().any(|(path, _)| path == "/index.html"));
    assert_eq!(sizes.len(), 4, "each file once, no route: {sizes:?}");
    let (_, body) = get(site.addr, "/assets/app.css", "");
    assert_eq!(body, best_gzip(&style));
    assert_eq!((runs(&script), runs(&style)), (1, 1));
}

#[test]
fn a_documents_outline_names_the_ids_its_fixture_gives_each_section_and_none_otherwise() {
    let document = |anchors: Option<serde_json::Value>| {
        let mut document = serde_json::json!({"version": 1, "path": "docs/a.md", "title": "A",
            "waiting": [], "crumbs": [], "headings": [], "tasks": [], "cards": [], "threads": {},
            "sections": [{"start": 1, "end": 2, "key": "card:one", "hash": "h", "part": 0,
                "blocks": [{"type": "paragraph", "spans": []}]}]});
        if let Some(anchors) = anchors {
            document["anchors"] = anchors;
        }
        document
    };
    let named = outline_of(&document(Some(serde_json::json!({"card:one": ["one"]}))));
    assert_eq!(named["anchors"], serde_json::json!({"card:one": ["one"]}));
    assert_eq!(
        named["document"].get("anchors"),
        None,
        "the ids are the outline's alone"
    );
    assert_eq!(
        named["document"]["sections"][0]["blocks"],
        serde_json::json!([])
    );
    assert_eq!(
        outline_of(&document(None))["anchors"],
        serde_json::json!({})
    );
}

/// An open feed stream follows the sections its request named, each by its whole key, and none
/// once it is closed; a stream naming none, or no list, follows none.
#[test]
fn a_feed_stream_follows_the_sections_its_request_named_while_it_is_open() {
    let dist = Dist::new("/assets/shell.js", &[]);
    let site = Site::start(dist.path(), &board());
    let backstop = test_time::backstop(std::time::Duration::from_secs(10));
    let open = |query: &str, open: usize| {
        let mut stream = TcpStream::connect(site.addr).unwrap();
        write!(
            stream,
            "GET {}?since=1&document=docs%2FPROBE.md{query} HTTP/1.1\r\nHost: site\r\n\r\n",
            symphony_api::FEED_PATH
        )
        .unwrap();
        test_time::until_true("the feed stream open", backstop, || site.feeds() == open)
            .expect("the stream never opened");
        stream
    };
    let streams = [
        open("", 1),
        open("&sections=%5B%5D", 2),
        open("&sections=intro", 3),
    ];
    assert!(
        !site.following("intro"),
        "a stream naming no section, an empty list or no list follows none"
    );
    let named = open("&sections=%5B%22intro%22%2C%22card%3Aone%22%5D", 4);
    assert!(site.following("intro") && site.following("card:one"));
    assert!(
        !site.following("intr") && !site.following("one") && !site.following("other"),
        "a section is followed by its whole key alone"
    );
    drop(named);
    test_time::until_true("the closed stream counted no more", backstop, || {
        site.feeds() == 3
    })
    .expect("the stream never closed");
    assert!(!site.following("intro"), "a closed stream follows nothing");
    drop(streams);
}

/// What `stream` said until it said `said`: every byte, the head's too. A stream that never says
/// it is a stall once `backstop` has passed over all its reads ([`test_time::backstop`]).
fn heard_until(stream: &mut TcpStream, said: &[u8], backstop: std::time::Duration) -> Vec<u8> {
    let until = std::time::Instant::now() + backstop;
    let mut heard = Vec::new();
    let mut chunk = [0; 512];
    while !heard.windows(said.len()).any(|window| window == said) {
        let left = until.saturating_duration_since(std::time::Instant::now());
        let read = if left.is_zero() {
            Err(std::io::ErrorKind::TimedOut.into())
        } else {
            (stream.set_read_timeout(Some(left))).and_then(|()| stream.read(&mut chunk))
        };
        let read = read.unwrap_or_else(|error: std::io::Error| {
            panic!(
                "never said {:?} ({error}), only {:?}",
                String::from_utf8_lossy(said),
                String::from_utf8_lossy(&heard)
            )
        });
        assert!(
            read > 0,
            "ended, saying only {:?}",
            String::from_utf8_lossy(&heard)
        );
        heard.extend_from_slice(&chunk[..read]);
    }
    heard
}

/// Whether `heard` says `said` first after its head (`head`, up to its end).
fn first_after(heard: &[u8], head: &[u8], said: &[u8]) -> bool {
    heard
        .windows(head.len())
        .position(|window| window == head)
        .is_some_and(|at| heard[at + head.len()..].starts_with(said))
}

/// The server's notice is said on every open feed stream and first on each one opened after it,
/// the last one said, an empty one too, as a Server-Sent Event or a WebSocket's frame, as the
/// server says it (apps/symphony-server/src/api.rs, `feed`); a stream opened before any notice
/// says none as it opens.
#[test]
fn the_servers_notice_is_said_on_each_open_stream_and_first_on_each_opened_after() {
    use super::Said;
    let dist = Dist::new("/assets/shell.js", &[]);
    let site = Site::start(dist.path(), &board());
    let backstop = test_time::backstop(std::time::Duration::from_secs(10));
    let open = |socket: bool, open: usize| {
        let mut stream = TcpStream::connect(site.addr).unwrap();
        let upgrade = if socket {
            "Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n"
        } else {
            ""
        };
        write!(
            stream,
            "GET {}?since=1 HTTP/1.1\r\nHost: site\r\n{upgrade}\r\n",
            symphony_api::FEED_PATH
        )
        .unwrap();
        test_time::until_true("the feed stream open", backstop, || site.feeds() == open)
            .expect("the stream never opened");
        stream
    };
    let (event_head, socket_head) = (&b": open\n\n"[..], &b"\r\n\r\n"[..]);
    let mut before = open(false, 1);
    let opened = heard_until(&mut before, event_head, backstop);
    assert!(
        !String::from_utf8_lossy(&opened).contains(symphony_api::SERVER_EVENT),
        "no notice before one is said: {:?}",
        String::from_utf8_lossy(&opened)
    );

    let held = Said::notice("held");
    site.server_notice("held");
    heard_until(&mut before, held.sse().as_bytes(), backstop);
    let mut events = open(false, 2);
    let mut socket = open(true, 3);
    assert!(first_after(
        &heard_until(&mut events, held.sse().as_bytes(), backstop),
        event_head,
        held.sse().as_bytes()
    ));
    assert!(first_after(
        &heard_until(&mut socket, &held.frame(), backstop),
        socket_head,
        &held.frame()
    ));

    let cleared = Said::notice("");
    site.server_notice("");
    for stream in [&mut before, &mut events] {
        heard_until(stream, cleared.sse().as_bytes(), backstop);
    }
    heard_until(&mut socket, &cleared.frame(), backstop);
    let mut after = open(false, 4);
    assert!(
        first_after(
            &heard_until(&mut after, cleared.sse().as_bytes(), backstop),
            event_head,
            cleared.sse().as_bytes()
        ),
        "the last notice said, the empty one, and not the one before it"
    );
}
