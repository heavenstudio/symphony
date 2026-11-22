//! Timestamp the shared server's complete output, including inherited subprocess output.
//!
//! Each stream is a socket the server's own descriptor and every child's write to, read by a
//! forwarder that dates each line into the stream's destination. A forwarder ends only with the
//! capture ([`Log::finish`]): a socket that ends under it (a child shut down the one it inherited,
//! a read failed) would leave every later write to that stream failing, and `eprintln!` panics
//! the thread that makes one, so the forwarder puts a new socket in the stream's place and says
//! so ([`recapture`]).
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use symphony_board::lock::Recovered as _;
use symphony_board::sys::Stdio;

pub struct Log {
    saved: [OwnedFd; 2],
    control: Arc<Control>,
    readers: [std::thread::JoinHandle<io::Result<()>>; 2],
}

#[derive(Default)]
struct Control {
    stopping: AtomicBool,
    failure: Mutex<Failure>,
}

#[derive(Default)]
enum Failure {
    #[default]
    Clean,
    Pending(String),
    Reported,
}

impl Control {
    fn record(&self, out: &mut impl Write, report: &mut impl Write, bytes: &[u8]) {
        let error = line(out, bytes).err();
        let mut failure = self.failure.lock().recover();
        if let Some(error) = error
            && matches!(*failure, Failure::Clean)
        {
            *failure = Failure::Pending(format!(
                "server log write failed: {error}; output was dropped"
            ));
        }
        // Use the original stderr, never the socket we are draining. If its sink also failed,
        // keep the report pending so the first recovered record can report the loss once.
        if let Failure::Pending(message) = &*failure
            && line(report, message.as_bytes()).is_ok()
        {
            *failure = Failure::Reported;
        }
    }
}

static ORIGINAL_OUTPUT: Mutex<Option<[i32; 2]>> = Mutex::new(None);

/// A replacement generation must inherit the real destinations, rather than pipes serviced by
/// the generation that is about to exit. Take owned duplicates before spawning, not in pre_exec.
pub(crate) fn output_for_reload() -> io::Result<Option<(Stdio, Stdio)>> {
    let original = ORIGINAL_OUTPUT.lock().recover();
    original
        .as_ref()
        .map(|fds| {
            Ok((
                Stdio::from(duplicate(fds[0])?),
                Stdio::from(duplicate(fds[1])?),
            ))
        })
        .transpose()
}

fn duplicate(fd: i32) -> io::Result<OwnedFd> {
    // SAFETY: fcntl returns a new owned descriptor, with close-on-exec for our saved originals.
    let copy = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 3) };
    if copy < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the successful fcntl returned a descriptor we own.
    Ok(unsafe { OwnedFd::from_raw_fd(copy) })
}

/// How long a forwarder's read waits for output before it looks at the stop.
const LOOK: Duration = Duration::from_millis(100);

/// The process's `stream` (1 or 2) written into a new socket: the end a forwarder reads.
fn capture(stream: i32) -> io::Result<UnixStream> {
    let (reader, writer) = UnixStream::pair()?;
    reader.set_read_timeout(Some(LOOK))?;
    redirect(writer.as_raw_fd(), stream)?;
    Ok(reader)
}

/// What a forwarder does once the socket it read has ended.
enum Next<R> {
    /// The stream is captured again: read this.
    Again(R),
    /// The stream could not be captured again and writes to its destination itself, undated.
    Direct(String),
    /// The capture is over: there is nothing to read.
    Over,
}

/// The capture of the process's `stream` (1 or 2) taken again after its socket ended, while the
/// capture is active. One that ended again `at_once`, before anything was read of it, or that
/// cannot be made, gives the stream its own destination back instead: undated lines, and no
/// failing write.
fn recapture(stream: i32, at_once: bool) -> Next<UnixStream> {
    let original = ORIGINAL_OUTPUT.lock().recover();
    let Some(saved) = original.as_ref() else {
        return Next::Over;
    };
    let why = if at_once {
        "its new capture ended at once".to_string()
    } else {
        match capture(stream) {
            Ok(reader) => return Next::Again(reader),
            Err(error) => format!("it could not be captured again: {error}"),
        }
    };
    let index = usize::from(stream == 2);
    match redirect(saved[index], stream) {
        Ok(()) => Next::Direct(why),
        Err(error) => Next::Direct(format!(
            "{why}, and its destination did not take it: {error}"
        )),
    }
}

