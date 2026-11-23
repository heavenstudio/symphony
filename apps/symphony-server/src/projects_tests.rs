//! The one server's dispatch: projects under `/<id>/`, the list, the theme tokens, the redirects
//! of today's paths, and the registry read again when it changes.

use super::*;
use crate::gate::Gate;
use crate::testing::Tree;
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// A machine folder registering `alpha` (the starter) and `beta`, each with a theme of its own,
/// and an install folder holding a client shell.
pub(super) struct Machine {
    pub(super) tree: Tree,
    pub(super) projects: Arc<Projects>,
}

const SHELL: &str = "<!DOCTYPE html><html lang=\"en\"><head><title>Symphony</title>\
<link rel=\"icon\" href=\"data:,\"></head><body><div id=\"main\"></div></body></html>";

impl Machine {
    pub(super) fn new() -> Machine {
        Machine::with_gate(crate::testing::open_gate())
    }

    /// [`Machine::new`], its starter judging who may reach it by `gate`.
    pub(super) fn with_gate(gate: Gate) -> Machine {
        let tree = Tree::new();
        for (id, color) in [("alpha", "#a00"), ("beta", "#0b0")] {
            tree.write(&format!("{id}/docs/README.md"), "# Docs\n");
            tree.write(
                &format!("{id}/symphony.toml"),
                &format!(
                    "[project]\nname = \"{id} works\"\nid = \"{id}\"\n[theme.tokens]\nprimary = \
                     \"{color}\"\n"
                ),
            );
        }
        tree.write("install/client/public/index.html", SHELL);
        let projects = Machine::serving(&tree, "machine", gate, &["alpha", "beta"]);
        Machine { tree, projects }
    }

    /// Another server over this machine's checkouts and install folder, its own machine folder
    /// `folder` registering `ids` of `alpha` (its starter) and `beta`, judging who may reach it by
    /// `gate`: what this machine's answers are compared with, the same checkouts on both.
    pub(super) fn beside(&self, folder: &str, gate: Gate, ids: &[&str]) -> Arc<Projects> {
        Machine::serving(&self.tree, folder, gate, ids)
    }

    /// A server over `tree`'s checkouts whose machine folder `folder` registers `ids`, its starter
    /// `alpha` judging who may reach it by `gate`.
    fn serving(tree: &Tree, folder: &str, gate: Gate, ids: &[&str]) -> Arc<Projects> {
        let machine = tree.path(folder);
        let mut registry = Registry::default();
        for id in ids {
            registry.add(Entry::of(&tree.path(id)).unwrap()).unwrap();
        }
        registry.write(&machine).unwrap();
        let root = tree.path("alpha").canonicalize().unwrap();
        let starter = Shared::project(1, 1, &root, Some(&tree.path("install")), gate, "/alpha");
        Projects::made(&machine, &starter, Warming::OnRequest)
    }

    async fn get(&self, path: &str, headers: &[(&str, &str)]) -> Response {
        self.from("127.0.0.1:1", path, headers).await
    }

    async fn from(&self, client: &str, path: &str, headers: &[(&str, &str)]) -> Response {
        self.named(client, "127.0.0.1:1", path, headers).await
    }

    /// `path` asked by `client` of the Host `host`.
    pub(super) async fn named(
        &self,
        client: &str,
        host: &str,
        path: &str,
        headers: &[(&str, &str)],
    ) -> Response {
        asked(self.projects.clone(), client, host, path, headers).await
    }
}

/// `path` asked of `projects`' server by `client` of the Host `host`.
async fn asked(
    projects: Arc<Projects>,
    client: &str,
    host: &str,
    path: &str,
    headers: &[(&str, &str)],
) -> Response {
    let mut request = Request::builder().uri(path).header(header::HOST, host);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    router(projects)
        .layer(MockConnectInfo(client.parse::<SocketAddr>().unwrap()))
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

/// How long a test waits on an answer asked aside; only a stall reaches it.
const BACKSTOP: std::time::Duration = test_time::backstop(std::time::Duration::from_secs(15));

/// `path` asked of `machine` on loopback on a thread of its own, its body sent once it answers:
/// what a test waits on, with [`BACKSTOP`], while it holds what the answer must not wait on.
fn asked_aside(machine: &Machine, path: &'static str) -> std::sync::mpsc::Receiver<String> {
    let projects = machine.projects.clone();
    let (send, answered) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let body = runtime.block_on(async {
            text(asked(projects, "127.0.0.1:1", "127.0.0.1:1", path, &[]).await).await
        });
        let _ = send.send(body);
    });
    answered
}

fn location(response: &Response) -> &str {
    response.headers()[header::LOCATION].to_str().unwrap()
}

pub(super) async fn text(response: Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

const PAGE: (&str, &str) = ("sec-fetch-mode", "navigate");

#[tokio::test]
async fn a_project_without_its_slash_and_todays_pages_redirect_into_a_project() {
    let machine = Machine::new();
    let bare = machine.get("/beta?x=1", &[]).await;
    assert_eq!(bare.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(location(&bare), "/beta/?x=1");
    // A bookmark of today opens under the project that started the server.
    let bookmark = machine.get("/docs/README.md?y=2", &[PAGE]).await;
    assert_eq!(location(&bookmark), "/alpha/docs/README.md?y=2");
    let older = machine
        .get("/tasks", &[("accept", "text/html,application/xhtml+xml")])
        .await;
    assert_eq!(
        location(&older),
        "/alpha/tasks",
        "a browser without fetch metadata"
    );
    // A page under another project keeps its links there.
    let linked = machine
        .get(
            "/tasks",
            &[PAGE, ("referer", "http://127.0.0.1:1/beta/docs/")],
        )
        .await;
    assert_eq!(location(&linked), "/beta/tasks");
    // Another site's page names no project of this server.
    let foreign = machine
        .get(
            "/tasks",
            &[PAGE, ("referer", "http://elsewhere:1/beta/docs/")],
        )
        .await;
    assert_eq!(location(&foreign), "/alpha/tasks");
    // A branch's pages are the Symphony's routes too, its board among them.
    for (path, target) in [
        ("/@work~x/tasks", "/alpha/@work~x/tasks"),
        ("/@work~x/", "/alpha/@work~x/"),
        ("/@work~x/docs/README.md", "/alpha/@work~x/docs/README.md"),
    ] {
        assert_eq!(location(&machine.get(path, &[PAGE]).await), target);
    }
}

/// The page a path under no project answers when no project answers it.
async fn unknown(machine: &Machine, path: &str, headers: &[(&str, &str)]) -> String {
    let response = machine.get(path, headers).await;
    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "{path} {headers:?}"
    );
    assert!(
        response.headers().get(header::LOCATION).is_none(),
        "{path} {headers:?}: {:?}",
        response.headers()
    );
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/html; charset=utf-8"
    );
    text(response).await
}

#[tokio::test]
async fn an_unknown_project_id_is_the_list_saying_so_never_another_projects_page() {
    let machine = Machine::new();
    let from_beta = ("referer", "http://127.0.0.1:1/beta/docs/");
    for path in [
        "/nope/",
        "/nope",
        "/alpha-typo/tasks",
        "/Alpha/",
        "/@work~x",
    ] {
        for headers in [vec![PAGE], vec![PAGE, from_beta], vec![]] {
            let page = unknown(&machine, path, &headers).await;
            let first = path.trim_start_matches('/').split('/').next().unwrap();
            assert!(
                page.contains(&format!("No project is called <q>{first}</q> here")),
                "{path}: {page}"
            );
            assert!(page.contains("<h1>Projects</h1>"), "{path}: {page}");
            for id in ["alpha", "beta"] {
                assert!(page.contains(&format!("href=\"/{id}/\"")), "{path}: {page}");
            }
        }
    }
    // An id the wrong case names one project's: the page says which.
    let near = unknown(&machine, "/Alpha/docs/README.md?x=1", &[PAGE]).await;
    assert!(
        near.contains("Did you mean <a href=\"/alpha/docs/README.md\">alpha</a>?"),
        "{near}"
    );
    let far = unknown(&machine, "/nope/", &[PAGE]).await;
    assert!(!far.contains("Did you mean"), "{far}");
    let escaped = unknown(&machine, "/%3Cb%3E/", &[PAGE]).await;
    assert!(
        escaped.contains("No project is called <q>&lt;b&gt;</q> here"),
        "{escaped}"
    );
}

