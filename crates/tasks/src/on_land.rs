//! The command a landing starts. `[board] on_land` in the project's `symphony.toml` names it (the
//! default is none), and `tasks land` runs it once the task is closed, last, after the regenerator
//! is started, detached from the main checkout with the task's slug and the commit that landed it
//! appended, with none of git's repository variables. The land listens to it for
//! [`SAYS`] at most ([`SAYS_VAR`] sets another window) and shows what it said by then (a count
//! says all it has to well within it),
//! then goes on, never waiting on it longer. Each start's output goes to a file of its own,
//! `symphony/tasks/on-land/STAMP-SLUG.log` in the git common dir (the newest [`KEPT_STARTS`] kept),
//! and a line naming that file, or why nothing started, to `symphony/tasks/on-land.log` (set aside
//! as `on-land.log.1` past [`LOG_MOST`] bytes). The Symphony's own project names there the build of
//! `symphony-build` its main checkout links (any run of its `release` recipe there links it), as
//! `release --due`: the release cadence counted in landings, which builds nothing unless a run is
//! due. A main checkout that links no such build starts nothing, which the log says, and the next
//! landing's count still counts every landing since the last release (docs/workflow/RELEASES.md, the
//! trigger). A board `TASKS_SERVER` names (a
//! test's) starts none unless `TASKS_ON_LAND=on`; `TASKS_ON_LAND=off` starts none anywhere.

use crate::cli::Env;
use crate::cli::machine_work::recorded::{PARENT_VAR, PIPELINE_VAR};
use crate::on_review::{MANIFEST, Started, open_log_named, program, startable};
use crate::sweep::trigger::{folder, now};
use crate::sys::{self, Stdio};
use serde::Deserialize;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A line per start, naming its own output, or saying why nothing started.
pub const LOG: &str = "on-land.log";
/// The folder beside [`LOG`] each start's own output goes to.
pub const STARTS: &str = "on-land";
/// What says whether a landing may start the command here: `on` or `off`, else only on the
/// repository's own board.
pub const SWITCH: &str = "TASKS_ON_LAND";
/// The longest a landing listens to its command before it goes on.
pub const SAYS: Duration = Duration::from_secs(3);
/// What sets, in seconds, how long a landing listens in place of [`SAYS`]: a test's, which gives a
/// loaded machine's command the time it needs to say what it says at once.
pub const SAYS_VAR: &str = "TASKS_ON_LAND_SAYS";
/// The starts' outputs kept: the oldest goes when a landing starts another past them.
pub const KEPT_STARTS: usize = 40;
/// The size [`LOG`] may reach before a start sets it aside as `on-land.log.1`, which replaces an
/// older one: the two hold at most about twice this.
pub const LOG_MOST: u64 = 256 * 1024;
/// How often a landing looks whether its command has ended while it listens.
const LOOK: Duration = Duration::from_millis(25);

/// What a landing heard of its command: the lines it said within [`SAYS`], whether it had ended
/// by then, and the file all it says goes to.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Heard {
    pub lines: Vec<String>,
    pub ended: bool,
    pub output: PathBuf,
}

/// How long a landing in `env` listens to its command: [`SAYS`], unless [`SAYS_VAR`] says.
pub fn says(env: &Env) -> Duration {
    env.seconds(SAYS_VAR, SAYS)
}

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
/// start one, and hear what it says within [`says`]. Nothing waits on it longer.
pub fn trigger(
    env: &Env,
    main: &Path,
    common: &Path,
    slug: &str,
    commit: &str,
) -> (Started, Heard) {
    let window = says(env);
    let mut until = None;
    // The window opens at the first look, once the command has started.
    let listening =
        move |_: &Path| *until.get_or_insert_with(|| Instant::now() + window) > Instant::now();
    trigger_listening(env, main, common, slug, commit, listening)
}

