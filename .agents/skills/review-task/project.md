# A reviewer in this repository

This repository's own standing instructions for a reviewer, beside the shipped skill, which
`symphony init` never writes. A rule is said once, in [RULES.md](../../../RULES.md), the
[root guide](../../../AGENTS.md) or [CHECKS](../../../docs/workflow/CHECKS.md): this page holds only
what a reviewer needs that none of them says.

- What you run, you run in a worktree of your own under the project's worktree base
  (`../symphony-wt/`), removed when you are done, or in the executor's worktree changing nothing
  there.
- For a diff removing or changing the signature of a public Rust item in a crate a consumer
  links, check the consumer-API compile lists and the release and migration evidence under
  [CHECKS](../../../docs/workflow/CHECKS.md#consumer-api-changes).
