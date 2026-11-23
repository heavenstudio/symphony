//! A launched agent's start: started with no message, it is given its first one once hc calls it
//! ready, and the launch is confirmed only by hc's receipt of that message. On the way, only the
//! observed Codex hooks chooser is answered.
//!
//! hc records a first message only for a template run (`first_message`); the board runs the agent's
//! own command line, so the receipt is `hc send`'s: never forced (hc refuses a program blocked on a
//! person), its `started` saying whether the agent went to work on the text.

use super::hc::{self, Hc, Object, text_of};
use super::{CONFIRM_LOOKS, LaunchError, PROMPT_WAIT, READY_STATES};
use serde_json::Value;

/// A resumed turn can outlast startup confirmation; it has not received the new message yet.
#[derive(PartialEq, Eq)]
pub(super) enum Message {
    Started,
    Working,
}

/// Give the agent in `terminal` (`claude` or `codex`) its first message, `text`, once it is
/// [`ready`], and take hc's word that it started working on it: `hc send` answering `started`. A
/// send hc refuses (a program blocked on a person), or whose turn did not start, or that hc cannot
/// tell of (a program that reports no status), fails naming the terminal. `look` runs before each
/// look at the terminal. Only a resumed Codex turn still working at the deadline returns
/// [`Message::Working`], unsent, so restore can keep it and queue its next message natively.
pub(super) fn first_message(
    hc: &dyn Hc,
    terminal: &str,
    agent: &str,
    text: &str,
    resumed_codex: bool,
    look: &mut dyn FnMut(),
    sleep: &dyn Fn(f64),
) -> Result<Message, LaunchError> {
    if !ready(hc, terminal, agent, resumed_codex, look, sleep)? {
        return Ok(Message::Working);
    }
    let said = hc.json(&["send", terminal, text]).map_err(|problem| {
        LaunchError(format!(
            "terminal {terminal} did not take its first message: {problem}"
        ))
    })?;
    match said.get("started") {
        Some(Value::Bool(true)) => Ok(Message::Started),
        Some(Value::Bool(false)) => {
            let status = hc::text(said.get("status"));
            Err(LaunchError(format!(
                "the agent in terminal {terminal} did not start working on its first message (its \
                 status when it went in: {})",
                if status.is_empty() { "none" } else { &status }
            )))
        }
        _ => Err(LaunchError(format!(
            "hc cannot say whether the agent in terminal {terminal} took its first message: it \
             reports no status"
        ))),
    }
}

/// Wait until hc calls the agent in `terminal` ready for a message (`idle` or `done`). A look hc
/// cannot answer is no evidence; a terminal that ended, or that hc no longer knows, fails at once,
/// as does an agent blocked on a person (a folder-trust dialog; where hc's word for it is not on
/// the screen, the refusal also carries the screen's last lines), and one still not ready after
/// [`CONFIRM_LOOKS`]. A resumed Codex still live and working then is reported separately. A Codex
/// start that shows its hooks chooser is answered "Continue without
/// trusting", the one answer that keeps the hooks off: navigation first, then enter only once a
/// fresh look shows that item selected. The look right after a key press is not yet judged blocked.
fn ready(
    hc: &dyn Hc,
    terminal: &str,
    agent: &str,
    resumed_codex: bool,
    look: &mut dyn FnMut(),
    sleep: &dyn Fn(f64),
) -> Result<bool, LaunchError> {
    let mut last = String::from("unknown");
    let (mut moved, mut answered, mut pressed) = (false, false, false);
    for _ in 0..CONFIRM_LOOKS {
        sleep(PROMPT_WAIT);
        look();
        let found = match hc.get(terminal) {
            Ok(Some(found)) => found,
            Ok(None) => {
                return Err(LaunchError(format!(
                    "terminal {terminal} is gone before its agent took its first message"
                )));
            }
            Err(problem) => {
                last = format!("unanswered: {problem}");
                continue;
            }
        };
        if text_of(&found, "state") != "live" {
            let code = Some(text_of(&found, "exit_code")).filter(|c| !c.is_empty());
            return Err(LaunchError(format!(
                "the agent in terminal {terminal} ended (exit code {}) before it took its first message",
                code.as_deref().unwrap_or("none")
            )));
        }
        let status = hc::status(&found);
        if READY_STATES.contains(&status.as_str()) {
            return Ok(true);
        }
        last = if status.is_empty() {
            "none".into()
        } else {
            status.clone()
        };
        let settling = std::mem::take(&mut pressed);
        if agent == "codex"
            && !answered
            && let Ok(screen) = hc.read(terminal, 0)
            && let Some(mut keys) = bottom_choice(&screen)
        {
            if moved {
                // A fresh look must show the no-trust item selected. A dropped key can leave
                // another one selected: never confirm that, nor try correcting it.
                answered = true;
                if keys == ["enter"] {
                    hc.keys(terminal, &keys)?;
                    pressed = true;
                }
            } else {
                if keys.len() > 1 {
                    keys.pop(); // navigation only; confirming needs the next look
                    moved = true;
                } else {
                    answered = true; // this first look already shows no-trust selected
                }
                hc.keys(terminal, &keys)?;
                pressed = true;
            }
            continue;
        }
        if status == "blocked" && !settling {
            let asked = asks(&found);
            let shown = hc.read(terminal, 0).unwrap_or_default();
            return Err(LaunchError(format!(
                "the agent in terminal {terminal} waits on a person before it took its first \
                 message{asked}{}",
                shows(&shown, &asked)
            )));
        }
    }
    if resumed_codex && last == "working" {
        return Ok(false);
    }
    Err(LaunchError(format!(
        "the agent in terminal {terminal} was never ready for its first message (its last status: \
         {last})"
    )))
}

