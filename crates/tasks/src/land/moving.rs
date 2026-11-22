//! How a land moves the main checkout, and what it does when a move did not happen: the
//! fast-forward shielded from signals in a group of its own ([`Landing::fast_forward`]), named in
//! [`MOVING`] while it runs ([`Moving`]), and the restore of what a move that ended without moving
//! the default branch half-wrote ([`Landing::restore`]), right after it or, for a land killed
//! outright meanwhile, by the next land ([`Landing::recover`]).

use super::{
    Ended, Landing, MOVING, Record, and, at, git_z, on_paths, run_git, short, signal_name, watched,
};
use crate::sweep::git::ancestor;
use crate::sys::{self, Stdio};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::os::unix::process::ExitStatusExt as _;
use std::path::Path;

/// The fast-forward of the main checkout a land is making, kept in [`MOVING`] while it runs: the
/// task, the default branch's tip it moves from and the commit it moves to, and whether the land
/// recorded its merge for this move ([`Record`]). A land killed outright while it ran leaves it,
/// and the next restores what a move that did not happen half-wrote ([`Landing::recover`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Moving {
    pub slug: String,
    pub from: String,
    pub to: String,
    pub recorded: bool,
}

impl Moving {
    /// The move named in the lands' `folder`, if one is.
    pub fn read(folder: &Path) -> Option<Moving> {
        let text = fs::read_to_string(folder.join(MOVING)).ok()?;
        crate::objects::parse(&text)
    }

    /// Named in `folder`, whole or not at all.
    pub fn write(&self, folder: &Path) -> Result<(), String> {
        let path = folder.join(MOVING);
        let staged = folder.join(format!(".{MOVING}.{}", std::process::id()));
        let text = serde_json::to_string(self).map_err(|e| e.to_string())?;
        fs::write(&staged, text).map_err(|e| at(&staged, &e))?;
        fs::rename(&staged, &path).map_err(|e| at(&path, &e))
    }

    /// No move is named in `folder` any more: it ended, and what it left is settled.
    pub fn done(folder: &Path) {
        let _ = fs::remove_file(folder.join(MOVING));
    }
}

/// What the fast-forward of the main checkout came to ([`Landing::fast_forward`]).
pub(super) enum Move {
    /// The default branch holds the commit it moved to.
    Done,
    /// It is where it was: why, and what the land restored of what the move half-wrote.
    Not {
        why: String,
        restored: Result<Restored, String>,
    },
    /// It moved under the land, to this commit.
    Raced(String),
}

/// What a default branch `default` that could not move to `to` from `base` says: `why`, what the
/// land `restored` of what the move half-wrote, or that it could not, and, for a land whose record
/// of the merge was `recorded`, that the record is dropped.
pub(super) fn not_moved(
    default: &str,
    to: &str,
    base: &str,
    why: &str,
    restored: &Result<Restored, String>,
    recorded: bool,
) -> String {
    let mut line = format!(
        "{default} could not move to {} in the main checkout: {why}",
        short(to)
    );
    match restored {
        Ok(restored) => {
            if let Some(said) = restored.said(default, base) {
                line.push_str(&format!("; {said}"));
            }
            if recorded {
                line.push_str("; its record dropped: nothing landed");
            }
        }
        Err(why) => line.push_str(&format!(
            "; what the move half-wrote in the main checkout could not be restored ({why}): the \
             next land tries again"
        )),
    }
    line
}

/// One side of a path in a tree or the index: its mode and object, as git writes them.
type Entry = (String, String);

/// A path the commits `from` and `to` differ in, with its entry in each (none: not there).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Change {
    path: String,
    from: Option<Entry>,
    to: Option<Entry>,
}

/// The paths `from` and `to` differ in, as `git diff-tree -r -z --no-renames` gives them.
fn changes(dir: &Path, from: &str, to: &str) -> Result<Vec<Change>, String> {
    let fields = git_z(dir, &["diff-tree", "-r", "-z", "--no-renames", from, to])?;
    let side = |mode: &str, object: &str| {
        (!mode.trim_matches('0').is_empty()).then(|| (mode.to_string(), object.to_string()))
    };
    let mut changes = Vec::new();
    let mut fields = fields.into_iter();
    while let (Some(meta), Some(path)) = (fields.next(), fields.next()) {
        let words: Vec<&str> = meta.trim_start_matches(':').split(' ').collect();
        let [from_mode, to_mode, from_object, to_object, ..] = words.as_slice() else {
            return Err(format!("git diff-tree said {meta:?}"));
        };
        changes.push(Change {
            path,
            from: side(from_mode, from_object),
            to: side(to_mode, to_object),
        });
    }
    Ok(changes)
}

