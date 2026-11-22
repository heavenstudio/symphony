//! Listening sockets and the server lease survive exec; accepted connections stay with the old
//! generation. A private readiness pipe makes a failed replacement harmless to the serving one.
//!
//! What the reload says goes to the log by [`crate::front::tell`], never `eprintln!`, which panics
//! its thread when stderr cannot be written: a reload that died of saying something replaces
//! nothing from then on, and says nothing of it.

use crate::backstop::{self, Backstop, Wake, Waker};
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
use std::sync::Mutex;
use std::time::{Duration, Instant};
use symphony_board::lock::Recovered as _;
use symphony_board::sys::{self, Stdio};

mod retry;
mod stamp;
use retry::Retry;
use stamp::{RunningImage, installed_source, stamp_install};

/// What a serving server hands its successor; no person sets it.
const INHERITED: symphony_env::Var = symphony_env::HANDOVER;
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
                #[expect(
                    clippy::disallowed_methods,
                    reason = "no-timed-waits baseline: reads a nonblocking socket again, where a blocking read under a deadline is the event"
                )]
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
    /// The installed image this generation runs, when it runs that image.
    running: Option<RunningImage>,
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

/// The transfer `parent` handed this process under [`INHERITED`] (`var` reads it), taken only from
/// that sender: a descendant that inherited it from its own parent takes none. A value that is not
/// a transfer is refused.
fn inherited(
    var: impl Fn(&'static str) -> Result<String, std::env::VarError>,
    parent: i64,
) -> Result<Option<Transfer>, String> {
    let Ok(text) = var(INHERITED.name) else {
        return Ok(None);
    };
    let transfer = serde_json::from_str::<Transfer>(&text).map_err(|e| e.to_string())?;
    Ok((transfer.parent == parent).then_some(transfer))
}

/// Called inside the runtime, before binding. Only the direct child of the transfer's sender may
/// consume it; build tools and other descendants cannot reuse stale descriptor numbers.
pub(crate) fn setup(serve: &Serve) -> Result<Setup, String> {
    let running = RunningImage::of(serve);
    if let Some(transfer) = inherited(std::env::var, crate::sys::parent())? {
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
            running,
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
        running,
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

/// What a generation hands its replacement: its listening sockets and the server's lease.
struct Handed {
    sockets: Vec<OwnedFd>,
    lease: File,
}

/// Hand the sockets and the lease to `exe`, the replacement serving `serve` (its machine folder,
/// its root and install folder), and wait until it is ready: its pid. `version` names what it
/// runs, the sources' fingerprint it was built from or the release it is, as its hand-off and
/// an installed binary's stamp record it.
fn replace(
    serve: &Serve,
    exe: &Path,
    handed: &Handed,
    version: &str,
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
        listeners: handed.sockets.iter().map(AsRawFd::as_raw_fd).collect(),
        lease: handed.lease.as_raw_fd(),
        ready: Some(replacement_ends.ready.as_raw_fd()),
        commit: Some(replacement_ends.commit.as_raw_fd()),
        control: None,
        fingerprint: version.into(),
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
    let transfer_json = serde_json::to_string(&transfer).map_err(|e| e.to_string())?;
    command.current_dir(&serve.root).stdin(Stdio::null());
    command.env(INHERITED.name, &transfer_json);
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
        if installed && let Err(why) = stamp_install(&install, version) {
            crate::front::tell(format_args!(
                "· Symphony reload: could not stamp installed binary: {why}"
            ));
        }
        let release = crate::home::release_of(exe);
        home.record(
            &Running {
                port: serve.port,
                pid: i64::from(child.id()),
                root: serve.root.display().to_string(),
            },
            release.as_deref(),
        )
        .map_err(|e| e.to_string())?;
        if let Some(client) = &client
            && let Err(why) = client.stamp()
        {
            crate::front::tell(format_args!(
                "· Symphony reload: could not stamp installed client: {why}"
            ));
        }
        // This generation records up to now, its agents and its board's hours alike, the
        // replacement from the commit on: closed last, so nothing that can fail falls between.
        shared
            .recording
            .close(symphony_board::sweep::trigger::now());
        commit.write_all(b"C").map_err(|e| e.to_string())
    });
    if let Err(why) = result {
        end_replacement(&mut child, &why);
        // No replacement committed: this generation records again (a gate never closed stays).
        shared
            .recording
            .reopen(symphony_board::sweep::trigger::now());
        // The old server is still accepting. Restore its publication if the child failed after it.
        let ours = symphony::machine::release::this().map(|(_, manifest)| &manifest.release);
        if let Err(error) = home.record(
            &Running {
                port: serve.port,
                pid: i64::from(std::process::id()),
                root: serve.root.display().to_string(),
            },
            ours.map(String::as_str),
        ) {
            crate::front::tell(format_args!(
                "· Symphony reload: could not restore this server's record: {error}"
            ));
        }
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

/// End the replacement `child` that failed its hand-off for `why`, and reap it: SIGKILL, said on
/// stderr first, unless it has ended already (its pid is then sent nothing).
fn end_replacement(child: &mut sys::Child, why: &str) {
    if matches!(child.try_wait(), Ok(None)) {
        crate::front::tell(format_args!(
            "· Symphony reload: SIGKILL to the replacement server, pid {}: {why}",
            child.id()
        ));
        let _ = child.kill();
    }
    let _ = child.wait();
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
    watch_with::<notify::RecommendedWatcher>(
        serve,
        setup,
        shared,
        sockets,
        Backstop::new(backstop::PERIOD),
        Retry::new(),
    )
}

/// [`watch`] with watcher `W`, waiting on `backstop`, which checks the sources on its own once its
/// period, and trying a version whose update failed again when `retry` says.
fn watch_with<W: Watcher + Send + 'static>(
    serve: Serve,
    setup: Setup,
    shared: Arc<Shared>,
    sockets: Vec<OwnedFd>,
    mut backstop: Backstop,
    mut retry: Retry,
) -> Option<std::thread::JoinHandle<()>> {
    let handed = Handed {
        sockets,
        lease: setup.lease?,
    };
    // A release's server builds nothing: it follows the machine's `current` link.
    if let (Some((_, manifest)), Some(machine)) =
        (symphony::machine::release::this(), serve.home.clone())
    {
        let release = manifest.release.clone();
        return Some(crate::front::worker(
            crate::front::RELOAD_WORKER,
            move || {
                follow::<W>(
                    &serve,
                    &Follow {
                        machine,
                        release,
                        handed,
                    },
                    &shared,
                    backstop,
                    retry,
                );
            },
        ));
    }
    let reload = move || {
        // The stop and a starter's hand-off request wake it, as its watcher does.
        shared.wake_at_stop(backstop.waker());
        shared.wake_at_handover(backstop.waker());
        let changed = backstop.waker();
        let directories = Arc::new(Mutex::new(BTreeSet::<PathBuf>::new()));
        let checked = directories.clone();
        let root = serve.root.clone();
        let handler = move |event: notify::Result<notify::Event>| {
            let relevant = event.as_ref().map_or(true, |event| {
                event.need_rescan()
                    || (!matches!(event.kind, notify::EventKind::Access(_))
                        && event
                            .paths
                            .iter()
                            .any(|p| source_event(&root, &checked.lock().recover(), p)))
            });
            if relevant {
                changed.wake();
            }
        };
        let mut watcher = match W::new(handler, notify::Config::default()) {
            Ok(watcher) => watcher,
            Err(why) => {
                crate::front::tell(format_args!(
                    "· Symphony reload: cannot watch Symphony sources: {why}"
                ));
                return;
            }
        };
        let mut watched = BTreeSet::new();
        if let Err(why) = watch_sources(&serve.root, &mut watcher, &directories, &mut watched) {
            crate::front::tell(format_args!("· Symphony reload: {why}"));
            return;
        }
        let mut attempted = setup.fingerprint;
        let mut running = setup.running;
        let mut last_problem = String::new();
        let mut initial = true;
        // Whether the notice says an update failed: since a failure [`replaced`] told, until a
        // kept build clears it ([`kept`]).
        let mut failure_told = false;
        while !shared.stopping() {
            // Read once on startup, then after a source event and once a period, since an event
            // can be lost. A wake for the stop or a hand-off is looked at first; the sources are
            // read after it all the same, as a watcher's event may have come with it.
            if !initial {
                let wake = backstop.wait();
                if wake == Wake::Event && !shared.stopping() {
                    backstop.debounce(backstop::QUIET, backstop::BURST);
                }
                if shared.stopping() {
                    return;
                }
            }
            // A starter whose build must serve the machine asked for the hand-off (host rule): a
            // request made while this loop read or built left its wake for the wait above.
            match handed_over(&serve, &shared, &handed, &retry) {
                HandOff::Replaced => return,
                HandOff::Failed => failure_told = true,
                HandOff::None => {}
            }
            if !initial
                && let Err(why) =
                    watch_sources(&serve.root, &mut watcher, &directories, &mut watched)
            {
                crate::front::tell(format_args!("· Symphony reload: {why}"));
                continue;
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
                        crate::front::tell(format_args!("· Symphony reload: {why}"));
                        last_problem = why;
                    }
                    continue;
                }
            };
            // A version tried before is built again only once its update failed and the wait
            // after that failure is over ([`retry`]); any other is built now.
            if attempted.as_deref() == Some(&fingerprint) && !retry.again(&fingerprint) {
                continue;
            }
            attempted = Some(fingerprint.clone());
            let Some(install) = serve.installed() else {
                return;
            };
            let exe = install.join("bin/symphony-server");
            // This generation's own bytes, read while they are still installed: the build may
            // install other bytes in their place.
            if let Some(running) = &mut running {
                running.hash(install);
            }
            let result = symphony::reload::build(&serve.root, install, || shared.stopping());
            if shared.stopping() {
                return;
            }
            // An edit during compilation may have produced a mixed binary. Build again for the
            // stable source set instead of installing a generation labelled with the wrong code.
            // That result is dropped, and counts as a failure of this version: were the sources to
            // read as it again (a read that failed in passing, an edit undone), it is built again
            // once that failure's wait is over, and any other version at its first check.
            if symphony::reload::fingerprint(&serve.root).ok().as_deref() != Some(&fingerprint) {
                let why = format!(
                    "The sources changed or could not be read during the build; its result is \
                     dropped, and that version is tried again at about {} if they read as it \
                     again.",
                    retry.failed(&fingerprint)
                );
                // Said in the log alone, as the version read next is usually another and is
                // built at once: no update failed. A notice that says one did says this in its
                // place, so the retry it names is the one to come, at the time to come.
                if !failure_told {
                    crate::front::tell(format_args!("· Symphony reload: {why}"));
                } else if replaced(&shared, Err(why)) {
                    return;
                }
                continue;
            }
            if matches!(result, Ok(None)) && kept(running.as_ref(), install, &fingerprint, &shared)
            {
                retry.kept();
                failure_told = false;
                continue;
            }
            let result = result
                .and_then(|client| replace(&serve, &exe, &handed, &fingerprint, &shared, client))
                .map_err(|why| {
                    let again = retry.failed(&fingerprint);
                    format!(
                        "{}. It tries this version again at about {again}.",
                        reason(&why)
                    )
                });
            if replaced(&shared, result) {
                return;
            }
            failure_told = true;
        }
    };
    Some(crate::front::worker(crate::front::RELOAD_WORKER, reload))
}

