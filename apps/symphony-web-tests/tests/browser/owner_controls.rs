//! Owner writes against an owned scratch board and the real native server.
use super::{SHOWN, binaries, chrome, restored::Native};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::time::Instant;

/// A board's PostgreSQL of the test's own, which it is seen to serve before any write: the
/// machine's programs, as a board finds them when its project file names none (the first
/// `postgres` on `PATH`), its data made by `initdb` in a scratch folder, listening on a socket in a
/// short folder under `/tmp` alone (a socket's path has a length limit, which a temp folder can
/// pass), never on TCP, and ended with the test however that ends. This crate links nothing of
/// the board's: its address is `postgresql:` and that folder, as the board names one.
pub(super) struct ScratchBoard {
    child: Option<Child>,
    reader: Option<std::thread::JoinHandle<()>>,
    root: PathBuf,
    socket: PathBuf,
    address: String,
    tasks: &'static Path,
}

fn scratch_root(stamp: u128) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "symphony-owner-board-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn scratch_roots_are_unique_when_clock_readings_repeat_across_threads() {
    let threads: Vec<_> = (0..8)
        .map(|_| std::thread::spawn(|| scratch_root(0)))
        .collect();
    let roots: std::collections::BTreeSet<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(roots.len(), 8, "scratch paths collided: {roots:?}");
}

/// The folder of the machine's PostgreSQL programs: that of the first executable `postgres` on
/// `PATH`, which must hold `initdb` too.
fn programs() -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let executable = |path: &Path| {
        std::fs::metadata(path)
            .is_ok_and(|file| file.is_file() && file.permissions().mode() & 0o111 != 0)
    };
    let path = std::env::var_os("PATH").unwrap_or_default();
    let folder = std::env::split_paths(&path)
        .find(|folder| executable(&folder.join("postgres")))
        .unwrap_or_else(|| {
            panic!(
                "no executable postgres on PATH: the browser tests' boards run on PostgreSQL 18 or \
                 later (`brew install postgresql@18` on a Mac, its bin folder on PATH)"
            )
        });
    assert!(
        executable(&folder.join("initdb")),
        "{} holds postgres but no initdb",
        folder.display()
    );
    folder
}

