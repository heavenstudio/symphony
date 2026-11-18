//! The adapter is a program of its own: no member of the workspace builds with it, so the
//! board, the library and the server carry none of it and a project that never writes its
//! settings runs none of it; and it builds with no member itself, reaching the board through
//! `tasks api` alone. Read from `cargo metadata`, which resolves each manifest as a build does.

#![expect(
    clippy::unwrap_used,
    reason = "the helper beside the test asserts as it does: an unwrap is the failure"
)]

use serde_json::Value;
use std::process::Command;

fn dependencies(package: &Value, dev: bool) -> Vec<&str> {
    package["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|dependency| (dependency["kind"] == "dev") == dev)
        .filter_map(|dependency| dependency["name"].as_str())
        .collect()
}

#[test]
fn no_member_builds_with_the_adapter_and_it_builds_with_none() {
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml");
    let ran = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
            manifest,
        ])
        .output()
        .unwrap();
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let metadata: Value = serde_json::from_slice(&ran.stdout).unwrap();
    let packages = metadata["packages"].as_array().unwrap();
    let members: Vec<&str> = packages.iter().filter_map(|p| p["name"].as_str()).collect();
    assert!(
        members.contains(&"symphony-board") && members.contains(&"symphony-server"),
        "{members:?}"
    );

    for package in packages {
        let name = package["name"].as_str().unwrap();
        for dev in [false, true] {
            assert!(
                !dependencies(package, dev).contains(&"symphony-github"),
                "{name} builds with the adapter"
            );
        }
    }
    let adapter = packages
        .iter()
        .find(|p| p["name"] == "symphony-github")
        .expect("the adapter");
    let linked: Vec<&str> = dependencies(adapter, false)
        .into_iter()
        .filter(|name| members.contains(name))
        .collect();
    assert!(
        linked.is_empty(),
        "the adapter links {linked:?}: it reaches the board by its api"
    );
    assert_eq!(
        adapter["publish"],
        serde_json::json!([]),
        "it is not published"
    );
}
