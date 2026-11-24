//! Which workspace members go to crates.io, and that each one that does carries what crates.io shows
//! and needs (docs/workflow/RELEASES.md, "Packages on crates.io"). A member is either published, with all of
//! it, or `publish = false`: [`PUBLISHED`] is the one place that says which, so a member published
//! by accident, or kept back by one, fails here. Read from `cargo metadata`, which resolves what a
//! member inherits from the workspace as `cargo publish` does, and from each manifest's own
//! `include`, which the metadata leaves out.

#[path = "common/owner_words.rs"]
mod owner_words;
#[path = "common/tree.rs"]
mod tree;

use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use tree::Tree;

/// The members crates.io gets, by package name: the plugin kit, the helpers, and the product, the
/// app's wire types among it.
const PUBLISHED: [&str; 13] = [
    "process-cwd",
    "repository-read",
    "symphony-api",
    "symphony-app",
    "symphony-args",
    "symphony-blocks",
    "symphony-board",
    "symphony-chrome",
    "symphony-docs",
    "symphony-env",
    "symphony-plugin",
    "symphony-schema",
    "symphony-server",
];

/// The plugin kit, whose three packages carry one version.
const KIT: [&str; 3] = ["symphony-blocks", "symphony-docs", "symphony-plugin"];

/// The product, whose five packages carry one version and require each other at exactly it (`=`):
/// a version below 1.0 that promises no name, so no two of them are ever taken at different ones.
const PRODUCT: [&str; 5] = [
    "symphony-api",
    "symphony-app",
    "symphony-board",
    "symphony-chrome",
    "symphony-server",
];

/// The files a published package carries as a copy of one outside it, which it cannot read from
/// there once packaged, each beside the file it copies: the copy first. The client's stylesheets
/// are its package's, which the server's does not carry, and the board's fixtures find the source
/// root by the descriptor-closing script where it is, so neither can move. The ways a notice goes
/// by and their addresses' rules are the notice crate's, which the published board cannot link.
const COPIES: [(&str, &str); 4] = [
    (
        "apps/symphony-server/src/board.css",
        "apps/symphony-web/src/styles/board.css",
    ),
    (
        "apps/symphony-server/src/frame.css",
        "apps/symphony-web/src/styles/frame.css",
    ),
    (
        "crates/chrome-devtools/src/fixture_process.sh",
        "tools/fixture_process.sh",
    ),
    (
        "crates/tasks/src/people/ways.rs",
        "crates/symphony-notice/src/ways.rs",
    ),
];

/// The largest `.crate` crates.io takes: 10 MiB.
const UPLOAD_LIMIT: u64 = 10 * 1024 * 1024;

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

/// The helpers, each on a version line of its own (docs/workflow/RELEASES.md): with the [`KIT`], what a
/// project may link at the version it names.
const HELPERS: [&str; 5] = [
    "process-cwd",
    "repository-read",
    "symphony-args",
    "symphony-env",
    "symphony-schema",
];

/// The workspace members (`members`, by name) that `package`, the kit's or a helper, builds with
/// and its promise leaves out: a kit package may build with the kit alone, a helper with the
/// helpers alone. A type of any other member shown in its public items would move its version with
/// that member's, and the kit's major with the app's; its tests' dependencies (`dev`) are not
/// published, and so not counted. That no public item of the kit shows a helper's type is read in
/// review, as rustc's lint for it is unstable (docs/workflow/CHECKS.md).
fn leaked<'a>(package: &'a Value, members: &BTreeSet<&str>) -> Vec<&'a str> {
    let line: &[&str] = if KIT.contains(&name(package)) {
        &KIT
    } else {
        &HELPERS
    };
    package["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|dependency| dependency["kind"].as_str() != Some("dev"))
        .filter_map(|dependency| dependency["name"].as_str())
        .filter(|dep| members.contains(dep) && !line.contains(dep))
        .collect()
}