impl ScratchBoard {
    pub(super) fn start() -> Self {
        static SOCKETS: AtomicUsize = AtomicUsize::new(0);
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let tasks = binaries::tasks();
        let programs = programs();
        let root = scratch_root(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        );
        std::fs::create_dir(&root).unwrap();
        let socket = PathBuf::from(format!(
            "/tmp/symphony-wb-{}-{}",
            std::process::id(),
            SOCKETS.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&socket);
        std::fs::create_dir(&socket).unwrap();
        let data = root.join("data");
        let made = Command::new(programs.join("initdb"))
            .arg("-D")
            .arg(&data)
            .args([
                "--encoding=UTF8",
                "--no-locale",
                "--auth-local=trust",
                "--auth-host=reject",
                "--no-instructions",
                "--no-sync",
                "--username=postgres",
            ])
            .env("LC_ALL", "C")
            .output()
            .unwrap();
        assert!(made.status.success(), "private board's initdb: {made:?}");
        let mut board = Self {
            child: None,
            reader: None,
            root,
            address: format!("postgresql:{}", socket.display()),
            socket,
            tasks,
        };
        // The store harness's pipe leash: EOF also stops the database when the suite is
        // interrupted and Rust destructors cannot run, by a fast shutdown, which ends its
        // connections. Both children are named by $!, and the database's pid is said on the
        // shell's output, with what the database says.
        board.child = Some(
            Command::new("bash")
                .arg("-c")
                .arg(concat!(
                    r#". "$3"; fixture_close_fds; exec 3<&0; "#,
                    r#""$0" -D "$1" -c listen_addresses= -c unix_socket_directories="$2" "#,
                    r#"-c unix_socket_permissions=0700 -c fsync=off -c lc_messages=C 3<&- 2>&1 & server=$!; "#,
                    r#"echo "server pid $server"; "#,
                    r#"(fixture_wait_eof <&3; kill -INT $server 2>/dev/null) & leash=$!; "#,
                    r#"exec 3<&-; wait $server; kill $leash 2>/dev/null; wait $leash 2>/dev/null"#,
                ))
                .arg(programs.join("postgres"))
                .arg(&data)
                .arg(&board.socket)
                .arg(workspace.join("tools/fixture_process.sh"))
                .env("LC_ALL", "C")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(std::fs::File::create(board.root.join("stderr.log")).unwrap())
                .spawn()
                .unwrap(),
        );
        let stdout = board.child.as_mut().unwrap().stdout.take().unwrap();
        let log = board.root.join("stdout.log");
        let (send, receive) = std::sync::mpsc::channel();
        board.reader = Some(std::thread::spawn(move || {
            let mut log = std::fs::File::create(log).unwrap();
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                writeln!(log, "{line}").unwrap();
                if line.contains("database system is ready to accept connections") {
                    let _ = send.send(());
                }
            }
        }));
        // A quiet machine has it up within SHOWN, and only a stall reaches the backstop
        // (RULES.md `tests/no-test-fails-on-load`).
        let ready_by = Instant::now() + super::STALL;
        match receive.recv_timeout(ready_by.saturating_duration_since(Instant::now())) {
            Ok(()) => {}
            Err(RecvTimeoutError::Disconnected) => {
                panic!("private board ended before it was ready: {}", board.ended())
            }
            Err(error @ RecvTimeoutError::Timeout) => panic!(
                "private board: stalled: not ready by the readiness backstop of {:?} ({error}); \
                 {}; {}",
                super::STALL,
                test_time::Load::now(),
                std::fs::read_to_string(board.root.join("stdout.log")).unwrap_or_default()
            ),
        }
        board
    }

    /// Everything an ended board said, once its reader has seen its output close; the board then
    /// holds no child.
    fn ended(&mut self) -> String {
        if let Some(mut child) = self.child.take() {
            let _ = child.wait();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        let read = |name| std::fs::read_to_string(self.root.join(name)).unwrap_or_default();
        format!("{}{}", read("stdout.log"), read("stderr.log"))
    }

    pub(super) fn address(&self) -> &str {
        &self.address
    }

    fn call(&self, method: &str, args: Value) -> Value {
        self.call_in_database("browser_owner_controls", method, args)
    }

    pub(super) fn call_in_database(&self, database: &str, method: &str, args: Value) -> Value {
        let input = self.root.join("call.json");
        std::fs::write(&input, json!({"method":method,"args":args}).to_string()).unwrap();
        let done = Command::new(self.tasks)
            .arg("api")
            .env("TASKS_SERVER", &self.address)
            .env("TASKS_DATABASE", database)
            .env("TASKS_SWEEP", "off")
            .env_remove("TASKS_SESSION")
            .env_remove("GIT_DIR")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .current_dir(&self.root)
            .stdin(std::fs::File::open(input).unwrap())
            .output()
            .unwrap();
        assert!(done.status.success(), "private board API: {done:?}");
        let response: Value = serde_json::from_slice(&done.stdout).unwrap();
        response
            .get("ok")
            .unwrap_or_else(|| panic!("private board API: {response}"))
            .clone()
    }
}

impl Drop for ScratchBoard {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            drop(child.stdin.take());
            let _ = child.wait();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        let _ = std::fs::remove_dir_all(&self.root);
        let _ = std::fs::remove_dir_all(&self.socket);
    }
}

