//! What reruns a red check's failures alone (RULES.md `tests/rerun-the-failure-alone`), read from
//! its log in one pass ([`read`]): each test target cargo said failed (`error: test failed, to
//! rerun pass `ARGS``, a doctest's and a bench's alike; the tests' lanes say a red lane that
//! listed no target of its own in those words, its targets the arguments), with the tests libtest
//! named `FAILED` since the target before it or since cargo started a target (`Running …`,
//! `Doc-tests …`), whichever came last, so a test named by what ended with no such line (a lane
//! killed) is no other target's, and each recipe just said failed (`error: recipe `NAME` failed
//! …`). A target that ended naming none, as one that crashed does, is rerun whole. A `just verify`
//! that listed no steps (one older than its steps' list) and failed in no test is rerun by the
//! step its recipes name ([`step`]): its lint, its client check or its size budget, each by its own
//! recipe; its docs audit's recipe says nothing when it fails (`[no-exit-message]`), so no line
//! names that one. A step's red is read from its own log. A test named before the log ended with
//! no target after it (a run stopped at its time) is left to the log's tail.
//!
//! A target that found files its own build had made gone (its executable, a library it links or a
//! build script's output: cargo's `could not execute process … (never executed)` for a missing
//! file, rustc's `extern location for … does not exist` and `couldn't find file` in an `OUT_DIR`)
//! and named no failed test failed for its build folder, not for a test of its own: it is no
//! failure to rerun, and the log says while which target the files went ([`Went`]), or, in a run
//! of the tests' lanes side by side (`tools/test-lanes.sh`, whose lines before any lane starts name
//! each lane's log), while which lanes: their outputs follow one another in the log, but they ran
//! at the same time, so the target before in the log's order is no target that ran before.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// A test target cargo said failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Failed {
    /// The arguments that select it, as cargo said them: `-p symphony --test release`.
    pub(crate) target: String,
    /// The tests libtest named failed in it, in the order it named them.
    pub(crate) tests: Vec<String>,
    /// The files its own build had made that it found gone, as the log names them, once each: it
    /// never ran or never compiled for them, and named no failed test. None when it failed by
    /// itself.
    pub(crate) gone: Vec<String>,
}

/// When the files a check's targets found gone went: while its build ran, before any target ran,
/// while a target ran, the last one cargo started before the first that found one gone, or while
/// the tests' lanes ran side by side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Went {
    Building,
    /// The target as cargo's line starting it names it (`Running tests/a.rs (…/a-0123)`, `Doc-tests
    /// a`), and the arguments that select it, when the log says its package: its own failure, or
    /// the failed targets on either side of it, of one package.
    While(String, Option<String>),
    /// The lanes the tests' runner said it ran (`tools/test-lanes.sh`), by name in its order, and
    /// how many it said run at once, more than one: found gone after its lines naming them, so
    /// while they ran, any of them and in no order the log can tell.
    Lanes(Vec<String>, usize),
}

/// What the tests' runner says before any lane starts: how many lanes run at once, and the log of
/// each, by whose name the lane is named.
#[derive(Default)]
struct Lanes {
    /// How many run at once, once its first line said it.
    at_once: Option<usize>,
    /// Each lane's name, in its order.
    names: Vec<String>,
    /// Whether the line read last was one of its lines, so the next may name a lane.
    listing: bool,
}

impl Lanes {
    /// Reads `line` as the runner's, when it is: its first line only before any target started
    /// (`started`), so a test printing one is not taken, then each lane's line right after it.
    fn read(&mut self, line: &str, started: bool) -> bool {
        if self.listing
            && let Some(name) = lane(line)
        {
            self.names.push(name);
            return true;
        }
        self.listing = false;
        if self.at_once.is_none() && !started {
            self.at_once = at_once(line);
            self.listing = self.at_once.is_some();
        }
        self.listing
    }

    /// Whether the lanes said ran side by side: more than one, more than one at once. Lanes run
    /// one at a time ran one after another in the order their outputs follow in the log.
    fn side_by_side(&self) -> bool {
        self.at_once.is_some_and(|at_once| at_once > 1) && self.names.len() > 1
    }
}

/// How many lanes the tests' runner says run at once: N of `test-lanes: lanes run at most N at
/// once (…), …`.
fn at_once(line: &str) -> Option<usize> {
    let rest = line.strip_prefix("test-lanes: lanes run at most ")?;
    rest.split_once(" at once")?.0.parse().ok()
}

/// The lane the tests' runner's line names: NAME of `test-lanes:     …/NAME.log (TARGETS)`.
fn lane(line: &str) -> Option<String> {
    let rest = line.strip_prefix("test-lanes:    ")?;
    let (log, targets) = rest.trim_start().rsplit_once(" (")?;
    let name = Path::new(log).file_name()?.to_str()?.strip_suffix(".log")?;
    (!name.is_empty() && targets.ends_with(')')).then(|| name.to_string())
}

/// What a red check's log says failed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Log {
    /// Each test target cargo said failed, in the log's order.
    pub(crate) targets: Vec<Failed>,
    /// Each recipe just said failed, in the log's order: the innermost first, as just says them.
    pub(crate) recipes: Vec<String>,
    /// When the files its targets found gone went; none when none found any.
    pub(crate) went: Option<Went>,
}

/// What follows a target's failure in cargo's line, before the arguments that rerun it.
const RERUN: &str = ", to rerun pass `";

/// The part of the log one target wrote: from cargo's line starting it to the next.
#[derive(Default)]
struct Segment {
    /// Cargo's line starting it, trimmed; none for what the log says before any target started.
    header: Option<String>,
    /// The index in [`Log::targets`] of the first failure said in it.
    from: usize,
    /// The files of the build it found gone.
    gone: Vec<String>,
    /// A process cargo could not execute, until the log says why.
    unexecuted: Option<String>,
}

