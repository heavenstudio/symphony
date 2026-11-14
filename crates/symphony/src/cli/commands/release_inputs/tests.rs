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
    /// The files the checkout lists, as `git ls-files` names them.
    listed: Vec<String>,
}

/// What a package builds with: the dependency's id and its kind, `None` for a normal one.
type Dependencies = &'static [(&'static str, Option<&'static str>)];

/// The workspace's packages: id, name, folder and what each builds with.
const PACKAGES_OF_THE_WORKSPACE: [(&str, &str, &str, Dependencies); 4] = [
    (
        "ui",
        "symphony-ui",
        "apps/symphony-ui",
        &[
            ("api", None),
            ("derive", Some("build")),
            ("serde", None),
            ("primitives", None),
            ("board", Some("dev")),
        ],
    ),
    (
        "api",
        "symphony-api",
        "crates/symphony-api",
        &[("serde", None)],
    ),
    ("derive", "symphony-derive", "crates/symphony-derive", &[]),
    (
        "board",
        "symphony-board",
        "crates/tasks",
        &[("api", None), ("serde", None)],
    ),
];

/// The checkout's files, the client's inputs among them, with their bodies.
const CHECKOUT: [(&str, &str); 26] = [
    (
        "crates/symphony/src/cli/commands/split_repair.rs",
        "pub fn repair() {}\n",
    ),
    (
        "crates/symphony/src/cli/commands/split_repair/order.rs",
        "pub fn order() {}\n",
    ),
    (
        "crates/symphony/src/cli/commands/publish.rs",
        "pub fn publish() {}\n",
    ),
    ("Cargo.toml", "[workspace]\nmembers=[]\n"),
    (".cargo/config.toml", "[build]\ntarget-dir=\".target\"\n"),
    ("rust-toolchain.toml", "[toolchain]\nchannel=\"1.99.0\"\n"),
    ("apps/symphony-server/src/notices.css", ".notice{}\n"),
    (
        "apps/symphony-ui/Cargo.toml",
        "[package]\nname=\"symphony-ui\"\n",
    ),
    ("apps/symphony-ui/src/main.rs", "fn main() {}\n"),
    (
        "apps/symphony-ui/src/components/checkbox/style.css",
        ".dx-checkbox{color:red}\n",
    ),
    ("apps/symphony-ui/bundle.sh", "#!/bin/sh\nexit 0\n"),
    (
        "crates/symphony-api/Cargo.toml",
        "[package]\nname=\"symphony-api\"\n",
    ),
    ("crates/symphony-api/src/lib.rs", "pub struct Board;\n"),
    (
        "crates/symphony-derive/Cargo.toml",
        "[package]\nname=\"symphony-derive\"\n",
    ),
    ("crates/symphony-derive/src/lib.rs", "pub fn derive() {}\n"),
    (
        "crates/tasks/Cargo.toml",
        "[package]\nname=\"symphony-board\"\n",
    ),
    ("crates/tasks/src/lib.rs", "pub fn board() {}\n"),
    ("crates/tasks/tests/cli.rs", "#[test]\nfn cli() {}\n"),
    ("apps/symphony-server/src/main.rs", "fn main() {}\n"),
    (
        "apps/symphony-server/client/public/index.html",
        "<!DOCTYPE html>\n",
    ),
    ("docs/README.md", "# Docs\n"),
    ("RULES.md", "# Rules\n"),
    ("symphony.toml", "[board]\n"),
    ("tools/entry.sh", "#!/bin/sh\nexit 0\n"),
    (".codex/rules/symphony.rules", "rule\n"),
    (".claude-notes", "kept out\n"),
];

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
        let mut nodes = Vec::new();
        let mut component_files = Vec::new();
        for (id, package, folder, version) in [
            (
                "attributes",
                "dioxus-attributes",
                "dioxus-attributes",
                "0.1.0",
            ),
            ("primitives", "dioxus-primitives", "primitives", "0.0.1"),
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
                json!({"id":id,"name":package,"version":version,"source":source,"manifest_path":manifest}),
            );
            component_files.push(format!("{folder}/Cargo.toml"));
        }
        nodes.push(json!({"id":"attributes","deps":[]}));
        nodes.push(json!({"id":"primitives","deps":[
            {"pkg":"attributes","dep_kinds":[{"kind":null,"target":null}]}
        ]}));
        let registry = "registry+https://github.com/rust-lang/crates.io-index";
        packages.push(json!({
            "id":"serde","name":"serde","version":"1.0.0","source":registry,
            "manifest_path":"/registry/serde-1.0.0/Cargo.toml"
        }));
        nodes.push(json!({"id":"serde","deps":[]}));
        lock.push_str(&format!("\n[[package]]\nname=\"unrelated\"\nversion=\"1.2.3\"\nsource=\"{registry}\"\nchecksum=\"unchanged-checksum\"\n"));
        let mut checkout = vec![("Cargo.lock".to_string(), lock)];
        checkout.extend(
            CHECKOUT
                .iter()
                .map(|(path, body)| (path.to_string(), body.to_string())),
        );
        for (path, body) in &checkout {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, body).unwrap();
        }
        for script in ["apps/symphony-ui/bundle.sh", "tools/entry.sh"] {
            fs::set_permissions(root.join(script), fs::Permissions::from_mode(0o755)).unwrap();
        }
        let root = root.canonicalize().unwrap();
        for (id, package, folder, dependencies) in PACKAGES_OF_THE_WORKSPACE {
            packages.push(json!({
                "id":id,"name":package,"version":"0.1.0","source":null,
                "manifest_path":root.join(folder).join("Cargo.toml")
            }));
            nodes.push(
                json!({"id":id,"deps":dependencies.iter().map(|(dependency, kind)| json!({
                "pkg":dependency,"dep_kinds":[{"kind":kind,"target":null}]
            })).collect::<Vec<_>>()}),
            );
        }
        let metadata = tree.write(
            &format!("{name}/metadata.json"),
            &json!({
                "workspace_root":root,"packages":packages,
                "workspace_members":PACKAGES_OF_THE_WORKSPACE.map(|(id, ..)| id),
                "resolve":{"nodes":nodes,"root":null}
            })
            .to_string(),
        );
        let files = tree.write(&format!("{name}/files"), "");
        let component_list = tree.write(&format!("{name}/component-files"), "");
        let listed: Vec<String> = checkout.into_iter().map(|(name, _)| name).collect();
        fs::write(
            &files,
            listed
                .iter()
                .map(|name| format!("{name}\0"))
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
            listed,
        }
    }

    /// Lists `names` too, as git lists a file added to the checkout.
    fn list(&mut self, names: &[&str]) {
        self.listed
            .extend(names.iter().map(|name| name.to_string()));
        fs::write(
            &self.files,
            self.listed
                .iter()
                .map(|name| format!("{name}\0"))
                .collect::<String>(),
        )
        .unwrap();
    }

    /// Writes `body` at `path` in the checkout.
    fn write(&self, path: &str, body: &str) {
        let path = self.root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    /// The files of the snapshot's source, by their path in it.
    fn captured(snapshot: &Path) -> Vec<String> {
        fn walk(root: &Path, at: &Path, found: &mut Vec<String>) {
            for entry in fs::read_dir(at).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(root, &path, found);
                } else {
                    found.push(path.strip_prefix(root).unwrap().display().to_string());
                }
            }
        }
        let mut found = Vec::new();
        walk(snapshot, snapshot, &mut found);
        found.sort();
        found
    }

    fn try_key(&self) -> io::Result<PathBuf> {
        prepare(
            &self.root,
            &self.metadata,
            &self.files,
            &self.component_files,
            &self.parent,
            true,
            None,
        )
    }

    fn key(&self) -> PathBuf {
        self.try_key().unwrap()
    }

    /// The snapshot, taken as bundle.sh takes it: its key, then the capture under that key.
    fn snapshot(&self) -> io::Result<PathBuf> {
        let key = self.try_key()?;
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
            .starts_with("usage: symphony-build release-inputs ")
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
            .starts_with("symphony-build release-inputs: ")
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
        fs::metadata(root.join("apps/symphony-ui/bundle.sh"))
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
    fixture.write("apps/symphony-ui/bundle.sh", "#!/bin/sh\nexit 17\n");
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
    let all = |_: &Path| true;
    let none = |_: &Path| false;
    // Framing and escapes are refused whether or not the entry is an input.
    for contents in ["", "Cargo.lock", "../Cargo.lock\0", "/Cargo.lock\0"] {
        fs::write(&fixture.files, contents).unwrap();
        for holds in [&all as &dyn Fn(&Path) -> bool, &none] {
            assert!(
                inventory(&fixture.root, &fixture.files, "source", holds).is_err(),
                "{contents:?}"
            );
        }
    }
    let outside = tree.write("outside", "do not copy");
    std::os::unix::fs::symlink(outside, fixture.root.join("link")).unwrap();
    fs::create_dir_all(fixture.root.join(".target")).unwrap();
    fs::write(fixture.root.join(".target/output"), "built").unwrap();
    // An input that is duplicated, derived, a link or not a file is refused; the same entry is
    // passed over when it is no input.
    for entry in [
        "Cargo.lock\0Cargo.lock\0",
        ".target/output\0",
        "link\0",
        ".cargo\0",
    ] {
        fs::write(&fixture.files, entry).unwrap();
        assert!(
            inventory(&fixture.root, &fixture.files, "source", all).is_err(),
            "{entry}"
        );
        assert_eq!(
            inventory(&fixture.root, &fixture.files, "source", none).unwrap(),
            Files::new(),
            "{entry}"
        );
    }
}

