//! The command a landing starts. `[board] on_land` in the project's `symphony.toml` names it (the
//! default is none), and `tasks land` runs it once the task is closed, detached from the main
//! checkout with the task's slug and the commit that landed it appended, so the land never waits on
//! it. Each start appends its lines to `sf-tasks/on-land.log` in the git common dir. The Symphony's
//! own project names `just nightly --due` there: the release cadence counted in landings
//! (docs/RELEASES.md). A board `TASKS_SERVER` names (a test's) starts none unless
//! `TASKS_ON_LAND=on`; `TASKS_ON_LAND=off` starts none anywhere.

use crate::cli::Env;
use crate::cli::machine_work::recorded::{PARENT_VAR, PIPELINE_VAR};
use crate::on_review::{MANIFEST, Started, open_log_named, program, startable};
use crate::sweep::trigger::{folder, now};
use crate::sys::{self, Stdio};
use serde::Deserialize;
use std::fs;
use std::io::Write;
use std::path::Path;

/// What each start said, under a line saying when and for which landing.
pub const LOG: &str = "on-land.log";
/// What says whether a landing may start the command here: `on` or `off`, else only on the
/// repository's own board.
pub const SWITCH: &str = "TASKS_ON_LAND";

/// Whether a landing in `env` may start the command: [`SWITCH`] says `on` or `off`; else only on
/// the repository's own board, which no `TASKS_SERVER` names.
pub fn enabled(env: &Env) -> bool {
    match env.var(SWITCH) {
        Some("off") => false,
        Some("on") => true,
        _ => env.var("TASKS_SERVER").is_none(),
    }
}

#[derive(Default, Deserialize)]
struct Manifest {
    #[serde(default)]
    board: Board,
}

#[derive(Default, Deserialize)]
struct Board {
    #[serde(default)]
    on_land: Vec<String>,
}

/// The command `[board] on_land` names in the manifest at `root`: none when the file, the table or
/// the key is missing or the list is empty. Only that key is read; the rest of the file is the
/// Symphony's.
pub fn command(root: &Path) -> Result<Option<Vec<String>>, String> {
    let path = root.join(MANIFEST);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };
    let manifest: Manifest = toml::from_str(&text)
        .map_err(|error| format!("{}: [board] on_land: {error}", path.display()))?;
    let command = manifest.board.on_land;
    if command.is_empty() || command[0].is_empty() {
        return Ok(None);
    }
    Ok(Some(command))
}

/// The task `slug` has landed at `commit` on the default branch of the main checkout `main`, whose
/// git common dir is `common`: start the manifest's command, detached, when `env` lets a landing
/// start one. Nothing waits on it.
pub fn trigger(env: &Env, main: &Path, common: &Path, slug: &str, commit: &str) -> Started {
    if !enabled(env) {
        return Started::None;
    }
    let command = match command(main) {
        Ok(Some(command)) => command,
        Ok(None) => return Started::None,
        Err(problem) => return refuse(common, slug, &problem),
    };
    let mut args = command;
    args.extend([slug.to_string(), commit.to_string()]);
    match spawn(env, main, common, slug, commit, &args) {
        Ok(()) => Started::Command(args),
        Err(error) => refuse(common, slug, &format!("cannot start {}: {error}", args[0])),
    }
}

/// Say in the log why nothing started for `slug`.
fn refuse(common: &Path, slug: &str, why: &str) -> Started {
    if let Ok(mut log) = open_log_named(&folder(common), LOG) {
        let _ = writeln!(
            log,
            "[{}] {slug} landed\nno command: {why}",
            sys::local_minute(now())
        );
    }
    Started::Refused(why.to_string())
}

