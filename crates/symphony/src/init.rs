//! `symphony init`: the agents' integration files a project needs, installed and kept current.
//!
//! The Symphony provides the generic skills, the allow rules (Claude's in `.claude/settings.json`,
//! Codex's in `.codex/rules/symphony.rules`), the heartbeat hook and the memory guard. Each file it
//! writes carries a stamp, which is how a later run knows the file is its own to bring up to date. A
//! file of the same name without the stamp is the project's own: it is the one in use and this never
//! writes over it. Where the project has none, the Symphony's is used. The project's own skills and
//! guards sit beside the shipped ones and are never touched. It writes no permission rule but the
//! allow rules.

use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

/// Where the skills live in a project, and where Claude Code loads them from.
const SKILLS: &str = ".agents/skills";
const CLAUDE_SKILLS: &str = ".claude/skills";
const SETTINGS: &str = ".claude/settings.json";
const CODEX_RULES: &str = ".codex/rules/symphony.rules";

/// The mark on a skill's `SKILL.md` and on the Codex rules file.
const MARK: &str = "symphony-stamp:";

/// A hook the Symphony installs in `.claude/settings.json`. Each runs the binary the project's
/// `.target/bin/symphony` names and builds nothing.
struct Hook {
    /// What names the hook in a command: one that names it without the stamp is the project's own.
    name: &'static str,
    command: &'static str,
    /// The ownership stamp, independent of a managed command's executable path or quoting.
    stamp: &'static str,
    timeout: u64,
    /// The events it runs on and the tool matcher of each.
    events: &'static [(&'static str, &'static str)],
}

/// The heartbeat: with no binary linked it passes quietly, and it never refuses a call.
const HEARTBEAT: Hook = Hook {
    name: "tasks-beat",
    command: "\"$CLAUDE_PROJECT_DIR/.target/bin/symphony\" hook tasks-beat 2>/dev/null || true # symphony-stamp: heartbeat-v1",
    stamp: "# symphony-stamp: heartbeat-",
    timeout: 10,
    events: &[("PreToolUse", "Bash"), ("PostToolUse", "")],
};

/// The memory guard (`crate::hook`). Its command passes on the guard's refusal, its one line and exit
/// 2, and nothing else: with no binary linked, or one built before the guard (whose `hook` refuses
/// the name with its usage, exit 2 too), the call goes through.
const MEMORY_GUARD: Hook = Hook {
    name: "memory-guard",
    command: "r=$(\"$CLAUDE_PROJECT_DIR/.target/bin/symphony\" hook memory-guard 2>&1 >/dev/null) || case $r in Blocked:*) printf '%s\\n' \"$r\" >&2; exit 2;; esac # symphony-stamp: memory-guard-v1",
    stamp: "# symphony-stamp: memory-guard-",
    timeout: 10,
    events: &[("PreToolUse", "Write|Edit|MultiEdit|NotebookEdit|Bash")],
};

const HOOKS: [Hook; 2] = [HEARTBEAT, MEMORY_GUARD];

/// What an agent on the board runs, which Claude allows without a prompt.
const ALLOW: [&str; 5] = [
    "Bash(just tasks:*)",
    "Bash(TASKS_SESSION=* just tasks *)",
    "Bash(just symphony-status:*)",
    "Bash(just symphony-reply:*)",
    "Bash(just symphony-ask:*)",
];

/// One shipped skill: its folder name and its files by path inside it (`SKILL.md` first).
pub struct Skill {
    pub name: String,
    pub files: Vec<(String, String)>,
}

/// Everything the Symphony provides to a project.
pub struct Shipped {
    pub skills: Vec<Skill>,
    pub codex_rules: String,
}

macro_rules! skill {
    ($name:literal $(, $file:literal)*) => {
        Skill {
            name: $name.to_string(),
            files: vec![
                ("SKILL.md".to_string(), include_str!(concat!("../shipped/skills/", $name, "/SKILL.md")).to_string())
                $(, ($file.to_string(), include_str!(concat!("../shipped/skills/", $name, "/", $file)).to_string()))*
            ],
        }
    };
}

impl Shipped {
    /// The files this build carries.
    pub fn builtin() -> Shipped {
        Shipped {
            skills: vec![
                skill!("apply-review", "rewrite-brief.md"),
                skill!("chat-doc"),
                skill!("create-task"),
                skill!("task-orchestrator"),
                skill!("tasks"),
                skill!("triage"),
                skill!("work"),
            ],
            codex_rules: include_str!("../shipped/codex.rules").to_string(),
        }
    }
}

/// What one run did to one thing.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Installed,
    Updated,
    Current,
    /// The project's own copy, left as it is.
    Own,
    Removed,
}

