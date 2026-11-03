//! herdr, over its JSON socket CLI: the binary `HERDR_BIN` names (default `herdr`), split as a POSIX
//! shell splits it, with the caller's environment and working directory. Every call the launcher and
//! the idle sweep make goes through [`Herdr`]; the tests point `HERDR_BIN` at the crate's `fake-herdr`.

use super::LaunchError;
use crate::cli::{Env, probes};
use crate::sys::Command;
use serde_json::{Map, Value};
use std::time::{Duration, Instant};

/// A JSON object herdr answered.
pub type Object = Map<String, Value>;

/// How long herdr may take to answer one call.
const PATIENCE: Duration = Duration::from_secs(120);

/// herdr's calls. [`Herdr::run`] is the one that reaches it; the rest read what it answers.
pub trait Herdr {
    /// What `herdr ARGS` printed: `LaunchError` when it could not be run, or failed.
    fn run(&self, args: &[&str]) -> Result<String, LaunchError>;

    /// The workspace the caller runs in, if it runs in one: another workspace's panes are none of
    /// its business.
    fn workspace(&self) -> Option<String>;

    /// The `result` of `herdr ARGS`'s JSON answer (an empty object when it has none).
    fn call(&self, args: &[&str]) -> Result<Object, LaunchError> {
        let out = self.run(args)?;
        let text = if out.is_empty() { "{}" } else { &out };
        let answer: Value = serde_json::from_str(text)
            .map_err(|_| LaunchError(format!("herdr {} printed no JSON", words(args))))?;
        Ok(match answer.get("result") {
            Some(Value::Object(result)) => result.clone(),
            _ => Object::new(),
        })
    }

    fn tabs(&self) -> Result<Vec<Object>, LaunchError> {
        let answer = self.call(&in_workspace(&["tab", "list"], self.workspace().as_deref()))?;
        Ok(objects(&answer, "tabs"))
    }

    fn panes(&self) -> Result<Vec<Object>, LaunchError> {
        let answer = self.call(&in_workspace(
            &["pane", "list"],
            self.workspace().as_deref(),
        ))?;
        Ok(objects(&answer, "panes"))
    }

    fn process_info(&self, pane: &str) -> Result<Object, LaunchError> {
        let answer = self.call(&["pane", "process-info", "--pane", pane])?;
        Ok(match answer.get("process_info") {
            Some(Value::Object(info)) => info.clone(),
            _ => Object::new(),
        })
    }

    fn close(&self, pane: &str) -> Result<(), LaunchError> {
        self.call(&["pane", "close", pane]).map(|_| ())
    }

    /// The panes running an agent, with the name herdr knows it by (`agent list`; `pane list` has no
    /// names), in the calling workspace only: `agent list` takes no `--workspace`, so they are
    /// filtered here, an agent naming no workspace kept.
    fn agents(&self) -> Result<Vec<Object>, LaunchError> {
        let workspace = self.workspace();
        Ok(self
            .all_agents()?
            .into_iter()
            .filter(|agent| match (&workspace, agent.get("workspace_id")) {
                (None, _) | (_, None | Some(Value::Null)) => true,
                (Some(ours), Some(theirs)) => theirs.as_str() == Some(ours.as_str()),
            })
            .collect())
    }

    /// Channel presence needs a machine-wide inventory; a scoped cleanup must not declare a
    /// thread in another workspace dead merely because its normal agent view omits that pane.
    fn all_agents(&self) -> Result<Vec<Object>, LaunchError> {
        let answer = self.call(&["agent", "list"])?;
        let Some(Value::Array(agents)) = answer.get("agents") else {
            return Err(LaunchError(
                "herdr agent list returned no agents array; presence is unknown".into(),
            ));
        };
        agents
            .iter()
            .map(|agent| {
                agent.as_object().cloned().ok_or_else(|| {
                    LaunchError(
                        "herdr agent list returned a non-object agent; presence is unknown".into(),
                    )
                })
            })
            .collect()
    }

    /// The last `lines` of what the pane shows, as plain text.
    fn read(&self, pane: &str, lines: usize) -> Result<String, LaunchError> {
        self.run(&["pane", "read", pane, "--lines", &lines.to_string()])
    }

    /// Press enter in the pane, as `herdr pane send-keys PANE enter` does.
    fn enter(&self, pane: &str) -> Result<(), LaunchError> {
        self.call(&["pane", "send-keys", pane, "enter"]).map(|_| ())
    }
}

