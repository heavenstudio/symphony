# Symphony documentation

The documents are in a folder for each area, and each folder's own page says what it holds. The
reference documents say current behavior and supported setup; each area keeps the proposals and
the evidence behind its decisions in an `analysis/` folder of its own, listed on the area's page.

## Product

- [Product specs](product/README.md) — what each page of the app is for and how it behaves, with
  pictures; the form of a spec and how its pictures are taken.
- [The Mac app](../apps/symphony-desktop/AGENTS.md) — the desktop app's guide: the servers it
  follows and how it finds them, what it counts and notifies, where a server's window may go, and
  what is not in it yet.
- [Its analyses](product/README.md#analysis) — its proposals and their evidence.

## Design

- [Design](design/README.md) — how the app looks: the header, the mark, a page while it loads and
  the timeline; analyses only, with no reference document yet.

## Workflow

How work moves: [its page](workflow/README.md).

- [Concept review](workflow/REVIEW.md) — review cards, comments, questions and the owner queue.
- [Task board](workflow/TASKS.md) — the board, sessions, roles and agent communication.
- [Workflow definitions](workflow/WORKFLOW.md) — the additive candidate format, validation and
  wire records; custom workflow activation remains unavailable.
- [Checks](workflow/CHECKS.md) — what a task in this repository proves before review, and the
  release run's full verification.
- [Releases](workflow/RELEASES.md) — how a release is cut by the release run or out of cycle,
  named and read, and the release run's schedule.
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

The contracts a project or a provider builds against: [its page](engineering/README.md).

- [Plugin protocol](engineering/PLUGIN_PROTOCOL.md) — a plugin answering from a process of its
  own: the setting, the seams it answers, its process, its commands, a register read in one
  request, the line, each call's minor, the versions and the wire's shape.
- [Plugin calls](engineering/PLUGIN_CALLS.md) — each call into a project's plugins timed, and sent
  through a JSON round trip inside the process: the two settings, the record and how to read it.
- [Commands for a project's tools](engineering/TOOL_COMMANDS.md) — draw one card, check a branch
  for a landing, read status and triage, and share the document write lock.
- [Working artifact contracts](engineering/ARTIFACTS.md) — optional verified publication and
  scoped private storage; the local/fake-provider core and the integrations still required for
  hosted use.
- [Execution adapter](engineering/EXECUTION_ADAPTER.md) — bounded provider operations, scoped
  identities, trusted actor context, capabilities and the independent execution wire contract.
- [Execution attempts](engineering/EXECUTION_ATTEMPTS.md) — the board's durable record of each
  provider attempt: admission, fenced recovery, controls, evidence eligibility and host grants; no
  route or client reaches it yet.
- [Its analyses](engineering/README.md#analysis) — its proposals and their evidence.

## Operations

Setting it up and running it: [its page](operations/README.md).

- [The project file](operations/PROJECT_FILE.md) — `symphony.toml`: every table and key a project
  can set, with its default.
- [PostgreSQL board lifecycle](operations/POSTGRESQL.md) — scratch engine identity, server
  selection, holds, snapshots, recovery and space reclamation.
- [Setting up GitHub](operations/GITHUB_SETUP.md) — the optional `symphony-github` from nothing to
  a check that passes: building it, the token for the issue exchange, the GitHub Apps and rulesets
  a delivery by pull request needs, in order, and what each failure a newcomer meets says.
- [GitHub issue exchange and pull request delivery](operations/GITHUB.md) — the reference for that
  adapter, turned off unless the default branch commits its settings: its settings and token
  files, its commands, what a reconcile writes, what it keeps, what pauses it, and how a land is
  delivered and the protection that delivery reads.
- [Upstream and downstream repositories](operations/UPSTREAM_DOWNSTREAM.md) — the regular import
  of the public repository into a private copy with its ancestry kept, the export of selected
  reusable changes back, their checks, refusals and recovery; manual tasks, with PR delivery not
  enabled.
- [Its analyses](operations/README.md#analysis) — its proposals and their evidence.

## Decisions and history

The [decision register](DECISIONS.md) holds Symphony's product and engineering decisions, one
card each. An approved design can still await implementation; its approval alone does not make it
current app behavior.

- [History](history/README.md) — retained records and superseded measurements.

The consuming project keeps its own review rules, content and workflow documentation.