/// What a blocked program asks, as hc's status record says (`msg`), if it says.
fn asks(terminal: &Object) -> String {
    match terminal.get("status") {
        Some(Value::Object(status)) if !text_of(status, "msg").is_empty() => {
            format!(": {}", text_of(status, "msg"))
        }
        _ => String::new(),
    }
}

/// How many of a blocked start's last screen lines its refusal carries, and how much of each.
const SHOWN_LINES: usize = 8;
const SHOWN_WIDTH: usize = 160;

/// What a blocked program's `screen` ends with, for a refusal whose `asked` (hc's word, as
/// [`asks`] gives it) is not what the screen says: hc names some dialogs only as open, and the
/// terminal is closed with the failed start, so the refusal is the one place left that says what
/// was asked. Nothing where the screen is empty or already holds hc's word.
fn shows(screen: &str, asked: &str) -> String {
    let asked = asked.trim_start_matches(": ");
    if !asked.is_empty() && screen.contains(asked) {
        return String::new();
    }
    let lines: Vec<String> = screen
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.chars().take(SHOWN_WIDTH).collect())
        .collect();
    if lines.is_empty() {
        return String::new();
    }
    let from = lines.len().saturating_sub(SHOWN_LINES);
    format!("; its terminal showed: {}", lines[from..].join(" | "))
}

/// The keys that answer the hooks chooser the bottom of `screen` shows, if it shows one: the
/// chooser from its title to the end of the screen, nothing after it ([`hooks_choice`]).
pub(super) fn bottom_choice(screen: &str) -> Option<Vec<&'static str>> {
    let lines: Vec<&str> = screen.lines().collect();
    let title = lines
        .iter()
        .rposition(|line| line.trim() == "Hooks need review")?;
    hooks_choice(&lines[title..].join("\n"))
}

/// Require the whole current chooser, including its selected item. Historical/quoted text and
/// unfamiliar dialogs authorize no keys. Describe the path to Continue without trusting; the
/// caller must observe that item highlighted again after navigation before it sends Enter.
pub(super) fn hooks_choice(screen: &str) -> Option<Vec<&'static str>> {
    let lines: Vec<_> = screen
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if lines.len() != 7
        || lines[0] != "Hooks need review"
        || lines[2] != "Hooks can run outside the sandbox after you trust them."
        || lines[6] != "enter confirm · esc skip"
    {
        return None;
    }
    let count = lines[1]
        .strip_suffix(" hooks are new or changed.")
        .or_else(|| lines[1].strip_suffix(" hook is new or changed."))?;
    if !count.bytes().all(|byte| byte.is_ascii_digit()) || count.parse::<u32>().ok()? == 0 {
        return None;
    }
    let choices = [
        "1. Review hooks",
        "2. Trust all and continue",
        "3. Continue without trusting (hooks won't run)",
    ];
    let mut selected = None;
    for (index, (line, expected)) in lines[3..6].iter().zip(choices).enumerate() {
        let text = if let Some(text) = line.strip_prefix('›') {
            if selected.replace(index).is_some() {
                return None;
            }
            text.trim()
        } else {
            line
        };
        if text != expected {
            return None;
        }
    }
    let mut keys = vec!["down"; 2 - selected?];
    keys.push("enter");
    Some(keys)
}

