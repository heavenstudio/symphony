//! `symphony init`: the agents' integration files a project needs, installed and kept current.
//!
//! The Symphony provides the generic skills, the recipes they run (`symphony.just`), the allow rules
//! (Claude's in `.claude/settings.json`, Codex's in `.codex/rules/symphony.rules`), the heartbeat
//! hook and the memory guard. Each file it writes carries a stamp, which is how a later run knows the
//! file is its own to bring up to date. A file of the same name without the stamp is the project's
//! own: it is the one in use and this never writes over it. A stamped one that differs from its
//! stamp, an edit since, is written over all the same, and the run says so and where the
//! project's own goes ([`Outcome::Replaced`]). Where the project has none, the
//! Symphony's is used. The project's own skills and guards sit beside the shipped ones and are never
//! touched, and so is a file of the project's in a shipped skill's folder that the stamp does not
//! name: `project.md` there is the project's standing instructions for that skill's role, which
//! the `work`, `review-task` and `assess-task` skills read first. It writes no permission rule but
//! the allow rules: each names a recipe `symphony.just` holds, its board and reply recipes and
//! `land`, never the server ones ([`ALLOW`]). A rule carries no stamp, so every rule a run finds is
//! the project's own but one a landed init wrote and this one dropped, which a run removes: an
//! allow rule ([`RETIRED`], the env-prefixed board rule, whose `*` passed any program) or an ask
//! rule ([`RETIRED_ASK`], those that asked before a land), an ask list they leave empty going with
//! them. What the Symphony's default land lands is decided by its checks, every stamp the task
//! needs standing on the tip it lands, never by a prompt; a project whose own `land` recipe
//! replaces it gets the same allow rule, and its own landing policy decides what lands.
//!
//! The recipes include the default `land` (`tasks land`), and `symphony.just` sets
//! `allow-duplicate-recipes`, so a recipe of the project's own of the same name replaces the
//! Symphony's with no setting the project adds; where the project's justfile sets it itself, the
//! copy init writes leaves it out (`just` refuses a setting made twice), so the import works either
//! way ([`recipes_for`]). A project with no justfile gets one whose one stamped line imports the
//! recipes; a justfile of the project's is never written, but for the one init wrote setting
//! `allow-duplicate-recipes` itself, which is brought current. Retiring the recipes removes the
//! stamped copy and that stamped line. The project's `.gitignore` gets a line
//! for each folder the Symphony fills that it does not ignore yet: the build links' `.target/` and
//! the artifacts folder; and one for the local settings file (`symphony.local.toml`), which git
//! never versions. A run refuses a root that is not a folder, or is in no git checkout (the
//! board lives in the repository's common dir; a git dir is git's own), making nothing. It ends with
//! what is left for the owner to do ([`steps`]) and what it cannot see from the folder, that Claude
//! Code must trust it ([`TRUST`]), and warns when the heartbeat has no build to run.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::builds;
use symphony_board::server::Engine;

/// Where the skills live in a project, and where Claude Code loads them from.
const SKILLS: &str = ".agents/skills";
const CLAUDE_SKILLS: &str = ".claude/skills";
const SETTINGS: &str = ".claude/settings.json";
const CODEX_RULES: &str = ".codex/rules/symphony.rules";
/// The Symphony's recipes, beside the project's justfile.
const RECIPES: &str = "symphony.just";
const IGNORE: &str = ".gitignore";
/// The folder of the build links (`builds::LINKS`), which the project's git ignores.
const TARGET: &str = ".target";

/// What marks a justfile's import of the recipes as the Symphony's.
const RECIPES_STAMP: &str = "# symphony-stamp: recipes";

/// The justfile a project without one gets: the import of the recipes, which a recipe written here
/// replaces (`land` among them) with no setting of its own: `symphony.just` sets
/// `allow-duplicate-recipes`, and under it just 1.58 takes the importing file's recipe wherever the
/// import stands. The stamped line is the Symphony's, taken out with the recipes.
const JUSTFILE: &str = "\
# The project's commands. The Symphony's are in symphony.just, which `symphony init` keeps current;
# a recipe of the same name here is the project's own and replaces the Symphony's, `land` included.
# The stamped line is the Symphony's: init takes it out if it stops shipping the recipes.
import 'symphony.just' # symphony-stamp: recipes
";

/// A justfile init wrote that sets `allow-duplicate-recipes` itself, which `symphony.just` now
/// sets (`just` refuses a setting made twice): init brings it to [`JUSTFILE`].
const JUSTFILE_SETTING_ITSELF: &str = "\
# The project's commands. The Symphony's are in symphony.just, which `symphony init` keeps current;
# a recipe of the same name here is the project's own and wins.
set allow-duplicate-recipes

import 'symphony.just'
";

/// Each recipe `symphony.just` holds that runs a binary, and that binary.
const RUNS: [(&str, &str); 4] = [
    ("tasks", "tasks"),
    ("agent-send", "agent-send"),
    ("symphony", "symphony-server"),
    ("symphony-cli", "symphony"),
];

/// The mark on a skill's `SKILL.md` and on the Codex rules file.
const MARK: &str = "symphony-stamp:";

/// The shell that sets `b` to the project's build of `symphony` as the recipes find it
/// ([`builds::find`]): the session's checkout's `.target/bin/symphony`, else its main checkout's
/// (a session started in a linked worktree with no links of its own), else `symphony` on `PATH`,
/// else empty. git runs only when the checkout has no link, and reads nothing of the event.
macro_rules! find_symphony {
    () => {
        "b=$CLAUDE_PROJECT_DIR/.target/bin/symphony; [ -x \"$b\" ] || { m=$(git -C \
         \"$CLAUDE_PROJECT_DIR\" rev-parse --path-format=absolute --git-common-dir 2>/dev/null \
         </dev/null); case $m in */.git) b=${m%/.git}/.target/bin/symphony;; esac; [ -x \"$b\" ] \
         || b=$(command -v symphony) || b=; }; "
    };
}

