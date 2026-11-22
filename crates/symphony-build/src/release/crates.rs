//! The packages this workspace publishes on crates.io (docs/workflow/RELEASES.md, "Packages on crates.io"):
//! which they are ([`packages`]); what crates.io holds of each
//! ([`Registry`]); the check that a package whose version crates.io holds still carries the sources
//! that version was published with ([`check`], `symphony-build crates-check`); and the release
//! run's step that publishes each version crates.io lacks once a green release is tagged and pushed
//! ([`publish`]). Nothing here reads a token: `cargo publish` finds its own, from its environment,
//! its credentials or a credential provider, and says when it has none.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use symphony_board::sys::Command;

use super::git::git_until;
use super::group::{Bound, Failed, output_until};
use symphony::children::command;

/// crates.io's sparse index, a file of each crate's versions.
const INDEX: &str = "https://index.crates.io";
/// Where crates.io serves each version's `.crate`.
const DOWNLOADS: &str = "https://static.crates.io/crates";
/// The tests' registry: a folder laid out as crates.io serves it, `index/` as the sparse index and
/// `crates/NAME/NAME-VERSION.crate`, read in place of crates.io. No flag sets it.
pub(crate) const REGISTRY: &str = "SYMPHONY_CRATES_IO";
/// The tests' stand-in for the `cargo` that publishes, run as `cargo publish --locked -p NAME …`
/// would be. No flag sets it.
pub(crate) const PUBLISHER: &str = "SYMPHONY_RELEASE_CARGO";
/// How long one read of crates.io may take, unless its bound ends sooner.
const READ: Duration = Duration::from_secs(120);
/// How crates.io is read: each try within [`READ`], up to three, two seconds before the second and
/// four before the third.
const READS: Reads = Reads {
    each: READ,
    tries: 3,
    pause: Duration::from_secs(2),
};
/// The files of a packaged crate that are not its sources, at its top: where it was packaged from,
/// and the workspace's resolution of its dependencies when it was.
const NOT_SOURCES: [&str; 2] = [".cargo_vcs_info.json", "Cargo.lock"];
/// The folder of the build folder a check packages and unpacks in.
const CHECKED: &str = "crates-check";
/// The changed files a check names of one package at most; the rest are counted.
const NAMED: usize = 10;

/// A member the workspace publishes: its name and its version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Package {
    pub(crate) name: String,
    pub(crate) version: String,
}

impl Package {
    /// `NAME VERSION`, as the run and the release's notes say a package.
    pub(crate) fn said(&self) -> String {
        format!("{} {}", self.name, self.version)
    }
}

/// `command` run within `bound`, as `what` names it when it cannot run or ends late.
fn run(command: &mut Command, what: &str, bound: Bound<'_>) -> Result<Output, String> {
    output_until(command, bound).map_err(|failed| match failed {
        Failed::Io(e) => format!("cannot run {what}: {e}"),
        Failed::Late(took) => format!(
            "{what} did not end within {} s and was stopped",
            took.as_secs()
        ),
        Failed::Interrupted(took) => {
            format!(
                "{what} was stopped after {} s: the run was told to stop",
                took.as_secs()
            )
        }
    })
}

/// What a command said on `stderr` that names its failure: its first line starting `error`, else
/// its last line, else nothing.
fn failure(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = (text.lines().map(str::trim))
        .filter(|line| !line.is_empty())
        .collect();
    (lines.iter().find(|line| line.starts_with("error")))
        .or(lines.last())
        .map_or_else(String::new, |line| (*line).to_string())
}

/// The members of the workspace at `root` that are published, by name; none when `root` holds no
/// `Cargo.toml`. Read by `cargo metadata`, which resolves
/// what a member inherits from the workspace as `cargo publish` does, offline and without the
/// members' dependencies.
pub(crate) fn packages(root: &Path, bound: Bound<'_>) -> Result<Vec<Package>, String> {
    Ok(metadata(root, bound)?.map_or_else(Vec::new, |metadata| published(&metadata)))
}

