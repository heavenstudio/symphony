//! Restored native browser contracts. Security cases use the real server, never fixture CSP.
use super::{READY, SHOWN, binaries, chrome, dist, site::Site, texts};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

mod activity;
mod queue;
mod viewer;

pub(super) struct Native {
    child: Child,
    root: PathBuf,
    outside: PathBuf,
    base: String,
}

impl Native {
    fn start() -> Self {
        Self::start_with_board(None)
    }

    pub(super) fn start_with_board(board: Option<&str>) -> Self {
        Self::start_with(board, &[])
    }

    /// A server on a scratch board, with `vars` added to its environment.
    pub(super) fn start_with(board: Option<&str>, vars: &[(&str, &std::path::Path)]) -> Self {
        Self::committed(board, "fixture", &[], None, vars)
    }

    /// A server over a checkout on `branch` whose first commit also holds `documents` (path, text),
    /// and with `tree` (branch, documents) a worktree beside it on that branch, whose own commit holds
    /// those documents: its pages are served under `/@<branch>/`.
    pub(super) fn start_committed(
        board: Option<&str>,
        branch: &str,
        documents: &[(&str, &str)],
        tree: Option<(&str, &[(&str, &str)])>,
    ) -> Self {
        Self::committed(board, branch, documents, tree, &[])
    }

