//! The release's hosted workflow (`.github/workflows/release-verify.yml`, docs/RELEASES.md, "On
//! GitHub's runners") held to its bounds: started by hand alone; its token given no permission but
//! reading the repository, no job giving it more, and left in no checkout; no secret or token handed
//! to a step; each action pinned by a commit; every checkout of the input commit, and no job run
//! before the input is found to be the full commit checked out. Each refusal is shown on a variant
//! of the file, and its two scripts (that check, and the steps the commit's `tools/verify.sh` names)
//! are run as GitHub runs them.
//!
//! The file is read by its lines, as it is written: a key a line, a deeper indent for what a key
//! holds, a list item's keys after its dash, and a `|` or `>` block's lines as text, which only the
//! secret's refusal reads.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use regex::Regex;

#[path = "../../symphony/tests/common/tree.rs"]
mod tree;
use tree::Tree;

const WORKFLOW: &str = include_str!("../../../.github/workflows/release-verify.yml");
/// The step that refuses an input that is not the full commit checked out.
const INPUT_STEP: &str = "The input is the full commit checked out";
/// The step that lists the commit's verify steps for the jobs.
const STEPS_STEP: &str = "The steps its tools/verify.sh names";
/// What names a secret or the job's token, in any case: a secret by the context's two notations
/// or every secret at once (`secrets: inherit`), the token by its context or its variable.
const HANDED: [&str; 5] = [
    "secrets.",
    "secrets:",
    "secrets[",
    "github.token",
    "github_token",
];

/// A line of the workflow: its indent (where its dash is, for a list item), whether it starts a
/// list item, its text after that dash with any comment cut, and whether it is a block's text.
struct Line<'a> {
    number: usize,
    indent: usize,
    item: bool,
    text: &'a str,
    scalar: bool,
}

impl<'a> Line<'a> {
    /// Its key and the value on its line (empty where the value is below it), if it is a key's.
    fn key(&self) -> Option<(&'a str, &'a str)> {
        if self.scalar {
            return None;
        }
        let (key, value) = self.text.split_once(':')?;
        if key.is_empty() || key.contains(' ') || !(value.is_empty() || value.starts_with(' ')) {
            return None;
        }
        Some((key, value.trim()))
    }
}

fn read(source: &str) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    // The indent of the key whose `|` or `>` block the lines below it deeper are.
    let mut block: Option<usize> = None;
    for (index, raw) in source.lines().enumerate() {
        let body = raw.trim_start_matches(' ');
        if body.trim().is_empty() {
            continue;
        }
        let indent = raw.len() - body.len();
        if let Some(of) = block {
            if indent > of {
                lines.push(Line {
                    number: index + 1,
                    indent,
                    item: false,
                    text: body.trim_end(),
                    scalar: true,
                });
                continue;
            }
            block = None;
        }
        if body.starts_with('#') {
            continue;
        }
        let (item, text) = match body.strip_prefix("- ") {
            Some(rest) => (true, rest),
            None => (false, body),
        };
        let text = match text.find(" #") {
            Some(at) => &text[..at],
            None => text,
        }
        .trim_end();
        let line = Line {
            number: index + 1,
            indent,
            item,
            text,
            scalar: false,
        };
        if let Some((_, value)) = line.key()
            && matches!(value, "|" | "|-" | "|+" | ">" | ">-" | ">+")
        {
            block = Some(indent);
        }
        lines.push(line);
    }
    lines
}

/// What the line at `at` holds: the lines after it that are deeper.
fn held<'a, 'b>(lines: &'b [Line<'a>], at: usize) -> &'b [Line<'a>] {
    let rest = &lines[at + 1..];
    let end = rest
        .iter()
        .position(|line| line.indent <= lines[at].indent)
        .unwrap_or(rest.len());
    &rest[..end]
}

