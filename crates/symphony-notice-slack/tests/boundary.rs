//! The plugin is a program of its own, apart from the core: no member of the workspace builds
//! with it, so the board, the library and the server carry none of it; it builds with the notice
//! message alone of the members; and it is not published. Read from `cargo metadata`, which
//! resolves each manifest as a build does.

#![expect(
    clippy::unwrap_used,
    reason = "the helper beside the test asserts as it does: an unwrap is the failure"
)]

use serde_json::{Value, json};
use std::process::Command;

const PLUGIN: &str = "symphony-notice-slack";

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
fn no_member_builds_with_the_plugin_and_it_builds_with_the_message_alone() {
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
                !dependencies(package, dev).contains(&PLUGIN),
                "{name} builds with the Slack plugin"
            );
        }
    }
    let plugin = packages
        .iter()
        .find(|p| p["name"] == PLUGIN)
        .expect("the plugin");
    let linked: Vec<&str> = [false, true]
        .into_iter()
        .flat_map(|dev| dependencies(plugin, dev))
        .filter(|name| members.contains(name))
        .collect();
    assert_eq!(linked, ["symphony-notice"], "the members it builds with");
    assert_eq!(plugin["publish"], json!([]), "it is not published");
}