    /// [`Self::start_committed`], with `vars` added to the server's environment.
    fn committed(
        board: Option<&str>,
        branch: &str,
        documents: &[(&str, &str)],
        tree: Option<(&str, &[(&str, &str)])>,
        vars: &[(&str, &std::path::Path)],
    ) -> Self {
        // Had before the readiness clock below starts; the owned child is this server itself.
        let binary = binaries::shelf_server();
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "symphony-browser-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        for dir in [
            "docs",
            "ui",
            "room/shelf/items",
            "artifacts",
            "tools/static",
        ] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        // The public launcher canonicalizes --root. Match that contract when starting the
        // internal serve command directly; macOS notify reports /private/var, never /var.
        let root = root.canonicalize().unwrap();
        std::fs::write(
            root.join("docs/PROBE.md"),
            "# Probe\n\n![Probe](/ui/probe.html)\n",
        )
        .unwrap();
        std::fs::write(
            root.join("symphony.toml"),
            "[plugins]\ncrates = [\"fixture-shelf\"]\n",
        )
        .unwrap();
        std::fs::write(root.join("docs/SHELF.md"), "# Shelf\n\n[[shelf:all]]\n").unwrap();
        std::fs::write(root.join("ui/probe.html"), r#"<!doctype html><h1>Probe</h1><script>
            const result = {ran:true, framed:top!==window};
            for (const [key, action] of Object.entries({
                parent:()=>parent.document.body.setAttribute('data-pwned','true'),
                storage:()=>localStorage.setItem('pwned','true'),
                navigation:()=>{if(top!==window) top.location='/ui/escaped.html'; else throw Error('top');}
            })) { try { action(); result[key]='allowed'; } catch (_) { result[key]='blocked'; } }
            Promise.all(['/api/queue','/api/decision'].map(async (url)=>{
                try { const r=await fetch(url, url.endsWith('decision') ? {method:'POST',headers:{'Content-Type':'text/plain'},body:'{}'} : {}); return r.status; }
                catch (_) { return 'blocked'; }
            })).then(api=>{ result.api=api; document.title=JSON.stringify(result); if(top!==window) parent.postMessage(result,'*'); });
        </script>"#).unwrap();
        std::fs::write(
            root.join("ui/hop.html"),
            "<script>location.replace('/ui/hop.svg')</script>",
        )
        .unwrap();
        std::fs::write(root.join("ui/hop.svg"), r#"<svg xmlns="http://www.w3.org/2000/svg"><script><![CDATA[
            const result={svg:true};
            try {localStorage.setItem('svg','true'); result.storage='allowed';} catch (_) {result.storage='blocked';}
            fetch('/api/decision',{method:'POST',headers:{'Content-Type':'text/plain'},body:'{}'}).then(r=>{result.post=r.status; document.title=JSON.stringify(result);},()=>{result.post='blocked'; document.title=JSON.stringify(result);});
        ]]></script></svg>"#).unwrap();
        std::fs::write(root.join("private.html"), "inside root").unwrap();
        let outside = root.with_extension("outside.html");
        std::fs::write(&outside, "outside root").unwrap();
        std::fs::write(
            root.join("room/shelf/items/FIRST.item"),
            "gallery record, never selected",
        )
        .unwrap();
        std::os::unix::fs::symlink(root.join("private.html"), root.join("ui/inside.html")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("ui/outside.html")).unwrap();
        for (path, text) in documents {
            std::fs::write(root.join(path), text).unwrap();
        }
        let git = |at: &std::path::Path, args: &[&str]| {
            let done = Command::new("git")
                .args(args)
                .current_dir(at)
                .env_remove("GIT_DIR")
                .env_remove("GIT_COMMON_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_INDEX_FILE")
                .output()
                .unwrap();
            assert!(done.status.success(), "fixture {args:?}: {done:?}");
        };
        let commit = [
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "Browser fixture",
        ];
        for args in [
            &["init", "-b", branch][..],
            &["config", "user.name", "Browser fixture"],
            &["config", "user.email", "browser@example.invalid"],
            &["config", "core.hooksPath", "/dev/null"],
            &["add", "."],
            &commit,
        ] {
            git(&root, args);
        }
        if let Some((name, documents)) = tree {
            let at = Self::tree_path(&root);
            git(
                &root,
                &["worktree", "add", "-b", name, at.to_str().unwrap()],
            );
            for (path, text) in documents {
                std::fs::write(at.join(path), text).unwrap();
            }
            git(&at, &["add", "."]);
            git(&at, &commit);
        }
        Self::serve(root, outside, binary, board, vars)
    }

    /// Where [`Self::start_committed`] puts the branch's worktree.
    fn tree_path(root: &std::path::Path) -> PathBuf {
        root.with_extension("tree")
    }

    fn serve(
        root: PathBuf,
        outside: PathBuf,
        binary: &std::path::Path,
        board: Option<&str>,
        vars: &[(&str, &std::path::Path)],
    ) -> Self {
        // Board writes reach only the caller's owned scratch server, or a socket folder nothing
        // serves.
        let closed = format!("postgresql:{}", root.join("artifacts/no-board").display());
        let log = std::fs::File::create(root.join("artifacts/server.log")).unwrap();
        let child = Command::new("bash")
            .args([
                "-c",
                r#". "$1"; fixture_close_fds; shift; exec "$@""#,
                "native-fixture",
            ])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../tools/fixture_process.sh"
            ))
            .arg(binary)
            .args(["serve", "--root"])
            .arg(&root)
            .args(["--port", "0", "--host", "127.0.0.1"])
            .env("SYMPHONY_UI_DIST", dist())
            .env("TASKS_SERVER", board.unwrap_or(&closed))
            .env("TASKS_DATABASE", "browser_owner_controls")
            .env("TASKS_SWEEP", "off")
            .env("ASKS_DB", root.join("artifacts/asks.db"))
            .env("SYMPHONY_TEST_PID", std::process::id().to_string())
            .envs(vars.iter().copied())
            .env_remove("TASKS_SESSION")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(log)
            .spawn()
            .unwrap();
        let mut native = Self {
            child,
            root,
            outside,
            base: String::new(),
        };
        let stdout = native.child.stdout.take().unwrap();
        let (send, recv) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(url) = line
                    .strip_prefix("→ Symphony at ")
                    .and_then(|s| s.split_whitespace().next())
                {
                    let _ = send.send(url.trim_end_matches('/').to_string());
                }
            }
        });
        native.base = super::announced("the native server", &recv).unwrap_or_else(|why| {
            panic!(
                "{why}; {}",
                std::fs::read_to_string(native.root.join("artifacts/server.log")).unwrap()
            )
        });
        native
    }

    pub(super) fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// The checkout this server serves, which holds its uploads.
    pub(super) fn root(&self) -> &std::path::Path {
        &self.root
    }

    pub(super) fn raw_status(&self, target: &str) -> u16 {
        let address = self.base.strip_prefix("http://").unwrap();
        let mut stream = TcpStream::connect(address).unwrap();
        // The answer's one wait: a quiet machine has it within SHOWN, and only a stall reaches
        // its backstop (RULES.md `tests/no-test-fails-on-load`).
        stream.set_read_timeout(Some(super::STALL)).unwrap();
        stream
            .write_all(
                format!("GET {target} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .unwrap();
        let mut status = String::new();
        if let Err(error) = BufReader::new(stream).read_line(&mut status) {
            panic!(
                "{target}: stalled: no status line by the read's wall-clock backstop of {:?} \
                 ({error}); {}",
                super::STALL,
                test_time::Load::now()
            );
        }
        status.split_whitespace().nth(1).unwrap().parse().unwrap()
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
        let _ = std::fs::remove_dir_all(Self::tree_path(&self.root));
        let _ = std::fs::remove_file(&self.outside);
    }
}

fn native_probe(browser: &mut super::chrome::Chrome) -> Value {
    assert!(
        browser.wait_for("document.title.startsWith('{')", SHOWN),
        "probe did not finish: {}",
        browser.eval("document.body.innerHTML")
    );
    browser.eval("JSON.parse(document.title)")
}

#[test]
fn later_advances_the_document_and_another_device_can_undo_it() {
    let Some(mut browser) = chrome() else { return };
    let Some(mut phone) = chrome() else { return };
    let site = Native::start();
    let original = "# Later\n\n- [ ] **alpha** — First question.\n- [ ] **bravo** — Second question.\n- [ ] **charlie** — Third question.\n";
    std::fs::write(site.root.join("docs/LATER.md"), original).unwrap();
    browser.open(&site.url("/docs/LATER.md#alpha"));
    phone.open(&site.url("/queue"));
    assert!(phone.wait_for(
        "document.querySelector('#you li[data-card]')?.dataset.card==='docs/LATER.md#alpha'",
        SHOWN
    ));
    phone.eval("window.queueIdentity = 'kept'");
    assert!(
        browser.wait_for(
            "document.querySelector('[data-doc-key=\"card:alpha\"] .doc-actions button') !== null",
            SHOWN
        ),
        "{}",
        browser.eval("document.body.innerText")
    );
    browser.eval(
        "document.querySelector('[data-doc-key=\"card:alpha\"] .doc-actions button').click()",
    );
    assert!(browser.wait_for("[...document.querySelectorAll('[data-doc-key=\"card:alpha\"] .verdict-choices button')].some(b=>b.textContent==='Later')", SHOWN));
    browser.eval("[...document.querySelectorAll('[data-doc-key=\"card:alpha\"] .verdict-choices button')].find(b=>b.textContent==='Later').click()");
    assert!(
        browser.wait_for(
            "location.hash==='#bravo' && document.querySelector('[data-ready=document]') !== null",
            SHOWN
        ),
        "{}",
        browser.eval("document.body.innerText")
    );
    assert!(phone.wait_for("document.querySelector('#you h2')?.textContent==='Waiting on you (3)' && document.querySelector('#you li:last-child .postponed')?.textContent==='postponed'", SHOWN), "{}", phone.eval("document.body.innerText"));
    assert_eq!(phone.eval("window.queueIdentity"), "kept");
    assert_eq!(
        phone.eval("[...document.querySelectorAll('#you li[data-card]')].map(e=>e.dataset.card)"),
        json!([
            "docs/LATER.md#bravo",
            "docs/LATER.md#charlie",
            "docs/LATER.md#alpha"
        ])
    );
    phone.eval("[...document.querySelectorAll('#you button')].find(b=>b.textContent==='Undo Later').click()");
    assert!(phone.wait_for("document.querySelector('#you li[data-card]')?.dataset.card==='docs/LATER.md#alpha' && !document.querySelector('#you .postponed')", SHOWN), "{}", phone.eval("document.body.innerText"));
    phone.eval("document.querySelector('#you li[data-card] button').click()");
    assert!(phone.wait_for("location.hash==='#bravo' && document.querySelector('[data-doc-key=\"card:bravo\"] .doc-actions button') !== null", SHOWN));
    phone.eval(
        "document.querySelector('[data-doc-key=\"card:bravo\"] .doc-actions button').click()",
    );
    // The choices are the click's to draw, as on the other device above: waited for, not assumed.
    assert!(phone.wait_for("[...document.querySelectorAll('[data-doc-key=\"card:bravo\"] .verdict-choices button')].some(b=>b.textContent==='Later')", SHOWN), "{}", phone.eval("document.querySelector('[data-doc-key=\"card:bravo\"]')?.outerHTML ?? document.body.innerText"));
    phone.eval("[...document.querySelectorAll('[data-doc-key=\"card:bravo\"] .verdict-choices button')].find(b=>b.textContent==='Later').click()");
    assert!(phone.wait_for(
        "location.hash==='#charlie' && document.querySelector('[data-ready=document]') !== null",
        SHOWN
    ));
    assert!(
        phone.wait_for(
            "fetch('/api/postponed').then(r=>r.json()).then(cards=>cards.length===0)",
            SHOWN
        ),
        "the same-document visit must finish the round"
    );
    assert_eq!(
        std::fs::read_to_string(site.root.join("docs/LATER.md")).unwrap(),
        original
    );
    assert!(!site.root.join("docs/DECISIONS.md").exists());
}

#[test]
fn restored_mockups_keep_opaque_origins_in_frames_full_size_and_svg_hops() {
    let Some(mut browser) = chrome() else { return };
    let site = Native::start();
    browser.call("Page.addScriptToEvaluateOnNewDocument",json!({"source":"window.probes=[]; window.addEventListener('message',e=>probes.push({origin:e.origin,...e.data}));"}));
    browser.viewport(420, 900, false);
    browser.open(&site.url("/docs/PROBE.md"));
    assert!(
        browser.wait_for("window.probes.length === 1", SHOWN),
        "{}",
        browser.eval("document.body.innerText")
    );
    let probe = browser.eval("probes[0]");
    assert_eq!(
        probe,
        json!({"ran":true,"framed":true,"origin":"null","parent":"blocked","storage":"blocked","navigation":"blocked","api":["blocked","blocked"]})
    );
    assert_eq!(
        browser.eval("document.body.hasAttribute('data-pwned')"),
        json!(false)
    );
    assert_eq!(browser.eval("location.pathname"), json!("/docs/PROBE.md"));
    assert_eq!(
        browser.eval("fetch('/api/queue').then(r=>r.status)"),
        json!(200),
        "the trusted app can read the same endpoint the mockup cannot"
    );
    assert!(browser.wait_for("(()=>{const f=document.querySelector('iframe');return f && f.style.transform.startsWith('scale(')})()",SHOWN));
    assert_eq!(browser.eval("(()=>{const f=document.querySelector('iframe');const stage=f.parentElement.getBoundingClientRect();const r=f.getBoundingClientRect();return f.width==='1280' && f.height==='800' && Math.abs(r.width-stage.width)<1 && r.width<640 && f.sandbox.value==='allow-scripts'})()"),json!(true));
    browser.open(&site.url("/ui/probe.html"));
    let probe = native_probe(&mut browser);
    assert_eq!(probe["ran"], true);
    assert_eq!(probe["framed"], false);
    assert_eq!(probe["storage"], "blocked");
    assert_eq!(probe["api"], json!(["blocked", "blocked"]));
    for path in ["/api/queue", "/api/decision"] {
        assert!(
            browser
                .requests()
                .iter()
                .any(|r| r.url.ends_with(path) && r.status == 403),
            "{path} must be refused by the app gate, beyond the browser's CORS refusal"
        );
    }
    browser.open(&site.url("/ui/hop.html"));
    let probe = native_probe(&mut browser);
    assert_eq!(
        probe,
        json!({"svg":true,"storage":"blocked","post":"blocked"})
    );
    assert!(
        browser
            .requests()
            .iter()
            .any(|r| r.url.ends_with("/api/decision") && r.status == 403)
    );
    assert_eq!(browser.eval("location.pathname"), json!("/ui/hop.svg"));
    // Run requests from the trusted app so a blocked cross-origin fetch cannot hide a missing
    // root-confinement check. In-root symlinks are a positive control for the file route.
    browser.open(&site.url("/docs/PROBE.md"));
    assert!(browser.wait_for(
        "document.querySelector('[data-ready=document]') !== null",
        SHOWN
    ));
    assert_eq!(browser.eval("Promise.all(['/ui/inside.html','/ui/outside.html','/.git/config'].map(async p=>(await fetch(p)).status))"),json!([200,404,404]));
    // A browser normalizes dot segments before sending. A raw socket preserves the encoded
    // target, which would reach an existing sibling file if root confinement were absent.
    assert_eq!(site.raw_status("/ui/inside.html"), 200);
    let outside = site.outside.file_name().unwrap().to_str().unwrap();
    for target in [
        format!("/ui/%2e%2e/%2e%2e/{outside}"),
        format!("/ui/../../{outside}"),
    ] {
        assert_eq!(site.raw_status(&target), 404, "raw traversal {target}");
    }
    for legacy in ["/app/docs/PROBE.md", "/doc/docs/PROBE.md"] {
        browser.open(&site.url(legacy));
        assert!(browser.wait_for(
            "document.querySelector('[data-ready=document]') !== null",
            SHOWN
        ));
        assert_eq!(browser.eval("location.pathname"), json!("/docs/PROBE.md"));
        // The documents' module, its script, came from the root's assets.
        let module = super::assets::module(dist(), "docs").script;
        assert!(
            browser
                .requests()
                .iter()
                .any(|r| r.url.ends_with(&module) && r.status == 200),
            "{module}"
        );
        assert!(
            !browser
                .requests()
                .iter()
                .any(|r| r.url.contains("/app/assets/"))
        );
    }
    browser.open(&site.url("/shelf"));
    assert!(browser.wait_for(
        "document.querySelectorAll('.gallery-pick').length===1",
        SHOWN
    ));
    assert_eq!(texts(&mut browser, ".gallery-pick strong"), ["FIRST"]);
    std::fs::write(
        site.root.join("room/shelf/items/SECOND.item"),
        "new gallery record",
    )
    .unwrap();
    assert!(
        browser.wait_for(
            "document.querySelectorAll('.gallery-pick').length===2",
            SHOWN
        ),
        "watcher did not invalidate the warm gallery: {}",
        std::fs::read_to_string(site.root.join("artifacts/server.log")).unwrap()
    );
    assert_eq!(
        texts(&mut browser, ".gallery-pick strong"),
        ["FIRST", "SECOND"]
    );
    std::fs::remove_file(site.root.join("room/shelf/items/FIRST.item")).unwrap();
    assert!(
        browser.wait_for("document.querySelectorAll('.gallery-pick').length===1 && document.querySelector('.gallery-pick strong')?.textContent==='SECOND'", SHOWN),
        "removed item remains: page={}; server={}; log={}",
        browser.eval("document.body.innerHTML"),
        browser.eval("fetch('/api/document?path=docs/SHELF.md').then(r=>r.json())"),
        std::fs::read_to_string(site.root.join("artifacts/server.log")).unwrap()
    );
    assert_eq!(
        browser.eval("document.querySelectorAll('.viewer-stage').length"),
        json!(0)
    );
}

fn paragraph(text: &str) -> Value {
    json!({"type":"paragraph","spans":[{"type":"text","text":text}]})
}

#[test]
fn restored_document_pictures_load_near_the_viewport_and_stay_mounted_after_scrolling_away() {
    let site = crate::site();
    document(
        &site,
        vec![
            paragraph("Visible introduction"),
            json!({"type":"image","image":{"src":"/late.svg","alt":"Late picture","presentation":{"attributes":{"width":"200","height":"200","style":"margin-top:20000px"}}}}),
        ],
    );
    site.set_file(
        "/late.svg",
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200"/>"#.to_vec(),
        "image/svg+xml",
    );
    let Some(mut browser) = chrome() else { return };
    browser.open(&site.url("/docs/PROBE.md"));
    assert!(browser.wait_for(
        "performance.getEntriesByName('document-ready').length>0 && document.querySelector('.doc-body img[alt=\"Late picture\"]') !== null",
        SHOWN
    ));
    assert_eq!(
        browser.eval("document.querySelector('.doc-body img').getAttribute('src')"),
        json!(symphony_web_tests::WAITING)
    );
    assert!(!site.requests().contains(&"/late.svg".into()));
    browser.eval("window.keptPicture=document.querySelector('.doc-body img');keptPicture.scrollIntoView({block:'center'})");
    assert!(browser.wait_for("keptPicture.naturalWidth===200", SHOWN), "{}; {:?}", browser.eval("JSON.stringify({src:keptPicture.getAttribute('src'),top:keptPicture.getBoundingClientRect().top,scroll:scrollY,ready:performance.getEntriesByName('document-ready').length})"), site.targets());
    browser.eval("window.scrollTo(0,0)");
    assert!(browser.wait_for("scrollY===0", SHOWN));
    assert_eq!(browser.eval("document.querySelector('.doc-body img')===keptPicture && keptPicture.getAttribute('src')==='/late.svg'"),json!(true));
    assert_eq!(
        site.requests().iter().filter(|p| *p == "/late.svg").count(),
        1
    );
}

/// A picture waiting for its latch already holds its sized box, so its showing moves nothing
/// after it; and focused before it shows (a keyboard reaching it scrolls it near), it keeps the
/// focus as its src lands.
#[test]
fn restored_document_picture_holds_its_box_and_the_keyboard_focus_as_it_shows() {
    let site = crate::site();
    document(
        &site,
        vec![
            paragraph("Visible introduction"),
            json!({"type":"image","image":{"src":"/held.svg","alt":"Held picture","presentation":{"attributes":{"width":"200","height":"200","style":"margin-top:20000px"}}}}),
            paragraph("After the picture"),
        ],
    );
    site.set_file(
        "/held.svg",
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200"/>"#.to_vec(),
        "image/svg+xml",
    );
    let Some(mut browser) = chrome() else { return };
    browser.open(&site.url("/docs/PROBE.md"));
    // Focusable once the picture viewer has marked it.
    assert!(browser.wait_for(
        "performance.getEntriesByName('document-ready').length>0 && document.querySelector('.doc-body img[alt=\"Held picture\"][tabindex=\"0\"]') !== null",
        SHOWN
    ));
    let place = "(() => { const image = document.querySelector('.doc-body img'), box = image.getBoundingClientRect(); const after = [...document.querySelectorAll('.doc-body p')].find(p => p.textContent === 'After the picture'); return {src: image.getAttribute('src'), width: Math.round(box.width), height: Math.round(box.height), after: Math.round(after.getBoundingClientRect().top + scrollY)}; })()";
    let waiting = browser.eval(place);
    assert_eq!(
        (&waiting["src"], &waiting["width"], &waiting["height"]),
        (
            &json!(symphony_web_tests::WAITING),
            &json!(200),
            &json!(200)
        ),
        "the waiting picture holds its sized box: {waiting}"
    );
    assert!(!site.requests().contains(&"/held.svg".into()));
    browser.eval("window.heldPicture=document.querySelector('.doc-body img');heldPicture.focus()");
    assert!(
        browser.wait_for("heldPicture.naturalWidth===200", SHOWN),
        "{}; {:?}",
        browser.eval(place),
        site.targets()
    );
    assert_eq!(
        browser.eval("[document.querySelector('.doc-body img')===heldPicture, document.activeElement===heldPicture]"),
        json!([true, true]),
        "the shown picture is the node focused, and keeps the focus: {}",
        browser.eval("document.activeElement.outerHTML")
    );
    let shown = browser.eval(place);
    assert_eq!(
        (
            &shown["src"],
            &shown["width"],
            &shown["height"],
            &shown["after"]
        ),
        (
            &json!("/held.svg"),
            &json!(200),
            &json!(200),
            &waiting["after"]
        ),
        "the picture's showing moves nothing after it: before {waiting}, after {shown}"
    );
}

/// The picture viewer shows the document's pictures that are pictures: one still waiting for its
/// latch, far down the page, is none of them.
#[test]
fn restored_picture_viewer_leaves_out_a_picture_still_waiting() {
    let site = crate::site();
    let image = |name: &str, style: &str| json!({"type":"image","image":{"src":format!("/{name}.svg"),"alt":name,"presentation":{"attributes":{"width":"200","height":"200","style":style}}}});
    // A paragraph between them: pictures side by side share a line, as tall as the far one's margin.
    document(
        &site,
        vec![
            image("near", ""),
            paragraph("Between the pictures"),
            image("far", "margin-top:20000px"),
        ],
    );
    for name in ["near", "far"] {
        site.set_file(
            &format!("/{name}.svg"),
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200"/>"#.to_vec(),
            "image/svg+xml",
        );
    }
    let Some(mut browser) = chrome() else { return };
    browser.open(&site.url("/docs/PROBE.md"));
    assert!(
        browser.wait_for(
            "document.querySelector('.doc-body img[alt=near][data-zoom-ready]')?.naturalWidth===200 && document.querySelector('.doc-body img[alt=far][data-zoom-ready]') !== null",
            SHOWN
        ),
        "{}; {:?}",
        browser.eval("[...document.querySelectorAll('.doc-body img')].map(i => i.outerHTML + ' ' + i.naturalWidth)"),
        site.targets()
    );
    assert_eq!(
        browser.eval("document.querySelector('.doc-body img[alt=far]').getAttribute('src')"),
        json!(symphony_web_tests::WAITING)
    );
    browser.eval("document.querySelector('.doc-body img[alt=near]').click()");
    assert!(browser.wait_for(
        "document.querySelector('#media-lightbox[data-state=open] img')?.getAttribute('src')==='/near.svg'",
        SHOWN
    ));
    // One picture: its caption counts none, and no button steps to another.
    assert_eq!(
        browser.eval("[document.querySelector('#media-lightbox .lightbox-caption').textContent, document.querySelectorAll('#media-lightbox [aria-label=\"Next picture\"]').length]"),
        json!(["near", 0])
    );
    assert!(!site.requests().contains(&"/far.svg".into()));
}

pub(super) fn document(site: &Site, blocks: Vec<Value>) {
    let sections =
        json!([{"start":1,"end":10,"key":"intro","hash":"intro","part":0,"blocks":blocks}]);
    site.set_json("/api/document",&json!({"document":{"version":4182,"path":"docs/PROBE.md","title":"Probe","waiting":[],"crumbs":[],"headings":[],"sections":sections,"cards":[],"threads":{},"tasks":[]},"anchors":{}}));
    site.set_json(
        "/api/section",
        &json!({"version":4182,"change":{"path":"docs/PROBE.md","sections":sections}}),
    );
    site.set_json("/api/asks", &json!({"asks":[],"recipient":null}));
}

/// Wait until the page's feed follows the document's intro, before a test pushes an event. The
/// page opens its feed before the intro is near the screen, then opens it again naming the intro,
/// live all along: the server sends the second stream what was pushed in between, the stand-in
/// never does (`Site::following`), and the event is lost.
fn wait_for_the_feed_on_the_intro(site: &Site) {
    test_time::until_true("the page's feed following the intro", super::STALL, || {
        site.following("intro")
    })
    .unwrap_or_else(|stall| {
        let feeds: Vec<String> = site
            .targets()
            .into_iter()
            .filter(|target| target.starts_with(symphony_api::FEED_PATH))
            .collect();
        panic!("{stall}: the feed requests {feeds:?}")
    });
}

fn assert_notice_surface(browser: &mut super::chrome::Chrome, selector: &str, role: &str) {
    let surface = browser.eval(&format!(
        r#"(() => {{
        const pill=document.querySelector({selector});
        if (!pill) return null;
        const canvas=document.createElement('canvas');
        canvas.width=canvas.height=1;
        const paint=canvas.getContext('2d');
        paint.clearRect(0,0,1,1);
        paint.fillStyle='transparent';
        paint.fillStyle=getComputedStyle(pill).backgroundColor;
        paint.fillRect(0,0,1,1);
        return [pill.getAttribute('role'), paint.getImageData(0,0,1,1).data[3],
            pill.getBoundingClientRect().height>0 && getComputedStyle(pill).opacity==='1'];
    }})()"#,
        selector = json!(selector)
    ));
    assert_eq!(
        surface,
        json!([role, 255, true]),
        "{selector}: a visible notice must have its own opaque surface and announcement role"
    );
}

