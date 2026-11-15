//! A run's log: its plain text whatever the reads cut, the copy of a command's streams on to where
//! they went and into the log in the order read, a reader gone or a writer kept, the copy of what
//! a caller's files gained, and the read of part of one, only of a regular file in a day folder of
//! the logs.

use super::*;
use std::path::PathBuf;
use std::time::Instant;

/// The backstop of a wait for what the copy's thread does, which it does at once on a quiet
/// machine.
const PATIENT: Duration = test_time::backstop(Duration::from_secs(10));
const NOW: f64 = 1_794_400_000.0; // 2026-11-11, 12:26 UTC

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("symphony-run-logs-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(letter: char) -> String {
    letter.to_string().repeat(32)
}

fn plain(chunks: &[&[u8]]) -> Vec<u8> {
    let mut stripper = Plain::default();
    let mut out = Vec::new();
    for chunk in chunks {
        stripper.strip(chunk, &mut out);
    }
    out
}

#[test]
fn a_plain_log_keeps_the_text_and_drops_every_escape_wherever_a_read_cuts_it() {
    let written: &[u8] = b"\x1b[1;31merror\x1b[0m: x\r\n\
        \x1b]0;a title\x07after a title\n\
        \x1b]8;;https://example.com\x1b\\a link\x1b]8;;\x1b\\\n\
        \x1b(Bcharset\x1b7\x1b8 \xc3\x9b\xe2\x80\xba ok\tand a tab\n\
        \x1bPdevice data\x1b\\done\x1b[?25l\x1b[2K\n";
    let kept: &[u8] =
        b"error: x\r\nafter a title\na link\ncharset \xc3\x9b\xe2\x80\xba ok\tand a tab\ndone\n";
    assert_eq!(plain(&[written]), kept);
    for cut in 0..=written.len() {
        let (one, two) = written.split_at(cut);
        assert_eq!(plain(&[one, two]), kept, "cut at {cut}");
    }
    let bytes: Vec<&[u8]> = written.chunks(1).collect();
    assert_eq!(plain(&bytes), kept, "a byte a read");
}

#[test]
fn an_escape_never_ended_takes_no_more_than_its_line_and_what_no_escape_takes_is_kept() {
    for (written, kept) in [
        // A string never ended stops at its line's end.
        (&b"\x1b]never ended\nnext\n"[..], &b"\nnext\n"[..]),
        (b"\x1bPnever ended\x1b\nnext\n", b"\nnext\n"),
        // A control character inside a control sequence acts, and the sequence goes on.
        (b"\x1b[1\n3mred\n", b"\nred\n"),
        // An escape cut by another starts again.
        (b"\x1b\x1b[31mred", b"red"),
        (b"\x1b[3\x1b[0mx", b"x"),
        // What no escape takes ends one, and is kept.
        (b"\x1b\nline", b"\nline"),
        (b"\x1b\xc3\x9b", b"\xc3\x9b"),
        (b"\x1b[31\xc3\x9b", b"\xc3\x9b"),
        // Nothing to strip.
        (b"", b""),
        (b"plain\x07bell\x08", b"plain\x07bell\x08"),
    ] {
        assert_eq!(
            plain(&[written]),
            kept,
            "{}",
            String::from_utf8_lossy(written)
        );
    }
}

/// A pipe as files: its reading end, its writing end.
fn pipe() -> (File, File) {
    let (read, write) = crate::sys::pipe().unwrap();
    (
        File::from(std::os::fd::OwnedFd::from(read)),
        File::from(std::os::fd::OwnedFd::from(write)),
    )
}

/// Everything `reader` holds until its writers are gone.
fn all(mut reader: File) -> Vec<u8> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    bytes
}

/// Once `copying` counts at least `bytes` in its log.
#[track_caller]
fn counted(copying: &Copying, bytes: u64) {
    test_time::until_true(format!("{bytes} bytes logged"), PATIENT, || {
        copying.drained(Duration::ZERO) >= bytes
    })
    .unwrap_or_else(|stall| panic!("{stall}: {} logged", copying.drained(Duration::ZERO)));
}

