# The project file

A project's settings are its `symphony.toml`, committed at the root of its main checkout. Every
table and key is optional: a key left out takes the default below, and an empty file, or none, is
a plain project of every default. A table or key the Symphony does not know is refused by name and
line where the project is loaded or linted (`just symphony-status --lint`), and left out, the rest
read, where the Symphony runs with the file as it is. A file that does not load is run as the plain
project: the server serves the project with the defaults, and every page of it says so and why,
until it loads. Three keys nothing acted on are gone (`[agents] transports`, `[board] export` and
`[board] disposable`), each refused as any unknown key is. The board reads its own keys (`[board]`'s
commands and thresholds, `[limits]`, `[agents] launch`, `[agents.models]` and `[stamps]`) from the
main checkout's copy, so a branch cannot change its own. A path is relative to the project's root
and stays inside it, unless its key says otherwise.

```toml
[project]
name = "Lamp Shop"

[agents]
launch = { default = "claude", model = "opus" }
```

## A first file

A new project needs none of this page to start. With no `symphony.toml` the project is named
after its folder, its documents are `docs/` served at `/<id>/docs/`, its register is
`docs/DECISIONS.md`, tasks land on `main` by the default landing, and a launch starts Codex. A
first file usually says three things: the project's name and the segment of its address, and the
agent its sessions are.

```toml
[project]
name = "Lamp Shop"
id = "lamp-shop"

[agents]
launch = { default = "claude" }
```