/// A document's picture or link to a file of its checkout outside the entrypoints is written
/// root-relative (`/look/icon.svg`, `/src/main.rs`): the project of the page that names it, which
/// holds that folder, answers it, a file missing there with its own miss.
#[tokio::test]
async fn a_documents_picture_or_link_to_a_file_of_its_checkout_reaches_its_project() {
    let machine = Machine::new();
    machine.tree.write("alpha/look/icon.svg", "<svg/>");
    machine.tree.write("alpha/src/main.rs", "fn main() {}\n");
    let from_alpha = ("referer", "http://127.0.0.1:1/alpha/docs/README.md");
    // The picture, fetched by the page, is answered in place.
    let picture = machine.get("/look/icon.svg?v=4b77", &[from_alpha]).await;
    assert_eq!(picture.status(), StatusCode::OK);
    assert_eq!(picture.headers()[header::CONTENT_TYPE], "image/svg+xml");
    assert_eq!(text(picture).await, "<svg/>");
    // A link, followed, opens under the project.
    for (path, target) in [
        ("/src/main.rs", "/alpha/src/main.rs"),
        ("/symphony.toml", "/alpha/symphony.toml"),
        ("/look/gone.svg", "/alpha/look/gone.svg"),
    ] {
        let link = machine.get(path, &[PAGE, from_alpha]).await;
        assert_eq!(link.status(), StatusCode::TEMPORARY_REDIRECT, "{path}");
        assert_eq!(location(&link), target);
    }
    // A picture missing from a folder the project holds is the project's own miss.
    let missing = machine.get("/look/gone.svg", &[from_alpha]).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let missing = text(missing).await;
    assert!(missing.contains("Nothing is here"), "{missing}");
    assert!(!missing.contains("No project is called"), "{missing}");
    // Beta holds no such folder, and alpha's files are not beta's page's to reach; nor is a name
    // the checkout holds that the router serves no file for.
    let from_beta = ("referer", "http://127.0.0.1:1/beta/docs/README.md");
    let page = unknown(&machine, "/look/icon.svg", &[from_beta]).await;
    assert!(page.contains("No project is called <q>look</q>"), "{page}");
    for path in ["/src/", "/src", "/../alpha/look/icon.svg"] {
        let page = unknown(&machine, path, &[PAGE, from_alpha]).await;
        assert!(page.contains("No project is called"), "{path}: {page}");
    }
}

/// The icon a browser asks the address for by itself is the Symphony's, the started project's
/// never: a project's own (`tools/static/icon.svg`) answers only a page of that project, and the
/// home screen's size is the Symphony's wherever it is asked.
#[tokio::test]
async fn the_addresss_own_icon_is_the_symphonys_and_a_projects_only_for_its_own_page() {
    let machine = Machine::new();
    // Alpha, the project the server was started from, has an icon of its own; beta has none.
    machine
        .tree
        .write("alpha/tools/static/icon.svg", "<svg>alpha</svg>");
    let from_alpha = ("referer", "http://127.0.0.1:1/alpha/src/picture.png");
    let from_beta = ("referer", "http://127.0.0.1:1/beta/src/picture.png");
    let elsewhere = ("referer", "http://other.example/alpha/");
    for (path, headers, alphas) in [
        ("/favicon.ico", &[][..], false),
        ("/favicon.ico", &[elsewhere], false),
        ("/favicon.ico", &[from_beta], false),
        ("/beta/favicon.ico", &[], false),
        ("/favicon.ico", &[from_alpha], true),
        ("/alpha/favicon.ico", &[], true),
    ] {
        let icon = machine.get(path, headers).await;
        assert_eq!(icon.status(), StatusCode::OK, "{path} {headers:?}");
        assert_eq!(icon.headers()[header::CONTENT_TYPE], "image/svg+xml");
        let icon = text(icon).await;
        assert_eq!(icon == "<svg>alpha</svg>", alphas, "{path} {headers:?}");
        assert_eq!(
            icon.contains("M5 21A11 11 0 0 1 27 21H24.6A8.6 7.2 0 0 0 7.4 21Z"),
            !alphas,
            "{path} {headers:?}: {icon}"
        );
    }
    for (path, headers) in [
        ("/favicon.ico?size=180", &[][..]),
        ("/favicon.ico?size=180", &[from_alpha]),
        ("/alpha/favicon.ico?size=180", &[]),
        ("/beta/favicon.ico?size=180", &[]),
    ] {
        let touch = machine.get(path, headers).await;
        assert_eq!(touch.status(), StatusCode::OK, "{path} {headers:?}");
        assert_eq!(touch.headers()[header::CONTENT_TYPE], "image/png", "{path}");
    }
    // The list is the Symphony's own page: its icons, and its mark and wordmark first.
    let list = text(machine.get("/", &[PAGE]).await).await;
    assert!(list.contains(&crate::mark::links("")), "{list}");
    assert!(
        list.contains(&format!(
            "<header class=\"app-header\" id=\"top\">{}<nav",
            crate::mark::brand("", "")
        )),
        "{list}"
    );
}

/// An id spelled with escapes names its project (`/%61lpha/` is `/alpha/`), and the page of a
/// mistyped one never offers the id it already spells.
#[tokio::test]
async fn an_escaped_id_is_its_projects_plain_address() {
    let machine = Machine::new();
    for (path, target) in [
        ("/%61lpha/tasks?x=1", "/alpha/tasks?x=1"),
        ("/%61lpha", "/alpha/"),
        ("/b%65ta/docs/A%20b.md", "/beta/docs/A%20b.md"),
    ] {
        let response = machine.get(path, &[PAGE]).await;
        assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT, "{path}");
        assert_eq!(location(&response), target, "{path}");
    }
    let near = unknown(&machine, "/%41lpha/", &[PAGE]).await;
    assert!(near.contains("No project is called <q>Alpha</q>"), "{near}");
    assert!(
        near.contains("Did you mean <a href=\"/alpha/\">alpha</a>?"),
        "{near}"
    );
}

#[tokio::test]
async fn a_project_served_at_the_root_answers_only_the_names_its_folder_holds() {
    let machine = Machine::new();
    machine.tree.write(
        "alpha/symphony.toml",
        "[project]\nname = \"alpha works\"\nid = \"alpha\"\n[[entrypoints]]\nurl = \"/\"\nfolder \
         = \"spec\"\n",
    );
    machine.tree.write("alpha/spec/guide/A.md", "# A\n");
    let held = machine.get("/guide/A.md", &[PAGE]).await;
    assert_eq!(location(&held), "/alpha/guide/A.md");
    let tasks = machine.get("/tasks", &[PAGE]).await;
    assert_eq!(location(&tasks), "/alpha/tasks");
    let page = unknown(&machine, "/docs/README.md", &[PAGE]).await;
    assert!(page.contains("No project is called <q>docs</q>"), "{page}");
}

/// An HTML document through the whole routing, the machine's and its project's: its route is the
/// project's app page whatever headers ask for it, none among them, as a browser asks a plain-http
/// address that is not loopback's (a phone over the tailnet); its file's address is the file,
/// under its sandbox and its entrypoint's bounds, for a frame, an object, an embed or a request
/// with no headers; an HTML file outside every entrypoint keeps the plain sandbox at either
/// address; and a document's own folder's files load as they are.
#[tokio::test]
async fn an_html_document_is_its_projects_page_at_its_route_and_its_file_at_its_files_address() {
    let machine = Machine::new();
    let page = "<!doctype html><title>Readout</title><script>document.title = 'ran'</script>";
    machine.tree.write("alpha/docs/analysis/readout.html", page);
    machine
        .tree
        .write("alpha/docs/analysis/board.css", "body { margin: 0 }");
    machine.tree.write("alpha/outside.html", page);
    let app = text(machine.get("/alpha/docs/README.md", &[]).await).await;
    assert!(app.contains("\"base\":\"/alpha\""), "{app}");
    let page_load = (
        "accept",
        "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
    );
    for headers in [
        &[][..],
        &[page_load],
        &[page_load, PAGE, ("sec-fetch-dest", "document")],
        &[page_load, ("sec-fetch-dest", "iframe")],
    ] {
        let route = machine
            .get("/alpha/docs/analysis/readout.html", headers)
            .await;
        assert_eq!(route.status(), StatusCode::OK, "{headers:?}");
        assert_eq!(text(route).await, app, "{headers:?}");
    }
    let bounded = "sandbox allow-scripts; default-src 'unsafe-inline' 'unsafe-eval' data: blob: \
                   127.0.0.1:1/alpha/docs/; connect-src 'none'";
    for (path, policy) in [
        ("/alpha/docs/analysis/readout.html?raw", bounded),
        ("/alpha/docs/analysis/readout.html?state=b&raw", bounded),
        ("/alpha/outside.html?raw", "sandbox allow-scripts"),
        ("/alpha/outside.html", "sandbox allow-scripts"),
    ] {
        for headers in [
            &[][..],
            &[page_load],
            &[page_load, ("sec-fetch-dest", "iframe")],
            &[("sec-fetch-dest", "object")],
            &[("sec-fetch-dest", "embed")],
        ] {
            let file = machine.get(path, headers).await;
            assert_eq!(file.status(), StatusCode::OK, "{path} {headers:?}");
            let said = |name| file.headers()[name].to_str().unwrap().to_string();
            assert_eq!(
                [
                    said(header::CONTENT_SECURITY_POLICY),
                    said(header::CONTENT_TYPE),
                    said(header::X_CONTENT_TYPE_OPTIONS),
                ],
                [policy, "text/html; charset=utf-8", "nosniff"],
                "{path} {headers:?}"
            );
            assert_eq!(text(file).await, page, "{path} {headers:?}");
        }
    }
    let beside = machine.get("/alpha/docs/analysis/board.css", &[]).await;
    assert_eq!(beside.status(), StatusCode::OK);
    assert_eq!(text(beside).await, "body { margin: 0 }");
    for missing in [
        "/alpha/docs/analysis/missing.html?raw",
        "/alpha/docs/analysis/..%2f..%2foutside.html?raw",
    ] {
        let response = machine.get(missing, &[]).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{missing}");
    }
}