/// A hook the Symphony installs in `.claude/settings.json`. Each runs the project's build of
/// `symphony` ([`find_symphony`]) and builds nothing.
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

/// The heartbeat: with no build to run it passes quietly, and it never refuses a call.
const HEARTBEAT: Hook = Hook {
    name: "tasks-beat",
    command: concat!(
        find_symphony!(),
        "[ -z \"$b\" ] || \"$b\" hook tasks-beat 2>/dev/null || true # symphony-stamp: heartbeat-v2"
    ),
    stamp: "# symphony-stamp: heartbeat-",
    timeout: 10,
    events: &[("PreToolUse", "Bash"), ("PostToolUse", "")],
};

/// The memory guard (`crate::hook`). Its command passes on the guard's refusal, its one line and exit
/// 2, and nothing else: with no build to run, or one built before the guard (whose `hook` refuses
/// the name with its usage, exit 2 too), the call goes through.
const MEMORY_GUARD: Hook = Hook {
    name: "memory-guard",
    command: concat!(
        find_symphony!(),
        "[ -z \"$b\" ] || { r=$(\"$b\" hook memory-guard 2>&1 >/dev/null) || case $r in Blocked:*) \
         printf '%s\\n' \"$r\" >&2; exit 2;; esac; } # symphony-stamp: memory-guard-v2"
    ),
    stamp: "# symphony-stamp: memory-guard-",
    timeout: 10,
    events: &[("PreToolUse", "Write|Edit|MultiEdit|NotebookEdit|Bash")],
};

const HOOKS: [Hook; 2] = [HEARTBEAT, MEMORY_GUARD];

/// What Claude allows without a prompt: the board and reply recipes of `symphony.just`, and `land`.
/// The Symphony's default land lands only what its checks pass, every stamp the task needs
/// standing on the tip, so a prompt before it would add only a wait on the owner; a
/// project's own `land` recipe, which replaces it, is allowed the same, and the project's landing
/// policy decides what that one lands. Its server recipes, `symphony` (which starts, replaces or
/// stops the machine's shared server) and `symphony-cli` (which reaches `serve` and `init`), ask
/// first. `just tasks` passes every board command but the recorder
/// (`tasks run`), which runs any command it is handed: the recipe refuses it, in each spelling
/// `tasks` reads, so no rule here need ask for it. The board and reply recipes keep each place
/// they write in the checkout, out of its git folders (`symphony_board::confine`). Every rule
/// starts with the recipe's words, never a `*`: Claude Code's `*` matches any text, spaces
/// included, and only the words before the first one bound a rule. An agent names its handle with
/// `--as HANDLE`, which binds its heartbeat as an env prefix did.
const ALLOW: [&str; 6] = [
    "Bash(just tasks:*)",
    "Bash(just agent-send:*)",
    "Bash(just symphony-status:*)",
    "Bash(just symphony-reply:*)",
    "Bash(just symphony-ask:*)",
    "Bash(just land:*)",
];

/// Allow rules a landed init wrote that [`ALLOW`] no longer holds, removed wherever a run finds
/// them ([`retire`]). A rule carries no stamp, so one goes here only once an init on the default
/// branch wrote it; any other copy a run finds is the project's own and stays. The env-prefixed
/// board rule bound nothing past its first word: its `*` takes any text, so it passed
/// `TASKS_SESSION=x sh -c '…' just tasks list` and `TASKS_SESSION=x JUST_JUSTFILE=… just tasks …`,
/// any program at all.
const RETIRED: [&str; 1] = ["Bash(TASKS_SESSION=* just tasks *)"];

/// The ask rules a landed init wrote, which none writes now, removed wherever a run finds them
/// ([`retire`]): the one stopping any command that named the marker the `land` recipe sets
/// (`symphony_board::land::RECIPE`), and those stopping `just tasks land`, bare or after the
/// handle, with or without an option (`--as H`, `--database NAME`) before `land`. The marker
/// stays, so `tasks land` run by itself still refuses; the prompts only made an approved land wait
/// on the owner. Any other ask rule is the project's own and stays.
const RETIRED_ASK: [&str; 9] = [
    "Bash(*SYMPHONY_LAND*)",
    "Bash(just tasks land)",
    "Bash(just tasks land *)",
    "Bash(just tasks -* land)",
    "Bash(just tasks -* land *)",
    "Bash(TASKS_SESSION=* just tasks land)",
    "Bash(TASKS_SESSION=* just tasks land *)",
    "Bash(TASKS_SESSION=* just tasks -* land)",
    "Bash(TASKS_SESSION=* just tasks -* land *)",
];

/// Takes each of the rules `retired` out of the rule list `rules`, leaving every other entry in its
/// place. Whether it took any.
fn retire(rules: &mut Vec<Value>, retired: &[&str]) -> bool {
    let had = rules.len();
    rules.retain(|have| !have.as_str().is_some_and(|rule| retired.contains(&rule)));
    rules.len() != had
}

/// Takes the retired ask rules ([`RETIRED_ASK`]) out of `permissions.ask`, and the list with them
/// when they leave it empty; a list that was empty before is the project's and stays. A list that
/// is not one is not the shape Claude Code reads. Whether it changed anything.
fn retire_ask(
    permissions: &mut Map<String, Value>,
    not_list: impl Fn() -> io::Error,
) -> io::Result<bool> {
    let Some(ask) = permissions.get_mut("ask") else {
        return Ok(false);
    };
    let ask = ask.as_array_mut().ok_or_else(not_list)?;
    if !retire(ask, &RETIRED_ASK) {
        return Ok(false);
    }
    if ask.is_empty() {
        permissions.shift_remove("ask");
    }
    Ok(true)
}