#[test]
fn server_notices_survive_loading_and_failed_document_and_folder_routes() {
    let site = crate::site();
    document(
        &site,
        vec![paragraph("A loaded document establishes the server feed.")],
    );
    let Some(mut browser) = chrome() else { return };
    browser.open(&site.url("/docs/PROBE.md"));
    assert!(browser.wait_for(
        "document.querySelector('[data-ready=document]')?.dataset.feed==='live'",
        SHOWN
    ));
    site.server_notice("Reload failed: the review server is unavailable.");
    assert!(browser.wait_for("document.querySelector('.page-notices .server-notice')?.textContent==='Reload failed: the review server is unavailable.'", SHOWN));
    for (path, api, loading, pending) in [
        (
            "/docs/UNAVAILABLE.md",
            "/api/document",
            "Loading document…",
            ".document-page[data-ready=loading] > p",
        ),
        (
            "/docs/unavailable/",
            "/api/folder",
            "Loading folder…",
            ".document-page[data-ready=loading] > p",
        ),
    ] {
        site.hold_route(api, true);
        browser.eval(&format!(
            "history.pushState({{}},'',{});dispatchEvent(new PopStateEvent('popstate'))",
            json!(path)
        ));
        assert!(
            browser.wait_for(
                &format!(
                    "document.querySelector({})?.textContent==={}",
                    json!(pending),
                    json!(loading)
                ),
                SHOWN
            ),
            "{path}: loading state must be reached: {}",
            browser.eval("document.body.innerText")
        );
        assert_eq!(
            browser.eval("document.querySelector('.page-notices .server-notice')?.textContent"),
            json!("Reload failed: the review server is unavailable."),
            "{path}: loading keeps the earlier server notice"
        );
        assert_notice_surface(&mut browser, ".page-notices .server-notice", "status");
        site.fail_route(api, true);
        site.hold_route(api, false);
        assert!(
            browser.wait_for(
                "document.querySelector('.document-page[data-ready=failed] .trouble p')?.textContent==='Board snapshot unavailable (the server answered 500). Trying again…'",
                SHOWN
            ),
            "{path}: the initial fetch must really fail: {}",
            browser.eval("document.body.innerText")
        );
        assert_eq!(
            browser.eval("document.querySelector('.page-notices .server-notice')?.textContent"),
            json!("Reload failed: the review server is unavailable."),
            "{path}: a failed initial fetch keeps the earlier server notice"
        );
        assert_eq!(
            browser.eval("[...document.querySelectorAll('.document-page p, .page-notices > p')].filter(p=>p.textContent.includes('Board snapshot unavailable')).length"),
            json!(1),
            "{path}: the failed initial fetch is reported once, by the page's trouble block"
        );
        assert_notice_surface(&mut browser, ".page-notices .server-notice", "status");
    }
}

