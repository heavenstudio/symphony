//! `tasks field SLUG NAME VALUE`: a plugin's board field on a task. The command line holds no
//! plugin, so the write goes through the Symphony server (`/api/task-field`), which asks the plugin
//! declaring the field to validate the value before the board's transaction.

use super::Env;
use super::args::Parsed;
use serde_json::{Value, json};
use std::io::Write;
use std::time::Duration;

/// The one Symphony server's folder in the git common dir, and its record there.
const FOLDER: &str = crate::folders::SERVER;
const RECORD: &str = "server.json";
/// The one server's port when nothing names another.
const DEFAULT_PORT: u16 = 8765;
/// The route that validates and writes a field.
const ROUTE: &str = "/api/task-field";
/// How long the server may take to answer one write.
const PATIENCE: Duration = Duration::from_secs(30);

/// Run the command: its line goes to `out`, its refusal to `err`; the exit code is returned.
pub fn run(parsed: &Parsed, env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match write(parsed, env) {
        Ok(line) => {
            let _ = writeln!(out, "{line}");
            0
        }
        Err((code, why)) => {
            let _ = writeln!(err, "✖ {why}");
            code
        }
    }
}

fn write(parsed: &Parsed, env: &Env) -> Result<String, (i32, String)> {
    let actor = parsed
        .get("actor")
        .filter(|actor| !actor.is_empty())
        .or_else(|| env.var("TASKS_SESSION"))
        .ok_or((
            2,
            "no session: pass --as HANDLE or set TASKS_SESSION".to_string(),
        ))?;
    super::check_actor(actor).map_err(|e| (1, e.to_string()))?;
    let port = port(env).map_err(|why| (2, why))?;
    let (slug, name) = (parsed.or("slug", ""), parsed.or("name", ""));
    let body = json!({"slug": slug, "name": name, "value": parsed.or("value", ""), "actor": actor});
    let addr = format!("127.0.0.1:{port}");
    let (status, answer) =
        crate::rpc::exchange(&addr, &request(port, &body), PATIENCE).map_err(|why| (2, why))?;
    answered(status, &answer, slug, name)
}

/// The port of the Symphony server holding the board `env` reaches: `SYMPHONY_PORT`, else the
/// one server's record in the git common dir, else its default. A scratch board (another
/// `TASKS_DATABASE`, or a `TASKS_SERVER`) is never written through the one server.
fn port(env: &Env) -> Result<u16, String> {
    if let Some(port) = env.read(symphony_env::PORT) {
        return port
            .parse()
            .map_err(|_| format!("SYMPHONY_PORT={port} is not a port"));
    }
    let scratch = env.var("TASKS_SERVER").is_some()
        || env
            .var("TASKS_DATABASE")
            .is_some_and(|database| database != crate::DATABASE);
    if scratch {
        return Err(
            "a scratch board's field is written through its own Symphony server: set SYMPHONY_PORT"
                .into(),
        );
    }
    let recorded = crate::git::common_dir(&env.cwd)
        .ok()
        .and_then(|common| std::fs::read_to_string(common.join(FOLDER).join(RECORD)).ok())
        .and_then(|text| serde_json::from_slice::<Value>(text.as_bytes()).ok())
        .and_then(|record| record["port"].as_u64())
        .and_then(|port| u16::try_from(port).ok());
    Ok(recorded.unwrap_or(DEFAULT_PORT))
}

