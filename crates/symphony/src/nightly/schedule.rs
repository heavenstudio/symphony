//! The schedule entry `--schedule` prints, which the owner installs (docs/RELEASES.md): on macOS a
//! launchd agent that runs the nightly once a day at a set time (later, on waking, when the Mac
//! slept through it), elsewhere a crontab line. Either runs `just nightly` in the checkout with the
//! `PATH` of the shell that printed it, and appends what it prints to one log. Printing it installs
//! nothing.

use std::path::Path;

/// The time of day an entry runs at, `HH:MM`: its hour and minute.
pub(crate) fn time(at: &str) -> Option<(u32, u32)> {
    let (hour, minute) = at.split_once(':')?;
    let number = |part: &str, below: u32| {
        (part.len() == 2 && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse::<u32>().ok())
            .flatten()
            .filter(|n| *n < below)
    };
    Some((number(hour, 24)?, number(minute, 60)?))
}

/// The label of the launchd agent of the project `id`.
pub(crate) fn label(id: &str) -> String {
    format!("symphony.nightly.{id}")
}

/// `text` safe in an XML element.
fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// `text` as one word to a POSIX shell.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// The entry running `just nightly` in `root` at `hour`:`minute` each day with `path` as its
/// `PATH`, appending to `log`: a launchd agent labelled `label` on macOS, else a crontab line.
pub(crate) fn entry(
    root: &Path,
    label: &str,
    (hour, minute): (u32, u32),
    path: &str,
    log: &Path,
    macos: bool,
) -> String {
    let root = root.to_string_lossy();
    let log = log.to_string_lossy();
    if !macos {
        return format!(
            "# The Symphony nightly (docs/RELEASES.md); install with `crontab -e`.\n\
             {minute} {hour} * * * cd {} && PATH={} just nightly >> {} 2>&1\n",
            quoted(&root),
            quoted(path),
            quoted(&log)
        );
    }
    let plist = format!("~/Library/LaunchAgents/{label}.plist");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<!-- The Symphony nightly (docs/RELEASES.md). Install: save this as {plist}, then
     launchctl bootstrap gui/$(id -u) {plist}
     Remove: launchctl bootout gui/$(id -u)/{label} -->
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{}</string>
	<key>ProgramArguments</key>
	<array>
		<string>/bin/sh</string>
		<string>-c</string>
		<string>{}</string>
	</array>
	<key>WorkingDirectory</key>
	<string>{}</string>
	<key>EnvironmentVariables</key>
	<dict>
		<key>PATH</key>
		<string>{}</string>
	</dict>
	<key>StartCalendarInterval</key>
	<dict>
		<key>Hour</key>
		<integer>{hour}</integer>
		<key>Minute</key>
		<integer>{minute}</integer>
	</dict>
	<key>StandardOutPath</key>
	<string>{}</string>
	<key>StandardErrorPath</key>
	<string>{}</string>
</dict>
</plist>
"#,
        xml(label),
        xml(&format!("cd {} && exec just nightly", quoted(&root))),
        xml(&root),
        xml(path),
        xml(&log),
        xml(&log),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_of_day_is_two_digit_hours_and_minutes() {
        assert_eq!(time("03:00"), Some((3, 0)));
        assert_eq!(time("23:59"), Some((23, 59)));
        assert_eq!(time("00:00"), Some((0, 0)));
        for at in [
            "24:00", "12:60", "3:00", "03:0", "0300", "03:00:00", "aa:bb", "", "-1:00",
        ] {
            assert_eq!(time(at), None, "{at}");
        }
    }

    #[test]
    fn the_crontab_line_runs_the_nightly_in_the_checkout_at_its_time() {
        let said = entry(
            Path::new("/home/o/sym phony"),
            "symphony.nightly.symphony",
            (3, 5),
            "/usr/bin:/bin",
            Path::new("/home/o/sym phony/.git/sf-symphony/nightly/scheduled.log"),
            false,
        );
        assert_eq!(
            said.lines().nth(1).unwrap(),
            "5 3 * * * cd '/home/o/sym phony' && PATH='/usr/bin:/bin' just nightly >> \
             '/home/o/sym phony/.git/sf-symphony/nightly/scheduled.log' 2>&1"
        );
        assert!(said.starts_with("# "), "{said}");
    }

    #[test]
    fn a_quoted_word_keeps_its_quotes() {
        assert_eq!(quoted("it's"), r"'it'\''s'");
    }

    #[test]
    fn the_launchd_agent_names_the_checkout_its_time_path_and_log() {
        let said = entry(
            Path::new("/Users/o/a&b"),
            "symphony.nightly.symphony",
            (3, 0),
            "/opt/bin:/usr/bin",
            Path::new("/Users/o/a&b/.git/sf-symphony/nightly/scheduled.log"),
            true,
        );
        for part in [
            "<string>symphony.nightly.symphony</string>",
            "<string>cd '/Users/o/a&amp;b' &amp;&amp; exec just nightly</string>",
            "<key>WorkingDirectory</key>\n\t<string>/Users/o/a&amp;b</string>",
            "<key>PATH</key>\n\t\t<string>/opt/bin:/usr/bin</string>",
            "<key>Hour</key>\n\t\t<integer>3</integer>",
            "<key>Minute</key>\n\t\t<integer>0</integer>",
            "<key>StandardOutPath</key>\n\t<string>/Users/o/a&amp;b/.git/sf-symphony/nightly/scheduled.log</string>",
            "launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/symphony.nightly.symphony.plist",
        ] {
            assert!(said.contains(part), "{part:?} in {said}");
        }
    }

    /// The agent is a property list launchd reads: `plutil` (macOS's own) finds it well formed.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_launchd_agent_is_a_well_formed_property_list() {
        let tree = crate::test_tree::Tree::new();
        let file = tree.write(
            "agent.plist",
            &entry(
                Path::new("/Users/o/it's <here>"),
                "symphony.nightly.symphony",
                (3, 0),
                "/usr/bin:/bin",
                Path::new("/tmp/log"),
                true,
            ),
        );
        let done =
            symphony_board::sys::output(crate::children::command("plutil").arg("-lint").arg(&file))
                .unwrap();
        assert!(done.status.success(), "{done:?}");
        let read = symphony_board::sys::output(
            crate::children::command("plutil")
                .args(["-extract", "ProgramArguments.2", "raw", "-o", "-"])
                .arg(&file),
        )
        .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&read.stdout).trim_end(),
            r"cd '/Users/o/it'\''s <here>' && exec just nightly"
        );
    }
}
