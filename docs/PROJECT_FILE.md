# The project file

A project's settings are its `symphony.toml`, committed at the root of its main checkout. Every
table and key is optional: a key left out takes the default below, and an empty file, or none, is
a plain project of every default. A table or key the Symphony does not know is refused by name and
line where the project is loaded or linted (`just symphony-status --lint`), and left out, the rest
read, where the Symphony runs with the file as it is. A file that does not load is run as the plain
project: the server serves the project with the defaults, and every page of it says so and why,
until it loads. The board reads its own keys (`[board]`'s
commands and thresholds, `[limits]`, `[agents] launch`, `[agents.models]` and `[stamps]`) from the
main checkout's copy, so a branch cannot change its own. A path is relative to the project's root
and stays inside it, unless its key says otherwise.

```toml
[project]
name = "Lamp Shop"

[agents]
launch = { default = "claude", model = "opus" }
```

This page lists every key the Symphony reads (`crates/symphony/src/project/file.rs`, `board.rs`
and `theme.rs`, a generator's in `crates/tasks/src/land.rs` and a stamp's in
`crates/tasks/src/stamps.rs`): a test,
`crates/symphony/tests/project/reference.rs`, fails on a key this page lacks or one the code does
not have.

## The settings check

`symphony settings-check [DIR]` (`just symphony-cli settings-check`) reads the project file of DIR,
the current folder by default, as the Symphony loads it, and judges its shape alone: a table or
key the Symphony does not know, a value of the wrong type (a number where a text is read) and a
required value left out (an entrypoint's `url` or `folder`) are each named on stderr, exit 1; a
file that loads is exit 0. It judges no name against a list: a model, an agent or a recipe is any
text, so a model nobody has heard of passes. Unlike the lint it reads no document and asks no
build which plugins it holds. A role or launch fallback model recognizable as the other agent's warns on
stderr with its key and line, without changing the check's exit code.

[The local settings file](#the-local-settings-file) is judged with it: a setting it may not hold,
a value the board cannot read there, or the file tracked by git, is named on stderr, exit 1.

A change of the values a project file gives settings the Symphony already reads lands on this
check green at its tip, recorded as a test run, with no reviewer
([TASKS](TASKS.md#needed-stamps)). This repository records it with `just settings-check`; a
project records it through its own build wrapper
([MACHINE_WORK](MACHINE_WORK.md#recording-builds-and-tests)).

## The local settings file

`symphony.local.toml`, beside the main checkout's `symphony.toml`, holds one machine's or one
person's preferences. Git does not version it: `symphony init` adds it to the project's
`.gitignore`, and the settings check and the lint refuse one git tracks. It is read after the
project file: a key it writes holds in the committed one's place, a key it leaves out keeps the
committed one, and with no such file the project file is read alone. A linked worktree has none of
its own and reads the main checkout's, as it reads the main checkout's project file for these
settings.

```toml
# symphony.local.toml
[agents.models]
executor = "fable"

[limits]
machine_agents = 4
```

It holds preferences alone, in the same tables, keys and types as the project file:

- `[agents.launch]`: the agent a launch starts, its model and its effort;
- `[agents.models]`: the model each role runs on;
- `[limits]`: every key of it.

A setting that holds a command or a program the Symphony runs, or that decides what lands, is never
read from it and is refused there by name: `[project]`'s `cargo` and `default_branch`, and all of
`[plugins]`, `[verdicts]`, `[land]`, `[releases]` and `[stamps]`, and `[board]`'s `on_review`,
`on_land`, `machine_work_reader`, `postgresql` and `assessments`. A file no commit records must
not change the landing gate or run a command: that would be a way around review that leaves no
trace. Every other table or key (the rest of `[project]`, `[board]` and `[agents]`, `[theme]`,
`[[entrypoints]]`, `[workflow]`, or a name misspelt) is refused by name too, as no setting a local
file overrides.

Whatever is wrong with it is said with the file and the line: a refused setting, a key of one of
its three tables the Symphony does not know, a value of the wrong type or below its least, an agent
a launch does not start, or text that is no TOML. None of the file is then read and the committed
settings hold, so a broken local file stops neither the board nor a launch: every board command
says it on stderr, as it says a limit it cannot read, and the settings check exits 1 on it.

One reader reads it (`crates/tasks/src/local.rs`), which the board's limits and launches, the
Symphony's load of the project and the settings check all take, so each takes the same override;
a board or a server kept open reads both files again when the length or time of either changes,
with no restart. `just tasks limits` names the file under the project file and, for each setting it
overrides, the file and the line (`symphony.local.toml [limits] machine_agents, line 6`).
`just symphony-cli model ROLE MODEL --local` writes a role's model there instead of in the project
file, and with `all` and `--agent` the launch agent and its model too, making the file when it is
not there; nothing is landed for it.

## `[workflow]`

This table is a strict **candidate format**, available through
`symphony::project::workflow::candidate`. Runtime project loading refuses any supplied workflow
with `unsupported_activation`: the authoritative storage/admission engine is not available.
Omission retains the current board behavior and selects built-in definition v1 in the candidate
reader. Candidate parsing never changes a board. The [workflow reference](WORKFLOW.md) documents
every nested field, validation and the remaining activation work.

All fields below are required when the candidate table is present; lists may be empty only
where validation allows it. Unknown fields and variants refuse the candidate.

| Key | Default | What it sets |
| --- | --- | --- |
| `format` | required | definition format version, currently `1` |
| `initial` | required | initial `{ phase, state }` pair |
| `task_kinds` | required | nonempty list of supported stable task-kind IDs |
| `phases` | required | phase IDs, labels, display/phone order and column membership |
| `states` | required | coordination-state IDs and labels |
| `positions` | required | valid pairs and their staffing, slot, dependency, outcome and recovery declarations |
| `edges` | required | typed routes, authority requirements, inputs, guards and effects |
| `capabilities` | required | declared host capability IDs; declarations grant no authority |
| `adapters` | required | declared host adapter IDs; declarations supply no adapter |
| `guards` | required | opaque policy requirements by stable ID and adapter |

## `[project]`

| Key | Default | What it sets |
| --- | --- | --- |
| `name` | the folder's name | the name the pages show |
| `id` | `name` made a slug (lowercase letters, digits and dashes), refused when it makes none; with no `name` written, the folder's name made one, `project` when that makes none | the project's segment on the machine's server (`/<id>/`) and its data folder |
| `register` | `DECISIONS.md` in the first entrypoint's folder | the decision register |
| `artifacts` | `artifacts` | the folder of served files and the Symphony's private files |
| `default_branch` | `main` | the branch tasks land on |
| `cargo` | `tools/cargo.sh` in the Symphony's own workspace, `cargo` elsewhere | the command that runs Cargo when the machine's server rebuilds itself from this checkout |

## `[[entrypoints]]`

Each table serves one folder at one URL. With none written, one keeps the convention: `docs/` at
`/docs/`, Markdown, `docs/history` listed but never read as documents. A key not listed here
refuses the file, as a misspelt one would change what is served.

| Key | Default | What it sets |
| --- | --- | --- |
| `url` | (required) | the route it is served at, such as `"/docs/"`, or `"/"` |
| `folder` | (required) | the folder it reads; `"."` is the project's root |
| `include` | `["**"]` | the files shown, as patterns over their path in the folder |
| `exclude` | `[]` | the files left out of those |
| `types` | `["markdown", "html"]` | the renderers that show its files, the first that takes a file's extension drawing it: `markdown`, `html` (HTML documents, below), `media` (pictures, and `.html` files listed by name) and the plugins' |
| `history` | `[]` | folders inside it served and listed but never read as documents |

**HTML documents.** An `.html` or `.htm` file the `html` renderer shows is a document beside the
Markdown ones: listed in its folder by its `<title>` (else its name), counted, and opened at its
route inside the app's page, under its header. The page draws the file as written, as wide as the
page and as tall as the window below the header, scrolling inside itself, in the app's light or
dark as the file's own styles follow them. It runs in a sandboxed frame whose scripts run in an
origin of their own, never the app's: the app's storage, cookies, feed, API and page are out of
its reach, and it cannot navigate the app away, submit a form or open a window, so a link in it
opens in the frame or not at all. The files beside it (stylesheets, scripts, pictures) load from
under its entrypoint's address and from nowhere else, and it opens no connection, a fetch of its
own files among them. For an entrypoint at the project's root that address is the whole project's,
the app's own files and answers among it, and the page fetches none of them, so no answer of the
app's reaches it as data. What it can run or apply from there, a script running in the page's own
origin, is the project's own files and the client's static assets: the app's JSON answers and its
feed are all sent with `X-Content-Type-Options: nosniff`, so the browser refuses each as a script
or a stylesheet, whether its route answered or not. Its route is the app's page, however it
is asked for; the frame loads the file at the route's own address with `?raw` added, and the file
is only ever served under that same sandbox, so no address of it runs with the app's origin. An
HTML document has none of a Markdown
document's outline, sections, cards, verdicts, notes, editing, versions or search. An entrypoint
whose `types` name `media` before `html`, or `media` without it, keeps listing its `.html` files
by name among its pictures; opened, each is drawn in the same frame. One whose `types` name
neither serves no `.html` file, as its patterns serve none they leave out.

## `[plugins]`

| Key | Default | What it sets |
| --- | --- | --- |
| `crates` | `[]` | the project's plugins, by crate name: a host linking them, or a process of their own ([PLUGIN_PROTOCOL](PLUGIN_PROTOCOL.md)) |
| `process` | none | `[plugins.process]`: a plugin's name, then the command that starts its process, run in the project's root |
| `timing` | `false` | record each plugin call's time ([PLUGIN_CALLS](PLUGIN_CALLS.md)) |
| `loopback` | `false` | send each call through a JSON round trip inside the process, and record both times |

## `[theme]`

| Key | Default | What it sets |
| --- | --- | --- |
| `package` | the Symphony's own | a theme package a selected plugin brings, by name |
| `name` | the package's, else the project's | the name in the tab, and in the header once the theme has a name or an icon of its own; with neither, the header says Symphony |
| `icon` | the package's, else the Symphony's mark, the fermata | the icon of the header and the tab |
| `stylesheet` | none | a stylesheet served after the package's |
| `tokens` | none | `[theme.tokens]`: the package's tokens set over, by name (`primary = "#a33"`) |
| `layouts` | none | `[theme.layouts]`: a variant for each region, `header` (`default`, `compact`) and `home` (`default`, `focus`) |

## `[agents]`

| Key | Default | What it sets |
| --- | --- | --- |
| `transports` | `["claude-uds", "codex-queue", "inbox"]` | the agent transports the project names; read and checked, acted on by nothing of the Symphony's own |
| `panes` | `"none"` | what agents' terminals run in: `"hc"` (Herdr), whose panes the pages can show, or `"none"` |
| `worktrees` | none: `symphony/tasks/land/` in the git common dir | the folder of a land's and the regenerator's worktrees; it may lie outside the project |
| `launch` | below | the table `[agents.launch]` |
| `models` | below | the table `[agents.models]` |

## `[agents.launch]`

What `just tasks launch` and the staffing pass start when the caller names nothing; `just tasks
limits` prints what holds and where it came from.

| Key | Default | What it sets |
| --- | --- | --- |
| `default` | `"codex"` | the agent a launch starts: `"codex"` or `"claude"` |
| `model` | the board's own for that agent | its model |
| `effort` | the board's own for that agent | its effort |

## `[agents.models]`

`just symphony-cli model ROLE MODEL` sets one role's model in the main checkout's file;
`all` sets all four. With `all`, `--agent codex|claude` also sets `[agents.launch] default` and its
fallback `model`; `--assessor MODEL` sets a separate assessor model. For example,
`just symphony-cli model all fable --agent claude --assessor haiku` switches to Claude on Fable
for the orchestrator, executor and reviewer, and Haiku for the assessor, in one edit.

The command preserves comments and order, adds missing keys, accepts any string as the model
and refuses unknown roles and agents. The model command and settings check warn on stderr for a
role or launch fallback model recognizable as the other agent's: `gpt-` names Codex; `claude-`, `fable`, `opus`,
`sonnet` and `haiku` name Claude. The warning names the key and line and never refuses the file;
unrecognized model text is accepted without warning. Checking and landing follow the project's
settings-only policy. `just tasks limits` prints the launch agent, resulting models and their
sources. A resumed agent keeps the model it started on.

With `--local`, alone or with the other two, the same edit goes to
[the local settings file](#the-local-settings-file) instead, which overrides these on one machine
and lands nothing. The warning is judged on the settings in force, the local file's over the
project file's, by the model command and the settings check alike: it names the file, key and line
that supply the model, and the file that names the agent when it is the other one.

| Key | Default | What it sets |
| --- | --- | --- |
| `orchestrator` | none: whoever starts one chooses | the model an orchestrator runs on |
| `executor` | `[agents.launch] model`, else the board's own | the model an executor runs on |
| `reviewer` | `[agents.launch] model`, else the board's own | the model a reviewer runs on |
| `assessor` | the smallest model the agent offers (Claude's `haiku`); for Codex, whose smallest the board does not name, `[agents.launch] model`, else the board's own | the model an assessor runs on, which names the stamps a task needs ([TASKS](TASKS.md#needed-stamps)) |

## `[verdicts]`

The owner's edits commit straight to the default branch unless a generator changes an output;
then they go on a branch with the outputs, checked and landed.

| Key | Default | What it sets |
| --- | --- | --- |
| `check` | `[]`: none | the command a generated branch passes before it lands, and a land's check (`[land] check`) |
| `land` | `[]` | the command that lands a generated branch, its name appended; needed once `generators` names any |
| `generators` | `[]` | the generators run on the owner's edits, each a table as below |
| `record` | none: no record | where a verdict commit's record goes, for the project's landing hook; under `.git/`, the git common dir |

## `[land]`

What the default landing, `just land SLUG`, does besides its merge ([TASKS](TASKS.md#the-default-landing)).

| Key | Default | What it sets |
| --- | --- | --- |
| `check` | `true` | whether a land runs `[verdicts] check` on its merge |
| `push` | `true` | whether a land pushes the default branch to `origin` |
| `timeout_seconds` | `3600` | how long each generator and the check may run |
| `generators` | `[]` | what the regenerator a land starts makes of the default branch's newest tip, each a table as below ([TASKS](TASKS.md#the-regenerator)) |
| `keys` | none | a generator's name, then the command printing its key: a tip whose key passed before runs nothing |
| `check_paths` | the list in [TASKS](TASKS.md#the-default-landing) | what holds the project's tests and checks, which a red's fix its orchestrator approves may not change |

## `[[land.generators]]`

A generator, in `[land] generators` or `[verdicts] generators`, usually written inline
(`{ name = "…", outputs = "…", command = […] }`). A key not listed here refuses the file.

| Key | Default | What it sets |
| --- | --- | --- |
| `name` | (required) | its name, in its key and in what the regenerator says |
| `outputs` | (required) | the path it writes, a folder ending in `/`; a land keeps the default branch's side of it |
| `command` | (required) | what runs it, in the project's root |
| `when_changed` | `false` | run only after an earlier generator changed something |

## `[board]`

| Key | Default | What it sets |
| --- | --- | --- |
| `guide` | `TASKS.md` | the board's guide: the links and pictures in task messages resolve from it |
| `on_review` | `[]`: none | the command started, detached from the main checkout, when a task moves to review, with its slug, branch and tip appended |
| `on_land` | `[]`: none | the command started once `just land` closed a task, with its slug and merge appended |
| `orchestrator_slots` | none: no cap | the open tasks an orchestrator holds before the board refuses it new work; `[limits]` wins |
| `review_alarm_ratio` | `2` | review over this many times the work in progress raises the pipeline alarm ([TASKS](TASKS.md#the-pipeline-alarm)) |
| `review_alarm_hours` | `4` | the hours review must stay over the ratio to raise it |
| `review_alarm_clear_hours` | `1` | the hours review must stay at or under it to clear it |
| `swept_cap_gb` | `5` | the gigabytes the evidence kept of removed worktrees may hold before the sweep asks for a check by hand |
| `postgresql` | discover `postgres` on `PATH` | an explicit absolute folder of the machine's PostgreSQL programs takes precedence over discovery; only a scratch board's server starts it so far ([DECISIONS](DECISIONS.md#postgresql-binary-source)) |
| `machine_wip` | none | `[board.machine_wip.MACHINE]`, advice per machine, as below |
| `machine_work_reader` | none | the project's own machine-work reader, an executable the server runs ([MACHINE_WORK](MACHINE_WORK.md)) |
| `slot_locks` | none | the folder of the project's test-slot locks, which the agents view probes; under `.git/`, the git common dir |
| `assessments` | none: an assessment is refused | the folder of the project's landing assessments; under `.git/`, the git common dir |
| `export` | none | read and checked, acted on by nothing of the Symphony's own (`just tasks export` takes `--out`) |
| `disposable` | `[]` | read and checked, acted on by nothing of the Symphony's own (the sweep keeps its own list) |

PostgreSQL setup requires version 18 or later, installed by the machine's package manager.
Without an explicit `postgresql`, discovery takes the first executable `postgres` on `PATH`;
`initdb`, `pg_ctl` and `psql` must be executable in the same folder. On a Mac with Homebrew:

```sh
brew install postgresql@18
export PATH="$(brew --prefix postgresql@18)/bin:$PATH"
```

For another installation, put its `bin` folder on `PATH` or set `[board] postgresql` to its
absolute path. An explicit value always wins, including when it is invalid. A missing tool,
failed version probe or server older than 18 fails with setup guidance; discovery never skips
an incompatible installation to select another. It inspects programs only, starting no database;
only scratch boards use PostgreSQL so far.

## `[board.machine_wip.MACHINE]`

Advice for one machine, by the installation ID the board's Advisory targets show; nothing
enforces it, and zero is advice too.

| Key | Default | What it sets |
| --- | --- | --- |
| `build` | none | the builds to run at once there |
| `test` | none | the tests to run at once there |

## `[limits]`

Read by the board itself ([TASKS](TASKS.md#limits-and-models)); a value it cannot read takes its
default and is said on stderr. [The local settings file](#the-local-settings-file) overrides each
key on one machine.

| Key | Default | What it sets |
| --- | --- | --- |
| `orchestrator_slots` | `[board] orchestrator_slots`, else none | the tasks in progress an orchestrator holds |
| `orchestrator_tasks` | `16` | the open tasks an orchestrator holds the orchestrator role on, at which the board gives it no more and its take or a hand-over to it is refused, a red apart ([TASKS](TASKS.md#the-limit-on-the-tasks-an-orchestrator-holds)); at least `1` |
| `low_volume` | `4` | PULL's low-volume line below this many open slots; `0` says it to none |
| `machine_agents` | none: no limit | the agents this machine runs, past which a launch is refused |

## `[stamps]`

The stamps the project's land can require, a table each ([TASKS](TASKS.md#needed-stamps)). Read by
the board itself from the main checkout's committed copy (`HEAD:symphony.toml`), so neither a
branch nor an edit not yet committed changes what a task is held to. With no `[stamps]` the
catalogue is the Symphony's four; a project that writes any writes its whole list, so a stamp it
leaves out is one its land never requires, and a `[stamps]` naming none is refused. A stamp's name
is how the land knows it stands, so a name the Symphony has no reader for is refused by name and
line. `just tasks stamps` prints the catalogue as it holds.

| Key | Default | What it sets |
| --- | --- | --- |
| `tests` | listed while no `[stamps]` is written | `[stamps.tests]`: met by the task's recorded runs at the tip, green and one test run among them |
| `review` | listed while no `[stamps]` is written | `[stamps.review]`: met by a fresh reviewer's approve naming the tip |
| `pictures` | listed while no `[stamps]` is written | `[stamps.pictures]`: met by a post on the task's thread naming the tip with a picture |
| `owner` | listed while no `[stamps]` is written | `[stamps.owner]`: met by the owner's own approve naming the tip |

## `[stamps.NAME]`

One stamp of the catalogue, by its name, usually written as its own table (`[stamps.review]`). A key
left out, or a text written empty, is the Symphony's own for that stamp. How the land knows a stamp
stands is its reader's, which no key sets. A key not listed here refuses the catalogue.

| Key | Default | What it sets |
| --- | --- | --- |
| `default` | `true` for `review`, `false` for the rest | whether a task no assessment has read needs it |
| `applies` | the Symphony's own | where an assessor needs it, as `just tasks stamps` prints it |
| `skips` | the Symphony's own | where an assessor leaves it out |

## `[releases]`

Read only by the release run of the Symphony's own repository ([RELEASES](RELEASES.md)), which no
other project runs.

| Key | Default | What it sets |
| --- | --- | --- |
| `every_landings` | `20` | a release run starts once this many tasks have landed; `0` starts none |
| `refresh` | `true` | whether a green release refreshes the machine |
| `publish` | `false` | whether a green release, once pushed, publishes on crates.io each package whose version it lacks ([RELEASES](RELEASES.md#packages-on-cratesio)) |
| `candidate_reruns` | `3` | the red reruns a release candidate takes before it is dropped |
| `candidate_hours` | `24` | the hours a release candidate is held before it is dropped |
