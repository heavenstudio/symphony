//! The adapter's settings, `symphony-github.toml`: read from the tree the project's default
//! branch commits, never from a checkout's working file, so a branch's file enables nothing. A
//! file that is absent is the adapter turned off; one that does not read is a refusal, and every
//! caller treats a refusal as turned off.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The settings file's name, at the root of the committed tree.
pub const FILE: &str = "symphony-github.toml";

/// What the adapter does with issues.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Issues {
    #[default]
    Off,
    /// Selected issues become proposals and their edits reach those tasks.
    Import,
    /// Tasks opted in are published and their curated text's edits reach those issues.
    Export,
    /// Both.
    Exchange,
}

impl Issues {
    #[must_use]
    pub fn imports(self) -> bool {
        matches!(self, Issues::Import | Issues::Exchange)
    }

    #[must_use]
    pub fn exports(self) -> bool {
        matches!(self, Issues::Export | Issues::Exchange)
    }

    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Issues::Off => "off",
            Issues::Import => "import",
            Issues::Export => "export",
            Issues::Exchange => "exchange",
        }
    }
}

/// What the adapter does with pull requests. The setting is read and held to its words; this
/// build serves `off` alone and says so of the others.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PullRequests {
    #[default]
    Off,
    Track,
    Publish,
}

impl PullRequests {
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            PullRequests::Off => "off",
            PullRequests::Track => "track",
            PullRequests::Publish => "publish",
        }
    }
}

/// The file as written. Every key is known, so a mistyped one is refused, never ignored.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Written {
    #[serde(default)]
    enabled: bool,
    repository: Option<String>,
    #[serde(default)]
    issues: Issues,
    #[serde(default)]
    pull_requests: PullRequests,
    token_file: Option<PathBuf>,
    #[serde(default)]
    import_labels: Vec<String>,
    #[serde(default)]
    import_comments: bool,
    export_label: Option<String>,
}

/// The one GitHub.com repository the adapter may reach.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repository {
    pub owner: String,
    pub name: String,
}

impl Repository {
    #[must_use]
    pub fn full_name(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

/// The settings, checked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Config {
    pub enabled: bool,
    pub repository: Option<Repository>,
    pub issues: Issues,
    pub pull_requests: PullRequests,
    /// Where the token is, never the token.
    pub token_file: Option<PathBuf>,
    pub import_labels: Vec<String>,
    pub import_comments: bool,
    pub export_label: Option<String>,
}

/// The settings with the revision they were read at: a digest of the committed file's text, so a
/// preview made under one revision is never applied under another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loaded {
    pub config: Config,
    pub revision: String,
}

impl Loaded {
    /// No file committed: turned off.
    #[must_use]
    pub fn absent() -> Loaded {
        Loaded {
            config: Config::default(),
            revision: "absent".into(),
        }
    }
}

fn label(text: &str) -> Result<(), String> {
    if text.is_empty() || text.chars().count() > 50 {
        return Err(format!("a label is 1 to 50 characters: {text:?}"));
    }
    if text.chars().any(|c| c.is_control() || c == ',') {
        return Err(format!(
            "a label holds no comma or control character: {text:?}"
        ));
    }
    Ok(())
}

fn repository(text: &str) -> Result<Repository, String> {
    let refusal =
        || format!("repository is OWNER/NAME on GitHub.com, never a URL or another host: {text:?}");
    let (owner, name) = text.split_once('/').ok_or_else(refusal)?;
    let plain = |part: &str, more: &str| {
        !part.is_empty()
            && part.len() <= 100
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || more.contains(c))
    };
    if !plain(owner, "-") || !plain(name, "-._") || name == "." || name == ".." {
        return Err(refusal());
    }
    Ok(Repository {
        owner: owner.into(),
        name: name.into(),
    })
}

