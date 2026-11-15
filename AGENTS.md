# Symphony

The app's workspace owns its server, its two web clients and its Mac app under `apps/` (the React
one, `apps/symphony-web`, taking the pages over a kind at a time from the Dioxus one; the Mac app,
`apps/symphony-desktop`, a thin shell over each server's own pages, `publish = false`, whose window
crates no other package links), and the review library, wire types, task board,
working-directory scanner, headless Chrome driver and the tests' CPU meters and waits
(`crates/test-time`, RULES.md `tests/no-test-fails-on-load`) under `crates/`.
The app's formats and operating contract live in [docs/](docs/README.md).
A consuming project owns its landing policy and shared-service safeguards.

This guide holds what every session here needs; how a part works is the document or module doc
each rule names.

## Rust

`Cargo.toml` and `Cargo.lock` belong to this workspace, and its members inherit its package
settings and dependency versions. The containing project's thin host and board-only packages
consume these libraries through an immutable Git dependency pin. The plugin kit, the helpers and
the product (the library as `symphony-app`, its library still `symphony`, the board as
`symphony-board`, the server, the Chrome driver as `symphony-chrome` and the wire types,
`symphony-api`) are made to be published on crates.io as well, each by its version; the client's
sources, the Mac app and the tests' time keep `publish = false`
([RELEASES](docs/RELEASES.md#packages-on-cratesio)).

### Where each part is said

- **A project's settings** are its `symphony.toml`, every table and key in
  [PROJECT_FILE](docs/PROJECT_FILE.md), the generic defaults in `crates/symphony/src/project.rs`.
- **A project's own code is a plugin**: the traits and block vocabulary in
  `crates/symphony-plugin`, linked by its host or answering from a process of its own
  ([PLUGIN_PROTOCOL](docs/PLUGIN_PROTOCOL.md)). A plugin renders to generic blocks, never HTML,
  and the client draws each block with one component; only the plugins a project selects answer
  for it.
- **A project's documents** are served from its `[[entrypoints]]`
  (`crates/symphony/src/entrypoint.rs`; none written is `/docs/` over `docs/`), each named by its
  route in every link and on the wire. Its reads, saves and commits name the file `Project::file`
  maps it to, and every walk goes through `Project::walk`, so what an entrypoint leaves out is
  never listed or read. The Symphony's own routes (`symphony_api::SYMPHONY_ROUTES`, which
  `assets/shell.js` repeats) win over every entrypoint.
- **The one server** serves every project the machine folder's registry holds, each under
  `/<id>/`, and the host rule decides whether a start's build serves, waits for or replaces the
  running one ([the server guide](apps/symphony-server/README.md), `crates/symphony/src/machine/`).
  A start in a linked worktree never starts, registers with or replaces the one server
  (`apps/symphony-server/src/starter.rs`): `just symphony --port 0` there runs a server of its
  own, and `--shared` is the owner's word. A project's state stays in its checkout: paths under its
  artifacts folder go through `Project::artifact`, never an `artifacts/` literal, and the client
  keys what it keeps in the browser by the project's id. Tests use a temporary machine folder
  (`SYMPHONY_MACHINE_DIR`).
- **The board** is [TASKS](docs/TASKS.md); recorded builds and tests are
  [MACHINE_WORK](docs/MACHINE_WORK.md) and [RUNS](docs/RUNS.md); what a task proves is
  [CHECKS](docs/CHECKS.md); releases are [RELEASES](docs/RELEASES.md).

Read [the client guide](apps/symphony-ui/AGENTS.md), [the React client's
guide](apps/symphony-web/AGENTS.md), [the Mac app's guide](apps/symphony-desktop/AGENTS.md) and
[the board guide](crates/tasks/AGENTS.md) before editing those packages. A consuming project
applies its own Rust, review and resource policy.
Never run a shared-service restart as a test: use a private port and scratch board.

### Building and testing

Use `just lint-rs`, `just test-rs`, `just symphony-ui-dist` and `just symphony-ui-dev`
from this folder; `just lint-rs` runs `just client-lint` as well, the React client's checks on
its pinned Node ([its guide](apps/symphony-web/AGENTS.md)). `tools/cargo.sh` runs ordinary Cargo
in this workspace, preserving its arguments, streams, status and inherited limits. A build or test
goes through `tools/record.sh`, as do the `lint-rs`, `test-rs`, `client-check`, `client-update`
and `verify` recipes, each as one operation the board's recorder records for the Build and Test
columns, or runs as it is where none answers
([MACHINE_WORK](docs/MACHINE_WORK.md#recording-builds-and-tests), its logs in
[RUNS](docs/RUNS.md#logs)). It uses this folder's `.target/` unless `CARGO_TARGET_DIR` names
another: a linked worktree's starts as a copy-on-write clone of the main checkout's compiled
dependencies (`tools/seed-target.sh`), never one folder for two worktrees, where Cargo names their
packages alike and each runs the other's build. The toolchain pin is `rust-toolchain.toml`, the
wasm target in it. The workspace supplies no project gate planner, test selection, certificate or
shared slot pool: the containing project keeps its own for its own consumers.

`tools/rs-bin.sh NAME` runs a declared workspace binary, asking Cargo to build it first only when
something Cargo would read changed since the last build the launcher asked for, through a link
renewed only when Cargo relinked; its comments say what it watches and what it cannot see.
`--build NAME` builds and prints that path, `--where NAME` names Cargo's own copy without
building, and `--link NAME` builds and points `.target/bin/NAME` at it. A board binary run from a
linked worktree against the live board runs the main checkout's launcher instead, and the live
board moves its schema forward only for a build of the schema source the default branch commits
([TASKS](docs/TASKS.md#where-it-lives), `crates/tasks/src/live.rs`): a branch's own build of the
board runs on a scratch board. `symphony-build` (`tools/rs-bin.sh symphony-build`) holds the
commands only this repository's own build and release run use, and is never shipped or installed
([CHECKS](docs/CHECKS.md#size-budgets)).

### The client's distribution

`just client-update` builds both clients (the Dioxus client's page modules and program, the React
client's page kinds) and publishes their bundle, one shell and one `builds.json` under
`apps/symphony-server/client/public/`, which the server package embeds at build time. A branch
commits no distribution, whatever it changed: the regenerator a land starts rebuilds it from
main's newest tip and lands it as a commit of its own ([TASKS](docs/TASKS.md#the-regenerator)),
so a task builds the client for its tests and pictures and puts the distribution back
(`git checkout -- apps/symphony-server/client/public`). The one exception is a release
candidate's fix that changes what the client is built from: no regenerator builds the candidate,
so that branch commits the distribution its own sources make, and the land keeps main's side of it
([RELEASES](docs/RELEASES.md#the-release-candidate)). `just client-check` rebuilds into a
separate folder and compares every path and byte with the committed distribution. How the build
stays the same from any checkout (its release snapshot, dx's one path, the builds a distribution
keeps) is [the client guide's](apps/symphony-ui/AGENTS.md) and [the React client's
guide's](apps/symphony-web/AGENTS.md).

`just verify` runs every check the release run does, each step with a log of its own
(`tools/verify.sh`): the release run's full verification, never a task's gate
([CHECKS](docs/CHECKS.md#the-release-run)).

### What it needs installed

Local acceptance requires a working Chrome executable (installed, or named with `SYMPHONY_CHROME`,
so a missing browser cannot pass as acceptance not run) and the React client's pinned Node (on
PATH, or fnm to fetch it once: `tools/node.sh`); the consuming project's binaries are not
prerequisites. The client build needs Git, Bash, jq, curl and a C/C++ build toolchain, and native
board fixtures need lsof and the pinned SurrealDB (`tools/surreal.sh`, or `SYMPHONY_SURREAL`). A
Linux build host makes the release snapshots' parent folder first (`sudo install -d -m 1777
/private/tmp`). This repository has no hosted GitHub CI requirement.

## This repository as its own project

`symphony.toml` registers this checkout as the project `symphony`: its docs and its
board are served under `/symphony/` by the machine's one server, and its board is this
repository's own, in its git common dir. Symphony work is filed, staffed and reviewed
on that board, never on a consuming project's. Every agent working here follows the
standing rules in [RULES.md](RULES.md).

**Its own names.** Symphony names only its own variables (`SYMPHONY_`), state folders
(`symphony/` in the git common dir), formats and settings, never a consuming project's
tools: `crates/symphony/tests/hooks/own_names.rs` fails on an old state or tool name
outside the fences it lists, each with its reason (a past incident's record in
`docs/history/`), and on the first consuming project's name, in any spelling or identifier
built from it, in a tracked file's text or path: say "the consuming project". The state
folders are named once, in `crates/tasks/src/folders.rs`.

**Memory lives in the repository.** Nothing goes in an agent's own memory folder
(`~/.claude/projects/…/memory/`): what must outlast a session is guidance here or in a
package's AGENTS.md, a skill, a doc or a board task, and it is committed, so every
agent sees it. In a Claude Code session the memory guard refuses the writes there it
can read (below), and `.claude/settings.json` denies Edit there as well.

**Models.** The model each role runs on is a setting, so a change of mind is one edit:
`[agents] launch` and `[agents.models]` in `symphony.toml` (`orchestrator`, `executor`,
`reviewer`, `assessor`), which a board launch takes and `just tasks limits` prints with where
each came from ([TASKS](docs/TASKS.md#limits-and-models)). The orchestrator may run on the most
capable model, for diagnosis and design; executors, reviewers and assessors run on Fable, as
`[agents] launch` and the table's `assessor` say. An Agent launch passes the model
`just tasks limits` names for the role, `model: "fable"` today (a subagent otherwise inherits
its orchestrator's model). A resumed agent keeps the model it started on. The work-in-progress
limits are settings the same way, in `[limits]`.

**Landing and pushing main.** An orchestrator lands an approved task with `just land SLUG --as
HANDLE`, never a merge by hand: it merges, closes the task and pushes `main` to `origin` as its
last step, without asking each time: the push is part of the land. The push fetches first,
merges what `origin/main` gained, never forces, and pushes no other branch and no tag; one that
fails leaves the landing done here and names the command to retry, and a land interrupted is run
again, which finishes it. A land here takes seconds: with `[land] check = false` it runs no check,
the docs audit being evidence at the tip, and it keeps main's side of the client distribution a
branch changed, which the regenerator it starts detached rebuilds and lands after
([TASKS](docs/TASKS.md#the-regenerator)). It lands a task once
every stamp it needs stands on its tip: the stamps its assessor wrote from the catalogue
(`just tasks stamps`), on the tip it read, a fresh reviewer's approve naming the tip alone where
none did; a missing stamp, a conflict or a red goes back on the thread, naming who gives it
([the default landing](docs/TASKS.md#the-default-landing)). An orchestrator starts the assessor
when it files a task and each time it moves to review, before any reviewer, and lands a task
without asking the owner: `just land` runs with no permission prompt, and the stamps standing on
the tip decide it. A decision the owner must make is a card in [DECISIONS](docs/DECISIONS.md),
not a question in chat.

**Releases.** A land starts a release run (`just release`, below) once `[releases]
every_landings` tasks (20) have landed since the last release or run ([the
cadence](docs/RELEASES.md#the-cadence)), and a green release refreshes the machine: each consuming
project's pin, main and board, and the shared server by its own reload ([the
refresh](docs/RELEASES.md#the-refresh)). Running a release run or `--commit` by hand, installing
its schedule and turning `[releases] publish` on are the owner's call; an orchestrator reruns
`just release --refresh` without asking once what a refresh left red, or left to a project, is put
right ([who runs it](docs/RELEASES.md#who-runs-it)). Pushing a consumer's branch and restarting
the shared server are separate acts that need their own word. A pin moved by hand to a commit of
main waits until `just tasks regenerate --status --wait` exits 0 and takes the commit it names, so
it never carries a client older than its sources ([TASKS](docs/TASKS.md#the-regenerator)).

**Board commands.** Run them as `just tasks … --as HANDLE`: a subagent's tool calls
count as its heartbeat only after a command of that shape binds it, and one that runs
the binary by its path reads lost while it works. A build or test is recorded for the
task its session holds; an orchestrator running one for a task, or a subagent without
a session of its own, names it: `TASKS_TASK=SLUG just test-rs …`, with
`TASKS_SESSION=HANDLE` when it has a handle (docs/MACHINE_WORK.md).

**Evidence, not the suite.** A task's gate is the tests closest to what it changed and
`just lint-rs`, with the client built for a client change (its distribution never committed),
and the docs audit for a docs change, posted on its thread as evidence
a reviewer judges and stamps (RULES.md `workflow/evidence-is-the-gate`; how to pick the tests and
what each kind of change shows: [CHECKS](docs/CHECKS.md)). No task runs `just verify`, whatever
its brief asks: it is the release run's (`just release`), on main's tip in a clean checkout outside
this folder (`../symphony-wt/release`), which tags a green tip as a release
([RELEASES](docs/RELEASES.md)).

## Agent integration

`symphony init --root ROOT` installs in a project the nine generic skills and their Claude links,
Claude allow rules and Codex rules, the heartbeat and memory-guard hooks, and the recipes the
skills name (`crates/symphony/shipped/symphony.just`: `tasks`, `agent-send`, `symphony`,
`symphony-cli`, `symphony-status`, `symphony-reply`, `symphony-ask`, and the default landing
`land`), which the project's justfile imports; a recipe of the project's own of the same name
wins, and so does an unstamped copy of a file it writes. What it writes, keeps current, retires
and refuses, and the `Next:` it ends with, is the module doc of `crates/symphony/src/init.rs`. Each
recipe and hook runs the project's build linked at `.target/bin/NAME`, else the main checkout's
link from a linked worktree, else NAME on PATH (`crates/symphony/src/builds.rs`), and builds
nothing.

The allow rules pass the board and reply recipes (`tasks`, `agent-send`, `symphony-status`,
`symphony-reply`, `symphony-ask`) and `land` with no prompt, and not the two server recipes:
`just symphony` starts, replaces or stops the machine's shared server and `just symphony-cli`
reaches `serve` and `init`, so an agent is asked first. `just land` needs no prompt, as what the
default land lands is decided by every stamp the task needs standing on the tip; its recipe sets
`SYMPHONY_LAND`, without which `tasks land` refuses, a guard against accident and not the gate. No
rule allows a command with an env prefix, since Claude Code's `*` matches any text: an agent names
its handle with `just tasks … --as HANDLE`, which binds its heartbeat, and a `TASKS_SESSION=`
prefix asks first. `just tasks` refuses the recorder (`tasks run`), which runs any command it is
handed ([MACHINE_WORK](docs/MACHINE_WORK.md#recording-builds-and-tests)). The board and reply
recipes set `SYMPHONY_UNASKED`, under which a place their command writes must lead into the
checkout it runs in, or from a linked worktree into the main checkout's artifacts folder, and into
no git folder (`crates/tasks/src/confine.rs`), and an agent the board starts stays there or under
the project's worktree base (`crates/tasks/src/launch/bounds.rs`,
[TASKS](docs/TASKS.md#where-it-lives)).

Every `just NAME` in the shipped skills, the docs, the guides and the binaries' strings is an
installed recipe or the consumer's landing, this repository's own where its lists
(`tools/named-recipes.txt`) name it for that file, or in a guide a recipe of its justfile: one
rule and one reader (`crates/symphony/src/docs_audit/recipes.rs`), by which the docs audit and
`crates/symphony/tests/hooks/named_recipes.rs` judge
([CHECKS](docs/CHECKS.md#the-recipes-a-text-names)).

`symphony hook tasks-beat` makes a session's tool calls its heartbeat on the board; it starts no
server, builds nothing, prints nothing and never refuses a tool call
(`crates/symphony/src/hook/tasks_beat.rs`). `symphony hook memory-guard` runs before a Claude Code
session's write tools and Bash and refuses, with exit 2 and one line naming where the guidance
goes instead, a write into an agent's memory folder (`~/.claude/projects/*/memory/`,
`~/.codex/memories/`): a write tool's path there, or a shell command that names the folder and
writes anything. It reads the command as text, so text that names the folder beside a write (a
commit message, a board post, a heredoc) is refused: say "the memory folder", or put the text in
a file with Write. What it reads as naming the folder and as a write, and what it misses, is its
module doc's (`crates/symphony/src/hook/memory_guard.rs`). A Codex session runs no Claude hook and
is not guarded.

**Looking at a page.** An agent sees a page with `just symphony-cli shot URL --out FILE.png`:
a private headless Chrome (`crates/chrome-devtools`) opens URL, waits until the page has drawn,
dismissing each dialog, runs the steps `--click CSS`, `--type CSS=TEXT`, `--key NAME`,
`--hover CSS`, `--eval JS` (its JSON on stdout), `--pause MS` and `--shot FILE.png` in the order
given, then saves the PNG. `--phone`, `--size WxH`, `--full-page` and `--scheme dark` set the
view, `--wait-for CSS` and `--timeout SECS` bound the waits, `--report` prints the console errors,
failed requests, dialogs dismissed and horizontal overflow it saw, and `--help` says the rest. It
only reads the page, so the live board (`http://127.0.0.1:8765/symphony/tasks`) is fair to shoot;
it never starts, stops or reconfigures a server. Pictures kept as a task's evidence are saved
under the project's artifacts folder in the task's slug (`artifacts/SLUG/` by default) and
attached, as a path there leads nowhere once the sweep moves a worktree's artifacts: `--attach`
(`just symphony-cli attach FILE…` for a file saved before) stores each by its content in the main
checkout's uploads, from a linked worktree too, and prints the line the thread post carries.

## Command ownership

`just symphony` runs the server, `just symphony-cli ARGS` the document CLI, and
`just tasks ARGS` and `just agent-send ARGS` the board entrypoints, and `just land SLUG`
the default landing (`tasks land`, `crates/tasks/src/land.rs`,
[TASKS](docs/TASKS.md#the-default-landing)), which runs only under that recipe, and never where
the project's justfile runs a `land` of its own. This repository's justfile imports the installed
`symphony.just` like any project's, and its own recipes of those names win: they execute this
workspace's declared binaries through ordinary Cargo; `land` it takes as installed, through its
own `tasks`. The standalone workspace requires no project gate or landing executable.

`just release` cuts this repository's releases ([RELEASES](docs/RELEASES.md)): main's tip
verified in full in a clean checkout outside this folder, judged by the checks this repository
owns and never by a consuming project's suite, each red check filed on this board, a green tip
tagged `release-YYYY-MM-DD` and that tag alone pushed, then the machine refreshed to it. A red
tip is held as the release candidate, the local branch `release-candidate` that is never pushed,
until its reds' own tasks, started there, land; then it is rerun and released. `--due` is what a
land's `[board] on_land` runs, `--refresh`, `--rerun` and `--drop` are an orchestrator's retries,
and `--status` runs nothing ([status](docs/RELEASES.md#status)). It is Symphony's own release
command, not a recipe `symphony init` installs.

Board roles, verdicts and configured review callbacks belong to Symphony, and so does the
default landing, convention over configuration. A consumer with a landing of its own keeps
it: its `land` recipe replaces the Symphony's everywhere for that project, and the workflow
examples' `land-approve`, `land-stamps`, `land-request`, `merge-risk` and `merge-feature`
recipes, with its verification, certificates and approval policy, are consumer integration,
not standalone product commands. This workspace supplies no stamp backend or aliases
forwarding to a containing project.
