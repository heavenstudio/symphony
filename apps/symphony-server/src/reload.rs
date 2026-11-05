//! Listening sockets and the server lease survive exec; accepted connections stay with the old
//! generation. A private readiness pipe makes a failed replacement harmless to the serving one.

use crate::backstop::{self, Backstop, Wake};
use crate::cli::Serve;
use crate::front::Shared;
use crate::home::{Home, Running};
use notify::{RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{PipeReader, PipeWriter, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};
use tasks::sys::{Command, Stdio};

const INHERITED: &str = "SF_SYMPHONY_HANDOVER";
const PATIENCE: Duration = Duration::from_secs(120);
const INSTALLED: &str = "installed.json";

#[derive(Serialize, Deserialize)]
struct Installed {
    source: String,
    binary: String,
}

fn binary_fingerprint(path: &std::path::Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let size = file.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        hash.update(&buffer[..size]);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// A stamp belongs to executable bytes, not their inode or mtime: cargo may install an unchanged
/// binary again. A manual replacement or an upgrade invalidates the old stamp safely.
fn installed_source(serve: &Serve) -> Option<String> {
    let home = serve.home.as_ref()?;
    let binary = home.join("bin/symphony-server");
    if std::env::current_exe().ok()?.canonicalize().ok()? != binary.canonicalize().ok()? {
        return None;
    }
    let installed: Installed =
        serde_json::from_slice(&fs::read(home.join(INSTALLED)).ok()?).ok()?;
    if installed.binary != binary_fingerprint(&binary).ok()? {
        return None;
    }
    // A current server stamp alone cannot certify an older (or manually replaced) client.
    // Transferred generations bypass this initial-install lookup: their parent just published
    // the bundle, even if writing its optional recovery stamp failed.
    (symphony::reload::installed_client_source(home)?
        == symphony::reload::client_fingerprint(&serve.root).ok()?)
    .then_some(installed.source)
}

fn stamp_install(home: &std::path::Path, source: &str) -> std::io::Result<()> {
    let installed = Installed {
        source: source.into(),
        binary: binary_fingerprint(&home.join("bin/symphony-server"))?,
    };
    let bytes = serde_json::to_vec(&installed).map_err(std::io::Error::other)?;
    let next = home.join(format!("{INSTALLED}.{}", std::process::id()));
    let result = fs::write(&next, bytes).and_then(|()| fs::rename(&next, home.join(INSTALLED)));
    if result.is_err() {
        let _ = fs::remove_file(next);
    }
    result
}

#[derive(Serialize, Deserialize)]
struct Transfer {
    parent: i64,
    listeners: Vec<RawFd>,
    lease: RawFd,
    /// The replacement's ends of the handover's two pipes: it says it is ready on `ready` and reads
    /// the commit from `commit`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ready: Option<RawFd>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    commit: Option<RawFd>,
    /// What a generation from before those pipes sends instead: one socket pair for both.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    control: Option<RawFd>,
    fingerprint: String,
}

impl Transfer {
    /// The replacement's handover descriptors: its two pipe ends, or the older generation's socket.
    fn ends(&self) -> Result<Vec<RawFd>, String> {
        match (self.ready, self.commit, self.control) {
            (Some(ready), Some(commit), None) => Ok(vec![ready, commit]),
            (None, None, Some(control)) => Ok(vec![control]),
            _ => Err("invalid inherited server descriptors".into()),
        }
    }
}

/// The replacement's ends of a handover. The parent sees the end of `ready` when the replacement
/// ends, and the replacement the end of `commit` when the parent does: each pipe is made
/// close-on-exec ([`tasks::sys::pipe`]), so no child another thread spawns meanwhile keeps an end
/// and masks either death until [`PATIENCE`].
#[derive(Debug)]
struct Control {
    ready: PipeWriter,
    commit: PipeReader,
}

impl Control {
    /// The parent's ends (readiness in, commit out) and the replacement's of a new handover.
    fn pair() -> std::io::Result<((PipeReader, PipeWriter), Self)> {
        let (ready, replacement_ready) = tasks::sys::pipe()?;
        let (replacement_commit, commit) = tasks::sys::pipe()?;
        Ok((
            (ready, commit),
            Self {
                ready: replacement_ready,
                commit: replacement_commit,
            },
        ))
    }

    /// An older generation's one socket pair end, which answers both ways: a copy for each.
    fn socket(socket: OwnedFd) -> std::io::Result<Self> {
        Ok(Self {
            ready: socket.try_clone()?.into(),
            commit: socket.into(),
        })
    }
}

/// What the other generation said on a handover pipe.
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    Byte(u8),
    /// The end of the pipe: the other generation is gone, since no one else holds its end.
    Ended,
    /// Asked to stop first.
    Stopped,
    /// Nothing within [`PATIENCE`].
    Outlasted,
}

/// The first [`Answer`] on `from`, read without blocking, checking `stopping` between reads.
fn answer(from: &PipeReader, stopping: impl Fn() -> bool) -> std::io::Result<Answer> {
    tasks::sys::set_nonblocking(from, true)?;
    let began = Instant::now();
    loop {
        if stopping() {
            return Ok(Answer::Stopped);
        }
        let mut byte = [0];
        let mut input = from;
        match input.read(&mut byte) {
            Ok(0) => return Ok(Answer::Ended),
            Ok(_) => return Ok(Answer::Byte(byte[0])),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if began.elapsed() >= PATIENCE {
                    return Ok(Answer::Outlasted);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(e),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Setup {
    pub listeners: Vec<tokio::net::TcpListener>,
    pub lease: Option<File>,
    control: Option<Control>,
    fingerprint: Option<String>,
}

fn cloexec(fd: RawFd, on: bool) -> std::io::Result<()> {
    // SAFETY: fcntl does not dereference a pointer; callers retain ownership of this descriptor.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0
        || unsafe {
            libc::fcntl(
                fd,
                libc::F_SETFD,
                if on {
                    flags | libc::FD_CLOEXEC
                } else {
                    flags & !libc::FD_CLOEXEC
                },
            )
        } < 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Called inside the runtime, before binding. Only the direct child of the transfer's sender may
/// consume it; build tools and other descendants cannot reuse stale descriptor numbers.
pub(crate) fn setup(serve: &Serve) -> Result<Setup, String> {
    let transfer = std::env::var(INHERITED)
        .ok()
        .map(|text| serde_json::from_str::<Transfer>(&text).map_err(|e| e.to_string()))
        .transpose()?
        .filter(|t| t.parent == crate::sys::parent());
    if let Some(transfer) = transfer {
        if serve.home.is_none() {
            return Err("socket handover requires a shared server home".into());
        }
        let ends = transfer.ends()?;
        let all: Vec<_> = transfer
            .listeners
            .iter()
            .copied()
            .chain([transfer.lease])
            .chain(ends.iter().copied())
            .collect();
        let unique: std::collections::BTreeSet<_> = all.iter().copied().collect();
        if unique.len() != all.len()
            || transfer.listeners.is_empty()
            || all.iter().any(|&fd| fd < 3)
        {
            return Err("invalid inherited server descriptors".into());
        }
        for fd in all {
            cloexec(fd, true).map_err(|e| e.to_string())?;
        }
        // SAFETY: these unique, live descriptors were made inheritable only in our parent's
        // pre_exec; this is their first owner in this process. They close on all subsequent execs.
        let lease = unsafe { File::from_raw_fd(transfer.lease) };
        let control = if let [ready, commit] = ends[..] {
            // SAFETY: the same unique transfer ownership, one pipe end per descriptor.
            unsafe {
                Control {
                    ready: PipeWriter::from_raw_fd(ready),
                    commit: PipeReader::from_raw_fd(commit),
                }
            }
        } else {
            // SAFETY: the same unique transfer ownership of the older generation's one socket.
            Control::socket(unsafe { OwnedFd::from_raw_fd(ends[0]) }).map_err(|e| e.to_string())?
        };
        let mut listeners = Vec::new();
        for fd in transfer.listeners {
            // SAFETY: same unique transfer ownership as above, one listening socket per fd.
            let listener = unsafe { std::net::TcpListener::from_raw_fd(fd) };
            listener.set_nonblocking(true).map_err(|e| e.to_string())?;
            listeners.push(tokio::net::TcpListener::from_std(listener).map_err(|e| e.to_string())?);
        }
        return Ok(Setup {
            listeners,
            lease: Some(lease),
            control: Some(control),
            fingerprint: Some(transfer.fingerprint),
        });
    }
    let lease = serve
        .home
        .as_ref()
        .map(|home| {
            std::fs::create_dir_all(home).map_err(|e| e.to_string())?;
            let path = home.join("server.lock");
            let lease = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|e| e.to_string())?;
            lease
                .try_lock()
                .map_err(|e| format!("{} is held by another server: {e}", path.display()))?;
            Ok::<_, String>(lease)
        })
        .transpose()?;
    Ok(Setup {
        listeners: Vec::new(),
        lease,
        control: None,
        fingerprint: installed_source(serve),
    })
}

impl Setup {
    pub fn is_replacement(&self) -> bool {
        self.control.is_some()
    }

    /// Listeners are already accepting and the read hub is warm when this is called. Wait for the
    /// parent's publication before watching sources, so generations cannot race to replace us.
    pub fn ready(&mut self) -> Result<(), String> {
        if let Some(mut control) = self.control.take() {
            control.ready.write_all(b"R").map_err(|e| e.to_string())?;
            match answer(&control.commit, || false) {
                Ok(Answer::Byte(b'C')) => {}
                // A parent that died after we became ready leaves us serving the sockets. The
                // next starter can recover our record from this process's serve command line.
                Ok(Answer::Ended) => {}
                result => return Err(format!("handover was not committed: {result:?}")),
            }
        }
        Ok(())
    }
}

fn replace(
    serve: &Serve,
    sockets: &[OwnedFd],
    lease: &File,
    fingerprint: &str,
    shared: &Shared,
    mut client: Option<symphony::reload::ClientUpdate>,
) -> Result<u32, String> {
    let home = serve.home.as_deref().ok_or("no server home")?;
    let exe = home.join("bin/symphony-server");
    let home = Home::new(home, &exe, &serve.root, serve.port, serve.hosts.clone());
    // Serialize publication with starters and --stop, but never hold this during the build.
    let _held = home
        .lock_until(|| shared.stopping())
        .map_err(|e| e.to_string())?;
    // The replacement's first requests must see its release client. If startup fails, the owned
    // update rolls this exchange back while the original generation keeps accepting requests.
    if let Some(client) = &mut client {
        client.publish()?;
    }
    let ((ready, mut commit), replacement_ends) = Control::pair().map_err(|e| e.to_string())?;
    let transfer = Transfer {
        parent: i64::from(std::process::id()),
        listeners: sockets.iter().map(AsRawFd::as_raw_fd).collect(),
        lease: lease.as_raw_fd(),
        ready: Some(replacement_ends.ready.as_raw_fd()),
        commit: Some(replacement_ends.commit.as_raw_fd()),
        control: None,
        fingerprint: fingerprint.into(),
    };
    let fds: Vec<_> = transfer
        .listeners
        .iter()
        .copied()
        .chain([transfer.lease])
        .chain(transfer.ready)
        .chain(transfer.commit)
        .collect();
    let mut command = Command::new(exe);
    command
        .arg("serve")
        .arg("--home")
        .arg(home.dir())
        .arg("--root")
        .arg(&serve.root)
        .arg("--port")
        .arg(serve.port.to_string());
    for host in &serve.hosts {
        command.arg("--host").arg(host);
    }
    command.current_dir(&serve.root).stdin(Stdio::null()).env(
        INHERITED,
        serde_json::to_string(&transfer).map_err(|e| e.to_string())?,
    );
    if let Some((stdout, stderr)) =
        crate::log::output_for_reload().map_err(|e| format!("cannot inherit server output: {e}"))?
    {
        command.stdout(stdout).stderr(stderr);
    }
    // A session of its own, handed these descriptors at their numbers. The parent's copies stay
    // close-on-exec, including during concurrent builds and board commands.
    command.new_session();
    for fd in fds {
        command.pass_fd(fd);
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    // Only the replacement holds its ends now: their end is its death.
    drop(replacement_ends);
    let result = match answer(&ready, || shared.stopping()) {
        Ok(Answer::Byte(b'R')) => Ok(()),
        Ok(Answer::Byte(_)) => Err("replacement sent invalid readiness".into()),
        Ok(Answer::Ended) => Err("replacement did not become ready: it ended".into()),
        Ok(Answer::Stopped) => Err("reload cancelled by shutdown".into()),
        Ok(Answer::Outlasted) => Err("replacement did not become ready within 120 s".into()),
        Err(e) => Err(format!("replacement did not become ready: {e}")),
    }
    .and_then(|()| {
        if let Err(why) = stamp_install(home.dir(), fingerprint) {
            eprintln!("· Symphony reload: could not stamp installed binary: {why}");
        }
        home.record(&Running {
            port: serve.port,
            pid: i64::from(child.id()),
            root: serve.root.display().to_string(),
        })
        .map_err(|e| e.to_string())?;
        if let Some(client) = &client
            && let Err(why) = client.stamp()
        {
            eprintln!("· Symphony reload: could not stamp installed client: {why}");
        }
        commit.write_all(b"C").map_err(|e| e.to_string())
    });
    if let Err(why) = result {
        let _ = child.kill();
        let _ = child.wait();
        // The old server is still accepting. Restore its publication if the child failed after it.
        let _ = home.record(&Running {
            port: serve.port,
            pid: i64::from(std::process::id()),
            root: serve.root.display().to_string(),
        });
        return Err(why);
    }
    if let Some(client) = &mut client {
        client.commit();
    }
    let pid = child.id();
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(pid)
}

/// Keep duplicate listening descriptors until the replacement acknowledges readiness. The old
/// axum servers keep accepting throughout the build and warmup, then drain without a time limit.
pub(crate) fn watch(
    serve: Serve,
    setup: Setup,
    shared: Arc<Shared>,
    sockets: Vec<OwnedFd>,
) -> Option<std::thread::JoinHandle<()>> {
    watch_with::<notify::RecommendedWatcher>(serve, setup, shared, sockets, backstop::PERIOD)
}

/// [`watch`] with watcher `W`, checking the sources on its own every `period`.
fn watch_with<W: Watcher + Send + 'static>(
    serve: Serve,
    setup: Setup,
    shared: Arc<Shared>,
    sockets: Vec<OwnedFd>,
    period: Duration,
) -> Option<std::thread::JoinHandle<()>> {
    let lease = setup.lease?;
    let reload = move || {
        let (send, recv) = mpsc::sync_channel(1);
        let directories = Arc::new(Mutex::new(BTreeSet::<PathBuf>::new()));
        let checked = directories.clone();
        let root = serve.root.clone();
        let handler = move |event: notify::Result<notify::Event>| {
            let relevant = event.as_ref().map_or(true, |event| {
                event.need_rescan()
                    || (!matches!(event.kind, notify::EventKind::Access(_))
                        && event.paths.iter().any(|p| {
                            source_event(
                                &root,
                                &checked.lock().unwrap_or_else(|e| e.into_inner()),
                                p,
                            )
                        }))
            });
            if relevant {
                let _ = send.try_send(());
            }
        };
        let mut watcher = match W::new(handler, notify::Config::default()) {
            Ok(watcher) => watcher,
            Err(why) => {
                eprintln!("· Symphony reload: cannot watch Symphony sources: {why}");
                return;
            }
        };
        let mut watched = BTreeSet::new();
        if let Err(why) = watch_sources(&serve.root, &mut watcher, &directories, &mut watched) {
            eprintln!("· Symphony reload: {why}");
            return;
        }
        let mut attempted = setup.fingerprint;
        let mut last_problem = String::new();
        let mut initial = true;
        let mut backstop = Backstop::new(period);
        while !shared.stopping() {
            // Read once on startup, then after a source event and once a period, since an event
            // can be lost. An idle tick checks shutdown; it does not read files, hash code or
            // spawn repository commands.
            if !initial {
                match backstop.wait(&recv, Duration::from_millis(250)) {
                    Wake::Event => {
                        std::thread::sleep(Duration::from_millis(100));
                        while recv.try_recv().is_ok() {}
                    }
                    Wake::Period => {}
                    Wake::Idle => continue,
                }
                if let Err(why) =
                    watch_sources(&serve.root, &mut watcher, &directories, &mut watched)
                {
                    eprintln!("· Symphony reload: {why}");
                    continue;
                }
            }
            initial = false;
            let fingerprint = match crate::live::measured("source fingerprint", || {
                symphony::reload::fingerprint(&serve.root)
            }) {
                Ok(fingerprint) => {
                    last_problem.clear();
                    fingerprint
                }
                Err(why) => {
                    if why != last_problem {
                        eprintln!("· Symphony reload: {why}");
                        last_problem = why;
                    }
                    continue;
                }
            };
            if attempted.as_deref() == Some(&fingerprint) {
                continue;
            }
            attempted = Some(fingerprint.clone());
            let Some(home) = serve.home.as_deref() else {
                return;
            };
            let result = symphony::reload::build(&serve.root, home, || shared.stopping());
            if shared.stopping() {
                return;
            }
            // An edit during compilation may have produced a mixed binary. Build again for the
            // stable source set instead of installing a generation labelled with the wrong code.
            if symphony::reload::fingerprint(&serve.root).ok().as_deref() != Some(&fingerprint) {
                continue;
            }
            match result
                .and_then(|client| replace(&serve, &sockets, &lease, &fingerprint, &shared, client))
            {
                Ok(pid) => {
                    eprintln!(
                        "· Symphony reload: replacement pid {pid} ready; draining pid {}",
                        shared.pid
                    );
                    shared
                        .replaced
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                    shared.stop();
                    return;
                }
                Err(why) => {
                    if shared.stopping() {
                        return;
                    }
                    let message = format!(
                        "Symphony server update failed; the previous server is still serving. {why}"
                    );
                    eprintln!("· Symphony reload: {message}");
                    if let Some(hub) = shared.api.get() {
                        hub.server_notice.send_replace(message);
                    }
                }
            }
        }
    };
    Some(crate::front::worker(crate::front::RELOAD_WORKER, reload))
}

fn source_event(root: &Path, directories: &BTreeSet<PathBuf>, path: &Path) -> bool {
    if path.starts_with(root.join("symphony/apps/symphony-ui"))
        || [
            "tools/rs-bin.sh",
            "symphony/tools/rs-bin.sh",
            "symphony/tools/check_lock.sh",
        ]
        .iter()
        .any(|name| path == root.join(name))
    {
        return true;
    }
    // A referenced crate may not have a manifest yet, so it is absent even from a nonempty graph.
    // Manifest events rediscover that graph; an unrelated manifest leaves its fingerprint unchanged.
    // A wholly unreadable graph also needs to recover from other source corrections.
    if [
        "apps",
        "crates",
        "tools",
        "symphony/apps",
        "symphony/crates",
        "symphony/tools",
    ]
    .iter()
    .any(|group| path.starts_with(root.join(group)))
        && (directories.is_empty() || path.file_name().is_some_and(|name| name == "Cargo.toml"))
    {
        return true;
    }
    if path.starts_with(root.join(".cargo")) || path.starts_with(root.join("symphony/.cargo")) {
        return true;
    }
    if (path.parent() == Some(root) || path.parent() == Some(root.join("symphony").as_path()))
        && matches!(
            path.file_name().and_then(|n| n.to_str()),
            Some(
                "Cargo.toml"
                    | "Cargo.lock"
                    | "rust-toolchain.toml"
                    | "justfile"
                    | "apps"
                    | "crates"
                    | "tools"
            )
        )
    {
        return true;
    }
    directories.iter().any(|dir| {
        path == dir
            || path.starts_with(dir.join("src"))
            || (path.parent() == Some(dir)
                && matches!(
                    path.file_name().and_then(|n| n.to_str()),
                    Some("Cargo.toml" | "build.rs")
                ))
    })
}

fn watch_sources(
    root: &Path,
    watcher: &mut impl Watcher,
    directories: &Mutex<BTreeSet<PathBuf>>,
    watched: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    let sources = symphony::reload::source_directories(root).unwrap_or_default();
    let mut paths = vec![
        (root.to_path_buf(), RecursiveMode::NonRecursive),
        (root.join(".cargo"), RecursiveMode::Recursive),
        (root.join("symphony"), RecursiveMode::NonRecursive),
        (root.join("symphony/.cargo"), RecursiveMode::Recursive),
    ];
    // Watch workspace source groups, including a dependency directory that does not exist yet.
    // Build output is outside these groups. The callback filters to the current dependency graph.
    for group in [
        "apps",
        "crates",
        "tools",
        "symphony/apps",
        "symphony/crates",
        "symphony/tools",
    ] {
        paths.push((root.join(group), RecursiveMode::Recursive));
    }
    *directories.lock().unwrap_or_else(|e| e.into_inner()) = sources;
    watched.retain(|path| {
        if path.is_dir() {
            true
        } else {
            let _ = watcher.unwatch(path);
            false
        }
    });
    for (path, mode) in paths {
        if !watched.contains(&path) && path.is_dir() {
            watcher
                .watch(&path, mode)
                .map_err(|e| format!("cannot watch {}: {e}", path.display()))?;
            watched.insert(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Setup, source_event, watch_with};
    use crate::front::Shared;
    use crate::gate::Gate;
    use crate::testing::{Tree, git, repository};
    use sha2::{Digest, Sha256};
    use std::collections::BTreeSet;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    #[test]
    fn a_source_edit_whose_event_is_lost_still_starts_a_build_within_a_period() {
        let mut tree = Tree::new();
        tree.root = tree.root.canonicalize().unwrap();
        let root = repository(&tree);
        tree.write(
            "main/symphony/apps/symphony-server/Cargo.toml",
            "[package]\nname = \"symphony-server\"\n",
        );
        tree.write(
            "main/symphony/apps/symphony-server/src/main.rs",
            "fn main() {}\n",
        );
        std::fs::create_dir_all(root.join("tools")).unwrap();
        tree.write("main/apps/symphony-host/Cargo.toml", "[package]\nname = 'symphony-host'\n[dependencies]\nsymphony-server = { path = '../../symphony/apps/symphony-server' }\n");
        tree.write(
            "main/apps/symphony-host/src/bin/symphony-server.rs",
            "fn main() {}\n",
        );
        // The owned build command records its selected package and fails immediately.
        tree.write(
            "main/tools/rs-bin.sh",
            "#!/bin/sh\nprintf '%s\\n' \"$*\" > \"${0%/tools/rs-bin.sh}/build-args\"\nexit 1\n",
        );
        std::fs::set_permissions(
            root.join("tools/rs-bin.sh"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "Reload sources"]);
        // The installed client is current, so only the server build runs.
        let home = tree.path("home");
        tree.write("home/client/public/index.html", "client");
        // The installed bundle's hash: each file's name and contents, each after its length.
        let mut distribution = Sha256::new();
        let (name, contents) = ("index.html", "client");
        distribution.update(name.len().to_le_bytes());
        distribution.update(name.as_bytes());
        distribution.update(contents.len().to_le_bytes());
        distribution.update(contents.as_bytes());
        let distribution: String = distribution
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        tree.write(
            "home/client-installed.json",
            &serde_json::json!({
                "source": symphony::reload::client_fingerprint(&root).unwrap(),
                "distribution": distribution,
            })
            .to_string(),
        );
        let shared = Shared::new(
            i64::from(std::process::id()),
            1,
            &root,
            Some(&home),
            Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
        );
        let hub = Arc::new(crate::live::Hub::new(&root, &home.join("client/public")));
        assert!(shared.api.set(hub.clone()).is_ok());
        let serve = crate::cli::Serve {
            home: Some(home.clone()),
            root: root.clone(),
            port: 1,
            hosts: Vec::new(),
        };
        let setup = Setup {
            listeners: Vec::new(),
            lease: Some(std::fs::File::create(home.join("server.lock")).unwrap()),
            control: None,
            fingerprint: Some(symphony::reload::fingerprint(&root).unwrap()),
        };
        let period = Duration::from_millis(500);
        // A watcher that never delivers: past the startup read, only the period sees an edit.
        let worker =
            watch_with::<notify::NullWatcher>(serve, setup, shared.clone(), Vec::new(), period)
                .unwrap();
        // The first edit may land before or after the startup read; the second lands after the
        // first attempt has ended, so the worker is waiting again.
        for edit in ["first", "second"] {
            hub.server_notice.send_replace(String::new());
            let edited = Instant::now();
            tree.write(
                "main/symphony/apps/symphony-server/src/main.rs",
                &format!("fn main() {{ /* {edit} edit */ }}\n"),
            );
            let failed = loop {
                let text = hub.server_notice.borrow().clone();
                if !text.is_empty() {
                    break text;
                }
                assert!(
                    edited.elapsed() < period + Duration::from_secs(30),
                    "no build for the {edit} edit"
                );
                std::thread::sleep(Duration::from_millis(20));
            };
            assert!(
                failed.starts_with(
                    "Symphony server update failed; the previous server is still serving. symphony-server build exit status: 1"
                ),
                "{failed}"
            );
            assert!(
                std::fs::read_to_string(root.join("build-args"))
                    .unwrap()
                    .contains("--path apps/symphony-host")
            );
        }
        shared.stop();
        worker.join().unwrap();
    }

    #[test]
    fn client_fingerprint_and_events_cover_assets_config_and_transitive_crates() {
        let tree = Tree::new();
        let root = repository(&tree);
        tree.write(
            "main/symphony/apps/symphony-server/Cargo.toml",
            "[package]\nname = \"symphony-server\"\n",
        );
        tree.write(
            "main/symphony/apps/symphony-server/src/main.rs",
            "fn main() {}\n",
        );
        tree.write("main/symphony/apps/symphony-ui/Cargo.toml", "[package]\nname = \"symphony-ui\"\n[dependencies]\nwire = { path = \"../../../crates/wire\" }\n");
        tree.write(
            "main/crates/wire/Cargo.toml",
            "[package]\nname = \"wire\"\n[build-dependencies]\nleaf = { path = \"../leaf\" }\n",
        );
        tree.write("main/crates/wire/src/lib.rs", "// wire\n");
        tree.write(
            "main/crates/leaf/Cargo.toml",
            "[package]\nname = \"leaf\"\n",
        );
        tree.write("main/crates/leaf/src/lib.rs", "// leaf\n");
        tree.write("main/apps/symphony-host/Cargo.toml", "[package]\nname = 'symphony-host'\n[dependencies]\nsymphony-server = { path = '../../symphony/apps/symphony-server' }\n");
        tree.write(
            "main/apps/symphony-host/src/bin/symphony-server.rs",
            "fn main() {}\n",
        );
        let directories = symphony::reload::source_directories(&root).unwrap();
        assert!(directories.contains(&root.join("crates/leaf")));
        let mut before = symphony::reload::client_fingerprint(&root).unwrap();
        let server_before = symphony::reload::fingerprint(&root).unwrap();
        tree.write(
            "main/apps/symphony-host/src/bin/symphony-server.rs",
            "fn main() { /* host */ }\n",
        );
        assert_ne!(server_before, symphony::reload::fingerprint(&root).unwrap());
        assert_eq!(before, symphony::reload::client_fingerprint(&root).unwrap());
        assert!(source_event(
            &root,
            &directories,
            &root.join("apps/symphony-host/src/bin/symphony-server.rs")
        ));
        for file in [
            "symphony/apps/symphony-ui/src/main.rs",
            "symphony/apps/symphony-ui/assets/docs.css",
            "symphony/apps/symphony-ui/Dioxus.toml",
            "symphony/apps/symphony-ui/index.html",
            "symphony/apps/symphony-ui/bin/cargo-symphony-ui",
            "symphony/apps/symphony-ui/bundle.sh",
            "symphony/apps/symphony-ui/package.sh",
            "symphony/apps/symphony-ui/tools.sh",
            "symphony/apps/symphony-ui/wasm-fresh.sh",
            "crates/wire/src/lib.rs",
            "crates/leaf/src/lib.rs",
            "crates/leaf/build.rs",
            "Cargo.toml",
            "Cargo.lock",
            "symphony/Cargo.toml",
            "symphony/Cargo.lock",
            "symphony/.cargo/config.toml",
            "symphony/justfile",
            "symphony/tools/rs-bin.sh",
            "symphony/tools/check_lock.sh",
            "rust-toolchain.toml",
            ".cargo/config.toml",
            "justfile",
            "tools/rs-bin.sh",
        ] {
            let all_before = symphony::reload::fingerprint(&root).unwrap();
            tree.write(&format!("main/{file}"), "changed build input\n");
            let after = symphony::reload::client_fingerprint(&root).unwrap();
            assert_ne!(before, after, "client identity must include {file}");
            assert_ne!(
                all_before,
                symphony::reload::fingerprint(&root).unwrap(),
                "reload identity must include {file}"
            );
            assert!(
                source_event(&root, &directories, &root.join(file)),
                "watch must wake for {file}"
            );
            before = after;
        }
        for file in [
            "symphony/apps/symphony-server/src/main.rs",
            "symphony/apps/symphony-ui/tests/view.rs",
            "docs/TOPIC.md",
            "crates/unrelated/src/lib.rs",
        ] {
            tree.write(&format!("main/{file}"), "not a client input\n");
            assert_eq!(
                before,
                symphony::reload::client_fingerprint(&root).unwrap(),
                "{file} must not rebuild the client"
            );
        }
        std::fs::remove_file(root.join("symphony/apps/symphony-ui/assets/docs.css")).unwrap();
        assert_ne!(
            before,
            symphony::reload::client_fingerprint(&root).unwrap(),
            "deleted assets must invalidate the client too"
        );
    }

    #[test]
    fn idle_source_watch_retries_when_a_missing_referenced_crate_is_created() {
        let tree = Tree::new();
        let root = repository(&tree);
        tree.write("main/Cargo.toml", "[workspace]\nmembers = [\"symphony/apps/symphony-server\", \"crates/present\", \"crates/late\"]\n");
        tree.write("main/symphony/apps/symphony-server/Cargo.toml", "[package]\nname = \"symphony-server\"\n[dependencies]\npresent = { path = \"../../../crates/present\" }\nlate = { path = \"../../../crates/late\" }\n");
        tree.write(
            "main/symphony/apps/symphony-server/src/main.rs",
            "fn main() {}\n",
        );
        tree.write(
            "main/crates/present/Cargo.toml",
            "[package]\nname = \"present\"\n",
        );
        tree.write("main/crates/present/src/lib.rs", "pub fn present() {}\n");
        let directories = symphony::reload::source_directories(&root).unwrap();
        assert_eq!(
            directories.len(),
            2,
            "this is a partial graph, not the empty-graph fallback"
        );
        assert!(!directories.contains(&root.join("crates/late")));
        // The failed build's attempted fingerprint excludes the still-missing dependency.
        let attempted = symphony::reload::fingerprint(&root).unwrap();

        tree.write(
            "main/crates/unrelated/Cargo.toml",
            "[package]\nname = \"unrelated\"\n",
        );
        assert_eq!(
            symphony::reload::fingerprint(&root).unwrap(),
            attempted,
            "rediscovery must not rebuild for an unrelated manifest"
        );
        assert!(!source_event(
            &root,
            &directories,
            &root.join("crates/unrelated/src/lib.rs")
        ));

        tree.write("main/crates/late/src/lib.rs", "pub fn late() {}\n");
        let manifest = tree
            .write(
                "main/crates/late/Cargo.toml",
                "[package]\nname = \"late\"\n",
            )
            .canonicalize()
            .unwrap();
        assert!(
            source_event(&root, &directories, &manifest),
            "creation of the missing manifest must wake the reload worker using the old graph"
        );
        let retry = symphony::reload::fingerprint(&root).unwrap();
        assert_ne!(
            retry, attempted,
            "the worker must retry the repaired source set"
        );
        let recovered = symphony::reload::source_directories(&root).unwrap();
        assert!(recovered.contains(&root.join("crates/late")));
        assert!(!recovered.contains(&root.join("crates/unrelated")));
        assert!(source_event(
            &root,
            &recovered,
            &root.join("crates/late/src/lib.rs")
        ));
    }

    #[test]
    fn idle_source_watch_follows_build_inputs_and_recovers_an_incomplete_graph() {
        let root = Path::new("/repo");
        let directories = BTreeSet::from([
            root.join("symphony/apps/symphony-server"),
            root.join("symphony/crates/tasks"),
            root.join("symphony/crates/symphony-api"),
        ]);
        for (path, expected) in [
            ("Cargo.toml", true),
            ("Cargo.lock", true),
            (".cargo/config.toml", true),
            ("symphony/crates/tasks/Cargo.toml", true),
            ("symphony/crates/tasks/build.rs", true),
            ("symphony/crates/tasks/src/new/module.rs", true),
            ("symphony/apps/symphony-server/src/api.rs", true),
            ("symphony/apps/symphony-ui/src/main.rs", true),
            ("symphony/apps/symphony-ui/assets/docs.css", true),
            ("symphony/apps/symphony-ui/Dioxus.toml", true),
            ("symphony/apps/symphony-ui/index.html", true),
            ("symphony/apps/symphony-ui/bin/cargo-symphony-ui", true),
            ("symphony/apps/symphony-ui/bundle.sh", true),
            ("symphony/apps/symphony-ui/package.sh", true),
            ("symphony/apps/symphony-ui/tools.sh", true),
            ("symphony/apps/symphony-ui/wasm-fresh.sh", true),
            ("symphony/crates/symphony-api/src/lib.rs", true),
            ("tools/rs-bin.sh", true),
            ("justfile", true),
            ("symphony/crates/tasks/tests/store.rs", false),
            ("docs/README.md", false),
            (".target/debug/symphony-server", false),
        ] {
            assert_eq!(
                source_event(root, &directories, &root.join(path)),
                expected,
                "{path}"
            );
        }
        assert!(source_event(
            root,
            &BTreeSet::new(),
            &root.join("crates/new/src/lib.rs")
        ));
        assert!(!source_event(
            root,
            &BTreeSet::new(),
            &root.join("artifacts/log.txt")
        ));
    }

    #[test]
    fn a_spawn_racing_a_handover_never_masks_the_replacements_death() {
        use super::{Answer, Control, answer};
        use std::io::{ErrorKind, Read};
        use std::os::fd::AsRawFd;
        use std::sync::atomic::{AtomicBool, Ordering};
        use tasks::sys::{Child, Command, Stdio};

        /// Children killed and reaped however the test ends.
        struct Children(Vec<Child>);
        impl Drop for Children {
            fn drop(&mut self) {
                for child in &mut self.0 {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }

        let spawned = AtomicBool::new(false);
        std::thread::scope(|scope| {
            // Children outliving every handover, spawned beside its pipes' making through the
            // helpers, as a build or a board command is spawned beside a reload. A std spawn that
            // skips the lock could keep a real pipe's first ends for its life.
            let other = scope.spawn(|| {
                let mut children = Children(Vec::new());
                for _ in 0..32 {
                    let child = Command::new("sleep")
                        .arg("60")
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .unwrap();
                    children.0.push(child);
                }
                spawned.store(true, Ordering::Relaxed);
                children
            });
            // Handovers made back to back, so the racers' spawns fall while pairs are being made.
            // Each generation's ends go at once, as a replacement or a parent that dies does. A
            // racer never keeps a copy past its exec, so an end not yet answered is kept and asked
            // again once every racer has started: waiting for it here would leave the spawns that
            // start meanwhile unraced.
            let mut handovers = 0;
            let mut held = Vec::new();
            while !spawned.load(Ordering::Relaxed) {
                let ((ready, commit), ends) = Control::pair().unwrap();
                let Control {
                    ready: replacement_ready,
                    commit: replacement_commit,
                } = ends;
                drop((replacement_ready, commit));
                for end in [ready, replacement_commit] {
                    tasks::sys::set_nonblocking(&end, true).unwrap();
                    match (&end).read(&mut [0]) {
                        Ok(0) => {}
                        Err(e)
                            if matches!(
                                e.kind(),
                                ErrorKind::WouldBlock | ErrorKind::Interrupted
                            ) =>
                        {
                            held.push(end);
                        }
                        other => panic!("handover {handovers} answered {other:?}"),
                    }
                }
                handovers += 1;
            }
            // An end a racer kept would have held its answer until that racer ended.
            let mut children = other.join().unwrap();
            for end in &held {
                assert_eq!(answer(end, || false).unwrap(), Answer::Ended);
            }
            for child in &mut children.0 {
                assert!(child.try_wait().unwrap().is_none(), "a racer ended first");
            }
            assert!(handovers > 0, "no handover was made beside the spawns");
        });

        // A replacement started as replace starts one, its ends kept across exec, that ends before
        // it is ready: its death is the end of its parent's ready pipe.
        let ((ready, _commit), ends) = Control::pair().unwrap();
        let fds = [ends.ready.as_raw_fd(), ends.commit.as_raw_fd()];
        let mut command = tasks::sys::Command::new("true");
        for fd in fds {
            command.pass_fd(fd);
        }
        let mut replacement = command.stdin(tasks::sys::Stdio::Null).spawn().unwrap();
        drop(ends);
        assert!(replacement.wait().unwrap().success());
        assert_eq!(answer(&ready, || false).unwrap(), Answer::Ended);
    }

    #[test]
    fn a_replacement_is_ready_once_committed_or_once_its_parent_has_gone() {
        use super::{Answer, Control, answer};
        use std::io::Write;
        use std::os::unix::net::UnixStream;

        let replacement = |control: Control| {
            std::thread::spawn(move || {
                Setup {
                    listeners: Vec::new(),
                    lease: None,
                    control: Some(control),
                    fingerprint: None,
                }
                .ready()
            })
        };
        let ((ready, mut commit), ends) = Control::pair().unwrap();
        let ready_thread = replacement(ends);
        assert_eq!(answer(&ready, || false).unwrap(), Answer::Byte(b'R'));
        commit.write_all(b"C").unwrap();
        assert_eq!(ready_thread.join().unwrap(), Ok(()));

        // A parent gone after the replacement said it was ready leaves it serving.
        let ((ready, commit), ends) = Control::pair().unwrap();
        let ready_thread = replacement(ends);
        assert_eq!(answer(&ready, || false).unwrap(), Answer::Byte(b'R'));
        drop(commit);
        assert_eq!(ready_thread.join().unwrap(), Ok(()));

        // Anything but the commit is refused.
        let ((ready, mut commit), ends) = Control::pair().unwrap();
        let ready_thread = replacement(ends);
        assert_eq!(answer(&ready, || false).unwrap(), Answer::Byte(b'R'));
        commit.write_all(b"X").unwrap();
        assert!(ready_thread.join().unwrap().is_err());

        // An older generation's one socket pair carries both.
        let (mut parent, end) = UnixStream::pair().unwrap();
        let ready_thread = replacement(Control::socket(end.into()).unwrap());
        let mut said = [0];
        std::io::Read::read_exact(&mut parent, &mut said).unwrap();
        assert_eq!(said, *b"R");
        parent.write_all(b"C").unwrap();
        assert_eq!(ready_thread.join().unwrap(), Ok(()));

        // Asked to stop first, the parent's wait says so.
        let ((ready, _commit), _ends) = Control::pair().unwrap();
        assert_eq!(answer(&ready, || true).unwrap(), Answer::Stopped);
    }

    #[test]
    fn a_transfer_names_the_handover_pipes_or_an_older_generations_socket() {
        use super::Transfer;
        let ends = |fields: &str| {
            serde_json::from_str::<Transfer>(&format!(
                r#"{{"parent":1,"listeners":[3],"lease":4,{fields}"fingerprint":"f"}}"#
            ))
            .unwrap()
            .ends()
        };
        assert_eq!(ends(r#""ready":5,"commit":6,"#), Ok(vec![5, 6]));
        // A generation from before the pipes reloads into this one with its one socket.
        assert_eq!(ends(r#""control":5,"#), Ok(vec![5]));
        for fields in ["", r#""ready":5,"#, r#""ready":5,"commit":6,"control":7,"#] {
            assert!(ends(fields).is_err(), "{fields}");
        }
        // What this generation sends names no socket: an older replacement refuses it whole.
        let sent = serde_json::to_value(Transfer {
            parent: 1,
            listeners: vec![3],
            lease: 4,
            ready: Some(5),
            commit: Some(6),
            control: None,
            fingerprint: "f".into(),
        })
        .unwrap();
        assert!(sent.get("control").is_none(), "{sent}");
    }
}