/// What the log at `log` says failed; nothing when it cannot be read.
pub(crate) fn read(log: &Path) -> Log {
    let Ok(file) = File::open(log) else {
        return Log::default();
    };
    let mut reader = BufReader::new(file);
    let (mut found, mut tests) = (Log::default(), Vec::new());
    let mut segment = Segment::default();
    // The segment before the current one, and where the files gone were first found: in the
    // segment after which one (with where the one that found them starts), none before any
    // target started, or while lanes ran side by side.
    let mut previous: Option<(String, usize)> = None;
    let mut first_gone: Option<First> = None;
    let mut lanes = Lanes::default();
    let mut nextest = super::failures::Reading::default();
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end_matches(['\n', '\r']);
        nextest.line(text);
        for failure in nextest.take_nextest_failures() {
            nextest_failed(&mut found, failure);
        }
        let started = segment.header.is_some() || !found.targets.is_empty();
        if lanes.read(text, started) {
            continue;
        }
        if let Some(name) = failed_test(text) {
            if !tests.contains(&name) {
                tests.push(name);
            }
        } else if let Some(target) = target(text) {
            found.targets.push(Failed {
                target,
                tests: std::mem::take(&mut tests),
                gone: Vec::new(),
            });
        } else if let Some(recipe) = recipe(text) {
            found.recipes.push(recipe);
        } else if let Some(header) = header(text) {
            // Tests named before a target started are no failures of it: what named them ended
            // with no line of its own (a lane killed while it ran).
            tests.clear();
            close(&mut found, &segment);
            previous = segment.header.take().map(|header| (header, segment.from));
            segment = Segment {
                header: Some(header),
                from: found.targets.len(),
                ..Segment::default()
            };
        } else if let Some(file) = gone(text, &mut segment.unexecuted) {
            if !segment.gone.contains(&file) {
                segment.gone.push(file);
            }
            if first_gone.is_none() {
                let after = (segment.header.is_some())
                    .then(|| previous.clone())
                    .flatten()
                    .map(|(header, from)| First::After(header, from, segment.from));
                first_gone = Some(if lanes.side_by_side() {
                    First::Beside
                } else {
                    after.unwrap_or(First::Building)
                });
            }
        }
    }
    nextest.end();
    for failure in nextest.take_nextest_failures() {
        nextest_failed(&mut found, failure);
    }
    close(&mut found, &segment);
    found.went = first_gone.map(|first| match first {
        First::Building => Went::Building,
        First::After(header, from, until) => {
            let args = selected(&found.targets, &header, from, until);
            Went::While(header, args)
        }
        First::Beside => Went::Lanes(lanes.names, lanes.at_once.unwrap_or_default()),
    });
    found
}

/// Each nextest case belongs to its metadata-selected Cargo target and is rerun once.
fn nextest_failed(found: &mut Log, failure: super::nextest::Failure) {
    if let Some(target) = found
        .targets
        .iter_mut()
        .find(|f| f.target == failure.target)
    {
        if !target.tests.contains(&failure.name) {
            target.tests.push(failure.name);
        }
    } else {
        found.targets.push(Failed {
            target: failure.target,
            tests: vec![failure.name],
            gone: Vec::new(),
        });
    }
}

/// Where a log first says files of its build gone.
enum First {
    /// Before any target started, or in the first one cargo started.
    Building,
    /// In the segment after the one cargo's line `.0` starts, whose failures start at `.1`, the
    /// one that found them gone at `.2`.
    After(String, usize, usize),
    /// After the tests' runner said its lanes, run side by side.
    Beside,
}

/// The files `segment` found gone given to each failure said in it that named no failed test.
fn close(found: &mut Log, segment: &Segment) {
    if segment.gone.is_empty() {
        return;
    }
    for failed in found.targets.iter_mut().skip(segment.from) {
        if failed.tests.is_empty() {
            failed.gone.clone_from(&segment.gone);
        }
    }
}

/// The line cargo starts a target with, trimmed: `Running unittests src/lib.rs (…)`, `Running
/// tests/a.rs (…)`, `Doc-tests NAME`.
pub(super) fn header(line: &str) -> Option<String> {
    let line = line.trim();
    let starts = (line.strip_prefix("Running "))
        .is_some_and(|rest| rest.ends_with(')') && rest.contains(" ("))
        || line
            .strip_prefix("Doc-tests ")
            .is_some_and(|name| !name.is_empty() && !name.contains(' '));
    starts.then(|| line.to_string())
}

/// The file of a build that `line` says is gone: a process cargo could not execute, once the log
/// says it was no such file (`unexecuted` holds it until then), a library rustc was told to link
/// and found missing, or a file a build script made (in its `OUT_DIR`, `…/build/UNIT/out/`) that
/// rustc could not find.
fn gone(line: &str, unexecuted: &mut Option<String>) -> Option<String> {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix("could not execute process `") {
        *unexecuted = rest
            .strip_suffix("` (never executed)")
            .map(|said| said.split(' ').next().unwrap_or(said).to_string());
        return None;
    }
    if line == "No such file or directory (os error 2)" {
        return unexecuted.take();
    }
    if let Some(rest) = line.strip_prefix("error: extern location for ") {
        let (_, file) = rest.split_once(" does not exist: ")?;
        return (!file.is_empty()).then(|| file.to_string());
    }
    let file = (line.strip_prefix("error: couldn't find file `"))
        .and_then(|rest| rest.strip_suffix('`'))
        .or_else(|| {
            (line.strip_prefix("error: couldn't read `"))
                .and_then(|rest| rest.split_once("`: No such file or directory"))
                .map(|(file, _)| file)
        })?;
    let parts: Vec<&str> = file.split('/').collect();
    let made = parts
        .windows(3)
        .any(|three| three[0] == "build" && three[2] == "out");
    made.then(|| file.to_string())
}

