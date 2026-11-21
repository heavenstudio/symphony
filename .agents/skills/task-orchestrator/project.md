# An orchestrator in this repository

This repository's own standing instructions for an orchestrator, beside the shipped skill, which
`symphony init` never writes.

- Register your own callback early in a full session, and check that full-session executors and
  reviewers do the same before working ([the connected stage](../work/connected.md)).
- A `BLOCKED` notice is an observation of a session you oversee, not leave to approve: inspect
  that terminal's exact prompt, task and command as they are now before approving what the task
  authorizes. Leave sessions another orchestrator owns to that owner.
- Fix a repeated permission prompt durably, with a narrow project rule for the named command
  ([the check rules](../../../.codex/rules/checks.rules), which keep `--one --` before the recipe
  and carry examples of each form) and guidance for its caller. Keep arbitrary shells, recorder
  commands and whole-directory access out of the allowlist.
- A Codex session that prompts for a check: first read the submitted command against the root
  guide's [forms](../../../AGENTS.md#checks-from-a-codex-session) (a redirection, a quoted prefix
  word, concatenated quotes or an unmatched compound part each explain a prompt). Codex loads the
  rules of its own checkout at process start when the project is trusted
  ([the rules reference](https://developers.openai.com/codex/rules)), so a process older than a
  rule needs a fresh process or a safe resume, and an untrusted project its trust resolved
  through the active approval flow. Never forcibly cycle an active worker, edit its branch or
  change global trust.
- A task that moves the board's schema (it raises `VERSION` in `crates/tasks/src/schema.rs`):
  before its land, read `just release --status` and hold it while a run is in progress or a
  release candidate is open, and for nothing else; after its land, run `just release` at once,
  without asking, unless the land's own `--due` started a run
  ([the cadence](../../../docs/RELEASES.md#the-cadence)).
- Bending a rule is an orchestrator's decision: where a rule here would hold development back
  for no gain the task needs (a size or first-visit budget at its line once the cut was made
  first, a wait the work does not need), decide it without a card and without asking, and say on
  the task's thread which rule gave way, why, and what was measured. A quality gate that gives
  way is recorded with `just tasks gate`
  ([gate decisions](../../../docs/TASKS.md#gate-decisions)): a raised budget or cap is still a
  commit of its own with its account, judged by the task's reviewer, and a waivable stamp is
  waived for one tip. What stays the owner's word is
  [who runs it](../../../docs/RELEASES.md#who-runs-it): pointing `current` back, restarting the
  shared server by hand, a release run by hand outside the cadence and the schema rule, and
  turning publication or a project's delivery on.