/// One shipped skill: its folder name and its files by path inside it (`SKILL.md` first).
pub struct Skill {
    pub name: String,
    pub files: Vec<(String, String)>,
}

/// Everything the Symphony provides to a project.
pub struct Shipped {
    pub skills: Vec<Skill>,
    pub codex_rules: String,
    /// `symphony.just`: the recipes the skills, docs and messages name, the default `land` among
    /// them; none retires the stamped copy and the justfile's stamped import of it.
    pub recipes: Option<String>,
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
                // The assessor's skill: the stamps a task needs to land, read from its diff.
                skill!("assess-task"),
                skill!("chat-doc"),
                skill!("create-task"),
                skill!("review-task"),
                skill!("task-orchestrator"),
                skill!("tasks"),
                skill!("triage"),
                // The executor's skill: a short page naming the one page of each stage.
                skill!(
                    "work",
                    "start.md",
                    "build.md",
                    "connected.md",
                    "finish.md",
                    "rare-endings.md"
                ),
            ],
            codex_rules: include_str!("../shipped/codex.rules").to_string(),
            recipes: Some(include_str!("../shipped/symphony.just").to_string()),
        }
    }
}

/// What one run did to one thing.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Installed,
    /// A copy init wrote, as it wrote it, brought to what this build ships.
    Updated,
    /// A copy init wrote that differed from its stamp, an edit since (a file of a skill gone
    /// among them), written whole again: the edit is gone. It names where the project's own goes.
    Replaced(&'static str),
    Current,
    /// The project's own copy, left as it is.
    Own,
    Removed,
}

impl Outcome {
    fn word(&self) -> String {
        match self {
            Outcome::Installed => "installed".into(),
            Outcome::Updated => "updated".into(),
            Outcome::Replaced(own) => {
                format!("replaced: it differed from its stamp, so an edit to it is gone; {own}")
            }
            Outcome::Current => "current".into(),
            Outcome::Own => "kept (the project's own)".into(),
            Outcome::Removed => "removed".into(),
        }
    }
}

/// Where a skill of the project's own goes, said when an edit to a stamped one is replaced.
const OWN_SKILL: &str = "a skill of the project's own is a SKILL.md with no stamp line, which init \
                         never writes over";
/// Where the project's own recipes go.
const OWN_RECIPES: &str = "a recipe of the project's own goes in its justfile, where it replaces \
                           the Symphony's of the same name";
/// Where the project's own Codex rules go.
const OWN_RULES: &str = "rules of the project's own are a copy with no stamp line, which init \
                         never writes over";

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

/// Whether `have`, a file init writes whole with its stamp on its first line ([`stamped_rules`]),
/// differs from that stamp: the stamp is the digest of the text below it as init wrote it, so any
/// other text there is an edit since.
fn differs_from_stamp(have: &str) -> bool {
    let (stamp, text) = have.split_once('\n').unwrap_or((have, ""));
    let named = stamp
        .strip_prefix("# ")
        .and_then(|rest| rest.strip_prefix(MARK))
        .map(str::trim);
    named != Some(digest(&[text]).as_str())
}

/// Whether the skill in `folder`, whose stamped `SKILL.md` reads `have`, differs from its stamp
/// ([`stamped_skill`]): the stamp is the digest of the files it names as init wrote them, the
/// `SKILL.md` without the stamp's line, so an edit to any of them, or one of them gone, is a
/// difference.
fn skill_differs_from_stamp(folder: &Path, have: &str) -> io::Result<bool> {
    let Some((line, named)) = have.lines().find_map(|line| {
        let rest = line.strip_prefix("<!-- ")?.strip_prefix(MARK)?;
        Some((line, rest.trim_start().split_once(" files=")?.0))
    }) else {
        return Ok(true);
    };
    let skill = have.replacen(&format!("{line}\n"), "", 1);
    let mut parts = Vec::new();
    for path in stamped_files(have) {
        let Some(body) = (if path == "SKILL.md" {
            Some(skill.clone())
        } else {
            read_optional(&folder.join(path))?
        }) else {
            return Ok(true);
        };
        parts.push((path, body));
    }
    let parts: Vec<&str> = parts
        .iter()
        .flat_map(|(path, body)| [*path, body.as_str()])
        .collect();
    Ok(digest(&parts) != named)
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
                let edited = match &have {
                    Some(old) => skill_differs_from_stamp(&folder, old)?,
                    None => false,
                };
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
                written(wrote, have.is_some(), edited.then_some(OWN_SKILL))
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

/// A file the Symphony writes whole, its stamp on its first line (the Codex rules, the recipes),
/// reported as `entry`; `own` says where the project's own goes, should an edit be replaced.
fn install_stamped(
    root: &Path,
    rel: &str,
    entry: &str,
    text: &str,
    own: &'static str,
    report: &mut Report,
) -> io::Result<()> {
    let path = root.join(rel);
    let have = read_optional(&path)?;
    let outcome = match &have {
        _ if is_link(&path) => Outcome::Own,
        Some(text) if !text.lines().next().is_some_and(is_mark_line) => Outcome::Own,
        _ => {
            let edited = have.as_deref().is_some_and(differs_from_stamp);
            let wrote = put(&path, &stamped_rules(text))?;
            written(wrote, have.is_some(), edited.then_some(own))
        }
    };
    report.push((entry.to_string(), outcome));
    Ok(())
}

/// What a write did: nothing, or brought an old copy up to date, or wrote over an edit to one
/// (`edited`, naming where the project's own goes), or made the file.
fn written(wrote: bool, existed: bool, edited: Option<&'static str>) -> Outcome {
    match (wrote, existed, edited) {
        (false, _, _) => Outcome::Current,
        (true, true, Some(own)) => Outcome::Replaced(own),
        (true, true, None) => Outcome::Updated,
        (true, false, _) => Outcome::Installed,
    }
}

/// The project's justfile at `root`, by any name `just` reads one by: `justfile` in any case, or
/// `.justfile`.
fn justfile(root: &Path) -> io::Result<Option<PathBuf>> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(at(root, error)),
    };
    let mut names: Vec<String> = entries
        .collect::<io::Result<Vec<_>>>()
        .map_err(|error| at(root, error))?
        .into_iter()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| matches!(name.to_lowercase().as_str(), "justfile" | ".justfile"))
        .collect();
    names.sort();
    Ok(names.first().map(|name| root.join(name)))
}

