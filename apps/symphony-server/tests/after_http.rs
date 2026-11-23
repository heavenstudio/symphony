//! HTTP contracts against the native router, private repositories and stores.

#[expect(
    dead_code,
    reason = "Only the private database client is needed by this HTTP fixture"
)]
#[path = "../../../crates/tasks/tests/store/common.rs"]
mod board;
#[path = "after_http/client.rs"]
mod client;
#[path = "after_http/owner_save.rs"]
mod owner_save;
#[path = "../../../crates/symphony/tests/common/tree.rs"]
mod tree;
#[path = "after_http/validators.rs"]
mod validators;

use client::{Feed, Fixture};
use serde_json::{Value, json};
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use symphony_board::sys::Command;
use symphony_server::front::{Shared, router};
use symphony_server::gate::Gate;
use symphony_server::shelf;

const CHILD: &str = "SYMPHONY_AFTER_HTTP_CHILD";
/// The wall-clock backstop of every wait of this target, six times the 30 s it had as a bound, which
/// only a stall reaches (`test_time::backstop`, RULES.md `tests/no-test-fails-on-load`).
const LIMIT: Duration = test_time::backstop(Duration::from_secs(30));

/// A wait for `what` that ran past [`LIMIT`]: a stall, and the machine's load then.
fn stalled(what: impl std::fmt::Display) -> String {
    format!(
        "{what}: stalled: the wait's wall-clock backstop of {LIMIT:?} ran out; {}",
        test_time::Load::now()
    )
}
const DOCUMENT: &str = "/api/document?path=docs/TOPIC.md";
const SCOPE: &str = "/@work~http";
const PHASE: &str = "after_http phase: ";
/// Where the fixture project, keeping the default artifacts folder, holds the queue order.
const ORDER: &str = "artifacts/review-order/postponed.json";
const DOC: &str = "# HTTP fixture\n\n[Other](OTHER.md)\n\n<!-- register:start -->\n- [ ] **one** — A question.\n<!-- register:end -->\n";

fn repository(root: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    let done = symphony_board::sys::output(
        command
            .args(args)
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1"),
    )
    .unwrap();
    assert!(done.status.success(), "{args:?}: {done:?}");
    String::from_utf8(done.stdout).unwrap().trim().into()
}

fn write(root: &Path, relative: &str, bytes: impl AsRef<[u8]>) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn fixture() -> tree::Tree {
    let mut tree = tree::Tree::new();
    tree.root = tree.root.canonicalize().unwrap();
    let root = tree.path("repo");
    std::fs::create_dir(&root).unwrap();
    // HTTP persistence is observed in files/stores. Main publication belongs to restore-commit.
    repository(&root, &["init", "-b", "http-fixture"]);
    repository(&root, &["config", "user.name", "HTTP fixture"]);
    repository(&root, &["config", "user.email", "http@example.invalid"]);
    repository(&root, &["config", "core.hooksPath", "/dev/null"]);
    write(&root, "docs/TOPIC.md", DOC);
    write(&root, "docs/OTHER.md", "# Other\n");
    write(&root, "symphony.toml", shelf::MANIFEST);
    write(&root, shelf::DOC, "# Shelf\n\n[[shelf:all]]\n");
    write(
        &root,
        "room/shelf/items/A.item",
        b"item fixture A".repeat(300),
    );
    write(&root, "artifacts/raw.svg", "<svg>private fixture</svg>");
    write(
        &root,
        "artifacts/large.svg",
        "<svg>large fixture</svg>".repeat(300),
    );
    write(&root, "artifacts/raw.png", b"PNG fallback fixture");
    write(&root, "tools/static/icon.svg", "<svg>icon fixture</svg>");
    write(&root, "data/custom.unknown", b"unknown extension download");
    write(
        &root,
        "build/symphony-ui/public/index.html",
        format!("<html><body>{}</body></html>", "Native shell. ".repeat(300)),
    );
    write(
        &root,
        "build/symphony-ui/public/assets/app-0123456789abcdef.js",
        "console.log('fixture');\n".repeat(100),
    );
    repository(&root, &["add", "."]);
    repository(&root, &["commit", "-qm", "Initial HTTP fixtures"]);
    repository(
        &root,
        &[
            "worktree",
            "add",
            "-qb",
            "work/http",
            tree.path("branch").to_str().unwrap(),
        ],
    );
    write(
        &tree.path("branch"),
        "docs/TOPIC.md",
        DOC.replace("HTTP fixture", "Branch fixture"),
    );
    write(
        &tree.path("branch"),
        "artifacts/raw.png",
        b"branch PNG fallback",
    );
    // Links into metadata/outside each root must never become downloadable documents or media.
    write(&root, ".git/hidden.md", "metadata secret");
    write(&root, ".git/hidden.png", "metadata image");
    write(&tree.root, "outside.svg", "outside secret");
    std::os::unix::fs::symlink(tree.path("outside.svg"), root.join("artifacts/outside.svg"))
        .unwrap();
    std::os::unix::fs::symlink(root.join(".git/hidden.md"), root.join("docs/METADATA.md")).unwrap();
    std::os::unix::fs::symlink(
        root.join(".git/hidden.png"),
        root.join("artifacts/metadata.png"),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        tree.path("outside.svg"),
        root.join("build/symphony-ui/public/assets/outside.svg"),
    )
    .unwrap();
    tree
}

fn run(test: &str, exercise: fn(&Fixture)) {
    run_on(test, fixture, exercise);
}

