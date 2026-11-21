# Symphony

The app's workspace owns its server, its web client and its Mac app under `apps/`
(`apps/symphony-server`; `apps/symphony-web`, its browser tests in `apps/symphony-web-tests`;
`apps/symphony-desktop`), and under `crates/` the review library (`crates/symphony`), its own build
and release commands (`crates/symphony-build`), the wire types, the task board (`crates/tasks`),
the plugin kit, the headless Chrome driver, the optional GitHub issue exchange
(`crates/symphony-github`, [GITHUB](docs/GITHUB.md)) and the tests' CPU meters and waits
(`crates/test-time`). The app's formats and operating contract live in [docs/](docs/README.md). A
consuming project owns its landing policy and shared-service safeguards.

This guide holds what every session here needs, each rule in one place: the standing rules are
[RULES.md](RULES.md), which every agent follows, and how a part works is the document or module
doc each line names. Read [the client's guide](apps/symphony-web/AGENTS.md), [the Mac app's
guide](apps/symphony-desktop/AGENTS.md) and [the board guide](crates/tasks/AGENTS.md) before
editing those packages.

## Rust

`Cargo.toml` and `Cargo.lock` belong to this workspace, and its members inherit its package
settings and dependency versions; the toolchain pin is `rust-toolchain.toml`. A consuming project
builds on these libraries through an immutable Git dependency pin, and the packages made to be
published are [RELEASES](docs/RELEASES.md#packages-on-cratesio)'s.

- A public Rust item a consumer calls changes by the
  [two-release rule](docs/CHECKS.md#consumer-api-changes) (RULES.md
  `code/consumer-api-two-releases`).
- A landing that changes what a project must do to take the next release (the board schema, such
  an item, a `symphony.toml` key, something to run once) says so in a
  `Consumer-Note: SECTION: TEXT` trailer of one of its own commits
  ([when a landing owes one](docs/CHECKS.md#what-a-landing-tells-a-consumer)); `just lint-rs` is
  red on the ones git can see left unsaid.

### Where each part is said

- **A project's settings** are its `symphony.toml` ([PROJECT_FILE](docs/PROJECT_FILE.md), the
  defaults in `crates/symphony/src/project.rs`); one machine's preferences over them are
  `symphony.local.toml` beside it, never versioned, read by `crates/tasks/src/local.rs` alone.
- **A project's own code is a plugin** (`crates/symphony-plugin`,
  [PLUGIN_PROTOCOL](docs/PLUGIN_PROTOCOL.md)): it renders to generic blocks, never HTML, and the
  client draws each block with one component.
- **A project's documents** are served from its `[[entrypoints]]`
  (`crates/symphony/src/entrypoint.rs`), each named by its route in every link and on the wire.
  Reads, saves and commits name the file `Project::file` maps a route to, and every walk goes
  through `Project::walk`. The Symphony's own routes (`symphony_api::SYMPHONY_ROUTES`, which
  `apps/symphony-web/shell/shell.ts` repeats) win over every entrypoint.
- **The one server** serves every project the machine folder's registry holds, each under
  `/<id>/` ([the server guide](apps/symphony-server/README.md), `crates/symphony/src/machine/`).
  A project's state stays in its checkout: a path under its artifacts folder goes through
  `Project::artifact`, never an `artifacts/` literal, and the client keys what it keeps in the
  browser by the project's id.
- **The board** is [TASKS](docs/TASKS.md); recorded builds and tests are
  [MACHINE_WORK](docs/MACHINE_WORK.md) and [RUNS](docs/RUNS.md); what a task proves is
  [CHECKS](docs/CHECKS.md); releases are [RELEASES](docs/RELEASES.md).

### Building and testing

Use `just lint-rs` (which runs `just client-lint` too), `just test-rs`, `just symphony-ui-dist`
and `just symphony-ui-dev` from this folder; `tools/cargo.sh` runs ordinary Cargo in this
workspace. A task's gate is the tests closest to what it changed and `just lint-rs`, posted as
evidence (RULES.md `workflow/evidence-is-the-gate`, [CHECKS](docs/CHECKS.md)). `just verify` is
the release run's alone, never a task's.

