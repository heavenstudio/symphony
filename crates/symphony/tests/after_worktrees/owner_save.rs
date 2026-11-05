//! Private database delays at the owner's quick-task save boundary.

#[path = "../../../tasks/tests/store/common.rs"]
#[expect(
    dead_code,
    reason = "Only the private database client is needed by this save fixture"
)]
mod board;

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use tasks::sys::Command;

const CHILD: &str = "SF_OWNER_SAVE_PROBE";
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

/// The proxy changes only the SQL sent to this case's private database. The database itself
/// waits inside the chosen phase; it can finish after the save's three-second socket wait.
struct Delayed {
    address: std::net::SocketAddr,
    worker: Option<std::thread::JoinHandle<()>>,
    workers: Arc<Mutex<Vec<std::thread::JoinHandle<()>>>>,
    delayed: mpsc::Receiver<()>,
}

impl Delayed {
    fn start(upstream: String, phase: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        assert_ne!(address.port(), 8765);
        let workers = Arc::new(Mutex::new(Vec::new()));
        let saved = workers.clone();
        let (send, delayed) = mpsc::channel();
        let chosen = Arc::new(Mutex::new(0_u8));
        let worker = std::thread::spawn(move || {
            for connection in listener.incoming() {
                let mut incoming = BufReader::new(connection.unwrap());
                let Some(first) = packet(&mut incoming).unwrap() else {
                    break;
                };
                let upstream = upstream.clone();
                let send = send.clone();
                let chosen = chosen.clone();
                saved.lock().unwrap().push(std::thread::spawn(move || {
                    let mut next = Some(first);
                    while let Some((headers, bytes)) = next.take() {
                        let mut call = tasks::cbor::decode(&bytes).unwrap();
                        let sql = call["params"][0].as_str().unwrap();
                        let create = sql.contains("CREATE type::record('task', $slug)");
                        let lookup =
                            sql.contains("record::exists(type::record('task', $slug))") && !create;
                        let mut chosen = chosen.lock().unwrap();
                        let applies = match phase {
                            "open" => sql.contains("INFO FOR DB"),
                            "lookup" => lookup,
                            "create" | "message" | "reply" => create,
                            "check" => create && *chosen == 0 || lookup && matches!(*chosen, 1 | 2),
                            _ => unreachable!(),
                        };
                        let delay = applies && (*chosen == 0 || phase == "check");
                        if delay {
                            *chosen += 1;
                            let sql = match phase {
                                "open" | "lookup" => format!("SLEEP 4s;\n{sql}"),
                                "create" | "check" if create => sql.replacen(
                                    "CREATE type::record('task', $slug)",
                                    "SLEEP 4s;\nCREATE type::record('task', $slug)",
                                    1,
                                ),
                                "check" => format!("SLEEP 4s;\n{sql}"),
                                "message" => sql.replacen(
                                    "CREATE type::record('message',",
                                    "SLEEP 4s;\nCREATE type::record('message',",
                                    1,
                                ),
                                "reply" => sql.to_string(),
                                _ => unreachable!(),
                            };
                            call["params"][0] = json!(sql);
                        }
                        drop(chosen);
                        let bytes = tasks::cbor::encode(&call);
                        let mut remote = TcpStream::connect(&upstream).unwrap();
                        remote.set_read_timeout(Some(LIMIT)).unwrap();
                        remote.set_write_timeout(Some(LIMIT)).unwrap();
                        let headers = headers
                            .lines()
                            .filter(|line| {
                                !line.to_ascii_lowercase().starts_with("content-length:")
                                    && !line.is_empty()
                            })
                            .collect::<Vec<_>>()
                            .join("\r\n");
                        write!(
                            remote,
                            "{headers}\r\nContent-Length: {}\r\n\r\n",
                            bytes.len()
                        )
                        .unwrap();
                        remote.write_all(&bytes).unwrap();
                        let (reply, bytes) = packet(&mut BufReader::new(remote)).unwrap().unwrap();
                        if delay && phase == "reply" {
                            std::thread::sleep(Duration::from_secs(4));
                        }
                        if delay {
                            send.send(()).unwrap();
                        }
                        if incoming
                            .get_mut()
                            .write_all(reply.as_bytes())
                            .and_then(|()| incoming.get_mut().write_all(&bytes))
                            .is_err()
                        {
                            break;
                        }
                        next = match packet(&mut incoming) {
                            Ok(next) => next,
                            Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {
                                break;
                            }
                            Err(error) => panic!("read the next private proxy request: {error}"),
                        };
                    }
                }));
            }
        });
        Self {
            address,
            worker: Some(worker),
            workers,
            delayed,
        }
    }
}

impl Drop for Delayed {
    fn drop(&mut self) {
        drop(TcpStream::connect(self.address).unwrap());
        self.worker.take().unwrap().join().unwrap();
        for worker in self.workers.lock().unwrap().drain(..) {
            worker.join().unwrap();
        }
    }
}