#[test]
fn owner_priority_and_cancellation_update_the_task_and_open_board_without_reload() {
    let Some(mut page) = chrome() else { return };
    let Some(mut overview) = chrome() else { return };
    overview.viewport(1440, 900, false);
    let board = ScratchBoard::start();
    let actor = board.call("register", json!({"kind":"worker"}));
    let slug = "fixture/owner-controls";
    board.call(
        "create",
        json!({"slug":slug,"title":"Owner controls","by":actor["handle"],"priority":4}),
    );
    let site = Native::start_with_board(Some(&board.address));
    page.open(&site.url(&format!("/task/{slug}")));
    assert!(page.wait_for("document.querySelector('#task-priority span')?.textContent==='p4' && document.querySelector('main')?.dataset.feed==='live'",SHOWN),"{}",page.eval("document.body.innerText"));
    page.eval("window.ownerControlsVisit=true");
    page.eval("document.getElementById('task-priority').click()");
    assert!(page.wait_for(
        "document.querySelectorAll('[role=listbox] [role=option]').length===5",
        SHOWN
    ));
    assert_eq!(
        page.eval("[...document.querySelectorAll('[role=listbox] [role=option]')].map(o=>o.textContent.trim())"),
        json!(["p0", "p1", "p2", "p3", "p4"])
    );
    super::components::key(&mut page, "Escape", "Escape", 27);
    assert!(page.wait_for(
        "document.getElementById('task-priority').getAttribute('aria-expanded')==='false'",
        SHOWN
    ));
    overview.open(&site.url("/tasks"));
    assert!(overview.wait_for(
        "document.querySelector('.column[data-status=ready]') !== null",
        SHOWN
    ));
    overview.eval(
        "document.querySelector('.column[data-status=ready]').scrollIntoView({block:'center'})",
    );
    assert!(overview.wait_for("document.querySelector('.column[data-status=ready] [data-slug=\"fixture/owner-controls\"].p4') !== null && document.querySelector('main')?.dataset.feed==='live'",SHOWN),"{}",overview.eval("document.body.innerText"));
    overview.eval("window.ownerControlsVisit=true");
    super::components::choose(&mut page, "#task-priority", "p0");
    // The header says the priority once, by its control: saved, and showing the new one.
    assert!(page.wait_for("document.querySelector('.task-controls [role=status]')?.textContent==='Priority saved' && document.querySelector('#task-priority span').textContent==='p0' && !document.getElementById('task-priority').disabled",SHOWN),"{}",page.eval("document.body.innerText"));
    assert!(overview.wait_for("document.querySelector('.column[data-status=ready] [data-slug=\"fixture/owner-controls\"].p0') !== null",SHOWN),"{}",overview.eval("document.body.innerText"));
    let stored = board.call("get", json!({"slug":slug}));
    assert_eq!(stored["priority"], 0);
    assert_eq!(stored["status"], "ready");
    page.eval("[...document.querySelectorAll('.task-controls button')].find(b=>b.textContent==='Cancel task').click()");
    assert!(page.wait_for(
        "document.querySelectorAll('.task-controls [role=group]').length===1",
        SHOWN
    ));
    assert_eq!(
        board.call("get", json!({"slug":slug}))["status"],
        "ready",
        "asking for confirmation must not cancel"
    );
    page.eval("[...document.querySelectorAll('.task-controls button')].find(b=>b.textContent==='Keep task').click()");
    assert!(page.wait_for(
        "document.querySelector('.task-controls [role=group]')===null",
        SHOWN
    ));
    assert_eq!(
        board.call("get", json!({"slug":slug}))["status"],
        "ready",
        "declining confirmation must not cancel"
    );
    page.eval("[...document.querySelectorAll('.task-controls button')].find(b=>b.textContent==='Cancel task').click()");
    assert!(page.wait_for(
        "document.querySelector('.task-controls [role=group]')!==null",
        SHOWN
    ));
    page.eval("[...document.querySelectorAll('.task-controls button')].find(b=>b.textContent==='Confirm cancellation').click()");
    assert!(page.wait_for("document.querySelector('.task-head .state')?.textContent==='cancelled' && ![...document.querySelectorAll('.task-controls button')].some(b=>b.textContent==='Cancel task' || b.textContent==='Confirm cancellation')",SHOWN),"{}",page.eval("document.body.innerText"));
    assert!(overview.wait_for("document.querySelector('.column[data-status=ready] .count')?.textContent==='0' && document.querySelector('[data-open-stage=cancelled]')?.textContent.startsWith('1 cancelled')",SHOWN),"{}",overview.eval("document.body.innerText"));
    overview.eval("document.querySelector('[data-open-stage=cancelled]').click()");
    assert!(overview.wait_for("document.querySelector('.column[data-status=cancelled] .count')?.textContent==='1' && document.querySelector('.column[data-status=cancelled] [data-slug=\"fixture/owner-controls\"].p0') !== null",SHOWN));
    assert_eq!(page.eval("window.ownerControlsVisit"), json!(true));
    assert_eq!(overview.eval("window.ownerControlsVisit"), json!(true));
    let stored = board.call("get", json!({"slug":slug}));
    assert_eq!(stored["status"], "cancelled");
    assert_eq!(stored["priority"], 0);
    let history = board.call("thread", json!({"slug":slug}));
    let history = history.as_array().unwrap();
    assert_eq!(history.len(), 3);
    assert_eq!(history[1]["sender"], "owner");
    assert_eq!(history[1]["text"], "owner changed priority p4 → p0");
    assert_eq!(history[2]["sender"], "owner");
    assert_eq!(history[2]["text"], "owner moved it ready → cancelled");
}

