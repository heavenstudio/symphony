# Symphony documentation

New to Symphony: read [getting started](operations/GETTING_STARTED.md), then the page specs. The
documents are in a folder for each area, and each folder's own page says what it holds. The
reference documents say how each part behaves now; each area keeps the proposals and the evidence
behind its decisions in an `analysis/` folder of its own, listed on the area's page.

## Start here

- [Getting started](operations/GETTING_STARTED.md) — a project of your own, in order: install,
  `symphony init`, a first document and decision card, a first task to its landing, agents on the
  work.
- [The commands](operations/COMMANDS.md) — the recipes, which command to reach for, and where each
  is explained.
- [Product specs](product/README.md) — what each page of the app is for and how it behaves, with
  pictures.

## Product

What each page is for: [its page](product/README.md).

- [Product specs](product/README.md) — what each page of the app is for and how it behaves, with
  pictures; the form of a spec and how its pictures are taken.
- [The Mac app](../apps/symphony-desktop/README.md) — the servers you follow, in the dock and the
  menu bar: what it does, how to build it, what is not in it yet.
- [Its analyses](product/README.md#analysis) — its proposals and their evidence.

## Design

- [Design](design/README.md) — how the app looks: the header, the mark, a page while it loads and
  the timeline; analyses only, with no reference document yet.

## Workflow

How work moves: [its page](workflow/README.md).

- [Concept review](workflow/REVIEW.md) — review cards, comments, questions and the owner queue.
- [Task board](workflow/TASKS.md) — the board, sessions, roles and agent communication; the pages
  are [the task board](product/BOARD.md) and [a task's page](product/TASK.md).
- [Workflow definitions](workflow/WORKFLOW.md) — a project's own delivery workflow: the format,
  its validation, activation, admission and wire records.
- [Checks](workflow/CHECKS.md) — what a task in this repository proves before review, and the
  release run's full verification.
- [Releases](workflow/RELEASES.md) — taking a release on a project, then how a release is cut by
  the release run or out of cycle, named and read, and the release run's schedule.
- [Machine build and test work](workflow/MACHINE_WORK.md) — observed build/test phases, the
  board's build and test recorder (`tasks run`), and advisory machine targets.
- [Runs](workflow/RUNS.md) — the day files of a project's runs and their steps: the line each
  writes, where, for how long, and how a reader takes them.
- [Agent activity](workflow/AGENT_ACTIVITY.md) — the day files of the agents' health changes and
  their hours: sessions started, stale and lost, running by role and model, and the machine's
  load.
- [Its analyses](workflow/README.md#analysis) — its proposals and their evidence.

Agents working in this repository follow the standing [Rules](../RULES.md).

## Engineering

Extending it, and the contracts a project or a provider builds against:
[its page](engineering/README.md).

- [Writing a plugin and a theme](engineering/PLUGIN_WRITING.md) — a first plugin from nothing: its
  crate, its code and test, its settings, its component in a document and how to see it drawn; a
  theme's tokens by name, in the project file alone and from a plugin.
- [The board's JSON APIs](engineering/BOARD_API.md) — `tasks api`, the board's methods as a
  versioned JSON contract for a program, and the agents API with what a callback and the PULL
  digest push.
- [Plugin protocol](engineering/PLUGIN_PROTOCOL.md) — a plugin answering from a process of its
  own: the setting, the seams it answers, its process, its commands, a register read in one
  request, the line, each call's minor, the versions and the wire's shape.
- [Plugin calls](engineering/PLUGIN_CALLS.md) — each call into a project's plugins timed: the
  setting, the record and how to read it.
- [Commands for a project's tools](engineering/TOOL_COMMANDS.md) — draw one card, check a branch
  for a landing, read status and triage, and share the document write lock.
- [Working artifact contracts](engineering/ARTIFACTS.md) — optional verified publication and
  scoped private storage; the local/fake-provider core and the integrations still required for
  hosted use.
- [Execution adapter](engineering/EXECUTION_ADAPTER.md) — bounded provider operations, scoped
  identities, trusted actor context, capabilities and the independent execution wire contract.
- [Execution attempts](engineering/EXECUTION_ATTEMPTS.md) — the board's durable record of each
  provider attempt: admission, fenced recovery, controls, evidence eligibility and host grants; a
  task's page reads its attempts, and no setting reaches them yet.
- [Its analyses](engineering/README.md#analysis) — its proposals and their evidence.

## Operations

Setting it up and running it: [its page](operations/README.md). A project on one machine needs
none of the repositories around this one: its board runs on the machine's PostgreSQL, which
`symphony init` asks for ([the setting](operations/PROJECT_FILE.md#board)).

- [Getting started](operations/GETTING_STARTED.md) and [the commands](operations/COMMANDS.md), as
  above.
- [The project file](operations/PROJECT_FILE.md) — `symphony.toml`: every table and key a project
  can set, with its default.
- [Setting up GitHub](operations/GITHUB_SETUP.md) — the optional `symphony-github` from nothing to
  a check that passes: building it, the token for the issue exchange, the GitHub Apps and rulesets
  a delivery by pull request needs, in order, and what each failure a newcomer meets says.
- [GitHub issue exchange and pull request delivery](operations/GITHUB.md) — the reference for that
  adapter, turned off unless the default branch commits its settings: its settings and token
  files, its commands, what a reconcile writes, what it keeps, what pauses it, and how a land is
  delivered and the protection that delivery reads.
- [Operating the board](operations/BOARD_OPERATIONS.md) — the board's store and the schema
  contract between its builds, which build moves a live board, who writes it, its server,
  snapshots and holds, what the command may write, stuck servers, a board that went back or was
  copied, and board health.
- [PostgreSQL board lifecycle](operations/POSTGRESQL.md) — the board's PostgreSQL: its engine
  identity, a board still on SurrealDB and how it moves, server selection, holds, snapshots,
  recovery and space reclamation; the hosted board, which no command reaches yet.
- [Upstream and downstream repositories](operations/UPSTREAM_DOWNSTREAM.md) — the regular import
  of the public repository into a private copy with its ancestry kept, the export of selected
  reusable changes back, their checks, refusals and recovery; manual tasks, with PR delivery not
  enabled.
- [Its analyses](operations/README.md#analysis) — its proposals and their evidence.

## Decisions and history

A decision is a card at the place in its document where the thing it decides is said
([where a card is written](workflow/REVIEW.md#where-a-card-is-written)); the
[decision register](DECISIONS.md) holds the cards no document says the subject of and links the
documents that hold the others. Each area's analyses hold proposals and evidence. An approved
design can still await implementation; its approval alone does not make it current app behavior.

- [History](history/README.md) — retained records and superseded measurements.

The consuming project keeps its own review rules, content and workflow documentation.