impl Outcome {
    fn word(&self) -> &'static str {
        match self {
            Outcome::Installed => "installed",
            Outcome::Updated => "updated",
            Outcome::Current => "current",
            Outcome::Own => "kept (the project's own)",
            Outcome::Removed => "removed",
        }
    }
}

/// A line of the run's report: the thing and what happened to it.
pub type Report = Vec<(String, Outcome)>;

fn digest(parts: &[&str]) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update(part.as_bytes());
        hash.update([0]);
    }
    hash.finalize()[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The stamp a skill's `SKILL.md` carries: after its front matter, a comment naming the files' digest.
fn stamped_skill(skill: &Skill, text: &str) -> String {
    let parts: Vec<&str> = skill
        .files
        .iter()
        .flat_map(|(path, body)| [path.as_str(), body.as_str()])
        .collect();
    let files: Vec<&str> = skill.files.iter().map(|(path, _)| path.as_str()).collect();
    let line = format!(
        "<!-- {MARK} {} files={} -->\n",
        digest(&parts),
        files.join(",")
    );
    let at = text
        .strip_prefix("---\n")
        .and_then(|rest| rest.find("\n---\n").map(|end| 4 + end + 5))
        .unwrap_or(0);
    format!("{}{line}{}", &text[..at], &text[at..])
}

/// Whether `text` carries the Symphony's stamp: a line that begins with the mark.
fn is_stamped(text: &str) -> bool {
    text.lines().any(|line| {
        line.strip_prefix("<!-- ")
            .or_else(|| line.strip_prefix("# "))
            .is_some_and(|rest| rest.starts_with(MARK))
    })
}

/// The files a stamp names: what the Symphony wrote into the skill's folder.
fn stamped_files(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| line.starts_with("<!-- symphony-stamp: "))
        .find_map(|line| line.split_once(" files=")?.1.strip_suffix(" -->"))
        .map(|names| {
            names
                .split(',')
                .filter(|name| {
                    !name.is_empty()
                        && Path::new(name)
                            .components()
                            .all(|part| matches!(part, std::path::Component::Normal(_)))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn stamped_rules(text: &str) -> String {
    format!("# {MARK} {}\n{text}", digest(&[text]))
}

/// Writes `text` at `path` when it differs; whether it wrote.
fn put(path: &Path, text: &str) -> io::Result<bool> {
    if is_link(path) {
        return Err(at(
            path,
            io::Error::other("managed file is a symlink; left as it is"),
        ));
    }
    if read_optional(path)?.is_some_and(|have| have == text) {
        return Ok(false);
    }
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|error| at(folder, error))?;
    }
    std::fs::write(path, text).map_err(|error| at(path, error))?;
    Ok(true)
}

fn is_link(path: &Path) -> bool {
    path.symlink_metadata().is_ok_and(|meta| meta.is_symlink())
}

fn at(path: &Path, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{}: {error}", path.display()))
}

fn read_optional(path: &Path) -> io::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error)
            if error.kind() == io::ErrorKind::NotFound && path.symlink_metadata().is_err() =>
        {
            Ok(None)
        }
        Err(error) => Err(at(path, error)),
    }
}

fn remove_file(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(at(path, error)),
    }
}

/// Makes `.claude/skills/NAME` a link to the skill's folder when nothing is there.
fn link_skill(root: &Path, name: &str) -> io::Result<()> {
    let link = root.join(CLAUDE_SKILLS).join(name);
    match link.symlink_metadata() {
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(at(&link, error)),
    }
    std::fs::create_dir_all(root.join(CLAUDE_SKILLS)).map_err(|error| at(&link, error))?;
    std::os::unix::fs::symlink(PathBuf::from("../../").join(SKILLS).join(name), &link)
        .map_err(|error| at(&link, error))
}

fn install_skills(root: &Path, shipped: &Shipped, report: &mut Report) -> io::Result<()> {
    for skill in &shipped.skills {
        let folder = root.join(SKILLS).join(&skill.name);
        let entry = format!("skill {}", skill.name);
        let main = folder.join("SKILL.md");
        let have = read_optional(&main)?;
        let outcome = match &have {
            _ if is_link(&folder) || is_link(&main) => Outcome::Own,
            Some(text) if !is_stamped(text) => Outcome::Own,
            _ => {
                let mut wrote = false;
                if let Some(old) = &have {
                    for path in stamped_files(old) {
                        if !skill.files.iter().any(|(name, _)| name == path) {
                            remove_file(&folder.join(path))?;
                            wrote = true;
                        }
                    }
                }
                for (path, body) in &skill.files {
                    let text = if path == "SKILL.md" {
                        stamped_skill(skill, body)
                    } else {
                        body.clone()
                    };
                    wrote |= put(&folder.join(path), &text)?;
                }
                match (wrote, have.is_some()) {
                    (false, _) => Outcome::Current,
                    (true, true) => Outcome::Updated,
                    (true, false) => Outcome::Installed,
                }
            }
        };
        if folder.is_dir() {
            link_skill(root, &skill.name)?;
        }
        report.push((entry, outcome));
    }
    Ok(())
}

