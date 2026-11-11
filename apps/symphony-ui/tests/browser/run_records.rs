//! A run `tools/cargo.sh` records, read by the real server from a scratch board's checkout: the
//! board's Build and Test columns follow it (docs/MACHINE_WORK.md, "Run records").
use super::{SHOWN, chrome, chrome::Chrome, owner_controls::ScratchBoard, restored::Native};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

const WORKING: &str = "fixture/recorded-run";
const REVIEWED: &str = "fixture/reviewed-run";

/// A `tools/cargo.sh test` run in the server's checkout, its Cargo a stand-in that waits for
/// `release`. Dropping it releases it and reaps the wrapper, so a failed assertion leaves no run.
struct Run {
    child: Child,
    release: PathBuf,
}

impl Run {
    fn start(root: &Path, name: &str, handle: &str) -> Self {
        let release = root.join(format!("artifacts/release-{name}"));
        let child = Command::new(root.join("tools/cargo.sh"))
            .args(["test", "--locked", "-p", name])
            .env("CARGO", root.join("artifacts/cargo"))
            .env("RELEASE", &release)
            .env("TASKS_SESSION", handle)
            .env_remove("SYMPHONY_RUN")
            .env_remove("SYMPHONY_RUN_LAUNCHER")
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

/// Scroll the board so its pipeline starts at the column `status`, less `before` pixels.
fn scroll_to(chrome: &mut Chrome, status: &str, before: u32) {
    chrome.eval(&format!("(() => {{ const board = document.querySelector('.board-scroll'); const column = document.querySelector('.pipeline > [data-status={status}]'); board.scrollTo({{left:board.scrollLeft + column.getBoundingClientRect().left - board.getBoundingClientRect().left - {before},top:0,behavior:'instant'}}); }})()"));
}

fn columns(chrome: &mut Chrome) -> Value {
    chrome.eval("[...document.querySelectorAll('.pipeline > .column')].map(e=>e.dataset.status)")
}

#[test]
fn a_recorded_test_run_moves_its_task_to_test_and_back_and_a_reviewers_stays_in_review() {
    let Some(mut chrome) = chrome() else { return };
    let fixture =
        Fixture::start(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/cargo.sh"));
    let (site, root) = (&fixture.site, fixture.site.root());
    let (executor, reviewer) = (&fixture.executor, &fixture.reviewer);
    let desktop = json!({"width":1600,"height":900,"deviceScaleFactor":1,"mobile":false});
    let phone = json!({"width":390,"height":844,"deviceScaleFactor":1,"mobile":true});
    chrome.call("Emulation.setDeviceMetricsOverride", desktop.clone());
    chrome.open(&site.url("/tasks"));
    // No run was ever recorded here: no machine reports, and Build and Test are one rail.
    assert!(chrome.wait_for(&format!("document.querySelector('.column[data-status=in_progress] .taskcard[data-slug=\"{WORKING}\"]') !== null && document.querySelector('main')?.dataset.feed==='live'"), SHOWN), "{}", chrome.eval("document.body.innerText"));
    assert_eq!(
        columns(&mut chrome),
        json!([
            "concept",
            "design",
            "ready",
            "in_progress",
            "build-test",
            "review",
            "done"
        ]),
        "{}",
        chrome.eval("fetch('/api/task-board?counts=1&activity=1').then(r=>r.json()).then(j=>JSON.stringify(j.machine_work))")
    );
    let rail = "Math.round(document.querySelector('.column.rail').getBoundingClientRect().width)";
    assert_eq!(chrome.eval(rail), json!(136));
    assert_eq!(
        chrome.eval("document.querySelectorAll('.column.rail p').length"),
        json!(1)
    );
    scroll_to(&mut chrome, "ready", 0);
    screenshot(&mut chrome, "after-none");
    chrome.call("Emulation.setDeviceMetricsOverride", phone);
    assert!(chrome.wait_for(
        "document.querySelector('.board-scroll').getBoundingClientRect().width < 400",
        SHOWN
    ));
    assert_eq!(
        chrome.eval(rail),
        json!(136),
        "the phone's 280px columns leave the rail narrow"
    );
    assert_eq!(
        chrome.eval("document.documentElement.scrollWidth <= innerWidth"),
        json!(true)
    );
    scroll_to(&mut chrome, "build-test", 150);
    screenshot(&mut chrome, "after-none-phone");
    chrome.call("Emulation.setDeviceMetricsOverride", desktop);
    // The executor's tests and the reviewer's, each recorded as it starts.
    let mut working = Run::start(root, "working", executor);
    let mut reviewing = Run::start(root, "reviewing", reviewer);
    assert!(
        chrome.wait_for(
            "document.querySelector('.pipeline > [data-status=test]') !== null",
            SHOWN
        ),
        "no machine reported the runs: {}",
        chrome.eval("document.querySelector('.pipeline')?.innerText")
    );
    scroll_to(&mut chrome, "in_progress", 0);
    assert!(
        chrome.wait_for(&format!("document.querySelector('.column[data-status=test] .taskcard[data-slug=\"{WORKING}\"]') !== null"), SHOWN),
        "the running test never placed its task: {} {}",
        column(&mut chrome, WORKING),
        chrome.eval("document.querySelector('.pipeline')?.innerText")
    );
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
    let chip = format!(".taskcard[data-slug=\"{WORKING}\"] .work-phase");
    assert!(chrome.wait_for(
        &format!("document.querySelector('{chip}')?.textContent.includes('Test running')"),
        SHOWN
    ));
    // A reviewer's run is a chip on its card; the card stays where the review is.
    let reviewed = format!(".column[data-status=review] .taskcard[data-slug=\"{REVIEWED}\"]");
    assert!(chrome.wait_for(&format!("document.querySelector('{reviewed} .work-phase')?.textContent.includes('Test running')"), SHOWN), "{}", chrome.eval("document.querySelector('.pipeline')?.innerText"));
    // The stage totals are the server's, read again once the machine's work changed.
    let counts = "[...document.querySelectorAll('.pipeline > .column')].map(c=>c.dataset.status+'='+c.querySelector('.count')?.textContent).join(' ')";
    assert!(
        chrome.wait_for("document.querySelector('.column[data-status=test] .count').textContent==='1' && document.querySelector('.column[data-status=in_progress] .count').textContent==='0'", SHOWN),
        "{}",
        chrome.eval(counts)
    );
    screenshot(&mut chrome, "after-going");
    working.end();
    reviewing.end();
    // The runs ended: the cards are back where their lifecycle puts them, and the columns stay,
    // since this checkout now has a machine that reports its runs.
    assert!(chrome.wait_for(&format!("document.querySelector('.column[data-status=in_progress] .taskcard[data-slug=\"{WORKING}\"]') !== null && document.querySelector('{chip}') === null && document.querySelector('{reviewed} .work-phase') === null"), SHOWN), "{}", chrome.eval("document.querySelector('.pipeline')?.innerText"));
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
    scroll_to(&mut chrome, "in_progress", 0);
    screenshot(&mut chrome, "after-ended");
}

/// A scratch board holding an executor's task in progress and an author's in review with its
/// reviewer, and the real server over a checkout whose `tools/cargo.sh` is `wrapper` and whose
/// Cargo is a stand-in, its machine folder of its own.
struct Fixture {
    // Dropped in this order: the server before its board, the machine folder after both.
    site: Native,
    _board: ScratchBoard,
    executor: String,
    reviewer: String,
    _machine: Folder,
}

/// A folder of the test's own, removed when dropped.
struct Folder(PathBuf);

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Fixture {
    fn start(wrapper: &Path) -> Self {
        let board = ScratchBoard::start();
        let call = |method: &str, args: Value| {
            board.call_in_database("browser_owner_controls", method, args)
        };
        let handle = |kind: &str| {
            call("register", json!({"kind":kind}))["handle"]
                .as_str()
                .unwrap()
                .to_string()
        };
        let (executor, author, reviewer) = (handle("worker"), handle("worker"), handle("reviewer"));
        for (slug, title, by) in [
            (WORKING, "A task whose tests run", &executor),
            (REVIEWED, "A task its reviewer tests", &author),
        ] {
            call(
                "create",
                json!({"slug":slug,"title":title,"by":by,"priority":2}),
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
        let machine = std::env::temp_dir().join(format!(
            "sf-run-columns-machine-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&machine).unwrap();
        let machine = Folder(machine);
        let site = Native::start_with(
            Some(board.address()),
            &[("SYMPHONY_MACHINE_DIR", &machine.0)],
        );
        let root = site.root();
        std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        std::fs::create_dir_all(root.join("tools")).unwrap();
        std::fs::copy(wrapper, root.join("tools/cargo.sh")).unwrap();
        super::fixture_script::write(
            &root.join("artifacts/cargo"),
            "#!/bin/sh\nwhile [ ! -e \"$RELEASE\" ]; do sleep 0.1; done\nexit 0\n",
            false,
        );
        Self {
            site,
            _board: board,
            executor,
            reviewer,
            _machine: machine,
        }
    }
}

// TEMPORARY: main's page for the before screenshots; removed before commit.
#[test]
#[ignore = "evidence driver"]
fn before_screenshots_on_main() {
    let Some(mut chrome) = chrome() else { return };
    let wrapper = std::env::var_os("SF_MAIN_CARGO_SH").expect("main's cargo.sh");
    let fixture = Fixture::start(Path::new(&wrapper));
    let (site, root) = (&fixture.site, fixture.site.root());
    let desktop = json!({"width":1600,"height":900,"deviceScaleFactor":1,"mobile":false});
    let phone = json!({"width":390,"height":844,"deviceScaleFactor":1,"mobile":true});
    chrome.call("Emulation.setDeviceMetricsOverride", desktop.clone());
    chrome.open(&site.url("/tasks"));
    assert!(chrome.wait_for(&format!("document.querySelector('.column[data-status=in_progress] .taskcard[data-slug=\"{WORKING}\"]') !== null && document.querySelector('main')?.dataset.feed==='live'"), SHOWN));
    eprintln!("before columns: {}", columns(&mut chrome));
    scroll_to(&mut chrome, "ready", 0);
    screenshot(&mut chrome, "before-none");
    chrome.call("Emulation.setDeviceMetricsOverride", phone);
    assert!(chrome.wait_for(
        "document.querySelector('.board-scroll').getBoundingClientRect().width < 400",
        SHOWN
    ));
    scroll_to(&mut chrome, "build", 150);
    screenshot(&mut chrome, "before-none-phone");
    chrome.call("Emulation.setDeviceMetricsOverride", desktop);
    let _working = Run::start(root, "working", &fixture.executor);
    let _reviewing = Run::start(root, "reviewing", &fixture.reviewer);
    std::thread::sleep(std::time::Duration::from_secs(5));
    eprintln!(
        "before, run going: {} {}",
        column(&mut chrome, WORKING),
        columns(&mut chrome)
    );
    eprintln!(
        "records: {:?}",
        std::fs::read_dir(root.join(".git"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect::<Vec<_>>()
    );
    scroll_to(&mut chrome, "in_progress", 0);
    screenshot(&mut chrome, "before-going");
}
