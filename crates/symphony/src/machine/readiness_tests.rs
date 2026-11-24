//! Registration and init read the board API on a private port, with a scratch machine registry.

use super::*;
use std::io::BufRead;
use std::net::TcpListener;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering::SeqCst},
};

fn board_readiness(front: &symphony_api::Front, id: &str) -> Result<(), String> {
    board_readiness_within(front, id, test_time::backstop(LOOK))
}

fn init_steps(root: &Path, machine: &crate::init::Machine) -> Vec<String> {
    crate::init::steps_within(root, machine, test_time::backstop(LOOK))
}

/// A private host whose shell always works, independently of its board's answer. Drop wakes and
/// joins its owned thread, including when an assertion fails.
pub(super) struct Peer {
    pub front: symphony_api::Front,
    paths: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Peer {
    pub fn new(schema: i64, status: u16, body: &str) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let mut front = front(&[], crate::machine::PIN, schema);
        front.port = listener.local_addr().unwrap().port();
        let said = serde_json::to_string(&front).unwrap();
        let body = body.to_string();
        let paths = Arc::new(Mutex::new(Vec::new()));
        let seen = paths.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let worker = std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                if stopped.load(SeqCst) {
                    break;
                }
                stream
                    .set_read_timeout(Some(test_time::backstop(LOOK)))
                    .unwrap();
                let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let path = line.split_whitespace().nth(1).unwrap().to_string();
                loop {
                    line.clear();
                    if reader.read_line(&mut line).unwrap() <= 2 {
                        break;
                    }
                }
                seen.lock().unwrap().push(path.clone());
                let (code, answer) = if path == symphony_api::FRONT_PATH {
                    (200, said.as_str())
                } else if path.ends_with("/tasks") {
                    (200, "<html>shell</html>")
                } else {
                    (status, body.as_str())
                };
                write!(
                    stream,
                    "HTTP/1.0 {code} answer\r\nConnection: close\r\n\r\n{answer}"
                )
                .unwrap();
            }
        });
        Self {
            front,
            paths,
            stop,
            worker: Some(worker),
        }
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.store(true, SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.front.port));
        let result = self.worker.take().unwrap().join();
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}

#[test]
fn an_older_registration_names_the_newer_hosts_board_refusal_without_claiming_readiness() {
    let tree = Tree::new();
    let root = project(&tree, "lamp", "");
    tree.write("lamp/.git/HEAD", "ref: refs/heads/main\n");
    let machine = tree.path("machine");
    let schema = symphony_board::SCHEMA;
    let why = format!(
        "the lamp board is at schema {schema}, behind this build's {}",
        schema + 2
    );
    let peer = Peer::new(
        schema + 2,
        503,
        &serde_json::json!({"error":why}).to_string(),
    );
    assert_eq!(
        get(peer.front.port, "/lamp/tasks", test_time::backstop(LOOK))
            .unwrap()
            .0,
        200
    );
    let added = add(&root, &tree.root, &machine, Some(&peer.front)).unwrap();
    assert!(added.contains("registered at /lamp/"), "{added}");
    assert!(!added.contains("the running Symphony serves it"), "{added}");
    assert!(added.contains(&format!("HTTP 503: {why}")), "{added}");
    assert!(
        added.contains("first link a board command compatible"),
        "{added}"
    );
    assert!(added.contains("`just tasks migrate`"), "{added}");
    assert!(added.contains("within 60 seconds"), "{added}");
    assert!(Registry::read(&machine).unwrap().get("lamp").is_some());
    assert_eq!(
        *peer.paths.lock().unwrap(),
        ["/lamp/tasks", "/lamp/api/task-board?counts=1&work=0"]
    );
}

#[test]
fn only_board_json_is_ready_even_when_the_host_and_registration_schemas_match() {
    for (status, body, expected) in [
        (200, "<html>shell</html>", "did not contain task-board JSON"),
        (
            200,
            r#"{"error":"not a listing"}"#,
            "did not contain task-board JSON",
        ),
        (200, "{", "did not contain task-board JSON"),
        (
            503,
            r#"{"error":"board unavailable"}"#,
            "HTTP 503: board unavailable",
        ),
        (404, "gone", "HTTP 404: no board JSON answer"),
    ] {
        let peer = Peer::new(symphony_board::SCHEMA, status, body);
        let why = board_readiness(&peer.front, "lamp").unwrap_err();
        assert!(why.contains(expected), "{why}");
    }
    let peer = Peer::new(symphony_board::SCHEMA, 200, r#"{"tasks":[],"next":null}"#);
    assert_eq!(board_readiness(&peer.front, "lamp"), Ok(()));
    let mut gone = peer.front.clone();
    gone.port = 0;
    assert!(
        board_readiness(&gone, "lamp")
            .unwrap_err()
            .starts_with("board readiness at http://127.0.0.1:0/")
    );
}

#[test]
fn an_incomplete_or_invalid_http_answer_is_not_readiness() {
    for (answer, expected) in [
        ("HTTP/1.0 200 OK\r\n", "incomplete HTTP answer"),
        ("HTTP/1.0 nope\r\n\r\n{}", "invalid HTTP status"),
        ("missing\r\n\r\n{}", "invalid HTTP status"),
    ] {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(test_time::backstop(LOOK)))
                .unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            loop {
                line.clear();
                if reader.read_line(&mut line).unwrap() <= 2 {
                    break;
                }
            }
            stream.write_all(answer.as_bytes()).unwrap();
        });
        let result = get(port, "/api/front", test_time::backstop(LOOK));
        worker.join().unwrap();
        assert_eq!(result, Err(expected.into()));
    }
}

