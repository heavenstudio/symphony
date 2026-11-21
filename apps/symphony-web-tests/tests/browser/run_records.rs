//! A test run `tools/cargo.sh` sends through the board's recorder (`tasks run`), read by the real
//! server from its checkout's work folder: the board's Build and Test columns follow it, a slice's
//! run on its parent's card, and until anything was recorded they are one rail
//! (docs/workflow/MACHINE_WORK.md).
use super::{
    SHOWN, binaries, chrome, chrome::Chrome, owner_controls::ScratchBoard, restored::Native,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

const DATABASE: &str = "browser_owner_controls";
const WORKING: &str = "fixture/recorded-run";
const REVIEWED: &str = "fixture/reviewed-run";
/// The `/work` instruction its executor holds beside the task it serves.
const WORK: &str = "fixture/recorded-run/work";
/// A task in progress whose slice, held by another session, is what runs.
const SLICED: &str = "fixture/sliced-run";
const SLICE: &str = "fixture/sliced-run/slice";

/// Names a scale of the recorder's waits, which it takes on a scratch board alone.
const SCALE_VAR: &str = "TASKS_RUN_WAIT_SCALE";
/// The scale, as the board's recorder tests take it: 20 s for the recorder's word that it watches
/// its command and for the board's answer of the task a run is for, 12 s for git's.
const SCALE: &str = "20";
/// Names, in seconds, how long `tools/record.sh` waits for its recorder's probe.
const PROBE_WAIT_VAR: &str = "TASKS_RECORDER_PROBE_WAIT";
/// That wait: the backstop of a test's wait, where the script's own is 2 s.
const PROBE_WAIT: &str = "120";

/// A `tools/cargo.sh test` run in the server's checkout as `handle`, recorded on the scratch board
/// at `board` into the work folder the server reads. Its Cargo is a stand-in that waits for its
/// release; dropping the run releases it and reaps it, so a failed assertion leaves no run. On that
/// scratch board its recorder takes the scale of its waits ([`SCALE`]), and the script waits long
/// for its probe ([`PROBE_WAIT`]): a loaded machine never leaves a run unrecorded, or recorded for
/// no task (docs/workflow/MACHINE_WORK.md, "A test's waits"). A recorder grown slow is the board's tests' to
/// turn red.
struct Run {
    child: Child,
    release: PathBuf,
}

impl Run {
    fn start(fixture: &Fixture, name: &str, handle: &str) -> Self {
        let root = fixture.site.root();
        let release = root.join(format!("artifacts/release-{name}"));
        let mut command = Command::new(root.join("tools/cargo.sh"));
        // The suite itself runs inside a recorded `just test-rs`: this run is not one of its steps.
        for name in [
            "TASKS_RUN_PARENT",
            "TASKS_TASK",
            "GIT_DIR",
            "GIT_COMMON_DIR",
        ] {
            command.env_remove(name);
        }
        let child = command
            .args(["test", "--locked", "-p", name])
            .env("CARGO", root.join("artifacts/cargo"))
            .env("RELEASE", &release)
            .env("TASKS_SESSION", handle)
            .env("TASKS_RECORDER", binaries::tasks())
            .env("TASKS_WORK", root.join(".git/symphony/tasks/work"))
            .env("TASKS_SERVER", fixture.board.address())
            .env("TASKS_DATABASE", DATABASE)
            .env(SCALE_VAR, SCALE)
            .env(PROBE_WAIT_VAR, PROBE_WAIT)
            .env("SYMPHONY_MACHINE_DIR", &fixture.machine.0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Self { child, release }
    }

    fn end(&mut self) {
        let _ = std::fs::write(&self.release, "");
        let _ = self.child.wait();
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        self.end();
    }
}

/// The page as the screenshot evidence keeps it, under `artifacts/run-columns/` of the checkout.
fn screenshot(chrome: &mut Chrome, name: &str) {
    chrome.eval("document.fonts.ready.then(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))))");
    let shot = chrome.call("Page.captureScreenshot", json!({"format":"png"}));
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/run-columns");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join(format!("{name}.base64")),
        shot["data"].as_str().unwrap(),
    )
    .unwrap();
}

/// The column the card `slug` is drawn in.
fn column(chrome: &mut Chrome, slug: &str) -> Value {
    chrome.eval(&format!(
        "document.querySelector('.taskcard[data-slug=\"{slug}\"]')?.closest('.column')?.dataset.status ?? null"
    ))
}

fn columns(chrome: &mut Chrome) -> Value {
    chrome
        .eval("[...document.querySelectorAll('.waiting-columns .column')].map(e=>e.dataset.status)")
}

fn desktop() -> Value {
    json!({"width":1600,"height":900,"deviceScaleFactor":1,"mobile":false})
}

fn phone() -> Value {
    json!({"width":390,"height":844,"deviceScaleFactor":1,"mobile":true})
}

#[test]
fn a_recorded_test_run_moves_its_task_to_test_and_back_and_a_reviewers_stays_in_review() {
    let Some(mut chrome) = chrome() else { return };
    let fixture = Fixture::start();
    chrome.call("Emulation.setDeviceMetricsOverride", desktop());
    chrome.open(&fixture.site.url("/tasks"));
    // Build and Test remain columns before this project records its first run.
    assert!(chrome.wait_for(&format!("document.querySelector('.column[data-status=in_progress] .taskcard[data-slug=\"{WORKING}\"]') !== null && document.querySelector('main')?.dataset.feed==='live'"), SHOWN), "{}", chrome.eval("document.body.innerText"));
    assert_eq!(
        columns(&mut chrome),
        json!([
            "concept",
            "design",
            "ready",
            "in_progress",
            "build",
            "test",
            "review",
            "done"
        ]),
        "{}",
        chrome.eval("fetch('/api/task-board?counts=1&activity=1').then(r=>r.json()).then(j=>JSON.stringify(j.machine_work))")
    );
    screenshot(&mut chrome, "after-none");
    chrome.call("Emulation.setDeviceMetricsOverride", phone());
    assert!(chrome.wait_for("document.querySelectorAll('.waiting-columns .column').length === 1 && document.querySelector('.column[data-status=review]') !== null",SHOWN));
    assert_eq!(
        chrome.eval("document.documentElement.scrollWidth <= innerWidth"),
        json!(true)
    );
    screenshot(&mut chrome, "after-none-phone");
    chrome.call("Emulation.setDeviceMetricsOverride", desktop());
    let mut working = Run::start(&fixture, "working", &fixture.executor);
    let mut reviewing = Run::start(&fixture, "reviewing", &fixture.reviewer);
    let mut slicing = Run::start(&fixture, "slicing", &fixture.slicer);
    assert!(
        chrome.wait_for(
            "document.querySelector('.column[data-status=test]') !== null",
            SHOWN
        ),
        "no machine reported its runs"
    );
    chrome.eval("document.querySelector('.column[data-status=test]').scrollIntoView({block:'nearest',inline:'center'})");
    assert!(chrome.wait_for(&format!("document.querySelector('.column[data-status=test] .taskcard[data-slug=\"{WORKING}\"]') !== null && document.querySelector('.column[data-status=test] .taskcard[data-slug=\"{SLICED}\"]') !== null && document.querySelector('.column[data-status=test] .count').textContent === '2'"),SHOWN),"recorded runs did not place executor and slice-parent cards: {}",chrome.eval("document.body.innerText"));
    assert_eq!(
        columns(&mut chrome),
        json!([
            "concept",
            "design",
            "ready",
            "in_progress",
            "build",
            "test",
            "review",
            "done"
        ])
    );
    assert!(chrome.wait_for(&format!("document.querySelector('.column[data-status=review] .taskcard[data-slug=\"{REVIEWED}\"]') !== null"), SHOWN));
    assert_eq!(
        column(&mut chrome, REVIEWED),
        json!("review"),
        "a reviewer's run never moves the review card to Test"
    );
    screenshot(&mut chrome, "after-going");
    chrome.eval("document.querySelector('.column[data-status=review]').scrollIntoView({block:'nearest',inline:'center'})");
    assert!(chrome.wait_for(&format!("document.querySelector('.column[data-status=review] .taskcard[data-slug=\"{REVIEWED}\"]') !== null && document.querySelector('.column[data-status=in_progress] .count').textContent === '0'"),SHOWN));
    working.end();
    reviewing.end();
    slicing.end();
    assert!(chrome.wait_for(&format!("document.querySelector('.column[data-status=in_progress] .taskcard[data-slug=\"{WORKING}\"]') !== null && document.querySelector('.column[data-status=in_progress] .taskcard[data-slug=\"{SLICED}\"]') !== null && document.querySelector('.column[data-status=review] .taskcard[data-slug=\"{REVIEWED}\"]') !== null"),SHOWN),"ended runs did not return their lifecycle cards: {}",chrome.eval("document.body.innerText"));
    assert!(chrome.wait_for(
        "document.querySelector('.column[data-status=test] .count').textContent === '0'",
        SHOWN
    ));
    assert_eq!(
        columns(&mut chrome),
        json!([
            "concept",
            "design",
            "ready",
            "in_progress",
            "build",
            "test",
            "review",
            "done"
        ])
    );
    screenshot(&mut chrome, "after-ended");
}

/// A scratch board holding an executor's task in progress and an author's in review with its
/// reviewer, and the real server over a checkout with this workspace's `tools/cargo.sh` and
/// `tools/record.sh`, a stand-in Cargo and a machine folder of its own.
struct Fixture {
    // Dropped in this order: the server before its board, the machine folder after both.
    site: Native,
    board: ScratchBoard,
    executor: String,
    reviewer: String,
    slicer: String,
    machine: Folder,
}

/// A folder of the test's own, removed when dropped.
struct Folder(PathBuf);

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Fixture {
    fn start() -> Self {
        let board = ScratchBoard::start();
        let call = |method: &str, args: Value| board.call_in_database(DATABASE, method, args);
        let handle = |kind: &str| {
            call("register", json!({"kind":kind}))["handle"]
                .as_str()
                .unwrap()
                .to_string()
        };
        let (executor, author, reviewer, slicer) = (
            handle("worker"),
            handle("worker"),
            handle("reviewer"),
            handle("worker"),
        );
        for (slug, title, by, parent) in [
            (WORKING, "A task whose tests run", &executor, None),
            (
                WORK,
                "Do the task whose tests run",
                &executor,
                Some(WORKING),
            ),
            (REVIEWED, "A task its reviewer tests", &author, None),
            (SLICED, "A task a slice of which is tested", &author, None),
            (SLICE, "A slice whose tests run", &slicer, Some(SLICED)),
        ] {
            call(
                "create",
                json!({"slug":slug,"title":title,"by":by,"priority":2,"parent":parent,
                    "instruction":slug == WORK}),
            );
            call("claim", json!({"slug":slug,"handle":by}));
        }
        call(
            "move",
            json!({"slug":REVIEWED,"status":"review","handle":author}),
        );
        call(
            "take_role",
            json!({"slug":REVIEWED,"handle":reviewer,"role":"reviewer"}),
        );
        assert_eq!(
            call("get", json!({"slug":WORKING}))["status"],
            "in_progress"
        );
        assert_eq!(call("get", json!({"slug":REVIEWED}))["status"], "review");
        for slug in [WORK, SLICED, SLICE] {
            assert_eq!(call("get", json!({"slug":slug}))["status"], "in_progress");
        }
        let machine = Folder(std::env::temp_dir().join(format!(
            "symphony-run-columns-machine-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
        std::fs::create_dir_all(&machine.0).unwrap();
        let site = Native::start_with(
            Some(board.address()),
            &[("SYMPHONY_MACHINE_DIR", &machine.0)],
        );
        let root = site.root();
        std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        std::fs::create_dir_all(root.join("tools")).unwrap();
        let tools = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools");
        for name in ["cargo.sh", "record.sh"] {
            std::fs::copy(tools.join(name), root.join("tools").join(name)).unwrap();
        }
        super::fixture_script::write(
            &root.join("artifacts/cargo"),
            "#!/bin/sh\nwhile [ ! -e \"$RELEASE\" ]; do sleep 0.1; done\nexit 0\n",
            false,
        );
        Self {
            site,
            board,
            executor,
            reviewer,
            slicer,
            machine,
        }
    }
}
