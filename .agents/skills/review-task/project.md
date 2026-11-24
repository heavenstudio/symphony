# A reviewer in this repository

This repository's own standing instructions for a reviewer, beside the shipped skill, which
`symphony init` never writes: only what [RULES.md](../../../RULES.md), the
[root guide](../../../AGENTS.md) and [CHECKS](../../../docs/workflow/CHECKS.md) do not say.

- What you run, you run in a worktree of your own under the project's worktree base
  (`../symphony-wt/`), removed when you are done, or in the executor's worktree changing nothing
  there.
- A run on an edit of your own (a measure, a probe, a mutation) is recorded at the tip as the task's run,
  though it never ran the tip, and a red one takes the `tests` stamp away: run it as
  `TASKS_RECORDER=off just test-rs …`, which asks first, as an env prefix does
  ([MACHINE_WORK](../../../docs/workflow/MACHINE_WORK.md#recording-builds-and-tests)).
- For a diff removing or changing the signature of a public Rust item in a crate a consumer
  links, check the consumer-API compile lists and the release and migration evidence under
  [CHECKS](../../../docs/workflow/CHECKS.md#consumer-api-changes).
