//! Native generations over private sockets and repositories. Builds and generated-output checks
//! rendezvous with the test; HTTP bodies hold accepted saves without a wall-time backlog.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::cell::Cell;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use symphony_board::sys::{Child, Command};
use symphony_board::{Store, pg, sys};
use symphony_server::sys::Taken;

#[expect(
    dead_code,
    reason = "Only the private database is needed by this fixture"
)]
#[path = "../../../crates/tasks/tests/store/common.rs"]
mod board;
#[path = "after_reload/first_run.rs"]
mod first_run;
#[path = "after_reload/gate.rs"]
mod gate;
#[path = "after_reload/lines.rs"]
mod lines;
use gate::{Entry, Gate};
use lines::Lines;

/// What a wait of this target takes on a quiet machine with room to spare: its allowance.
const ALLOWANCE: Duration = Duration::from_secs(45);
/// The wall-clock backstop of every wait of this target, which only a stall reaches
/// (`test_time::backstop`, RULES.md `tests/no-test-fails-on-load`).
const LIMIT: Duration = test_time::backstop(ALLOWANCE);

/// A wait for `what` that ran past [`LIMIT`]: a stall, and the machine's load then.
fn stalled(what: impl std::fmt::Display) -> String {
    format!(
        "{what}: stalled: the wait's wall-clock backstop of {LIMIT:?} ran out; {}",
        test_time::Load::now()
    )
}
const DOC: &str = "# Reload fixture\n\n<!-- register:start -->\n- [ ] **one** — A proposal.\n<!-- register:end -->\n";

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn repository(root: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    let done = sys::output(
        command
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .current_dir(root),
    )
    .unwrap();
    assert!(done.status.success(), "{args:?}: {done:?}");
    String::from_utf8(done.stdout).unwrap().trim_end().into()
}

