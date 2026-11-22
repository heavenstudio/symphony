# Workflow

How work moves: review, the board, checks, releases and the records of what ran. The other areas are in the [index](../README.md).

- [Concept review](REVIEW.md) — review cards, comments, questions and the owner queue.
- [Task board](TASKS.md) — the board, sessions, roles and agent communication; the pages are
  [the task board](../product/BOARD.md) and [a task's page](../product/TASK.md).
- [Workflow definitions](WORKFLOW.md) — a project's own delivery workflow: the format, its
  validation, activation, admission and wire records.
- [Checks](CHECKS.md) — what a task in this repository proves before review, and the release run's
  full verification.
- [Releases](RELEASES.md) — taking a release on a project, then how a release is cut by the
  release run or out of cycle, named and read, and the release run's schedule.
- [Machine build and test work](MACHINE_WORK.md) — observed build/test phases, the board's build
  and test recorder (`tasks run`), and advisory machine targets.
- [Runs](RUNS.md) — the day files of a project's runs and their steps: the line each writes,
  where, for how long, and how a reader takes them.
- [Agent activity](AGENT_ACTIVITY.md) — the day files of the agents' health changes and their
  hours: sessions started, stale and lost, running by role and model, and the machine's load.

## Analysis

An analysis holds a proposal and its evidence: an approved design can still await implementation, and its approval alone does not make it current behavior.

- [Where decision cards live](analysis/cards-in-their-documents.md) — a card in the document it
  decides about, and the folders by area.
- [Needed stamps](analysis/needed-stamps.md) — what a land requires of a task, named by an
  assessor from one catalogue.
- [A release candidate held still](analysis/release-candidate.md) — how a red of the full run is
  fixed so that a release comes.
- [The release pipeline, redesigned](analysis/release-pipeline.md) — a night of red release runs
  read by run, a pre-certified CI proposal checked against the stamps, and the options with what
  was chosen of them, a hosted run on GitHub among them.
- [A project's task workflow](analysis/project-workflow.md) — the proposed project-defined steps,
  the existing gate boundary, and migration that preserves the default flow.
- [Configurable delivery workflows](analysis/configurable-delivery-workflow.md) — the proposed C
  contract: separate phase/state, authoritative operations, migration and implementation briefs.
- [Artifact approvals at workflow transitions](analysis/revision-bound-transition-guards.md) —
  proposed input-bound human decisions, invalidation and atomic project-guard admission.
- [Landing through GitHub pull requests](analysis/github-pr-landing.md) — the proposed contract of
  a land delivered by a pull request.
- [Artifacts attached, not committed](analysis/attached-artifacts.md) — where a task's pictures
  and evidence live, how a link names one, and how long each is kept.
- [The slow tests, reviewed](analysis/slow-tests.md) — a time per test for the release run's six
  slowest binaries, what each slow test proves, and what is kept, made faster, merged or removed.
- [Updating a project's Symphony](analysis/symphony-update.md) — how a consuming project takes a
  newer Symphony.
