//! A locked, durable receipt serializes one identity and remembers its terminal outcome.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::super::Failure;
use super::{journal, pending};

/// The lock stays owned through preflight, dispatch and confirmation.
pub(in crate::save) struct Receipt {
    file: File,
    path: PathBuf,
    root: PathBuf,
    identity: String,
    /// Binds the UUID, scope and canonical submitted text to the board row.
    pub(in crate::save) fingerprint: String,
    /// Only the lock holder that initializes the receipt may prepare a new intent.
    pub(in crate::save) fresh: bool,
    terminal: Option<Value>,
}

fn failure(path: &Path, cause: impl std::fmt::Display) -> Failure {
    Failure {
        status: 503,
        error: format!("task save receipt {}: {cause}", path.display()),
    }
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
    /// Bind a UUID to its canonical payload before any board work and lock every replay.
    pub(in crate::save) fn open(
        root: &Path,
        identity: &str,
        scope: &str,
        payload: &Value,
    ) -> Result<Self, Failure> {
        let directory = root.join("artifacts/symphony-quick-tasks/.receipts");
        std::fs::create_dir_all(&directory).map_err(|error| failure(&directory, error))?;
        let path = directory.join(identity);
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|error| failure(&path, error))?;
        file.lock().map_err(|error| failure(&path, error))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| failure(&path, error))?;
        let fingerprint: String = Sha256::digest(canonical(&json!([scope, payload])).to_string())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let fresh = bytes.is_empty();
        let terminal = if fresh {
            file.write_all(fingerprint.as_bytes())
                .map_err(|error| failure(&path, error))?;
            None
        } else {
            if bytes.len() < 64 {
                return Err(failure(
                    &path,
                    "receipt is incomplete; check the original save",
                ));
            }
            if bytes[..64] != *fingerprint.as_bytes() {
                return Err(
                    "this task save identity belongs to different text; check the original save"
                        .to_string()
                        .into(),
                );
            }
            if bytes.len() == 64 {
                None
            } else {
                serde_json::from_slice(&bytes[64..]).ok()
            }
        };
        // A replay can read bytes whose writer could not confirm their durability.
        file.sync_all().map_err(|error| failure(&path, error))?;
        let artifacts = root.join("artifacts");
        let folders = [&directory, directory.parent().unwrap(), &artifacts, root];
        for folder in folders {
            File::open(folder)
                .and_then(|file| file.sync_all())
                .map_err(|error| failure(folder, error))?;
        }
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
        let persisted = self
            .file
            .seek(SeekFrom::Start(64))
            .and_then(|_| self.file.write_all(&bytes))
            .and_then(|()| self.file.set_len(64 + bytes.len() as u64))
            .and_then(|()| self.file.sync_all());
        match persisted {
            Ok(()) => journal::remove(&self.root, &self.identity),
            Err(error) if result.is_err() => return Ok(pending(&failure(&self.path, error).error)),
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
        drop(open(&tree.root, &payload));
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
            .expect("partial receipt stays uncertain");
        assert_eq!(error.status, 503);
        assert!(error.error.contains("incomplete"), "{}", error.error);
        assert!(
            error.error.contains(path.to_str().unwrap()),
            "{}",
            error.error
        );
    }
}
