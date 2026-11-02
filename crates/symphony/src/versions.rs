//! Document commits, rename following and unified patches, read in-process with gix.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

use gix::bstr::ByteSlice;
use symphony_api::{DocumentDiff, Version};

struct Record {
    version: Version,
    id: gix::ObjectId,
    time: i64,
    path: String,
    previous_path: String,
    before: Option<gix::ObjectId>,
    after: Option<gix::ObjectId>,
}

pub(crate) fn entry(tree: &gix::Tree<'_>, path: &str) -> Option<gix::ObjectId> {
    tree.lookup_entry_by_path(path)
        .ok()
        .flatten()
        .map(|entry| entry.id().detach())
}

fn renamed(before: &gix::Tree<'_>, after: &gix::Tree<'_>, path: &str) -> Option<String> {
    use gix::diff::tree::visit::Change as Tracked;
    use gix::object::tree::diff::Change;

    // A repository-wide rename search can exhaust gix's comparison budget on unrelated moves.
    // History has just one destination: match that document against every deletion. With one
    // addition the search is linear in deleted entries, so it needs no global comparison cutoff.
    let mut tracker = gix::diff::rewrites::Tracker::new(gix::diff::Rewrites {
        limit: 0,
        ..Default::default()
    });
    before
        .changes()
        .ok()?
        .options(|options| {
            options.track_path().track_rewrites(None);
        })
        .for_each_to_obtain_tree(after, |change| {
            match change {
                Change::Deletion {
                    location,
                    entry_mode,
                    id,
                    ..
                } => {
                    let _ = tracker.try_push_change(
                        Tracked::Deletion {
                            entry_mode,
                            oid: id.detach(),
                            relation: None,
                        },
                        location,
                    );
                }
                Change::Addition {
                    location,
                    entry_mode,
                    id,
                    ..
                } if location.as_bytes() == path.as_bytes() => {
                    let _ = tracker.try_push_change(
                        Tracked::Addition {
                            entry_mode,
                            oid: id.detach(),
                            relation: None,
                        },
                        location,
                    );
                }
                _ => {}
            }
            Ok::<_, std::convert::Infallible>(std::ops::ControlFlow::Continue(()))
        })
        .ok()?;
    let mut cache = before.repo.diff_resource_cache_for_tree_diff().ok()?;
    let mut found = None;
    tracker
        .emit(
            |_, source| {
                if let Some(source) = source {
                    found = Some(source.location.to_str_lossy().into_owned());
                    std::ops::ControlFlow::Break(())
                } else {
                    std::ops::ControlFlow::Continue(())
                }
            },
            &mut cache,
            &before.repo.objects,
            |_| Ok::<_, std::convert::Infallible>(()),
        )
        .ok()?;
    found
}

fn records(repo: &gix::Repository, rel: &str) -> Option<Vec<Record>> {
    let head = repo.head_id().ok()?.detach();
    let mut queued = VecDeque::from([(head, rel.to_string())]);
    let mut visited = BTreeSet::new();
    let mut found = BTreeMap::new();
    while let Some((id, path)) = queued.pop_front() {
        if !visited.insert((id, path.clone())) {
            continue;
        }
        let commit = repo.find_commit(id).ok()?;
        let tree = commit.tree().ok()?;
        let after = entry(&tree, &path);
        let mut parents = Vec::new();
        for parent in commit.parent_ids() {
            let parent_tree = parent.object().ok()?.try_into_commit().ok()?.tree().ok()?;
            let old_path = if after.is_some() && entry(&parent_tree, &path).is_none() {
                renamed(&parent_tree, &tree, &path).unwrap_or_else(|| path.clone())
            } else {
                path.clone()
            };
            let before = entry(&parent_tree, &old_path);
            parents.push((parent.detach(), old_path, before));
        }
        queued.extend(
            parents
                .iter()
                .map(|(parent, path, _)| (*parent, path.clone())),
        );
        // --follow does not show merge patches. Its versions are the original edits on each side.
        let unchanged = parents
            .iter()
            .any(|(_, old_path, before)| old_path == &path && *before == after);
        if parents.len() > 1 || unchanged || (parents.is_empty() && after.is_none()) {
            continue;
        }
        let (_, previous_path, before) = parents
            .into_iter()
            .next()
            .unwrap_or_else(|| (id, path.clone(), None));
        let time = commit.time().ok()?;
        let version = Version {
            sha: commit.short_id().ok()?.to_string(),
            date: time.format(gix::date::time::format::SHORT).ok()?,
            subject: commit_subject(&commit)?,
        };
        found.entry(id).or_insert(Record {
            version,
            id,
            time: time.seconds,
            path,
            previous_path,
            before,
            after,
        });
    }
    let mut found = found.into_values().collect::<Vec<_>>();
    found.sort_by(|a, b| b.time.cmp(&a.time).then_with(|| b.id.cmp(&a.id)));
    Some(found)
}