/// What `cargo metadata` says of the workspace at `root`, offline and without the members'
/// dependencies; none when `root` holds no `Cargo.toml`.
fn metadata(root: &Path, bound: Bound<'_>) -> Result<Option<Value>, String> {
    if !root.join("Cargo.toml").is_file() {
        return Ok(None);
    }
    let done = run(
        command("cargo")
            .args([
                "metadata",
                "--no-deps",
                "--format-version",
                "1",
                "--offline",
            ])
            .current_dir(root),
        "cargo metadata",
        bound.sooner(READ),
    )?;
    if !done.status.success() {
        return Err(format!(
            "cargo metadata failed in {}: {}",
            root.display(),
            failure(&done.stderr)
        ));
    }
    serde_json::from_slice(&done.stdout)
        .map(Some)
        .map_err(|e| format!("cargo metadata in {} said no JSON: {e}", root.display()))
}

/// The folder of the member `member` of `cargo metadata`'s, from the workspace's root: where its
/// manifest is, empty for a package at the root itself. None for a manifest outside the root.
fn folder(metadata: &Value, member: &Value) -> Option<PathBuf> {
    let manifest = Path::new(member["manifest_path"].as_str()?);
    let within = manifest.strip_prefix(metadata["workspace_root"].as_str()?);
    Some(within.ok()?.parent()?.to_path_buf())
}

/// The published packages of `metadata` that `files` change, each a path from the workspace's
/// root: a file in the package's folder, whether its package carries it or not, or the workspace's
/// own manifest, whose settings and dependency versions every member's packaged manifest inherits.
fn changed_by(metadata: &Value, files: &[&str]) -> Vec<Package> {
    let members = metadata["packages"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    let all = files.contains(&"Cargo.toml");
    let changed = |package: &Package| {
        all || (members.iter())
            .filter(|member| member["name"] == package.name.as_str())
            .filter_map(|member| folder(metadata, member))
            .any(|folder| {
                files
                    .iter()
                    .any(|file| Path::new(file).starts_with(&folder))
            })
    };
    let mut found = published(metadata);
    found.retain(changed);
    found
}

/// The published packages the checkout at `root` changes since it left the default branch
/// `default` ([`changed_by`] every tracked file that differs from their merge base, committed or
/// not); none where the checkout has no `default` to compare with. On `default`'s own line that is
/// what is not committed yet.
pub(crate) fn touched(
    root: &Path,
    default: &str,
    bound: Bound<'_>,
) -> Result<Option<Vec<Package>>, String> {
    let Ok(base) = git_until(root, &["merge-base", "HEAD", default], bound) else {
        return Ok(None);
    };
    // A file moved out of a package is gone from it: both of a rename's paths are listed.
    let diff = ["diff", "--name-only", "--no-renames", "--relative", &base];
    let listed = git_until(root, &diff, bound)?;
    let files: Vec<&str> = listed.lines().collect();
    Ok(Some(
        metadata(root, bound)?.map_or_else(Vec::new, |metadata| changed_by(&metadata, &files)),
    ))
}

/// The published members `cargo metadata` names, by name: a member with no `publish` list
/// publishes anywhere, one with `publish = false` (an empty list) nowhere, and one with a list
/// where it names crates.io.
fn published(metadata: &Value) -> Vec<Package> {
    let members = metadata["packages"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    let publishes = |member: &Value| match &member["publish"] {
        Value::Null => true,
        Value::Array(registries) => registries.iter().any(|registry| registry == "crates-io"),
        _ => false,
    };
    let text = |value: &Value| value.as_str().unwrap_or_default().to_string();
    let mut found: Vec<Package> = (members.iter().filter(|member| publishes(member)))
        .map(|member| Package {
            name: text(&member["name"]),
            version: text(&member["version"]),
        })
        .collect();
    found.sort_by(|one, other| one.name.cmp(&other.name));
    found
}

/// `packages` split by what `registry` holds: those whose version it lists, then those it lacks.
fn split(
    registry: &Registry,
    packages: &[Package],
    bound: Bound<'_>,
) -> Result<(Vec<Package>, Vec<Package>), Unread> {
    let (mut held, mut lacked) = (Vec::new(), Vec::new());
    for package in packages {
        if registry.has(&package.name, &package.version, bound)? {
            held.push(package.clone());
        } else {
            lacked.push(package.clone());
        }
    }
    Ok((held, lacked))
}

/// Each of `packages` said, `NAME VERSION`.
fn said(packages: &[Package]) -> Vec<String> {
    packages.iter().map(Package::said).collect()
}

/// Why a registry was not read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Unread {
    /// It gave no answer, or a server's error, on the last try of a read ([`fetched_by`]): the
    /// network's or the registry's for now, which says nothing of what it holds.
    NoAnswer(String),
    /// Anything else: an answer that stands as an error, a file that cannot be read or written.
    Failed(String),
}

impl From<Unread> for String {
    fn from(unread: Unread) -> String {
        match unread {
            Unread::NoAnswer(why) | Unread::Failed(why) => why,
        }
    }
}

impl From<String> for Unread {
    fn from(why: String) -> Unread {
        Unread::Failed(why)
    }
}

/// Where a registry holds what crates.io holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Registry {
    /// crates.io itself, read with curl.
    CratesIo,
    /// A server laid out as [`Registry::Folder`] is, read with curl as crates.io is but by reads
    /// of its own: the tests' own, at its URL.
    #[cfg(test)]
    At(String, Reads),
    /// A folder laid out as crates.io serves it ([`REGISTRY`]): the tests'.
    Folder(PathBuf),
}

