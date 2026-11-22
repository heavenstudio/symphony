//! The Settings page's route: no answer before a hub, the project's settings through one, each
//! with its source, and no save route of its own, as the page writes nothing.

use crate::testing::Tree;
use axum::http::StatusCode;
use std::sync::Arc;
use symphony_api::{SETTINGS_PATH, SettingsPage};

#[tokio::test]
async fn the_settings_wait_for_a_hub_then_answer_the_project_s_with_each_source() {
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
    let sources: Vec<(&str, &str, &str)> = page
        .sections
        .iter()
        .flat_map(|section| &section.entries)
        .flat_map(|entry| &entry.settings)
        .filter(|setting| ["[agents] panes", "[limits] low_volume"].contains(&&*setting.key))
        .map(|setting| (&*setting.key, &*setting.value, &*setting.source.said))
        .collect();
    assert_eq!(
        sources,
        [
            (
                "[agents] panes",
                "hc",
                "symphony.toml [agents] panes, line 2"
            ),
            (
                "[limits] low_volume",
                "2",
                "symphony.toml [limits] low_volume, line 5"
            ),
        ]
    );
}

#[test]
fn the_settings_are_a_read_and_no_route_saves_one() {
    assert!(crate::api::READS.contains(&SETTINGS_PATH));
    for path in [SETTINGS_PATH, "/settings", "/api/setting"] {
        assert_eq!(crate::write::methods(path), None, "{path}");
    }
}