#[test]
fn reconnect_pills_are_statuses_and_leave_document_reading_layout_unchanged() {
    let site = crate::site();
    document(
        &site,
        (0..100)
            .map(|index| paragraph(&format!("Reading passage {index}. The document keeps its place while the feed reconnects.")))
            .collect(),
    );
    site.set_json(
        "/api/folder",
        &json!({
            "path":"docs","title":"Documents","crumbs":[],"docs":1,"entries":[],
            "intro":{"version":4182,"path":"docs/PROBE.md","title":"Probe","waiting":[],"crumbs":[],
            "headings":[],"sections":[],"cards":[],"threads":{},"tasks":[]}
        }),
    );
    let Some(mut browser) = chrome() else { return };
    for (path, page, ready) in [
        ("/tasks", "main", ".kanban"),
        ("/queue", "main", "[data-ready=queue]"),
        (
            "/docs/",
            ".document-page[data-ready=folder]",
            "[data-ready=document] [data-doc-key=intro] p",
        ),
        (
            "/docs/PROBE.md",
            ".document-page",
            "[data-ready=document] [data-doc-key=intro] p",
        ),
    ] {
        browser.open(&site.url(path));
        assert!(browser.wait_for(
            &format!("{READY} && document.querySelector({}) !== null && document.querySelector({})?.dataset.feed === 'live'", json!(ready), json!(page)),
            SHOWN,
        ), "{path}: {}", browser.eval("document.body.innerText"));
        let reading = path.starts_with("/docs/");
        if reading {
            // Disable the browser aid that can conceal a reconnect notice moving the page.
            browser.eval("document.documentElement.style.overflowAnchor='none'; scrollTo({top:1200,behavior:'instant'})");
            browser.frames();
            assert_eq!(
                browser.eval("scrollY"),
                json!(1200),
                "setup: reading well inside the long document"
            );
            browser.eval("window.keptScroll=scrollY;window.keptSection=document.querySelector('[data-doc-key=intro]');window.keptBox=JSON.stringify(keptSection.getBoundingClientRect().toJSON())");
        }
        // Keep stale observable without racing the production reconnect backoff.
        site.fail_feeds(usize::MAX);
        site.drop_feeds();
        assert!(
            browser.wait_for(
                &format!(
                    "document.querySelector({})?.dataset.feed === 'stale'",
                    json!(page)
                ),
                SHOWN,
            ),
            "{path} did not report the lost feed"
        );
        assert_eq!(
            browser.eval(&format!(
                "document.querySelector({})?.getAttribute('role')",
                json!(format!("{page} .pill.wait"))
            )),
            json!("status"),
            "{path}: reconnecting must be announced as a status",
        );
        assert_notice_surface(&mut browser, ".page-notices .pill.wait", "status");
        if reading {
            assert_eq!(
                browser.eval("scrollY===keptScroll"),
                json!(true),
                "stale keeps the exact reading offset"
            );
            assert_eq!(browser.eval("document.querySelector('[data-doc-key=intro]')===keptSection && JSON.stringify(keptSection.getBoundingClientRect().toJSON())===keptBox"), json!(true), "stale keeps the exact section box");
        }
        site.fail_feeds(0);
        assert!(browser.wait_for(
            &format!("document.querySelector({})?.dataset.feed === 'live' && !document.querySelector({})", json!(page), json!(format!("{page} .pill.wait"))),
            SHOWN,
        ), "{path} did not reconnect");
        if reading {
            assert_eq!(
                browser.eval("scrollY===keptScroll"),
                json!(true),
                "live again keeps the exact reading offset"
            );
            assert_eq!(browser.eval("document.querySelector('[data-doc-key=intro]')===keptSection && JSON.stringify(keptSection.getBoundingClientRect().toJSON())===keptBox"), json!(true), "live again keeps the exact section box");
            // A removal keeps the shown document and drafts. Its warning must
            // not insert a row above the section the owner is reading.
            site.push(
                4183,
                &json!({"version":4183,"changes":[{"region":"document","data":{"path":"docs/PROBE.md","removed":true}}]}).to_string(),
            );
            assert!(browser.wait_for("document.querySelector('.page-notices .document-removed')?.textContent==='This document was removed. Your draft is kept on this device.'", SHOWN), "{path}: a live removal must display its alert");
            assert_notice_surface(&mut browser, ".page-notices .document-removed", "alert");
            assert_eq!(
                browser.eval("scrollY"),
                json!(1200),
                "{path}: removal keeps the exact reading offset"
            );
            assert_eq!(browser.eval("document.querySelector('[data-doc-key=intro]')===keptSection && JSON.stringify(keptSection.getBoundingClientRect().toJSON())===keptBox"), json!(true), "{path}: removal keeps the exact section node and box");
        }
    }
}

/// What starts each of the notice case's own scrolls: the reader's input (a wheel), which ends
/// whatever holds the page's place, as [`Chrome::reveal`](super::chrome::Chrome::reveal)'s does, so
/// the page stays where the case put it and only what it then does moves it.
const READER: &str = "dispatchEvent(new WheelEvent('wheel'));";

/// The board's stages of the notice case, twelve tasks in each.
const NOTICE_STAGES: [&str; 6] = [
    "concept",
    "design",
    "ready",
    "in_progress",
    "review",
    "done",
];

/// Every row of `status`'s column mounted, each because its own slot came on screen and the
/// page's viewport observer saw it ([`Chrome::reveal`](super::chrome::Chrome::reveal)): a scroll
/// to the column's end mounts the rows between only where a frame falls between the scrolls.
fn mount_notice_rows(browser: &mut super::chrome::Chrome, status: &str) {
    browser.reveal(&format!("#column-{status}"));
    browser.until(
        &format!("{status}'s twelve row slots"),
        &format!("document.querySelectorAll('[data-status={status}] [id^=row-]').length === 12"),
    );
    for i in 0..12 {
        browser.reveal(&format!("[id='row-notice/{status}-{i}']"));
    }
    assert_eq!(
        browser.eval(&format!(
            "document.querySelectorAll('[data-status={status}] .taskcard').length"
        )),
        json!(12),
        "{status}: a row once mounted stays mounted"
    );
}