fn redirect(from: i32, to: i32) -> io::Result<()> {
    // SAFETY: both descriptors are open; dup2 atomically replaces only the destination.
    if unsafe { libc::dup2(from, to) } < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

impl Log {
    /// Called once before the runtime starts. Home has already connected both streams to its log;
    /// foreground callers retain their existing pipes. Preserve each stream's destination.
    pub fn start() -> io::Result<Self> {
        let mut original = ORIGINAL_OUTPUT.lock().recover();
        if original.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "server log capture is already active",
            ));
        }
        let saved = [duplicate(1)?, duplicate(2)?];
        let out = File::from(duplicate(1)?);
        let err = File::from(duplicate(2)?);
        let out_report = File::from(duplicate(2)?);
        let err_report = File::from(duplicate(2)?);
        let out_reader = capture(1)?;
        let err_reader = match capture(2) {
            Ok(reader) => reader,
            Err(error) => {
                redirect(saved[0].as_raw_fd(), 1)?;
                return Err(error);
            }
        };
        *original = Some([saved[0].as_raw_fd(), saved[1].as_raw_fd()]);
        drop(original);
        let control = Arc::new(Control::default());
        let out_control = control.clone();
        let err_control = control.clone();
        let readers = [
            std::thread::spawn(move || {
                forward(out_reader, out, out_report, &out_control, |at_once| {
                    recapture(1, at_once)
                })
            }),
            std::thread::spawn(move || {
                forward(err_reader, err, err_report, &err_control, |at_once| {
                    recapture(2, at_once)
                })
            }),
        ];
        Ok(Self {
            saved,
            control,
            readers,
        })
    }

    pub fn finish(self) -> io::Result<()> {
        io::stdout().flush()?;
        io::stderr().flush()?;
        let mut original = ORIGINAL_OUTPUT.lock().recover();
        redirect(self.saved[0].as_raw_fd(), 1)?;
        redirect(self.saved[1].as_raw_fd(), 2)?;
        *original = None;
        drop(original);
        self.control.stopping.store(true, Ordering::Release);
        let [out, err] = self.readers;
        let out = out
            .join()
            .map_err(|_| io::Error::other("server log writer panicked"))
            .and_then(|result| result);
        let err = err
            .join()
            .map_err(|_| io::Error::other("server log writer panicked"))
            .and_then(|result| result);
        out.and(err)
    }
}

fn stamp(at: SystemTime) -> String {
    let elapsed = at.duration_since(UNIX_EPOCH).unwrap_or_default();
    let seconds = libc::time_t::try_from(elapsed.as_secs()).unwrap_or(0);
    let mut date = std::mem::MaybeUninit::<libc::tm>::uninit();
    // SAFETY: gmtime_r writes the tm from our valid time_t into owned storage.
    if unsafe { libc::gmtime_r(&seconds, date.as_mut_ptr()) }.is_null() {
        return format!("unix:{}", elapsed.as_secs());
    }
    // SAFETY: the successful gmtime_r initialized tm above.
    let date = unsafe { date.assume_init() };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        date.tm_year + 1900,
        date.tm_mon + 1,
        date.tm_mday,
        date.tm_hour,
        date.tm_min,
        date.tm_sec,
        elapsed.subsec_millis()
    )
}

fn line(out: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    let mut record = format!("[{}] ", stamp(SystemTime::now())).into_bytes();
    record.extend_from_slice(bytes);
    record.push(b'\n');
    out.write_all(&record)
}

