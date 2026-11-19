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

/// What the adapter does with pull requests: `publish` answers a land's delivery
/// ([`crate::deliver`]); `track` is read and held to its word, and this build serves none of it.
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

/// What the adapter does with the project's releases: `publish` answers the release run's
/// `release` ([`crate::release`]), a GitHub Release for a tag pushed.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Releases {
    #[default]
    Off,
    Publish,
}

impl Releases {
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Releases::Off => "off",
            Releases::Publish => "publish",
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
    #[serde(default)]
    releases: Releases,
    token_file: Option<PathBuf>,
    #[serde(default)]
    import_labels: Vec<String>,
    #[serde(default)]
    import_comments: bool,
    export_label: Option<String>,
    checks_token_file: Option<PathBuf>,
    checks_app_id: Option<i64>,
    landing_app_id: Option<i64>,
    policy_token_file: Option<PathBuf>,
    policy_app_id: Option<i64>,
    #[serde(default)]
    close_issues: bool,
    upstream_id: Option<i64>,
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
    pub releases: Releases,
    /// Where the token is, never the token.
    pub token_file: Option<PathBuf>,
    pub import_labels: Vec<String>,
    pub import_comments: bool,
    pub export_label: Option<String>,
    /// Where the token that writes a delivery's checks is: an installation token of the one
    /// GitHub App the branch's protection takes them from, never the token that merges.
    pub checks_token_file: Option<PathBuf>,
    /// That App's number: a check written by any other is not read as one of ours.
    pub checks_app_id: Option<i64>,
    /// The number of the GitHub App `token_file`'s token is an installation token of, for a
    /// delivery: the one actor the default branch's rules let update it
    /// ([`crate::preflight`]), never the App that writes the checks.
    pub landing_app_id: Option<i64>,
    /// Where the token that reads who bypasses the default branch's rulesets is: an installation
    /// token of a third GitHub App, which GitHub shows those lists because it may write the
    /// repository's administration. It is sent with two kinds of read and nothing else
    /// ([`crate::preflight::Reader`]), never the token that merges or writes the checks.
    pub policy_token_file: Option<PathBuf>,
    /// That App's number: neither of the other two.
    pub policy_app_id: Option<i64>,
    /// Whether a delivered task's mapped issue is closed once the board closed the task.
    pub close_issues: bool,
    /// The number at GitHub of the one repository an import is of, where the project imports
    /// from an upstream: a delivery merges an import's head by a merge commit only from the
    /// repository of that number ([`crate::deliver`]), and the default branch then permits merge
    /// commits beside squash ([`crate::preflight`]). With none, no import is delivered.
    pub upstream_id: Option<i64>,
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

/// The repository `OWNER/NAME` names on GitHub.com, refused where it is not that plainly.
pub(crate) fn repository(text: &str) -> Result<Repository, String> {
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

/// Refuse an `upstream_id` that names no repository, or that nothing delivers through.
fn upstream(written: &Written) -> Result<(), String> {
    match written.upstream_id {
        Some(id) if id <= 0 => Err("upstream_id is a GitHub repository's number".into()),
        Some(_) if written.pull_requests != PullRequests::Publish => Err(
            "upstream_id needs pull_requests = \"publish\": an import is delivered by a pull \
             request"
                .into(),
        ),
        _ => Ok(()),
    }
}

/// What a delivery's settings leave out: every App and token it needs.
fn delivery(written: &Written) -> Result<(), String> {
    if written.checks_token_file.is_none() || written.checks_app_id.is_none() {
        return Err(
            "pull_requests = \"publish\" needs checks_token_file and checks_app_id: the GitHub \
             App whose checks the branch's protection takes, and a token of its own"
                .into(),
        );
    }
    if written.landing_app_id.is_none() {
        return Err(
            "pull_requests = \"publish\" needs landing_app_id: the GitHub App token_file's token \
             is of, the one actor the default branch's rules let update it"
                .into(),
        );
    }
    if written.checks_token_file == written.token_file {
        return Err(
            "checks_token_file is token_file: what writes the checks is never what merges".into(),
        );
    }
    if written.policy_token_file.is_none() || written.policy_app_id.is_none() {
        return Err(
            "pull_requests = \"publish\" needs policy_token_file and policy_app_id: the GitHub \
             App GitHub shows who bypasses the default branch's rules, and a token of its own"
                .into(),
        );
    }
    Ok(())
}

/// The policy token and its App are neither of the other two, written or not enabled.
fn apart(written: &Written) -> Result<(), String> {
    if written.policy_app_id.is_some()
        && [written.landing_app_id, written.checks_app_id].contains(&written.policy_app_id)
    {
        return Err(
            "policy_app_id is landing_app_id or checks_app_id: what reads who bypasses the rules \
             neither merges nor writes the checks"
                .into(),
        );
    }
    if written.policy_token_file.is_some()
        && [&written.token_file, &written.checks_token_file].contains(&&written.policy_token_file)
    {
        return Err(
            "policy_token_file is token_file or checks_token_file: the token that reads who \
             bypasses the rules is a token of its own"
                .into(),
        );
    }
    Ok(())
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
    for (key, file) in [
        ("token_file", &written.token_file),
        ("checks_token_file", &written.checks_token_file),
        ("policy_token_file", &written.policy_token_file),
    ] {
        if file.as_ref().is_some_and(|file| !file.is_absolute()) {
            return Err(wrong(format!(
                "{key} is an absolute path outside the repository"
            )));
        }
    }
    for (key, id) in [
        ("checks_app_id", written.checks_app_id),
        ("landing_app_id", written.landing_app_id),
        ("policy_app_id", written.policy_app_id),
    ] {
        if id.is_some_and(|id| id <= 0) {
            return Err(wrong(format!("{key} is a GitHub App's number")));
        }
    }
    upstream(&written).map_err(wrong)?;
    if written.landing_app_id.is_some() && written.landing_app_id == written.checks_app_id {
        return Err(wrong(
            "landing_app_id is checks_app_id: what merges is never what writes the checks".into(),
        ));
    }
    apart(&written).map_err(wrong)?;
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
        if written.pull_requests == PullRequests::Publish {
            delivery(&written).map_err(wrong)?;
        }
    }
    Ok(Config {
        enabled: written.enabled,
        repository,
        issues: written.issues,
        pull_requests: written.pull_requests,
        releases: written.releases,
        token_file: written.token_file,
        import_labels: written.import_labels,
        import_comments: written.import_comments,
        export_label: written.export_label,
        checks_token_file: written.checks_token_file,
        checks_app_id: written.checks_app_id,
        landing_app_id: written.landing_app_id,
        policy_token_file: written.policy_token_file,
        policy_app_id: written.policy_app_id,
        close_issues: written.close_issues,
        upstream_id: written.upstream_id,
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

/// The project's default branch, as the main checkout `main`'s `symphony.toml` names it.
///
/// # Errors
///
/// The file does not read, or names no branch.
pub fn default_branch(main: &Path) -> Result<String, String> {
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