/// The first two words of a call, which its errors name it by.
fn words(args: &[&str]) -> String {
    args.iter().take(2).copied().collect::<Vec<_>>().join(" ")
}

fn in_workspace<'a>(args: &[&'a str], workspace: Option<&'a str>) -> Vec<&'a str> {
    let mut all = args.to_vec();
    if let Some(workspace) = workspace {
        all.extend(["--workspace", workspace]);
    }
    all
}

/// The objects of the list `answer` holds under `key`; none when it holds none.
fn objects(answer: &Object, key: &str) -> Vec<Object> {
    match answer.get(key) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_object().cloned())
            .collect(),
        _ => Vec::new(),
    }
}

/// The herdr binary `env` names, run with `env`'s variables in its working directory.
pub struct Binary {
    words: Result<Vec<String>, String>,
    env: Env,
}

impl Binary {
    pub fn of(env: &Env) -> Binary {
        Binary {
            words: probes::split(env.var("HERDR_BIN").unwrap_or("herdr")),
            env: env.clone(),
        }
    }

    fn command(&self, args: &[&str]) -> Result<Command, LaunchError> {
        let named = self
            .words
            .as_ref()
            .map_err(|why| LaunchError(format!("HERDR_BIN: {why}")))?;
        let (program, rest) = named
            .split_first()
            .ok_or_else(|| LaunchError("HERDR_BIN names no command".into()))?;
        let mut command = Command::new(program);
        command
            .args(rest)
            .args(args)
            .env_clear()
            .envs(&self.env.vars)
            .current_dir(&self.env.cwd);
        Ok(command)
    }

    fn bounded(&self, args: &[&str], limit: Duration) -> Result<String, LaunchError> {
        let done = probes::within(&mut self.command(args)?, limit)
            .map_err(|why| LaunchError(format!("herdr {}: {why}", words(args))))?;
        answer(args, done)
    }
}

impl Herdr for Binary {
    fn run(&self, args: &[&str]) -> Result<String, LaunchError> {
        let Some(pane) = super::startup::codex_pane(args) else {
            return self.bounded(args, PATIENCE);
        };
        let mut answered = false;
        let mut moved = false;
        let done = super::startup::within(&mut self.command(args)?, PATIENCE, |remaining| {
            if answered {
                return Ok(());
            }
            let observed = Instant::now();
            let query_limit = remaining.min(Duration::from_secs(2));
            let Ok(screen) = self.bounded(
                &[
                    "pane",
                    "read",
                    pane,
                    "--source",
                    "detection",
                    "--lines",
                    "80",
                ],
                query_limit,
            ) else {
                // An unreadable screen authorizes no input; the original start still decides.
                return Ok(());
            };
            let Some(mut keys) = super::startup::hooks_choice(&screen) else {
                return Ok(());
            };
            if moved {
                // A fresh read must prove the navigation actually selected no-trust. A dropped
                // down can leave Trust all highlighted: never confirm or try correcting that.
                answered = true;
                if keys != ["enter"] {
                    return Ok(());
                }
            } else if keys.len() > 1 {
                keys.pop(); // Navigation only; confirmation needs the next observation.
                moved = true;
            } else {
                answered = true; // This fresh initial read already shows no-trust selected.
            }
            let mut call = vec!["pane", "send-keys", pane];
            call.extend(keys);
            let key_limit = remaining
                .saturating_sub(observed.elapsed())
                .min(Duration::from_secs(2));
            if key_limit.is_zero() {
                return Ok(());
            }
            self.bounded(&call, key_limit).map_err(|e| e.0)?;
            Ok(())
        })
        .map_err(|why| LaunchError(format!("herdr {}: {why}", words(args))))?;
        answer(args, done)
    }

    fn workspace(&self) -> Option<String> {
        self.env.var("HERDR_WORKSPACE_ID").map(str::to_string)
    }
}

fn answer(args: &[&str], done: std::process::Output) -> Result<String, LaunchError> {
    if !done.status.success() {
        let said = if done.stderr.is_empty() {
            &done.stdout
        } else {
            &done.stderr
        };
        return Err(LaunchError(format!(
            "herdr {} failed: {}",
            words(args),
            String::from_utf8_lossy(said).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&done.stdout).into_owned())
}

/// `value` as Python's `str` gives it for a JSON string or number: what herdr ids are read as.
pub fn text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

/// Whether `value` is true as Python reads a JSON value: not missing, null, false, zero or empty.
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(fields)) => !fields.is_empty(),
    }
}
