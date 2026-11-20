# Blueprints

A blueprint would be a named, ready-made setup a project starts from: its board columns, its
checks and the stamps a land needs, its recipes, its skills and standing rules, its look and the
plugins it turns on. It has two jobs. A new project works well from its first day, and several
projects of one kind stay on the same process, tools and look as the blueprint improves. The three
examples asked for are a game made with Godot, a Rust web application set up as this repository
is, and a personal assistant with a calendar and a list of things for a person to do.

This is a concept. Nothing here is built, no setting below is read today, and each choice is the
owner's, on a card in [DECISIONS](../DECISIONS.md), a few at a time. The first three are
[what a blueprint is inside a project](../DECISIONS.md#blueprint-shape),
[where blueprints live](../DECISIONS.md#blueprint-home) and
[which one is built first](../DECISIONS.md#blueprint-first).

- **Most of a blueprint can already be written.** Every element asked for has a place in a project
  today. What is missing is a layer between the Symphony's built-in defaults and a project's own
  file, and something that carries that layer from one project to the next.
- **A blueprint holds commands and decides what lands.** So its content has to be committed in
  the project, and an update of it has to land there as any change does. That rules out a
  blueprint read live from the machine.
- **The Symphony already keeps its own files apart from a project's**, twice over. The same rule
  lets a project take a blueprint's update and lose nothing of its own.
- **The three examples are not equally near.** Two need a setup pulled out of a working project.
  The personal assistant needs plugins nobody has written and a board that can hold work with no
  branch, which is more than a blueprint.

How facts are marked: a fact about the code names the file it was read in; "read 2026-11-20" is
this machine that day, main at `59538b83e`, the other registered projects' checkouts read in place
and changed in nothing.

## What a project is given today

`symphony init` installs the agents' integration and nothing else: nine generic skills, the
recipes they run, the allow rules and two hooks (`crates/symphony/src/init.rs`). A project with no
`symphony.toml` is a plain project of every default ([PROJECT_FILE](../PROJECT_FILE.md)): `docs/`
served at `/docs/`, the built-in seven-column board, the four stamps, no plugin, no check and the
Symphony's look.

Everything past that a project writes by hand. Read 2026-11-20, the machine's registry holds three
projects and their three project files share no line but a habit:

| Project | What its file sets |
| --- | --- |
| This repository | Models for each role, the docs audit as the check of an owner's edit, one generator for the built client, a command run after each land, the release cadence. |
| The first consuming project, a Godot game | A plugin answering from a process, that plugin's theme package and an icon, three generators, its own check and its own land, a command run at each review, its own reader of machine work. About ten skills of its own beside the shipped ones. |
| A second project | Two document entrypoints, a name and a stylesheet for its look, models, two limits. |

Nothing tells the second Godot game, or the next Rust service, what the first one learned.

## Each element against what exists

| Element | Where it lives today | What sits over what | What a blueprint lacks |
| --- | --- | --- | --- |
| Settings | `symphony.toml`, its defaults in `crates/symphony/src/project.rs`. | The project file over the defaults. `symphony.local.toml` over it, for the launch agent, the models and `[limits]` alone. | A layer between the defaults and the project file. |
| Board columns | `[workflow]`: phases, states, positions and edges ([WORKFLOW](../WORKFLOW.md)). Omitted, the built-in definition. | A project writes the whole table or none. The board follows the one the default branch commits and activates a changed one when it next reads it. | A changed workflow needs `[[workflow_mappings]]` for every pair it drops, and its guards need keys only the project holds. |
| Checks a land needs | `[stamps]`, a catalogue of four stamps, each with a reader in the board (`crates/tasks/src/stamps/read.rs`). `[verdicts] check`. A plugin's gates and doc checks. | A project writes its whole catalogue or none. A stamp with no reader is refused. | A check of the project's own tools (Godot's export, Cargo's tests) is a recipe recorded as a test run, which today the project writes itself. |
| Recipes | `symphony.just`, written by init and imported by the project's justfile. | A recipe of the project's own of the same name wins. | Recipes for a kind of project: its lint, its tests, its build, each through the recorder. |
| Skills | `.agents/skills/`, each shipped skill stamped. | An unstamped file is the project's and is never written over. `project.md` beside a shipped skill is the project's own page for that role. | Skills for a kind of project (shaders, art, Rust conventions). |
| Standing rules | `RULES.md` and `AGENTS.md`, the project's own. This repository's hold its whole process: evidence as the gate, tests after, one merge of main before review. | Nothing: each project writes its own. | The process itself. It is the largest thing similar projects would share, and it has no carrier at all. |
| Plugins | Programs built on the plugin kit, named in `[plugins]` ([PLUGIN_PROTOCOL](../PLUGIN_PROTOCOL.md)). | Only the plugins a project selects answer for it. | The Symphony ships none: the only plugins in this workspace are its tests'. A calendar or a to-do list is a program somebody has to write and deliver. |
| Look | A theme package a plugin brings, then `[theme]` over it: name, icon, a stylesheet after the package's, tokens by name, a layout variant for each region (`crates/symphony/src/theme.rs`). A reader's own saved theme in the browser sits over both. | Token by token, the file over the package. | A theme package comes from a plugin alone, so a look that is only colours, an icon and a stylesheet still needs a program. |
| UI components | The client draws each block of a fixed vocabulary with one component (`crates/symphony-blocks`). A plugin renders to blocks and never adds client code. | The client holds every layout variant; a project picks among them. | Nothing a blueprint could carry. A new kind of component is a change to the client, for every project. |
| Documents | `[[entrypoints]]`; a register, a board guide. | The project's own. | A starting set: a README, an empty register, a guide for agents. |

Two conclusions follow. A blueprint needs no new kind of setting: it is the tables and files a
project already has, held one layer below the project's own. And three things in the table are not
settings at all (rules, plugins, components), each with a question of its own in a later round.

## What a blueprint must respect

**It holds commands, and it decides what lands.** Its checks, generators and recipes are commands
the Symphony or an agent runs. Its stamps and its workflow decide what a land requires and who may
move a task. The Symphony keeps one rule for such settings: they are read from a commit, and a
change of them is reviewed. The local settings file may not hold them, because a file no commit
records must not change the landing gate or run a command
([PROJECT_FILE](../PROJECT_FILE.md#the-local-settings-file)). The board reads `[stamps]` and
`[workflow]` from the default branch's committed project file, so a branch cannot change what it
is held to. A blueprint is the same kind of content, so the same rule holds for it: what a project
takes from a blueprint is committed in that project, and a new version arrives by a landing there.

**A release never writes a project's checkout.** A release is installed on the machine and each
project takes it by its own landing ([RELEASES](../RELEASES.md#the-refresh)). A blueprint's update
moves the same way: offered by the release, taken by the project.

**A project keeps what is its own.** Init already separates the two. A file carrying the
Symphony's stamp is the Symphony's, brought current on each run and written over even when it was
edited; a file without the stamp is the project's and is never touched; and the project's own word
sits beside the shipped file (`project.md`) or over it (a recipe of the same name). Settings do
the same with precedence: a key the local file writes holds, a key it leaves out keeps the
committed one. Neither needs a merge, because the project never edits the Symphony's copy.

**A blueprint is not a project's whole setup.** The first consuming project's file is mostly its
own: its landing tool, its pool of test slots, its generators for its own content. A consuming
project owns its landing policy ([the root guide](../../AGENTS.md#command-ownership)). What another
Godot game could take is smaller: recipes for Godot's checks, skills, a start for the documents,
a look. Pulling a blueprint out of a working project means telling "any game" from "this game",
and that is work in that project, by its own agents.

## How a project takes one

Three shapes, on [their card](../DECISIONS.md#blueprint-shape).

| | A. A starter | B. A layer | C. A reference |
| --- | --- | --- | --- |
| What init does | Copies the blueprint's files into the project once. | Commits the blueprint's files in a folder of their own, stamped, and a project file that names it. | Writes one line naming the blueprint and its version. |
| Whose files they are after | The project's. Nothing records where they came from. | The folder is the blueprint's; everything outside it is the project's. | Nothing is copied; the content stays in the installed release. |
| A project's own change | An edit of the copied file. | A key in its own project file, a recipe or a skill of its own, each over the blueprint's. The folder is never edited. | A key in its own file over the release's. |
| An update | None. The project reads the release's notes and edits by hand. | One command replaces the folder on a branch, which lands as a task with a diff a reviewer reads. | It arrives when the machine takes a release. |
| Similar projects stay alike | No: each drifts from its first day. | Yes, by each project's own landing. | Yes, at once and with no landing. |
| What breaks a rule above | Nothing, and it does half the job. | Nothing. | What lands in a project changes with no commit in it; a clone on a machine with another release reads another setup. |

**Why B.** A answers the first job and not the second. C gives up the commit: the stamps a task is
held to would depend on which release a machine has installed. B is init's stamp rule and the
settings' precedence, applied to more files.

Under B a blueprint's content is of two kinds, and each file is one or the other:

- **Layered**: settings, recipes, skills, rules and theme files. They stay the blueprint's, in its
  folder, and an update replaces them. The project's own sit over them.
- **Starter**: documents the project writes in from its first day, such as a README, the register
  and its guide for agents. They are copied once and are the project's from then on. An update
  never touches them.

**What B costs.** The project file gains a layer under it, in each place that reads it: the
library's reader (`crates/symphony/src/project/file.rs`), the board's read of the committed copy
for its own keys, stamps and workflow, the settings check, and `just tasks limits`, which says
where each value came from. One file cannot hold both layers: TOML refuses a table written twice,
so a blueprint's keys and the project's keys of one table need two files. How the two combine,
key by key or a whole table at a time, differs by table and is a later card
([below](#later-rounds)).

A fourth shape was considered and left out: the blueprint as a Git history the project merges, as
a downstream imports its upstream ([UPSTREAM_DOWNSTREAM](../UPSTREAM_DOWNSTREAM.md)). It keeps a
project's edits by merging them, so every update can conflict, in files agents edit daily.

## Where blueprints live

Three homes, on [their card](../DECISIONS.md#blueprint-home).

- **A. In the Symphony's repository, shipped in each release.** A blueprint is a folder beside the
  shipped skills (`crates/symphony/shipped/`), written and reviewed here as any change is, and its
  version is the release that carries it. A release can always read the blueprints it ships,
  because both come from one commit. Only work landed here makes or changes a blueprint.
- **B. Each blueprint a Git repository of its own**, named by address and tag in the project file.
  Anyone can publish one. The Symphony would fetch commands from an address and run them in a
  project, which it does nowhere today, and a blueprint written for a newer release than the
  machine's could name keys that release refuses.
- **C. Another project is the blueprint.** One project on the machine takes its setup from
  another's checkout. It is the shortest way to "like that project", and it copies what is that
  project's alone together with what is shareable.

**Why A first.** The three blueprints asked for are general ones, and the Symphony's own release is
where its skills and recipes already come from. One need A does not meet is a private blueprint: an
organisation's own process for its own projects, which is not fit for a public repository. B's
folder format is A's, so naming a folder elsewhere can be added later without changing A.

## Which one first

On [its card](../DECISIONS.md#blueprint-first).

| | What it takes | What it proves |
| --- | --- | --- |
| **A. Rust web, from this repository** | This repository's own process is the source: its rules, its checks guide, its lint and test recipes through the recorder. This repository becomes the first project on the blueprint, so every later change to it is proved here first. | The layer, the update and the split between "any Rust project" and "the Symphony". Every agent working here can do it. |
| **B. Godot, from the first consuming project** | That project's checkout is read-only to agents here, so the split is filed as work on its board and done by its agents; the result is brought here as a blueprint. | A project that is not Rust, with generators and a look. It is the case the idea came from. It needs two repositories moving together. |
| **C. Personal assistant** | A calendar plugin and a to-do plugin, neither written, and a board for work with no branch. | The most, and most of it is not about blueprints. |

**Why A.** It is the only one whose source and first user are both in reach of one task, and it
puts this repository's own setup under the mechanism. B follows it with the mechanism proved. C
waits on the two questions below.

## The personal assistant

It is listed last because it asks two things the other two do not.

- **Plugins that do not exist.** A calendar and a to-do list are programs. The Symphony ships no
  plugin, and a project with no Rust toolchain cannot build one. Where a blueprint's plugins come
  from is the first card of the next round. A calendar also needs an account's credentials, which
  no committed file may hold.
- **Work that is not code.** A thing a person has to do has no branch, no diff, no tests and no
  land. The board's defaults assume all four: a `[stamps]` naming none is refused, and the default
  workflow ends in a land. A custom `[workflow]` can declare other task kinds and other columns,
  so the graph is there; whether a board works well with no repository behind its tasks is a
  product question of its own.

## The look over a blueprint

The order already exists and a blueprint is one more step in it: the Symphony's own look, then the
blueprint's, then the project's `[theme]`, then a reader's own saved theme in the browser. A
project that wants its own icons and another mood over a shared blueprint sets its tokens, its
icon and a stylesheet in its own file, as it would over a plugin's package today, and keeps taking
the blueprint's updates.

Two gaps are for a later round. A theme package comes from a plugin alone, so a blueprint's look
needs a way to be plain files. And a look is tokens, a stylesheet, one icon and layout variants:
a set of icons for the board's own controls is not something a theme can set today.

## How it meets the other two concepts

**The Settings page** (`app/settings-page`). The page shows a setting and changes it; a blueprint
decides what it starts as.

- Each setting would have one more source to show: the default, the blueprint, the project or this
  machine. `just tasks limits` already says where each of its own keys came from.
- Turning off something the blueprint turns on is a key in the project's own file. Removing that
  key is "back to the blueprint's".
- The settings that hold commands or decide what lands are one list, the one the local settings
  file refuses. The page needs it to know what a save may write without review, and a blueprint's
  update needs it to know when a reviewer reads the diff. It should stay one list.
- Choosing a blueprint, or taking its update, could be an action of the page that files a task. It
  is never a save: both land.

**Users and identity** (`app/users-and-identity`).

- Taking a blueprint or its update changes what lands, so it is an act of whoever may change a
  project's settings: the owner today, an admin once there are several people.
- A blueprint can hold a policy (which role may take which edge of the workflow) and never a
  person, a key or a secret. A workflow's approval authorities and a hosted board's connection are
  already the project's alone for that reason.
- An identity provider that is a plugin is turned on as any plugin is. It belongs to a deployment
  more than to a kind of project, so a general blueprint would not name one.
- A reader's own look is per person and stays over everything a blueprint or a project sets.
- The to-do list of the personal assistant is tasks held by people, which is that concept's
  question before it is this one's.

## Later rounds

Each is a card once the first three are answered, and some only under B.

1. **Where a blueprint's plugins come from**: programs the Symphony builds and ships in each
   release, source the project builds, or none until a blueprint needs one.
2. **How the two files combine**, table by table: key over key for plain values; the whole table
   for `[workflow]` and `[stamps]`, which are whole lists today; and whether a project adds to a
   blueprint's lists (plugins, generators, entrypoints) or replaces them.
3. **A blueprint's update**: the command, who files its task in each project on that blueprint,
   and which stamps it needs. A changed workflow brings its own mappings from the version before.
4. **Rules and skills as a layer**: where a blueprint's standing rules sit so that every agent
   reads them, and how a project's own rules stand over them.
5. **A look as plain files**, and whether a theme can set the board's own icons.
6. **Checks of the project's own tools**: whether recorded recipes under the `tests` stamp are
   enough, or a project needs stamps of its own beyond the four readers.
7. **Work that is not code**, for the personal assistant.
8. **Private blueprints**: a folder of the same format outside the Symphony's repository.
9. **What a blueprint leaves alone**: the launch agent, the models and the limits are one
   machine's or one person's choice, and a kind of project may have no say in them.