#[test]
fn the_snapshot_holds_the_clients_packages_the_workspace_files_and_the_other_members_manifests() {
    let tree = Tree::new();
    let mut fixture = Fixture::new(&tree, "source");
    // The agents' integration: a skill, its link, an agent's worktree; and the derived folders.
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
    fixture.write("artifacts/shot.base64", "picture");
    fixture.write(".target/debug/symphony", "binary");
    fixture.list(&[
        ".agents/skills/work/SKILL.md",
        ".claude/skills/work",
        ".claude/worktrees/agent-1/",
        "artifacts/shot.base64",
        ".target/debug/symphony",
    ]);
    let snapshot = fixture.snapshot().unwrap();
    assert_eq!(
        Fixture::captured(&snapshot),
        [
            ".cargo/config.toml",
            "Cargo.lock",
            "Cargo.toml",
            "apps/symphony-server/src/notices.css",
            "apps/symphony-ui/Cargo.toml",
            "apps/symphony-ui/bundle.sh",
            "apps/symphony-ui/src/components/checkbox/style.css",
            "apps/symphony-ui/src/main.rs",
            "crates/symphony-api/Cargo.toml",
            "crates/symphony-api/src/lib.rs",
            "crates/symphony-derive/Cargo.toml",
            "crates/symphony-derive/src/lib.rs",
            "crates/symphony/src/cli/commands/split_repair.rs",
            "crates/symphony/src/cli/commands/split_repair/order.rs",
            "crates/tasks/Cargo.toml",
            "crates/tasks/src/lib.rs",
            "rust-toolchain.toml",
        ]
    );
    // The Cargo configuration and the client's style sheets are patched in the copy (configure),
    // and another member's library is an empty target (below).
    let patched = [
        ".cargo/config.toml",
        "apps/symphony-ui/src/components/checkbox/style.css",
        "crates/tasks/src/lib.rs",
    ];
    for (path, body) in CHECKOUT {
        if !patched.contains(&path) && snapshot.join(path).exists() {
            assert_eq!(
                fs::read_to_string(snapshot.join(path)).unwrap().as_str(),
                body,
                "{path}: an input is copied as it is"
            );
        }
    }
    assert_eq!(
        fs::read(snapshot.join("crates/tasks/src/lib.rs")).unwrap(),
        b"",
        "another member's library is an empty target, not its source"
    );
}

