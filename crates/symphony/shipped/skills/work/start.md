# Start: claim the task, then question it

1. Claim exactly the assigned task, its `claim` edge: `just tasks claim SLUG --as HANDLE`. If it
   is held, reserved
   for someone else or blocked by a dependency, say so on its thread and stop dependent work. If
   the claim is refused at a cap while the task is reserved for you, work under the reservation:
   it lapses after an hour, so renew it with `just tasks reserve SLUG --as HANDLE` within each
   hour and claim again after each renewal.
2. Work in the assigned checkout, under the project's branch and integration rules. Coordinate
   overlapping files, and keep other sessions' work and evidence.
3. Before any code, question the task: read the brief against the tree and the board as they
   are now, and answer two things on its thread.
   - Is it still needed? Already done, overtaken by another change, about a state that no longer
     exists, a case nobody has?
   - Is the brief's way the simplest that solves it? A problem met once is solved once: run the
     commands or a throwaway script, say what ran and what came of it, and stop, with no
     committed tool, tests or review round. Code is committed only for what will run again.

   A no to either goes to the orchestrator (`--to @orchestrator`) with the smaller alternative,
   which may be cancelling the task; do not build the brief as written while you wait for the
   answer ([connected.md](connected.md): your callback or inbox, and how to wait). A brief is an
   instruction, not proof the work is worth doing.
4. From then on follow the description exactly. If it cannot be fulfilled, say what prevents it
   and take its `block` edge, naming what it waits on (`just tasks move SLUG blocked` with
   `--on decision:NAME`, `check:NAME`, `task:SLUG` or `parked`, and `--why TEXT`). Resolve
   conflicting instructions under project policy; never change the scope silently or invent a
   replacement task.

## Fast bug fixes

A bug fix is recovery, not a feature cycle: one agent, a concrete risk assessment and focused
regression checks. A simple fix takes the quick delivery path; a complex or high-risk repair may
need the stamps the project names. Restore broken functionality before unrelated test or gate
cleanup, and record honestly what still fails. No separate review or gate subtasks. After
recovery another agent makes a bounded root-cause and recurrence-prevention postmortem, reusing
a relevant analysis task; it does not block recovery or start another delivery cycle. This rule
supersedes the feature cycle in the other pages.

Next: [build.md](build.md).
