use super::*;
use serde_json::json;

const CHUNK: &str = "/assets/shared-0123456789abcdef.js";
const OTHER: &str = "/assets/other-fedcba9876543210.js";

fn source(value: serde_json::Value) -> Vec<u8> {
    format!("const modules = {value};\n// the rest of the shell").into_bytes()
}

#[test]
fn only_the_selected_kinds_imports_are_preloaded_once_in_manifest_order() {
    let script = source(json!({
        "board": {"script": OTHER, "imports": [CHUNK, OTHER, CHUNK], "styles": ["/style.css"]},
        "panel": {"imports": ["/assets/panel-1111111111111111.js"]}
    }));
    assert_eq!(
        links(&script, "board"),
        Some(format!(
            "<link rel=\"modulepreload\" href=\"{CHUNK}\"><link rel=\"modulepreload\" href=\"{OTHER}\">"
        ))
    );
    assert_eq!(links(&script, "missing"), None);
    assert_eq!(
        links(&source(json!({"board": {"imports": []}})), "board"),
        Some(String::new())
    );
}

#[test]
fn an_older_or_unreadable_manifest_leaves_loading_to_the_shell() {
    for script in [
        b"".as_slice(),
        b"\xff",
        b"const modules = nope;\n",
        b"const modules = {};",
        b"// old shell\nconst modules = {};",
        b"const modules = {}",
        b"const modules = [];",
    ] {
        assert_eq!(links(script, "board"), None, "{script:?}");
    }
    for value in [
        json!({"board": {"script": OTHER}}),
        json!({"board": {"imports": null}}),
        json!({"board": {"imports": [CHUNK, 1]}}),
    ] {
        assert_eq!(links(&source(value), "board"), None);
    }
}

#[test]
fn only_local_immutable_javascript_paths_can_be_preloaded() {
    for href in [
        "https://example.test/file.js",
        "//example.test/file.js",
        "assets/shared-0123456789abcdef.js",
        "/assets/shared.js",
        "/assets/shared-0123456789abcdef.css",
        "/assets/../shared-0123456789abcdef.js",
        "/assets/shared-0123456789abcdef.js?x=1",
        "/assets/x\"-0123456789abcdef.js",
        "/other/shared-0123456789abcdef.js",
    ] {
        assert_eq!(
            links(
                &source(json!({"board": {"imports": [CHUNK, href]}})),
                "board"
            ),
            None,
            "{href}"
        );
    }
}

#[tokio::test]
async fn embedded_and_project_scoped_pages_use_the_same_root_asset_urls() {
    let tree = crate::testing::Tree::new();
    let root = tree.path("repo");
    std::fs::create_dir(&root).unwrap();
    let mut shared =
        crate::front::Shared::project(1, 1, &root, None, crate::testing::open_gate(), "/alpha");
    // This case exercises the embedded bundle, including when the release run names its
    // freshly built directory through SYMPHONY_UI_DIST. Keep that process-wide setting intact.
    std::sync::Arc::get_mut(&mut shared).unwrap().embedded_ui = true;
    assert!(shared.embedded_ui);
    let mut html = std::str::from_utf8(crate::client::asset("index.html").unwrap())
        .unwrap()
        .to_string();
    let original = html.clone();
    let script = crate::client::script(&html).trim_start_matches('/');
    let expected = links(crate::client::asset(script).unwrap(), "board").unwrap();
    assert!(!expected.is_empty());
    page(&shared, "board", &mut html).await;
    assert_eq!(
        html,
        original.replace("</head>", &format!("{expected}</head>"))
    );
    assert!(!html.contains("href=\"/alpha/assets/"));
    let mut unknown = original.clone();
    page(&shared, "missing", &mut unknown).await;
    assert_eq!(unknown, original);
}