- A build or test goes through `tools/record.sh`, as the recipes do, so the board records it for
  the task its session holds ([MACHINE_WORK](docs/MACHINE_WORK.md#recording-builds-and-tests)).
  An orchestrator running one for a task, or a subagent without a session of its own, names it:
  `TASKS_TASK=SLUG just test-rs …`, with `TASKS_SESSION=HANDLE` when it has a handle.
- The target folder is this folder's `.target/` unless `CARGO_TARGET_DIR` names another: a linked
  worktree's starts as a copy-on-write clone of the main checkout's (`tools/seed-target.sh`),
  never one folder for two worktrees.
- `tools/rs-bin.sh NAME` runs a declared workspace binary, building it first when it must
  (`--build`, `--where` and `--link` in its comments). A board binary run from a linked worktree
  against the live board runs the main checkout's build: a branch's own build of the board runs
  on a scratch board ([TASKS](docs/TASKS.md#where-it-lives)).
- Tests use a temporary machine folder (`SYMPHONY_MACHINE_DIR`). Never run a shared-service
  restart as a test: use a private port and a scratch board. A start in a linked worktree never
  starts, registers with or replaces the one server: `just symphony --port 0` there runs a server
  of its own, and `--shared` is the owner's word.

### The client's distribution

`just client-update` builds the client and publishes its bundle under
`apps/symphony-server/client/public/`, which the server package embeds. A branch commits no
distribution, whatever it changed: the regenerator a land starts rebuilds it from main's newest
tip and delivers it as a task of its own
([TASKS](docs/TASKS.md#what-the-generators-make-delivered-by-a-pull-request)), and a land refuses
a branch that changes it, so a task builds the client for its tests and pictures and puts the
distribution back (`git checkout -- apps/symphony-server/client/public`). The one exception is a
release candidate's fix that changes what the client is built from: the head its approve names
commits the distribution its own sources make, and the branch then merges main once, keeping
main's side of it, and delivers that second head
([RELEASES](docs/RELEASES.md#the-release-candidate)). `just client-check` compares a fresh build
with the committed distribution; the rest is
[the client's guide's](apps/symphony-web/AGENTS.md#the-build-and-the-distribution).

### What it needs installed

A tool that is absent or incompatible fails with setup guidance; no hosted GitHub CI is required.

- Acceptance: a working Chrome (installed, or `SYMPHONY_CHROME`) and the client's pinned Node (on
  PATH, or fnm to fetch it once: `tools/node.sh`).
- The client build: Git, Bash and jq.
- Board fixtures: lsof and the pinned SurrealDB (`tools/surreal.sh`, or `SYMPHONY_SURREAL`).
- The board's server tests (`just test-rs -p symphony-board --test server`): PostgreSQL 18 or
  later, from `[board] postgresql` or the first `postgres` on `PATH`, with `initdb`, `pg_ctl` and
  `psql` beside it ([the setup](docs/PROJECT_FILE.md#board)).
- The hosted board's check (`just hosted-check`): PostgreSQL 16's programs as well (Homebrew's
  `postgresql@16` keg, or `SYMPHONY_POSTGRESQL_16`) and `openssl`
  ([POSTGRESQL](docs/POSTGRESQL.md#a-hosted-board)).
- The release run's tests: cargo-nextest, provisioned by its own `nextest-setup` step; focused
  `just test-rs` uses Cargo and needs none.

## This repository as its own project

`symphony.toml` registers this checkout as the project `symphony`: its docs and its board are
served under `/symphony/` by the machine's one server, and its board is this repository's own, in
its git common dir. Symphony work is filed, staffed and reviewed on that board, never on a
consuming project's.

- **Its own names.** Symphony names only its own variables (`SYMPHONY_`), state folders, formats
  and settings, never a consuming project's tools, and no tracked file names the first consuming
  project: say "the consuming project" (`crates/symphony/tests/hooks/own_names.rs` fails on
  either).
- **Board commands** run as `just tasks … --as HANDLE`: that shape binds a subagent's tool calls
  as its heartbeat, and no allow rule passes a command with an env prefix. `just tasks limits`
  prints the limits and each role's model with where each came from. A full session registers its
  own callback early (`just tasks callback --as HANDLE`); an in-process agent reads its inbox.
- **Models and limits are settings**: `[agents] launch`, `[agents.models]` and `[limits]` in
  `symphony.toml`, a machine's own choice in `symphony.local.toml`
  ([TASKS](docs/TASKS.md#limits-and-models),
  [PROJECT_FILE](docs/PROJECT_FILE.md#the-local-settings-file)). An Agent launch passes the model
  `just tasks limits` names for the role, as a subagent otherwise inherits its orchestrator's.
  `just symphony-cli model ROLE MODEL` changes one.
- **A change of settings needs no reviewer**: a change of the values `symphony.toml` gives
  settings the Symphony already reads, with the words here that repeat them and nothing else,
  lands on `just settings-check` green at its tip, which is its `tests` stamp. A value that is a
  command or a program, or that decides what lands or where the board is kept, keeps `review`
  ([which](docs/PROJECT_FILE.md#the-settings-check), [TASKS](docs/TASKS.md#needed-stamps)).
- **Landing by pull request.** Nobody runs a land and nobody pushes `main`, the owner included
  (`[land] delivery`, [GITHUB_SETUP](docs/GITHUB_SETUP.md#delivery-by-pull-request)). A task's
  move to review dispatches it: its branch pushed, its pull request opened, its evidence written
  there as checks. Once every stamp it needs stands on its head, GitHub squashes it within
  seconds and the board closes the task
  ([a land dispatched at review](docs/TASKS.md#a-land-dispatched-at-review)). So an orchestrator
  starts the assessor when it files a task and each time it moves to review, before any
  reviewer: a task nobody assessed at its head waits in review, and nothing merges it. A
  dispatch that stops, a red check, a conflict or a pull request closed unmerged is said on the
  thread to `@orchestrator`, naming what follows; `just land SLUG --as HANDLE` dispatches by
  hand a task whose merge did not come. What dispatches is the main checkout's
  `.target/bin/symphony-github`, a build of main put there by hand
  (`tools/cargo.sh build -p symphony-github`, then copied) and again once a change to that
  program lands. A decision the owner must make is a card in [DECISIONS](docs/DECISIONS.md),
  not a question in chat.
- **Releases.** A land starts a release run by [the cadence](docs/RELEASES.md#the-cadence), and a
  green release refreshes the machine. What an orchestrator runs without asking (a refresh's
  retry, the run after a landing that moves the board's schema, once the board's landed line is
  on its thread, pointing `current` forward after a schema move) and what stays the owner's
  word (a run by hand, the schedule, publication, restarting the shared server by hand,
  pointing `current` back) is
  [who runs it](docs/RELEASES.md#who-runs-it), [the way back](docs/RELEASES.md#the-way-back) and
  [the orchestrator's page](.agents/skills/task-orchestrator/project.md). A pin moved by hand to
  a commit of main waits until `just tasks regenerate --status --wait` exits 0 and takes the
  commit it names.

### Checks from a Codex session

A board-launched Codex executor or reviewer inherits `TASKS_SESSION`, and this repository's
`.codex/rules/checks.rules` allows its named checks, with examples of each form.

- Run each check as one recipe in a tool invocation of its own: `just --one -- test-rs …`,
  `just --one -- lint-rs`, `just --one -- docs-check`. Keep `--one --` before the recipe. A check
  run for another task names it (`TASKS_TASK=SLUG`) and goes through the active approval flow.
- Plain arguments only, with a single quoted body for a `just tasks say` message: no redirection,
  environment assignment, substitution or concatenated quote fragments, any of which keeps the
  command from matching its rule. `just tasks runs SLUG --as HANDLE` shows the recorded results,
  so nothing is redirected into a file.
- `just --one -- symphony-cli shot …` and `… attach FILE …` use the active approval flow: inspect
  the whole argument list each time, and never turn one approval into a reusable grant.

## Agent integration

`symphony init --root ROOT` installs in a project the nine generic skills and their Claude links,
the allow rules, the heartbeat and memory-guard hooks, and the recipes the skills name; what it
writes, keeps current and refuses is the module doc of `crates/symphony/src/init.rs`. The skills'
source is `crates/symphony/shipped/skills/`: edit it there and run init again, never the installed
copy alone.

- **What an agent is told, and where.** A launch prompt is the skill, the slug and the handle.
  What is one task's alone goes in its brief or on its thread. What holds for every task is
  standing and committed: a rule in [RULES.md](RULES.md), guidance here, in a package's guide or
  in [CHECKS](docs/CHECKS.md), and what one role alone needs in `project.md` beside that role's
  installed skill (`.agents/skills/work/project.md`, `.agents/skills/review-task/project.md`,
  `.agents/skills/task-orchestrator/project.md`), which init never writes.
- **Prompts.** The allow rules pass the board and reply recipes (`just tasks`, `just agent-send`,
  `just symphony-status`, `just symphony-reply`, `just symphony-ask`) and `just land` with no
  prompt; `just symphony` and `just symphony-cli` ask first, and so does any command with an env
  prefix. A place an unasked recipe writes must lead into its checkout, or from a linked worktree
  into the main checkout's artifacts folder (`crates/tasks/src/confine.rs`).
- **Recipes in text.** Every `just NAME` a skill, a doc, a guide or a binary's string names is an
  installed recipe, this repository's own where `tools/named-recipes.txt` lists it, or the
  consumer's landing ([CHECKS](docs/CHECKS.md#the-recipes-a-text-names)); the docs audit and
  `crates/symphony/tests/hooks/named_recipes.rs` fail on any other.
- **Hooks.** `symphony hook tasks-beat` makes a session's tool calls its heartbeat and never
  refuses one. `symphony hook memory-guard` refuses a write into an agent's memory folder
  (RULES.md `memory-lives-in-the-repo`).

### Looking at a page

`just symphony-cli shot URL --out FILE.png` opens URL in a private headless Chrome, waits until
the page has drawn, runs the steps given in order (`--click CSS`, `--type CSS=TEXT`, `--key NAME`,
`--hover CSS`, `--eval JS`, `--pause MS`, `--shot FILE.png`) and saves the PNG; `--phone`,
`--size WxH`, `--full-page` and `--scheme dark` set the view, `--report` prints what went wrong
on the page, and `--help` says the rest. A capture with no step only reads, so the live board
(`http://127.0.0.1:8765/symphony/tasks`) is fair to shoot. A picture kept as a task's evidence is
saved under the artifacts folder in the task's slug and attached (`--attach`, or
`just symphony-cli attach FILE…`), which prints the line the thread post carries: a path in a
worktree leads nowhere once the sweep moves it.

## Command ownership

`just symphony` runs the server, `just symphony-cli ARGS` the document CLI, `just tasks ARGS` and
`just agent-send ARGS` the board, and `just land SLUG` the default landing. This repository's
justfile imports the installed `symphony.just` like any project's, and its own recipes of those
names win: they run this workspace's binaries through ordinary Cargo. `just release` cuts this
repository's releases ([RELEASES](docs/RELEASES.md)) and is no recipe `symphony init` installs.

Board roles, verdicts, review callbacks and the default landing belong to Symphony. A consumer
with a landing of its own keeps it: its `land` recipe replaces the Symphony's for that project,
and the workflow examples' `land-approve`, `land-stamps`, `land-request`, `merge-risk` and
`merge-feature` recipes, with its verification, certificates and approval policy, are consumer
integration, not product commands.
