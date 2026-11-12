//! `symphony init`: the agents' integration files a project needs, installed and kept current.
//!
//! The Symphony provides the generic skills, the recipes they run (`symphony.just`), the allow rules
//! (Claude's in `.claude/settings.json`, Codex's in `.codex/rules/symphony.rules`), the heartbeat
//! hook and the memory guard. Each file it writes carries a stamp, which is how a later run knows the
//! file is its own to bring up to date. A file of the same name without the stamp is the project's
//! own: it is the one in use and this never writes over it. Where the project has none, the
//! Symphony's is used. The project's own skills and guards sit beside the shipped ones and are never
//! touched. It writes no permission rule but the allow rules and the ask rules: each allow rule names
//! a recipe `symphony.just` holds, its board and reply recipes, never the server ones or `land`
//! ([`ALLOW`]); the ask rules stop any command naming the marker `land` runs `tasks land` by, and
//! `just tasks land` itself ([`ASK`]): a guard against accident, the land's approval check being
//! the gate.
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
//! the artifacts folder. The run ends with what is left for the owner to do ([`steps`]), and warns
//! when the heartbeat has no build to run.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::builds;

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

/// What Claude allows without a prompt: the board and reply recipes of `symphony.just`. Its server
/// recipes, `symphony` (which starts, replaces or stops the machine's shared server) and
/// `symphony-cli` (which reaches `serve`, `init` and `publish-client`), and `land` (which moves and
/// pushes the default branch), ask first.
const ALLOW: [&str; 6] = [
    "Bash(just tasks:*)",
    "Bash(TASKS_SESSION=* just tasks *)",
    "Bash(just agent-send:*)",
    "Bash(just symphony-status:*)",
    "Bash(just symphony-reply:*)",
    "Bash(just symphony-ask:*)",
];

