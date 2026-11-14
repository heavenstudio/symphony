//! `launch`'s usage and help, the longest of any command, kept beside [`super::COMMANDS`]. Its help
//! ends with what a bare launch starts where it runs ([`shown`]).

use crate::cli::Env;
use crate::launch::{Defaults, root_of};

pub const USAGE: &str = r#"usage: just tasks launch [-h] [--as ACTOR] [--model MODEL]
                         [--kind {codex,claude}] [--role {executor,reviewer}]
                         [--branch BRANCH | --detached] [--worktree WORKTREE]
                         [--tab TAB] [--prompt PROMPT]
                         slug
"#;

pub const HELP: &str = r#"usage: just tasks launch [-h] [--as ACTOR] [--model MODEL]
                         [--kind {codex,claude}] [--role {executor,reviewer}]
                         [--branch BRANCH | --detached] [--worktree WORKTREE]
                         [--tab TAB] [--prompt PROMPT]
                         slug

Start an agent on a task in an hc terminal of its own, its first message
/work SLUG HANDLE (a reviewer's /review-task SLUG HANDLE). --kind says which
agent runs, --role what it does there.
Refused while this machine runs as many agents as the project's [limits]
machine_agents allows (`just tasks limits` prints it).

positional arguments:
  slug

options:
  -h, --help            show this help message and exit
  --as ACTOR
  --model MODEL         default the model the project's [agents.models] names
                        for the role, else its [agents] launch model, for its
                        agent; else gpt-6.1-sol for codex, sonnet for claude
  --kind {codex,claude}
                        the agent it runs (default: the project's [agents]
                        launch, else codex); worker and reviewer, older
                        spellings, run claude
  --role {executor,reviewer}
                        the role it takes on the task (default: executor)
  --branch BRANCH       the branch its worktree is on (default: the slug)
  --detached            a worktree on no branch, for read-only work (a
                        reviewer's and an instruction's are)
  --worktree WORKTREE   use this worktree instead of making one under
                        <repo>-wt/ (or SYMPHONY_WT_DIR); through just tasks,
                        one in the checkout or under that folder
  --tab TAB             the Herdr app tab (stream) for its terminal; default
                        code for an executor, review for a reviewer
  --prompt PROMPT       text sent after /work SLUG HANDLE (or /review-task);
                        refused through just tasks: say it on the task's
                        thread
"#;

/// [`HELP`] as `launch --help` prints it in `env`: then what a bare launch starts there, from the
/// main checkout's `symphony.toml`.
pub fn shown(env: &Env) -> String {
    format!("{HELP}\n{}", Defaults::said_at(&root_of(env)))
}