/// What the index of the checkout `dir` holds of each path: its entry, or none for a path whose
/// conflict is unresolved (an entry of a stage past 0).
fn staged(dir: &Path) -> Result<HashMap<String, Option<Entry>>, String> {
    let mut index = HashMap::new();
    for record in git_z(dir, &["ls-files", "-s", "-z"])? {
        let Some((meta, path)) = record.split_once('\t') else {
            continue;
        };
        let words: Vec<&str> = meta.split(' ').collect();
        let [mode, object, stage] = words.as_slice() else {
            continue;
        };
        let entry = (*stage == "0").then(|| ((*mode).to_string(), (*object).to_string()));
        index
            .entry(path.to_string())
            .and_modify(|held: &mut Option<Entry>| *held = None)
            .or_insert(entry);
    }
    Ok(index)
}

/// Whether nothing is at `path`, not even a link.
fn absent(path: &Path) -> bool {
    fs::symlink_metadata(path).is_err()
}

/// Remove the folders `path` was in that it left empty, up to `root`.
fn prune(root: &Path, path: &Path) {
    let mut folder = path.parent();
    while let Some(dir) = folder {
        if dir == root || !dir.starts_with(root) || fs::remove_dir(dir).is_err() {
            break;
        }
        folder = dir.parent();
    }
}

/// What a land put back of a fast-forward that ended without moving the default branch: the paths
/// it restored as the branch has them, those it removed, which the branch lacks, and those it left,
/// which someone changed otherwise.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Restored {
    pub(super) put: Vec<String>,
    pub(super) removed: Vec<String>,
    pub(super) left: Vec<String>,
}

impl Restored {
    /// What the land says of it, the default branch `default` being at `from`: none when it found
    /// nothing written.
    pub(super) fn said(&self, default: &str, from: &str) -> Option<String> {
        let names = |paths: &[String]| and(&paths.iter().map(String::as_str).collect::<Vec<_>>());
        let at = format!("{default} at {}", short(from));
        let mut parts = Vec::new();
        if !self.put.is_empty() {
            parts.push(format!(
                "restored {} as {at} has {}",
                names(&self.put),
                if self.put.len() == 1 { "it" } else { "them" }
            ));
        }
        if !self.removed.is_empty() {
            parts.push(format!(
                "removed {}, which {at} lacks",
                names(&self.removed)
            ));
        }
        if !self.left.is_empty() {
            parts.push(format!(
                "left {} as {} found {}, changed otherwise",
                names(&self.left),
                if self.left.len() == 1 { "it" } else { "they" },
                if self.left.len() == 1 { "it" } else { "them" }
            ));
        }
        (!parts.is_empty()).then(|| {
            format!(
                "in the main checkout, which the move half-wrote, the land {}",
                parts.join("; ")
            )
        })
    }
}