/// The keys the line at `at` holds directly, each with its index in `lines`.
fn children<'a>(lines: &[Line<'a>], at: usize) -> Vec<(usize, &'a str, &'a str)> {
    let inner = held(lines, at);
    let Some(depth) = inner.iter().map(|line| line.indent).min() else {
        return Vec::new();
    };
    inner
        .iter()
        .enumerate()
        .filter(|(_, line)| line.indent == depth && !line.item)
        .filter_map(|(offset, line)| {
            let (key, value) = line.key()?;
            Some((at + 1 + offset, key, value))
        })
        .collect()
}

/// The index of the top-level key `name`.
fn top(lines: &[Line<'_>], name: &str) -> Option<usize> {
    lines
        .iter()
        .position(|line| line.indent == 0 && line.key().is_some_and(|(key, _)| key == name))
}

/// Each list item's line with what it holds, its own line first.
fn items<'a, 'b>(lines: &'b [Line<'a>]) -> Vec<&'b [Line<'a>]> {
    (0..lines.len())
        .filter(|&at| lines[at].item)
        .map(|at| &lines[at..at + 1 + held(lines, at).len()])
        .collect()
}

/// The value of the key `name` among `lines`, if one has it.
fn value<'a>(lines: &[Line<'a>], name: &str) -> Option<&'a str> {
    lines.iter().find_map(|line| match line.key() {
        Some((key, value)) if key == name => Some(value),
        _ => None,
    })
}

/// Every way the workflow leaves its bounds, a sentence each; none when it keeps them.
fn refusals(source: &str) -> Vec<String> {
    let lines = read(source);
    let mut said = Vec::new();
    triggers(&lines, &mut said);
    permissions(&lines, &mut said);
    pins(&lines, &mut said);
    secrets(&lines, &mut said);
    checkouts(&lines, &mut said);
    input_first(&lines, &mut said);
    said
}