/// A canvas picture of `w` by `h`, as a transfer holding one `File`, for a paste or a drop.
const PICTURES: &str = "window.attachPicture = async (name, w, h) => {
    const canvas = document.createElement('canvas');
    canvas.width = w; canvas.height = h;
    const pen = canvas.getContext('2d'); pen.fillStyle = '#c33'; pen.fillRect(0, 0, w, h);
    const blob = await new Promise(done => canvas.toBlob(done, 'image/png'));
    const transfer = new DataTransfer();
    transfer.items.add(new File([blob], name, {type: 'image/png'}));
    return transfer;
}; true";

/// The picture named `alt` under `scope`, scrolled to and loaded at `width` from the upload store.
fn shown(scope: &str, alt: &str, width: u32) -> String {
    format!(
        "(() => {{ const i = [...document.querySelectorAll({scope})].find(i => i.alt === {alt});
            if (!i) return false; i.scrollIntoView({{block: 'center'}});
            return i.complete && i.naturalWidth === {width} && i.getAttribute('src').startsWith('/artifacts/uploads/'); }})()",
        scope = json!(format!("{scope} img")),
        alt = json!(alt),
    )
}

/// The upload address of the file attached in `form`, once its chip shows.
fn chip(page: &mut super::chrome::Chrome, form: &str) -> String {
    let found = format!(
        "document.querySelector({})?.dataset.url",
        json!(format!(
            "{form} .attach-file[data-url^=\"/artifacts/uploads/\"]"
        ))
    );
    assert!(
        page.wait_for(&found, SHOWN),
        "no attached file in {form}: {}",
        page.eval(&format!(
            "document.querySelector({})?.outerHTML",
            json!(form)
        ))
    );
    page.eval(&found).as_str().unwrap().to_string()
}

fn type_into(page: &mut super::chrome::Chrome, field: &str, text: &str) {
    page.eval(&format!(
        "(() => {{ const t = document.querySelector({}); t.value = {}; t.dispatchEvent(new Event('input', {{bubbles: true}})); return true; }})()",
        json!(field),
        json!(text)
    ));
}

