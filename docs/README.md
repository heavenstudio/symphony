# Symphony documentation

- [DECISIONS](DECISIONS.md) — Symphony's own product and engineering decisions, one card each;
  the evidence behind a card is under [analysis](analysis/).
- [REVIEW](REVIEW.md) — review cards, comments, questions and the owner queue.
- [TASKS](TASKS.md) — the board, sessions, roles and agent communication.
- [CHECKS](CHECKS.md) — what a task in this repository proves before review, and the nightly's
  full verification.
- [RELEASES](RELEASES.md) — how a release is cut by the nightly or out of cycle, named and read,
  and the nightly's schedule.
- [MACHINE_WORK](MACHINE_WORK.md) — observed build/test phases, the board's build and test
  recorder (`tasks run`), and advisory machine targets.
- [RUNS](RUNS.md) — the day files of a project's runs and their steps: the line each writes,
  where, for how long, and how a reader takes them.
- [PLUGIN_CALLS](PLUGIN_CALLS.md) — each call into a project's plugins timed, and sent through a
  JSON round trip inside the process: the two settings, the record and how to read it.

Agents working in this repository follow the standing rules in [RULES](../RULES.md).

The consuming project keeps its own review rules, content and workflow documentation.
