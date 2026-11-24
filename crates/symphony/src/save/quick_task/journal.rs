//! A save identity fixes its slug before a database write can outlive its reply.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::super::Failure;

/// The chosen task fields and original payload, published before dispatch.
#[derive(Serialize, Deserialize)]
pub(in crate::save) struct Record {
    /// The slug fixed before the board can receive the create transaction.
    pub(in crate::save) slug: String,
    title: String,
    body: String,
    priority: i64,
    links: Vec<String>,
    /// Who asked for it, by their short name: `owner`, as a record written before it was kept says.
    #[serde(default = "the_machines_person")]
    by: String,
    /// Whether `by` is the person at the machine, under the name they gave themselves: the task
    /// is then filed as theirs ([`symphony_board::Store::for_machine`]). Left out for anyone else,
    /// and for `owner`, who is the person at the machine by that word alone.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    machine: bool,
    payload: Value,
}

/// Who a record written before records named who asked was asked by: the person at the machine.
fn the_machines_person() -> String {
    symphony_board::workflow::local_actor().sender.name
}

impl Record {
    /// Whether the one who asked for it is the person at the machine ([`Record::machine`]).
    pub(in crate::save) fn machine(&self) -> bool {
        self.machine
    }

    pub(in crate::save) fn task(&self) -> symphony_board::NewTask {
        symphony_board::NewTask {
            slug: self.slug.clone(),
            title: self.title.clone(),
            body: self.body.clone(),
            priority: self.priority,
            links: self.links.clone(),
            by: self.by.clone(),
            status: "concept".into(),
            ..symphony_board::NewTask::default()
        }
    }
}

/// Require the draft's lowercase hexadecimal save identity before dispatch.
pub(in crate::save) fn identity(payload: &Value) -> Result<String, Failure> {
    let supplied = super::super::string(payload, "request");
    if supplied.is_empty() {
        return Err("Reload this page before saving the task; your text is kept"
            .to_string()
            .into());
    }
    if supplied.len() != 32
        || !supplied
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(format!(
            "task save identity {supplied:?} must be 32 lowercase hexadecimal characters"
        )
        .into());
    }
    Ok(supplied.into())
}

fn path(root: &Path, identity: &str) -> PathBuf {
    root.join("artifacts/symphony-quick-tasks")
        .join(format!("{identity}.json"))
}

/// Unreadable intent metadata is conservatively treated as a possible dispatch.
pub(super) fn present(root: &Path, identity: &str) -> bool {
    match std::fs::metadata(path(root, identity)) {
        Ok(_) => true,
        Err(error) => !matches!(
            error.kind(),
            std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
        ),
    }
}

fn failure(path: &Path, error: impl std::fmt::Display) -> Failure {
    Failure {
        status: 503,
        error: format!("task save record {}: {error}", path.display()),
    }
}

/// Read an intent and refuse a different payload bound to the same identity.
pub(in crate::save) fn load(
    root: &Path,
    identity: &str,
    scope: &str,
    payload: &Value,
) -> Result<Option<Record>, Failure> {
    let path = path(root, identity);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(failure(&path, error)),
    };
    let record: Record = serde_json::from_slice(&bytes).map_err(|error| failure(&path, error))?;
    if record.payload != json!([scope, payload]) {
        return Err("this task save identity belongs to different text; check its save before starting another".to_string().into());
    }
    Ok(Some(record))
}

