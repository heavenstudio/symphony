use super::*;
use crate::test_tree::Tree;
use serde_json::json;

struct Fixture {
    root: PathBuf,
    components: PathBuf,
    metadata: PathBuf,
    files: PathBuf,
    component_files: PathBuf,
    parent: PathBuf,
}

impl Fixture {
    // Protocol fixtures exercise capture and publication; real declared-member builds are separate.
    fn new(tree: &Tree, name: &str) -> Self {
        let root = tree.path(&format!("{name}/source"));
        let components = tree.path(&format!("{name}/components"));
        let source = format!(
            "git+https://github.com/DioxusLabs/components?rev={0}#{0}",
            "a".repeat(40)
        );
        let mut lock = "version = 4\n".to_string();
        let mut packages = Vec::new();
        let mut component_files = Vec::new();
        for (package, folder, version) in [
            ("dioxus-attributes", "dioxus-attributes", "0.1.0"),
            ("dioxus-primitives", "primitives", "0.0.1"),
        ] {
            lock.push_str(&format!(
                "\n[[package]]\nname={package:?}\nversion={version:?}\nsource={source:?}\n"
            ));
            let manifest = components.join(folder).join("Cargo.toml");
            fs::create_dir_all(manifest.parent().unwrap()).unwrap();
            fs::write(
                &manifest,
                format!("[package]\nname={package:?}\nversion={version:?}\n"),
            )
            .unwrap();
            packages.push(
                json!({"name":package,"version":version,"source":source,"manifest_path":manifest}),
            );
            component_files.push(format!("{folder}/Cargo.toml"));
        }
        lock.push_str("\n[[package]]\nname=\"unrelated\"\nversion=\"1.2.3\"\nsource=\"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum=\"unchanged-checksum\"\n");
        let inputs = [
            ("Cargo.toml", "[workspace]\nmembers=[]\n".to_string()),
            ("Cargo.lock", lock),
            (
                ".cargo/config.toml",
                "[build]\ntarget-dir=\".target\"\n".to_string(),
            ),
            (
                "apps/symphony-ui/src/components/checkbox/style.css",
                ".dx-checkbox{color:red}\n".to_string(),
            ),
            ("tools/entry.sh", "#!/bin/sh\nexit 0\n".to_string()),
        ];
        for (path, body) in &inputs {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, body).unwrap();
        }
        fs::set_permissions(
            root.join("tools/entry.sh"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        let root = root.canonicalize().unwrap();
        let metadata = tree.write(
            &format!("{name}/metadata.json"),
            &json!({
                "workspace_root":root,"packages":packages
            })
            .to_string(),
        );
        let files = tree.write(&format!("{name}/files"), "");
        let component_list = tree.write(&format!("{name}/component-files"), "");
        fs::write(
            &files,
            inputs
                .iter()
                .map(|(name, _)| format!("{name}\0"))
                .collect::<String>(),
        )
        .unwrap();
        fs::write(
            &component_list,
            component_files
                .iter()
                .map(|name| format!("{name}\0"))
                .collect::<String>(),
        )
        .unwrap();
        let parent = tree.path("snapshots");
        fs::create_dir_all(&parent).unwrap();
        let parent = parent.canonicalize().unwrap();
        Self {
            root,
            components,
            metadata,
            files,
            component_files: component_list,
            parent,
        }
    }

    fn key(&self) -> PathBuf {
        prepare(
            &self.root,
            &self.metadata,
            &self.files,
            &self.component_files,
            &self.parent,
            true,
            None,
        )
        .unwrap()
    }

    fn snapshot(&self) -> io::Result<PathBuf> {
        let key = self.key();
        prepare(
            &self.root,
            &self.metadata,
            &self.files,
            &self.component_files,
            &self.parent,
            false,
            Some(&key),
        )
    }
}

#[test]
fn cli_status_and_output_failures_never_claim_success_or_publish_an_unselected_input() {
    let tree = Tree::new();
    let fixture = Fixture::new(&tree, "source");
    let mut out = Vec::new();
    let mut err = Vec::new();
    assert_eq!(run(&[], &mut out, &mut err, &fixture.parent), 2);
    assert!(out.is_empty());
    assert!(
        String::from_utf8(err)
            .unwrap()
            .starts_with("usage: symphony release-inputs ")
    );
    let mut args: Vec<String> = [
        &fixture.root,
        &fixture.metadata,
        &fixture.files,
        &fixture.component_files,
    ]
    .into_iter()
    .map(|path| path.to_string_lossy().into_owned())
    .collect();
    args.push("--key".into());
    let mut err = Vec::new();
    assert_eq!(run(&args, &mut out, &mut err, &fixture.parent), 0);
    assert!(err.is_empty());
    let key = PathBuf::from(String::from_utf8(out).unwrap().trim());
    args[4] = key.to_string_lossy().into_owned();
    let mut out = Vec::new();
    assert_eq!(run(&args, &mut out, &mut err, &fixture.parent), 0);
    let root = PathBuf::from(String::from_utf8(out).unwrap().trim());
    assert!(root.join("Cargo.toml").is_file());
    struct Failed;
    impl Write for Failed {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("writer refused"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    assert_eq!(run(&args, &mut Failed, &mut err, &fixture.parent), 1);
    assert!(String::from_utf8(err).unwrap().contains("writer refused"));
    fs::write(&fixture.metadata, "{").unwrap();
    let mut out = Vec::new();
    let mut err = Vec::new();
    assert_eq!(run(&args, &mut out, &mut err, &fixture.parent), 1);
    assert!(out.is_empty());
    assert!(
        String::from_utf8(err)
            .unwrap()
            .starts_with("symphony release-inputs: ")
    );
    assert!(
        root.join("Cargo.toml").is_file(),
        "an input error preserves the previous snapshot"
    );
}

#[test]
fn identical_inputs_at_different_roots_use_one_verified_snapshot_without_changing_the_sources() {
    let tree = Tree::new();
    let left = Fixture::new(&tree, "left with spaces");
    let right = Fixture::new(&tree, "right");
    let original_lock = fs::read(left.root.join("Cargo.lock")).unwrap();
    let original_config = fs::read(left.root.join(".cargo/config.toml")).unwrap();
    let root = left.snapshot().unwrap();
    assert_eq!(root, right.snapshot().unwrap());
    assert!(!root.starts_with(&left.root));
    assert!(!root.starts_with(&right.root));
    assert_eq!(
        fs::read(left.root.join("Cargo.lock")).unwrap(),
        original_lock
    );
    assert_eq!(
        fs::read(left.root.join(".cargo/config.toml")).unwrap(),
        original_config
    );
    assert_eq!(
        fs::metadata(root.join("tools/entry.sh"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    let lock: toml::Table = fs::read_to_string(root.join("Cargo.lock"))
        .unwrap()
        .parse()
        .unwrap();
    let mut expected: toml::Table = std::str::from_utf8(&original_lock)
        .unwrap()
        .parse()
        .unwrap();
    for package in expected["package"].as_array_mut().unwrap() {
        if ["dioxus-attributes", "dioxus-primitives"].contains(&package["name"].as_str().unwrap()) {
            package.as_table_mut().unwrap().remove("source");
        }
    }
    assert_eq!(
        lock, expected,
        "only the two copied component sources become private paths"
    );
    let config: toml::Table = fs::read_to_string(root.join(".cargo/config.toml"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(config["build"]["target-dir"].as_str(), Some(".target"));
    for (package, folder) in [
        ("dioxus-attributes", "dioxus-attributes"),
        ("dioxus-primitives", "primitives"),
    ] {
        assert_eq!(
            config["patch"][REPOSITORY][package]["path"]
                .as_str()
                .unwrap(),
            root.parent()
                .unwrap()
                .join("components")
                .join(folder)
                .to_str()
                .unwrap()
        );
    }
}

#[test]
fn private_capture_refuses_existing_patches_and_missing_configuration_without_altering_original_inputs()
 {
    let tree = Tree::new();
    let fixture = Fixture::new(&tree, "source");
    let config = fixture.root.join(".cargo/config.toml");
    let body = "[build]\ntarget-dir=\".target\"\n[patch.crates-io]\nunrelated={path=\"../developer-copy\"}\n";
    fs::write(&config, body).unwrap();
    let error = fixture.snapshot().unwrap_err().to_string();
    assert!(error.contains("already contain Cargo patches"), "{error}");
    assert_eq!(fs::read_to_string(&config).unwrap(), body);
    fs::write(&config, "[build]\ntarget-dir=\".target\"\n").unwrap();
    let original = fs::read_to_string(&fixture.files).unwrap();
    fs::write(&fixture.files, original.replace(".cargo/config.toml\0", "")).unwrap();
    let error = fixture.snapshot().unwrap_err().to_string();
    assert!(
        error.contains("missing owning Cargo configuration"),
        "{error}"
    );
    assert_eq!(
        fs::read_to_string(&config).unwrap(),
        "[build]\ntarget-dir=\".target\"\n"
    );
}

#[test]
fn changed_inputs_need_a_new_lock_and_css_bytes_bind_the_scoped_selectors_to_new_asset_names() {
    let tree = Tree::new();
    let fixture = Fixture::new(&tree, "source");
    let key = fixture.key();
    let old = fixture.snapshot().unwrap();
    let stylesheet = "apps/symphony-ui/src/components/checkbox/style.css";
    fs::write(fixture.root.join("tools/entry.sh"), "#!/bin/sh\nexit 17\n").unwrap();
    let why = prepare(
        &fixture.root,
        &fixture.metadata,
        &fixture.files,
        &fixture.component_files,
        &fixture.parent,
        false,
        Some(&key),
    )
    .unwrap_err()
    .to_string();
    assert!(
        why.contains("changed before their publication lock"),
        "{why}"
    );
    let new = fixture.snapshot().unwrap();
    assert_ne!(old, new);
    let before = fs::read_to_string(old.join(stylesheet)).unwrap();
    let after = fs::read_to_string(new.join(stylesheet)).unwrap();
    assert!(before.starts_with(".dx-checkbox{color:red}"));
    assert!(after.starts_with(".dx-checkbox{color:red}"));
    assert!(before.contains("--symphony-release-input:symphony-"));
    assert_ne!(
        before.split("--symphony-release-input:").nth(1),
        after.split("--symphony-release-input:").nth(1)
    );
    assert_eq!(fixture.snapshot().unwrap(), new);
}

#[test]
fn foreign_missing_ambiguous_and_malformed_metadata_never_select_a_component_provider() {
    let tree = Tree::new();
    let fixture = Fixture::new(&tree, "source");
    let original: Value = serde_json::from_slice(&fs::read(&fixture.metadata).unwrap()).unwrap();
    for (field, value, expected) in [
        (
            "workspace_root",
            json!(fixture.components),
            "another workspace",
        ),
        ("packages", json!([]), "ambiguous component"),
        (
            "packages",
            json!([
                original["packages"][0],
                original["packages"][0],
                original["packages"][1]
            ]),
            "ambiguous component",
        ),
    ] {
        let mut changed = original.clone();
        changed[field] = value;
        fs::write(&fixture.metadata, changed.to_string()).unwrap();
        let error = prepare(
            &fixture.root,
            &fixture.metadata,
            &fixture.files,
            &fixture.component_files,
            &fixture.parent,
            true,
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains(expected), "{error}");
    }
    for (field, value) in [
        ("source", "git+https://example.invalid/components#abc"),
        ("version", "9.0.0"),
    ] {
        let mut changed = original.clone();
        changed["packages"][0][field] = json!(value);
        fs::write(&fixture.metadata, changed.to_string()).unwrap();
        let error = prepare(
            &fixture.root,
            &fixture.metadata,
            &fixture.files,
            &fixture.component_files,
            &fixture.parent,
            true,
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("does not match its declared locked source"),
            "{error}"
        );
    }
    fs::write(&fixture.metadata, "{").unwrap();
    assert!(
        prepare(
            &fixture.root,
            &fixture.metadata,
            &fixture.files,
            &fixture.component_files,
            &fixture.parent,
            true,
            None
        )
        .is_err()
    );
}

#[test]
fn inventory_refuses_escape_duplicates_links_nonregular_entries_and_incomplete_framing() {
    let tree = Tree::new();
    let fixture = Fixture::new(&tree, "source");
    for contents in [
        "",
        "Cargo.lock",
        "../Cargo.lock\0",
        "/Cargo.lock\0",
        "Cargo.lock\0Cargo.lock\0",
        ".target/output\0",
    ] {
        fs::write(&fixture.files, contents).unwrap();
        assert!(
            inventory(&fixture.root, &fixture.files, "source").is_err(),
            "{contents:?}"
        );
    }
    let outside = tree.write("outside", "do not copy");
    std::os::unix::fs::symlink(outside, fixture.root.join("link")).unwrap();
    for entry in ["link\0", ".cargo\0"] {
        fs::write(&fixture.files, entry).unwrap();
        assert!(
            inventory(&fixture.root, &fixture.files, "source").is_err(),
            "{entry}"
        );
    }
}

#[test]
fn inventory_leaves_out_the_agents_integration_links_and_worktrees_but_not_a_lookalike() {
    let tree = Tree::new();
    let fixture = Fixture::new(&tree, "source");
    let skill = fixture.root.join(".agents/skills/work/SKILL.md");
    fs::create_dir_all(skill.parent().unwrap()).unwrap();
    fs::write(&skill, "work").unwrap();
    fs::create_dir_all(fixture.root.join(".claude/skills")).unwrap();
    std::os::unix::fs::symlink(
        "../../.agents/skills/work",
        fixture.root.join(".claude/skills/work"),
    )
    .unwrap();
    fs::create_dir_all(fixture.root.join(".claude/worktrees/agent-1")).unwrap();
    fs::create_dir_all(fixture.root.join(".codex/rules")).unwrap();
    fs::write(fixture.root.join(".codex/rules/symphony.rules"), "rule").unwrap();
    fs::write(fixture.root.join(".claude-notes"), "kept").unwrap();
    fs::write(
        &fixture.files,
        "Cargo.lock\0.agents/skills/work/SKILL.md\0.claude/skills/work\0\
         .claude/worktrees/agent-1/\0.codex/rules/symphony.rules\0.claude-notes\0",
    )
    .unwrap();
    let files = inventory(&fixture.root, &fixture.files, "source").unwrap();
    let names: Vec<&Path> = files.keys().map(PathBuf::as_path).collect();
    assert_eq!(
        names,
        [
            Path::new("source/.claude-notes"),
            Path::new("source/Cargo.lock")
        ]
    );
    // A link anywhere else is still refused.
    std::os::unix::fs::symlink("Cargo.lock", fixture.root.join("docs-link")).unwrap();
    fs::write(&fixture.files, "docs-link\0").unwrap();
    assert!(inventory(&fixture.root, &fixture.files, "source").is_err());
}

#[test]
fn an_existing_snapshot_is_verified_in_full_and_never_repaired_by_following_links() {
    let tree = Tree::new();
    let fixture = Fixture::new(&tree, "source");
    let root = fixture.snapshot().unwrap();
    let path = root.join("Cargo.toml");
    let original = fs::read(&path).unwrap();
    fs::write(&path, "altered").unwrap();
    assert!(
        fixture
            .snapshot()
            .unwrap_err()
            .to_string()
            .contains("snapshot changed")
    );
    fs::write(&path, original).unwrap();
    let extra = root.join("extra");
    std::os::unix::fs::symlink(tree.path("outside"), &extra).unwrap();
    assert!(fixture.snapshot().is_err());
    assert!(
        fs::symlink_metadata(extra)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let key = fixture.key();
    fs::set_permissions(key, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        prepare(
            &fixture.root,
            &fixture.metadata,
            &fixture.files,
            &fixture.component_files,
            &fixture.parent,
            true,
            None
        )
        .unwrap_err()
        .to_string()
        .contains("private owned directory")
    );
}
