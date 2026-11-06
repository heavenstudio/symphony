//! What the owner may be looking at in the Herdr app (`herdr-app status`): the board never closes a
//! terminal on screen, and keeps one when the window cannot say.
//!
//! A terminal is on screen when its `herdr.tab` label is the tab the window shows (`tab`), when it
//! is the agent the window shows (`agent`), or when an overlay or a float holds it (`overlays`,
//! `floats`, each naming its `terminal`). The window cannot say when `herdr-app status` fails or
//! reports an error, or when an overlay or float does not name the terminal it holds.

use super::LaunchError;
use super::hc::{self, Hc, Object, text, text_of};
use serde_json::Value;

/// What the window shows: its tab, its agent and the terminals held over them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Screen {
    pub tab: String,
    pub agent: String,
    pub held: Vec<String>,
}

impl Screen {
    /// What `herdr-app status` printed, read: an error naming what it does not say.
    pub fn of(said: &str) -> Result<Screen, LaunchError> {
        let fail = |why: &str| LaunchError(format!("herdr-app status {why}"));
        let Ok(Value::Object(said)) = serde_json::from_str::<Value>(said.trim()) else {
            return Err(fail("printed no JSON object"));
        };
        match said.get("errors") {
            Some(Value::Array(errors)) if errors.is_empty() => {}
            Some(Value::Array(errors)) => {
                let errors: Vec<String> = errors.iter().map(|e| text(Some(e))).collect();
                return Err(fail(&format!("reports errors: {}", errors.join("; "))));
            }
            _ => return Err(fail("gives no list of errors")),
        }
        let shown = |key: &str| match said.get(key) {
            Some(Value::String(shown)) => Ok(shown.clone()),
            Some(Value::Null) => Ok(String::new()),
            _ => Err(fail(&format!("names no {key}"))),
        };
        let (tab, agent) = (shown("tab")?, shown("agent")?);
        let mut held = Vec::new();
        for key in ["overlays", "floats"] {
            let over: Vec<&Value> = match said.get(key) {
                Some(Value::Array(over)) => over.iter().collect(),
                Some(Value::Object(over)) => over.values().collect(),
                _ => return Err(fail(&format!("gives no {key}"))),
            };
            for one in over {
                match one.get("terminal").and_then(Value::as_str) {
                    Some(terminal) if !terminal.is_empty() => held.push(terminal.to_string()),
                    _ => {
                        return Err(fail(&format!(
                            "does not say which terminal one of its {key} holds"
                        )));
                    }
                }
            }
        }
        Ok(Screen { tab, agent, held })
    }

    /// Whether `terminal` (as `hc get` shows it) is on screen.
    pub fn shows(&self, terminal: &Object) -> bool {
        let address = text_of(terminal, "address");
        (!self.tab.is_empty() && hc::label(terminal, "herdr.tab") == self.tab)
            || (!address.is_empty() && (self.agent == address || self.held.contains(&address)))
    }
}

/// Whether the owner may be looking at `terminal`, by what the window says now: an error when it
/// cannot say, and the terminal then stays.
pub fn on_screen(hc: &dyn Hc, terminal: &Object) -> Result<bool, LaunchError> {
    Ok(Screen::of(&hc.app(&["status"])?)?.shows(terminal))
}