/// Publish a complete intent once, including when two private servers reach the same checkout.
pub(in crate::save) fn reserve(
    root: &Path,
    identity: &str,
    scope: &str,
    payload: &Value,
    new: &symphony_board::NewTask,
    machine: bool,
) -> Result<Record, Failure> {
    if let Some(record) = load(root, identity, scope, payload)? {
        return Ok(record);
    }
    let path = path(root, identity);
    let directory = path.parent().expect("save record has a directory");
    std::fs::create_dir_all(directory).map_err(|error| failure(directory, error))?;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let part = directory.join(format!(
        ".{identity}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let record = Record {
        slug: new.slug.clone(),
        title: new.title.clone(),
        body: new.body.clone(),
        priority: new.priority,
        links: new.links.clone(),
        // A filing that names nobody is the machine's person's, as a record from before names were
        // kept reads.
        by: Some(new.by.clone())
            .filter(|by| !by.is_empty())
            .unwrap_or_else(the_machines_person),
        machine: machine && new.by != the_machines_person(),
        payload: json!([scope, payload]),
    };
    let result = (|| {
        let bytes = serde_json::to_vec(&record).map_err(|error| failure(&part, error))?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&part)
            .map_err(|error| failure(&part, error))?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| failure(&part, error))?;
        match std::fs::hard_link(&part, &path) {
            Ok(()) => {
                std::fs::File::open(directory)
                    .and_then(|file| file.sync_all())
                    .map_err(|error| failure(directory, error))?;
                Ok(record)
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                load(root, identity, scope, payload)?
                    .ok_or_else(|| failure(&path, "record disappeared"))
            }
            Err(error) => Err(failure(&path, error)),
        }
    })();
    let _ = std::fs::remove_file(part);
    result
}