/// A skill the Symphony wrote and no longer provides: its `SKILL.md` goes, its folder with it when
/// nothing else of the project's is in it, and the link to it.
fn remove_retired(root: &Path, shipped: &Shipped, report: &mut Report) -> io::Result<()> {
    let folders = match std::fs::read_dir(root.join(SKILLS)) {
        Ok(folders) => folders,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(at(&root.join(SKILLS), error)),
    };
    let mut names: Vec<String> = folders
        .collect::<io::Result<Vec<_>>>()
        .map_err(|error| at(&root.join(SKILLS), error))?
        .into_iter()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| !shipped.skills.iter().any(|skill| &skill.name == name))
        .collect();
    names.sort();
    for name in names {
        let folder = root.join(SKILLS).join(&name);
        let main = folder.join("SKILL.md");
        if is_link(&folder) || is_link(&main) {
            continue;
        }
        let Some(text) = read_optional(&main)? else {
            continue;
        };
        if !is_stamped(&text) {
            continue;
        }
        for file in stamped_files(&text) {
            if file != "SKILL.md" {
                remove_file(&folder.join(file))?;
            }
        }
        remove_file(&main)?;
        match std::fs::remove_dir(&folder) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty => {}
            Err(error) => return Err(at(&folder, error)),
        }
        let link = root.join(CLAUDE_SKILLS).join(&name);
        let target = PathBuf::from("../../").join(SKILLS).join(&name);
        if std::fs::read_link(&link).is_ok_and(|have| have == target) {
            remove_file(&link)?;
        }
        report.push((format!("skill {name}"), Outcome::Removed));
    }
    Ok(())
}

fn install_codex_rules(root: &Path, shipped: &Shipped, report: &mut Report) -> io::Result<()> {
    let path = root.join(CODEX_RULES);
    let have = read_optional(&path)?;
    let outcome = match &have {
        _ if is_link(&path) => Outcome::Own,
        Some(text) if !text.lines().next().is_some_and(is_mark_line) => Outcome::Own,
        _ => match (
            put(&path, &stamped_rules(&shipped.codex_rules))?,
            have.is_some(),
        ) {
            (false, _) => Outcome::Current,
            (true, true) => Outcome::Updated,
            (true, false) => Outcome::Installed,
        },
    };
    report.push(("codex rules".to_string(), outcome));
    Ok(())
}

fn is_mark_line(line: &str) -> bool {
    line.strip_prefix("# ")
        .is_some_and(|rest| rest.starts_with(MARK))
}

/// A command the project wrote for `hook`: it names the hook and is not the Symphony's.
fn own(hook: &Hook, command: &str) -> bool {
    command.contains(hook.name) && !managed(hook, command)
}

fn managed(hook: &Hook, command: &str) -> bool {
    command.lines().any(|line| {
        line.rsplit_once(hook.stamp).is_some_and(|(_, version)| {
            let version = version.trim();
            !version.is_empty()
                && version
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        })
    })
}

/// `hooks.EVENT` carries `hook`: the Symphony's command brought up to date, or the project's own left
/// alone, else a group added. Whether it changed anything.
fn ensure_hook(
    hooks: &mut Map<String, Value>,
    hook: &Hook,
    event: &str,
    matcher: &str,
) -> io::Result<bool> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{SETTINGS}: hooks.{event} must be an array of hook groups; left as it is"),
        )
    };
    let groups = hooks
        .entry(event.to_string())
        .or_insert_with(|| json!([]))
        .as_array_mut();
    let groups = groups.ok_or_else(invalid)?;
    for group in groups.iter() {
        let list = group
            .get("hooks")
            .and_then(Value::as_array)
            .ok_or_else(invalid)?;
        if list.iter().any(|hook| {
            !hook.is_object()
                || (!matches!(hook["type"].as_str(), Some("prompt" | "agent"))
                    && !hook.get("command").is_some_and(Value::is_string))
        }) {
            return Err(invalid());
        }
    }
    let ours = |entry: &Value| {
        entry["command"]
            .as_str()
            .is_some_and(|command| managed(hook, command))
    };
    let project_owns = groups.iter().any(|group| {
        group["hooks"].as_array().is_some_and(|list| {
            list.iter()
                .any(|entry| own(hook, entry["command"].as_str().unwrap_or("")))
        })
    });
    let found: Vec<_> = groups
        .iter()
        .flat_map(|group| {
            group["hooks"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|entry| ours(entry))
                .map(move |entry| (group, entry))
        })
        .collect();
    let current = !project_owns
        && found.len() == 1
        && found[0].0["matcher"] == matcher
        && found[0].1["command"] == hook.command
        && found[0].1["timeout"] == hook.timeout
        && found[0].1["type"] == "command";
    if current || (project_owns && found.is_empty()) {
        return Ok(false);
    }
    // Only this hook's managed entries move. A guard sharing their old matcher keeps its group.
    groups.retain_mut(|group| {
        let list = group["hooks"].as_array_mut().expect("validated hook group");
        let before = list.len();
        list.retain(|entry| !ours(entry));
        before == list.len() || !list.is_empty()
    });
    if !project_owns {
        groups.push(json!({
            "matcher": matcher,
            "hooks": [{"type":"command", "command":hook.command, "timeout":hook.timeout}]
        }));
    }
    Ok(true)
}