/// How a starter's hand-off request came out at the reload loop's look ([`handed_over`]).
#[derive(Debug, PartialEq, Eq)]
enum HandOff {
    /// None was asked for, or the request was refused, said in the log.
    None,
    /// A replacement took over: this generation drains.
    Replaced,
    /// The replacement failed, said on the notice.
    Failed,
}

/// The hand-off a starter asked for ([`Shared::ask_handover`]), to the build its request in the
/// machine folder names ([`crate::host::Handover`]), when one asked. A failure is said on the
/// notice, with the time `retry` will try the sources' own failed update again, when one waits:
/// the hand-off is the starter's to ask again.
fn handed_over(serve: &Serve, shared: &Shared, handed: &Handed, retry: &Retry) -> HandOff {
    if !shared
        .handover
        .swap(false, std::sync::atomic::Ordering::SeqCst)
    {
        return HandOff::None;
    }
    let Some(machine) = serve.home.as_deref() else {
        return HandOff::None;
    };
    match crate::host::Handover::take(machine) {
        Some(Ok(request)) => {
            let successor = Serve {
                root: request.root.clone(),
                install: Some(request.install.clone()),
                ..serve.clone()
            };
            let fingerprint = symphony::reload::fingerprint(&request.root).unwrap_or_default();
            let result = replace(
                &successor,
                &request.exe,
                handed,
                &fingerprint,
                shared,
                None,
            )
            .map_err(|why| match retry.waiting() {
                Some(again) => format!(
                    "{}. The version whose update failed before is tried again at about {again}.",
                    reason(&why)
                ),
                None => why,
            });
            if replaced(shared, result) {
                HandOff::Replaced
            } else {
                HandOff::Failed
            }
        }
        Some(Err(why)) => {
            crate::front::tell(format_args!("· Symphony hand-off refused: {why}"));
            HandOff::None
        }
        None => HandOff::None,
    }
}

