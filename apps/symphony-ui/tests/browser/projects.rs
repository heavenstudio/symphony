//! Two projects on the one real server, each as the owner first opens it: its home within the
//! first visit's budget, its theme in the page's computed look and the rest of the look the
//! other's, project links under its own `/<id>/`, and the header switch back to the root picker;
//! what one keeps in the browser, a draft or a
//! saved theme, never shows in the other, whose origin it shares. `alpha` is a plain project whose
//! file sets a token and a layout; `pantry` names the shelf plugin and the theme package it
//! provides.
use super::{
    FIRST_VISIT_BYTES, SHOWN, binaries, chrome, chrome::Chrome, completed_requests, dist,
    needed_bytes, owner_controls::ScratchBoard, ping_bytes,
};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

const ALPHA: &str = "[project]\nname = \"Alpha\"\nid = \"alpha\"\n\
                     [theme.tokens]\nprimary = \"#7a2e8f\"\n[theme.layouts]\nhome = \"focus\"\n";
const PANTRY: &str = "[project]\nname = \"Pantry\"\nid = \"pantry\"\n\
                      [plugins]\ncrates = [\"fixture-shelf\"]\n[theme]\npackage = \"fixture-shelf\"\n";
/// A register with one open card, so each home has a next card to show.
const REGISTER: &str = "# Decisions\n\n- [ ] **first** — The first question.\n";

/// The one server of a scratch machine folder, its client installed, both projects registered.
pub(super) struct OneServer {
    child: Child,
    scratch: PathBuf,
    base: String,
}

impl OneServer {
    fn start(board: &str) -> Self {
        Self::with_orphans(board, 0)
    }