impl Registry {
    /// The tests' server at `url`, each read of it `tries` tries with no pause between, a try
    /// within a time no test reaches: a test of what a read's end means waits on nothing.
    #[cfg(test)]
    pub(crate) fn at(url: String, tries: u32) -> Registry {
        let reads = Reads {
            each: Duration::from_secs(60),
            tries,
            pause: Duration::ZERO,
        };
        Registry::At(url, reads)
    }

    /// The folder [`REGISTRY`] names, else crates.io.
    pub(crate) fn of_env() -> Registry {
        std::env::var_os(REGISTRY).map_or(Registry::CratesIo, |folder| {
            Registry::Folder(PathBuf::from(folder))
        })
    }

    /// What its lines call it.
    fn said(&self) -> String {
        match self {
            Registry::CratesIo => "crates.io".into(),
            #[cfg(test)]
            Registry::At(url, _) => format!("the registry {url}"),
            Registry::Folder(folder) => format!("the registry {}", folder.display()),
        }
    }

    /// Whether it holds `name` at `version`, yanked or not: a version is published once.
    pub(crate) fn has(&self, name: &str, version: &str, bound: Bound<'_>) -> Result<bool, Unread> {
        let path = index_path(name);
        let listed = match self {
            Registry::CratesIo => fetched(&format!("{INDEX}/{path}"), None, bound)?,
            #[cfg(test)]
            Registry::At(url, reads) => {
                fetched_by(*reads, &format!("{url}/index/{path}"), None, bound)?
            }
            Registry::Folder(folder) => match fs::read(folder.join("index").join(&path)) {
                Ok(bytes) => Some(bytes),
                Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                Err(e) => {
                    let file = folder.join("index").join(&path);
                    return Err(symphony::text::os_error(&e, &file).into());
                }
            },
        };
        Ok(listed.is_some_and(|listed| lists(&String::from_utf8_lossy(&listed), version)))
    }

    /// `name` at `version` as it holds it, into the file `to`.
    fn download(
        &self,
        name: &str,
        version: &str,
        to: &Path,
        bound: Bound<'_>,
    ) -> Result<(), Unread> {
        let file = format!("{name}/{name}-{version}.crate");
        let (url, reads) = match self {
            Registry::CratesIo => (format!("{DOWNLOADS}/{file}"), READS),
            #[cfg(test)]
            Registry::At(url, reads) => (format!("{url}/crates/{file}"), *reads),
            Registry::Folder(folder) => {
                let from = folder.join("crates").join(&file);
                return match fs::copy(&from, to) {
                    Ok(_) => Ok(()),
                    Err(e) => Err(symphony::text::os_error(&e, &from).into()),
                };
            }
        };
        match fetched_by(reads, &url, Some(to), bound)? {
            Some(_) => Ok(()),
            None => Err(format!(
                "{} lists {name} {version} but serves no {file}",
                self.said()
            )
            .into()),
        }
    }
}

