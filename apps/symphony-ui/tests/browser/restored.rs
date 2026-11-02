//! Restored native browser contracts. Security cases use the real server, never fixture CSP.
use super::{SHOWN, binaries, chrome, common, dist, site::Site, texts};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

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
        // Had before the readiness clock below starts; the owned child is this server itself.
        let binary = binaries::symphony_server();
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "sf-browser-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        for dir in [
            "docs/ships",
            "ui",
            "view/models/hulls",
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
            root.join("docs/ships/HULLS.md"),
            "# Hulls\n\n<div data-hull-gallery></div>\n",
        )
        .unwrap();
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
            root.join("view/models/hulls/FIRST.glb"),
            "gallery record, never selected",
        )
        .unwrap();
        std::os::unix::fs::symlink(root.join("private.html"), root.join("ui/inside.html")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("ui/outside.html")).unwrap();
        for args in [
            vec!["init", "-b", "fixture"],
            vec!["config", "user.name", "Browser fixture"],
            vec!["config", "user.email", "browser@example.invalid"],
            vec!["config", "core.hooksPath", "/dev/null"],
            vec!["add", "."],
            vec![
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                "Browser fixture",
            ],
        ] {
            let done = Command::new("git")
                .args(&args)
                .current_dir(&root)
                .env_remove("GIT_DIR")
                .env_remove("GIT_COMMON_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_INDEX_FILE")
                .output()
                .unwrap();
            assert!(done.status.success(), "fixture {args:?}: {done:?}");
        }
        Self::serve(root, outside, binary, board)
    }

    pub(super) fn checkout(root: PathBuf, binary: &std::path::Path) -> Self {
        let outside = root.with_extension("outside.html");
        Self::serve(root, outside, binary, None)
    }

    fn serve(
        root: PathBuf,
        outside: PathBuf,
        binary: &std::path::Path,
        board: Option<&str>,
    ) -> Self {
        // Board writes reach only the caller's owned scratch server, or a closed endpoint.
        let log = std::fs::File::create(root.join("artifacts/server.log")).unwrap();
        let child = Command::new("bash")
            .args([
                "-c",
                r#". "$1"; fixture_close_fds; shift; exec "$@""#,
                "native-fixture",
            ])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../tools/fixture_process.sh"
            ))
            .arg(binary)
            .args(["serve", "--root"])
            .arg(&root)
            .args(["--port", "0", "--host", "127.0.0.1"])
            .env("SF_SYMPHONY_UI_DIST", dist())
            .env("TASKS_SERVER", board.unwrap_or("127.0.0.1:1"))
            .env("TASKS_DATABASE", "browser_owner_controls")
            .env("TASKS_SWEEP", "off")
            .env("ASKS_DB", root.join("artifacts/asks.db"))
            .env("SF_SYMPHONY_TEST_PID", std::process::id().to_string())
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
        native.base = recv.recv_timeout(SHOWN).unwrap_or_else(|e| {
            panic!(
                "native server readiness: {e}; {}",
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
        stream.set_read_timeout(Some(SHOWN)).unwrap();
        stream
            .write_all(
                format!("GET {target} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .unwrap();
        let mut status = String::new();
        BufReader::new(stream).read_line(&mut status).unwrap();
        status.split_whitespace().nth(1).unwrap().parse().unwrap()
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
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
    browser.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":420,"height":900,"deviceScaleFactor":1,"mobile":false}),
    );
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
        assert!(
            browser
                .requests()
                .iter()
                .any(|r| r.url.ends_with(".wasm") && r.status == 200)
        );
        assert!(
            !browser
                .requests()
                .iter()
                .any(|r| r.url.contains("/app/assets/"))
        );
    }
    browser.open(&site.url("/hulls"));
    assert!(browser.wait_for("document.querySelectorAll('.hull-pick').length===1", SHOWN));
    assert_eq!(texts(&mut browser, ".hull-pick strong"), ["FIRST"]);
    std::fs::write(
        site.root.join("view/models/hulls/SECOND.glb"),
        "new gallery record",
    )
    .unwrap();
    assert!(
        browser.wait_for("document.querySelectorAll('.hull-pick').length===2", SHOWN),
        "watcher did not invalidate the warm gallery: {}",
        std::fs::read_to_string(site.root.join("artifacts/server.log")).unwrap()
    );
    assert_eq!(
        texts(&mut browser, ".hull-pick strong"),
        ["FIRST", "SECOND"]
    );
    std::fs::remove_file(site.root.join("view/models/hulls/FIRST.glb")).unwrap();
    assert!(
        browser.wait_for("document.querySelectorAll('.hull-pick').length===1 && document.querySelector('.hull-pick strong')?.textContent==='SECOND'", SHOWN),
        "removed hull remains: page={}; server={}; log={}",
        browser.eval("document.body.innerHTML"),
        browser.eval("fetch('/api/document?path=docs/ships/HULLS.md').then(r=>r.json())"),
        std::fs::read_to_string(site.root.join("artifacts/server.log")).unwrap()
    );
    assert_eq!(
        browser.eval("document.querySelectorAll('model-viewer').length"),
        json!(0)
    );
}

fn queue_fixture() -> Value {
    let mut card: Value = serde_json::from_str(&common::fixture("board.json")).unwrap();
    let mut first = card["next"][0].take();
    first["slug"] = json!("first");
    first["title"] = json!("First decision");
    first["doc"] = json!("@topic/docs/PROBE.md");
    let mut second = first.clone();
    second["slug"] = json!("second");
    second["title"] = json!("Second decision");
    second["task"] = json!("topic");
    let item = |slug: &str| json!({"slug":slug,"path":"@topic/docs/PROBE.md","title":slug,"href":format!("/@topic/docs/PROBE.md#{slug}"),"kind":"question","text":"Review it","branch":null});
    json!({"waiting":[second,first],"spot_check":[item("spot")],"agent":[item("agent")],"branches":[]})
}

fn paragraph(text: &str) -> Value {
    json!({"type":"paragraph","spans":[{"type":"text","text":text}]})
}

#[test]
fn restored_document_pictures_load_near_the_viewport_and_stay_mounted_after_scrolling_away() {
    let site = Site::start(dist(), &common::fixture("board.json"));
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
        browser.eval("document.querySelector('.doc-body img').hasAttribute('src')"),
        json!(false)
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

#[test]
fn restored_document_native_hash_card_uses_one_anchor_inset() {
    let site = Site::start(dist(), &common::fixture("board.json"));
    let mut blocks = vec![
        paragraph("Document introduction."),
        json!({"type":"card","slug":"anchor-card","mark":"pending","title":"Anchor card","body":[paragraph("A question to review.")],"verdict":{"group":"decide","open":true,"posted":null,"decided":null},"thread":[],"line":3,"line_offset":null,"comparison":[]}),
    ];
    // Following content leaves room for the browser to align the card above the page's end.
    blocks.extend((0..40).map(|_| paragraph("Following document content.")));
    document(&site, blocks);
    let Some(mut browser) = chrome() else { return };
    for width in [1280, 390] {
        browser.call(
            "Emulation.setDeviceMetricsOverride",
            json!({"width":width,"height":900,"deviceScaleFactor":1,"mobile":false}),
        );
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
    let site = Site::start(dist(), &common::fixture("board.json"));
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
        browser.eval("document.querySelector('.app-header > a').getAttribute('href')"),
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
    let site = Site::start(dist(), &common::fixture("board.json"));
    let image = |id: &str| json!({"type":"image","image":{"src":format!("/{id}-thumb.svg"),"alt":id,"presentation":{"attributes":{"width":"200","height":"200","data-full":format!("/{id}-original.svg")}}}});
    document(
        &site,
        vec![
            image("first"),
            image("second"),
            json!({"type":"media","media":{"kind":"gallery","hulls":[{"id":"FIRST","model":"/first.glb?v=one","picture":"/first-thumb.svg"},{"id":"SECOND","model":"/second.glb?v=one","picture":"/second-thumb.svg"}]}}),
        ],
    );
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    site.set_file(
        "/static/model-viewer.min.js",
        std::fs::read(root.join("tools/static/model-viewer.min.js")).unwrap(),
        "text/javascript",
    );
    let model = std::fs::read(root.join("view/models/hulls/EMP-DD01.glb")).unwrap();
    for path in ["/first.glb", "/second.glb"] {
        site.set_file(path, model.clone(), "model/gltf-binary");
    }
    for id in ["first", "second"] {
        for size in ["thumb", "original"] {
            site.set_file(&format!("/{id}-{size}.svg"),br#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200"><rect width="200" height="200" fill="teal"/></svg>"#.to_vec(),"image/svg+xml");
        }
    }
    let Some(mut browser) = chrome() else { return };
    browser.open(&site.url("/docs/PROBE.md"));
    assert!(browser.wait_for("document.querySelectorAll('.hull-pick').length===2 && document.querySelector('.doc-body img[data-zoom-ready]')?.naturalWidth===200",SHOWN),"{}; {:?}",browser.eval("document.body.innerText"),site.targets());
    assert_eq!(
        browser.eval("document.querySelectorAll('model-viewer,#review-model-viewer').length"),
        json!(0)
    );
    assert!(
        !site
            .requests()
            .iter()
            .any(|p| p.ends_with(".glb") || p.contains("model-viewer"))
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
    browser.eval("document.querySelector('.hull-pick').scrollIntoView({block:'center'});document.querySelector('.hull-pick img').click()");
    assert!(
        browser.wait_for(
            "document.querySelector('model-viewer')?.loaded === true",
            SHOWN
        ),
        "{}",
        browser.eval("document.body.innerHTML")
    );
    assert!(site.requests().contains(&"/first.glb".into()));
    assert_eq!(
        browser.eval("document.querySelector('#media-lightbox[data-state=open]') !== null"),
        json!(false)
    );
    assert!(!site.requests().contains(&"/second.glb".into()));
    browser.eval("document.querySelectorAll('.hull-pick')[1].scrollIntoView({block:'center'});document.querySelectorAll('.hull-pick')[1].click()");
    assert!(browser.wait_for("document.querySelectorAll('model-viewer').length===1 && document.querySelector('model-viewer')?.getAttribute('src')==='/second.glb?v=one' && document.querySelector('model-viewer').loaded===true",SHOWN));
    assert!(site.requests().contains(&"/second.glb".into()));
    assert_eq!(
        site.requests()
            .iter()
            .filter(|p| *p == "/static/model-viewer.min.js")
            .count(),
        1
    );
    browser.eval("document.querySelector('[data-glb-view=tilt]').click()");
    assert_eq!(
        browser.eval("document.querySelector('model-viewer').getAttribute('camera-orbit')"),
        json!("0deg 25deg auto")
    );
    browser.eval("document.querySelector('[data-glb-view=reset]').click()");
    assert_eq!(
        browser.eval("document.querySelector('model-viewer').getAttribute('camera-orbit')"),
        json!("-45deg 60deg auto")
    );
    assert_eq!(
        browser.eval("document.querySelector('model-viewer').getAttribute('field-of-view')"),
        json!("auto")
    );
    browser.eval("document.querySelectorAll('.hull-pick')[1].click()");
    assert!(browser.wait_for(
        "document.querySelectorAll('model-viewer').length===0",
        SHOWN
    ));
}

#[test]
fn restored_ask_polling_and_navigation_preserve_drafts_while_rendering_typed_answers() {
    let site = Site::start(dist(), &common::fixture("board.json"));
    document(&site, vec![paragraph("A passage for a question.")]);
    let mut ask = json!({"id":7,"path":"docs/PROBE.md","anchor":"","quote":"","question":"Question","asked":1.0,"answers":[],"status":"open","updated":1.0,"messages":[{"seq":1,"kind":"question","by":"owner","text":"","at":1.0,"blocks":[paragraph("Original typed answer")]}]});
    site.set_json("/api/asks", &json!({"asks":[ask],"recipient":"fixture"}));
    let Some(mut browser) = chrome() else { return };
    browser.call("Page.addScriptToEvaluateOnNewDocument",json!({"source":"window.askPoll=[];const timer=window.setTimeout;window.setTimeout=function(fn,ms,...args){if(ms===3000){askPoll.push(()=>fn(...args));return 0}return timer(fn,ms,...args)}"}));
    browser.open(&site.url("/docs/PROBE.md"));
    // Document hydration performs its initial anchor scroll. Wait for that readiness marker
    // before revealing the ask thread, or its later scroll can move the deferred thread away.
    assert!(browser.wait_for(
        "document.querySelector('[data-ready=document] .asks-shell') !== null",
        SHOWN
    ));
    browser.eval("document.querySelector('.asks-shell').scrollIntoView({block:'center'})");
    assert!(
        browser.wait_for(
            "document.getElementById('ask-7') !== null && askPoll.length===1",
            SHOWN
        ),
        "{}; {:?}",
        browser.eval("document.body.innerText"),
        site.targets()
    );
    browser.eval("document.getElementById('ask-7').scrollIntoView({block:'center'})");
    assert!(browser.wait_for("document.getElementById('reply-7') !== null", SHOWN));
    browser.eval("for(const [id,value] of [['new-ask','Question draft'],['reply-7','Follow-up draft']]){const t=document.getElementById(id);t.value=value;t.dispatchEvent(new Event('input',{bubbles:true}))} document.activeElement.blur()");
    ask["messages"].as_array_mut().unwrap().push(json!({"seq":2,"kind":"answer","by":"agent","text":"","at":2.0,"blocks":[paragraph("Updated typed answer"),{"type":"code","language":"rust","text":"let answer = 42;"}]}));
    site.set_json("/api/asks", &json!({"asks":[ask],"recipient":"fixture"}));
    browser.eval("askPoll.shift()()");
    assert!(browser.wait_for(
        "document.querySelector('.ask-message pre code')?.textContent==='let answer = 42;'",
        SHOWN
    ));
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
    assert!(
        site.targets()
            .iter()
            .any(|p| p.starts_with("/api/asks?format=blocks&path="))
    );
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

#[test]
fn queue_stylesheet_failure_keeps_the_previous_page_and_allows_retry() {
    let site = Site::start(dist(), &common::fixture("board.json"));
    let Some(mut browser) = chrome() else { return };
    browser.open(&site.url("/"));
    super::wait_for_board(&mut browser);
    browser.call(
        "Network.setBlockedURLs",
        json!({"urls":["*/assets/queue-*.css"]}),
    );
    browser.eval(
        "history.pushState(null, '', '/queue'); dispatchEvent(new PopStateEvent('popstate'))",
    );
    assert!(browser.wait_for(
        "document.documentElement.dataset.navigation === 'failed'",
        SHOWN
    ));
    assert_eq!(browser.eval("document.querySelector('.hero-title') !== null && document.getElementById('you') === null"), json!(true),
        "a stylesheet failure must leave the previous page mounted");
    browser.call("Network.setBlockedURLs", json!({"urls":[]}));
    browser.eval("document.querySelector('#module-error button').click()");
    assert!(browser.wait_for("document.documentElement.dataset.navigation === 'ready' && document.querySelector('[data-ready=queue] #you') !== null", SHOWN),
        "retry must fetch the failed stylesheet and mount the queue");
    // queue.css overrides the generic card padding only when its rules actually apply.
    assert_eq!(
        browser.eval("getComputedStyle(document.getElementById('you')).paddingTop"),
        json!("6px"),
        "the retried queue stylesheet must apply when the queue is ready"
    );
}

#[test]
fn restored_queue_counts_order_scope_anchors_and_refresh_keep_reading_position() {
    let site = Site::start(dist(), &common::fixture("board.json"));
    site.set_json("/api/postpone", &json!({"ok":true,"changed":true}));
    let mut queue = queue_fixture();
    site.set_json("/api/queue", &queue);
    // The fixture reserves enough space to align the last section before any module runs.
    let shell = std::fs::read_to_string(dist().join("index.html")).unwrap();
    site.set_file(
        "/queue",
        shell
            .replace(
                "</head>",
                "<style>.queue-group{min-height:900px}</style></head>",
            )
            .into_bytes(),
        "text/html; charset=utf-8",
    );
    let Some(mut browser) = chrome() else { return };
    browser.call("Page.addScriptToEvaluateOnNewDocument", json!({"source":r#"
        window.heldStyles = [];
        const append = Element.prototype.append;
        Element.prototype.append = function(...nodes) {
            if (this === document.head && nodes.length === 1 && nodes[0].matches?.('link[rel=stylesheet]')) {
                heldStyles.push(nodes[0]);
            } else { append.apply(this, nodes); }
        };
        window.releaseStyles = () => {
            Element.prototype.append = append;
            for (const link of heldStyles) append.call(document.head, link);
        };
        Object.defineProperty(window, 'reviewModule_queue', { configurable: true, set(module) {
            const run = module.run;
            module.run = function() {
                window.queueStartedWithStyles = heldStyles.length > 0 && heldStyles.every(link => link.sheet !== null);
                return run.apply(this, arguments);
            };
            Object.defineProperty(window, 'reviewModule_queue', { value: module });
        }});
    "#}));
    browser.open(&site.url("/@topic/queue#agent"));
    assert!(browser.wait_for("heldStyles.length > 0", SHOWN));
    browser.eval("releaseStyles()");
    assert!(browser.wait_for(
        "document.querySelector('#agent .queue-row') !== null",
        SHOWN
    ));
    assert_eq!(
        browser.eval("queueStartedWithStyles"),
        json!(true),
        "the queue must mount only after its delayed styles apply"
    );
    // The shared document inset reserves 76 px once, including on phones where the header
    // scrolls away. Derive only the heading's offset inside the section from its box styles.
    let anchored = r#"(() => {
        const section = document.getElementById('agent'), heading = section.querySelector('h2');
        const box = getComputedStyle(section), text = getComputedStyle(heading);
        const inside = parseFloat(box.borderTopWidth) + parseFloat(box.paddingTop) + parseFloat(text.marginTop);
        return Math.abs(heading.getBoundingClientRect().top - (76 + inside)) <= 1;
    })()"#;
    assert!(
        browser.wait_for(anchored, SHOWN),
        "the addressed heading must match the single anchor inset within one pixel"
    );
    for width in [1280, 390] {
        browser.call(
            "Emulation.setDeviceMetricsOverride",
            json!({"width":width,"height":900,"deviceScaleFactor":1,"mobile":false}),
        );
        browser.eval("document.getElementById('agent').scrollIntoView()");
        assert!(
            browser.wait_for(anchored, SHOWN),
            "at {width} px, the heading must sit one 76 px inset plus its internal offset down, within one pixel"
        );
    }
    browser.call("Emulation.clearDeviceMetricsOverride", json!({}));
    // The restored viewport's resize must reach layout before the section is aligned.
    browser.eval("new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))");
    browser.eval("document.getElementById('agent').scrollIntoView()");
    assert!(
        browser.wait_for(anchored, SHOWN),
        "the heading must match the single anchor inset after restoring the viewport: {}",
        browser.eval("(()=>{const s=document.getElementById('agent'),h=s.querySelector('h2');return {viewport:[innerWidth,innerHeight],heading:h.getBoundingClientRect().top,section:s.getBoundingClientRect().top,scroll:scrollY,padding:getComputedStyle(document.documentElement).scrollPaddingTop,margin:getComputedStyle(s).scrollMarginTop}})()")
    );
    assert_eq!(
        texts(&mut browser, ".queue-group h2"),
        [
            "Waiting on you (2)",
            "Auto-approved: spot-check (1)",
            "Waiting on the agent (1)"
        ]
    );
    assert_eq!(
        texts(&mut browser, "#you li[data-card]"),
        vec!["", ""],
        "far rows stay deferred"
    );
    assert_eq!(browser.eval("[...document.querySelectorAll('#you [data-card]')].map(e=>e.dataset.card.split('#').pop())"),json!(["second","first"]));
    assert!(
        site.targets()
            .iter()
            .any(|p| p.starts_with("/@topic/api/queue"))
    );
    assert_eq!(
        browser.eval("document.querySelector('#agent .queue-row a').getAttribute('href')"),
        json!("/@topic/docs/PROBE.md#agent")
    );
    browser.eval("document.getElementById('you').scrollIntoView()");
    assert!(browser.wait_for(
        "document.querySelectorAll('#you .queue-row').length===2",
        SHOWN
    ));
    assert_eq!(
        texts(&mut browser, "#you .queue-row a:not(.chip)"),
        ["Second decision", "First decision"]
    );
    assert_eq!(texts(&mut browser, "#you .queue-rank"), ["1", "2"]);
    assert_eq!(
        texts(&mut browser, "#you .postpone-controls button"),
        ["Later", "Later"]
    );
    assert_eq!(texts(&mut browser, "#you .chip"), ["topic"]);
    assert_eq!(
        browser.eval("document.querySelector('#you .chip').getAttribute('href')"),
        json!("/task/topic")
    );
    assert_eq!(
        browser.eval("document.querySelector('#you .queue-row a').getAttribute('href')"),
        json!("/@topic/docs/PROBE.md#second")
    );
    browser.eval("document.querySelector('#you .postpone-controls button').click()");
    assert!(browser.wait_for(
        "performance.getEntriesByType('resource').some(e=>new URL(e.name).pathname==='/api/postpone')",
        SHOWN
    ));
    assert!(
        site.posts()
            .iter()
            .any(|(path, body)| path == "/api/postpone"
                && body["path"] == "@topic/docs/PROBE.md"
                && body["slug"] == "second"
                && body["action"] == "later")
    );
    browser.eval("document.getElementById('agent').scrollIntoView()");
    assert!(
        browser.wait_for(anchored, SHOWN),
        "returning to the agent section must align its heading within one pixel"
    );
    assert!(super::wait_for_feed(&site));
    browser.eval("window.scrollBy(0,100);window.keptScroll=scrollY;window.keptRow=document.querySelector('#agent li[data-card]')");
    queue["waiting"] = json!([]);
    queue["spot_check"] = json!([]);
    queue["branches"] = json!([]);
    site.set_json("/api/queue", &queue);
    site.push(
        4183,
        &json!({"version":4183,"changes":[{"region":"queue-item-order","data":{"key":"@topic/docs/PROBE.md#first","after":null}}]}).to_string(),
    );
    assert!(browser.wait_for(
        "document.querySelector('#you h2')?.textContent==='Waiting on you (0)'",
        SHOWN
    ));
    assert_eq!(
        browser.eval(
            "scrollY===keptScroll && document.querySelector('#agent li[data-card]')===keptRow"
        ),
        json!(true)
    );
    assert_eq!(
        texts(&mut browser, "#you li, #spot li"),
        ["nothing to decide", "none"]
    );
    assert_eq!(
        browser.eval("document.getElementById('branches')===null"),
        json!(true)
    );
    for (index, region) in [
        "queue-item",
        "queue-item-order",
        "spot-check",
        "spot-check-order",
        "agent-queue",
        "agent-queue-order",
    ]
    .into_iter()
    .enumerate()
    {
        let title = format!("Updated via {region}");
        queue["agent"][0]["title"] = json!(title);
        site.set_json("/api/queue", &queue);
        let data = if region.ends_with("-order") {
            json!({"key":"fixture","after":null})
        } else {
            json!({"key":"fixture","removed":true})
        };
        let version = 4184 + index as u64;
        site.push(
            version,
            &json!({"version":version,"changes":[{"region":region,"data":data}]}).to_string(),
        );
        assert!(
            browser.wait_for(
                &format!(
                    "document.querySelector('#agent .queue-row a')?.textContent==={}",
                    json!(title)
                ),
                SHOWN
            ),
            "{region} did not refresh the queue"
        );
        assert_eq!(
            browser.eval(
                "scrollY===keptScroll && document.querySelector('#agent li[data-card]')===keptRow"
            ),
            json!(true),
            "{region}"
        );
    }
    queue["agent"] = json!([]);
    site.set_json("/api/queue", &queue);
    browser
        .eval("window.keptAgentTop=document.getElementById('agent').getBoundingClientRect().top");
    // Keep the outage stable until it is observed, rather than racing the reconnect backoff.
    site.fail_feeds(usize::MAX);
    site.drop_feeds();
    assert!(browser.wait_for(
        "document.querySelector('main')?.dataset.feed==='stale'",
        SHOWN
    ));
    assert_eq!(
        browser.eval("scrollY===keptScroll"),
        json!(true),
        "entering the stale state must keep the reading position"
    );
    assert_eq!(
        browser.eval("document.getElementById('agent').getBoundingClientRect().top===keptAgentTop"),
        json!(true),
        "the stale notice must not move the section, even if native scroll anchoring is inactive"
    );
    site.fail_feeds(0);
    assert!(
        browser.wait_for("document.querySelector('#agent h2')?.textContent==='Waiting on the agent (0)' && document.querySelector('main')?.dataset.feed==='live'",SHOWN),
        "after reconnecting, the queue must show no agent work and a live feed"
    );
    assert_eq!(browser.eval("scrollY===keptScroll"), json!(true));
    assert_eq!(
        texts(&mut browser, "#agent li"),
        ["every comment is answered"]
    );
}
