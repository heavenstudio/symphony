//! The schedule entry `--schedule` prints, which the owner installs (docs/workflow/RELEASES.md): on macOS a
//! launchd agent that runs the release run once a day at a set time (later, on waking, when the Mac
//! slept through it), elsewhere a crontab line. Either makes the log's folder, then runs `just
//! release` in the checkout with the `PATH` of the shell that printed it, appending what it prints
//! to one log. Printing it installs and makes nothing.

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
    format!("symphony.release.{id}")
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

/// The shell line the entry runs: make the log's folder, then `just release` in `root`, appending
/// to `log`; `path`, when given, is the release run's `PATH`.
fn program(root: &Path, log: &Path, path: Option<&str>) -> String {
    let folder = log.parent().unwrap_or(Path::new("/"));
    let run = path.map_or_else(
        || "exec just release".to_string(),
        |path| format!("PATH={} just release", quoted(path)),
    );
    format!(
        "mkdir -p {} && cd {} && {run} >> {} 2>&1",
        quoted(&folder.to_string_lossy()),
        quoted(&root.to_string_lossy()),
        quoted(&log.to_string_lossy()),
    )
}

/// The entry running `just release` in `root` at `hour`:`minute` each day with `path` as its
/// `PATH`, appending to `log`: a launchd agent labelled `label` on macOS, else a crontab line.
pub(crate) fn entry(
    root: &Path,
    label: &str,
    (hour, minute): (u32, u32),
    path: &str,
    log: &Path,
    macos: bool,
) -> String {
    if !macos {
        // cron reads an unescaped `%` as a new line.
        return format!(
            "# The Symphony release run (docs/workflow/RELEASES.md); install with `crontab -e`.\n\
             {minute} {hour} * * * {}\n",
            program(root, log, Some(path)).replace('%', "\\%")
        );
    }
    let plist = format!("~/Library/LaunchAgents/{label}.plist");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<!-- The Symphony release run (docs/workflow/RELEASES.md). Install: save this as {plist}, then
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
</dict>
</plist>
"#,
        xml(label),
        xml(&program(root, log, None)),
        xml(path),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_tree::Tree;
    use std::fs;
    use symphony_board::sys::output;

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

    /// A `just` that says where it ran and with what, in a folder of `tree`'s: the folder.
    fn fake_just(tree: &Tree) -> std::path::PathBuf {
        let just = tree.write(
            "bin/just",
            "#!/bin/sh\necho \"just $* in $PWD\"\necho \"its stderr\" >&2\n",
        );
        fs::set_permissions(&just, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        just.parent().unwrap().to_path_buf()
    }

    /// Run the shell line `line` with `PATH` `path`, as the schedule would.
    fn scheduled(line: &str, path: &str) {
        let done = output(
            symphony::children::command("/bin/sh")
                .args(["-c", line])
                .env("PATH", path)
                .current_dir("/"),
        )
        .unwrap();
        assert!(done.status.success(), "{line}: {done:?}");
    }

    #[test]
    fn the_crontab_line_runs_the_release_run_in_the_checkout_and_makes_its_logs_folder() {
        let said = entry(
            Path::new("/home/o/sym phony"),
            "symphony.release.symphony",
            (3, 5),
            "/usr/bin:/bin",
            Path::new("/home/o/sym phony/.git/symphony/release/scheduled.log"),
            false,
        );
        assert_eq!(
            said.lines().nth(1).unwrap(),
            "5 3 * * * mkdir -p '/home/o/sym phony/.git/symphony/release' && \
             cd '/home/o/sym phony' && PATH='/usr/bin:/bin' just release >> \
             '/home/o/sym phony/.git/symphony/release/scheduled.log' 2>&1"
        );
        assert!(said.starts_with("# "), "{said}");

        // As cron runs it: the command after the five time fields, `\%` read as `%`.
        let tree = Tree::new();
        let bin = fake_just(&tree);
        let root = tree.path("100% checkout");
        fs::create_dir_all(&root).unwrap();
        let log = tree.path("100% checkout/.git/symphony/release/scheduled.log");
        let path = format!("{}:/usr/bin:/bin", bin.display());
        let said = entry(&root, "l", (3, 0), &path, &log, false);
        let line = said.lines().nth(1).unwrap();
        assert!(line.contains("100\\% checkout"), "{line}");
        let command = line.splitn(6, ' ').nth(5).unwrap().replace("\\%", "%");
        scheduled(&command, "/usr/bin:/bin");
        assert_eq!(
            fs::read_to_string(&log).unwrap(),
            format!("just release in {}\nits stderr\n", root.display())
        );
        scheduled(&command, "/usr/bin:/bin");
        assert_eq!(
            fs::read_to_string(&log).unwrap().lines().count(),
            4,
            "appended"
        );
    }

    #[test]
    fn a_quoted_word_keeps_its_quotes() {
        assert_eq!(quoted("it's"), r"'it'\''s'");
    }

    #[test]
    fn the_launchd_agent_names_the_checkout_its_time_and_path_and_logs_from_its_own_line() {
        let said = entry(
            Path::new("/Users/o/a&b"),
            "symphony.release.symphony",
            (3, 0),
            "/opt/bin:/usr/bin",
            Path::new("/Users/o/a&b/.git/symphony/release/scheduled.log"),
            true,
        );
        for part in [
            "<string>symphony.release.symphony</string>",
            "<string>mkdir -p '/Users/o/a&amp;b/.git/symphony/release' &amp;&amp; \
             cd '/Users/o/a&amp;b' &amp;&amp; exec just release &gt;&gt; \
             '/Users/o/a&amp;b/.git/symphony/release/scheduled.log' 2&gt;&amp;1</string>",
            "<key>PATH</key>\n\t\t<string>/opt/bin:/usr/bin</string>",
            "<key>Hour</key>\n\t\t<integer>3</integer>",
            "<key>Minute</key>\n\t\t<integer>0</integer>",
            "launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/symphony.release.symphony.plist",
        ] {
            assert!(said.contains(part), "{part:?} in {said}");
        }
        assert!(
            !said.contains("StandardOutPath") && !said.contains("StandardErrorPath"),
            "launchd opens no log of its own, in a folder that may not be there: {said}"
        );
    }

    /// The agent is a property list launchd reads: `plutil` (macOS's own) finds it well formed,
    /// and its program, run as launchd runs it, makes the log's folder and logs there.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_launchd_agent_is_a_well_formed_property_list_whose_program_runs_as_printed() {
        let tree = Tree::new();
        let bin = fake_just(&tree);
        let root = tree.path("it's <here>");
        fs::create_dir_all(&root).unwrap();
        let log = tree.path("it's <here>/.git/symphony/release/scheduled.log");
        let path = format!("{}:/usr/bin:/bin", bin.display());
        let file = tree.write(
            "agent.plist",
            &entry(
                &root,
                "symphony.release.symphony",
                (3, 0),
                &path,
                &log,
                true,
            ),
        );
        let done = output(
            symphony::children::command("plutil")
                .arg("-lint")
                .arg(&file),
        )
        .unwrap();
        assert!(done.status.success(), "{done:?}");
        let read = |key: &str| {
            let read = output(
                symphony::children::command("plutil")
                    .args(["-extract", key, "raw", "-o", "-"])
                    .arg(&file),
            )
            .unwrap();
            assert!(read.status.success(), "{key}: {read:?}");
            String::from_utf8_lossy(&read.stdout).trim_end().to_string()
        };
        assert_eq!(read("ProgramArguments.0"), "/bin/sh");
        assert_eq!(read("ProgramArguments.1"), "-c");
        assert!(!log.parent().unwrap().exists());
        scheduled(
            &read("ProgramArguments.2"),
            &read("EnvironmentVariables.PATH"),
        );
        assert_eq!(
            fs::read_to_string(&log).unwrap(),
            format!("just release in {}\nits stderr\n", root.display())
        );
    }
}
