# Symphony

The app's workspace owns its server, its web client and its Mac app under `apps/` (the client,
`apps/symphony-web`, React and TypeScript built by Vite, its browser tests a package of their own,
`apps/symphony-web-tests`; the Mac app, `apps/symphony-desktop`, a thin shell over each
server's own pages, `publish = false`, whose window crates no other package links), and the review
library, its own build and release commands (`crates/symphony-build`), wire types, task board,
working-directory scanner, headless Chrome driver, the optional GitHub issue exchange
(`crates/symphony-github`, a program of its own no other package builds with,
[GITHUB](docs/GITHUB.md)) and the tests' CPU meters and waits
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
sources, the Mac app, the build and release commands (`symphony-build`) and the tests' time keep
`publish = false` ([RELEASES](docs/RELEASES.md#packages-on-cratesio)).

Public Rust items a consumer calls follow the
[two-release rule](docs/CHECKS.md#consumer-api-changes): ship the replacement beside the working
old item without `#[deprecated]`, then remove the old item in a separate later task after the
consumer migrations are proved. Keep the compile guards in step with those calls; kit and helper
major-version requirements still apply.

A landing that changes what a project must do to take the next release (the board schema, such an
item, a `symphony.toml` key, something to run once) says so in a `Consumer-Note: SECTION: TEXT`
trailer of one of its own commits, which the release's notes carry to the agent that moves a
project's pin ([when a landing owes one](docs/CHECKS.md#what-a-landing-tells-a-consumer),
[RELEASES](docs/RELEASES.md#its-notes)); `just lint-rs` is red on the ones git can see left unsaid.

### Where each part is said

- **A project's settings** are its `symphony.toml`, every table and key in
  [PROJECT_FILE](docs/PROJECT_FILE.md), the generic defaults in `crates/symphony/src/project.rs`.
  One machine's preferences over them are `symphony.local.toml` beside it, never versioned, whose
  one reader is `crates/tasks/src/local.rs`.
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
  `apps/symphony-web/shell/shell.ts` repeats) win over every entrypoint.
- **The one server** serves every project the machine folder's registry holds, each under
  `/<id>/`. It is the server of the release installed on the machine (`current`) while every
  registered project's plugins answer from a process, which any host serves, and a project's build
  while one names a plugin linked into its host; the host rule decides whether a start serves,
  waits or replaces, and no start replaces a release's server, which follows `current` by its own
  reload ([the server guide](apps/symphony-server/README.md), `crates/symphony/src/machine/`).
  A start in a linked worktree never starts, registers with or replaces the one server
  (`apps/symphony-server/src/starter.rs`): `just symphony --port 0` there runs a server of its
  own, and `--shared` is the owner's word. A project's state stays in its checkout: paths under its
  artifacts folder go through `Project::artifact`, never an `artifacts/` literal, and the client
  keys what it keeps in the browser by the project's id. Tests use a temporary machine folder
  (`SYMPHONY_MACHINE_DIR`).
- **The board** is [TASKS](docs/TASKS.md); recorded builds and tests are
  [MACHINE_WORK](docs/MACHINE_WORK.md) and [RUNS](docs/RUNS.md); what a task proves is
  [CHECKS](docs/CHECKS.md); releases are [RELEASES](docs/RELEASES.md).

Read [the client's guide](apps/symphony-web/AGENTS.md), [the Mac app's
guide](apps/symphony-desktop/AGENTS.md) and
[the board guide](crates/tasks/AGENTS.md) before editing those packages. A consuming project
applies its own Rust, review and resource policy.
Never run a shared-service restart as a test: use a private port and scratch board.

### Building and testing

Use `just lint-rs`, `just test-rs`, `just symphony-ui-dist` and `just symphony-ui-dev`
from this folder; `just lint-rs` runs `just client-lint` as well, the client's checks on
its pinned Node ([its guide](apps/symphony-web/AGENTS.md)). `tools/cargo.sh` runs ordinary Cargo
in this workspace, preserving its arguments, streams, status and inherited limits. A build or test
goes through `tools/record.sh`, as do the `lint-rs`, `test-rs`, `client-check`, `client-update`
and `verify` recipes and the docs checks (`just symphony-cli docs-audit --check`,
`status --lint`), each as one operation the board's recorder records for the Build and Test
columns, or runs as it is where none answers
([MACHINE_WORK](docs/MACHINE_WORK.md#recording-builds-and-tests), its logs in
[RUNS](docs/RUNS.md#logs)). It uses this folder's `.target/` unless `CARGO_TARGET_DIR` names
another: a linked worktree's starts as a copy-on-write clone of the main checkout's compiled
dependencies (`tools/seed-target.sh`), never one folder for two worktrees, where Cargo names their
packages alike and each runs the other's build. The toolchain pin is `rust-toolchain.toml`. The workspace supplies no project gate planner, test selection, certificate or
shared slot pool: the containing project keeps its own for its own consumers.

`tools/rs-bin.sh NAME` runs a declared workspace binary, asking Cargo to build it first only when
something Cargo would read changed since the last build the launcher asked for, through a link
renewed only when Cargo relinked; its comments say what it watches and what it cannot see.
`--build NAME` builds and prints that path, `--where NAME` names Cargo's own copy without
building, and `--link NAME` builds and points `.target/bin/NAME` at it. A board binary run from a
linked worktree against the live board runs the main checkout's launcher instead, and the live
board moves its schema forward only for a build of the schema source the default branch commits
([TASKS](docs/TASKS.md#where-it-lives), `crates/tasks/src/live.rs`): a branch's own build of the
board runs on a scratch board. `symphony-build` (`tools/rs-bin.sh symphony-build`, a package of
its own, `crates/symphony-build`) holds the commands only this repository's own build and release
run use, and is never shipped, installed or published: it reads the library through its public
API alone, so neither the library every project links nor its `symphony` compiles any of it
([CHECKS](docs/CHECKS.md#size-budgets)).

### The client's distribution

`just client-update` builds the client (one Vite build of `apps/symphony-web`: its kinds over the
stylesheet every page shares) and publishes its bundle, one shell and one
`builds.json` under
`apps/symphony-server/client/public/`, which the server package embeds at build time. A branch
commits no distribution, whatever it changed: the regenerator a land starts rebuilds it from
main's newest tip and delivers it as a task of its own, `generated/outputs-at-TIP-COMMIT`
([TASKS](docs/TASKS.md#what-the-generators-make-delivered-by-a-pull-request)), and a land refuses
a branch that changes it, so a task builds the client for its tests and pictures and puts the
distribution back (`git checkout -- apps/symphony-server/client/public`). The one exception is a
release candidate's fix that changes what the client is built from: no regenerator builds the
candidate, so the head its approve names commits the distribution its own sources make, and the
branch then merges main once, keeping main's side of it, and delivers that second head
([RELEASES](docs/RELEASES.md#the-release-candidate)). `just client-check` rebuilds into a
separate folder and compares every path and byte with the committed distribution. How the build
stays the same from any checkout, and the builds a distribution keeps, is [the client's
guide's](apps/symphony-web/AGENTS.md#the-build-and-the-distribution).

`just verify` runs every check the release run does, each step with a log of its own
(`tools/verify.sh`): the release run's full verification, never a task's gate
([CHECKS](docs/CHECKS.md#the-release-run)).

### What it needs installed

Local acceptance requires a working Chrome executable (installed, or named with `SYMPHONY_CHROME`,
so a missing browser cannot pass as acceptance not run) and the client's pinned Node (on
PATH, or fnm to fetch it once: `tools/node.sh`); the consuming project's binaries are not
prerequisites. The client build needs Git, Bash and jq, and native
board fixtures need lsof and the pinned SurrealDB (`tools/surreal.sh`, or `SYMPHONY_SURREAL`).
The release verification owns an explicit `nextest-setup` step before tests, provisioning
cargo-nextest 0.9.152 from checksum-pinned upstream binaries (Bash, curl, tar and `sha256sum` or
`shasum`, HTTPS to GitHub). The launcher checks this target's `tools/nextest/` first, then a linked
worktree's main checkout `.target/tools/nextest/`; setup provisions that main cache once for its
worktrees. Tests never install it. Focused `just test-rs` uses Cargo and needs no nextest: the live
runner fixture is explicitly ignored there and selected by name by the release tests step.
The board's server tests (`just test-rs -p symphony-board --test server`, and so the release run)
need the machine's PostgreSQL 18 or later: the absolute folder `symphony.toml`'s `[board]
postgresql` names, otherwise the first executable `postgres` on `PATH`, with `initdb`, `pg_ctl`
and `psql` in that same folder. [The setup](docs/PROJECT_FILE.md#board) gives the portable
Homebrew line; absent or incompatible tools fail with setup guidance. The hosted board's check
(`just hosted-check`, and so the release run) needs PostgreSQL 16's programs as well, in
Homebrew's keg (`brew install postgresql@16`, which starts nothing and changes no `postgres` on
`PATH`) or the folder `SYMPHONY_POSTGRESQL_16` names, and `openssl`
([POSTGRESQL](docs/POSTGRESQL.md#a-hosted-board)). This repository has no hosted GitHub CI
requirement.

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
built from it, in a tracked file's text or path: say "the consuming project". It passes over
the owner's own dated lines under a card, wherever it sits, saved as they said them, and judges
every other line. The state folders are named once, in `crates/tasks/src/folders.rs`.

**Memory lives in the repository.** Nothing goes in an agent's own memory folder
(`~/.claude/projects/…/memory/`): what must outlast a session is guidance here or in a
package's AGENTS.md, a skill, a doc or a board task, and it is committed, so every
agent sees it. In a Claude Code session the memory guard refuses the writes there it
can read (below), and `.claude/settings.json` denies Edit there as well.

**Models.** The model each role runs on is a setting, so a change of mind is one edit:
`[agents] launch` and `[agents.models]` in `symphony.toml` (`orchestrator`, `executor`,
`reviewer`, `assessor`), which a board launch takes and `just tasks limits` prints with where
each came from ([TASKS](docs/TASKS.md#limits-and-models)). Each runs on the model `just tasks limits`
names for the role. `just symphony-cli model ROLE MODEL` sets a role's model in the main checkout,
or `all` for all four roles, preserving comments and order. `just symphony-cli model all fable
--agent claude --assessor haiku` also sets the launch agent and its fallback model, with a separate
assessor model. Recognizable cross-agent model names warn without refusing arbitrary text; the
change follows the settings-only landing below. A machine's own choice needs no landing:
`symphony.local.toml` beside the main checkout's `symphony.toml`, which git does not version,
overrides the launch agent, the models and `[limits]` there and nothing else (`--local` on the
model command writes it there instead), and `just tasks limits` names it for each setting it
overrides ([PROJECT_FILE](docs/PROJECT_FILE.md#the-local-settings-file)). Between the two the
board keeps preferences of its own (`just tasks settings`), read after `symphony.toml` and before
the local file, which hold at once and need no landing either
([PROJECT_FILE](docs/PROJECT_FILE.md#the-boards-store)). An Agent launch passes the
model `just tasks limits` names for the role (a subagent otherwise inherits its orchestrator's
model). A resumed agent keeps the model it started on. The work-in-progress limits are settings
the same way, in `[limits]`.

**Landing by pull request.** This repository's tasks land through pull requests on
`heavenstudio/symphony` (`[land] delivery`,
[GITHUB_SETUP](docs/GITHUB_SETUP.md#delivery-by-pull-request)), and nobody pushes `main`, the
owner included: its ruleset refuses every update but GitHub's squash of a pull request whose
`symphony/eligible` check is green. Nobody runs a land either. A task's move to review dispatches
it, detached from the main checkout: its branch pushed, its pull request opened, its evidence
written there as checks and GitHub's auto-merge armed; each later assessment, verdict, gate
decision or move writes the checks again. Once every stamp it needs stands on its head (the stamps
its assessor wrote from the catalogue, `just tasks stamps`, on the head it read, and a fresh
reviewer's approve naming the head alone where none did), `symphony/eligible` turns green, GitHub
squashes it within seconds, and the board closes the task, fast-forwards the main checkout and
starts `[board] on_land` and the regenerator
([a land dispatched at review](docs/TASKS.md#a-land-dispatched-at-review)). A dispatch that
stops, another check's red, a conflict or a pull request closed unmerged is said on the thread to
`@orchestrator`, naming what follows; `just land SLUG --as HANDLE`, which needs no permission
prompt, dispatches by hand a task that reached done before these settings landed, or one whose
merge did not come. What dispatches is the main checkout's `.target/bin/symphony-github`, a build
of main put there by hand (`tools/cargo.sh build -p symphony-github`, then copied): nothing
rebuilds it, so a change to the program is built and put there again once it lands. An
orchestrator starts the assessor when it files a task and each time it moves to review, before any
reviewer: a task nobody assessed at its head waits in review with its checks pending, and nothing
merges it. A decision the owner must make is a card at the place in its document where the thing
it decides is said ([REVIEW](docs/REVIEW.md#where-a-card-is-written)), not a question in chat.

**Releases.** A land starts a release run (`just release`, below) once `[releases]
every_landings` tasks (20) have landed since the last release or run ([the
cadence](docs/RELEASES.md#the-cadence)), or, where `[releases] hosted` has the release verified
on GitHub's runners, once `hosted_every_minutes` passed since the last run started, that run
verifying nothing here ([the hosted clock](docs/RELEASES.md#the-hosted-clock)), and a green
release refreshes the machine: the release
installed in the machine folder ([the install](docs/RELEASES.md#the-install)), each registered
project's board moved to its schema once every writer seen there in the last week is on the
release, `current` pointed at it, and the machine's server following `current` by its own reload
([the refresh](docs/RELEASES.md#the-refresh)). It writes no project's checkout: no branch, no
commit and no pin moved, as a project takes a release by its own landing. While a registered
project names a linked plugin, the release is installed and nothing more. Running a release run
or `--commit` by hand, installing its schedule and turning `[releases] publish` on are the
owner's call; an orchestrator reruns `just release --refresh` without asking once what a refresh
left red, or left to a project, is put right ([who runs it](docs/RELEASES.md#who-runs-it)).
The one exception is the run that follows a landing that moves the board's schema, which the
task's orchestrator starts at once without asking, once the board's landed line is on its thread
([the cadence](docs/RELEASES.md#the-cadence)).
A schema move never waits for the owner: when this board is at a schema the release `current`
names does not serve, the refresh of a green release serving it points `current` forward itself
([the refresh](docs/RELEASES.md#the-refresh)). Where one did not, its failed update's task
filed ([an update](docs/RELEASES.md#an-update)), an orchestrator points `current` forward at that
release at once, without asking, and says so on that task's thread; another project's pages then
save nothing until its board moves, which its own update's task tells. Restarting the shared
server by hand, its first switch to a release's own server among it, is a separate act that needs
its own word, and so is pointing `current` back to an older release
([the way back](docs/RELEASES.md#the-way-back)). A pin moved by hand to a commit of main waits
until `just tasks regenerate --status --wait` exits 0 and takes the commit it names, so it never
carries a client older than its sources ([TASKS](docs/TASKS.md#the-regenerator)).

**Board commands.** Run them as `just tasks … --as HANDLE`: a subagent's tool calls
count as its heartbeat only after a command of that shape binds it, and one that runs
the binary by its path reads lost while it works. A build or test is recorded for the
task its session holds; an orchestrator running one for a task, or a subagent without
a session of its own, names it: `TASKS_TASK=SLUG just test-rs …`, with
`TASKS_SESSION=HANDLE` when it has a handle (docs/MACHINE_WORK.md).

A board-launched Codex executor or reviewer already inherits `TASKS_SESSION`: run its own
checks as one recipe (`just --one -- test-rs …`, `just --one -- lint-rs`), so the recorder derives
the task from its held role. Keep `--one --` before the recipe: the first option refuses a second
recipe before either runs, and the separator ends option parsing. Do not repeat session/task
assignments on each command. This repository's `.codex/rules/checks.rules` allows its named
checks to record shared state outside the worktree sandbox; it grants no directory-wide access.
For both document checks use `just --one -- docs-check`, whose arguments are fixed; the generic
document CLI also accepts flags that write documents.

Give each check or board command its own tool invocation, with plain arguments and a quoted
message for `just tasks say`. Keep the executable, recipe and subcommand words unquoted.
Concatenating shell quote fragments to encode one message argument can also keep the command
sandboxed. Use a single literal quoted body where that preserves its text; complex quoting
uses the active approval flow. Do not redirect checks into task artifacts or add environment
assignments, substitutions or other shell expansion: those forms prevent Codex from
matching the named command rule ([shell handling](https://developers.openai.com/codex/rules#shell-wrappers-and-compound-commands)).
A simple compound command is evaluated by all its parts; one unmatched part can keep the whole
invocation sandboxed. Independent tool calls can still run concurrently. Keep the tool's output
or read the recorder's existing [run log](docs/RUNS.md#logs); `just tasks runs SLUG --as HANDLE`
shows the recorded results, so an extra redirected run is unnecessary. A check
run for another task still needs the explicit attribution above and the active approval flow.
Full sessions register their own callback early with `just tasks callback --as HANDLE`;
in-process agents read their inbox instead. An orchestrator also reads
[its project guidance](.agents/skills/task-orchestrator/project.md).

**Evidence, not the suite.** A task's gate is the tests closest to what it changed and
`just lint-rs`, with the client built for a client change (its distribution never committed),
and the docs audit for a docs change, posted on its thread as evidence
a reviewer judges and stamps (RULES.md `workflow/evidence-is-the-gate`; how to pick the tests and
what each kind of change shows: [CHECKS](docs/CHECKS.md)). No task runs `just verify`, whatever
its brief asks: it is the release run's (`just release`), on main's tip in a clean checkout outside
this folder (`../symphony-wt/release`), which tags a green tip as a release
([RELEASES](docs/RELEASES.md)).

**A change of settings needs no reviewer.** A change of the values `symphony.toml` gives settings
the Symphony already reads (a role's model, a limit, a count), with this guide's or the rules'
words that repeat them and nothing else, lands on `just settings-check` green at its tip: the
project file read as the Symphony reads it and judged on its shape alone (every table and key
known, each value of its type, each required value there; a model is any text, never one of a
list), recorded as a test run, which is the `tests` stamp. Its assessor names `tests` alone, and
no reviewer reads it. A setting the Symphony does not read yet is code, and `[project]`'s
`cargo` and `default_branch`, `[plugins]`'s `crates` and `process`, `[verdicts]`, `[land]`,
`[releases]`, `[stamps]` and `[board]`'s `on_review`, `on_land`, `machine_work_reader`,
`postgresql`, `external_postgresql` and `assessments` hold commands or programs the Symphony
runs, or decide what lands or where the board is kept: those keep `review`
([TASKS](docs/TASKS.md#needed-stamps), [PROJECT_FILE](docs/PROJECT_FILE.md#the-settings-check)).

## Agent integration

`symphony init --root ROOT` installs in a project the nine generic skills and their Claude links,
Claude allow rules and Codex rules, the heartbeat and memory-guard hooks, and the recipes the
skills name (`crates/symphony/shipped/symphony.just`: `tasks`, `agent-send`, `symphony`,
`symphony-cli`, `symphony-status`, `symphony-reply`, `symphony-ask`, and the default landing
`land`), which the project's justfile imports; a recipe of the project's own of the same name
wins, and so does an unstamped copy of a file it writes. What it writes, keeps current, retires
and refuses, and the `Next:` it ends with, is the module doc of `crates/symphony/src/init.rs`. Each
recipe runs the project's build linked at `.target/bin/NAME`, else the main checkout's link from
a linked worktree, else the release installed on the machine (`current/bin/NAME` in the machine
folder, [RELEASES](docs/RELEASES.md#the-install)), else NAME on PATH
(`crates/symphony/src/builds.rs`), and builds nothing; a hook finds `symphony` by the links, else
on PATH.

**What an agent is told, and where.** What a launch prompt carries, and where what is one task's
alone goes instead, is step 5 of [the orchestrator's
skill](.agents/skills/task-orchestrator/SKILL.md). What holds for every task here is standing and
committed, so every orchestrator's agents work under the same words and a change to them is a
commit: a rule in [RULES.md](RULES.md), guidance here, in a package's guide or in
[CHECKS](docs/CHECKS.md), and what one role alone needs in `project.md` beside that role's
installed skill (`.agents/skills/work/project.md`, `.agents/skills/review-task/project.md`),
which the skill reads first. That page is the project's own: init writes and retires only the
files a skill's stamp names, and never a file beside them.

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
view, `--clock SECS` holds the page's own clock at that moment (seconds since 1970, UTC), so a
page that draws an age or a time is the same picture on any day, `--wait-for CSS` and
`--timeout SECS` bound the waits, `--report` prints the console errors,
failed requests, dialogs dismissed and horizontal overflow it saw, and `--help` says the rest. A
capture without interaction steps reads the page, so the live board
(`http://127.0.0.1:8765/symphony/tasks`) is fair to shoot. Inspect interaction steps and script
text as described below. Pictures kept as a task's evidence are saved
under the project's artifacts folder in the task's slug (`artifacts/SLUG/` by default) and
attached, as a path there leads nowhere once the sweep moves a worktree's artifacts: `--attach`
(`just symphony-cli attach FILE…` for a file saved before) stores each by its content in the main
checkout's uploads, from a linked worktree too, and prints the line the thread post carries.

For Codex evidence commands use `just --one -- symphony-cli shot URL --out FILE.png …` or
`just --one -- symphony-cli attach FILE …`, each as a plain tool invocation. These commands use
the active approval flow: inspect the entire argument list, the task's artifact paths, the
local URL and every interaction or `--eval` script. A prefix ending at one filename still
permits extra file arguments; it does not confine uploads to that file or task. Do not turn
one approved capture or attachment into a reusable `symphony-cli`, shell or script grant.

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
`--github` asks for the GitHub Release of releases tagged already
([RELEASES](docs/RELEASES.md#a-github-release)), and `--status` runs nothing ([status](docs/RELEASES.md#status)). It is Symphony's own release
command, not a recipe `symphony init` installs.

Board roles, verdicts and configured review callbacks belong to Symphony, and so does the
default landing, convention over configuration. A consumer with a landing of its own keeps
it: its `land` recipe replaces the Symphony's everywhere for that project, and the workflow
examples' `land-approve`, `land-stamps`, `land-request`, `merge-risk` and `merge-feature`
recipes, with its verification, certificates and approval policy, are consumer integration,
not standalone product commands. This workspace supplies no stamp backend or aliases
forwarding to a containing project.
