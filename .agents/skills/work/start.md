# Start: claim the task, then question it

1. Claim exactly the assigned task: `just tasks claim SLUG --as HANDLE`. If it is held, reserved
   for someone else or blocked by a dependency, say so on its thread and stop. Refused at a cap
   while it is reserved for you, work under the reservation: renew it within each hour
   (`just tasks reserve SLUG --as HANDLE`) and claim again after each renewal.
2. Work in the assigned checkout, under the project's branch and integration rules. Coordinate
   overlapping files, and keep other sessions' work and evidence.
3. Before any code, question the task: read the brief against the tree and the board as they are
   now, and answer two things on its thread.
   - Is it still needed? Already done, overtaken by another change, about a state that no longer
     exists, a case nobody has?
   - Is the brief's way the simplest that solves it? A problem met once is solved once: run the
     commands or a throwaway script, say what ran and what came of it, and stop, with no
     committed tool, tests or review round. Code is committed only for what will run again.

   A no to either goes to the orchestrator (`--to @orchestrator`) with the smaller alternative,
   which may be cancelling the task; do not build the brief as written while you wait
   ([connected.md](connected.md)). A brief is an instruction, not proof the work is worth doing.
4. From then on follow the description exactly. If it cannot be fulfilled, say what prevents it
   and block it, naming what it waits on: `just tasks move SLUG blocked --on THING --why TEXT
   --as HANDLE`, THING one of `decision:NAME`, `check:NAME`, `task:SLUG` or `parked`. Never change
   the scope silently or invent a replacement task.

## Fast bug fixes

A bug fix is recovery, not a feature cycle: one agent, a concrete risk assessment and the
smallest regression checks that cover the repair, with no review or gate subtasks. Restore what
is broken before unrelated test or gate cleanup, and record honestly what still fails. After
recovery another agent makes a bounded postmortem (the root cause and how it would recur), which
blocks nothing. This supersedes the feature cycle in the other pages.

Next: [build.md](build.md).
