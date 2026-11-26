//! Owned process environment and real listeners; no production testing seam or machine lookup.

#[path = "approving.rs"]
pub mod approving;

use serde_json::{Value, json};
use std::io::{BufRead, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use symphony_board::sys::{Child, Command, Spawning as _};
use symphony_server::front::{Shared, router};
use symphony_server::gate::Gate;

/// The wall-clock backstop of every wait of this fixture, six times the 30 s it had as a bound, which
/// only a stall reaches (`test_time::backstop`, RULES.md `tests/no-test-fails-on-load`).
const BACKSTOP: Duration = test_time::backstop(Duration::from_secs(30));

/// A wait for `what` that ran past [`BACKSTOP`]: a stall, and the machine's load then.
fn stalled(what: impl std::fmt::Display) -> String {
    format!(
        "{what}: stalled: the wait's wall-clock backstop of {BACKSTOP:?} ran out; {}",
        test_time::Load::now()
    )
}

pub struct Fixture {
    root: PathBuf,
    address: SocketAddr,
    pub board: symphony_board::Store,
}

#[derive(Debug)]
pub struct Response {
    pub status: u16,
    body: String,
}

impl Response {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or_else(|e| panic!("{self:?}: {e}"))
    }
}

#[derive(Debug, PartialEq)]
pub struct Snapshot {
    document: String,
    asks: String,
    board: Value,
    uploads: Vec<String>,
}

struct OwnedChild(Child);

impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Release a blocked stand-in even when an HTTP assertion unwinds, so no child keeps the test's
/// runtime or scratch repository alive. An unreleased gate deliberately has no timing deadline.
pub struct FinishRelease(std::fs::File, bool);

impl FinishRelease {
    pub fn release(&mut self, result: &str) {
        writeln!(self.0, "{result}").unwrap();
        self.1 = true;
    }
}

impl Drop for FinishRelease {
    fn drop(&mut self) {
        if !self.1 {
            let _ = writeln!(self.0, "ok");
        }
    }
}

struct ScratchRepo {
    root: PathBuf,
}

impl ScratchRepo {
    fn new(test: &str) -> Self {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "symphony-after-host-origin-{}-{unique}-{test}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        // Own cleanup before any further setup can fail.
        let mut scratch = Self { root };
        scratch.root = scratch.root.canonicalize().unwrap();
        let root = &scratch.root;
        std::fs::create_dir(root.join("docs")).unwrap();
        std::fs::write(
            root.join("docs/TOPIC.md"),
            "# Host Origin fixture\n\nInitial document.\n",
        )
        .unwrap();
        // An independent repository outside the source tree cannot discover its worktree.
        for args in [
            vec!["init", "-b", "fixture"],
            vec!["config", "user.name", "Host Origin fixture"],
            vec!["config", "user.email", "fixture@example.invalid"],
            vec!["config", "core.hooksPath", "/dev/null"],
            vec!["add", "docs"],
            vec![
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                "Initial fixture document",
            ],
        ] {
            let done = symphony_board::sys::output(
                Command::new("git")
                    .args(&args)
                    .current_dir(root)
                    .env_remove("GIT_DIR")
                    .env_remove("GIT_WORK_TREE")
                    .env_remove("GIT_INDEX_FILE")
                    .env_remove("GIT_COMMON_DIR"),
            )
            .unwrap();
            assert!(
                done.status.success(),
                "scratch repository {args:?}: {done:?}"
            );
        }
        scratch
    }
}

impl Drop for ScratchRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn scratch_repository_is_removed_after_an_early_return() {
    fn end_early(root: &mut PathBuf) -> Result<(), &'static str> {
        let scratch = ScratchRepo::new("early-return");
        root.clone_from(&scratch.root);
        assert!(root.join(".git/HEAD").is_file());
        assert!(root.join("docs/TOPIC.md").is_file());
        assert!(root.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        assert!(
            !root.starts_with(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .canonicalize()
                    .unwrap()
            )
        );
        Err("fixture ended before starting its server")
    }

