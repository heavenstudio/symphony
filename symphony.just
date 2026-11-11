# symphony-stamp: e9fc5aab800a0451
# The Symphony's commands, which `symphony init` installs and keeps current: the task board
# (`just tasks`), messages between agents (`just agent-send`), the server (`just symphony`), the
# document CLI (`just symphony-cli`) and its review commands (`just symphony-status`,
# `just symphony-reply`, `just symphony-ask`). The project's justfile imports this file. Under
# `set allow-duplicate-recipes` a recipe of the same name there is the project's own and wins, and
# the review commands then run the project's `symphony-cli`. A copy without the stamp line is the
# project's own, and init never writes it.

# Runs the project's build of a binary: this checkout's `.target/bin/NAME` link, else the main
# checkout's (from a linked worktree), else NAME on PATH.
_symphony_run := 'symphony_run() { n=$1; shift; m=$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null); case $m in */.git) m=${m%/.git};; *) m=;; esac; for b in ".target/bin/$n" ${m:+"$m/.target/bin/$n"}; do if [ -x "$b" ]; then exec "$b" "$@"; fi; done; if b=$(command -v "$n"); then exec "$b" "$@"; fi; echo "no $n binary: link the project build of $n at .target/bin/$n, or put $n on PATH" >&2; exit 127; }; symphony_run'

# The task board: `just tasks --help`.
[positional-arguments]
tasks *ARGS:
    @{{ _symphony_run }} tasks "$@"

# A message to an agent through its registered channel: `just agent-send HANDLE TEXT --as SENDER`.
[positional-arguments]
agent-send *ARGS:
    @{{ _symphony_run }} agent-send "$@"

# The machine's Symphony server, serving this project; `--port 0` runs one of its own.
[positional-arguments]
symphony *ARGS:
    @{{ _symphony_run }} symphony-server "$@"

# The document CLI: `just symphony-cli --help`.
[positional-arguments]
symphony-cli *ARGS:
    @{{ _symphony_run }} symphony "$@"

# The review board in the terminal: `--open`, `--json`, `--tidy`, `--lint`.
[positional-arguments]
symphony-status *ARGS:
    @{{ quote(just_executable()) }} --justfile {{ quote(justfile()) }} symphony-cli status "$@"

# An agent's line under a card: `just symphony-reply PATH SLUG TEXT [--question]`.
[positional-arguments]
symphony-reply *ARGS:
    @{{ quote(just_executable()) }} --justfile {{ quote(justfile()) }} symphony-cli reply "$@"

# The owner's asks: `list --open`, `show ID`, `answer ID TEXT --by HANDLE`.
[positional-arguments]
symphony-ask *ARGS:
    @{{ quote(just_executable()) }} --justfile {{ quote(justfile()) }} symphony-cli ask "$@"
