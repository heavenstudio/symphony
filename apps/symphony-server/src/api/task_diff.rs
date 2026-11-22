//! A task's bounded Git comparison and its individually requested file patches. A summary
//! keeps at most 500 files and 256 KiB of numstat output; a patch keeps 256 KiB of Git output.
//! All Git calls in a request share five seconds. No patch is part of the summary or task page.

use super::{Request, error, no_store, query};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use symphony_api::{TaskDiff, TaskDiffFile, TaskDiffPatch, TaskDiffSource};
use tokio::time::Instant;

mod git;

const BYTES: usize = 256 * 1024;
const FILES: usize = 500;
const TIME: Duration = Duration::from_secs(5);
const BOUND: &str = "Showing at most 500 files and 256 KiB of file counts.";

struct Input {
    branch: String,
    default: String,
    landing: Option<String>,
}

pub(super) async fn answer(
    hub: Arc<crate::live::Hub>,
    root: PathBuf,
    request: &Request,
    patch: bool,
) -> Response {
    let slug = query(request, "slug").unwrap_or_default();
    let folder = root.clone();
    let input = crate::spawn::blocking(move || {
        let store = hub.reader().map_err(|e| super::unavailable(&e, ""))?;
        // The board's server may have gone, or moved: the next read opens it anew.
        let read = store
            .get(&slug)
            .and_then(|_| Ok((store.thread(&slug)?, store.task_sessions(&slug)?)))
            .map_err(super::task_failure);
        let (thread, sessions) = hub.read_on(&store, read)?;
        let branch = sessions
            .into_iter()
            .rev()
            .find(|s| s.role == "executor" && !s.branch.is_empty())
            .map_or(slug, |s| s.branch);
        Ok::<_, (StatusCode, String)>(Input {
            branch,
            default: symphony::project::Project::of(&folder)
                .default_branch
                .clone(),
            landing: symphony_board::landing(&thread).map(str::to_owned),
        })
    })
    .await;
    let input = match input {
        Ok(Ok(input)) => input,
        Ok(Err((status, why))) => return error(status, why),
        Err(why) => return error(StatusCode::SERVICE_UNAVAILABLE, why.to_string()),
    };
    let deadline = Instant::now() + TIME;
    if patch {
        let path = query(request, "path").unwrap_or_default();
        let base = query(request, "base").unwrap_or_default();
        let head = query(request, "head").unwrap_or_default();
        match file(&root, &input, (&base, &head), &path, deadline).await {
            Ok(patch) => no_store(Json(patch)).into_response(),
            Err((status, why)) => error(status, why),
        }
    } else {
        no_store(Json(summary(&root, &input, deadline).await)).into_response()
    }
}

fn unavailable(reason: String) -> TaskDiff {
    TaskDiff {
        source: TaskDiffSource::Unavailable,
        base: None,
        head: None,
        files: vec![],
        truncated: false,
        reason: Some(reason),
    }
}

async fn commit(root: &Path, spec: &str, deadline: Instant) -> Result<String, String> {
    let output = git::read(
        root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{spec}^{{commit}}"),
        ],
        128,
        deadline,
    )
    .await?;
    let id = String::from_utf8(output.bytes).map_err(|e| e.to_string())?;
    let id = id.trim();
    if output.truncated
        || !matches!(id.len(), 40 | 64)
        || !id.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("Git did not return a full commit id.".into());
    }
    Ok(id.into())
}

async fn comparison(root: &Path, input: &Input, deadline: Instant) -> Result<TaskDiff, String> {
    let (source, base, head) = if let Some(landing) = &input.landing {
        if !(7..=64).contains(&landing.len()) || !landing.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("The recorded landing does not name a commit.".into());
        }
        let head = commit(root, landing, deadline).await?;
        // A landing names a merge, and must still belong to the project's default branch.
        commit(root, &format!("{head}^2"), deadline).await?;
        git::read(
            root,
            &[
                "merge-base",
                "--is-ancestor",
                &head,
                &format!("refs/heads/{}", input.default),
            ],
            128,
            deadline,
        )
        .await?;
        let base = commit(root, &format!("{head}^1"), deadline).await?;
        (TaskDiffSource::Landing, base, head)
    } else {
        let head = commit(root, &format!("refs/heads/{}", input.branch), deadline)
            .await
            .map_err(|why| {
                format!("The task branch is unavailable or gone; no landing is recorded. {why}")
            })?;
        let default = commit(root, &format!("refs/heads/{}", input.default), deadline).await?;
        let base = git::read(root, &["merge-base", &default, &head], 128, deadline).await?;
        if base.truncated {
            return Err("The merge base could not be read within its bound.".into());
        }
        let base = String::from_utf8(base.bytes).map_err(|e| e.to_string())?;
        (TaskDiffSource::Branch, base.trim().to_owned(), head)
    };
    Ok(TaskDiff {
        source,
        base: Some(base),
        head: Some(head),
        files: vec![],
        truncated: false,
        reason: None,
    })
}

