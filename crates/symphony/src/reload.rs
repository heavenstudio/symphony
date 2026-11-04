//! Main-checkout source tracking and staged server/client builds. Socket and HTTP lifetimes belong
//! to the server; client publication can be rolled back until its replacement is ready.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;
use tasks::sys::{Child, Command, Stdio};

const POLL: Duration = Duration::from_millis(50);
const BUILD_GRACE: Duration = Duration::from_secs(2);

/// Lock waits must observe shutdown too: a stopper may hold the startup lock while waiting for
/// this server to end. A kernel-blocking lock in the reload worker would deadlock that stop.
pub fn lock_until(file: &File, stopping: impl Fn() -> bool) -> Result<(), String> {
    loop {
        if stopping() {
            return Err("reload cancelled by shutdown".into());
        }
        match file.try_lock() {
            Ok(()) => return Ok(()),
            Err(TryLockError::WouldBlock) => std::thread::sleep(POLL),
            Err(TryLockError::Error(problem)) => return Err(problem.to_string()),
        }
    }
}

/// The launcher, cargo and rustc inherit one process group of this build's own. Keep its leader
/// unreaped through cancellation, so its pid cannot be recycled before the last group signal.
struct BuildProcess {
    child: Child,
    reaped: bool,
}

impl BuildProcess {
    fn cancel(&mut self) {
        if self.reaped {
            return;
        }
        let group = libc::pid_t::try_from(self.child.id()).expect("a native child pid fits pid_t");
        // SAFETY: process_group(0) made this owned child the leader. No other server or build
        // joins it. The unreaped leader reserves this group id throughout both signals.
        unsafe { libc::killpg(group, libc::SIGTERM) };
        std::thread::sleep(BUILD_GRACE);
        // The launcher may already have ended while a compiler ignores TERM. Kill the whole
        // owned group before reaping the leader, including children that inherited pipe/lock fds.
        unsafe { libc::killpg(group, libc::SIGKILL) };
        let _ = self.child.wait();
        self.reaped = true;
    }
}

impl Drop for BuildProcess {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let done = tasks::sys::output(
        Command::new("git")
            .args(args)
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_COMMON_DIR"),
    )
    .map_err(|e| e.to_string())?;
    if !done.status.success() {
        return Err(String::from_utf8_lossy(&done.stderr).trim().into());
    }
    Ok(String::from_utf8_lossy(&done.stdout).trim().into())
}

/// Reject a branch checkout even when somebody explicitly names it as a server's root.
pub fn main_only(root: &Path) -> Result<(), String> {
    let trees = git(root, &["worktree", "list", "--porcelain"])?;
    let main = trees
        .lines()
        .next()
        .and_then(|s| s.strip_prefix("worktree "));
    if main.map(Path::new).and_then(|p| p.canonicalize().ok()) != root.canonicalize().ok()
        || git(root, &["symbolic-ref", "--quiet", "--short", "HEAD"])?
            != crate::project::Project::of(root).default_branch
    {
        return Err(format!(
            "{} is not the main checkout on main",
            root.display()
        ));
    }
    Ok(())
}

fn files(path: &Path, into: &mut BTreeSet<PathBuf>) -> Result<(), String> {
    if path.is_dir() {
        for entry in fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))? {
            files(&entry.map_err(|e| e.to_string())?.path(), into)?;
        }
    } else if path.is_file() {
        into.insert(path.into());
    }
    Ok(())
}

fn crate_directories(root: &Path, app: &str) -> Result<BTreeSet<PathBuf>, String> {
    static PATHS: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"\bpath\s*=\s*["']([^"']+)["']"#).expect("a constant path expression")
    });
    let mut pending = vec![root.join(app)];
    let mut seen = BTreeSet::new();
    while let Some(folder) = pending.pop() {
        let folder = folder
            .canonicalize()
            .map_err(|e| format!("{}: {e}", folder.display()))?;
        if !folder.starts_with(root) || !seen.insert(folder.clone()) {
            continue;
        }
        let manifest = folder.join("Cargo.toml");
        let text =
            fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
        for capture in PATHS.captures_iter(&text) {
            let dependency = folder.join(&capture[1]);
            if dependency.join("Cargo.toml").is_file() {
                pending.push(dependency);
            }
        }
    }
    Ok(seen)
}

