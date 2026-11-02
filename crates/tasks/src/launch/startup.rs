//! Answer only the observed Codex hooks chooser while Herdr's original startup is pending.

use std::io::Read;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

pub(super) fn codex_pane<'a>(args: &'a [&str]) -> Option<&'a str> {
    let options = &args[..args.iter().position(|s| *s == "--").unwrap_or(args.len())];
    if !options.starts_with(&["agent", "start"])
        || !options.windows(2).any(|s| s == ["--kind", "codex"])
    {
        return None;
    }
    options.windows(2).find(|s| s[0] == "--pane").map(|s| s[1])
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

struct Owned(Child);
impl Drop for Owned {
    fn drop(&mut self) {
        // The CLI child remains ours until reaped, including observer failures and unwinding.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Preserve the existing call deadline and output while observing the owned pane during startup.
/// The CLI's own startup deadline is unchanged; it alone confirms readiness and session identity.
pub(super) fn within(
    command: &mut Command,
    limit: Duration,
    observe: impl FnMut(Duration) -> Result<(), String>,
) -> Result<Output, String> {
    let (stdout, out_writer) = UnixStream::pair().map_err(|e| e.to_string())?;
    let (stderr, err_writer) = UnixStream::pair().map_err(|e| e.to_string())?;
    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::from(OwnedFd::from(out_writer)))
        .stderr(Stdio::from(OwnedFd::from(err_writer)))
        .spawn();
    // Command retains configured descriptors after spawn; only the child may keep writers open.
    command.stdout(Stdio::null()).stderr(Stdio::null());
    collect(
        child.map_err(|e| e.to_string())?,
        stdout,
        stderr,
        limit,
        observe,
    )
}

fn collect(
    child: Child,
    mut stdout: UnixStream,
    mut stderr: UnixStream,
    limit: Duration,
    mut observe: impl FnMut(Duration) -> Result<(), String>,
) -> Result<Output, String> {
    let mut owned = Owned(child);
    stdout.set_nonblocking(true).map_err(|e| e.to_string())?;
    stderr.set_nonblocking(true).map_err(|e| e.to_string())?;
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let started = Instant::now();
    let mut next_observation = Duration::from_millis(250);
    loop {
        let out_closed = drain(&mut stdout, &mut out)?;
        let err_closed = drain(&mut stderr, &mut err)?;
        let status = owned.0.try_wait().map_err(|e| e.to_string())?;
        if let Some(status) = status
            && out_closed
            && err_closed
        {
            return Ok(Output {
                status,
                stdout: out,
                stderr: err,
            });
        }
        let elapsed = started.elapsed();
        let remaining = limit.saturating_sub(elapsed);
        if remaining.is_zero() {
            return Err(format!("no answer in {} s", limit.as_secs()));
        }
        if status.is_none() && elapsed >= next_observation {
            observe(remaining)?;
            next_observation = started.elapsed() + Duration::from_millis(250);
        }
        std::thread::sleep(Duration::from_millis(10).min(remaining));
    }
}

/// One bounded read per iteration, so even a busy inherited writer cannot starve the deadline.
fn drain(stream: &mut UnixStream, output: &mut Vec<u8>) -> Result<bool, String> {
    let mut buffer = [0; 8192];
    match stream.read(&mut buffer) {
        Ok(0) => Ok(true),
        Ok(n) => {
            output.extend_from_slice(&buffer[..n]);
            Ok(false)
        }
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
            ) =>
        {
            Ok(false)
        }
        Err(e) => Err(e.to_string()),
    }
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
    fn only_a_codex_start_observes_the_pane_and_native_arguments_cannot_change_it() {
        assert_eq!(
            codex_pane(&[
                "agent", "start", "probe", "--kind", "codex", "--pane", "ours", "--", "--pane",
                "other"
            ]),
            Some("ours")
        );
        for args in [
            vec!["agent", "prompt", "probe", "text"],
            vec![
                "agent", "start", "probe", "--kind", "claude", "--pane", "ours", "--", "--kind",
                "codex",
            ],
            vec!["agent", "start", "probe", "--kind", "codex"],
        ] {
            assert_eq!(codex_pane(&args), None);
        }
    }

    #[test]
    fn an_unresponsive_start_is_bounded_by_the_existing_call_deadline() {
        let result = within(
            Command::new("sleep").arg("60"),
            Duration::from_millis(20),
            |_| Ok(()),
        );
        assert!(
            matches!(result, Err(ref why) if why == "no answer in 0 s"),
            "{result:?}"
        );
    }

    #[test]
    fn inherited_output_writers_cannot_hold_a_finished_start_past_its_deadline() {
        let (stdout, out_writer) = UnixStream::pair().unwrap();
        let (stderr, err_writer) = UnixStream::pair().unwrap();
        let child = Command::new("true").spawn().unwrap();
        // These owned endpoints model a wrapper descendant retaining stdout/stderr after its
        // parent exits. No EOF will arrive until this test drops them; collection must not join.
        let result = collect(child, stdout, stderr, Duration::from_millis(20), |_| Ok(()));
        assert!(
            matches!(result, Err(ref why) if why == "no answer in 0 s"),
            "{result:?}"
        );
        drop((out_writer, err_writer));
    }
}
