//! What the owner may be looking at in the Herdr app (`herdr-app status`): the board never closes a
//! terminal on screen, and keeps one when the window cannot say.
//!
//! A terminal is on screen when its `herdr.tab` label is the tab the window shows (`tab`), when it
//! is the agent the window shows (`agent`), or when an overlay or a float holds it (`overlays`,
//! `floats`, each naming its `terminal`). The window cannot say when `herdr-app status` fails or
//! reports an error, or when an overlay or float does not name the terminal it holds. The look that
//! decides is the last call before the kill ([`close_unseen`]), after the last `hc get`, which must
//! still find the terminal finished.

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
        let Ok(Value::Object(said)) = serde_json::from_slice::<Value>(said.trim().as_bytes())
        else {
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

/// Why a terminal the owner may be looking at stays.
pub const ON_SCREEN: &str = "on screen in the Herdr app";

/// Why a terminal stays when `problem` kept a call from saying whether it may close.
pub fn no_information(problem: &LaunchError) -> String {
    format!("never closed on no information: {problem}")
}

/// Why [`close_unseen`] did not close a terminal.
#[derive(Debug, Clone, PartialEq)]
pub enum NotClosed {
    /// Left as it is, saying why: it works again, it is on screen ([`ON_SCREEN`]), or
    /// [`no_information`].
    Kept(String),
    /// hc refused to kill or remove it.
    Refused(LaunchError),
}

/// Close `terminal` (killed while live, then removed) only as the last look before the kill finds
/// it: a fresh `hc get`, which may wait long, still showing it finished (`finished` checks that
/// answer, and the processes under it, and says why it stays when it works again), and then the
/// window not showing it. A terminal hc no longer knows is closed already; one it cannot answer
/// for stays.
pub fn close_unseen(
    hc: &dyn Hc,
    terminal: &str,
    finished: &dyn Fn(&Object) -> Result<(), String>,
) -> Result<(), NotClosed> {
    let found = match hc.get(terminal) {
        Ok(None) => return Ok(()),
        Ok(Some(found)) => found,
        Err(problem) => return Err(NotClosed::Kept(no_information(&problem))),
    };
    finished(&found).map_err(NotClosed::Kept)?;
    match on_screen(hc, &found) {
        Ok(false) => {}
        Ok(true) => return Err(NotClosed::Kept(ON_SCREEN.into())),
        Err(problem) => return Err(NotClosed::Kept(no_information(&problem))),
    }
    hc.end(terminal, text_of(&found, "state") == "live")
        .map_err(NotClosed::Refused)
}