/// A legacy alias whose path climbs out of its project (`..`, escaped or not) is no page: its
/// redirect, put back under the project's prefix, would land in another project's files, past the
/// path an HTML document's policy bounds its loads by.
#[tokio::test]
async fn a_legacy_alias_that_would_leave_its_project_is_not_found() {
    let machine = Machine::new();
    machine
        .tree
        .write("beta/docs/data.js", "window.secret = 1;");
    for path in [
        "/alpha/history/..%2f..%2fbeta%2fdocs%2fdata.js",
        "/alpha/history/..%2F..%2Fbeta/docs/data.js",
        "/alpha/app/..%2f..%2fbeta%2fdocs%2fdata.js",
        "/alpha/doc/.%2e/.%2e/beta/docs/data.js",
        "/alpha/diff/abc/..%2fbeta%2fdocs%2fdata.js",
        "/alpha/history/docs/.%2fdata.js",
    ] {
        let response = machine.get(path, &[]).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        assert!(
            response.headers().get(header::LOCATION).is_none(),
            "{path}: {:?}",
            response.headers()
        );
    }
    // A path that stays in the project still redirects there.
    let kept = machine.get("/alpha/history/docs/README.md", &[]).await;
    assert_eq!(kept.status(), StatusCode::PERMANENT_REDIRECT);
    assert_eq!(location(&kept), "/alpha/docs/README.md");
}

#[tokio::test]
async fn a_project_answers_under_its_prefix_and_a_subresource_in_its_page_in_place() {
    let machine = Machine::new();
    // The legacy alias redirects inside the project's router; its Location stays in the project.
    let alias = machine.get("/beta/app/docs/README.md", &[]).await;
    assert_eq!(alias.status(), StatusCode::PERMANENT_REDIRECT);
    assert_eq!(location(&alias), "/beta/docs/README.md");
    // A fetch of a path of today from beta's page is beta's, answered where it is.
    let fetched = machine
        .get(
            "/app/docs/README.md",
            &[("referer", "http://127.0.0.1:1/beta/tasks")],
        )
        .await;
    assert_eq!(location(&fetched), "/beta/docs/README.md");
    let starter = machine.get("/app/docs/README.md", &[]).await;
    assert_eq!(location(&starter), "/alpha/docs/README.md");
}

#[tokio::test]
async fn a_project_page_carries_its_base_and_theme_without_remembering_a_root_redirect() {
    let machine = Machine::new();
    let page = machine.get("/beta/", &[PAGE]).await;
    assert_eq!(page.status(), StatusCode::OK);
    assert!(
        page.headers().get(header::SET_COOKIE).is_none(),
        "beta navigation: {:?}",
        page.headers()
    );
    let body = text(page).await;
    assert!(body.contains("\"base\":\"/beta\""), "{body}");
    assert!(
        body.contains("--primary:#0b0;"),
        "beta's own tokens: {body}"
    );
    assert!(!body.contains("#a00"), "nothing of alpha's: {body}");
    assert!(body.contains("<title>beta works</title>"));
    // Fetches also need no project-selection cookie.
    let fetched = machine.get("/beta/", &[]).await;
    assert!(
        fetched.headers().get(header::SET_COOKIE).is_none(),
        "beta fetch: {:?}",
        fetched.headers()
    );
}

#[tokio::test]
async fn root_is_always_the_picker_with_one_or_multiple_projects_and_old_cookies() {
    let machine = Machine::new();
    for ids in [vec!["alpha", "beta"], vec!["alpha"]] {
        Registry::change(&machine.tree.path("machine"), |registry| {
            registry
                .projects
                .retain(|entry| ids.contains(&entry.id.as_str()));
            Ok(())
        })
        .unwrap();
        for cookie in [
            None,
            Some("a=b; symphony-project=alpha"),
            Some("symphony-project=gone"),
        ] {
            let mut headers = vec![PAGE, ("referer", "http://127.0.0.1:1/alpha/tasks")];
            if let Some(cookie) = cookie {
                headers.push(("cookie", cookie));
            }
            let page = machine.get("/", &headers).await;
            assert_eq!(
                page.status(),
                StatusCode::OK,
                "{ids:?} {cookie:?}: {:?}",
                page.headers()
            );
            assert!(
                page.headers().get(header::LOCATION).is_none(),
                "{ids:?} {cookie:?}: {:?}",
                page.headers()
            );
            assert_eq!(
                page.headers()
                    .get(header::CACHE_CONTROL)
                    .and_then(|value| value.to_str().ok()),
                Some("no-store"),
                "{ids:?} {cookie:?}: {:?}",
                page.headers()
            );
            let body = text(page).await;
            assert!(body.contains("<h1>Projects</h1>"), "{body}");
            assert_eq!(body.matches("<li>").count(), ids.len(), "{body}");
            for id in &ids {
                assert!(body.contains(&format!("href=\"/{id}/\"")), "{body}");
            }
        }
    }
}

#[tokio::test]
async fn the_list_wears_the_apps_frame_tokens_and_icon() {
    let machine = Machine::new();
    let list = machine.get("/", &[PAGE]).await;
    assert_eq!(
        list.headers()[header::CONTENT_TYPE],
        "text/html; charset=utf-8"
    );
    let list = text(list).await;
    assert!(list.contains("<title>Symphony</title>"), "{list}");
    assert!(
        list.contains("<header class=\"app-header\" id=\"top\">"),
        "{list}"
    );
    assert!(
        list.contains("<a href=\"/\" class=\"active\" aria-current=\"page\">Projects</a>"),
        "{list}"
    );
    assert!(!list.contains("class=\"crumbs\""), "{list}");
    assert!(
        list.contains("--background:"),
        "the client's tokens: {list}"
    );
    assert!(
        list.contains("<link rel=\"icon\" href=\"data:image/svg+xml,"),
        "{list}"
    );
    assert!(
        !list.contains("system-ui,sans-serif;margin:0;background:#fff"),
        "{list}"
    );
    assert!(
        list.contains("<ul class=\"projects\"><li><a href=\"/alpha/\">alpha works</a>"),
        "{list}"
    );
}

#[tokio::test]
async fn the_list_names_every_project_and_why_one_is_not_served() {
    let machine = Machine::new();
    let gamma = Entry {
        plugins: vec!["fixture-missing".into()],
        ..Entry::of(&machine.tree.path("alpha")).unwrap()
    };
    machine.tree.write("gamma/docs/README.md", "# Docs\n");
    let gamma = Entry {
        id: "gamma".into(),
        name: "gamma".into(),
        root: machine.tree.path("gamma").canonicalize().unwrap(),
        ..gamma
    };
    let path = machine.tree.path("machine");
    let mut registry = Registry::read(&path).unwrap();
    registry.projects.push(gamma);
    registry.write(&path).unwrap();
    let list = machine.get("/", &[]).await;
    assert_eq!(list.status(), StatusCode::OK);
    let list = text(list).await;
    for (link, name) in [
        ("/alpha/", "alpha works"),
        ("/beta/", "beta works"),
        ("/gamma/", "gamma"),
    ] {
        assert!(
            list.contains(&format!("<a href=\"{link}\">{name}</a>")),
            "{list}"
        );
    }
    assert!(
        list.contains("this Symphony lacks the plugin fixture-missing, which gamma names"),
        "{list}"
    );
    let refused = machine.get("/gamma/tasks", &[PAGE]).await;
    assert_eq!(refused.status(), StatusCode::SERVICE_UNAVAILABLE);
    let said = text(refused).await;
    assert!(said.contains("run `just symphony` in"), "{said}");
    // A refused project's tokens are still its own to read.
    let tokens = machine.get("/gamma/theme.css", &[]).await;
    assert_eq!(tokens.status(), StatusCode::OK);
}