impl Landing<'_> {
    /// Settle the move of the main checkout an earlier land left named in [`MOVING`], killed
    /// outright while it moved, once no process of that move runs ([`Landing::leftover_group`]):
    /// a move that happened leaves the record of its merge for the land of its task to finish; one
    /// that did not has what it half-wrote restored ([`Landing::restore`]) and its record dropped,
    /// said here and on that task's thread. With a git still holding the main checkout's index, it
    /// refuses.
    pub(super) fn recover(&mut self) -> Result<(), String> {
        let Some(moving) = Moving::read(&self.folder) else {
            return Ok(());
        };
        let default = self.settings.default_branch.clone();
        let now = self.base()?;
        if now != moving.from {
            // It moved, or the branch moved by hand since: nothing of the land's is left half-done.
            Moving::done(&self.folder);
            return Ok(());
        }
        let lock = run_git(
            &self.main,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                "index.lock",
            ],
        )?;
        if Path::new(&lock).exists() {
            return Err(format!(
                "an earlier land's move of {default} to {} in the main checkout ended without \
                 moving it, and {lock} is still there: once no git works in the main checkout \
                 (`lsof {lock}` lists one that does), remove it and run the land again, which \
                 restores what the move half-wrote",
                short(&moving.to)
            ));
        }
        let restored = self.restore(&moving.from, &moving.to)?;
        if moving.recorded {
            Record::drop_in(&self.folder, &moving.slug);
        }
        Moving::done(&self.folder);
        let line = format!(
            "land {}: its move of {default} to {} in the main checkout ended without moving it, \
             the land killed meanwhile; {}{}",
            moving.slug,
            short(&moving.to),
            restored
                .said(&default, &moving.from)
                .unwrap_or_else(|| "it wrote nothing there to restore".into()),
            if moving.recorded {
                "; its record dropped: nothing of it landed"
            } else {
                ""
            }
        );
        self.say(&line);
        let _ = self.store.say(&moving.slug, self.actor, &line, None);
        Ok(())
    }

    /// Move the default branch from `base` to `top` in the main checkout by a fast-forward,
    /// shielded from signals: in a process group of its own, which a terminal's Ctrl-C or hang-up
    /// never reaches and to which the land forwards none it takes (it acts on them once the move
    /// ends), so only the land's timeout ends it part way. [`MOVING`] names the move meanwhile and
    /// [`RUNNING`](super::RUNNING) its group, which a later land waits for. Where the branch is
    /// once git ends says what happened, whatever git answered: at `top`, or past it, it moved and
    /// the record stays; still at `base` it did not, and what it half-wrote is restored
    /// ([`Landing::restore`]) and a merge `recorded` for it dropped; anywhere else it moved under
    /// the land, and the record goes unless its merge is there.
    pub(super) fn fast_forward(
        &mut self,
        base: &str,
        top: &str,
        recorded: bool,
    ) -> Result<Move, String> {
        let slug = self.slug;
        let moving = Moving {
            slug: slug.into(),
            from: base.into(),
            to: top.into(),
            recorded,
        };
        moving.write(&self.folder)?;
        let args = ["merge", "--ff-only", "--quiet", top];
        let mut command = self.main_git(&args);
        let why = match sys::spawn_output(&mut command) {
            Err(e) => format!("git merge: {e}"),
            Ok(child) => {
                let group = child.id();
                self.running(group, &format!("git {}", args.join(" ")), true);
                let limit = self.settings.timeout;
                let (done, ended) =
                    watched(group, limit, None, self.now, || sys::wait_output(child));
                self.ran();
                match (done, ended) {
                    (_, Ended::Late) => format!(
                        "git merge --ff-only ran past the land's timeout ([land] timeout_seconds, \
                         {} s) and was ended",
                        limit.as_secs()
                    ),
                    (Err(e), _) => format!("git merge: {e}"),
                    (Ok(done), _) => match done.status.signal() {
                        Some(signal) => {
                            format!("git merge --ff-only was ended by {}", signal_name(signal))
                        }
                        None => String::from_utf8_lossy(&done.stderr)
                            .lines()
                            .map(str::trim)
                            .find(|l| !l.is_empty())
                            .unwrap_or("git merge --ff-only left it where it was")
                            .to_string(),
                    },
                }
            }
        };
        let now = self.base()?;
        let moved = if now == top || ancestor(&self.main, top, &now) {
            Move::Done
        } else if now == base {
            let restored = self.restore(base, top);
            if recorded {
                Record::drop_in(&self.folder, slug);
            }
            if restored.is_err() {
                // Named still, so the next land restores it.
                return Ok(Move::Not { why, restored });
            }
            Move::Not { why, restored }
        } else {
            // Moved under the land: no record of this merge stands unless main holds it.
            let gone = Record::read(&self.folder, slug)
                .is_some_and(|record| !ancestor(&self.main, &record.merge, &now));
            if recorded && gone {
                Record::drop_in(&self.folder, slug);
            }
            Move::Raced(now)
        };
        Moving::done(&self.folder);
        Ok(moved)
    }

    /// Put back, as `from` has them, the paths of the main checkout that a fast-forward from `from`
    /// to `to` wrote before it ended without moving the branch: each path the two commits differ
    /// in whose index entry and file are each as `from` or `to` has it, and not both as `from` has
    /// it already. A path one of them holds otherwise someone else changed, and it is left. A path
    /// `from` lacks is removed (from the index too, should the move have written it), the folders
    /// it leaves empty with it; the others are checked out from `from`.
    fn restore(&self, from: &str, to: &str) -> Result<Restored, String> {
        let changes = changes(&self.main, from, to)?;
        let mut restored = Restored::default();
        if changes.is_empty() {
            return Ok(restored);
        }
        let index = staged(&self.main)?;
        let off_from = self.unlike(from)?;
        let off_to = self.unlike(to)?;
        let mut unstage = Vec::new();
        for change in changes {
            let path = change.path.clone();
            let file_is = |side: &Option<Entry>, unlike: &HashSet<String>| match side {
                Some(_) => !unlike.contains(&path),
                None => absent(&self.main.join(&path)),
            };
            let file_from = file_is(&change.from, &off_from);
            let file_to = file_is(&change.to, &off_to);
            // What the index holds of it: its entry, none (not there), or a conflict (no side).
            let held: Option<Option<Entry>> = match index.get(&path) {
                None => Some(None),
                Some(entry) => entry.clone().map(Some),
            };
            let (index_from, index_to) = (
                held.as_ref() == Some(&change.from),
                held.as_ref() == Some(&change.to),
            );
            if index_from && file_from {
                continue;
            }
            if !(index_from || index_to) || !(file_from || file_to) {
                restored.left.push(path);
            } else if change.from.is_some() {
                restored.put.push(path);
            } else {
                if index_to {
                    unstage.push(path.clone());
                }
                restored.removed.push(path);
            }
        }
        if !unstage.is_empty() {
            on_paths(
                &self.main,
                &["rm", "-q", "--cached", "--ignore-unmatch"],
                &unstage,
            )?;
        }
        for path in &restored.removed {
            let file = self.main.join(path);
            if !absent(&file) {
                fs::remove_file(&file).map_err(|e| at(&file, &e))?;
            }
            prune(&self.main, &file);
        }
        if !restored.put.is_empty() {
            on_paths(&self.main, &["checkout", "-q", from], &restored.put)?;
        }
        Ok(restored)
    }

    /// The paths of the commit `tree` whose file in the main checkout is not as `tree` has it (or
    /// is not there), as git judges a file against the index: through an index of `tree` of its
    /// own, beside the land's folder, which starts from the checkout's index so that only the
    /// files whose entry differs are read again.
    fn unlike(&self, tree: &str) -> Result<HashSet<String>, String> {
        let index = self
            .folder
            .join(format!(".index.{}.{}", short(tree), std::process::id()));
        let real = run_git(
            &self.main,
            &["rev-parse", "--path-format=absolute", "--git-path", "index"],
        )?;
        // Its stat data spares reading the files that did not change; without it, all are read.
        let _ = fs::copy(&real, &index);
        let indexed = |args: &[&str]| {
            let mut command = sys::git();
            command
                .args(args)
                .current_dir(&self.main)
                .env("GIT_INDEX_FILE", &index)
                .stdin(Stdio::null());
            sys::output(&mut command).map_err(|e| format!("git {}: {e}", args[0]))
        };
        let differs = (|| {
            let read = indexed(&["read-tree", "--reset", tree])?;
            if !read.status.success() {
                return Err(format!(
                    "git read-tree {}: {}",
                    short(tree),
                    String::from_utf8_lossy(&read.stderr).trim()
                ));
            }
            // It answers 1 when a file needs reading again: that is what it is for.
            indexed(&["update-index", "-q", "--refresh"])?;
            let listed = indexed(&["diff-files", "--name-only", "-z"])?;
            if !listed.status.success() {
                return Err(format!(
                    "git diff-files: {}",
                    String::from_utf8_lossy(&listed.stderr).trim()
                ));
            }
            Ok(listed
                .stdout
                .split(|b| *b == 0)
                .filter(|s| !s.is_empty())
                .map(|s| String::from_utf8_lossy(s).into_owned())
                .collect())
        })();
        let _ = fs::remove_file(&index);
        differs
    }
}