/// The path of `name`'s file in a sparse index: by its length, then its first letters, lower case.
fn index_path(name: &str) -> String {
    let name = name.to_ascii_lowercase();
    let part = |skip: usize, take: usize| name.chars().skip(skip).take(take).collect::<String>();
    match name.chars().count() {
        1 => format!("1/{name}"),
        2 => format!("2/{name}"),
        3 => format!("3/{}/{name}", part(0, 1)),
        _ => format!("{}/{}/{name}", part(0, 2), part(2, 2)),
    }
}

/// Whether a sparse index file, a JSON line for each version, lists `version`.
fn lists(file: &str, version: &str) -> bool {
    file.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .any(|entry| entry["vers"].as_str() == Some(version))
}

/// How [`fetched`] reads: each try by curl within `each`, up to `tries`, waiting `pause` times the
/// tries made so far before the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Reads {
    each: Duration,
    tries: u32,
    pause: Duration,
}

/// How one try of a read ended.
enum Try {
    /// An answer that stands: what was served, none, or an error.
    Stands(Result<Option<Vec<u8>>, String>),
    /// No answer, or a server's error, which another try may not meet: why.
    Again(String),
}

/// What `url` serves, by curl within `bound` as [`READS`] says: into the file `to` when one is
/// named (its bytes then none), else its bytes; none when it serves nothing there (404, or 403 as a
/// bucket of files answers for one it lacks). Any other answer is an error, and so is the last try
/// of a read that got none.
fn fetched(url: &str, to: Option<&Path>, bound: Bound<'_>) -> Result<Option<Vec<u8>>, Unread> {
    fetched_by(READS, url, to, bound)
}

/// [`fetched`] by `reads`. A try that got no answer (curl failed or was stopped: a timeout, a
/// connection refused or reset, an empty reply) or a server's error (5xx, 408, 429) is the
/// network's or the registry's for a moment, so the read is tried again while `reads` has tries
/// left and `bound` has time for the pause and was not told to stop; the last failure is said, with
/// the tries it took.
fn fetched_by(
    reads: Reads,
    url: &str,
    to: Option<&Path>,
    bound: Bound<'_>,
) -> Result<Option<Vec<u8>>, Unread> {
    let mut tried = 1;
    loop {
        let problem = match tried_once(url, to, reads.each, bound) {
            Try::Stands(answer) => return answer.map_err(Unread::Failed),
            Try::Again(problem) => problem,
        };
        let pause = reads.pause * tried;
        if tried >= reads.tries || bound.interrupted() || Instant::now() + pause >= bound.until {
            return Err(Unread::NoAnswer(if tried == 1 {
                problem
            } else {
                format!("{problem} (tried {tried} times)")
            }));
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "no-timed-waits baseline: tries crates.io again after a fixed pause, where a backoff is the helper"
        )]
        std::thread::sleep(pause);
        tried += 1;
    }
}

/// One try of [`fetched`], by curl within `each` and `bound`.
fn tried_once(url: &str, to: Option<&Path>, each: Duration, bound: Bound<'_>) -> Try {
    let mut curl = command("curl");
    curl.args(["--silent", "--show-error", "--location", "--max-time"])
        .arg(each.as_secs_f64().to_string());
    match to {
        Some(to) => curl
            .arg("--output")
            .arg(to)
            .args(["--write-out", "%{http_code}"]),
        None => curl.args(["--write-out", "\n%{http_code}"]),
    };
    curl.arg(url);
    let done = match run(&mut curl, &format!("curl {url}"), bound.sooner(each)) {
        Ok(done) => done,
        Err(problem) => return Try::Again(problem),
    };
    if !done.status.success() {
        return Try::Again(format!("curl {url} failed: {}", failure(&done.stderr)));
    }
    let said = String::from_utf8_lossy(&done.stdout).into_owned();
    let (body, code) = match to {
        Some(_) => ("", said.trim()),
        None => said.rsplit_once('\n').unwrap_or(("", said.as_str())),
    };
    match code {
        "200" => Try::Stands(Ok(Some(body.as_bytes().to_vec()))),
        "404" | "403" => Try::Stands(Ok(None)),
        "408" | "429" => Try::Again(format!("{url} answered {code}")),
        _ if code.len() == 3 && code.starts_with('5') => {
            Try::Again(format!("{url} answered {code}"))
        }
        other => Try::Stands(Err(format!("{url} answered {other}"))),
    }
}

