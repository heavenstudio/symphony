//! The one server's dispatch: projects under `/<id>/`, the list, the theme tokens, the redirects
//! of today's paths, and the registry read again when it changes.

use super::*;
use crate::gate::Gate;
use crate::testing::Tree;
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use std::time::Instant;

/// A machine folder registering `alpha` (the starter) and `beta`, each with a theme of its own,
/// and an install folder holding a client shell.
struct Machine {
    tree: Tree,
    projects: Arc<Projects>,
}

const SHELL: &str = "<!DOCTYPE html><html lang=\"en\"><head><title>Symphony</title>\
<link rel=\"icon\" href=\"data:,\"></head><body><div id=\"main\"></div></body></html>";

impl Machine {
    fn new() -> Machine {
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
        let machine = tree.path("machine");
        let mut registry = Registry::default();
        for id in ["alpha", "beta"] {
            registry.add(Entry::of(&tree.path(id)).unwrap()).unwrap();
        }
        registry.write(&machine).unwrap();
        let root = tree.path("alpha").canonicalize().unwrap();
        let starter = Shared::project(
            1,
            1,
            &root,
            Some(&tree.path("install")),
            Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
            "/alpha",
        );
        let projects = Projects::made(&machine, &starter, false);
        Machine { tree, projects }
    }

    async fn get(&self, path: &str, headers: &[(&str, &str)]) -> Response {
        self.from("127.0.0.1:1", path, headers).await
    }

    async fn from(&self, client: &str, path: &str, headers: &[(&str, &str)]) -> Response {
        let mut request = Request::builder()
            .uri(path)
            .header(header::HOST, "127.0.0.1:1");
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        router(self.projects.clone())
            .layer(MockConnectInfo(client.parse::<SocketAddr>().unwrap()))
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }
}

fn location(response: &Response) -> &str {
    response.headers()[header::LOCATION].to_str().unwrap()
}

async fn text(response: Response) -> String {
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
async fn a_project_page_carries_its_base_and_theme_and_is_remembered_for_the_list() {
    let machine = Machine::new();
    let page = machine.get("/beta/", &[PAGE]).await;
    assert_eq!(page.status(), StatusCode::OK);
    let cookie = page.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .to_string();
    assert!(cookie.starts_with("symphony-project=beta;"), "{cookie}");
    let body = text(page).await;
    assert!(body.contains("\"base\":\"/beta\""), "{body}");
    assert!(
        body.contains("--primary:#0b0;"),
        "beta's own tokens: {body}"
    );
    assert!(!body.contains("#a00"), "nothing of alpha's: {body}");
    assert!(body.contains("<title>beta works</title>"));
    // A fetch, not a page load, leaves the cookie as it is.
    let fetched = machine.get("/beta/", &[]).await;
    assert!(fetched.headers().get(header::SET_COOKIE).is_none());
    // The list opens the last project visited, while it is served.
    let back = machine
        .get("/", &[("cookie", "a=b; symphony-project=beta")])
        .await;
    assert_eq!(location(&back), "/beta/");
    let unknown = machine
        .get("/", &[("cookie", "symphony-project=gone")])
        .await;
    assert_eq!(unknown.status(), StatusCode::OK);
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

#[tokio::test]
async fn a_client_outside_this_machine_and_its_tailnet_reads_nothing() {
    let machine = Machine::new();
    for path in ["/", "/beta/theme.css", "/docs/README.md"] {
        let response = machine.from("192.0.2.7:1", path, &[]).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
    }
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
