//! PID-aware listings preserve readable presence separately from proof of absence.

use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::process::Output;
use std::sync::mpsc;
use std::time::Instant;

/// Readable working directories and whether the global listing finished without omissions.
#[derive(Debug, Clone, PartialEq)]
pub struct PidCwds {
    pub listed: Vec<(i64, PathBuf)>,
    pub complete: bool,
}

pub(super) fn read(done: Output, deadline: Instant) -> Option<PidCwds> {
    // A caret name may need filesystem identity. Bound those reads by the scan's remaining
    // budget without retaining any process or signalling the already reaped lsof PID.
    read_before(deadline, move || {
        parse(
            &done.stdout,
            done.status.success() && done.stderr.is_empty(),
        )
    })
}

fn read_before<T: Send + 'static>(
    deadline: Instant,
    read: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    if Instant::now() >= deadline {
        return None;
    }
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        if Instant::now() < deadline {
            let _ = send.send(read());
        }
    });
    // Channel/thread setup and the producer's elapsed time consume the same deadline.
    let remaining = deadline.checked_duration_since(Instant::now())?;
    let result = receive.recv_timeout(remaining).ok()?;
    (Instant::now() < deadline).then_some(result)
}

fn parse(stdout: &[u8], complete: bool) -> PidCwds {
    let mut scan = PidCwds {
        listed: Vec::new(),
        complete,
    };
    let mut pid = None;
    let mut has_path = true;
    let mut file = File::Missing;
    for bytes in stdout.split(|b| *b == b'\n') {
        let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
        if let Some(digits) = bytes.strip_prefix(b"p") {
            has_path |= file.finish(pid, &mut scan);
            scan.complete &= has_path;
            pid = std::str::from_utf8(digits)
                .ok()
                .and_then(|digits| digits.parse::<i64>().ok())
                .filter(|pid| *pid > 0);
            scan.complete &= pid.is_some();
            has_path = false;
        } else if bytes.starts_with(b"f") {
            has_path |= file.finish(pid, &mut scan);
            file = if bytes == b"fcwd" && pid.is_some() {
                File::Cwd(Cwd::default())
            } else {
                scan.complete = false;
                File::Unusable
            };
        } else if !bytes.is_empty() {
            let valid = match &mut file {
                File::Cwd(cwd) => cwd.field(bytes),
                _ => false,
            };
            if !valid {
                scan.complete = false;
                file = File::Unusable;
            }
        }
    }
    has_path |= file.finish(pid, &mut scan);
    scan.complete &= has_path;
    scan
}

