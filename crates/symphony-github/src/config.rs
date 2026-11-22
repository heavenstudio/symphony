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

/// Whether the adapter runs the release's verification on GitHub's machines: `hosted` answers the
/// release run's `verify` ([`crate::verify`]), which starts the workflow for one commit and reads
/// its result.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Verification {
    #[default]
    Off,
    Hosted,
}

impl Verification {
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Verification::Off => "off",
            Verification::Hosted => "hosted",
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
    #[serde(default)]
    verification: Verification,
    token_file: Option<PathBuf>,
    #[serde(default)]
    import_labels: Vec<String>,
    #[serde(default)]
    import_comments: bool,
    export_label: Option<String>,
    #[serde(default)]
    close_issues: bool,
    upstream_id: Option<i64>,
    app_id: Option<i64>,
    installation_id: Option<i64>,
    key_file: Option<PathBuf>,
    /// The keys of three Apps, which one App replaced: each refused by name ([`RETIRED`]).
    #[serde(default)]
    checks_token_file: Option<toml::Value>,
    #[serde(default)]
    checks_app_id: Option<toml::Value>,
    #[serde(default)]
    checks_installation_id: Option<toml::Value>,
    #[serde(default)]
    checks_key_file: Option<toml::Value>,
    #[serde(default)]
    landing_app_id: Option<toml::Value>,
    #[serde(default)]
    landing_installation_id: Option<toml::Value>,
    #[serde(default)]
    landing_key_file: Option<toml::Value>,
    #[serde(default)]
    policy_token_file: Option<toml::Value>,
    #[serde(default)]
    policy_app_id: Option<toml::Value>,
    #[serde(default)]
    policy_installation_id: Option<toml::Value>,
    #[serde(default)]
    policy_key_file: Option<toml::Value>,
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
    pub verification: Verification,
    /// Where the token is, never the token.
    pub token_file: Option<PathBuf>,
    pub import_labels: Vec<String>,
    pub import_comments: bool,
    pub export_label: Option<String>,
    /// Whether a delivered task's mapped issue is closed once the board closed the task.
    pub close_issues: bool,
    /// The number at GitHub of the one repository an import is of, where the project imports
    /// from an upstream: a delivery merges an import's head by a merge commit only from the
    /// repository of that number ([`crate::deliver`]), and the default branch then permits merge
    /// commits beside squash ([`crate::preflight`]). With none, no import is delivered.
    pub upstream_id: Option<i64>,
    /// The number of the one GitHub App `token_file`'s token is an installation token of: for a
    /// delivery, the App whose checks the default branch requires, which pushes, opens the pull
    /// request and merges ([`crate::preflight`]).
    pub app_id: Option<i64>,
    /// That App's installation on the repository and the path of its private key, set together:
    /// where they are, the program makes `token_file`'s token itself ([`crate::mint`]).
    pub installation_id: Option<i64>,
    pub key_file: Option<PathBuf>,
}

/// The keys of the three Apps a delivery once took, which one App replaced: a file that still
/// names one is refused by name, so it is never read as one App's settings it is not.
pub const RETIRED: [&str; 11] = [
    "checks_token_file",
    "checks_app_id",
    "checks_installation_id",
    "checks_key_file",
    "landing_app_id",
    "landing_installation_id",
    "landing_key_file",
    "policy_token_file",
    "policy_app_id",
    "policy_installation_id",
    "policy_key_file",
];

/// What makes an installation token of the App: its number, its installation's on the
/// repository, and where its private key is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    pub id: i64,
    pub installation: i64,
    pub key_file: PathBuf,
}

impl Config {
    /// The App that makes `token_file`'s token here: none where its installation and key are not
    /// set, and the file's token is then kept by whoever writes it.
    #[must_use]
    pub fn app(&self) -> Option<App> {
        Some(App {
            id: self.app_id?,
            installation: self.installation_id?,
            key_file: self.key_file.clone()?,
        })
    }
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

/// The first of the three Apps' keys `written` still names, which one App replaced.
fn retired(written: &Written) -> Option<&'static str> {
    let named = [
        written.checks_token_file.is_some(),
        written.checks_app_id.is_some(),
        written.checks_installation_id.is_some(),
        written.checks_key_file.is_some(),
        written.landing_app_id.is_some(),
        written.landing_installation_id.is_some(),
        written.landing_key_file.is_some(),
        written.policy_token_file.is_some(),
        written.policy_app_id.is_some(),
        written.policy_installation_id.is_some(),
        written.policy_key_file.is_some(),
    ];
    RETIRED
        .iter()
        .zip(named)
        .find_map(|(key, named)| named.then_some(*key))
}