/// Whether a justfile's `text` imports the recipes.
fn imports(text: &str) -> bool {
    imported(text)
        .into_iter()
        .any(|path| path.strip_prefix("./").unwrap_or(path) == RECIPES)
}

/// The setting `symphony.just` makes, on a line of its own, so a recipe of the project's own of the
/// same name replaces the Symphony's.
const SETTING: &str = "set allow-duplicate-recipes\n";
/// That line in the copy for a project whose justfile makes the setting itself.
const SETTING_LEFT: &str =
    "# set allow-duplicate-recipes: the project's justfile sets it, and `just` refuses it twice\n";

/// The recipes as the project at `root` takes them: `shipped`, but where the project's justfile, or
/// a file it imports, sets `allow-duplicate-recipes` itself (`just` refuses a setting made twice,
/// which fails every recipe), with that setting left to it. So the import works whether the project
/// sets it or not, and a recipe of its own replaces the Symphony's either way.
fn recipes_for(root: &Path, shipped: &str) -> io::Result<String> {
    if project_sets_duplicates(root)? {
        Ok(shipped.replacen(SETTING, SETTING_LEFT, 1))
    } else {
        Ok(shipped.to_string())
    }
}

/// Whether the project's justfile at `root`, or a file it imports other than the recipes, sets
/// `allow-duplicate-recipes`; never the one init wrote setting it, which this run brings to
/// [`JUSTFILE`].
fn project_sets_duplicates(root: &Path) -> io::Result<bool> {
    let Some(path) = justfile(root)? else {
        return Ok(false);
    };
    let Some(text) = read_optional(&path)? else {
        return Ok(false);
    };
    if text == JUSTFILE_SETTING_ITSELF && !is_link(&path) {
        return Ok(false);
    }
    if sets_duplicates(&text) {
        return Ok(true);
    }
    let folder = path.parent().unwrap_or(root);
    for imported in imported(&text) {
        if folder.join(imported) == root.join(RECIPES) {
            continue;
        }
        if read_optional(&folder.join(imported))?.is_some_and(|text| sets_duplicates(&text)) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The paths a justfile's `text` imports, as written.
fn imported(text: &str) -> Vec<&str> {
    text.lines()
        .filter_map(|line| {
            let line = line.split('#').next().unwrap_or_default().trim();
            let path = line
                .strip_prefix("import?")
                .or_else(|| line.strip_prefix("import"))
                .filter(|rest| rest.starts_with(char::is_whitespace))?;
            Some(path.trim().trim_matches(['\'', '"']))
        })
        .collect()
}

/// Whether a justfile's `text` sets `allow-duplicate-recipes` itself.
fn sets_duplicates(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.split('#').next().unwrap_or_default().trim();
        line.strip_prefix("set")
            .filter(|rest| rest.starts_with(char::is_whitespace))
            .and_then(|rest| rest.trim_start().strip_prefix("allow-duplicate-recipes"))
            .is_some_and(|rest| rest.is_empty() || rest.trim_start().starts_with(":="))
    })
}

/// A justfile importing the recipes where the project has none; the project's own is left as it
/// is, but for one init wrote setting what `symphony.just` now sets, which is brought current. One
/// that is still what init writes is current: the project's once it changes it.
fn install_justfile(root: &Path, report: &mut Report) -> io::Result<()> {
    let outcome = match justfile(root)? {
        Some(path) if is_link(&path) => Outcome::Own,
        Some(path) => match read_optional(&path)?.as_deref() {
            Some(JUSTFILE_SETTING_ITSELF) => {
                put(&path, JUSTFILE)?;
                Outcome::Updated
            }
            Some(JUSTFILE) => Outcome::Current,
            _ => Outcome::Own,
        },
        None => {
            put(&root.join("justfile"), JUSTFILE)?;
            Outcome::Installed
        }
    };
    report.push(("justfile".to_string(), outcome));
    Ok(())
}

/// The recipes the Symphony no longer delivers: a stamped `symphony.just` goes, and then the
/// justfile's stamped import of it, the whole file when it is still the one init wrote. A copy of
/// the recipes the project made its own stays, imported as it is.
fn retire_recipes(root: &Path, report: &mut Report) -> io::Result<()> {
    let path = root.join(RECIPES);
    if is_link(&path) {
        report.push((RECIPES.to_string(), Outcome::Own));
        return Ok(());
    }
    match read_optional(&path)? {
        Some(text) if !text.lines().next().is_some_and(is_mark_line) => {
            report.push((RECIPES.to_string(), Outcome::Own));
            return Ok(());
        }
        Some(_) => {
            remove_file(&path)?;
            report.push((RECIPES.to_string(), Outcome::Removed));
        }
        None => {}
    }
    let Some(justfile) = justfile(root)? else {
        return Ok(());
    };
    if is_link(&justfile) {
        return Ok(());
    }
    let Some(text) = read_optional(&justfile)? else {
        return Ok(());
    };
    if text == JUSTFILE {
        remove_file(&justfile)?;
        report.push(("justfile".to_string(), Outcome::Removed));
        return Ok(());
    }
    let kept: String = text
        .split_inclusive('\n')
        .filter(|line| !line.trim_end().ends_with(RECIPES_STAMP))
        .collect();
    if kept != text {
        put(&justfile, &kept)?;
        report.push(("justfile".to_string(), Outcome::Updated));
    }
    Ok(())
}

