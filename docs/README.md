# Symphony documentation

- [Product specs](product/README.md) — what each page of the app is for and how it behaves, with
  pictures; the form of a spec and how its pictures are taken.
- [The project file](PROJECT_FILE.md) — `symphony.toml`: every table and key a project can set,
  with its default.
- [Decision register](DECISIONS.md) — Symphony's own product and engineering decisions, one card
  each; the evidence behind a card is under [Analysis](analysis/).
- [A project's task workflow](analysis/project-workflow.md) — options for project-defined steps,
  the existing gate boundary, and migration that preserves the default flow.
- [Managed PostgreSQL](analysis/hosted-postgresql.md) — the proposed hosted connection,
  connection budget, database boundary and PostgreSQL 16/18 validation contract.
- [Configurable delivery workflows](analysis/configurable-delivery-workflow.md) — the proposed
  C contract: separate phase/state, authoritative operations, migration and implementation briefs.
- [Execution providers and durable attempts](analysis/remote-execution-attempts.md) — a proposal
  for project-owned execution adapters, durable recovery and authorized attempt history.
- [Concept review](REVIEW.md) — review cards, comments, questions and the owner queue.
- [Task board](TASKS.md) — the board, sessions, roles and agent communication.
- [Checks](CHECKS.md) — what a task in this repository proves before review, and the release run's
  full verification.
- [Releases](RELEASES.md) — how a release is cut by the release run or out of cycle, named and
  read, and the release run's schedule.
- [Machine build and test work](MACHINE_WORK.md) — observed build/test phases, the board's build
  and test recorder (`tasks run`), and advisory machine targets.
- [Runs](RUNS.md) — the day files of a project's runs and their steps: the line each writes,
  where, for how long, and how a reader takes them.
- [Agent activity](AGENT_ACTIVITY.md) — the day files of the agents' health changes and their
  hours: sessions started, stale and lost, running by role and model, and the machine's load.
- [Plugin calls](PLUGIN_CALLS.md) — each call into a project's plugins timed, and sent through a
  JSON round trip inside the process: the two settings, the record and how to read it.
- [Plugin protocol](PLUGIN_PROTOCOL.md) — a plugin answering from a process of its own: the
  setting, the seams it answers, its process, its commands, a register read in one request, the
  line, each call's minor, the versions and the wire's shape.
- [Hosted ingress and board readiness](analysis/hosted-ingress.md) — proposed trusted proxy and
  visitor admission, project permissions, foreground lifecycle and readiness; the local gate stays.
- [The Mac app](../apps/symphony-desktop/AGENTS.md) — the desktop app's guide: the servers it
  follows and how it finds them, what it counts and notifies, where a server's window may go, and
  what is not in it yet; why it is built so is [the desktop client
  analysis](analysis/desktop-client.md).

Agents working in this repository follow the standing [Rules](../RULES.md).

The consuming project keeps its own review rules, content and workflow documentation.
