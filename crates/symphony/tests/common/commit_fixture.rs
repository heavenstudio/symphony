
#[path = "../../../tasks/tests/store/common.rs"]
#[allow(dead_code)]
mod board;
#[path = "mod.rs"]
mod common;
#[path = "../../../../../tools/gate/tests/verify/script_board.rs"]
mod fixture_script;

#[allow(unused_imports)]
use std::io::{BufRead, BufReader, Read, Write};
#[allow(unused_imports)]
use std::net::{TcpListener, TcpStream};
#[allow(unused_imports)]
use std::os::unix::fs::PermissionsExt;
#[allow(unused_imports)]
use std::path::{Path, PathBuf};
#[allow(unused_imports)]
use std::process::{Child, Command, Stdio};
#[allow(unused_imports)]
use symphony_board::sys::Spawning as _;
#[allow(unused_imports)]
use std::sync::{Arc, Barrier, Mutex};
#[allow(unused_imports)]
use std::time::{Duration, Instant};

#[allow(unused_imports)]
use symphony::commit::{Committer, Edit};
#[allow(unused_imports)]
use symphony::save::Saves;
#[allow(unused_imports)]
use serde_json::{Value, json};

const DOC: &str = "# A\n\nfirst paragraph\n\n<!-- register:start -->\n- [ ] **one** — proposal.\n<!-- register:end -->\n\nlast paragraph\n";
const TWO: &str =
    "# B\n\n<!-- register:start -->\n- [ ] **two** — second proposal.\n<!-- register:end -->\n";
/// How the fixture's project commits verdicts: a project's commands, which its justfile stands
/// in for.
const VERDICTS: &str = r#"[verdicts]
check = ["just", "verify"]
land = ["just", "land"]
generators = [
  { name = "content", outputs = "content/", command = ["just", "content"] },
  { name = "content-ids", outputs = "sim/content_ids.gd", command = ["just", "content-ids"], when_changed = true },
  { name = "lamp-atlas", outputs = "view/lamp_atlas", command = ["just", "lamp-atlas", "build"], when_changed = true },
]
record = ".git/sf-land/verdicts"
"#;
const CHILD: &str = "SF_AFTER_COMMIT_ROOT";
const DEADLINE: Duration = Duration::from_secs(45);

#[allow(dead_code)]
fn stage(name: impl std::fmt::Display) {
    static BEGAN: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    // Write directly: libtest otherwise withholds this until the case finishes,
    // which can be later than the target's outer timeout.
    let line = format!(
        "after_commit case={} pid={} +{:.3}s: {name}\n",
        std::thread::current().name().unwrap_or("worker"),
        std::process::id(),
        BEGAN.get_or_init(Instant::now).elapsed().as_secs_f64()
    );
    let _ = std::io::stderr().write_all(line.as_bytes());
}