/// `exercise` against a server on the repository `made` builds, in a child process of its own.
fn run_on(test: &str, made: fn() -> tree::Tree, exercise: fn(&Fixture)) {
    if std::env::var_os(CHILD).is_none() {
        let began = Instant::now();
        let tree = made();
        let client = board::client();
        // The board's command line makes the board; the Symphony, a reader, never does.
        symphony_board::Store::open(symphony_board::rpc::Client::new(
            client.addr(),
            "sf",
            client.database(),
        ))
        .unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        match test {
            "native_low_disk_configured_threshold_notice" => {
                command.env("SYMPHONY_LOW_DISK_GB", "1000000000000");
            }
            "native_low_disk_zero_threshold_notice" => {
                command.env("SYMPHONY_LOW_DISK_GB", "0");
            }
            "native_low_disk_malformed_threshold_falls_back" => {
                command.env("SYMPHONY_LOW_DISK_GB", "malformed-fixture");
            }
            _ => {}
        }
        let done = symphony_board::sys::output(
            command
                .args(["--exact", test, "--nocapture"])
                .env(CHILD, &tree.root)
                .env("TASKS_SERVER", client.addr())
                .env("TASKS_DATABASE", client.database())
                .env("ASKS_DB", tree.path("asks.db"))
                .env_remove("TASKS_SESSION")
                .env_remove("SYMPHONY_UI_DIST")
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_INDEX_FILE")
                .env_remove("GIT_COMMON_DIR")
                .current_dir(tree.path("repo")),
        )
        .unwrap();
        let out = String::from_utf8_lossy(&done.stdout);
        let err = String::from_utf8_lossy(&done.stderr);
        if symphony_env::PROFILE.os().is_some() {
            for line in err.lines().filter(|line| {
                line.starts_with("→ native read data warmed:")
                    || line.starts_with("· review timing: initial")
                    || line.starts_with("· review timing: hub new:")
            }) {
                eprintln!("{line}");
            }
        }
        assert!(done.status.success(), "{out}\n{err}");
        if test == "native_low_disk_malformed_threshold_falls_back" {
            assert!(
                err.lines().any(|line| line == "· SYMPHONY_LOW_DISK_GB=\"malformed-fixture\" is not a number of gigabytes: the pages warn under 20 GB free"),
                "{err}"
            );
        }
        assert!(
            out.contains("1 passed"),
            "exact child filter ran no assertions: {out}"
        );
        if test == "watcher_refreshes_documents_without_gallery_work_and_items_only_in_their_tree" {
            let phases: Vec<_> = err
                .split(PHASE)
                .skip(1)
                .map(|phase| {
                    (
                        phase.lines().next().unwrap(),
                        phase
                            .lines()
                            .filter(|line| line.starts_with("→ shelf warmed:"))
                            .collect::<Vec<_>>(),
                    )
                })
                .collect();
            assert_eq!(
                phases.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
                ["document", "branch item", "root item", "finished"],
                "{err}"
            );
            assert!(
                phases[0].1.is_empty(),
                "document refresh rebuilt a gallery: {err}"
            );
            for (index, root) in [(1, tree.path("branch")), (2, tree.path("repo"))] {
                assert!(
                    !phases[index].1.is_empty(),
                    "an item edit never warmed a gallery: {err}"
                );
                assert!(
                    phases[index]
                        .1
                        .iter()
                        .all(|line| line.contains(&format!("{}:", root.display()))),
                    "an item refresh warmed its neighbour: {err}"
                );
            }
            assert!(
                phases[3].1.is_empty(),
                "late gallery work escaped its phase: {err}"
            );
        }
        println!(
            "{out}\n{test} wall time {:.3}s",
            began.elapsed().as_secs_f64()
        );
        return;
    }
    // The child serves the shelf as a host serves its plugins.
    shelf::install().unwrap();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let root = PathBuf::from(std::env::var_os(CHILD).unwrap()).join("repo");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        assert_ne!(address.port(), 8765);
        let shared = Shared::new(
            i64::from(std::process::id()),
            address.port(),
            &root,
            None,
            Gate::new(
                Arc::new(|_| panic!("loopback needs no peer lookup")),
                Arc::new(Instant::now),
            ),
        );
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let app = router(shared.clone()).into_make_service_with_connect_info::<SocketAddr>();
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        let result = tokio::task::spawn_blocking(move || {
            let f = Fixture { root, address };
            // Successful root read is the readiness handshake, including tree discovery/watch setup.
            assert_eq!(f.get(DOCUMENT).json()["title"], "HTTP fixture");
            exercise(&f);
        })
        .await;
        shared.stop();
        stop.send(()).unwrap();
        server.await.unwrap();
        result.unwrap();
    });
}

#[test]
fn native_routes_media_and_persistence() {
    run("native_routes_media_and_persistence", contracts);
}

/// A project whose documents are in `manual/`, beside a stray `docs/` folder that is no document.
fn manual_fixture() -> tree::Tree {
    let mut tree = tree::Tree::new();
    tree.root = tree.root.canonicalize().unwrap();
    let root = tree.path("repo");
    std::fs::create_dir(&root).unwrap();
    repository(&root, &["init", "-b", "http-fixture"]);
    repository(&root, &["config", "user.name", "HTTP fixture"]);
    repository(&root, &["config", "user.email", "http@example.invalid"]);
    repository(&root, &["config", "core.hooksPath", "/dev/null"]);
    write(
        &root,
        "symphony.toml",
        "[[entrypoints]]\nurl = \"/docs/\"\nfolder = \"manual\"\n",
    );
    write(&root, "manual/TOPIC.md", DOC);
    write(&root, "manual/OTHER.md", "# Other\n");
    write(&root, "manual/pictures/a.png", b"PNG in the manual");
    write(&root, "docs/TOPIC.md", "# Stray\n");
    repository(&root, &["add", "."]);
    repository(&root, &["commit", "-qm", "Manual fixtures"]);
    tree
}

#[test]
fn a_projects_own_documents_folder_is_read_and_saved_by_route() {
    run_on(
        "a_projects_own_documents_folder_is_read_and_saved_by_route",
        manual_fixture,
        manual_routes,
    );
}

fn manual_routes(f: &Fixture) {
    let doc = f.get(DOCUMENT).status(200).json();
    assert_eq!(doc["path"], "docs/TOPIC.md");
    assert_eq!(doc["cards"][0]["slug"], "one");
    f.get("/api/document?path=manual/TOPIC.md").status(400);
    // A route no file has finds the document of its name, by route, wherever its folder is.
    f.get("/api/document?path=docs/old/OTHER.md").status(404);
    for (route, moved) in [
        ("docs/old/OTHER.md", json!(["docs/OTHER.md"])),
        ("docs/OTHER.md", json!([])),
        ("docs/old/GONE.md", json!([])),
    ] {
        let path = format!("/api/moved?path={route}");
        assert_eq!(f.get(&path).status(200).json(), moved, "{route}");
    }
    f.get("/api/moved?path=manual/OTHER.md").status(400);
    let versions = f.get("/api/versions?path=docs/TOPIC.md").status(200).json();
    assert_eq!(versions["versions"].as_array().unwrap().len(), 1);
    assert_eq!(
        f.get("/docs/pictures/a.png").status(200).body,
        b"PNG in the manual"
    );
    let decision = json!({"path":"docs/TOPIC.md", "slug":"one", "state":"approved", "note":"saved in the manual"});
    assert_eq!(
        f.post("/api/decision", &decision).status(200).json()["ok"],
        true
    );
    let saved = std::fs::read_to_string(f.root.join("manual/TOPIC.md")).unwrap();
    assert!(
        saved.contains("owner · approved — saved in the manual"),
        "{saved}"
    );
    assert_eq!(
        std::fs::read_to_string(f.root.join("docs/TOPIC.md")).unwrap(),
        "# Stray\n"
    );
}

#[test]
fn later_order_is_shared_without_document_writes_and_reloaded_by_a_new_server() {
    run(
        "later_order_is_shared_without_document_writes_and_reloaded_by_a_new_server",
        later_order,
    );
}

fn later_cards(f: &Fixture) -> (String, Feed) {
    let original =
        format!("{DOC}\n- [ ] **two** — Another question.\n- [ ] **three** — Third question.\n");
    let mut feed = f.feed("");
    write(&f.root, "docs/TOPIC.md", &original);
    feed.until("three");
    (original, feed)
}