#[test]
fn the_kit_and_the_helpers_build_with_no_member_their_promise_leaves_out() {
    let names: BTreeSet<&str> = members().iter().map(name).collect();
    for promised in KIT.iter().chain(&HELPERS) {
        assert!(
            PUBLISHED.contains(promised) && names.contains(promised),
            "{promised} is promised, so a published member"
        );
    }
    let wrong: Vec<String> = members()
        .iter()
        .filter(|package| KIT.contains(&name(package)) || HELPERS.contains(&name(package)))
        .flat_map(|package| {
            leaked(package, &names)
                .into_iter()
                .map(|dep| format!("{} builds with {dep}", name(package)))
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    // As it reads them: a kit package on the library or the wire types, or a helper on the kit,
    // builds with what its promise leaves out; a helper on a helper, a crate from crates.io and a
    // test's own dependency do not.
    let package = |name: &str, dependencies: &[(&str, Option<&str>)]| {
        let dependencies: Vec<Value> = dependencies
            .iter()
            .map(|(dep, kind)| serde_json::json!({"name": dep, "kind": kind}))
            .collect();
        serde_json::json!({"name": name, "dependencies": dependencies})
    };
    let library = members()
        .iter()
        .find(|package| folder(package).ends_with("crates/symphony"))
        .map(name)
        .unwrap();
    let kit = package(
        "symphony-plugin",
        &[
            (library, None),
            ("symphony-api", None),
            ("symphony-blocks", None),
            ("serde", None),
        ],
    );
    assert_eq!(leaked(&kit, &names), [library, "symphony-api"]);
    let helper = package(
        "repository-read",
        &[
            ("symphony-docs", Some("build")),
            ("symphony-env", None),
            ("test-time", Some("dev")),
        ],
    );
    assert_eq!(leaked(&helper, &names), ["symphony-docs"]);
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
    } else if !std::fs::symlink_metadata(here.join("LICENSE")).is_ok_and(|meta| meta.is_file()) {
        lacks.push(
            "its LICENSE is a link, which a package cannot carry out of the workspace: a \
             copy of the workspace's"
                .to_owned(),
        );
    }
    let keywords: Vec<&str> = package["keywords"]
        .as_array()
        .map(|words| words.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    for keyword in keywords.iter().filter(|keyword| !keyword_shape(keyword)) {
        lacks.push(format!(
            "the keyword {keyword:?} is not one crates.io takes: at most 20 characters, ASCII \
             letters, digits, `_`, `-` or `+`, a letter or digit first"
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
    let readme = package["readme"].as_str().unwrap_or("README.md");
    for needed in [readme, "LICENSE"] {
        if !include.is_empty() && !include.contains(&needed) {
            lacks.push(format!("its `include` leaves out {needed}"));
        }
    }
    lacks
}

/// A keyword as crates.io takes one: at most 20 characters, ASCII letters, digits, `_`, `-` or `+`,
/// and a letter or digit first.
fn keyword_shape(keyword: &str) -> bool {
    keyword.len() <= 20
        && keyword
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphanumeric())
        && keyword
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+'))
}

/// What a published package's dependencies would make crates.io refuse, or the version lines
/// break, each as a line: a git source; a workspace member it builds with that is not published,
/// or that it names with no version or one the member is not at, or not as its line asks (exactly,
/// `=`, between two of the [`PRODUCT`]'s packages, else by caret); a development dependency on an
/// unpublished member named with a version (one named by `path` alone is dropped from the
/// published manifest).
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
        } else {
            let exact = PRODUCT.contains(&name(package)) && PRODUCT.contains(&dep);
            let (wanted, written) = if exact {
                (format!("={version}"), format!("={version}"))
            } else {
                (format!("^{version}"), version.clone())
            };
            if req != wanted {
                refused.push(format!(
                    "{dep} is required as {req:?}, where its version is {version}: name it \
                     `{{ path = …, version = \"{written}\" }}`"
                ));
            }
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

/// That the packages `line` names, all members, carry one version, `what` naming them in a failure.
fn one_version(line: &[&str], what: &str) {
    let packages: BTreeSet<(&str, &str)> = members()
        .iter()
        .filter(|package| line.contains(&name(package)))
        .map(|package| (name(package), text(package, "version")))
        .collect();
    let versions: BTreeSet<&str> = packages.iter().map(|(_, version)| *version).collect();
    assert_eq!(
        packages.len(),
        line.len(),
        "a package of {what} is missing: {packages:?}"
    );
    assert_eq!(
        versions.len(),
        1,
        "{what}'s packages differ in version: {packages:?}"
    );
}

#[test]
fn the_kits_packages_carry_one_version() {
    one_version(&KIT, "the kit");
}

#[test]
fn the_products_packages_carry_one_version() {
    one_version(&PRODUCT, "the product");
}

/// A copy a package carries in place of a file outside it is that file, byte for byte, and a file
/// of its own: a link would package the file it leads to, or nothing where links are not kept.
#[test]
fn every_copy_a_package_carries_is_the_file_it_copies() {
    let root = workspace();
    let mut wrong = Vec::new();
    for (copy, source) in COPIES {
        let regular = std::fs::symlink_metadata(root.join(copy)).is_ok_and(|meta| meta.is_file());
        let same =
            std::fs::read(root.join(copy)).ok() == Some(std::fs::read(root.join(source)).unwrap());
        if !regular || !same {
            wrong.push(format!(
                "{copy} is not a copy of {source}: `cp {source} {copy}`"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// No file a published package carries, its manifest's comments among them, gives a rule as the
/// owner's words, dated or quoted: each says the rule it sets, without the quote, the date or the
/// card ([`owner_words`]). What each package carries is what `cargo package --list` names.
#[test]
fn no_published_package_carries_the_owners_words() {
    let words = owner_words::pattern();
    let mut found = Vec::new();
    for package in members().iter().filter(|package| published(package)) {
        let out = Command::new(env!("CARGO"))
            .args([
                "package",
                "--list",
                "--allow-dirty",
                "--offline",
                "-p",
                name(package),
            ])
            .current_dir(workspace())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        for file in String::from_utf8_lossy(&out.stdout).lines() {
            let text = std::fs::read_to_string(folder(package).join(file)).unwrap_or_default();
            found.extend(
                text.lines()
                    .enumerate()
                    .filter(|(_, line)| words.is_match(line))
                    .map(|(at, line)| format!("{}: {file}:{}: {line}", name(package), at + 1)),
            );
        }
    }
    assert!(
        found.is_empty(),
        "the owner's words, dated or quoted, to say as the rule they set:\n{}",
        found.join("\n")
    );
}

/// The server's package carries the client it embeds, every build its folder keeps: the day that
/// grows past what crates.io takes is a red here, not an upload refused. Packaged by Cargo itself,
/// into a target folder of the test's own, building nothing (`--no-verify`) and resolving none of
/// its siblings (`--exclude-lockfile`), from the tree as it stands (`--allow-dirty`): a size, not
/// a publication.
#[test]
fn the_servers_package_is_one_crates_io_takes() {
    let target = Tree::new();
    let out = Command::new(env!("CARGO"))
        .args([
            "package",
            "--package",
            "symphony-server",
            "--no-verify",
            "--exclude-lockfile",
            "--allow-dirty",
            "--offline",
            "--quiet",
        ])
        .env("CARGO_TARGET_DIR", &target.root)
        .current_dir(workspace())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "cargo package failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let version = members()
        .iter()
        .find(|package| name(package) == "symphony-server")
        .map(|package| text(package, "version"))
        .unwrap();
    let crate_file = format!("symphony-server-{version}.crate");
    let size = std::fs::metadata(target.path(&format!("package/{crate_file}")))
        .unwrap()
        .len();
    assert!(
        size <= UPLOAD_LIMIT,
        "{crate_file} is {size} bytes, past the {UPLOAD_LIMIT} crates.io takes: the client builds \
         it carries (apps/symphony-server/client/public) are to be cut to the newest \
         (docs/workflow/RELEASES.md, \"Packages on crates.io\")"
    );
    // Every distributed client file, notices included, survives Cargo's include rules byte for
    // byte. The client build target separately proves each build carries its required notices.
    let unpacked = target.path("unpacked");
    std::fs::create_dir(&unpacked).unwrap();
    let unpack = Command::new("tar")
        .arg("-xf")
        .arg(target.path(&format!("package/{crate_file}")))
        .arg("-C")
        .arg(&unpacked)
        .output()
        .unwrap();
    assert!(unpack.status.success(), "{unpack:?}");
    let original = workspace().join("apps/symphony-server/client/public");
    let packaged = unpacked.join(format!("symphony-server-{version}/client/public"));
    assert_eq!(
        client_files(&original),
        client_files(&packaged),
        "the source crate carries the complete client, including each build's notices"
    );
    // Compile the archive's actual build script and its generated include_bytes! manifest in
    // isolation: installing the client needs neither Node nor files outside the source crate.
    let crate_root = packaged.parent().unwrap().parent().unwrap();
    let generator = target.path("embed-generator");
    let compile = Command::new("rustc")
        .arg("--edition=2024")
        .arg(crate_root.join("build.rs"))
        .arg("-o")
        .arg(&generator)
        .output()
        .unwrap();
    assert!(compile.status.success(), "{compile:?}");
    let generated = target.path("generated");
    std::fs::create_dir(&generated).unwrap();
    let generate = Command::new(generator)
        .env("CARGO_MANIFEST_DIR", crate_root)
        .env("OUT_DIR", &generated)
        .env_remove("SYMPHONY_CLIENT_BESIDE")
        .output()
        .unwrap();
    assert!(generate.status.success(), "{generate:?}");
    let probe = target.write("embedded.rs", &format!(
        "mod client {{ include!({:?}); }}\nfn main() {{\nassert_eq!(client::FILES.len(), {});\nfor (name, bytes) in client::FILES {{ assert_eq!(*bytes, std::fs::read(std::path::Path::new({:?}).join(name)).unwrap(), \"{{name}}\"); }}\n}}\n",
        generated.join("client.rs"), client_files(&original).len(), packaged,
    ));
    let binary = target.path("embedded");
    let compile = Command::new("rustc")
        .arg("--edition=2024")
        .arg(probe)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(compile.status.success(), "{compile:?}");
    let embedded = Command::new(binary).output().unwrap();
    assert!(embedded.status.success(), "{embedded:?}");
}

fn client_files(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    let mut files = std::collections::BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(folder) = pending.pop() {
        for entry in std::fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    std::fs::read(path).unwrap(),
                );
            }
        }
    }
    files
}

#[test]
fn a_keyword_crates_io_refuses_is_told_by_its_shape() {
    for taken in [
        "symphony",
        "json-schema",
        "os_unix",
        "c++",
        "a",
        "1password",
        "3d",
        "a2345678901234567890",
    ] {
        assert!(
            keyword_shape(taken),
            "{taken:?} is a keyword crates.io takes"
        );
    }
    for refused in [
        "",
        "-cli",
        "_private",
        "+plus",
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

/// The hosted board's driver (TLS and PostgreSQL's wire protocol over TCP) is compiled into no
/// build that does not ask for it by name: the board has the feature, no member's features turn
/// it on, its own default among them, no member asks the board for it, and the driver's crates
/// are the board's alone, each optional. So the installed release and every default build are
/// what they were without it (docs/operations/POSTGRESQL.md, "A hosted board").
#[test]
fn no_member_turns_the_hosted_board_on_unless_a_build_names_its_feature() {
    const FEATURE: &str = "hosted-postgresql";
    const BOARD: &str = "symphony-board";
    const DRIVER: [&str; 3] = ["tokio-postgres", "tokio-postgres-rustls", "rustls"];
    let mut has = false;
    for package in members() {
        for (feature, enables) in package["features"].as_object().unwrap() {
            if name(package) == BOARD && feature == FEATURE {
                has = true;
                continue;
            }
            for enabled in enables.as_array().unwrap() {
                assert!(
                    !enabled.as_str().unwrap().contains(FEATURE),
                    "{}'s feature {feature} turns {FEATURE} on",
                    name(package)
                );
            }
        }
        for dependency in package["dependencies"].as_array().unwrap() {
            let depended = dependency["name"].as_str().unwrap();
            let asked = dependency["features"].as_array().unwrap();
            assert!(
                depended != BOARD || !asked.iter().any(|feature| feature == FEATURE),
                "{} asks the board for {FEATURE}",
                name(package)
            );
            if DRIVER.contains(&depended) {
                assert_eq!(name(package), BOARD, "{depended} is the board's alone");
                assert_eq!(dependency["optional"], true, "{depended} is optional");
            }
        }
    }
    assert!(has, "the board has the feature {FEATURE}");
}