/// Whether a watcher's `event` in the machine folder can have moved its `current` link: a new link
/// renamed over it ([`symphony::machine::release::point`]), a rescan, or the watcher's own error.
fn moves_current(event: &notify::Result<notify::Event>) -> bool {
    event.as_ref().map_or(true, |event| {
        event.need_rescan()
            || (!matches!(event.kind, notify::EventKind::Access(_))
                && event.paths.iter().any(|path| {
                    path.file_name()
                        == Some(std::ffi::OsStr::new(symphony::machine::release::CURRENT))
                }))
    })
}

/// A watcher `W` of `machine`'s own entries, kept while it is followed, which wakes `moved` at the
/// events that can have moved its `current` link, one wake for a burst ([`moves_current`]). A
/// watcher that cannot start, or cannot watch the folder, is said in the log and wakes nothing:
/// the backstop's period then reads `current` alone.
fn watch_current<W: Watcher>(machine: &Path, moved: Waker) -> Option<W> {
    let handler = move |event: notify::Result<notify::Event>| {
        if moves_current(&event) {
            moved.wake();
        }
    };
    let watcher = W::new(handler, notify::Config::default()).and_then(|mut watcher| {
        watcher.watch(machine, RecursiveMode::NonRecursive)?;
        Ok(watcher)
    });
    watcher
        .inspect_err(|why| {
            crate::front::tell(format_args!(
                "· Symphony reload: cannot watch {} for `current` moving: {why}; it is read once \
                 a period",
                machine.display()
            ));
        })
        .ok()
}

