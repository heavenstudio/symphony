//! Two projects on the one real server, each as the owner first opens it: its home and its tasks
//! board each within the first visit's budget, its theme in the page's computed look and the rest
//! of the look the other's, project links under its own `/<id>/`, and the header switch back to
//! the root picker; what one keeps in the browser, a draft or a
//! saved theme, never shows in the other, whose origin it shares. `alpha` is a plain project whose
//! file sets a token and a layout; `pantry` names the shelf plugin and the theme package it
//! provides; `gamma`, in the one case that registers it, serves its documents at its root.
use super::{
    HOME_FIRST_VISIT_BYTES, PAGE_WIDTH, SHOWN, TASKS_FIRST_VISIT_BYTES, binaries, chrome,
    chrome::Chrome, completed_requests, dist, needed_bytes, owner_controls::ScratchBoard,
    ping_bytes,
};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

const ALPHA: &str = "[project]\nname = \"Alpha\"\nid = \"alpha\"\n\
                     [theme.tokens]\nprimary = \"#7a2e8f\"\n[theme.layouts]\nhome = \"focus\"\n";
const PANTRY: &str = "[project]\nname = \"Pantry\"\nid = \"pantry\"\n\
                      [plugins]\ncrates = [\"fixture-shelf\"]\n[theme]\npackage = \"fixture-shelf\"\n";
/// A register with one open card, so each home has a next card to show.
const REGISTER: &str = "# Decisions\n\n- [ ] **first** — The first question.\n";
/// A document whose text links another document, its own heading and a page outside the server,
/// and shows a picture of its folder.
const NOTES: &str = "# Notes\n\nThe [register](DECISIONS.md) holds the questions; \
                     [this page](#notes) and [elsewhere](https://example.invalid/page) are not \
                     the project's to place.\n\n![A dot](dot.svg)\n";
const DOT: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"8\" height=\"8\">\
                   <circle cx=\"4\" cy=\"4\" r=\"4\"/></svg>\n";
/// A project whose documents are served at its root: a path under its base that is not the
/// Symphony's own names a document or a folder of it.
const GAMMA: &str = "[project]\nname = \"Gamma\"\nid = \"gamma\"\n\
                     [[entrypoints]]\nurl = \"/\"\nfolder = \"docs\"\n";

/// The scratch board's database every project of the one server reads.
const DATABASE: &str = "browser_projects";

/// The one server of a scratch machine folder, its client installed, alpha and pantry registered.
struct OneServer {
    child: Child,
    scratch: PathBuf,
    base: String,
}

impl OneServer {
    fn start(board: &str) -> Self {
        Self::launch(board, &[])
    }

    /// Both projects and `gamma`, whose documents are served at its root.
    fn with_root_project(board: &str) -> Self {
        Self::launch(board, &[("gamma", GAMMA)])
    }

    /// `project`'s folder.
    fn root(&self, project: &str) -> PathBuf {
        self.scratch.join(project)
    }

    fn launch(board: &str, more: &[(&str, &str)]) -> Self {
        // Each server its own folder: the cases of this binary run on threads side by side.
        static STARTED: AtomicUsize = AtomicUsize::new(0);
        let binary = binaries::shelf_server();
        let scratch = fresh(&std::env::temp_dir().join(format!(
            "symphony-browser-projects-{}-{}",
            std::process::id(),
            STARTED.fetch_add(1, Ordering::Relaxed)
        )));
        let machine = scratch.join("machine");
        let install = scratch.join("install");
        std::fs::create_dir_all(install.join("client")).unwrap();
        let copied = Command::new("cp")
            .arg("-R")
            .arg(dist())
            .arg(install.join("client/public"))
            .status()
            .unwrap();
        assert!(copied.success(), "copy the client: {copied}");
        // The tailnet worker of a server with a home asks `tailscale`: here it answers nothing.
        let bin = scratch.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink("/usr/bin/false", bin.join("tailscale")).unwrap();
        let path = format!("{}:/usr/bin:/bin", bin.display());
        for &(id, file) in [("alpha", ALPHA), ("pantry", PANTRY)].iter().chain(more) {
            let root = scratch.join(id);
            for (rel, text) in [
                ("symphony.toml", file),
                ("docs/DECISIONS.md", REGISTER),
                ("docs/SHELF.md", "# Shelf\n\n[[shelf:all]]\n"),
                // The Symphony's own place for the review guide; the home links it by its route.
                ("docs/REVIEW.md", "# Review\n\nHow a card is reviewed.\n"),
                ("docs/NOTES.md", NOTES),
                ("docs/dot.svg", DOT),
            ] {
                std::fs::create_dir_all(root.join(rel).parent().unwrap()).unwrap();
                std::fs::write(root.join(rel), text).unwrap();
            }
            git(&root);
            let added = Command::new(binary)
                .args(["project", "add"])
                .arg(&root)
                .env("SYMPHONY_MACHINE_DIR", &machine)
                .env("PATH", &path)
                .output()
                .unwrap();
            assert!(added.status.success(), "project add {id}: {added:?}");
        }
        let log = std::fs::File::create(scratch.join("server.log")).unwrap();
        let mut child = Command::new(binary)
            .args(["serve", "--home"])
            .arg(&machine)
            .arg("--root")
            .arg(scratch.join("alpha"))
            .arg("--install")
            .arg(&install)
            .args(["--port", "0", "--host", "127.0.0.1"])
            .env("PATH", &path)
            .env("SYMPHONY_MACHINE_DIR", &machine)
            .env("TASKS_SERVER", board)
            .env("TASKS_DATABASE", DATABASE)
            .env("TASKS_SWEEP", "off")
            .env("ASKS_DB", scratch.join("asks.db"))
            .env("SYMPHONY_TEST_PID", std::process::id().to_string())
            .env_remove("TASKS_SESSION")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(log)
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, recv) = std::sync::mpsc::channel();
        // A server with a home stamps each line of its output with the time.
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(url) = line
                    .split_once("→ Symphony at ")
                    .and_then(|(_, said)| said.split_whitespace().next())
                {
                    let _ = send.send(url.split('/').take(3).collect::<Vec<_>>().join("/"));
                }
            }
        });
        let mut server = Self {
            child,
            base: String::new(),
            scratch,
        };
        server.base = super::announced("the one server", &recv).unwrap_or_else(|why| {
            panic!(
                "{why}; {}",
                std::fs::read_to_string(server.scratch.join("server.log")).unwrap()
            )
        });
        server
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }
}