/// Started by hand alone: `workflow_dispatch` and no other trigger.
fn triggers(lines: &[Line<'_>], said: &mut Vec<String>) {
    let Some(on) = top(lines, "on") else {
        said.push("it names no trigger: it is started by hand alone (workflow_dispatch)".into());
        return;
    };
    let (_, inline) = lines[on].key().unwrap_or_default();
    if !inline.is_empty() {
        said.push(format!(
            "it starts on {inline}: it is started by hand alone (workflow_dispatch)"
        ));
        return;
    }
    let keys = children(lines, on);
    for (_, key, _) in &keys {
        if *key != "workflow_dispatch" {
            said.push(format!(
                "it starts on {key}: it is started by hand alone (workflow_dispatch)"
            ));
        }
    }
    if !keys.iter().any(|(_, key, _)| *key == "workflow_dispatch") {
        said.push("it cannot be started by hand: it names no workflow_dispatch".into());
    }
}

/// Its token reads the repository and nothing more, and no job gives it more.
fn permissions(lines: &[Line<'_>], said: &mut Vec<String>) {
    match top(lines, "permissions") {
        None => said.push(
            "it names no permissions: its token would take the repository's default, which may \
             write; contents: read alone"
                .into(),
        ),
        Some(at) => {
            let (_, inline) = lines[at].key().unwrap_or_default();
            let given: Vec<(&str, &str)> = if inline.is_empty() {
                children(lines, at)
                    .into_iter()
                    .map(|(_, key, value)| (key, value))
                    .collect()
            } else {
                vec![("permissions", inline)]
            };
            if given != [("contents", "read")] {
                for (key, value) in given {
                    if (key, value) != ("contents", "read") {
                        said.push(format!(
                            "its token is given {key}: {value}; contents: read alone"
                        ));
                    }
                }
            }
        }
    }
    for line in lines {
        if line.indent > 0 && line.key().is_some_and(|(key, _)| key == "permissions") {
            said.push(format!(
                "line {}: a job gives its token permissions of its own; the workflow's contents: \
                 read alone",
                line.number
            ));
        }
    }
}

/// Every action is pinned by a commit.
fn pins(lines: &[Line<'_>], said: &mut Vec<String>) {
    let pinned = Regex::new(r"^[A-Za-z0-9-]+/[A-Za-z0-9._/-]+@[0-9a-f]{40}$").unwrap();
    for line in lines {
        if let Some(("uses", action)) = line.key()
            && !pinned.is_match(action)
        {
            said.push(format!(
                "line {}: the action {action} is not pinned by a commit",
                line.number
            ));
        }
    }
}

/// No secret, and not its token, is handed to a step.
fn secrets(lines: &[Line<'_>], said: &mut Vec<String>) {
    for line in lines {
        let text = line.text.to_ascii_lowercase();
        if HANDED.iter().any(|named| text.contains(named)) {
            said.push(format!(
                "line {}: it hands a secret or its token on: {}",
                line.number, line.text
            ));
        }
    }
}

/// Every checkout is of the input commit and leaves its token in no checkout.
fn checkouts(lines: &[Line<'_>], said: &mut Vec<String>) {
    for step in items(lines) {
        let Some(action) = value(step, "uses") else {
            continue;
        };
        if !action.starts_with("actions/checkout@") {
            continue;
        }
        let number = step[0].number;
        if value(step, "ref") != Some("${{ inputs.commit }}") {
            said.push(format!(
                "line {number}: a checkout is not of the input commit (ref: ${{{{ inputs.commit }}}})"
            ));
        }
        if value(step, "persist-credentials") != Some("false") {
            said.push(format!(
                "line {number}: a checkout leaves its token in the checkout (persist-credentials: \
                 false)"
            ));
        }
    }
}

/// One job refuses an input that is not the full commit checked out, and every other job needs it.
fn input_first(lines: &[Line<'_>], said: &mut Vec<String>) {
    let Some(jobs) = top(lines, "jobs") else {
        said.push("it names no jobs".into());
        return;
    };
    let jobs = children(lines, jobs);
    let checks = |at: usize| {
        held(lines, at)
            .iter()
            .any(|line| line.key() == Some(("name", INPUT_STEP)))
    };
    let Some(&(_, checker, _)) = jobs.iter().find(|&&(at, _, _)| checks(at)) else {
        said.push(format!(
            "no job refuses an input that is not the full commit: the step {INPUT_STEP:?}"
        ));
        return;
    };
    for &(at, job, _) in &jobs {
        if job == checker {
            continue;
        }
        let needs = children(lines, at)
            .into_iter()
            .find_map(|(_, key, value)| (key == "needs").then_some(value))
            .unwrap_or_default();
        let named = needs
            .trim_matches(['[', ']'])
            .split(',')
            .any(|name| name.trim() == checker);
        if !named {
            said.push(format!(
                "the job {job} runs whether or not the input was refused: needs: {checker}"
            ));
        }
    }
}

/// The `run` text of the step named `name`, as GitHub hands it to the shell.
fn script(name: &str) -> String {
    let lines = read(WORKFLOW);
    let step = items(&lines)
        .into_iter()
        .find(|step| value(step, "name") == Some(name))
        .unwrap_or_else(|| panic!("no step named {name:?}"));
    let at = step
        .iter()
        .position(|line| line.key() == Some(("run", "|")))
        .unwrap_or_else(|| panic!("the step {name:?} runs no `run: |` block"));
    let text: Vec<&Line<'_>> = step[at + 1..].iter().take_while(|l| l.scalar).collect();
    let depth = text.iter().map(|line| line.indent).min().unwrap_or(0);
    text.iter()
        .map(|line| format!("{}{}\n", " ".repeat(line.indent - depth), line.text))
        .collect()
}

/// `script` in `dir`, as GitHub runs a step's bash, with `env` beside the caller's.
fn run(script: &str, dir: &Path, env: &[(&str, &str)]) -> Output {
    let mut command = Command::new("bash");
    command
        .args(["--noprofile", "--norc", "-eo", "pipefail", "-c", script])
        .current_dir(dir);
    for (key, value) in env {
        command.env(key, value);
    }
    command.output().unwrap()
}

/// `git ARGS` in `dir`, with no configuration of this machine's, which must succeed: its output.
fn git(dir: &Path, args: &[&str]) -> String {
    let done = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .output()
        .unwrap();
    assert!(
        done.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&done.stderr)
    );
    String::from_utf8(done.stdout).unwrap().trim().to_string()
}

/// The workflow with its first `from` made `to`, which must be there.
fn with(from: &str, to: &str) -> String {
    assert!(WORKFLOW.contains(from), "the workflow holds no {from:?}");
    WORKFLOW.replacen(from, to, 1)
}

/// `source` is refused with a sentence that says `says`.
#[track_caller]
fn refused(source: &str, says: &str) {
    let said = refusals(source);
    assert!(
        said.iter().any(|sentence| sentence.contains(says)),
        "no refusal says {says:?}; the refusals: {said:#?}"
    );
}

#[test]
fn the_workflow_keeps_its_bounds() {
    assert_eq!(refusals(WORKFLOW), Vec::<String>::new());
}

#[test]
fn a_trigger_besides_the_hand_is_refused() {
    for trigger in [
        "push",
        "pull_request",
        "pull_request_target",
        "schedule",
        "workflow_run",
    ] {
        let source = with(
            "on:\n  workflow_dispatch:",
            &format!("on:\n  {trigger}:\n  workflow_dispatch:"),
        );
        refused(&source, &format!("it starts on {trigger}:"));
    }
}

#[test]
fn triggers_on_the_line_of_on_are_refused() {
    let source = with(
        "on:\n  workflow_dispatch:",
        "on: [push, workflow_dispatch]\nlater:\n  workflow_dispatch:",
    );
    refused(&source, "it starts on [push, workflow_dispatch]");
}

#[test]
fn a_workflow_no_hand_can_start_is_refused() {
    refused(
        &with("on:\n  workflow_dispatch:", "on:\n  push:"),
        "it names no workflow_dispatch",
    );
    refused(
        &with("on:\n  workflow_dispatch:", "never:\n  workflow_dispatch:"),
        "it names no trigger",
    );
}

#[test]
fn a_token_that_writes_is_refused() {
    refused(
        &with("contents: read", "contents: write"),
        "its token is given contents: write",
    );
    refused(
        &with("contents: read", "contents: read\n  packages: write"),
        "its token is given packages: write",
    );
    refused(
        &with("permissions:\n  contents: read", "permissions: write-all"),
        "its token is given permissions: write-all",
    );
}

#[test]
fn a_workflow_naming_no_permissions_is_refused() {
    refused(
        &with("permissions:\n  contents: read\n", ""),
        "it names no permissions",
    );
}

#[test]
fn a_job_with_permissions_of_its_own_is_refused() {
    let source = with(
        "    runs-on: macos-15\n",
        "    runs-on: macos-15\n    permissions:\n      contents: write\n",
    );
    refused(&source, "a job gives its token permissions of its own");
}

#[test]
fn an_action_not_pinned_by_a_commit_is_refused() {
    let pinned = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";
    for action in [
        "actions/checkout@v7",
        "actions/checkout@main",
        "actions/checkout@3d3c42e",
        "actions/checkout@3D3C42E5AAC5BA805825DA76410C181273BA90B1",
        "./.github/actions/setup",
        "docker://alpine:3",
    ] {
        refused(
            &with(pinned, action),
            &format!("the action {action} is not pinned by a commit"),
        );
    }
}

#[test]
fn a_secret_or_the_token_handed_on_is_refused() {
    for given in [
        "TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}",
        "TOKEN: ${{ secrets['CARGO_REGISTRY_TOKEN'] }}",
        "TOKEN: ${{ github.token }}",
        "GITHUB_TOKEN: ${{ github.token }}",
    ] {
        let source = with(
            "          STEP: ${{ matrix.step }}\n",
            &format!("          STEP: ${{{{ matrix.step }}}}\n          {given}\n"),
        );
        refused(&source, "it hands a secret or its token on");
    }
    refused(
        &with(
            "    needs: plan\n",
            "    needs: plan\n    secrets: inherit\n",
        ),
        "it hands a secret or its token on: secrets: inherit",
    );
    refused(
        &with(
            "run: just verify \"$STEP\"",
            "run: |\n          echo \"${{ secrets.CARGO_REGISTRY_TOKEN }}\" | cargo login",
        ),
        "it hands a secret or its token on",
    );
}

#[test]
fn a_checkout_that_keeps_its_token_is_refused() {
    refused(
        &with("persist-credentials: false", "persist-credentials: true"),
        "a checkout leaves its token in the checkout",
    );
    refused(
        &with("          persist-credentials: false\n", ""),
        "a checkout leaves its token in the checkout",
    );
}

#[test]
fn a_checkout_of_another_commit_is_refused() {
    refused(
        &with("ref: ${{ inputs.commit }}", "ref: main"),
        "a checkout is not of the input commit",
    );
}

#[test]
fn a_job_that_runs_before_the_input_is_checked_is_refused() {
    refused(
        &with("    needs: plan\n", ""),
        "the job verify runs whether or not the input was refused",
    );
    refused(
        &with(&format!("name: {INPUT_STEP}"), "name: Anything else"),
        "no job refuses an input that is not the full commit",
    );
}

#[test]
fn the_input_must_be_the_full_commit_checked_out() {
    let tree = Tree::new();
    let dir = &tree.root;
    git(dir, &["init", "--quiet", "--initial-branch=main"]);
    fs::write(dir.join("a"), "a").unwrap();
    git(dir, &["add", "a"]);
    git(dir, &["commit", "--quiet", "-m", "first"]);
    let first = git(dir, &["rev-parse", "HEAD"]);
    fs::write(dir.join("a"), "b").unwrap();
    git(dir, &["commit", "--quiet", "-am", "second"]);
    let head = git(dir, &["rev-parse", "HEAD"]);
    git(dir, &["tag", "release-2026-11-20"]);
    let check = script(INPUT_STEP);
    let upper = head.to_ascii_uppercase();
    let spaced = format!("{head} ");

    let full = run(&check, dir, &[("COMMIT", &head)]);
    assert!(
        full.status.success(),
        "the full commit was refused: {}",
        String::from_utf8_lossy(&full.stdout)
    );
    for input in [
        &head[..7],
        &head[..39],
        upper.as_str(),
        spaced.as_str(),
        "main",
        "HEAD",
        "release-2026-11-20",
        "",
        first.as_str(),
    ] {
        let refused = run(&check, dir, &[("COMMIT", input)]);
        let said = String::from_utf8_lossy(&refused.stdout);
        assert!(!refused.status.success(), "{input:?} was not refused");
        assert!(
            said.contains(&format!(
                "::error::the input is not the full commit it checked out, {head}"
            )),
            "{input:?} was refused saying {said:?}"
        );
    }
}

#[test]
fn the_jobs_are_the_steps_of_the_commits_own_verify() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let verify = fs::read_to_string(root.join("tools/verify.sh")).unwrap();
    let named = verify
        .lines()
        .find_map(|line| line.strip_prefix("steps=(")?.strip_suffix(')'))
        .expect("tools/verify.sh names its steps on a line `steps=(...)`");
    let expected: Vec<&str> = named
        .split_whitespace()
        .filter(|step| *step != "nextest-setup")
        .collect();
    assert!(expected.contains(&"tests") && expected.contains(&"lint"));

    let tree = Tree::new();
    let output = tree.path("output");
    let listed = run(
        &script(STEPS_STEP),
        &root,
        &[("GITHUB_OUTPUT", output.to_str().unwrap())],
    );
    assert!(
        listed.status.success(),
        "{}",
        String::from_utf8_lossy(&listed.stderr)
    );
    let written = fs::read_to_string(&output).unwrap();
    let steps = written
        .strip_prefix("steps=")
        .and_then(|rest| rest.strip_suffix('\n'))
        .unwrap_or_else(|| panic!("it wrote {written:?}, not one line steps=JSON"));
    let steps: Vec<String> = serde_json::from_str(steps).unwrap();
    assert_eq!(steps, expected);
}
