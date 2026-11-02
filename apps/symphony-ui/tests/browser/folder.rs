//! Extraction acceptance on clean, owned repositories. Published binary artifacts and the
//! browser suite's real release bundle are setup; the committed-main pointer starts absent.
use super::{SHOWN, binaries, dist, owner_controls::ScratchBoard, restored::Native};
use serde_json::json;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Checkout {
    scratch: PathBuf,
    root: PathBuf,
}

fn git(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null");
    for key in [
        "GIT_DIR",
        "GIT_COMMON_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
    ] {
        command.env_remove(key);
    }
    command
}

fn passed(done: Output) -> String {
    assert!(
        done.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&done.stdout),
        String::from_utf8_lossy(&done.stderr)
    );
    String::from_utf8(done.stdout).unwrap()
}

impl Checkout {
    fn capture() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let scratch = std::env::temp_dir().join(format!(
            "sf-symphony-folder-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&scratch).unwrap();
        let scratch = scratch.canonicalize().unwrap();
        let root = scratch.join("clean checkout with spaces");
        fs::create_dir(&root).unwrap();
        let owned = Self { scratch, root };
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        // Capture current sources without changing the real index or any branch/ref. This
        // also works before the author's source changes have been committed.
        let index = owned.scratch.join("capture-index");
        passed(
            git(&source, &["read-tree", "HEAD"])
                .env("GIT_INDEX_FILE", &index)
                .output()
                .unwrap(),
        );
        passed(
            git(&source, &["add", "-A", "--", "."])
                .env("GIT_INDEX_FILE", &index)
                .output()
                .unwrap(),
        );
        let tree = passed(
            git(&source, &["write-tree"])
                .env("GIT_INDEX_FILE", &index)
                .output()
                .unwrap(),
        );
        let archive = owned.scratch.join("source.tar");
        let done = git(&source, &["archive", "--format=tar", tree.trim()])
            .stdout(fs::File::create(&archive).unwrap())
            .output()
            .unwrap();
        passed(done);
        passed(
            Command::new("tar")
                .arg("-xf")
                .arg(&archive)
                .arg("-C")
                .arg(&owned.root)
                .output()
                .unwrap(),
        );
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.name", "Extraction fixture"],
            vec!["config", "user.email", "fixture@example.invalid"],
            vec!["config", "core.hooksPath", "/dev/null"],
            vec!["add", "-A"],
            vec![
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-qm",
                "Clean source fixture",
            ],
        ] {
            passed(git(&owned.root, &args).output().unwrap());
        }
        assert!(
            passed(
                git(&owned.root, &["status", "--porcelain"])
                    .output()
                    .unwrap()
            )
            .is_empty()
        );
        owned
    }

    fn command(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
        let mut command = Command::new(program);
        command.current_dir(&self.root).stdin(Stdio::null());
        for key in [
            "GIT_DIR",
            "GIT_COMMON_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "SF_GIT_COMMON_DIR",
            "SF_CARGO_WORKSPACE",
            "SF_VERIFY_PARENT",
            "TASKS_SERVER",
            "TASKS_DATABASE",
            "TASKS_SESSION",
        ] {
            command.env_remove(key);
        }
        command
    }

    fn cache_artifact(&self, name: &str) -> PathBuf {
        let source = binaries::built(name);
        let folder = self.root.join(".git/sf-cargo/bin");
        fs::create_dir_all(&folder).unwrap();
        let artifact = folder.join(source.file_name().unwrap());
        // Published artifacts are immutable. Copy only the executable, no mutable build
        // folder or another repository's committed-main pointer.
        fs::copy(source, &artifact).unwrap();
        let launcher = self.root.join("tools/rs-bin.sh");
        let expected = passed(
            self.command(&launcher)
                .args(["--where", name])
                .output()
                .unwrap(),
        );
        assert_eq!(
            Path::new(expected.trim()),
            artifact,
            "source identity must survive the workspace export"
        );
        artifact
    }
}