/// A published package whose version crates.io holds, packaged here other than crates.io holds
/// it: the package (`NAME VERSION`) and each file that differs, said `added`, `gone` or `changed`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Changed {
    pub(crate) package: String,
    pub(crate) files: Vec<String>,
}

/// What [`check`] found: a line for each published package, and those whose sources moved without
/// their version.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Checked {
    pub(crate) lines: Vec<String>,
    pub(crate) changed: Vec<Changed>,
}

/// Each package the workspace at `root` publishes against `registry`: one whose version the
/// registry lacks has nothing to compare with; one whose version it holds is packaged here,
/// into `target` (the build folder), and compared, file by file, with that version as the registry
/// serves it. Every file counts but [`NOT_SOURCES`]. A registry it cannot read, or a package it
/// cannot make or unpack, is an error: the check fails closed.
pub(crate) fn check(
    root: &Path,
    registry: &Registry,
    target: &Path,
    until: Instant,
) -> Result<Checked, String> {
    let bound = Bound::until(until);
    let packages = packages(root, bound)?;
    compared(root, registry, target, &packages, bound, None).map_err(String::from)
}

/// How long a branch's check waits on the registry at most: for what it lists of the packages the
/// branch changes, and again for each version it holds. A task's lint is not held longer by a
/// network that answers nothing.
pub(crate) const BRANCH_READS: Duration = Duration::from_secs(20);

/// What [`check_branch`] found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Branch {
    /// The checkout has no default branch to compare with.
    NoBase,
    /// The packages the branch changes, checked: none of either for a branch that changes none.
    Checked(Checked),
    /// The registry gave no answer ([`Unread::NoAnswer`]), so the packages named, each
    /// `NAME VERSION`, were not checked: no red, as it says nothing of the branch, and no proof.
    Unread { packages: Vec<String>, why: String },
}

/// [`check`] of the packages a branch changes alone ([`touched`]), which is what a task's lint
/// runs: nothing is read, of the registry or of the packages, for a branch that changes no
/// published package, and a registry that gives no answer within `reads` is said, not an error.
/// What the registry answers as an error, and a package it cannot make or unpack, still are.
pub(crate) fn check_branch(
    root: &Path,
    default: &str,
    registry: &Registry,
    target: &Path,
    reads: Duration,
    until: Instant,
) -> Result<Branch, String> {
    let bound = Bound::until(until);
    let Some(touched) = touched(root, default, bound)? else {
        return Ok(Branch::NoBase);
    };
    match compared(root, registry, target, &touched, bound, Some(reads)) {
        Ok(checked) => Ok(Branch::Checked(checked)),
        Err(Unread::NoAnswer(why)) => Ok(Branch::Unread {
            packages: said(&touched),
            why,
        }),
        Err(Unread::Failed(why)) => Err(why),
    }
}

