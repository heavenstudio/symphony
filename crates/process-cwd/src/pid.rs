//! PID-aware listings preserve readable presence separately from proof of absence.

use std::path::{Component, Path, PathBuf};
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
                File::Cwd(None)
            } else {
                scan.complete = false;
                File::Unusable
            };
        } else if let Some(name) = bytes.strip_prefix(b"n") {
            let path = std::str::from_utf8(name)
                .ok()
                .map(Path::new)
                .filter(|path| {
                    path.is_absolute()
                        && !name.contains(&0)
                        && path.components().all(|part| part != Component::ParentDir)
                });
            file = match (&file, path) {
                (File::Cwd(None), Some(path)) => File::Cwd(Some(path.to_path_buf())),
                _ => {
                    scan.complete = false;
                    File::Unusable
                }
            };
        } else if !bytes.is_empty() {
            scan.complete = false;
            file = File::Unusable;
        }
    }
    has_path |= file.finish(pid, &mut scan);
    scan.complete &= has_path;
    scan
}

/// A name is usable only in one valid cwd file record. Delay publication until the next record
/// so a duplicate name or malformed field cannot leave a previously accepted false positive.
#[derive(Default)]
enum File {
    #[default]
    Missing,
    Cwd(Option<PathBuf>),
    Unusable,
}

impl File {
    fn finish(&mut self, pid: Option<i64>, scan: &mut PidCwds) -> bool {
        match (std::mem::take(self), pid) {
            (Self::Cwd(Some(path)), Some(pid)) => {
                scan.listed.push((pid, path));
                true
            }
            (Self::Missing, _) => false,
            _ => {
                scan.complete = false;
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unusable_cwd_fields_neither_authorize_ownership_nor_hide_valid_neighbors() {
        for fields in [
            b"fcwd\nnrelative/path\n".as_slice(),
            b"fcwd\nnunknown\n".as_slice(),
            b"fcwd\nn/owned/../elsewhere\n".as_slice(),
            b"ftxt\nn/owned\n".as_slice(),
            b"f0\nn/owned\n".as_slice(),
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