fn later_order(f: &Fixture) {
    let (original, mut feed) = later_cards(f);
    let baseline = f.get("/api/board").status(200).json()["counts"]["waiting"].clone();
    let action = json!({"path":"docs/TOPIC.md","slug":"one","action":"later"});
    for path in ["../outside.md", "docs/METADATA.md"] {
        f.post(
            "/api/postpone",
            &json!({"path":path,"slug":"one","action":"later"}),
        )
        .status(400);
    }
    f.request(
        "POST",
        "/api/postpone",
        &[("Origin", "null")],
        action.to_string().as_bytes(),
    )
    .status(403);
    let reply = f.post("/api/postpone", &action).status(200).json();
    assert_eq!(reply["next"], "/docs/TOPIC.md#two");
    feed.until("queue-item-order");
    let queue = f.get("/api/queue").status(200).json();
    assert_eq!(
        queue["waiting"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["slug"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["two", "three", "one"]
    );
    assert_eq!(queue["waiting"][2]["postponed"], true);
    assert_eq!(
        f.get("/api/board").status(200).json()["counts"]["waiting"],
        baseline
    );
    assert_eq!(
        std::fs::read_to_string(f.root.join("docs/TOPIC.md")).unwrap(),
        original
    );
    assert!(!f.root.join("docs/DECISIONS.md").exists());
    // A new server state reads the persisted file, with no state carried from the live hub.
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        use axum::http::Request;
        use tower::ServiceExt;
        let shared = Shared::new(
            1,
            1234,
            &f.root,
            None,
            Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
        );
        let response = router(shared.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/queue")
                    .header("Host", "localhost:1234")
                    .extension(axum::extract::ConnectInfo(
                        "127.0.0.1:1234".parse::<SocketAddr>().unwrap(),
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let bytes = axum::body::to_bytes(response.into_body(), 1_000_000)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap()["waiting"],
            queue["waiting"]
        );
        shared.stop();
    });
    for (slug, changed) in [("two", false), ("three", true)] {
        let visit = f
            .post(
                "/api/postpone",
                &json!({"path":"docs/TOPIC.md", "slug":slug, "action":"seen"}),
            )
            .status(200)
            .json();
        assert_eq!(visit["changed"], changed);
        assert!(visit["next"].is_null());
    }
    let restored = f.get("/api/queue").status(200).json();
    assert_eq!(restored["waiting"][0]["slug"], "one");
    assert!(
        restored["waiting"]
            .as_array()
            .unwrap()
            .iter()
            .all(|card| card["postponed"] != true)
    );
    assert_eq!(
        std::fs::read_to_string(f.root.join("docs/TOPIC.md")).unwrap(),
        original
    );
    assert_eq!(
        f.post("/api/postpone", &action).status(200).json()["changed"],
        true
    );
    f.post(
        "/api/decision",
        &json!({"path":"docs/TOPIC.md","slug":"one","state":"approved"}),
    )
    .status(200);
    assert!(
        !std::fs::read_to_string(symphony::postpone::order_file(&f.root))
            .unwrap()
            .contains("one")
    );
    write(&f.root, ORDER, "corrupt preferences");
    f.post(
        "/api/decision",
        &json!({"path":"docs/TOPIC.md","slug":"two","state":"approved"}),
    )
    .status(200);
    assert!(
        std::fs::read_to_string(f.root.join("docs/TOPIC.md"))
            .unwrap()
            .contains("[x] **two**")
    );
    write(
        &f.root,
        "docs/SHELF.md",
        "# Shelf\n\nCorrupt preference recovery\n",
    );
    feed.until("Corrupt preference recovery");
    assert_eq!(
        f.get("/api/board").status(200).json()["counts"]["waiting"],
        1
    );
    let queue = f.get("/api/queue").status(200).json();
    assert_eq!(queue["waiting"][0]["slug"], "three");
    assert_ne!(queue["waiting"][0]["postponed"], true);
    f.get(DOCUMENT).status(200);
    assert_eq!(
        std::fs::read_to_string(symphony::postpone::order_file(&f.root)).unwrap(),
        "corrupt preferences"
    );
}

#[test]
fn later_expiry_restores_order_without_document_edits() {
    run(
        "later_expiry_restores_order_without_document_edits",
        later_expiry,
    );
}

fn later_expiry(f: &Fixture) {
    let (original, _) = later_cards(f);
    f.post(
        "/api/postpone",
        &json!({"path":"docs/TOPIC.md", "slug":"one", "action":"later"}),
    )
    .status(200);
    let before = f.get("/api/queue").status(200).json();
    assert_eq!(before["waiting"][0]["slug"], "two");
    assert_eq!(before["waiting"][2]["postponed"], true);
    let mut feed = f.socket("");
    let path = symphony::postpone::order_file(&f.root);
    let mut order: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    order["owners"]["owner"]["one"]["until"] = json!(0);
    std::fs::write(&path, order.to_string()).unwrap();
    // Only the expiry timer watches this state; no document notification is sent.
    feed.until("queue-item-order");
    let after = f.get("/api/queue").status(200).json();
    assert_eq!(
        after["waiting"]
            .as_array()
            .unwrap()
            .iter()
            .map(|card| card["slug"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["one", "two", "three"]
    );
    assert!(
        after["waiting"]
            .as_array()
            .unwrap()
            .iter()
            .all(|card| card["postponed"] != true)
    );
    assert_eq!(
        f.get("/api/board").status(200).json()["counts"]["waiting"],
        3
    );
    assert_eq!(
        std::fs::read_to_string(f.root.join("docs/TOPIC.md")).unwrap(),
        original
    );
    assert!(!std::fs::read_to_string(path).unwrap().contains("one"));
}

fn contracts(f: &Fixture) {
    let shell = f.get("/").status(200);
    assert!(shell.text().contains("Native shell."));
    // The fixture has no docs/DECISIONS.md: its page names no register, so its home links none.
    let project = format!(
        "<html><body><script type=\"application/json\" id=\"symphony-project\">{}</script>",
        json!({
            "name": f.root.file_name().unwrap().to_str().unwrap(),
            "register": "",
            "nav": [{"label": "▤ shelf", "title": "Shelf items", "href": "/shelf", "document": "docs/SHELF.md"}],
            "entrypoints": ["docs"],
        })
    );
    assert!(shell.text().starts_with(&project), "{}", shell.text());
    assert_eq!(shell.header("content-type"), "text/html; charset=utf-8");
    assert_eq!(shell.header("cache-control"), "no-cache");
    for route in [
        "/tasks",
        "/task",
        "/agents",
        "/queue",
        "/shelf/",
        "/task/fixture/one",
        "/docs/TOPIC.md",
        "/docs",
        "/@work~http/docs/TOPIC.md",
    ] {
        assert_eq!(f.get(route).status(200).body, shell.body, "{route}");
    }
    for (route, target) in [
        ("/app", "/"),
        ("/app/tasks?q=a%20b", "/tasks?q=a%20b"),
        ("/doc/docs/TOPIC.md", "/docs/TOPIC.md"),
        ("/history/docs/TOPIC.md", "/docs/TOPIC.md"),
        ("/diff/abc/docs/TOPIC.md?x=1", "/docs/TOPIC.md?x=1"),
        (
            "/@work~http/doc/docs/A%20%25%3F.md?q=x%2By",
            "/@work~http/docs/A%20%25%3F.md?q=x%2By",
        ),
    ] {
        assert_eq!(f.get(route).status(308).header("location"), target);
    }
    let scoped = f.get(&format!("{SCOPE}{DOCUMENT}")).status(200);
    assert_eq!(scoped.json()["title"], "Branch fixture");
    assert!(
        scoped.text().contains("/@work~http/docs/OTHER.md"),
        "{scoped:?}"
    );
    assert_eq!(
        f.get("/api/document?path=%40work~http%2Fdocs%2FTOPIC.md")
            .status(200)
            .json(),
        scoped.json()
    );
    f.get("/@unknown/docs/TOPIC.md").status(404);
    f.get("/@unknown/api/document?path=docs/TOPIC.md")
        .status(404);
    f.get("/@work~http/api/document?path=%40other%2Fdocs%2FTOPIC.md")
        .status(400);
    misses(f);
    client_decodes_document_reads(f);
    for route in [
        "/",
        DOCUMENT,
        "/api/board",
        "/assets/app-0123456789abcdef.js",
        "/artifacts/raw.svg",
        "/room/shelf/items/A.item",
        "/x/shelf/A.item",
    ] {
        let get = f.get(route).status(200);
        let head = f.request("HEAD", route, &[], &[]).status(200);
        assert!(head.body.is_empty(), "{route}: {head:?}");
        assert_eq!(head.header("content-type"), get.header("content-type"));
        assert_eq!(
            get.header("content-length"),
            get.body.len().to_string(),
            "{route}"
        );
        assert_eq!(
            head.header("content-length"),
            get.header("content-length"),
            "{route}"
        );
        f.request("PUT", route, &[], &[]).status(405);
    }
    for route in [
        "/",
        "/assets/app-0123456789abcdef.js",
        "/room/shelf/items/A.item",
        "/x/shelf/A.item",
        "/artifacts/raw.svg",
    ] {
        let get = f.get(route).status(200);
        let etag = get.header("etag");
        for method in ["GET", "HEAD"] {
            for tag in [etag.to_string(), format!("\"other\", W/{etag}"), "*".into()] {
                let cached = f
                    .request(method, route, &[("If-None-Match", &tag)], &[])
                    .status(304);
                assert!(cached.body.is_empty());
                assert_eq!(cached.header("etag"), etag);
                assert_eq!(cached.header("cache-control"), get.header("cache-control"));
            }
            let multiple = f
                .request(
                    method,
                    route,
                    &[("If-None-Match", "\"other\""), ("If-None-Match", etag)],
                    &[],
                )
                .status(304);
            assert!(multiple.body.is_empty());
            let fresh = f
                .request(method, route, &[("If-None-Match", "\"stale\"")], &[])
                .status(200);
            assert_eq!(
                fresh.body,
                if method == "HEAD" {
                    &[]
                } else {
                    get.body.as_slice()
                }
            );
        }
    }
    assert_eq!(f.get("/queue").status(200).body, shell.body);
    assert!(
        f.request("GET", "/queue", &[("Accept", "application/json")], &[])
            .status(200)
            .json()
            .is_object()
    );
    for encoding in ["identity", "gzip;q=0", "br", "gzip;q=0, identity;q=1"] {
        let response = f
            .request("GET", "/", &[("Accept-Encoding", encoding)], &[])
            .status(200);
        assert!(
            !response.headers.contains_key("content-encoding"),
            "{encoding}: {response:?}"
        );
        assert_eq!(response.body, shell.body);
        let head = f
            .request("HEAD", "/", &[("Accept-Encoding", encoding)], &[])
            .status(200);
        assert!(head.body.is_empty(), "{encoding}: {head:?}");
        assert!(
            !head.headers.contains_key("content-encoding"),
            "{encoding}: {head:?}"
        );
        assert_eq!(head.header("content-length"), shell.body.len().to_string());
        assert_eq!(head.header("vary"), response.header("vary"));
    }
    let compressed = f
        .request(
            "GET",
            "/",
            &[("Accept-Encoding", "br;q=1, gzip;q=0.8")],
            &[],
        )
        .status(200);
    assert_eq!(compressed.header("content-encoding"), "gzip");
    assert!(
        compressed
            .header("vary")
            .to_ascii_lowercase()
            .contains("accept-encoding")
    );
    for route in [
        "/",
        "/assets/app-0123456789abcdef.js",
        DOCUMENT,
        "/api/board",
        "/artifacts/large.svg",
        "/room/shelf/items/A.item",
        "/x/shelf/A.item",
    ] {
        let get = if route == "/" {
            &compressed
        } else {
            &f.request(
                "GET",
                route,
                &[("Accept-Encoding", "br;q=1, gzip;q=0.8")],
                &[],
            )
            .status(200)
        };
        assert_eq!(get.header("content-encoding"), "gzip", "{route}");
        let head = f
            .request(
                "HEAD",
                route,
                &[("Accept-Encoding", "br;q=1, gzip;q=0.8")],
                &[],
            )
            .status(200);
        assert!(head.body.is_empty(), "{route}: {head:?}");
        for header in ["content-encoding", "vary", "content-type", "cache-control"] {
            assert_eq!(head.header(header), get.header(header), "{route}: {header}");
        }
        assert_eq!(
            head.header("content-length"),
            get.body.len().to_string(),
            "{route}"
        );
    }
    let cached_headers = [
        ("Accept-Encoding", "gzip"),
        ("If-None-Match", shell.header("etag")),
    ];
    let cached_get = f.request("GET", "/", &cached_headers, &[]).status(304);
    let cached = f.request("HEAD", "/", &cached_headers, &[]).status(304);
    assert!(cached.body.is_empty());
    assert_eq!(cached.header("etag"), shell.header("etag"));
    assert!(!cached.headers.contains_key("content-encoding"));
    assert_eq!(
        cached.headers.get("content-length"),
        cached_get.headers.get("content-length")
    );
    let mut decoder = symphony_board::sys::spawn_piped(Command::new("gzip").arg("-dc")).unwrap();
    let mut input = decoder.stdin.take().unwrap();
    let body = &compressed.body;
    // Written beside the read: gzip answers while it reads, and a pipe holds only so much. The
    // writer moves into its thread, so gzip reads its end when the write is done.
    let decoded = std::thread::scope(|scope| {
        let written = scope.spawn(move || input.write_all(body));
        let decoded = symphony_board::sys::wait_output(decoder).unwrap();
        written.join().unwrap().unwrap();
        decoded
    });
    assert!(decoded.status.success());
    assert_eq!(decoded.stdout, shell.body);
    assert_eq!(
        f.get("/assets/app-0123456789abcdef.js")
            .status(200)
            .header("cache-control"),
        "public, max-age=31536000, immutable"
    );
    for (route, mime, body) in [
        (
            "/artifacts/raw.svg",
            "image/svg+xml",
            b"<svg>private fixture</svg>".as_slice(),
        ),
        (
            "/favicon.ico",
            "image/svg+xml",
            b"<svg>icon fixture</svg>".as_slice(),
        ),
        (
            "/data/custom.unknown",
            "application/octet-stream",
            b"unknown extension download".as_slice(),
        ),
    ] {
        let raw = f.get(route).status(200);
        assert_eq!(raw.body, body);
        assert_eq!(raw.header("content-type"), mime);
        assert_eq!(
            raw.header("content-security-policy"),
            "sandbox allow-scripts"
        );
        assert_eq!(raw.header("x-content-type-options"), "nosniff");
        assert_eq!(raw.header("content-length"), body.len().to_string());
        assert_eq!(raw.header("cache-control"), "no-cache");
    }
    let raw = f.get("/artifacts/raw.svg").status(200);
    let version = raw.header("etag").trim_matches('"');
    assert_eq!(
        f.get(&format!("/artifacts/raw.svg?v={version}"))
            .status(200)
            .header("cache-control"),
        "public, max-age=31536000, immutable"
    );
    assert_eq!(
        f.get("/artifacts/raw.svg?v=stale")
            .status(200)
            .header("cache-control"),
        "no-cache"
    );
    write(
        &f.root,
        "artifacts/raw.svg",
        "<svg>updated and longer fixture</svg>",
    );
    let changed = f
        .request(
            "GET",
            "/artifacts/raw.svg",
            &[("If-None-Match", raw.header("etag"))],
            &[],
        )
        .status(200);
    assert_ne!(changed.header("etag"), raw.header("etag"));
    assert_eq!(changed.body, b"<svg>updated and longer fixture</svg>");
    media_refusals_and_thumbnail(f);
    persistence(f);
    task_pages(f);
    validators::json_reads(f);
    // Removing the installed shell must give actionable instructions without restarting a server.
    std::fs::remove_file(f.root.join("build/symphony-ui/public/index.html")).unwrap();
    assert!(
        f.get("/")
            .status(503)
            .text()
            .contains("just symphony-ui-dist")
    );
}

/// The Symphony client decodes `/api/document?view=outline` as `DocumentOutline`; a body without
/// the `document` wrapper reaches the owner as "missing field `document`". Decode what the real
/// router answers, for the gallery document, a plain document and a branch scope, with the
/// client's own types.
/// Every miss is one answer: a page in the app's look off the API, saying what is missing and
/// where to go, and a JSON error under it, 400 for an argument missing or malformed and 404 for a
/// thing; a save route read says it takes POST.
fn misses(f: &Fixture) {
    for (route, status, said) in [
        ("/nonexistent", 404, "<code>/nonexistent</code>"),
        ("/docs/%ZZ", 400, "This address cannot be read"),
        (
            "/artifacts/uploads/nope.png",
            404,
            "<code>/artifacts/uploads/nope.png</code>",
        ),
        ("/room/shelf/items/missing.item", 404, "Nothing is here"),
        ("/.git/config", 404, "<code>/.git/config</code>"),
        (
            "/@unknown/tasks?x=1",
            404,
            "<a href=\"/tasks?x=1\">Open this page in the main checkout</a>",
        ),
        (
            "/@unknown/docs/TOPIC.md",
            404,
            "The worktree of branch <q>unknown</q> is gone",
        ),
        // A server of its own links the same page by its path alone, never another site.
        (
            "/@unknown//evil.example",
            404,
            "<a href=\"/evil.example\">Open this page",
        ),
        (
            "/@unknown/%2F%2Fevil.example",
            404,
            "<a href=\"/evil.example\">Open this page",
        ),
    ] {
        let page = f.get(route).status(status);
        assert!(!page.text().contains("href=\"//"), "{route}");
        assert_eq!(
            page.header("content-type"),
            "text/html; charset=utf-8",
            "{route}"
        );
        assert_eq!(page.header("cache-control"), "no-store", "{route}");
        let text = page.text();
        assert!(
            text.contains("<header class=\"app-header\""),
            "{route}: {text}"
        );
        assert!(text.contains(said), "{route}: {text}");
        assert!(
            text.contains("<a href=\"/docs/\">Docs</a>"),
            "{route}: {text}"
        );
        // The same miss read as a page's subresource or by HEAD is the same answer.
        let head = f.request("HEAD", route, &[], &[]).status(status);
        assert!(head.body.is_empty(), "{route}");
        assert_eq!(head.header("content-type"), "text/html; charset=utf-8");
    }
    for (route, status, error) in [
        ("/api/nope", 404, "no such API route: /api/nope"),
        ("/@work~http/api/nope", 404, "no such API route: /api/nope"),
        (
            "/@unknown/api/document?path=docs/TOPIC.md",
            404,
            "no worktree for this branch",
        ),
        ("/api/folder", 400, "path is required"),
        (
            "/api/document?path=docs/%00.md",
            400,
            "path is not valid: it holds a control character",
        ),
        (
            "/api/document?path=docs/%FF.md",
            400,
            "path is not valid: its percent-encoding is not UTF-8",
        ),
        ("/api/folder?path=", 404, "not found: the path is empty"),
        (
            "/api/task-page?slug=%FF",
            400,
            "slug is not valid: its percent-encoding is not UTF-8",
        ),
        ("/api/task-page?slug=", 400, "slug is required"),
        ("/api/task-thread", 400, "slug is required"),
    ] {
        let refused = f.get(route).status(status);
        assert_eq!(
            refused.header("content-type"),
            "application/json",
            "{route}"
        );
        assert_eq!(refused.json(), json!({"error": error}), "{route}");
    }
    let unknown = f.get("/api/task-board?status=nope").status(400);
    let said = unknown.json()["error"].as_str().unwrap().to_string();
    assert!(
        said.starts_with("no status \"nope\": a listing selects one of backlog, concept,")
            && said.ends_with(", signoff, build, test, or in view=waiting phase:PHASE"),
        "{said}"
    );
    f.get("/api/task-board?status=review").status(200);
    for (method, route, allowed) in [
        ("GET", "/api/task-cancel", "POST"),
        ("GET", "/@work~http/api/decision", "POST"),
        ("GET", "/api/upload", "POST"),
        ("PUT", "/api/ask", "GET, HEAD, POST"),
    ] {
        let refused = f.request(method, route, &[], &[]).status(405);
        assert_eq!(refused.header("allow"), allowed, "{route}");
        assert_eq!(
            refused.json(),
            json!({"error": format!("this route takes {allowed}")}),
            "{route}"
        );
    }
}

fn client_decodes_document_reads(f: &Fixture) {
    for scope in ["", SCOPE] {
        for path in ["docs/SHELF.md", "docs/TOPIC.md"] {
            let outline = f
                .get(&format!("{scope}/api/document?path={path}&view=outline"))
                .status(200);
            let outline: symphony_api::DocumentOutline = serde_json::from_str(outline.text())
                .unwrap_or_else(|why| panic!("{scope}{path} outline: {why}"));
            assert!(
                outline.document.path.ends_with(path),
                "{}",
                outline.document.path
            );
            let plain = f
                .get(&format!("{scope}/api/document?path={path}"))
                .status(200);
            let plain: symphony_api::Document = serde_json::from_str(plain.text())
                .unwrap_or_else(|why| panic!("{scope}{path} document: {why}"));
            assert!(plain.path.ends_with(path), "{}", plain.path);
            // The plain body has no wrapper, so the outline type must refuse it by name.
            let why = serde_json::from_value::<symphony_api::DocumentOutline>(
                f.get(&format!("{scope}/api/document?path={path}"))
                    .status(200)
                    .json(),
            )
            .unwrap_err()
            .to_string();
            assert!(why.contains("missing field `document`"), "{why}");
        }
    }
}

fn media_refusals_and_thumbnail(f: &Fixture) {
    std::os::unix::fs::symlink(
        f.root.parent().unwrap().join("outside.svg"),
        f.root.join("room/shelf/items/outside.item"),
    )
    .unwrap();
    std::fs::create_dir(f.root.join("room/shelf/items/directory.item")).unwrap();
    for route in ["/bad%", "/bad%GG", "/bad%FF"] {
        f.get(route).status(400);
    }
    for route in [
        "/.git/hidden.md",
        "/%2Egit/hidden.png",
        "/artifacts/metadata.png",
        "/artifacts/outside.svg",
        "/assets/outside.svg",
        "/assets/%2E%2E/index.html",
        "/assets/missing.js",
        "/artifacts/%2E%2E/%2E%2E/outside.svg",
        "/artifacts/missing.svg",
        "/data",
        "/room/shelf/items/outside.item",
        "/room/shelf/items/directory.item",
        "/room/shelf/items/missing.item",
        "/room/shelf/items/%2E%2E/outside.item",
        "/x/shelf/outside.item",
        "/x/shelf/directory.item",
        "/x/shelf/missing.item",
        "/x/shelf/%2E%2E/outside.item",
        "/x/shelf/",
        "/x/unknown/A.item",
        "/api/document?path=docs/METADATA.md",
        "/thumb/400/stale/artifacts/metadata.png.jpg",
        "/thumb/400/stale/artifacts/outside.svg.jpg",
        "/thumb/no/stale/artifacts/raw.png.jpg",
        "/thumb/400/stale/artifacts/raw.svg.jpg",
    ] {
        f.get(route).status(404);
    }
    for route in [
        "/api/document?path=../outside.md",
        "/api/document?path=artifacts/raw.svg",
        "/api/document?path=docs/TOPIC.txt",
    ] {
        f.get(route).status(400);
    }
    // Stale signature forces fallback without depending on sips or decoding an actual picture.
    let fallback = f.get("/thumb/400/stale/artifacts/raw.png.jpg").status(200);
    assert_eq!(fallback.body, b"PNG fallback fixture");
    assert_eq!(fallback.header("content-type"), "image/png");
    assert_eq!(fallback.header("cache-control"), "no-cache");
    let branch = f
        .get("/@work~http/thumb/400/stale/artifacts/raw.png.jpg")
        .status(200);
    assert_eq!(branch.body, b"branch PNG fallback");
    assert!(
        f.request(
            "HEAD",
            "/@work~http/thumb/400/stale/artifacts/raw.png.jpg",
            &[],
            &[]
        )
        .status(200)
        .body
        .is_empty()
    );
}

fn persistence(f: &Fixture) {
    let baseline = std::fs::read(f.root.join("docs/TOPIC.md")).unwrap();
    for path in ["../outside.md", "docs/METADATA.md", "artifacts/raw.svg"] {
        f.post("/api/comment", &json!({"path":path, "after_line":3, "kind":"question", "title":"Refused comment", "body":"Must not be written"})).status(400);
        assert_eq!(
            std::fs::read(f.root.join("docs/TOPIC.md")).unwrap(),
            baseline
        );
        assert_eq!(
            std::fs::read_to_string(f.root.join(".git/hidden.md")).unwrap(),
            "metadata secret"
        );
    }
    for body in [b"{".as_slice(), b"[]".as_slice()] {
        let response = f
            .request(
                "POST",
                "/api/comment",
                &[("Content-Type", "application/json")],
                body,
            )
            .status(400);
        assert!(
            response.json()["error"]
                .as_str()
                .unwrap()
                .contains("bad JSON")
        );
        assert_eq!(
            std::fs::read(f.root.join("docs/TOPIC.md")).unwrap(),
            baseline
        );
    }
    let decision = json!({"path":"docs/TOPIC.md", "slug":"one", "state":"approved", "note":"HTTP verdict persisted", "tap":"http-verdict-once"});
    assert_eq!(
        f.post("/api/decision", &decision).status(200).json()["ok"],
        true
    );
    let decided = std::fs::read_to_string(f.root.join("docs/TOPIC.md")).unwrap();
    assert!(decided.contains("[x] **one**"));
    assert!(decided.contains("owner · approved — HTTP verdict persisted"));
    assert_eq!(
        f.post("/api/decision", &decision).status(200).json()["repeat"],
        true
    );
    assert_eq!(
        std::fs::read_to_string(f.root.join("docs/TOPIC.md")).unwrap(),
        decided
    );
    let comment = f.post("/@work~http/api/comment", &json!({"path":"docs/TOPIC.md", "after_line":3, "kind":"question", "title":"HTTP comment", "body":"Branch only comment"})).status(200);
    assert_eq!(comment.header("cache-control"), "no-store");
    assert_eq!(comment.json()["ok"], true);
    let branch = f.root.parent().unwrap().join("branch/docs/TOPIC.md");
    assert!(
        std::fs::read_to_string(branch)
            .unwrap()
            .contains("Branch only comment")
    );
    assert_eq!(
        std::fs::read_to_string(f.root.join("docs/TOPIC.md")).unwrap(),
        decided
    );
    let ask = f.post("/@work~http/api/ask", &json!({"path":"docs/TOPIC.md", "question":"HTTP question <tag>", "anchor":"one", "quote":"A question"})).status(200).json();
    let id = ask["id"].as_i64().unwrap();
    assert_eq!(
        f.post(
            "/api/ask-reply",
            &json!({"id":id, "text":"Owner follow-up"})
        )
        .status(200)
        .json()["ok"],
        true
    );
    let stored = symphony::asks::AskStore::open(&f.root.parent().unwrap().join("asks.db"))
        .unwrap()
        .thread(id)
        .unwrap();
    assert_eq!(stored.path, "docs/TOPIC.md");
    assert_eq!(stored.anchor, "one");
    assert_eq!(stored.quote, "A question");
    assert_eq!(
        stored.question,
        "HTTP question <tag>\n\n(sent from /@work~http/docs/TOPIC.md#one)"
    );
    assert!(stored.messages.iter().any(|m| m.text == "Owner follow-up"));
    // An ask's read names the chat recipient as the hub's last board tick read it, not as the board
    // has it this instant: a page hears the new recipient on its feed, and every read after says it.
    let mut feed = f.feed("");
    let store = symphony_board::cli::connect(&symphony_board::cli::Env::of_process()).unwrap();
    let orchestrator = store
        .register(&symphony_board::NewSession::new("orchestrator"))
        .unwrap()
        .row
        .handle;
    // An isolated remote callback exercises recipient selection without invoking a local agent.
    store
        .raw()
        .row("callback_state", orchestrator.as_str())
        .put(json!({
            "handle": orchestrator,
            "address": "codex:abcdef01",
            "host": "http-fixture-remote",
        }))
        .unwrap();
    feed.until(&format!("\"recipient\":\"{orchestrator}\""));
    let read = f.get(&format!("/api/ask?id={id}")).status(200).json();
    assert_eq!(read["ask"]["id"], id);
    assert_eq!(read["recipient"], orchestrator);
    assert_eq!(read["listening"], true);
    let asks = f.get("/api/asks?path=docs/TOPIC.md").status(200).json();
    assert_eq!(asks["recipient"], orchestrator);
    assert_eq!(asks["listening"], true);
    let html = read["ask"]["messages"][0]["html"].as_str().unwrap();
    assert!(!html.contains("<tag>"));
    assert!(html.contains("&lt;tag&gt;"));
    assert_eq!(
        f.get("/api/asks?path=docs/TOPIC.md").status(200).json()["asks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    f.get("/api/ask?id=bad").status(400);
    f.get("/api/ask?id=999999").status(404);
    // Each save above that waits on an agent journals the ask it opens: the verdict with its note,
    // keyed by its card, the branch's comment, and the chat question its follow-up joins; the
    // repeated tap journals nothing, and no save files a task. This private server leaves the
    // journal to the shared one, so the board holds no ask until the journal is handed over as the
    // shared server's clock hands it ([`symphony::callbacks::Flusher`]).
    assert!(
        store.asks(None).unwrap().is_empty(),
        "a private server opened an ask"
    );
    symphony::callbacks::Flusher::new(&f.root)
        .flush(&f.root)
        .unwrap();
    let asks: Vec<_> = store
        .asks(None)
        .unwrap()
        .into_iter()
        .map(|ask| {
            let about = ask.card.or(ask.block).or(ask.thread);
            (ask.kind, ask.path, about, ask.tree, ask.words, ask.lines)
        })
        .collect();
    let (path, branch) = (
        Some("docs/TOPIC.md".to_string()),
        Some("work/http".to_string()),
    );
    assert_eq!(
        asks,
        [
            (
                "verdict".to_string(),
                path.clone(),
                Some("one".to_string()),
                None,
                "HTTP verdict persisted".to_string(),
                1
            ),
            (
                "comment".into(),
                path.clone(),
                Some("HTTP comment".into()),
                branch.clone(),
                "HTTP comment Branch only comment".into(),
                1
            ),
            (
                "chat".into(),
                path,
                Some(id.to_string()),
                branch,
                "HTTP question <tag>".into(),
                2
            ),
        ]
    );
    assert!(store.tasks(None).unwrap().is_empty(), "a save filed a task");
    owner_save::quick_tasks(f);
}

/// The numbers of the `mark-NNNNNN` lines `text` shows, in the order it shows them.
fn marks(text: &str) -> Vec<usize> {
    text.match_indices("mark-")
        .filter_map(|(at, _)| text.get(at + 5..at + 11)?.parse().ok())
        .collect()
}

/// A long brief's task page sends a part of it, and `/api/task-brief` the rest, a chunk at a time,
/// all of one revision, so the owner reaches every line of it.
fn task_pages(f: &Fixture) {
    let store = symphony_board::cli::connect(&symphony_board::cli::Env::of_process()).unwrap();
    let mut body = String::new();
    let mut lines = 0;
    while body.len() < 400_000 {
        body.push_str(&format!("mark-{lines:06} of a long brief\n"));
        lines += 1;
    }
    store
        .create(&symphony_board::NewTask {
            body: body.clone(),
            ..symphony_board::NewTask::new("fixture/long-brief", "Long brief", "owner")
        })
        .unwrap();
    store
        .create(&symphony_board::NewTask {
            parent: Some("fixture/long-brief".into()),
            ..symphony_board::NewTask::new("fixture/long-brief-step", "A step", "owner")
        })
        .unwrap();
    let page = f
        .get("/api/task-page?slug=fixture%2Flong-brief")
        .status(200);
    assert_eq!(page.header("cache-control"), "no-store");
    assert!(
        page.body.len() < body.len(),
        "the page of a {}-byte brief sent {} bytes",
        body.len(),
        page.body.len()
    );
    let json = page.json();
    assert_eq!(json["children_count"], 1);
    let revision = json["brief_revision"].as_str().unwrap().to_owned();
    let (mut from, mut omitted) = (
        json["brief_next"].as_u64().unwrap(),
        json["brief_omitted"].as_u64().unwrap(),
    );
    assert_eq!(usize::try_from(from + omitted).unwrap(), body.len());
    let mut shown = marks(&json["blocks"].to_string());
    assert_eq!(shown.first(), Some(&0), "the page shows the brief's start");
    while omitted > 0 {
        let chunk = f
            .get(&format!(
                "/api/task-brief?slug=fixture%2Flong-brief&from={from}"
            ))
            .status(200);
        assert_eq!(chunk.header("cache-control"), "no-store");
        let chunk = chunk.json();
        assert_eq!(chunk["revision"], revision.as_str());
        let next = chunk["next"].as_u64().unwrap();
        assert!(next > from, "a chunk from byte {from} read nothing");
        shown.extend(marks(&chunk["blocks"].to_string()));
        (from, omitted) = (next, chunk["omitted"].as_u64().unwrap());
    }
    assert_eq!(
        shown,
        (0..lines).collect::<Vec<_>>(),
        "every line of the brief is reached once, in order"
    );
    let past = f
        .get("/api/task-brief?slug=fixture%2Flong-brief&from=99999999")
        .status(200)
        .json();
    assert_eq!((&past["blocks"], &past["omitted"]), (&json!([]), &json!(0)));
    f.get("/api/task-brief?from=0").status(400);
    f.get("/api/task-brief?slug=fixture%2Fno-such-task")
        .status(404);
    // A task no session took tokens for: its page's figures all none, no card's cost.
    let tokens = f
        .get("/api/task-tokens?slug=fixture%2Flong-brief")
        .status(200);
    assert_eq!(tokens.header("cache-control"), "no-store");
    let tokens = tokens.json();
    assert_eq!(
        (&tokens["by_role"], &tokens["by_round"], &tokens["rows"]),
        (&json!([]), &json!([]), &json!([]))
    );
    assert_eq!(tokens["total"]["calls"], 0);
    f.get("/api/task-tokens?slug=fixture%2Fno-such-task")
        .status(404);
    let listed = |slugs: &[String]| {
        let list = serde_json::to_string(slugs).unwrap();
        let encoded: String = (list.chars())
            .map(|c| match c {
                '[' => "%5B".to_string(),
                ']' => "%5D".to_string(),
                '"' => "%22".to_string(),
                ',' => "%2C".to_string(),
                '/' => "%2F".to_string(),
                c => c.to_string(),
            })
            .collect();
        format!("/api/task-costs?slugs={encoded}")
    };
    let costs = f
        .get(&listed(&[
            "fixture/long-brief".into(),
            "fixture/none".into(),
        ]))
        .status(200)
        .json();
    assert_eq!(costs["costs"], json!([]));
    assert!(costs["prices"].as_str().is_some_and(|day| day.len() == 10));
    let most: Vec<String> = (0..=symphony_api::TASK_COSTS_MOST)
        .map(|n| format!("w/t{n}"))
        .collect();
    f.get(&listed(&most[1..])).status(200);
    f.get(&listed(&most)).status(400);
    f.get("/api/task-costs?slugs=fixture").status(400);
}

#[test]
fn an_upload_shows_on_every_branch_and_on_the_asks_cards_and_tasks_that_name_it() {
    run(
        "an_upload_shows_on_every_branch_and_on_the_asks_cards_and_tasks_that_name_it",
        uploads,
    );
}

/// Every image the posted item shows, as (src, alt), from its typed blocks.
fn pictures(blocks: &Value) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut stack = vec![blocks];
    while let Some(value) = stack.pop() {
        match value {
            Value::Object(fields) => {
                if let Some(image) = fields.get("image") {
                    found.push((
                        image["src"].as_str().unwrap().to_string(),
                        image["alt"].as_str().unwrap().to_string(),
                    ));
                }
                stack.extend(fields.values());
            }
            Value::Array(items) => stack.extend(items),
            _ => {}
        }
    }
    found
}

fn uploads(f: &Fixture) {
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x04\0\0\0\x06";
    let branch = f.root.parent().unwrap().join("branch");
    // Sent from a branch page, stored in the server's own checkout and read from every scope.
    let upload = f
        .request(
            "POST",
            "/@work~http/api/upload?name=phone%20shot.png",
            &[("Content-Type", "image/png")],
            PNG,
        )
        .status(200)
        .json();
    let url = upload["url"].as_str().unwrap().to_string();
    let markup = upload["markup"].as_str().unwrap().to_string();
    assert_eq!(markup, format!("![phone shot.png]({url})"));
    let file = url.trim_start_matches('/');
    assert_eq!(std::fs::read(f.root.join(file)).unwrap(), PNG);
    assert!(!branch.join(file).exists());
    for scope in ["", SCOPE] {
        let read = f.get(&format!("{scope}{url}")).status(200);
        assert_eq!(read.body, PNG, "{scope}");
        assert_eq!(read.header("content-type"), "image/png");
        assert_eq!(read.header("x-content-type-options"), "nosniff");
    }

    // An ask: the owner's words stay literal, the picture is drawn at its real size.
    let ask = f
        .post(
            "/api/ask",
            &json!({"path":"docs/TOPIC.md", "question":format!("What is <this>?\n{markup}")}),
        )
        .status(200)
        .json();
    let read = f
        .get(&format!("/api/ask?format=blocks&id={}", ask["id"]))
        .status(200)
        .json();
    let question = &read["ask"]["messages"][0]["blocks"];
    let shown = pictures(question);
    assert_eq!(shown.len(), 1, "{question}");
    assert!(shown[0].0.starts_with(&format!("{url}?v=")), "{shown:?}");
    assert_eq!(shown[0].1, "phone shot.png");
    assert!(
        question.to_string().contains("What is <this>?"),
        "{question}"
    );
    assert!(
        question.to_string().contains("\"width\":\"4\""),
        "{question}"
    );

    // A branch card's message keeps the line in that branch's file, and the page draws it from
    // the branch's address, which the server answers from its own store.
    f.post(
        &format!("{SCOPE}/api/message"),
        &json!({"path":"docs/TOPIC.md", "slug":"one", "text":format!("Like this\n{markup}")}),
    )
    .status(200);
    let written = std::fs::read_to_string(branch.join("docs/TOPIC.md")).unwrap();
    assert!(written.contains(&format!("    {markup}\n")), "{written}");
    let threads = f
        .get(&format!("{SCOPE}/api/history?path=docs/TOPIC.md"))
        .status(200)
        .json();
    let shown = pictures(&threads["one"]["blocks"]);
    assert_eq!(
        shown,
        [(format!("{SCOPE}{url}"), "phone shot.png".into())],
        "{threads}"
    );
    assert_eq!(f.get(&shown[0].0).status(200).body, PNG);

    // A new task's brief and a thread message both show the picture.
    let filed = f
        .post(
            "/api/quick-task",
            &json!({"request":"cccccccccccccccccccccccccccccccc", "title":"Upload task", "body":format!("The bug:\n\n{markup}"), "page":"/tasks"}),
        )
        .status(200)
        .json();
    let slug = filed["slug"].as_str().unwrap().to_string();
    let page = f
        .get(&format!("/api/task-page?slug={slug}"))
        .status(200)
        .json();
    let shown = pictures(&page["blocks"]);
    assert_eq!(shown.len(), 1, "{page}");
    assert!(shown[0].0.starts_with(&format!("{url}?v=")), "{shown:?}");
    f.post(
        "/api/task-message",
        &json!({"slug":slug, "text":format!("And again\n{markup}")}),
    )
    .status(200);
    let history = f
        .get(&format!("/api/task-history?slug={slug}"))
        .status(200)
        .json();
    let owner = history["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["message"]["text"] == format!("And again\n{markup}"))
        .unwrap_or_else(|| panic!("{history}"));
    let shown = pictures(&owner["blocks"]);
    assert_eq!(shown.len(), 1, "{owner}");
    assert_eq!(shown[0].1, "phone shot.png");
}

#[test]
fn installed_client_failure_names_stop_and_restart_from_main() {
    let tree = fixture();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        use axum::http::Request;
        use tower::ServiceExt;
        let root = tree.path("repo");
        let home = tree.path("installed");
        let shared = Shared::new(1, 1234, &root, Some(&home), Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)));
        for missing_index in [false, true] {
            if missing_index { std::fs::create_dir_all(home.join("client/public")).unwrap(); }
            let response = router(shared.clone()).oneshot(Request::builder().uri("/").header("Host", "localhost:1234").extension(axum::extract::ConnectInfo("127.0.0.1:1234".parse::<SocketAddr>().unwrap())).body(axum::body::Body::empty()).unwrap()).await.unwrap();
            assert_eq!(response.status(), 503);
            let bytes = axum::body::to_bytes(response.into_body(), 10_000).await.unwrap();
            assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap()["error"], "the Symphony client has not been built: run just symphony --stop, then just symphony from the main checkout");
        }
    });
}

#[test]
fn watcher_refreshes_documents_without_gallery_work_and_items_only_in_their_tree() {
    run(
        "watcher_refreshes_documents_without_gallery_work_and_items_only_in_their_tree",
        watcher,
    );
}

fn gallery(f: &Fixture, scope: &str) -> String {
    f.get(&format!("{scope}/api/document?path=docs/SHELF.md"))
        .status(200)
        .text()
        .to_string()
}

fn watcher(f: &Fixture) {
    assert!(gallery(f, "").contains("A.item"));
    assert!(gallery(f, SCOPE).contains("A.item"));
    let mut root_feed = f.feed("");
    // A branch's feed on a WebSocket, as the web client follows it: the same events, filtered to
    // the same document and sections.
    let mut branch_feed = f.socket(SCOPE);
    eprintln!("{PHASE}document");
    write(
        &f.root,
        "docs/SHELF.md",
        "# Shelf\n\nDocument refresh handshake\n\n[[shelf:all]]\n",
    );
    root_feed.until("Document refresh handshake");
    assert!(gallery(f, "").contains("Document refresh handshake"));
    assert!(gallery(f, SCOPE).contains("A.item"));
    eprintln!("{PHASE}branch item");
    write(
        &f.root.parent().unwrap().join("branch"),
        "room/shelf/items/BRANCH.item",
        b"branch item",
    );
    branch_feed.until("BRANCH.item");
    assert!(gallery(f, SCOPE).contains("BRANCH.item"));
    assert!(!gallery(f, "").contains("BRANCH.item"));
    // A document edit can replace section keys. Subscribe to its current viewport for this edit.
    root_feed = f.feed("");
    eprintln!("{PHASE}root item");
    write(&f.root, "room/shelf/items/ROOT.item", b"root item");
    root_feed.until("ROOT.item");
    assert!(gallery(f, "").contains("ROOT.item"));
    assert!(!gallery(f, SCOPE).contains("ROOT.item"));
    eprintln!("{PHASE}finished");
}

#[test]
fn native_low_disk_configured_threshold_notice() {
    run("native_low_disk_configured_threshold_notice", |f| {
        let notice = f.server_notice("");
        let expected = symphony_board::board_health::disk::Low::of(&f.root, 0, 1_000_000_000_000.0)
            .unwrap()
            .notice();
        assert!(notice.contains(&expected), "{notice}");
    });
}

#[test]
fn native_low_disk_zero_threshold_notice() {
    run("native_low_disk_zero_threshold_notice", |f| {
        assert!(!f.server_notice("").contains("The disk has under"));
    });
}

#[test]
fn native_low_disk_malformed_threshold_falls_back() {
    run("native_low_disk_malformed_threshold_falls_back", |f| {
        // The parent checks the exact fallback diagnostic. Reading the feed proves the server
        // publishes its notice; a later disk observation cannot determine that notice's state.
        let _ = f.server_notice("");
    });
}