/// Drop typed text only after a durable terminal receipt replaces the pending intent.
pub(super) fn remove(root: &Path, identity: &str) {
    let path = path(root, identity);
    if std::fs::remove_file(&path).is_ok() {
        let _ = std::fs::File::open(path.parent().expect("journal directory"))
            .and_then(|file| file.sync_all());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_tree;

    fn ok<T>(result: Result<T, Failure>) -> T {
        result.unwrap_or_else(|error| panic!("{}", error.error))
    }

    fn task(slug: &str) -> symphony_board::NewTask {
        symphony_board::NewTask {
            slug: slug.into(),
            title: "First owner task".into(),
            body: "Original resolved details".into(),
            priority: 1,
            links: vec!["docs/PROBE.md#probe".into()],
            ..symphony_board::NewTask::default()
        }
    }

    #[test]
    fn save_identities_are_required_and_cannot_be_paths() {
        let missing =
            identity(&json!({"title":"Owner task"})).expect_err("an older page must keep its text");
        assert_eq!(missing.status, 400);
        assert!(
            missing.error.contains("Reload this page"),
            "{}",
            missing.error
        );
        for request in ["../file", "a", &"a".repeat(33), &"A".repeat(32)] {
            let error = identity(&json!({"request":request})).expect_err("unsafe identity");
            assert_eq!(error.status, 400);
        }
        let request = "a".repeat(32);
        assert_eq!(ok(identity(&json!({"request":request}))), request);
    }

    #[test]
    fn one_complete_record_wins_a_race_and_binds_its_original_text() {
        let tree = test_tree::Tree::new();
        let identity = "a".repeat(32);
        let payload = json!({"title":"First owner task"});
        let records = std::thread::scope(|scope| {
            let one = scope.spawn(|| {
                ok(reserve(
                    &tree.root,
                    &identity,
                    "",
                    &payload,
                    &task("inbox/one"),
                    false,
                ))
            });
            let two = scope.spawn(|| {
                ok(reserve(
                    &tree.root,
                    &identity,
                    "",
                    &payload,
                    &task("inbox/two"),
                    false,
                ))
            });
            [one.join().unwrap(), two.join().unwrap()]
        });
        assert_eq!(records[0].slug, records[1].slug);
        let restored = records[0].task();
        assert_eq!(restored.title, "First owner task");
        assert_eq!(restored.body, "Original resolved details");
        assert_eq!(restored.priority, 1);
        assert_eq!(restored.links, ["docs/PROBE.md#probe"]);
        assert_eq!(restored.by, "owner");
        assert_eq!(restored.status, "concept");
        let repeated = ok(reserve(
            &tree.root,
            &identity,
            "",
            &payload,
            &task("inbox/different-choice"),
            false,
        ));
        assert_eq!(repeated.slug, records[0].slug);
        assert_eq!(repeated.task().body, "Original resolved details");
        assert_eq!(
            ok(load(&tree.root, &identity, "", &payload)).unwrap().slug,
            records[0].slug
        );
        let files = std::fs::read_dir(path(&tree.root, &identity).parent().unwrap())
            .unwrap()
            .count();
        assert_eq!(files, 1, "only the complete durable intent remains");
        let error = load(&tree.root, &identity, "", &json!({"title":"Other text"}))
            .err()
            .expect("identity reuse must refuse");
        assert_eq!(error.status, 400);
        assert!(error.error.contains("different text"), "{}", error.error);
    }

    /// A record written before records kept who asked carries no `by`: it reads as the machine's
    /// person's, and the rest of it as written.
    #[test]
    fn a_record_from_before_records_named_who_asked_reads_as_the_machines_persons() {
        let tree = test_tree::Tree::new();
        let identity = "c".repeat(32);
        let payload = json!({"title":"First owner task"});
        let record = path(&tree.root, &identity);
        std::fs::create_dir_all(record.parent().unwrap()).unwrap();
        let old = json!({
            "slug": "inbox/old", "title": "First owner task", "body": "Original resolved details",
            "priority": 1, "links": ["docs/PROBE.md#probe"], "payload": ["", payload],
        });
        std::fs::write(&record, old.to_string()).unwrap();
        let read = ok(load(&tree.root, &identity, "", &payload)).expect("the old record");
        let restored = read.task();
        assert_eq!(
            (restored.slug.as_str(), restored.by.as_str()),
            ("inbox/old", "owner")
        );
        assert_eq!(restored.body, "Original resolved details");
        // A reservation of the same identity, by someone else, keeps the old record and its person.
        let other = symphony_board::NewTask {
            by: "ana".into(),
            ..task("inbox/new")
        };
        let kept = ok(reserve(&tree.root, &identity, "", &payload, &other, false));
        assert_eq!(
            (kept.slug.as_str(), kept.task().by.as_str()),
            ("inbox/old", "owner")
        );
    }

    /// A task the person at the machine asked for under the name they gave themselves is kept as
    /// theirs, so it is filed as theirs again after a restart; one under `owner` needs no mark, and
    /// a person a board named is never the machine's.
    #[test]
    fn a_record_keeps_whether_the_machines_person_asked_under_their_name() {
        let tree = test_tree::Tree::new();
        let payload = json!({"title":"First owner task"});
        let by = |by: &str| symphony_board::NewTask {
            by: by.into(),
            ..task("inbox/named")
        };
        let cases = [
            ("d", "stefano", true, true),
            ("e", "owner", true, false),
            ("f", "ana", false, false),
        ];
        for (letter, name, machine, kept) in cases {
            let identity = letter.repeat(32);
            let made = ok(reserve(
                &tree.root,
                &identity,
                "",
                &payload,
                &by(name),
                machine,
            ));
            assert_eq!((made.task().by.as_str(), made.machine()), (name, kept));
            let text = std::fs::read_to_string(path(&tree.root, &identity)).unwrap();
            assert_eq!(text.contains("\"machine\""), kept, "{text}");
            let read = ok(load(&tree.root, &identity, "", &payload)).unwrap();
            assert_eq!((read.task().by.as_str(), read.machine()), (name, kept));
        }
    }

    #[test]
    fn missing_corrupt_and_unwritable_records_are_distinct() {
        let tree = test_tree::Tree::new();
        let identity = "b".repeat(32);
        let payload = json!({"title":"Owner task"});
        let absent = ok(load(&tree.root, &identity, "", &payload));
        assert!(absent.is_none(), "unexpected journal for {identity}");
        let record = path(&tree.root, &identity);
        std::fs::create_dir_all(record.parent().unwrap()).unwrap();
        std::fs::write(&record, "unfinished JSON").unwrap();
        let corrupt = load(&tree.root, &identity, "", &payload)
            .err()
            .expect("corrupt record");
        assert_eq!(corrupt.status, 503);
        assert!(
            corrupt.error.contains(record.to_str().unwrap()),
            "{}",
            corrupt.error
        );
        let blocked = tree.root.join("blocked");
        std::fs::write(&blocked, "not a directory").unwrap();
        let error = reserve(
            &blocked,
            &identity,
            "",
            &payload,
            &task("inbox/task"),
            false,
        )
        .err()
        .expect("unwritable record");
        assert_eq!(error.status, 503);
        assert!(error.error.contains("blocked"), "{}", error.error);
    }
}
