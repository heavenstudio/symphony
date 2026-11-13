# Start: claim the task, then question it

1. Claim exactly the assigned task: `just tasks claim SLUG --as HANDLE`. If it is held, reserved
   for someone else or blocked by a dependency, say so on its thread and stop dependent work. If
   the claim is refused at a cap while the task is reserved for you, work under the reservation:
   it lapses after an hour, so renew it with `just tasks reserve SLUG --as HANDLE` within each
   hour and claim again after each renewal.
2. Work in the assigned checkout, under the project's branch and integration rules. Coordinate
   overlapping files, and keep other sessions' work and evidence.
3. Before any code, question the task against the tree and the board as they stand now:
   - Is it still needed? Done already on the default branch, covered by another task, or
     overtaken by a later decision?
   - Is the brief's way the simplest thing that solves what it asks? A smaller change, a
     mechanism that already exists, fewer files?

   A no to either goes on its thread to the orchestrator (`--to @orchestrator`) with the smaller
   alternative, which may be cancelling the task; wait one round for the answer before building.
4. From then on follow the description exactly. If it cannot be fulfilled, say what prevents it
   and move the task blocked. Resolve conflicting instructions under project policy; never change
   the scope silently or invent a replacement task.

## Fast bug fixes

A bug fix is recovery, not a feature cycle: one agent, a concrete risk assessment and focused
regression checks. A simple fix takes the quick delivery path; a complex or high-risk repair may
need the stamps the project names. Restore broken functionality before unrelated test or gate
cleanup, and record honestly what still fails. No separate review or gate subtasks. After
recovery another agent makes a bounded root-cause and recurrence-prevention postmortem, reusing
a relevant analysis task; it does not block recovery or start another delivery cycle. This rule
supersedes the feature cycle in the other pages.

Next: [build.md](build.md).