/// The arguments that select the target cargo started with `header`, whose failures are said in
/// `failed` from `from` until `until`, where the next target's start: its own failure's, or, when
/// it failed in nothing, the package the failures on either side of it share, with the target its
/// line names.
fn selected(failed: &[Failed], header: &str, from: usize, until: usize) -> Option<String> {
    if from < until {
        return failed.get(from).map(|own| own.target.clone());
    }
    let before = package(&failed.get(from.checked_sub(1)?)?.target)?;
    if package(&failed.get(from)?.target) != Some(before) {
        return None;
    }
    let kind = target_of(header)?;
    Some(format!("-p {before} {kind}"))
}

/// The package the arguments `target` select a target of: PKG of `-p PKG …`.
fn package(target: &str) -> Option<&str> {
    let mut words = target.split_whitespace();
    (words.next() == Some("-p")).then(|| words.next()).flatten()
}

/// What selects, within its package, the target cargo's line `header` starts: `--lib`, `--bin
/// NAME`, `--test NAME`, `--bench NAME` (NAME the executable's, without its hash) or `--doc`.
fn target_of(header: &str) -> Option<String> {
    if header.starts_with("Doc-tests ") {
        return Some("--doc".into());
    }
    let rest = header.strip_prefix("Running ")?;
    let (source, executable) = rest.rsplit_once(" (")?;
    let file = executable.strip_suffix(')')?.rsplit('/').next()?;
    let name = file.rsplit_once('-').map_or(file, |(name, _)| name);
    let source = source.strip_prefix("unittests ").unwrap_or(source);
    if source == "src/lib.rs" {
        return Some("--lib".into());
    }
    let bin = (source.strip_prefix("src/bin/")).map(|rest| {
        rest.split('/')
            .next()
            .unwrap_or(rest)
            .trim_end_matches(".rs")
    });
    if let Some(bin) = bin.or_else(|| (source == "src/main.rs").then_some(name)) {
        return Some(format!("--bin {bin}"));
    }
    let kind = match source.split('/').next()? {
        "tests" => "--test",
        "benches" => "--bench",
        _ => return None,
    };
    Some(format!("{kind} {name}"))
}

/// The recipe a just line says failed: NAME of `error: recipe `NAME` failed …`, `Recipe` as an
/// older just writes it.
pub(super) fn recipe(line: &str) -> Option<String> {
    let rest = line.strip_prefix("error: ")?;
    let rest = (rest.strip_prefix("recipe `")).or_else(|| rest.strip_prefix("Recipe `"))?;
    let (name, after) = rest.split_once('`')?;
    (after.starts_with(" failed") && !name.is_empty()).then(|| name.to_string())
}

/// The command that reruns alone the step of a `just verify` that lists none that `recipes`
/// ([`Log::recipes`]) say failed: `just NAME` for the last of them, the one nearest `verify` as
/// just names them, that is neither verify's own (`verify`, `_verify`) nor private (`_NAME`), so a
/// step is named by the recipe the justfile gives it and by no list of its own here; none when no
/// such recipe is there.
pub(crate) fn step(recipes: &[String]) -> Option<String> {
    (recipes.iter().rev())
        .find(|recipe| recipe.as_str() != "verify" && !recipe.starts_with('_'))
        .map(|recipe| format!("just {recipe}"))
}

/// The test a libtest line names failed, `test NAME ... FAILED`, a `should panic` test's name
/// without that note.
pub(super) fn failed_test(line: &str) -> Option<String> {
    let name = line.strip_prefix("test ")?.strip_suffix(" ... FAILED")?;
    let name = name.strip_suffix(" - should panic").unwrap_or(name);
    (!name.trim().is_empty()).then(|| name.to_string())
}

/// The arguments a cargo line says rerun a failed target: ARGS of
/// `error: test failed, to rerun pass `ARGS``.
pub(super) fn target(line: &str) -> Option<String> {
    let (_, args) = line.strip_prefix("error: ")?.split_once(RERUN)?;
    let args = args.strip_suffix('`')?;
    (!args.trim().is_empty() && !args.contains('`')).then(|| args.to_string())
}

/// The cargo test arguments that rerun each failure of `failed` alone: `ARGS -- NAME --exact` for
/// each test it named, and `ARGS` for a target that named none, but one that failed for files of
/// its build gone, which no rerun of its own explains. The live fixture's explicit selection
/// supersedes its library fallback; duplicate commands appear once and it keeps `--ignored`.
pub(crate) fn arguments(failed: &[Failed]) -> Vec<String> {
    let mut each = Vec::new();
    let mut seen = HashSet::new();
    let live_selection = format!(
        "-p symphony-build --lib -- --ignored {} --exact",
        super::nextest::LIVE_FIXTURE
    );
    let mut add = |args: String| {
        if seen.insert(args.clone()) {
            each.push(args);
        }
    };
    for target in failed {
        if target.tests.is_empty() && target.gone.is_empty() {
            // The explicit fixture's runner names its failed selection even if the process died
            // without naming a case. Other targets retain their ordinary whole-target fallback.
            let live = target.target == "-p symphony-build --lib"
                && failed.iter().any(|other| other.target == live_selection);
            if !live {
                add(target.target.clone());
            }
        }
        for test in &target.tests {
            let ignored = if target.target == "-p symphony-build --lib"
                && test == super::nextest::LIVE_FIXTURE
            {
                "--ignored "
            } else {
                ""
            };
            add(format!(
                "{} -- {ignored}{} --exact",
                target.target,
                quoted(test)
            ));
        }
    }
    each
}