    let mut root = PathBuf::new();
    assert_eq!(
        end_early(&mut root),
        Err("fixture ended before starting its server")
    );
    assert!(
        !root.exists(),
        "early-return scratch repository remains: {}",
        root.display()
    );
}

impl Fixture {
    pub fn run(test: &str) {
        Self::run_kind(test, false);
    }

    pub fn run_finishing(test: &str) {
        Self::run_kind(test, true);
    }

    fn run_kind(test: &str, finishing: bool) {
        let started = Instant::now();
        let scratch = ScratchRepo::new(test);
        let root = scratch.root.clone();
        if finishing {
            std::fs::create_dir_all(root.join(".target/bin")).unwrap();
            let notice = root.join("finish.notice");
            let done = std::process::Command::new("mkfifo")
                .arg(&notice)
                .status_locked()
                .unwrap();
            assert!(done.success(), "mkfifo {}: {done}", notice.display());
            // Each call is recorded, then noticed with a line on the FIFO (`Fixture::notices`).
            // Opening it read-write never waits for a reader, so a stand-in outliving the case
            // never blocks there. It is the project's build of `tasks`, which a cancel's cleanup
            // runs, and the sweep's on this test's board (`TASKS_BIN`).
            let stub = root.join(".target/bin/tasks");
            crate::fixture_script::write(
                &stub,
                concat!(
                    "#!/bin/sh\nset -eu\n",
                    "printf '%s|session=%s|terminal=%s|socket=%s\\n' \"$*\" \"${TASKS_SESSION-unset}\" \"${HC_TERMINAL_ID-unset}\" \"${HC_TERMINAL_SOCKET-unset}\" >> \"$SYMPHONY_AFTER_HOST_ORIGIN_FINISH\"\n",
                    "printf '\\n' 1<> \"$SYMPHONY_AFTER_HOST_ORIGIN_NOTICE\"\n",
                    "if [ \"$1\" = finish ] && [ -p \"$PWD/finish.release\" ]; then\n",
                    "  IFS= read -r result < \"$PWD/finish.release\"\n",
                    "  case \"$result\" in\n",
                    "    fail) printf '%s\\n' 'fixture finish failure' >&2; exit 9 ;;\n",
                    "    reported) printf '→ could not clean up the sessions of %s: fixture hc unavailable\\nfixture hc details\\n' \"$2\" ;;\n",
                    "    closed) printf '%s\\n' '→ closed pane t7 of fixture-session (finished); restore: fixture' ;;\n",
                    "  esac\n",
                    "fi\n"
                ),
                false,
            );
        }

        // Execute the installed shell rather than a freshly compiled helper: under load that
        // helper can spend the entire CLI deadline starting up, before it reaches main.
        // As in after_access/fixture.rs, bash reads its first argument as a private script.
        let lookup = root.join("lookup");
        std::fs::create_dir(&lookup).unwrap();
        std::fs::write(
            root.join("status"),
            r#"[ "$#" -eq 1 ] && [ "$1" = --json ] || exit 2
printf '%s\n' "$0 $*" >> "$SYMPHONY_AFTER_HOST_ORIGIN_LOOKUP"
printf '%s\n' '{"Self":{"DNSName":"review-fixture.tail.test.","TailscaleIPs":["fd7a:115c:a1e0::15"]},"TailscaleIPs":["100.83.24.15"]}'
"#,
        )
        .unwrap();
        std::os::unix::fs::symlink("/bin/bash", lookup.join("tailscale")).unwrap();

        // A database of its own on the test binary's PostgreSQL, which ends with this process
        // however it ends.
        let board = crate::board::postgresql();
        let store = symphony_board::Store::opening_on(
            board.another().into(),
            Box::new(symphony_board::sweep::trigger::now),
            Box::new(symphony_board::sys::pid_alive),
            &symphony_board::sys::hostname(),
            &symphony_board::Opening::write(),
        )
        .unwrap();
        store
            .create(&symphony_board::NewTask::new(
                "fixture/host-origin",
                "Private Host Origin task",
                "owner",
            ))
            .unwrap();
        let path = std::env::join_paths(
            std::iter::once(lookup)
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        let out = std::fs::File::create(root.join("http.log")).unwrap();
        let err = std::fs::File::create(root.join("http-stderr.log")).unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", test, "--nocapture"])
            .env("SYMPHONY_AFTER_HOST_ORIGIN_CHILD", &root)
            .env(
                "SYMPHONY_AFTER_HOST_ORIGIN_LOOKUP",
                root.join("lookup.calls"),
            )
            .env("ASKS_DB", root.join("asks.db"))
            .env("TASKS_SERVER", board.addr())
            .env("TASKS_DATABASE", board.database())
            .env("PATH", path)
            .env("TASKS_SWEEP", if finishing { "on" } else { "off" })
            .env_remove("BASH_ENV")
            .env_remove("TASKS_SESSION")
            .env_remove("HC_TERMINAL_ID")
            .env_remove("HC_TERMINAL_SOCKET")
            .env_remove("TASKS_BIN")
            .env_remove("SYMPHONY_UI_DIST")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_COMMON_DIR")
            .current_dir(&root)
            .stdout(out)
            .stderr(err);
        if finishing {
            command // started from an agent's hc terminal, which no board child may join
                .env("TASKS_SESSION", "fixture-session")
                .env("HC_TERMINAL_ID", "t7")
                .env("HC_TERMINAL_SOCKET", "/fixture/hc/t7.sock")
                .env("TASKS_BIN", root.join(".target/bin/tasks"))
                .env(
                    "SYMPHONY_AFTER_HOST_ORIGIN_FINISH",
                    root.join("finish.calls"),
                )
                .env(
                    "SYMPHONY_AFTER_HOST_ORIGIN_NOTICE",
                    root.join("finish.notice"),
                );
        }
        let mut child = OwnedChild(command.spawn().unwrap());
        let status = child.0.wait().unwrap();
        let output = std::fs::read_to_string(root.join("http.log")).unwrap();
        let error = std::fs::read_to_string(root.join("http-stderr.log")).unwrap();
        println!(
            "{output}\nHTTP suite wall time {:.3}s; logs {}",
            started.elapsed().as_secs_f64(),
            root.display()
        );
        assert!(
            status.success(),
            "private HTTP child failed: {output}\n{error}"
        );
        assert!(
            output.contains("1 passed"),
            "the exact child filter must run its assertions"
        );
        drop(scratch);
        assert!(
            !root.exists(),
            "HTTP scratch repository remains: {}",
            root.display()
        );
    }

    pub fn child(address: SocketAddr) -> Self {
        let root = PathBuf::from(std::env::var_os("SYMPHONY_AFTER_HOST_ORIGIN_CHILD").unwrap());
        let env = symphony_board::cli::Env::of_process();
        let board = symphony_board::cli::connect(&env).unwrap();
        Self {
            root,
            address,
            board,
        }
    }

    pub fn host(&self) -> String {
        self.address.to_string()
    }

    pub fn feed(&self) -> Feed {
        let board = self.request("GET", "/api/board", Some(&self.host()), None, &Value::Null);
        assert_eq!(board.status, 200, "{board:?}");
        let version = board.json()["version"].as_u64().unwrap();
        let mut stream = TcpStream::connect_timeout(&self.address, BACKSTOP).unwrap();
        stream.set_read_timeout(Some(BACKSTOP)).unwrap();
        write!(
            stream,
            "GET /api/feed?since={version} HTTP/1.0\r\nHost: {}\r\n\r\n",
            self.host()
        )
        .unwrap();
        let mut feed = Feed(std::io::BufReader::new(stream));
        let status = feed.line();
        assert!(status.starts_with("HTTP/1.0 200 "), "{status}");
        feed.until("headers", |line| line == "\r\n");
        // The notice follows the hub subscription, so no later activity event can be missed.
        feed.until("server notice", |line| line.starts_with("event: server"));
        feed
    }
    /// Makes the checkout's project name the [`approving`] plugin, which answers its owner's
    /// approval with `refusal` (none when empty) and releases the slugs the JSON `released` lists.
    pub fn approving(&self, refusal: &str, released: &str) {
        std::fs::write(
            self.root.join(symphony::project::FILE),
            format!("[plugins]\ncrates = [\"{}\"]\n", approving::NAME),
        )
        .unwrap();
        std::fs::write(self.root.join(approving::REFUSAL), refusal).unwrap();
        std::fs::write(self.root.join(approving::RELEASED), released).unwrap();
    }

    pub fn land_calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.root.join("land.calls"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    pub fn finish_calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.root.join("finish.calls"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    pub fn wait_for_finish(&self) -> Vec<String> {
        self.wait_for_finish_calls(2)
    }

    /// Wakes once the stand-in has recorded `count` calls. The FIFO is open before the first
    /// read, so a record that read misses is noticed after it.
    pub fn wait_for_finish_calls(&self, count: usize) -> Vec<String> {
        let notices = self.notices().lock().unwrap();
        let deadline = Instant::now() + BACKSTOP;
        loop {
            let calls = self.finish_calls();
            if calls.len() >= count {
                return calls;
            }
            notices
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|_| {
                    panic!(
                        "{}",
                        stalled(format!("{count} finish/sweep calls: {calls:?}"))
                    )
                });
        }
    }

    /// One wake per line the stand-in writes on `finish.notice`, from the one reader this process
    /// holds: a second would take some of the lines. A pipe is the kernel's, where a file watcher
    /// on macOS depends on fseventsd, whose stream under load can deliver no event at all.
    fn notices(&self) -> &'static Mutex<Receiver<()>> {
        static NOTICES: OnceLock<Mutex<Receiver<()>>> = OnceLock::new();
        NOTICES.get_or_init(|| {
            // Read-write, as the stand-in opens it: the reader never sees an end of file.
            let fifo = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(self.root.join("finish.notice"))
                .unwrap();
            let (send, receive) = crate::test_channel::channel();
            std::thread::spawn(move || {
                for line in std::io::BufReader::new(fifo).lines() {
                    line.unwrap();
                    if send.send(()).is_err() {
                        return;
                    }
                }
            });
            Mutex::new(receive)
        })
    }

    pub fn block_finish(&self) -> FinishRelease {
        let path = self.root.join("finish.release");
        if !path.exists() {
            let done = std::process::Command::new("mkfifo")
                .arg(&path)
                .status_locked()
                .unwrap();
            assert!(done.success(), "mkfifo {}: {done}", path.display());
        }
        FinishRelease(
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .unwrap(),
            false,
        )
    }
    pub fn document_text(&self) -> String {
        std::fs::read_to_string(self.root.join("docs/TOPIC.md")).unwrap()
    }
    pub fn asks(&self) -> symphony::asks::AskStore {
        symphony::asks::AskStore::open(&self.root.join("asks.db")).unwrap()
    }
    pub fn comment(&self) -> Value {
        json!({"path":"docs/TOPIC.md", "after_line":3, "kind":"question", "title":"Host Origin fixture comment", "body":"Private owner write"})
    }
    pub fn ask(&self) -> Value {
        json!({"path":"docs/TOPIC.md", "question":"Host Origin fixture question"})
    }
    pub fn task_message(&self) -> Value {
        json!({"slug":"fixture/host-origin", "text":"Private owner write"})
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            document: self.document_text(),
            asks: serde_json::to_string(&self.asks().all().unwrap()).unwrap(),
            board: serde_json::to_value(self.board.dump().unwrap()).unwrap(),
            uploads: std::fs::read_dir(symphony::uploads::dir(&self.root))
                .map(|dir| {
                    let mut files: Vec<_> = dir
                        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                        .collect();
                    symphony_board::order::sort_by(&mut files, &Ord::cmp);
                    files
                })
                .unwrap_or_default(),
        }
    }
    pub fn assert_unchanged(&self, baseline: &Snapshot, case: &str) {
        assert_eq!(
            &self.snapshot(),
            baseline,
            "refusal mutated scratch state: {case}"
        );
    }
    pub fn document(&self, host: Option<&str>, origin: Option<&str>) {
        let response = self.request("GET", super::DOCUMENT, host, origin, &Value::Null);
        assert_eq!(response.status, 200, "{host:?} {origin:?}: {response:?}");
        assert_eq!(response.json()["path"], "docs/TOPIC.md");
        assert!(
            response.body.contains("Host Origin fixture"),
            "{response:?}"
        );
    }
    pub fn refused_read(&self, host: Option<&str>, origin: Option<&str>, error: &str) {
        let response = self.request("GET", super::DOCUMENT, host, origin, &Value::Null);
        assert_eq!(response.status, 403, "{host:?} {origin:?}: {response:?}");
        assert_eq!(response.json()["error"], error);
    }
    /// The write refusals the server has logged so far. It logs to this child's stderr, which the
    /// parent sends to `http-stderr.log`.
    pub fn refusals(&self) -> Vec<String> {
        std::fs::read_to_string(self.root.join("http-stderr.log"))
            .unwrap()
            .lines()
            .filter(|line| line.starts_with("· refused a write to "))
            .map(str::to_string)
            .collect()
    }
    /// Each write route refuses `host` and `origin`, and logs one line saying `why`, with the
    /// headers as sent and nothing of the body.
    pub fn refused_writes(&self, host: Option<&str>, origin: Option<&str>, why: &str) {
        let sent =
            |value: Option<&str>| value.map_or_else(|| "absent".into(), |v| format!("\"{v}\""));
        for (route, body) in [
            ("/api/comment", self.comment()),
            ("/api/ask", self.ask()),
            ("/api/task-message", self.task_message()),
            (
                "/api/task-priority",
                json!({"slug":"fixture/host-origin", "priority":0}),
            ),
            ("/api/task-cancel", json!({"slug":"fixture/host-origin"})),
            // An upload's raw body is never read either: nothing is stored.
            ("/api/upload", json!("an owner's screenshot")),
        ] {
            let logged = self.refusals().len();
            let response = self.request("POST", route, host, origin, &body);
            assert_eq!(
                response.status, 403,
                "{route} {host:?} {origin:?}: {response:?}"
            );
            assert_eq!(
                response.json()["error"],
                "Forbidden: the Host or Origin header does not name this app"
            );
            assert_eq!(
                self.refusals()[logged..],
                [format!(
                    "· refused a write to {route}: {why}; Host {}, Origin {}",
                    sent(host),
                    sent(origin)
                )],
                "{route} {host:?} {origin:?}"
            );
        }
    }
    pub fn assert_front(&self, host: Option<&str>, status: u16) {
        let response = self.request("GET", "/api/front", host, None, &Value::Null);
        assert_eq!(response.status, status, "front {host:?}: {response:?}");
        if status == 200 {
            let front = response.json();
            assert_eq!(front["port"], self.address.port());
            assert_eq!(front["ready"], true);
            assert_eq!(front["root"], self.root.to_string_lossy().as_ref());
        } else {
            assert_eq!(
                response.body,
                "Forbidden: the Host header does not name this machine's loopback\n"
            );
        }
    }
    pub fn assert_lookup_used(&self) {
        let calls = std::fs::read_to_string(self.root.join("lookup.calls")).unwrap();
        assert!(!calls.is_empty());
        assert!(calls.lines().all(|line| line == "status --json"), "{calls}");
    }

