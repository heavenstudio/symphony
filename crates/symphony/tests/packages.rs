//! Which workspace members go to crates.io, and that each one that does carries what crates.io shows
//! and needs (docs/RELEASES.md, "Packages on crates.io"). A member is either published, with all of
//! it, or `publish = false`: [`PUBLISHED`] is the one place that says which, so a member published
//! by accident, or kept back by one, fails here. Read from `cargo metadata`, which resolves what a
//! member inherits from the workspace as `cargo publish` does, and from each manifest's own
//! `include`, which the metadata leaves out.

use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The members crates.io gets, by package name: the plugin kit and the helpers.
const PUBLISHED: [&str; 8] = [
    "process-cwd",
    "repository-read",
    "symphony-api",
    "symphony-args",
    "symphony-docs",
    "symphony-env",
    "symphony-plugin",
    "symphony-schema",
];

/// The plugin kit, whose three packages carry one version.
const KIT: [&str; 3] = ["symphony-api", "symphony-docs", "symphony-plugin"];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The workspace's members as `cargo metadata` reads them, without resolving their dependencies
/// (`--no-deps`, so nothing is fetched): read once for every test here.
fn members() -> &'static [Value] {
    static MEMBERS: OnceLock<Vec<Value>> = OnceLock::new();
    MEMBERS.get_or_init(read_members)
}

fn read_members() -> Vec<Value> {
    let out = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
        ])
        .current_dir(workspace())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let metadata: Value = serde_json::from_slice(&out.stdout).unwrap();
    metadata["packages"].as_array().unwrap().clone()
}

fn name(package: &Value) -> &str {
    package["name"].as_str().unwrap()
}

fn folder(package: &Value) -> PathBuf {
    Path::new(package["manifest_path"].as_str().unwrap())
        .parent()
        .unwrap()
        .to_path_buf()
}

/// `publish = true` reads as no list (any registry), `publish = false` as an empty one; a list of
/// registries is neither, and no member names one.
fn published(package: &Value) -> bool {
    match &package["publish"] {
        Value::Null => true,
        Value::Array(registries) if registries.is_empty() => false,
        other => panic!(
            "{}: `publish = {other}` names registries; a member is `publish = true` or `false`",
            name(package)
        ),
    }
}

fn text<'a>(package: &'a Value, field: &str) -> &'a str {
    package[field].as_str().unwrap_or_default()
}

/// What a published package's manifest lacks of what crates.io shows and needs, each as a line.
fn lacks(package: &Value, licence: &[u8]) -> Vec<String> {
    let mut lacks = Vec::new();
    let here = folder(package);
    for field in ["description", "repository"] {
        if text(package, field).trim().is_empty() {
            lacks.push(format!("no `{field}`"));
        }
    }
    if text(package, "license") != "MIT" {
        lacks.push(format!(
            "`license` is {:?}, not \"MIT\"",
            text(package, "license")
        ));
    }
    match package["readme"].as_str() {
        Some(readme) if here.join(readme).is_file() => {}
        Some(readme) => lacks.push(format!(
            "its `readme` {readme} is not a file in the package"
        )),
        None => lacks.push("no `readme`".to_owned()),
    }
    if std::fs::read(here.join("LICENSE")).ok().as_deref() != Some(licence) {
        lacks.push("no LICENSE in the package holding the workspace's".to_owned());
    }
    let keywords: Vec<&str> = package["keywords"]
        .as_array()
        .map(|words| words.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    for keyword in keywords.iter().filter(|keyword| !keyword_shape(keyword)) {
        lacks.push(format!(
            "the keyword {keyword:?} is not one crates.io takes: at most 20 characters, ASCII \
             letters, digits, `_`, `-` or `+`, a letter first"
        ));
    }
    let keywords = keywords.len();
    if !(1..=5).contains(&keywords) {
        lacks.push(format!(
            "{keywords} `keywords`, where crates.io takes 1 to 5"
        ));
    }
    if package["categories"].as_array().is_none_or(Vec::is_empty) {
        lacks.push("no `categories`".to_owned());
    }
    let manifest: toml::Table =
        toml::from_str(&std::fs::read_to_string(here.join("Cargo.toml")).unwrap()).unwrap();
    let include: Vec<&str> = manifest["package"]
        .get("include")
        .and_then(toml::Value::as_array)
        .map(|patterns| patterns.iter().filter_map(toml::Value::as_str).collect())
        .unwrap_or_default();
    if include.is_empty() {
        lacks.push("no `include`, so its tests and fixtures would be packaged".to_owned());
    }
    for needed in ["README.md", "LICENSE"] {
        if !include.is_empty() && !include.contains(&needed) {
            lacks.push(format!("its `include` leaves out {needed}"));
        }
    }
    lacks
}

/// A keyword as crates.io takes one: at most 20 characters, ASCII letters, digits, `_`, `-` or `+`,
/// and a letter first.
fn keyword_shape(keyword: &str) -> bool {
    keyword.len() <= 20
        && keyword
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic())
        && keyword
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+'))
}

