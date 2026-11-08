//! A locked, durable receipt serializes one identity and remembers its terminal outcome.

use super::super::Failure;
use super::{journal, pending};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The lock stays owned through preflight, dispatch and confirmation.
pub(in crate::save) struct Receipt {
    file: Lease,
    path: PathBuf,
    root: PathBuf,
    identity: String,
    /// Binds the UUID, scope and canonical submitted text to the board row.
    pub(in crate::save) fingerprint: String,
    /// Only the publisher of a synced fingerprint may prepare a new intent.
    pub(in crate::save) fresh: bool,
    terminal: Option<Value>,
}

struct Claim {
    file: Lease,
    fresh: bool,
    preparation_error: Option<String>,
}

/// A held lock, released before its descriptor closes. The lock belongs to the open file
/// description, and every spawn or fork on another thread copies this descriptor into its child
/// until that child's exec: close-on-exec, which std sets in the open call itself, closes the copy
/// only there. Closing this descriptor alone would leave the lock with that child, and the next
/// open of the receipt would find it locked.
struct Lease(File);

impl Drop for Lease {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

fn failure(path: &Path, cause: impl std::fmt::Display) -> Failure {
    Failure {
        status: 503,
        error: format!("task save receipt {}: {cause}", path.display()),
    }
}

fn preflight_failure(
    root: &Path,
    identity: &str,
    path: &Path,
    cause: impl std::fmt::Display,
) -> Failure {
    let mut error = failure(path, cause);
    if !journal::present(root, identity) {
        error.status = 422;
    }
    error
}

fn lock(file: File, path: &Path) -> Result<Lease, Failure> {
    file.try_lock().map_err(|error| failure(path, error))?;
    Ok(Lease(file))
}

fn existing(root: &Path, identity: &str, path: &Path) -> Result<Option<Claim>, Failure> {
    match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => Ok(Some(Claim {
            file: lock(file, path)?,
            fresh: false,
            preparation_error: None,
        })),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) if path.is_dir() => Err(preflight_failure(root, identity, path, error)),
        Err(error) => Err(failure(path, error)),
    }
}

fn sync_directories(root: &Path, directory: &Path) -> Result<(), Failure> {
    let artifacts = root.join("artifacts");
    let folders = [
        directory,
        directory
            .parent()
            .expect("the receipts folder is under the journal folder"),
        &artifacts,
        root,
    ];
    for folder in folders {
        File::open(folder)
            .and_then(|file| file.sync_all())
            .map_err(|error| failure(folder, error))?;
    }
    Ok(())
}

fn claim(root: &Path, identity: &str, path: &Path, fingerprint: &str) -> Result<Claim, Failure> {
    let directory = path.parent().expect("a receipt path includes its folder");
    std::fs::create_dir_all(directory)
        .map_err(|error| preflight_failure(root, identity, directory, error))?;
    if let Some(claim) = existing(root, identity, path)? {
        return Ok(claim);
    }
    // Publication is serialized only while preparing files; database work holds its own identity lock.
    let guard_path = directory.join(".publish");
    let guard = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&guard_path)
        .map_err(|error| preflight_failure(root, identity, &guard_path, error))?;
    let _guard = lock(guard, &guard_path)?;
    if let Some(claim) = existing(root, identity, path)? {
        return Ok(claim);
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let part = directory.join(format!(
        ".{identity}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&part)
            .map_err(|error| preflight_failure(root, identity, &part, error))?;
        let mut file = lock(file, &part)?;
        let preparation_error = file
            .0
            .write_all(fingerprint.as_bytes())
            .and_then(|()| file.0.sync_all())
            .err()
            .map(|error| error.to_string());
        if preparation_error.is_some() {
            // A published empty receipt seals an attempt that cannot dispatch.
            file.0
                .set_len(0)
                .and_then(|()| file.0.sync_all())
                .map_err(|error| failure(&part, error))?;
        }
        match std::fs::hard_link(&part, path) {
            Ok(()) => {
                sync_directories(root, directory)?;
                Ok(Claim {
                    file,
                    fresh: preparation_error.is_none(),
                    preparation_error,
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                existing(root, identity, path)?.ok_or_else(|| failure(path, "receipt disappeared"))
            }
            Err(error) => Err(preflight_failure(root, identity, path, error)),
        }
    })();
    let _ = std::fs::remove_file(part);
    result
}

fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(fields) => {
            let ordered: std::collections::BTreeMap<_, _> = fields
                .iter()
                .map(|(key, value)| (key.clone(), canonical(value)))
                .collect();
            json!(ordered)
        }
        Value::Array(values) => Value::Array(values.iter().map(canonical).collect()),
        _ => value.clone(),
    }
}

