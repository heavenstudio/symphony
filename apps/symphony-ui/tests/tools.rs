//! `tools.sh`, which installs the client's build tools into `symphony/tools/` in the git common dir
//! and names the folder a build runs them from: until `tools/move-state.sh` moves a repository's
//! state, an older build's copies in `sf-cargo/`, where they stand, never installed beside nor
//! removed. Each case is a folder of its own under the temp folder, its common dir named by
//! `SYMPHONY_GIT_COMMON_DIR`, so nothing here reads a real repository's.
//!
//! No freshly written file is executed: the installed tools are only tested for being there, and
//! the one Cargo a case runs is a link to the system's `false`.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A tree holding the client's `tools.sh` and a lock pinning wasm-bindgen, and a common dir.
struct Fixture(PathBuf);

impl Fixture {
    fn new(case: &str) -> Fixture {
        let base =
            std::env::temp_dir().join(format!("symphony-ui-tools {case}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("apps/symphony-ui")).unwrap();
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tools.sh"),
            base.join("apps/symphony-ui/tools.sh"),
        )
        .unwrap();
        fs::write(
            base.join("Cargo.lock"),
            "name = \"wasm-bindgen\"\nversion = \"fixture\"\n",
        )
        .unwrap();
        // The only Cargo on the script's PATH, which fails: an install it starts ends there.
        fs::create_dir_all(base.join("bin")).unwrap();
        std::os::unix::fs::symlink("/usr/bin/false", base.join("bin/cargo")).unwrap();
        fs::create_dir(base.join("common")).unwrap();
        Fixture(base)
    }

    fn common(&self) -> PathBuf {
        self.0.join("common")
    }

    /// A file under the common dir, its folders made; executable for an installed tool.
    fn put(&self, path: &str, executable: bool) {
        let file = self.common().join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, path).unwrap();
        if executable {
            fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    /// Every entry under the common dir, a folder as `PATH/`, a file with what it holds.
    fn tree(&self) -> BTreeMap<String, String> {
        fn walk(top: &Path, dir: &Path, found: &mut BTreeMap<String, String>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let relative = path.strip_prefix(top).unwrap().display().to_string();
                if path.is_dir() {
                    found.insert(format!("{relative}/"), String::new());
                    walk(top, &path, found);
                } else {
                    found.insert(relative, fs::read_to_string(&path).unwrap());
                }
            }
        }
        let mut found = BTreeMap::new();
        walk(&self.common(), &self.common(), &mut found);
        found
    }

    /// `bash tools.sh ARGS` on this fixture's common dir, Cargo the failing one.
    fn run(&self, args: &[&str]) -> Output {
        Command::new("/bin/bash")
            .arg(self.0.join("apps/symphony-ui/tools.sh"))
            .args(args)
            .env("SYMPHONY_GIT_COMMON_DIR", self.common())
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.0.join("bin").display()),
            )
            .env("TMPDIR", &self.0)
            .env_remove("CARGO_TARGET_DIR")
            .output()
            .unwrap()
    }

    /// The folder `tools.sh --folder` names, under the common dir.
    fn folder(&self) -> String {
        let done = self.run(&["--folder"]);
        assert!(done.status.success(), "{done:?}");
        assert_eq!(String::from_utf8(done.stderr).unwrap(), "");
        let named = String::from_utf8(done.stdout).unwrap();
        let common = format!("{}/", self.common().display());
        named
            .strip_suffix('\n')
            .and_then(|named| named.strip_prefix(&common))
            .unwrap_or_else(|| panic!("{named:?} is not a folder of {common}"))
            .to_string()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const DX: &str = "symphony/tools/dx-0.7.10/bin/dx";
const BINDGEN: &str = "symphony/tools/dx-home/tools/wasm-bindgen-fixture/wasm-bindgen";
const OLD_DX: &str = "sf-cargo/dx-0.7.10/bin/dx";
const OLD_BINDGEN: &str = "sf-cargo/dx-home/tools/wasm-bindgen-fixture/wasm-bindgen";

#[test]
fn a_repository_whose_state_moved_runs_its_own_tools_and_installs_what_they_lack() {
    // Both tools in symphony/tools: named, and nothing to do.
    let f = Fixture::new("own");
    f.put(DX, true);
    f.put(BINDGEN, true);
    assert_eq!(f.folder(), "symphony/tools");
    let before = f.tree();
    let done = f.run(&[]);
    assert!(done.status.success(), "{done:?}");
    assert_eq!(String::from_utf8(done.stderr).unwrap(), "");
    assert_eq!(f.tree(), before);

    // wasm-bindgen missing, and nothing of an older build's: its install starts, and fails here.
    let f = Fixture::new("own-partial");
    f.put(DX, true);
    assert_eq!(f.folder(), "symphony/tools");
    let done = f.run(&[]);
    let err = String::from_utf8(done.stderr).unwrap();
    assert!(!done.status.success(), "{err}");
    assert!(
        err.starts_with(&format!(
            "symphony-ui tools: building wasm-bindgen fixture into {}/{} (once per version)\n",
            f.common().display(),
            BINDGEN.strip_suffix("/wasm-bindgen").unwrap()
        )),
        "{err}"
    );

    // Neither, in an empty common dir: dx's install starts first.
    let f = Fixture::new("own-none");
    assert_eq!(f.folder(), "symphony/tools");
    let done = f.run(&[]);
    let err = String::from_utf8(done.stderr).unwrap();
    assert!(!done.status.success(), "{err}");
    assert!(
        err.starts_with(&format!(
            "symphony-ui tools: building dx 0.7.10 into {}/symphony/tools (once per machine)\n",
            f.common().display()
        )),
        "{err}"
    );
}

#[test]
fn an_older_builds_copies_run_where_they_stand_and_nothing_installs_beside_or_removes_them() {
    let f = Fixture::new("older");
    f.put(OLD_DX, true);
    f.put(OLD_BINDGEN, true);
    f.put("sf-cargo/dx-home/tools/esbuild-fixture/esbuild", true);
    f.put("sf-cargo/target/debug/keep", false);
    assert_eq!(f.folder(), "sf-cargo");
    let before = f.tree();
    let done = f.run(&[]);
    assert!(done.status.success(), "{done:?}");
    assert_eq!(String::from_utf8(done.stderr).unwrap(), "");
    assert_eq!(f.tree(), before, "nothing installed, nothing removed");

    // A whole install of the new code's beside them (made before this rule) is the one run; the
    // older copies stay for the move to settle.
    let f = Fixture::new("older-beside");
    f.put(OLD_DX, true);
    f.put(OLD_BINDGEN, true);
    f.put(DX, true);
    f.put(BINDGEN, true);
    assert_eq!(f.folder(), "symphony/tools");
    let before = f.tree();
    assert!(f.run(&[]).status.success());
    assert_eq!(f.tree(), before);
}

#[test]
fn beside_an_older_builds_copies_that_lack_a_tool_nothing_installs_and_the_move_is_named() {
    let refusal = |f: &Fixture, missing: &str| {
        format!(
            "symphony-ui tools: {}/sf-cargo holds an older build's copies of the client's build \
             tools, without {missing}, and a build of this code installs none beside them: move \
             this repository's state (tools/move-state.sh, while nothing builds) and build again\n",
            f.common().display()
        )
    };
    // wasm-bindgen at another version than the lock's, and dx not executable: each is lacking.
    for (case, dx, missing) in [
        ("older-bindgen", true, "wasm-bindgen fixture"),
        ("older-dx", false, "dx 0.7.10"),
    ] {
        let f = Fixture::new(case);
        f.put(OLD_DX, dx);
        f.put(
            "sf-cargo/dx-home/tools/wasm-bindgen-0.2.1/wasm-bindgen",
            true,
        );
        assert_eq!(f.folder(), "symphony/tools", "{case}");
        let before = f.tree();
        let done = f.run(&[]);
        assert_eq!(
            (done.status.code(), String::from_utf8(done.stderr).unwrap()),
            (Some(1), refusal(&f, missing)),
            "{case}"
        );
        assert_eq!(
            f.tree(),
            before,
            "{case}: nothing installed, nothing removed"
        );
    }

    // Either older folder alone says the state has not moved; a new install part way is not
    // finished beside it.
    let f = Fixture::new("older-home");
    f.put("sf-cargo/dx-home/tools/esbuild-fixture/esbuild", true);
    f.put(DX, true);
    let before = f.tree();
    let done = f.run(&[]);
    assert_eq!(
        (done.status.code(), String::from_utf8(done.stderr).unwrap()),
        (Some(1), refusal(&f, "dx 0.7.10"))
    );
    assert_eq!(f.tree(), before);
}

#[test]
fn an_argument_other_than_folder_is_refused() {
    let f = Fixture::new("usage");
    let done = f.run(&["--install"]);
    assert_eq!(
        (done.status.code(), String::from_utf8(done.stderr).unwrap()),
        (Some(2), "usage: tools.sh [--folder]\n".to_string())
    );
}
