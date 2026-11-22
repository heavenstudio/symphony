//! Where an agent the board starts for a command an agent ran unasked may run.
//!
//! `symphony init` allows `just tasks` with no prompt, and the recipe sets [`confine::MARKER`].
//! Under it `launch` and `restore` start an agent, in auto mode, only in a folder the
//! [`confine::places`] of the command's folder admit (its checkout, from a linked worktree the main
//! checkout's artifacts folder, never a git folder) or under the project's worktree base
//! ([`super::Machine::wt_base`], where a launch makes them: `SYMPHONY_WT_DIR`, or the folder beside
//! a main checkout that lists the command's checkout as its own, [`Bounds::new`]), each path's
//! links and `..` resolved first: `launch --worktree DIR`, the worktree a launch makes, and the
//! worktree a record names that a restore makes again and starts in, an older record's too. A
//! launch takes no `--prompt` there: an agent's first message is its person's own word, so it is
//! `/work SLUG HANDLE` alone (a reviewer's `/review-task SLUG HANDLE`, each with `$` for Codex),
//! and what else the launcher has to say goes on the task's thread, which the skill reads, as board
//! text.
//!
//! The conversation a restore resumes is the one the record names. Under the mark `tasks api`'s
//! `record_launch` writes no record a restore would start elsewhere, nor one naming a conversation;
//! no agent id is bound to a handle (`tasks api`'s `bind_agent` is refused, [`may_bind`], and
//! `tasks beat` binds nothing: a heartbeat binding is the hook's, which runs the binary itself on
//! the session's own tool events); and `callback` registers a Codex thread only as its caller's own
//! (`CODEX_THREAD_ID`, [`own_thread`]), which the board writes into the handle's open Codex record
//! (`codex:`). So a conversation a record names was recorded by the board's own launcher (the id it
//! chose at a launch, or one it read from the agent's terminal, the hook's binding or a callback),
//! or is the Codex thread an agent registered from its own process. Three limits stay: `--as`
//! names any handle, so an agent may give another handle its own conversation (the hook binds the
//! session to the handle its board command names, and a callback registers its thread for it),
//! whose restore then resumes that agent's conversation; a record an older build wrote is not told
//! apart; and a folder is checked before the agent starts in it, so a folder on its path replaced
//! by a link in between still leads out. The binary run by itself, which no rule allows, starts an
//! agent anywhere, records any conversation and binds any agent.

use crate::NewLaunch;
use crate::cli::Env;
use crate::confine::{self, Places};
use std::path::{Path, PathBuf};

/// The folders an agent started for a command run under [`confine::MARKER`] may run in.
#[derive(Debug, PartialEq)]
pub struct Bounds {
    places: Places,
    /// The project's worktree base, resolved: a folder below it is admitted, not the base itself.
    /// None where the base would be beside a main checkout the command's own checkout does not
    /// confirm ([`Places::main`]).
    base: Option<PathBuf>,
}

impl Bounds {
    /// Those of a command run in `cwd`, whose worktrees go under `wt_base`, the folder beside the
    /// main checkout at `root` ([`super::worktree_base`]), which counts only when `root` is the main
    /// checkout `cwd`'s checkout confirms ([`Places::main`], what [`confine`] admits the main
    /// artifacts folder by). `root` is read from the worktree's `.git` file, untracked, and one
    /// rewritten to name another repository moves `root`, and the base beside it, there. A
    /// submodule, a `--separate-git-dir` checkout or a bare repository's worktree has no main
    /// checkout (its `root` is the checkout itself, [`super::root_of`], which no repository lists
    /// back), so only `SYMPHONY_WT_DIR` gives it a base ([`Bounds::of`]).
    pub fn new(cwd: &Path, root: &Path, wt_base: &Path) -> Bounds {
        let places = confine::places(cwd);
        let confirmed = places
            .main
            .as_ref()
            .is_some_and(|main| root.canonicalize().is_ok_and(|root| root == *main));
        Bounds {
            base: confirmed.then(|| base(root, wt_base)),
            places,
        }
    }

    /// Those of a command run in `env` under the marker, its main checkout at `root` and its
    /// worktree base `wt_base`; none for one run without it, which starts an agent anywhere. A base
    /// `SYMPHONY_WT_DIR` names is the setting's, taken as it is, from `root` when it is relative.
    pub fn of(env: &Env, root: &Path, wt_base: &Path) -> Option<Bounds> {
        env.var(confine::MARKER)?;
        if env.read(symphony_env::WT_DIR).is_some() {
            return Some(Bounds {
                places: confine::places(&env.cwd),
                base: Some(base(root, wt_base)),
            });
        }
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
            let below_base = self
                .base
                .as_ref()
                .is_some_and(|base| resolved != *base && self.places.admit_in(base, &resolved));
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
        let base = match &self.base {
            Some(base) => format!(
                ", or a folder under the project's worktree base {}",
                base.display()
            ),
            None => {
                ", and no worktree base, which is taken only beside a main checkout that lists \
                     this checkout as its own, or where SYMPHONY_WT_DIR names one (a submodule, a \
                     --separate-git-dir checkout or a bare repository's worktree has no main \
                     checkout)"
                    .into()
            }
        };
        Err(format!(
            "{place} not a folder a recipe an agent runs unasked starts an agent in: {}{base}; the \
             binary run by itself starts one anywhere",
            self.places.said()
        ))
    }

