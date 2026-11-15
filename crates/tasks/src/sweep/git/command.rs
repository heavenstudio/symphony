//! Commands always discover the explicitly selected checkout and leave its index untouched.

use crate::sys::{Command, Stdio};
use std::path::Path;
use std::process::Output;

/// `git ARGS` in `dir`: none when git cannot be started there (the folder is gone).
pub fn git(dir: &Path, args: &[&str]) -> Option<Output> {
    crate::sys::output(command(dir).args(args)).ok()
}

fn command(dir: &Path) -> Command {
    // A hook or launcher can inherit the caller's repository and index. Each sweep operation
    // must instead discover the repository belonging to its explicit working directory.
    let mut command = crate::sys::git();
    command.current_dir(dir).stdin(Stdio::null());
    command
}

/// What `git ARGS` printed, when it succeeded.
pub(crate) fn stdout(dir: &Path, args: &[&str]) -> Option<String> {
    read(dir, args).map(|done| String::from_utf8_lossy(&done.stdout).into_owned())
}

/// A read's original bytes, including NUL-delimited names, without refreshing the writer's index.
pub(crate) fn read(dir: &Path, args: &[&str]) -> Option<Output> {
    told(dir, args).ok()
}

/// [`read`], or why it could not be made: git's own first line, or that it could not run.
pub(crate) fn told(dir: &Path, args: &[&str]) -> Result<Output, String> {
    let done = crate::sys::output(reading(dir).args(args)).map_err(|error| {
        format!(
            "git {} could not run in {}: {error}",
            args[0],
            dir.display()
        )
    })?;
    if done.status.success() {
        return Ok(done);
    }
    let said = String::from_utf8_lossy(&done.stderr);
    let said = said.lines().map(str::trim).find(|l| !l.is_empty());
    Err(format!(
        "git {} failed in {}: {}",
        args[0],
        dir.display(),
        said.unwrap_or("no reason given")
    ))
}

/// `git` in `dir` for a read of another agent's tree: a diff can refresh the index even when
/// optional locks are disabled, so both settings are needed to leave the writer's index alone.
fn reading(dir: &Path) -> Command {
    let mut command = command(dir);
    command
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(["-c", "diff.autoRefreshIndex=false"]);
    command
}

/// The text of each object `specs` names (`REV:PATH`), in order, none where it names none: one
/// `git cat-file --batch` in `dir`, or why it could not be read.
pub(crate) fn blobs(dir: &Path, specs: &[String]) -> Result<Vec<Option<String>>, String> {
    if specs.is_empty() {
        return Ok(Vec::new());
    }
    let failed = |why: String| format!("git cat-file could not read in {}: {why}", dir.display());
    let mut child = crate::sys::spawn_piped(reading(dir).args(["cat-file", "--batch"]))
        .map_err(|error| failed(error.to_string()))?;
    let input: String = specs.iter().map(|spec| format!("{spec}\n")).collect();
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| failed("no input".into()))?;
    // Written beside the read: git answers as it reads, so its output never waits on its input.
    let writer = std::thread::spawn(move || {
        use std::io::Write as _;
        let _ = stdin.write_all(input.as_bytes());
    });
    let done = crate::sys::wait_output(child).map_err(|error| failed(error.to_string()))?;
    let _ = writer.join();
    if !done.status.success() {
        let said = String::from_utf8_lossy(&done.stderr).trim().to_string();
        return Err(failed(said));
    }
    let data = done.stdout;
    let mut out = Vec::with_capacity(specs.len());
    let mut at = 0;
    for spec in specs {
        let end = data
            .get(at..)
            .and_then(|rest| rest.iter().position(|b| *b == b'\n'))
            .map(|p| at + p)
            .ok_or_else(|| failed(format!("no answer for {spec}")))?;
        let head = String::from_utf8_lossy(&data[at..end]).into_owned();
        at = end + 1;
        // `NAME missing`: no such file there, as for a file the commit does not hold.
        if head == format!("{spec} missing") {
            out.push(None);
            continue;
        }
        let mut words = head.split(' ');
        let (Some(_), Some(kind), Some(size), None) =
            (words.next(), words.next(), words.next(), words.next())
        else {
            return Err(failed(format!("an answer it cannot read: {head}")));
        };
        let size: usize = size
            .parse()
            .map_err(|_| failed(format!("an answer it cannot read: {head}")))?;
        let body = data
            .get(at..at + size)
            .ok_or_else(|| failed(format!("{spec} cut short")))?;
        // Another kind of object (a folder, a submodule's commit) holds no text.
        out.push((kind == "blob").then(|| String::from_utf8_lossy(body).into_owned()));
        at += size + 1;
    }
    Ok(out)
}

/// Whether `git ARGS` succeeded.
pub fn succeeds(dir: &Path, args: &[&str]) -> bool {
    git(dir, args).is_some_and(|done| done.status.success())
}

/// The commit `rev` names, if it names one.
pub fn rev(dir: &Path, rev: &str) -> Option<String> {
    stdout(dir, &["rev-parse", "--verify", "--quiet", rev]).map(|out| out.trim().to_string())
}
