#[test]
fn a_bundle_after_another_trees_wasm32_builds_cleans_this_workspaces_crates_from_both_profiles() {
    let scratch = Scratch::new("wasm-build-bundle");
    let common = scratch.0.join("common");
    let target = another_trees_build(&common);
    let ran = scratch.0.join("dx-ran");
    stand_in_tools(&common, &ran);
    let done = Command::new("bash")
        .arg(crate_dir().join("bundle.sh"))
        .arg(scratch.0.join("out"))
        .env("SF_GIT_COMMON_DIR", &common)
        .env("CARGO_TARGET_DIR", &target)
        .output()
        .expect("run bundle.sh");
    assert!(
        done.status.success(),
        "bundle.sh failed: {}",
        String::from_utf8_lossy(&done.stderr)
    );
    assert!(ran.exists(), "bundle.sh never ran dx");
    for profile in ["debug", "wasm-release"] {
        assert_eq!(
            left_in(&target, profile),
            [A_DEPENDENCY],
            "another tree's {WASM32} {profile} build of this workspace's crates is still there"
        );
    }
    let record = fs::read_to_string(record_in(&target)).unwrap();
    assert!(
        record.starts_with(&format!(
            "{} ",
            crate_dir().join("../..").canonicalize().unwrap().display()
        )),
        "the record names {record:?}, not this tree"
    );
}

#[test]
fn a_new_canonical_snapshot_cleans_all_private_release_dependencies_once_without_changing_default_records() {
    let scratch = Scratch::new("release-snapshot-guard");
    let tree = scratch.0.join("source");
    let fresh = tree.join("apps/symphony-ui/wasm-fresh.sh");
    executable(&fresh, &fs::read_to_string(crate_dir().join("wasm-fresh.sh")).unwrap());
    fs::write(tree.join("Cargo.toml"), "[workspace]\n").unwrap();
    let said = scratch.0.join("cargo-said");
    let cargo = scratch.0.join("cargo");
    executable(&cargo, &format!("#!/bin/bash\nprintf '%s\\n' \"$*\" >> '{}'\n", said.display()));
    let target = scratch.0.join("private-release-target");
    fs::create_dir_all(&target).unwrap();
    fs::write(record_in(&target), "default caller record").unwrap();
    let run = || {
        let done = Command::new("bash").arg(&fresh).arg("release-snapshot")
            .env("CARGO", &cargo).env("CARGO_TARGET_DIR", &target).output().unwrap();
        succeeded(&done, "private canonical release guard");
    };
    run();
    let expected = format!("clean --target {WASM32} --profile wasm-release --quiet --locked");
    assert_eq!(calls(&said), std::slice::from_ref(&expected));
    assert_eq!(fs::read_to_string(record_in(&target)).unwrap(), "default caller record");
    run();
    assert_eq!(calls(&said), std::slice::from_ref(&expected), "the same snapshot retains its dependency products");
    let new = tree.join("Cargo.toml.new");
    fs::write(&new, "[workspace]\n").unwrap();
    fs::rename(new, tree.join("Cargo.toml")).unwrap();
    run();
    assert_eq!(calls(&said), [expected.clone(), expected]);
    assert_eq!(fs::read_to_string(record_in(&target)).unwrap(), "default caller record");
}

