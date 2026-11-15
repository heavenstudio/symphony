//! A project's pin to a release (docs/RELEASES.md): its manifests name the release's tag for each
//! dependency on the Symphony's git source (`tag = "release-…"`, in place of the `branch = "main"`,
//! older tag or commit they named), and its `Cargo.lock` binds the tag's commit. This reads the
//! Symphony packages a lock takes from git and the commit each pins, rewrites a manifest's
//! dependencies on those sources and nothing else, and says what else a lock's move took.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;

use super::consumers::git_url;
use super::git::git;

/// The names of the packages the repository at `root` holds at `commit`: each `Cargo.toml` there
/// that has a `[package] name`.
pub(crate) fn packages_at(root: &Path, commit: &str) -> Result<Vec<String>, String> {
    let listed = git(root, &["ls-tree", "-r", "--name-only", commit])?;
    let mut names = BTreeSet::new();
    for path in listed
        .lines()
        .filter(|path| *path == "Cargo.toml" || path.ends_with("/Cargo.toml"))
    {
        let text = git(root, &["show", &format!("{commit}:{path}")])?;
        let name = (text.parse::<toml::Table>().ok()).and_then(|table| {
            table
                .get("package")?
                .get("name")?
                .as_str()
                .map(String::from)
        });
        names.extend(name);
    }
    Ok(names.into_iter().collect())
}

/// A lock's package, as `Cargo.lock` writes it.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
struct Package {
    name: String,
    version: String,
    #[serde(default)]
    source: Option<String>,
}

#[derive(Deserialize)]
struct Lock {
    #[serde(default)]
    package: Vec<Package>,
}

/// The packages of a lock's text.
fn read(text: &str) -> Result<Vec<Package>, String> {
    toml::from_str::<Lock>(text)
        .map(|lock| lock.package)
        .map_err(|e| format!("Cargo.lock does not read: {e}"))
}

/// A Symphony package a project's lock takes from git: its name and version, the source's URL and
/// the commit it pins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Locked {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) url: String,
    pub(crate) commit: String,
}

impl Locked {
    /// How `cargo update -p` names it.
    pub(crate) fn spec(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }
}

/// The packages named `names` that the lock `text` takes from one of the git sources `sources` (by
/// URL, without the branch, tag or commit asked), each with the commit it pins.
pub(crate) fn locked(
    text: &str,
    sources: &[String],
    names: &[String],
) -> Result<Vec<Locked>, String> {
    let mut found = Vec::new();
    for package in read(text)? {
        let Some(source) = package.source.as_deref() else {
            continue;
        };
        let Some(url) = git_url(source).filter(|url| sources.iter().any(|known| known == url))
        else {
            continue;
        };
        if !names.contains(&package.name) {
            continue;
        }
        let commit = source
            .rsplit_once('#')
            .map(|(_, commit)| commit)
            .unwrap_or_default();
        found.push(Locked {
            url: url.to_string(),
            commit: commit.to_string(),
            name: package.name,
            version: package.version,
        });
    }
    found.sort_by(|a, b| (&a.url, &a.name).cmp(&(&b.url, &b.name)));
    Ok(found)
}

/// The keys a git dependency names what it takes by, of which a release's leaves `tag` alone.
const REFERENCES: [&str; 3] = ["branch", "tag", "rev"];

/// Whether two git URLs name one repository: the same but for a trailing `/` or `.git`.
fn same_url(a: &str, b: &str) -> bool {
    let plain = |url: &str| -> String {
        let url = url.trim_end_matches('/');
        url.strip_suffix(".git").unwrap_or(url).to_string()
    };
    plain(a) == plain(b)
}

/// Where in `line` the pair `key = "VALUE"` stands, `key` a word of its own: the pair's start and
/// end, and its value.
fn pair(line: &str, key: &str) -> Option<(usize, usize, String)> {
    let mut from = 0;
    while let Some(found) = line[from..].find(key) {
        let start = from + found;
        from = start + key.len();
        let before = line[..start].chars().next_back();
        if before.is_some_and(|c| !(c.is_whitespace() || c == '{' || c == ',')) {
            continue;
        }
        let rest = &line[from..];
        let Some(value) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let Some(value) = value.trim_start().strip_prefix('"') else {
            continue;
        };
        let Some(close) = value.find('"') else {
            continue;
        };
        let end = line.len() - value.len() + close + 1;
        return Some((start, end, value[..close].to_string()));
    }
    None
}