#[allow(dead_code)]
fn child_output(home: &Path) -> String {
    ["child.log", "filing.log"]
        .into_iter()
        .filter_map(|name| {
            let path = home.join(name);
            path.exists().then(|| {
                format!(
                    "--- {name} ---\n{}",
                    std::fs::read(&path)
                        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                        .unwrap_or_else(|error| format!("{}: {error}", path.display()))
                )
            })
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[allow(dead_code)]
fn watchdog_report(name: &str, home: &Path) -> String {
    format!("{name} watchdog: {}", child_output(home))
}

#[allow(dead_code)]
fn commit_verdicts(root: &Path, edits: &[Edit], message: &str) -> symphony::commit::Outcome {
    stage(format_args!("publication start: {message}"));
    let outcome = symphony::commit::commit_verdicts(root, edits, message);
    stage(format_args!(
        "publication finished: {message}, committed={}, error={}",
        outcome.committed, outcome.error
    ));
    outcome
}

#[allow(dead_code)]
fn git(root: &Path, args: &[&str]) -> String {
    stage(format_args!("git start {args:?}"));
    let done = symphony_board::sys::output(symphony_board::sys::Command::new("git")
        .args(args)
        .current_dir(root)
        )
        .unwrap();
    stage(format_args!("git finished {args:?}: {}", done.status));
    assert!(
        done.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&done.stderr)
    );
    String::from_utf8(done.stdout)
        .unwrap()
        .trim_end_matches('\n')
        .into()
}

#[allow(dead_code)]
fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[allow(dead_code)]
fn script(path: &Path, text: &str) {
    write(path, &format!("#!/bin/sh\nset -eu\n{text}\n"));
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[allow(dead_code)]
fn prepared_script(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    fixture_script::write(path, &format!("#!/bin/sh\nset -eu\n{text}\n"), false);
}

#[allow(dead_code)]
fn quote(path: &Path) -> String {
    format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"))
}

#[allow(dead_code)]
fn relay_output(
    mut reader: impl Read,
    mut saved: impl Write,
    mut live: impl Write,
) -> std::io::Result<()> {
    let mut buffer = [0; 8192];
    loop {
        let count = match reader.read(&mut buffer) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            return Ok(());
        }
        saved.write_all(&buffer[..count])?;
        live.write_all(&buffer[..count])?;
    }
}

// Reporting only: the existing watchdog and target budgets remain unchanged.
const PUBLICATION_REPORT_AFTER: Duration = Duration::from_secs(5);

#[allow(dead_code)]
struct PublicationProgress {
    line: String,
    began: Instant,
    reported: bool,
}

#[allow(dead_code)]
#[derive(Default)]
struct PendingPublications(
    std::collections::BTreeMap<String, (Instant, String, bool)>,
    std::collections::BTreeMap<String, PublicationProgress>,
);

#[allow(dead_code)]
impl PendingPublications {
    fn thread_key(line: &str) -> String {
        let header = line.split_once(" stage=").map_or(line, |(header, _)| header);
        let (process, rest) = header.split_once(" id=").unwrap_or((header, ""));
        let thread = rest.split_once(" thread=").map_or("", |(_, thread)| thread);
        format!("{process} thread={thread}")
    }

    fn observe(&mut self, line: &str, now: Instant) -> bool {
        if !line.starts_with("publication pid=") {
            return false;
        }
        let key = if let Some((key, _)) = line.rsplit_once(" started elapsed=") {
            self.0.insert(key.into(), (now, line.into(), false));
            key
        } else if let Some((key, _)) = line.rsplit_once(" finished elapsed=") {
            self.0.remove(key);
            key
        } else {
            return true;
        };
        self.1.insert(Self::thread_key(key), PublicationProgress {
            line: line.into(),
            began: now,
            reported: false,
        });
        true
    }

    fn report(began: Instant, line: &str, last: &str, now: Instant) -> String {
        format!(
            "after_commit publication still open after {:.3}s: {} | last trace: {}\n",
            now.duration_since(began).as_secs_f64(),
            line.trim_end(),
            last.trim_end()
        )
    }

    fn overdue(&mut self, now: Instant) -> String {
        let mut output = String::new();
        let mut just_reported = std::collections::BTreeSet::new();
        for (began, line, reported) in self.0.values_mut() {
            if !*reported && now.duration_since(*began) >= PUBLICATION_REPORT_AFTER {
                *reported = true;
                let thread = Self::thread_key(line);
                output.push_str(&Self::report(*began, line, &self.1[&thread].line, now));
                just_reported.insert(thread);
            }
        }
        // A new trace line starts a new quiet period, even when an enclosing
        // publication has already been reported. Preserve the last finished
        // stage so a gap between stages is located before the target timeout.
        for (thread, progress) in &mut self.1 {
            if !progress.reported && now.duration_since(progress.began) >= PUBLICATION_REPORT_AFTER {
                progress.reported = true;
                if !just_reported.contains(thread) {
                    for (began, line, _) in self.0.values() {
                        if Self::thread_key(line) == *thread {
                            output.push_str(&Self::report(*began, line, &progress.line, now));
                        }
                    }
                }
            }
        }
        output
    }
}

#[allow(dead_code)]
fn relay_publication_output(
    reader: impl Read,
    mut saved: impl Write,
    mut live: impl Write,
    pending: &Mutex<PendingPublications>,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            return Ok(());
        }
        saved.write_all(&line)?;
        let trace = pending.lock().unwrap().observe(&String::from_utf8_lossy(&line), Instant::now());
        if !trace {
            live.write_all(&line)?;
        }
    }
}

#[allow(dead_code)]
fn quiet_publication_output(output: &str) -> String {
    output.split_inclusive('\n').filter(|line| !line.starts_with("publication pid=")).collect()
}

