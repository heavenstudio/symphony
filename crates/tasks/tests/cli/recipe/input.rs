//! Bounded private input receipts. This is test evidence, never an input/lifetime seal.
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use symphony_board::sys::Command;

const CONTROL: &str = "SF_TASKS_INPUT_RECEIPT";
const BYTES: usize = 131_072;
const CHILDREN: usize = 16;
const FILES: [&str; 11] = [
    "owner",
    "where.claim",
    "build.claim",
    "invalid",
    "where-0",
    "where-final",
    "build-0",
    "build-1",
    "build-2",
    "build-3",
    "build-final",
];
static NEXT: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Caller {
    Warm,
    Recipe,
}

pub(super) struct Capture {
    dir: PathBuf,
    id: String,
    caller: Caller,
    uid: u32,
    device: u64,
    inode: u64,
}

pub(super) struct Evidence {
    state: &'static str,
    reason: Option<&'static str>,
    id: Option<String>,
    hashes: usize,
    keys: [Option<String>; 2],
}

impl std::fmt::Debug for Evidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputEvidence")
            .field("state", &self.state)
            .field("reason", &self.reason)
            .field("id", &self.id)
            .field("hashes", &self.hashes)
            .field("keys", &self.keys)
            .finish()
    }
}

impl Evidence {
    pub(super) fn unknown() -> Self {
        Self {
            state: "unknown",
            reason: Some("unavailable"),
            id: None,
            hashes: 0,
            keys: [None, None],
        }
    }
}

impl Capture {
    pub(super) fn new(root: &Path, caller: Caller) -> Option<Self> {
        if root.as_os_str().as_bytes().len() > 4000 {
            return None;
        }
        let n = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                (n < CHILDREN).then_some(n + 1)
            })
            .ok()?;
        let id = format!("{}-{n}", std::process::id());
        let dir = root.join("artifacts").join(format!("recipe-inputs-{id}"));
        // Exclusive directory creation. Collision means unavailable, never a retry/overwrite.
        DirBuilder::new().mode(0o700).create(&dir).ok()?;
        let meta = fs::symlink_metadata(&dir).ok()?;
        if !meta.is_dir() || meta.permissions().mode() & 0o7777 != 0o700 {
            return None;
        }
        let mut owner = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(dir.join("owner"))
            .ok()?;
        writeln!(owner, "{id} {caller:?}").ok()?;
        Some(Self {
            dir,
            id,
            caller,
            uid: meta.uid(),
            device: meta.dev(),
            inode: meta.ino(),
        })
    }

    fn file(&self, name: &str, buf: &mut Vec<u8>) -> Result<bool, &'static str> {
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(self.dir.join(name))
        {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err("private open"),
        };
        let m = file.metadata().map_err(|_| "private metadata")?;
        let limit = if name == "owner" {
            64
        } else if name.ends_with(".claim") {
            0
        } else {
            BYTES
        };
        if !m.is_file()
            || m.uid() != self.uid
            || m.nlink() != 1
            || m.permissions().mode() & 0o7777 != 0o600
            || m.len() > limit as u64
        {
            return Err("private file identity or limit");
        }
        buf.clear();
        file.take((limit + 1) as u64)
            .read_to_end(buf)
            .map_err(|_| "private read")?;
        if buf.len() > BYTES {
            return Err("private overflow");
        }
        Ok(true)
    }

    fn inventory(&self) -> Result<(), &'static str> {
        let m = fs::symlink_metadata(&self.dir).map_err(|_| "private directory identity")?;
        if !m.is_dir()
            || m.uid() != self.uid
            || m.dev() != self.device
            || m.ino() != self.inode
            || m.permissions().mode() & 0o7777 != 0o700
        {
            return Err("private directory identity");
        }
        let entries = fs::read_dir(&self.dir).map_err(|_| "private directory")?;
        let mut seen = 0_u16;
        for (i, entry) in entries.enumerate() {
            if i >= FILES.len() {
                return Err("unexpected private entry");
            }
            let entry = entry.map_err(|_| "private entry")?;
            let name = entry.file_name();
            if name.as_bytes().len() > 64 {
                return Err("private name");
            }
            let index = FILES
                .iter()
                .position(|s| name == *s)
                .ok_or("unexpected private entry")?;
            let bit = 1_u16 << index;
            if seen & bit != 0 {
                return Err("duplicate private entry");
            }
            seen |= bit;
        }
        if seen & 1 == 0 || seen & (1 << 3) != 0 {
            return Err("missing owner or invalid");
        }
        Ok(())
    }

    fn validate(&self, expected: Option<(&Path, &str)>) -> Result<Evidence, &'static str> {
        self.inventory()?;
        let mut buf = Vec::with_capacity(BYTES + 1);
        if !self.file("owner", &mut buf)? {
            return Err("missing owner");
        }
        if buf != format!("{} {:?}\n", self.id, self.caller).as_bytes() {
            return Err("owner conflict");
        }
        let mut count = 0;
        let mut keys: [Option<String>; 2] = [None, None];
        let mut paths: [Option<String>; 2] = [None, None];
        for (index, op) in ["where", "build"].into_iter().enumerate() {
            let claim = self.file(&format!("{op}.claim"), &mut buf)?;
            if claim && !buf.is_empty() {
                return Err("claim conflict");
            }
            let mut last = None;
            let mut gap = false;
            let phases = if op == "where" { 1 } else { 4 };
            for phase in 0..phases {
                let present = self.file(&format!("{op}-{phase}"), &mut buf)?;
                if !present {
                    gap = true;
                    continue;
                }
                if !claim || gap || (op == "where" && phase != 0) {
                    return Err("phase or writer conflict");
                }
                last = Some(hash_record(&buf, op, phase, &self.id)?);
                count += 1;
            }
            let final_present = self.file(&format!("{op}-final"), &mut buf)?;
            if !claim {
                if final_present {
                    return Err("unclaimed final");
                }
                continue;
            }
            let last = last.ok_or("missing hash phase")?;
            if !final_present {
                return Err("missing final");
            }
            let (key, path) = final_record(&buf, op, &self.id)?;
            if key != last {
                return Err("unmatched final key");
            }
            keys[index] = Some(key);
            paths[index] = Some(path);
        }
        match self.caller {
            Caller::Warm => {
                let (path, key) = expected.ok_or("missing warm receipt")?;
                if keys[0].is_some()
                    || keys[1].as_deref() != Some(key)
                    || paths[1].as_deref().map(Path::new) != Some(path)
                {
                    return Err("warm final association");
                }
            }
            Caller::Recipe => {
                if keys[0].is_none() {
                    return Err("missing natural where");
                }
                // Optional build is a separate natural selector operation, never inferred equal.
            }
        }
        self.inventory()?;
        Ok(Evidence {
            state: "captured",
            reason: None,
            id: Some(self.id.clone()),
            hashes: count,
            keys,
        })
    }
}