/// [`check`] of `packages`, members the workspace at `root` publishes, waiting on the registry no
/// longer than `reads`, where one is given, for what it lists and for each version it serves.
fn compared(
    root: &Path,
    registry: &Registry,
    target: &Path,
    packages: &[Package],
    bound: Bound<'_>,
    reads: Option<Duration>,
) -> Result<Checked, Unread> {
    let reading = || reads.map_or(bound, |most| bound.sooner(most));
    let mut checked = Checked::default();
    let (held, lacked) = split(registry, packages, reading())?;
    for package in &lacked {
        checked.lines.push(format!(
            "{}: not on {} yet: nothing to compare, and a release publishes it while [releases] \
             publish is on",
            package.said(),
            registry.said()
        ));
    }
    if held.is_empty() {
        return Ok(checked);
    }
    let scratch = target.join(CHECKED);
    if scratch.exists() {
        fs::remove_dir_all(&scratch).map_err(|e| symphony::text::os_error(&e, &scratch))?;
    }
    let (ours, theirs) = (scratch.join("ours"), scratch.join("theirs"));
    for folder in [&ours, &theirs] {
        fs::create_dir_all(folder).map_err(|e| symphony::text::os_error(&e, folder))?;
    }
    let mut package = command("cargo");
    package
        .args([
            "package",
            "--no-verify",
            "--exclude-lockfile",
            "--allow-dirty",
            "--offline",
            "--quiet",
        ])
        .env("CARGO_TARGET_DIR", target)
        .current_dir(root);
    for each in &held {
        package.args(["-p", &each.name]);
    }
    let done = run(&mut package, "cargo package", bound)?;
    if !done.status.success() {
        return Err(format!(
            "cargo package failed in {}: {}",
            root.display(),
            failure(&done.stderr)
        )
        .into());
    }
    for each in held {
        let crate_file = format!("{}-{}.crate", each.name, each.version);
        let theirs_file = scratch.join(&crate_file);
        registry.download(&each.name, &each.version, &theirs_file, reading())?;
        unpacked(&target.join("package").join(&crate_file), &ours, bound)?;
        unpacked(&theirs_file, &theirs, bound)?;
        let folder = format!("{}-{}", each.name, each.version);
        let files = differ(&ours.join(&folder), &theirs.join(&folder))?;
        if files.is_empty() {
            checked.lines.push(format!(
                "{}: on {}, its packaged sources the same",
                each.said(),
                registry.said()
            ));
        } else {
            checked.changed.push(Changed {
                package: each.said(),
                files,
            });
        }
    }
    let _ = fs::remove_dir_all(&scratch);
    Ok(checked)
}

/// The `.crate` at `file` (a gzipped tar) unpacked into `into`, by tar within `bound`.
fn unpacked(file: &Path, into: &Path, bound: Bound<'_>) -> Result<(), String> {
    let done = run(
        command("tar").arg("-xzf").arg(file).arg("-C").arg(into),
        "tar",
        bound.sooner(READ),
    )?;
    if done.status.success() {
        return Ok(());
    }
    Err(format!(
        "tar cannot unpack {}: {}",
        file.display(),
        failure(&done.stderr)
    ))
}

/// How the package unpacked at `ours` differs from the one at `theirs`: each file under either, by
/// its path from there, that is only in ours (`added`), only in theirs (`gone`) or holds other
/// bytes (`changed`), in the order of their paths, [`NOT_SOURCES`] at the top left out.
fn differ(ours: &Path, theirs: &Path) -> Result<Vec<String>, String> {
    let (mine, published) = (files(ours)?, files(theirs)?);
    let mut said = Vec::new();
    for path in mine.union(&published) {
        let what = match (mine.contains(path), published.contains(path)) {
            (true, false) => "added",
            (false, true) => "gone",
            _ if bytes(&ours.join(path))? == bytes(&theirs.join(path))? => continue,
            _ => "changed",
        };
        said.push(format!("{path} {what}"));
    }
    Ok(said)
}

/// Every file under `root` but a folder, by its path from there with `/` between its parts, but
/// [`NOT_SOURCES`] at its top.
fn files(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut found = BTreeSet::new();
    let mut pending = vec![(root.to_path_buf(), String::new())];
    while let Some((folder, named)) = pending.pop() {
        let entries = fs::read_dir(&folder).map_err(|e| symphony::text::os_error(&e, &folder))?;
        for entry in entries {
            let entry = entry.map_err(|e| symphony::text::os_error(&e, &folder))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = if named.is_empty() {
                name.clone()
            } else {
                format!("{named}/{name}")
            };
            let kind = entry
                .file_type()
                .map_err(|e| symphony::text::os_error(&e, &entry.path()))?;
            if kind.is_dir() {
                pending.push((entry.path(), path));
            } else if !(named.is_empty() && NOT_SOURCES.contains(&name.as_str())) {
                found.insert(path);
            }
        }
    }
    Ok(found)
}

/// The bytes of the file at `path`: a `.crate` holds no link, as cargo packs a link's target.
fn bytes(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| symphony::text::os_error(&e, path))
}