impl Receipt {
    /// Publish a synced fingerprint under its lock; short receipts never initialize again.
    pub(in crate::save) fn open(
        root: &Path,
        identity: &str,
        scope: &str,
        payload: &Value,
    ) -> Result<Self, Failure> {
        let path = root
            .join("artifacts/symphony-quick-tasks/.receipts")
            .join(identity);
        let fingerprint: String = Sha256::digest(canonical(&json!([scope, payload])).to_string())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let Claim {
            mut file,
            fresh,
            preparation_error,
        } = claim(root, identity, &path, &fingerprint)?;
        file.0
            .seek(SeekFrom::Start(0))
            .map_err(|error| failure(&path, error))?;
        let mut bytes = Vec::new();
        file.0
            .read_to_end(&mut bytes)
            .map_err(|error| failure(&path, error))?;
        if bytes.len() < 64 {
            let cause = preparation_error
                .unwrap_or_else(|| format!("fingerprint has {} of 64 bytes", bytes.len()));
            return Err(preflight_failure(
                root,
                identity,
                &path,
                format!(
                    "{cause}; this save stopped before dispatch; your text is kept for another try"
                ),
            ));
        }
        if bytes[..64] != *fingerprint.as_bytes() {
            return Err(
                "this task save identity belongs to different text; check the original save"
                    .to_string()
                    .into(),
            );
        }
        let terminal = if bytes.len() == 64 {
            None
        } else {
            serde_json::from_slice(&bytes[64..]).ok()
        };
        // A replay can read bytes whose writer could not confirm their durability.
        file.0.sync_all().map_err(|error| failure(&path, error))?;
        sync_directories(
            root,
            path.parent().expect("a receipt path includes its folder"),
        )?;
        Ok(Self {
            file,
            path,
            root: root.into(),
            identity: identity.into(),
            fingerprint,
            fresh,
            terminal,
        })
    }

    /// A completed save or refusal is replayed without reusing its typed text.
    pub(in crate::save) fn completed(&self) -> Option<Result<Value, Failure>> {
        self.terminal.as_ref().map(|value| {
            if value["slug"].is_string() {
                Ok(value.clone())
            } else {
                Err(Failure {
                    status: value["status"].as_u64().unwrap_or(422) as u16,
                    error: value["error"]
                        .as_str()
                        .unwrap_or("This task save was refused; your text is kept")
                        .into(),
                })
            }
        })
    }