`just symphony-cli settings-check` checks it ([below](#the-settings-check)). The tables a project
reaches for next are [`[[entrypoints]]`](#entrypoints) (which folders are served),
[`[agents.models]`](#agentsmodels) (the model each role runs on), [`[limits]`](#limits) (how much
runs at once) and [`[verdicts]`](#verdicts) with [`[land]`](#land) (what a landing checks).
[`[workflow]`](#workflow), [`[board.external_postgresql]`](#boardexternal_postgresql) and
[`[releases]`](#releases) are for a custom delivery workflow, a hosted board and the Symphony's own
release run: most projects never write them.

A command a key holds (`on_review`, `on_land`, `[verdicts] check`, a generator's `command`) is a
list of words, `["just", "check"]`, never one line of shell.

The rest of this page lists every key the Symphony reads (`crates/symphony/src/project/file.rs`, `board.rs`
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
([TASKS](../workflow/TASKS.md#needed-stamps)). This repository records it with `just settings-check`; a
project records it through its own build wrapper
([MACHINE_WORK](../workflow/MACHINE_WORK.md#recording-builds-and-tests)).

## The local settings file

`symphony.local.toml`, beside the main checkout's `symphony.toml`, holds one machine's or one
person's preferences. Git does not version it: `symphony init` adds it to the project's
`.gitignore`, and the settings check and the lint refuse one git tracks. It is read after the
project file and [the board's store](#the-boards-store): a key it writes holds in the place of
both, a key it leaves out keeps what they say, and with no such file they are read alone. A linked worktree has none of
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
`on_land`, `machine_work_reader`, `postgresql`, `external_postgresql` and `assessments`. A file no commit records must
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

## The board's store

The board keeps a project's preferences too, in a table of its own
([TASKS](../workflow/TASKS.md#where-it-lives)): what the project was told on its board and no file holds. A
setting is read in this order, each place over the one before:

1. the Symphony's default;
2. the committed project file;
3. the board's store;
4. [the local settings file](#the-local-settings-file).

A board that was never told anything has no entry, so it reads as the files alone. A stored value
holds at once, from the next read of the settings, and waits for no commit and no landing; it
stands over a later commit of the same key, as it is read after the file, until its entry is
removed. The local file stays the last word on its machine.

An entry is keyed by its setting, the tables and the key the project file names it by, joined by
dots: `limits.low_volume`, `agents.launch.model`, `agents.models.reviewer`, `agents.panes`. It
keeps every change made to it, not only the last: the value before, the value after, when, and the
session or page that wrote it. A removed entry holds no value and keeps its record, and the
files' value is in force again.

It holds preferences alone:

- `[agents.launch]`: the agent a launch starts, its model and its effort;
- `[agents.models]`: the model each role runs on;
- `[agents]`'s `panes`;
- `[limits]`: every key of it.

What the local file is refused, the store is refused, by the same rules and the same two lists
(`PROTECTED` and `PROTECTED_KEYS` in `crates/tasks/src/local.rs`): a setting that holds a command
or a program the Symphony runs, or that decides what lands, is never kept there, as a place no
commit records must not change the landing gate or run a command. A value is judged when it is
written, since no landing passes over it: its type, a limit's least, an agent a launch starts,
one of `panes`' two values. A refusal names the key and why, and nothing is written. Each entry
is judged again when it is read, alone: one the store may not hold, however its row came to be
in the table, is said on stderr and never read, and the others hold.

```sh
just tasks settings set limits.machine_agents 4 --as HANDLE
just tasks settings remove limits.machine_agents --as HANDLE
just tasks settings list
```

`list` prints each entry with its record, and `--json` the same as JSON. The store's calls are
the board's own (`Store::store_setting`, `Store::remove_setting`, `Store::stored_entries`,
`crates/tasks/src/store/stored_settings.rs`). The board's reader of the settings reads it where it
reads the local file (`crates/tasks/src/settings.rs`, with `limits.rs` and
`launch/defaults.rs`), so the limits a command is held to and the agent and model a launch starts
agree, and `just tasks limits` names the store for each value it gives, with the committed value
that one stands over (`the board's store [limits] machine_agents, over 12 ([limits]
machine_agents, line 9)`). The Symphony's own load of the project file reads the files alone:
`panes`, which only that load reads, is kept, judged and listed here and is a row of `just tasks
limits` once stored.

The table comes with an additive step of the board's schema
([CHECKS](../workflow/CHECKS.md#the-board-is-a-contract)): a build without the step reads every setting from
the files, so a stored value holds for the builds that have it. A move of the board to another
engine carries the entries.

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
`/docs/`, Markdown and HTML, `docs/history` listed but never read as documents. A key not listed here
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
| `crates` | `[]` | the project's plugins, each by the name its plugin declares: a host linking them, or a process of their own ([PLUGIN_PROTOCOL](../engineering/PLUGIN_PROTOCOL.md)) |
| `process` | none | `[plugins.process]`: a plugin's name, then the command that starts its process, run in the project's root |
| `timing` | `false` | record each plugin call's time ([PLUGIN_CALLS](../engineering/PLUGIN_CALLS.md)) |
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

The tokens by name, with what each colours, and a whole example with and without a plugin are in
[Writing a plugin and a theme](../engineering/PLUGIN_WRITING.md#a-theme).

## `[agents]`

| Key | Default | What it sets |
| --- | --- | --- |
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
| `assessor` | the smallest model the agent offers (Claude's `haiku`); for Codex, whose smallest the board does not name, `[agents.launch] model`, else the board's own | the model an assessor runs on, which names the stamps a task needs ([TASKS](../workflow/TASKS.md#needed-stamps)) |

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

What the default landing, `just land SLUG`, does besides its merge ([TASKS](../workflow/TASKS.md#the-default-landing)).

| Key | Default | What it sets |
| --- | --- | --- |
| `check` | `true` | whether a land runs `[verdicts] check` on its merge |
| `push` | `true` | whether a land pushes the default branch to `origin` |
| `timeout_seconds` | `3600` | how long each generator and the check may run |
| `generators` | `[]` | what the regenerator a land starts makes of the default branch's newest tip, each a table as below ([TASKS](../workflow/TASKS.md#the-regenerator)) |
| `keys` | none | a generator's name, then the command printing its key: a tip whose key passed before runs nothing |
| `check_paths` | the list in [TASKS](../workflow/TASKS.md#the-default-landing) | what holds the project's tests and checks, which a red's fix its orchestrator approves may not change |
| `delivery` | none: the land merges and pushes here | the command that delivers a task by a hosted pull request in place of that merge and push, a list of words run as `COMMAND VERB --request FILE` ([TASKS](../workflow/TASKS.md#a-land-delivered-by-a-pull-request)); it decides what lands, so a change of it is reviewed |
| `watch_seconds` | `30` | how long the delivery watcher rests between two rounds of reading what the host made of each armed pull request ([TASKS](../workflow/TASKS.md#a-land-dispatched-at-review)); `0` is refused |
| `merge_within_minutes` | `30` | how long an armed pull request whose evidence stands may stay neither merged nor red before the watcher turns its merge off and says so; `0` is refused |
| `upstream` | none: no import is delivered | `[land.upstream]`, the repository this one imports from, as below |

## `[land.upstream]`

The upstream a downstream project imports from ([UPSTREAM_DOWNSTREAM](UPSTREAM_DOWNSTREAM.md)):
where a land delivered by a pull request fetches it for every head, to judge whether the head
imports it, and the one history such a head may bring in, merged by a merge commit rather than
squashed ([TASKS](../workflow/TASKS.md#an-import-delivered-by-a-pull-request)). An address that does not
answer refuses every such land, a task's as well. With none, no import is delivered and a head
holding another history is refused. Both keys are needed, and no other is read; it decides what
lands, so a change of it is reviewed.

| Key | What it sets |
| --- | --- |
| `url` | the upstream's address, as `git fetch` takes it; never one starting with `-` |
| `branch` | the upstream's default branch, a plain branch name, which an import's commit must be on |

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
| `orchestrator_slots` | none: no cap | the tasks in progress an orchestrator holds before the board refuses it new work; `[limits]` wins |
| `review_alarm_ratio` | `2` | review over this many times the work in progress raises the pipeline alarm ([TASKS](../workflow/TASKS.md#the-pipeline-alarm)) |
| `review_alarm_hours` | `4` | the hours review must stay over the ratio to raise it |
| `review_alarm_clear_hours` | `1` | the hours review must stay at or under it to clear it |
| `swept_cap_gb` | `5` | the gigabytes the evidence kept of removed worktrees may hold before the sweep asks for a check by hand |
| `stale_writer_hours` | `24` | the hours a build that wrote the board, on this machine, may go without running before it holds no move of the board to a release, unless it is a build the project runs now: one a link in its main checkout's `.target/bin` names ([RELEASES](../workflow/RELEASES.md#the-refresh)); a whole number over 0 |
| `postgresql` | discover `postgres` on `PATH` | an explicit absolute folder of the machine's PostgreSQL programs takes precedence over discovery; the server of a board whose home names PostgreSQL starts it ([DECISIONS](../DECISIONS.md#postgresql-binary-source)) |
| `external_postgresql` | none: the board is the machine's own | `[board.external_postgresql]`, where a hosted board's PostgreSQL is and the budget of its connections, as below |
| `machine_wip` | none | `[board.machine_wip.MACHINE]`, advice per machine, as below |
| `machine_work_reader` | none | the project's own machine-work reader, an executable the server runs ([MACHINE_WORK](../workflow/MACHINE_WORK.md)) |
| `slot_locks` | none | the folder of the project's test-slot locks, which the agents view probes; under `.git/`, the git common dir |
| `assessments` | none: an assessment is refused | the folder of the project's landing assessments; under `.git/`, the git common dir |

A project's board needs no PostgreSQL: it runs on the SurrealDB `symphony init` fetches
([getting started](GETTING_STARTED.md#2-set-the-project-up)). Only a board whose home names
PostgreSQL uses what follows. Its setup requires version 18 or later, installed by the machine's
package manager. Without an explicit `postgresql`, discovery takes the first executable `postgres` on `PATH`;
`initdb`, `pg_ctl` and `psql` must be executable in the same folder. On a Mac with Homebrew:

```sh
brew install postgresql@18
export PATH="$(brew --prefix postgresql@18)/bin:$PATH"
```

For another installation, put its `bin` folder on `PATH` or set `[board] postgresql` to its
absolute path. An explicit value always wins, including when it is invalid. A missing tool,
failed version probe or server older than 18 fails with setup guidance; discovery never skips
an incompatible installation to select another. It inspects programs only, starting no database:
a board uses PostgreSQL once its home names it ([POSTGRESQL](POSTGRESQL.md)).

## `[board.external_postgresql]`

Not usable yet: no command reaches a hosted board. Where a hosted board's PostgreSQL is, for a build with the `hosted-postgresql` feature
([POSTGRESQL](POSTGRESQL.md#a-hosted-board)); every build reads and judges the table, so the
settings check says the same anywhere. It holds no credential: each `_env` key is the name of a
variable of the process's environment, never a value, and a URL, a password or a key written
here is a key the table does not have and is refused. No command reaches a hosted board yet:
the table is read by the library's hosted opening alone.

| Key | Default | What it sets |
| --- | --- | --- |
| `url_env` | one of the two | the variable holding the PostgreSQL URI: one host, the project's own database and login, a password; TLS is always verified, whatever it asks |
| `url_file_env` | one of the two | the variable holding the path of a mounted file of that URI |
| `ca_file_env` | required | the variable holding the path of the certificate authority's PEM file: the only authority trusted |
| `client_cert_file_env` | none | the variable holding the path of a client certificate, where the endpoint asks for one; with `client_key_file_env` or not at all |
| `client_key_file_env` | none | the variable holding the path of that certificate's key |
| `class_env` | required | the variable holding this process's class, one of the budget's |
| `connect_timeout_seconds` | `10` | how long a connection may take to be made |
| `acquire_timeout_seconds` | `10` | how long a request waits for a connection of the pool |
| `request_timeout_seconds` | `300` | how long a request may take once it has its connection |
| `queue_max` | `16` | the requests that may wait for a connection at once; one more is refused at once |
| `budget` | required | `[board.external_postgresql.budget]`, the deployment's connections, as below |

## `[board.external_postgresql.budget]`

The deployment's whole use of the login role, counted for every process that can be alive at
once. A hosted board's table without it is refused.

| Key | Default | What it sets |
| --- | --- | --- |
| `role_connection_cap` | required | the connections the platform holds the role to |
| `other_users` | `0` | the role's connections that are not the board's |
| `maintenance` | required, at least 1 | the connections of a migration or an export, the class `maintenance` |
| `headroom` | required | the connections left free |
| `classes` | required, at least one | `[board.external_postgresql.budget.classes.CLASS]`, a class of process by its name, as below |

Every class's `processes` times its `pool_max`, with `other_users`, `maintenance` and
`headroom`, is at most `role_connection_cap`, or the table is refused. Each connection takes
one of its class's slots in the database, so a class never passes its total whatever each
process counts.

## `[board.external_postgresql.budget.classes.CLASS]`

One class of process, named in lower-case letters, digits, `-` and `_`; `maintenance` is the
budget's own and no class's name.

| Key | Default | What it sets |
| --- | --- | --- |
| `processes` | required, at least 1 | the processes of the class that can be alive at once: old and new revisions during a rollout, draining instances, each process of an instance |
| `pool_max` | required, at least 1 | the connections one process of the class holds at most: its pool |

## `[board.machine_wip.MACHINE]`

Advice for one machine, by the installation ID the board's Advisory targets show; nothing
enforces it, and zero is advice too.

| Key | Default | What it sets |
| --- | --- | --- |
| `build` | none | the builds to run at once there |
| `test` | none | the tests to run at once there |

## `[limits]`

Read by the board itself ([TASKS](../workflow/TASKS.md#limits-and-models)); a value it cannot read takes its
default and is said on stderr. [The local settings file](#the-local-settings-file) overrides each
key on one machine.

| Key | Default | What it sets |
| --- | --- | --- |
| `orchestrator_slots` | `[board] orchestrator_slots`, else none | the tasks in progress an orchestrator holds |
| `orchestrator_tasks` | `16` | the open tasks an orchestrator holds the orchestrator role on, at which the board gives it no more and its take or a hand-over to it is refused, a red apart ([TASKS](../workflow/TASKS.md#the-limit-on-the-tasks-an-orchestrator-holds)); at least `1` |
| `low_volume` | `4` | PULL's low-volume line below this many open slots; `0` says it to none |
| `machine_agents` | none: no limit | the agents this machine runs, past which a launch is refused |

## `[stamps]`

The stamps the project's land can require, a table each ([TASKS](../workflow/TASKS.md#needed-stamps)). Read by
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
| `waivable` | `true` for `pictures`, `false` for the rest | whether a gate decision may waive it for one tip ([TASKS](../workflow/TASKS.md#gate-decisions)); `true` on `tests`, `review` or `owner` refuses the catalogue, as they are never waived |

## `[workflow]`

Advanced: a project that keeps the board's own workflow writes none of the three workflow tables.
This table is a project's custom delivery workflow, strict, read by
`symphony::project::workflow::candidate`. Project loading accepts one the local host supports and
refuses any other with `unsupported_activation`, naming what it needs: a capability no grant of
the host names, a guard no committed approval authority attests
([below](#workflow_authorities)) or a route around one, an adapter, or what durable attempts,
landing adapters or accepted outputs bring. The board activates the workflow the project's default branch commits, with its
`[[workflow_mappings]]`, and moves its tasks by its edges alone (`just tasks workflow`).
Omission retains the current board behavior and selects built-in definition v1 in the candidate
reader. Candidate parsing never changes a board. The [workflow reference](../workflow/WORKFLOW.md) documents
every nested field, validation, activation and admission.

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
| `guards` | required | policy requirements by stable ID and the adapter whose approval authority attests them |

## `[[workflow_mappings]]`

Where a task goes when a newly committed `[workflow]` no longer declares the pair it stands at
(or the pair it resumes to when blocked). Each entry names a removed pair and a pair the workflow
declares, and no pair is mapped twice; a pair the workflow still declares is never remapped, as an
ID keeps its meaning (rename it instead). A task the board's existing writers kept starts from
built-in v1's pair of its status ([WORKFLOW](../workflow/WORKFLOW.md#activation-and-mappings)).

Mappings in a file with no `[workflow]` map onto built-in v1, the board's own writers: each
names a pair v1 does not declare and one it does. They say where each task goes when a project
that ran a custom workflow drops it ([WORKFLOW](../workflow/WORKFLOW.md#returning-to-the-boards-own-writers)),
and a board with no workflow active reads them and does nothing.

| Key | Default | What it sets |
| --- | --- | --- |
| `from` | required | the removed `{ phase, state }` pair |
| `to` | required | the declared `{ phase, state }` pair a task there goes to |

## `[[workflow_authorities]]`

The approval authorities whose signed word meets the workflow's guards, one for each adapter its
guards name ([WORKFLOW](../workflow/WORKFLOW.md#guards)). A guard is a decision the project keeps in its own
approval ledger; the board never holds that ledger and never decides a guard. What it asks of a
project, in plain words:

- **It holds a signing key.** The project runs an authority over its ledger that holds an
  Ed25519 signing key and signs, with it, each observation of the ledger's state and each
  attestation that a guard holds for one operation. The secret half stays with that authority:
  never in the repository, the board or a request.
- **It commits the public half here**, as `ed25519:` and the key's 32 bytes in lowercase hex,
  with the adapter it answers for and the issuer it signs as. The board verifies every
  observation and attestation by the key the default branch commits, and by nothing else.
- **A changed key is a new generation.** Changing a key, an issuer or an authority is a commit of
  this file, which activates a new generation of the workflow: every attestation made for the
  previous one stops at once, and the new key goes on from the ledger state the board last
  accepted.
- **It names itself by `[project] id`.** An attestation names the project it is for by the id this
  file commits, which a project with an authority must write: an id taken from a folder's name or
  a machine's registry could differ between machines, and could match another project's.

An authority with no `[workflow]`, for an adapter the workflow does not declare, named twice,
with no issuer or a key that does not read is refused by name, and a guard with no authority
committed for its adapter leaves the workflow unsupported.

| Key | Default | What it sets |
| --- | --- | --- |
| `adapter` | required | the adapter of the workflow's `adapters` this authority answers for |
| `issuer` | required | the name the authority signs as, which its observations and attestations repeat |
| `contract` | required | the attestation contract it speaks, `1` in this release |
| `key` | required | the public half of its signing key, `ed25519:` and 64 lowercase hex digits |

## `[releases]`

Read only by the release run of the Symphony's own repository ([RELEASES](../workflow/RELEASES.md)), which no
other project runs.

| Key | Default | What it sets |
| --- | --- | --- |
| `every_landings` | `20` | a release run starts once this many tasks have landed; `0` starts none |
| `refresh` | `true` | whether a green release refreshes the machine |
| `publish` | `false` | whether a green release, once pushed, publishes on crates.io each package whose version it lacks ([RELEASES](../workflow/RELEASES.md#packages-on-cratesio)) |
| `github` | none | the command a green release, once pushed, asks for its GitHub Release, run as `COMMAND… --request FILE`: `["symphony-github", "release"]` names the GitHub adapter's; with none, no program is run ([RELEASES](../workflow/RELEASES.md#a-github-release)) |
| `hosted` | none | the command a release run asks to verify its commit on GitHub's machines in place of this one, run as `COMMAND… --request FILE`: `["symphony-github", "verify"]` names the GitHub adapter's; with none, no program is run and every run verifies here ([RELEASES](../workflow/RELEASES.md#on-githubs-runners)) |
| `hosted_every_minutes` | `30` | while `hosted` names a command, a run of main's tip also starts at a landing once this many minutes passed since the last run started, unless that tip is judged already: the least time between two starts, since one run at a time holds the release's lock, so a value below a whole run's length makes the runs back to back. Such a run verifies nothing on this machine, and still builds at its tip and, when green, installs and refreshes here. `0` starts none by the clock, and without `hosted` it is not read ([RELEASES](../workflow/RELEASES.md#the-hosted-clock)) |
| `rulesets` | none | the command each run runs after its checks as the check `rulesets`, in the main checkout, red unless it exits 0: `["symphony-github", "rulesets", "--verify"]` verifies again who bypasses the default branch's rules, where the adapter's settings publish pull requests; with none, the run has no such check ([RELEASES](../workflow/RELEASES.md#who-bypasses-the-rules-again)) |
| `candidate_reruns` | `3` | the red reruns a release candidate takes before it is dropped |
| `candidate_hours` | `24` | the hours a release candidate is held before it is dropped |