/// Whether a `.gitignore`'s `text` ignores the folder `folder` (project-relative) whole.
fn ignores(text: &str, folder: &str) -> bool {
    text.lines().map(str::trim).any(|line| {
        let line = line.strip_prefix('/').unwrap_or(line);
        let line = ["/**", "/*", "/"]
            .iter()
            .find_map(|end| line.strip_suffix(end))
            .unwrap_or(line);
        line == folder
    })
}

/// Whether a `.gitignore`'s `text` ignores the regular file `name` at the project's root: a line
/// that is its name, anchored or not. A folder's rule of that name (`name/`, `name/*`) ignores no
/// file, so it does not count.
fn ignores_file(text: &str, name: &str) -> bool {
    text.lines()
        .map(str::trim)
        .any(|line| line.strip_prefix('/').unwrap_or(line) == name)
}

/// A line in the project's `.gitignore` for each folder the Symphony fills that it does not ignore
/// yet, the build links' and the artifacts folder `artifacts`, and for the local settings file
/// ([`crate::project::LOCAL_FILE`]), which is one machine's and never versioned.
fn install_ignores(root: &Path, artifacts: &str, report: &mut Report) -> io::Result<()> {
    let path = root.join(IGNORE);
    if is_link(&path) {
        report.push((IGNORE.to_string(), Outcome::Own));
        return Ok(());
    }
    let have = read_optional(&path)?;
    let mut text = have.clone().unwrap_or_default();
    let local = crate::project::LOCAL_FILE;
    let missing: Vec<String> = [TARGET, artifacts]
        .into_iter()
        .filter(|folder| !ignores(&text, folder))
        .map(|folder| format!("/{folder}/\n"))
        .chain((!ignores_file(&text, local)).then(|| format!("/{local}\n")))
        .collect();
    if !missing.is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.extend(missing);
    }
    let outcome = written(put(&path, &text)?, have.is_some(), None);
    report.push((IGNORE.to_string(), outcome));
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
    changed |= retire(allow, &RETIRED);
    for rule in ALLOW {
        if !allow.iter().any(|have| have.as_str() == Some(rule)) {
            allow.push(json!(rule));
            changed = true;
        }
    }
    changed |= retire_ask(permissions, not_object)?;

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
    install_stamped(
        root,
        CODEX_RULES,
        "codex rules",
        &shipped.codex_rules,
        OWN_RULES,
        &mut report,
    )?;
    install_settings(root, &mut report)?;
    match &shipped.recipes {
        Some(recipes) => {
            let recipes = recipes_for(root, recipes)?;
            install_stamped(root, RECIPES, RECIPES, &recipes, OWN_RECIPES, &mut report)?;
            install_justfile(root, &mut report)?;
        }
        None => retire_recipes(root, &mut report)?,
    }
    let artifacts = crate::project::Project::load(root)
        .map_or_else(|_| "artifacts".to_string(), |project| project.artifacts);
    install_ignores(root, &artifacts, &mut report)?;
    Ok(report)
}

/// What the machine tells [`steps`]: its Symphony's folder (the registry's), the search path, the
/// SurrealDB `SYMPHONY_SURREAL` names and the folder of the `symphony` build running init, where a
/// Symphony checkout's build of the others is too.
pub struct Machine<'a> {
    /// None when the machine has no Symphony folder to read.
    pub folder: Option<&'a Path>,
    pub path: Option<&'a OsStr>,
    pub surreal: Option<&'a Path>,
    pub beside: Option<&'a Path>,
}

impl Machine<'_> {
    /// What a search for a build reads past a checkout's links: this machine's folder, whose
    /// `current` names the release it runs, and its search path.
    pub fn beyond(&self) -> builds::Beyond {
        builds::Beyond {
            machine: self.folder.map(Path::to_path_buf),
            path: self.path.map(OsStr::to_os_string),
        }
    }
}

/// The engine the board of the checkout at `root` runs on, read from its home without starting
/// anything ([`symphony_board::server::Home::backend`]): SurrealDB for a board that holds its
/// store, PostgreSQL for one whose home names it and for a board not made yet, which is made on
/// it. None outside a checkout, or for a home a board command refuses, which says why itself.
fn board_engine(root: &Path, named: Option<&Path>) -> Option<Engine> {
    let common = symphony_board::git::checkout_common_dir(root)?;
    symphony_board::server::Home::in_common(&common, named)
        .backend()
        .ok()
}

/// The step that makes a board not made yet.
const MAKE_BOARD: &str = "make the board: the first board command makes it, on PostgreSQL \
                          (`just tasks list`); the Symphony only reads a board and never makes one";

/// The step for the PostgreSQL the board runs on, which discovery did not find as `why` says:
/// the machine's own, 18 or later, by the project file's `[board] postgresql` or `PATH`
/// ([`symphony_board::server::postgres::discover_programs`]), whose refusal carries its setup.
fn postgresql_step(why: &str) -> String {
    format!(
        "have PostgreSQL {} or later for the board, which runs on the machine's own: {why}",
        symphony_board::server::postgres::OLDEST_MAJOR
    )
}

/// The SurrealDB the board of the checkout at `root` would start, when it is not there to run:
/// `named` (`SYMPHONY_SURREAL`), else the pinned one in the repository's common dir. None outside a
/// checkout. Asked only of a board on SurrealDB ([`board_engine`]).
fn missing_surreal(root: &Path, named: Option<&Path>) -> Option<PathBuf> {
    let common = symphony_board::git::checkout_common_dir(root)?;
    let home = symphony_board::server::Home::in_common(&common, named);
    (!builds::runnable(home.surreal())).then(|| home.surreal().to_path_buf())
}

