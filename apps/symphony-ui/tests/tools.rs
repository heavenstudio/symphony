//! `tools.sh`, which installs the client's build tools into `symphony/tools/` in the git common dir
//! and names the folder a build runs them from. Each case is a folder of its own under the temp
//! folder, its common dir named by `SYMPHONY_GIT_COMMON_DIR`, so nothing here reads a real
//! repository's.
//!
//! `dx-path.sh` keeps the installed dx at the one path every release build runs it from, a folder
//! of each case's own standing for `/private/tmp/symphony-client-tools` (`SYMPHONY_CLIENT_TOOLS`).
//!
//! No freshly written file is executed: the installed tools are only tested for being there, and
//! the one Cargo, or `ln`, a case runs is a link to the system's `false`.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::SystemTime;

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

#[test]
fn a_repository_runs_its_own_tools_and_installs_what_they_lack() {
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

    // wasm-bindgen missing: its install starts, and fails here.
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
fn an_argument_other_than_folder_is_refused() {
    let f = Fixture::new("usage");
    let done = f.run(&["--install"]);
    assert_eq!(
        (done.status.code(), String::from_utf8(done.stderr).unwrap()),
        (Some(2), "usage: tools.sh [--folder]\n".to_string())
    );
}

/// What `dx-path.sh` is given below the tools' folder, as bundle.sh gives it.
const DX_NAME: &str = "dx-0.7.10/bin/dx";

/// A case of `dx-path.sh`: the script, a tools folder holding the installed dx, and the stand, the
/// folder `SYMPHONY_CLIENT_TOOLS` names in place of `/private/tmp/symphony-client-tools`.
struct Stand(PathBuf);

impl Stand {
    fn new(case: &str) -> Stand {
        let base =
            std::env::temp_dir().join(format!("symphony-ui-dx-path {case}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("tools")).unwrap();
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("dx-path.sh"),
            base.join("dx-path.sh"),
        )
        .unwrap();
        fs::create_dir(base.join("bin")).unwrap();
        Stand(base)
    }

    fn installed(&self) -> PathBuf {
        self.0.join("tools").join(DX_NAME)
    }

    fn stand(&self) -> PathBuf {
        self.0.join("stand")
    }

    /// Where the stand's dx lies, by the canonical path dx would find itself at.
    fn placed(&self) -> PathBuf {
        self.stand().canonicalize().unwrap().join(DX_NAME)
    }

    /// The installed dx, a new file holding `bytes`: another install's when it held others.
    fn install(&self, bytes: &str) {
        let dx = self.installed();
        fs::create_dir_all(dx.parent().unwrap()).unwrap();
        let _ = fs::remove_file(&dx);
        fs::write(&dx, bytes).unwrap();
        fs::set_permissions(&dx, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// `bash dx-path.sh ARGS`, the stand named, the system's tools on its PATH after this case's
    /// `bin/`.
    fn run_with(&self, args: &[&str]) -> Output {
        Command::new("/bin/bash")
            .arg(self.0.join("dx-path.sh"))
            .args(args)
            .env("SYMPHONY_CLIENT_TOOLS", self.stand())
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.0.join("bin").display()),
            )
            .output()
            .unwrap()
    }

    fn run(&self) -> Output {
        let tools = self.0.join("tools");
        self.run_with(&[tools.to_str().unwrap(), DX_NAME])
    }

    /// A run that names the stand's dx and says nothing else.
    fn runs_placed(&self) {
        let done = self.run();
        assert_eq!(
            (
                done.status.code(),
                String::from_utf8(done.stdout).unwrap(),
                String::from_utf8(done.stderr).unwrap()
            ),
            (
                Some(0),
                format!("{}\n", self.placed().display()),
                String::new()
            )
        );
    }

    /// A run refused with `said`, exit 2, naming nothing.
    fn refused(&self, args: Option<&[&str]>, said: &str) {
        let done = match args {
            Some(args) => self.run_with(args),
            None => self.run(),
        };
        assert_eq!(
            (
                done.status.code(),
                String::from_utf8(done.stdout).unwrap(),
                String::from_utf8(done.stderr).unwrap()
            ),
            (
                Some(2),
                String::new(),
                format!("symphony-ui dx path: {said}\n")
            )
        );
    }

    /// What the stand's dx folder holds, each name with its inode, and when the folder last changed.
    fn shelf(&self) -> (Vec<(String, u64)>, SystemTime) {
        let folder = self.placed().parent().unwrap().to_owned();
        let mut entries: Vec<(String, u64)> = fs::read_dir(&folder)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    entry.metadata().unwrap().ino(),
                )
            })
            .collect();
        entries.sort();
        (entries, fs::metadata(&folder).unwrap().modified().unwrap())
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn ino(path: &Path) -> u64 {
    fs::symlink_metadata(path).unwrap().ino()
}

#[test]
fn the_installed_dx_stands_at_one_private_path_as_its_own_file_and_a_second_run_writes_nothing() {
    let s = Stand::new("link");
    s.install("dx one");
    s.runs_placed();
    let stand = fs::symlink_metadata(s.stand()).unwrap();
    assert_eq!(
        stand.permissions().mode() & 0o777,
        0o700,
        "a private folder"
    );
    assert_eq!(
        ino(&s.placed()),
        ino(&s.installed()),
        "a hard link of the install"
    );
    let shelf = s.shelf();
    assert_eq!(
        shelf.0,
        [("dx".to_string(), ino(&s.installed()))],
        "nothing staged is left"
    );
    s.runs_placed();
    assert_eq!(s.shelf(), shelf, "the same file found, nothing written");
}

#[test]
fn another_install_or_a_link_where_dx_stands_is_replaced_whole_and_a_copy_of_its_bytes_kept() {
    let s = Stand::new("replace");
    s.install("dx one");
    s.runs_placed();
    // Another install: its file stands there in place of the first's.
    s.install("dx two");
    s.runs_placed();
    assert_eq!(ino(&s.placed()), ino(&s.installed()));
    assert_eq!(fs::read_to_string(s.placed()).unwrap(), "dx two");
    // A link to the install, which dx would follow back to it: replaced by the install's file.
    fs::remove_file(s.placed()).unwrap();
    std::os::unix::fs::symlink(s.installed(), s.placed()).unwrap();
    s.runs_placed();
    assert!(fs::symlink_metadata(s.placed()).unwrap().is_file());
    assert_eq!(ino(&s.placed()), ino(&s.installed()));
    // A link to a folder, which a move would go into: replaced the same way.
    fs::remove_file(s.placed()).unwrap();
    std::os::unix::fs::symlink(s.stand(), s.placed()).unwrap();
    s.runs_placed();
    assert_eq!(ino(&s.placed()), ino(&s.installed()));
    assert_eq!(s.shelf().0.len(), 1);
    // Another file of the same bytes (a copy-on-write clone's install): kept as it is.
    fs::remove_file(s.placed()).unwrap();
    fs::write(s.placed(), "dx two").unwrap();
    fs::set_permissions(s.placed(), fs::Permissions::from_mode(0o755)).unwrap();
    let shelf = s.shelf();
    s.runs_placed();
    assert_eq!(s.shelf(), shelf, "the same bytes found, nothing written");
    assert_ne!(ino(&s.placed()), ino(&s.installed()));
    // The same bytes, not executable: replaced.
    fs::set_permissions(s.placed(), fs::Permissions::from_mode(0o644)).unwrap();
    s.runs_placed();
    assert_eq!(ino(&s.placed()), ino(&s.installed()));
}

#[test]
fn where_no_link_can_be_made_a_copy_stands_and_stays_while_it_holds_the_installs_bytes() {
    let s = Stand::new("copy");
    std::os::unix::fs::symlink("/usr/bin/false", s.0.join("bin/ln")).unwrap();
    s.install("dx one");
    s.runs_placed();
    let placed = fs::symlink_metadata(s.placed()).unwrap();
    assert!(placed.is_file());
    assert_ne!(placed.ino(), ino(&s.installed()), "a copy, no link");
    assert_eq!(placed.permissions().mode() & 0o777, 0o755);
    assert_eq!(fs::read_to_string(s.placed()).unwrap(), "dx one");
    let shelf = s.shelf();
    assert_eq!(shelf.0.len(), 1, "nothing staged is left");
    s.runs_placed();
    assert_eq!(s.shelf(), shelf, "its bytes found, nothing written");
    s.install("dx two");
    s.runs_placed();
    assert_eq!(fs::read_to_string(s.placed()).unwrap(), "dx two");
}

#[test]
fn a_stand_others_may_write_or_an_install_not_there_is_refused_and_nothing_placed() {
    // A stand others may write, or a link to a private one: no dx is put there.
    let s = Stand::new("refused");
    s.install("dx one");
    fs::create_dir(s.stand()).unwrap();
    fs::set_permissions(s.stand(), fs::Permissions::from_mode(0o755)).unwrap();
    let open = format!(
        "{} is not a private folder of this user's",
        s.stand().display()
    );
    s.refused(None, &open);
    assert_eq!(fs::read_dir(s.stand()).unwrap().count(), 0);
    fs::remove_dir(s.stand()).unwrap();
    let private = s.0.join("private");
    fs::create_dir(&private).unwrap();
    fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).unwrap();
    std::os::unix::fs::symlink(&private, s.stand()).unwrap();
    s.refused(None, &open);
    assert_eq!(fs::read_dir(&private).unwrap().count(), 0);
    fs::remove_file(s.stand()).unwrap();

    // A folder where dx would stand.
    fs::create_dir_all(s.stand().join(DX_NAME)).unwrap();
    fs::set_permissions(s.stand(), fs::Permissions::from_mode(0o700)).unwrap();
    s.refused(None, &format!("{} is no file", s.placed().display()));
    fs::remove_dir_all(s.stand()).unwrap();

    // No installed dx, or one not executable; a name outside the tools' folder; no name.
    let tools = s.0.join("tools");
    let tools = tools.to_str().unwrap();
    fs::set_permissions(s.installed(), fs::Permissions::from_mode(0o644)).unwrap();
    let none = format!("no executable {tools}/{DX_NAME}");
    s.refused(None, &none);
    fs::remove_file(s.installed()).unwrap();
    s.refused(None, &none);
    for name in ["../dx", "/dx", "dx-0.7.10//dx", "./dx", ""] {
        s.refused(
            Some(&[tools, name]),
            &format!("NAME must be a path inside TOOLS: {name}"),
        );
    }
    assert!(!s.stand().exists(), "nothing placed");
    let done = s.run_with(&[tools]);
    assert_eq!(
        (done.status.code(), String::from_utf8(done.stderr).unwrap()),
        (Some(2), "usage: dx-path.sh TOOLS NAME\n".to_string())
    );
}