/// A server notice and a failed board snapshot leave the board's loaded rows, its box and the
/// reading offset exactly where they were, at a phone's width, a small phone's and a desk's, and
/// the page's recovery neither clamps the page nor loses a fetched row. Every wait is on its
/// condition ([`Chrome::until`](super::chrome::Chrome::until)).
#[test]
fn board_fetch_and_server_notices_keep_loaded_rows_in_place_after_narrowing() {
    let site = crate::site();
    let tasks: Vec<_> = NOTICE_STAGES.iter().flat_map(|status| (0..12).map(move |i| json!({
        "slug": format!("notice/{status}-{i}"),
        "title": format!("Loaded {status} task {i} keeps its real row through an unavailable board snapshot"),
        "body": "", "status": status, "priority":1, "parent":null, "created_by":"owner",
        "created":0.0, "updated":4102444800.0, "links":[], "depends":[], "roles":[],
        "instruction":false
    }))).collect();
    site.set_json(
        "/api/task-board",
        &json!({
            "tasks":tasks, "statuses":NOTICE_STAGES,
            "counts":NOTICE_STAGES.map(|s| (s,12)), "signoff":[], "next":null
        }),
    );
    let Some(mut browser) = chrome() else { return };
    browser.viewport(1024, 900, false);
    browser.open(&site.url("/tasks"));
    browser.until(
        "the board, following its feed",
        "document.querySelector('.kanban') !== null && document.querySelector('main')?.dataset.feed==='live'",
    );
    // Mount every real row before narrowing. The first-page placeholder is deliberately taller
    // than a short fetched row; that loading transition must not be mistaken for a reconnect
    // notice moving settled content.
    for status in NOTICE_STAGES {
        mount_notice_rows(&mut browser, status);
    }
    assert_eq!(
        browser.eval("document.querySelectorAll('.taskcard').length"),
        json!(72)
    );
    for width in [300, 390, 1024] {
        let phone = width <= 760;
        let rows = if phone { 12 } else { 72 };
        browser.viewport(width, 900, false);
        browser.until(
            &format!("{width}px: the stage columns drawn"),
            &format!(
                "document.querySelectorAll('.waiting-columns .column').length === {}",
                if phone { 1 } else { 8 }
            ),
        );
        // A change of width remounts the stage lists; mount each shown stage's rows again.
        let shown = if phone {
            &NOTICE_STAGES[4..5]
        } else {
            &NOTICE_STAGES[..]
        };
        for status in shown {
            mount_notice_rows(&mut browser, status);
        }
        browser.eval(&format!("{READER}scrollTo(0,0)"));
        browser.until(
            &format!("{width}px: the shown stages' row slots"),
            &format!("document.querySelectorAll('.waiting-columns [id^=row-]').length === {rows}"),
        );
        browser.until(
            &format!("{width}px: each column's first row drawn"),
            "[...document.querySelectorAll('.waiting-columns .column:not([data-status=build]):not([data-status=test])')].every(column => column.querySelector('[id^=row-]:first-child .taskcard'))",
        );
        browser.eval(&format!(
            "{READER}scrollTo(0,document.documentElement.scrollHeight)"
        ));
        browser.until(
            &format!("{width}px: every shown row drawn at the board's end"),
            &format!("document.querySelectorAll('.taskcard').length === {rows}"),
        );
        browser.eval(&format!("document.documentElement.style.overflowAnchor='none';{READER}scrollTo({{top:1200,behavior:'instant'}});new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))"));
        browser.eval("window.keptScroll=scrollY;window.keptBoard=document.querySelector('.kanban');window.keptBox=JSON.stringify(keptBoard.getBoundingClientRect().toJSON());window.keptRows=[...document.querySelectorAll('.taskcard')];window.keptIds=keptRows.map(e=>e.dataset.slug).join(',')");
        site.server_notice("The review server is reconnecting to its task database.");
        browser.until(
            &format!("{width}px: the server notice shown"),
            "document.querySelector('.page-notices .server-notice')?.textContent.includes('task database') === true",
        );
        assert_notice_surface(&mut browser, ".page-notices .server-notice", "status");
        assert_eq!(browser.eval("scrollY===keptScroll && JSON.stringify(keptBoard.getBoundingClientRect().toJSON())===keptBox"), json!(true), "{width}px: server notice must leave the board box and reading offset exact");
        site.fail_route("/api/task-board", true);
        site.fail_feeds(usize::MAX);
        site.drop_feeds();
        // Every read that fails here (the check's counts, each column's first page) is one the
        // page tries again by itself, and says so.
        browser.until(
            &format!("{width}px: the board-fetch alert, the feed stale"),
            "document.querySelector('main')?.dataset.feed==='stale' && document.querySelector('.page-notices [role=alert]')?.textContent==='Board snapshot unavailable (the server answered 500). Trying again…'",
        );
        assert_notice_surface(&mut browser, ".page-notices [role=alert]", "alert");
        assert_eq!(browser.eval("scrollY===keptScroll && JSON.stringify(keptBoard.getBoundingClientRect().toJSON())===keptBox && [...document.querySelectorAll('.taskcard')].every((e,i)=>e===keptRows[i]) && keptRows.map(e=>e.dataset.slug).join(',')===keptIds"), json!(true), "{width}px: failed snapshots must retain all mounted row identities, the board height and reading offset");
        browser.eval(&format!("{READER}scrollTo(0,document.documentElement.scrollHeight);new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))"));
        assert_eq!(browser.eval(r#"(() => {
            const notice=document.querySelector('.page-notices').getBoundingClientRect();
            const last=[...document.querySelectorAll('.taskcard')].map(card=>card.getBoundingClientRect()).sort((a,b)=>b.bottom-a.bottom)[0];
            return last.height>0 && last.top>=0 && last.bottom<=notice.top && notice.bottom<=innerHeight;
        })()"#), json!(true), "{width}px: the last real card clears the whole notice stack");
        browser.eval("window.failureHeight=document.documentElement.scrollHeight;window.failureScroll=scrollY");
        site.fail_route("/api/task-board", false);
        site.fail_feeds(0);
        browser.until(
            &format!("{width}px: the feed live again, the alert gone"),
            "document.querySelector('main')?.dataset.feed==='live' && !document.querySelector('.page-notices [role=alert]')",
        );
        site.server_notice("");
        browser.until(
            &format!("{width}px: the server notice gone"),
            "!document.querySelector('.page-notices .server-notice')",
        );
        assert_eq!(browser.eval("scrollY===failureScroll && document.documentElement.scrollHeight===failureHeight && [...document.querySelectorAll('.taskcard')].map(e=>e.dataset.slug).join(',')===keptIds"), json!(true), "{width}px: recovery must not clamp the page or lose fetched rows");
    }
}

// A long document's end content clears the reconnect notice at a phone's width and a desk's, and
// two real toasts show whole: above the notice where it floats over the window (1024 px), at their
// own place above the dock where it is in the page's flow (390 px, `noticeGeometry`).
#[test]
fn document_page_end_clears_the_reconnect_notice_and_two_real_toasts_rise_over_a_floating_one() {
    let site = crate::site();
    document(
        &site,
        (0..100)
            .map(|i| {
                paragraph(&format!(
                    "Reading passage {i} keeps document content visible."
                ))
            })
            .collect(),
    );
    let Some(mut browser) = chrome() else { return };
    activity::hold_toast_lifetimes(&mut browser);
    browser.open(&site.url("/docs/PROBE.md"));
    assert!(browser.wait_for("document.querySelector('.document-page')?.dataset.feed==='live' && document.querySelector('[data-doc-key=intro] p')!==null && Array.isArray(window.reviewKnownIds)", SHOWN));
    for (i, width) in [390, 1024].into_iter().enumerate() {
        browser.viewport(width, 900, false);
        browser.eval("document.documentElement.style.overflowAnchor='none';scrollTo(0,document.documentElement.scrollHeight);new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))");
        assert!(browser.wait_for("document.querySelector('#new-ask') !== null && [...document.querySelectorAll('link[rel=stylesheet]')].every(link=>link.sheet!==null)", SHOWN));
        browser.eval("scrollTo(0,document.documentElement.scrollHeight);new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))");
        browser.eval(
            "window.endScroll=scrollY;window.endHeight=document.documentElement.scrollHeight",
        );
        let items: Vec<_> = (0..2)
            .map(|n| {
                json!({
                    "id":format!("notice-{i}-{n}"),"kind":"question","path":"docs/PROBE.md",
                    "slug":format!("Unread notification {i}-{n}"),"ts":"2026-11-02T06:00:00Z",
                    "text":"A real activity notification","href":"/docs/PROBE.md","answered":false
                })
            })
            .collect();
        site.set_json("/api/activity", &json!({"items":items}));
        wait_for_the_feed_on_the_intro(&site);
        let version = 4183 + i as u64;
        site.push(
            version,
            &format!(r#"{{"version":{version},"changes":[{{"region":"activity","data":{{}}}}]}}"#),
        );
        assert!(
            browser.wait_for(
                "document.querySelectorAll('#toasts .pop').length===2",
                SHOWN
            ),
            "both real activity toasts must mount"
        );
        site.fail_feeds(usize::MAX);
        site.drop_feeds();
        assert!(browser.wait_for(
            "document.querySelector('.document-page')?.dataset.feed==='stale'",
            SHOWN
        ));
        let before_paint = browser.eval(r#"({
            exact:scrollY===endScroll && document.documentElement.scrollHeight===endHeight,
            notice:document.querySelector('.page-notices').getBoundingClientRect().toJSON(),
            toasts:[...document.querySelectorAll('#toasts .pop')].map(e=>e.getBoundingClientRect().toJSON()),
            bottom:getComputedStyle(document.documentElement).getPropertyValue('--notice-toast-bottom')
        })"#);
        assert_eq!(
            before_paint["exact"],
            json!(true),
            "{width}px: stale immediately preserves the exact reading offset and page height: {before_paint}"
        );
        // ResizeObserver delivers after layout and before paint. Sample the
        // painted stale geometry at a fixed frame boundary, never by retrying
        // its bounds. The reading offset is asserted both before and after.
        browser.frames();
        let geometry = browser.eval(r#"(() => {
            const stack=document.querySelector('.page-notices');
            const notice=stack.getBoundingClientRect();
            const last=document.querySelector('[data-doc-key=intro] p:last-child').getBoundingClientRect();
            const ask=document.querySelector('#asks button[type=submit]').getBoundingClientRect();
            const toasts=[...document.querySelectorAll('#toasts .pop')].map(e=>e.getBoundingClientRect());
            const clear=b=>b.right<=notice.left || b.left>=notice.right || b.bottom<=notice.top || b.top>=notice.bottom;
            const bottom=getComputedStyle(document.documentElement).getPropertyValue('--notice-toast-bottom');
            const place=getComputedStyle(stack).position==='fixed'
                ? (toasts.every(clear) ? 'above the floating notice' : 'over the floating notice')
                : (bottom==='66px' ? 'their own' : `lifted to ${bottom}`);
            return {checks:{exact:scrollY===endScroll && document.documentElement.scrollHeight===endHeight,
                end:scrollY===Math.max(0,document.documentElement.scrollHeight-innerHeight),
                content:[last,ask].every(b=>b.height>0 && b.top>=0 && b.bottom<=notice.top),
                toasts:toasts.length===2 && toasts.every(b=>b.width>0 && b.height>0 && b.top>=0 && b.bottom<=innerHeight),
                place},
                notice:notice.toJSON(),toasts:toasts.map(b=>b.toJSON()),bottom};
        })()"#);
        let place = if width > 600 {
            "above the floating notice"
        } else {
            "their own"
        };
        assert_eq!(
            geometry["checks"],
            json!({"exact":true,"end":true,"content":true,"toasts":true,"place":place}),
            "{width}px: end content must clear the notice, and two real toasts show whole at their place; before paint {before_paint}; painted {geometry}"
        );
        site.fail_feeds(0);
        assert!(browser.wait_for(
            "document.querySelector('.document-page')?.dataset.feed==='live'",
            SHOWN
        ));
        assert_eq!(
            browser
                .eval("scrollY===endScroll && document.documentElement.scrollHeight===endHeight"),
            json!(true),
            "{width}px: live recovery preserves the same document reserve"
        );
        browser.eval("document.querySelectorAll('#toasts button').forEach(b=>b.click())");
    }
}

