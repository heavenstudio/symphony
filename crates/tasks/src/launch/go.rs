//! Going to an agent's session in its pane (docs/workflow/TASKS.md, "Going to a session"). On the machine
//! the panes run on, the Herdr app shows the pane and is brought forward: `herdr-app do agent.focus
//! --terminal T` names the terminal alone, and the app finds the space, tab and pane that hold it
//! (`pane.focus --terminal T`, the same for a terminal that runs no agent); the app has no URL
//! scheme a page could link to instead. From anywhere else the pane's visible screen is read, as
//! `hc read T --lines 0` gives it. Neither ever types into the pane: no `send`, no `keys`, no paste.

use super::LaunchError;
use super::hc::{Binary, Hc, is_not_found, text};
use crate::cli::Env;
use serde_json::Value;
use std::time::Duration;

/// How long one focus or one screen read may take, all its calls together (a focus is up to
/// three: `agent.focus`, `pane.focus` and `open -a`): under the 15 s a page waits for its answer
/// (`ANSWER_MS`, `apps/symphony-web/src/request.ts`), so the call has ended, its child killed, before the page
/// gives up and lets the button be pressed again.
pub const DEADLINE: Duration = Duration::from_secs(10);

/// hc and the Herdr app in `env` as a Go to session asks them: every call of one focus or one
/// screen read answered within [`DEADLINE`] from now, all of them together.
pub fn asked(env: &Env) -> Binary {
    Binary::of(env).within(DEADLINE)
}

/// What a focus did: the terminal the app shows, and whether the app was brought forward, else
/// why it could not be.
#[derive(Debug, Clone, PartialEq)]
pub struct Focused {
    pub terminal: String,
    pub raised: Result<bool, String>,
}

impl Focused {
    /// What the page says it did.
    pub fn said(&self) -> String {
        let terminal = &self.terminal;
        match &self.raised {
            Ok(true) => format!("The Herdr app shows {terminal}, in front."),
            Ok(false) => format!(
                "The Herdr app shows {terminal}; bring its window forward if another covers it."
            ),
            Err(why) => format!(
                "The Herdr app shows {terminal}, but its window could not be brought forward: {why}"
            ),
        }
    }
}

/// Show `terminal` in the Herdr app and bring the app forward. Refused when the address is not
/// one hc gives, or the app shows it neither as an agent nor as a pane (the window is closed, or
/// it knows no such terminal); a window that cannot be brought forward is said, not refused.
pub fn focus(hc: &dyn Hc, terminal: &str) -> Result<Focused, LaunchError> {
    let terminal = addressed(terminal)?;
    shown(hc, "agent.focus", terminal).or_else(|agent| {
        shown(hc, "pane.focus", terminal).map_err(|pane| {
            LaunchError(format!(
                "the Herdr app did not show {terminal}: {agent}; {pane}"
            ))
        })
    })?;
    Ok(Focused {
        terminal: terminal.to_string(),
        raised: hc.raise().map_err(|error| error.0),
    })
}

/// What `terminal` shows now: its visible screen, no history. Refused when the address is not
/// one hc gives, or hc knows no such terminal.
pub fn screen(hc: &dyn Hc, terminal: &str) -> Result<String, LaunchError> {
    let terminal = addressed(terminal)?;
    hc.read(terminal, 0).map_err(|error| {
        if is_not_found(&error) {
            LaunchError(format!("hc knows no terminal {terminal}: its pane is gone"))
        } else {
            error
        }
    })
}

/// `herdr-app do COMMAND --terminal T`, refused when the window answers an error, on its stdout
/// whatever its exit code.
fn shown(hc: &dyn Hc, command: &str, terminal: &str) -> Result<(), LaunchError> {
    let out = hc.app(&["do", command, "--terminal", terminal])?;
    match serde_json::from_str::<Value>(out.trim()) {
        Ok(Value::Object(said)) if said.get("error").is_some() => Err(LaunchError(format!(
            "herdr-app do {command}: {}",
            text(said.get("error"))
        ))),
        _ => Ok(()),
    }
}