pub(super) fn bind(capture: Option<&Capture>, command: &mut Command) {
    command.env_remove(CONTROL);
    if let Some(capture) = capture {
        command.env(CONTROL, &capture.dir);
    }
}

/// The actual before-launch reporting path. A reporting error never skips the subject.
pub(super) fn launch<T>(
    mut capture: Option<Capture>,
    writer: &mut impl Write,
    subject: impl FnOnce(Option<&Capture>) -> T,
) -> (T, Option<Capture>) {
    if let Some(c) = &capture {
        if writeln!(
            writer,
            "input receipt child={} caller={:?} pending",
            c.id, c.caller
        )
        .is_err()
        {
            capture = None; // Explicitly unavailable evidence; retain the private directory.
        }
    }
    let result = subject(capture.as_ref()); // Exactly once. A real subject panic propagates.
    (result, capture)
}

/// The actual before-Ran reporting path. Preserve the subject value on every diagnostic error.
pub(super) fn report_result<T>(
    result: T,
    capture: Option<Capture>,
    expected: Option<(&Path, &str)>,
    writer: &mut impl Write,
) -> (T, Evidence) {
    let mut evidence = finish(capture, expected);
    if writeln!(writer, "recipe input evidence: {evidence:?}").is_err() {
        evidence.state = "unknown";
        evidence.reason = Some("report unavailable");
        evidence.hashes = 0;
        evidence.keys = [None, None];
    }
    (result, evidence)
}

pub(super) fn finish(capture: Option<Capture>, expected: Option<(&Path, &str)>) -> Evidence {
    let Some(capture) = capture else {
        return Evidence::unknown();
    };
    match capture.validate(expected) {
        Ok(evidence) => evidence,
        Err(reason) => Evidence {
            state: "unknown",
            reason: Some(reason),
            id: Some(capture.id.clone()),
            hashes: 0,
            keys: [None, None],
        },
    }
    // Capture handles/buffers close normally or on unwind. The exclusive private directory
    // is retained under ignored artifacts on every outcome. No subject/cache deletion.
}

mod records;
use records::{final_record, hash_record};
#[cfg(test)]
mod panic_control;
#[cfg(test)]
mod shell_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
use tests::{FailingWriter, private_fixture};
