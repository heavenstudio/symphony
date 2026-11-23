# An orchestrator in this repository

This repository's own standing instructions for an orchestrator, beside the shipped skill, which
`symphony init` never writes.

- Keep about seven tasks being built, not waiting on review or the owner, while ready work
  remains, whatever usage limit you see (RULES.md `workflow/no-slowing-for-usage-limits`). Each
  holds a [slot](../../../docs/workflow/TASKS.md#slots); PULL's `low_volume` line says when you
  hold fewer than `[limits] low_volume`.
- Work a task needs before it can finish is a task of its own: file and staff it at once, without
  asking, and name it on the blocked task's thread and, on this board, as that task's `task:SLUG`
  block ([what a task waits on](../../../docs/workflow/TASKS.md#what-a-task-waits-on)). Another
  project's work (a consuming project's pin, code or board) is filed on its own board and staffed
  on the models `just tasks limits` names in its checkout, and the acts on that board that follow
  stay yours. Only a decision the owner alone can make goes to the owner, as a
  [card](../../../docs/workflow/REVIEW.md#where-a-card-is-written).
- Register your own callback early in a full session, and check that full-session executors and
  reviewers do too before working ([the connected stage](../work/connected.md)).
- A `BLOCKED` notice is an observation, not leave to approve: inspect that terminal's prompt,
  task and command as they are now before approving what the task authorizes. Leave another
  orchestrator's sessions to it.
- Fix a repeated permission prompt durably: a narrow rule for the named command in the form of
  [the check rules](../../../.codex/rules/checks.rules), with its examples, and guidance for its
  caller. Keep arbitrary shells, recorder commands and whole-directory access out of the allowlist.
- A Codex session that prompts for a check: read its command against the root guide's
  [forms](../../../AGENTS.md#checks-from-a-codex-session) first. Codex loads its checkout's
  [rules](https://developers.openai.com/codex/rules) at process start when the project is
  trusted: a process older than a rule needs a fresh process or a safe resume, an untrusted
  project its trust resolved through the active approval flow. Never forcibly cycle an active
  worker, edit its branch or change global trust.
- A task that moves the board's schema (raises `VERSION` in `crates/tasks/src/schema.rs`): while
  `just release --status` shows a run in progress or a release candidate open, and for nothing
  else, hold its last stamp and its reviewer's start. Once the board's landed line is on its
  thread, run `just release` at once, without asking, unless the land's own `--due` started a run
  ([the cadence](../../../docs/workflow/RELEASES.md#the-cadence)).
- Bending a rule here that holds development back for no gain the task needs (a size or
  first-visit budget at its line once the cut came first, a wait the work does not need) is your
  decision, without a card or asking: say on the task's thread which rule gave way, why, and what
  was measured, and record a quality gate that gives way with `just tasks gate`
  ([gate decisions](../../../docs/workflow/TASKS.md#gate-decisions)). What stays the owner's word
  is [who runs it](../../../docs/workflow/RELEASES.md#who-runs-it): pointing `current` back,
  restarting the shared server by hand, a release run by hand outside the cadence and the schema
  rule, and turning publication or a project's delivery on.