impl Drop for OneServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}

/// Where the one server the cases that write nothing share answers.
struct Shared(String);

impl Shared {
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.0)
    }
}

/// The shared server and its board, held until the binary's exit ends them.
static SHARED: Mutex<Option<(OneServer, ScratchBoard)>> = Mutex::new(None);

/// One board and one server of alpha and pantry for every case that writes nothing to either (what
/// such a case keeps is in its own browser's profile): made by the first that asks, once for this
/// run of the binary, and ended at its exit, its tests passed or not. A case that writes to the
/// board, a project's folder or the machine folder starts its own.
fn shared() -> &'static Shared {
    static SERVED: OnceLock<Shared> = OnceLock::new();
    SERVED.get_or_init(|| {
        let board = ScratchBoard::start();
        let server = OneServer::start(board.address());
        let served = Shared(server.base.clone());
        *SHARED.lock().unwrap() = Some((server, board));
        // SAFETY: atexit only records a function that takes nothing, to run once at the exit.
        assert_eq!(
            unsafe { libc::atexit(end_shared) },
            0,
            "the shared server's end is set"
        );
        served
    })
}

/// At the binary's exit: the shared server is stopped, then its board, and their folders go. A
/// binary that is killed runs none of this, and each then ends by its own leash (the server on its
/// test's process, the board on its input's end).
extern "C" fn end_shared() {
    if let Ok(mut shared) = SHARED.try_lock() {
        drop(shared.take());
    }
}

/// `folder` made empty and named as the server names it (macOS's `/private/var`).
fn fresh(folder: &Path) -> PathBuf {
    let _ = std::fs::remove_dir_all(folder);
    std::fs::create_dir_all(folder).unwrap();
    folder.canonicalize().unwrap()
}

fn git(root: &Path) {
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec!["config", "user.name", "Browser fixture"],
        vec!["config", "user.email", "browser@example.invalid"],
        vec!["config", "core.hooksPath", "/dev/null"],
        vec!["add", "."],
        vec![
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "Projects fixture",
        ],
    ] {
        let done = Command::new("git")
            .args(&args)
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .unwrap();
        assert!(done.status.success(), "fixture {args:?}: {done:?}");
    }
}

/// What the look of a page reads, its theme's parts and the rest.
const LOOK: &str = "(() => { const root = getComputedStyle(document.documentElement); \
    const body = getComputedStyle(document.body); \
    const hero = getComputedStyle(document.querySelector('.hero-title')); \
    return {primary: root.getPropertyValue('--primary').trim(), \
        layouts: Object.assign({}, document.documentElement.dataset), \
        background: body.backgroundColor, color: body.color, font: hero.fontFamily, \
        size: hero.fontSize}; })()";

/// Open the page's quick composer and wait for its text field.
fn quick_text(browser: &mut Chrome) {
    assert!(browser.wait_for("document.querySelector('#quick-open') !== null", SHOWN));
    browser.eval("document.querySelector('#quick-open').click()");
    assert!(browser.wait_for("document.querySelector('#quick-text') !== null", SHOWN));
}

/// The visible switch leaves the project's shell and opens the machine picker.
fn change_board(browser: &mut Chrome) {
    assert!(
        browser.wait_for(
            "document.querySelector('header a.project-switch') !== null",
            SHOWN
        ),
        "switch control missing: {}",
        observed_page(browser)
    );
    browser.eval("document.querySelector('header a.project-switch').click()");
    wait_for_picker(browser);
}

fn wait_for_picker(browser: &mut Chrome) {
    assert!(
        browser.wait_for(
            "location.pathname === '/' && document.querySelector('h1')?.textContent === 'Projects'",
            SHOWN
        ),
        "project picker missing: {}",
        observed_page(browser)
    );
    assert_eq!(
        browser.eval(
            "Array.from(document.querySelectorAll('.projects a')).map(a => a.getAttribute('href'))"
        ),
        json!(["/alpha/", "/pantry/"])
    );
}

fn observed_page(browser: &mut Chrome) -> Value {
    browser.eval("({url:location.href, body:document.body?.innerText.slice(0, 2000)})")
}

/// The link the keyboard is on: its text and destination, whether it is the page header's first
/// link, and whether it shows its focus ring wholly inside the viewport.
fn focused(browser: &mut Chrome) -> Value {
    browser.eval(
        r#"(() => {
        const link = document.activeElement;
        const box = link.getBoundingClientRect();
        const first = document.querySelector('header a');
        return {name:link.textContent.trim(), destination:link.getAttribute('href'),
            first:link === first, focused:link.matches(':focus-visible'),
            visible:box.width > 0 && box.height > 0 && box.left >= 0 && box.right <= innerWidth && box.top >= 0 && box.bottom <= innerHeight};
    })()"#,
    )
}