/// Both products and their transitive local crates, including path build dependencies.
pub fn source_directories(root: &Path) -> Result<BTreeSet<PathBuf>, String> {
    let mut directories = crate_directories(root, "symphony/apps/symphony-server")?;
    if root.join("apps/symphony-host/Cargo.toml").is_file() {
        directories.extend(crate_directories(root, "apps/symphony-host")?);
    }
    if root.join("symphony/apps/symphony-ui/Cargo.toml").is_file() {
        directories.extend(crate_directories(root, "symphony/apps/symphony-ui")?);
    }
    Ok(directories)
}

fn crate_inputs(
    directories: BTreeSet<PathBuf>,
    inputs: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    for folder in directories {
        inputs.insert(folder.join("Cargo.toml"));
        files(&folder.join("src"), inputs)?;
        files(&folder.join("build.rs"), inputs)?;
    }
    Ok(())
}

const CLIENT_INPUTS: &[&str] = &[
    "assets",
    "bin",
    "Dioxus.toml",
    "index.html",
    "bundle.sh",
    "package.sh",
    "tools.sh",
    "wasm-fresh.sh",
];

fn client_inputs(root: &Path, inputs: &mut BTreeSet<PathBuf>) -> Result<(), String> {
    if root.join("symphony/apps/symphony-ui/Cargo.toml").is_file() {
        crate_inputs(
            crate_directories(root, "symphony/apps/symphony-ui")?,
            inputs,
        )?;
    }
    for name in CLIENT_INPUTS {
        files(&root.join("symphony/apps/symphony-ui").join(name), inputs)?;
    }
    // The recipe and cargo launcher configure the release bundle as well as dx itself.
    for name in [
        "justfile",
        "tools/rs-bin.sh",
        "symphony/justfile",
        "symphony/tools/rs-bin.sh",
        "symphony/tools/check_lock.sh",
    ] {
        files(&root.join(name), inputs)?;
    }
    Ok(())
}

fn common_inputs(root: &Path) -> Result<BTreeSet<PathBuf>, String> {
    let mut inputs = BTreeSet::new();
    for name in [
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        ".cargo",
        "symphony/Cargo.toml",
        "symphony/Cargo.lock",
        "symphony/.cargo",
    ] {
        files(&root.join(name), &mut inputs)?;
    }
    Ok(inputs)
}

fn hash_files(root: &Path, inputs: BTreeSet<PathBuf>) -> Result<String, String> {
    let mut hash = Sha256::new();
    for path in inputs {
        let name = path
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_string_lossy();
        let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        hash.update(name.len().to_le_bytes());
        hash.update(name.as_bytes());
        hash.update(bytes.len().to_le_bytes());
        hash.update(bytes);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Hash both products' build inputs. Runtime documents belong to the read hub.
pub fn fingerprint(root: &Path) -> Result<String, String> {
    main_only(root)?;
    let mut inputs = common_inputs(root)?;
    crate_inputs(source_directories(root)?, &mut inputs)?;
    client_inputs(root, &mut inputs)?;
    hash_files(root, inputs)
}

/// Separate client identity lets a server-only reload reuse its installed release bundle.
pub fn client_fingerprint(root: &Path) -> Result<String, String> {
    main_only(root)?;
    let mut inputs = common_inputs(root)?;
    client_inputs(root, &mut inputs)?;
    hash_files(root, inputs)
}

const CLIENT_INSTALLED: &str = "client-installed.json";

#[derive(Serialize, Deserialize)]
struct ClientInstalled {
    source: String,
    distribution: String,
}

fn distribution(public: &Path) -> Result<String, String> {
    let mut inputs = BTreeSet::new();
    files(public, &mut inputs)?;
    if !public.join("index.html").is_file() {
        return Err(format!(
            "{}: client build has no index.html",
            public.display()
        ));
    }
    hash_files(public, inputs)
}

/// A missing or manually changed bundle cannot inherit another distribution's source stamp.
pub fn installed_client_source(home: &Path) -> Option<String> {
    let stamp: ClientInstalled =
        serde_json::from_slice(&fs::read(home.join(CLIENT_INSTALLED)).ok()?).ok()?;
    (distribution(&home.join("client/public")).ok()? == stamp.distribution).then_some(stamp.source)
}

fn copy_assets(from: &Path, to: &Path) -> Result<(), String> {
    if !from.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for entry in fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let dest = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_assets(&entry.path(), &dest)?;
        } else {
            fs::copy(entry.path(), &dest).map_err(|e| format!("{}: {e}", dest.display()))?;
        }
    }
    Ok(())
}

