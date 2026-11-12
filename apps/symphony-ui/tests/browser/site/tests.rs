//! A start reads the shell and no build file; a build file is gzipped at the best level by the
//! first request for it, exactly as the server gzips it, and kept for the run, whichever site of
//! the run asks for it next.

use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;

use super::{Site, fnv1a, runs};

/// A build folder of its own under the system's temporary folder: a shell naming `script` and
/// each (path, bytes) of `files`, removed on drop.
struct Dist(PathBuf);

impl Dist {
    fn new(script: &str, files: &[(&str, &[u8])]) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "sf-browser-site-{}-{}",
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