#[test]
fn a_draft_and_a_theme_one_project_keeps_never_appear_in_another() {
    let server = shared();
    let Some(mut browser) = chrome() else { return };
    browser.open(&server.url("/alpha/"));
    browser.eval(
        "localStorage.setItem('alpha:studio.cssVars', ':root[data-studio-theme]{--radius:23px;}')",
    );
    quick_text(&mut browser);
    browser.eval("(() => { const field = document.querySelector('#quick-text'); field.value = 'Alpha private draft'; field.dispatchEvent(new Event('input', {bubbles: true})); return true; })()");
    assert!(
        browser.wait_for("JSON.parse(localStorage.getItem('alpha:review-quick-draft') || '{}').text === 'Alpha private draft'", SHOWN),
        "alpha keeps its draft under its own key: {}",
        browser.eval("JSON.stringify(localStorage)")
    );
    assert_eq!(
        browser.eval("localStorage.getItem('review-quick-draft')"),
        Value::Null
    );
    browser.open(&server.url("/alpha/"));
    assert!(
        browser.wait_for("document.querySelector('#studio-theme') !== null", SHOWN),
        "alpha's saved theme applies to alpha"
    );
    // An old valid cookie cannot trap the switch in Alpha, including after a root revisit.
    browser.eval("document.cookie = 'symphony-project=alpha; Path=/'");
    change_board(&mut browser);
    browser.eval("document.querySelector('.projects a[href=\"/pantry/\"]').click()");
    quick_text(&mut browser);
    assert_eq!(
        browser.eval("({draft: document.querySelector('#quick-text').value, theme: document.querySelector('#studio-theme') !== null})"),
        json!({"draft": "", "theme": false}),
        "pantry shows nothing alpha saved"
    );
    browser.open(&server.url("/"));
    wait_for_picker(&mut browser);
    browser.eval("document.querySelector('.projects a[href=\"/alpha/\"]').click()");
    quick_text(&mut browser);
    assert_eq!(
        browser.eval("({draft: document.querySelector('#quick-text').value, radius: getComputedStyle(document.documentElement).getPropertyValue('--radius').trim()})"),
        json!({"draft": "Alpha private draft", "radius": "23px"}),
        "returning through the picker restores Alpha's own draft and saved theme"
    );
}

#[test]
fn the_top_left_switch_and_picker_work_by_keyboard_on_a_compact_nested_page() {
    use super::components::key;

    let server = shared();
    let Some(mut browser) = chrome() else { return };
    browser.call(
        "Emulation.setDeviceMetricsOverride",
        json!({
            "width":390,"height":844,"deviceScaleFactor":1,"mobile":true
        }),
    );
    browser.open(&server.url("/alpha/tasks"));
    assert!(browser.wait_for("document.documentElement.dataset.navigation === 'ready' && document.querySelector('header a.project-switch') !== null", SHOWN), "nested page missing: {}", observed_page(&mut browser));
    key(&mut browser, "Tab", "Tab", 9);
    assert_eq!(
        focused(&mut browser),
        json!({"name":"Projects","destination":"/","first":true,"focused":true,"visible":true})
    );
    key(&mut browser, "Enter", "Enter", 13);
    wait_for_picker(&mut browser);
    // The picker is a machine page in the app's frame: its header's link to the project list,
    // marked as the page shown, is the first stop, under the same name as the projects' switch,
    // then each project in the list's order.
    for (name, destination, first) in [
        ("Projects", "/", true),
        ("Alpha", "/alpha/", false),
        ("The Shelf", "/pantry/", false),
    ] {
        key(&mut browser, "Tab", "Tab", 9);
        assert_eq!(
            focused(&mut browser),
            json!({"name":name,"destination":destination,"first":first,"focused":true,"visible":true}),
            "the picker's tab order"
        );
    }
    assert_eq!(
        browser.eval("document.querySelector('header a[href=\"/\"]').getAttribute('aria-current')"),
        json!("page"),
        "the picker's own link is lit"
    );
    key(&mut browser, "Enter", "Enter", 13);
    assert!(
        browser.wait_for(
            "location.pathname === '/pantry/' && document.querySelector('.hero-title') !== null",
            SHOWN
        ),
        "selected project missing: {}",
        observed_page(&mut browser)
    );
    browser.eval("document.querySelector('header a[href=\"/pantry/tasks\"]').click()");
    assert!(browser.wait_for("location.pathname === '/pantry/tasks' && document.documentElement.dataset.navigation === 'ready'", SHOWN), "project tasks missing: {}", observed_page(&mut browser));
    // The Home link stays separate from the machine picker, even in the compact theme.
    browser.eval("document.querySelector('header a[href=\"/pantry/\"]').click()");
    assert!(
        browser.wait_for(
            "location.pathname === '/pantry/' && document.querySelector('.hero-title') !== null",
            SHOWN
        ),
        "project home missing: {}",
        observed_page(&mut browser)
    );
    change_board(&mut browser);
    browser.open(&server.url("/"));
    wait_for_picker(&mut browser);
}

/// Each project of the one server, the primary token its theme sets and a layout it sets, as the
/// page's dataset names it.
const THEMES: [(&str, &str, (&str, &str)); 2] = [
    ("alpha", "#7a2e8f", ("layoutHome", "focus")),
    ("pantry", "#2a6f4e", ("layoutHeader", "compact")),
];

/// What a page's theme sets: its primary token and its layouts.
const THEME: &str = "({primary: getComputedStyle(document.documentElement)\
    .getPropertyValue('--primary').trim(), layouts: Object.assign({}, document.documentElement.dataset)})";