struct OwnedChild(Child);
impl OwnedChild {
    fn wait(&mut self) {
        let id = self.0.id();
        let status = test_time::until(format!("child {id} to end"), LIMIT, || {
            self.0.try_wait().unwrap()
        })
        .unwrap_or_else(|stall| panic!("{stall}"));
        assert!(status.success(), "child {id}: {status}");
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// This native fixture stands at the compiler/generator boundary, never at the HTTP or persistence
/// boundary. It executes the actual server after the startup handshake. No script files are made.
/// The stand-ins run the machine's copy of its bytes, already run (`first_run::stand_in`).
fn driver(logs: &Path) -> &'static Path {
    static DRIVER: OnceLock<PathBuf> = OnceLock::new();
    DRIVER.get_or_init(|| {
        // The linker names its signature after the output file, so one name for every compile
        // gives an unchanged source the same bytes, and every run the same machine copy. The
        // folder's name is one no earlier run's driver file took.
        let folder = logs.join(format!("reload-driver-build-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        let output = folder.join("reload-driver");
        // rustc reads the whole source before it says anything, so the source is written first.
        let mut compiler = OwnedChild(
            sys::spawn_piped(
                Command::new("rustc")
                    .args(["--edition=2024", "-", "-o"])
                    .arg(&output),
            )
            .unwrap(),
        );
        compiler
            .0
            .stdin
            .take()
            .unwrap()
            .write_all(
                DRIVER_SOURCE
                    .replace(
                        "__DIRTY_MAIN__",
                        &format!("{:?}", symphony::landing::DIRTY_MAIN),
                    )
                    .replace("__LIMIT_SECS__", &LIMIT.as_secs().to_string())
                    .as_bytes(),
            )
            .unwrap();
        let mut diagnostic = String::new();
        compiler
            .0
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut diagnostic)
            .unwrap();
        assert!(
            compiler.0.wait().unwrap().success(),
            "driver compile: {diagnostic}"
        );
        let copy = first_run::stand_in(&output);
        fs::remove_dir_all(&folder).unwrap();
        copy
    })
}

const DRIVER_SOURCE: &str = r#"
use std::{env, fs, io::{BufRead, BufReader, Write}, os::unix::net::UnixStream, path::PathBuf, process::Command};
use std::os::unix::process::CommandExt;
const DIRTY_MAIN: &str = __DIRTY_MAIN__;
fn event(kind: &str) -> String {
    let mut s = UnixStream::connect(env::var_os("SYMPHONY_AFTER_RELOAD_GATE").unwrap()).unwrap();
    // The case answers within the target's backstop, which only a stall reaches.
    s.set_read_timeout(Some(std::time::Duration::from_secs(__LIMIT_SECS__))).unwrap();
    writeln!(s, "{kind} {}", std::process::id()).unwrap();
    let mut answer = String::new();
    assert!(BufReader::new(s).read_line(&mut answer).unwrap() > 0);
    answer.trim().into()
}
fn main() {
    let args: Vec<_> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("symphony-ui-dist") => {
            assert_eq!(args.len(), 2);
            let public = PathBuf::from(&args[1]).join("public");
            fs::create_dir_all(public.join("assets")).unwrap();
            fs::write(public.join("index.html"), fs::read("symphony/apps/symphony-web/src/main.tsx").unwrap()).unwrap();
            fs::write(public.join("assets/new-0123456789abcdef.js"), "new module").unwrap();
            let action = event("client");
            if action == "fail" { eprintln!("fixture client compiler refused source"); std::process::exit(25); }
            if action == "empty" { fs::remove_file(public.join("index.html")).unwrap(); return; }
            assert_eq!(action, "bundle");
        }
        Some("slots") => {
            assert_eq!(&args[..3], ["slots", "cargo", "install"]);
            let action = event("build");
            if action == "fail" { eprintln!("fixture compiler refused source"); std::process::exit(23); }
            assert_eq!(action, "install");
            let home = PathBuf::from(args.last().unwrap());
            let next = home.join("bin/next");
            // A link to this driver, already run: a copy would be a new file, its first run the
            // replacement's, inside the case's startup wait.
            let _ = fs::remove_file(&next);
            std::os::unix::fs::symlink(env::current_exe().unwrap().canonicalize().unwrap(), &next).unwrap();
            fs::rename(next, home.join("bin/symphony-server")).unwrap();
        }
        Some("serve") => {
            if event("startup") == "exit" { std::process::exit(24); }
            panic!("exec failed: {}", Command::new(env::var_os("SYMPHONY_AFTER_RELOAD_SERVER").unwrap()).args(args).exec());
        }
        Some("content") => {
            if fs::read_to_string("docs/TOPIC.md").unwrap().contains("[x] **one**") {
                fs::create_dir_all("content").unwrap();
                fs::write("content/one", "approved\n").unwrap();
            }
        }
        Some("content-ids") => assert_eq!(args, ["content-ids"]),
        Some("lamp-atlas") => {
            assert_eq!(args, ["lamp-atlas", "build"]);
            fs::create_dir_all("view").unwrap();
            fs::copy("content/one", "view/lamp_atlas").unwrap();
        }
        Some("verify") => {
            assert_eq!(args, ["verify"]);
            assert!(fs::read_to_string("docs/TOPIC.md").unwrap().contains("[x] **one**"));
            assert_eq!(fs::read_to_string("content/one").unwrap(), "approved\n");
            assert_eq!(fs::read_to_string("view/lamp_atlas").unwrap(), "approved\n");
            let actor = env::var("TASKS_SESSION").unwrap();
            assert!(!actor.is_empty() && actor != "owner");
            let cwd = env::current_dir().unwrap();
            let tip = Command::new("git").args(["rev-parse", "HEAD"]).output().unwrap();
            assert!(tip.status.success());
            use std::fs::OpenOptions;
            writeln!(OpenOptions::new().create(true).append(true).open(env::var_os("SYMPHONY_AFTER_RELOAD_COMMANDS").unwrap()).unwrap(), "verify\t{}\t{actor}\t{}", cwd.display(), String::from_utf8(tip.stdout).unwrap().trim()).unwrap();
            assert_eq!(event("outputs"), "publish");
        }
        Some("land") => {
            assert_eq!(args.len(), 2);
            let actor = env::var("TASKS_SESSION").unwrap();
            assert!(!actor.is_empty() && actor != "owner");
            let cwd = env::current_dir().unwrap();
            let git = |args: &[&str]| {
                let done = Command::new("git").args(args).output().unwrap();
                assert!(done.status.success(), "{args:?}: {done:?}");
                String::from_utf8(done.stdout).unwrap()
            };
            let tip = git(&["rev-parse", "HEAD"]);
            use std::fs::OpenOptions;
            writeln!(OpenOptions::new().create(true).append(true).open(env::var_os("SYMPHONY_AFTER_RELOAD_COMMANDS").unwrap()).unwrap(), "land\t{}\t{actor}\t{}", cwd.display(), tip.trim()).unwrap();
            assert_eq!(git(&["symbolic-ref", "--short", "HEAD"]).trim(), args[1]);
            let main = env::var("SYMPHONY_AFTER_RELOAD_ROOT").unwrap();
            let clean = || {
                if !git(&["-C", &main, "status", "--porcelain", "--untracked-files=no"]).is_empty() {
                    eprintln!("main checkout {main} {DIRTY_MAIN}");
                    std::process::exit(1);
                }
            };
            clean();
            git(&["rebase", "main"]);
            let writes = fs::OpenOptions::new().read(true).write(true)
                .open(PathBuf::from(&main).join(".git/symphony/server/verdict-writes.lock")).unwrap();
            writes.lock().unwrap();
            clean();
            git(&["-C", &main, "merge", "--ff-only", "-q", &args[1]]);
            writes.unlock().unwrap();
        }
        Some("status") => println!("{{}}"),
        Some("tasks") => assert_eq!(args, ["tasks", "reap"]),
        _ => panic!("unexpected fixture command: {args:?}"),
    }
}
"#;

struct Fixture {
    name: String,
    dir: PathBuf,
    root: PathBuf,
    home: PathBuf,
    gate: Gate,
    board: Store,
    /// The board's database, of its own on the test binary's PostgreSQL, which ends with this
    /// process however it ends.
    database: pg::Client,
    /// Each server generation seen, by the start the kernel stamped on it: the drop signals only
    /// one its pid still names, never a process given the pid since.
    generations: Vec<Taken>,
    began: Instant,
    /// When the last source edit landed, until a handshake answers it.
    edited: Cell<Option<Instant>>,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let logs =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/amber-kestrel-54");
        fs::create_dir_all(&logs).unwrap();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = logs.join(format!("{name}-{}-{unique}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let root = dir.join("repo");
        fs::create_dir(&root).unwrap();
        repository(&root, &["init", "-q", "-b", "main"]);
        repository(&root, &["config", "user.name", "Review fixture"]);
        repository(&root, &["config", "user.email", "fixture@example.invalid"]);
        repository(&root, &["config", "core.hooksPath", "/dev/null"]);
        write(&root.join("docs/TOPIC.md"), DOC);
        write(&root.join(".gitignore"), "artifacts/\n");
        // The project's verdict commands, which the fixture's `just` stands in for, and the command
        // the server's rebuild runs Cargo by, which the driver stands in for.
        write(
            &root.join("symphony.toml"),
            "[project]\ncargo = [\"tools/rs-bin.sh\", \"slots\", \"cargo\"]\n\
             [verdicts]\ncheck = [\"just\", \"verify\"]\nland = [\"just\", \"land\"]\ngenerators = [\n  \
             { name = \"content\", outputs = \"content/\", command = [\"just\", \"content\"] },\n  \
             { name = \"content-ids\", outputs = \"sim/content_ids.gd\", command = [\"just\", \"content-ids\"], when_changed = true },\n  \
             { name = \"lamp-atlas\", outputs = \"view/lamp_atlas\", command = [\"just\", \"lamp-atlas\", \"build\"], when_changed = true },\n]\n\
             record = \".git/land/verdicts\"\n",
        );
        write(
            &root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"symphony/apps/symphony-server\"]\n",
        );
        write(
            &root.join("symphony/apps/symphony-server/Cargo.toml"),
            "[package]\nname = \"symphony-server\"\n[dependencies]\nsymphony-api = { path = \"../../crates/symphony-api\" }\n",
        );
        write(
            &root.join("symphony/apps/symphony-server/src/live.rs"),
            "// initial module\n",
        );
        write(
            &root.join("symphony/apps/symphony-server/src/main.rs"),
            "fn main() {}\n",
        );
        write(
            &root.join("symphony/apps/symphony-web/package.json"),
            "{}\n",
        );
        write(
            &root.join("symphony/apps/symphony-web/src/main.tsx"),
            "initial client",
        );
        write(
            &root.join("symphony/apps/symphony-web/src/styles/frame.css"),
            "body {}\n",
        );
        write(
            &root.join("symphony/crates/symphony-api/Cargo.toml"),
            "[package]\nname = \"symphony-api\"\n",
        );
        write(
            &root.join("symphony/crates/symphony-api/src/lib.rs"),
            "// initial contract\n",
        );
        write(
            &root.join("tools/test_budget.json"),
            r#"{"before_verify_after_port":true}"#,
        );
        fs::create_dir_all(root.join("crates")).unwrap();
        fs::create_dir_all(dir.join("bin")).unwrap();
        let driver = driver(&logs);
        // The verdict commit's first run of the board CLI, here rather than in the case's wait.
        tasks_binary();
        for path in [
            root.join("tools/rs-bin.sh"),
            dir.join("bin/just"),
            dir.join("bin/tailscale"),
        ] {
            std::os::unix::fs::symlink(driver, path).unwrap();
        }
        repository(&root, &["add", "."]);
        repository(&root, &["commit", "-qm", "Initial fixture"]);
        let home = dir.join("installation");
        fs::create_dir_all(home.join("bin")).unwrap();
        fs::create_dir(dir.join("tmp")).unwrap();
        // A link, as a copy would be a new file whose first run is this case's start. Its stamp
        // names the image file its hash was taken of, as the server's install stamp does, so the
        // start takes the hash as stamped instead of reading the image again.
        std::os::unix::fs::symlink(first_run::server_image(), home.join("bin/symphony-server"))
            .unwrap();
        let fingerprint = symphony::reload::fingerprint(&root).unwrap();
        let (binary, image) = first_run::server_stamp();
        write(
            &home.join("installed.json"),
            &json!({"source":fingerprint,"binary":binary,"image":image}).to_string(),
        );
        write(&home.join("client/public/index.html"), "initial client");
        write(
            &home.join("client/public/assets/old-456456456456456a.js"),
            "old module",
        );
        write(
            &home.join("client/public/assets/chunks/old-789789789789789a.js"),
            "old chunk",
        );
        let mut distribution = Sha256::new();
        // This fixture's installed files in lexical order, matching the documented stamp
        // format. No production publication helper is used to prepare the old generation.
        for (name, contents) in [
            ("assets/chunks/old-789789789789789a.js", "old chunk"),
            ("assets/old-456456456456456a.js", "old module"),
            ("index.html", "initial client"),
        ] {
            distribution.update(name.len().to_le_bytes());
            distribution.update(name.as_bytes());
            distribution.update(contents.len().to_le_bytes());
            distribution.update(contents.as_bytes());
        }
        let distribution: String = distribution
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        write(&home.join("client-installed.json"), &json!({
            "source": symphony::reload::client_fingerprint(&root).unwrap(), "distribution": distribution
        }).to_string());

        let database = board::postgresql();
        let board = Store::opening_on(
            database.another().into(),
            Box::new(symphony_board::sweep::trigger::now),
            Box::new(sys::pid_alive),
            &sys::hostname(),
            &symphony_board::Opening::write(),
        )
        .unwrap();
        Self {
            name: name.into(),
            dir,
            root,
            home,
            gate: Gate::new(),
            board,
            database,
            generations: Vec::new(),
            began: Instant::now(),
            edited: Cell::new(None),
        }
    }
    fn server(&self, installed: bool) -> (OwnedChild, Lines, SocketAddr) {
        let mut command = Command::new(if installed {
            self.home.join("bin/symphony-server")
        } else {
            first_run::server_image().into()
        });
        for (key, _) in std::env::vars_os() {
            let name = key.to_string_lossy();
            if name.starts_with("SYMPHONY_")
                || name.starts_with("TASKS_")
                || name.starts_with("GIT_")
                || name.starts_with("HC_")
                || name.starts_with("HERDR_")
            {
                command.env_remove(key);
            }
        }
        // Keep profiler output isolated too, while allowing the instrumented generation's exec.
        if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
            command.env("LLVM_PROFILE_FILE", profile);
        }
        let path = std::env::join_paths(
            std::iter::once(self.dir.join("bin"))
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        command
            .args(["serve", "--root"])
            .arg(&self.root)
            .args(["--port", "0", "--host", "127.0.0.1"])
            .env("PATH", path)
            .env("SYMPHONY_AFTER_RELOAD_GATE", self.gate.path())
            .env(
                "SYMPHONY_AFTER_RELOAD_COMMANDS",
                self.dir.join("commands.log"),
            )
            .env("SYMPHONY_AFTER_RELOAD_ROOT", &self.root)
            .env("SYMPHONY_AFTER_RELOAD_SERVER", first_run::server_image())
            .env("SYMPHONY_TEST_PID", std::process::id().to_string())
            .env("TASKS_SERVER", self.database.addr())
            .env("TASKS_DATABASE", self.database.database())
            .env(
                "SYMPHONY_TASKS_CLI",
                format!("'{}'", tasks_binary().display()),
            )
            .env("HC_BIN", "/usr/bin/false")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("ASKS_DB", self.dir.join("asks.db"))
            .env("TMPDIR", self.dir.join("tmp"))
            .current_dir(&self.root);
        if installed {
            command.arg("--home").arg(&self.home);
        }
        let spawned = Instant::now();
        let mut child = OwnedChild(sys::spawn_output(&mut command).unwrap());
        let mut lines = Lines::of(&mut child.0, &self.dir.join("server.log"));
        let line = lines.until("→ Symphony at http://127.0.0.1:");
        println!(
            "after_reload: {}: the {} start's ready line after {:.3}s",
            self.name,
            if installed { "installed" } else { "built" },
            spawned.elapsed().as_secs_f64()
        );
        let url = line
            .split_once("→ Symphony at ")
            .unwrap()
            .1
            .split_whitespace()
            .next()
            .unwrap();
        let address: SocketAddr = url
            .strip_prefix("http://")
            .unwrap()
            .trim_end_matches('/')
            .parse()
            .unwrap();
        assert_ne!(address.port(), 8765);
        assert_front(address, i64::from(child.0.id()));
        (child, lines, address)
    }
    fn startup(&mut self) -> Entry {
        let entry = self.entered("startup");
        self.generations.push(Taken::of(entry.pid));
        entry
    }
    /// The next `kind` handshake; the first after an edit says how long the edit took to wake the
    /// reload watcher and reach it: an edit its file event missed waits for the watcher's period.
    fn entered(&self, kind: &str) -> Entry {
        let entry = self.gate.entered(kind);
        if let Some(edited) = self.edited.take() {
            println!(
                "after_reload: {}: the edit's {kind} handshake after {:.3}s",
                self.name,
                edited.elapsed().as_secs_f64()
            );
        }
        entry
    }
    /// The verdict commit's check (`outputs`) for a decision whose body was sent at `decided`: the
    /// commit, its board CLI run and the check's start lie between the two.
    fn committed(&self, decided: Instant) -> Entry {
        let entry = self.gate.entered("outputs");
        println!(
            "after_reload: {}: the verdict commit's check after {:.3}s",
            self.name,
            decided.elapsed().as_secs_f64()
        );
        entry
    }
    fn edit(&self, file: &str, text: &str) {
        self.source(&format!("symphony/apps/symphony-server/src/{file}"), text);
    }
    /// A watched source replaced whole by a rename, as an editor saves it. Written in place, it
    /// can be hashed truncated; the watcher then builds again for the finished file
    /// (`reload::watch_with`), and a second client build meets the case waiting for a startup.
    fn source(&self, relative: &str, text: &str) {
        let staged = self.dir.join("tmp/fixture-source");
        fs::write(&staged, text).unwrap();
        fs::rename(&staged, self.root.join(relative)).unwrap();
        self.edited.set(Some(Instant::now()));
    }
    fn published(&self) {
        let commands = fs::read_to_string(self.dir.join("commands.log")).unwrap();
        let commands: Vec<Vec<_>> = commands
            .lines()
            .map(|line| line.split('\t').collect())
            .collect();
        assert_eq!(commands.len(), 2, "one verify then land: {commands:?}");
        assert_eq!(commands[0][0], "verify");
        assert_eq!(commands[1][0], "land");
        assert_eq!(
            commands[0][1..],
            commands[1][1..],
            "same checkout, actor and tip"
        );
        assert_ne!(Path::new(commands[0][1]), self.root);
        let main = repository(&self.root, &["show", "main:docs/TOPIC.md"]);
        assert!(main.contains("[x] **one**"), "{main}");
        for path in ["content/one", "view/lamp_atlas"] {
            assert_eq!(
                repository(&self.root, &["show", &format!("main:{path}")]),
                "approved"
            );
        }
        assert_eq!(
            repository(
                &self.root,
                &["status", "--porcelain", "--", "docs", "content", "view"]
            ),
            ""
        );
        assert_eq!(
            repository(&self.root, &["worktree", "list", "--porcelain"])
                .lines()
                .filter(|s| s.starts_with("worktree "))
                .count(),
            1
        );
        assert!(
            self.board
                .tasks(None)
                .unwrap()
                .iter()
                .all(|t| !t.slug.starts_with("tools/review-app-verdict-"))
        );
    }
    fn stop_generation(&mut self, pid: i64, lines: &mut Lines) {
        symphony_server::sys::signal(pid, libc::SIGTERM);
        lines.until("· Symphony server stopped");
        // The stop message precedes runtime teardown and the reload worker's lease release.
        // EOF on both inherited generation pipes observes actual process cleanup.
        lines.closed();
        self.generations.retain(|server| server.pid != pid);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for server in &self.generations {
            server.signal(libc::SIGTERM, || {});
        }
        println!(
            "after_reload wall time {:.3}s; logs {}",
            self.began.elapsed().as_secs_f64(),
            self.dir.display()
        );
    }
}

fn connect(address: SocketAddr) -> TcpStream {
    let stream = TcpStream::connect_timeout(&address, LIMIT).unwrap();
    stream.set_read_timeout(Some(LIMIT)).unwrap();
    stream.set_write_timeout(Some(LIMIT)).unwrap();
    stream
}
fn response(mut stream: TcpStream) -> Value {
    serde_json::from_str(&text_response(&mut stream)).unwrap()
}
fn text_response(stream: &mut TcpStream) -> String {
    let mut text = String::new();
    stream.read_to_string(&mut text).unwrap();
    let (headers, body) = text
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("{text:?}"));
    assert!(headers.starts_with("HTTP/1.1 200 "), "{text}");
    body.into()
}
/// The shell the server's own project serves, under its `/<id>/` on a server with a home.
fn client_page(address: SocketAddr) -> String {
    let front = get(address, "/api/front");
    let page = front["projects"][0]
        .as_str()
        .map_or_else(|| "/".to_string(), |id| format!("/{id}/"));
    text_get(address, &page)
}
fn text_get(address: SocketAddr, route: &str) -> String {
    let mut stream = connect(address);
    write!(
        stream,
        "GET {route} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    text_response(&mut stream)
}
fn get(address: SocketAddr, route: &str) -> Value {
    let mut stream = connect(address);
    write!(
        stream,
        "GET {route} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    response(stream)
}
fn assert_front(address: SocketAddr, pid: i64) {
    let front = get(address, "/api/front");
    assert_eq!(front["pid"], pid);
    assert_eq!(front["port"], address.port());
    assert_eq!(front["ready"], true);
}
fn await_front(address: SocketAddr, pid: i64) {
    let mut last = Value::Null;
    let served = test_time::until_true(format!("replacement {pid} to serve"), LIMIT, || {
        let front = get(address, "/api/front");
        assert_eq!(front["port"], address.port());
        assert_eq!(front["ready"], true);
        let served = front["pid"] == pid;
        last = front;
        served
    });
    if let Err(stall) = served {
        panic!("{stall}; the last answer: {last}");
    }
}

/// The board CLI the generations run: this process's link of the one the workspace's test run built
/// (`SYMPHONY_TEST_TASKS_BINARY`, from a runner that built it first), else of one built here, run
/// once in setup (`first_run::board_cli`).
fn tasks_binary() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| {
        let built = std::env::var_os("SYMPHONY_TEST_TASKS_BINARY")
            .filter(|b| !b.is_empty())
            .map_or_else(
                || {
                    let wrapper =
                        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/rs-bin.sh");
                    let done =
                        sys::output(Command::new(wrapper).args(["--build", "tasks"])).unwrap();
                    assert!(done.status.success(), "tasks binary build: {done:?}");
                    PathBuf::from(String::from_utf8(done.stdout).unwrap().trim())
                },
                PathBuf::from,
            );
        first_run::board_cli(&built)
    })
}
fn post(address: SocketAddr, route: &str, payload: &Value) -> Value {
    let body = payload.to_string();
    let mut stream = accepted(address, route, body.len());
    stream.write_all(body.as_bytes()).unwrap();
    response(stream)
}
fn accepted(address: SocketAddr, route: &str, size: usize) -> TcpStream {
    let mut stream = connect(address);
    write!(stream, "POST {route} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {size}\r\nExpect: 100-continue\r\nConnection: close\r\n\r\n").unwrap();
    let mut reply = [0; 25];
    stream.read_exact(&mut reply).unwrap();
    assert_eq!(
        &reply, b"HTTP/1.1 100 Continue\r\n\r\n",
        "the old generation accepted this body"
    );
    stream
}

/// What a case's waits run is a file setup already ran, never a copy of it made for the case: the
/// installation links this process's link of the built image, the generations run its link of the
/// board CLI, every stand-in the driver's one copy, and an install links that copy too
/// (`first_run`).
#[test]
fn installations_and_stand_ins_link_the_files_setup_ran() {
    use std::os::unix::fs::MetadataExt;
    let f = Fixture::new("first-runs");
    let resolved = |path: &Path| fs::canonicalize(path).unwrap();
    let image = f.home.join("bin/symphony-server");
    assert_eq!(resolved(&image), resolved(first_run::server_image()));
    // Not the build output's own name, which a rebuild moves to a new file
    // (`first_run::tests`), and the stamp names the installation's own file.
    let built = resolved(Path::new(env!("CARGO_BIN_EXE_symphony-server")));
    assert_ne!(resolved(&image), built);
    let stamp: Value =
        serde_json::from_slice(&fs::read(f.home.join("installed.json")).unwrap()).unwrap();
    let (binary, named) = first_run::server_stamp();
    assert_eq!((&stamp["binary"], &stamp["image"]), (&json!(binary), named));
    let file = fs::metadata(&image).unwrap();
    assert_eq!(
        (named["device"].as_u64(), named["inode"].as_u64()),
        (Some(file.dev()), Some(file.ino()))
    );
    assert_eq!(
        tasks_binary().parent(),
        first_run::server_image().parent(),
        "the board CLI is this process's link too"
    );
    if let Some(runners) = std::env::var_os("SYMPHONY_TEST_TASKS_BINARY").filter(|b| !b.is_empty())
    {
        assert_ne!(resolved(tasks_binary()), resolved(Path::new(&runners)));
    }
    let driver = resolved(&f.root.join("tools/rs-bin.sh"));
    // Not the compile output rustc writes in the logs folder, but the machine copy of its bytes:
    // the one file machine_copy hands out for them under the compile's name, already run.
    let logs = f.dir.parent().unwrap();
    assert!(!driver.starts_with(logs), "{}", driver.display());
    let bytes = f.dir.join("reload-driver");
    fs::copy(&driver, &bytes).unwrap();
    assert_eq!(resolved(&first_run::stand_in(&bytes)), driver);
    for name in ["just", "tailscale"] {
        assert_eq!(resolved(&f.dir.join("bin").join(name)), driver, "{name}");
    }
    let mut install = OwnedChild(
        sys::spawn_output(
            Command::new(f.root.join("tools/rs-bin.sh"))
                .args(["slots", "cargo", "install", "--root"])
                .arg(&f.home)
                .env("SYMPHONY_AFTER_RELOAD_GATE", f.gate.path()),
        )
        .unwrap(),
    );
    f.entered("build").release("install");
    install.wait();
    assert_eq!(resolved(&image), driver, "the install links the stand-in");
    assert!(!f.home.join("bin/next").exists());
}

#[test]
fn a_decision_lands_on_main_without_a_manual_commit() {
    let f = Fixture::new("automatic-verdict");
    fs::remove_file(f.root.join("tools/test_budget.json")).unwrap();
    repository(
        &f.root,
        &["commit", "-qam", "fixture without the migration switch"],
    );
    let (mut server, mut lines, address) = f.server(false);
    let before = repository(&f.root, &["rev-parse", "main"]);
    let decided = Instant::now();
    assert_eq!(
        post(
            address,
            "/api/decision",
            &json!({"path":"docs/TOPIC.md","slug":"one","state":"approved","note":"Fine."})
        )["ok"],
        true
    );
    let output = f.committed(decided);
    // The publication worker is blocked in its output check, with its verdict branch visible.
    // Complete a real read-hub scan before releasing it, then require that publication still lands.
    let scanned = symphony::branches::scan(&f.root, &Default::default());
    assert!(
        scanned.iter().any(|card| {
            card.tree.branch.starts_with("docs/verdict-")
                && card.doc == "docs/TOPIC.md"
                && card.card.slug == "one"
                && card.card.state.as_str() == "approved"
        }),
        "the held publication must actually be observed by the scan"
    );
    assert_eq!(
        repository(&f.root, &["rev-parse", "main"]),
        before,
        "verdict and outputs publish together"
    );
    assert_front(address, i64::from(server.0.id()));
    symphony_server::sys::signal(i64::from(server.0.id()), libc::SIGTERM);
    output.release("publish");
    lines.until("· Symphony server stopped");
    server.wait();
    f.published();
    assert!(
        repository(&f.root, &["show", "main:docs/TOPIC.md"]).contains("owner · approved — Fine.")
    );
    assert!(!f.root.join("artifacts/docs-app.port").exists());
}

#[test]
fn live_branch_discovery_does_not_refresh_a_writers_index() {
    let f = Fixture::new("read-only-branch-scan");
    let branch = f.dir.join("branch");
    repository(
        &f.root,
        &[
            "worktree",
            "add",
            "-qb",
            "work/scan",
            branch.to_str().unwrap(),
        ],
    );
    let index = PathBuf::from(repository(
        &branch,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    ));
    assert_eq!(symphony::branches::trees(&f.root)[0].branch, "work/scan");
    write(
        &branch.join("docs/CARD.md"),
        "# Branch card\n\n<!-- register:start -->\n- [ ] **scan-card** — A branch-only question.\n<!-- register:end -->\n",
    );
    for unmerged in [false, true] {
        if unmerged {
            // posting the card makes the branch its own, unmerged
            repository(&branch, &["add", "docs/CARD.md"]);
            repository(&branch, &["commit", "-qm", "Branch-only work"]);
        }
        let before = fs::read(&index).unwrap();
        let document = branch.join("docs/TOPIC.md");
        let old = fs::metadata(&document).unwrap().modified().unwrap();
        // Content is unchanged, but its cached stat data is stale. An ordinary status/diff refresh
        // would take index.lock and rewrite it, racing the verdict worker's mandatory index writes.
        let modified = std::time::UNIX_EPOCH + Duration::from_secs(1_000_000 + u64::from(unmerged));
        assert_ne!(old, modified);
        File::open(&document)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
        assert_eq!(
            fs::metadata(&document).unwrap().modified().unwrap(),
            modified
        );
        let cards = symphony::branches::scan(&f.root, &Default::default());
        if unmerged {
            assert_eq!(cards.len(), 1, "discovery must actually read the branch");
            assert_eq!(cards[0].card.slug, "scan-card");
        } else {
            // a merged branch's unsaved draft reaches no one, though its worktree is read for the
            // owner's lines
            assert!(cards.is_empty(), "{:?}", cards.len());
        }
        assert_eq!(
            fs::read(&index).unwrap(),
            before,
            "read-hub discovery must leave the writer's index alone: unmerged={unmerged}"
        );
    }
}

#[test]
fn requests_accepted_before_and_during_handover_all_receive_a_status() {
    let mut f = Fixture::new("handover-responses");
    let (mut original, mut lines, address) = f.server(true);
    let original_pid = i64::from(original.0.id());
    // A POST split after P remains in Hyper's protocol detector, before HTTP/1 takes over.
    // Sending a GET prefix would leave that window. The later 100 Continue rendezvous proves
    // the old generation's accept loop has progressed past these sockets and the idle client.
    let mut incomplete: Vec<_> = (0..8)
        .map(|_| {
            let mut stream = connect(address);
            stream.write_all(b"P").unwrap();
            stream
        })
        .collect();
    let mut idle = connect(address);
    let body = json!({"path":"docs/TOPIC.md","slug":"one","text":"Drain every accepted request."})
        .to_string();
    let mut held = accepted(address, "/api/message", body.len());
    f.edit("live.rs", "// handover with unanswered connections\n");
    f.entered("build").release("install");
    let startup = f.startup();
    let replacement_pid = startup.pid;
    assert_front(address, original_pid);
    startup.release("exec");
    lines.until(&format!(
        "replacement pid {replacement_pid} ready; draining pid {original_pid}"
    ));
    // Fresh requests race the accept loop's shutdown too. Each is a separate request, and an
    // empty response or transport error fails immediately; none is retried.
    std::thread::scope(|scope| {
        let fresh: Vec<_> = (0..8)
            .map(|_| scope.spawn(|| {
                let mut stream = connect(address);
                write!(stream, "GET /api/front HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n").unwrap();
                let mut response = String::new();
                stream.read_to_string(&mut response).unwrap();
                let (headers, body) = response.split_once("\r\n\r\n").unwrap_or_else(|| panic!("{response:?}"));
                assert!(headers.starts_with("HTTP/1.1 "), "{response:?}");
                match headers.split_whitespace().nth(1) {
                    Some("200") => {
                        let front: Value = serde_json::from_str(body).unwrap();
                        assert!(front["pid"] == original_pid || front["pid"] == replacement_pid, "{front}");
                    }
                    Some("503") => assert!(body.is_empty(), "{response}"),
                    _ => panic!("a handover request needs a served response or the drain's 503: {response:?}"),
                }
            }))
            .collect();
        for request in fresh {
            request.join().unwrap();
        }
    });
    assert!(
        original.0.try_wait().unwrap().is_none(),
        "the accepted body holds the drain"
    );
    held.write_all(body.as_bytes()).unwrap();
    assert_eq!(response(held)["ok"], true);
    lines.until("· Symphony server stopped");
    assert!(original.0.wait().unwrap().success());
    // No prefix client sent another byte, and the idle client neither wrote nor disconnected.
    // The process exit above must still happen, and every socket must retain a real response.
    for stream in incomplete.iter_mut().chain(std::iter::once(&mut idle)) {
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert_eq!(
            response,
            "HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
        );
    }
    assert_front(address, replacement_pid);
    f.stop_generation(replacement_pid, &mut lines);
}

#[test]
fn failed_build_and_startup_keep_serving_then_module_and_main_handover_preserve_requests() {
    let mut f = Fixture::new("generations");
    let (mut original, mut lines, address) = f.server(true);
    let original_pid = i64::from(original.0.id());
    f.edit("live.rs", "// build failure\n");
    let build = f.entered("build");
    assert_front(address, original_pid);
    build.release("fail");
    lines.until("fixture compiler refused source");
    assert_front(address, original_pid);
    let board = get(address, "/api/board");
    let mut feed = connect(address);
    write!(
        feed,
        "GET /api/feed?since={} HTTP/1.1\r\nHost: {address}\r\n\r\n",
        board["version"]
    )
    .unwrap();
    let mut feed = BufReader::new(feed);
    loop {
        let mut line = String::new();
        assert!(feed.read_line(&mut line).unwrap() > 0);
        if line.contains("previous server is still serving") {
            break;
        }
    }
    drop(feed);

    f.edit(
        "live.rs",
        "// starts a replacement that exits before readiness\n",
    );
    f.entered("build").release("install");
    let failed = f.startup();
    let failed_pid = failed.pid;
    assert_front(address, original_pid);
    failed.release("exit");
    lines.until("replacement did not become ready");
    assert!(
        !symphony_server::sys::pid_alive(failed_pid),
        "failed replacement was reaped"
    );
    f.generations.retain(|server| server.pid != failed_pid);
    assert_front(address, original_pid);
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(f.home.join("server.json")).unwrap()).unwrap()["pid"],
        original_pid
    );

    let body =
        json!({"path":"docs/TOPIC.md","slug":"one","state":"approved","note":"Across handover."})
            .to_string();
    let mut held = accepted(address, "/api/decision", body.len());
    f.edit("live.rs", "// module replacement\n");
    let build = f.entered("build");
    assert_front(address, original_pid);
    build.release("install");
    let startup = f.startup();
    let second_pid = startup.pid;
    assert_front(address, original_pid);
    startup.release("exec");
    lines.until(&format!(
        "replacement pid {second_pid} ready; draining pid {original_pid}"
    ));
    await_front(address, second_pid);
    assert!(
        original.0.try_wait().unwrap().is_none(),
        "accepted save keeps the old generation draining"
    );
    let decided = Instant::now();
    held.write_all(body.as_bytes()).unwrap();
    assert_eq!(
        response(held)["ok"],
        true,
        "accepted request survived handover"
    );
    let output = f.committed(decided);
    assert_eq!(
        repository(&f.root, &["show", "main:docs/TOPIC.md"]),
        DOC.trim_end()
    );
    assert_front(address, second_pid);
    assert!(
        original.0.try_wait().unwrap().is_none(),
        "old generation waits for verdict outputs"
    );
    // The reload inputs represent committed app work; ordinary land correctly refuses
    // an unrelated tracked-dirty main checkout. Commit only those fixture app inputs.
    repository(&f.root, &["add", "--", "symphony/apps"]);
    repository(
        &f.root,
        &[
            "commit",
            "-qm",
            "fixture app source changes before publication",
        ],
    );
    let released = Instant::now();
    output.release("publish");
    lines.until("· Symphony server stopped");
    // The old generation's stop, each stage it named after the seconds since the release.
    eprintln!(
        "after_reload: the drained generation's stop after its publication:\n{}",
        lines.since(released)
    );
    original.wait();
    f.published();
    assert_eq!(
        fs::read_to_string(f.root.join("artifacts/docs-app.port"))
            .unwrap()
            .trim(),
        address.port().to_string(),
        "draining generation must leave the replacement's discovery record"
    );

    // Native main-entry changes follow the same generation contract, including a request already
    // accepted by the outgoing generation.
    let body =
        json!({"path":"docs/TOPIC.md","slug":"one","text":"Main entry preserved this request."})
            .to_string();
    let mut held = accepted(address, "/api/message", body.len());
    f.edit("main.rs", "fn main() { /* main entry changed */ }\n");
    f.entered("build").release("install");
    let startup = f.startup();
    let third_pid = startup.pid;
    assert_front(address, second_pid);
    startup.release("exec");
    lines.until(&format!(
        "replacement pid {third_pid} ready; draining pid {second_pid}"
    ));
    await_front(address, third_pid);
    held.write_all(body.as_bytes()).unwrap();
    assert_eq!(response(held)["ok"], true);
    // The outputs already match this save. Draining must still finish its ordinary verdict commit.
    lines.until("· Symphony server stopped");
    f.generations.retain(|server| server.pid != second_pid);
    assert!(
        repository(&f.root, &["show", "main:docs/TOPIC.md"])
            .contains("Main entry preserved this request.")
    );
    f.stop_generation(third_pid, &mut lines);
    assert!(!f.root.join("artifacts/docs-app.port").exists());
    File::open(f.home.join("server.lock"))
        .unwrap()
        .try_lock()
        .unwrap();
}