/// The write as the server reads it: its Host names the server, as every write's must.
fn request(port: u16, body: &Value) -> Vec<u8> {
    let body = body.to_string();
    format!(
        "POST {ROUTE} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

/// The line a write the server made prints, or its refusal: a 4xx is the server's or the
/// plugin's refusal (exit 1), anything else a server that could not write (exit 2).
fn answered(status: u16, answer: &[u8], slug: &str, name: &str) -> Result<String, (i32, String)> {
    let said: Value = serde_json::from_slice(answer).unwrap_or(Value::Null);
    if status == 200 && said["ok"] == true {
        let field = name.rsplit_once('.').map_or(name, |(_, field)| field);
        let value = match &said["value"] {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        let plugin = said["plugin"].as_str().unwrap_or("");
        return Ok(format!("{slug}: {plugin}.{field} is {value}"));
    }
    let why = said["error"].as_str().map_or_else(
        || String::from_utf8_lossy(answer).trim().to_string(),
        str::to_string,
    );
    let code = if (400..500).contains(&status) { 1 } else { 2 };
    Err((
        code,
        format!("the Symphony server answered {status}: {why}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::children::Child as _;
    use crate::sys::Spawning as _;
    use std::collections::BTreeMap;
    use std::io::{BufRead, BufReader, Read};
    use std::net::TcpListener;
    use std::path::PathBuf;

    fn env(vars: &[(&str, &str)], cwd: PathBuf) -> Env {
        Env {
            vars: vars
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<BTreeMap<_, _>>(),
            cwd,
        }
    }

    fn parsed(args: &[&str]) -> Parsed {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        match super::super::args::parse(&args) {
            super::super::args::Outcome::Run(parsed) => parsed,
            _ => panic!("{args:?} does not parse"),
        }
    }

    /// A server answering one request with `status` and `body`, handing back what it was sent.
    fn serve_once(status: u16, body: &'static str) -> (u16, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut head = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(n) = line.to_lowercase().strip_prefix("content-length: ") {
                    length = n.trim().parse().unwrap();
                }
                head.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            let mut sent = vec![0; length];
            reader.read_exact(&mut sent).unwrap();
            let reply = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            std::io::Write::write_all(reader.get_mut(), reply.as_bytes()).unwrap();
            head + &String::from_utf8(sent).unwrap()
        });
        (port, handle)
    }

    fn run_on(port: u16, args: &[&str], session: &str) -> (i32, String, String) {
        let port = port.to_string();
        let env = env(
            &[("SYMPHONY_PORT", &port), ("TASKS_SESSION", session)],
            std::env::temp_dir(),
        );
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&parsed(args), &env, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn a_write_the_server_makes_prints_the_stored_value() {
        let (port, sent) = serve_once(200, r#"{"ok":true,"plugin":"game","value":3.0}"#);
        let (code, out, err) = run_on(port, &["field", "sim/jump", "game.effort", "3"], "ash-1");
        assert_eq!(
            (code, out.as_str(), err.as_str()),
            (0, "sim/jump: game.effort is 3.0\n", "")
        );
        let sent = sent.join().unwrap();
        assert!(
            sent.starts_with("POST /api/task-field HTTP/1.1\r\n"),
            "{sent}"
        );
        assert!(
            sent.contains(&format!("Host: 127.0.0.1:{port}\r\n")),
            "{sent}"
        );
        let body: Value = serde_json::from_str(sent.rsplit("\r\n\r\n").next().unwrap()).unwrap();
        assert_eq!(
            body,
            json!({"slug": "sim/jump", "name": "game.effort", "value": "3", "actor": "ash-1"})
        );
    }

    #[test]
    fn a_text_value_prints_bare() {
        let (port, _sent) = serve_once(200, r#"{"ok":true,"plugin":"game","value":"high"}"#);
        let (code, out, _) = run_on(port, &["field", "a", "risk", "high"], "ash-1");
        assert_eq!((code, out.as_str()), (0, "a: game.risk is high\n"));
    }

    #[test]
    fn a_plugin_refusal_exits_one_with_the_servers_reason() {
        let (port, _sent) = serve_once(400, r#"{"error":"\"x\" is not a number"}"#);
        let (code, out, err) = run_on(port, &["field", "a", "effort", "x"], "ash-1");
        assert_eq!(
            (code, out.as_str(), err.as_str()),
            (
                1,
                "",
                "✖ the Symphony server answered 400: \"x\" is not a number\n"
            )
        );
    }

    #[test]
    fn a_server_that_cannot_write_exits_two_with_its_body() {
        let (port, _sent) = serve_once(503, "down for now");
        let (code, _, err) = run_on(port, &["field", "a", "effort", "1"], "ash-1");
        assert_eq!(
            (code, err.as_str()),
            (2, "✖ the Symphony server answered 503: down for now\n")
        );
    }

    #[test]
    fn no_server_listening_exits_two() {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let (code, _, err) = run_on(port, &["field", "a", "effort", "1"], "ash-1");
        assert_eq!(code, 2);
        assert!(
            err.contains("cannot reach the server at 127.0.0.1:"),
            "{err}"
        );
    }

    #[test]
    fn the_owner_and_no_session_are_refused_before_any_write() {
        let (code, _, err) = run_on(1, &["field", "a", "effort", "1"], "owner");
        assert_eq!(code, 1);
        assert!(err.contains("the owner acts from the Symphony"), "{err}");
        let env = env(&[], std::env::temp_dir());
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&parsed(&["field", "a", "b", "c"]), &env, &mut out, &mut err);
        assert_eq!(code, 2);
        assert!(String::from_utf8(err).unwrap().starts_with("✖ no session"));
    }

    #[test]
    fn the_port_is_the_environments_else_the_record_else_the_default() {
        let dir = std::env::temp_dir().join(format!("symphony-field-port-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let repo = dir.as_path();
        std::process::Command::of("git")
            .args(["init", "-q"])
            .current_dir(repo)
            .status_locked()
            .unwrap();
        assert_eq!(port(&env(&[], repo.into())), Ok(DEFAULT_PORT));
        std::fs::create_dir_all(repo.join(".git").join(FOLDER)).unwrap();
        let record = repo.join(".git").join(FOLDER).join(RECORD);
        std::fs::write(&record, "{not a record").unwrap();
        assert_eq!(port(&env(&[], repo.into())), Ok(DEFAULT_PORT));
        std::fs::write(&record, r#"{"port": 70000, "pid": 1}"#).unwrap();
        assert_eq!(port(&env(&[], repo.into())), Ok(DEFAULT_PORT));
        std::fs::write(&record, r#"{"port": 9123, "pid": 1, "root": "/r"}"#).unwrap();
        assert_eq!(port(&env(&[], repo.into())), Ok(9123));
        assert_eq!(
            port(&env(&[("SYMPHONY_PORT", "9200")], repo.into())),
            Ok(9200)
        );
        assert_eq!(
            port(&env(&[("SYMPHONY_PORT", "x")], repo.into())),
            Err("SYMPHONY_PORT=x is not a port".into())
        );
        assert_eq!(
            port(&env(&[], std::env::temp_dir().join("no-repo-here"))),
            Ok(DEFAULT_PORT)
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_scratch_board_needs_its_own_server() {
        let cwd = std::env::temp_dir();
        for scratch in [
            [("TASKS_DATABASE", "scratch")],
            [("TASKS_SERVER", "127.0.0.1:1")],
        ] {
            let refused = port(&env(&scratch, cwd.clone())).unwrap_err();
            assert!(refused.contains("set SYMPHONY_PORT"), "{refused}");
        }
        assert_eq!(
            port(&env(
                &[("TASKS_DATABASE", crate::DATABASE)],
                cwd.join("none")
            )),
            Ok(DEFAULT_PORT)
        );
        assert_eq!(
            port(&env(
                &[("TASKS_DATABASE", "scratch"), ("SYMPHONY_PORT", "9300")],
                cwd
            )),
            Ok(9300)
        );
    }
}