#[test]
fn each_project_fits_the_first_visit_budget_in_its_own_theme_and_the_shared_look() {
    let board = ScratchBoard::start();
    // Initialize the scratch database before the server warms its board snapshot.
    let actor = board.call_in_database(DATABASE, "register", json!({"kind":"worker"}));
    let server = OneServer::start(board.address());
    let mut looks = Vec::new();
    for (id, primary, layout) in THEMES {
        let Some(mut browser) = chrome() else { return };
        browser.open(&server.url(&format!("/{id}/")));
        assert!(
            browser.wait_for("document.querySelector('.hero-title') !== null", SHOWN),
            "{id} did not show its home: {:?}",
            browser.eval("document.body.innerText")
        );
        let requests = completed_requests(&mut browser);
        let bytes = needed_bytes(&requests);
        eprintln!(
            "/{id}/ first visit: {bytes} bytes; keep-alive pings beside it: {} bytes",
            ping_bytes(&requests)
        );
        assert!(
            bytes <= HOME_FIRST_VISIT_BYTES,
            "/{id}/ first visit loads {bytes} bytes, over {HOME_FIRST_VISIT_BYTES}: {requests:#?}"
        );
        let look = browser.eval(LOOK);
        assert_eq!(look["primary"], json!(primary), "{id}: {look}");
        assert_eq!(look["layouts"][layout.0], json!(layout.1), "{id}: {look}");
        looks.push(look);
        // The header's links are the project's routes, written and followed under its own `/<id>/`.
        browser.eval(&format!(
            "document.querySelector('header a[href=\"/{id}/tasks\"]').click()"
        ));
        assert!(
            browser.wait_for(
                &format!("location.pathname === {}", json!(format!("/{id}/tasks"))),
                SHOWN
            ),
            "{id}'s tasks link went to {:?}",
            browser.eval("location.pathname")
        );
    }
    // Past its theme, a project looks as every other does.
    let shared =
        |look: &Value| ["background", "color", "font", "size"].map(|part| look[part].clone());
    assert_eq!(shared(&looks[0]), shared(&looks[1]));

    // Each project's tasks board, the other entry page, is a first visit of its own too, in a
    // browser of its own and in its own theme. On a phone Review opens first; choose Ready to
    // draw the seeded card's title, counting both stages' requests in this cold visit.
    board.call_in_database(
        DATABASE,
        "create",
        json!({"slug":"budget/card","title":"A card on the board","by":actor["handle"]}),
    );
    for (id, primary, layout) in THEMES {
        let Some(mut browser) = chrome() else { return };
        browser.call(
            "Emulation.setDeviceMetricsOverride",
            json!({"width":390,"height":844,"deviceScaleFactor":1,"mobile":true}),
        );
        let page = format!("/{id}/tasks");
        browser.open(&server.url(&page));
        assert!(
            browser.wait_for(
                &ready(".waiting-columns .column[data-status=review]"),
                SHOWN
            ),
            "{page} did not open Review: {}",
            observed_page(&mut browser)
        );
        browser.eval("[...document.querySelectorAll('.stage-tabs button')].find(button => button.textContent.startsWith('Ready')).click()");
        assert!(
            browser.wait_for(
                &format!(
                    "document.querySelector({}) !== null",
                    json!(format!(".taskcard a[href='/{id}/task/budget/card']"))
                ),
                SHOWN
            ),
            "{page} did not draw its card: {:?}",
            browser.eval("document.body.innerText")
        );
        let requests = completed_requests(&mut browser);
        let bytes = needed_bytes(&requests);
        eprintln!(
            "{page} first visit: {bytes} bytes; keep-alive pings beside it: {} bytes",
            ping_bytes(&requests)
        );
        assert!(
            bytes <= TASKS_FIRST_VISIT_BYTES,
            "{page} first visit loads {bytes} bytes, over {TASKS_FIRST_VISIT_BYTES}: {requests:#?}"
        );
        let theme = browser.eval(THEME);
        assert_eq!(theme["primary"], json!(primary), "{page}: {theme}");
        assert_eq!(
            theme["layouts"][layout.0],
            json!(layout.1),
            "{page}: {theme}"
        );
    }

    // The server's own pages, the picker and what an unknown project answers, stand in the
    // column a project's pages stand in (the queue's: the board alone takes the window's width),
    // their titles at the same size.
    let Some(mut browser) = chrome() else { return };
    browser.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":1600,"height":900,"deviceScaleFactor":1,"mobile":false}),
    );
    browser.open(&server.url("/alpha/tasks"));
    assert!(
        browser.wait_for(
            "document.querySelector('main .page-head h1') !== null && document.querySelector('main[data-ready=board] > .waiting-board > .kanban.waiting-columns') !== null",
            SHOWN
        ),
        "/alpha/tasks drew no board: {:?}",
        browser.eval("document.body.innerText")
    );
    let board = browser.eval(COLUMN);
    let window = browser.eval(&format!("Math.round({PAGE_WIDTH})"));
    assert_eq!(
        board["width"], window,
        "the board's column fills the window: {board}"
    );
    browser.open(&server.url("/alpha/queue"));
    assert!(
        browser.wait_for(
            "document.querySelector('main [data-ready=queue] .page-head h1') !== null",
            SHOWN
        ),
        "/alpha/queue drew no title: {:?}",
        browser.eval("document.body.innerText")
    );
    let project = browser.eval(COLUMN);
    assert_eq!(
        project,
        json!({"width": 1488, "padding": "24px", "title": board["title"]}),
        "the project's column"
    );
    for page in ["/", "/nope/"] {
        browser.open(&server.url(page));
        assert!(browser.wait_for(
            "document.querySelector('main.server-page h1') !== null",
            SHOWN
        ));
        assert_eq!(browser.eval(COLUMN), project, "{page}");
    }
}

/// The page's column: its main's width and padding, and its title's size.
const COLUMN: &str = "(() => {const main=document.querySelector('main'),look=getComputedStyle(main);\
    return {width:Math.round(main.getBoundingClientRect().width),padding:look.paddingLeft,\
    title:getComputedStyle(main.querySelector('h1')).fontSize};})()";

/// Where each link of `selector` on the page leads, as `[href, status]`: an in-page anchor is
/// 200 when its element is on the page; a document or folder route is what its read answers,
/// since the server sends any route its page; any other link is what the server answers at its
/// address, the project's `base` written in it.
fn landings(browser: &mut Chrome, base: &str, selector: &str) -> Vec<(String, u64)> {
    let found = browser.eval(&format!(
        r#"(async () => {{
            const base = {base}, seen = [];
            for (const link of document.querySelectorAll({selector})) {{
                const href = link.getAttribute('href');
                if (seen.some(([h]) => h === href)) continue;
                if (href.startsWith('#')) {{ seen.push([href, document.getElementById(href.slice(1)) ? 200 : 404]); continue; }}
                const path = href.split('#')[0];
                const route = (path.startsWith(`${{base}}/`) ? path.slice(base.length) : path).slice(1).replace(/\/$/, '');
                const probe = link.dataset.native ? path
                    : path.endsWith('.md') ? `${{base}}/api/document?view=outline&path=${{encodeURIComponent(route)}}`
                    : path.length > 1 && path.endsWith('/') && route ? `${{base}}/api/folder?view=outline&path=${{encodeURIComponent(route)}}`
                    : path;
                seen.push([href, (await fetch(probe)).status]);
            }}
            return seen;
        }})()"#,
        base = json!(base),
        selector = json!(selector),
    ));
    serde_json::from_value(found).unwrap()
}