#[test]
fn client_only_and_shared_edits_publish_once_and_failures_preserve_the_serving_bundle() {
    let mut f = Fixture::new("client-generations");
    let (mut original, mut lines, address) = f.server(true);
    let original_pid = i64::from(original.0.id());
    let old_stamp = fs::read(f.home.join("client-installed.json")).unwrap();
    let old_binary = Sha256::digest(fs::read(f.home.join("bin/symphony-server")).unwrap());
    assert_eq!(client_page(address), "initial client");

    f.source("symphony/apps/symphony-web/src/main.tsx", "refused client");
    let client = f.entered("client");
    assert_front(address, original_pid);
    assert_eq!(
        client_page(address),
        "initial client",
        "staged output must stay private"
    );
    client.release("fail");
    lines.until("fixture client compiler refused source");
    assert_front(address, original_pid);
    assert_eq!(client_page(address), "initial client");
    assert_eq!(
        fs::read(f.home.join("client-installed.json")).unwrap(),
        old_stamp
    );
    assert_eq!(
        Sha256::digest(fs::read(f.home.join("bin/symphony-server")).unwrap()),
        old_binary
    );
    assert!(
        !f.home
            .join(format!(".reload-client.{original_pid}"))
            .exists()
    );
    assert_eq!(
        lines
            .history
            .iter()
            .filter(|line| line.contains("fixture client compiler refused source"))
            .count(),
        1
    );

    f.source(
        "symphony/apps/symphony-web/src/main.tsx",
        "incomplete bundle",
    );
    f.entered("client").release("empty");
    lines.until("client build has no index.html");
    assert_front(address, original_pid);
    assert_eq!(client_page(address), "initial client");
    assert_eq!(
        fs::read(f.home.join("client-installed.json")).unwrap(),
        old_stamp
    );

    // A successful bundle followed by a failed replacement must also restore the client.
    f.source("symphony/apps/symphony-web/src/main.tsx", "unready client");
    f.entered("client").release("bundle");
    let build = f.entered("build");
    assert_eq!(client_page(address), "initial client");
    build.release("install");
    let startup = f.startup();
    assert_eq!(
        client_page(address),
        "unready client",
        "swap precedes replacement readiness"
    );
    let failed_pid = startup.pid;
    startup.release("exit");
    lines.until("replacement did not become ready");
    f.generations.retain(|server| server.pid != failed_pid);
    assert_front(address, original_pid);
    assert_eq!(client_page(address), "initial client");
    assert_eq!(
        fs::read(f.home.join("client-installed.json")).unwrap(),
        old_stamp
    );

    f.source("symphony/apps/symphony-web/src/main.tsx", "shipped client");
    f.entered("client").release("bundle");
    f.entered("build").release("install");
    let startup = f.startup();
    let second_pid = startup.pid;
    assert_eq!(client_page(address), "shipped client");
    startup.release("exec");
    lines.until(&format!(
        "replacement pid {second_pid} ready; draining pid {original_pid}"
    ));
    await_front(address, second_pid);
    assert!(original.0.wait().unwrap().success());
    assert_eq!(client_page(address), "shipped client");
    assert_eq!(
        text_get(address, "/assets/old-456456456456456a.js"),
        "old module",
        "old tabs retain their hashed modules"
    );
    assert_eq!(
        text_get(address, "/assets/new-0123456789abcdef.js"),
        "new module"
    );
    assert_eq!(
        text_get(address, "/assets/chunks/old-789789789789789a.js"),
        "old chunk"
    );
    let second_source = symphony::reload::client_fingerprint(&f.root).unwrap();
    assert_eq!(
        symphony::reload::installed_client_source(&f.home).as_deref(),
        Some(second_source.as_str())
    );
    assert_ne!(
        fs::read(f.home.join("client-installed.json")).unwrap(),
        old_stamp
    );

    // The lockfile is an input to both products (the client's publisher is the workspace's):
    // exactly one client build, one server build and one handover must cover that event, without
    // a second reload from the new generation. A crate is the server's alone now, the wire types'
    // too: the client reads them as the TypeScript its package commits.
    f.source("Cargo.lock", "# changed shared lockfile\n");
    f.entered("client").release("bundle");
    f.entered("build").release("install");
    let startup = f.startup();
    let third_pid = startup.pid;
    startup.release("exec");
    lines.until(&format!(
        "replacement pid {third_pid} ready; draining pid {second_pid}"
    ));
    await_front(address, third_pid);
    let third_source = symphony::reload::client_fingerprint(&f.root).unwrap();
    assert_ne!(second_source, third_source);
    assert_eq!(
        symphony::reload::installed_client_source(&f.home).as_deref(),
        Some(third_source.as_str())
    );
    assert_eq!(client_page(address), "shipped client");
    f.stop_generation(third_pid, &mut lines);
    f.generations.clear();
    write(
        &f.home.join("client/public/index.html"),
        "manually replaced bundle",
    );
    assert!(
        symphony::reload::installed_client_source(&f.home).is_none(),
        "an installed stamp must identify actual output bytes"
    );
    write(&f.home.join("client-installed.json"), "invalid stamp");
    assert!(symphony::reload::installed_client_source(&f.home).is_none());
}

