use crate::front::Shared;
use crate::testing::Tree;
use axum::http::{Request, StatusCode, header};
use std::path::Path;
use std::sync::Arc;

#[test]
fn selection_keeps_install_override_and_local_build_failures_authoritative() {
    let tree = Tree::new();
    let root = tree.path("repo");
    let installed = tree.path("install");
    let missing = tree.path("override");
    assert_eq!(
        super::select(&root, Some(&installed), Some(missing.clone())),
        (installed.join("client/public"), false)
    );
    assert_eq!(
        super::select(&root, None, Some(missing.clone())),
        (missing, false)
    );
    assert_eq!(
        super::select(&root, None, None),
        (root.join("build/symphony-ui/public"), true)
    );
    std::fs::create_dir_all(root.join("build/symphony-ui")).unwrap();
    std::os::unix::fs::symlink(
        tree.path("not-there"),
        root.join("build/symphony-ui/public"),
    )
    .unwrap();
    assert_eq!(
        super::select(&root, None, None),
        (root.join("build/symphony-ui/public"), false)
    );
    std::fs::remove_file(root.join("build/symphony-ui/public")).unwrap();
    tree.write(
        "repo/build/symphony-ui/public/not-an-index",
        "incomplete local build",
    );
    assert_eq!(
        super::select(&root, None, None),
        (root.join("build/symphony-ui/public"), false)
    );
}

/// The names an ES module's last `export{…}` clause gives what it exports, as a minifier writes it
/// (`export{a as run,b as stop}`); none for a module without one.
fn module_exports(module: &str) -> Vec<&str> {
    let Some((_, clause)) = module.rsplit_once("export{") else {
        return Vec::new();
    };
    let clause = clause.split('}').next().unwrap_or_default();
    clause
        .split(',')
        .map(|item| item.rsplit(" as ").next().unwrap_or(item).trim())
        .filter(|name| !name.is_empty())
        .collect()
}

#[test]
fn a_modules_exports_are_read_from_its_clause() {
    assert_eq!(
        module_exports("let a;export{tr as run,nr as stop};"),
        ["run", "stop"]
    );
    assert_eq!(module_exports("export{run, stop}"), ["run", "stop"]);
    assert_eq!(module_exports("export{}"), Vec::<&str>::new());
    assert_eq!(module_exports("globalThis.x=1"), Vec::<&str>::new());
}