#[test]
fn a_change_outside_the_clients_inputs_keeps_its_snapshot_and_one_inside_names_a_new_one() {
    let tree = Tree::new();
    let mut fixture = Fixture::new(&tree, "source");
    let key = fixture.key();
    let snapshot = fixture.snapshot().unwrap();
    for (path, body) in [
        ("crates/tasks/src/lib.rs", "pub fn board() { todo!() }\n"),
        ("crates/tasks/tests/cli.rs", "#[test]\nfn other() {}\n"),
        (
            "apps/symphony-server/src/main.rs",
            "fn main() { serve() }\n",
        ),
        (
            "apps/symphony-server/client/public/index.html",
            "<p>new</p>\n",
        ),
        ("docs/README.md", "# Docs, revised\n"),
        ("RULES.md", "# Rules, revised\n"),
        ("symphony.toml", "[board]\non_review = \"true\"\n"),
        ("tools/entry.sh", "#!/bin/sh\nexit 3\n"),
        (".claude-notes", "revised\n"),
        (
            "crates/symphony/src/cli/commands/publish.rs",
            "pub fn publish() { todo!() }\n",
        ),
    ] {
        fixture.write(path, body);
        assert_eq!(fixture.key(), key, "{path} is no input of the client");
    }
    fixture.write("crates/tasks/tests/sweep.rs", "#[test]\nfn sweep() {}\n");
    fixture.write("docs/NEW.md", "# New\n");
    fixture.list(&["crates/tasks/tests/sweep.rs", "docs/NEW.md"]);
    assert_eq!(fixture.key(), key, "a new file outside is no input either");
    assert_eq!(fixture.snapshot().unwrap(), snapshot);
    for path in [
        "Cargo.toml",
        ".cargo/config.toml",
        "rust-toolchain.toml",
        "apps/symphony-server/src/notices.css",
        "apps/symphony-ui/Cargo.toml",
        "apps/symphony-ui/src/main.rs",
        "crates/symphony-api/src/lib.rs",
        "crates/symphony-derive/src/lib.rs",
        "crates/tasks/Cargo.toml",
        "crates/symphony/src/cli/commands/split_repair.rs",
        "crates/symphony/src/cli/commands/split_repair/order.rs",
    ] {
        let original = fs::read_to_string(fixture.root.join(path)).unwrap();
        fixture.write(path, &format!("{original}# changed\n"));
        assert_ne!(fixture.key(), key, "{path} is an input of the client");
        fixture.write(path, &original);
        assert_eq!(fixture.key(), key, "{path} restored");
    }
    fixture.write("apps/symphony-ui/src/view.rs", "pub fn view() {}\n");
    fixture.list(&["apps/symphony-ui/src/view.rs"]);
    assert_ne!(fixture.key(), key, "a new file of the client is an input");
    let key = fixture.key();
    fixture.write(
        "crates/symphony/src/cli/commands/split_repair/wasm.rs",
        "pub fn read() {}\n",
    );
    fixture.list(&["crates/symphony/src/cli/commands/split_repair/wasm.rs"]);
    assert_ne!(fixture.key(), key, "a new file of the split repair is one");
}

