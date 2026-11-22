//! Native JSON routes over a private board, with foreign machine evidence in the hub.
//! This module is a child of the existing live read tests so no production fixture seam is needed.

use super::super::Hub;
use super::agent;
use crate::front::Shared;
use crate::hosted::tests::{assert_no_diagnostics, diagnostic_work, native_host, request};
use crate::testing::{
    Tree,
    board::{Fixture, Seed},
    repository, store_on,
};
use axum::body::{Body, to_bytes};
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, StatusCode, header};
use std::sync::Arc;
use tower::ServiceExt;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_hosted_project_models_withhold_machine_inventory_and_keep_project_data() {
    let tree = Tree::new();
    let root = repository(&tree);
    let board = Fixture::new(
        Seed::default()
            .session("worker")
            .task("tools/visible", "in_progress")
            .role("tools/visible", "worker", "executor"),
    );
    let mut hub = Hub::new(&root, &root);
    let client = board.other_client();
    hub.connect_reader = Box::new(move |_| Ok(store_on(&client)));
    hub.refresh_page("", &root).unwrap();
    let mut visible = agent("worker");
    visible.task = Some("tools/visible".into());
    visible.role = Some("executor".into());
    let agents = symphony_api::Agents {
        machine_work: diagnostic_work(),
        generated: board.store.now(),
        probes: symphony_api::AgentProbes {
            panes: "inventory-reader-secret".into(),
            slots: "/private/inventory-reader".into(),
        },
        agents: vec![visible],
    };
    hub.update_board(&board.store, board.store.board().unwrap(), agents)
        .unwrap();
    let shared = Shared::new(1, 1, &root, None, crate::testing::open_gate());
    assert!(shared.api.set(Arc::new(hub)).is_ok());
    let project = symphony::project::Project::of(&root).id.clone();
    let (config, host) = native_host(&project);
    let peer = MockConnectInfo("127.0.0.1:1234".parse::<std::net::SocketAddr>().unwrap());
    let hosted = crate::hosted::router(shared.clone(), config, host)
        .unwrap()
        .layer(peer);
    let local = crate::front::router(shared.clone()).layer(peer);

    for path in [
        "/api/agents",
        "/api/agent-page",
        "/api/task-board?activity=1",
        "/api/task-page?slug=tools/visible",
    ] {
        let response = hosted.clone().oneshot(request(path)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "private, no-store"
        );
        let hosted_etag = response.headers()[header::ETAG].clone();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert_no_diagnostics(text);
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        match path {
            "/api/agents" => {
                let view: symphony_api::Agents = serde_json::from_value(value).unwrap();
                assert_eq!(view.agents[0].handle, "worker");
                assert_eq!(view.machine_work, symphony_api::MachineWork::default());
                assert_eq!(view.probes.panes, "unavailable");
                assert_eq!(view.probes.slots, "unavailable");
            }
            "/api/agent-page" => {
                let page: symphony_api::AgentPage = serde_json::from_value(value).unwrap();
                assert_eq!(page.view.agents[0].handle, "worker");
                assert_eq!(page.tasks[0].slug, "tools/visible");
                assert_eq!(page.view.machine_work, symphony_api::MachineWork::default());
                assert_eq!(page.view.probes.panes, "unavailable");
                assert_eq!(page.view.probes.slots, "unavailable");
            }
            "/api/task-board?activity=1" => {
                let listing: symphony_api::TaskListing = serde_json::from_value(value).unwrap();
                assert_eq!(listing.tasks[0].slug, "tools/visible");
                assert_eq!(listing.machine_work, symphony_api::MachineWork::default());
                let counts: std::collections::BTreeMap<_, _> = listing.counts.into_iter().collect();
                assert_eq!(
                    counts.get("build").copied().unwrap_or(0),
                    0,
                    "foreign work cannot change project task stages"
                );
            }
            _ => {
                let page: symphony_api::TaskPage = serde_json::from_value(value).unwrap();
                assert_eq!(page.task.slug, "tools/visible");
                assert_eq!(page.machine_work, symphony_api::MachineWork::default());
                assert!(page.sessions.is_empty());
                assert!(!page.local);
            }
        }

        // The same native route still supplies its original machine evidence to local callers.
        let request = Request::builder()
            .uri(path)
            .header("host", "localhost")
            .body(Body::empty())
            .unwrap();
        let response = local.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let local_etag = response.headers()[header::ETAG].clone();
        assert_ne!(hosted_etag, local_etag);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(
            text.contains("foreign-machine-id"),
            "local behavior changed: {path}"
        );
        assert!(
            text.contains("inventory-reader-secret"),
            "local diagnostics changed: {path}"
        );
        for private in crate::hosted::tests::PRIVATE_DIAGNOSTICS {
            assert!(
                text.contains(private),
                "native fixture must contain {private}: {path}"
            );
        }
        if path == "/api/task-board?activity=1" {
            let local: symphony_api::TaskListing = serde_json::from_slice(&bytes).unwrap();
            let counts: std::collections::BTreeMap<_, _> = local.counts.into_iter().collect();
            assert_eq!(
                counts.get("build"),
                Some(&1),
                "fixture demonstrates the foreign-stage collision"
            );
        }
        {
            let mut agents = shared.api.get().unwrap().agents.lock().unwrap();
            let agents = agents.as_mut().unwrap();
            agents.machine_work.machines[0].sequence += 1;
            agents.machine_work.machines[0].reason.push_str(" changed");
        }
        let mut cached = crate::hosted::tests::request(path);
        cached
            .headers_mut()
            .insert(header::IF_NONE_MATCH, hosted_etag);
        let cached = hosted.clone().oneshot(cached).await.unwrap();
        assert_eq!(
            cached.status(),
            StatusCode::NOT_MODIFIED,
            "machine diagnostics cannot influence a hosted JSON validator: {path}"
        );
        assert!(to_bytes(cached.into_body(), 0).await.unwrap().is_empty());
    }
    assert_eq!(
        hosted
            .oneshot(request("/api/machine-work"))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    shared.stop();
}