/// Own a scratch bundle and, after publication, the old bundle needed for startup rollback.
pub struct ClientUpdate {
    stage: PathBuf,
    home: PathBuf,
    stamp: ClientInstalled,
    previous_stamp: Option<Vec<u8>>,
    published: bool,
    committed: bool,
}

impl ClientUpdate {
    pub fn publish(&mut self) -> Result<(), String> {
        let public = self.home.join("client/public");
        fs::create_dir_all(public.parent().expect("public has a parent"))
            .map_err(|e| e.to_string())?;
        crate::cli::publish_client(&self.stage.join("public"), &public)
            .map_err(|e| format!("could not publish client {}: {e}", public.display()))?;
        self.published = true;
        Ok(())
    }

    /// Called only after the replacement is ready. An unstamped successful install is safe:
    /// the next reload will rebuild it rather than assume that an unknown bundle is current.
    pub fn stamp(&self) -> Result<(), String> {
        let next = self
            .home
            .join(format!("{CLIENT_INSTALLED}.{}", std::process::id()));
        let bytes = serde_json::to_vec(&self.stamp).map_err(|e| e.to_string())?;
        fs::write(&next, bytes)
            .and_then(|()| fs::rename(&next, self.home.join(CLIENT_INSTALLED)))
            .map_err(|e| format!("{}: {e}", next.display()))
    }

    pub fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for ClientUpdate {
    fn drop(&mut self) {
        if self.published && !self.committed {
            let public = self.home.join("client/public");
            let previous = self.stage.join("public");
            let result = if previous.exists() {
                crate::cli::publish_client(&previous, &public)
            } else {
                fs::rename(&public, &previous)
            };
            if let Err(why) = result {
                eprintln!("· Symphony reload: could not restore previous client: {why}");
                return; // Retain the old bundle for recovery.
            }
            let stamp = self.home.join(CLIENT_INSTALLED);
            let restored = match &self.previous_stamp {
                Some(bytes) => fs::write(&stamp, bytes),
                None if stamp.exists() => fs::remove_file(&stamp),
                None => Ok(()),
            };
            if let Err(why) = restored {
                eprintln!("· Symphony reload: could not restore previous client stamp: {why}");
            }
        }
        let _ = fs::remove_dir_all(&self.stage);
    }
}

/// Serialize builds/installations separately from the startup lock (a server keeps serving while
/// cargo holds its shared build-folder lock). All callers use main's wrapper and main's sources.
pub fn build(
    root: &Path,
    home: &Path,
    stopping: impl Fn() -> bool,
) -> Result<Option<ClientUpdate>, String> {
    main_only(root)?;
    let lock = home.join("reload-build.lock");
    let held: File = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&lock)
        .map_err(|e| format!("{}: {e}", lock.display()))?;
    lock_until(&held, &stopping).map_err(|e| format!("{}: {e}", lock.display()))?;
    main_only(root)?;
    if stopping() {
        return Err("reload cancelled by shutdown".into());
    }
    let source = client_fingerprint(root)?;
    let client = if installed_client_source(home).as_deref() != Some(&source) {
        let stage = home.join(format!(".reload-client.{}", std::process::id()));
        // Only this reload worker owns its staging directory, including after a failed attempt.
        fs::create_dir(&stage).map_err(|e| format!("{}: {e}", stage.display()))?;
        let mut client = ClientUpdate {
            stage,
            home: home.into(),
            stamp: ClientInstalled {
                source,
                distribution: String::new(),
            },
            previous_stamp: fs::read(home.join(CLIENT_INSTALLED)).ok(),
            published: false,
            committed: false,
        };
        // Existing tabs can request an older module after the new index has been published.
        copy_assets(
            &home.join("client/public/assets"),
            &client.stage.join("public/assets"),
        )?;
        run_build(
            client_build(&client.stage),
            root,
            "Symphony client",
            &stopping,
        )?;
        client.stamp.distribution = distribution(&client.stage.join("public"))?;
        Some(client)
    } else {
        None
    };
    run_build(server_build(root, home), root, "symphony-server", stopping)?;
    Ok(client)
}