#[test]
fn another_member_gets_an_empty_file_for_each_target_its_manifest_names_without_a_path() {
    let tree = Tree::new();
    let fixture = Fixture::new(&tree, "source");
    let manifest = "crates/tasks/Cargo.toml";
    fixture.write(
        manifest,
        "[package]\nname=\"symphony-board\"\n[lib]\npath=\"src/board.rs\"\n\
         [[bin]]\nname=\"tasks\"\n[[test]]\nname=\"perf\"\nharness=false\n\
         [[example]]\nname=\"demo\"\npath=\"demos/demo.rs\"\n[[bench]]\nname=\"speed\"\n",
    );
    let snapshot = fixture.snapshot().unwrap();
    let board: Vec<String> = Fixture::captured(&snapshot)
        .into_iter()
        .filter(|path| path.starts_with("crates/tasks/"))
        .collect();
    assert_eq!(
        board,
        [
            "crates/tasks/Cargo.toml",
            "crates/tasks/benches/speed.rs",
            "crates/tasks/src/bin/tasks.rs",
            "crates/tasks/src/lib.rs",
            "crates/tasks/tests/perf.rs",
        ]
    );
    for target in ["benches/speed.rs", "src/bin/tasks.rs", "tests/perf.rs"] {
        assert_eq!(
            fs::read(snapshot.join("crates/tasks").join(target)).unwrap(),
            b""
        );
    }
    for name in ["../escape", "a/b", ""] {
        fixture.write(
            manifest,
            &format!("[package]\nname=\"symphony-board\"\n[[test]]\nname={name:?}\n"),
        );
        let error = fixture.try_key().unwrap_err().to_string();
        assert!(
            error.contains("invalid test target name"),
            "{name}: {error}"
        );
    }
    fixture.write(manifest, "[package\n");
    assert!(fixture.snapshot().is_err(), "an unreadable manifest");
}