#[test]
fn the_copy_writes_each_stream_on_as_it_came_and_its_log_plain_in_the_order_read() {
    let dir = scratch("copy");
    let name = name(&run('a'), NOW);
    let log = create(&dir, &name).unwrap();
    let ((out_from, mut out_writer), (out_reader, out_to)) = (pipe(), pipe());
    let ((err_from, mut err_writer), (err_reader, err_to)) = (pipe(), pipe());
    let streams = vec![
        Stream {
            from: out_from,
            to: out_to,
        },
        Stream {
            from: err_from,
            to: err_to,
        },
    ];
    let copying = Copying::start(streams, log).unwrap();
    // Each written once the one before is in the log: the log keeps the order they came in.
    out_writer.write_all(b"\x1b[32mone\x1b[0m\n").unwrap();
    counted(&copying, 4);
    err_writer.write_all(b"two\n").unwrap();
    counted(&copying, 8);
    out_writer.write_all(b"three\n").unwrap();
    drop((out_writer, err_writer));
    assert_eq!(copying.drained(PATIENT), 14);
    copying.join();
    assert_eq!(all(out_reader), b"\x1b[32mone\x1b[0m\nthree\n");
    assert_eq!(all(err_reader), b"two\n");
    assert_eq!(fs::read(dir.join(&name)).unwrap(), b"one\ntwo\nthree\n");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_stream_whose_reader_went_is_read_no_more_and_its_writer_finds_that() {
    let dir = scratch("copy-gone");
    let name = name(&run('a'), NOW);
    let log = create(&dir, &name).unwrap();
    let ((out_from, mut out_writer), (out_reader, out_to)) = (pipe(), pipe());
    let ((err_from, mut err_writer), (err_reader, err_to)) = (pipe(), pipe());
    drop(out_reader);
    let streams = vec![
        Stream {
            from: out_from,
            to: out_to,
        },
        Stream {
            from: err_from,
            to: err_to,
        },
    ];
    let copying = Copying::start(streams, log).unwrap();
    out_writer.write_all(b"lost\n").unwrap();
    counted(&copying, 5);
    // The copy dropped its end: the command's next write is refused, as it was before the copy.
    let refused = test_time::until("a write refused", PATIENT, || {
        out_writer.write_all(b"again\n").err()
    })
    .unwrap();
    assert_eq!(refused.kind(), io::ErrorKind::BrokenPipe);
    // The other stream goes on, copied and logged.
    err_writer.write_all(b"still\n").unwrap();
    drop((out_writer, err_writer));
    copying.drained(PATIENT);
    copying.join();
    assert_eq!(all(err_reader), b"still\n");
    let logged = fs::read_to_string(dir.join(&name)).unwrap();
    assert!(
        logged.starts_with("lost\n") && logged.ends_with("still\n"),
        "{logged:?}"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_drain_waits_no_longer_than_it_is_given_for_a_stream_a_process_still_holds() {
    let dir = scratch("copy-held");
    let name = name(&run('a'), NOW);
    let log = create(&dir, &name).unwrap();
    let ((from, mut writer), (reader, to)) = (pipe(), pipe());
    let copying = Copying::start(vec![Stream { from, to }], log).unwrap();
    writer.write_all(b"now\n").unwrap();
    counted(&copying, 4);
    // Its promise is a time: it says what the log holds once that time passed.
    let within = Duration::from_millis(50);
    let began = Instant::now();
    assert_eq!(copying.drained(within), 4);
    assert!(began.elapsed() >= within, "{:?}", began.elapsed());
    // What comes later is still copied on and kept.
    writer.write_all(b"later\n").unwrap();
    drop(writer);
    assert_eq!(copying.drained(PATIENT), 10);
    copying.join();
    assert_eq!(all(reader), b"now\nlater\n");
    assert_eq!(fs::read(dir.join(&name)).unwrap(), b"now\nlater\n");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_caller_that_cannot_take_more_yet_is_waited_for_and_a_log_that_fails_stops_only_the_log() {
    let dir = scratch("copy-full");
    let name = name(&run('a'), NOW);
    drop(create(&dir, &name).unwrap());
    // A log that refuses every write: opened to read alone.
    let log = File::open(dir.join(&name)).unwrap();
    let ((from, mut writer), (reader, mut to)) = (pipe(), pipe());
    // A caller's stream left non-blocking (a terminal a program set so), full: it says so.
    // SAFETY: sets a flag of a descriptor this test owns.
    unsafe {
        let flags = libc::fcntl(to.as_raw_fd(), libc::F_GETFL);
        assert_ne!(
            libc::fcntl(to.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK),
            -1
        );
    }
    let mut expected = Vec::new();
    loop {
        match to.write(&[b'.'; 4096]) {
            Ok(wrote) => expected.resize(expected.len() + wrote, b'.'),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) => panic!("{error}"),
        }
    }
    // What the source holds unread, seen beside the copy.
    let source = from.try_clone().unwrap();
    let unread = || {
        let mut held: libc::c_int = 0;
        // SAFETY: FIONREAD writes one int into it.
        assert_ne!(
            unsafe { libc::ioctl(source.as_raw_fd(), libc::FIONREAD, &raw mut held) },
            -1
        );
        held
    };
    let copying = Copying::start(vec![Stream { from, to }], log).unwrap();
    writer.write_all(b"first").unwrap();
    // The copy took it, and cannot hand it on until the caller reads.
    test_time::until_true("the copy read the source", PATIENT, || unread() == 0).unwrap();
    expected.extend_from_slice(b"first");
    let reading = std::thread::spawn(move || all(reader));
    // More than a pipe holds, after it.
    let sent: Vec<u8> = (0..1024 * 1024_u32).map(|at| (at % 251) as u8).collect();
    writer.write_all(&sent).unwrap();
    drop(writer);
    expected.extend_from_slice(&sent);
    assert_eq!(
        copying.drained(PATIENT),
        0,
        "a log that failed counts nothing"
    );
    copying.join();
    assert!(
        reading.join().unwrap() == expected,
        "the caller missed or repeated bytes"
    );
    assert_eq!(fs::read(dir.join(&name)).unwrap(), b"");
    fs::remove_dir_all(&dir).unwrap();
}

/// `path` made holding `holds`, and a handle on it that only writes, at its end: as a caller's
/// `>>` hands it.
fn appended(path: &Path, holds: &[u8]) -> File {
    fs::write(path, holds).unwrap();
    OpenOptions::new().append(true).open(path).unwrap()
}

#[test]
fn a_callers_files_are_read_again_from_where_they_ended_a_file_both_streams_name_once() {
    let dir = scratch("files");
    let name = name(&run('a'), NOW);
    let log = create(&dir, &name).unwrap();
    // `>>both 2>&1`: two handles, open to write alone, on one file that held a line already.
    let mut out = appended(&dir.join("both"), b"before\n");
    let mut err = out.try_clone().unwrap();
    let files = Files::mark([out.try_clone().unwrap(), err.try_clone().unwrap()]);
    out.write_all(b"\x1b[1;32mone\x1b[0m\n").unwrap();
    err.write_all(b"two \x1b]0;a title\x07on stderr\n").unwrap();
    out.write_all(b"three\n").unwrap();
    // The caller's file holds what was written, escapes and all; the log what the command added,
    // plain, once.
    assert_eq!(files.copy(&log), 24);
    assert_eq!(
        fs::read(dir.join("both")).unwrap(),
        b"before\n\x1b[1;32mone\x1b[0m\ntwo \x1b]0;a title\x07on stderr\nthree\n"
    );
    assert_eq!(
        fs::read(dir.join(&name)).unwrap(),
        b"one\ntwo on stderr\nthree\n"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_callers_files_are_copied_once_by_whichever_asks_first_and_what_comes_after_is_not() {
    let dir = scratch("files-once");
    let name = name(&run('a'), NOW);
    let log = create(&dir, &name).unwrap();
    let mut out = appended(&dir.join("out"), b"");
    let source = Source::files(Files::mark([out.try_clone().unwrap()]), log);
    let ending = source.ending().expect("a source of files has an ending");
    out.write_all(b"ran\n").unwrap();
    // The thread that saw the command end copies; the record's ask waits for that copy.
    let seen = std::thread::spawn(move || ending.copy());
    assert_eq!(source.ended(Duration::ZERO), 4);
    assert_eq!(seen.join().unwrap(), 4);
    // Its caller's next writes, after it ended, are its caller's.
    out.write_all(b"next\n").unwrap();
    assert_eq!(source.ended(Duration::ZERO), 4);
    source.join();
    assert_eq!(fs::read(dir.join(&name)).unwrap(), b"ran\n");
    // Streams copied as they come have no such ending.
    let ((from, writer), (_reader, to)) = (pipe(), pipe());
    let other = create(&dir, &super::name(&run('b'), NOW)).unwrap();
    let copying = Source::Copying(Copying::start(vec![Stream { from, to }], other).unwrap());
    assert!(copying.ending().is_none());
    drop(writer);
    copying.join();
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn two_files_are_read_in_turn_each_from_its_own_mark_and_plain_on_its_own() {
    let dir = scratch("files-two");
    let name = name(&run('a'), NOW);
    let log = create(&dir, &name).unwrap();
    let mut out = appended(&dir.join("out"), b"old out\n");
    let mut err = appended(&dir.join("err"), b"");
    let files = Files::mark([out.try_clone().unwrap(), err.try_clone().unwrap()]);
    // Written the other way about, and an escape cut at one file's end never takes the other's.
    err.write_all(b"first on stderr\n").unwrap();
    out.write_all(b"on stdout \x1b[3").unwrap();
    assert_eq!(files.copy(&log), 26);
    assert_eq!(
        fs::read(dir.join(&name)).unwrap(),
        b"on stdout first on stderr\n"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn what_is_no_regular_file_is_left_out_and_a_file_cut_short_or_a_log_that_fails_gives_nothing() {
    let dir = scratch("files-none");
    let name = name(&run('a'), NOW);
    let log = create(&dir, &name).unwrap();
    // A pipe and `/dev/null` are no files to read again.
    let (_reader, mut piped) = pipe();
    let null = OpenOptions::new().write(true).open("/dev/null").unwrap();
    let none = Files::mark([piped.try_clone().unwrap(), null]);
    piped.write_all(b"piped\n").unwrap();
    assert_eq!(none.copy(&log), 0);
    // A file its caller cut below its mark since gives nothing, whatever it holds now.
    let mut cut = appended(&dir.join("cut"), b"0123456789\n");
    let shrunk = Files::mark([cut.try_clone().unwrap()]);
    cut.set_len(0).unwrap();
    cut.write_all(b"short\n").unwrap();
    assert_eq!(shrunk.copy(&log), 0);
    assert_eq!(fs::read(dir.join(&name)).unwrap(), b"");
    // A log that refuses its writes counts nothing.
    let mut grown = appended(&dir.join("grown"), b"");
    let files = Files::mark([grown.try_clone().unwrap()]);
    grown.write_all(b"lost\n").unwrap();
    let refusing = File::open(dir.join(&name)).unwrap();
    assert_eq!(files.copy(&refusing), 0);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_log_is_named_by_its_day_and_run_made_new_and_never_through_a_link() {
    let dir = scratch("create");
    let name = name(&run('a'), NOW);
    assert_eq!(name, format!("logs/2026-11-11/{}.log", run('a')));
    create(&dir, &name).unwrap().write_all(b"kept\n").unwrap();
    assert_eq!(
        create(&dir, &name).unwrap_err().kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(fs::read(dir.join(&name)).unwrap(), b"kept\n");
    let elsewhere = dir.join("elsewhere");
    fs::write(&elsewhere, "theirs\n").unwrap();
    let linked = dir.join(super::name(&run('b'), NOW));
    std::os::unix::fs::symlink(&elsewhere, &linked).unwrap();
    assert!(create(&dir, &super::name(&run('b'), NOW)).is_err());
    assert_eq!(fs::read(&elsewhere).unwrap(), b"theirs\n");
    fs::remove_dir_all(&dir).unwrap();
}

/// A log of `size` bytes, each its place's digit, for `run` in the day folder `day` of `runs`.
fn written(runs: &Path, day: &str, run: &str, size: usize) -> Vec<u8> {
    let bytes: Vec<u8> = (0..size)
        .map(|at| b'0' + u8::try_from(at % 10).unwrap())
        .collect();
    let folder = runs.join(FOLDER).join(day);
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join(format!("{run}.log")), &bytes).unwrap();
    bytes
}

#[test]
fn a_read_answers_the_tail_first_or_a_range_and_never_more_than_its_most() {
    let dir = scratch("read");
    let id = run('a');
    let most = usize::try_from(RUN_LOG_MOST).unwrap();
    let bytes = written(&dir, "2026-11-11", &id, most + most / 2);
    let size = bytes.len() as u64;
    let read = |asked| read(&dir, &id, asked).unwrap();
    let tail = read(Asked::Tail(10));
    assert_eq!(
        (tail.log.as_str(), tail.size, tail.from, tail.to),
        (
            format!("logs/2026-11-11/{id}.log").as_str(),
            size,
            size - 10,
            size
        )
    );
    assert_eq!(tail.text.as_bytes(), &bytes[bytes.len() - 10..]);
    assert_eq!(tail.run, id);
    let capped = read(Asked::Tail(u64::MAX));
    assert_eq!((capped.from, capped.to), (size - RUN_LOG_MOST, size));
    let range = read(Asked::Range {
        from: 5,
        to: Some(15),
    });
    assert_eq!(
        (range.from, range.to, range.text.as_bytes()),
        (5, 15, &bytes[5..15])
    );
    let from_start = read(Asked::Range { from: 0, to: None });
    assert_eq!((from_start.from, from_start.to), (0, RUN_LOG_MOST));
    let rest = read(Asked::Range {
        from: RUN_LOG_MOST,
        to: None,
    });
    assert_eq!((rest.from, rest.to), (RUN_LOG_MOST, size));
    for (from, to) in [(size + 5, None), (20, Some(10)), (size, Some(size + 9))] {
        let empty = read(Asked::Range { from, to });
        assert_eq!(
            (empty.from, empty.to, empty.text.as_str()),
            (from.min(size), from.min(size), ""),
            "{from} {to:?}"
        );
    }
    // A range that cuts a character reads it as the replacement character.
    let other = run('b');
    let folder = dir.join(FOLDER).join("2026-11-11");
    fs::write(folder.join(format!("{other}.log")), "aé").unwrap();
    let cut = super::read(
        &dir,
        &other,
        Asked::Range {
            from: 0,
            to: Some(2),
        },
    )
    .unwrap();
    assert_eq!(cut.text, "a\u{fffd}");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_read_takes_the_newest_day_holding_the_log_and_refuses_what_is_no_run_or_no_log() {
    let dir = scratch("find");
    let id = run('c');
    let missing = |found: Result<RunLog, Unread>| match found {
        Err(Unread::Missing(why)) => why,
        other => panic!("{other:?}"),
    };
    // No logs at all yet.
    assert!(missing(read(&dir, &id, Asked::Tail(10))).contains("no log of the run"));
    for named in ["../x", "C".repeat(31).as_str(), "g".repeat(32).as_str(), ""] {
        assert!(
            matches!(read(&dir, named, Asked::Tail(10)), Err(Unread::NotARun)),
            "{named}"
        );
    }
    written(&dir, "2026-11-10", &id, 3);
    written(&dir, "2026-11-11", &id, 7);
    assert_eq!(read(&dir, &id, Asked::Tail(100)).unwrap().size, 7);
    // A folder that is no day's, and a day folder that is a link, are not read.
    let other = run('d');
    written(&dir, "notes", &other, 5);
    let elsewhere = scratch("find-elsewhere");
    fs::write(elsewhere.join(format!("{other}.log")), "theirs").unwrap();
    std::os::unix::fs::symlink(&elsewhere, dir.join(FOLDER).join("2026-11-12")).unwrap();
    assert!(missing(read(&dir, &other, Asked::Tail(10))).contains("no log of the run"));
    // A link named as a log, or a FIFO, is refused, never followed and never waited on.
    let day = dir.join(FOLDER).join("2026-11-11");
    std::os::unix::fs::symlink(
        elsewhere.join(format!("{other}.log")),
        day.join(format!("{other}.log")),
    )
    .unwrap();
    assert!(missing(read(&dir, &other, Asked::Tail(10))).contains("not a regular file"));
    let fifo = run('e');
    let path = std::ffi::CString::new(
        day.join(format!("{fifo}.log"))
            .as_os_str()
            .as_encoded_bytes(),
    )
    .unwrap();
    // SAFETY: a NUL-terminated path, alive through the call.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    assert!(missing(read(&dir, &fifo, Asked::Tail(10))).contains("not a regular file"));
    fs::remove_dir_all(&dir).unwrap();
    fs::remove_dir_all(&elsewhere).unwrap();
}