/// `line`, an inline table, without its pair `key = "…"` and the comma that joined it.
fn without(line: &str, key: &str) -> String {
    let Some((start, end, _)) = pair(line, key) else {
        return line.to_string();
    };
    let before = line[..start].trim_end();
    let after = &line[end..];
    if let Some(before) = before.strip_suffix(',') {
        return format!("{before}{after}");
    }
    let after = after.trim_start();
    let after = after.strip_prefix(',').map_or(after, str::trim_start);
    format!("{} {after}", before.trim_end())
}

/// The manifest `text` with each dependency on one of the git `sources` taking the tag `tag`, in
/// place of the branch, tag or commit it named: the text and how many dependencies it moved. It
/// reads a dependency written as an inline table on one line (`NAME = { git = "URL", branch =
/// "main" }`) or as a table of its own (`[dependencies.NAME]` and its `git = "URL"` line); any
/// other form is left as it is, which [`tagged`] then finds.
pub(crate) fn retag(text: &str, sources: &[String], tag: &str) -> (String, usize) {
    let named = |line: &str| {
        pair(line, "git")
            .is_some_and(|(_, _, url)| sources.iter().any(|known| same_url(known, &url)))
    };
    let reference = |line: &str| {
        let line = line.trim_start();
        REFERENCES.iter().any(|key| {
            line.strip_prefix(key)
                .is_some_and(|rest| rest.trim_start().starts_with('='))
        })
    };
    let header = |line: &str| line.trim_start().starts_with('[');
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut moved = 0;
    let mut at = 0;
    while at < lines.len() {
        if !named(&lines[at]) {
            at += 1;
            continue;
        }
        moved += 1;
        if lines[at].contains('{') {
            let mut line = lines[at].clone();
            for key in REFERENCES {
                while pair(&line, key).is_some() {
                    line = without(&line, key);
                }
            }
            let (_, end, _) = pair(&line, "git").unwrap_or_default();
            line.insert_str(end, &format!(", tag = \"{tag}\""));
            lines[at] = line;
            at += 1;
            continue;
        }
        // A table of its own: its reference lines go, before and after its git line.
        let mut first = at;
        while first > 0 && !header(&lines[first - 1]) {
            first -= 1;
        }
        let mut line = first;
        while line < lines.len() && (line == first || !header(&lines[line])) {
            if reference(&lines[line]) {
                lines.remove(line);
                if line < at {
                    at -= 1;
                }
            } else {
                line += 1;
            }
        }
        let indent: String = lines[at]
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect();
        lines.insert(at + 1, format!("{indent}tag = \"{tag}\""));
        at += 2;
    }
    let mut out = lines.join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    (out, moved)
}

/// Each dependency of the manifest `text` on one of the git `sources` that does not take the tag
/// `tag` alone, by where it stands: none when every one does.
pub(crate) fn tagged(text: &str, sources: &[String], tag: &str) -> Result<Vec<String>, String> {
    fn walk(table: &toml::Table, at: &str, sources: &[String], tag: &str, off: &mut Vec<String>) {
        let ours = (table.get("git").and_then(toml::Value::as_str))
            .is_some_and(|url| sources.iter().any(|known| same_url(known, url)));
        if ours {
            let takes = table.get("tag").and_then(toml::Value::as_str) == Some(tag)
                && !table.contains_key("branch")
                && !table.contains_key("rev");
            if !takes {
                off.push(at.to_string());
            }
        }
        for (key, value) in table {
            if let toml::Value::Table(inner) = value {
                let at = if at.is_empty() {
                    key.clone()
                } else {
                    format!("{at}.{key}")
                };
                walk(inner, &at, sources, tag, off);
            }
        }
    }
    let table = text
        .parse::<toml::Table>()
        .map_err(|e| format!("does not read as TOML: {e}"))?;
    let mut off = Vec::new();
    walk(&table, "", sources, tag, &mut off);
    off.sort();
    Ok(off)
}

