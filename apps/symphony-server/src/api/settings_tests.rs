//! The Settings page's routes: no answer before a hub, the project's settings through one, each
//! with its source, and its one write (`SETTING_WRITE_PATH`), a POST held to the Host and Origin
//! checks every write is, its body never read when they refuse it.

use crate::testing::Tree;
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, StatusCode, header};
use std::net::SocketAddr;
use std::sync::Arc;
use symphony_api::{SETTING_WRITE_PATH, SETTINGS_PATH, SettingsPage};
use tower::ServiceExt;

const CHILD: &str = "SYMPHONY_SETTINGS_READ_CHILD";
const TEST: &str =
    "api::settings_tests::the_settings_wait_for_a_hub_then_answer_the_project_s_with_each_source";

/// Run alone on a private board whose store holds `[limits] low_volume = 3`.
#[test]
fn the_settings_wait_for_a_hub_then_answer_the_project_s_with_each_source() {
    if std::env::var_os(CHILD).is_none() {
        let board = crate::testing::board::database();
        // The board's command line makes the board; the Symphony, a reader, never does.
        let store = crate::testing::store_on(&board);
        store
            .store_setting(
                "limits.low_volume",
                &symphony_board::settings::written(&serde_json::json!(3)).unwrap(),
                "owner",
            )
            .unwrap();
        crate::testing::child_on(&board, CHILD, TEST);
        return;
    }
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(read_on_a_board());
}

async fn read_on_a_board() {
    let tree = Tree::new();
    let root = crate::testing::repository(&tree);
    std::fs::write(
        root.join("symphony.toml"),
        "[agents]\npanes = \"hc\"\n\n[limits]\nlow_volume = 2\n",
    )
    .unwrap();
    let shared = crate::front::Shared::new(
        1,
        1234,
        &root,
        None,
        crate::gate::Gate::new(Arc::new(|_| Ok(None)), Arc::new(std::time::Instant::now)),
    );
    let ask = || {
        let shared = shared.clone();
        let request = axum::http::Request::builder()
            .uri(SETTINGS_PATH)
            .header("Host", "localhost:1234")
            .body(())
            .unwrap();
        async move {
            let response = crate::api::answer(&shared, request).await.unwrap();
            let status = response.status();
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            (
                status,
                serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
            )
        }
    };
    let (status, _) = ask().await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "no hub yet");
    let hub = crate::live::Hub::new(&root, &root.join("build/client"));
    assert!(shared.api.set(Arc::new(hub)).is_ok());
    let (status, said) = ask().await;
    assert_eq!(status, StatusCode::OK, "{said}");
    let page: SettingsPage = serde_json::from_value(said).unwrap();
    assert_eq!(page.problems, Vec::<String>::new());
    let sources: Vec<(&str, &str, &str, Option<&str>)> = page
        .sections
        .iter()
        .flat_map(|section| &section.entries)
        .flat_map(|entry| &entry.settings)
        .filter(|setting| ["[agents] panes", "[limits] low_volume"].contains(&&*setting.key))
        .map(|setting| {
            let source = &setting.source;
            (
                &*setting.key,
                &*setting.value,
                &*source.said,
                source.over.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        sources,
        [
            (
                "[agents] panes",
                "hc",
                "symphony.toml [agents] panes, line 2",
                None
            ),
            (
                "[limits] low_volume",
                "3",
                "the board's store [limits] low_volume",
                Some("2 (symphony.toml [limits] low_volume, line 5)")
            ),
        ]
    );
    // The page lists what it writes, the board's entry with it.
    let low = page
        .stored
        .iter()
        .find(|item| item.key == "limits.low_volume")
        .unwrap();
    assert_eq!((low.value.as_deref(), low.held), (Some("3"), true));
}

#[test]
fn the_settings_are_a_read_and_their_write_a_post_of_its_own() {
    assert!(crate::api::READS.contains(&SETTINGS_PATH));
    assert!(!crate::api::READS.contains(&SETTING_WRITE_PATH));
    for path in [SETTINGS_PATH, "/settings"] {
        assert_eq!(crate::write::methods(path), None, "{path}");
    }
    assert_eq!(crate::write::methods(SETTING_WRITE_PATH), Some("POST"));
}

#[tokio::test]
async fn a_settings_write_from_another_host_or_origin_is_refused_and_writes_nothing() {
    let tree = Tree::checkout();
    let shared = crate::front::Shared::new(1, 8123, &tree.root, None, crate::testing::open_gate());
    let app = crate::front::router(shared.clone()).layer(MockConnectInfo(
        "127.0.0.1:5000".parse::<SocketAddr>().unwrap(),
    ));
    for (host, origin) in [
        ("foreign.example", Some("http://foreign.example")),
        ("localhost:8123", Some("http://foreign.example")),
        ("localhost:8123", Some("null")),
        ("foreign.example", None),
    ] {
        let mut request = Request::post(SETTING_WRITE_PATH).header(header::HOST, host);
        if let Some(origin) = origin {
            request = request.header(header::ORIGIN, origin);
        }
        let body = r#"{"key":"limits.low_volume","action":"store","value":1}"#;
        let response = app
            .clone()
            .oneshot(request.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "{host} {origin:?}"
        );
    }
    // No save store was opened, so the board's store was never asked.
    assert!(shared.saves.get().is_none());
    assert_eq!(tree.names(""), [".git"]);
}
