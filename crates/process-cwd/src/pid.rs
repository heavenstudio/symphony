//! PID-aware listings preserve readable presence separately from proof of absence.

use std::path::PathBuf;
use std::process::Output;

/// Readable working directories and whether the global listing finished without omissions.
#[derive(Debug, Clone, PartialEq)]
pub struct PidCwds {
    pub listed: Vec<(i64, PathBuf)>,
    pub complete: bool,
}

pub(super) fn read(done: &Output) -> PidCwds {
    parse(
        &done.stdout,
        done.status.success() && done.stderr.is_empty(),
    )
}

fn parse(stdout: &[u8], mut complete: bool) -> PidCwds {
    let mut listed = Vec::new();
    let mut pid = None;
    let mut has_path = true;
    for bytes in stdout.split(|b| *b == b'\n') {
        let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
        let Ok(line) = std::str::from_utf8(bytes) else {
            complete = false;
            pid = None;
            continue;
        };
        if let Some(digits) = line.strip_prefix('p') {
            complete &= has_path;
            pid = digits.parse::<i64>().ok().filter(|pid| *pid > 0);
            complete &= pid.is_some();
            has_path = false;
        } else if let Some(path) = line.strip_prefix('n') {
            if let Some(pid) = pid.filter(|_| !path.is_empty()) {
                listed.push((pid, PathBuf::from(path)));
                has_path = true;
            } else {
                complete = false;
            }
        } else if line == "fcwd" {
            complete &= pid.is_some();
        } else if !line.is_empty() {
            complete = false;
        }
    }
    PidCwds {
        listed,
        complete: complete && has_path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            b"p2\nn\n".as_slice(),
            b"p2\nn/mnt/\xffx\n".as_slice(),
            b"fcwd\n".as_slice(),
            b"unexpected\n".as_slice(),
        ] {
            let mut bytes = broken.to_vec();
            bytes.extend_from_slice(b"p3\nn/work/c\n");
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
            parse(b"p3\r\nn/work/c\r\n", true),
            PidCwds {
                listed: vec![(3, "/work/c".into())],
                complete: true
            }
        );
    }
}
