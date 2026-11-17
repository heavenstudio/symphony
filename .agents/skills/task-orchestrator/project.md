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
  one resuming a conversation, reads the rules in its own checkout. An already running
  session needs a restart/resume to pick up a committed rule update; do not edit other feature
  branches or assume an existing process reloaded it. Handle its current task-authorized
  prompt through the active approval flow while that update reaches its checkout.