/// `terminal` when it is an address as hc gives one (`t7`): letters, digits, `_`, `-`, `.` and
/// `:`, not starting with `-`, so no flag or other word ever reaches hc in its place.
fn addressed(terminal: &str) -> Result<&str, LaunchError> {
    let fits = !terminal.is_empty()
        && terminal.len() <= 64
        && !terminal.starts_with('-')
        && terminal
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'));
    if fits {
        Ok(terminal)
    } else {
        Err(LaunchError(format!(
            "{terminal:?} is no hc terminal address"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// An hc that answers the window's `do` calls from a script and keeps every call, so a test
    /// sees that nothing was typed into a pane.
    #[derive(Default)]
    struct Window {
        calls: RefCell<Vec<Vec<String>>>,
        agent: Option<&'static str>,
        pane: Option<&'static str>,
        failed: bool,
        raised: Option<Result<bool, LaunchError>>,
        screen: Option<Result<String, LaunchError>>,
    }

    impl Hc for Window {
        fn run(&self, args: &[&str]) -> Result<String, LaunchError> {
            self.calls
                .borrow_mut()
                .push(args.iter().map(ToString::to_string).collect());
            self.screen.clone().unwrap_or_else(|| Ok(String::new()))
        }
        fn feed(&self, _: &[&str], _: &str) -> Result<String, LaunchError> {
            unreachable!("going to a session feeds hc nothing")
        }
        fn app(&self, args: &[&str]) -> Result<String, LaunchError> {
            let mut call = vec!["app".to_string()];
            call.extend(args.iter().map(ToString::to_string));
            self.calls.borrow_mut().push(call);
            if self.failed {
                return Err(LaunchError(format!(
                    "herdr-app do {}: the window did not answer",
                    args[1]
                )));
            }
            let said = if args[1] == "agent.focus" {
                self.agent
            } else {
                self.pane
            };
            Ok(said.map_or_else(
                || r#"{"ok":true}"#.to_string(),
                |why| format!(r#"{{"error":"{why}"}}"#),
            ))
        }
        fn raise(&self) -> Result<bool, LaunchError> {
            self.calls.borrow_mut().push(vec!["raise".into()]);
            self.raised.clone().unwrap_or(Ok(true))
        }
        fn caller(&self) -> Option<String> {
            None
        }
        fn board(&self) -> String {
            String::new()
        }
    }

    fn calls(hc: &Window) -> Vec<String> {
        hc.calls.borrow().iter().map(|c| c.join(" ")).collect()
    }

    #[test]
    fn an_agent_is_shown_by_its_terminal_alone_and_the_app_brought_forward() {
        let hc = Window::default();
        let focused = focus(&hc, "t7").unwrap();
        assert_eq!(
            calls(&hc),
            ["app do agent.focus --terminal t7", "raise"],
            "no space or tab is named: the app finds them"
        );
        assert_eq!(focused.said(), "The Herdr app shows t7, in front.");
    }

    #[test]
    fn a_terminal_that_runs_no_agent_is_shown_as_a_pane() {
        let hc = Window {
            agent: Some("terminal \\\"t7\\\" runs no agent"),
            ..Window::default()
        };
        focus(&hc, "t7").unwrap();
        assert_eq!(
            calls(&hc),
            [
                "app do agent.focus --terminal t7",
                "app do pane.focus --terminal t7",
                "raise"
            ]
        );
    }

    #[test]
    fn a_pane_the_app_cannot_show_is_refused_naming_both_answers_and_nothing_is_raised() {
        let hc = Window {
            agent: Some("no terminal t9"),
            pane: Some("no pane shows t9"),
            ..Window::default()
        };
        let refused = focus(&hc, "t9").unwrap_err();
        assert_eq!(
            refused.0,
            "the Herdr app did not show t9: herdr-app do agent.focus: no terminal t9; \
             herdr-app do pane.focus: no pane shows t9"
        );
        assert!(!calls(&hc).contains(&"raise".to_string()));
        let closed = Window {
            failed: true,
            ..Window::default()
        };
        let refused = focus(&closed, "t9").unwrap_err();
        assert!(refused.0.contains("the window did not answer"), "{refused}");
    }

    #[test]
    fn a_window_that_cannot_come_forward_is_said_not_refused() {
        let stuck = Window {
            raised: Some(Err(LaunchError("open -a: no such app".into()))),
            ..Window::default()
        };
        assert_eq!(
            focus(&stuck, "t7").unwrap().said(),
            "The Herdr app shows t7, but its window could not be brought forward: open -a: no such app"
        );
        let unbundled = Window {
            raised: Some(Ok(false)),
            ..Window::default()
        };
        assert!(
            focus(&unbundled, "t7")
                .unwrap()
                .said()
                .contains("bring its window forward")
        );
    }

    #[test]
    fn the_screen_is_the_visible_one_read_and_never_typed_into() {
        let hc = Window {
            screen: Some(Ok("❯ working on it\n".into())),
            ..Window::default()
        };
        assert_eq!(screen(&hc, "t7").unwrap(), "❯ working on it\n");
        assert_eq!(calls(&hc), ["read t7 --lines 0"]);
        let gone = Window {
            screen: Some(Err(LaunchError(
                "hc read t7: not_found: no terminal \"t7\"".into(),
            ))),
            ..Window::default()
        };
        assert_eq!(
            screen(&gone, "t7").unwrap_err().0,
            "hc knows no terminal t7: its pane is gone"
        );
        let down = Window {
            screen: Some(Err(LaunchError("hc read t7: socket gone".into()))),
            ..Window::default()
        };
        assert_eq!(
            screen(&down, "t7").unwrap_err().0,
            "hc read t7: socket gone"
        );
    }

    #[test]
    fn only_an_address_hc_gives_reaches_it() {
        for kept in ["t7", "t123", "term_1", "a-b.c:2"] {
            assert_eq!(addressed(kept), Ok(kept));
        }
        let long = "t".repeat(65);
        for refused in [
            "",
            "-t7",
            "--json",
            "t 7",
            "t7;rm",
            "t/7",
            "t\n7",
            long.as_str(),
        ] {
            assert!(addressed(refused).is_err(), "{refused:?}");
            let hc = Window::default();
            assert!(focus(&hc, refused).is_err());
            assert!(screen(&hc, refused).is_err());
            assert!(calls(&hc).is_empty(), "{refused:?} reached hc");
        }
    }
}
