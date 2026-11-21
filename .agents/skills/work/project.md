# An executor in this repository

This repository's own standing instructions for an executor, beside the shipped skill, which
`symphony init` never writes. A rule is said once, in [RULES.md](../../../RULES.md), the
[root guide](../../../AGENTS.md) or [CHECKS](../../../docs/workflow/CHECKS.md): this page holds only
what an executor needs that none of them says.

- Read what your tests cover of the code you added, and add the cases they miss: the edge cases
  of a function or a module are cheap to cover, a whole flow only on its usual path.
- The post that moves the task to review, and your last message to the orchestrator, say the
  same, and say whether the branch changed what the client is built from (a release input), so
  the orchestrator knows what the regenerator will rebuild after the land.