/// What Claude asks before, whatever allows it: a command naming the marker the `land` recipe sets
/// (`symphony_board::land::RECIPE`), without which `tasks land` refuses, and `just tasks land`
/// itself, bare or with the handle before it, which the allow rules for `just tasks` would pass.
/// The marker and these prompts are a guard against accident, not the gate: what lands is decided
/// by the land's approval check, a fresh reviewer's approve standing on the tip it lands. The marker
/// rule names it anywhere in a command, so a search for it asks too: a rule for `SYMPHONY_LAND=`
/// alone would let `printf -v` or `read` set it unasked.
const ASK: [&str; 5] = [
    "Bash(*SYMPHONY_LAND*)",
    "Bash(just tasks land)",
    "Bash(just tasks land *)",
    "Bash(TASKS_SESSION=* just tasks land)",
    "Bash(TASKS_SESSION=* just tasks land *)",
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
                skill!("chat-doc"),
                skill!("create-task"),
                skill!("task-orchestrator"),
                skill!("tasks"),
                skill!("triage"),
                skill!("work"),
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

/// A file the Symphony writes whole, its stamp on its first line (the Codex rules, the recipes),
/// reported as `entry`.
fn install_stamped(
    root: &Path,
    rel: &str,
    entry: &str,
    text: &str,
    report: &mut Report,
) -> io::Result<()> {
    let path = root.join(rel);
    let have = read_optional(&path)?;
    let outcome = match &have {
        _ if is_link(&path) => Outcome::Own,
        Some(text) if !text.lines().next().is_some_and(is_mark_line) => Outcome::Own,
        _ => written(put(&path, &stamped_rules(text))?, have.is_some()),
    };
    report.push((entry.to_string(), outcome));
    Ok(())
}

/// What a write did: nothing, or brought an old copy up to date, or made the file.
fn written(wrote: bool, existed: bool) -> Outcome {
    match (wrote, existed) {
        (false, _) => Outcome::Current,
        (true, true) => Outcome::Updated,
        (true, false) => Outcome::Installed,
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
/// is, but for one init wrote setting what `symphony.just` now sets, which is brought current.
fn install_justfile(root: &Path, report: &mut Report) -> io::Result<()> {
    let outcome = match justfile(root)? {
        Some(path)
            if !is_link(&path)
                && read_optional(&path)?.as_deref() == Some(JUSTFILE_SETTING_ITSELF) =>
        {
            put(&path, JUSTFILE)?;
            Outcome::Updated
        }
        Some(_) => Outcome::Own,
        None => {
            put(&root.join("justfile"), JUSTFILE)?;
            Outcome::Installed
        }
    };
    report.push(("justfile".to_string(), outcome));
    Ok(())
}

/// The recipes the Symphony no longer ships: a stamped `symphony.just` goes, and then the
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

/// A line in the project's `.gitignore` for each folder the Symphony fills that it does not ignore
/// yet: the build links' and the artifacts folder `artifacts`.
fn install_ignores(root: &Path, artifacts: &str, report: &mut Report) -> io::Result<()> {
    let path = root.join(IGNORE);
    if is_link(&path) {
        report.push((IGNORE.to_string(), Outcome::Own));
        return Ok(());
    }
    let have = read_optional(&path)?;
    let mut text = have.clone().unwrap_or_default();
    let missing: Vec<&str> = [TARGET, artifacts]
        .into_iter()
        .filter(|folder| !ignores(&text, folder))
        .collect();
    if !missing.is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        for folder in missing {
            text.push_str(&format!("/{folder}/\n"));
        }
    }
    let outcome = written(put(&path, &text)?, have.is_some());
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
    for rule in ALLOW {
        if !allow.iter().any(|have| have.as_str() == Some(rule)) {
            allow.push(json!(rule));
            changed = true;
        }
    }
    let ask = permissions
        .entry("ask")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(not_object)?;
    for rule in ASK {
        if !ask.iter().any(|have| have.as_str() == Some(rule)) {
            ask.push(json!(rule));
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
    install_stamped(
        root,
        CODEX_RULES,
        "codex rules",
        &shipped.codex_rules,
        &mut report,
    )?;
    install_settings(root, &mut report)?;
    match &shipped.recipes {
        Some(recipes) => {
            let recipes = recipes_for(root, recipes)?;
            install_stamped(root, RECIPES, RECIPES, &recipes, &mut report)?;
            install_justfile(root, &mut report)?;
        }
        None => retire_recipes(root, &mut report)?,
    }
    let artifacts = crate::project::Project::load(root)
        .map_or_else(|_| "artifacts".to_string(), |project| project.artifacts);
    install_ignores(root, &artifacts, &mut report)?;
    Ok(report)
}

/// What the machine tells [`steps`]: its Symphony's folder (the registry's), the search path and
/// the SurrealDB `SYMPHONY_SURREAL` names.
pub struct Machine<'a> {
    /// None when the machine has no Symphony folder to read.
    pub folder: Option<&'a Path>,
    pub path: Option<&'a OsStr>,
    pub surreal: Option<&'a Path>,
}

/// The SurrealDB the board of the checkout at `root` would start, when it is not there to run:
/// `named` (`SYMPHONY_SURREAL`), else the pinned one in the repository's common dir. None outside a
/// checkout.
fn missing_surreal(root: &Path, named: Option<&Path>) -> Option<PathBuf> {
    let common = symphony_board::git::checkout_common_dir(root)?;
    let home = symphony_board::server::Home::in_common(&common, named);
    (!builds::runnable(home.surreal())).then(|| home.surreal().to_path_buf())
}

/// The heartbeat's builds, which the hooks run from the checkout's links alone.
const HEARTBEAT_BUILDS: [&str; 2] = ["symphony", "tasks"];

/// The heartbeat's builds the checkout at `root` has no link to.
fn unlinked_heartbeat(root: &Path) -> Vec<&'static str> {
    HEARTBEAT_BUILDS
        .into_iter()
        .filter(|name| builds::link(root, name).is_none())
        .collect()
}

/// What is left for the owner to do after a run at `root`, each one line: what init cannot do for
/// the project. None left means agents can run the board.
pub fn steps(root: &Path, machine: &Machine) -> Vec<String> {
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
                 its agents, verdicts and board",
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
    if let Ok(Some(justfile)) = justfile(root) {
        let text = std::fs::read_to_string(&justfile).unwrap_or_default();
        let name = justfile.file_name().unwrap_or_default().to_string_lossy();
        if !imports(&text) {
            steps.push(format!(
                "import the recipes in {name}: add `import '{RECIPES}'`; a recipe of its own of the \
                 same name (`land`, say) replaces the Symphony's"
            ));
        }
    }
    let unlinked = unlinked_heartbeat(root);
    if !unlinked.is_empty() {
        let (noun, them) = if unlinked.len() == 1 {
            ("build", "it")
        } else {
            ("builds", "them")
        };
        steps.push(format!(
            "link the project's {noun} at {}: the heartbeat and the memory guard run {them} \
             and build nothing{}",
            names_and(&unlinked, |name| format!("{}/{name}", builds::LINKS)),
            link_commands(&unlinked, machine.path)
        ));
    }
    let unfound: Vec<&str> = RUNS
        .iter()
        .filter(|(_, binary)| builds::find(root, binary, machine.path).is_none())
        .map(|(_, binary)| *binary)
        .filter(|binary| !unlinked.contains(binary))
        .collect();
    if !unfound.is_empty() {
        let (noun, them, run) = if unfound.len() == 1 {
            ("build", "it", "runs")
        } else {
            ("builds", "them", "run")
        };
        steps.push(format!(
            "link the project's {noun} at {}, or put {them} on PATH: {} {run} {them}",
            names_and(&unfound, |name| format!("{}/{name}", builds::LINKS)),
            names_and(&unfound, |name| {
                let recipe = RUNS.iter().find(|(_, binary)| *binary == name);
                format!("`just {}`", recipe.map_or(name, |(recipe, _)| *recipe))
            })
        ));
    }
    if let Some(surreal) = missing_surreal(root, machine.surreal) {
        steps.push(format!(
            "put SurrealDB's v{} release binary at {}, or name one with SYMPHONY_SURREAL: the board's \
             server runs it",
            symphony_board::server::SURREAL_VERSION,
            surreal.display()
        ));
    }
    if let Some(folder) = machine.folder
        && !registered(root, folder)
    {
        steps.push(
            "register the project with the machine's Symphony: `just symphony-cli project add` \
             (a start of `just symphony` registers it too)"
                .to_string(),
        );
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

/// The commands that link each of `names` the search path `path` holds, after a colon; none when it
/// holds none of them.
fn link_commands(names: &[&str], path: Option<&OsStr>) -> String {
    let commands: Vec<String> = names
        .iter()
        .filter_map(|name| {
            let found = builds::on_path(name, path)?;
            let quoted = found.to_string_lossy().replace('\'', "'\\''");
            Some(format!("ln -s '{quoted}' {}/{name}", builds::LINKS))
        })
        .collect();
    if commands.is_empty() {
        String::new()
    } else {
        format!(
            " (`mkdir -p {} && {}`)",
            builds::LINKS,
            commands.join(" && ")
        )
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

/// The warning a run at `root` ends with when the heartbeat has no build to run there.
pub fn heartbeat_warning(root: &Path) -> Option<String> {
    let unlinked = unlinked_heartbeat(root);
    if unlinked.is_empty() {
        return None;
    }
    let links = names_and(&unlinked, |name| format!("{}/{name}", builds::LINKS));
    let verb = if unlinked.len() == 1 { "is" } else { "are" };
    let guard = if unlinked.contains(&"symphony") {
        " and the memory guard refuses nothing"
    } else {
        ""
    };
    Some(format!(
        "warning: {links} {verb} missing: no tool call counts as a beat{guard} until the \
         project's builds are linked there (see Next)"
    ))
}

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
  --root ROOT  the project's root (default: the current folder)

A file of the same name without the Symphony's stamp is the project's own: it is used and never
written over.";

/// The project's root a run names: `--root`'s value (else the current folder), read from `cwd`
/// and resolved, so a project named after its folder is named after this one, never `.`. One
/// that cannot be resolved is left as given, for the run to refuse.
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
    match init(&root, &Shipped::builtin()) {
        Ok(report) => {
            for (entry, outcome) in report {
                let _ = writeln!(out, "{entry}: {}", outcome.word());
            }
            let folder = crate::machine::dir().ok();
            let path = std::env::var_os("PATH");
            let surreal = symphony_env::SURREAL.os();
            let machine = Machine {
                folder: folder.as_deref(),
                path: path.as_deref(),
                surreal: surreal.as_deref().map(Path::new),
            };
            let _ = writeln!(out, "\n{}", next(&steps(&root, &machine)));
            if let Some(warning) = heartbeat_warning(&root) {
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
