//! The plugin is apart from the core: no member of the workspace builds with it, so neither the
//! server, the board nor the library carries any of it, and it builds with no member but the
//! notice message's. Read from `cargo metadata`, which resolves each manifest as a build does.

use serde_json::Value;
use std::process::Command;

const PLUGIN: &str = "symphony-notice-whatsapp";

fn dependencies(package: &Value, dev: bool) -> Vec<&str> {
    (package["dependencies"]
        .as_array()
        .expect("its dependencies"))
    .iter()
    .filter(|dependency| (dependency["kind"] == "dev") == dev)
    .filter_map(|dependency| dependency["name"].as_str())
    .collect()
}

#[test]
fn no_member_builds_with_the_plugin_and_it_builds_with_the_notice_message_alone() {
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
        .expect("cargo metadata runs");
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let metadata: Value = serde_json::from_slice(&ran.stdout).expect("its JSON");
    let packages = metadata["packages"].as_array().expect("the packages");
    let members: Vec<&str> = packages.iter().filter_map(|p| p["name"].as_str()).collect();
    assert!(
        members.contains(&"symphony-server") && members.contains(&"symphony-board"),
        "{members:?}"
    );

    for package in packages {
        let name = package["name"].as_str().expect("its name");
        for dev in [false, true] {
            assert!(
                !dependencies(package, dev).contains(&PLUGIN),
                "{name} builds with the plugin"
            );
        }
    }
    let plugin = (packages.iter())
        .find(|p| p["name"] == PLUGIN)
        .expect("the plugin");
    let linked: Vec<&str> = dependencies(plugin, false)
        .into_iter()
        .filter(|name| members.contains(name))
        .collect();
    assert_eq!(linked, ["symphony-notice"], "the plugin links the core");
    assert_eq!(
        plugin["publish"],
        serde_json::json!([]),
        "it is not published"
    );
}