    pub fn request(
        &self,
        method: &str,
        route: &str,
        host: Option<&str>,
        origin: Option<&str>,
        body: &Value,
    ) -> Response {
        let body = if method == "POST" {
            body.to_string()
        } else {
            String::new()
        };
        // HTTP/1.0 lets a missing Host reach the application, independently of HTTP/1.1 parser rules.
        let mut request = format!(
            "{method} {route} HTTP/1.0\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        );
        for (name, value) in [("Host", host), ("Origin", origin)] {
            if let Some(value) = value {
                request.push_str(&format!("{name}: {value}\r\n"));
            }
        }
        request.push_str(&format!("\r\n{body}"));
        let mut stream = TcpStream::connect_timeout(&self.address, BACKSTOP).unwrap();
        stream.set_read_timeout(Some(BACKSTOP)).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
        Response {
            status,
            body: body.into(),
        }
    }

    /// The status the feed's WebSocket handshake gets under `host` and `origin`, each sent only
    /// when named: `101` when the feed is on its socket, which is then dropped.
    pub fn socket(&self, host: Option<&str>, origin: Option<&str>) -> u16 {
        let board = self.request("GET", "/api/board", Some(&self.host()), None, &Value::Null);
        assert_eq!(board.status, 200, "{board:?}");
        let version = board.json()["version"].as_u64().unwrap();
        let mut request = format!(
            "GET /api/feed?since={version} HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n"
        );
        for (name, value) in [("Host", host), ("Origin", origin)] {
            if let Some(value) = value {
                request.push_str(&format!("{name}: {value}\r\n"));
            }
        }
        request.push_str("\r\n");
        let mut stream = TcpStream::connect_timeout(&self.address, BACKSTOP).unwrap();
        stream.set_read_timeout(Some(BACKSTOP)).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        // Only the head: a refusal keeps the connection alive, and a socket says on and on.
        let mut head = Vec::new();
        let mut byte = [0_u8];
        while !head.ends_with(b"\r\n\r\n") {
            assert_eq!(
                stream.read(&mut byte).unwrap(),
                1,
                "the handshake under {host:?} {origin:?} closed: {}",
                String::from_utf8_lossy(&head)
            );
            head.push(byte[0]);
        }
        let head = String::from_utf8(head).unwrap();
        let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
        if status == 101 {
            // The accept key of RFC 6455's own example: the handshake is the feed's socket's.
            assert!(
                head.to_ascii_lowercase()
                    .contains("sec-websocket-accept: s3pplmbitxaq9kygzzhzrbk+xoo="),
                "{head}"
            );
        }
        status
    }
}

pub struct Feed(std::io::BufReader<TcpStream>);

impl Feed {
    fn line(&mut self) -> String {
        let mut line = String::new();
        assert!(
            self.0.read_line(&mut line).unwrap() > 0,
            "feed closed before its event"
        );
        line
    }