/// Whether `root` lies in its repository's git dir `common` (canonical) and in no checkout there:
/// a bare repository, or a folder of a checkout's `.git`, which git answers a common dir for too.
/// A worktree placed inside the git dir has a `.git` of its own, and is a checkout.
fn in_git_dir(root: &Path, common: &Path) -> bool {
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    root.starts_with(common)
        && !root
            .ancestors()
            .take_while(|folder| folder.starts_with(common))
            .any(|folder| folder.join(".git").symlink_metadata().is_ok())
}

/// Whether the board of the checkout at `root`, its server run from `named` (`SYMPHONY_SURREAL`)
/// or the pinned one, is not made yet, read without starting anything: by the server's answer
/// while one runs (a board's server may run before its board is made), else by its home holding
/// neither engine's data ([`symphony_board::server::Home::unmade`]): a stopped board, on either
/// engine, is made. False outside a checkout.
fn unmade_board(root: &Path, named: Option<&Path>) -> bool {
    let Some(common) = symphony_board::git::checkout_common_dir(root) else {
        return false;
    };
    let home = symphony_board::server::Home::in_common(&common, named);
    match home.schema(symphony_board::DATABASE) {
        Ok(schema) => schema.is_none(),
        Err(_) => home.unmade().unwrap_or(false),
    }
}

/// The builds a project runs: the hooks' first (they run `symphony`, whose heartbeat runs `tasks`),
/// then the other ones [`RUNS`] names. These are the binaries the workspace delivers, each held to
/// a size budget (`symphony-build size-budget`, docs/workflow/CHECKS.md "Size budgets"). The workspace's
/// `symphony-build`, the commands only its own build and release run use, is none of them.
pub const BUILDS: [&str; 4] = ["symphony", "tasks", "agent-send", "symphony-server"];

/// What runs the build `name`: the hooks that do, then each recipe of [`RUNS`] that does but those
/// the project's justfile has of its own (`own`), which replace the Symphony's.
fn runners(name: &str, own: &[&str]) -> Vec<String> {
    let hooks: &[&str] = match name {
        "symphony" => &["the heartbeat", "the memory guard"],
        "tasks" => &["the heartbeat"],
        _ => &[],
    };
    let recipes = RUNS
        .iter()
        .filter(|(recipe, binary)| *binary == name && !own.contains(recipe))
        .map(|(recipe, _)| format!("`just {recipe}`"));
    hooks
        .iter()
        .map(ToString::to_string)
        .chain(recipes)
        .collect()
}

/// Of `names`, the builds the checkout at `root` has none of where the recipes and the hooks look
/// ([`builds::find`]), past its links in `beyond`.
fn unfound<'a>(root: &Path, names: &[&'a str], beyond: &builds::Beyond) -> Vec<&'a str> {
    names
        .iter()
        .copied()
        .filter(|name| builds::find(root, name, beyond).is_none())
        .collect()
}

/// The recipes a justfile's text defines itself: a line starting with the name, then any
/// parameters, then `:`, never `:=` (an assignment or a setting).
fn defined(text: &str) -> Vec<&str> {
    text.lines()
        .filter_map(|line| {
            let word =
                |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-';
            let (name, rest) = line.split_at(line.find(|c: char| !word(c)).unwrap_or(line.len()));
            let rest = if rest.starts_with(char::is_whitespace) {
                rest.trim_start_matches(|c| c != ':')
            } else {
                rest
            };
            (name.starts_with(|c: char| c.is_ascii_lowercase())
                && rest.starts_with(':')
                && !rest.starts_with(":="))
            .then_some(name)
        })
        .collect()
}

/// The heartbeat's builds.
const HEARTBEAT_BUILDS: [&str; 2] = ["symphony", "tasks"];

/// Where a Symphony checkout's `cargo build --release` leaves its builds.
const RELEASE: &str = ".target/release";

/// The reference of the project file's tables and keys, docs/operations/PROJECT_FILE.md, where a project
/// that has no Symphony checkout reads it: in the Symphony's repository.
pub const PROJECT_FILE_REFERENCE: &str = concat!(
    env!("CARGO_PKG_REPOSITORY"),
    "/blob/main/docs/operations/PROJECT_FILE.md"
);

/// What is left for the owner to do after a run at `root`, each one line: what init cannot do for
/// the project. None left means agents can run the board.
pub fn steps(root: &Path, machine: &Machine) -> Vec<String> {
    steps_within(root, machine, crate::machine::command::LOOK)
}