#[test]
fn a_target_of_another_member_that_is_also_listed_or_a_member_left_unlisted_is_refused() {
    let tree = Tree::new();
    let mut fixture = Fixture::new(&tree, "source");
    let original: Value = serde_json::from_slice(&fs::read(&fixture.metadata).unwrap()).unwrap();
    // A member inside the client's package: its library is both a listed input and a target.
    let mut nested = original.clone();
    for package in nested["packages"].as_array_mut().unwrap() {
        if package["id"] == "board" {
            package["manifest_path"] = json!(fixture.root.join("apps/symphony-ui/Cargo.toml"));
        }
    }
    fixture.write("apps/symphony-ui/src/lib.rs", "pub fn app() {}\n");
    fixture.list(&["apps/symphony-ui/src/lib.rs"]);
    fs::write(&fixture.metadata, nested.to_string()).unwrap();
    let error = fixture.try_key().unwrap_err().to_string();
    assert!(
        error.contains("a member's target is a release input"),
        "{error}"
    );
    fs::write(&fixture.metadata, original.to_string()).unwrap();
    let listed = fs::read_to_string(&fixture.files).unwrap();
    fs::write(
        &fixture.files,
        listed.replace("crates/tasks/Cargo.toml\0", ""),
    )
    .unwrap();
    let error = fixture.try_key().unwrap_err().to_string();
    assert!(
        error.contains("member manifest is no release input"),
        "{error}"
    );
}

#[test]
fn metadata_without_one_client_member_its_resolve_or_a_local_package_in_the_workspace_is_refused() {
    let tree = Tree::new();
    let fixture = Fixture::new(&tree, "source");
    let original: Value = serde_json::from_slice(&fs::read(&fixture.metadata).unwrap()).unwrap();
    let outside = tree.write("elsewhere/Cargo.toml", "[package]\nname=\"symphony-api\"\n");
    let mut cases: Vec<(Value, &str)> = Vec::new();
    let mut changed = original.clone();
    changed["workspace_members"] = json!(["api", "derive", "board"]);
    cases.push((changed, "no single client member"));
    let mut changed = original.clone();
    for package in changed["packages"].as_array_mut().unwrap() {
        if package["id"] == "board" {
            package["name"] = json!("symphony-ui");
        }
    }
    cases.push((changed, "no single client member"));
    let mut changed = original.clone();
    changed["resolve"] = Value::Null;
    cases.push((changed, "missing Cargo resolve"));
    let mut changed = original.clone();
    changed["workspace_members"] = Value::Null;
    cases.push((changed, "missing Cargo workspace members"));
    let mut changed = original.clone();
    changed["resolve"]["nodes"]
        .as_array_mut()
        .unwrap()
        .retain(|node| node["id"] != "api");
    cases.push((changed, "Cargo resolve has no node api"));
    let mut changed = original.clone();
    for package in changed["packages"].as_array_mut().unwrap() {
        if package["id"] == "api" {
            package["manifest_path"] = json!(outside);
        }
    }
    cases.push((changed, "local package outside the workspace"));
    for (metadata, expected) in cases {
        fs::write(&fixture.metadata, metadata.to_string()).unwrap();
        let error = fixture.try_key().unwrap_err().to_string();
        assert!(error.contains(expected), "{expected}: {error}");
    }
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

#[test]
fn the_regenerators_client_key_captures_the_inputs_as_the_client_build_does() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let key = fs::read_to_string(root.join("tools/client-key.sh")).unwrap();
    let bundle = fs::read_to_string(root.join("apps/symphony-ui/bundle.sh")).unwrap();
    let capture: Vec<&str> = key
        .lines()
        .skip_while(|line| *line != "# bundle.sh's capture, line for line:")
        .skip(1)
        .take_while(|line| *line != "# Its key, as bundle.sh names its snapshot's lock:")
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert!(capture.len() > 10, "{capture:?}");
    // Each line of the key's capture is one of bundle.sh's, in bundle.sh's order.
    let mut lines = bundle.lines();
    for wanted in &capture {
        assert!(
            lines.any(|line| line == *wanted),
            "bundle.sh captures no `{wanted}` after the lines before it"
        );
    }
    // Its key is the one bundle.sh names its snapshot's lock by, after that capture.
    let printed = key
        .lines()
        .skip_while(|line| *line != "# Its key, as bundle.sh names its snapshot's lock:")
        .nth(1)
        .unwrap();
    assert!(
        lines.any(|line| line.contains(printed)),
        "bundle.sh names its lock by no `{printed}`"
    );
    assert_eq!(
        key.lines().last(),
        Some(printed),
        "the key is the last line printed"
    );
}