#[test]
fn startup_rebuilds_an_older_client_even_when_the_server_stamp_is_current() {
    let mut f = Fixture::new("startup-client-stamp");
    let old_source = symphony::reload::installed_client_source(&f.home).unwrap();
    f.source("symphony/apps/symphony-web/src/main.tsx", "current client");
    // Model a partial installation: the binary/source stamp is current, while the client
    // stamp still correctly identifies the old bundle. Only its source identity is stale.
    let installed_path = f.home.join("installed.json");
    let mut installed: Value = serde_json::from_slice(&fs::read(&installed_path).unwrap()).unwrap();
    installed["source"] = symphony::reload::fingerprint(&f.root).unwrap().into();
    write(&installed_path, &installed.to_string());
    let current_source = symphony::reload::client_fingerprint(&f.root).unwrap();
    assert_ne!(old_source, current_source);
    assert_eq!(
        symphony::reload::installed_client_source(&f.home),
        Some(old_source)
    );

    let (mut original, mut lines, address) = f.server(true);
    let original_pid = i64::from(original.0.id());
    let client = f.entered("client");
    assert_eq!(client_page(address), "initial client");
    assert_front(address, original_pid);
    client.release("bundle");
    f.entered("build").release("install");
    let startup = f.startup();
    let replacement_pid = startup.pid;
    assert_eq!(client_page(address), "current client");
    startup.release("exec");
    lines.until(&format!(
        "replacement pid {replacement_pid} ready; draining pid {original_pid}"
    ));
    assert!(original.0.wait().unwrap().success());
    await_front(address, replacement_pid);
    assert_eq!(
        symphony::reload::installed_client_source(&f.home),
        Some(current_source)
    );
    f.stop_generation(replacement_pid, &mut lines);
}

