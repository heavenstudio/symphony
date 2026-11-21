//! What a run's checks read, and what it ran on (docs/workflow/RELEASES.md, "The rerun").
//!
//! **Inputs.** Before its checks a run asks the tree it checks what each of its steps reads: the
//! script [`SCRIPT`] in the checkout, which prints a line for each check, step or part of a step
//! it can tell, its name as the run's record names it (`verify/lint`, `verify/tests/browser`), a
//! tab, and a digest of its inputs at that commit ([`ask`]). The run records what it was told
//! ([`Inputs`]), a green standing of a release candidate keeps the digest it was green on, and
//! the candidate's rerun carries the green of whatever the tip gives that same digest
//! ([`super::candidate::Candidate::plan`]). A tree without the script names no inputs, and a
//! script that fails, runs past [`BOUND`] or prints a line that does not read tells none, said
//! with why: then nothing is carried by a digest. The digest is the tree's own word, never
//! computed here, so the script that computes it is among what it digests.
//!
//! **The environment.** No tree pins the system, Chrome or PostgreSQL a run's checks ran on, so
//! each run's record names them ([`Environment`]), and a green of another environment is never
//! carried.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::group::{Bound, Failed, output_until};
use symphony::children::command;

/// The script a tree says its checks' inputs with, in the checkout.
pub(crate) const SCRIPT: &str = "tools/check-inputs.sh";
/// How long the script, or a program asked its version, may run, unless the run's time ends
/// sooner.
const BOUND: Duration = Duration::from_secs(5 * 60);

/// A check, step or part of a step, and the digest of its inputs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Input {
    pub(crate) name: String,
    pub(crate) digest: String,
}

/// What a run was told of its checks' inputs: each the tree could tell, or why it told none.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Inputs {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) told: Vec<Input>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}

impl Inputs {
    /// The digest of the inputs of `name`, when the tree told it.
    pub(crate) fn of(&self, name: &str) -> Option<&str> {
        (self.told.iter())
            .find(|input| input.name == name)
            .map(|input| input.digest.as_str())
    }

    /// The names directly under `parent` that the names told say there are, `PARENT/NAME`, once
    /// each in the order told: one told itself, and one known only by what is told under it, as a
    /// step is by its parts (`verify/tests` by `verify/tests/browser`).
    pub(crate) fn under(&self, parent: &str) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for input in &self.told {
            let rest = (input.name.strip_prefix(parent)).and_then(|rest| rest.strip_prefix('/'));
            let Some(first) = rest.and_then(|rest| rest.split('/').next()) else {
                continue;
            };
            let name = format!("{parent}/{first}");
            if !first.is_empty() && !names.contains(&name) {
                names.push(name);
            }
        }
        names
    }
}

/// The inputs the tree checked out at `checkout` tells, within `bound`: none when it has no
/// [`SCRIPT`]; told none, with why, when the script fails or what it printed does not read.
pub(crate) fn ask(checkout: &Path, bound: Bound<'_>) -> Option<Inputs> {
    if !checkout.join(SCRIPT).is_file() {
        return None;
    }
    let failed = |why: String| Inputs {
        told: Vec::new(),
        error: Some(why),
    };
    let mut script = command("bash");
    script.arg(SCRIPT).current_dir(checkout);
    let done = match output_until(&mut script, bound.sooner(BOUND)) {
        Ok(done) => done,
        Err(Failed::Io(e)) => return Some(failed(format!("cannot run {SCRIPT}: {e}"))),
        Err(Failed::Late(took)) => {
            return Some(failed(format!(
                "{SCRIPT} did not end within {} s and was stopped",
                took.as_secs()
            )));
        }
        Err(Failed::Interrupted(_)) => {
            return Some(failed(format!(
                "{SCRIPT} was stopped: the run was told to stop"
            )));
        }
    };
    if !done.status.success() {
        let stderr = String::from_utf8_lossy(&done.stderr);
        let said: Vec<&str> = (stderr.lines().map(str::trim))
            .filter(|line| !line.is_empty())
            .collect();
        return Some(failed(format!(
            "{SCRIPT} failed ({}){}",
            done.status,
            if said.is_empty() {
                String::new()
            } else {
                format!(": {}", said.join(" / "))
            }
        )));
    }
    Some(match parse(&String::from_utf8_lossy(&done.stdout)) {
        Ok(told) => Inputs { told, error: None },
        Err(why) => failed(format!("what {SCRIPT} printed does not read: {why}")),
    })
}

