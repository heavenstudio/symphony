//! The owner's quick-task save at its board boundary: a confirmed save is recovered by a new
//! worker, also from its receipt's fingerprint alone; a save whose write it cannot confirm stays
//! pending until a check finds its task, or is confirmed at once when its check does; a save
//! refused before it reaches the board stays refused and writes nothing; and saves made at once by
//! separate workers make each task once.

#[path = "../../../tasks/tests/store/common.rs"]
#[expect(
    dead_code,
    reason = "Only the private database client is needed by this save fixture"
)]
mod board;

use serde_json::{Value, json};
use std::time::Duration;
use symphony_board::sys::Command;

const CHILD: &str = "SYMPHONY_OWNER_SAVE_PROBE";
const CASE: &str = "owner_save::reports_each_private_database_failure_phase";
/// How long a save may stay uncertain before the wait for it is a stall: the backstop of 30 s.
const LIMIT: Duration = test_time::backstop(Duration::from_secs(30));

#[path = "owner_save/proxy.rs"]
mod proxy;
use proxy::Holding;

/// The phases whose save is confirmed: plain, or after a write it could not confirm.
fn confirmed_phase(phase: &str) -> bool {
    matches!(phase, "saved" | "create" | "reply")
}

fn payload() -> Value {
    json!({"request":"11111111111111111111111111111111", "title":"Owner delay probe",
        "body":"Keep these typed details", "page":"/docs/PROBE.md#probe"})
}

fn validate(saves: &symphony::save::Saves, payload: &Value) {
    let mut legacy = payload.clone();
    legacy.as_object_mut().unwrap().remove("request");
    let refused = saves
        .post("", "/api/quick-task", &legacy)
        .expect_err("legacy form");
    assert_eq!(refused.status, 400);
    assert!(
        refused.error.contains("Reload this page"),
        "{}",
        refused.error
    );
    let mut too_long = payload.clone();
    too_long["title"] = json!("x".repeat(symphony_api::QUICK_TASK_TITLE_MAX + 1));
    let refused = saves
        .post("", "/api/quick-task", &too_long)
        .expect_err("overlong title");
    assert_eq!(refused.status, 400);
    assert!(
        refused.error.contains("characters for the task title"),
        "{}",
        refused.error
    );
}

fn child(root: &str) {
    let root = std::path::Path::new(root);
    let phase = std::env::var("SYMPHONY_OWNER_SAVE_PHASE").unwrap();
    if phase == "concurrent" {
        concurrent::saves(root);
        println!(
            "OWNER_SAVE_PROBE {}",
            json!({"confirmed":"three distinct tasks"})
        );
        return;
    }
    let saves = symphony::save::Saves::start(root);
    let mut payload = payload();
    if phase == "journal-write" {
        payload["body"] = json!("x".repeat(2000));
    }
    validate(&saves, &payload);
    let result = saves.post("", "/api/quick-task", &payload);
    let first = match &result {
        Ok(value) => json!({"saved":value}),
        Err(error) => json!({"status":error.status,"error":error.error}),
    };
    if confirmed_phase(&phase) {
        let mut result = Some(result);
        let saved = test_time::until("the save confirmed", LIMIT, || {
            let now = result
                .take()
                .unwrap_or_else(|| saves.post("", "/api/quick-task", &payload));
            match now {
                Ok(value) if value["slug"].is_string() => Some(value),
                Ok(value) if value["pending"] == true => None,
                Ok(value) => panic!("an uncertain save must stay pending: {value}"),
                Err(error) => panic!("{}", error.error),
            }
        })
        .unwrap_or_else(|stall| panic!("{stall}: {first}"));
        let slug = saved["slug"].clone();
        assert_eq!(slug, "inbox/owner-delay-probe");
        saves.close();
        let journal =
            root.join("artifacts/symphony-quick-tasks/11111111111111111111111111111111.json");
        assert!(
            !journal.exists(),
            "confirmed typed text remains at {}",
            journal.display()
        );
        replay(root, &payload, &slug);
        println!(
            "OWNER_SAVE_PROBE {}",
            json!({"first":first,"confirmed":slug})
        );
        return;
    }
    let refused = result.expect_err("a pre-dispatch failure must unlock the draft");
    assert_eq!(refused.status, 422, "{}", refused.error);
    let again = saves
        .post("", "/api/quick-task", &payload)
        .expect_err("refusal stays terminal");
    assert_eq!(again.status, 422);
    if matches!(phase.as_str(), "receipt-write" | "journal-write") {
        let receipt =
            root.join("artifacts/symphony-quick-tasks/.receipts/11111111111111111111111111111111");
        assert_eq!(
            std::fs::metadata(receipt).unwrap().len(),
            0,
            "a failed attempt remains sealed"
        );
    }
    saves.close();
    println!(
        "OWNER_SAVE_PROBE {}",
        json!({"first":first,"confirmed":null})
    );
}

