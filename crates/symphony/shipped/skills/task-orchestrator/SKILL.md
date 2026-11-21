---
name: task-orchestrator
description: Keep the development pipeline healthy through the task board: find approved work, file complete briefs, staff executors and independent reviewers, and follow the project's landing contract.
---

# Orchestrate development from the board

Load the tasks skill, the project's `AGENTS.md` and `project.md` beside this file where the
project has one. Follow the project's staffing limits, model choices, verification and landing
rules; `just tasks limits` prints the limits and each role's model in force. Staff, brief and
land by how an executor carries the work: [fast bug fixes](../work/start.md#fast-bug-fixes),
[parent feature delivery](../work/finish.md#parent-feature-delivery) and
[evidence is the gate](../work/build.md#evidence-is-the-gate).

1. Reuse your handle or register as an orchestrator (`just tasks register --kind orchestrator`),
   and pass it on every command as `--as HANDLE`. A task you file is yours from that write, and
   `just tasks role SLUG orchestrator --as HANDLE` takes one nobody holds; the board gives open
   work with no orchestrator to one that stays, saying so in a `GIVEN` line, and holds you to
   `[limits] orchestrator_tasks` open tasks, past which you take none but a red. To hear the
   board, a session that can hold a background command runs `just tasks wait --as HANDLE` in the
   background, and again each time it exits; one that cannot registers its native callback
   (`just tasks callback --as HANDLE`), and an agent with neither reads its inbox.
2. Read `just tasks list --as HANDLE` and `just tasks agents --json --as HANDLE`: who is doing
   what, with each agent's task, checkout, presence, active tests and blocked input. The
   multiplexer is only a control surface.
3. Act on the PULL digest as well as events. Pushes say only what changed: at each decision
   point (resuming, before staffing, after a verdict or a landing) run
   `just tasks pull --as HANDLE` for the whole digest, with any owner verdict nobody received.
   Staff a red (`tests/main-red-*`, `tests/release-red-*`, `tests/nightly-red-*`) before anything
   else, by what its text says of where its fix starts and what it merges; a test red on load is
   discarded in its red's own fix, and no other work is held back to keep the load down for a
   run. Then staff ready work, restore orphaned work and arrange independent reviews. File a gap
   between approved documents and the implementation only with evidence, complete acceptance and
   no existing task (`just tasks create`, by the create-task skill).
4. Follow the project's integration and dependency policy before starting, and the stacking the
   task states. Coordinate file ownership, a separate checkout per executor, and record actual
   conflicts on the thread.
5. Register each agent a handle of its own, `just tasks register --kind worker --parent HANDLE`
   (`--kind reviewer` for a reviewer or an assessor), and give it only its prompt, on the task
   itself, with no work or review subtask: `/work SLUG HANDLE`, `/review-task SLUG HANDLE` or
   `/assess-task SLUG HANDLE` (Codex: `$` in place of `/`). The skill, the slug and the handle
   are the whole prompt.
   - What is this task's alone goes in its brief or on its thread (`--to @executor`,
     `--to @reviewer`): a task touching the same files, a finding since the brief, the tip to
     review, a file to leave alone.
   - What holds for every task is standing: the project's guides and rules, or, for one role
     alone, `project.md` beside that role's skill (under `.agents/skills`, a file init never
     writes), committed, never a file outside the repository. An instruction you write a second
     time is standing.
   - Start an assessor on each task you file, and again each time it requests review, before
     any reviewer, on the model `just tasks limits` names for the assessor. It writes the stamps
     the task needs at the tip it read; never write them yourself. Staff a reviewer only where
     they name review (`just tasks stamps SLUG --as HANDLE`).
   - A later review round resumes the same reviewer with the new tip, posted `--to @reviewer`;
     resumed, it is still a fresh reviewer for the land. Only one that cannot be resumed is
     replaced.
6. Choose a supported launch method under project policy: an in-process agent, a headless run,
   or `just tasks launch SLUG --kind claude|codex` for work that needs a terminal or outlives
   your session. Pass the model `just tasks limits` names for the role, and its required
   effort. Never run tests in another agent's terminal.
7. A full-session worker registers its own callback; an in-process one never binds its
   parent's, and reads its inbox between steps. When a headless run ends, read its task's thread
   before resuming its recorded conversation.
8. Executors post ready tips with their evidence before review; a brief asks for focused
   checks, never the full suite. A reviewer is independent of author, requester and lander: the
   board refuses the verdict of a session that executed the task or a slice of it, and the
   reviewer's role to one that orchestrates, orchestrated or filed it, you included.
   - Land with `just land SLUG --as HANDLE` once every stamp the task needs stands on its tip,
     in the background, never a merge by hand: it checks the merge, closes the task and pushes
     the default branch, and a project's own `land` recipe replaces it. A
     missing stamp refuses, naming who gives it: its executor its tests or pictures at the tip,
     you an assessor of the tip or a fresh reviewer, the owner its approve on the task's page.
     Never waive a check it refuses on. Run it again after an interrupted land.
   - A task whose work landed with another task's branch, leaving none of its own, is closed
     with `just tasks done SLUG "what landed, where" --covered-by OTHER --as HANDLE`, never a
     verdict of yours.
   - Cancel work no longer wanted: `just tasks move SLUG cancelled --reason "why" --as HANDLE`.
9. Post only a review-ready tip, a request for review or a block, a collision, a finding, a
   verdict or a question, addressed `--to @executor`, `--to @reviewer` or a handle. Never
   acknowledge a message or repeat a milestone on a parent and its subtask.
10. Name the dependency, verdict or landing you await on the thread, then end the turn; do not
    poll. `just tasks restore SLUG` resumes recorded work and `just tasks cleanup --dry-run`
    shows what cleanup would close. Never remove another agent's checkout or close its terminal
    by hand.

Owner approvals travel with the ask: quote the exact words, apply them within scope and never
ask again. An unseen design or artifact still needs the project's review.