#[test]
fn restored_document_native_hash_card_uses_one_anchor_inset() {
    let site = crate::site();
    let mut blocks = vec![
        paragraph("Document introduction."),
        json!({"type":"card","slug":"anchor-card","mark":"pending","title":"Anchor card","body":[paragraph("A question to review.")],"verdict":{"group":"decide","open":true,"posted":null,"decided":null},"thread":[],"line":3,"line_offset":null,"comparison":[]}),
    ];
    // Following content leaves room for the browser to align the card above the page's end.
    blocks.extend((0..40).map(|_| paragraph("Following document content.")));
    document(&site, blocks);
    let Some(mut browser) = chrome() else { return };
    for width in [1280, 390] {
        browser.viewport(width, 900, false);
        browser.open(&site.url("/docs/PROBE.md"));
        assert!(browser.wait_for(
            "location.hash==='' && performance.getEntriesByName('document-ready').length>0 && document.querySelector('.decision#anchor-card') !== null",
            SHOWN
        ));
        // A native hash change applies :target as well as the shared document scroll padding.
        browser.eval("location.hash='anchor-card'");
        assert!(
            browser.wait_for(
                "location.hash==='#anchor-card' && document.querySelector('.decision#anchor-card:target') !== null && Math.abs(document.getElementById('anchor-card').getBoundingClientRect().top-76)<=1",
                SHOWN
            ),
            "at {width} px, the targeted card must sit at the single 76 px inset: {}",
            browser.eval("document.getElementById('anchor-card').getBoundingClientRect().top")
        );
    }
}

#[test]
fn branch_document_comparisons_show_previous_content_and_keep_removed_blocks_read_only() {
    let site = crate::site();
    let path = "@design~policy/docs/PROBE.md";
    let sections = json!([
        {"start":1,"end":2,"key":"added","hash":"a","part":0,"blocks":[paragraph("New paragraph")],"comparison":{"kind":"added"}},
        {"start":3,"end":4,"key":"changed","hash":"c","part":0,"blocks":[paragraph("Revised policy")],"comparison":{"kind":"changed","before":[paragraph("Previous policy")]}},
        {"start":0,"end":0,"key":"removed:old","hash":"r","part":0,"blocks":[paragraph("Removed policy")],"comparison":{"kind":"removed"}},
        {"start":5,"end":6,"key":"card:settled","hash":"s","part":0,"blocks":[{"type":"settled-card","summary":[{"type":"text","text":"Settled history"}],"open":false,"blocks":[paragraph("Settled body")]}],"comparison":{"kind":"changed","before":[paragraph("Previously settled")]}}
    ]);
    let mut outline = sections.clone();
    for section in outline.as_array_mut().unwrap() {
        section["blocks"] = json!([]);
        section["comparison"]["before"] = json!([]);
    }
    site.set_json("/api/document", &json!({"document":{"version":4182,"path":path,"title":"Policy","waiting":[],"crumbs":[],"headings":[],"sections":outline,"cards":[{"slug":"settled","state":"approved","text":"Settled history","group":"resolved","open":false}],"threads":{},"tasks":[],"comparison":{"task":"design/policy"}},"anchors":{}}));
    site.set_json(
        "/api/section",
        &json!({"version":4182,"change":{"path":path,"sections":sections}}),
    );
    site.set_json("/api/asks", &json!({"asks":[],"recipient":null}));
    site.set_json("/api/pending", &json!({"docs":["@design~policy/docs/PROBE.md","docs/NEXT.md"],"items":[{"path":"@design~policy/docs/PROBE.md","slug":"settled","href":"/@design~policy/docs/PROBE.md#settled","branch":"design/policy"},{"path":"docs/NEXT.md","slug":"main-next","href":"/docs/NEXT.md#main-next","branch":null}]}));
    let Some(mut browser) = chrome() else { return };
    browser.open(&site.url("/@design~policy/docs/PROBE.md"));
    assert!(browser.wait_for(
        "document.querySelectorAll('[data-difference]').length===4 && document.querySelector('#settled') !== null && document.querySelectorAll('.section-loading').length===0",
        SHOWN
    ));
    assert_eq!(texts(&mut browser, ".doc-head .eyebrow"), ["docs/PROBE.md"]);
    assert_eq!(
        browser
            .eval("document.querySelector('header .crumbs').textContent.includes('design~policy')"),
        json!(false)
    );
    assert_eq!(
        texts(&mut browser, ".difference-label"),
        ["Added", "Changed", "Removed", "Changed"]
    );
    assert_eq!(
        browser.eval("document.getElementById('settled').open"),
        json!(false)
    );
    browser.eval("document.getElementById('settled').scrollIntoView({block:'center'})");
    assert!(browser.wait_for("(()=>{const r=document.getElementById('settled').getBoundingClientRect();return r.top>=0&&r.top<innerHeight})()", SHOWN));
    browser.eval("document.querySelector('#settled > summary').click()");
    assert!(
        browser.wait_for(
            "document.querySelector('[data-doc-key=\"card:settled\"]') !== null",
            SHOWN
        ),
        "settled state {}; requests {:?}",
        browser.eval("document.getElementById('settled').outerHTML"),
        site.targets()
    );
    assert_eq!(
        browser.eval("document.querySelectorAll('[data-difference]').length"),
        json!(4)
    );
    assert_eq!(
        browser.eval("document.querySelectorAll('#settled').length"),
        json!(1)
    );
    assert_eq!(
        browser.eval("document.querySelector('.document-comparison a').getAttribute('href')"),
        json!("/task/design/policy")
    );
    browser.eval("document.querySelector('.difference-before').open=true");
    assert!(browser.wait_for(
        "document.querySelector('.difference-before').textContent.includes('Previous policy')",
        SHOWN
    ));
    assert_eq!(browser.eval("document.querySelectorAll('.difference-removed button,.difference-removed form').length"), json!(0));
    assert_eq!(browser.eval("document.querySelectorAll('.difference-added button.add,.difference-changed button.add').length"), json!(2));
    assert!(
        site.targets()
            .iter()
            .any(|p| p.starts_with("/@design~policy/api/section"))
    );
    assert_eq!(
        browser.eval("document.querySelector('.app-pages > a').getAttribute('href')"),
        json!("/")
    );
    assert!(site.targets().iter().any(|p| p == "/api/pending"));
    assert!(
        !site
            .targets()
            .iter()
            .any(|p| p.starts_with("/@design~policy/api/pending"))
    );
    browser.eval("location.hash='settled'; document.getElementById('settled').scrollIntoView({block:'center'})");
    assert!(browser.wait_for("location.hash==='#settled' && document.getElementById('settled').getBoundingClientRect().top>0", SHOWN));
    browser.eval(
        "document.querySelector('[data-pending-step=next], [data-pending-action=next]').click()",
    );
    assert!(browser.wait_for(
        "location.pathname==='/docs/NEXT.md' && location.hash==='#main-next'",
        SHOWN
    ));
}