/// [`trigger`], listening to the command for as long as `listening`, asked at each look with the
/// file its output goes to, says: a test's clock, which the command's own words move.
fn trigger_listening(
    env: &Env,
    main: &Path,
    common: &Path,
    slug: &str,
    commit: &str,
    listening: impl FnMut(&Path) -> bool,
) -> (Started, Heard) {
    if !enabled(env) {
        return (Started::None, Heard::default());
    }
    let command = match command(main) {
        Ok(Some(command)) => command,
        Ok(None) => return (Started::None, Heard::default()),
        Err(problem) => return (refuse(common, slug, &problem), Heard::default()),
    };
    let mut args = command;
    args.extend([slug.to_string(), commit.to_string()]);
    match spawn(env, main, common, (slug, commit), &args, listening) {
        Ok(heard) => (Started::Command(args), heard),
        Err(error) => (
            refuse(common, slug, &format!("cannot start {}: {error}", args[0])),
            Heard::default(),
        ),
    }
}

/// Say in the log why nothing started for `slug`.
fn refuse(common: &Path, slug: &str, why: &str) -> Started {
    if let Ok(mut log) = open_log(common) {
        let _ = writeln!(
            log,
            "[{}] {slug} landed\nno command: {why}",
            sys::local_minute(now())
        );
    }
    Started::Refused(why.to_string())
}

/// [`LOG`] in `common`'s board folder, opened to append: set aside first as `on-land.log.1` once
/// it holds [`LOG_MOST`] bytes or more.
fn open_log(common: &Path) -> std::io::Result<File> {
    let dir = folder(common);
    let path = dir.join(LOG);
    if fs::metadata(&path).is_ok_and(|meta| meta.len() >= LOG_MOST) {
        let _ = fs::rename(&path, dir.join(format!("{LOG}.1")));
    }
    open_log_named(&dir, LOG)
}

/// The file in `starts` the start for `slug` at `at` (seconds) writes to, named by its time first,
/// so their names sort by age: the oldest past the newest [`KEPT_STARTS`] less one go first.
fn output(starts: &Path, slug: &str, at: f64) -> std::io::Result<PathBuf> {
    fs::create_dir_all(starts)?;
    let mut found: Vec<PathBuf> = (fs::read_dir(starts)?.filter_map(Result::ok))
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "log"))
        .collect();
    found.sort_unstable();
    let gone = found.len().saturating_sub(KEPT_STARTS.saturating_sub(1));
    for path in found.into_iter().take(gone) {
        let _ = fs::remove_file(path);
    }
    let key: String = (slug.chars())
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // a time now, in ms
    let stamp = (at * 1000.0) as u64;
    Ok(starts.join(format!("{stamp:015}-{key}.log")))
}

