# Boards and projects

Whether a project may hold several boards, or a second board is a project of its own. Three
concepts lean on the answer: who is on a board ([users and identity](users-and-identity.md)),
what a project starts from ([blueprints](blueprints.md)), and a [desktop app](desktop-client.md)
that reaches more than one server. This is a concept: it questions the idea against what the
Symphony has, compares the shapes it could take, and says the one chosen: one board for each
project, and a repository that may hold several projects, over folders of their own or the same
folders. Nothing here is built, and nothing is built from it but by a task filed from an approved
card.

Read at `66d362173`, and the blueprints concept as it landed at `11f6a2490`. The links name the
modules and documents read. A fact marked *inferred* was reasoned from them and not tried.

## The answer in short

- **Today a project has one board because the board is its repository's**, kept in its git
  common dir. Nothing says a project has one board; it follows from where the board is kept
  ([the picture](#what-the-symphony-has-today)).
- **One board for each project** ([its card](#boards-per-project)). Every tie between a board
  and its project (the landing, the cards, the project file) holds the whole project, so a second
  board in one project either separates only tasks and people, which groups already do, or
  splits the rest and is a project in all but name.
- **What a second board was asked for is served without one.** People with different powers over
  parts of the work are groups on the one board, as [users and identity](users-and-identity.md#what-the-owner-becomes)
  settled. A part that needs its own process, template, rules or people is a project of its own.
- **A repository may hold several projects** ([its card](#projects-per-repository)), over folders
  of their own or over the same folders. What sets two projects apart is their settings, every
  kind of them, and their board, which holds their people, their tasks and their decision queue.
  It is built when a repository first needs a second project.
- **Several machines on one project ask for fewer boards, not more.** Two clones on two machines
  keep two boards today, counted apart, until the central board every machine reaches. That board
  is the project's one.
- **What the three concepts read** ([below](#what-the-three-concepts-read)). People, groups and
  powers are each project's board's. A blueprint attaches to a project, with one record for each.
  The desktop app lists each server's projects, each one board, as it does.

## What the Symphony has today

![Today: one repository, one project, one board](boards-and-projects/today.svg)

| What | How it is today | Where |
| --- | --- | --- |
| A project | A folder and its `symphony.toml` (or none), served at `/<id>/` by the machine's one server. The nearest project file from where a command runs, up to the top of its checkout, names it. | `crates/tasks/src/project.rs`, [PROJECT_FILE](../../operations/PROJECT_FILE.md) |
| Its board | One PostgreSQL database named `tasks`, whose server keeps its data in the repository's git common dir, so every worktree reaches it. `TASKS_DATABASE` names another database on that server, which is how a scratch board is kept. A hosted board is the project's own database on the platform's PostgreSQL. | `DATABASE` in `crates/tasks/src/lib.rs`, `Home::in_common` in `crates/tasks/src/server.rs`, [TASKS](../../workflow/TASKS.md#where-it-lives), [PROJECT_FILE](../../operations/PROJECT_FILE.md#boardexternal_postgresql) |
| What else of a board is the repository's | Its heartbeat keys, its sweep state and its staffing lock, in the common dir. | [TASKS](../../workflow/TASKS.md#where-it-lives) |
| A task's branch | Its slug. A delivery keeps a receipt under the slug, and a second delivery of a slug is refused. | [TASKS](../../workflow/TASKS.md#a-slug-is-delivered-once) |
| What a board is held to | The stamps, the workflow, the landing and the board's own keys, read from the main checkout's committed project file; preferences from the board's store and the machine's local file over it. | [PROJECT_FILE](../../operations/PROJECT_FILE.md#the-boards-store) |
| The registry | `projects.json` in the machine folder: one entry for each project, by its root and its id. | `crates/symphony/src/machine/registry.rs` |
| Documents and cards | The project's entrypoints. A card is in its document, in git; the owner's word on it is an ask the board holds until an agent answers. | [REVIEW](../../workflow/REVIEW.md#where-a-card-is-written), [TASKS](../../workflow/TASKS.md#asks) |
| The agents' guides | `AGENTS.md`, `RULES.md`, the skills folder and the justfile, at the root `symphony init --root ROOT` is given. | `crates/symphony/src/init.rs` |
| People (decided, not built) | A profile for each person, kept by the server; their groups and powers, kept by each board; a group owns what the board's map of places gives it. | [users and identity](users-and-identity.md#where-people-are-kept) |
| Two machines, one project | Each clone keeps its board in its own common dir: two boards, counted apart, until a central board. | [the desktop analysis](desktop-client.md#several-servers) |

Two things follow that no document says:

1. **One board for each repository is where the board is kept, not a rule.** Nothing names a
   board apart from its repository, and no command asks which board.
2. **A project file in a subfolder makes a second project on the first one's board.** The walk to
   the nearest `symphony.toml` names the subfolder's project, with its own id and pages, and the
   board it reaches is the repository's one database (`project::find`, `Home::in_common`,
   `DATABASE`). Read in code, not tried: no project here has such a file.

## The idea, questioned

### What a second board would be for

The owner gave three examples, on three cards.

1. **Different people with different powers.** "This is a board specific setting. There could
   even be multiple boards for the same project where people have different permissions"
   ([its card](users-and-identity.md#people-kept-where)).
2. **A different template for part of the work.** "There can be multiple boards for the same
   project, which means, even multiple templates for it"
   ([its card](blueprints.md#blueprint-in-project)).
3. **Several machines on one project.** "For now I think the model should be indeed two separate
   boards. Albeit we will soon move to a 'central board' with 'multiple machines connected'"
   ([the desktop analysis](desktop-client.md#open-questions)).

The third asks for the opposite of the first two: one board that several machines share. Two
boards for one project exist today only because each clone keeps its own.

The first two, taken apart, ask four things:

- **Who may decide what.** Settled inside one board: a group owns what its places map to, and any
  member decides it; speaking and directing agents are powers of their own
  ([users and identity](users-and-identity.md#what-the-owner-becomes)). Two people can hold
  different powers over two parts of one board's work.
- **Who may see what.** Not settled: on a board everyone reads everything. Keeping a person from
  seeing part of the work is the one thing groups do not give. A board is not where the work is,
  though: every agent's worktree and every clone holds the whole repository, its documents among
  it. A part kept from someone is a repository kept from them.
- **A process of its own**: other columns, other checks. A board runs one workflow graph. Its
  definition lists task kinds, and no column or edge depends on a task's kind
  (`Definition` in `crates/symphony-api/src/workflow.rs`). What a land needs already differs task
  by task: an assessor names each task's stamps from the one catalogue
  ([needed stamps](../../workflow/TASKS.md#needed-stamps)).
- **A template of its own.** A blueprint is mostly files read at fixed places: the skills folder,
  the rules, the recipes, the project file. Two templates in one checkout write the same places. A
  template of its own needs files of its own: a project over folders of its own.

### What a board holds, and what a project holds

| A board holds | A project holds |
| --- | --- |
| tasks, threads, sessions and asks | its settings: what lands, the workflow, the stamps, the plugins, the look |
| its people's groups and powers, and its map of places | its address and its entry in the registry |
| its store of preferences | the folders it works over: documents and their cards, the guides, skills and recipes its agents read |

A project's folders may be its own, or the same as another project's in its repository; its
settings and its board are always its own.

They are tied in three places, and each tie holds the whole project:

- **Landing.** A task's branch is cut from the repository and lands on its default branch, held
  to the project file's stamps and workflow.
- **Cards.** A card is in a project's document, and the owner's word on it is an ask a board holds
  until an agent answers.
- **Settings.** A value made the project's from the Settings page changes the one project file
  that every board of the project would read.

So a second board in one project either shares all of that, and separates nothing but its tasks
and its people, which groups already separate, or splits it, and is then a project in all but
name.

## The shapes

Three shapes give a second board, and one keeps a single board with groups. For each: where the
board is kept, what branches and landing mean, what is shared and what is split, the address and
the registry, what a person and a group belong to, what a blueprint attaches to, what moves a
task, and what it costs from where the code stands.

![A second board: three shapes](boards-and-projects/shapes.svg)

### Several boards in one project

- **Where each board is kept.** A database of its own on the repository's server, as a scratch
  board is today, or rows of one database marked with their board.
- **Branches and landing.** Both boards' tasks branch from one repository and land on its default
  branch. Their slugs, branches and delivery receipts share one namespace, so a slug filed on both
  is refused at its second delivery. Neither land sees the other board's queue: the two boards'
  changes meet at the merge, as two tasks of one board do.
- **Settings, skills and the justfile.** One project file, and one set of guides, skills and
  recipes, for every board. A board's own process needs tables of its own in the file (its
  workflow, its stamps, its models and limits) and a store of its own. Skills, rules and recipes
  cannot differ by board: both boards' agents work in the same files.
- **Address and registry.** One registry entry and one `/<id>/`, with the board in the address
  beneath it, and in every command and session: a session in the main checkout cannot tell from
  its folder which board it is on.
- **People and groups.** Each board's own, as decided. The documents are the project's, so a
  person on either board reads all of them.
- **Cards.** One set of documents for two boards. Each document's cards need one board, for whose
  people decide them and which board holds the owner's ask: a map of documents to boards, above
  each board's map of places to groups.
- **A blueprint.** Split in two: the board's own tables for each board, the files for the project.
  Two blueprints writing one skills folder or one justfile need a rule for which wins.
- **Moving a task.** It keeps its branch; its thread and history go to the other board.
- **What it costs.** The most of any shape: a board named in every command, page and session;
  tables for each board in the project file, and a store each; the map of documents to boards; the
  heartbeat keys, sweep state and staffing lock kept for each board; and a value one board makes
  the project's changing it for the others.

### Several projects in one repository

- **Where each board is kept.** A database for each project on the repository's server, named for
  its project.
- **Branches and landing.** As above: one repository, one namespace of slugs and receipts, and
  each project landing as it does today.
- **Settings, skills and the justfile.** Each project has its own settings, every kind of them.
  It works over folders of its own (its project file, documents, guides, skills and recipes;
  `symphony init --root FOLDER` takes such a root today) or over the same folders as another
  project of the repository, which then share those files and differ in their settings alone.
  The guides at the repository's root are read by every project. *Inferred*: Claude Code reads its
  guides from the folder it starts in and the folders above, Codex from the repository's root down
  to that folder, and `just` finds the nearest justfile above it.
- **Address and registry.** One entry for each project, and a `/<id>/` each. Two projects over one
  folder are told apart by their ids, as their folder no longer tells them apart. The server's
  scan of a repository's worktrees keeps to the branches its own board names.
- **People and groups.** Each project's board's own. A person on both is added to each, and a
  group of one project is not the other's.
- **Cards.** Each project has its own decision queue. Over folders of its own, that is its own
  documents and register; in a document two projects serve, each card is given to one project.
- **A blueprint.** One for each project. Its files are in the project's folders, so two projects
  over the same folders share them.
- **Moving a task.** It can keep its branch, as the repository is one; its thread goes to the other
  project's board. No command does it today.
- **What it costs.** Some: the board kept for each project rather than each repository (its
  database, heartbeat keys, sweep state and staffing lock); slugs that cannot meet between a
  repository's projects; a launch, and its worktree's session, starting in the task's project and
  on its board; the sweep and the server's branch scan keeping to their own board. Projects over
  the same folders add two more: a project named by its id wherever a folder names two, and each
  card of a shared document given to one project's queue. For projects over folders of their own,
  the walk to the nearest project file and the registry by root are there already.

### One project per repository

Today's shape.

- **Where the board is kept, branches, landing, settings, address, registry:** as today, all the
  project's own.
- **People and groups.** Each project's board's own. A person on two projects is added to each.
- **A blueprint.** One for each project.
- **Moving a task.** It is filed again in the other project, linking the first. Its branch cannot
  follow, as the other repository has none of its history.
- **What it costs.** Nothing to build. A part that needs its own board must be split into a
  repository of its own, which costs its history in one place, and any code the two parts share.

### Inside one board

One board with groups, as users and identity has it, gives most of what a second board was asked
for:

- **Different powers over parts of the work**: a group owns what the board's map of places gives
  it, and its members decide it ([decided](users-and-identity.md#which-group-a-thing-belongs-to)).
- **Moving work to other people**: any member hands a card or a task to another group, and the
  board records who did.
- **A view of one part**: a group's own tasks and cards on the board, as a filter.

And two things it does not:

- **Keeping part of the work from someone on the board.** Everyone on it reads all of it. A power
  to read by place would be a card of its own; it would hide tasks and cards, never the files.
- **A process for one kind of work.** One graph for the board. Columns and edges that depend on a
  task's kind would be a change to the workflow, smaller than a second board.

### Compared

| | Boards in one project | Projects in one repository | A repository each |
| --- | --- | --- | --- |
| Documents and cards | shared by every board | a queue each, folders own or shared | each project's own |
| A part kept from someone | its tasks, not its files | its tasks, not its files | yes |
| A process of its own | tables for each board | yes | yes |
| A template of its own | the board's tables only | over folders of its own | yes |
| A task moved keeps its branch | yes | yes | no |
| What it costs | the most | some | nothing |

Under all three, people with different powers over parts of the work are groups on a board, and
several machines on one project reach its one board, the central board.

## The direction

### One board for each project

A project has one board, and a board is one project's. What a second board was asked for goes
elsewhere:

- **People with different powers over parts of the work** are groups on the one board, each
  owning what its places map to.
- **A part with its own process, template, rules or people** is a project of its own: in the same
  repository, over the same folders or folders of its own ([below](#projects-sharing-a-repository)),
  or in a repository of its own. Its settings, people, tasks, board and decision queue are its
  own whichever folders it works over.
- **A part kept from people who must not see it** is a repository of its own, as every worktree
  and every clone holds all of its repository's files.
- **Several machines** reach the one board: the central board, a concept of its own.

What stays as written elsewhere: the Settings page's control that makes a value the project's is
one board's say, and the whole of it; a board's map of places is its project's.

- [r] **boards-per-project** — Today a project has one board because the board is kept in its repository. Before people, blueprints and the desktop app build on boards, this decides whether a project may hold several. A: several boards in one project; they share its documents, its project file, its guides and its address, and each has its own tasks, people and process. B: one board for each project; people with different powers over its parts are groups on that board, and a part that needs its own process, template or people is a project of its own. Recommended: B. Approve and say A or B in the note, or comment. [[analysis/boards-and-projects#One board for each project]]
  [2026-11-23 12:45] agent — Posted for the board task `app/boards-and-projects`, from your rejection of the blueprints card on how a project records its blueprint (branch `app/blueprints`): "We need to decide upon if we are going to do multiple boards per project or multiple project entirely." A concept: nothing is built by this card.

    ![A second board: three shapes](boards-and-projects/shapes.svg)

    **Base (today).** One board for each repository, in its git common dir: by where it is kept, not by a rule. Two clones on two machines keep two boards, counted apart, until the central board.

    **A, several boards in one project.**
    - Each board has its own tasks, people and powers, and its own process written in the project file.
    - Shared: the documents and their cards, the project file, the skills, rules and recipes, the address, the branches and the landing.
    - So a board's template can differ only in what the board itself reads. Its agents read the same skills and rules as the other board's.
    - Every command, page and session names its board, and each document's cards are given to one board.

    **B, one board for each project.**
    - Different powers over parts of the work: groups on the one board, as already decided.
    - A part with its own process, template, rules or people: a project of its own, in a folder of the repository or a repository of its own (the next card).
    - Several machines: one board they all reach, the central board you described.

    **What neither gives.** Keeping part of the work from someone who can read the repository: an agent's worktree and every clone hold all of its files. Only a repository of its own does that.

    **Why B.** Each tie between a board and its project (the landing, the cards, the project file) holds the whole project. A second board in one project either separates only tasks and people, which groups already do, or splits the rest and is a project in all but name. B costs nothing now; A costs the most of any shape.

    **Earlier verdicts checked.**
    - **people-kept-where** (B), with your note "There could even be multiple boards for the same project where people have different permissions", keeps people with each board. That holds under both options; the note names a possibility and chooses no shape.
    - **settings-who-changes** (A) said "With one board for a project, that board's say is the whole of it" and left several boards to this concept.
    - **desktop-client** (A), your answer on two servers: two boards "for now", until a central board. That is about two machines, and points to one board.
    - Your question on the blueprints card is a question, and its rejection asks for this concept.

    None decides this.
  [2026-11-26 08:13] owner · approved — B, but they may even reference the same base folder. They should have different settings including people, tasks, board, decision queue tho and they could use different folders.
  [2026-11-26 08:21] agent — Recorded: B, as your note widens it: "they may even reference the same base folder. They should have different settings including people, tasks, board, decision queue tho and they could use different folders." A project has one board, and two projects can work over the same folder: each keeps its own settings, people, tasks, board and decision queue, and its own address. Two costs follow that the card did not state. In a shared folder a command names its project, because the folder no longer does. A card in a document both projects serve must be given to one project's queue. The analysis now says so under One board for each project and Projects sharing a repository. It lists as later rounds where such a project keeps its own settings, and how a shared card is given to one queue.
  [2026-11-26 08:24] agent · resolved

### Projects sharing a repository

A repository that holds two kinds of work (a game and its web service, say) may hold a project
for each, so that each has its own board, template and people. Two projects of a repository work
over folders of their own, or over the same folders. What sets them apart is their settings,
every kind of them, and their board, which holds their people, their tasks and their decision
queue. They share the repository's history, its branches and its landing, and any folders they
both work over.

![Two projects in one repository](boards-and-projects/two-projects.svg)

Today the Symphony half-allows it: a project file in a subfolder names a project, and that
project reaches its repository's one board ([above](#what-the-symphony-has-today)). Building
this ends that silent sharing: the second project gets a board of its own.

What building it takes, when a repository first needs it:

- **The board kept for each project**: its own database on the repository's server, and its own
  heartbeat keys, sweep state and staffing lock.
- **Slugs that cannot meet.** A branch is its slug and a delivery keeps a receipt under it, both
  the repository's. Two projects' slugs need keeping apart, as a later card.
- **A project named by its id.** The registry knows a project by its folder and the id its project
  file gives (`Entry` in `crates/symphony/src/machine/registry.rs`). Where two projects work over
  one folder, the folder names neither, so a command run there names its project.
- **Each project's own settings**, kept where the other project does not read them, as a later
  card. Over folders of their own, that is each folder's project file, as today.
- **Each card in one project's decision queue.** Over folders of its own, a project's queue is
  its documents' cards; a card in a document two projects serve is given to one of them, as a
  later card.
- **Agents start in their project and on its board**: in its folder, and named by its id where
  its folder is shared.
- **The sweep and the branch scan** keep to the branches their own board names.

- [r] **projects-per-repository** — If each project has one board, a repository with two kinds of work (a game and its web service, say) needs two projects to give each its own board, template and people. This decides whether a repository may hold several projects. A: one project per repository, as today; a part that needs its own board moves to a repository of its own, and a second project file inside a repository is refused. B: several projects in one repository, each a folder with its own project file, documents, guides and board; they share the repository's history, branches and landing. Built when a repository first needs a second project. Recommended: B. Approve and say A or B in the note, or comment. [[analysis/boards-and-projects#Projects sharing a repository]]
  [2026-11-23 12:45] agent — Posted for the board task `app/boards-and-projects`. It matters only if **boards-per-project** is B, and is asked apart as its own choice. A concept: nothing is built by this card.

    **Base (today).** A project file in a subfolder already names a project of its own, with its own id and pages, but that project reaches the repository's one board: two projects silently on one board. Read in code, not tried.

    **A, one project per repository.**
    - Nothing to build but refusing a second project file inside a repository, by name.
    - A part that needs its own board is split into a repository of its own, away from its history and from any code it shares with the rest.

    **B, several projects in one repository.**
    - Each project is a folder: its project file, documents, guides, skills, recipes and board, at its own address.
    - Shared: the repository's history and branches, the landing both projects' tasks reach, and the guides at its root, which both read.
    - To build when a repository needs it: the board kept for each project rather than each repository, slugs that cannot meet between a repository's projects, and agents started in their project's folder.

    **Why B.** It is where your "multiple boards for the same project" goes for a repository with two kinds of work, without what several boards in one project cost. A folder is a boundary git, `just` and the agents' guides already follow. Nothing is built until a repository needs it.

    **Earlier verdicts checked.**
    - **blueprint-home** (B) makes each blueprint a repository of its own; it says where a blueprint is kept, not what a project is.
    - **people-kept-where** (B) and **person-profile-where** (B) keep people with each board and profiles with the server, which holds under both options.

    None decides this.
  [2026-11-26 08:12] owner · approved — B, but they may even use the same folders too. What generally could change is settings generally between the two
  [2026-11-26 08:21] agent — Recorded: B, with your note: "they may even use the same folders too. What generally could change is settings generally between the two." Two projects of one repository use folders of their own, or the same ones. What sets them apart is their settings, every kind of them, and their board, with its people, tasks and decision queue. The section above now says so, and what building it takes gains two items. A project sharing a folder is named by its id, because today the registry knows a project by its folder. And it keeps its own settings where the other project does not read them. Where that is (a table for each project in one file, or a file for each project) is left until a repository first needs it.
  [2026-11-26 08:24] agent · resolved

## What the three concepts read

**Users and identity** ([its analysis](users-and-identity.md)).

- Each "board" there is a project's one board: its people, groups, powers and map of places are
  that project's, whether its folders are its own or shared. A person on two projects is added to
  each, with one profile.
- Its later round on several boards for one project is answered: a project has one. Two projects
  in one repository have two boards, each with its own people and groups, even over the same
  folders.
- A value made the project's from the Settings page is that project's alone: one board's say,
  the whole of it, as that analysis already says for one board.
- A short name is unique on one server, as decided. A project served from two servers is the
  central board's concept, not a second board's.

**Blueprints** ([its analysis](blueprints.md#what-the-project-holds-of-its-blueprint)).

- A blueprint attaches to a project, with one record for each project. The card on how a project
  records its blueprint can be asked again on that ground.
- A project has one blueprint, so one project never has two blueprints writing its skills folder
  or its justfile. A second template is a second project over folders of its own, its
  blueprint's files inside them. Two projects over the same folders share those files; which of
  their blueprints writes them is a later round.
- Its later rounds stand as written.

**The desktop app** ([its analysis](desktop-client.md#several-servers)).

- It lists each server's projects, each one board, as it is built. Nothing changes.
- Two clones on two machines stay two boards, counted apart, until the central board; then the
  project is counted once, by its board.
- A repository with two projects shows two rows, one for each, whether they share folders or not.

## Later rounds

Not carded yet.

- Whether a board can keep part of its work from a person on it: a power to read by place, which
  hides tasks and cards and never the files.
- Columns and edges for one kind of task on a board, for a process that differs by the work.
- How a repository's projects keep their slugs apart: each project's branches under its id, or a
  slug refused where another project of the repository holds it.
- Where a project keeps its own settings when it shares its folder with another: a table for each
  project in one project file, or a file for each project naming the folder it works over.
- Which project's decision queue holds a card in a document two projects serve.
- Which blueprint writes the files that two projects over the same folders share.
- Moving a task between two projects of one repository, its branch kept.
- Groups shared by a server's projects, such as a company's, where today each board has its own.
- The central board: several machines on one project's board, which the desktop app and the run
  records wait on.