#[test]
fn reports_each_private_database_timeout_phase() {
    if let Ok(root) = std::env::var(CHILD) {
        let saves = symphony::save::Saves::start(std::path::Path::new(&root));
        let payload = json!({"request":"11111111111111111111111111111111", "title":"Owner delay probe", "body":"Keep these typed details", "page":"/docs/PROBE.md#probe"});
        let mut legacy = payload.clone();
        legacy.as_object_mut().unwrap().remove("request");
        let refused = saves
            .post("", "/api/quick-task", &legacy)
            .expect_err("a form without a save identity must keep its text");
        assert_eq!(refused.status, 400);
        assert!(refused.error.contains("Reload this page"));
        let mut too_long = payload.clone();
        too_long["title"] = json!("x".repeat(symphony_api::QUICK_TASK_TITLE_MAX + 1));
        let refused = saves
            .post("", "/api/quick-task", &too_long)
            .expect_err("an overlong title must refuse before saving");
        assert_eq!(refused.status, 400);
        assert!(refused.error.contains("characters for the task title"));
        let began = Instant::now();
        let mut result = saves.post("", "/api/quick-task", &payload);
        let first = match &result {
            Ok(value) => json!({"saved":value}),
            Err(error) => json!({"status":error.status,"error":error.error}),
        };
        let elapsed = began.elapsed().as_secs_f64();
        if matches!(
            std::env::var("SF_OWNER_SAVE_PHASE").as_deref(),
            Ok("create" | "message" | "reply" | "check")
        ) {
            assert!(
                first["saved"]["pending"] == true || first["saved"]["slug"].is_string(),
                "an uncertain write must not be reported as failed: {first}"
            );
        }
        let mut pending_checks = 0;
        while !result.as_ref().is_ok_and(|value| value["slug"].is_string()) {
            assert!(began.elapsed() < LIMIT, "save never resolved: {first}");
            if std::env::var("SF_OWNER_SAVE_PHASE").as_deref() == Ok("check") {
                assert!(
                    result.as_ref().is_ok_and(|value| value["pending"] == true),
                    "a failed confirmation lookup must keep the save pending"
                );
                pending_checks += 1;
            }
            if let Err(error) = &result {
                assert_eq!(error.status, 503, "{}", error.error);
            }
            result = saves.post("", "/api/quick-task", &payload);
        }
        if std::env::var("SF_OWNER_SAVE_PHASE").as_deref() == Ok("check") {
            assert_eq!(
                pending_checks, 2,
                "both failed confirmation reads stay pending"
            );
        }
        let slug = result
            .as_ref()
            .unwrap_or_else(|error| panic!("{}", error.error))["slug"]
            .clone();
        assert_eq!(slug, "inbox/owner-delay-probe");
        saves.close();
        std::fs::remove_file(std::path::Path::new(&root).join("docs/PROBE.md")).unwrap();
        let reopened = symphony::save::Saves::start(std::path::Path::new(&root));
        let repeated = reopened
            .post("", "/api/quick-task", &payload)
            .unwrap_or_else(|error| panic!("{}", error.error));
        assert_eq!(
            repeated["slug"], slug,
            "a new Saves must recover the same intent"
        );
        let mut invalid = payload.clone();
        invalid["request"] = json!("22222222222222222222222222222222");
        invalid["title"] = json!("Invalid priority");
        invalid["priority"] = json!(99);
        let refused = reopened
            .post("", "/api/quick-task", &invalid)
            .expect_err("invalid priority must refuse");
        assert_eq!(refused.status, 400);
        let mut collision = payload.clone();
        collision["request"] = json!("33333333333333333333333333333333");
        collision["slug"] = slug.clone();
        collision["title"] = json!("Unrelated owner task");
        let refused = reopened
            .post("", "/api/quick-task", &collision)
            .expect_err("an unrelated task must not confirm this save");
        assert_eq!(refused.status, 400);
        let refused = reopened
            .post("", "/api/quick-task", &collision)
            .expect_err("checking a known collision must retain its refusal");
        assert_eq!(refused.status, 400);
        let mut changed = payload;
        changed["title"] = json!("Different text under the same identity");
        let refused = reopened
            .post("", "/api/quick-task", &changed)
            .expect_err("an identity cannot be rebound");
        assert_eq!(refused.status, 400);
        reopened.close();
        println!(
            "OWNER_SAVE_PROBE {}",
            json!({"first":first,"elapsed_s":elapsed,"confirmed":slug})
        );
        return;
    }
    for phase in ["open", "lookup", "create", "message", "reply", "check"] {
        let tree = super::Tree::new();
        tree.write("docs/PROBE.md", "# Probe\n");
        let client = board::client();
        let store = tasks::Store::open(tasks::rpc::Client::new(
            client.addr(),
            "sf",
            client.database(),
        ))
        .unwrap();
        let delay = Delayed::start(client.addr().into(), phase);
        let done = tasks::sys::output(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", CASE, "--nocapture"])
                .env(CHILD, &tree.root)
                .env("SF_OWNER_SAVE_PHASE", phase)
                .env("TASKS_SERVER", delay.address.to_string())
                .env("TASKS_DATABASE", client.database())
                .env("TASKS_SWEEP", "off")
                .env_remove("TASKS_SESSION")
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_COMMON_DIR")
                .env_remove("GIT_INDEX_FILE")
                .current_dir(&tree.root),
        )
        .unwrap();
        let out = String::from_utf8(done.stdout).unwrap();
        assert!(
            done.status.success(),
            "{phase}: {out}\n{}",
            String::from_utf8_lossy(&done.stderr)
        );
        for _ in 0..if phase == "check" { 3 } else { 1 } {
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
        let tasks = store.tasks(None).unwrap();
        let messages: usize = tasks
            .iter()
            .map(|task| store.thread(&task.slug).unwrap().len())
            .sum();
        assert_eq!(tasks.len(), 1, "{phase}: {record}");
        assert_eq!(messages, 1, "{phase}: {record}");
        assert_eq!(tasks[0].title, "Owner delay probe");
        assert_eq!(
            tasks[0].body,
            "Keep these typed details\n\nSent from [/docs/PROBE.md#probe](/docs/PROBE.md#probe)"
        );
        assert_eq!(tasks[0].links, ["docs/PROBE.md#probe"]);
        println!(
            "phase={phase} response={record} final_tasks={} creation_messages={messages}",
            tasks.len()
        );
    }
}