    /// Persist a known outcome before unlocking the browser; pending intents retain their text.
    pub(in crate::save) fn finish(
        &mut self,
        result: Result<Value, Failure>,
    ) -> Result<Value, Failure> {
        let terminal = match &result {
            Ok(value) if value["slug"].is_string() => value.clone(),
            Err(error) if error.status < 500 => json!({"status":error.status,"error":error.error}),
            _ => return result,
        };
        let bytes = serde_json::to_vec(&terminal).expect("JSON value serializes");
        let file = &mut self.file.0;
        let persisted = file
            .seek(SeekFrom::Start(64))
            .and_then(|_| file.write_all(&bytes))
            .and_then(|()| file.set_len(64 + bytes.len() as u64))
            .and_then(|()| file.sync_all());
        match persisted {
            Ok(()) => journal::remove(&self.root, &self.identity),
            Err(_) if result.is_err() => {
                // Truncating a known refusal needs no data block and prevents any later dispatch.
                if let Err(error) = file.set_len(0).and_then(|()| file.sync_all()) {
                    return Ok(pending(&failure(&self.path, error).error));
                }
                journal::remove(&self.root, &self.identity);
            }
            Err(_) => {} // A confirmed board commit remains saved; its journal still supports checks.
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_tree::Tree;

    fn open(root: &Path, payload: &Value) -> Receipt {
        Receipt::open(root, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "", payload)
            .unwrap_or_else(|error| panic!("{}", error.error))
    }

    #[test]
    fn save_receipts_bind_canonical_text_and_replay_terminal_outcomes() {
        let tree = Tree::new();
        let payload = json!({"title":"Task", "nested":{"z":1,"a":2}});
        let fingerprint;
        {
            let receipt = open(&tree.root, &payload);
            assert!(receipt.fresh, "first receipt was not fresh");
            fingerprint = receipt.fingerprint;
        }
        let reordered: Value =
            serde_json::from_str(r#"{"nested":{"a":2,"z":1},"title":"Task"}"#).unwrap();
        let mut repeated = open(&tree.root, &reordered);
        assert!(!repeated.fresh, "existing receipt was fresh");
        assert_eq!(repeated.fingerprint, fingerprint);
        assert_eq!(
            repeated.finish(Ok(json!({"slug":"inbox/task"}))).ok(),
            Some(json!({"slug":"inbox/task"}))
        );
        drop(repeated);
        assert_eq!(
            open(&tree.root, &payload).completed().unwrap().ok(),
            Some(json!({"slug":"inbox/task"}))
        );
        let error = Receipt::open(
            &tree.root,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "other",
            &payload,
        )
        .err()
        .expect("scope cannot rebind the request");
        assert_eq!(error.status, 400);
        assert!(error.error.contains("different text"), "{}", error.error);
    }

    /// What held the lock in the full library runs that found a receipt reopened at once still
    /// locked (a 503, or a panic in `open`): a child a sibling test spawned while the receipt was
    /// open, which keeps its copy of the receipt's descriptor, the same open file description,
    /// until its exec. A child handed that copy as its output is that child, held while its input
    /// is open.
    #[test]
    fn a_receipt_lets_go_of_its_lock_whatever_copy_of_its_descriptor_a_child_still_holds() {
        use symphony_board::sys::{Command, Stdio};
        let tree = Tree::new();
        let payload = json!({"title":"Task"});
        let receipt = open(&tree.root, &payload);
        let fingerprint = receipt.fingerprint.clone();
        let mut child = Command::new("/bin/cat")
            .stdin(Stdio::piped())
            .stdout(receipt.file.0.try_clone().unwrap())
            .spawn()
            .unwrap();
        drop(receipt);
        let repeated = open(&tree.root, &payload);
        assert!(
            child.try_wait().unwrap().is_none(),
            "the child let go of its copy before the receipt was opened again"
        );
        assert!(!repeated.fresh, "existing receipt was fresh");
        assert_eq!(repeated.fingerprint, fingerprint);
        drop(repeated);
        assert!(child.wait().unwrap().success());
    }

    #[test]
    fn save_receipts_keep_pending_intents_and_persist_known_refusals() {
        let tree = Tree::new();
        let payload = json!({"title":"Task"});
        let mut receipt = open(&tree.root, &payload);
        assert_eq!(
            receipt.finish(Ok(pending("waiting"))).ok(),
            Some(pending("waiting"))
        );
        assert!(receipt.completed().is_none(), "pending became terminal");
        let error = receipt
            .finish(Err(Failure {
                status: 422,
                error: "no write".into(),
            }))
            .err()
            .unwrap();
        assert_eq!(error.status, 422);
        drop(receipt);
        let error = open(&tree.root, &payload)
            .completed()
            .unwrap()
            .err()
            .unwrap();
        assert_eq!(error.status, 422);
        assert_eq!(error.error, "no write");
    }

    #[test]
    fn a_save_interrupted_before_its_journal_refuses_without_dispatch() {
        let tree = Tree::new();
        let payload = json!({"request":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","title":"Task"});
        let receipt = open(&tree.root, &payload);
        let path = receipt.path.clone();
        drop(receipt);
        std::fs::write(path, "").unwrap();
        let saves = crate::save::Saves::start(&tree.root);
        for _ in 0..2 {
            let error = saves
                .post("", "/api/quick-task", &payload)
                .expect_err("no journal can have dispatched a task");
            assert_eq!(error.status, 422, "{}", error.error);
            assert!(error.error.contains("before dispatch"), "{}", error.error);
        }
        saves.close();
    }

    #[test]
    fn unreadable_save_state_distinguishes_fresh_and_already_dispatched_intents() {
        let payload = json!({"request":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","title":"Task"});
        for (case, status) in [("fresh", 422), ("checking", 503), ("different-text", 400)] {
            let tree = Tree::new();
            if case == "checking" {
                drop(open(&tree.root, &payload));
            }
            let directory = tree.root.join("artifacts/symphony-quick-tasks");
            std::fs::create_dir_all(&directory).unwrap();
            let journal = if case == "different-text" {
                json!({"slug":"inbox/task","title":"Task","body":"","priority":2,"links":[],"payload":["",{"title":"Other"}]}).to_string()
            } else {
                "broken JSON".into()
            };
            std::fs::write(
                directory.join("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.json"),
                journal,
            )
            .unwrap();
            let saves = crate::save::Saves::start(&tree.root);
            let error = saves
                .post("", "/api/quick-task", &payload)
                .expect_err("unreadable intent cannot dispatch");
            assert_eq!(error.status, status, "{case}: {}", error.error);
            saves.close();
        }
        let tree = Tree::new();
        let receipt = open(&tree.root, &payload);
        let path = receipt.path.clone();
        drop(receipt);
        std::fs::write(&path, "short").unwrap();
        let error = Receipt::open(&tree.root, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "", &payload)
            .err()
            .expect("a short fingerprint cannot have dispatched");
        assert_eq!(error.status, 422);
        assert!(error.error.contains("before dispatch"), "{}", error.error);
        assert!(
            error.error.contains(path.to_str().unwrap()),
            "{}",
            error.error
        );
    }

    #[test]
    fn fresh_save_receipt_folder_and_file_failures_keep_a_repeatable_refusal() {
        let payload = json!({"request":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","title":"Task"});
        for case in ["folder", "file"] {
            let tree = Tree::new();
            let directory = tree.root.join("artifacts/symphony-quick-tasks/.receipts");
            if case == "folder" {
                std::fs::create_dir_all(directory.parent().expect("receipt parent")).unwrap();
                std::fs::write(&directory, "blocked").unwrap();
            } else {
                std::fs::create_dir_all(directory.join("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"))
                    .unwrap();
            }
            let saves = crate::save::Saves::start(&tree.root);
            for _ in 0..2 {
                let error = saves
                    .post("", "/api/quick-task", &payload)
                    .expect_err("unwritable receipt");
                assert_eq!(error.status, 422, "{case}: {}", error.error);
                assert!(
                    error.error.contains("task save receipt"),
                    "{case}: {}",
                    error.error
                );
            }
            saves.close();
        }
    }
}
