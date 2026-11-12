//! `tools.sh`, which installs the client's build tools into `symphony/tools/` in the git common dir,
//! removing the copies an older build kept in `sf-cargo/` once both tools are installed. Each case
//! is a folder of its own under the temp folder, its common dir named by `SYMPHONY_GIT_COMMON_DIR`,
//! so nothing here reads a real repository's.
//!
//! No freshly written file is executed: the installed tools are only tested for being there, and
//! the one Cargo a case runs is a link to the system's `false`.

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

    fn exists(&self, path: &str) -> bool {
        fs::symlink_metadata(self.common().join(path)).is_ok()
    }

    /// `bash tools.sh` on this fixture's common dir, Cargo the failing one.
    fn run(&self) -> Output {
        Command::new("/bin/bash")
            .arg(self.0.join("apps/symphony-ui/tools.sh"))
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
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const DX: &str = "symphony/tools/dx-0.7.10/bin/dx";
const BINDGEN: &str = "symphony/tools/dx-home/tools/wasm-bindgen-fixture/wasm-bindgen";
const OLD: [&str; 2] = [
    "sf-cargo/dx-0.7.10/bin/dx",
    "sf-cargo/dx-home/tools/esbuild/esbuild",
];

#[test]
fn an_install_whole_removes_the_older_builds_copies_and_keeps_the_rest_of_their_folder() {
    let f = Fixture::new("whole");
    f.put(DX, true);
    f.put(BINDGEN, true);
    for old in OLD {
        f.put(old, false);
    }
    f.put("sf-cargo/target/debug/keep", false);
    let done = f.run();
    let err = String::from_utf8(done.stderr).unwrap();
    assert!(done.status.success(), "{err}");
    let common = f.common();
    assert_eq!(
        err,
        format!(
            "symphony-ui tools: removed {0}/sf-cargo/dx-0.7.10, an older build's copy of these \
             tools\nsymphony-ui tools: removed {0}/sf-cargo/dx-home, an older build's copy of \
             these tools\n",
            common.display()
        )
    );
    assert!(!f.exists("sf-cargo/dx-0.7.10") && !f.exists("sf-cargo/dx-home"));
    assert!(f.exists("sf-cargo/target/debug/keep"), "the project's own");
    assert!(f.exists(DX) && f.exists(BINDGEN));

    // With nothing else in it, the old folder goes too; a run with none to remove says nothing.
    let f = Fixture::new("whole-alone");
    f.put(DX, true);
    f.put(BINDGEN, true);
    for old in OLD {
        f.put(old, false);
    }
    assert!(f.run().status.success());
    assert!(!f.exists("sf-cargo"));
    let again = f.run();
    assert!(again.status.success());
    assert_eq!(String::from_utf8(again.stderr).unwrap(), "");
}

#[test]
fn an_install_that_fails_keeps_the_older_builds_copies() {
    let f = Fixture::new("partial");
    // dx is here; wasm-bindgen is not, and its build fails.
    f.put(DX, true);
    for old in OLD {
        f.put(old, false);
    }
    let done = f.run();
    let err = String::from_utf8(done.stderr).unwrap();
    assert!(!done.status.success(), "{err}");
    assert!(
        err.contains("symphony-ui tools: building wasm-bindgen fixture into"),
        "{err}"
    );
    assert!(!err.contains("removed"), "{err}");
    for old in OLD {
        assert!(f.exists(old), "{old}");
    }
}
