//! Answer only the observed Codex hooks chooser while a launched Codex has not taken its first
//! message.

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