/// A hub of `shared`'s project set as its hub, on a clock the test moves, from `at`.
fn clocked(shared: &Shared, at: f64) -> (Arc<crate::live::Hub>, Clock) {
    let clock = Clock(Arc::new(AtomicU64::new(at.to_bits())));
    let mut hub = crate::live::Hub::new(&shared.root, &shared.root.join("build/client"));
    hub.clock = {
        let clock = clock.clone();
        Box::new(move || clock.now())
    };
    let hub = Arc::new(hub);
    assert!(shared.api.set(hub.clone()).is_ok());
    (hub, clock)
}

/// A hub's clock, as the test moves it.
#[derive(Clone)]
struct Clock(Arc<AtomicU64>);

impl Clock {
    fn now(&self) -> f64 {
        f64::from_bits(self.0.load(Ordering::SeqCst))
    }

    fn set(&self, at: f64) {
        self.0.store(at.to_bits(), Ordering::SeqCst);
    }
}

/// `id`'s row of the project list `list`.
fn row<'a>(list: &'a str, id: &str) -> &'a str {
    list.split("<li>")
        .find(|row| row.contains(&format!("href=\"/{id}/\"")))
        .unwrap_or_else(|| panic!("no row for {id}: {list}"))
}

/// The dot a row opens with, by its signal's name and what it says.
fn dot(signal: &str, said: &str) -> String {
    format!(
        "<span class=\"dot {signal}\" role=\"img\" aria-label=\"{said}\" title=\"{said}\"></span>"
    )
}

/// The line a row says how its board stands in, in its signal's colour.
fn line(signal: &str, said: &str) -> String {
    format!("<span class=\"health {signal}\" role=\"status\">{said}</span>")
}

#[tokio::test]
async fn the_list_warns_under_a_project_whose_board_is_unwell_until_it_is_well() {
    let machine = Machine::new();
    let starter = &machine.projects.starter;
    let at = symphony_board::sweep::trigger::now();
    let (hub, clock) = clocked(starter, at);
    // Plain text, as the pages' notice says it too: no markup's backticks, and today's words.
    let warning = line("red", DISPATCH_FAILING);

    hub.health.dispatched(at, Err("the board did not answer"));
    let list = text(machine.get("/", &[]).await).await;
    let alpha = row(&list, "alpha");
    assert!(
        alpha.starts_with(&dot("red", "failing")) && alpha.contains(&warning),
        "{list}"
    );
    assert_eq!(
        list.matches("class=\"health ").count(),
        1,
        "beta is well: {list}"
    );
    // The project list as data says the same warning, as plain text, and only of alpha.
    let unwell = listed(&machine, "alpha").await.unwell.unwrap();
    assert!(
        unwell.starts_with("The task board is unwell: its callback dispatch is failing")
            && unwell.ends_with("to see why."),
        "{unwell}"
    );
    assert_eq!(listed(&machine, "beta").await.unwell, None);

    hub.health.dispatched(at + 1.0, Ok(()));
    clock.set(at + 1.0);
    let list = text(machine.get("/", &[]).await).await;
    assert!(
        list.contains(&warning),
        "one good pass does not clear it: {list}"
    );
    let later = at + 1.0 + symphony_board::board_health::CLEARS_AFTER;
    hub.health.dispatched(later, Ok(()));
    clock.set(later);
    let list = text(machine.get("/", &[]).await).await;
    assert!(!list.contains("class=\"health "), "{list}");
    assert!(
        row(&list, "alpha").starts_with(&dot("green", "well")),
        "{list}"
    );
    assert_eq!(listed(&machine, "alpha").await.unwell, None, "well again");
}

/// Each project's dot is its board's worst signal: green while well, with no line; amber while its
/// writes took a second or more at p90 over the hour, or a request went unanswered in it, its line
/// saying so with the figures, until the hour holds none; red while its dispatch fails, in today's
/// words. Of two projects, only the one not well carries a line, and the list as data says the
/// same line.
#[tokio::test]
async fn the_list_dots_each_board_by_its_worst_signal_and_says_why_only_when_it_is_not_green() {
    use std::panic::Location;
    use symphony_board::board_health::{Query, QueryLog, WINDOW};
    let machine = Machine::new();
    let at = symphony_board::sweep::trigger::now();
    let (alpha, clock) = clocked(&machine.projects.starter, at);
    let served = machine.projects.served();
    let beta = served.iter().find(|p| p.entry.id == "beta").unwrap();
    let (_beta, _) = clocked(&beta.shared, at);
    let log = Arc::new(QueryLog::default());
    alpha.health.reading(Arc::clone(&log));
    let request = |at: f64, ms: f64, write: bool, failed: bool| {
        log.record(Query {
            at,
            ms,
            source: Location::caller(),
            write,
            failed,
        });
    };
    let list = || async { text(machine.get("/", &[]).await).await };

    // Well: a green dot and no line, beta's alike.
    let well = list().await;
    for id in ["alpha", "beta"] {
        let row = row(&well, id);
        assert!(row.starts_with(&dot("green", "well")), "{well}");
        assert!(!row.contains("class=\"health"), "{well}");
    }
    assert_eq!(listed(&machine, "alpha").await.unwell, None);

    // Writes at 2.6 s at p90 over the hour (the ninth of ten): amber, in whole seconds.
    for n in 0..8 {
        request(at - 600.0 + f64::from(n), 40.0, true, false);
    }
    request(at - 90.0, 2_600.0, true, false);
    request(at - 60.0, 3_400.0, true, false);
    let slow = list().await;
    let said = "Board slow (writes 3 s this hour)";
    assert!(
        row(&slow, "alpha").starts_with(&dot("amber", "slow or unstaffed"))
            && row(&slow, "alpha").contains(&line("amber", said)),
        "{slow}"
    );
    assert!(
        !row(&slow, "beta").contains("class=\"health"),
        "only the project not well carries a line: {slow}"
    );
    assert_eq!(
        listed(&machine, "alpha").await.unwell.as_deref(),
        Some(said)
    );
    assert_eq!(listed(&machine, "beta").await.unwell, None);

    // An hour on, one request went unanswered and every write was quick: amber, its count alone.
    let later = at + WINDOW;
    clock.set(later);
    request(later - 30.0, 10_000.0, false, true);
    request(later - 20.0, 30.0, true, false);
    let unanswered = list().await;
    assert!(
        row(&unanswered, "alpha").contains(&line("amber", "Board slow (1 unanswered this hour)")),
        "{unanswered}"
    );

    // Another hour on, with no request in it: green again.
    clock.set(later + WINDOW);
    let quiet = list().await;
    assert!(
        row(&quiet, "alpha").starts_with(&dot("green", "well"))
            && !quiet.contains("class=\"health"),
        "{quiet}"
    );

    // A failing dispatch is red, in today's words, whatever the hour held.
    request(later + WINDOW - 5.0, 3_000.0, true, true);
    alpha
        .health
        .dispatched(later + WINDOW, Err("the board did not answer"));
    let failing = list().await;
    let alpha_row = row(&failing, "alpha");
    assert!(alpha_row.starts_with(&dot("red", "failing")), "{failing}");
    assert!(
        alpha_row.contains(&line("red", DISPATCH_FAILING)),
        "{failing}"
    );
    assert!(
        !alpha_row.contains("Board slow"),
        "red says its words alone: {failing}"
    );
}

/// What the pages and the list say today of a board whose callback dispatch fails.
const DISPATCH_FAILING: &str = "The task board is unwell: its callback dispatch is failing, so \
                                agents may not hear from it. Run “just tasks status” to see why.";

