//! Two projects on the one real server, each as the owner first opens it: its home within the
//! first visit's budget, its theme in the page's computed look and the rest of the look the
//! other's, every link under its own `/<id>/`; and what one keeps in the browser, a draft or a
//! saved theme, never shows in the other, whose origin it shares. `alpha` is a plain project whose
//! file sets a token and a layout; `pantry` names the shelf plugin and the theme package it
//! provides.
use super::{
    FIRST_VISIT_BYTES, SHOWN, binaries, chrome, chrome::Chrome, completed_requests, dist,
    needed_bytes, owner_controls::ScratchBoard,
};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

const ALPHA: &str = "[project]\nname = \"Alpha\"\nid = \"alpha\"\n\
                     [theme.tokens]\nprimary = \"#7a2e8f\"\n[theme.layouts]\nhome = \"focus\"\n";
const PANTRY: &str = "[project]\nname = \"Pantry\"\nid = \"pantry\"\n\
                      [plugins]\ncrates = [\"fixture-shelf\"]\n[theme]\npackage = \"fixture-shelf\"\n";
/// A register with one open card, so each home has a next card to show.
const REGISTER: &str = "# Decisions\n\n- [ ] **first** — The first question.\n";

/// The one server of a scratch machine folder, its client installed, both projects registered.
struct OneServer {
    child: Child,
    scratch: PathBuf,
    base: String,
}

impl OneServer {
    fn start(board: &str) -> Self {
        let binary = binaries::shelf_server();
        let scratch = fresh(
            &std::env::temp_dir().join(format!("sf-browser-projects-{}", std::process::id())),
        );
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
        for (id, file) in [("alpha", ALPHA), ("pantry", PANTRY)] {
            let root = scratch.join(id);
            for (rel, text) in [
                ("symphony.toml", file),
                ("docs/DECISIONS.md", REGISTER),
                ("docs/SHELF.md", "# Shelf\n\n[[shelf:all]]\n"),
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
            .env("SF_SYMPHONY_TEST_PID", std::process::id().to_string())
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
    browser.open(&server.url("/pantry/"));
    quick_text(&mut browser);
    assert_eq!(
        browser.eval("({draft: document.querySelector('#quick-text').value, theme: document.querySelector('#studio-theme') !== null})"),
        json!({"draft": "", "theme": false}),
        "pantry shows nothing alpha saved"
    );
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
        eprintln!("/{id}/ first visit: {bytes} bytes");
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