/// The inputs `text` says, a line each, a name, a tab and a digest: an error naming the first line
/// that is not that, or a name said twice. A digest is one word.
pub(crate) fn parse(text: &str) -> Result<Vec<Input>, String> {
    let mut told: Vec<Input> = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let read = line.split_once('\t').filter(|(name, digest)| {
            !name.trim().is_empty()
                && !digest.is_empty()
                && !digest.contains(|c: char| c.is_whitespace())
        });
        let Some((name, digest)) = read else {
            return Err(format!(
                "line {}, {line:?}, is not a name, a tab and a digest",
                n + 1
            ));
        };
        if told.iter().any(|known| known.name == name) {
            return Err(format!("`{name}` is said twice"));
        }
        told.push(Input {
            name: name.to_string(),
            digest: digest.to_string(),
        });
    }
    Ok(told)
}

/// What a run ran on, which no tree pins: the system and its version, and the Chrome and the
/// PostgreSQL its tests use, each as it says its own version, or why it does not; and the
/// PostgreSQL 16 the hosted board's check proves the board on beside that one
/// ([`symphony_board::server::postgres::hosted_programs_16`], the check's own rule). A record
/// written before that was kept names none, and so is no environment a run now runs on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Environment {
    pub(crate) system: String,
    pub(crate) chrome: String,
    pub(crate) postgresql: String,
    #[serde(default)]
    pub(crate) postgresql_16: String,
}

/// What an environment says of a program its record does not name.
const UNRECORDED: &str = "not recorded";

impl Environment {
    /// The environment of this machine now, for the project at `root`, each program asked within
    /// `bound`.
    pub(crate) fn now(root: &Path, bound: Bound<'_>) -> Environment {
        let chrome = match symphony_chrome::find() {
            Some(chrome) => version(&chrome, bound),
            None => "none found".into(),
        };
        let postgresql = match symphony_board::server::postgres::discover_programs(root) {
            Ok(programs) => version(&programs.join("postgres").to_string_lossy(), bound),
            Err(_) => "none found".into(),
        };
        let sixteen = symphony_board::server::postgres::hosted_programs_16();
        Environment {
            system: system(bound),
            chrome,
            postgresql,
            postgresql_16: sixteen_at(&sixteen, bound),
        }
    }

    /// It on one line, as a run's output and a carried green's record say it.
    pub(crate) fn said(&self) -> String {
        let sixteen = match self.postgresql_16.as_str() {
            "" => UNRECORDED,
            said => said,
        };
        format!(
            "{}; Chrome: {}; PostgreSQL: {}; the hosted check's PostgreSQL 16: {sixteen}",
            self.system, self.chrome, self.postgresql
        )
    }
}

/// The version the `postgres` in `folder` says, or why it does not: the hosted check's
/// PostgreSQL 16, which is there or the check is red.
fn sixteen_at(folder: &Path, bound: Bound<'_>) -> String {
    version(&folder.join("postgres").to_string_lossy(), bound)
}

/// What `program ARGS` printed, its first line trimmed; none when it failed or printed nothing.
fn first_line(program: &str, args: &[&str], bound: Bound<'_>) -> Result<String, String> {
    let mut asked = command(program);
    asked.args(args);
    let done = output_until(&mut asked, bound.sooner(BOUND)).map_err(|failed| match failed {
        Failed::Io(e) => e.to_string(),
        Failed::Late(_) => "it did not answer in time".to_string(),
        Failed::Interrupted(_) => "the run was told to stop".to_string(),
    })?;
    if !done.status.success() {
        return Err(format!("{}", done.status));
    }
    let said = String::from_utf8_lossy(&done.stdout);
    match said.lines().map(str::trim).find(|line| !line.is_empty()) {
        Some(line) => Ok(line.to_string()),
        None => Err("it printed nothing".into()),
    }
}

/// The version `program --version` says, or that it does not, with why.
fn version(program: &str, bound: Bound<'_>) -> String {
    first_line(program, &["--version"], bound)
        .unwrap_or_else(|why| format!("{program} does not say its version ({why})"))
}

