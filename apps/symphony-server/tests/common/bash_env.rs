//! Supply a noisy shell startup file to the fixture's caller, in an owned process.

use std::path::PathBuf;
use symphony_board::sys::Command;

const CHILD: &str = "SF_SYMPHONY_FIXTURE_BASH_ENV_CHILD";

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn run(test: &str, exercise: fn()) {
    if std::env::var(CHILD).as_deref() == Ok(test) {
        exercise();
        return;
    }
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "sf-fixture-bash-env-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir(&root).unwrap();
    let scratch = Scratch(root);
    let startup = scratch.0.join("startup");
    std::fs::write(&startup, "printf 'unexpected BASH_ENV startup output\\n'\n").unwrap();
    let done = symphony_board::sys::output(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture"])
            .env(CHILD, test)
            .env("BASH_ENV", startup),
    )
    .unwrap();
    assert!(
        done.status.success(),
        "fixture with BASH_ENV: {}{}",
        String::from_utf8_lossy(&done.stdout),
        String::from_utf8_lossy(&done.stderr)
    );
    assert!(
        String::from_utf8_lossy(&done.stdout).contains("1 passed"),
        "the exact child filter must run its assertions: {done:?}"
    );
}