/// The client's release bundle, written under `stage` by main's recipe.
fn client_build(stage: &Path) -> Command {
    let mut command = Command::new("just");
    command.arg("symphony-ui-dist").arg(stage);
    command
}

/// The server binary, installed under `home` by main's cargo wrapper.
fn server_build(root: &Path, home: &Path) -> Command {
    let mut command = Command::new(root.join("tools/rs-bin.sh"));
    command
        .args([
            "sf-slots",
            "cargo",
            "install",
            "--quiet",
            "--locked",
            "--force",
            "--path",
            if root.join("apps/symphony-host/Cargo.toml").is_file() {
                "apps/symphony-host"
            } else {
                "symphony/apps/symphony-server"
            },
            "--bin",
            "symphony-server",
            "--root",
        ])
        .arg(home);
    command
}

/// Run one build from `root` in a process group of its own, its diagnostics the error when it fails.
/// It starts without a fork and inherits only its streams (tasks::sys::Command): macOS's accept
/// marks a connection close-on-exec in a second call, and a build started between the two would
/// hold that client's connection open for its whole run.
fn run_build(
    mut command: Command,
    root: &Path,
    product: &str,
    stopping: impl Fn() -> bool,
) -> Result<(), String> {
    // Read to its end once the build has ended: a child another thread spawns meanwhile must not
    // hold its writer, so it is made close-on-exec (tasks::sys::pipe) and only the build gets it.
    let (mut stderr, writer) =
        tasks::sys::pipe().map_err(|e| format!("could not build {product}: {e}"))?;
    let child = command
        .current_dir(root)
        .env_remove("TASKS_SESSION")
        .env_remove("SF_CONTENT_DIR")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(writer)
        .process_group(0)
        .spawn()
        .map_err(|e| format!("could not build {product}: {e}"))?;
    let mut process = BuildProcess {
        child,
        reaped: false,
    };
    eprintln!(
        "· Symphony reload: {product} build pid {} started",
        process.child.id()
    );
    // Drain concurrently: compiler diagnostics can exceed a pipe's capacity while we wait.
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).map(|_| bytes)
    });
    let result = loop {
        if stopping() {
            process.cancel();
            break Err("reload cancelled by shutdown".to_string());
        }
        match process.child.try_wait() {
            Ok(Some(status)) => {
                process.reaped = true;
                break Ok(status);
            }
            Ok(None) => std::thread::sleep(POLL),
            Err(problem) => {
                process.cancel();
                break Err(format!("could not wait for Symphony build: {problem}"));
            }
        }
    };
    let stderr = reader
        .join()
        .map_err(|_| "build diagnostic reader panicked")?
        .map_err(|e| format!("could not read build diagnostics: {e}"))?;
    let status = result?;
    if !status.success() {
        return Err(format!(
            "{product} build {}: {}",
            status,
            String::from_utf8_lossy(&stderr)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        ));
    }
    if stopping() {
        return Err("reload cancelled by shutdown".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "reload/build_tests.rs"]
mod build_tests;