/// Date each line read from `input` into `out`, until the capture's stop. An `input` that ends
/// before it (its end, or a read that fails) is not the forwarder's end: what was read of a line is
/// written, `again` gives the input to read next (whether the one before ended at once, with
/// nothing read of it), and `report` says what ended and what reads now.
fn forward<R: Read>(
    mut input: R,
    mut out: impl Write,
    mut report: impl Write,
    control: &Control,
    mut again: impl FnMut(bool) -> Next<R>,
) -> io::Result<()> {
    let mut pending = Vec::new();
    let mut buffer = [0; 8192];
    // Whether this input has given nothing yet, not even a wait.
    let mut at_once = false;
    loop {
        // The stop as it stood when this read began. [`Log::finish`] stores it after the server's
        // last write, so only a read begun once it was seen has found every line written before
        // it: one that timed out first, its thread then held off the CPU past the stop, would end
        // the drain with the last lines, the stopped line among them, still unread.
        let stopped = control.stopping.load(Ordering::Acquire);
        let ended = match input.read(&mut buffer) {
            Ok(0) => "it ended".to_string(),
            Ok(n) => {
                at_once = false;
                // Search each byte once, even when a subprocess prints one very long line.
                for part in buffer[..n].split_inclusive(|b| *b == b'\n') {
                    if let Some(tail) = part.strip_suffix(b"\n") {
                        pending.extend_from_slice(tail);
                        control.record(&mut out, &mut report, &pending);
                        pending.clear();
                    } else {
                        pending.extend_from_slice(part);
                    }
                }
                continue;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                if stopped {
                    break;
                }
                at_once = false;
                continue;
            }
            Err(e) => format!("its read failed: {e}"),
        };
        if stopped || control.stopping.load(Ordering::Acquire) {
            break;
        }
        if !pending.is_empty() {
            control.record(&mut out, &mut report, &pending);
            pending.clear();
        }
        let (next, now) = match again(at_once) {
            Next::Again(next) => (Some(next), "captured again".to_string()),
            Next::Direct(why) => (None, format!("{why}: its lines go on undated")),
            Next::Over => break,
        };
        let said = format!("server log capture: {ended}; {now}");
        let _ = line(&mut report, said.as_bytes());
        let Some(next) = next else {
            break;
        };
        input = next;
        at_once = true;
    }
    if !pending.is_empty() {
        control.record(&mut out, &mut report, &pending);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphony_board::sys::Spawning as _;

    /// Each wait here is on its event; only a stall reaches this wall-clock backstop.
    const BACKSTOP: Duration = test_time::backstop(Duration::from_secs(5));

    /// The capture of a test whose input's end is its own: nothing is read after it.
    fn over<R>(_at_once: bool) -> Next<R> {
        Next::Over
    }

    /// What a wait that ran out says: `what` stalled, and the machine's load then.
    fn stalled(what: &str) -> String {
        format!(
            "{what}: stalled: the wait's wall-clock backstop of {BACKSTOP:?} ran out; {}",
            test_time::Load::now()
        )
    }

    #[test]
    fn timestamps_are_utc_with_milliseconds() {
        assert_eq!(
            stamp(UNIX_EPOCH + Duration::from_millis(1_800_000_000_123)),
            "2027-01-15T08:00:00.123Z"
        );
    }

    #[test]
    fn failed_sinks_keep_the_socket_open_and_report_once_after_recovery() {
        struct Recovering {
            failures: usize,
            bytes: Arc<Mutex<Vec<u8>>>,
            failed: Option<std::sync::mpsc::Sender<()>>,
        }
        impl Write for Recovering {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if self.failures != 0 {
                    self.failures -= 1;
                    if let Some(failed) = &self.failed {
                        failed.send(()).unwrap();
                    }
                    return Err(io::Error::from_raw_os_error(libc::ENOSPC));
                }
                self.bytes.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        // A pipe no child another test spawns can keep: the reader ends when `writer` does.
        let (input, mut writer) = symphony_board::sys::pipe().unwrap();
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let reports = Arc::new(Mutex::new(Vec::new()));
        let (failed, failures) = std::sync::mpsc::channel();
        let out = Recovering {
            failures: 2,
            bytes: bytes.clone(),
            failed: Some(failed),
        };
        let report = Recovering {
            failures: 2,
            bytes: reports.clone(),
            failed: None,
        };
        let reader =
            std::thread::spawn(move || forward(input, out, report, &Control::default(), over));
        for message in [b"dropped one\n".as_slice(), b"dropped two\n".as_slice()] {
            writer.write_all(message).unwrap();
            failures.recv_timeout(BACKSTOP).unwrap_or_else(|_| {
                panic!(
                    "{}",
                    stalled("the sink to actually fail before the next write")
                )
            });
        }
        // This write happens after both observed failures. A reader that exited on an error
        // either breaks this pipe or fails to forward this newly arriving record.
        writer.write_all(b"recovered\n").unwrap();
        drop(writer);
        reader.join().unwrap().unwrap();
        let text = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
        assert_eq!(&text[27..], "recovered\n");
        let reports = String::from_utf8(reports.lock().unwrap().clone()).unwrap();
        assert_eq!(reports.lines().count(), 1, "{reports}");
        assert!(reports.contains("server log write failed"), "{reports}");
    }

    #[test]
    fn both_streams_and_inherited_child_output_are_dated_and_drained() {
        const CHILD: &str = "SYMPHONY_LOG_TEST_HOME";
        const TEST: &str =
            "log::tests::both_streams_and_inherited_child_output_are_dated_and_drained";
        // Descriptor replacement is process-wide. Exercise it only in an owned test process,
        // leaving the parent harness and its other concurrently running tests untouched.
        if let Some(home) = std::env::var_os(CHILD) {
            let home = std::path::PathBuf::from(home);
            let original = [duplicate(1).unwrap(), duplicate(2).unwrap()];
            let out = File::create(home.join("stdout.log")).unwrap();
            let err = File::create(home.join("stderr.log")).unwrap();
            redirect(out.as_raw_fd(), 1).unwrap();
            redirect(err.as_raw_fd(), 2).unwrap();
            assert!(output_for_reload().unwrap().is_none());
            let log = Log::start().unwrap();
            assert!(output_for_reload().unwrap().is_some());
            println!("parent stdout");
            io::stdout().flush().unwrap();
            eprintln!("parent stderr");
            assert!(
                std::process::Command::new("sh")
                    .args([
                        "-c",
                        "printf 'child stdout\\n'; printf 'child stderr\\n' >&2"
                    ])
                    .status_locked()
                    .unwrap()
                    .success()
            );
            log.finish().unwrap();
            assert!(output_for_reload().unwrap().is_none());
            redirect(original[0].as_raw_fd(), 1).unwrap();
            redirect(original[1].as_raw_fd(), 2).unwrap();
            return;
        }
        let home =
            std::env::temp_dir().join(format!("symphony-log-process-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        let child = symphony_board::sys::output(
            symphony_board::sys::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST, "--nocapture"])
                .env(CHILD, &home),
        )
        .unwrap();
        assert!(
            child.status.success(),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        for stream in ["stdout", "stderr"] {
            let path = home.join(format!("{stream}.log"));
            let text = std::fs::read_to_string(&path).unwrap();
            let bodies = text
                .lines()
                .map(|line| {
                    assert!(
                        line.starts_with('[') && line.get(24..27) == Some("Z] "),
                        "{line}"
                    );
                    line[27..].to_string()
                })
                .collect::<Vec<_>>();
            assert_eq!(
                bodies,
                [format!("parent {stream}"), format!("child {stream}")],
                "each stream must retain its own destination"
            );
            std::fs::remove_file(path).unwrap();
        }
        std::fs::remove_dir(home).unwrap();
    }

    /// What `text`'s dated lines say, without their dates.
    fn bodies(text: &[u8]) -> Vec<String> {
        String::from_utf8_lossy(text)
            .lines()
            .map(|line| {
                assert_eq!(line.get(24..27), Some("Z] "), "{line}");
                line[27..].to_string()
            })
            .collect()
    }

    /// An input that gives `parts` in turn and then fails as `end` says, or ends.
    struct Ending {
        parts: Vec<&'static [u8]>,
        end: Option<io::ErrorKind>,
    }

    impl Read for Ending {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.parts.is_empty() {
                return self.end.map_or(Ok(0), |kind| Err(kind.into()));
            }
            let part = self.parts.remove(0);
            buffer[..part.len()].copy_from_slice(part);
            Ok(part.len())
        }
    }

    /// The socket a stream is captured by ends under its forwarder. The forwarder reads the capture taken in its place, having written the line
    /// the end cut short, and says so once for each end; a read that fails is such an end too.
    #[test]
    fn a_capture_that_ends_before_the_stop_is_taken_again_and_said() {
        let mut next = vec![
            Ending {
                parts: vec![b"second\n"],
                end: Some(io::ErrorKind::ConnectionReset),
            },
            Ending {
                parts: vec![b"third\n"],
                end: None,
            },
        ];
        let mut asked = Vec::new();
        let (mut out, mut report) = (Vec::new(), Vec::new());
        forward(
            Ending {
                parts: vec![b"first\ncut ", b"short"],
                end: None,
            },
            &mut out,
            &mut report,
            &Control::default(),
            |at_once| {
                asked.push(at_once);
                if next.is_empty() {
                    Next::Over
                } else {
                    Next::Again(next.remove(0))
                }
            },
        )
        .unwrap();
        assert_eq!(bodies(&out), ["first", "cut short", "second", "third"]);
        assert_eq!(asked, [false, false, false], "each gave a line first");
        assert_eq!(
            bodies(&report),
            [
                "server log capture: it ended; captured again",
                "server log capture: its read failed: connection reset; captured again"
            ]
        );
    }

    /// A capture taken again that ends with nothing read of it is said to its taker, which gives
    /// the stream its destination back: the forwarder says that and ends, never a spin.
    #[test]
    fn a_capture_that_ends_at_once_again_is_said_to_its_taker_and_ends_the_forwarder_when_direct() {
        let mut asked = Vec::new();
        let (mut out, mut report) = (Vec::new(), Vec::new());
        let ended = || Ending {
            parts: Vec::new(),
            end: None,
        };
        forward(
            ended(),
            &mut out,
            &mut report,
            &Control::default(),
            |at_once| {
                asked.push(at_once);
                if at_once {
                    Next::Direct("its new capture ended at once".into())
                } else {
                    Next::Again(ended())
                }
            },
        )
        .unwrap();
        assert_eq!(asked, [false, true]);
        assert!(out.is_empty());
        assert_eq!(
            bodies(&report),
            [
                "server log capture: it ended; captured again",
                "server log capture: it ended; its new capture ended at once: its lines go on \
                 undated"
            ]
        );
    }

    /// Once the capture is stopping, an input's end is the forwarder's: nothing is taken again.
    #[test]
    fn a_capture_that_ends_at_its_stop_is_not_taken_again() {
        let control = Control {
            stopping: AtomicBool::new(true),
            ..Control::default()
        };
        let (mut out, mut report) = (Vec::new(), Vec::new());
        forward(
            Ending {
                parts: vec![b"last"],
                end: Some(io::ErrorKind::ConnectionReset),
            },
            &mut out,
            &mut report,
            &control,
            |_| -> Next<Ending> { panic!("asked for another capture at the stop") },
        )
        .unwrap();
        assert_eq!(bodies(&out), ["last"]);
        assert!(report.is_empty());
    }

    /// The stream's own descriptor, in an owned test process (as the test above): a child that
    /// shuts down the socket it inherited as its stderr ends it for every writer. The forwarder
    /// puts a new socket in the stream's place, so the server's next line is dated into the log
    /// and the `eprintln!` that writes it does not panic its thread; the other stream is as it was.
    #[test]
    fn a_stream_whose_socket_a_child_shut_down_is_captured_again() {
        const CHILD: &str = "SYMPHONY_LOG_SHUT_HOME";
        const TEST: &str = "log::tests::a_stream_whose_socket_a_child_shut_down_is_captured_again";
        if let Some(home) = std::env::var_os(CHILD) {
            let home = std::path::PathBuf::from(home);
            let original = [duplicate(1).unwrap(), duplicate(2).unwrap()];
            let out = File::create(home.join("stdout.log")).unwrap();
            let err = File::create(home.join("stderr.log")).unwrap();
            redirect(out.as_raw_fd(), 1).unwrap();
            redirect(err.as_raw_fd(), 2).unwrap();
            let log = Log::start().unwrap();
            eprintln!("before the shutdown");
            // As a child does to the socket it inherited: the one socket, whoever holds it.
            // SAFETY: shutdown only changes the state of the socket descriptor 2 names.
            assert_eq!(unsafe { libc::shutdown(2, libc::SHUT_WR) }, 0);
            // Said once the new socket is in the stream's place.
            test_time::until_true("stderr to be captured again", BACKSTOP, || {
                std::fs::read_to_string(home.join("stderr.log"))
                    .is_ok_and(|log| log.contains("captured again"))
            })
            .unwrap_or_else(|stall| panic!("{stall}"));
            eprintln!("after the shutdown");
            println!("the other stream");
            io::stdout().flush().unwrap();
            log.finish().unwrap();
            redirect(original[0].as_raw_fd(), 1).unwrap();
            redirect(original[1].as_raw_fd(), 2).unwrap();
            return;
        }
        let home = std::env::temp_dir().join(format!("symphony-log-shut-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        let child = symphony_board::sys::output(
            symphony_board::sys::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST, "--nocapture"])
                .env(CHILD, &home),
        )
        .unwrap();
        let read = |stream: &str| std::fs::read(home.join(format!("{stream}.log"))).unwrap();
        let (out, err) = (read("stdout"), read("stderr"));
        std::fs::remove_dir_all(&home).unwrap();
        assert!(
            child.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&child.stderr),
            String::from_utf8_lossy(&err)
        );
        assert_eq!(
            bodies(&err),
            [
                "before the shutdown",
                "server log capture: it ended; captured again",
                "after the shutdown"
            ]
        );
        assert_eq!(bodies(&out), ["the other stream"]);
    }

    #[test]
    fn multiline_fragmented_output_and_an_unterminated_tail_all_get_dated() {
        let path = std::env::temp_dir().join(format!("symphony-log-{}", std::process::id()));
        let (input, mut writer) = symphony_board::sys::pipe().unwrap();
        let sender = std::thread::spawn(move || {
            writer.write_all(b"one\n").unwrap();
            writer.write_all(&[b'x'; 9000]).unwrap();
            writer.write_all(b"\ntrailing").unwrap();
        });
        forward(
            input,
            File::create(&path).unwrap(),
            io::sink(),
            &Control::default(),
            over,
        )
        .unwrap();
        sender.join().unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        let lines = text.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 3);
        for line in &lines {
            assert_eq!(&line[24..27], "Z] ");
        }
        assert_eq!(&lines[0][27..], "one");
        assert_eq!(&lines[1][27..], "x".repeat(9000));
        assert_eq!(&lines[2][27..], "trailing");
    }

    #[test]
    fn shutdown_does_not_wait_for_an_inherited_writer_to_close() {
        let path = std::env::temp_dir().join(format!("symphony-log-held-{}", std::process::id()));
        let (input, held_writer) = UnixStream::pair().unwrap();
        input
            .set_read_timeout(Some(Duration::from_millis(10)))
            .unwrap();
        let out = File::create(&path).unwrap();
        let (send, recv) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            send.send(forward(
                input,
                out,
                io::sink(),
                &Control {
                    stopping: AtomicBool::new(true),
                    ..Control::default()
                },
                over,
            ))
            .unwrap();
        });
        let ended = recv.recv_timeout(BACKSTOP);
        drop(held_writer);
        thread.join().unwrap();
        ended
            .unwrap_or_else(|_| {
                panic!(
                    "{}",
                    stalled("the log writer to stop while the inherited writer is still open")
                )
            })
            .unwrap();
        assert!(std::fs::read(&path).unwrap().is_empty());
        std::fs::remove_file(path).unwrap();
    }

    /// The stop of a loaded machine: the forwarder's read times out with nothing to read, and
    /// before its thread runs again the server writes its last line and [`Log::finish`] stores
    /// the stop. That line is still forwarded, and the forwarder still ends while a child holds
    /// the writer open.
    #[test]
    fn a_line_written_before_the_stop_reaches_the_log_after_a_read_timed_out_before_it() {
        /// The socket's reads, but for the first: it has timed out, and its answer comes back
        /// only after the server's last line and the stop. `writer` stays open as a child's would.
        struct Late {
            input: UnixStream,
            writer: UnixStream,
            control: Arc<Control>,
            timed_out: bool,
        }
        impl Read for Late {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                if self.timed_out {
                    return self.input.read(buffer);
                }
                self.timed_out = true;
                (&self.writer).write_all(b"Symphony server stopped\n")?;
                self.control.stopping.store(true, Ordering::Release);
                Err(io::ErrorKind::WouldBlock.into())
            }
        }
        let (input, writer) = UnixStream::pair().unwrap();
        input
            .set_read_timeout(Some(Duration::from_millis(10)))
            .unwrap();
        let control = Arc::new(Control::default());
        let late = Late {
            input,
            writer,
            control: control.clone(),
            timed_out: false,
        };
        let mut out = Vec::new();
        forward(late, &mut out, io::sink(), &control, over).unwrap();
        let text = String::from_utf8(out).unwrap();
        let bodies: Vec<_> = text.lines().map(|line| &line[27..]).collect();
        assert_eq!(bodies, ["Symphony server stopped"], "{text}");
    }
}