#[allow(dead_code)]
struct OwnedChild(symphony_board::sys::Child, Vec<std::thread::JoinHandle<std::io::Result<()>>>, Arc<Mutex<PendingPublications>>);
impl OwnedChild {
    fn logged(command: &mut symphony_board::sys::Command, path: &Path) -> Self {
        let log = std::fs::File::create(path).unwrap();
        let saved = log.try_clone().unwrap();
        // The relays end with the case's output, never later because another case's child kept it.
        let mut child = symphony_board::sys::spawn_output(command).unwrap();
        let error = child.stderr.take().unwrap();
        let output = child.stdout.take().unwrap();
        let pending = Arc::new(Mutex::new(PendingPublications::default()));
        let out_pending = Arc::clone(&pending);
        let err_pending = Arc::clone(&pending);
        Self(
            child,
            vec![
                std::thread::spawn(move || relay_publication_output(output, saved, std::io::stderr(), &out_pending)),
                std::thread::spawn(move || relay_publication_output(error, log, std::io::stderr(), &err_pending)),
            ],
            pending,
        )
    }

    fn finish_output(&mut self) {
        for relay in self.1.drain(..) {
            relay.join().unwrap().unwrap();
        }
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Environment overrides belong to an owned child, never the concurrently running test binary.
#[allow(dead_code)]
fn isolated(name: &str, run: impl FnOnce(&Fixture)) {
    if let Some(output) = isolated_output(name, run) {
        eprint!("{}", quiet_publication_output(&output));
    }
}

#[allow(dead_code)]
fn isolated_output(name: &str, run: impl FnOnce(&Fixture)) -> Option<String> {
    if let Ok(root) = std::env::var(CHILD) {
        stage(format_args!("case entered: {name}"));
        run(&Fixture { home: root.into() });
        stage(format_args!("case complete: {name}"));
        return None;
    }
    let case_began = Instant::now();
    stage(format_args!("case setup started: {name}"));
    let tree = common::Tree::new();
    stage(format_args!("private board setup started: {name}"));
    let db = board::client();
    stage(format_args!("private board setup completed: {name}"));
    stage(format_args!("tasks CLI resolution started: {name}"));
    let tasks = common::tasks_binary();
    stage(format_args!("tasks CLI resolution completed: {name}"));
    let output = tree.path("child.log");
    let path = std::env::var_os("PATH").unwrap();
    let just = std::env::split_paths(&path)
        .map(|path| path.join("just"))
        .find(|path| path.is_file())
        .expect("just is installed");
    let mut child = symphony_board::sys::Command::new(std::env::current_exe().unwrap());
    // Drop inherited board, callback, index and repository overrides. This process may only
    // reach this fixture; its CLI children receive the same private environment.
    for (key, _) in std::env::vars() {
        if key.starts_with("TASKS_") || key.starts_with("GIT_") || key.starts_with("SF_") {
            child.env_remove(key);
        }
    }
    child
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env(CHILD, &tree.root)
        .env("SF_SYMPHONY_PUBLICATION_TRACE", "1")
        .env("TASKS_SERVER", db.addr())
        .env("TASKS_DATABASE", db.database())
        .env("ASKS_DB", tree.path("asks.db"))
        .env("SF_TASKS_CLI", tasks)
        .env("SF_AFTER_COMMIT_TASKS", tasks)
        .env("SF_AFTER_COMMIT_JUST", just)
        .env(
            "PATH",
            std::env::join_paths(
                std::iter::once(tree.path("bin")).chain(std::env::split_paths(&path)),
            )
            .unwrap(),
        )
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .current_dir(&tree.root)
        .stdin(symphony_board::sys::Stdio::Null);
    let mut child = OwnedChild::logged(&mut child, &output);
    stage(format_args!("case child spawned: {name}"));
    let began = Instant::now();
    let status = loop {
        // Full detail stays in child.log; an open stage reaches the live stream
        // before the target timeout, without flooding successful suite output.
        let pending = child.2.lock().unwrap().overdue(Instant::now());
        let _ = std::io::stderr().write_all(pending.as_bytes());
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            began.elapsed() < DEADLINE,
            "{}",
            watchdog_report(name, &tree.root)
        );
        // Only the outer watchdog polls process completion. All ordering assertions below use
        // sockets or worker joins, with no elapsed-time or sleep-based success criterion.
        std::thread::park_timeout(Duration::from_millis(10));
    };
    child.finish_output();
    stage(format_args!(
        "case child completed: {name}, {status}, wall={:.3}s",
        case_began.elapsed().as_secs_f64()
    ));
    let said = String::from_utf8_lossy(&std::fs::read(&output).unwrap()).into_owned();
    assert!(status.success(), "{name}: {}", child_output(&tree.root));
    assert!(
        said.contains("1 passed"),
        "the exact child case must run: {said}"
    );
    Some(said)
}

#[allow(dead_code)]
struct Fixture {
    home: PathBuf,
}
// The save and publication targets use different subsets of this shared harness.
#[allow(dead_code)]
impl Fixture {
    fn root(&self) -> PathBuf {
        self.home.join("repo")
    }
    fn repo(&self) -> PathBuf {
        let root = self.root();
        std::fs::create_dir(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.name", "Review test"]);
        git(
            &root,
            &["config", "user.email", "review-test@example.invalid"],
        );
        write(&root.join("docs/a.md"), DOC);
        write(&root.join("docs/b.md"), TWO);
        write(&root.join("other"), "original\n");
        write(&root.join(".gitignore"), "artifacts/\n");
        write(&root.join(symphony::project::FILE), VERDICTS);
        write(
            &root.join("tools/test_budget.json"),
            r#"{"before_verify_after_port":true}"#,
        );
        write(
            &root.join("justfile"),
            &format!(
                "content:\n    @sh {}\n\ncontent-ids:\n    @true\n\nlamp-atlas action:\n    @mkdir -p view\n    @cp content/one view/lamp_atlas\n\ncheck:\n    @sh {}\n\nverify:\n    @sh {}\n\nland branch:\n    @sh {} {{{{branch}}}}\n",
                quote(&self.home.join("generate.sh")),
                quote(&self.home.join("check.sh")),
                quote(&self.home.join("check.sh")),
                quote(&self.home.join("publish.sh"))
            ),
        );
        script(&self.home.join("generate.sh"), "exit 0");
        script(&self.home.join("check.sh"), "exit 0");
        script(
            &self.home.join("publish.sh"),
            &format!(
                "if [ -n \"$(git -C {} status --porcelain --untracked-files=no)\" ]; then\n echo 'main checkout {} {}' >&2\n exit 1\nfi\ngit rebase main\nSF_AFTER_COMMIT_SWAP_ROOT={} SF_AFTER_COMMIT_SWAP_BRANCH=\"$1\" exec {} --exact publication_swap --nocapture",
                quote(&root),
                root.display(),
                symphony::landing::DIRTY_MAIN,
                quote(&root),
                quote(&std::env::current_exe().unwrap())
            ),
        );
        prepared_script(
            &self.home.join("bin/just"),
            &format!(
                "case \"$1\" in verify|land)\n printf '%s\\t%s\\t%s\\t%s\\t%s\\n' \"$1\" \"$*\" \"$PWD\" \"${{TASKS_SESSION:?}}\" \"$(git rev-parse HEAD)\" >> {};; esac\nexec \"$SF_AFTER_COMMIT_JUST\" \"$@\"",
                quote(&self.home.join("commands.log"))
            ),
        );
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "fixture"]);
        root
    }
    fn edit(&self, rel: &str, base: &str, new: &str) -> Edit {
        write(&self.root().join(rel), new);
        Edit {
            rel: rel.into(),
            base: base.into(),
            new: new.into(),
        }
    }
    fn main(&self, rel: &str) -> String {
        git(&self.root(), &["show", &format!("main:{rel}")]) + "\n"
    }
    fn store(&self) -> symphony_board::Store {
        stage("connecting to private board");
        let store = symphony::taskview::connect(&self.root()).unwrap();
        stage("connected to private board");
        store
    }
    fn generate(&self) {
        script(
            &self.home.join("generate.sh"),
            "mkdir -p content\nif grep -qF -- '- [x] **one**' docs/a.md; then printf 'approved\\n' > content/one; fi",
        );
    }
    fn check_gate(&self, gate: &Gate) {
        script(
            &self.home.join("check.sh"),
            &format!(
                "if mkdir {} 2>/dev/null; then\n  SF_AFTER_COMMIT_GATE={} {} --exact publication_gate --nocapture\nfi",
                quote(&self.home.join("checked-once")),
                gate.address(),
                quote(&std::env::current_exe().unwrap())
            ),
        );
    }
    fn main_notice(&self, notice: &Gate) {
        prepared_script(
            &self.root().join(".git/hooks/reference-transaction"),
            &format!(
                "[ \"$1\" = committed ] || exit 0\nSF_AFTER_COMMIT_NOTICE={} exec {} --exact main_notice --nocapture",
                notice.address(),
                quote(&std::env::current_exe().unwrap())
            ),
        );
    }
    /// Open the queued job's existing lock while a socket holds its worker. Acquiring this
    /// descriptor after releasing the socket observes job completion, including failure filing.
    fn pending_job(&self) -> std::fs::File {
        let paths: Vec<_> = std::fs::read_dir(self.root().join(".git/sf-symphony/verdict-backlog"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .parse::<u64>()
                    .is_ok()
            })
            .collect();
        assert_eq!(paths.len(), 1, "exactly one queued job: {paths:?}");
        let held = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&paths[0])
            .unwrap();
        assert!(matches!(
            held.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        held
    }
    fn pending_job_for(&self, rel: &str) -> std::fs::File {
        let root = self.root();
        let path = std::fs::read_dir(root.join(".git/sf-symphony/verdict-publications"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.extension().is_some_and(|extension| extension == "json")
                    && serde_json::from_slice::<Value>(&std::fs::read(path).unwrap()).unwrap()["edit"]["rel"] == rel
            })
            .expect("the socket-held save has a durable journal");
        let held = std::fs::OpenOptions::new().read(true).write(true)
            .open(root.join(".git/sf-symphony/verdict-backlog").join(path.file_stem().unwrap())).unwrap();
        assert!(matches!(held.try_lock(), Err(std::fs::TryLockError::WouldBlock)));
        held
    }
    fn clean_publication_state(&self) {
        let root = self.root();
        let listing = git(&root, &["worktree", "list", "--porcelain"]);
        assert_eq!(
            listing
                .lines()
                .filter(|l| l.starts_with("worktree "))
                .count(),
            1,
            "{listing}"
        );
        let backlog = root.join(".git/sf-symphony/verdict-backlog");
        if backlog.exists() {
            let mut names: Vec<_> = std::fs::read_dir(backlog)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            names.sort();
            assert_eq!(names, ["next"]);
        }
    }
    fn clean_workers(&self) {
        self.clean_publication_state();
        let commands = self.commands();
        for pair in commands.chunks(2) {
            assert_eq!(pair.len(), 2, "verify followed by land: {commands:?}");
            assert_eq!(pair[0][0], "verify");
            assert_eq!(pair[1][0], "land");
            assert_eq!(pair[0][2], pair[1][2], "same private checkout");
            assert_ne!(Path::new(&pair[0][2]), self.root());
            assert!(!pair[0][3].is_empty() && pair[0][3] != "owner");
            assert_eq!(pair[0][3], pair[1][3], "same app actor");
            assert_eq!(pair[0][4], pair[1][4], "land sees the verified tip");
        }
        assert!(
            self.store()
                .tasks(None)
                .unwrap()
                .iter()
                .all(|t| !t.slug.starts_with("tools/review-app-verdict-")),
            "no silent worker errors"
        );
    }

    fn commands(&self) -> Vec<Vec<String>> {
        std::fs::read_to_string(self.home.join("commands.log"))
            .unwrap_or_default()
            .lines()
            .map(|line| line.split('\t').map(str::to_owned).collect())
            .collect()
    }
}

#[allow(dead_code)]
fn assert_linear_main(root: &Path, base: &str, commits: usize) {
    let history = git(
        root,
        &[
            "rev-list",
            "--reverse",
            "--parents",
            &format!("{base}..main"),
        ],
    );
    let rows: Vec<_> = history.lines().collect();
    assert_eq!(rows.len(), commits, "{history}");
    let mut parent = base;
    for row in rows {
        let ids: Vec<_> = row.split_whitespace().collect();
        assert_eq!(ids.len(), 2, "each verdict has exactly one parent: {row}");
        assert_eq!(
            ids[1], parent,
            "no verdict can replace the preceding commit"
        );
        parent = ids[0];
    }
    assert_eq!(parent, git(root, &["rev-parse", "main"]));
}

#[allow(dead_code)]
struct Gate(TcpListener);
impl Gate {
    fn new() -> Self {
        Self(TcpListener::bind("127.0.0.1:0").unwrap())
    }
    fn address(&self) -> String {
        self.0.local_addr().unwrap().to_string()
    }
    fn entered(&self) -> (Release, String) {
        stage(format_args!(
            "waiting for socket entry at {}",
            self.address()
        ));
        let (stream, _) = self.0.accept().unwrap();
        stream.set_read_timeout(Some(DEADLINE)).unwrap();
        let mut text = String::new();
        BufReader::new(&stream).read_line(&mut text).unwrap();
        assert!(!text.is_empty(), "the child must announce entry");
        stage(format_args!("socket entered: {}", text.trim_end()));
        (Release(stream), text.trim_end().into())
    }
}
#[allow(dead_code)]
struct Release(TcpStream);
impl Drop for Release {
    fn drop(&mut self) {
        let _ = self.0.write_all(b"x");
    }
}

/// Executed by the fixture's real `just check`, or its blocked board command.
#[test]
#[allow(dead_code)]
fn publication_gate() {
    let Ok(address) = std::env::var("SF_AFTER_COMMIT_GATE") else {
        return;
    };
    let mut stream = TcpStream::connect(address).unwrap();
    stream.set_read_timeout(Some(DEADLINE)).unwrap();
    writeln!(stream, "{}", std::env::current_dir().unwrap().display()).unwrap();
    stream.read_exact(&mut [0]).unwrap();
}

/// The stand-in lander enforces the native final swap's lock and tracked-clean requirement.
#[test]
#[allow(dead_code)]
fn publication_swap() {
    let Some(root) = std::env::var_os("SF_AFTER_COMMIT_SWAP_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let branch = std::env::var("SF_AFTER_COMMIT_SWAP_BRANCH").unwrap();
    let _writes = symphony::commit::write_lock(&root).unwrap();
    if !git(&root, &["status", "--porcelain", "--untracked-files=no"]).is_empty() {
        eprintln!(
            "main checkout {} {}",
            root.display(),
            symphony::landing::DIRTY_MAIN
        );
        std::process::exit(1);
    }
    if git(&root, &["symbolic-ref", "--quiet", "HEAD"]) == "refs/heads/main" {
        git(&root, &["merge", "--ff-only", "-q", &branch]);
    } else {
        let tip = git(&std::env::current_dir().unwrap(), &["rev-parse", "HEAD"]);
        git(&root, &["update-ref", "refs/heads/main", &tip]);
    }
}

/// A real ref-transaction hook observes the next verdict's completed main update.
#[test]
#[allow(dead_code)]
fn main_notice() {
    let Ok(address) = std::env::var("SF_AFTER_COMMIT_NOTICE") else {
        return;
    };
    let mut refs = String::new();
    std::io::stdin().read_to_string(&mut refs).unwrap();
    if refs.lines().any(|line| line.ends_with(" refs/heads/main")) {
        let root = std::env::current_dir().unwrap();
        let mut stream = TcpStream::connect(address).unwrap();
        writeln!(
            stream,
            "{}",
            git(&root, &["show", "main:docs/b.md"]).replace('\n', "|")
        )
        .unwrap();
    }
}

#[allow(dead_code)]
struct ClosingCommitter(Committer);
impl ClosingCommitter {
    fn start(root: &Path) -> Self {
        Self(Committer::start(root, Arc::new(Mutex::new(()))))
    }
}
impl Drop for ClosingCommitter {
    fn drop(&mut self) {
        self.0.close();
    }
}
#[allow(dead_code)]
struct ClosingSaves(Saves);
#[allow(dead_code)]
impl ClosingSaves {
    fn start(root: &Path) -> Self {
        Self(Saves::start(root))
    }
    fn post(&self, scope: &str, route: &str, payload: Value) -> Value {
        self.0
            .post(scope, route, &payload)
            .unwrap_or_else(|e| panic!("{route}: {} {}", e.status, e.error))
    }
}
impl Drop for ClosingSaves {
    fn drop(&mut self) {
        self.0.close();
    }
}
