//! `symphony attach FILE…`: evidence a thread post shows, named by its content. Each file is stored
//! as the owner's uploads are ([`crate::uploads::store`], under their rules) in the main checkout's
//! store, which the server reads for every page and branch, so the line printed for it
//! ([`crate::uploads::Stored::markup`]) outlives the worktree the file was made in.

use std::io::Write;
use std::path::{Path, PathBuf};

use symphony_args::parse;

use crate::uploads::{self, MAX_BYTES, Refusal};

const USAGE: &str = "usage: symphony attach [-h] FILE [FILE ...]";
const HELP: &str =
    "Evidence attached by its content: each FILE is stored, once per content, in the uploads
of the main checkout of the repository this runs in (a linked worktree's too), where the server
reads the owner's uploads for every page and branch, and the Markdown line a thread post carries
to show it is printed, one line a file. A file the uploads refuse (empty, over 10 MB, or no
picture or document they store: SVG, HTML and scripts are refused) is named with why, stays
where it is, and the rest are still attached.

positional arguments:
  FILE        a picture (PNG, JPEG, GIF, WebP) or a document (PDF, text, Word, Excel,
              PowerPoint), by its path from here

options:
  -h, --help  show this help message and exit

Exit 0 with every file attached, 1 when one was refused or could not be read, 2 for a command
line it cannot run.";

/// `symphony attach FILE…`, from the current folder. Its exit code.
pub(super) fn main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let files = match parse(args, &[]) {
        Ok(parsed) if parsed.help => {
            let _ = writeln!(out, "{USAGE}\n\n{HELP}");
            return 0;
        }
        Ok(parsed) if parsed.positionals.is_empty() => {
            return crate::cli::refuse(
                err,
                USAGE,
                "attach",
                "the following arguments are required: FILE",
            );
        }
        Ok(parsed) => parsed.positionals,
        Err(problem) => return crate::cli::refuse(err, USAGE, "attach", &problem),
    };
    let here = match std::env::current_dir() {
        Ok(here) => here,
        Err(error) => {
            let _ = writeln!(err, "symphony attach: the current folder: {error}");
            return 1;
        }
    };
    let files: Vec<PathBuf> = files.into_iter().map(PathBuf::from).collect();
    attach(&here, &files, out, err)
}

/// Each of `files`, by its path from `here`, stored in the uploads of the main checkout of the
/// repository `here` is in, in order: its line on `out`, or on `err` the file as given and why it
/// was not stored. Exit 0 with every file stored, else 1.
pub(super) fn attach(
    here: &Path,
    files: &[PathBuf],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let root = match main_checkout(here) {
        Ok(root) => root,
        Err(problem) => {
            let _ = writeln!(err, "symphony attach: {problem}");
            return 1;
        }
    };
    let mut refused = false;
    for file in files {
        match stored(&root, &here.join(file)) {
            Ok(stored) => {
                let _ = writeln!(out, "{}", stored.markup());
            }
            Err(why) => {
                refused = true;
                let _ = writeln!(err, "symphony attach: {}: {why}", file.display());
            }
        }
    }
    i32::from(refused)
}

/// The main checkout of the repository `here` is in, read from its markers with no `git` process:
/// the checkout's own top folder, or a linked worktree's main checkout. The server's root, so its
/// uploads are the ones every page reads.
fn main_checkout(here: &Path) -> Result<PathBuf, String> {
    let (top, _) = symphony_board::git::checkout(here).ok_or_else(|| {
        format!(
            "{} is in no git checkout, so there is no main checkout to store in",
            here.display()
        )
    })?;
    Ok(symphony_board::builds::main_of(&top))
}

/// `file` stored in `root`'s uploads, by its file name and content. One larger than the uploads
/// take is refused by its size, unread.
fn stored(root: &Path, file: &Path) -> Result<uploads::Stored, String> {
    let meta = std::fs::metadata(file).map_err(|error| error.to_string())?;
    if !meta.is_file() {
        return Err("not a file".into());
    }
    if meta.len() > MAX_BYTES as u64 {
        let bytes = usize::try_from(meta.len()).unwrap_or(usize::MAX);
        return Err(Refusal::TooLarge { bytes }.to_string());
    }
    let bytes = std::fs::read(file).map_err(|error| error.to_string())?;
    let name = file
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    uploads::store(root, &name, &bytes).map_err(|failure| failure.error)
}

#[cfg(test)]
mod tests;