fn install_settings(root: &Path, report: &mut Report) -> io::Result<()> {
    let path = root.join(SETTINGS);
    let have = read_optional(&path)?;
    let mut settings: Value = match &have {
        Some(text) => serde_json::from_str(text).map_err(|error| {
            io::Error::new(io::ErrorKind::InvalidData, format!("{SETTINGS}: {error}"))
        })?,
        None => json!({}),
    };
    let not_object = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{SETTINGS}: not the shape Claude Code reads; left as it is"),
        )
    };
    let top = settings.as_object_mut().ok_or_else(not_object)?;
    let mut changed = false;

    let permissions = top
        .entry("permissions")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(not_object)?;
    let allow = permissions
        .entry("allow")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(not_object)?;
    for rule in ALLOW {
        if !allow.iter().any(|have| have.as_str() == Some(rule)) {
            allow.push(json!(rule));
            changed = true;
        }
    }

    let hooks = top
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(not_object)?;
    for hook in &HOOKS {
        for (event, matcher) in hook.events {
            changed |= ensure_hook(hooks, hook, event, matcher)?;
        }
    }

    let outcome = match (changed, have.is_some()) {
        (false, _) => Outcome::Current,
        (true, existed) => {
            let mut text = serde_json::to_string_pretty(&settings).map_err(io::Error::other)?;
            text.push('\n');
            put(&path, &text)?;
            if existed {
                Outcome::Updated
            } else {
                Outcome::Installed
            }
        }
    };
    report.push(("claude settings".to_string(), outcome));
    Ok(())
}

/// Installs and brings current everything `shipped` holds in the project at `root`.
pub fn init(root: &Path, shipped: &Shipped) -> io::Result<Report> {
    let mut report = Report::new();
    install_skills(root, shipped, &mut report)?;
    remove_retired(root, shipped, &mut report)?;
    install_codex_rules(root, shipped, &mut report)?;
    install_settings(root, &mut report)?;
    Ok(report)
}

const USAGE: &str = "usage: symphony init [-h] [--root ROOT]";
const HELP: &str = "Install the agents' integration files and keep them current.

options:
  -h, --help   show this help message and exit
  --root ROOT  the project's root (default: the current folder)

A file of the same name without the Symphony's stamp is the project's own: it is used and never
written over.";

/// `symphony init [--root ROOT]`. Its exit code.
pub fn main(args: &[String], out: &mut dyn io::Write, err: &mut dyn io::Write) -> i32 {
    use crate::cli::{Flag, parse, refuse};
    let flags = [Flag {
        name: "--root",
        value: true,
    }];
    let parsed = match parse(args, &flags) {
        Ok(parsed) if parsed.help => {
            let _ = writeln!(out, "{USAGE}\n\n{HELP}");
            return 0;
        }
        Ok(parsed) if !parsed.positionals.is_empty() => {
            let problem = format!("unrecognized arguments: {}", parsed.positionals.join(" "));
            return refuse(err, USAGE, "init", &problem);
        }
        Ok(parsed) => parsed,
        Err(problem) => return refuse(err, USAGE, "init", &problem),
    };
    let root = PathBuf::from(parsed.value("--root").unwrap_or("."));
    match init(&root, &Shipped::builtin()) {
        Ok(report) => {
            for (entry, outcome) in report {
                let _ = writeln!(out, "{entry}: {}", outcome.word());
            }
            0
        }
        Err(error) => {
            let _ = writeln!(err, "symphony init: {error}");
            1
        }
    }
}

#[cfg(test)]
#[path = "init_tests.rs"]
mod tests;