/// What a release's server hands over when the machine's `current` moves: the machine folder, the
/// release it runs, and what it hands its replacement.
struct Follow {
    machine: PathBuf,
    release: String,
    handed: Handed,
}

/// What a follow of `current` keeps from one check to the next, and from a loop that died to the
/// one started in its place.
struct Following {
    /// Whether the next check comes before any wait: the first of the server's life.
    initial: bool,
    /// The release it tried to hand over to last, while `current` names another than this one.
    attempted: Option<String>,
    /// Whether the notice says something of this follow, which `current` naming this release
    /// clears.
    noticed: bool,
    /// Why it hands over to nothing, as the log said it last: said again once it is another.
    idle: String,
}

impl Following {
    /// It hands over to nothing for `why`: one log line when that is news.
    fn idle(&mut self, why: String) {
        if self.idle != why {
            crate::front::tell(format_args!("· Symphony reload: {why}"));
            self.idle = why;
        }
    }
}

/// The reload of the server of the release `follow.release`: its follow of the machine's `current`
/// ([`follow_current`]), which ends only with the server. One that ends before, a bug, is said in
/// the log and on the notice and started again once this has waited for the wake or the period
/// after, wherever in the follow it died: a server that follows nothing holds every release until
/// someone restarts it by hand, and one that dies at every start costs one line a wait.
fn follow<W: Watcher>(
    serve: &Serve,
    follow: &Follow,
    shared: &Shared,
    mut backstop: Backstop,
    mut retry: Retry,
) {
    // The stop and a starter's hand-off request wake it, as the watcher does.
    shared.wake_at_stop(backstop.waker());
    shared.wake_at_handover(backstop.waker());
    let mut state = Following {
        initial: true,
        attempted: None,
        noticed: false,
        idle: String::new(),
    };
    while !shared.stopping() {
        let ended = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            follow_current::<W>(serve, follow, shared, &mut backstop, &mut retry, &mut state);
        }));
        if shared.stopping() {
            return;
        }
        let why = match ended {
            Ok(()) => "its loop ended".to_string(),
            Err(panic) => format!("it panicked: {}", panicked(panic.as_ref())),
        };
        let message = format!(
            "Symphony server reload stopped following {}: {why}. It is started again at its next \
             check; this server is still the release {}.",
            follow
                .machine
                .join(symphony::machine::release::CURRENT)
                .display(),
            follow.release
        );
        crate::front::tell(format_args!("· Symphony reload: {message}"));
        if let Some(hub) = shared.api.get() {
            hub.tell_update(message);
        }
        // Waited for here, not in the follow started next, which may die before its own wait.
        // That one then reads `current` at once, and says again what holds it.
        backstop.wait();
        state.initial = true;
        state.noticed = true;
        state.idle.clear();
    }
}