/// The line a check says of a changed package: what changed, and that its version is to move.
pub(crate) fn changed_line(changed: &Changed) -> String {
    let mut named = (changed.files.iter().take(NAMED))
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if changed.files.len() > NAMED {
        named.push_str(&format!(" and {} more", changed.files.len() - NAMED));
    }
    format!(
        "{}: on crates.io, and its packaged sources differ from that version's ({named}): \
         a version is published once, so move its version, its line's as the packages' lines say \
         (docs/workflow/RELEASES.md, \"Packages on crates.io\")",
        changed.package
    )
}

/// What the release run's step did with crates.io, recorded with its run and said by it and by
/// `--status`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Publication {
    /// The release it published from.
    pub(crate) release: String,
    /// Each package it uploaded (`NAME VERSION`), as the index read after the upload lists it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) uploaded: Vec<String>,
    /// Each package whose version crates.io held already.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) skipped: Vec<String>,
    /// Each package it did not publish, and why not: the step off, no token, an upload or a read
    /// that failed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) left: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) why: Option<String>,
    /// Whether what kept them is a failure the run's exit says (an upload or a read that failed),
    /// rather than the step off or no token, which leave a release green.
    #[serde(default)]
    pub(crate) failed: bool,
}

/// Why the step publishes nothing while `[releases] publish` is off.
pub(crate) const OFF: &str = "[releases] publish is off";
/// Why the rest wait: cargo found no token for crates.io.
const NO_TOKEN: &str = "no token: cargo found none for crates.io (its CARGO_REGISTRY_TOKEN, its \
                        credentials or a credential provider, which only cargo reads), so nothing \
                        more is tried";

impl Publication {
    /// One that published nothing from `release`, for `why`; a `failed` one is red.
    pub(crate) fn not(release: &str, why: &str, failed: bool) -> Publication {
        Publication {
            release: release.into(),
            why: Some(why.into()),
            failed,
            ..Publication::default()
        }
    }

    /// The lines the run says of it, which `--status` repeats.
    pub(crate) fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.uploaded.is_empty() {
            lines.push(format!("published {}", self.uploaded.join(", ")));
        }
        if !self.skipped.is_empty() {
            lines.push(format!("on crates.io already: {}", self.skipped.join(", ")));
        }
        match (&self.why, self.left.is_empty()) {
            (Some(why), true) => lines.push(format!("not published: {why}")),
            (Some(why), false) => {
                lines.push(format!("not published: {why}: {}", self.left.join(", ")));
            }
            (None, _) if lines.is_empty() => lines.push("no package to publish".into()),
            (None, _) => {}
        }
        if self.failed {
            lines.push(
                "the release stays tagged; `just release --refresh`, or the next release, \
                 publishes what is left"
                    .into(),
            );
        }
        lines
    }
}

/// How one `cargo publish` ended.
#[derive(Debug, PartialEq, Eq)]
enum Upload {
    Done,
    /// crates.io holds that version already: an index read that lagged an upload.
    There,
    NoToken,
    /// Any other failure, by cargo's line.
    Failed(String),
}

/// How a `cargo publish` that ended with `done` went.
fn upload(done: &Output) -> Upload {
    if done.status.success() {
        return Upload::Done;
    }
    let said = String::from_utf8_lossy(&done.stderr);
    if said.contains("already exists on") || said.contains("is already uploaded") {
        Upload::There
    } else if said.contains("no token found") {
        Upload::NoToken
    } else {
        Upload::Failed(failure(&done.stderr))
    }
}