/// `args` in a session of its own, from the main checkout `main`, with `env`'s environment but the
/// lander's session ([`sys::SESSION_VARS`]), the task the land recorded its builds for and any
/// recorded run it ran in, its output appended to the log; a thread reaps it when it ends. A program
/// that cannot start starts nothing.
fn spawn(
    env: &Env,
    main: &Path,
    common: &Path,
    slug: &str,
    commit: &str,
    args: &[String],
) -> std::io::Result<()> {
    let mut log = open_log_named(&folder(common), LOG)?;
    writeln!(
        log,
        "[{}] {slug} landed at {commit}; {}",
        sys::local_minute(now()),
        args.join(" ")
    )?;
    let program = program(main, &args[0]);
    startable(main, &program, env)?;
    let mut command = sys::command_with(program, &env.vars);
    command
        .env_remove(PARENT_VAR)
        .env_remove(PIPELINE_VAR)
        .env_remove("TASKS_TASK")
        .env_remove("SYMPHONY_LAND")
        .args(&args[1..])
        .current_dir(main)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .new_session();
    let mut child = command.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    fn env(pairs: &[(&str, &str)]) -> Env {
        Env {
            vars: pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            cwd: std::env::temp_dir(),
        }
    }

    /// A folder of the test's own, emptied.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("on-land-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn only_the_repositorys_own_board_starts_the_command_unless_told() {
        assert!(enabled(&env(&[])));
        assert!(!enabled(&env(&[("TASKS_SERVER", "127.0.0.1:1")])));
        assert!(enabled(&env(&[
            ("TASKS_SERVER", "127.0.0.1:1"),
            (SWITCH, "on")
        ])));
        assert!(!enabled(&env(&[(SWITCH, "off")])));
        assert!(!enabled(&env(&[
            ("TASKS_SERVER", "127.0.0.1:1"),
            (SWITCH, "yes")
        ])));
    }

    #[test]
    fn the_manifest_names_the_command_or_none_and_a_broken_one_is_an_error() {
        let dir = scratch("manifest");
        assert_eq!(command(&dir), Ok(None), "no manifest");
        for (text, expected) in [
            ("[project]\nname = \"x\"\n", None),
            ("[board]\non_review = [\"x\"]\n", None),
            ("[board]\non_land = []\n", None),
            ("[board]\non_land = [\"\"]\n", None),
            (
                "[board]\non_land = [\"just\", \"nightly\", \"--due\"]\n",
                Some(vec![
                    "just".to_string(),
                    "nightly".to_string(),
                    "--due".to_string(),
                ]),
            ),
        ] {
            fs::write(dir.join(MANIFEST), text).unwrap();
            assert_eq!(command(&dir), Ok(expected), "{text}");
        }
        fs::write(dir.join(MANIFEST), "[board]\non_land = \"just\"\n").unwrap();
        let problem = command(&dir).unwrap_err();
        assert!(
            problem.contains("symphony.toml: [board] on_land:"),
            "{problem}"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    /// The log of `common` once it holds `wanted`, waited for up to ten seconds.
    fn logged(common: &Path, wanted: &str) -> String {
        let began = Instant::now();
        loop {
            let text = fs::read_to_string(folder(common).join(LOG)).unwrap_or_default();
            if text.contains(wanted) || began.elapsed() > Duration::from_secs(10) {
                return text;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn a_landing_starts_the_command_detached_with_its_slug_and_commit_and_no_session() {
        let main = scratch("starts");
        let common = main.join(".git");
        fs::write(
            main.join(MANIFEST),
            "[board]\non_land = [\"sh\", \"-c\", \"echo ran \\\"$1\\\" \\\"$2\\\" in \\\"$PWD\\\" \
             session=${TASKS_SESSION:-none} task=${TASKS_TASK:-none}\", \"hook\"]\n",
        )
        .unwrap();
        let with = env(&[
            ("PATH", "/usr/bin:/bin"),
            ("TASKS_SESSION", "lander"),
            ("TASKS_TASK", "tools/x"),
        ]);
        let Started::Command(args) = trigger(&with, &main, &common, "tools/x", "abc123") else {
            panic!("the command starts");
        };
        assert_eq!(args[..2], ["sh", "-c"]);
        assert_eq!(args[3..], ["hook", "tools/x", "abc123"], "{args:?}");
        let text = logged(&common, "ran tools/x");
        assert!(
            text.contains("] tools/x landed at abc123; sh -c "),
            "{text}"
        );
        let main_seen = main.canonicalize().unwrap();
        assert!(
            text.contains(&format!(
                "ran tools/x abc123 in {} session=none task=none",
                main_seen.display()
            )),
            "{text}"
        );
        fs::remove_dir_all(&main).unwrap();
    }

    #[test]
    fn a_command_that_cannot_start_or_a_broken_manifest_is_logged_and_nothing_runs() {
        let main = scratch("refused");
        let common = main.join(".git");
        let quiet = env(&[("PATH", "/usr/bin:/bin")]);
        assert_eq!(
            trigger(&quiet, &main, &common, "tools/x", "abc"),
            Started::None,
            "no manifest, no command"
        );
        assert!(!folder(&common).join(LOG).exists());
        fs::write(
            main.join(MANIFEST),
            "[board]\non_land = [\"tools/missing.sh\"]\n",
        )
        .unwrap();
        let Started::Refused(why) = trigger(&quiet, &main, &common, "tools/x", "abc") else {
            panic!("a missing program refuses");
        };
        assert!(why.starts_with("cannot start tools/missing.sh:"), "{why}");
        fs::write(main.join(MANIFEST), "[board]\non_land = 3\n").unwrap();
        let Started::Refused(why) = trigger(&quiet, &main, &common, "tools/y", "def") else {
            panic!("a broken manifest refuses");
        };
        assert!(why.contains("[board] on_land:"), "{why}");
        let text = fs::read_to_string(folder(&common).join(LOG)).unwrap();
        assert!(
            text.contains("tools/x landed at abc; tools/missing.sh tools/x abc"),
            "{text}"
        );
        assert!(
            text.contains("tools/y landed\nno command: ") && text.contains("[board] on_land:"),
            "{text}"
        );
        assert_eq!(
            trigger(
                &env(&[("PATH", "/usr/bin:/bin"), (SWITCH, "off")]),
                &main,
                &common,
                "tools/z",
                "abc"
            ),
            Started::None
        );
        assert!(!text.contains("tools/z"));
        fs::remove_dir_all(&main).unwrap();
    }
}