/// What else a move from the lock `before` to the lock `after` changed, one line each: a package
/// taken from none of the git `sources` that came, went or changed its version.
pub(crate) fn others(before: &str, after: &str, sources: &[String]) -> Result<Vec<String>, String> {
    let theirs = |packages: Vec<Package>| {
        let mut by_name: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for package in packages {
            let ours = (package.source.as_deref())
                .and_then(git_url)
                .is_some_and(|url| sources.iter().any(|known| known == url));
            if !ours {
                by_name
                    .entry(package.name)
                    .or_default()
                    .insert(package.version);
            }
        }
        by_name
    };
    let before = theirs(read(before)?);
    let after = theirs(read(after)?);
    let names: BTreeSet<&String> = before.keys().chain(after.keys()).collect();
    let listed =
        |versions: &BTreeSet<String>| versions.iter().cloned().collect::<Vec<String>>().join(", ");
    let mut said = Vec::new();
    for name in names {
        match (before.get(name), after.get(name)) {
            (Some(was), Some(now)) if was != now => {
                said.push(format!("{name} {} → {}", listed(was), listed(now)));
            }
            (None, Some(now)) => said.push(format!("{name} {} added", listed(now))),
            (Some(was), None) => said.push(format!("{name} {} removed", listed(was))),
            _ => {}
        }
    }
    Ok(said)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCK: &str = r#"
version = 4

[[package]]
name = "consumer-host"
version = "0.1.0"
dependencies = ["fixture-core", "serde"]

[[package]]
name = "fixture-core"
version = "0.1.0"
source = "git+https://github.com/heavenstudio/symphony?branch=main#aaaaaaaa"

[[package]]
name = "fixture-extra"
version = "0.2.0"
source = "git+https://github.com/heavenstudio/symphony?branch=main#aaaaaaaa"

[[package]]
name = "elsewhere"
version = "1.0.0"
source = "git+https://example.com/other?branch=main#cccccccc"

[[package]]
name = "serde"
version = "1.0.200"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;

    fn sources() -> Vec<String> {
        vec!["https://github.com/heavenstudio/symphony".into()]
    }

    fn names() -> Vec<String> {
        ["fixture-core", "fixture-extra", "unused"]
            .map(String::from)
            .to_vec()
    }

    #[test]
    fn the_lock_names_each_symphony_package_it_takes_from_git_with_its_commit() {
        let found = locked(LOCK, &sources(), &names()).unwrap();
        assert_eq!(
            found,
            [
                Locked {
                    name: "fixture-core".into(),
                    version: "0.1.0".into(),
                    url: "https://github.com/heavenstudio/symphony".into(),
                    commit: "aaaaaaaa".into(),
                },
                Locked {
                    name: "fixture-extra".into(),
                    version: "0.2.0".into(),
                    url: "https://github.com/heavenstudio/symphony".into(),
                    commit: "aaaaaaaa".into(),
                },
            ]
        );
        // A package of another source, or of the source but no name the release holds, is not one.
        assert!(
            locked(LOCK, &sources(), &["elsewhere".into(), "serde".into()])
                .unwrap()
                .is_empty()
        );
        assert!(locked("not = [toml", &sources(), &names()).is_err());
    }

    #[test]
    fn each_dependency_on_the_symphony_takes_the_release_tag_and_nothing_else_moves() {
        let manifest = "\
[workspace.dependencies]
symphony = { git = \"https://github.com/heavenstudio/symphony\", branch = \"main\" }
symphony-api = { branch = \"main\", git = \"https://github.com/heavenstudio/symphony.git\", features = [\"x\"] }
older = {git=\"https://github.com/heavenstudio/symphony/\",tag=\"release-2026-11-01\"}
pinned = { git = \"https://github.com/heavenstudio/symphony\", rev = \"abc\" }
other = { git = \"https://example.com/other\", branch = \"main\" }
git2 = \"0.19\"
serde = { version = \"1\" }

[dependencies.symphony-board]
branch = \"main\"
git = \"https://github.com/heavenstudio/symphony\"
default-features = false

[dependencies.elsewhere]
git = \"https://example.com/other\"
branch = \"main\"
";
        let (moved, count) = retag(manifest, &sources(), "release-2026-11-12");
        assert_eq!(count, 5);
        assert_eq!(
            moved,
            "\
[workspace.dependencies]
symphony = { git = \"https://github.com/heavenstudio/symphony\", tag = \"release-2026-11-12\" }
symphony-api = { git = \"https://github.com/heavenstudio/symphony.git\", tag = \"release-2026-11-12\", features = [\"x\"] }
older = {git=\"https://github.com/heavenstudio/symphony/\", tag = \"release-2026-11-12\"}
pinned = { git = \"https://github.com/heavenstudio/symphony\", tag = \"release-2026-11-12\" }
other = { git = \"https://example.com/other\", branch = \"main\" }
git2 = \"0.19\"
serde = { version = \"1\" }

[dependencies.symphony-board]
git = \"https://github.com/heavenstudio/symphony\"
tag = \"release-2026-11-12\"
default-features = false

[dependencies.elsewhere]
git = \"https://example.com/other\"
branch = \"main\"
"
        );
        assert!(
            tagged(&moved, &sources(), "release-2026-11-12")
                .unwrap()
                .is_empty()
        );
        // Retagged again, it is the same; what it took before is named.
        assert_eq!(
            retag(&moved, &sources(), "release-2026-11-12").0,
            moved,
            "a manifest at the release is left as it is"
        );
        assert_eq!(
            tagged(manifest, &sources(), "release-2026-11-12").unwrap(),
            [
                "dependencies.symphony-board",
                "workspace.dependencies.older",
                "workspace.dependencies.pinned",
                "workspace.dependencies.symphony",
                "workspace.dependencies.symphony-api",
            ]
        );
        assert!(tagged("not [toml", &sources(), "x").is_err());
        assert_eq!(retag("", &sources(), "x"), (String::new(), 0));
    }

    #[test]
    fn what_else_moved_is_each_package_of_no_symphony_source_that_changed() {
        let after = LOCK
            .replace("#aaaaaaaa", "#ffffffff")
            .replace("1.0.200", "1.0.210")
            .replace(
                "[[package]]\nname = \"elsewhere\"\nversion = \"1.0.0\"\nsource = \"git+https://example.com/other?branch=main#cccccccc\"\n",
                "[[package]]\nname = \"added\"\nversion = \"0.3.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
            );
        assert_eq!(
            others(LOCK, &after, &sources()).unwrap(),
            [
                "added 0.3.0 added",
                "elsewhere 1.0.0 removed",
                "serde 1.0.200 → 1.0.210"
            ]
        );
        let only_ours = LOCK.replace("#aaaaaaaa", "#ffffffff");
        assert!(others(LOCK, &only_ours, &sources()).unwrap().is_empty());
    }

    #[test]
    fn the_packages_a_commit_holds_are_its_manifests_package_names() {
        let tree = crate::test_tree::Tree::new();
        let root = tree.path("repo");
        let write = |path: &str, text: &str| {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        };
        write("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n");
        write(
            "crates/a/Cargo.toml",
            "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\n",
        );
        write(
            "crates/b/Cargo.toml",
            "[package]\nname = \"beta\"\nversion = \"0.1.0\"\n",
        );
        write("crates/broken/Cargo.toml", "[package\n");
        write(
            "notes/Cargo.toml.txt",
            "[package]\nname = \"not-a-manifest\"\n",
        );
        let run = |args: &[&str]| {
            git(
                &root,
                &[
                    &["-c", "user.name=t", "-c", "user.email=t@example.com"][..],
                    args,
                ]
                .concat(),
            )
            .unwrap()
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["add", "-A"]);
        run(&["commit", "-q", "-m", "first"]);
        assert_eq!(packages_at(&root, "HEAD").unwrap(), ["alpha", "beta"]);
        assert!(packages_at(&root, "nowhere").is_err());
    }
}