/// `args` in a session of its own, from the main checkout `main`, with `env`'s environment but the
/// lander's session ([`sys::SESSION_VARS`]), the marks of how the land was asked for
/// ([`sys::MARKS`]: a release run's checks are no command an agent ran unasked, and it lands
/// nothing), the task the land recorded its builds for and any recorded run it ran in, its output
/// to a file of its own the log names: what it said while `listening` said to listen. A thread
/// reaps it when it ends later. A program that cannot start starts nothing.
fn spawn(
    env: &Env,
    main: &Path,
    common: &Path,
    (slug, commit): (&str, &str),
    args: &[String],
    mut listening: impl FnMut(&Path) -> bool,
) -> std::io::Result<Heard> {
    let at = now();
    let output = output(&folder(common).join(STARTS), slug, at)?;
    let mut log = open_log(common)?;
    writeln!(
        log,
        "[{}] {slug} landed at {commit}; {} (its lines: {})",
        sys::local_minute(at),
        args.join(" "),
        output.display()
    )?;
    let program = program(main, &args[0]);
    startable(main, &program, env)?;
    let file = File::create(&output)?;
    let mut command = sys::command_with(program, &env.vars);
    command
        .env_remove(PARENT_VAR)
        .env_remove(PIPELINE_VAR)
        .env_remove("TASKS_TASK");
    // The land's children name no repository to git but the one they run in.
    for name in crate::land::GIT_VARS {
        command.env_remove(name);
    }
    command
        .args(&args[1..])
        .current_dir(main)
        .stdin(Stdio::null())
        .stdout(file.try_clone()?)
        .stderr(file)
        .new_session();
    let mut child = command.spawn()?;
    let mut ended = false;
    while listening(&output) {
        if !matches!(child.try_wait(), Ok(None)) {
            ended = true;
            break;
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "no-timed-waits baseline: looks again for its child's end, where a wait on the child under a deadline is the event"
        )]
        std::thread::sleep(LOOK);
    }
    if !ended {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
    let text = fs::read_to_string(&output).unwrap_or_default();
    Ok(Heard {
        lines: text.lines().map(str::to_string).collect(),
        ended,
        output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Env {
        Env {
            vars: pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            cwd: std::env::temp_dir(),
        }
    }

    /// Looks enough for any loaded machine's `echo` to end within: two minutes of them.
    const LONG_WINDOW_LOOKS: u32 = 4800;

    /// A folder of the test's own, emptied.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("on-land-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The file `path` once it holds `wanted`, waited for up to the backstop of the ten seconds
    /// these tests once gave it.
    #[track_caller]
    fn holding(path: &Path, wanted: &str) -> String {
        test_time::until(
            format!("{} holds {wanted:?}", path.display()),
            test_time::backstop(Duration::from_secs(10)),
            || {
                fs::read_to_string(path)
                    .ok()
                    .filter(|text| text.contains(wanted))
            },
        )
        .unwrap_or_else(|stall| {
            let text = fs::read_to_string(path).unwrap_or_default();
            panic!("{stall}: it holds {text:?}")
        })
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
                "[board]\non_land = [\".target/bin/symphony-build\", \"release\", \"--due\"]\n",
                Some(vec![
                    ".target/bin/symphony-build".to_string(),
                    "release".to_string(),
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

    #[test]
    fn a_landing_starts_the_command_detached_and_hears_what_it_says_at_once() {
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
        // A window no loaded machine's `echo` outlasts, counted in looks: only the command's end
        // ends this listening short of it.
        let mut looks = 0;
        let (Started::Command(args), heard) =
            trigger_listening(&with, &main, &common, "tools/x", "abc123", |_| {
                looks += 1;
                looks <= LONG_WINDOW_LOOKS
            })
        else {
            panic!("the command starts");
        };
        assert!(heard.ended, "it ended, so the land went on at once");
        assert!(looks <= LONG_WINDOW_LOOKS, "{looks} looks");
        assert_eq!(args[..2], ["sh", "-c"]);
        assert_eq!(args[3..], ["hook", "tools/x", "abc123"], "{args:?}");
        let main_seen = main.canonicalize().unwrap();
        assert_eq!(
            heard.lines,
            [format!(
                "ran tools/x abc123 in {} session=none task=none",
                main_seen.display()
            )]
        );
        let starts = folder(&common).join(STARTS);
        assert_eq!(heard.output.parent(), Some(starts.as_path()));
        let name = heard.output.file_name().unwrap().to_string_lossy();
        assert!(name.ends_with("-tools-x.log"), "{name}");
        let log = fs::read_to_string(folder(&common).join(LOG)).unwrap();
        assert!(
            log.contains("] tools/x landed at abc123; sh -c echo ran")
                && log.ends_with(&format!(" (its lines: {})\n", heard.output.display())),
            "{log}"
        );
        fs::remove_dir_all(&main).unwrap();
    }

    /// `just land` is `just tasks land` under the `land` recipe's mark, and the `tasks` recipe's
    /// unasked mark with it; the command starts with neither, nor the lander's session, task,
    /// recorded run or git repository, and with the rest of its environment.
    #[test]
    fn the_command_runs_without_the_marks_of_how_the_land_was_asked_for() {
        let main = scratch("unmarked");
        let common = main.join(".git");
        let names = [
            crate::confine::MARKER,
            crate::land::RECIPE,
            "TASKS_SESSION",
            "TASKS_TASK",
            PARENT_VAR,
            PIPELINE_VAR,
            "GIT_DIR",
            "SYMPHONY_ON_LAND_KEPT",
        ];
        let said: Vec<String> = (names.iter())
            .map(|name| format!("{name}=${{{name}-unset}}"))
            .collect();
        fs::write(
            main.join(MANIFEST),
            format!(
                "[board]\non_land = [\"sh\", \"-c\", \"echo {}\"]\n",
                said.join(" ")
            ),
        )
        .unwrap();
        let mut pairs = vec![("PATH", "/usr/bin:/bin")];
        pairs.extend(names.iter().map(|name| (*name, "1")));
        let (Started::Command(_), heard) = trigger(&env(&pairs), &main, &common, "tools/x", "abc")
        else {
            panic!("the command starts");
        };
        assert_eq!(
            holding(&heard.output, "SYMPHONY_ON_LAND_KEPT="),
            format!(
                "SYMPHONY_UNASKED=unset SYMPHONY_LAND=unset TASKS_SESSION=unset TASKS_TASK=unset \
                 {PARENT_VAR}=unset {PIPELINE_VAR}=unset GIT_DIR=unset SYMPHONY_ON_LAND_KEPT=1\n"
            )
        );
        fs::remove_dir_all(&main).unwrap();
    }

    /// A command that says its first line, then runs until `gate` exists (or a backstop of its own
    /// runs out, so a test that never opens it leaves nothing running), then says `later` and makes
    /// `done`.
    fn held_at(main: &Path, gate: &Path, done: &Path) {
        fs::write(
            main.join(MANIFEST),
            format!(
                "[board]\non_land = [\"sh\", \"-c\", \"echo started; i=0; while [ ! -e '{}' ] && \
                 [ $i -lt 2400 ]; do sleep 0.05; i=$((i+1)); done; echo later; touch '{}'\"]\n",
                gate.display(),
                done.display()
            ),
        )
        .unwrap();
    }

    /// Opens the gate of a command [`held_at`] it and waits until it has said `later` into
    /// `output` and made `done`, its last act, so nothing it still does races what the test does
    /// next (a `touch` into a folder the test is removing leaves it not empty). What `output`
    /// holds then.
    fn opened(gate: &Path, done: &Path, output: &Path) -> String {
        fs::write(gate, "").unwrap();
        let said = holding(output, "later");
        test_time::until_true(
            "the command made its done file",
            test_time::backstop(Duration::from_secs(10)),
            || done.exists(),
        )
        .unwrap();
        said
    }

    #[test]
    fn a_command_that_runs_on_is_heard_for_a_while_and_left_running() {
        let main = scratch("runs-on");
        let common = main.join(".git");
        let (gate, done) = (main.join("gate"), main.join("done"));
        held_at(&main, &gate, &done);
        // The window runs out once the command has said its first line, however long a loaded
        // machine took to say it, while the command is held at its gate.
        let (Started::Command(_), heard) = trigger_listening(
            &env(&[("PATH", "/usr/bin:/bin")]),
            &main,
            &common,
            "tools/long",
            "abc",
            |output| !fs::read_to_string(output).is_ok_and(|text| text.contains("started\n")),
        ) else {
            panic!("the command starts");
        };
        assert!(!heard.ended);
        assert_eq!(heard.lines, ["started"]);
        // It runs on past the land, into its own file.
        assert!(opened(&gate, &done, &heard.output).contains("started\nlater\n"));
        fs::remove_dir_all(&main).unwrap();
    }

    /// The window a landing listens for is [`SAYS`], or what [`SAYS_VAR`] sets, and never shorter:
    /// a command still held when it runs out is left running.
    #[test]
    fn a_landing_listens_its_whole_window_to_a_command_that_runs_on() {
        let main = scratch("window");
        let common = main.join(".git");
        let (gate, done) = (main.join("gate"), main.join("done"));
        held_at(&main, &gate, &done);
        for (set, window) in [(None, SAYS), (Some("0.2"), Duration::from_millis(200))] {
            let _ = fs::remove_file(&gate);
            let _ = fs::remove_file(&done);
            let mut pairs = vec![("PATH", "/usr/bin:/bin")];
            pairs.extend(set.map(|value| (SAYS_VAR, value)));
            let began = Instant::now();
            let (Started::Command(_), heard) =
                trigger(&env(&pairs), &main, &common, "tools/w", "a")
            else {
                panic!("the command starts");
            };
            let waited = began.elapsed();
            assert!(waited >= window, "{waited:?} of {window:?}");
            assert!(!heard.ended, "held at its gate, it runs on");
            // The first command has ended before the second starts, and the second before the
            // folder goes.
            assert!(opened(&gate, &done, &heard.output).contains("started\nlater\n"));
        }
        assert_eq!(says(&env(&[(SAYS_VAR, "45")])), Duration::from_secs(45));
        assert_eq!(says(&env(&[])), SAYS);
        fs::remove_dir_all(&main).unwrap();
    }

    #[test]
    fn a_command_that_cannot_start_or_a_broken_manifest_is_logged_and_nothing_runs() {
        let main = scratch("refused");
        let common = main.join(".git");
        let quiet = env(&[("PATH", "/usr/bin:/bin")]);
        assert_eq!(
            trigger(&quiet, &main, &common, "tools/x", "abc"),
            (Started::None, Heard::default()),
            "no manifest, no command"
        );
        assert!(!folder(&common).join(LOG).exists());
        fs::write(
            main.join(MANIFEST),
            "[board]\non_land = [\"tools/missing.sh\"]\n",
        )
        .unwrap();
        let (Started::Refused(why), _) = trigger(&quiet, &main, &common, "tools/x", "abc") else {
            panic!("a missing program refuses");
        };
        assert!(why.starts_with("cannot start tools/missing.sh:"), "{why}");
        fs::write(main.join(MANIFEST), "[board]\non_land = 3\n").unwrap();
        let (Started::Refused(why), _) = trigger(&quiet, &main, &common, "tools/y", "def") else {
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
            (Started::None, Heard::default())
        );
        assert!(!text.contains("tools/z"));
        fs::remove_dir_all(&main).unwrap();
    }

    #[test]
    fn the_log_is_set_aside_past_its_size_and_only_the_newest_outputs_are_kept() {
        let main = scratch("kept");
        let common = main.join(".git");
        let dir = folder(&common);
        fs::create_dir_all(dir.join(STARTS)).unwrap();
        // A log past its size, and an older one set aside before, which the new one replaces.
        let full = "x".repeat(usize::try_from(LOG_MOST).unwrap());
        fs::write(dir.join(LOG), &full).unwrap();
        fs::write(dir.join(format!("{LOG}.1")), "the oldest\n").unwrap();
        for n in 0..KEPT_STARTS + 5 {
            fs::write(dir.join(STARTS).join(format!("{n:015}-tools-old.log")), "").unwrap();
        }
        fs::write(dir.join(STARTS).join("notes.txt"), "not a start's").unwrap();
        fs::write(main.join(MANIFEST), "[board]\non_land = [\"true\"]\n").unwrap();
        let (Started::Command(_), heard) = trigger(
            &env(&[("PATH", "/usr/bin:/bin")]),
            &main,
            &common,
            "tools/new",
            "abc",
        ) else {
            panic!("the command starts");
        };
        assert_eq!(
            fs::read_to_string(dir.join(format!("{LOG}.1"))).unwrap(),
            full
        );
        let log = fs::read_to_string(dir.join(LOG)).unwrap();
        assert!(
            log.starts_with('[') && log.contains("tools/new landed at abc"),
            "{log}"
        );
        let mut kept: Vec<String> = (fs::read_dir(dir.join(STARTS)).unwrap())
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        kept.sort();
        assert_eq!(kept.len(), KEPT_STARTS + 1, "{kept:?}");
        assert_eq!(
            kept[0],
            format!("{:015}-tools-old.log", 6),
            "the oldest went"
        );
        assert!(kept.contains(&"notes.txt".to_string()));
        assert!(
            kept.contains(
                &heard
                    .output
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            )
        );
        fs::remove_dir_all(&main).unwrap();
    }
}