/// The shipped distribution is the regenerator's, made after a land from main's sources (a branch
/// commits none), so it may be older than this tree: every kind it names is a script of the web
/// client (apps/symphony-web) that exports its `run` and `stop`, which the shell runs and stops.
#[test]
fn shipped_shell_references_its_real_modules() {
    let shell =
        std::str::from_utf8(super::asset("index.html").expect("shipped release index")).unwrap();
    let script = shell
        .split("src=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    assert!(script.starts_with("/assets/symphony-shell-"));
    let js = std::str::from_utf8(super::asset(script.trim_start_matches('/')).unwrap()).unwrap();
    let manifest = js
        .split("const modules = ")
        .nth(1)
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let manifest: serde_json::Value = serde_json::from_str(manifest).unwrap();
    let mut keys: Vec<_> = manifest
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    symphony_board::order::sort_by(&mut keys, &Ord::cmp);
    // The kinds the committed build and the one regenerated from this tree both have.
    for kind in ["board", "docs", "overlay", "panel", "queue", "review"] {
        assert!(keys.contains(&kind), "no {kind} in {keys:?}");
    }
    for (kind, module) in manifest.as_object().unwrap() {
        assert!(module.get("wasm").is_none(), "{kind} names a wasm");
        let path = module["script"].as_str().unwrap().trim_start_matches('/');
        let exported = module_exports(std::str::from_utf8(super::asset(path).unwrap()).unwrap());
        assert!(
            exported.contains(&"run") && exported.contains(&"stop"),
            "{path} exports {exported:?}"
        );
        for style in module["styles"].as_array().unwrap() {
            assert!(
                !super::asset(style.as_str().unwrap().trim_start_matches('/'))
                    .unwrap()
                    .is_empty()
            );
        }
    }
    assert!(super::asset("assets/missing.js").is_none());
    assert!(super::asset("../Cargo.toml").is_none());
}

#[test]
fn installed_consumer_uses_its_pinned_release_even_with_a_stale_external_client() {
    let tree = Tree::new();
    tree.write("project/apps/symphony-host/Cargo.toml", "[package]\n");
    tree.write("install/client/public/index.html", "old unrelated release");
    let root = tree.path("project");
    let install = tree.path("install");
    tree.write(
        "project/build/symphony-ui/public/index.html",
        "old local release",
    );
    assert_eq!(
        super::select(&root, Some(&install), Some(tree.path("override"))),
        (install.join("client/public"), true)
    );
    assert_eq!(
        super::select(&root, None, None),
        (root.join("build/symphony-ui/public"), true)
    );
    let preview = tree.path("explicit-preview");
    assert_eq!(
        super::select(&root, None, Some(preview.clone())),
        (preview, false)
    );
    // A source kit still owns its explicitly installed bundle and its errors.
    tree.write(
        "project/symphony/apps/symphony-server/Cargo.toml",
        "[package]\n",
    );
    assert_eq!(
        super::select(&root, Some(&install), None),
        (install.join("client/public"), false)
    );
    assert_eq!(
        super::select(&root, None, None),
        (root.join("build/symphony-ui/public"), false)
    );
}

fn shared(root: &Path, directory: &Path, embedded: bool, installed: bool) -> Arc<Shared> {
    let mut shared = Shared::new(1, 1234, root, None, crate::testing::open_gate());
    let writable = Arc::get_mut(&mut shared).unwrap();
    writable.ui_dist = directory.into();
    writable.embedded_ui = embedded;
    writable.installed_ui = installed;
    shared
}

async fn read(shared: &Arc<Shared>, path: &str, etag: Option<&str>) -> axum::response::Response {
    let mut request = Request::builder()
        .uri(path)
        .header("Host", "localhost:1234");
    if let Some(etag) = etag {
        request = request.header(header::IF_NONE_MATCH, etag);
    }
    crate::api::answer(shared, request.body(()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn embedded_assets_keep_theme_cache_types_and_missing_file_refusals() {
    let tree = Tree::new();
    let root = tree.path("project");
    tree.write(
        "project/symphony.toml",
        "[project]\nid = 'local'\nname = 'Local independent client'\n",
    );
    let shared = shared(&root, &tree.path("absent"), true, false);
    let page = read(&shared, "/", None).await;
    assert_eq!(page.status(), StatusCode::OK);
    assert_eq!(page.headers()[header::CACHE_CONTROL], "no-cache");
    let html = axum::body::to_bytes(page.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = std::str::from_utf8(&html).unwrap();
    assert!(
        html.contains("<title>Local independent client</title>"),
        "{html}"
    );
    let script = html
        .split("src=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    let asset = read(&shared, script, None).await;
    assert_eq!(asset.status(), StatusCode::OK);
    assert_eq!(
        asset.headers()[header::CONTENT_TYPE],
        "text/javascript; charset=utf-8"
    );
    let etag = asset.headers()[header::ETAG].to_str().unwrap().to_owned();
    let bytes = axum::body::to_bytes(asset.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(
        &bytes[..],
        super::asset(script.trim_start_matches('/')).unwrap()
    );
    let cached = read(&shared, script, Some(&etag)).await;
    assert_eq!(cached.status(), StatusCode::NOT_MODIFIED);
    assert!(
        axum::body::to_bytes(cached.into_body(), 0)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        read(&shared, "/assets/missing.js", None).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        read(&shared, "/assets/../Cargo.toml", None).await.status(),
        StatusCode::NOT_FOUND
    );
    assert!(
        crate::client::build(&tree.path("absent"), true).starts_with("/assets/symphony-shell-")
    );
}

#[tokio::test]
async fn explicit_bundles_never_fall_back_and_containment_still_refuses_links() {
    let tree = Tree::new();
    let root = tree.path("project");
    for installed in [false, true] {
        let shared = shared(&root, &tree.path("missing"), false, installed);
        let response = read(&shared, "/", None).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = axum::body::to_bytes(response.into_body(), 1000)
            .await
            .unwrap();
        let error: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let expected = if installed {
            "the Symphony client has not been built: run just symphony --stop, then just symphony from the main checkout"
        } else {
            "the Symphony client has not been built: run just symphony-ui-dist"
        };
        assert_eq!(error["error"], expected);
    }
    let directory = tree.path("bundle");
    tree.write(
        "bundle/index.html",
        "<html><head></head><body>local</body></html>",
    );
    tree.write("outside.js", "must not escape");
    std::fs::create_dir_all(directory.join("assets")).unwrap();
    std::os::unix::fs::symlink(tree.path("outside.js"), directory.join("assets/escape.js"))
        .unwrap();
    tree.write("bundle/data.json", r#"{"owned":true}"#);
    std::os::unix::fs::symlink(
        directory.join("data.json"),
        directory.join("assets/alias.js"),
    )
    .unwrap();
    let shared = shared(&root, &directory, false, true);
    let alias = read(&shared, "/assets/alias.js", None).await;
    assert_eq!(alias.status(), StatusCode::OK);
    assert_eq!(alias.headers()[header::CONTENT_TYPE], "application/json");
    let response = read(&shared, "/", None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 10000)
        .await
        .unwrap();
    assert!(std::str::from_utf8(&bytes).unwrap().contains("local"));
    assert_eq!(
        read(&shared, "/assets/escape.js", None).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        read(&shared, "/assets/absent.js", None).await.status(),
        StatusCode::NOT_FOUND
    );
}
