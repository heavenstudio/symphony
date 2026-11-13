//! Where an agent the board starts for a command an agent ran unasked may run.
//!
//! `symphony init` allows `just tasks` with no prompt, and the recipe sets [`confine::MARKER`].
//! Under it `launch` and `restore` start an agent, in auto mode, only in a folder the
//! [`confine::places`] of the command's folder admit (its checkout, from a linked worktree the main
//! checkout's artifacts folder, never a git folder) or under the project's worktree base
//! ([`super::Machine::wt_base`], where a launch makes them), each path's links and `..` resolved
//! first: `launch --worktree DIR`, the worktree a launch makes, and the worktree a record names that
//! a restore makes again and starts in, an older record's too. A launch takes no `--prompt` there:
//! an agent's first message is its person's own word, so it is `/work SLUG HANDLE` alone, and what
//! else the launcher has to say goes on the task's thread, which `/work` reads, as board text. And
//! `tasks api`'s `record_launch` writes no record a restore would start elsewhere, nor one naming a
//! conversation: a restore resumes only one the board recorded itself, launching or finding the
//! agent. The binary run by itself, which no rule allows, starts an agent anywhere.

use crate::NewLaunch;
use crate::cli::Env;
use crate::confine::{self, Places};
use std::path::{Path, PathBuf};

/// The folders an agent started for a command run under [`confine::MARKER`] may run in.
#[derive(Debug, PartialEq)]
pub struct Bounds {
    places: Places,
    /// The project's worktree base, resolved: a folder below it is admitted, not the base itself.
    base: PathBuf,
}

impl Bounds {
    /// Those of a command run in `cwd`, whose worktrees go under `wt_base`, which git makes from the
    /// main checkout at `root`.
    pub fn new(cwd: &Path, root: &Path, wt_base: &Path) -> Bounds {
        Bounds {
            places: confine::places(cwd),
            base: confine::resolve(root, wt_base).unwrap_or_else(|_| root.join(wt_base)),
        }
    }

    /// Those of a command run in `env` under the marker, its main checkout at `root` and its
    /// worktree base `wt_base`; none for one run without it, which starts an agent anywhere.
    pub fn of(env: &Env, root: &Path, wt_base: &Path) -> Option<Bounds> {
        env.var(confine::MARKER)?;
        Some(Bounds::new(&env.cwd, root, wt_base))
    }

    /// [`Bounds::of`] a command run in `env`, its main checkout and worktree base found as a
    /// launch finds them ([`super::Machine::of`]).
    pub fn unasked(env: &Env) -> Option<Bounds> {
        env.var(confine::MARKER)?;
        let root = super::root_of(env);
        Bounds::of(env, &root, &super::worktree_base(&root, env))
    }

    /// `folder`, resolved, when an agent may start there; `what` names it in a refusal.
    ///
    /// # Errors
    ///
    /// One line naming the folder, where it leads and where an agent may start.
    pub fn admit(&self, what: &str, folder: &Path) -> Result<PathBuf, String> {
        let place = if folder.as_os_str().is_empty() {
            format!("{what} names no folder, so it is")
        } else if !folder.is_absolute() {
            format!("{what} {} is no absolute path, so it is", folder.display())
        } else {
            let resolved =
                confine::resolve(Path::new("/"), folder).map_err(|why| format!("{what} {why}"))?;
            let below_base = resolved != self.base && self.places.admit_in(&self.base, &resolved);
            if self.places.admit(&resolved) || below_base {
                return Ok(resolved);
            }
            if resolved == folder {
                format!("{what} {} is", folder.display())
            } else {
                format!(
                    "{what} {} leads to {}, which is",
                    folder.display(),
                    resolved.display()
                )
            }
        };
        Err(format!(
            "{place} not a folder a recipe an agent runs unasked starts an agent in: {}, or a \
             folder under the project's worktree base {}; the binary run by itself starts one \
             anywhere",
            self.places.said(),
            self.base.display()
        ))
    }

    /// Whether a launch may give its agent `prompt` after `/work SLUG HANDLE`: only none.
    ///
    /// # Errors
    ///
    /// One line saying where the text goes instead.
    pub fn prompt(&self, slug: &str, prompt: &str) -> Result<(), String> {
        if prompt.trim().is_empty() {
            return Ok(());
        }
        Err(format!(
            "no --prompt through a recipe an agent runs unasked: an agent's first message is its \
             person's own word, so it is /work {slug} HANDLE alone; say the rest on the task's \
             thread (just tasks say {slug} TEXT), which /work reads, or run the binary by itself"
        ))
    }

    /// Whether `tasks api` may write `launch`: a record naming no conversation, of a worktree an
    /// agent may start in.
    ///
    /// # Errors
    ///
    /// One line naming what the record may not hold.
    pub fn record(&self, launch: &NewLaunch) -> Result<(), String> {
        if !launch.claude_session.is_empty() {
            return Err(format!(
                "a record written through a recipe an agent runs unasked names no conversation, \
                 not {}: a restore resumes only one the board recorded itself, launching or \
                 finding the agent; the binary run by itself records any",
                launch.claude_session
            ));
        }
        self.admit("the record's worktree", Path::new(&launch.worktree))
            .map(|_| ())
    }
}
