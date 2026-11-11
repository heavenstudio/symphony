//! `launch`'s usage and help, the longest of any command, kept beside [`super::COMMANDS`]. Its help
//! ends with what a bare launch starts where it runs ([`shown`]).

use crate::cli::Env;
use crate::launch::{Defaults, root_of};

pub const USAGE: &str = r#"usage: just tasks launch [-h] [--as ACTOR] [--model MODEL]
                         [--kind {codex,claude,worker,reviewer}]
                         [--role {executor,reviewer}] [--branch BRANCH |
                         --detached] [--worktree WORKTREE] [--tab TAB]
                         [--prompt PROMPT]
                         slug
"#;

pub const HELP: &str = r#"usage: just tasks launch [-h] [--as ACTOR] [--model MODEL]
                         [--kind {codex,claude,worker,reviewer}]
                         [--role {executor,reviewer}] [--branch BRANCH |
                         --detached] [--worktree WORKTREE] [--tab TAB]
                         [--prompt PROMPT]
                         slug

positional arguments:
  slug

options:
  -h, --help            show this help message and exit
  --as ACTOR
  --model MODEL         default the model of the project's [agents] launch for
                        its agent, else gpt-6.1-sol for codex, sonnet for
                        claude
  --kind {codex,claude,worker,reviewer}
                        agent kind, default the project's [agents] launch
                        default, else codex; worker/reviewer retain Claude
                        compatibility
  --role {executor,reviewer}
  --branch BRANCH
  --detached
  --worktree WORKTREE   use this worktree instead of making one under
                        <repo>-wt/
  --tab TAB             the Herdr app tab (stream) for its terminal; default
                        code for an executor, review for a reviewer
  --prompt PROMPT       text sent after /work SLUG HANDLE
"#;

/// [`HELP`] as `launch --help` prints it in `env`: then what a bare launch starts there, from the
/// main checkout's `symphony.toml`.
pub fn shown(env: &Env) -> String {
    format!("{HELP}\n{}", Defaults::said_at(&root_of(env)))
}