#[tokio::test]
async fn the_list_says_calmly_that_a_project_with_no_board_has_none() {
    let machine = Machine::new();
    let starter = &machine.projects.starter;
    let hub = Arc::new(crate::live::Hub::new(
        &starter.root,
        &starter.root.join("build/client"),
    ));
    assert!(starter.api.set(hub.clone()).is_ok());
    let missing =
        symphony_board::server::ServerError::NoSurreal(machine.tree.path("surreal")).to_string();
    assert!(
        hub.reconnect(&mut |_: &Path| Err(missing.clone()))
            .is_none()
    );
    let list = text(machine.get("/", &[]).await).await;
    let alpha = row(&list, "alpha");
    assert!(
        alpha.contains("<span class=\"counts\">No task board</span>"),
        "its counts not read yet: {list}"
    );
    assert!(
        alpha.starts_with(&dot("green", "well"))
            && !list.contains("class=\"health")
            && !list.contains("⚠"),
        "no board is not an unwell board: {list}"
    );
    assert!(
        !row(&list, "beta").to_lowercase().contains("no task board"),
        "{list}"
    );

    // With its counts read, it says so after them, where a board's task counts would be.
    let served = machine.projects.served();
    let alpha = served.iter().find(|p| p.entry.id == "alpha").unwrap();
    let counts = symphony_api::Counts {
        waiting: 2,
        ..Default::default()
    };
    let none = Blocked::default();
    assert!(
        list_row(alpha, Some(&counts), None, none, true).contains(
            "<span class=\"counts\">2 decision cards waiting on you · no task board</span>"
        )
    );
    assert!(
        list_row(alpha, Some(&counts), None, none, false)
            .contains("<span class=\"counts\">2 decision cards waiting on you</span>")
    );
    // A board's in-progress count is the status's, the board's Build and Test columns in it, and
    // it says so: the board's own In progress column leaves them out.
    let counts = symphony_api::Counts {
        waiting: 1,
        tasks: Some(symphony_api::TaskCounts {
            in_progress: 9,
            in_review: 3,
            agents_live: 1,
        }),
        ..Default::default()
    };
    let row = list_row(alpha, Some(&counts), None, none, false);
    assert!(
        row.contains(
            "<span class=\"counts\">1 decision card waiting on you · 9 in progress (Build and Test included) · 3 in review</span>"
        ),
        "{row}"
    );
    // A refused project says why alone: no dot and no line, whatever its board's standing.
    let mut refused = alpha.clone();
    refused.refused = Some("lacks the plugin fixture-uncompiled".into());
    let failing = Standing {
        signal: Signal::Red,
        line: "The task board is unwell".into(),
    };
    let row = list_row(&refused, Some(&counts), Some(&failing), none, false);
    assert_eq!(
        row,
        "<li><a href=\"/alpha/\">alpha works</a><span class=\"refused\">lacks the plugin \
         fixture-uncompiled</span></li>",
    );
}

/// The blocked part of the counts: none while nothing is blocked, the count alone while none of
/// them waits on the owner's decision, and how many do when some do; never a person.
#[test]
fn the_blocked_part_counts_the_blocked_and_those_waiting_on_the_owners_decision() {
    let blocked = |tasks, on_decision| Blocked { tasks, on_decision }.said();
    assert_eq!(blocked(0, 0), "");
    assert_eq!(blocked(1, 0), " · <strong>1 blocked</strong>");
    assert_eq!(
        blocked(41, 9),
        " · <strong>41 blocked, 9 of them waiting on your decision</strong>"
    );
    assert_eq!(
        blocked(2, 2),
        " · <strong>2 blocked, 2 of them waiting on your decision</strong>"
    );
}

#[tokio::test]
async fn the_list_warns_under_a_project_whose_store_cannot_be_served_and_never_calls_it_boardless()
{
    let machine = Machine::new();
    let starter = &machine.projects.starter;
    let hub = Arc::new(crate::live::Hub::new(
        &starter.root,
        &starter.root.join("build/client"),
    ));
    assert!(starter.api.set(hub.clone()).is_ok());
    // The SurrealDB that served the board is gone, as after a version bump before the new one is
    // fetched; its store is still there.
    let unserved = symphony_board::server::ServerError::Unserved {
        surreal: machine.tree.path("surreal"),
        data: machine.tree.path("symphony/tasks/data"),
    }
    .to_string();
    assert!(
        hub.reconnect(&mut |_: &Path| Err(unserved.clone()))
            .is_none()
    );
    let list = text(machine.get("/", &[]).await).await;
    let alpha = row(&list, "alpha");
    assert!(
        alpha.starts_with(&dot("red", "failing"))
            && alpha.contains("class=\"health red\"")
            && alpha.contains("The task board is unwell"),
        "{list}"
    );
    assert!(!list.to_lowercase().contains("no task board"), "{list}");
}

#[tokio::test]
async fn theme_tokens_are_served_per_project_and_revalidated_by_etag() {
    let machine = Machine::new();
    let first = machine.get("/beta/theme.css", &[]).await;
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(
        first.headers()[header::CONTENT_TYPE],
        "text/css; charset=utf-8"
    );
    let etag = first.headers()[header::ETAG].to_str().unwrap().to_string();
    assert_eq!(text(first).await, ":root{--primary:#0b0;}\n");
    let again = machine
        .get("/beta/theme.css", &[("if-none-match", &etag)])
        .await;
    assert_eq!(again.status(), StatusCode::NOT_MODIFIED);
    let weak = format!("W/{etag}");
    let again = machine
        .get("/beta/theme.css", &[("if-none-match", &weak)])
        .await;
    assert_eq!(again.status(), StatusCode::NOT_MODIFIED);
    let alpha = machine
        .get("/alpha/theme.css", &[("if-none-match", &etag)])
        .await;
    assert_eq!(
        alpha.status(),
        StatusCode::OK,
        "another project's tokens differ"
    );
}