#[test]
fn shutdown_cancels_build_or_startup_and_releases_the_lease() {
    for phase in ["client", "build", "startup"] {
        let mut f = Fixture::new(&format!("cancel-{phase}"));
        let (mut server, mut lines, address) = f.server(true);
        let pid = i64::from(server.0.id());
        let build = if phase == "client" {
            f.source(
                "symphony/apps/symphony-web/src/main.tsx",
                "cancelled client",
            );
            f.entered("client")
        } else {
            f.edit("main.rs", "fn main() { /* cancelled replacement */ }\n");
            f.entered("build")
        };
        let held = if phase == "startup" {
            build.release("install");
            f.startup()
        } else {
            build
        };
        assert_front(address, pid);
        symphony_server::sys::signal(pid, libc::SIGTERM);
        lines.until("· Symphony server stopped");
        server.wait();
        assert!(
            !symphony_server::sys::pid_alive(held.pid),
            "cancelled {phase} was reaped"
        );
        f.generations.clear();
        assert!(!f.root.join("artifacts/docs-app.port").exists());
        for name in ["server.lock", "reload-build.lock"] {
            File::open(f.home.join(name)).unwrap().try_lock().unwrap();
        }
        assert!(!f.home.join(format!(".reload-client.{pid}")).exists());
        assert!(
            TcpStream::connect(address).is_err(),
            "no listener survived cancelled {phase}"
        );
        drop(held);
    }
}
