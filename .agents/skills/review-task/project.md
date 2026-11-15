# A reviewer in this repository

This repository's own standing instructions for a reviewer, beside the shipped skill, which
`symphony init` never writes. A rule is said once, in [RULES.md](../../../RULES.md), the
[root guide](../../../AGENTS.md) or [CHECKS](../../../docs/CHECKS.md): this page holds only
what a reviewer needs that none of them says, and a prompt repeats none of it.

- Read RULES.md whole, and in CHECKS what the change's kind shows
  ([evidence by kind of change](../../../docs/CHECKS.md#evidence-by-kind-of-change)): it is what
  the evidence is judged against.
- What you run, you run in a worktree of your own under the project's worktree base
  (`../symphony-wt/`), removed when you are done, or in the executor's worktree changing nothing
  there.
- What [the executor's page](../work/project.md#while-building) says of live state, a consuming
  project's checkout, pushing and the machine's load holds for you as well.
- Your last message to the orchestrator gives the verdict, the tip it names and each defect.
