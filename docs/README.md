# Symphony documentation

- [Decision register](DECISIONS.md) — Symphony's own product and engineering decisions, one card
  each; the evidence behind a card is under [Analysis](analysis/).
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
- [PLUGIN_CALLS](PLUGIN_CALLS.md) — each call into a project's plugins timed, and sent through a
  JSON round trip inside the process: the two settings, the record and how to read it.
- [PLUGIN_PROTOCOL](PLUGIN_PROTOCOL.md) — a plugin answering from a process of its own: the
  setting, its process, the line, each call's minor, the versions and the wire's shape.

Agents working in this repository follow the standing [Rules](../RULES.md).

The consuming project keeps its own review rules, content and workflow documentation.