#[test]
fn every_header_and_home_link_lands_in_a_project_whose_guide_is_docs_review() {
    let server = shared();
    let Some(mut browser) = chrome() else { return };
    browser.open(&server.url("/alpha/"));
    assert!(
        browser.wait_for("document.querySelector('.hero-title') !== null", SHOWN),
        "{}",
        observed_page(&mut browser)
    );
    let links = landings(&mut browser, "/alpha", "header a[href], .dash a[href]");
    assert!(
        links
            .iter()
            .any(|(href, _)| href == "/alpha/docs/REVIEW.md"),
        "the home links the project's own review guide: {links:?}"
    );
    let lost: Vec<_> = links.iter().filter(|(_, status)| *status != 200).collect();
    assert!(
        lost.is_empty(),
        "links that lead nowhere: {lost:?} of {links:?}"
    );
}

/// The page's links and pictures on this server as a new tab, a copy or a long-press opens them
/// (`a.href`, `img.src`, the address resolved against the page): each by its path and fragment,
/// and, apart, the markup of those whose address leaves the project at `base` without being its
/// switch to the machine's list. An address on another server is neither.
fn addresses(browser: &mut Chrome, base: &str) -> (Vec<String>, Vec<String>) {
    let found = browser.eval(&format!(
        r#"(() => {{
            const base = {base}, here = [], strays = [];
            for (const link of document.querySelectorAll('a[href], img[src]')) {{
                const url = new URL(link.href ?? link.src);
                if (url.origin !== location.origin) continue;
                here.push(url.pathname + url.hash);
                const ours = url.pathname === base || url.pathname.startsWith(`${{base}}/`);
                const machine = link.classList.contains('project-switch') && url.pathname === '/';
                if (!ours && !machine) strays.push(link.outerHTML.slice(0, 160));
            }}
            return [here, strays];
        }})()"#,
        base = json!(base),
    ));
    serde_json::from_value(found).unwrap()
}

/// The page has finished its navigation and drawn `drawn`.
fn ready(drawn: &str) -> String {
    format!(
        "document.documentElement.dataset.navigation === 'ready' && document.querySelector({}) !== null",
        json!(drawn)
    )
}

/// Once `page` has drawn `drawn`, no link or picture on it leaves the project at `base` (but the
/// switch to the machine's list), and each of `named` is among their addresses.
fn links_stay(browser: &mut Chrome, page: &str, base: &str, drawn: &str, named: &[&str]) {
    assert!(
        browser.wait_for(&ready(drawn), SHOWN),
        "{page} did not draw {drawn}: {}",
        observed_page(browser)
    );
    let (here, strays) = addresses(browser, base);
    assert!(
        strays.is_empty(),
        "{page}: addresses that open another project's page: {strays:#?}"
    );
    for name in named {
        assert!(
            here.iter().any(|path| path == name),
            "{page} names {name}: {here:#?}"
        );
    }
}

/// What a link on each kind of page names is the project's page a click on it shows, wherever it
/// is opened: every link the client draws on the home, the tasks board, a task, a document and
/// the queue carries the project's `/<id>/`, as does a document's picture, but the switch to the
/// machine's list and the pages of other servers. A click on one is still followed in place,
/// under the base once.
#[test]
fn every_link_of_a_projects_pages_names_its_page_under_the_projects_base() {
    let board = ScratchBoard::start();
    // Initialize the database before the server warms its board snapshot.
    let actor = board.call_in_database(DATABASE, "register", json!({"kind":"worker"}));
    let server = OneServer::start(board.address());
    let by = &actor["handle"];
    for task in [
        json!({"slug":"links/first","title":"First","by":by}),
        json!({"slug":"links/parent","title":"Parent","by":by,"links":["docs/NOTES.md"]}),
        json!({"slug":"links/child","title":"Child","by":by,"parent":"links/parent",
            "depends":["links/first"],"links":["docs/NOTES.md"],
            "body":"Read [the notes](/docs/NOTES.md) first."}),
    ] {
        board.call_in_database(DATABASE, "create", task);
    }
    let Some(mut browser) = chrome() else { return };
    // This case checks every stage's card links and the machine rail together. A phone mounts
    // Review first, while the seeded parent is Ready and its rail opens as a separate list.
    browser.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":1600,"height":900,"deviceScaleFactor":1,"mobile":false}),
    );
    for (page, drawn, named) in [
        (
            "/alpha/",
            ".hero-title",
            &[
                "/alpha/",
                "/alpha/tasks",
                "/alpha/queue",
                "/alpha/docs/DECISIONS.md#first",
            ][..],
        ),
        (
            "/alpha/tasks",
            ".taskcard a[href$='/task/links/parent']",
            &[
                "/alpha/task/links/parent",
                "/alpha/agents",
                // No machine reports alpha's builds: the Build and Test rail's link.
                "/alpha/agents#machine-context",
            ],
        ),
        (
            "/alpha/task/links/child",
            ".task .facts a[href$='/task/links/first']",
            &[
                "/alpha/task/links/parent",
                "/alpha/task/links/first",
                "/alpha/docs/NOTES.md",
            ],
        ),
        (
            "/alpha/docs/NOTES.md",
            "[data-ready=document]:has(.doc-tasks a[href$='/task/links/child']):has(p a[href$='DECISIONS.md']):has(img[src*='dot.svg'])",
            &[
                "/alpha/",
                "/alpha/docs/",
                "/alpha/docs/DECISIONS.md",
                "/alpha/docs/NOTES.md#notes",
                "/alpha/task/links/child",
                "/alpha/docs/dot.svg",
            ],
        ),
        (
            "/alpha/queue",
            "[data-ready=queue] .queue-list a[href]",
            &["/alpha/", "/alpha/docs/DECISIONS.md#first"],
        ),
    ] {
        browser.open(&server.url(page));
        links_stay(&mut browser, page, "/alpha", drawn, named);
    }
    // A click on a written link is followed in place, at its address as written: from the queue
    // to the tasks board, and from there to a task.
    browser.eval(
        "window.keptPage = true; document.querySelector(\"header a[href='/alpha/tasks']\").click()",
    );
    assert!(
        browser.wait_for(
            &ready(".taskcard a[href='/alpha/task/links/parent']"),
            SHOWN
        ),
        "{}",
        observed_page(&mut browser)
    );
    browser
        .eval("document.querySelector(\".taskcard a[href='/alpha/task/links/parent']\").click()");
    assert!(
        browser.wait_for(&ready("[id='subtasks-links/parent']"), SHOWN),
        "{}",
        observed_page(&mut browser)
    );
    assert_eq!(
        browser.eval("({path: location.pathname, kept: window.keptPage === true})"),
        json!({"path": "/alpha/task/links/parent", "kept": true}),
        "the shell follows the link in place, adding nothing to its address"
    );
    browser.eval("document.querySelector(\"[id='subtasks-links/parent']\").scrollIntoView()");
    assert!(
        browser.wait_for(
            "document.querySelector(\"[id='child-links/child'] a[href]\") !== null",
            SHOWN
        ),
        "{}",
        observed_page(&mut browser)
    );
    let (here, strays) = addresses(&mut browser, "/alpha");
    assert!(strays.is_empty(), "a parent task's page: {strays:#?}");
    assert!(
        here.iter().any(|path| path == "/alpha/task/links/child"),
        "{here:#?}"
    );
    // The approved board has no document chips. Its linked parent and child pages each keep
    // the document link's prefix coverage instead of expecting that retired board markup.
    assert!(
        here.iter().any(|path| path == "/alpha/docs/NOTES.md"),
        "{here:#?}"
    );
}