#[test]
fn restored_gallery_loads_only_the_selected_model_and_lightbox_uses_originals_and_keys() {
    let site = crate::site();
    let image = |id: &str| json!({"type":"image","image":{"src":format!("/{id}-thumb.svg"),"alt":id,"presentation":{"attributes":{"width":"200","height":"200","data-full":format!("/{id}-original.svg")}}}});
    document(
        &site,
        vec![
            image("first"),
            image("second"),
            json!({"type":"media","media":{"kind":"gallery","items":[{"name":"FIRST","src":"/first.item?v=one","poster":"/first-thumb.svg"},{"name":"SECOND","src":"/second.item?v=one","poster":"/second-thumb.svg"}],"viewer":viewer::viewer()}}),
        ],
    );
    site.set_file(viewer::SCRIPT, viewer::MODULE.to_vec(), "text/javascript");
    for path in ["/first.item", "/second.item"] {
        site.set_file(path, path.as_bytes().to_vec(), "text/plain");
    }
    for id in ["first", "second"] {
        for size in ["thumb", "original"] {
            site.set_file(&format!("/{id}-{size}.svg"),br#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200"><rect width="200" height="200" fill="teal"/></svg>"#.to_vec(),"image/svg+xml");
        }
    }
    let Some(mut browser) = chrome() else { return };
    browser.open(&site.url("/docs/PROBE.md"));
    assert!(browser.wait_for("document.querySelectorAll('.gallery-pick').length===2 && document.querySelector('.doc-body img[data-zoom-ready]')?.naturalWidth===200",SHOWN),"{}; {:?}",browser.eval("document.body.innerText"),site.targets());
    assert_eq!(
        browser
            .eval("document.querySelectorAll('fixture-viewer,script[data-viewer-module]').length"),
        json!(0)
    );
    assert!(
        !site
            .requests()
            .iter()
            .any(|p| p.ends_with(".item") || p.contains("fixture-viewer"))
    );
    browser.eval("document.querySelector('.doc-body img').focus();document.activeElement.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}))");
    assert!(browser.wait_for("document.querySelector('#media-lightbox[data-state=open] img')?.getAttribute('src')==='/first-original.svg'",SHOWN));
    assert!(browser.wait_for(
        "document.querySelector('#media-lightbox [role=dialog]')?.contains(document.activeElement)",
        SHOWN
    ));
    for _ in 0..6 {
        super::components::key(&mut browser, "Tab", "Tab", 9);
        assert_eq!(browser.eval("document.querySelector('#media-lightbox [role=dialog]').contains(document.activeElement)"), json!(true), "Tab must stay inside the modal picture viewer");
    }
    browser.eval(
        "document.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowLeft',bubbles:true}))",
    );
    assert!(browser.wait_for("document.querySelector('#media-lightbox img')?.getAttribute('src')==='/second-original.svg'",SHOWN));
    browser.eval(
        "document.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowRight',bubbles:true}))",
    );
    assert!(browser.wait_for("document.querySelector('#media-lightbox img')?.getAttribute('src')==='/first-original.svg'",SHOWN));
    super::components::key(&mut browser, "Escape", "Escape", 27);
    assert!(browser.wait_for(
        "document.querySelector('#media-lightbox[data-state=open]') === null",
        SHOWN
    ));
    assert!(browser.wait_for(
        "document.activeElement === document.querySelector('.doc-body img')",
        SHOWN
    ));
    browser.eval("document.querySelector('.gallery-pick').scrollIntoView({block:'center'});document.querySelector('.gallery-pick img').click()");
    assert!(
        browser.wait_for(
            "document.querySelector('fixture-viewer')?.loaded === true",
            SHOWN
        ),
        "{}",
        browser.eval("document.body.innerHTML")
    );
    assert!(site.requests().contains(&"/first.item".into()));
    assert_eq!(
        browser.eval("document.querySelector('#media-lightbox[data-state=open]') !== null"),
        json!(false)
    );
    assert!(!site.requests().contains(&"/second.item".into()));
    browser.eval("document.querySelectorAll('.gallery-pick')[1].scrollIntoView({block:'center'});document.querySelectorAll('.gallery-pick')[1].click()");
    assert!(browser.wait_for("document.querySelectorAll('fixture-viewer').length===1 && document.querySelector('fixture-viewer')?.getAttribute('src')==='/second.item?v=one' && document.querySelector('fixture-viewer').loaded===true",SHOWN));
    assert!(site.requests().contains(&"/second.item".into()));
    assert_eq!(
        site.requests()
            .iter()
            .filter(|p| *p == viewer::SCRIPT)
            .count(),
        1
    );
    let state = "(e => [e.getAttribute('angle'), e.getAttribute('zoom'), e.settled ?? 0])(document.querySelector('fixture-viewer'))";
    browser.eval("document.querySelectorAll('[data-viewer-set]')[0].click()");
    assert_eq!(browser.eval(state), json!(["90", "2", 1]));
    browser.eval("document.querySelectorAll('[data-viewer-set]')[1].click()");
    assert_eq!(browser.eval(state), json!(["-45", "auto", 2]));
    browser.eval("document.querySelectorAll('.gallery-pick')[1].click()");
    assert!(browser.wait_for(
        "document.querySelectorAll('fixture-viewer').length===0",
        SHOWN
    ));
}

/// How many times the page read a document's questions so far.
fn ask_reads(site: &Site) -> usize {
    site.targets()
        .iter()
        .filter(|p| p.starts_with("/api/asks?format=blocks&path="))
        .count()
}

/// The page's reads of a document's questions, as Chrome's network events tell of them.
fn question_reads(browser: &mut super::Chrome) -> Vec<super::chrome::Request> {
    browser
        .requests()
        .into_iter()
        .filter(|request| request.url.contains("/api/asks?format=blocks&path="))
        .collect()
}

/// A read of a document's questions that the feed's news replaces while it waits is aborted: left
/// to answer with nobody to read it, its body went unread, which Chrome counts in flight for as
/// long as the page is open, so `symphony shot` of a document page waited on it until its timeout.
/// The read that replaced it answers and draws, and no read is left in flight.
#[test]
fn a_question_read_the_feeds_news_replaces_is_aborted_not_left_in_flight() {
    let site = crate::site();
    document(&site, vec![paragraph("A passage for a question.")]);
    let ask = json!({"id":7,"path":"docs/PROBE.md","anchor":"","quote":"","question":"Question","asked":1.0,"answers":[],"status":"open","updated":1.0,"messages":[{"seq":1,"kind":"question","by":"owner","text":"","at":1.0,"blocks":[paragraph("A typed question")]}]});
    site.set_json("/api/asks", &json!({"asks":[ask],"recipient":"fixture"}));
    let Some(mut browser) = chrome() else { return };
    browser.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({"source":"window.asksNews=[];addEventListener('review:asks',event=>asksNews.push(event.moved))"}),
    );
    // Every read of the questions waits until the news below has replaced one.
    site.hold_route("/api/asks", true);
    browser.open(&site.url("/docs/PROBE.md"));
    assert!(browser.wait_for(
        "document.querySelector('[data-ready=document] .asks-shell') !== null && asksNews.includes(true)",
        SHOWN
    ));
    browser.eval("document.querySelector('.asks-shell').scrollIntoView({block:'center'})");
    let before = test_time::until("a read of the questions sent", super::STALL, || {
        let sent = question_reads(&mut browser).len();
        (sent > 0).then_some(sent)
    })
    .unwrap_or_else(|stall| panic!("{stall}: {:#?}", question_reads(&mut browser)));
    wait_for_the_feed_on_the_intro(&site);
    site.push(
        4183,
        r#"{"version":4183,"changes":[{"region":"asks","data":{"threads":[{"id":7,"path":"docs/PROBE.md"}]}}]}"#,
    );
    let reads = test_time::until("the held reads replaced and aborted", super::STALL, || {
        let reads = question_reads(&mut browser);
        (reads.len() > before && reads[..before].iter().all(|read| read.failed.is_some()))
            .then_some(reads)
    })
    .unwrap_or_else(|stall| panic!("{stall}: {:#?}", question_reads(&mut browser)));
    for read in &reads[..before] {
        assert!(
            read.canceled && !read.finished && read.status == 0,
            "the page gave the read up before any answer: {read:#?}"
        );
    }
    site.hold_route("/api/asks", false);
    assert!(
        browser.wait_for("document.getElementById('ask-7') !== null", SHOWN),
        "{}",
        browser.eval("document.body.innerText")
    );
    let reads = test_time::until("every read of the questions ended", super::STALL, || {
        let reads = question_reads(&mut browser);
        reads
            .iter()
            .all(|read| read.finished || read.failed.is_some())
            .then_some(reads)
    })
    .unwrap_or_else(|stall| panic!("{stall}: {:#?}", question_reads(&mut browser)));
    let (last, replaced) = reads.split_last().unwrap();
    for read in replaced {
        assert!(read.canceled && !read.finished, "given up: {read:#?}");
    }
    assert!(
        last.finished && last.status == 200,
        "the read that replaced them answered and was read: {last:#?}"
    );
}