#[tokio::test]
async fn front_names_the_projects_and_answers_loopback_alone() {
    let machine = Machine::new();
    let front = machine.get(symphony_api::FRONT_PATH, &[]).await;
    assert_eq!(front.status(), StatusCode::OK);
    let front: symphony_api::Front = serde_json::from_str(&text(front).await).unwrap();
    assert_eq!(front.projects, ["alpha", "beta"]);
    assert_eq!(front.pin, symphony::machine::PIN);
    let mut request = Request::builder()
        .uri(symphony_api::FRONT_PATH)
        .header(header::HOST, "rebound.example");
    request = request.header("x", "y");
    let response = router(machine.projects.clone())
        .layer(MockConnectInfo(
            "127.0.0.1:1".parse::<SocketAddr>().unwrap(),
        ))
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

/// The project list as data: every project, the starter's with its hub's counts, its first card,
/// its feed's version, its task asks and that this server writes the board it read, and one with no
/// hub yet with none of them.
#[tokio::test]
async fn the_project_list_names_each_project_with_its_counts_first_card_and_task_asks() {
    use crate::testing::board::{Fixture, Seed};
    let machine = Machine::new();
    let root = crate::testing::repository_holding(
        &machine.tree,
        "alpha",
        "docs/TOPIC.md",
        "# Topic\n\n- [ ] **initial-card** — Initial question.\n",
    );
    let hub = Arc::new(crate::live::Hub::new(&root, &root.join("build/client")));
    hub.refresh_page("", &root).unwrap();
    let board = Fixture::new(
        Seed::default()
            .session("worker")
            .task("tools/ask", "in_progress"),
    );
    board
        .store
        .say("tools/ask", "worker", "flat or falling?", Some("owner"))
        .unwrap();
    hub.observe(&board.store);
    assert!(machine.projects.starter.api.set(hub.clone()).is_ok());

    let response = machine.get(symphony_api::PROJECTS_PATH, &[]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    let list: symphony_api::ProjectList = serde_json::from_str(&text(response).await).unwrap();
    assert_eq!(list.pin, symphony::machine::PIN);
    let installation = list.installation.clone().unwrap();
    assert_eq!(
        installation,
        symphony::machine::installation_id_in(&machine.tree.path("machine")).unwrap(),
        "the machine folder the registry is in names the server"
    );
    assert_eq!(
        list.projects
            .iter()
            .map(|project| (project.id.as_str(), project.name.as_str()))
            .collect::<Vec<_>>(),
        [("alpha", "alpha works"), ("beta", "beta works")]
    );
    let alpha = &list.projects[0];
    let page = hub.pages.read().unwrap()[""].board.clone();
    assert_eq!(alpha.version, Some(page.version));
    assert_eq!(alpha.waiting, Some(1));
    assert_eq!(
        alpha.next,
        Some(symphony_api::ListedCard {
            title: page.next[0].title.clone(),
            href: "/alpha/docs/TOPIC.md#initial-card".into(),
        })
    );
    let asks = alpha.asks.as_ref().unwrap();
    assert_eq!(asks.questions["owner"], 1);
    assert!(asks.verdicts.is_empty());
    assert_eq!((&alpha.refused, &alpha.unwell), (&None, &None));
    assert_eq!(
        alpha.writes,
        Some(true),
        "a board read at this build's schema"
    );
    // Beta's hub is not made yet: named, with nothing of its board.
    let beta = &list.projects[1];
    assert_eq!(
        (
            beta.version,
            beta.waiting,
            &beta.next,
            &beta.asks,
            &beta.refused,
            beta.writes
        ),
        (None, None, &None, &None, &None, None)
    );
    // Asked again, the same server answers the same id.
    assert_eq!(
        project_list(&machine).await.installation,
        Some(installation)
    );
}

/// Work waiting to start that no live orchestrator staffs is amber: nothing while its one
/// orchestrator is live, `orchestrator stale` while that one is stale, one line with the slow part
/// when the board is slow too, and `no orchestrator` once it is gone. The counts say how many tasks
/// are blocked and how many of them wait on the owner's decision, a subtask and a block that names
/// nothing left out. Drawing the list, or listing it as data, sends the board no request.
#[tokio::test]
async fn the_list_says_queued_work_no_live_orchestrator_staffs_and_counts_the_blocked() {
    use crate::testing::board::{Fixture, GONE, HOST, NOW, Seed, task};
    use std::panic::Location;
    use std::sync::atomic::AtomicUsize;
    use symphony_board::board_health::{Query, QueryLog, queries_of};
    use symphony_board::rows::{SessionRow, TaskRow};
    use symphony_board::waits_on::{BLOCKED, Blocked as Block};
    let machine = Machine::new();
    let root = crate::testing::repository_holding(
        &machine.tree,
        "alpha",
        "docs/TOPIC.md",
        "# Topic\n\n- [ ] **initial-card** — Initial question.\n",
    );
    let at = symphony_board::sweep::trigger::now();
    let clock = Clock(Arc::new(AtomicU64::new(at.to_bits())));
    let mut hub = crate::live::Hub::new(&root, &root.join("build/client"));
    hub.clock = {
        let clock = clock.clone();
        Box::new(move || clock.now())
    };
    hub.refresh_page("", &root).unwrap();
    // A task blocked on `on`, or with no record of what it waits on.
    let blocked = |id: i64, slug: &str, on: Option<&str>, parent: Option<&str>| {
        let mut row = TaskRow {
            id,
            parent: parent.map(String::from),
            ..task(slug, "blocked")
        };
        if let Some(on) = on {
            let record = Block {
                on: on.into(),
                why: None,
                by: "quiet-plover".into(),
                at: NOW,
                from: Some("ready".into()),
            };
            row.fields.insert(
                symphony_board::red::FIELDS.into(),
                [(BLOCKED.to_string(), record.value())].into(),
            );
        }
        row
    };
    let board = Fixture::new(
        Seed::default()
            .session_row(SessionRow {
                handle: "amber-heron".into(),
                kind: "orchestrator".into(),
                connected: NOW,
                last_seen: NOW,
                ..Default::default()
            })
            .task("tools/queued", "ready")
            .role("tools/queued", "amber-heron", "orchestrator")
            .task_row(blocked(
                2,
                "tools/card",
                Some("decision:projects-health"),
                None,
            ))
            .task_row(blocked(3, "tools/stamp", Some("check:owner"), None))
            .task_row(blocked(4, "tools/tests", Some("check:tests"), None))
            .task_row(blocked(5, "tools/silent", None, None))
            .task_row(blocked(6, "tools/unnamed", Some("nothing"), None))
            .task_row(blocked(
                7,
                "tools/card/work",
                Some("decision:projects-health"),
                Some("tools/card"),
            )),
    );
    let client = board.store.client();
    hub.health
        .reading(queries_of(client.addr(), "sf", client.database()));
    // The hub's reader, the way it reads the board for a page, opens this board, counted: a read
    // through it is a request this board's count holds.
    let connects = Arc::new(AtomicUsize::new(0));
    hub.connect_reader = {
        let (counted, client) = (Arc::clone(&connects), board.other_client());
        Box::new(move |_| {
            counted.fetch_add(1, Ordering::SeqCst);
            symphony_board::Store::with(
                symphony_board::rpc::Client::new(client.addr(), "sf", client.database()),
                Box::new(|| NOW),
                Box::new(|_| false),
                HOST,
            )
            .map_err(|e| e.to_string())
        })
    };
    hub.observe(&board.store);
    let hub = Arc::new(hub);
    assert!(machine.projects.starter.api.set(hub.clone()).is_ok());
    let list = || async { text(machine.get("/", &[]).await).await };

    // Drawn and listed from the hub's memory: the board's request count does not move, and no
    // reader of it is opened.
    let requests = || hub.health.report(clock.now()).queries.count;
    let read = requests();
    assert!(read > 0, "the board's read is counted");
    let live = list().await;
    listed(&machine, "alpha").await;
    assert_eq!(
        (requests(), connects.load(Ordering::SeqCst)),
        (read, 0),
        "the list and the listing sent the board no request and opened no reader of it"
    );
    // From here the hour's requests are the test's own.
    let log = Arc::new(QueryLog::default());
    hub.health.reading(Arc::clone(&log));

    let alpha = row(&live, "alpha");
    assert!(
        alpha.contains(
            "<span class=\"counts\">1 decision card waiting on you · 0 in progress (Build and \
             Test included) · 0 in review · <strong>3 blocked, 2 of them waiting on your \
             decision</strong></span>"
        ),
        "the card and the owner's stamp wait on him; the check, the subtask and the blocks \
         that name nothing are not his: {live}"
    );
    assert!(
        alpha.starts_with(&dot("green", "well")) && !alpha.contains("class=\"health"),
        "its one orchestrator is live: {live}"
    );

    board.advance(200.0);
    hub.observe(&board.store);
    let stale = list().await;
    assert!(
        row(&stale, "alpha").starts_with(&dot("amber", "slow or unstaffed"))
            && row(&stale, "alpha").contains(&line("amber", "orchestrator stale")),
        "{stale}"
    );
    assert_eq!(
        listed(&machine, "alpha").await.unwell.as_deref(),
        Some("orchestrator stale")
    );

    // A board read holds the hub's task state across its reads of the board, as a slow board's
    // does for as long as it is slow: the list and the listing answer meanwhile from the board's
    // last read, waiting on none of it. One that waited would wait until the hold ends.
    let held = hub.hold_board_read();
    let drawn = asked_aside(&machine, "/");
    let listing = asked_aside(&machine, symphony_api::PROJECTS_PATH);
    let drawn = drawn
        .recv_timeout(BACKSTOP)
        .expect("the list waited on the board's read");
    let listing = listing
        .recv_timeout(BACKSTOP)
        .expect("the listing waited on the board's read");
    drop(held);
    assert_eq!(row(&drawn, "alpha"), row(&stale, "alpha"));
    let listing: symphony_api::ProjectList = serde_json::from_str(&listing).unwrap();
    assert_eq!(
        listing.projects[0].unwell.as_deref(),
        Some("orchestrator stale")
    );

    for (n, ms) in [
        40.0, 40.0, 40.0, 40.0, 40.0, 40.0, 40.0, 40.0, 1_600.0, 1_700.0,
    ]
    .into_iter()
    .enumerate()
    {
        #[expect(clippy::cast_precision_loss, reason = "ten requests")]
        let at = at - 100.0 + n as f64;
        log.record(Query {
            at,
            ms,
            source: Location::caller(),
            write: true,
            failed: false,
        });
    }
    let both = list().await;
    assert!(
        row(&both, "alpha").contains(&line(
            "amber",
            "Board slow (writes 2 s this hour) · orchestrator stale"
        )),
        "one line, both parts: {both}"
    );
    assert_eq!(both.matches("class=\"health ").count(), 1, "{both}");

    hub.health.reading(Arc::new(QueryLog::default()));
    board.advance(GONE);
    hub.observe(&board.store);
    let gone = list().await;
    assert!(
        row(&gone, "alpha").contains(&line("amber", "no orchestrator")),
        "{gone}"
    );
}

/// The one server's project list, as `machine` answers it on loopback.
async fn project_list(machine: &Machine) -> symphony_api::ProjectList {
    serde_json::from_str(&text(machine.get(symphony_api::PROJECTS_PATH, &[]).await).await).unwrap()
}

/// The project `id` as `machine`'s project list lists it.
async fn listed(machine: &Machine, id: &str) -> symphony_api::ListedProject {
    let list = project_list(machine).await;
    list.projects.into_iter().find(|p| p.id == id).unwrap()
}

/// A refused project is listed with why, and nothing of a board its build cannot serve.
#[tokio::test]
async fn the_project_list_says_why_a_project_is_refused_and_nothing_else_of_it() {
    let machine = Machine::new();
    let path = machine.tree.path("machine");
    let mut registry = Registry::read(&path).unwrap();
    let beta = registry
        .projects
        .iter_mut()
        .find(|entry| entry.id == "beta")
        .unwrap();
    beta.plugins.push("fixture-uncompiled".into());
    registry.write(&path).unwrap();
    let beta = listed(&machine, "beta").await;
    assert!(
        beta.refused
            .as_deref()
            .is_some_and(|why| why.contains("lacks the plugin fixture-uncompiled")),
        "{beta:?}"
    );
    assert_eq!(
        (beta.version, &beta.asks, &beta.unwell, beta.writes),
        (None, &None, &None, None)
    );
}

/// The project list answers a tailnet peer that names this machine by its tailnet name, as every
/// route but `/api/front` does, and nobody else.
#[tokio::test]
async fn the_project_list_answers_this_machines_tailnet_name_and_front_does_not() {
    // The phone is under the login this machine is under.
    let peer = |_: &str| {
        Ok(Some(
            r#"{"Node":{"Name":"phone.tailnet-name.ts.net."},"UserProfile":{"ID":11,"LoginName":"owner@example.com"}}"#.to_string(),
        ))
    };
    let names =
        || Ok(r#"{"Self":{"DNSName":"studio.tailnet-name.ts.net.","UserID":11}}"#.to_string());
    let machine = Machine::with_gate(
        Gate::new(Arc::new(peer), Arc::new(Instant::now)).with_status(Arc::new(names)),
    );
    let phone = "100.101.102.103:1";
    let tailnet = "studio.tailnet-name.ts.net:8765";
    let listed = machine
        .named(phone, tailnet, symphony_api::PROJECTS_PATH, &[])
        .await;
    assert_eq!(listed.status(), StatusCode::OK);
    let list: symphony_api::ProjectList = serde_json::from_str(&text(listed).await).unwrap();
    assert_eq!(list.projects.len(), 2);
    assert_eq!(
        machine
            .named(phone, tailnet, symphony_api::FRONT_PATH, &[])
            .await
            .status(),
        StatusCode::FORBIDDEN,
        "the front answers loopback's name alone"
    );
    for (client, host) in [
        (phone, "rebound.example"),
        ("192.0.2.7:1", tailnet),
        ("127.0.0.1:1", "rebound.example"),
    ] {
        assert_eq!(
            machine
                .named(client, host, symphony_api::PROJECTS_PATH, &[])
                .await
                .status(),
            StatusCode::FORBIDDEN,
            "{client} naming {host}"
        );
    }
}

#[tokio::test]
async fn a_client_outside_this_machine_and_its_tailnet_reads_nothing() {
    let machine = Machine::new();
    for path in ["/", "/beta/theme.css", "/docs/README.md"] {
        let response = machine.from("192.0.2.7:1", path, &[]).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
    }
}

#[tokio::test]
async fn a_host_that_names_another_machine_reads_no_route_of_any_project() {
    let machine = Machine::new();
    for path in [
        "/",
        "/beta",
        "/beta/theme.css",
        "/alpha/",
        "/docs/README.md",
        "/tasks",
    ] {
        let response = router(machine.projects.clone())
            .layer(MockConnectInfo(
                "127.0.0.1:1".parse::<SocketAddr>().unwrap(),
            ))
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header(header::HOST, "rebound.example")
                    .header("sec-fetch-mode", "navigate")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
    }
    assert_eq!(
        machine.get("/beta/theme.css", &[]).await.status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_refused_project_is_refused_through_todays_paths_as_under_its_prefix() {
    let machine = Machine::new();
    let path = machine.tree.path("machine");
    let original = Registry::read(&path).unwrap();
    // The checkouts' common folder, as the registry names them.
    let folders = original.projects[0]
        .root
        .parent()
        .unwrap()
        .display()
        .to_string();
    for lack in ["plugin", "pin", "schema"] {
        let mut registry = original.clone();
        let beta = registry
            .projects
            .iter_mut()
            .find(|entry| entry.id == "beta")
            .unwrap();
        match lack {
            "plugin" => beta.plugins.push("fixture-uncompiled".into()),
            "pin" => beta.pin += 1,
            _ => beta.schema += 1,
        }
        registry.write(&path).unwrap();
        let from_beta = ("referer", "http://127.0.0.1:1/beta/docs/");
        for (route, headers, kind) in [
            ("/beta/tasks", vec![], "text/html; charset=utf-8"),
            ("/tasks", vec![from_beta], "text/html; charset=utf-8"),
            ("/tasks", vec![from_beta, PAGE], "text/html; charset=utf-8"),
            ("/api/board", vec![from_beta], "application/json"),
            ("/beta/@work~x/api/board", vec![], "application/json"),
        ] {
            let response = machine.get(route, &headers).await;
            assert_eq!(
                response.status(),
                StatusCode::SERVICE_UNAVAILABLE,
                "{lack}: {route} {headers:?}"
            );
            assert_eq!(
                response.headers()[header::CONTENT_TYPE],
                kind,
                "{lack}: {route} {headers:?}"
            );
            let said = text(response).await;
            assert!(said.contains("does not serve beta"), "{lack}: {said}");
            assert!(
                !said.contains(folders.as_str()),
                "{lack}: the page names no folder: {said}"
            );
            if kind == "application/json" {
                let error: serde_json::Value = serde_json::from_str(&said).unwrap();
                assert!(
                    error["error"]
                        .as_str()
                        .unwrap()
                        .starts_with("The Symphony does not serve beta now: this Symphony"),
                    "{lack}: {said}"
                );
            } else {
                assert!(
                    said.contains(
                        "<div class=\"shell-notice\" role=\"alert\"><p><strong>The \
                                   Symphony does not serve beta now</strong>: this Symphony"
                    ),
                    "{lack}: {said}"
                );
                assert!(said.contains("href=\"/alpha/\""), "the list: {said}");
            }
        }
        // The starter's project, which this build serves, still answers today's paths.
        let starter = machine.get("/tasks", &[PAGE]).await;
        assert_eq!(location(&starter), "/alpha/tasks", "{lack}");
    }
}

/// The one server's own JSON answers, its `/api/front` and a refused project's error, say
/// `nosniff` as a project's own answers do, under its prefix and through today's paths.
#[tokio::test]
async fn the_one_servers_own_json_answers_say_nosniff_as_a_projects_do() {
    let machine = Machine::new();
    let path = machine.tree.path("machine");
    let mut registry = Registry::read(&path).unwrap();
    let beta = registry
        .projects
        .iter_mut()
        .find(|entry| entry.id == "beta")
        .unwrap();
    beta.plugins.push("fixture-uncompiled".into());
    registry.write(&path).unwrap();
    let from_beta = ("referer", "http://127.0.0.1:1/beta/docs/");
    for (route, headers, status) in [
        (symphony_api::FRONT_PATH, vec![], StatusCode::OK),
        (symphony_api::PROJECTS_PATH, vec![], StatusCode::OK),
        ("/beta/api/board", vec![], StatusCode::SERVICE_UNAVAILABLE),
        (
            "/api/board",
            vec![from_beta],
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        ("/alpha/api/nope", vec![], StatusCode::NOT_FOUND),
    ] {
        let response = machine.get(route, &headers).await;
        assert_eq!(response.status(), status, "{route} {headers:?}");
        let said = |name| response.headers()[name].to_str().unwrap().to_string();
        assert_eq!(
            [
                said(header::CONTENT_TYPE),
                said(header::X_CONTENT_TYPE_OPTIONS)
            ],
            ["application/json", "nosniff"],
            "{route} {headers:?}"
        );
    }
}

#[tokio::test]
async fn the_starters_id_taken_by_another_checkout_serves_that_checkout() {
    let machine = Machine::new();
    machine.tree.write(
        "alpha/symphony.toml",
        "[project]\nid = \"gamma\"\nname = \"Gamma private\"\n",
    );
    machine.tree.write(
        "replacement/symphony.toml",
        "[project]\nid = \"alpha\"\nname = \"Replacement Alpha\"\n",
    );
    machine
        .tree
        .write("replacement/docs/README.md", "# Replacement\n");
    Registry::change(&machine.tree.path("machine"), |registry| {
        registry.add(Entry::of(&machine.tree.path("alpha"))?)?;
        registry.add(Entry::of(&machine.tree.path("replacement"))?)
    })
    .unwrap();
    let alpha = text(machine.get("/alpha/", &[PAGE]).await).await;
    assert!(
        alpha.contains("<title>Replacement Alpha</title>"),
        "{alpha}"
    );
    assert!(!alpha.contains("Gamma private"), "{alpha}");
    let gamma = text(machine.get("/gamma/", &[PAGE]).await).await;
    assert!(gamma.contains("<title>Gamma private</title>"), "{gamma}");
    assert!(gamma.contains("\"base\":\"/gamma\""), "{gamma}");
    let served = machine.projects.served();
    for served in &served {
        assert_eq!(
            served.shared.root.canonicalize().unwrap(),
            served.entry.root,
            "{}",
            served.entry.id
        );
    }
    // Today's paths stay the starter's checkout's, under the id it holds now.
    let bookmark = machine.get("/docs/README.md", &[PAGE]).await;
    assert_eq!(location(&bookmark), "/gamma/docs/README.md");
    assert!(
        !machine.projects.starter.stopping(),
        "the starter's state is the server's own"
    );
}

#[tokio::test]
async fn a_moved_starter_is_served_from_its_new_root() {
    let machine = Machine::new();
    std::fs::rename(machine.tree.path("alpha"), machine.tree.path("moved-alpha")).unwrap();
    Registry::change(&machine.tree.path("machine"), |registry| {
        registry.add(Entry::of(&machine.tree.path("moved-alpha"))?)
    })
    .unwrap();
    let served = machine.projects.served();
    let alpha = served
        .iter()
        .find(|served| served.entry.id == "alpha")
        .unwrap();
    assert!(alpha.entry.root.ends_with("moved-alpha"));
    assert_eq!(alpha.shared.root, alpha.entry.root);
    assert_eq!(served.len(), 2, "the old root is served no more");
    let page = machine.get("/alpha/", &[PAGE]).await;
    assert_eq!(page.status(), StatusCode::OK);
    let bookmark = machine.get("/docs/README.md", &[PAGE]).await;
    assert_eq!(location(&bookmark), "/alpha/docs/README.md");
}

#[tokio::test]
async fn the_registry_is_read_again_when_it_changes_and_a_project_moved_is_served_anew() {
    let machine = Machine::new();
    let before = machine.projects.served();
    let beta = before
        .iter()
        .find(|served| served.entry.id == "beta")
        .unwrap();
    let path = machine.tree.path("machine");
    // Unchanged, the same state serves.
    let again = machine.projects.served();
    assert!(Arc::ptr_eq(&again[1].shared, &beta.shared));
    machine.tree.write("moved/docs/README.md", "# Docs\n");
    machine.tree.write(
        "moved/symphony.toml",
        "[project]\nname = \"beta works\"\nid = \"beta\"\n",
    );
    std::fs::remove_dir_all(machine.tree.path("beta")).unwrap();
    Registry::change(&path, |registry| {
        registry.add(Entry::of(&machine.tree.path("moved")).unwrap())
    })
    .unwrap();
    let after = machine.projects.served();
    let moved = after
        .iter()
        .find(|served| served.entry.id == "beta")
        .unwrap();
    assert!(
        !Arc::ptr_eq(&moved.shared, &beta.shared),
        "a new root is a new project state"
    );
    assert!(moved.entry.root.ends_with("moved"));
    // The starter stays first and the same.
    assert!(Arc::ptr_eq(&after[0].shared, &machine.projects.starter));
    // A registry that does not read leaves the starter's project alone.
    std::fs::write(path.join(PROJECTS), "not json at all").unwrap();
    let broken = machine.projects.served();
    assert_eq!(broken.len(), 1);
    assert_eq!(broken[0].entry.id, "alpha");
    machine.projects.stop();
    assert!(
        !machine.projects.starter.stopping(),
        "the starter's stop is the server's own"
    );
}

#[test]
fn a_page_load_is_a_navigation_and_a_fetch_is_not() {
    let headers = |pairs: &[(&'static str, &'static str)]| {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, HeaderValue::from_static(value));
        }
        map
    };
    assert!(navigation(&Method::GET, &headers(&[PAGE])));
    assert!(navigation(
        &Method::HEAD,
        &headers(&[("accept", "text/html")])
    ));
    assert!(!navigation(&Method::POST, &headers(&[PAGE])));
    assert!(!navigation(
        &Method::GET,
        &headers(&[("sec-fetch-mode", "cors"), ("accept", "text/html")])
    ));
    assert!(!navigation(
        &Method::GET,
        &headers(&[("accept", "application/json")])
    ));
    assert!(!navigation(&Method::GET, &headers(&[])));
}

/// A project in `tree` naming `plugins`, those of `processes` with a `[plugins.process]` command.
fn plugged(tree: &Tree, id: &str, plugins: &[&str], processes: &[&str]) -> Entry {
    let names: Vec<String> = plugins.iter().map(|name| format!("\"{name}\"")).collect();
    let mut file = format!(
        "[project]\nid = \"{id}\"\n[plugins]\ncrates = [{}]\n",
        names.join(", ")
    );
    if !processes.is_empty() {
        file.push_str("[plugins.process]\n");
        for name in processes {
            file.push_str(&format!("{name} = [\"bin/{name}\"]\n"));
        }
    }
    tree.write(&format!("{id}/docs/README.md"), "# Docs\n");
    tree.write(&format!("{id}/symphony.toml"), &file);
    Entry::of(&tree.path(id)).unwrap()
}

/// The server of the release `name`, at this build's pin and board schema.
fn released(name: &str) -> Build {
    Build {
        release: Some(name.into()),
        plugins: Vec::new(),
        ..Build::this(Path::new("/work/alpha"))
    }
}

#[test]
fn a_release_serves_process_plugins_and_names_the_command_a_linked_one_needs() {
    let tree = Tree::new();
    let processes = plugged(&tree, "processes", &["shelf"], &["shelf"]);
    let linked = plugged(&tree, "linked", &["shelf", "map"], &["shelf"]);
    let release = released("release-1");
    assert_eq!(lacking(&release, "alpha", &processes), None);
    assert_eq!(
        lacking(&release, "alpha", &linked),
        Some(format!(
            "this Symphony is the release release-1, which serves a plugin only from its process: \
             linked names the plugin map linked: a [plugins.process] command for map in {}, then \
             `just symphony` in its checkout, lets this server serve it (docs/engineering/PLUGIN_PROTOCOL.md)",
            linked.root.join("symphony.toml").display()
        ))
    );
    // A project's build without the plugin still names the build that holds it.
    let plain = Build {
        release: None,
        ..release.clone()
    };
    assert!(lacking(&plain, "alpha", &linked).is_some_and(|why| {
        why.ends_with("run `just symphony` in linked's checkout, whose build holds them")
    }));
    // A project newer than the release waits for a release that serves it.
    let newer = Entry {
        pin: release.pin + 1,
        ..processes.clone()
    };
    assert_eq!(
        lacking(&release, "alpha", &newer),
        Some(format!(
            "this Symphony is the release release-1, at Symphony pin {}, older than processes's \
             {}: this server serves processes once the machine's `current` names a release that \
             does, which its reload follows",
            release.pin,
            release.pin + 1
        ))
    );
}

#[test]
fn a_project_naming_a_linked_plugin_says_it_holds_the_machine_on_a_projects_build() {
    use symphony::machine::release::{Manifest, Serves, Version};
    let tree = Tree::new();
    let linked = plugged(&tree, "linked", &["shelf"], &[]);
    let processes = plugged(&tree, "processes", &["shelf"], &["shelf"]);
    let version = Version { major: 0, minor: 3 };
    let manifest = Manifest {
        release: "release-1".into(),
        commit: "abc".into(),
        serves: Serves {
            schema: symphony_board::SCHEMA,
            additions: symphony_board::schema::ADDITIONS,
            protocol: version,
            tasks_api: version,
            pin: symphony::machine::PIN,
        },
        client: "client/0".into(),
        artifacts: Vec::new(),
    };
    assert_eq!(holding(None, &linked), "", "no release is installed");
    assert_eq!(holding(Some(&manifest), &processes), "");
    // Its own pages name its project file as its own, never by the checkout's path.
    assert_eq!(
        holding(Some(&manifest), &linked),
        "The machine's Symphony stays a project's build, not the release release-1: linked names \
         the linked plugin shelf, and a release's server serves a plugin only from its process: a \
         [plugins.process] command for it in its symphony.toml lets the release serve every \
         project (docs/engineering/PLUGIN_PROTOCOL.md)."
    );
}