    fn until(&mut self, why: &str, matches: impl Fn(&str) -> bool) {
        let started = Instant::now();
        loop {
            assert!(
                started.elapsed() < BACKSTOP,
                "{}",
                stalled(format!("the feed's {why}"))
            );
            let line = self.line();
            if matches(&line) {
                return;
            }
        }
    }

    pub fn activity(&mut self, id: i64) {
        let key = format!("task#{id}");
        self.until(&format!("activity {key}"), |line| {
            let Some(data) = line.strip_prefix("data: ") else {
                return false;
            };
            let event: Value = serde_json::from_str(data).unwrap();
            event["changes"].as_array().is_some_and(|changes| {
                changes.iter().any(|change| {
                    change["region"] == "activity" && change["data"]["value"]["id"] == key
                })
            })
        });
    }
}

pub struct Server {
    pub address: SocketAddr,
    shared: Arc<Shared>,
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

impl Server {
    pub async fn start() -> Self {
        Self::start_kind(false).await
    }

    pub async fn start_shared() -> Self {
        Self::start_kind(true).await
    }

    async fn start_kind(installed: bool) -> Self {
        let root = PathBuf::from(std::env::var_os("SYMPHONY_AFTER_HOST_ORIGIN_CHILD").unwrap());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        assert_ne!(address.port(), 8765);
        let home = root.join("installation");
        let shared = Shared::new(
            i64::from(std::process::id()),
            address.port(),
            &root,
            installed.then_some(home.as_path()),
            Gate::new(
                Arc::new(|_| panic!("a loopback HTTP client needs no peer lookup")),
                Arc::new(Instant::now),
            ),
        );
        let app = router(shared.clone()).into_make_service_with_connect_info::<SocketAddr>();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        Self {
            address,
            shared,
            stop,
            task,
        }
    }

    pub async fn finish(self) {
        self.shared.stop();
        self.stop.send(()).unwrap();
        self.task.await.unwrap();
    }
}