#[test]
fn restored_ask_feed_and_navigation_preserve_drafts_while_rendering_typed_answers() {
    let site = crate::site();
    document(&site, vec![paragraph("A passage for a question.")]);
    let mut ask = json!({"id":7,"path":"docs/PROBE.md","anchor":"","quote":"","question":"Question","asked":1.0,"answers":[],"status":"open","updated":1.0,"messages":[{"seq":1,"kind":"question","by":"owner","text":"","at":1.0,"blocks":[paragraph("Original typed answer")]}]});
    site.set_json("/api/asks", &json!({"asks":[ask],"recipient":"fixture"}));
    let Some(mut browser) = chrome() else { return };
    // Every timer of the asks' retry the page sets is held here: an answered read must leave none
    // waiting. The page's news of the asks is kept from its start.
    super::hold_ask_polls(&mut browser);
    browser.call("Page.addScriptToEvaluateOnNewDocument",json!({"source":"window.asksNews=[];addEventListener('review:asks',event=>asksNews.push(event.moved))"}));
    browser.open(&site.url("/docs/PROBE.md"));
    // Document hydration performs its initial anchor scroll. Wait for that readiness marker
    // before revealing the ask thread, or its later scroll can move the deferred thread away.
    // The opened stream's news is in before the thread reads, so it reads once.
    assert!(browser.wait_for(
        "document.querySelector('[data-ready=document] .asks-shell') !== null && asksNews.includes(true)",
        SHOWN
    ));
    browser.eval("document.querySelector('.asks-shell').scrollIntoView({block:'center'})");
    assert!(
        browser.wait_for("document.getElementById('ask-7') !== null", SHOWN),
        "{}; {:?}",
        browser.eval("document.body.innerText"),
        site.targets()
    );
    assert_eq!(
        browser.eval("askPoll.size"),
        json!(0),
        "a read that answered waits for the feed, not a timer"
    );
    let read = ask_reads(&site);
    browser.eval("document.getElementById('ask-7').scrollIntoView({block:'center'})");
    assert!(browser.wait_for("document.getElementById('reply-7') !== null", SHOWN));
    browser.eval("for(const [id,value] of [['new-ask','Question draft'],['reply-7','Follow-up draft']]){const t=document.getElementById(id);t.value=value;t.dispatchEvent(new Event('input',{bubbles:true}))} document.activeElement.blur()");
    ask["messages"].as_array_mut().unwrap().push(json!({"seq":2,"kind":"answer","by":"agent","text":"","at":2.0,"blocks":[paragraph("Updated typed answer"),{"type":"code","language":"rust","text":"let answer = 42;"}]}));
    site.set_json("/api/asks", &json!({"asks":[ask],"recipient":"fixture"}));
    // Another document's thread moving is no reason to read this one's again.
    browser.eval("window.asksHeard=0;addEventListener('review:asks',()=>asksHeard++)");
    wait_for_the_feed_on_the_intro(&site);
    site.push(
        4183,
        r#"{"version":4183,"changes":[{"region":"asks","data":{"threads":[{"id":8,"path":"docs/OTHER.md"}]}}]}"#,
    );
    assert!(browser.wait_for("asksHeard===1", SHOWN));
    site.push(
        4184,
        r#"{"version":4184,"changes":[{"region":"asks","data":{"threads":[{"id":7,"path":"docs/PROBE.md"}]}}]}"#,
    );
    assert!(browser.wait_for(
        "document.querySelector('.ask-message pre code')?.textContent==='let answer = 42;'",
        SHOWN
    ));
    assert_eq!(
        ask_reads(&site),
        read + 1,
        "one read, for this document's thread alone"
    );
    assert_eq!(browser.eval("askPoll.size"), json!(0));
    assert_eq!(
        browser.eval(
            "[document.getElementById('new-ask').value,document.getElementById('reply-7').value]"
        ),
        json!(["Question draft", "Follow-up draft"])
    );
    browser.eval("document.getElementById('new-ask').form.requestSubmit();document.getElementById('reply-7').form.requestSubmit()");
    assert!(browser.wait_for("document.querySelectorAll('.asks form [role=alert]').length===2 && !document.getElementById('new-ask').disabled && !document.getElementById('reply-7').disabled",SHOWN));
    assert_eq!(
        browser.eval(
            "[document.getElementById('new-ask').value,document.getElementById('reply-7').value]"
        ),
        json!(["Question draft", "Follow-up draft"])
    );
    let posts = site.posts();
    assert!(posts.iter().any(|(path, body)| path == "/api/ask"
        && body["question"] == "Question draft"
        && body["path"] == "docs/PROBE.md"));
    assert!(posts.iter().any(|(path, body)| path == "/api/ask-reply"
        && body["text"] == "Follow-up draft"
        && body["id"] == "7"));
    browser.eval("document.querySelector('header a[href=\"/\"]').click()");
    super::wait_for_board(&mut browser);
    browser.eval("history.back()");
    assert!(browser.wait_for(
        "document.querySelector('[data-ready=document] .asks-shell') !== null",
        SHOWN
    ));
    browser.eval("document.querySelector('.asks-shell').scrollIntoView({block:'center'})");
    assert!(browser.wait_for("document.getElementById('ask-7') !== null", SHOWN));
    browser.eval("document.getElementById('ask-7').scrollIntoView({block:'center'})");
    assert!(browser.wait_for("document.getElementById('reply-7') !== null", SHOWN));
    assert_eq!(
        browser.eval(
            "[document.getElementById('new-ask').value,document.getElementById('reply-7').value]"
        ),
        json!(["Question draft", "Follow-up draft"])
    );
}

/// A script true once the document page has read every section it mounted, and no placeholder's
/// sentinel (600 px around it) is left on the screen to mount another: what it wants is what it
/// holds, and nothing a scroll brought near is still due.
const SETTLED: &str = "document.querySelector('.section-loading') === null && [...document.querySelectorAll('.viewport-sentinel')].every(sentinel => { const box = sentinel.getBoundingClientRect(); return box.bottom <= 0 || box.top >= innerHeight })";

/// A long document reopens its stream for the sections each scroll brings near the screen, from
/// the cursor of the last event it heard, from which the server sends every event after it: such
/// a reopen missed nothing, and reads no question, chat or activity again. A stream lost is read
/// again from the outline, and its open reads each of them once more.
#[test]
fn a_document_s_own_stream_reopens_while_scrolling_read_no_ask_or_chat_until_a_loss() {
    const QUESTIONS: &str = "/api/asks?format=blocks&path=docs%2FLONG.md";
    const CHAT: &str = "/api/ask?format=blocks&id=7";
    let site = crate::site();
    let sections: Vec<Value> = (0..40)
        .map(|at| {
            let blocks: Vec<Value> = (0..6)
                .map(|line| paragraph(&format!("Section {at}, passage {line}: a long document is read a screen at a time.")))
                .collect();
            json!({"start": at * 10 + 1, "end": at * 10 + 9, "key": format!("s{at}"), "hash": format!("h{at}"), "part": null, "blocks": blocks})
        })
        .collect();
    site.set_document(&json!({"version":4182,"path":"docs/LONG.md","title":"Long","waiting":[],"crumbs":[],"headings":[],"sections":sections,"cards":[],"threads":{},"tasks":[]}));
    let ask = json!({"id":7,"path":"docs/LONG.md","anchor":"","quote":"","question":"Question","asked":1.0,"answers":[],"status":"open","updated":1.0,"messages":[{"seq":1,"kind":"question","by":"owner","text":"A typed question","at":1.0,"blocks":[paragraph("A typed question")]}]});
    site.set_json("/api/asks", &json!({"asks":[ask],"recipient":"fixture"}));
    site.set_json("/api/ask", &json!({"ask":ask,"recipient":"fixture"}));
    site.set_json("/api/postponed", &json!([]));
    let Some(mut browser) = chrome() else { return };
    // The chat's draft names thread 7, and the page's news of the asks is kept from its start.
    browser.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({"source":"localStorage.setItem('review-quick-draft',JSON.stringify({open:false,task:false,ask:7}));window.asksNews=[];addEventListener('review:asks',event=>asksNews.push(event.moved))"}),
    );
    browser.open(&site.url("/docs/LONG.md"));
    assert!(
        browser.wait_for(
            "document.querySelector('[data-ready=document] [data-doc-key=s0] p') !== null && document.querySelector('main > .document-page')?.dataset.feed === 'live' && asksNews.includes(true)",
            SHOWN
        ),
        "{}",
        browser.eval("document.body.innerText.slice(0, 2000)")
    );
    browser.eval("document.querySelector('#quick-open').click()");
    assert!(
        browser.wait_for(
            "[...document.querySelectorAll('#quick-sheet .quick-message p:not(.by)')].some(p => p.textContent === 'A typed question')",
            SHOWN
        ),
        "the chat opens on its thread: {}",
        browser.eval("document.getElementById('quick-sheet')?.innerText ?? null")
    );
    // The questions, at the document's end, are read once they come near the screen.
    browser.eval("document.querySelector('.asks-shell').scrollIntoView({block:'center'})");
    assert!(browser.wait_for(
        &format!("document.getElementById('ask-7') !== null && {SETTLED}"),
        SHOWN
    ));
    // Each scroll up to a section not drawn yet draws it and the sections beside it, and opens the
    // stream again for exactly the sections the page then holds.
    for at in [30, 22, 14, 8] {
        let before = site.feed_requests();
        browser.eval(&format!(
            "document.getElementById('L{}').scrollIntoView({{block:'start'}})",
            at * 10 + 1
        ));
        assert!(
            browser.wait_for(
                &format!("document.querySelector('[data-doc-key=s{at}] p') !== null && {SETTLED}"),
                SHOWN
            ),
            "s{at}: {}",
            browser.eval("document.body.innerText.slice(0, 2000)")
        );
        test_time::until(format!("the stream opened again for s{at}"), super::STALL, || {
            let held: Vec<String> = serde_json::from_value(browser.eval(
                "[...document.querySelectorAll('[data-doc-key]')].map(section => section.dataset.docKey).sort()",
            ))
            .unwrap();
            let mut subscribed: Vec<String> = site
                .targets()
                .iter()
                .rfind(|target| target.starts_with(symphony_api::FEED_PATH))
                .and_then(|target| super::site::query(target, "sections"))
                .and_then(|keys| serde_json::from_str(&keys).ok())
                .unwrap_or_default();
            subscribed.sort();
            let asked = site.feed_requests();
            (asked > before
                && site.opened_feeds() == asked
                && held.contains(&format!("s{at}"))
                && subscribed == held)
                .then_some(())
        })
        .unwrap_or_else(|stall| panic!("{stall}: {:?}", site.targets()));
    }
    let asked = |path: &str| {
        site.targets()
            .iter()
            .filter(|target| target.starts_with(path))
            .count()
    };
    let reads = || {
        [
            asked(QUESTIONS),
            asked(CHAT),
            site.requests()
                .iter()
                .filter(|path| *path == "/api/activity")
                .count(),
        ]
    };
    // A stream lost: the page reads the outline again and follows the feed from it, and the
    // stream's open reads the questions, the chat and the activity once more.
    site.drop_feeds();
    let after = test_time::until(
        "the stream lost, opened again and read after",
        super::STALL,
        || {
            let after = reads();
            let news = browser.eval("asksNews.filter(moved => moved === true).length");
            (after.iter().all(|count| *count >= 2) && news.as_u64() >= Some(2)).then_some(after)
        },
    )
    .unwrap_or_else(|stall| {
        panic!(
            "{stall}: reads {:?}, news {}",
            reads(),
            browser.eval("JSON.stringify(asksNews)")
        )
    });
    assert_eq!(
        after,
        [2, 2, 2],
        "the questions, the chat and the activity were each read once before the loss and once after it, never for a stream the page opened again: {:?}",
        site.targets()
    );
    assert_eq!(
        browser.eval("asksNews.filter(moved => moved === true).length"),
        json!(2),
        "the stream's first open and the one after the loss alone told the readers to read"
    );
}