/// A confirmed save recovered by a new `Saves`, and the saves it refuses after.
fn replay(root: &std::path::Path, payload: &Value, slug: &Value) {
    std::fs::remove_file(root.join("docs/PROBE.md")).unwrap();
    let reopened = symphony::save::Saves::start(root);
    let repeated = reopened
        .post("", "/api/quick-task", payload)
        .unwrap_or_else(|error| panic!("{}", error.error));
    assert_eq!(
        repeated["slug"], *slug,
        "a new Saves must recover the same receipt"
    );
    // A receipt left with its fingerprint alone, its outcome gone, is checked on the board.
    let receipt =
        root.join("artifacts/symphony-quick-tasks/.receipts/11111111111111111111111111111111");
    std::fs::OpenOptions::new()
        .write(true)
        .open(receipt)
        .unwrap()
        .set_len(64)
        .unwrap();
    let checked = reopened
        .post("", "/api/quick-task", payload)
        .unwrap_or_else(|error| panic!("{}", error.error));
    assert_eq!(
        checked["slug"], *slug,
        "a header without its outcome checks the board fingerprint"
    );
    refusals(&reopened, payload, slug);
    reopened.close();
}

fn refusals(saves: &symphony::save::Saves, payload: &Value, slug: &Value) {
    let mut invalid = payload.clone();
    invalid["request"] = json!("22222222222222222222222222222222");
    invalid["title"] = json!("Invalid priority");
    invalid["priority"] = json!(99);
    let refused = saves
        .post("", "/api/quick-task", &invalid)
        .expect_err("invalid priority");
    assert_eq!(refused.status, 400);
    let mut collision = payload.clone();
    collision["request"] = json!("33333333333333333333333333333333");
    collision["slug"] = slug.clone();
    collision["title"] = json!("Unrelated owner task");
    for _ in 0..2 {
        let refused = saves
            .post("", "/api/quick-task", &collision)
            .expect_err("unrelated save");
        assert_eq!(refused.status, 400);
    }
    let mut changed = payload.clone();
    changed["title"] = json!("Different text under the same identity");
    let refused = saves
        .post("", "/api/quick-task", &changed)
        .expect_err("identity rebinding");
    assert_eq!(refused.status, 400);
}

#[path = "owner_save/concurrent.rs"]
mod concurrent;

fn scratch(base: &std::path::Path, phase: &str) -> std::path::PathBuf {
    let mut root = base.to_path_buf();
    if phase == "journal-write" {
        // The refusal includes this path, exceeding the child's one-block regular-file limit.
        while root.as_os_str().len() < 880 {
            let component = (879 - root.as_os_str().len()).min(200);
            root = root.join("x".repeat(component));
        }
    }
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("docs/PROBE.md"), "# Probe\n").unwrap();
    let directory = root.join("artifacts/symphony-quick-tasks");
    if phase == "journal" {
        std::fs::create_dir_all(&directory).unwrap();
        std::os::unix::fs::symlink(
            "missing-journal",
            directory.join("11111111111111111111111111111111.json"),
        )
        .unwrap();
    }
    if phase == "interrupted" {
        use sha2::Digest as _;
        let original = payload();
        let sorted = json!({"body":original["body"],"page":original["page"],"request":original["request"],"title":original["title"]});
        let fingerprint: String = sha2::Sha256::digest(json!(["", sorted]).to_string())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        std::fs::create_dir_all(directory.join(".receipts")).unwrap();
        std::fs::write(
            directory.join(".receipts/11111111111111111111111111111111"),
            fingerprint,
        )
        .unwrap();
    }
    root
}