/// The `href` of the first element `selector` names.
fn href_of(browser: &mut Chrome, selector: &str) -> Value {
    browser.eval(&format!(
        "document.querySelector({}).getAttribute('href')",
        json!(selector)
    ))
}

/// The rest of a project's pages write their links under its base too: the agents page, the toast
/// and the activity panel that show an agent's reply, a folder, another project's document and its
/// picture, a plugin's page and a missing task's way back. A copied picture address, opened with
/// no page to refer it, loads the picture.
#[test]
fn the_agents_activity_folder_plugin_and_missing_pages_name_their_links_under_the_base() {
    let board = ScratchBoard::start();
    let server = OneServer::start(board.address());
    let actor = board.call_in_database(DATABASE, "register", json!({"kind":"worker"}));
    let by = &actor["handle"];
    board.call_in_database(
        DATABASE,
        "create",
        json!({"slug":"links/held","title":"Held","by":by}),
    );
    board.call_in_database(DATABASE, "claim", json!({"slug":"links/held","handle":by}));
    let Some(mut browser) = chrome() else { return };
    browser.open(&server.url("/alpha/agents"));
    links_stay(
        &mut browser,
        "/alpha/agents",
        "/alpha",
        ".agents-now a[href$='/task/links/held']",
        &["/alpha/task/links/held"],
    );
    // An agent's reply under the register's card, written as `symphony reply` writes it, arrives
    // as a toast; the bell lists it.
    std::fs::write(
        server.root("alpha").join("docs/DECISIONS.md"),
        format!("{REGISTER}  [2026-11-11 09:30] agent — An agent's answer.\n"),
    )
    .unwrap();
    let card = "/alpha/docs/DECISIONS.md#L3";
    links_stay(&mut browser, "a toast", "/alpha", "#toasts a[href]", &[]);
    assert_eq!(href_of(&mut browser, "#toasts a[href]"), json!(card));
    browser.eval("document.getElementById('bell').click()");
    links_stay(
        &mut browser,
        "the activity panel",
        "/alpha",
        "#activity-panel li a[href]",
        &[],
    );
    assert_eq!(
        href_of(&mut browser, "#activity-panel li a[href]"),
        json!(card)
    );
    for (page, base, drawn, named) in [
        (
            "/alpha/docs/",
            "/alpha",
            "[data-ready=folder] a[href$='/docs/NOTES.md']",
            &["/alpha/docs/NOTES.md", "/alpha/docs/DECISIONS.md"][..],
        ),
        (
            "/pantry/shelf",
            "/pantry",
            "[data-ready=document]",
            &["/pantry/shelf", "/pantry/tasks"],
        ),
        (
            "/alpha/task/nope",
            "/alpha",
            ".trouble a[href]",
            &["/alpha/tasks"],
        ),
        (
            "/pantry/docs/NOTES.md",
            "/pantry",
            "[data-ready=document] img[src*='dot.svg']",
            &["/pantry/docs/DECISIONS.md", "/pantry/docs/dot.svg"],
        ),
    ] {
        browser.open(&server.url(page));
        links_stay(&mut browser, page, base, drawn, named);
    }
    // Pantry is not the server's starter, where an address without its base would land: the
    // picture's own address names pantry.
    assert_eq!(
        browser.eval(
            "fetch(document.querySelector(\"[data-ready=document] img[src*='dot.svg']\").getAttribute('src'), \
             {referrerPolicy: 'no-referrer', redirect: 'manual'}) \
             .then(r => [r.status, r.headers.get('content-type')])"
        ),
        json!([200, "image/svg+xml"]),
        "a copied picture address loads with no page to refer it"
    );
}

/// A task's gates write their evidence's link under the project's base too (the server answers
/// it as a path of the app, `/docs/DECISIONS.md#first`): copied or opened in a new tab it names the
/// project's page, and a click on it is followed in place, under the base once. The task posted
/// the register's card `first`, so its owner gate holds on it.
#[test]
fn a_tasks_gate_links_its_evidence_under_the_projects_base_and_a_click_stays_in_the_project() {
    let board = ScratchBoard::start();
    let server = OneServer::start(board.address());
    let actor = board.call_in_database(DATABASE, "register", json!({"kind":"worker"}));
    board.call_in_database(
        DATABASE,
        "create",
        json!({"slug":"links/gated","title":"Gated","by":actor["handle"],
            "links":["docs/DECISIONS.md"],"body":"Posted first for the owner."}),
    );
    let Some(mut browser) = chrome() else { return };
    browser.open(&server.url("/alpha/task/links/gated"));
    let gate = ".task-gates li.gate[data-gate=owner] .gate-detail a[href]";
    links_stay(
        &mut browser,
        "/alpha/task/links/gated",
        "/alpha",
        gate,
        &["/alpha/docs/DECISIONS.md#first"],
    );
    assert_eq!(
        (
            href_of(&mut browser, gate),
            browser.eval(&format!(
                "document.querySelector({}).textContent",
                json!(gate)
            ))
        ),
        (
            json!("/alpha/docs/DECISIONS.md#first"),
            json!("docs/DECISIONS.md")
        ),
        "the gate's link, as written and on the words that are its evidence"
    );
    browser.eval(&format!(
        "window.keptPage = true; document.querySelector({}).click()",
        json!(gate)
    ));
    assert!(
        browser.wait_for(&ready("[data-ready=document]"), SHOWN),
        "{}",
        observed_page(&mut browser)
    );
    assert_eq!(
        browser.eval(
            "({path: location.pathname, hash: location.hash, kept: window.keptPage === true})"
        ),
        json!({"path": "/alpha/docs/DECISIONS.md", "hash": "#first", "kept": true}),
        "the shell follows the gate's link in place, adding nothing to its address"
    );
}