    /// Whether a launch may give its agent of `role` `prompt` after `/work SLUG HANDLE` (a
    /// reviewer's `/review-task SLUG HANDLE`): only none.
    ///
    /// # Errors
    ///
    /// One line saying where the text goes instead.
    pub fn prompt(&self, slug: &str, role: &str, prompt: &str) -> Result<(), String> {
        if prompt.trim().is_empty() {
            return Ok(());
        }
        let skill = super::skill(role);
        Err(format!(
            "no --prompt through a recipe an agent runs unasked: an agent's first message is its \
             person's own word, so it is /{skill} {slug} HANDLE alone; say the rest on the task's \
             thread (just tasks say {slug} TEXT), which /{skill} reads, or run the binary by itself"
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
                 not {}: the board's launcher records one, and a Codex thread comes from its \
                 agent's own callback; the binary run by itself records any",
                launch.claude_session
            ));
        }
        self.admit("the record's worktree", Path::new(&launch.worktree))
            .map(|_| ())
    }
}

/// The worktree base `wt_base`, from `root` when it is relative, resolved ([`confine::resolve`]);
/// as written where a link on it leads nowhere.
fn base(root: &Path, wt_base: &Path) -> PathBuf {
    confine::resolve(root, wt_base).unwrap_or_else(|_| root.join(wt_base))
}

/// Whether `callback`, run in `env`, may register `address`: under the mark a Codex thread only as
/// the caller's own (`CODEX_THREAD_ID`, which `callback` detects with no address), since the board
/// writes it into the handle's open Codex launch record as the conversation a restore resumes. Any
/// other address, and any address without the mark, passes.
///
/// # Errors
///
/// One line naming the thread and whose a callback registers.
pub fn own_thread(env: &Env, address: &str) -> Result<(), String> {
    let Some(thread) = address.strip_prefix(crate::launch::CODEX) else {
        return Ok(());
    };
    if env.var(confine::MARKER).is_none() || env.var("CODEX_THREAD_ID") == Some(thread) {
        return Ok(());
    }
    Err(format!(
        "{address} is not this session's own Codex thread (CODEX_THREAD_ID, which a callback with \
         no address detects): through a recipe an agent runs unasked a callback registers only \
         its caller's, as the board takes it into the handle's launch record for a restore to \
         resume; the binary run by itself registers any"
    ))
}

/// Whether `tasks api`'s `bind_agent`, run in `env`, may bind an agent id to a handle: not under the
/// mark, since the id a handle is bound to is the conversation the agents listing gives it where its
/// record and terminal name none, which staffing then records for a restore to resume. Nothing of
/// the board's calls it: the heartbeat binds in `tasks beat`, which the hook runs as the binary
/// itself, and under the mark binds nothing either.
///
/// # Errors
///
/// One line saying where a binding comes from instead.
pub fn may_bind(env: &Env) -> Result<(), String> {
    if env.var(confine::MARKER).is_none() {
        return Ok(());
    }
    Err(
        "no bind_agent through a recipe an agent runs unasked: the agent id a handle is bound to \
         is a conversation staffing may record for it, for a restore to resume; a session's own \
         heartbeat binds it (the hook runs tasks beat as the binary itself), and the binary run \
         by itself binds any"
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(vars: &[(&str, &str)]) -> Env {
        Env {
            vars: vars
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
            cwd: PathBuf::from("/"),
        }
    }

    #[test]
    fn under_the_mark_a_callback_registers_only_its_callers_own_codex_thread() {
        let mark = (confine::MARKER, "1");
        let marked = env(&[mark, ("CODEX_THREAD_ID", "0abc-12")]);
        assert_eq!(own_thread(&marked, "codex:0abc-12"), Ok(()));
        assert!(own_thread(&marked, "codex:dead-beef").is_err());
        // No thread of its own, or an empty one, is no thread to register.
        assert!(own_thread(&env(&[mark]), "codex:0abc-12").is_err());
        assert!(own_thread(&env(&[mark, ("CODEX_THREAD_ID", "")]), "codex:").is_err());
        // A socket reaches no launch record.
        assert_eq!(own_thread(&marked, "uds:/tmp/inbox.sock"), Ok(()));
        // Without the mark, or with an empty one, any.
        for unmarked in [env(&[]), env(&[(confine::MARKER, "")])] {
            assert_eq!(own_thread(&unmarked, "codex:dead-beef"), Ok(()));
        }
    }

    #[test]
    fn under_the_mark_the_api_binds_no_agent() {
        assert!(
            may_bind(&env(&[(confine::MARKER, "1")]))
                .unwrap_err()
                .starts_with("no bind_agent through a recipe an agent runs unasked: ")
        );
        for unmarked in [env(&[]), env(&[(confine::MARKER, "")])] {
            assert_eq!(may_bind(&unmarked), Ok(()));
        }
    }
}
