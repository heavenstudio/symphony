//! A command fixture, selected only in an owned test process's PATH. The process it runs is
//! started as one an agent starts from its hc terminal, and the stand-in records what it holds of
//! that session: the server's Tailscale calls must hold none of it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use symphony_board::sys::Command;

const ROOT: &str = "SYMPHONY_AFTER_ACCESS_ROOT";
const CASE: &str = "SYMPHONY_AFTER_ACCESS_CASE";

/// The session the test process is started in: the board handle, hc's terminal, that terminal's
/// socket, the core and the scope.
const STARTED: [(&str, &str); 5] = [
    ("TASKS_SESSION", "fixture-session"),
    ("HC_TERMINAL_ID", "t7"),
    ("HC_TERMINAL_SOCKET", "/fixture/hc/t7.sock"),
    ("HC_SESSION", "fixture-core"),
    ("HC_SCOPE", "fixture-scope"),
];

pub fn child_case() -> Option<String> {
    std::env::var(CASE).ok()
}

pub struct Fixture {
    root: PathBuf,
}

impl Fixture {
    pub fn new(stdout: &[u8], stderr: &[u8], status: u8) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "symphony-native-access-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        // Shell builtins emit fixture bytes without launching additional programs under load.
        let octal = |bytes: &[u8]| {
            bytes
                .iter()
                .map(|byte| format!("\\{byte:03o}"))
                .collect::<String>()
        };
        let script = format!(
            r#"printf '%s\n' "$0" "$@" >> "$SYMPHONY_AFTER_ACCESS_ROOT/args"
printf '%s|%s|%s|%s|%s\n' "${{TASKS_SESSION-unset}}" "${{HC_TERMINAL_ID-unset}}" \
    "${{HC_TERMINAL_SOCKET-unset}}" "${{HC_SESSION-unset}}" "${{HC_SCOPE-unset}}" \
    >> "$SYMPHONY_AFTER_ACCESS_ROOT/held"
printf '%b' '{}'
printf '%b' '{}' >&2
exit {status}
"#,
            octal(stdout),
            octal(stderr)
        );
        for command in ["whois", "status"] {
            std::fs::write(root.join(command), &script).unwrap();
        }
        // Execute the installed shell directly, avoiding new shebang-executable startup inside
        // the CLI's deadline. Bash reads its first argument (whois or status) as the script in
        // this child's private working directory.
        std::os::unix::fs::symlink("/bin/bash", root.join("tailscale")).unwrap();
        Self { root }
    }

    pub fn run(&self, test: &str, case: &str) {
        // Set PATH and the session only on this child: concurrent tests and the machine's CLI
        // stay untouched.
        let done = symphony_board::sys::output(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", test, "--nocapture"])
                .current_dir(&self.root)
                .env(ROOT, &self.root)
                .env(CASE, case)
                .env("PATH", &self.root)
                .envs(STARTED)
                .env_remove("BASH_ENV"),
        )
        .unwrap();
        assert!(
            done.status.success(),
            "{case} (recorded command: {:?}): {}{}",
            std::fs::read_to_string(self.root.join("args")),
            String::from_utf8_lossy(&done.stdout),
            String::from_utf8_lossy(&done.stderr)
        );
        // A misspelled exact filter must not turn an unexecuted probe into a pass.
        assert!(
            String::from_utf8_lossy(&done.stdout).contains("1 passed"),
            "{case}: the child must execute its assertion: {done:?}"
        );
    }

    /// The one call the stand-in answered was `tailscale` with `expected`, and it ran without the
    /// session its caller was started in.
    pub fn assert_args(&self, expected: &[&str]) {
        let args = std::fs::read_to_string(self.root.join("args")).unwrap();
        assert_eq!(args.lines().collect::<Vec<_>>(), expected);
        let held = std::fs::read_to_string(self.root.join("held")).unwrap();
        assert_eq!(held, "unset|unset|unset|unset|unset\n", "{expected:?}");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