/// lsof's printable names escape backslashes, C control characters and nonprinting bytes.
/// Decode the name once, then validate the resulting path, never the field markers.
fn decode_name(name: &[u8]) -> Option<PathBuf> {
    let mut decoded = Vec::with_capacity(name.len());
    let mut remaining = name.iter().copied();
    while let Some(byte) = remaining.next() {
        decoded.push(if byte == b'\\' {
            match remaining.next()? {
                b'\\' => b'\\',
                b'b' => b'\x08',
                b'f' => b'\x0c',
                b'n' => b'\n',
                b'r' => b'\r',
                b't' => b'\t',
                b'x' => hex(remaining.next()?)?.checked_mul(16)? + hex(remaining.next()?)?,
                _ => return None,
            }
        } else {
            // Unescaped controls are not a supported printable name representation.
            if byte.is_ascii_control() {
                return None;
            }
            byte
        });
    }
    let path = Path::new(std::str::from_utf8(&decoded).ok()?);
    (path.is_absolute()
        && !decoded.contains(&0)
        && path.components().all(|part| part != Component::ParentDir))
    .then(|| path.to_path_buf())
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// A name is usable only in one valid cwd file record. Delay publication until the next record
/// so a duplicate name or malformed field cannot leave a previously accepted false positive.
#[derive(Default)]
enum File {
    #[default]
    Missing,
    Cwd(Cwd),
    Unusable,
}

impl File {
    fn finish(&mut self, pid: Option<i64>, scan: &mut PidCwds) -> bool {
        let row = match (std::mem::take(self), pid) {
            (Self::Cwd(cwd), Some(pid)) => cwd.verified_path().map(|path| (pid, path)),
            (Self::Missing, _) => return false,
            _ => None,
        };
        if let Some(row) = row {
            scan.listed.push(row);
            true
        } else {
            scan.complete = false;
            false
        }
    }
}

#[derive(Default)]
struct Cwd {
    path: Option<PathBuf>,
    device: Option<u64>,
    inode: Option<u64>,
}

impl Cwd {
    fn field(&mut self, bytes: &[u8]) -> bool {
        if let Some(name) = bytes.strip_prefix(b"n") {
            if self.path.is_some() {
                return false;
            }
            self.path = decode_name(name);
            return self.path.is_some();
        }
        if let Some(digits) = bytes.strip_prefix(b"D0x") {
            number(&mut self.device, digits, 16)
        } else if let Some(digits) = bytes.strip_prefix(b"i") {
            number(&mut self.inode, digits, 10)
        } else {
            false
        }
    }

    fn verified_path(self) -> Option<PathBuf> {
        let path = self.path?;
        // lsof prints both a literal ^A and control byte 1 as ^A. Do not guess either.
        // A literal candidate is usable only with this file record's directory identity;
        // existence alone would allow a different real lookalike checkout to match.
        let caret =
            path.as_os_str().as_encoded_bytes().windows(2).any(|pair| {
                pair[0] == b'^' && (pair[1] == b'?' || (b'@'..=b'_').contains(&pair[1]))
            });
        if !caret {
            return Some(path);
        }
        let (device, inode) = (self.device?, self.inode?);
        let same_directory = |path: &Path| {
            std::fs::metadata(path)
                .is_ok_and(|found| found.is_dir() && found.dev() == device && found.ino() == inode)
        };
        if !same_directory(&path) {
            return None;
        }
        let canonical = path.canonicalize().ok()?;
        // This is a filesystem path, not another display field. Never decode it again.
        (Self::usable_path(&canonical) && same_directory(&canonical)).then_some(canonical)
    }

    fn usable_path(path: &Path) -> bool {
        path.is_absolute()
            && path.to_str().is_some()
            && !path.as_os_str().as_encoded_bytes().contains(&0)
            && path.components().all(|part| part != Component::ParentDir)
    }
}

fn number(slot: &mut Option<u64>, digits: &[u8], radix: u32) -> bool {
    if slot.is_some()
        || digits.is_empty()
        || !digits.iter().all(|byte| {
            if radix == 16 {
                byte.is_ascii_hexdigit()
            } else {
                byte.is_ascii_digit()
            }
        })
    {
        return false;
    }
    *slot = std::str::from_utf8(digits)
        .ok()
        .and_then(|digits| u64::from_str_radix(digits, radix).ok());
    slot.is_some()
}

#[cfg(test)]
mod identity_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::Duration;

    #[test]
    fn expired_command_budget_starts_no_reader_and_slow_reads_cannot_reset_it() {
        let (started, starts) = mpsc::channel();
        let expired: Option<()> = read_before(Instant::now(), move || started.send(()).unwrap());
        assert_eq!(expired, None);
        assert_eq!(starts.try_recv(), Err(mpsc::TryRecvError::Disconnected));

        // Actual command time belongs to the same deadline passed to the reader afterward.
        let deadline = Instant::now() + Duration::from_millis(20);
        let mut command = crate::Command::new("/bin/cat");
        let mut child = crate::spawn::spawn_piped(&mut command).unwrap();
        let mut input = child.stdin.take().unwrap();
        input.write_all(b"p1\nfcwd\nn/work\n").unwrap();
        // Hold the actual producer's stdin open until the original deadline expires.
        let (hold, elapsed) = mpsc::channel::<()>();
        let waited = elapsed.recv_timeout(deadline.saturating_duration_since(Instant::now()));
        drop(hold);
        drop(input);
        let done = crate::spawn::wait_output(child).unwrap();
        assert_eq!(waited, Err(mpsc::RecvTimeoutError::Timeout));
        assert_eq!(done.stdout, b"p1\nfcwd\nn/work\n");
        assert_eq!(read(done, deadline), None);

        let deadline = Instant::now() + Duration::from_secs(5);
        assert_eq!(read_before(deadline, || Ok::<_, &str>(7)), Some(Ok(7)));
        assert_eq!(
            read_before(deadline, || Err::<u8, _>("unreadable")),
            Some(Err("unreadable"))
        );

        let (started, starts) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let (finished, finishes) = mpsc::channel();
        let deadline = Instant::now() + Duration::from_secs(1);
        let result = read_before(deadline, move || {
            let _ = started.send(());
            // Dropping the test's sender also releases the worker on a test failure.
            let _ = released.recv();
            let _ = finished.send(());
            9
        });
        let entered = starts.recv_timeout(Duration::from_secs(5));
        let _ = release.send(());
        let completed = finishes.recv_timeout(Duration::from_secs(5));
        assert_eq!(result, None, "a stalled reader cannot establish a scan");
        assert_eq!(entered, Ok(()), "the reader really reached its barrier");
        assert_eq!(completed, Ok(()), "the owned stalled worker was released");
    }

    #[test]
    fn printable_names_decode_once_before_path_validation() {
        for (printed, path) in [
            (br"/work/tab\tname".as_slice(), "/work/tab\tname"),
            (br"/work/caf\xc3\xa9".as_slice(), "/work/café"),
            ("/work/café".as_bytes(), "/work/café"),
            (br"/work/\\t\\x41".as_slice(), "/work/\\t\\x41"),
            (br"/work/\\x00".as_slice(), "/work/\\x00"),
            (br"/work/\b\f\n\r\t".as_slice(), "/work/\x08\x0c\n\r\t"),
            (br"/work/\x7f".as_slice(), "/work/\x7f"),
            (br"/work/\xC3\xA9".as_slice(), "/work/é"),
        ] {
            assert_eq!(decode_name(printed), Some(path.into()), "{printed:?}");
        }
        // Every hex byte boundary is decoded into bytes before the UTF8/NUL checks.
        for byte in u8::MIN..=u8::MAX {
            let printed = format!("/work/\\x{byte:02x}");
            let mut bytes = b"/work/".to_vec();
            bytes.push(byte);
            let expected = String::from_utf8(bytes)
                .ok()
                .filter(|_| byte != 0)
                .map(PathBuf::from);
            assert_eq!(decode_name(printed.as_bytes()), expected, "{printed}");
        }
        for printed in [
            br"/work/\".as_slice(),
            br"/work/\x".as_slice(),
            br"/work/\x1".as_slice(),
            br"/work/\xgg".as_slice(),
            br"/work/\z".as_slice(),
            br"/work/\012".as_slice(),
            br"/work/\xff".as_slice(),
            br"/work/\xc3".as_slice(),
            br"/work/\x00".as_slice(),
            br"/work/\x2e\x2e/other".as_slice(),
            b"/work/\t".as_slice(),
        ] {
            assert_eq!(decode_name(printed), None, "{printed:?}");
            let mut rows = b"p1\nfcwd\nn/before\np2\nfcwd\nn".to_vec();
            rows.extend_from_slice(printed);
            rows.extend_from_slice(b"\np3\nfcwd\nn/after\n");
            assert_eq!(
                parse(&rows, true),
                PidCwds {
                    listed: vec![(1, "/before".into()), (3, "/after".into())],
                    complete: false,
                },
                "{printed:?}"
            );
        }
    }

    #[test]
    fn unusable_cwd_fields_neither_authorize_ownership_nor_hide_valid_neighbors() {
        for fields in [
            b"fcwd\nnrelative/path\n".as_slice(),
            b"fcwd\nnunknown\n".as_slice(),
            b"fcwd\nn/owned/../elsewhere\n".as_slice(),
            b"ftxt\nn/owned\n".as_slice(),
            b"f0\nn/owned\n".as_slice(),
            b"f\\x63wd\nn/owned\n".as_slice(),
            b"f\nn/owned\n".as_slice(),
            b"n/owned\n".as_slice(),
            b"fcwd\nn\n".as_slice(),
            b"fcwd\nn/owned/\xff\n".as_slice(),
            b"fcwd\nn/owned/\0bad\n".as_slice(),
            b"fcwd\nn/owned\nn/duplicate\n".as_slice(),
            b"fcwd\nn/owned\nunexpected\n".as_slice(),
        ] {
            let mut bytes = b"p1\nfcwd\nn/before\np7\n".to_vec();
            bytes.extend_from_slice(fields);
            bytes.extend_from_slice(b"p9\nfcwd\nn/after\n");
            assert_eq!(
                parse(&bytes, true),
                PidCwds {
                    listed: vec![(1, "/before".into()), (9, "/after".into())],
                    complete: false,
                },
                "{fields:?}"
            );
        }
    }

    #[test]
    fn a_fresh_cwd_descriptor_recovers_valid_presence_after_a_bad_file_in_the_same_process() {
        assert_eq!(
            parse(
                b"p5\nfcwd\nn/before\nftxt\nn/not-cwd\nfcwd\nn/after\n",
                true
            ),
            PidCwds {
                listed: vec![(5, "/before".into()), (5, "/after".into())],
                complete: false,
            }
        );
    }

    #[test]
    fn readable_rows_survive_missing_or_malformed_neighbors_without_proving_absence() {
        let valid = b"p12\nfcwd\nn/work/a\np7\nfcwd\nn/work/b\n";
        assert_eq!(
            parse(valid, true),
            PidCwds {
                listed: vec![(12, "/work/a".into()), (7, "/work/b".into())],
                complete: true,
            }
        );
        for broken in [
            b"n/no/pid\npbad\nn/bad/pid\np0\nn/zero\n".as_slice(),
            b"p1\nfcwd\n".as_slice(),
            b"p9223372036854775808\nn/overflow\n".as_slice(),
            b"p\\x31\nfcwd\nn/not-a-decoded-pid\n".as_slice(),
            b"p2\nfcwd\nn\n".as_slice(),
            b"p2\nfcwd\nn/mnt/\xffx\n".as_slice(),
            b"fcwd\n".as_slice(),
            b"unexpected\n".as_slice(),
        ] {
            let mut bytes = broken.to_vec();
            bytes.extend_from_slice(b"p3\nfcwd\nn/work/c\n");
            assert_eq!(
                parse(&bytes, true),
                PidCwds {
                    listed: vec![(3, "/work/c".into())],
                    complete: false,
                },
                "{broken:?}"
            );
        }
        assert_eq!(
            parse(b"p1\nfcwd\n", true),
            PidCwds {
                listed: vec![],
                complete: false
            }
        );
        assert_eq!(
            parse(b"fcwd\n", true),
            PidCwds {
                listed: vec![],
                complete: false
            }
        );
        assert_eq!(
            parse(b"", true),
            PidCwds {
                listed: vec![],
                complete: true
            }
        );
        assert_eq!(
            parse(b"p3\r\nfcwd\r\nn/work/c\r\n", true),
            PidCwds {
                listed: vec![(3, "/work/c".into())],
                complete: true
            }
        );
    }
}