/// From a project whose documents are served at its root, a click on another project's link loads
/// that project: its address is not under the page's base, so the shell leaves it to the browser,
/// where it once took it for a folder of its own (`/gamma/pantry/tasks`, "Folder not found").
#[test]
fn a_click_on_another_projects_link_from_one_served_at_its_root_loads_that_project() {
    let board = ScratchBoard::start();
    let server = OneServer::with_root_project(board.address());
    let Some(mut browser) = chrome() else { return };
    browser.open(&server.url("/gamma/tasks"));
    links_stay(
        &mut browser,
        "/gamma/tasks",
        "/gamma",
        "main[data-ready=board]",
        &["/gamma/tasks"],
    );
    browser.eval(
        "(() => { window.keptPage = true; const link = document.createElement('a'); \
         link.href = '/pantry/tasks'; link.textContent = 'Pantry tasks'; \
         document.querySelector('main').append(link); link.click(); })()",
    );
    assert!(
        browser.wait_for(
            "location.pathname === '/pantry/tasks' && document.documentElement.dataset.navigation === 'ready'",
            SHOWN
        ),
        "{}",
        observed_page(&mut browser)
    );
    assert_eq!(
        browser.eval(
            "({kept: window.keptPage === true, \
              base: JSON.parse(document.getElementById('symphony-project').textContent).base})"
        ),
        json!({"kept": false, "base": "/pantry"}),
        "the browser loaded pantry's own page"
    );
}

/// A project whose theme has a name and an icon of its own (a picture each fixture project
/// holds).
const LANTERN: &str = "[project]\nname = \"Lantern\"\nid = \"lantern\"\n\
                       [theme]\nname = \"Lantern Works\"\nicon = \"docs/dot.svg\"\n";

/// What the page's header opens with and the icons its head links: the mark's element, the
/// file a theme's own is read from, whether that address is the tab's icon's, version and all
/// (the one its answer is kept for good under), whether it is the mark drawn in the page's
/// primary, the name with its class and whether it is set in the wordmark's serif, whether the
/// tab's icon is the Symphony's fermata inline with both schemes' colours, and the home screen's
/// icon with the type its address answers.
const BRAND: &str = r#"(async () => {
    const brand = document.querySelector('.app-header').firstElementChild;
    const mark = brand.querySelector('.app-mark'), name = brand.querySelector('span');
    const probe = document.body.appendChild(document.createElement('i'));
    probe.style.color = 'var(--primary)';
    const primary = getComputedStyle(probe).color;
    probe.remove();
    const box = mark.getBoundingClientRect();
    const icon = document.querySelector('link[rel=icon]').getAttribute('href');
    const touch = document.querySelector('link[rel=apple-touch-icon]')?.getAttribute('href') ?? null;
    const src = mark.getAttribute('src');
    return {
        brand: brand.className, link: brand.closest('a') !== null || brand.querySelector('a') !== null,
        mark: mark.tagName.toLowerCase(), src: src === null ? null : src.split('?')[0],
        tabs: src !== null && src === icon && /\?v=[0-9a-f]{12}$/.test(src),
        drawn: box.width > 0 && box.height > 0,
        inPrimary: mark.tagName === 'svg' && getComputedStyle(mark).fill === primary,
        name: name.textContent, named: name.className,
        serif: getComputedStyle(name).fontFamily.includes('Iowan Old Style'),
        fermata: icon.startsWith('data:image/svg+xml,')
            && decodeURIComponent(icon).includes('M5 21A11 11 0 0 1 27 21H24.6A8.6 7.2 0 0 0 7.4 21Z')
            && decodeURIComponent(icon).includes('prefers-color-scheme:dark'),
        touch, touchType: touch === null ? null : (await fetch(touch)).headers.get('content-type'),
    };
})()"#;

