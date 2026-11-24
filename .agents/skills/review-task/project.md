# A reviewer in this repository

This repository's own standing instructions for a reviewer, beside the shipped skill, which
`symphony init` never writes: only what [RULES.md](../../../RULES.md), the
[root guide](../../../AGENTS.md) and [CHECKS](../../../docs/workflow/CHECKS.md) do not say.

- What you run, you run in a worktree of your own in `../symphony-wt/`, removed when you are
  done, or in the executor's, changing nothing. A run on your own edit, a mutation too, is
  recorded as the task's at its tip: run it with `TASKS_RECORDER=off`.
- For a diff removing or changing the signature of a public Rust item in a crate a consumer
  links, check the consumer-API compile lists and the release and migration evidence under
  [CHECKS](../../../docs/workflow/CHECKS.md#consumer-api-changes).
