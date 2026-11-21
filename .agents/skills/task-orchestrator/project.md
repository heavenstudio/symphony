# An orchestrator in this repository

Read this page with the orchestrator skill, as the [root guide](../../../AGENTS.md) requires.

- Register your own callback early in a full session. Check that full-session executors and
  reviewers do the same before working; an in-process assessor or worker reads its inbox and
  never registers its parent's channel ([the connected stage](../work/connected.md)).
- A `BLOCKED` notice is an observation of a session you currently oversee. Inspect that
  terminal's current exact prompt, task and command before approving an action the task
  authorizes. A delayed notice does not authorize the prompt now on screen. Leave sessions
  owned by another orchestrator to that owner; a delegated coordinator keeps its workers.
- Fix repeated permission prompts durably with a narrow project rule for the named command
  and guidance for its caller. Keep arbitrary shells, recorder commands and whole-directory
  access out of the allowlist. The project's [check rules](../../../.codex/rules/checks.rules)
  require `--one --` before the recipe so an allowed prefix cannot run a second recipe or
  reinterpret later words as command-runner options. They show positive and negative examples;
  keep those and the installed-parser regression current when adding a recurring check.
- Codex loads project rules at process startup when its project configuration is trusted
  ([the rules reference](https://developers.openai.com/codex/rules)). A fresh process, including
  one resuming a conversation, reads the rules in its own checkout. First check the exact
  invocation against the root guide's [command forms](../../../AGENTS.md#this-repository-as-its-own-project):
  redirection, quoted prefix words, concatenated message quotes or an unmatched compound part
  can explain a prompt even when the rules loaded. Inspect the submitted tool command;
  displayed logical arguments and rule-file presence alone do not establish what was evaluated.
  `codex execpolicy check` evaluates the supplied argv and inline examples; it does not prove
  that a running session loaded those files or test that session's shell splitting.
  If a plain supported form still prompts, inspect startup time, rule availability in that
  checkout and project trust through read-only metadata. A process that predates its rule
  update needs a fresh process or safe resume after the update reaches its checkout; an
  untrusted project layer needs its trust resolved through the active approval flow. Never
  forcibly cycle an active worker, edit its feature branch or change global trust. Handle its
  current task-authorized prompt through the active approval flow; a redirected prompt alone
  is no reason to restart or repeat a recorded check.
- A task that moves the board's schema (it raises `VERSION` in `crates/tasks/src/schema.rs`):
  before its land, read `just release --status` and hold it while a run is in progress or a
  release candidate is open, and for nothing else (not for `current` to serve the last schema);
  after its land, run `just release` at once, without asking, unless the land's own `--due`
  started a run ([the cadence](../../../docs/workflow/RELEASES.md#the-cadence)).
- Bending a rule is an orchestrator's decision, given by the owner on 2026-11-19: where a rule
  here would hold development back or slow it for no gain the task needs (a size or first-visit
  budget at its line once the cut was made first, a wait the work does not need), decide it
  without a card and without asking, and say on the task's thread which rule gave way, why, and
  what was measured. A quality gate that gives way is recorded with `just tasks gate`, where the
  assessor, the reviewer and the land read it ([gate decisions](../../../docs/workflow/TASKS.md#gate-decisions)):
  a raised budget or cap names its limit and number, and is still a commit of its own with its
  account, judged by the task's reviewer; a waivable stamp is waived for one tip. What stays the
  owner's word is unchanged: pointing `current` back, restarting the shared server by hand,
  running a release by hand outside the cadence and the schema rule, and turning publication or a
  project's delivery on.