/// The system and its version: macOS's product version where `sw_vers` says one, then the kernel's
/// name, release and machine as `uname` says them.
fn system(bound: Bound<'_>) -> String {
    let kernel =
        first_line("uname", &["-srm"], bound).unwrap_or_else(|why| format!("unknown ({why})"));
    match first_line("sw_vers", &["-productVersion"], bound) {
        Ok(product) => format!("macOS {product}, {kernel}"),
        Err(_) => kernel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release::waits::far;
    use crate::test_tree::Tree;

    fn input(name: &str, digest: &str) -> Input {
        Input {
            name: name.into(),
            digest: digest.into(),
        }
    }

    /// A checkout whose inputs script is `body`.
    fn checkout(body: &str) -> Tree {
        let tree = Tree::new();
        tree.write(SCRIPT, body);
        tree
    }

    #[test]
    fn each_line_is_a_name_and_its_digest() {
        assert_eq!(
            parse("verify/lint\tabc\nverify/tests/browser\t0f\n"),
            Ok(vec![
                input("verify/lint", "abc"),
                input("verify/tests/browser", "0f")
            ])
        );
        assert_eq!(parse(""), Ok(Vec::new()));
    }

    #[test]
    fn a_line_that_is_no_name_and_digest_or_a_name_said_twice_reads_as_nothing() {
        for (bad, why) in [
            ("verify/lint", "line 1, \"verify/lint\", is not a name"),
            ("verify/lint\t", "line 1"),
            ("\tabc", "line 1"),
            ("a\tb\n\n", "line 2, \"\""),
            ("a\tb c", "line 1"),
            ("a\tb\tc", "line 1"),
            ("a\tb\na\tc", "`a` is said twice"),
        ] {
            let said = parse(bad).unwrap_err();
            assert!(said.contains(why), "{bad:?}: {said}");
        }
    }

    #[test]
    fn the_inputs_told_are_found_by_name_and_by_what_they_are_under() {
        let inputs = Inputs {
            told: vec![
                input("verify/lint", "1"),
                input("verify/tests/browser", "2"),
                input("verify/tests/perf", "3"),
                input("verify/tests", "4"),
                input("verifying/x", "5"),
            ],
            error: None,
        };
        assert_eq!(inputs.of("verify/tests/perf"), Some("3"));
        assert_eq!(inputs.of("verify/size-check"), None);
        assert_eq!(inputs.under("verify"), ["verify/lint", "verify/tests"]);
        assert_eq!(
            inputs.under("verify/tests"),
            ["verify/tests/browser", "verify/tests/perf"]
        );
        assert!(inputs.under("verify/lint").is_empty());
        // A step told only by its parts is a name under its check all the same, once.
        let lanes = Inputs {
            told: vec![
                input("verify/tests/browser", "2"),
                input("verify/tests/perf", "3"),
                input("verify/", "4"),
            ],
            error: None,
        };
        assert_eq!(lanes.under("verify"), ["verify/tests"]);
    }

    #[test]
    fn a_tree_without_the_script_names_no_inputs() {
        let tree = Tree::new();
        assert_eq!(ask(&tree.root, far()), None);
    }

    #[test]
    fn the_script_is_asked_in_the_checkout_and_what_it_prints_is_told() {
        let tree = checkout("printf 'verify/lint\\t%s\\n' \"$(basename \"$PWD\")\"\n");
        let folder = tree.root.file_name().unwrap().to_string_lossy();
        assert_eq!(
            ask(&tree.root, far()),
            Some(Inputs {
                told: vec![input("verify/lint", &folder)],
                error: None
            })
        );
    }

    #[test]
    fn a_script_that_fails_or_prints_what_does_not_read_tells_none_and_says_why() {
        let failed = ask(
            &checkout("echo 'no package x' >&2\necho 'verify/lint\tabc'\nexit 2\n").root,
            far(),
        )
        .unwrap();
        assert!(failed.told.is_empty(), "{failed:?}");
        let why = failed.error.unwrap();
        assert!(
            why.starts_with("tools/check-inputs.sh failed (exit status: 2): no package x"),
            "{why}"
        );
        let unread = ask(&checkout("echo 'verify/lint abc'\n").root, far()).unwrap();
        assert!(unread.told.is_empty(), "{unread:?}");
        assert_eq!(
            unread.error.as_deref(),
            Some(
                "what tools/check-inputs.sh printed does not read: line 1, \"verify/lint abc\", \
                 is not a name, a tab and a digest"
            )
        );
    }

    #[test]
    fn a_script_past_its_bound_is_stopped_and_tells_none() {
        let tree = checkout("exec sleep 600\n");
        let past = Bound::until(std::time::Instant::now());
        let late = ask(&tree.root, past).unwrap();
        assert!(late.told.is_empty());
        assert!(
            late.error
                .as_deref()
                .is_some_and(|why| why.contains("did not end within")),
            "{late:?}"
        );
    }

    #[test]
    fn the_environment_names_the_system_and_each_program_or_why_it_cannot() {
        let tree = Tree::new();
        let environment = Environment::now(&tree.root, far());
        assert!(!environment.system.is_empty(), "{environment:?}");
        assert!(!environment.chrome.is_empty() && !environment.postgresql.is_empty());
        assert!(!environment.postgresql_16.is_empty(), "{environment:?}");
        assert_eq!(
            environment.said(),
            format!(
                "{}; Chrome: {}; PostgreSQL: {}; the hosted check's PostgreSQL 16: {}",
                environment.system,
                environment.chrome,
                environment.postgresql,
                environment.postgresql_16
            )
        );
    }

    /// A folder of `tree` whose `postgres` says `version`.
    fn postgres(tree: &Tree, folder: &str, version: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        let program = format!("{folder}/postgres");
        tree.write(&program, &format!("#!/bin/sh\necho '{version}'\n"));
        let path = tree.root.join(&program);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        tree.root.join(folder)
    }

    #[test]
    fn another_postgresql_16_is_another_environment_with_the_same_18_and_tree() {
        let tree = Tree::new();
        let on = |sixteen: &Path| Environment {
            system: "Stand-in 1".into(),
            chrome: "Chrome 2".into(),
            postgresql: "postgres (PostgreSQL) 18.7".into(),
            postgresql_16: sixteen_at(sixteen, far()),
        };
        let was = on(&postgres(&tree, "was", "postgres (PostgreSQL) 16.16"));
        let upgraded = on(&postgres(&tree, "upgraded", "postgres (PostgreSQL) 16.17"));
        assert_eq!(was.postgresql_16, "postgres (PostgreSQL) 16.16");
        assert_eq!(was.postgresql, upgraded.postgresql);
        assert_ne!(was.said(), upgraded.said());
        assert_eq!(was.said(), on(&tree.root.join("was")).said());
        assert!(
            was.said()
                .ends_with("; the hosted check's PostgreSQL 16: postgres (PostgreSQL) 16.16"),
            "{}",
            was.said()
        );
        // None installed is said, with why, and is another environment than one that has it.
        let none = on(&tree.root.join("none"));
        assert!(
            none.postgresql_16.contains("does not say its version ("),
            "{none:?}"
        );
        assert_ne!(none.said(), was.said());
    }

    #[test]
    fn a_record_written_before_the_16_was_kept_reads_and_is_no_environment_of_now() {
        let old: Environment = serde_json::from_str(
            r#"{"system":"Stand-in 1","chrome":"Chrome 2","postgresql":"postgres 18"}"#,
        )
        .unwrap();
        assert_eq!(old.postgresql_16, "");
        assert_eq!(
            old.said(),
            "Stand-in 1; Chrome: Chrome 2; PostgreSQL: postgres 18; the hosted check's \
             PostgreSQL 16: not recorded"
        );
        // Whatever a machine's 16 says now, or that it has none, it is never what the old
        // record says: a green that stood on it is carried nowhere.
        let tree = Tree::new();
        for now in [
            sixteen_at(
                &postgres(&tree, "has", "postgres (PostgreSQL) 16.16"),
                far(),
            ),
            sixteen_at(&tree.root.join("none"), far()),
        ] {
            let now = Environment {
                postgresql_16: now,
                ..old.clone()
            };
            assert_ne!(now.said(), old.said());
        }
        // What is written now holds the field, and reads back as itself.
        let kept = Environment {
            postgresql_16: "postgres (PostgreSQL) 16.16".into(),
            ..old
        };
        let read: Environment =
            serde_json::from_str(&serde_json::to_string(&kept).unwrap()).unwrap();
        assert_eq!(read, kept);
    }

    #[test]
    fn a_program_says_its_version_on_its_first_line_or_why_it_does_not() {
        let tree = Tree::new();
        tree.write(
            "says",
            "#!/bin/sh\necho\necho '  Stand-in 1.2  '\necho more\n",
        );
        tree.write("silent", "#!/bin/sh\n");
        tree.write("fails", "#!/bin/sh\necho x\nexit 3\n");
        for name in ["says", "silent", "fails"] {
            use std::os::unix::fs::PermissionsExt as _;
            let path = tree.root.join(name);
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let at = |name: &str| tree.root.join(name).to_string_lossy().into_owned();
        assert_eq!(version(&at("says"), far()), "Stand-in 1.2");
        assert_eq!(
            version(&at("silent"), far()),
            format!(
                "{} does not say its version (it printed nothing)",
                at("silent")
            )
        );
        assert_eq!(
            version(&at("fails"), far()),
            format!("{} does not say its version (exit status: 3)", at("fails"))
        );
        let missing = version(&at("missing"), far());
        assert!(missing.contains("does not say its version ("), "{missing}");
    }
}
