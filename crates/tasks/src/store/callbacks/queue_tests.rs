//! Delivery through `codex queue`: each case runs again in a process of its own, beside a stand-in
//! that is the only codex on its `PATH`.

use super::*;
use crate::sys::Command;
use std::time::Instant;

/// Where a re-run of a queue test below finds its folder, whose stand-in is the only codex on its
/// `PATH`.
const QUEUE_CASE: &str = "SF_CODEX_QUEUE_CASE";

/// The codex queue stand-in. In mode `wait` it ends once a line reaches its release; otherwise it
/// says what it was given and ends as its mode says.
const CODEX: &str = r#"#!/bin/sh
here=${0%/*}
read -r mode value < "$here/mode"
if [ "$mode" = wait ]; then
    read -r line < "$here/release"
    exit 0
fi
printf '%s\0' "$@" > "$here/argv"
input=other; output=other; error=other
test /dev/fd/0 -ef /dev/null && input=null
test /dev/fd/1 -ef /dev/null && output=null
test /dev/fd/2 -ef /dev/null && error=null
printf '%s|' "${CLAUDE_CODE_MESSAGING_TOKEN-unset}" "${SF_CODEX_QUEUE_KEPT-unset}" \
    "$input" "$output" "$error" > "$here/saw"
printf %s "$$" > "$here/pid"
case $mode in
    exit) exit "$value" ;;
    killed) kill -KILL $$ ;;
    hang) exec sleep 600 ;;
esac
"#;

/// A folder of its own under the temporary one, holding the stand-in.
fn stand_in(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sf-codex-queue-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    fixture_script::write(&dir.join("codex"), CODEX, false);
    dir
}

/// Test `name` run again in a process of its own, with `dir`'s stand-in the only codex on its
/// `PATH` and a messaging token set: set here, they would reach every other test's children too.
/// None of its streams is `/dev/null`, so a queue that inherited one would not read null.
fn rerun(name: &str, dir: &std::path::Path) {
    let case = crate::sys::spawn_piped(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &format!("store::callbacks::queue_tests::{name}")])
            .env(QUEUE_CASE, dir)
            .env("PATH", format!("{}:/usr/bin:/bin", dir.display()))
            .env("CLAUDE_CODE_MESSAGING_TOKEN", "a-token")
            .env("SF_CODEX_QUEUE_KEPT", "kept"),
    )
    .unwrap();
    let done = crate::sys::wait_output(case).unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&done.stdout),
        String::from_utf8_lossy(&done.stderr)
    );
    assert!(done.status.success(), "{said}");
    assert!(said.contains("test result: ok. 1 passed"), "{said}");
    std::fs::remove_dir_all(dir).unwrap();
}

/// How a delivery ended: a refusal is always one the outbox retries.
fn answered(result: Result<(), DeliveryError>) -> Result<(), String> {
    result.map_err(|error| {
        assert!(error.kind == DeliveryFailure::Retry, "{error}");
        error.message
    })
}

#[test]
fn codex_queue_takes_the_message_whole_without_the_token_and_answers_how_it_ended() {
    let Some(dir) = std::env::var_os(QUEUE_CASE).map(PathBuf::from) else {
        let dir = stand_in("contract");
        rerun(
            "codex_queue_takes_the_message_whole_without_the_token_and_answers_how_it_ended",
            &dir,
        );
        return;
    };
    const MESSAGE: &str = "AGENT from a to b — one\n-- 'two' $HOME";
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
    // The stand-in in `mode`, with no record of an earlier run left to be read for this one's.
    let staged = |mode: &str| {
        for record in ["argv", "saw", "pid"] {
            match std::fs::remove_file(dir.join(record)) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => panic!("{error}"),
                _ => {}
            }
        }
        std::fs::write(dir.join("mode"), mode).unwrap();
    };
    let queued = |mode: &str, due: &dyn Fn(Duration) -> bool| {
        staged(mode);
        answered(queue_until("abcdef01", MESSAGE, due))
    };
    // A stand-in in these modes ends by itself, however late a loaded machine starts it: with no
    // deadline before its end, the answer is the one its end gives, and its record is there.
    let never = |_: Duration| false;
    assert_eq!(queued("exit 0", &never), Ok(()));
    assert_eq!(
        read("argv"),
        format!("queue\0--thread\0abcdef01\0--message\0{MESSAGE}\0")
    );
    assert_eq!(read("saw"), "unset|kept|null|null|null|");
    assert_eq!(
        queued("exit 3", &never),
        Err("codex queue exited exit status: 3".into())
    );
    assert_eq!(
        queued("killed", &never),
        Err("codex queue exited signal: 9 (SIGKILL)".into())
    );
    // A deadline that comes once the stand-in has written its pid, its last word before it hangs:
    // the queue it says it is, is the one ended and reaped.
    let started = || std::fs::read_to_string(dir.join("pid")).is_ok_and(|pid| !pid.is_empty());
    assert_eq!(
        queued("hang", &|_| started()),
        Err("codex queue timed out".into())
    );
    let pid: i64 = read("pid").parse().unwrap();
    assert!(
        !crate::sys::pid_alive(pid),
        "the timed-out queue is ended and reaped"
    );
    // Delivery's own deadline, whether or not the stand-in has started by then: its `sleep 600`
    // would end it well (a queue that answers), so "timed out" is the kill at 5 s, never before.
    staged("hang");
    let began = Instant::now();
    assert_eq!(
        answered(queue("abcdef01", MESSAGE)),
        Err("codex queue timed out".into())
    );
    let took = began.elapsed();
    assert!(took >= Duration::from_secs(5), "{took:?}");
    std::fs::remove_file(dir.join("codex")).unwrap();
    assert_eq!(
        answered(queue("abcdef01", MESSAGE)),
        Err(std::io::Error::from_raw_os_error(libc::ENOENT).to_string())
    );
}

/// What libproc (`libproc.h`, `sys/proc_info.h`) lists for process `pid`: its children, and what
/// each of its descriptors is (`PROX_FDTYPE_*`), none once the process has ended.
#[cfg(target_os = "macos")]
fn listed(pid: libc::pid_t) -> (Vec<libc::pid_t>, BTreeMap<std::os::fd::RawFd, u32>) {
    let mut children: [libc::pid_t; 64] = [0; 64];
    let size = libc::c_int::try_from(size_of_val(&children)).unwrap();
    // SAFETY: libproc writes at most `size` bytes of pids into the buffer.
    let found = unsafe { libc::proc_listchildpids(pid, children.as_mut_ptr().cast(), size) };
    assert!(found >= 0, "{}", std::io::Error::last_os_error());
    let empty = libc::proc_fdinfo {
        proc_fd: 0,
        proc_fdtype: 0,
    };
    let mut table = vec![empty; 4096];
    let size = libc::c_int::try_from(size_of_val(table.as_slice())).unwrap();
    // SAFETY: libproc writes at most `size` bytes of whole entries into the table.
    let used = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDLISTFDS,
            0,
            table.as_mut_ptr().cast(),
            size,
        )
    };
    table.truncate(usize::try_from(used).unwrap_or(0) / size_of::<libc::proc_fdinfo>());
    (
        children.into_iter().filter(|&child| child > 0).collect(),
        table
            .into_iter()
            .map(|entry| (entry.proc_fd, entry.proc_fdtype))
            .collect(),
    )
}

/// The Symphony server's case: a connection accepted on macOS is not close-on-exec until a second
/// call marks it, and a delivery started between the two must neither hold it nor keep its client
/// waiting for the end of its answer. The queue's child is read from outside, by its descriptor
/// table, so the case holds however late the stand-in itself starts, and its deadline is one only
/// a stuck queue reaches. It runs on macOS alone because the gap is macOS's: Linux's accept marks
/// a connection close-on-exec in the call that makes it, so no connection there is ever open to a
/// spawn without the mark, and a child std starts there would keep the one this case leaves
/// unmarked.
#[cfg(target_os = "macos")]
#[test]
fn an_accepted_connection_not_yet_close_on_exec_reaches_no_codex_queue() {
    use std::fs::File;
    use std::io::Read;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    let Some(dir) = std::env::var_os(QUEUE_CASE).map(PathBuf::from) else {
        let dir = stand_in("descriptor");
        rerun(
            "an_accepted_connection_not_yet_close_on_exec_reaches_no_codex_queue",
            &dir,
        );
        return;
    };
    let release = dir.join("release");
    let name = std::ffi::CString::new(release.as_os_str().as_bytes()).unwrap();
    // SAFETY: the path is NUL-terminated and outlives the call.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    // Open for reading and writing here, the release waits for no other end, and the stand-in's
    // open waits for none either; a line written to it stays there for a stand-in however late it
    // starts.
    let mut go = File::options()
        .read(true)
        .write(true)
        .open(&release)
        .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut client = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (accepted, _) = listener.accept().unwrap();
    // SAFETY: this test owns the descriptor; only its close-on-exec flag changes.
    assert_eq!(
        unsafe { libc::fcntl(accepted.as_raw_fd(), libc::F_SETFD, 0) },
        0
    );
    let socket = u32::try_from(libc::PROX_FDTYPE_SOCKET).unwrap();
    let own = libc::pid_t::try_from(std::process::id()).unwrap();
    let (_, ours) = listed(own);
    assert_eq!(
        ours.get(&accepted.as_raw_fd()),
        Some(&socket),
        "the table of a process that holds the connection lists it: {ours:?}"
    );
    std::fs::write(dir.join("mode"), "wait").unwrap();
    // Delivery's 5 s kill is no part of this case: a deadline no loaded machine reaches before the
    // release, so the child is there to be read however long this test is kept from reading it.
    let due = |since: Duration| since >= RELEASED_WITHIN;
    std::thread::scope(|scope| {
        let queued = scope.spawn(|| answered(queue_until("abcdef01", "a message", due)));
        // The queue's child, read under the spawn lock: after its spawn has left it its streams
        // alone, and while it waits for its release.
        let table = loop {
            {
                let _spawning = crate::sys::spawning();
                match listed(own).0[..] {
                    [] => {}
                    [child] => break listed(child).1,
                    ref many => panic!("this case's one child is its queue: {many:?}"),
                }
            }
            if queued.is_finished() {
                break BTreeMap::new();
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        if table.is_empty() {
            let ended = queued.join().unwrap();
            panic!("codex queue ended ({ended:?}) before its descriptors were read");
        }
        assert!(
            !table.values().any(|&kind| kind == socket),
            "codex queue holds a socket: {table:?}"
        );
        // The server answers and closes its end: the client reads to the answer's end before the
        // queue is released.
        (&accepted).write_all(b"answer").unwrap();
        drop(accepted);
        let mut said = Vec::new();
        client.read_to_end(&mut said).unwrap();
        assert_eq!(said, b"answer");
        writeln!(go, "go").unwrap();
        // Released, the stand-in ends, however late it started.
        assert_eq!(queued.join().unwrap(), Ok(()));
    });
}

/// The deadline of a queue the case above releases: the wall-clock backstop of the delivery's own
/// 5 s, which only a stuck queue reaches.
#[cfg(target_os = "macos")]
const RELEASED_WITHIN: Duration = Duration::from_secs(120);