/// `word` as a shell reads it back: as it is when it holds nothing a shell reads apart, else in
/// single quotes.
pub(super) fn quoted(word: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "_:./-".contains(c);
    if !word.is_empty() && word.chars().all(plain) {
        return word.to_string();
    }
    format!("'{}'", word.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_tree::Tree;

    /// What cargo 1.99 printed for `cargo test --no-fail-fast` in a workspace: a library whose
    /// tests failed (one a `should panic`), an integration target with two, one that crashed
    /// naming none, a passing one and a doctest, then its summary, which repeats the targets.
    const LOG: &str = "   Compiling symphony v0.1.0 (/wt/nightly/crates/symphony)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.77s
     Running unittests src/lib.rs (.target/debug/deps/symphony_board-c55f883224b5f683)

running 3 tests
test tests::lib_passes ... ok
test tests::should_but_does_not - should panic ... FAILED
test tests::lib_fails ... FAILED

failures:

---- tests::lib_fails stdout ----
assertion `left == right` failed

failures:
    tests::lib_fails
    tests::should_but_does_not

test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

error: test failed, to rerun pass `-p symphony-board --lib`
     Running tests/nightly/main.rs (.target/debug/deps/nightly-dfaaafee719b0dda)

running 3 tests
test reds::passes ... ok
test reds::one_fails ... FAILED
test reds::two_fails ... FAILED

test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

error: test failed, to rerun pass `-p symphony --test nightly`
     Running tests/beta.rs (.target/debug/deps/beta-81cdc7807bf4ce06)

running 1 test
error: test failed, to rerun pass `-p symphony --test beta`

Caused by:
  process didn't exit successfully: `.target/debug/deps/beta-81cdc7807bf4ce06` (signal: 6, SIGABRT: process abort signal)
     Running tests/green.rs (.target/debug/deps/green-0123)

running 1 test
test fine ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests symphony

running 1 test
test src/lib.rs - f (line 1) ... FAILED

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

error: doctest failed, to rerun pass `-p symphony --doc`
error: 4 targets failed:
    `-p symphony-board --lib`
    `-p symphony --test nightly`
    `-p symphony --test beta`
    `-p symphony --doc`
";

    fn read_in(text: &str) -> Log {
        let tree = Tree::new();
        read(&tree.write("verify.log", text))
    }

    fn failed_in(text: &str) -> Vec<Failed> {
        read_in(text).targets
    }

    fn recipes(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    /// The end of the nightly's verify log of 2026-11-12, a verify that listed no steps, red in its
    /// size budget: the step's own recipe, the public one it runs through, and verify.
    const SIZE: &str = "    Finished `release` profile [optimized] target(s) in 17m 36s
size-budget: agent-send       4.9 MiB of 5.0 MiB
size-budget: symphony is 18.8 MiB (19,719,136 bytes), 0.5 MiB past its budget of 18.3 MiB (19,200,000 bytes) in tools/size-budget.toml.
size-budget: tasks            23.2 MiB of 23.6 MiB
error: recipe `_size-check` failed on line 101 with exit code 1
error: recipe `size-check` failed on line 96 with exit code 1
error: recipe `verify` failed on line 105 with exit code 1
";

    #[test]
    fn a_red_step_of_verify_is_rerun_by_its_own_command() {
        let size = read_in(SIZE);
        assert_eq!(
            size,
            Log {
                targets: Vec::new(),
                recipes: recipes(&["_size-check", "size-check", "verify"]),
                went: None,
            }
        );
        assert_eq!(step(&size.recipes).as_deref(), Some("just size-check"));
        for (said, command) in [
            (&["_lint-rs", "lint-rs", "verify"][..], "just lint-rs"),
            (
                &["_client-check", "client-check", "_verify", "verify"],
                "just client-check",
            ),
        ] {
            assert_eq!(step(&recipes(said)).as_deref(), Some(command), "{said:?}");
        }
        // The tests' step and the docs audit (whose recipe says nothing), verify itself and no
        // recipe at all name no step.
        for said in [&["_verify", "verify"][..], &["verify"], &[]] {
            assert_eq!(step(&recipes(said)), None, "{said:?}");
        }
        // An older just's capital, and lines that only look like one.
        assert_eq!(
            read_in("error: Recipe `lint-rs` failed on line 21 with exit code 101\n").recipes,
            ["lint-rs"]
        );
        for line in [
            "error: recipe `` failed on line 1",
            "error: recipe `lint-rs` succeeded",
            "warning: recipe `lint-rs` failed on line 1",
            "  error: recipe `lint-rs` failed on line 1",
        ] {
            assert_eq!(read_in(line).recipes, Vec::<String>::new(), "{line}");
        }
    }

    fn target(target: &str, tests: &[&str]) -> Failed {
        Failed {
            tests: tests.iter().map(|test| (*test).to_string()).collect(),
            ..gone_from(target, &[])
        }
    }

    fn gone_from(target: &str, files: &[&str]) -> Failed {
        Failed {
            target: target.into(),
            tests: Vec::new(),
            gone: files.iter().map(|file| (*file).to_string()).collect(),
        }
    }

    /// The shape of a test run's log of 2026-11-12 (run 2026-11-12T2221-57549), cut: a test of
    /// its own emptied the build folder while it ran (`dependency_boundary`), so every
    /// target cargo started after it found its executable gone, and the doctests a library they
    /// link or a build script's output. Two targets failed by a test of their own before that.
    const EMPTIED: &str = "    Finished `test` profile [unoptimized + debuginfo] target(s) in 1m 43s
     Running unittests src/lib.rs (.target/debug/deps/app-18768e759f4fccee)

running 9 tests
test mesh::fidelity::tests::mount ... ok
     Running tests/golden_card.rs (.target/debug/deps/golden_card-87ae9d016e3016ae)

running 2 tests
test help_usage ... ok
test shared_app_discovers_branch_card ... FAILED

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.59s

error: test failed, to rerun pass `-p app --test golden_card`
     Running tests/after_tool_runner.rs (.target/debug/deps/after_tool_runner-6586517a5c7842d6)

running 1 test
test tool_runner_the_real_catalogue_agrees_with_the_budget_table ... FAILED

error: test failed, to rerun pass `-p gate --test after_tool_runner`
     Running tests/dependency_boundary.rs (.target/debug/deps/dependency_boundary-c5f610d098084456)

running 1 test
test wrappers_build_without_the_symphony_app_or_board ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 295.44s

     Running tests/failure_capture.rs (.target/debug/deps/failure_capture-c2d3a9b32caa50f6)
error: test failed, to rerun pass `-p gate --test failure_capture`

Caused by:
  could not execute process `/c/sf/.target/debug/deps/failure_capture-c2d3a9b32caa50f6` (never executed)

Caused by:
  No such file or directory (os error 2)
     Running tests/gate/main.rs (.target/debug/deps/gate-22883f8af040c755)
error: test failed, to rerun pass `-p gate --test gate`

Caused by:
  could not execute process `/c/sf/.target/debug/deps/gate-22883f8af040c755` (never executed)

Caused by:
  No such file or directory (os error 2)
   Doc-tests app
error: extern location for app_checks does not exist: /c/sf/.target/debug/deps/libapp_checks-7fe7.rlib
  --> tools/app/src/lib.rs:10:9
error: extern location for serde_json does not exist: /c/sf/.target/debug/deps/libserde_json-ca7c.rlib
error[E0432]: unresolved import `crate::mesh::Result`
error: extern location for app_checks does not exist: /c/sf/.target/debug/deps/libapp_checks-7fe7.rlib
error: aborting due to 3 previous errors

error: doctest failed, to rerun pass `-p app --doc`

Caused by:
  process didn't exit successfully: `rustdoc --edition=2024 --crate-name app --test tools/app/src/lib.rs` (exit status: 1)
   Doc-tests content_index
error: couldn't find file `/c/sf/.target/debug/build/content-index-c3c4/out/index.rs`
  --> crates/content-index/src/lib.rs:38:1
error: aborting due to 1 previous error

error: doctest failed, to rerun pass `-p content-index --doc`
error: 6 targets failed:
    `-p app --test golden_card`
    `-p gate --test after_tool_runner`
    `-p gate --test failure_capture`
    `-p gate --test gate`
    `-p app --doc`
    `-p content-index --doc`
";

    #[test]
    fn targets_that_found_their_builds_files_gone_are_told_from_failed_tests() {
        let log = read_in(EMPTIED);
        assert_eq!(
            log.targets,
            [
                target(
                    "-p app --test golden_card",
                    &["shared_app_discovers_branch_card"]
                ),
                target(
                    "-p gate --test after_tool_runner",
                    &["tool_runner_the_real_catalogue_agrees_with_the_budget_table"]
                ),
                gone_from(
                    "-p gate --test failure_capture",
                    &["/c/sf/.target/debug/deps/failure_capture-c2d3a9b32caa50f6"]
                ),
                gone_from(
                    "-p gate --test gate",
                    &["/c/sf/.target/debug/deps/gate-22883f8af040c755"]
                ),
                gone_from(
                    "-p app --doc",
                    &[
                        "/c/sf/.target/debug/deps/libapp_checks-7fe7.rlib",
                        "/c/sf/.target/debug/deps/libserde_json-ca7c.rlib"
                    ]
                ),
                gone_from(
                    "-p content-index --doc",
                    &["/c/sf/.target/debug/build/content-index-c3c4/out/index.rs"]
                ),
            ]
        );
        assert_eq!(
            log.went,
            Some(Went::While(
                "Running tests/dependency_boundary.rs \
                 (.target/debug/deps/dependency_boundary-c5f610d098084456)"
                    .into(),
                Some("-p gate --test dependency_boundary".into())
            )),
            "they went while the target cargo started last before the first that found one gone"
        );
        assert_eq!(
            arguments(&log.targets),
            [
                "-p app --test golden_card -- shared_app_discovers_branch_card --exact",
                "-p gate --test after_tool_runner -- \
                 tool_runner_the_real_catalogue_agrees_with_the_budget_table --exact",
            ],
            "only the failed tests are rerun"
        );
    }

    #[test]
    fn a_test_run_in_lanes_reruns_each_red_lanes_failed_tests_and_nothing_a_test_printed() {
        // tools/test-lanes.sh: each lane's output whole, after the build's `Executable` lines, a
        // test printing a list of its own, each red lane's list, then the lists as one.
        let log = read_in(include_str!("fixtures/tests-lanes-red.log"));
        assert_eq!(
            log.targets,
            [
                target("-p symphony-board --test sweep", &["land::a"]),
                target("-p symphony-app --test release", &["cut::b"]),
            ]
        );
        assert_eq!(log.went, None);
        assert_eq!(
            arguments(&log.targets),
            [
                "-p symphony-board --test sweep -- land::a --exact",
                "-p symphony-app --test release -- cut::b --exact",
            ]
        );
    }

    /// tools/test-lanes.sh's run with three red lanes: the sweep's killed after a test of it
    /// failed, the browser's cargo failing before any target and the board's in a target, listing
    /// it.
    const LANES_DEAD: &str = include_str!("fixtures/tests-lanes-dead.log");

    #[test]
    fn a_red_lane_that_listed_no_target_is_rerun_whole_but_by_the_tests_it_named_failed() {
        let log = read_in(LANES_DEAD);
        assert_eq!(
            log.targets,
            [
                target("--workspace --test sweep", &["land::a"]),
                target("--workspace --test browser", &[]),
                target("-p symphony-board --test cli", &[]),
            ]
        );
        assert_eq!(log.went, None);
        assert_eq!(
            arguments(&log.targets),
            [
                "--workspace --test sweep -- land::a --exact",
                "--workspace --test browser",
                "-p symphony-board --test cli",
            ]
        );
    }

    #[test]
    fn a_test_named_failed_before_a_target_started_is_no_failure_of_it() {
        // The same run without the lanes' own lines, as a runner that named no red lane printed
        // it: the sweep's failed test is no failure of the board's target that failed after it,
        // whose rerun by that name would run no test and read green.
        let unnamed: String = (LANES_DEAD.lines())
            .filter(|line| !line.starts_with("error: the lane "))
            .map(|line| format!("{line}\n"))
            .collect();
        assert_ne!(unnamed, LANES_DEAD);
        let log = read_in(&unnamed);
        assert_eq!(log.targets, [target("-p symphony-board --test cli", &[])]);
        assert_eq!(arguments(&log.targets), ["-p symphony-board --test cli"]);
        // A doctest's start too; a target's own failed tests after it are its.
        let text = "     Running tests/a.rs (.target/debug/deps/a-01)
test t ... FAILED
   Doc-tests p
test src/lib.rs - f (line 1) ... FAILED
error: doctest failed, to rerun pass `-p p --doc`
";
        assert_eq!(
            failed_in(text),
            [target("-p p --doc", &["src/lib.rs - f (line 1)"])]
        );
    }

    /// tools/test-lanes.sh's run whose `rest` lane found its targets' executables gone, and whose
    /// `doc` lane a library its doctests link: the target before them in the log's order is the
    /// `browser` lane's, which ran beside them, not before.
    const LANES_GONE: &str = include_str!("fixtures/tests-lanes-gone.log");

    fn names(lanes: &[&str]) -> Vec<String> {
        lanes.iter().map(|lane| (*lane).to_string()).collect()
    }

    #[test]
    fn files_found_gone_while_lanes_ran_side_by_side_went_while_those_lanes_ran() {
        let log = read_in(LANES_GONE);
        let deps = "/work/symphony-wt/release/.target/debug/deps";
        assert_eq!(
            log.targets,
            [
                gone_from(
                    "-p symphony-app --test after_commit",
                    &[&format!("{deps}/after_commit-4d5e6f708192a3b4")]
                ),
                gone_from(
                    "-p symphony-app --test standalone",
                    &[&format!("{deps}/standalone-5e6f708192a3b4c5")]
                ),
                gone_from(
                    "-p symphony-app --doc",
                    &[&format!("{deps}/libserde_json-ca7c.rlib")]
                ),
            ]
        );
        assert_eq!(
            log.went,
            Some(Went::Lanes(names(&["sweep", "browser", "rest", "doc"]), 8)),
            "never the browser lane's target, before them in the log's order"
        );
        assert_eq!(arguments(&log.targets), Vec::<String>::new());

        // Lanes run one at a time ran one after another in the log's order: read as a run without
        // lanes, by the target cargo started last before the first that found one gone.
        let one_at_a_time = LANES_GONE.replace(
            "lanes run at most 8 at once (16 processors online)",
            "lanes run at most 1 at once (no number of processors online)",
        );
        assert_ne!(one_at_a_time, LANES_GONE);
        let browser = "Running tests/browser/main.rs (.target/debug/deps/browser-5e6f7a8b9c0d1e2f)";
        assert_eq!(
            read_in(&one_at_a_time).went,
            Some(Went::While(browser.into(), None))
        );
        // So did the targets of a run whose runner said one lane.
        let one_lane = "test-lanes: lanes run at most 8 at once (16 processors online), each \
                        writing its log as it runs:
test-lanes:     /l/rest.log (--test a --test b)
     Running tests/a.rs (.target/debug/deps/a-01)
test t ... ok
     Running tests/b.rs (.target/debug/deps/b-02)
error: test failed, to rerun pass `-p p --test b`

Caused by:
  could not execute process `/t/debug/deps/b-02` (never executed)

Caused by:
  No such file or directory (os error 2)
";
        let a = "Running tests/a.rs (.target/debug/deps/a-01)";
        assert_eq!(read_in(one_lane).went, Some(Went::While(a.into(), None)));
        // The runner's lines said by a test, after a target started, are not the run's.
        let printed = format!(
            "     {a}\n{}",
            one_lane.replace(
                "test-lanes:     /l/rest.log (--test a --test b)\n     Running tests/a.rs \
                 (.target/debug/deps/a-01)\n",
                "test-lanes:     /l/a.log (--test a)\ntest-lanes:     /l/b.log (--test b)\n",
            )
        );
        assert!(printed.starts_with(&format!("     {a}\ntest-lanes: lanes run at most 8")));
        assert_eq!(read_in(&printed).went, Some(Went::While(a.into(), None)));
        // Files gone before the runner said its lanes went while it built.
        let building = format!(
            "error: extern location for a does not exist: /t/debug/deps/liba-1.rlib\n{LANES_GONE}"
        );
        assert_eq!(read_in(&building).went, Some(Went::Building));
    }

    #[test]
    fn the_runners_lines_name_each_lane_by_its_log_and_how_many_run_at_once() {
        for (line, at) in [
            (
                "test-lanes: lanes run at most 8 at once (16 processors online), each writing its \
                 log as it runs:",
                Some(8),
            ),
            (
                "test-lanes: lanes run at most 1 at once (no number)",
                Some(1),
            ),
            ("test-lanes: lanes run at most many at once", None),
            ("test-lanes: lanes run at most 8", None),
            ("  test-lanes: lanes run at most 8 at once", None),
        ] {
            assert_eq!(at_once(line), at, "{line}");
        }
        for (line, name) in [
            (
                "test-lanes:     /l/7/tests-lanes/perf.log (--test perf)",
                Some("perf"),
            ),
            (
                "test-lanes:     /work/a b (c)/tests-lanes/rest.log (--test a --test b)",
                Some("rest"),
            ),
            (
                "test-lanes:     build/test-lanes/doc.log (--doc)",
                Some("doc"),
            ),
            ("test-lanes:     /l/perf.txt (--test perf)", None),
            ("test-lanes:     /l/.log (--test perf)", None),
            ("test-lanes:     /l/perf.log", None),
            (
                "test-lanes: the lane perf ended before it could say how",
                None,
            ),
            (
                "test-lanes: interrupted (exit 130): a lane it cut short ended there",
                None,
            ),
        ] {
            assert_eq!(lane(line).as_deref(), name, "{line}");
        }
    }

    #[test]
    fn only_a_build_file_missing_counts_as_gone() {
        // A process cargo could not run for another reason, and a file of the sources rustc could
        // not find, are failures of the target's own.
        let text = "     Running tests/a.rs (.target/debug/deps/a-01)
error: test failed, to rerun pass `-p p --test a`

Caused by:
  could not execute process `/t/debug/deps/a-01` (never executed)

Caused by:
  Permission denied (os error 13)
   Doc-tests p
error: couldn't find file `/c/p/src/data.txt`
error: couldn't read `/c/p/src/other.txt`: No such file or directory (os error 2)
error: doctest failed, to rerun pass `-p p --doc`
";
        let log = read_in(text);
        assert_eq!(
            log.targets,
            [target("-p p --test a", &[]), target("-p p --doc", &[])]
        );
        assert_eq!(log.went, None);
        assert_eq!(arguments(&log.targets), ["-p p --test a", "-p p --doc"]);
        // A build script's output rustc could not read is gone; a target that named a failed
        // test failed by it, whatever else the log says beside it.
        let text = "     Running tests/a.rs (.target/debug/deps/a-01)
test t ... FAILED
error: couldn't read `/c/t/debug/build/p-1/out/x.rs`: No such file or directory (os error 2)
error: test failed, to rerun pass `-p p --test a`
";
        let log = read_in(text);
        assert_eq!(log.targets, [target("-p p --test a", &["t"])]);
        assert_eq!(
            log.went,
            Some(Went::Building),
            "found in the first target cargo started: gone before any ran"
        );
    }

    #[test]
    fn files_found_gone_before_any_target_ran_went_while_it_built() {
        let text = "error: extern location for a does not exist: /t/debug/deps/liba-1.rlib
error: could not compile `p` (lib test) due to 1 previous error
";
        assert_eq!(read_in(text).went, Some(Went::Building));
    }

    #[test]
    fn the_target_files_went_while_is_named_by_its_own_failure_or_its_neighbours_package() {
        // Its own failure names it.
        let text = "     Running tests/a.rs (.target/debug/deps/a-01)
test t ... FAILED
error: test failed, to rerun pass `-p p --test a`
     Running tests/b.rs (.target/debug/deps/b-02)
error: test failed, to rerun pass `-p p --test b`

Caused by:
  could not execute process `/t/debug/deps/b-02` (never executed)

Caused by:
  No such file or directory (os error 2)
";
        assert_eq!(
            read_in(text).went,
            Some(Went::While(
                "Running tests/a.rs (.target/debug/deps/a-01)".into(),
                Some("-p p --test a".into())
            ))
        );
        // Failures on either side of it in two packages: its package is not said.
        let text = "     Running tests/a.rs (.target/debug/deps/a-01)
test t ... FAILED
error: test failed, to rerun pass `-p p --test a`
     Running tests/b.rs (.target/debug/deps/b-02)
     Running unittests src/lib.rs (.target/debug/deps/q-03)
error: test failed, to rerun pass `-p q --lib`

Caused by:
  could not execute process `/t/debug/deps/q-03` (never executed)

Caused by:
  No such file or directory (os error 2)
";
        assert_eq!(
            read_in(text).went,
            Some(Went::While(
                "Running tests/b.rs (.target/debug/deps/b-02)".into(),
                None
            ))
        );
        // It passed, and the one that found them gone failed in a test of its own: the failure
        // after it is not its.
        let text = "     Running tests/a.rs (.target/debug/deps/a-01)
test t ... FAILED
error: test failed, to rerun pass `-p p --test a`
     Running tests/b.rs (.target/debug/deps/b-02)
     Running tests/c.rs (.target/debug/deps/c-03)
test u ... FAILED
error: couldn't read `/t/debug/build/p-1/out/x.rs`: No such file or directory (os error 2)
error: test failed, to rerun pass `-p p --test c`
";
        assert_eq!(
            read_in(text).went,
            Some(Went::While(
                "Running tests/b.rs (.target/debug/deps/b-02)".into(),
                Some("-p p --test b".into())
            ))
        );
        // No failure after it at all (a log cut short): nothing to name it by.
        let text = "     Running tests/a.rs (.target/debug/deps/a-01)
test t ... FAILED
error: test failed, to rerun pass `-p p --test a`
     Running tests/b.rs (.target/debug/deps/b-02)
   Doc-tests p
error: extern location for q does not exist: /t/debug/deps/libq-1.rlib
";
        assert_eq!(
            read_in(text).went,
            Some(Went::While(
                "Running tests/b.rs (.target/debug/deps/b-02)".into(),
                None
            ))
        );
    }

    #[test]
    fn a_target_is_selected_within_its_package_as_its_line_names_it() {
        for (header, kind) in [
            (
                "Running unittests src/lib.rs (.target/debug/deps/app-1876)",
                Some("--lib"),
            ),
            (
                "Running unittests src/bin/app.rs (/t/debug/deps/app-4b49)",
                Some("--bin app"),
            ),
            (
                "Running unittests src/bin/tool/main.rs (t/deps/tool-1)",
                Some("--bin tool"),
            ),
            (
                "Running unittests src/main.rs (.target/debug/deps/cli-12)",
                Some("--bin cli"),
            ),
            (
                "Running tests/gate/main.rs (.target/debug/deps/gate-2288)",
                Some("--test gate"),
            ),
            (
                "Running tests/a_b.rs (.target/debug/deps/a_b-ff)",
                Some("--test a_b"),
            ),
            (
                "Running benches/speed.rs (.target/release/deps/speed-1)",
                Some("--bench speed"),
            ),
            ("Doc-tests app_checks", Some("--doc")),
            ("Running examples/x.rs (.target/debug/examples/x-1)", None),
            ("Running tests/a.rs", None),
        ] {
            assert_eq!(target_of(header).as_deref(), kind, "{header}");
        }
        for (line, starts) in [
            ("     Running tests/a.rs (.target/debug/deps/a-01)", true),
            ("   Doc-tests app", true),
            ("Running the numbers", false),
            ("   Doc-tests two words", false),
            ("   Doc-tests ", false),
        ] {
            assert_eq!(header(line).is_some(), starts, "{line}");
        }
    }

    #[test]
    fn each_target_cargo_said_failed_is_named_once_with_the_tests_failed_in_it() {
        assert_eq!(
            failed_in(LOG),
            [
                target(
                    "-p symphony-board --lib",
                    &["tests::should_but_does_not", "tests::lib_fails"]
                ),
                target(
                    "-p symphony --test nightly",
                    &["reds::one_fails", "reds::two_fails"]
                ),
                target("-p symphony --test beta", &[]),
                target("-p symphony --doc", &["src/lib.rs - f (line 1)"]),
            ],
            "the summary's repeat of the targets adds none"
        );
    }

    #[test]
    fn the_explicit_live_fixture_keeps_ignored_in_its_narrow_rerun() {
        let live = super::super::nextest::LIVE_FIXTURE;
        let log = read_in(&format!(
            "     Running unittests src/lib.rs (.target/debug/deps/symphony_build-01)\n\
             test {live} ... FAILED\n\
             error: test failed, to rerun pass `-p symphony-build --lib`\n"
        ));
        assert_eq!(
            arguments(&log.targets),
            [format!(
                "-p symphony-build --lib -- --ignored {live} --exact"
            )]
        );
        let selection = format!("-p symphony-build --lib -- --ignored {live} --exact");
        let runner = format!(
            "error: the explicit runner fixture is red (exit 101), to rerun pass `{selection}`\n"
        );
        for named in [true, false] {
            let log = read_in(&format!(
                "     Running unittests src/lib.rs (.target/debug/deps/symphony_build-01)\n{}\
                 error: test failed, to rerun pass `-p symphony-build --lib`\n{runner}",
                if named {
                    format!("test {live} ... FAILED\n")
                } else {
                    String::new()
                }
            ));
            assert_eq!(
                arguments(&log.targets),
                std::slice::from_ref(&selection),
                "{log:?}"
            );
        }
        assert_eq!(
            arguments(&[target("-p symphony-build --lib", &[])]),
            ["-p symphony-build --lib"]
        );
        // The flag belongs to that selected case, not another package, target or ordinary test.
        assert_eq!(
            arguments(&[
                target("-p other --lib", &[live]),
                target("-p symphony-build --test integration", &[live]),
                target("-p symphony-build --lib", &["ordinary"]),
            ]),
            [
                format!("-p other --lib -- {live} --exact"),
                format!("-p symphony-build --test integration -- {live} --exact"),
                "-p symphony-build --lib -- ordinary --exact".to_string(),
            ]
        );
    }

    #[test]
    fn each_failure_is_rerun_alone_and_a_target_that_named_none_whole() {
        assert_eq!(
            arguments(&failed_in(LOG)),
            [
                "-p symphony-board --lib -- tests::should_but_does_not --exact",
                "-p symphony-board --lib -- tests::lib_fails --exact",
                "-p symphony --test nightly -- reds::one_fails --exact",
                "-p symphony --test nightly -- reds::two_fails --exact",
                "-p symphony --test beta",
                "-p symphony --doc -- 'src/lib.rs - f (line 1)' --exact",
            ]
        );
    }

    #[test]
    fn a_log_with_no_target_cargo_said_failed_names_nothing() {
        for (text, case) in [
            ("", "an empty log"),
            (
                "error: could not compile `symphony` (lib) due to 2 previous errors\n\
                 error: Recipe `lint-rs` failed on line 21 with exit code 101\n",
                "a lint that failed",
            ),
            (
                "running 2 tests\ntest a ... FAILED\ntest b ... ok\n(stopped after 36000 s: the \
                 run's time was up)\n",
                "a test named failed and no target after it: the run stopped",
            ),
            (
                "error: test failed, to rerun pass ``\nerror: test failed, to rerun pass `-p a\n",
                "a rerun line that names no arguments, or never ends",
            ),
            (
                "    `-p symphony --lib`\ntest result: FAILED. 0 passed; 1 failed\n",
                "a summary's line alone",
            ),
        ] {
            assert_eq!(failed_in(text), [], "{case}");
        }
        assert_eq!(
            read(Path::new("/no/such/verify.log")),
            Log::default(),
            "no log"
        );
    }

    #[test]
    fn a_test_named_failed_twice_is_rerun_once_and_names_left_unclosed_go_to_no_target() {
        let text = "test a ... FAILED\ntest a ... FAILED\n\
                    error: test failed, to rerun pass `--lib`\n\
                    test b ... FAILED\n\
                    error: test failed, to rerun pass `--test t`\n\
                    test c ... FAILED\n";
        assert_eq!(
            failed_in(text),
            [target("--lib", &["a"]), target("--test t", &["b"])]
        );
    }

    #[test]
    fn a_log_that_is_not_utf8_is_read_as_far_as_it_says_something() {
        let tree = Tree::new();
        let log = tree.path("verify.log");
        std::fs::write(
            &log,
            b"\xff\xfe noise\r\ntest a ... FAILED\r\nerror: test failed, to rerun pass `--lib`\r\n",
        )
        .unwrap();
        assert_eq!(read(&log).targets, [target("--lib", &["a"])]);
    }

    #[test]
    fn a_name_a_shell_reads_apart_is_quoted() {
        assert_eq!(quoted("reds::a_red_check"), "reds::a_red_check");
        assert_eq!(quoted("src/lib.rs"), "src/lib.rs");
        assert_eq!(
            quoted("src/lib.rs - f (line 1)"),
            "'src/lib.rs - f (line 1)'"
        );
        assert_eq!(quoted("it's"), r"'it'\''s'");
        assert_eq!(quoted("$HOME"), "'$HOME'");
        assert_eq!(quoted(""), "''");
    }
}
