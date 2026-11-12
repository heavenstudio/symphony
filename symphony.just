# symphony-stamp: ffb92a221fc3e5f2
# The Symphony's commands, which `symphony init` installs and keeps current: the task board
# (`just tasks`), messages between agents (`just agent-send`), the server (`just symphony`), the
# document CLI (`just symphony-cli`) and its review commands (`just symphony-status`,
# `just symphony-reply`, `just symphony-ask`), and the default landing (`just land`). The project's
# justfile imports this file. A recipe of the same name there is the project's own and replaces
# the one here, by the setting below, wherever the import stands: the review commands then run the
# project's `symphony-cli`, and `land` and the board run its `tasks`. Where the project's justfile
# makes the setting itself, init writes this file without it, as `just` refuses it twice. A copy
# without the stamp line is the project's own, and init never writes it. Each command says its own
# errors, and a failing one ends with them, with no recipe-failed line after them
# (`[no-exit-message]`).

set allow-duplicate-recipes

# Runs the project's build of a binary: this checkout's `.target/bin/NAME` link, else the main
# checkout's (from a linked worktree), else NAME on PATH.
_symphony_run := 'symphony_run() { n=$1; shift; m=$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null); case $m in */.git) m=${m%/.git};; *) m=;; esac; for b in ".target/bin/$n" ${m:+"$m/.target/bin/$n"}; do if [ -x "$b" ]; then exec "$b" "$@"; fi; done; if b=$(command -v "$n"); then exec "$b" "$@"; fi; echo "no $n binary: link the project build of $n at .target/bin/$n, or put $n on PATH" >&2; exit 127; }; symphony_run'

# The task board: `just tasks help`.
[positional-arguments]
[no-exit-message]
tasks *ARGS:
    @{{ _symphony_run }} tasks "$@"

# A message to an agent through its registered channel: `just agent-send HANDLE TEXT --as SENDER`.
[positional-arguments]
[no-exit-message]
agent-send *ARGS:
    @{{ _symphony_run }} agent-send "$@"

# The machine's Symphony server, serving this project; `--port 0` runs one of its own.
[positional-arguments]
[no-exit-message]
symphony *ARGS:
    @{{ _symphony_run }} symphony-server "$@"

# The document CLI: `just symphony-cli help`.
[positional-arguments]
[no-exit-message]
symphony-cli *ARGS:
    @{{ _symphony_run }} symphony "$@"

# The review board in the terminal: `--open`, `--json`, `--tidy`, `--lint`.
[positional-arguments]
[no-exit-message]
symphony-status *ARGS:
    @{{ quote(just_executable()) }} --justfile {{ quote(justfile()) }} symphony-cli status "$@"

# An agent's line under a card, or with the slug `L<line>` the answer that resolves that line's
# comment: `just symphony-reply PATH SLUG TEXT [--question] [--as HANDLE]`, `--as` naming the
# session that answers the owner's ask.
[positional-arguments]
[no-exit-message]
symphony-reply *ARGS:
    @{{ quote(just_executable()) }} --justfile {{ quote(justfile()) }} symphony-cli reply "$@"

# The owner's asks: `list --open`, `show ID`, `answer ID TEXT --as HANDLE`.
[positional-arguments]
[no-exit-message]
symphony-ask *ARGS:
    @{{ quote(just_executable()) }} --justfile {{ quote(justfile()) }} symphony-cli ask "$@"

# `just land SLUG --as HANDLE` (`just tasks land --help`); a `land` of the project's own replaces it.
# Land a task's approved branch on the default branch, check it, push it and close the task. It
# sets SYMPHONY_LAND, without which `tasks land` refuses: `just tasks land` by itself never lands.
[positional-arguments]
[no-exit-message]
land *ARGS:
    @SYMPHONY_LAND=1 {{ quote(just_executable()) }} --justfile {{ quote(justfile()) }} tasks land "$@"
