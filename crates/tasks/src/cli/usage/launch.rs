//! `launch`'s usage and help, the longest of any command, kept beside [`super::COMMANDS`].

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
  --model MODEL        default gpt-6.1-sol (sonnet for --kind claude)
  --kind {codex,claude,worker,reviewer}
                        agent kind, default codex; worker/reviewer retain Claude compatibility
  --role {executor,reviewer}
  --branch BRANCH
  --detached
  --worktree WORKTREE   use this worktree instead of making one under
                        <repo>-wt/
  --tab TAB             the Herdr app tab (stream) for its terminal; default
                        code for an executor, review for a reviewer
  --prompt PROMPT       text sent after /work SLUG HANDLE
"#;