/// The release `release` published from the checkout `at` within `bound`: the packages
/// [`packages`] names whose version `registry` lacks, by one `cargo publish --locked -p NAME …` run
/// as `cargo` (cargo itself, or the tests' stand-in), its output in `log`. cargo orders them,
/// packages and builds each against the others as packaged before it uploads the first, so one
/// that does not build uploads none, and waits for the index between uploads. The index is read
/// again after it: what it lists went up, and the rest is left for the next publication, why by
/// what cargo said ([`upload`]).
pub(crate) fn publish(
    at: &Path,
    release: &str,
    registry: &Registry,
    cargo: &OsStr,
    log: &Path,
    bound: Bound<'_>,
) -> Publication {
    let packages = match packages(at, bound) {
        Ok(packages) => packages,
        Err(problem) => {
            return Publication::not(
                release,
                &format!("its packages cannot be read: {problem}"),
                true,
            );
        }
    };
    let unread = |problem: String, left: &[Package]| Publication {
        left: said(left),
        ..Publication::not(
            release,
            &format!("{} cannot be read: {problem}", registry.said()),
            true,
        )
    };
    let (held, lacked) = match split(registry, &packages, bound) {
        Ok(split) => split,
        Err(problem) => return unread(problem.into(), &packages),
    };
    let mut done = Publication {
        release: release.into(),
        skipped: said(&held),
        ..Publication::default()
    };
    if lacked.is_empty() {
        return done;
    }
    let named: Vec<String> = (lacked.iter())
        .map(|package| format!("-p {}", package.name))
        .collect();
    let line = format!("cargo publish --locked {}", named.join(" "));
    let mut publish = command(cargo);
    publish.args(["publish", "--locked"]);
    for package in &lacked {
        publish.args(["-p", &package.name]);
    }
    publish
        .env("CARGO_TARGET_DIR", at.join(".target"))
        .current_dir(at);
    let ended = run(&mut publish, &line, bound);
    logged(log, &line, &ended);
    let (up, left) = match split(registry, &lacked, bound) {
        Ok(split) => split,
        Err(problem) => return unread(problem.into(), &lacked),
    };
    done.uploaded = said(&up);
    done.left = said(&left);
    if left.is_empty() {
        return done;
    }
    let (why, failed) = match ended.as_ref().map(upload) {
        Ok(Upload::NoToken) => (NO_TOKEN.to_string(), false),
        Ok(Upload::There) => (
            "cargo found a version on crates.io already, which the index read before it lacked: \
             the next publication finishes what is left"
                .to_string(),
            false,
        ),
        Ok(Upload::Failed(line)) => (
            format!("cargo publish failed ({line}; log {})", log.display()),
            true,
        ),
        Ok(Upload::Done) => (
            format!(
                "cargo publish ended well, but the index does not list them (log {})",
                log.display()
            ),
            true,
        ),
        Err(problem) => (format!("{problem} (log {})", log.display()), true),
    };
    done.why = Some(why);
    done.failed = failed;
    done
}

/// What the publishing command `line` printed, or why it did not end, appended to `log`.
fn logged(log: &Path, line: &str, ended: &Result<Output, String>) {
    let mut text = format!("$ {line}\n");
    match ended {
        Ok(done) => {
            text.push_str(&String::from_utf8_lossy(&done.stdout));
            text.push_str(&String::from_utf8_lossy(&done.stderr));
            text.push_str(&format!("{}\n", done.status));
        }
        Err(problem) => text.push_str(&format!("{problem}\n")),
    }
    if let Some(folder) = log.parent() {
        let _ = fs::create_dir_all(folder);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log) {
        let _ = file.write_all(text.as_bytes());
    }
}

/// The lines a release's notes say of what it publishes: each package of the checkout `at` whose
/// version `registry` lacks when it is tagged; none for a tree that publishes none.
pub(crate) fn notes(at: &Path, registry: &Registry, bound: Bound<'_>) -> Vec<String> {
    let packages = match packages(at, bound) {
        Ok(packages) if packages.is_empty() => return Vec::new(),
        Ok(packages) => packages,
        Err(problem) => return vec![format!("Its packages could not be read: {problem}.")],
    };
    let lacked = match split(registry, &packages, bound) {
        Ok((_, lacked)) => said(&lacked),
        Err(problem) => {
            let problem = String::from(problem);
            return vec![format!(
                "{} could not be read when it was tagged ({problem}): the step after its push \
                 publishes what it lacks.",
                registry.said()
            )];
        }
    };
    if lacked.is_empty() {
        vec![format!(
            "Publishes nothing: {} holds each package's version.",
            registry.said()
        )]
    } else {
        vec![format!(
            "Publishes to {}: {}.",
            registry.said(),
            lacked.join(", ")
        )]
    }
}

#[cfg(test)]
#[path = "crates_tests.rs"]
mod tests;
