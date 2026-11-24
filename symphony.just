# symphony-stamp: b6cc844b4a24183b
# The Symphony's commands, which `symphony init` installs and keeps current: the task board
# (`just tasks`, and `just tasks-people` for a change to its people), messages between agents
# (`just agent-send`), the server (`just symphony`), the document CLI (`just symphony-cli`) and its
# review commands (`just symphony-status`, `just symphony-reply`, `just symphony-ask`), and the
# default landing (`just land`). The project's justfile imports this file. A recipe of the same
# name there is the project's own and replaces the one here, by the setting below, wherever the
# import stands: the review commands then run the project's `symphony-cli`, and `land` and the
# board run its `tasks`. Where the project's justfile makes the setting itself, init writes this
# file without it, as `just` refuses it twice. A copy without the stamp line is the project's own,
# and init never writes it. Each command says its own errors, and a failing one ends with them,
# with no recipe-failed line after them (`[no-exit-message]`). The comment line right above each
# recipe is the one line `just --list` shows for it, so it says what the recipe does; anything
# longer goes in the lines before it.

set allow-duplicate-recipes

# Runs the project's build of a binary: this checkout's `.target/bin/NAME` link, else the main
# checkout's (from a linked worktree), else the release the machine runs (`current/bin/NAME` in the
# machine folder: SYMPHONY_MACHINE_DIR, else the platform's data folder for the Symphony), else NAME
# on PATH. A project with no build of its own so runs the release.
_symphony_run := 'symphony_run() { n=$1; shift; m=$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null); case $m in */.git) m=${m%/.git};; *) m=;; esac; for b in ".target/bin/$n" ${m:+"$m/.target/bin/$n"}; do if [ -x "$b" ]; then exec "$b" "$@"; fi; done; d=${SYMPHONY_MACHINE_DIR:-}; if [ -z "$d" ] && [ -n "${HOME:-}" ]; then case $(uname -s) in Darwin) d="$HOME/Library/Application Support/Symphony";; *) d="$HOME/.local/share/symphony"; case ${XDG_DATA_HOME:-} in /*) d="$XDG_DATA_HOME/symphony";; esac;; esac; fi; case $d in /*) b="$d/current/bin/$n"; if [ -x "$b" ]; then exec "$b" "$@"; fi;; esac; if b=$(command -v "$n"); then exec "$b" "$@"; fi; echo "no $n binary: link the project build of $n at .target/bin/$n, install a release on the machine, or put $n on PATH" >&2; exit 127; }; symphony_run'

# Refuses the board's recorder (`tasks run`, its command first after any `--database NAME`, as
# `tasks` reads it), which runs whatever command it is handed: the allow rules let an agent run
# `just tasks` unasked. A build is recorded by the board's binary itself, which no rule allows.
_symphony_board := 'symphony_board() { while [ "$#" -ge 2 ] && [ "$1" = --database ]; do shift 2; done; if [ "${1-}" = run ]; then echo "just tasks refuses run: the recorder runs any command it is handed, and an agent runs just tasks unasked. Record a build by the board binary itself (.target/bin/tasks run); just tasks help run says how" >&2; exit 2; fi; }; symphony_board "$@"; '

# Marks a command as run by a recipe the allow rules pass unasked: under it a place the command
# writes to (`tasks export --out`, `reply PATH`, `ask --db`/`--root`, `status --tidy --root`) must be
# in the checkout it runs in, or from a linked worktree in the main checkout's artifacts folder, and
# in none of their git folders, its links and `..` resolved; the binary run by itself, which asks,
# writes anywhere. A project's own `tasks` or `symphony-*` recipe replaces the one here and the
# allow rule still passes it unasked, but nothing confines it unless it sets the marker too, by
# starting with `{{ _symphony_unasked }}` as these do.
_symphony_unasked := 'export SYMPHONY_UNASKED=1; '

# Under the marker `tasks` refuses a change to a board's people or to a profile too, which
# `just tasks-people` makes.
# The task board: `just tasks help`. Every command of the board's but its recorder.
[positional-arguments]
[no-exit-message]
tasks *ARGS:
    @{{ _symphony_board }}{{ _symphony_unasked }}{{ _symphony_run }} tasks "$@"

# No allow rule passes this recipe, so an agent's run of it asks first, as `just symphony` does; it
# sets no marker. `just tasks people list` and `log` read them unasked.
# A change to a board's people or a profile, which asks first: `just tasks help people`.
[positional-arguments]
[no-exit-message]
tasks-people *ARGS:
    @{{ _symphony_run }} tasks people "$@"

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
    @{{ _symphony_unasked }}{{ quote(just_executable()) }} --justfile {{ quote(justfile()) }} symphony-cli status "$@"

# A reply with the slug `L<line>` is the answer that resolves that line's comment, and `--as` names
# the session that answers the owner's ask.
# An agent's line under a card: `just symphony-reply PATH SLUG TEXT [--question] [--as HANDLE]`.
[positional-arguments]
[no-exit-message]
symphony-reply *ARGS:
    @{{ _symphony_unasked }}{{ quote(just_executable()) }} --justfile {{ quote(justfile()) }} symphony-cli reply "$@"

# The owner's asks: `list --open`, `show ID`, `answer ID TEXT --as HANDLE`.
[positional-arguments]
[no-exit-message]
symphony-ask *ARGS:
    @{{ _symphony_unasked }}{{ quote(just_executable()) }} --justfile {{ quote(justfile()) }} symphony-cli ask "$@"

# The default landing merges the branch named as the task on the default branch, checks it, pushes
# it and closes the task (`just tasks land --help`). It sets SYMPHONY_LAND, without which
# `tasks land` refuses: `just tasks land` by itself never lands. `symphony init` allows it unasked:
# this land lands only what its checks pass, every stamp the task needs standing on the tip (a
# fresh reviewer's approve among them where it needs review); a `land` of the project's own
# replaces it, gets the same allow rule, and its landing policy decides what it lands.
# Land a task's approved branch: `just land SLUG --as HANDLE`.
[positional-arguments]
[no-exit-message]
land *ARGS:
    @SYMPHONY_LAND=1 {{ quote(just_executable()) }} --justfile {{ quote(justfile()) }} tasks land "$@"