#[test]
fn init_keeps_the_board_refusal_as_a_next_step_for_a_registered_project() {
    let tree = Tree::checkout();
    tree.write("symphony.toml", "[project]\nid = 'lamp'\n");
    let machine = tree.path("machine");
    let entry = Entry::of(&tree.root).unwrap();
    Registry::change(&machine, |registry| registry.add(entry)).unwrap();
    let peer = Peer::new(
        symphony_board::SCHEMA + 2,
        503,
        r#"{"error":"schema behind this build"}"#,
    );
    tree.write(
        "machine/server.json",
        &serde_json::json!({"port":peer.front.port}).to_string(),
    );
    let steps = init_steps(
        &tree.root,
        &crate::init::Machine {
            folder: Some(&machine),
            path: None,
            beside: None,
        },
    );
    let refusal = steps
        .iter()
        .find(|step| step.starts_with("board readiness"))
        .unwrap();
    assert!(
        refusal.contains("HTTP 503: schema behind this build"),
        "{steps:?}"
    );
    assert_eq!(
        *peer.paths.lock().unwrap(),
        [
            symphony_api::FRONT_PATH,
            "/lamp/api/task-board?counts=1&work=0"
        ]
    );
    let healthy = Peer::new(symphony_board::SCHEMA, 200, r#"{"tasks":[],"next":null}"#);
    tree.write(
        "machine/server.json",
        &serde_json::json!({"port":healthy.front.port}).to_string(),
    );
    let steps = init_steps(
        &tree.root,
        &crate::init::Machine {
            folder: Some(&machine),
            path: None,
            beside: None,
        },
    );
    assert!(
        !steps.iter().any(|step| step.starts_with("board readiness")),
        "{steps:?}"
    );
}

#[test]
fn init_identifies_the_selected_older_installed_writer_even_before_registration() {
    use std::os::unix::fs::PermissionsExt;
    let tree = Tree::checkout();
    let machine = tree.path("machine");
    let schema = symphony_board::SCHEMA;
    let writer = tree.write(
        "machine/releases/release-old/bin/tasks",
        "#!/bin/sh\nexit 0\n",
    );
    std::fs::set_permissions(&writer, std::fs::Permissions::from_mode(0o755)).unwrap();
    tree.write(
        "machine/releases/release-old/manifest.json",
        &serde_json::json!({
            "release":"release-old", "commit":"0123abcd", "schema":schema - 2,
            "protocol":{"major":0,"minor":3}, "tasks_api":{"major":1,"minor":14},
            "pin":crate::machine::PIN, "client":"client/hash", "artifacts":[],
        })
        .to_string(),
    );
    std::os::unix::fs::symlink("releases/release-old", machine.join("current")).unwrap();
    let peer = Peer::new(schema, 200, r#"{"tasks":[],"next":null}"#);
    tree.write(
        "machine/server.json",
        &serde_json::json!({"port":peer.front.port}).to_string(),
    );
    let steps = || {
        init_steps(
            &tree.root,
            &crate::init::Machine {
                folder: Some(&machine),
                path: None,
                beside: None,
            },
        )
    };
    let asked = steps();
    let warning = asked
        .iter()
        .find(|step| step.starts_with("link a compatible board command"))
        .unwrap();
    assert!(
        warning.contains(&format!("release-old at schema {}", schema - 2)),
        "{warning}"
    );
    assert!(
        warning.contains(&format!("host requires schema {schema}")),
        "{warning}"
    );
    assert_eq!(
        crate::builds::find(
            &tree.root,
            "tasks",
            &crate::builds::Beyond {
                machine: Some(machine.clone()),
                path: None
            }
        ),
        Some(machine.join("current/bin/tasks"))
    );
    let manifest_path = "machine/releases/release-old/manifest.json";
    let mut manifest: serde_json::Value = serde_json::from_str(&tree.read(manifest_path)).unwrap();
    for compatible in [schema, schema + 1] {
        manifest["schema"] = compatible.into();
        tree.write(manifest_path, &manifest.to_string());
        assert!(
            !steps()
                .iter()
                .any(|step| step.starts_with("link a compatible board command"))
        );
    }
    manifest["schema"] = (schema - 2).into();
    tree.write(manifest_path, &manifest.to_string());
    assert!(
        steps()
            .iter()
            .any(|step| step.starts_with("link a compatible board command"))
    );
    // A checkout's own link takes precedence. Init neither replaces it nor assumes its schema.
    let linked = tree.write(".target/bin/tasks", "#!/bin/sh\nexit 0\n");
    std::fs::set_permissions(&linked, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        !steps()
            .iter()
            .any(|step| step.starts_with("link a compatible board command"))
    );
    assert_eq!(
        std::fs::read_to_string(linked).unwrap(),
        "#!/bin/sh\nexit 0\n"
    );
    // No board was made or started.
    for made in ["postgresql", "backend.json"] {
        assert!(!tree.path(&format!(".git/symphony/tasks/{made}")).exists());
    }
}