#[cfg(test)]
mod tests {
    use super::*;
    const SCREEN: &str = include_str!("../../tests/launch/fixtures/codex-hooks.txt");

    #[test]
    fn only_the_full_current_hooks_chooser_authorizes_continuing_without_trust() {
        assert_eq!(hooks_choice(SCREEN), Some(vec!["down", "down", "enter"]));
        assert_eq!(
            hooks_choice(&SCREEN.replace("› 1.", "  1.").replace("  2.", "› 2.")),
            Some(vec!["down", "enter"])
        );
        assert_eq!(
            hooks_choice(&SCREEN.replace("› 1.", "  1.").replace("  3.", "› 3.")),
            Some(vec!["enter"])
        );
        assert!(hooks_choice(&SCREEN.replace("7 hooks are", "1 hook is")).is_some());
        for screen in [
            SCREEN.replace("Hooks need review", "Folder access"),
            SCREEN.replace("7 hooks", "0 hooks"),
            SCREEN.replace("7 hooks", "unknown hooks"),
            SCREEN.replace("7 hooks", "+7 hooks"),
            SCREEN.replace("› 1.", "  1."),
            SCREEN.replace("  2.", "› 2."),
            SCREEN.replace(
                "3. Continue without trusting (hooks won't run)",
                "3. Trust hooks",
            ),
            SCREEN.replace("enter confirm · esc skip", "other controls"),
            format!("Earlier screen:\n{SCREEN}"),
            format!("{SCREEN}\n› Do you want to allow this command?"),
            "› Ask Codex to do anything".into(),
            include_str!("../../tests/launch/fixtures/codex-hooks-collapsed.txt").into(),
        ] {
            assert_eq!(hooks_choice(&screen), None, "{screen}");
        }
    }

    #[test]
    fn a_refusal_carries_the_screens_end_only_where_hcs_word_is_not_on_it() {
        let dialog = "  A long conversation\n\n  Continue it whole or from a summary?\n\n❯ 1. From a \
                      summary\n  2. Whole\n";
        let end = "; its terminal showed: A long conversation | Continue it whole or from a \
                   summary? | ❯ 1. From a summary | 2. Whole";
        assert_eq!(shows(dialog, ": dialog open"), end);
        assert_eq!(shows(dialog, ""), end);
        // hc's word is the question the screen asks: the refusal already says it.
        assert_eq!(shows(dialog, ": Continue it whole or from a summary?"), "");
        assert_eq!(shows(" \n\n", ": dialog open"), "");
        // Only the last lines, each cut: a screen full of history stays one readable line.
        let long: String = (0..20)
            .map(|n| format!("line {n} {}\n", "x".repeat(300)))
            .collect();
        let said = shows(&long, ": dialog open");
        let kept: Vec<&str> = said["; its terminal showed: ".len()..]
            .split(" | ")
            .collect();
        assert_eq!(kept.len(), SHOWN_LINES);
        assert!(kept[0].starts_with("line 12 ") && kept[7].starts_with("line 19 "));
        assert!(kept.iter().all(|line| line.chars().count() == SHOWN_WIDTH));
    }

    #[test]
    fn a_whole_screen_answers_only_for_the_chooser_at_its_bottom() {
        let banner = ">_ OpenAI Codex (v0.167.0)\n   ~/somewhere\n\n";
        assert_eq!(
            bottom_choice(&format!("{banner}{SCREEN}\n\n")),
            Some(vec!["down", "down", "enter"])
        );
        // A quoted chooser above the live one does not count; the one at the bottom does.
        let earlier = SCREEN.replace("› 1.", "  1.").replace("  3.", "› 3.");
        assert_eq!(
            bottom_choice(&format!("{earlier}\n{SCREEN}")),
            Some(vec!["down", "down", "enter"])
        );
        for screen in [
            format!("{banner}{SCREEN}\n› Ask Codex to do anything"),
            format!("{banner}› Ask Codex to do anything"),
            String::new(),
        ] {
            assert_eq!(bottom_choice(&screen), None, "{screen}");
        }
    }
}