/// What a panic said, when it said text.
fn panicked(panic: &(dyn std::any::Any + Send)) -> &str {
    panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("with no message")
}

/// One follow of `current`, until the server stops: whether the link names another release than
/// `follow.release`, read at the start, as its watcher `W` hears the link move, and once
/// `backstop`'s period as an event can be lost; when it does, the hand-off to that release's
/// server, by its own folder's path, with no build. Each hand-off it begins is one log line, and
/// so is each reason it begins none (`current` naming this release, or none), said when it
/// changes. A release not installed whole, or whose server does not become ready, leaves this one
/// serving, said in the log and on the notice with when it is tried again, which is when `retry`
/// says, at the check that finds it due; `current` named back at this release clears the notice. A
/// starter's request to hand over is refused, said in the log: no start replaces a release's
/// server ([`symphony::machine::host`]).
fn follow_current<W: Watcher>(
    serve: &Serve,
    follow: &Follow,
    shared: &Shared,
    backstop: &mut Backstop,
    retry: &mut Retry,
    state: &mut Following,
) {
    let Follow {
        machine,
        release,
        handed,
    } = follow;
    let link = machine.join(symphony::machine::release::CURRENT);
    // Watched before its first read, so no move after that read goes unheard.
    let _watcher = watch_current::<W>(machine, backstop.waker());
    while !shared.stopping() {
        // `current` is read at the start, at a wake and once a period: a wake for the stop or a
        // hand-off is looked at first, and `current` read after it all the same.
        if !state.initial {
            // A new link and its rename are one move: read once both are done.
            if backstop.wait() == Wake::Event && !shared.stopping() {
                backstop.debounce(backstop::QUIET, backstop::QUIET);
            }
            if shared.stopping() {
                return;
            }
        }
        state.initial = false;
        if shared
            .handover
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            match crate::host::Handover::take(machine) {
                Some(Ok(request)) => crate::front::tell(format_args!(
                    "· Symphony hand-off refused: this server is the release {release}, which only \
                     its reload replaces, following {}; {} asked for {}",
                    link.display(),
                    request.root.display(),
                    request.exe.display()
                )),
                Some(Err(why)) => {
                    crate::front::tell(format_args!("· Symphony hand-off refused: {why}"));
                }
                None => {}
            }
        }
        let Some(next) = symphony::machine::release::current(machine) else {
            state.idle(format!(
                "{} names no release, so the release {release} hands over to none",
                link.display()
            ));
            continue;
        };
        if &next == release {
            state.attempted = None;
            if std::mem::take(&mut state.noticed)
                && let Some(hub) = shared.api.get()
            {
                hub.tell_update(String::new());
            }
            state.idle(format!(
                "{} names {release}, the release this server is: it follows the link",
                link.display()
            ));
            continue;
        }
        // Said at its failure, with the time it is tried again.
        if state.attempted.as_deref() == Some(next.as_str()) && !retry.again(&next) {
            continue;
        }
        state.attempted = Some(next.clone());
        state.idle.clear();
        crate::front::tell(format_args!(
            "· Symphony reload: current names {next}; handing over to its server"
        ));
        let result = to_release(machine, &next)
            .and_then(|exe| replace(serve, &exe, handed, &next, shared, None))
            .map_err(|why| {
                let again = retry.failed(&next);
                format!(
                    "The release {next} did not take over: {}. It is tried again at about {again}.",
                    reason(&why)
                )
            });
        if replaced(shared, result) {
            return;
        }
        state.noticed = true;
    }
}