fn child_command(phase: &str) -> Command {
    let executable = std::env::current_exe().unwrap();
    let limit = match phase {
        "receipt-write" => Some(0),
        "journal-write" => Some(1),
        _ => None,
    };
    if let Some(limit) = limit {
        let mut command = Command::new("/bin/sh");
        command
            .args([
                "-c",
                &format!(r#"trap '' XFSZ; ulimit -S -f {limit}; exec "$@""#),
                "owner-save-size",
            ])
            .arg(executable)
            // The lowered regular-file limit applies only to this private child; profiles are not regular files here.
            .env("LLVM_PROFILE_FILE", "/dev/null");
        command
    } else {
        Command::new(executable)
    }
}

fn phase(phase: &'static str) {
    let tree = super::Tree::new();
    tree.write("docs/PROBE.md", "# Probe\n");
    let root = scratch(&tree.root, phase);
    let client = board::postgresql();
    let store = symphony_board::Store::open(symphony_board::rpc::Client::new(
        &client.addr(),
        "sf",
        client.database(),
    ))
    .unwrap();
    // The phases whose write is made uncertain go through the proxy that holds it.
    let holding =
        matches!(phase, "create" | "reply").then(|| Holding::start(&client.addr(), phase));
    let server = holding
        .as_ref()
        .map_or_else(|| client.addr(), |held| held.address.clone());
    let mut command = child_command(phase);
    let done = symphony_board::sys::output(
        command
            .args(["--exact", CASE, "--nocapture"])
            .env(CHILD, &root)
            .env("SYMPHONY_OWNER_SAVE_PHASE", phase)
            .env("TASKS_SERVER", &server)
            .env("TASKS_DATABASE", client.database())
            .env("TASKS_SWEEP", "off")
            .env_remove("TASKS_SESSION")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_INDEX_FILE")
            .current_dir(&root),
    )
    .unwrap();
    let out = String::from_utf8(done.stdout).unwrap();
    assert!(
        done.status.success(),
        "{phase}: {out}\n{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let record: Value = serde_json::from_str(
        out.lines()
            .find_map(|line| line.strip_prefix("OWNER_SAVE_PROBE "))
            .unwrap(),
    )
    .unwrap();
    check_phase(&store, phase, &record);
    if let Some(held) = holding {
        assert_eq!(
            *held.ran.lock().unwrap(),
            1,
            "{phase}: the held CREATE ran once"
        );
        // The first answer says what the save could tell: nothing yet, or its task once checked.
        let first = &record["first"]["saved"];
        match phase {
            "create" => assert_eq!(first["pending"], true, "{record}"),
            _ => assert_eq!(first["slug"], "inbox/owner-delay-probe", "{record}"),
        }
    }
}

fn check_phase(store: &symphony_board::Store, phase: &str, record: &Value) {
    let tasks = store.tasks(None).unwrap();
    let messages: usize = tasks
        .iter()
        .map(|task| {
            store
                .thread(&task.slug)
                .unwrap()
                .iter()
                .filter(|message| message.text == "created by owner")
                .count()
        })
        .sum();
    let count = match phase {
        "concurrent" => 3,
        phase if confirmed_phase(phase) => 1,
        _ => 0,
    };
    assert_eq!(tasks.len(), count, "{phase}: {record}");
    assert_eq!(messages, count, "{phase}: {record}");
    if count == 1 {
        let task = &tasks[0];
        assert_eq!(task.title, "Owner delay probe");
        assert_eq!(
            task.body,
            "Keep these typed details\n\nSent from [/docs/PROBE.md#probe](/docs/PROBE.md#probe)"
        );
        assert_eq!(task.links, ["docs/PROBE.md#probe"]);
        assert!(
            task.fields["_symphony"]["owner_save"].is_string(),
            "{:?}",
            task.fields
        );
    }
    println!(
        "phase={phase} response={record} final_tasks={} creation_messages={messages}",
        tasks.len()
    );
}

#[test]
fn reports_each_private_database_failure_phase() {
    if let Ok(root) = std::env::var(CHILD) {
        child(&root);
        return;
    }
    for name in [
        "saved",
        "create",
        "reply",
        "receipt-write",
        "journal-write",
        "interrupted",
        "journal",
        "concurrent",
    ] {
        phase(name);
    }
}

#[test]
fn owner_task_receipts_commit_once_and_validation_writes_nothing() {
    let db = board::postgresql();
    let store = symphony_board::Store::open(symphony_board::rpc::Client::new(
        &db.addr(),
        "sf",
        db.database(),
    ))
    .unwrap();
    let mut new = symphony_board::NewTask {
        slug: "inbox/atomic-receipt".into(),
        title: "Atomic receipt".into(),
        by: "owner".into(),
        status: "concept".into(),
        ..symphony_board::NewTask::default()
    };
    let invalid = store
        .create_owner_task(&new, "")
        .expect_err("empty receipt");
    assert!(
        matches!(invalid, symphony_board::Error::Refused(_)),
        "{invalid:?}"
    );
    new.by = "someone-else".into();
    let invalid = store
        .create_owner_task(&new, "receipt")
        .expect_err("only owner creates owner tasks");
    assert!(
        matches!(invalid, symphony_board::Error::Refused(_)),
        "{invalid:?}"
    );
    assert_eq!(store.tasks(None).unwrap().len(), 0);
    new.by = "owner".into();
    let task = store.create_owner_task(&new, "first-receipt").unwrap();
    assert_eq!(task.fields["_symphony"]["owner_save"], "first-receipt");
    let duplicate = store
        .create_owner_task(&new, "second-receipt")
        .expect_err("slug guard");
    assert!(
        matches!(duplicate, symphony_board::Error::Refused(_)),
        "{duplicate:?}"
    );
    assert_eq!(
        store.get(&new.slug).unwrap().fields["_symphony"]["owner_save"],
        "first-receipt"
    );
    assert_eq!(store.tasks(None).unwrap().len(), 1);
    assert_eq!(store.thread(&new.slug).unwrap().len(), 1);
    let edited = store.set_priority(&new.slug, 4, "owner").unwrap();
    assert_eq!(edited.fields["_symphony"]["owner_save"], "first-receipt");
    assert_eq!(edited.priority, 4);
}