    pub(super) fn with_orphans(board: &str, count: usize) -> Self {
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
        // A deterministic process table goes through the real census. No host process is
        // manufactured or stopped; the server only counts these fixture rows.
        let table: String = (0..count)
            .map(|i| format!("{} 1 /fixture/docs_server --root /gone\n", 100_000 + i))
            .collect();
        // Prime the shared executable before the census's readiness bound; each case's
        // rows remain interpreter input instead of a freshly assessed executable.
        super::fixture_script::write(
            &bin.join("ps"),
            &format!("#!/bin/sh\nprintf '%s' '{table}'\n"),
            false,
        );
        let path = format!("{}:/usr/bin:/bin", bin.display());
        for (id, file) in [("alpha", ALPHA), ("pantry", PANTRY)] {
            let root = scratch.join(id);
            for (rel, text) in [
                ("symphony.toml", file),
                ("docs/DECISIONS.md", REGISTER),
                ("docs/SHELF.md", "# Shelf\n\n[[shelf:all]]\n"),
                // The Symphony's own place for the review guide; the home links it by its route.
                ("docs/REVIEW.md", "# Review\n\nHow a card is reviewed.\n"),
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
            .env("TASKS_DATABASE", "browser_projects")
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
        server.base = recv.recv_timeout(SHOWN).unwrap_or_else(|e| {
            panic!(
                "the one server's readiness: {e}; {}",
                std::fs::read_to_string(server.scratch.join("server.log")).unwrap()
            )
        });
        server
    }

    pub(super) fn url(&self, path: &str) -> String {
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
    let board = ScratchBoard::start();
    let server = OneServer::start(board.address());
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

    let board = ScratchBoard::start();
    let server = OneServer::start(board.address());
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
        json!({"name":"Change board","destination":"/","first":true,"focused":true,"visible":true})
    );
    key(&mut browser, "Enter", "Enter", 13);
    wait_for_picker(&mut browser);
    // The picker is a machine page in the app's frame: its header's link to the project list,
    // marked active as a client page's own link is, is the first stop, then each project in the
    // list's order.
    for (name, destination, first) in [
        ("⌂ projects", "/", true),
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
    key(&mut browser, "Enter", "Enter", 13);
    assert!(
        browser.wait_for(
            "location.pathname === '/pantry/' && document.querySelector('.hero-title') !== null",
            SHOWN
        ),
        "selected project missing: {}",
        observed_page(&mut browser)
    );
    browser.eval("document.querySelector('header a[href=\"/tasks\"]').click()");
    assert!(browser.wait_for("location.pathname === '/pantry/tasks' && document.documentElement.dataset.navigation === 'ready'", SHOWN), "project tasks missing: {}", observed_page(&mut browser));
    // The board-home link stays separate from the machine picker, even in the compact theme.
    browser.eval("document.querySelector('header a[href=\"/\"]:not(.project-switch)').click()");
    assert!(
        browser.wait_for(
            "location.pathname === '/pantry/' && document.querySelector('.hero-title') !== null",
            SHOWN
        ),
        "project board home missing: {}",
        observed_page(&mut browser)
    );
    change_board(&mut browser);
    browser.open(&server.url("/"));
    wait_for_picker(&mut browser);
}

#[test]
fn each_project_fits_the_first_visit_budget_in_its_own_theme_and_the_shared_look() {
    let board = ScratchBoard::start();
    let server = OneServer::start(board.address());
    let mut looks = Vec::new();
    for (id, primary, layout) in [
        ("alpha", "#7a2e8f", ("layoutHome", "focus")),
        ("pantry", "#2a6f4e", ("layoutHeader", "compact")),
    ] {
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
            bytes <= FIRST_VISIT_BYTES,
            "/{id}/ first visit loads {bytes} bytes, over {FIRST_VISIT_BYTES}: {requests:#?}"
        );
        let look = browser.eval(LOOK);
        assert_eq!(look["primary"], json!(primary), "{id}: {look}");
        assert_eq!(look["layouts"][layout.0], json!(layout.1), "{id}: {look}");
        looks.push(look);
        // The header's links are the project's routes, followed under its own `/<id>/`.
        browser.eval("document.querySelector('header a[href=\"/tasks\"]').click()");
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
}

/// Where each link of `selector` on the page leads, as `[href, status]`: an in-page anchor is
/// 200 when its element is on the page; a document or folder route is what its read answers,
/// since the server sends any route its page; any other route is what the server answers for it.
fn landings(browser: &mut Chrome, base: &str, selector: &str) -> Vec<(String, u64)> {
    let found = browser.eval(&format!(
        r#"(async () => {{
            const base = {base}, seen = [];
            for (const link of document.querySelectorAll({selector})) {{
                const href = link.getAttribute('href');
                if (seen.some(([h]) => h === href)) continue;
                if (href.startsWith('#')) {{ seen.push([href, document.getElementById(href.slice(1)) ? 200 : 404]); continue; }}
                const path = href.split('#')[0];
                const route = path.slice(1).replace(/\/$/, '');
                const probe = link.dataset.native ? path
                    : path.endsWith('.md') ? `${{base}}/api/document?view=outline&path=${{encodeURIComponent(route)}}`
                    : path.length > 1 && path.endsWith('/') ? `${{base}}/api/folder?view=outline&path=${{encodeURIComponent(route)}}`
                    : base + path;
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
    let board = ScratchBoard::start();
    let server = OneServer::start(board.address());
    let Some(mut browser) = chrome() else { return };
    browser.open(&server.url("/alpha/"));
    assert!(
        browser.wait_for("document.querySelector('.hero-title') !== null", SHOWN),
        "{}",
        observed_page(&mut browser)
    );
    let links = landings(&mut browser, "/alpha", "header a[href], .dash a[href]");
    assert!(
        links.iter().any(|(href, _)| href == "/docs/REVIEW.md"),
        "the home links the project's own review guide: {links:?}"
    );
    let lost: Vec<_> = links.iter().filter(|(_, status)| *status != 200).collect();
    assert!(
        lost.is_empty(),
        "links that lead nowhere: {lost:?} of {links:?}"
    );
}
