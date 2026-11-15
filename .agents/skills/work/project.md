# An executor in this repository

This repository's own standing instructions for an executor, beside the shipped skill, which
`symphony init` never writes. A rule is said once, in [RULES.md](../../../RULES.md), the
[root guide](../../../AGENTS.md) or [CHECKS](../../../docs/CHECKS.md): this page holds only
what an executor needs that none of them says, and a prompt repeats none of it.

## Before the first edit

- Read RULES.md whole, and in CHECKS the entry for your kind of change
  ([evidence by kind of change](../../../docs/CHECKS.md#evidence-by-kind-of-change)) and
  [picking the focused set](../../../docs/CHECKS.md#picking-the-focused-set): they name the
  checks your evidence needs, and when each is run.
- Work on the branch the claim names, the task's slug with its slashes kept.

## While building

- Read what your tests cover of the code you added, and add the cases they miss: the edge cases
  of a function or a module are cheap to cover, a whole flow only on its usual path.
- Nothing you start touches live state: a sweep or a migration runs against a scratch repository
  and a scratch board, never the real worktrees or the live board.
- A consuming project's checkout on this machine is read-only to you: read it, change nothing in
  it.
- Never push. The land pushes main, and nothing else of a task is pushed.
- The machine is shared by many agents and its disk is limited. Start a long command in the
  background and wait for it: a second start of a run that is still going doubles the load and
  finishes later. Copy no build folder, and remove the `build/` output of your worktree when you
  finish.

## The report

The post that moves the task to review, and your last message to the orchestrator, say the same:
the commits and the tip, what changed, each check run with its result, what the brief asked for
that was left out and why, and whether the branch changed what the client is built from (a
release input), so the orchestrator knows what the regenerator will rebuild after the land.