/// What a delivery's settings leave out: the App it is held to.
fn delivery(written: &Written) -> Result<(), String> {
    if written.app_id.is_none() {
        return Err(
            "pull_requests = \"publish\" needs app_id: the GitHub App token_file's token is of, \
             whose checks the default branch requires"
                .into(),
        );
    }
    Ok(())
}

/// The App's installation and key are set together, with its number and the token file the
/// token made is kept in; the key file is not the token file.
fn maker(written: &Written) -> Result<(), String> {
    if written.installation_id.is_some_and(|id| id <= 0) {
        return Err("installation_id is a GitHub App installation's number".into());
    }
    let Some(key_file) = &written.key_file else {
        if written.installation_id.is_some() {
            return Err(
                "installation_id needs key_file: the App's private key, which makes the \
                 installation's token"
                    .into(),
            );
        }
        return Ok(());
    };
    if !key_file.is_absolute() {
        return Err("key_file is an absolute path outside the repository".into());
    }
    if written.installation_id.is_none() {
        return Err(
            "key_file needs installation_id: the installation whose token the key makes".into(),
        );
    }
    if written.app_id.is_none() {
        return Err("key_file needs app_id: the App the key is of".into());
    }
    if written.token_file.is_none() {
        return Err("key_file needs token_file: where the token it makes is kept".into());
    }
    if written.token_file.as_ref() == Some(key_file) {
        return Err("key_file is token_file: a token made would be written over the key".into());
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
    if let Some(key) = retired(&written) {
        return Err(wrong(format!(
            "{key} is gone: one GitHub App reads the issues, writes a delivery's checks and \
             merges, named by app_id, installation_id and key_file (docs/operations/GITHUB_SETUP.md)"
        )));
    }
    if (written.token_file.as_ref()).is_some_and(|file| !file.is_absolute()) {
        return Err(wrong(
            "token_file is an absolute path outside the repository".into(),
        ));
    }
    if written.app_id.is_some_and(|id| id <= 0) {
        return Err(wrong("app_id is a GitHub App's number".into()));
    }
    upstream(&written).map_err(wrong)?;
    maker(&written).map_err(wrong)?;
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
        verification: written.verification,
        token_file: written.token_file,
        import_labels: written.import_labels,
        import_comments: written.import_comments,
        export_label: written.export_label,
        close_issues: written.close_issues,
        upstream_id: written.upstream_id,
        app_id: written.app_id,
        installation_id: written.installation_id,
        key_file: written.key_file,
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
/// checkout: `git show refs/heads/BRANCH:symphony-github.toml`, BRANCH the `[project]
/// default_branch` the main checkout's HEAD commits ([`default_branch`], `main` unless it says
/// another).
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

/// The project's default branch, as `[project] default_branch` names it in the `symphony.toml`
/// the main checkout `main`'s HEAD commits (`main` where none is committed or it names none):
/// read as the land reads it, so the two agree, and an edit of the working file names no
/// branch until it is committed.
///
/// # Errors
///
/// git cannot read it, the file does not read, or it names no branch.
pub fn default_branch(main: &Path) -> Result<String, String> {
    let at = "HEAD:symphony.toml";
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(main)
            .args(args)
            .output()
            .map_err(|error| format!("git: {error}"))
    };
    let text = if git(&["cat-file", "-e", at])?.status.success() {
        let shown = git(&["cat-file", "blob", at])?;
        if !shown.status.success() {
            return Err(format!(
                "git could not read symphony.toml at {}'s HEAD",
                main.display()
            ));
        }
        String::from_utf8(shown.stdout).map_err(|error| format!("symphony.toml: {error}"))?
    } else {
        String::new()
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
