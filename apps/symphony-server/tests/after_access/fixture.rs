//! A command fixture, selected only in an owned test process's PATH.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

const ROOT: &str = "SF_AFTER_ACCESS_ROOT";
const CASE: &str = "SF_AFTER_ACCESS_CASE";

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
            "sf-native-access-{}-{}",
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
            r#"printf '%s\n' "$0" "$@" >> "$SF_AFTER_ACCESS_ROOT/args"
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
        // Set PATH only on this child: concurrent tests and the machine's CLI stay untouched.
        let done = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture"])
            .current_dir(&self.root)
            .env(ROOT, &self.root)
            .env(CASE, case)
            .env("PATH", &self.root)
            .env_remove("BASH_ENV")
            .output()
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

    pub fn assert_args(&self, expected: &[&str]) {
        let args = std::fs::read_to_string(self.root.join("args")).unwrap();
        assert_eq!(args.lines().collect::<Vec<_>>(), expected);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