/// The server of the release `name` in `machine`, by its own folder's path, when the release is
/// installed whole; why not otherwise.
fn to_release(machine: &Path, name: &str) -> Result<PathBuf, String> {
    use symphony::machine::release;
    match release::installed(machine, name)? {
        Some(_) => {
            let exe = release::binary(machine, name, release::SERVER);
            symphony::builds::runnable(&exe)
                .then_some(exe.clone())
                .ok_or_else(|| format!("{} does not run", exe.display()))
        }
        None => Err(format!(
            "{} holds no manifest",
            release::folder(machine, name).display()
        )),
    }
}

/// Why an update failed, ending where the notice's next sentence starts: a build that said nothing
/// leaves its separator, and diagnostics may end a sentence.
fn reason(why: &str) -> &str {
    why.trim_end_matches([' ', ':', '.'])
}

/// After a build of `fingerprint` into `install` with no client to publish: whether the installed
/// image still holds the bytes this generation runs (`running`, [`RunningImage::kept`]), stamped
/// then as that build's, so this generation keeps serving with no hand-off, said in one line. The
/// notice of an update that failed before, which said an older server still serves, goes.
///
/// A build with a client to publish hands over all the same: its publication is staged against
/// the replacement's readiness and rolled back when that fails ([`replace`]).
fn kept(
    running: Option<&RunningImage>,
    install: &Path,
    fingerprint: &str,
    shared: &Shared,
) -> bool {
    let Some(stamped) = running.and_then(|running| running.kept(install, fingerprint)) else {
        return false;
    };
    match stamped {
        Ok(()) => crate::front::tell(format_args!(
            "· Symphony reload: the build left the running image as it was; stamped"
        )),
        Err(why) => crate::front::tell(format_args!(
            "· Symphony reload: the build left the running image as it was; could not stamp it: \
             {why}"
        )),
    }
    if let Some(hub) = shared.api.get() {
        hub.tell_update(String::new());
    }
    true
}

/// Whether a replacement took over: this generation then drains. A failure is said on the
/// server's notice, this generation still serving.
fn replaced(shared: &Shared, result: Result<u32, String>) -> bool {
    match result {
        Ok(pid) => {
            crate::front::tell(format_args!(
                "· Symphony reload: replacement pid {pid} ready; draining pid {}",
                shared.pid
            ));
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
            crate::front::tell(format_args!("· Symphony reload: {message}"));
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
    *directories.lock().recover() = sources;
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