/// The header opens with the Symphony's mark and wordmark on the machine's own pages and on a
/// project whose theme has no name or icon of its own, the mark in that project's primary colour;
/// a theme's name takes the wordmark's place, and a theme's icon the mark's, on the page the
/// client draws and on the one the server draws. A page with no icon of its theme's links the
/// Symphony's, inline for the tab and by its address for a phone's home screen; the icon the
/// address answers by itself is the Symphony's. On a phone the first row stays one row, from
/// 320 wide up.
#[test]
fn the_header_and_the_icons_are_the_symphonys_until_a_theme_has_its_own() {
    let board = ScratchBoard::start();
    let server = OneServer::launch(board.address(), &[("lantern", LANTERN)]);
    let Some(mut browser) = chrome() else { return };
    let symphonys = |name: &str, named: &str, touch: &str| {
        json!({
            "brand":"app-brand","link":false,"mark":"svg","src":null,"tabs":false,"drawn":true,
            "inPrimary":true,
            "name":name,"named":named,"serif":named == "app-wordmark","fermata":true,
            "touch":touch,"touchType":"image/png"
        })
    };
    let own = json!({
        "brand":"app-brand","link":false,"mark":"img","src":"/lantern/docs/dot.svg","tabs":true,
        "drawn":true,"inPrimary":false,"name":"Lantern Works","named":"app-name","serif":false,
        "fermata":false,"touch":null,"touchType":null
    });
    let client = "document.documentElement.dataset.navigation === 'ready' && document.querySelector('.hero-title') !== null";
    let served = "document.querySelector('main.server-page') !== null";
    for (page, ready, want) in [
        // The machine's own pages: the list, and what no project answers.
        (
            "/",
            served,
            symphonys("Symphony", "app-wordmark", "/favicon.ico?size=180"),
        ),
        (
            "/nope/",
            served,
            symphonys("Symphony", "app-wordmark", "/favicon.ico?size=180"),
        ),
        // A project whose theme sets a colour alone, on a page of the client's and of the server's.
        (
            "/alpha/",
            client,
            symphonys("Symphony", "app-wordmark", "/alpha/favicon.ico?size=180"),
        ),
        (
            "/alpha/no/such/page",
            served,
            symphonys("Symphony", "app-wordmark", "/alpha/favicon.ico?size=180"),
        ),
        // A theme package's name beside the Symphony's mark.
        (
            "/pantry/",
            client,
            symphonys("The Shelf", "app-name", "/pantry/favicon.ico?size=180"),
        ),
        // A theme's own name and icon, and nothing of the Symphony's.
        ("/lantern/", client, own.clone()),
        ("/lantern/no/such/page", served, own.clone()),
    ] {
        browser.open(&server.url(page));
        assert!(
            browser.wait_for(
                &format!(
                    "{ready} && (m => m !== null && (m.tagName !== 'IMG' || (m.complete && m.naturalWidth > 0)))\
                     (document.querySelector('.app-header > .app-brand > .app-mark'))"
                ),
                SHOWN
            ),
            "{page} drew no mark: {}",
            observed_page(&mut browser)
        );
        assert_eq!(browser.eval(BRAND), want, "{page}");
    }
    // Alpha's mark is drawn in alpha's own primary, not the Symphony's blue.
    browser.open(&server.url("/alpha/"));
    assert!(browser.wait_for(client, SHOWN));
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.app-brand .app-mark')).fill"),
        json!("rgb(122, 46, 143)")
    );
    // The address's own icon, asked for from the list, is the Symphony's.
    browser.open(&server.url("/"));
    assert!(browser.wait_for(served, SHOWN));
    assert_eq!(
        browser.eval(
            "fetch('/favicon.ico').then(async answer => ({type: answer.headers.get('content-type'), \
             fermata: (await answer.text()).includes('M5 21A11 11 0 0 1 27 21H24.6A8.6 7.2 0 0 0 7.4 21Z')}))"
        ),
        json!({"type":"image/svg+xml","fermata":true})
    );
    // On a phone the mark and the name, the switch and the two buttons share the first row, the
    // pages under them, at every width from the suite's narrowest phone up: two rows and never
    // more. The name shows whole down to 320 wide, where the bell is drawn alone to leave it the
    // room, a name longer than the row's room on a second line of it, the mark whole beside it.
    for (width, id, cut) in [
        (320, "alpha", Some(false)),
        (320, "lantern", Some(false)),
        (360, "alpha", Some(false)),
        (360, "lantern", Some(false)),
        (375, "alpha", Some(false)),
        (375, "lantern", Some(false)),
        (390, "alpha", Some(false)),
        (390, "lantern", Some(false)),
    ] {
        browser.call(
            "Emulation.setDeviceMetricsOverride",
            json!({"width":width,"height":844,"deviceScaleFactor":1,"mobile":true}),
        );
        browser.open(&server.url(&format!("/{id}/")));
        assert!(browser.wait_for(client, SHOWN), "{id} at {width}");
        let seen = browser.eval(
            "(name => name.scrollWidth > name.clientWidth || name.scrollHeight > name.clientHeight + 1)(document.querySelector('.app-brand span'))",
        );
        if let Some(cut) = cut {
            assert_eq!(seen, json!(cut), "{id} at {width}: the name cut");
        }
        assert_eq!(
            browser.eval(
                r#"(() => {
                    const box = e => e.getBoundingClientRect();
                    const mid = e => (box(e).top + box(e).bottom) / 2;
                    const first = ['.app-brand', '.project-switch', '#appearance', '#bell'].map(s => document.querySelector(s));
                    const name = document.querySelector('.app-brand span');
                    return {
                        oneRow: first.every(e => Math.abs(mid(e) - mid(first[0])) < 2),
                        inOrder: first.every((e, at) => at === 0 || box(e).left >= box(first[at - 1]).right),
                        pagesBelow: box(document.querySelector('.app-pages')).top >= box(first[3]).bottom,
                        headerUnder100: box(document.querySelector('.app-header')).height < 100,
                        markWhole: box(document.querySelector('.app-brand .app-mark')).width >= 20,
                        nameShows: name.clientWidth >= 16,
                        fits: document.documentElement.scrollWidth <= innerWidth,
                    };
                })()"#
            ),
            json!({"oneRow":true,"inOrder":true,"pagesBelow":true,"headerUnder100":true,"markWhole":true,"nameShows":true,"fits":true}),
            "{id} at {width}"
        );
    }
}

/// Projects share an origin, but the phone's board stage is the project's own saved choice.
#[test]
fn a_phone_keeps_its_selected_board_stage_only_for_that_project() {
    let board = ScratchBoard::start();
    board.call_in_database(DATABASE, "register", json!({"kind":"worker"}));
    let server = OneServer::start(board.address());
    let Some(mut browser) = chrome() else { return };
    browser.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":390,"height":844,"deviceScaleFactor":1,"mobile":true}),
    );
    for (project, expected, choose) in [
        ("alpha", "review", "Ready"),
        ("pantry", "review", "Design"),
        ("alpha", "ready", "Ready"),
        ("pantry", "design", "Design"),
    ] {
        browser.open(&server.url(&format!("/{project}/tasks")));
        assert!(browser.wait_for(&format!("document.querySelector('.waiting-columns .column[data-status={expected}]') !== null && document.documentElement.dataset.navigation === 'ready'"),SHOWN),"{project} did not restore {expected}: {}", observed_page(&mut browser));
        browser.eval(&format!("[...document.querySelectorAll('.stage-tabs button')].find(button=>button.textContent.startsWith({})).click()",json!(choose)));
    }
    assert_eq!(browser.eval("[localStorage.getItem('alpha:board-waiting-stage'),localStorage.getItem('pantry:board-waiting-stage'),localStorage.getItem('board-waiting-stage')]"),json!(["ready","design",null]));
}
