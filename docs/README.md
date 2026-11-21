# Symphony documentation

New to Symphony: read [getting started](GETTING_STARTED.md), then the page specs. The reference
sections below say how each part behaves now. Proposed changes and the evidence behind decisions
are under [Design and history](#design-and-history).

## Start here

- [Getting started](GETTING_STARTED.md) — a project of your own, in order: install, `symphony
  init`, a first document and decision card, a first task to its landing, agents on the work.
- [The commands](COMMANDS.md) — the recipes, which command to reach for, and where each is
  explained.
- [Product specs](product/README.md) — what each page of the app is for and how it behaves, with
  pictures; the form of a spec and how its pictures are taken.
- [The Mac app](../apps/symphony-desktop/README.md) — the servers you follow, in the dock and the
  menu bar: what it does, how to build it, what is not in it yet.

## Using it

- [Concept review](REVIEW.md) — review cards, comments, questions and the owner queue.
- [Task board](TASKS.md) — the board, sessions, roles and agent communication.
- [The project file](PROJECT_FILE.md) — `symphony.toml`: every table and key a project can set,
  with its default.
- [Releases](RELEASES.md) — taking a release on a project, then how a release is cut by the
  release run or out of cycle, named and read, and the release run's schedule.
- [Setting up GitHub](GITHUB_SETUP.md) — the optional `symphony-github` from nothing to a check
  that passes: building it, the token for the issue exchange, the GitHub Apps and rulesets a
  delivery by pull request needs, in order, and what each failure a newcomer meets says.
- [GitHub issue exchange and pull request delivery](GITHUB.md) — the reference for that adapter,
  turned off unless the default branch commits its settings: its settings and token files, its
  commands, what a reconcile writes, what it keeps, what pauses it, and how a land is delivered
  and the protection that delivery reads.

## Extending it

- [Plugin protocol](PLUGIN_PROTOCOL.md) — a plugin answering from a process of its own: the
  setting, the seams it answers, its process, its commands, a register read in one request, the
  line, each call's minor, the versions and the wire's shape.
- [Plugin calls](PLUGIN_CALLS.md) — each call into a project's plugins timed, and sent through a
  JSON round trip inside the process: the two settings, the record and how to read it.
- [Commands for a project's tools](TOOL_COMMANDS.md) — draw one card, check a branch for a
  landing, read status and triage, and share the document write lock.
- [Workflow definitions](WORKFLOW.md) — a project's own delivery workflow: the format, its
  validation, activation, admission and wire records.

## What it records

- [Machine build and test work](MACHINE_WORK.md) — observed build/test phases, the board's build
  and test recorder (`tasks run`), and advisory machine targets.
- [Runs](RUNS.md) — the day files of a project's runs and their steps: the line each writes,
  where, for how long, and how a reader takes them.
- [Agent activity](AGENT_ACTIVITY.md) — the day files of the agents' health changes and their
  hours: sessions started, stale and lost, running by role and model, and the machine's load.

## Hosting and contracts

For whoever runs Symphony for others or changes Symphony itself. A project on one machine needs
none of these: its board runs on the SurrealDB that `symphony init` fetches.

- [PostgreSQL board lifecycle](POSTGRESQL.md) — a board on PostgreSQL: scratch engine identity,
  server selection, holds, snapshots, recovery and space reclamation; the hosted board, which no
  command reaches yet.
- [Upstream and downstream repositories](UPSTREAM_DOWNSTREAM.md) — the regular import of the
  public repository into a private copy with its ancestry kept, the export of selected reusable
  changes back, their checks, refusals and recovery; manual tasks, with PR delivery not enabled.
- [Working artifact contracts](ARTIFACTS.md) — optional verified publication and scoped private
  storage; the local/fake-provider core and the integrations still required for hosted use.
- [Execution adapter](EXECUTION_ADAPTER.md) — bounded provider operations, scoped identities,
  trusted actor context, capabilities and the independent execution wire contract.
- [Execution attempts](EXECUTION_ATTEMPTS.md) — the board's durable record of each provider
  attempt: admission, fenced recovery, controls, evidence eligibility and host grants; a task's
  page reads its attempts, and no setting reaches them yet.
- [Checks](CHECKS.md) — what a task in this repository proves before review, and the release run's
  full verification.

Agents working in this repository follow the standing [Rules](../RULES.md).

## Design and history

The [decision register](DECISIONS.md) holds Symphony's product and engineering decisions, one
card each; [Analysis](analysis/) holds their proposals and evidence. An approved design can
still await implementation; its approval alone does not make it current app behavior.

- [Durable project artifacts](analysis/remote-artifact-store.md) — a proposed optional storage
  boundary for working evidence, Git revisions and recovery on a fresh application disk.
- [A project's task workflow](analysis/project-workflow.md) — the proposed project-defined
  steps, the existing gate boundary, and migration that preserves the default flow.
- [The board's cutover to PostgreSQL](analysis/board-pg-cutover.md) — the order for moving
  live boards, one board's move, the way back, what is not built yet and what the move waits for.
- [Managed PostgreSQL](analysis/hosted-postgresql.md) — the proposed hosted connection,
  connection budget, database boundary and PostgreSQL 16/18 validation contract.
- [Configurable delivery workflows](analysis/configurable-delivery-workflow.md) — the proposed
  C contract: separate phase/state, authoritative operations, migration and implementation briefs.
- [Artifact approvals at workflow transitions](analysis/revision-bound-transition-guards.md) —
  proposed input-bound human decisions, invalidation and atomic project-guard admission.
- [Execution providers and durable attempts](analysis/remote-execution-attempts.md) — a proposal
  for project-owned execution adapters, durable recovery and authorized attempt history.
- [Hosted ingress and board readiness](analysis/hosted-ingress.md) — proposed trusted proxy and
  visitor admission, project permissions, foreground lifecycle and readiness; the local gate stays.
- [Desktop client analysis](analysis/desktop-client.md) — the design and evidence behind the
  Mac app.
- [The slow tests, reviewed](analysis/slow-tests.md) — a time per test for the release run's six
  slowest binaries, what each slow test proves, and what is kept, made faster, merged or removed.
- [The release pipeline, redesigned](analysis/release-pipeline.md) — a night of red release runs
  read by run, a pre-certified CI proposal checked against the stamps, and the options with
  what was chosen of them, a hosted run on GitHub among them.
- [A Settings page](analysis/settings-page.md) — a concept: where a project's settings live
  now, what on and off mean for its integrations and tools, and a page that shows what is in
  force and writes the preferences alone.
- [Naming inventory and superseded plan](analysis/own-names.md) — historical naming evidence,
  with pointers to the current naming rules and state paths.
- [History](history/README.md) — retained records and superseded measurements.

The consuming project keeps its own review rules, content and workflow documentation.