fn open(root: &Path) -> Option<gix::Repository> {
    let mut repo = gix::discover(root).ok()?;
    repo.object_cache_size(Some(16 * 1024 * 1024));
    Some(repo)
}

pub fn versions(root: &Path, rel: &str, limit: usize) -> Vec<Version> {
    let mut rows = open(root)
        .and_then(|repo| records(&repo, rel))
        .unwrap_or_default();
    if limit != 0 {
        rows.truncate(limit);
    }
    rows.into_iter().map(|record| record.version).collect()
}

pub fn subject(root: &Path, sha: &str) -> String {
    if !valid_sha(sha) {
        return String::new();
    }
    open(root)
        .and_then(|repo| {
            let commit = repo
                .rev_parse_single(sha)
                .ok()?
                .object()
                .ok()?
                .try_into_commit()
                .ok()?;
            commit_subject(&commit)
        })
        .unwrap_or_default()
}

fn commit_subject(commit: &gix::Commit<'_>) -> Option<String> {
    Some(
        commit
            .message()
            .ok()?
            .title
            .to_str_lossy()
            .lines()
            .map(str::trim)
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn valid_sha(sha: &str) -> bool {
    (4..=40).contains(&sha.len())
        && sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn diff(root: &Path, rel: &str, sha: &str) -> Option<DocumentDiff> {
    if !valid_sha(sha) {
        return None;
    }
    let repo = open(root)?;
    let resolved = repo.rev_parse_single(sha).ok()?.detach();
    let record = records(&repo, rel)?
        .into_iter()
        .find(|record| record.id == resolved)?;
    let data = |id: Option<gix::ObjectId>| -> Option<Vec<u8>> {
        match id {
            Some(id) => Some(repo.find_object(id).ok()?.try_into_blob().ok()?.take_data()),
            None => Some(Vec::new()),
        }
    };
    let before = data(record.before)?;
    let after = data(record.after)?;
    let mut patch = format!("diff --git a/{} b/{}\n", record.previous_path, record.path);
    if record.previous_path != record.path {
        patch.push_str(&format!(
            "rename from {}\nrename to {}\n",
            record.previous_path, record.path
        ));
    }
    if before != after {
        let old_name = if record.before.is_some() {
            format!("a/{}", record.previous_path)
        } else {
            "/dev/null".into()
        };
        let new_name = if record.after.is_some() {
            format!("b/{}", record.path)
        } else {
            "/dev/null".into()
        };
        patch.push_str(&format!("--- {old_name}\n+++ {new_name}\n"));
        let input = gix::diff::blob::InternedInput::new(before.as_slice(), after.as_slice());
        let difference =
            gix::diff::blob::diff_with_slider_heuristics(gix::diff::blob::Algorithm::Myers, &input);
        let output = gix::diff::blob::UnifiedDiff::new(
            &difference,
            &input,
            Patch(patch),
            Default::default(),
        )
        .consume()
        .ok()?;
        patch = output;
    }
    Some(DocumentDiff {
        sha: record.version.sha,
        subject: record.version.subject,
        diff: patch,
    })
}

struct Patch(String);

impl gix::diff::blob::unified_diff::ConsumeHunk for Patch {
    type Out = String;
    fn consume_hunk(
        &mut self,
        header: gix::diff::blob::unified_diff::HunkHeader,
        lines: &[(gix::diff::blob::unified_diff::DiffLineKind, &[u8])],
    ) -> std::io::Result<()> {
        use gix::diff::blob::unified_diff::DiffLineKind;
        let range = |start: u32, count: u32| match count {
            0 => format!("{},0", start.saturating_sub(1)),
            1 => start.to_string(),
            _ => format!("{start},{count}"),
        };
        self.0.push_str(&format!(
            "@@ -{} +{} @@\n",
            range(header.before_hunk_start, header.before_hunk_len),
            range(header.after_hunk_start, header.after_hunk_len)
        ));
        for (kind, line) in lines {
            self.0.push(match kind {
                DiffLineKind::Add => '+',
                DiffLineKind::Remove => '-',
                DiffLineKind::Context => ' ',
            });
            self.0.push_str(&String::from_utf8_lossy(line));
            if !line.ends_with(b"\n") {
                self.0.push_str("\n\\ No newline at end of file\n");
            }
        }
        Ok(())
    }
    fn finish(self) -> Self::Out {
        self.0
    }
}
