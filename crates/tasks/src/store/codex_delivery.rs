//! Installed Codex protocol (generated offline): inspect runtime state and steer only an active
//! turn, using its id as a precondition. Disabled unless [`STEER`] (`symphony.codexSteer`) is
//! explicitly true; idle/unavailable threads and the default configuration retain the CLI queue
//! path.

use super::DeliveryError;
use crate::channel::{Receiver, Sender, channel};
use crate::sys::{self, Child, Stdio};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::time::Instant;

struct Proxy {
    child: Child,
    input: Option<Sender<Vec<u8>>>,
    responses: Receiver<Result<Value, String>>,
    deadline: Instant,
}

impl Proxy {
    fn connect() -> Result<Self, DeliveryError> {
        // Pipes no child another thread spawns can hold, and a spawn that hands the proxy only them.
        let (stdin, mut input) = sys::pipe().map_err(DeliveryError::retry)?;
        let (output, stdout) = sys::pipe().map_err(DeliveryError::retry)?;
        let child = sys::command("codex")
            .args(["app-server", "proxy"])
            .env_remove("CLAUDE_CODE_MESSAGING_TOKEN")
            .stdin(stdin)
            .stdout(stdout)
            .stderr(Stdio::null())
            .spawn()
            .map_err(DeliveryError::retry)?;
        let (send, responses) = channel();
        let (writes, written) = channel::<Vec<u8>>();
        let failed = send.clone();
        // A proxy can leave a descendant holding either pipe. Detach the IO workers: the shared
        // response deadline bounds the caller, and teardown must not wait for inherited pipe EOF.
        let _writer = std::thread::spawn(move || {
            for bytes in written {
                if let Err(error) = input.write_all(&bytes) {
                    let _ = failed.send(Err(error.to_string()));
                    break;
                }
            }
        });
        let _reader = std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let value = line
                    .map_err(|e| e.to_string())
                    .and_then(|line| serde_json::from_str(&line).map_err(|e| e.to_string()));
                if send.send(value).is_err() {
                    break;
                }
            }
            let _ = send.send(Err("codex proxy closed before its response".into()));
        });
        // The steer's requests share the delivery's time, read where its `codex` is found.
        let patience = crate::codex_queue::patience(&crate::cli::Env::of_process());
        Ok(Self {
            child,
            input: Some(writes),
            responses,
            deadline: Instant::now() + patience,
        })
    }

    fn write(&mut self, value: &Value) -> Result<(), DeliveryError> {
        let mut bytes = serde_json::to_vec(value).map_err(DeliveryError::retry)?;
        bytes.push(b'\n');
        self.input
            .as_ref()
            .expect("proxy input is open")
            .send(bytes)
            .map_err(|_| DeliveryError::retry("sending on a closed channel"))
    }

    fn request(&mut self, id: u64, method: &str, params: Value) -> Result<Value, DeliveryError> {
        self.write(&json!({"id":id, "method":method, "params":params}))?;
        loop {
            let value = self
                .responses
                .recv_timeout(self.deadline.saturating_duration_since(Instant::now()))
                .map_err(|e| DeliveryError::retry(format!("codex {method}: {e}")))?
                .map_err(DeliveryError::retry)?;
            if value["id"] == id {
                return Ok(value);
            }
        }
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        // This is only our proxy process, never the shared daemon it connects to.
        let _ = self.child.kill();
        let _ = self.child.wait();
        drop(self.input.take());
    }
}

/// The git setting that turns active-turn steering on.
const STEER: &str = "symphony.codexSteer";

/// Whether steering is on: [`STEER`] is true, as `git config --bool` reads it. `key` asks git for
/// one key's value; none when it has none.
fn steering(key: impl FnOnce(&'static str) -> Option<bool>) -> bool {
    key(STEER).unwrap_or(false)
}

/// One git setting as `git config --bool --get` answers it in the repository of this process's
/// folder.
fn git_bool(key: &str) -> Option<bool> {
    sys::output(sys::git().args(["config", "--bool", "--get", key]))
        .ok()
        .filter(|output| output.status.success())
        .map(|output| output.stdout == b"true\n")
}

/// False means no input was attempted, so the caller can safely use `codex queue`.
/// Once steering is attempted, an ambiguous failure stays retryable in the durable outbox.
pub(super) fn steer(thread: &str, text: &str) -> Result<bool, DeliveryError> {
    if !steering(git_bool) {
        return Ok(false);
    }
    let Ok(mut proxy) = Proxy::connect() else {
        return Ok(false);
    };
    let inspect = |proxy: &mut Proxy| -> Result<Option<String>, DeliveryError> {
        let initialized = proxy.request(
            1,
            "initialize",
            json!({
                "clientInfo":{"name":"symphony-board", "version":env!("CARGO_PKG_VERSION")},
                "capabilities":{"experimentalApi":true}
            }),
        )?;
        if initialized.get("error").is_some() {
            return Ok(None);
        }
        proxy.write(&json!({"method":"initialized"}))?;
        let read = proxy.request(
            2,
            "thread/read",
            json!({"threadId":thread, "includeTurns":false}),
        )?;
        let state = &read["result"]["thread"];
        if state["status"]["type"] != "active" || state["canAcceptDirectInput"] == false {
            return Ok(None);
        }
        let turns = proxy.request(
            3,
            "thread/turns/list",
            json!({
                "threadId":thread, "limit":1, "sortDirection":"desc", "itemsView":"notLoaded"
            }),
        )?;
        Ok(turns["result"]["data"].as_array().and_then(|turns| {
            turns
                .iter()
                .find(|t| t["status"] == "inProgress")
                .and_then(|t| t["id"].as_str())
                .map(str::to_owned)
        }))
    };
    let Ok(Some(turn)) = inspect(&mut proxy) else {
        return Ok(false);
    };
    let response = proxy.request(
        4,
        "turn/steer",
        json!({
            "threadId":thread, "expectedTurnId":turn,
            "input":[{"type":"text", "text":text}]
        }),
    )?;
    if let Some(error) = response.get("error") {
        // A confirmed refusal accepted no input (including a turn that ended during inspection).
        // Queue it so an idle or non-steerable review/compact turn still receives the callback.
        if error.is_object() {
            return Ok(false);
        }
        return Err(DeliveryError::retry(format!("codex turn/steer: {error}")));
    }
    if response.get("result").is_none() {
        return Err(DeliveryError::retry("codex turn/steer returned no result"));
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::steering;

    /// A repository whose git has `value` under the setting.
    fn keys(value: Option<bool>) -> impl FnOnce(&'static str) -> Option<bool> {
        move |key| match key {
            "symphony.codexSteer" => value,
            other => panic!("asked git for {other}"),
        }
    }

    #[test]
    fn steering_is_the_setting_else_off() {
        assert!(!steering(keys(None)));
        assert!(steering(keys(Some(true))));
        assert!(!steering(keys(Some(false))));
    }
}