/// [`steps`], with each front and board probe given `look` to connect and answer.
pub(crate) fn steps_within(
    root: &Path,
    machine: &Machine,
    look: std::time::Duration,
) -> Vec<String> {
    let mut steps = Vec::new();
    let project = match crate::project::Project::load(root) {
        Ok(project) => Some(project),
        Err(problem) => {
            steps.push(format!("fix {}: {problem}", crate::project::FILE));
            None
        }
    };
    if let Some(project) = &project {
        if !root.join(crate::project::FILE).is_file() {
            steps.push(format!(
                "write {} to name the project (until then it is `{}`, after its folder) and to set \
                 its agents, verdicts and board: {PROJECT_FILE_REFERENCE} lists its tables and keys",
                crate::project::FILE,
                project.id
            ));
        }
        for entry in &project.entrypoints {
            if !root.join(&entry.folder).is_dir() {
                let url = if entry.url.is_empty() {
                    String::new()
                } else {
                    format!("{}/", entry.url)
                };
                steps.push(format!(
                    "make {}/ for the documents the Symphony serves at /{}/{url}, or name the \
                     folders it serves with [[entrypoints]] in {}",
                    entry.folder,
                    project.id,
                    crate::project::FILE
                ));
            }
        }
    }
    let mut text = String::new();
    if let Ok(Some(justfile)) = justfile(root) {
        text = std::fs::read_to_string(&justfile).unwrap_or_default();
        let name = justfile.file_name().unwrap_or_default().to_string_lossy();
        if !imports(&text) {
            steps.push(format!(
                "import the recipes in {name}: add `import '{RECIPES}'`; a recipe of its own of the \
                 same name (`land`, say) replaces the Symphony's"
            ));
        }
    }
    // A build only recipes the project replaced run is never run: nothing asks for it.
    let own = defined(&text);
    let wanted: Vec<&str> = BUILDS
        .into_iter()
        .filter(|name| !runners(name, &own).is_empty())
        .collect();
    steps.extend(build_steps(
        &unfound(root, &wanted, &machine.beyond()),
        &own,
        machine.beside,
    ));
    if builds::on_path("just", machine.path).is_none() {
        steps.push(
            "install `just` (https://just.systems) and put it on PATH: the skills, the agents and \
             the owner run the board and the Symphony by its recipes (`just tasks`, `just symphony`)"
                .to_string(),
        );
    }
    match board_engine(root, machine.surreal) {
        Some(Engine::SurrealDb) => {
            if let Some(surreal) = missing_surreal(root, machine.surreal) {
                let fetch = machine.surreal.is_none().then(|| {
                    symphony_board::server::surreal_fetch(
                        &surreal,
                        std::env::consts::OS,
                        std::env::consts::ARCH,
                    )
                });
                steps.push(surreal_step(
                    &surreal,
                    machine.surreal.is_some(),
                    fetch.flatten(),
                ));
            }
        }
        Some(Engine::PostgreSql) => {
            if let Err(why) =
                symphony_board::server::postgres::discover_with_path(root, machine.path)
            {
                steps.push(postgresql_step(&why));
            }
        }
        None => {}
    }
    if unmade_board(root, machine.surreal) {
        steps.push(MAKE_BOARD.to_string());
    }
    if let Some(folder) = machine.folder {
        let registered = registered(root, folder);
        if !registered {
            steps.push(
                "register the project with the machine's Symphony: `just symphony-cli project add` \
                 (a start of `just symphony` registers it too)"
                    .to_string(),
            );
        }
        if let Some(front) = crate::machine::command::running_within(folder, None, look) {
            if let Some(writer) = builds::find(root, "tasks", &machine.beyond())
                && let Some((_, release)) = crate::machine::release::of_binary(&writer)
                && release.serves.schema < front.schema
            {
                steps.push(format!(
                    "link a compatible board command: {} selects {} at schema {}, but the \
                     running host requires schema {}; after updating the project's writer, \
                     `just tasks migrate` in its main checkout moves the board. Init never \
                     changes the writer or migrates the board",
                    writer.display(),
                    release.release,
                    release.serves.schema,
                    front.schema
                ));
            }
            if registered
                && let Some(project) = &project
                && let Err(why) =
                    crate::machine::command::board_readiness_within(&front, &project.id, look)
            {
                steps.push(why);
            }
        }
    }
    steps
}

/// `names` as words, each made by `word`, the last two joined by "and".
fn names_and(names: &[&str], word: impl Fn(&str) -> String) -> String {
    let words: Vec<String> = names.iter().map(|name| word(name)).collect();
    match words.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        Some((last, _)) => last.clone(),
        None => String::new(),
    }
}

/// The steps for the builds `unfound`, which the project has nowhere, its justfile having the
/// recipes `own`: one to link them, with the commands for those the folder `beside` holds (the
/// running `symphony`'s), and one to build the others in a checkout of the Symphony.
fn build_steps(unfound: &[&str], own: &[&str], beside: Option<&Path>) -> Vec<String> {
    if unfound.is_empty() {
        return Vec::new();
    }
    let mut who: Vec<String> = Vec::new();
    for runner in unfound.iter().flat_map(|name| runners(name, own)) {
        if !who.contains(&runner) {
            who.push(runner);
        }
    }
    let who: Vec<&str> = who.iter().map(String::as_str).collect();
    let them = if unfound.len() == 1 { "it" } else { "them" };
    let (run, build) = if who.len() == 1 {
        ("runs", "builds")
    } else {
        ("run", "build")
    };
    let ready: Vec<(&str, PathBuf)> = unfound
        .iter()
        .filter_map(|name| {
            let found = beside?.join(name);
            builds::runnable(&found).then_some((*name, found))
        })
        .collect();
    let links = if ready.is_empty() {
        String::new()
    } else {
        let commands: Vec<String> = ready
            .iter()
            .map(|(name, found)| {
                let quoted = found.to_string_lossy().replace('\'', "'\\''");
                format!("ln -s '{quoted}' {}/{name}", builds::LINKS)
            })
            .collect();
        format!(
            " (`mkdir -p {} && {}`)",
            builds::LINKS,
            commands.join(" && ")
        )
    };
    let mut steps = vec![format!(
        "link the project's build{} at {}, or put {them} on PATH: {} {run} {them} and {build} \
         nothing{links}",
        if unfound.len() == 1 { "" } else { "s" },
        names_and(unfound, |name| format!("{}/{name}", builds::LINKS)),
        names_and(&who, str::to_string),
    )];
    let unbuilt: Vec<&str> = unfound
        .iter()
        .copied()
        .filter(|name| !ready.iter().any(|(built, _)| built == name))
        .collect();
    if !unbuilt.is_empty() {
        let bins: Vec<String> = unbuilt.iter().map(|name| format!("--bin {name}")).collect();
        steps.push(format!(
            "build {} to link in a checkout of the Symphony: `cargo build --locked --release {}` \
             there leaves {} in its {RELEASE}/",
            names_and(&unbuilt, |name| format!("`{name}`")),
            bins.join(" "),
            if unbuilt.len() == 1 { "it" } else { "them" },
        ));
    }
    steps
}

