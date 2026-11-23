# Blueprints

A blueprint is a named, ready-made setup a project starts from: its board columns, its checks and
the stamps a land needs, its recipes, its skills and standing rules, its look and the plugins it
turns on. It has two jobs. A new project works well from its first day, and several projects of
one kind stay on the same process, tools and look as the blueprint improves. The three examples
asked for are a game made with Godot, a Rust web application set up as this repository is, and a
personal assistant with a calendar and a list of things for a person to do.

This is a concept. Nothing here is built and no setting below is read today. Each choice is on a
card at the section that argues it, a few at a time.

- **The shape.** A blueprint is a starter a project keeps following. Its files are copied into
  the project, and while the project leaves alone what the blueprint put there, each new version
  of the blueprint replaces them. A project that changes one becomes its own version: it can
  still ask for the blueprint's later changes, each as an agent's task.
- **The home.** Each blueprint is a Git repository of its own, named by its address and a tag.
- **The order.** Rust web first, taken from this repository; then Godot; then the personal
  assistant.
- **Still open.** [How a new version reaches a project that follows](#how-a-new-version-arrives),
  and [what the project holds of its blueprint](#what-the-project-holds-of-its-blueprint).

How facts are marked: a fact about the code names the file it was read in; "read 2026-11-20" is
this machine that day, the other registered projects' checkouts read in place and changed in
nothing.

## What a project is given today

`symphony init` installs the agents' integration and nothing else: nine generic skills, the
recipes they run, the allow rules and two hooks (`crates/symphony/src/init.rs`). A project with no
`symphony.toml` is a plain project of every default
([PROJECT_FILE](../../operations/PROJECT_FILE.md)): `docs/` served at `/docs/`, the built-in
seven-column board, the four stamps, no plugin, no check and the Symphony's look.

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
| Settings | `symphony.toml`, its defaults in `crates/symphony/src/project.rs`. | The project file over the defaults; the board's store of preferences and `symphony.local.toml` over it, for preferences alone ([the Settings page](settings-page.md#what-the-board-keeps)). | Something that says which of the file's values a blueprint put there. |
| Board columns | `[workflow]`: phases, states, positions and edges ([WORKFLOW](../../workflow/WORKFLOW.md)). Omitted, the built-in definition. | A project writes the whole table or none. The board follows the one the default branch commits and activates a changed one when it next reads it. | A changed workflow needs `[[workflow_mappings]]` for every pair it drops, and its guards need keys only the project holds. |
| Checks a land needs | `[stamps]`, a catalogue of four stamps, each with a reader in the board (`crates/tasks/src/stamps/read.rs`). `[verdicts] check`. A plugin's gates and doc checks. | A project writes its whole catalogue or none. A stamp with no reader is refused. | A check of the project's own tools (Godot's export, Cargo's tests) is a recipe recorded as a test run, which today the project writes itself. |
| Recipes | `symphony.just`, written by init and imported by the project's justfile. | A recipe of the project's own of the same name wins. | Recipes for a kind of project: its lint, its tests, its build, each through the recorder. |
| Skills | `.agents/skills/`, each shipped skill stamped. | An unstamped file is the project's and is never written over. `project.md` beside a shipped skill is the project's own page for that role. | Skills for a kind of project (shaders, art, Rust conventions). |
| Standing rules | `RULES.md` and `AGENTS.md`, the project's own. This repository's hold its whole process: evidence as the gate, tests after, one merge of main before review. | Nothing: each project writes its own. | The process itself. It is the largest thing similar projects would share, and it has no carrier at all. |
| Plugins | Programs built on the plugin kit, named in `[plugins]` ([PLUGIN_PROTOCOL](../../engineering/PLUGIN_PROTOCOL.md)). | Only the plugins a project selects answer for it. | The Symphony ships none: the only plugins in this workspace are its tests'. A calendar or a to-do list is a program somebody has to write and deliver. |
| Look | A theme package a plugin brings, then `[theme]` over it: name, icon, a stylesheet after the package's, tokens by name, a layout variant for each region (`crates/symphony/src/theme.rs`). A reader's own saved theme in the browser sits over both. | Token by token, the file over the package. | A theme package comes from a plugin alone, so a look that is only colours, an icon and a stylesheet still needs a program. |
| UI components | The client draws each block of a fixed vocabulary with one component (`crates/symphony-blocks`). A plugin renders to blocks and never adds client code. | The client holds every layout variant; a project picks among them. | Nothing a blueprint could carry. A new kind of component is a change to the client, for every project. |
| Documents | `[[entrypoints]]`; a register, a board guide. | The project's own. | A starting set: a README, an empty register, a guide for agents. |

A blueprint needs no new kind of setting: it is the tables and files a project already has. Three
things in the table are not settings at all (rules, plugins, components), each with a question of
its own in a later round.

## What a blueprint must respect

**It holds commands, and it decides what lands.** Its checks, generators and recipes are commands
the Symphony or an agent runs, and its skills and rules tell agents what to do. Its stamps and its
workflow decide what a land requires and who may move a task. The Symphony keeps one rule for such
settings: they are read from a commit. The local settings file and the board's store may not hold
them, because a place no commit records must not change the landing gate or run a command
([PROJECT_FILE](../../operations/PROJECT_FILE.md#the-local-settings-file)). The board reads
`[stamps]` and `[workflow]` from the default branch's committed project file, so a branch cannot
change what it is held to. So what a project takes from a blueprint is committed in that project,
and a new version arrives as a commit there.

**A release never writes a project's checkout.** A release is installed on the machine and each
project takes it by its own landing ([RELEASES](../../workflow/RELEASES.md#the-refresh)).

**A project keeps what is its own.** Init already separates the two. A file carrying the
Symphony's stamp is the Symphony's, brought current on each run; a file without the stamp is the
project's and is never touched; and the project's own word sits beside the shipped file
(`project.md`) or over it (a recipe of the same name).

**A blueprint is not a project's whole setup.** The first consuming project's file is mostly its
own: its landing tool, its pool of test slots, its generators for its own content. A consuming
project owns its landing policy ([the root guide](../../../AGENTS.md#command-ownership)). What
another Godot game could take is smaller: recipes for Godot's checks, skills, a start for the
documents, a look. Pulling a blueprint out of a working project means telling "any game" from
"this game", and that is work in that project, by its own agents.

## How a project takes one

A blueprint is a starter the project keeps following, until the project changes it.

**Taking one.** Its files are copied into the project, each at the place its reader looks: the
project file at the root, the recipes beside the justfile, the skills in the skills folder, the
rules, the theme's files, the starting documents. They are committed there. With them goes a
record of what the blueprint put: its address, its version, each file it wrote and each key it
set in the project file.

**Following.** While everything in that record is still as the blueprint put it, the project
follows. A new version replaces those files and keys. Nothing of the project's can be lost by
that, since the project changed none of them, so the update needs no agent to work it out.

**What is the project's own from the first day.** A project adds without ceasing to follow: a key
the blueprint does not set (its name, its id), a skill, a recipe or a document of its own. The
starting documents a project writes in (its README, its register, its guide for agents) are
copied once, are outside the record, and are never replaced. A preference changed on the Settings
page is kept in the board's store, not in the committed file, so it changes nothing the blueprint
put ([below](#how-it-meets-the-other-two-concepts)).

**Its own version.** A landing that changes something in the record (a key the blueprint set, a
file it wrote) makes the project its own version of the blueprint. Nothing replaces its files
from then on. The record keeps the version it left from, so the project can still ask for what
the blueprint changed since: that is an agent's task, which reads the blueprint's changes, fits
them to the project's own, and lands under review as any change does, since nothing promises
they still fit.

**What this asks of a blueprint.** A file the blueprint puts and a project is sure to write in
would end the following on the first day. So a blueprint keeps its part and the project's part in
separate files wherever a reader allows it, as the shipped skills do with `project.md`: its rules
in a file of their own that the project's guide links, its recipes in a file the project's
justfile imports. The project file is one file by its reader, which is why it is recorded key by
key.

**Against the three shapes first proposed.** It is the starter (A) with a record, so later
versions can still arrive. It is not the layer (B): there is no folder apart and no second file
read under the project's, so the project file keeps one reader; and a project's change over the
blueprint ends the following, where the layer kept it. It is nearest the reference (C), in that a
project that changed nothing stays current with no work. It differs from C in two ways: the
blueprint's content is committed in the project, so a clone at any commit reads the setup that
commit had, whatever the machine has installed; and a project can leave and become its own.

- [-] **blueprint-shape** — A blueprint would be a ready-made setup a project starts from: its board columns, checks, recipes, skills, rules and look. This card decides what a blueprint is inside a project once taken, which decides whether similar projects can stay alike. A: a starter. Its files are copied in once and are the project's from then on; nothing updates them. B: a layer. Its files are committed in a folder of their own that the project never edits, the project's own settings, recipes and skills sit over them, and an update replaces that folder by a task that lands in the project. C: a reference. The project names a blueprint, and Symphony reads it from the release installed on the machine; an update arrives with the release, with no commit in the project. Recommended: B. Approve and say A, B or C in the note, or comment. [[analysis/blueprints#How a project takes one]]
  [2026-11-20 06:25] agent — Posted for the board task `app/blueprints`, from your ask: "an out of the box setup, that has a certain set of preconfigured elements, including ui compoenents and themes, columns, tooling and quality gate checks, and so on", for new projects and to "\"maintain\" similar boards projects on the same set of processes, tools and UI". Nothing is built; this is the first of several rounds.

    **Base.** Today `symphony init` installs the nine skills, their recipes and two hooks. Everything else a project writes by hand: the three projects on this machine have three project files with nothing shared.

    **A** gives a good first day and nothing after it: each project drifts from the day it starts. **B** keeps similar projects alike, each by its own landing, and a project loses nothing of its own because it never edits the blueprint's folder. **C** keeps them alike with no landing at all.

    **Why B.** A blueprint holds commands (checks, recipes) and decides what a land needs (stamps, columns). Symphony reads such settings from a commit and reviews a change of them; the settings file of one machine may not hold them for that reason. Under C what lands in a project would change when the machine takes a release, with no commit in that project. B is how `symphony init` already treats its skills: a stamped file is Symphony's and is brought current, a file beside it is the project's and is never touched.

    **What B costs.** The project file gains a layer under it, in each place that reads it: the app, the board, the settings check and `just tasks limits`. A and C cost less to build.

    **Earlier verdicts checked.**
    - **project-workflow-shape** (C) and **configurable-workflow-activation** (B) decide what a project's workflow may be and when a committed one takes effect. Under B here a blueprint's changed columns take effect when its update lands, by that rule.
    - **needed-stamps** decides what a land requires, read from the committed project file.
    - **release-delivery** and **plugin-delivery-choice** (B) decide how a project takes a release of Symphony itself.

    None of them says what a setup shared between projects is.
  [2026-11-23 11:14] owner · rejected — This should work like the create-react-app used to work like. Which is to have it be and starter but that allows future changes to continue to propagate, as long as the project does not change settings that the blueprint puts in place. Once they do, then this now becomes a separate and its own version, which the user can still ask for upstream changes, but would require an agent task for updates given there's no guarantee of non breaking changes there. This sounds like C? But not exactly
  [2026-11-23 11:30] agent — Recorded as the design: a blueprint is a starter the project keeps following. Its files are copied into the project and committed, with a record of what the blueprint put. While the project leaves those alone, each new version replaces them. A landing that changes one makes the project its own version; it can still ask for the blueprint's later changes, each as an agent's task under review. The section above this card now says so, and how it differs from C. The one thing your note leaves open, how a new version reaches a project that still follows, is the card **blueprint-following**.

### How a new version arrives

For a project that follows, replacing the blueprint's files is mechanical. What is open is what
starts it and what it must pass. Three ways:

- **A. By itself.** When the blueprint has a newer tag, the Symphony files a task in the project
  that replaces what the blueprint put, runs the project's own checks at that tip, and lands it on
  those green, with no reviewer. The change was reviewed where it was made, in the blueprint's
  repository; the project trusts that address as it trusts a dependency.
- **B. By itself, read first.** As A, and a fresh reviewer reads the diff in the project before it
  lands. It costs an agent's review for each version in each project, and catches a version that
  does not suit this project, or one from a blueprint somebody else publishes that should not run
  here.
- **C. When asked.** The project's page says a newer version exists. Nothing is filed until the
  person asks; then as A.

A is nearest to "keeps propagating", and an update that needs no agent's work is what the shape
promises a project that follows. B is the careful one where a blueprint is not the project
owner's own. Under each, the version arrives as a commit in the project.

- [ ] **blueprint-following** — A blueprint would work as you described: a starter whose later versions keep arriving while the project leaves alone what the blueprint put there. A project that changes one becomes its own version, and asks for later changes as an agent's task. That leaves one thing open: how a new version reaches a project that still follows. A: by itself. Symphony files the update when the blueprint has a newer tag, runs the project's own checks, and lands it once they pass, with no reviewer. B: by itself, read first. As A, and a reviewer agent reads the changes before it lands. C: when asked. The project's page says a newer version exists, and nothing happens until you ask. Recommended: A. Approve and say A, B or C in the note, or comment. *Replaces:* blueprint-shape. [[analysis/blueprints#How a new version arrives]]
  [2026-11-23 11:23] agent — Posted for the board task `app/blueprints`, from your note on **blueprint-shape**: "have it be and starter but that allows future changes to continue to propagate, as long as the project does not change settings that the blueprint puts in place".

    **Base.** Nothing is built. Your shape is written up as the design, above this card: what "the blueprint put" is recorded when a project takes it, key by key and file by file.

    **How it differs from C.** Like C, a project that changed nothing stays current with no work. Unlike C, the blueprint's files are committed in the project and each version arrives as a commit there, so a clone reads the same setup on any machine. And a project can leave and become its own version, which C had no way to do.

    **A** costs nothing per version. The changes were reviewed where the blueprint is made, and the project trusts the blueprint's address as it trusts a dependency. **B** costs an agent's review for each version in each project, never your time. It catches a version that does not suit this project, or one from somebody else's blueprint that should not run here. **C** costs a click, and projects fall behind until someone asks.

    **Why A.** You described updates for a project that follows as needing no agent's task, since nothing of the project's can break. A is that. B is worth it mainly for a blueprint you do not write yourself.

    **Earlier verdicts checked.** Your note on **blueprint-shape** gives the shape and does not say what starts an update or whether anyone reads it. **settings-page-shape** keeps settings that run commands off the page's saves; a blueprint's version is a commit under every option here, so it does not decide this. **blueprint-home** (B) says where a version comes from, not how it lands.

### What the project holds of its blueprint

The copies sit where their readers look under either way below: Claude Code, `just` and the
Symphony each read fixed places, and a submodule lives at one path. What differs is how the
project knows its blueprint and what it left.

- **A. A record alone.** One committed file names the address, the version and what the
  blueprint put. The Symphony fetches the blueprint into the machine's folder when it takes a
  version, compares, or hands an agent what changed.
- **B. A submodule as well.** The blueprint's repository is a Git submodule inside the project's,
  at the version taken. Git records the version, and the untouched blueprint is in the checkout to
  compare with and for an agent's task to read. Every clone and every task's worktree has to
  fetch and check out the submodule, and the Symphony makes a worktree for each task; Git's own
  manual says its support for submodules in several checkouts of one repository is incomplete and
  advises against it. The checkout holds the blueprint twice.

- [ ] **blueprint-in-project** — You asked whether a blueprint could be added to a project as a Git submodule inside its repository. Either way the blueprint's files are copied to where they are read (the skills folder, the project file at the root), since each reader looks in a fixed place. The choice is how the project remembers its blueprint. A: a record alone. One committed file names the blueprint's address, its version and what it put; Symphony fetches the blueprint when it needs to compare or update. B: a submodule as well, inside the project's repository, at the version taken. Recommended: A. Approve and say A or B in the note, or comment. [[analysis/blueprints#What the project holds of its blueprint]]
  [2026-11-23 11:23] agent — Posted for the board task `app/blueprints`, from your note on **blueprint-home**: "Could use submodules to add it to a symphony project (inside the project repo?)".

    **Base.** Nothing is built. A blueprint is a Git repository of its own, as you approved.

    **A** keeps a project an ordinary repository. Symphony needs the network, or a copy it fetched before, to compare with the blueprint or to hand an agent what changed.

    **B** has Git itself record the version, and keeps the untouched blueprint in the checkout to compare with. It costs every clone and every task's worktree a submodule to fetch and check out, and Symphony makes a worktree for each task. Git's own manual says its support for submodules in several checkouts of one repository is incomplete and advises against it. The checkout also holds the blueprint twice.

    **Why A.** B's gain is convenience for Symphony; its cost falls on every task of every project.

    **Earlier verdicts checked.** **blueprint-home** (B) decides that a blueprint is its own repository and is fetched, not how the project holds it.

## Where blueprints live

Each blueprint is a Git repository of its own, named in the project by its address and a tag.
Anyone can publish one, and an organisation can keep one private for its own projects.

What follows from that:

- **A version** is a tag, recorded with the commit it named when it was taken, so a tag moved
  later changes nothing already taken.
- **Trust.** The Symphony fetches a blueprint from its address, and what it brings is run in the
  project: commands, and instructions agents follow. Naming an address is trusting it as a
  dependency is trusted. The Symphony fetches from an address nowhere else today.
- **The release it needs.** A blueprint can be written for a newer Symphony than the machine has.
  It says the oldest release that reads it, and taking it on an older one is refused by name.
- **A blueprint's repository can be a project itself**, with a board, so a change to it is filed,
  reviewed and landed there before any project receives it.
- **The Symphony's repository holds no blueprint.** The first one, Rust web, is taken from this
  repository's setup into a repository of its own.

The two homes set aside: the Symphony's own repository, shipped in each release, which needs no
fetching and covers no private blueprint; and another project's checkout on the machine, which
copies what is that project's alone together with what is shareable.

- [x] **blueprint-home** — Where blueprints are kept, and what a version of one is. This decides who can make a blueprint and how far a project has to trust it. A: in Symphony's own repository, shipped in each release beside the skills; a blueprint's version is the release, and a blueprint is made or changed only by work that lands here. B: each blueprint a Git repository of its own, named by its address and a tag; anyone can publish one, and Symphony fetches it. C: another project on the machine is the blueprint; a project takes its setup from that project's checkout. Recommended: A. Approve and say A, B or C in the note, or comment. [[analysis/blueprints#Where blueprints live]]
  [2026-11-20 06:25] agent — Posted for the board task `app/blueprints`.

    **Base.** Today the only setup Symphony hands a project is the skills and recipes its release ships.

    **A** needs no fetching and no new trust: a release can always read the blueprints it ships, as both come from one commit. It does not cover a private blueprint, such as one organisation's own process for its own projects. **B** covers that, and has Symphony fetch commands from an address and run them in a project, which it does nowhere today. **C** is the shortest way to "like that project", and copies what is that project's alone with what is shareable: most of the first consuming project's file is its own landing tool, its test slots and its content generators.

    **Why A.** The three blueprints you named are general ones. A blueprint under A is a plain folder, so a folder of the same format kept elsewhere can be added later, as a card of its own, without changing A.

    **Earlier verdicts checked.** **release-delivery** decides how a release reaches a machine, and **consumer-contract-versioning** (B) what a project links of Symphony. Neither says where a shared setup is kept.
  [2026-11-23 11:15] owner · approved — B. Could use submodules to add it to a symphony project (inside the project repo?)
  [2026-11-23 11:30] agent — Recorded: B. Each blueprint is a Git repository of its own, named in the project by its address and a tag, and Symphony's repository holds none. What follows is in the section above: a version is a tag recorded with its commit, a project trusts the address as a dependency, and a blueprint says the oldest Symphony release that reads it. On the submodule: it can hold the blueprint inside the project's repository, but the files still have to be copied to where each is read, and every clone and every task's worktree would have to fetch it. That is a choice of yours, so it is a card of its own: **blueprint-in-project**.

## Which one first

Rust web, then Godot, then the personal assistant.

| | What it takes | What it proves |
| --- | --- | --- |
| **Rust web, from this repository** | This repository's own process is the source: its rules, its checks guide, its lint and test recipes through the recorder. This repository becomes the first project on the blueprint, so every later change to it is proved here first. | Taking, following, leaving, and the split between "any Rust project" and "the Symphony". Every agent working here can do it. |
| **Godot, from the first consuming project** | That project's checkout is read-only to agents here, so the split is filed as work on its board and done by its agents; the result becomes a blueprint's repository. | A project that is not Rust, with generators and a look. It is the case the idea came from. |
| **Personal assistant** | A calendar plugin and a to-do plugin, neither written, and a board for work with no branch. | The most, and most of it is not about blueprints. |

- [x] **blueprint-first** — Which blueprint is built first. The first one proves the mechanism, so it decides how soon it works and where. A: Rust web, taken from this repository's own setup (its rules, its checks, its lint and test recipes), with this repository as the first project on it. B: Godot, taken from the first consuming project's setup. C: the personal assistant. Recommended: A, then B, then C. Approve and say A, B or C in the note, or comment. [[analysis/blueprints#Which one first]]
  [2026-11-20 06:25] agent — Posted for the board task `app/blueprints`.

    **Base.** No blueprint exists. Each of the three would be pulled out of something: two out of a working project, one out of nothing yet.

    **A** has its source and its first user in one repository, so one task can prove a blueprint, its update and the split between "any Rust project" and "Symphony itself". **B** is the case the idea came from and proves a project that is not Rust. That project's checkout is read-only to agents here, so the split is work on its board, by its agents, brought here afterwards. **C** needs a calendar plugin and a to-do plugin, neither written, and a board for work with no branch, diff or land; most of that is not about blueprints.

    **Why A first.** It is the only one a single task here can finish, and it puts this repository's own process under the mechanism before another project depends on it.

    **Earlier verdicts checked.** No card names a first blueprint or an order of project kinds.
  [2026-11-23 11:18] owner · approved — A
  [2026-11-23 11:18] owner · approved — Agree with A -> B -> C rollout
  [2026-11-23 11:30] agent — Recorded: A, and the order Rust web, then Godot, then the personal assistant. With **blueprint-home** answered B, the Rust web blueprint is taken from this repository's setup into a repository of its own, and this repository is the first project on it. Its build is filed once **blueprint-following** and **blueprint-in-project** are answered; making that new repository is yours to ask for then.

## The personal assistant

It is last because it asks two things the other two do not.

- **Plugins that do not exist.** A calendar and a to-do list are programs. The Symphony ships no
  plugin, and a project with no Rust toolchain cannot build one. Where a blueprint's plugins come
  from is a later card. A calendar also needs an account's credentials, which no committed file
  may hold.
- **Work that is not code.** A thing a person has to do has no branch, no diff, no tests and no
  land. The board's defaults assume all four: a `[stamps]` naming none is refused, and the default
  workflow ends in a land. A custom `[workflow]` can declare other task kinds and other columns,
  so the graph is there; whether a board works well with no repository behind its tasks is a
  product question of its own.

## The look over a blueprint

A blueprint's look is the `[theme]` keys and the files it puts. A project that wants its own
icons and another mood changes those, and by the shape above that makes it its own version. Two
ways keep it following, each a later card: a blueprint that leaves the look to the project (it
puts none, or puts it among the starting files, outside the record), and the reader's own saved
theme in the browser, which is one person's and sits over everything.

Two gaps are for a later round as well. A theme package comes from a plugin alone, so a
blueprint's look needs a way to be plain files. And a look is tokens, a stylesheet, one icon and
layout variants: a set of icons for the board's own controls is not something a theme can set
today.

## How it meets the other two concepts

**The Settings page** ([its analysis](settings-page.md)). The page shows a setting and changes
it; a blueprint decides what it starts as.

- A value is read from the default, the committed project file, the board's store, then the local
  file. A blueprint adds no place to that order: its values are in the committed file, and the
  record says which of them it put. The page's source line can say "the blueprint's" for those.
- A preference changed on the page goes to the board's store, which no commit holds, so the
  project still follows. The control that makes a stored value the project's commits it; where
  that key is one the blueprint set, the commit makes the project its own version, and the page
  says so before it is pressed.
- The settings that hold commands or decide what lands are one list, the one the store and the
  local file refuse. A blueprint sets many of them, which is why its versions arrive as commits.
- Taking a blueprint, and asking for its changes once the project is its own version, are actions
  the page can offer that file a task. Neither is a save.

**Users and identity** (`app/users-and-identity`).

- Taking a blueprint, or leaving it, changes what lands, so it is an act of whoever may change a
  project's reviewed settings: the owner today, an admin once there are several people.
- A blueprint can hold a policy (which role may take which edge of the workflow) and never a
  person, a key or a secret. A workflow's approval authorities and a hosted board's connection are
  already the project's alone for that reason.
- An identity provider that is a plugin is turned on as any plugin is. It belongs to a deployment
  more than to a kind of project, so a general blueprint would not name one.
- The to-do list of the personal assistant is tasks held by people, which is that concept's
  question before it is this one's.

## Later rounds

Each is a card once the two open ones are answered.

1. **Where a blueprint's plugins come from**: programs built and published with the blueprint,
   source the project builds, or none until a blueprint needs one.
2. **Rules as separate files**: where a blueprint's standing rules sit so that every agent reads
   them and a project adds its own without changing them.
3. **The look**: whether a blueprint puts one inside the record, a look as plain files, and
   whether a theme can set the board's own icons.
4. **Checks of the project's own tools**: whether recorded recipes under the `tests` stamp are
   enough, or a project needs stamps of its own beyond the four readers.
5. **Work that is not code**, for the personal assistant.
6. **What a blueprint leaves alone**: the launch agent, the models and the limits are one
   machine's or one person's choice, and a kind of project may have no say in them.