/// The settings `text` gives.
///
/// # Errors
///
/// One line naming the key that does not read or the value its settings cannot hold together.
pub fn parse(text: &str) -> Result<Config, String> {
    let written: Written =
        toml::from_str(text).map_err(|error| format!("{FILE}: {}", error.message()))?;
    let wrong = |why: String| format!("{FILE}: {why}");
    let repository = written
        .repository
        .as_deref()
        .map(repository)
        .transpose()
        .map_err(wrong)?;
    for text in written.import_labels.iter().chain(&written.export_label) {
        label(text).map_err(wrong)?;
    }
    if let Some(file) = &written.token_file
        && !file.is_absolute()
    {
        return Err(wrong(
            "token_file is an absolute path outside the repository".into(),
        ));
    }
    if written.enabled {
        if repository.is_none() {
            return Err(wrong("enabled needs repository = \"OWNER/NAME\"".into()));
        }
        if written.token_file.is_none() {
            return Err(wrong(
                "enabled needs token_file, the path of a file holding the token".into(),
            ));
        }
        if written.issues.imports() && written.import_labels.is_empty() {
            return Err(wrong(format!(
                "issues = \"{}\" needs import_labels, the labels that select an issue",
                written.issues.word()
            )));
        }
    }
    Ok(Config {
        enabled: written.enabled,
        repository,
        issues: written.issues,
        pull_requests: written.pull_requests,
        token_file: written.token_file,
        import_labels: written.import_labels,
        import_comments: written.import_comments,
        export_label: written.export_label,
    })
}

/// The settings of `text` with their revision.
///
/// # Errors
///
/// As [`parse`].
pub fn load(text: &str) -> Result<Loaded, String> {
    Ok(Loaded {
        config: parse(text)?,
        revision: crate::digest(&[text]),
    })
}

/// Where the settings come from, asked again before every dispatch.
pub trait Source {
    /// The settings as committed now.
    ///
    /// # Errors
    ///
    /// Why they do not read; the caller then does nothing.
    fn load(&self) -> Result<Loaded, String>;
}

/// The settings the project's default branch commits, read from git's objects by the main
/// checkout: `git show refs/heads/BRANCH:symphony-github.toml`, BRANCH the main checkout's
/// `[project] default_branch` (`main` unless it says another).
pub struct Committed {
    /// The main checkout.
    pub main: PathBuf,
}

#[derive(Default, Deserialize)]
struct ProjectFile {
    #[serde(default)]
    project: ProjectTable,
}

#[derive(Default, Deserialize)]
struct ProjectTable {
    default_branch: Option<String>,
}

fn default_branch(main: &Path) -> Result<String, String> {
    let file = main.join("symphony.toml");
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("{}: {error}", file.display())),
    };
    let read: ProjectFile =
        toml::from_str(&text).map_err(|error| format!("symphony.toml: {}", error.message()))?;
    let branch = read.project.default_branch.unwrap_or_else(|| "main".into());
    let plain = !branch.is_empty()
        && !branch.starts_with('-')
        && branch
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._/".contains(c));
    if plain {
        Ok(branch)
    } else {
        Err(format!("symphony.toml: no branch is named {branch:?}"))
    }
}

/// The checkouts of the repository `cwd` is in, the main one first, then its git folder: every
/// folder a token file must be outside.
///
/// # Errors
///
/// `cwd` is in no repository, or git is not run.
pub fn checkouts(cwd: &Path) -> Result<Vec<PathBuf>, String> {
    let git = |args: &[&str]| {
        let ran = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .output()
            .map_err(|error| format!("git: {error}"))?;
        if ran.status.success() {
            Ok(String::from_utf8_lossy(&ran.stdout).into_owned())
        } else {
            Err(format!("{} is in no git repository", cwd.display()))
        }
    };
    let listed = git(&["worktree", "list", "--porcelain", "-z"])?;
    let mut folders: Vec<PathBuf> = listed
        .split('\0')
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(PathBuf::from)
        .collect();
    if folders.is_empty() {
        return Err("git lists no worktree".into());
    }
    let common = git(&["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    folders.push(PathBuf::from(common.trim_end_matches('\n')));
    Ok(folders)
}

impl Source for Committed {
    fn load(&self) -> Result<Loaded, String> {
        let branch = default_branch(&self.main)?;
        let object = format!("refs/heads/{branch}:{FILE}");
        let git = |args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(&self.main)
                .args(args)
                .output()
                .map_err(|error| format!("git: {error}"))
        };
        // Whether the branch commits the file is asked apart from reading it, so a file that is
        // not there is told from one git could not read.
        let branch_ref = format!("refs/heads/{branch}");
        if !git(&["rev-parse", "--verify", "--quiet", &branch_ref])?
            .status
            .success()
        {
            return Err(format!("the repository has no branch {branch}"));
        }
        if !git(&["cat-file", "-e", &object])?.status.success() {
            return Ok(Loaded::absent());
        }
        let shown = git(&["cat-file", "blob", &object])?;
        if !shown.status.success() {
            return Err(format!("git could not read {FILE} on {branch}"));
        }
        let text = String::from_utf8(shown.stdout).map_err(|error| format!("{FILE}: {error}"))?;
        load(&text)
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