impl Drop for Checkout {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.scratch);
    }
}

#[test]
fn a_cold_committed_main_pointer_exports_both_workspaces_and_lists_only_the_private_board() {
    let board = ScratchBoard::start();
    let actor = board.call_in_database("tasks", "register", json!({"kind":"worker"}));
    board.call_in_database(
        "tasks",
        "create",
        json!({"slug":"fixture/cold-launcher","title":"Cold exported board","by":actor["handle"]}),
    );
    let checkout = Checkout::capture();
    checkout.cache_artifact("tasks");
    let pointer = checkout.root.join(".git/sf-cargo/tasks-main/current");
    assert!(!pointer.exists());
    fs::create_dir_all(checkout.root.join(".git/sf-tasks")).unwrap();
    fs::write(
        checkout.root.join(".git/sf-tasks/server.json"),
        json!({"addr":board.address(),"pid":1,"version":"fixture"}).to_string(),
    )
    .unwrap();
    let done = checkout
        .command("bash")
        .arg(checkout.root.join("tools/tasks.sh"))
        .arg("list")
        .env("TASKS_SERVER", board.address())
        .env("TASKS_SWEEP", "off")
        .output()
        .unwrap();
    let listed = passed(done);
    assert!(listed.contains("fixture/cold-launcher"), "{listed}");
    assert!(listed.contains("Cold exported board"), "{listed}");
    assert!(
        pointer.is_file(),
        "the live-launcher path must publish its own main pointer"
    );
    assert!(
        passed(
            git(&checkout.root, &["status", "--porcelain"])
                .output()
                .unwrap()
        )
        .is_empty()
    );
}

#[test]
fn a_clean_checkout_builds_its_thin_host_and_serves_the_real_hashed_client_assets() {
    let public = dist();
    let checkout = Checkout::capture();
    let artifact = checkout.cache_artifact("symphony-server");
    let built = passed(
        checkout
            .command(checkout.root.join("tools/rs-bin.sh"))
            .args(["--build", "symphony-server"])
            .output()
            .unwrap(),
    );
    assert_eq!(Path::new(built.trim()), artifact);
    assert!(
        passed(
            git(&checkout.root, &["status", "--porcelain"])
                .output()
                .unwrap()
        )
        .is_empty()
    );
    let server = Native::checkout(checkout.root.clone(), &artifact);
    let address = server.url("/");
    let address = address
        .strip_prefix("http://")
        .unwrap()
        .trim_end_matches('/');
    let mut stream = TcpStream::connect(address).unwrap();
    stream.set_read_timeout(Some(SHOWN)).unwrap();
    stream
        .write_all(
            format!("GET / HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n").as_bytes(),
        )
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200 "), "{response}");
    let html = fs::read_to_string(public.join("index.html")).unwrap();
    let assets: Vec<_> = html
        .split("/assets/")
        .skip(1)
        .map(|tail| {
            format!(
                "/assets/{}",
                tail.split(['"', '\'', '<', '>', ' ']).next().unwrap()
            )
        })
        .collect();
    assert!(
        !assets.is_empty(),
        "the real release HTML must name its assets"
    );
    for asset in assets {
        assert!(
            response.contains(&asset),
            "served HTML does not match the release bundle: {asset}"
        );
        assert_eq!(server.raw_status(&asset), 200, "{asset}");
    }
    let wasm: Vec<_> = fs::read_dir(public.join("assets"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "wasm")
        })
        .collect();
    assert!(
        !wasm.is_empty(),
        "the shipped client must contain compiled wasm"
    );
    for asset in wasm {
        let name = asset.file_name().unwrap().to_str().unwrap();
        assert!(
            name.split(['-', '_', '.']).any(|part| {
                let hash = part.strip_prefix("dxh").unwrap_or(part);
                hash.len() >= 8 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            }),
            "unhashed asset {name}"
        );
        assert_eq!(server.raw_status(&format!("/assets/{name}")), 200, "{name}");
    }
}
