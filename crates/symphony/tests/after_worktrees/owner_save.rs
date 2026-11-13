//! Private database delays at the owner's quick-task save boundary.

#[path = "../../../tasks/tests/store/common.rs"]
#[expect(
    dead_code,
    reason = "Only the private database client is needed by this save fixture"
)]
mod board;

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read};
use std::net::TcpStream;
use std::time::{Duration, Instant};
use symphony_board::sys::Command;

const CHILD: &str = "SYMPHONY_OWNER_SAVE_PROBE";
const CASE: &str = "owner_save::reports_each_private_database_timeout_phase";
const LIMIT: Duration = Duration::from_secs(30);

fn packet(stream: &mut BufReader<TcpStream>) -> std::io::Result<Option<(String, Vec<u8>)>> {
    let mut headers = String::new();
    let mut size = 0;
    loop {
        let mut line = String::new();
        if stream.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        headers.push_str(&line);
        if let Some((key, value)) = line.split_once(':')
            && key.eq_ignore_ascii_case("content-length")
        {
            size = value.trim().parse().unwrap();
        }
        if line == "\r\n" {
            break;
        }
    }
    let mut bytes = vec![0; size];
    stream.read_exact(&mut bytes)?;
    Ok(Some((headers, bytes)))
}

#[path = "owner_save/proxy.rs"]
mod proxy;
use proxy::Delayed;

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
    let began = Instant::now();
    let mut result = saves.post("", "/api/quick-task", &payload);
    let first = match &result {
        Ok(value) => json!({"saved":value}),
        Err(error) => json!({"status":error.status,"error":error.error}),
    };
    let elapsed = began.elapsed().as_secs_f64();
    if no_task_phase(&phase) {
        let refused = result.expect_err("a pre-dispatch failure must unlock the draft");
        assert_eq!(refused.status, 422, "{}", refused.error);
        let again = saves
            .post("", "/api/quick-task", &payload)
            .expect_err("refusal stays terminal");
        assert_eq!(again.status, 422);
        if matches!(phase.as_str(), "receipt-write" | "journal-write") {
            let receipt = root
                .join("artifacts/symphony-quick-tasks/.receipts/11111111111111111111111111111111");
            assert_eq!(
                std::fs::metadata(receipt).unwrap().len(),
                0,
                "a failed attempt remains sealed"
            );
        }
        saves.close();
        println!(
            "OWNER_SAVE_PROBE {}",
            json!({"first":first,"elapsed_s":elapsed,"confirmed":null})
        );
        return;
    }
    while !result.as_ref().is_ok_and(|value| value["slug"].is_string()) {
        assert!(began.elapsed() < LIMIT, "save never resolved: {first}");
        assert!(
            result.as_ref().is_ok_and(|value| value["pending"] == true),
            "an uncertain write or check must stay pending: {first}"
        );
        result = saves.post("", "/api/quick-task", &payload);
    }
    let slug = result.unwrap_or_else(|error| panic!("{}", error.error))["slug"].clone();
    assert_eq!(slug, "inbox/owner-delay-probe");
    saves.close();
    let journal = root.join("artifacts/symphony-quick-tasks/11111111111111111111111111111111.json");
    assert!(
        !journal.exists(),
        "confirmed typed text remains at {}",
        journal.display()
    );
    replay(root, &payload, &slug, &phase);
    println!(
        "OWNER_SAVE_PROBE {}",
        json!({"first":first,"elapsed_s":elapsed,"confirmed":slug})
    );
}

fn replay(root: &std::path::Path, payload: &Value, slug: &Value, phase: &str) {
    std::fs::remove_file(root.join("docs/PROBE.md")).unwrap();
    let reopened = symphony::save::Saves::start(root);
    let repeated = reopened
        .post("", "/api/quick-task", payload)
        .unwrap_or_else(|error| panic!("{}", error.error));
    assert_eq!(
        repeated["slug"], *slug,
        "a new Saves must recover the same receipt"
    );
    if phase == "wipe" {
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
    }
    refusals(&reopened, payload, slug);
    reopened.close();
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
    let client = board::client();
    let store = symphony_board::Store::open(symphony_board::rpc::Client::new(
        client.addr(),
        "sf",
        client.database(),
    ))
    .unwrap();
    let delay = Delayed::start(client.addr().into(), phase, &root);
    let mut command = child_command(phase);
    let done = symphony_board::sys::output(
        command
            .args(["--exact", CASE, "--nocapture"])
            .env(CHILD, &root)
            .env("SYMPHONY_OWNER_SAVE_PHASE", phase)
            .env("TASKS_SERVER", delay.address.to_string())
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
    let delays = match phase {
        "check" | "reopen" => 3,
        "open" | "lookup" => 2,
        "journal" | "concurrent" | "receipt-write" | "journal-write" | "interrupted" => 0,
        _ => 1,
    };
    for _ in 0..delays {
        delay.delayed.recv_timeout(LIMIT).unwrap_or_else(|error| {
            panic!("{phase} was not delayed to completion: {error}; {out}")
        });
    }
    let record: Value = serde_json::from_str(
        out.lines()
            .find_map(|line| line.strip_prefix("OWNER_SAVE_PROBE "))
            .unwrap(),
    )
    .unwrap();
    check_phase(&store, phase, &record);
}

fn no_task_phase(phase: &str) -> bool {
    matches!(
        phase,
        "open" | "lookup" | "journal" | "receipt-write" | "journal-write" | "interrupted"
    )
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
        phase if no_task_phase(phase) => 0,
        "concurrent" => 3,
        _ => 1,
    };
    assert_eq!(tasks.len(), count, "{phase}: {record}");
    assert_eq!(messages, count, "{phase}: {record}");
    if count == 1 {
        let task = &tasks[0];
        if phase == "edited" {
            assert_eq!(task.title, "Edited title");
            assert_eq!(task.body, "Edited body");
            assert_eq!(task.priority, 4);
            assert_eq!(task.links, Vec::<String>::new());
        } else {
            assert_eq!(task.title, "Owner delay probe");
            assert_eq!(
                task.body,
                "Keep these typed details\n\nSent from [/docs/PROBE.md#probe](/docs/PROBE.md#probe)"
            );
            assert_eq!(task.links, ["docs/PROBE.md#probe"]);
        }
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
fn reports_each_private_database_timeout_phase() {
    if let Ok(root) = std::env::var(CHILD) {
        child(&root);
        return;
    }
    for name in [
        "open",
        "lookup",
        "create",
        "message",
        "reply",
        "check",
        "reopen",
        "edited",
        "wipe",
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
    let store = symphony_board::Store::open(board::client()).unwrap();
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