/// The step for the SurrealDB the board's server runs from `surreal`, which is not there: `named`
/// when `SYMPHONY_SURREAL` names it, else the pinned one, with `fetch` the shell that fetches it for
/// this machine ([`symphony_board::server::surreal_fetch`]).
fn surreal_step(surreal: &Path, named: bool, fetch: Option<String>) -> String {
    let version = symphony_board::server::SURREAL_VERSION;
    let at = surreal.display();
    match (named, fetch) {
        (true, _) => format!(
            "put SurrealDB {version} at {at}, where SYMPHONY_SURREAL names it: the board's server \
             runs it"
        ),
        (false, Some(fetch)) => format!(
            "fetch SurrealDB {version}, which the board's server runs from {at}: `{fetch}` (or name \
             another with SYMPHONY_SURREAL)"
        ),
        (false, None) => format!(
            "put SurrealDB {version}'s release binary for this machine at {at}, where the board's \
             server runs it, or name one with SYMPHONY_SURREAL"
        ),
    }
}

/// Whether the machine's registry in `folder` holds the project of `root`'s main checkout. A
/// registry that cannot be read holds nothing.
fn registered(root: &Path, folder: &Path) -> bool {
    let Ok(main) = crate::machine::command::main_checkout(root).canonicalize() else {
        return false;
    };
    crate::machine::Registry::read(folder)
        .is_ok_and(|registry| registry.projects.iter().any(|entry| entry.root == main))
}

/// The warning a run at `root` ends with when the heartbeat has no build to run there, each found
/// as the hooks find it, past its links in `beyond`: `symphony` as the hooks' own shell does
/// ([`find_symphony`]: the links, then the search path, never the machine's release), and `tasks`
/// as the beat does ([`builds::find`], the machine's release before the search path). So a project
/// with no build of its own is warned though a release is installed: its recipes run the release,
/// and its hooks run nothing.
pub fn heartbeat_warning(root: &Path, beyond: &builds::Beyond) -> Option<String> {
    let shell = builds::Beyond::path(beyond.path.as_deref());
    let missing: Vec<&str> = HEARTBEAT_BUILDS
        .into_iter()
        .filter(|name| {
            let past = if *name == "symphony" { &shell } else { beyond };
            builds::find(root, name, past).is_none()
        })
        .collect();
    if missing.is_empty() {
        return None;
    }
    let names = names_and(&missing, str::to_string);
    let (have, they, do_) = if missing.len() == 1 {
        ("has", "it", "does")
    } else {
        ("have", "they", "do")
    };
    let guard = if missing.contains(&"symphony") {
        " and the memory guard refuses nothing"
    } else {
        ""
    };
    Some(format!(
        "warning: {names} {have} no build at {}/ or on PATH: no tool call counts as a beat{guard} \
         until {they} {do_} (see Next)",
        builds::LINKS
    ))
}

/// What a run says after [`next`], whatever is left: what init cannot see from the folder.
const TRUST: &str = "Claude Code counts the allow rules only in a folder it trusts: trust this one \
                     once, at the prompt its first session there shows. Until then an agent the \
                     board launches there stops at that prompt, and its launch fails.";

/// The run's last words: the steps left, one a line, or that none is.
fn next(steps: &[String]) -> String {
    if steps.is_empty() {
        return "Next: nothing; agents can run the board (`just tasks register`).".to_string();
    }
    let mut text = "Next:".to_string();
    for step in steps {
        text.push_str(&format!("\n- {step}"));
    }
    text
}

const USAGE: &str = "usage: symphony init [-h] [--root ROOT]";
const HELP: &str = "Install the agents' integration files and keep them current: the skills, the
recipes (symphony.just, and a justfile importing it where the project has none), the allow rules,
the heartbeat and the memory guard. Then say what is left to do.

options:
  -h, --help   show this help message and exit
  --root ROOT  the project's root, a folder that is there (default: the current folder)

A file of the same name without the Symphony's stamp is the project's own: it is used and never
written over.";

/// The project's root a run names: `--root`'s value (else the current folder), read from `cwd`
/// and resolved, so a project named after its folder is named after this one, never `.`. One
/// that cannot be resolved is left as given, for the run to refuse: init never makes the folder.
fn project_root(given: Option<&str>, cwd: &Path) -> PathBuf {
    let root = cwd.join(given.unwrap_or("."));
    root.canonicalize().unwrap_or(root)
}

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
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = project_root(parsed.value("--root"), &cwd);
    if !root.is_dir() {
        let problem = format!(
            "no folder at {}: --root names the project's folder, which init never makes",
            root.display()
        );
        return refuse(err, USAGE, "init", &problem);
    }
    let common = match symphony_board::git::common_dir(&root) {
        Ok(common) => common,
        Err(outside) => {
            let problem = format!(
                "{outside}: the board lives in the project's git repository, so init sets up only \
                 a folder in a checkout of one (`git init` makes it one)"
            );
            return refuse(err, USAGE, "init", &problem);
        }
    };
    if in_git_dir(&root, &common) {
        let problem = format!(
            "{} is in the git dir {}, in no checkout of it: init sets up only a folder in a git \
             checkout, never among git's own files",
            root.display(),
            common.display()
        );
        return refuse(err, USAGE, "init", &problem);
    }
    match init(&root, &Shipped::builtin()) {
        Ok(report) => {
            for (entry, outcome) in report {
                let _ = writeln!(out, "{entry}: {}", outcome.word());
            }
            let folder = crate::machine::dir().ok();
            let path = std::env::var_os("PATH");
            let surreal = symphony_env::SURREAL.os();
            let running = std::env::current_exe().and_then(|exe| exe.canonicalize());
            let machine = Machine {
                folder: folder.as_deref(),
                path: path.as_deref(),
                surreal: surreal.as_deref().map(Path::new),
                beside: running.as_deref().ok().and_then(Path::parent),
            };
            let _ = writeln!(out, "\n{}\n\n{TRUST}", next(&steps(&root, &machine)));
            if let Some(warning) = heartbeat_warning(&root, &machine.beyond()) {
                let _ = writeln!(err, "symphony init: {warning}");
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
