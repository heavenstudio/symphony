//! Listening sockets and the server lease survive exec; accepted connections stay with the old
//! generation. A private readiness pipe makes a failed replacement harmless to the serving one.

use crate::backstop::{self, Backstop, Wake};
use crate::cli::Serve;
use crate::front::Shared;
use crate::home::{Home, Running};
use notify::{RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::{PipeReader, PipeWriter, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};
use symphony_board::board_health::hours::Gate;
use symphony_board::sys::{self, Stdio};

mod stamp;
use stamp::{installed_source, stamp_install};

const INHERITED: &str = "SF_SYMPHONY_HANDOVER";
const PATIENCE: Duration = Duration::from_secs(120);

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
/// close-on-exec ([`sys::pipe`]), so no child another thread spawns meanwhile keeps an end
/// and masks either death until [`PATIENCE`].
#[derive(Debug)]
struct Control {
    ready: PipeWriter,
    commit: PipeReader,
}

impl Control {
    /// The parent's ends (readiness in, commit out) and the replacement's of a new handover.
    fn pair() -> std::io::Result<((PipeReader, PipeWriter), Self)> {
        let (ready, replacement_ready) = sys::pipe()?;
        let (replacement_commit, commit) = sys::pipe()?;
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
    sys::set_nonblocking(from, true)?;
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
    let fingerprint = lease
        .as_ref()
        .and_then(|lease| installed_source(serve, lease));
    Ok(Setup {
        listeners: Vec::new(),
        lease,
        control: None,
        fingerprint,
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

/// Hand the sockets and the lease to `exe`, the replacement serving `serve` (its machine folder,
/// its root and install folder), and wait until it is ready: its pid.
fn replace(
    serve: &Serve,
    exe: &Path,
    sockets: &[OwnedFd],
    lease: &File,
    fingerprint: &str,
    shared: &Shared,
    mut client: Option<symphony::reload::ClientUpdate>,
) -> Result<u32, String> {
    let machine = serve.home.as_deref().ok_or("no server home")?;
    let install = serve.installed().ok_or("no install folder")?.to_path_buf();
    let installed = exe == install.join("bin/symphony-server");
    let home = Home::new(machine, exe, &serve.root, serve.port, serve.hosts.clone());
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
    let mut command = symphony::children::command(exe);
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
    command.arg("--install").arg(&install);
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
        // This generation's board hours end here, before the commit; the replacement's begin after.
        Gate::process().close(symphony_board::sweep::trigger::now());
        if installed && let Err(why) = stamp_install(&install, fingerprint) {
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
        // This generation records its agents up to now, the replacement from the commit on.
        shared.agents.close(symphony_board::sweep::trigger::now());
        commit.write_all(b"C").map_err(|e| e.to_string())
    });
    if let Err(why) = result {
        let _ = child.kill();
        let _ = child.wait();
        shared.agents.open();
        // The old server is still accepting. Restore its publication if the child failed after it.
        let _ = home.record(&Running {
            port: serve.port,
            pid: i64::from(std::process::id()),
            root: serve.root.display().to_string(),
        });
        // No replacement committed: this generation keeps the board's hours again.
        Gate::process().reopen(symphony_board::sweep::trigger::now());
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
/// axum servers keep accepting throughout the build and warmup, then drain what they accepted for
/// at most `front`'s `REPLACED_DRAIN`, and the old generation ends.
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
            // A starter whose build must serve the machine asked for the hand-off (host rule).
            if shared
                .handover
                .swap(false, std::sync::atomic::Ordering::SeqCst)
                && let Some(machine) = serve.home.as_deref()
            {
                match crate::host::Handover::take(machine) {
                    Some(Ok(request)) => {
                        let successor = Serve {
                            root: request.root.clone(),
                            install: Some(request.install.clone()),
                            ..serve.clone()
                        };
                        let fingerprint =
                            symphony::reload::fingerprint(&request.root).unwrap_or_default();
                        let result = replace(
                            &successor,
                            &request.exe,
                            &sockets,
                            &lease,
                            &fingerprint,
                            &shared,
                            None,
                        );
                        if replaced(&shared, result) {
                            return;
                        }
                    }
                    Some(Err(why)) => eprintln!("· Symphony hand-off refused: {why}"),
                    None => {}
                }
            }
            // Read once on startup, then after a source event and once a period, since an event
            // can be lost. An idle tick checks shutdown and the hand-off flag; it does not read
            // files, hash code or spawn repository commands.
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
            let Some(install) = serve.installed() else {
                return;
            };
            let exe = install.join("bin/symphony-server");
            let result = symphony::reload::build(&serve.root, install, || shared.stopping());
            if shared.stopping() {
                return;
            }
            // An edit during compilation may have produced a mixed binary. Build again for the
            // stable source set instead of installing a generation labelled with the wrong code.
            if symphony::reload::fingerprint(&serve.root).ok().as_deref() != Some(&fingerprint) {
                continue;
            }
            let result = result.and_then(|client| {
                replace(
                    &serve,
                    &exe,
                    &sockets,
                    &lease,
                    &fingerprint,
                    &shared,
                    client,
                )
            });
            if replaced(&shared, result) {
                return;
            }
        }
    };
    Some(crate::front::worker(crate::front::RELOAD_WORKER, reload))
}

/// Whether a replacement took over: this generation then drains. A failure is said on the
/// server's notice, this generation still serving.
fn replaced(shared: &Shared, result: Result<u32, String>) -> bool {
    match result {
        Ok(pid) => {
            eprintln!(
                "· Symphony reload: replacement pid {pid} ready; draining pid {}",
                shared.pid
            );
            shared
                .replaced
                .store(true, std::sync::atomic::Ordering::SeqCst);
            shared.stop();
            true
        }
        Err(why) => {
            if shared.stopping() {
                return true;
            }
            let message = format!(
                "Symphony server update failed; the previous server is still serving. {why}"
            );
            eprintln!("· Symphony reload: {message}");
            if let Some(hub) = shared.api.get() {
                hub.tell_update(message);
            }
            false
        }
    }
}

mod source_event;
use source_event::source_event;

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
mod tests;