#[test]
fn a_pasted_dropped_or_picked_picture_shows_on_the_message_task_and_card_it_was_sent_with() {
    let Some(mut page) = chrome() else { return };
    let board = ScratchBoard::start();
    let actor = board.call("register", json!({"kind":"worker"}));
    let slug = "fixture/owner-attachments";
    board.call(
        "create",
        json!({"slug":slug,"title":"Owner attachments","by":actor["handle"],"priority":2}),
    );
    let site = Native::start_with_board(Some(&board.address));
    let stored = |url: &str| site.root().join(url.trim_start_matches('/'));

    // A screenshot pasted into the task's message box is uploaded at once and sent with the words.
    // The head, the gates and the timeline's box can put the lazy message box below the window and
    // past its look-ahead: scroll to its place, always in the page, as a person would.
    page.open(&site.url(&format!("/task/{slug}")));
    let composer = json!(format!("composer-{slug}"));
    assert!(
        page.wait_for(
            &format!("document.getElementById({composer}) !== null"),
            SHOWN
        ),
        "{}",
        page.eval("document.body.innerText")
    );
    page.eval(&format!(
        "document.getElementById({composer}).scrollIntoView({{block:'center'}})"
    ));
    assert!(
        page.wait_for(
            "document.querySelector('#task-send[data-attach] input[data-attach-input]') !== null && document.querySelector('main')?.dataset.feed === 'live'",
            SHOWN
        ),
        "{}",
        page.eval("document.body.innerText")
    );
    page.eval(PICTURES);
    let pasted = page.eval(
        "(async () => { const transfer = await attachPicture('pasted shot.png', 3, 2);
            const box = document.getElementById('task-message'); box.focus();
            return box.dispatchEvent(new ClipboardEvent('paste', {clipboardData: transfer, bubbles: true, cancelable: true})); })()",
    );
    assert_eq!(
        pasted,
        json!(false),
        "the paste is taken, not typed into the box"
    );
    let url = chip(&mut page, "#task-send");
    assert!(
        stored(&url).is_file(),
        "{url} is stored in the server's checkout"
    );
    type_into(&mut page, "#task-message", "Pasted");
    page.eval("document.querySelector('#task-send button[type=submit]').click()");
    assert!(
        page.wait_for(&shown(".entry.owner", "pasted shot.png", 3), SHOWN),
        "{}",
        page.eval("document.getElementById('task-thread')?.outerHTML")
    );
    assert!(page.wait_for(
        "document.querySelector('#task-send .attach-file') === null && document.getElementById('task-message').value === ''",
        SHOWN
    ));
    let thread = board.call("thread", json!({"slug":slug}));
    let sent = thread.as_array().unwrap().last().unwrap();
    assert_eq!(sent["sender"], "owner");
    assert_eq!(sent["text"], format!("Pasted\n\n![pasted shot.png]({url})"));
    assert!(
        page.eval("[...document.querySelectorAll('.entry.owner .entry-text')].map(e => e.textContent).join(' ')")
            .as_str()
            .unwrap()
            .contains("Pasted"),
        "the words stay beside the picture"
    );

    // A picture dropped on the new-task form goes into the brief the task page shows.
    page.eval("document.getElementById('quick-open').click()");
    assert!(page.wait_for(
        "document.querySelector('#quick-sheet form') !== null",
        SHOWN
    ));
    page.eval("[...document.querySelectorAll('#quick-sheet .tabs button')].find(b => b.textContent === 'Add task').click()");
    assert!(page.wait_for("document.getElementById('quick-title') !== null", SHOWN));
    assert_eq!(
        page.eval("[document.querySelectorAll('style#attach-style').length, [...document.querySelectorAll('link[rel=stylesheet]')].some(l => l.href.includes('attach'))]"),
        json!([1, false]),
        "the page's and the panel's modules share one inserted style, and no stylesheet is named for it"
    );
    type_into(&mut page, "#quick-title", "Dropped picture");
    let dropped = page.eval(
        "(async () => { const transfer = await attachPicture('dropped.png', 5, 4);
            const form = document.querySelector('#quick-sheet form'), box = form.querySelector('textarea');
            const over = new DragEvent('dragover', {dataTransfer: transfer, bubbles: true, cancelable: true});
            box.dispatchEvent(over);
            const marked = form.hasAttribute('data-dropping');
            const kept = box.dispatchEvent(new DragEvent('drop', {dataTransfer: transfer, bubbles: true, cancelable: true}));
            return [over.defaultPrevented, marked, kept, form.hasAttribute('data-dropping')]; })()",
    );
    assert_eq!(
        dropped,
        json!([true, true, false, false]),
        "a file over the form is let in and marked, then taken and unmarked"
    );
    let url = chip(&mut page, "#quick-sheet form");
    assert!(stored(&url).is_file());
    page.eval("document.querySelector('#quick-sheet form button[type=submit]').click()");
    let created = "document.querySelector('#quick-sheet [role=status] a[href^=\"/task/\"]')?.getAttribute('href')";
    assert!(
        page.wait_for(created, SHOWN),
        "{}",
        page.eval("document.getElementById('quick-sheet').innerText")
    );
    let created = page.eval(created).as_str().unwrap().to_string();
    assert_eq!(
        page.eval("document.querySelector('#quick-sheet .attach-file')"),
        Value::Null
    );
    page.open(&site.url(&created));
    assert!(
        page.wait_for(&shown(".task-body", "dropped.png", 5), SHOWN),
        "{}",
        page.eval("document.querySelector('.task-body')?.outerHTML")
    );

    // On a phone, the picker attaches a file to a card's message.
    std::fs::write(
        site.root().join("docs/CARDS.md"),
        "# Cards\n\n<!-- register:start -->\n## Waiting\n\n### Fixture\n\n- [ ] **one** — A question.\n<!-- register:end -->\n",
    )
    .unwrap();
    page.viewport_scaled(390, 844, 3, true);
    page.open(&site.url("/docs/CARDS.md"));
    assert!(
        page.wait_for(
            "document.querySelector('.doc-card .doc-actions button') !== null",
            SHOWN
        ),
        "{}",
        page.eval("document.body.innerText")
    );
    page.eval("document.querySelector('.doc-card .doc-actions button').click()");
    let picker = ".doc-card form.verdict:not([hidden]) input[data-attach-input]";
    assert!(page.wait_for(
        &format!("document.querySelector({}) !== null", json!(picker)),
        SHOWN
    ));
    assert_eq!(
        page.eval("(() => { const r = document.querySelector('.doc-card .attach-pick').getBoundingClientRect(); return r.height >= 44 && r.left >= 0 && r.right <= innerWidth; })()"),
        json!(true),
        "the picker is a whole touch target on the phone's screen"
    );
    let input = page.call(
        "Runtime.evaluate",
        json!({"expression": format!("document.querySelector({})", json!(picker))}),
    );
    let icon = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/browser/fixtures/apple-touch-icon.png")
        .canonicalize()
        .unwrap();
    page.call(
        "DOM.setFileInputFiles",
        json!({"files":[icon], "objectId": input["result"]["objectId"]}),
    );
    let url = chip(&mut page, ".doc-card form.verdict");
    assert_eq!(
        std::fs::read(stored(&url)).unwrap(),
        std::fs::read(&icon).unwrap()
    );
    type_into(
        &mut page,
        ".doc-card form.verdict textarea",
        "From the picker",
    );
    page.eval("document.querySelector('.doc-card form.verdict button[type=submit]').click()");
    assert!(
        page.wait_for(&shown(".doc-card", "apple-touch-icon.png", 180), SHOWN),
        "{}",
        page.eval("document.querySelector('.doc-card')?.outerHTML")
    );
    let written = std::fs::read_to_string(site.root().join("docs/CARDS.md")).unwrap();
    assert!(
        written.contains(&format!("    ![apple-touch-icon.png]({url})\n")),
        "{written}"
    );
}