/// NUL-terminated numstat records, with renames disabled. Tabs and newlines in a path are
/// literal; only the two count separators are split. An incomplete last record is omitted.
fn files(output: &git::Output) -> Result<(Vec<TaskDiffFile>, bool), String> {
    let mut files = Vec::new();
    let mut truncated = output.truncated;
    let mut records = output.bytes.split_inclusive(|b| *b == 0);
    for record in &mut records {
        if files.len() == FILES || record.last() != Some(&0) {
            truncated = true;
            break;
        }
        let mut columns = record[..record.len() - 1].splitn(3, |b| *b == b'\t');
        let count = |value: Option<&[u8]>| -> Result<Option<u64>, String> {
            match value {
                Some(b"-") => Ok(None),
                Some(bytes) => std::str::from_utf8(bytes)
                    .ok()
                    .and_then(|n| n.parse().ok())
                    .map(Some)
                    .ok_or_else(|| "Git returned unreadable file counts.".into()),
                None => Err("Git returned incomplete file counts.".into()),
            }
        };
        let added = count(columns.next())?;
        let removed = count(columns.next())?;
        let path = columns.next().ok_or("Git returned no file path.")?;
        let Ok(path) = std::str::from_utf8(path) else {
            truncated = true;
            continue;
        };
        if path.is_empty() || added.is_none() != removed.is_none() {
            return Err("Git returned an unreadable file record.".into());
        }
        files.push(TaskDiffFile {
            path: path.into(),
            added,
            removed,
        });
    }
    Ok((files, truncated))
}

async fn numstat(
    root: &Path,
    diff: &TaskDiff,
    path: Option<&str>,
    deadline: Instant,
) -> Result<git::Output, String> {
    let mut args = vec![
        "diff",
        "--numstat",
        "-z",
        "--no-renames",
        "--no-ext-diff",
        "--no-textconv",
        "--ignore-submodules=none",
        "--submodule=short",
        diff.base.as_deref().unwrap(),
        diff.head.as_deref().unwrap(),
        "--",
    ];
    if let Some(path) = path {
        args.push(path);
    }
    git::read(root, &args, BYTES, deadline).await
}

async fn summary(root: &Path, input: &Input, deadline: Instant) -> TaskDiff {
    let read = async {
        let mut diff = comparison(root, input, deadline).await?;
        let output = numstat(root, &diff, None, deadline).await?;
        (diff.files, diff.truncated) = files(&output)?;
        if diff.truncated {
            diff.reason = Some(format!("{BOUND} Paths that are not UTF-8 are omitted."));
        }
        Ok::<_, String>(diff)
    };
    read.await.unwrap_or_else(unavailable)
}

async fn file(
    root: &Path,
    input: &Input,
    pair: (&str, &str),
    path: &str,
    deadline: Instant,
) -> Result<TaskDiffPatch, (StatusCode, String)> {
    if path.is_empty()
        || path.contains('\0')
        || Path::new(path)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "A file must name one repository-relative path.".into(),
        ));
    }
    let diff = comparison(root, input, deadline)
        .await
        .map_err(|why| (StatusCode::SERVICE_UNAVAILABLE, why))?;
    if diff.base.as_deref() != Some(pair.0) || diff.head.as_deref() != Some(pair.1) {
        return Err((
            StatusCode::CONFLICT,
            "The task comparison changed. Refresh its file list.".into(),
        ));
    }
    let listed = numstat(root, &diff, Some(path), deadline)
        .await
        .map_err(|why| (StatusCode::SERVICE_UNAVAILABLE, why))?;
    let (listed, _) = files(&listed).map_err(|why| (StatusCode::SERVICE_UNAVAILABLE, why))?;
    if !listed.iter().any(|file| file.path == path) {
        return Err((
            StatusCode::NOT_FOUND,
            "This comparison has no changed file by that path.".into(),
        ));
    }
    let output = git::read(
        root,
        &[
            "diff",
            "--patch",
            "--no-color",
            "--no-renames",
            "--no-ext-diff",
            "--no-textconv",
            "--ignore-submodules=none",
            "--submodule=short",
            pair.0,
            pair.1,
            "--",
            path,
        ],
        BYTES,
        deadline,
    )
    .await
    .map_err(|why| (StatusCode::SERVICE_UNAVAILABLE, why))?;
    let mut patch = String::from_utf8_lossy(&output.bytes).into_owned();
    let truncated = output.truncated || patch.len() > BYTES;
    patch.truncate(patch.floor_char_boundary(BYTES.min(patch.len())));
    Ok(TaskDiffPatch {
        base: pair.0.into(),
        head: pair.1.into(),
        path: path.into(),
        patch,
        truncated,
    })
}

#[cfg(test)]
mod tests;