/// What a published package's dependencies would make crates.io refuse, each as a line: a git
/// source; a workspace member it builds with that is not published, or that it names with no
/// version or one the member is not at; a development dependency on an unpublished member named
/// with a version (one named by `path` alone is dropped from the published manifest).
fn refused(package: &Value, versions: &[(String, String, bool)]) -> Vec<String> {
    let mut refused = Vec::new();
    for dependency in package["dependencies"].as_array().unwrap() {
        let dep = dependency["name"].as_str().unwrap();
        if dependency["source"]
            .as_str()
            .is_some_and(|source| source.starts_with("git+"))
        {
            refused.push(format!("{dep} comes from git"));
        }
        if dependency["path"].is_null() {
            continue;
        }
        let Some((_, version, sibling_published)) =
            versions.iter().find(|(member, ..)| member == dep)
        else {
            refused.push(format!("{dep} is a path outside the workspace"));
            continue;
        };
        let req = dependency["req"].as_str().unwrap();
        let development = dependency["kind"].as_str() == Some("dev");
        if development {
            if !sibling_published && req != "*" {
                refused.push(format!(
                    "the development dependency {dep} is unpublished yet named with `version`"
                ));
            }
        } else if !sibling_published {
            refused.push(format!("{dep} is not published"));
        } else if req != format!("^{version}") {
            refused.push(format!(
                "{dep} is required as {req:?}, where its version is {version}: name it \
                 `{{ path = …, version = \"{version}\" }}`"
            ));
        }
    }
    refused
}

#[test]
fn every_member_is_published_whole_or_kept_back() {
    let members = members();
    let licence = std::fs::read(workspace().join("LICENSE")).unwrap();
    let names: BTreeSet<&str> = members.iter().map(name).collect();
    let publishing: BTreeSet<&str> = members
        .iter()
        .filter(|package| published(package))
        .map(name)
        .collect();
    let listed: BTreeSet<&str> = PUBLISHED.into_iter().collect();
    assert!(
        listed.is_subset(&names),
        "PUBLISHED names {:?}, which no member is",
        listed.difference(&names).collect::<Vec<_>>()
    );
    assert_eq!(
        publishing, listed,
        "the members that say `publish = true` are not PUBLISHED: publishing a member is an edit \
         of that list, and every other member keeps the workspace's `publish = false`"
    );
    let versions: Vec<(String, String, bool)> = members
        .iter()
        .map(|package| {
            (
                name(package).to_owned(),
                text(package, "version").to_owned(),
                published(package),
            )
        })
        .collect();
    let mut wrong = Vec::new();
    for package in members.iter().filter(|package| published(package)) {
        for line in lacks(package, &licence)
            .into_iter()
            .chain(refused(package, &versions))
        {
            wrong.push(format!("{}: {line}", name(package)));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn the_kits_packages_carry_one_version() {
    let members = members();
    let kit: BTreeSet<(&str, &str)> = members
        .iter()
        .filter(|package| KIT.contains(&name(package)))
        .map(|package| (name(package), text(package, "version")))
        .collect();
    let versions: BTreeSet<&str> = kit.iter().map(|(_, version)| *version).collect();
    assert_eq!(kit.len(), KIT.len(), "a kit package is missing: {kit:?}");
    assert_eq!(
        versions.len(),
        1,
        "the kit's packages differ in version: {kit:?}"
    );
}

#[test]
fn a_keyword_crates_io_refuses_is_told_by_its_shape() {
    for taken in [
        "symphony",
        "json-schema",
        "os_unix",
        "c++",
        "a",
        "a2345678901234567890",
    ] {
        assert!(
            keyword_shape(taken),
            "{taken:?} is a keyword crates.io takes"
        );
    }
    for refused in [
        "",
        "1password",
        "-cli",
        "über",
        "two words",
        "a23456789012345678901",
    ] {
        assert!(
            !keyword_shape(refused),
            "{refused:?} is a keyword crates.io refuses"
        );
    }
}
