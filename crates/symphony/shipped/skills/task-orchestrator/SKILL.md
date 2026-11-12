---
name: task-orchestrator
description: Keep the development pipeline healthy through the task board: find approved work, file complete briefs, staff executors and independent reviewers, and follow the project's landing contract.
---

# Orchestrate development from the board

## Fast bug fixes

Use one agent, a concrete risk assessment and focused regression checks.
Simple fixes take the quick delivery path; complex or high-risk repairs may
need specific stamps. Restore broken functionality before unrelated test or
gate cleanup, recording remaining failures honestly. Do not create separate
review/gate subtasks. After recovery, another agent performs a bounded root-cause
and recurrence-prevention postmortem, reusing a relevant analysis task; it does
not block recovery or start another delivery cycle. This recovery rule supersedes blanket feature-cycle
instructions below.

## Parent feature delivery

A top-level task owns one feature branch and one delivery cycle. Subtasks are
small implementation slices, not independent releases. One agent continues the
feature by default; use another agent only for disjoint work that demonstrably
shortens the critical path. Commit slices into the feature branch, or merge
their worktree commits into it with the project's `just merge-feature FEATURE [SOURCE]`
where it has one: that refuses the default branch and runs no delivery hooks or stamps,
and `just land SLUG` (the Symphony's default, or the project's own `land`, which replaces
it) remains the completed feature delivery command. Run only focused checks needed for that slice;
no slice-level full build, full suite, reviewer staffing, certificate, stamps or
landing. Close the slice with its integration commit and focused result using
tasks done. When the complete feature is ready, the parent goes to review once
with its evidence, then lands. Review fixes stay with that parent and do not
create independent delivery rounds.
The board shows parents by default; subtasks are nested under their parent.
This rule supersedes conflicting per-subtask gate instructions.

## Evidence is the gate

A task's gate, parent or slice, feature or fix, is evidence that what it changed
works, not the full suite: the tests closest to the change and the project's
lint, posted with their exact commands and results, plus what the kind of change
calls for (before and after screenshots of a changed page, a measurement before
and after). A reviewer reruns those tests, never the suite, judges whether the
evidence is good enough for the change's risk, and records its approval as the
task's stamp. Higher risk asks for stronger evidence, never for the whole suite.
The full verification runs on the project's main line on its own schedule, such
as a nightly, and a red run there is filed as a task of its own. The project's
guidance names its focused checks, its stamp and its nightly.


Load the tasks skill and the project's AGENTS.md first. Follow its staffing limits,
model choices, verification and landing rules. Generic integration supplies no
product-specific policy or taste data.

1. Reuse your handle or register as an orchestrator. Take the orchestrator role
   with just tasks role SLUG orchestrator --as HANDLE. To hear the board, a session
   that can hold a background command runs just tasks wait --as HANDLE in the
   background, and again each time it exits: the lines a channel would carry,
   without its notice, and a vacant chat seat. One that cannot registers its native
   callback with just tasks callback --as HANDLE; a registered chat recipient wins
   over the orchestrator fallback. An agent with neither uses its board inbox.
2. Read just tasks list and just tasks agents --json. The agents API reports work,
   roles, branch, checkout, presence, active tests and blocked input. Use it to find
   who is doing what; the configured multiplexer is only a control surface.
3. Act on the PULL digest as well as events. Pushes say only what changed; at each
   decision point (resuming, before staffing, after a verdict or landing) run
   just tasks pull --as HANDLE for the whole digest as you hear it, with any owner
   verdict nobody received. Staff ready work, restore orphaned work and arrange
   independent reviews. File gaps between approved docs and implementation only
   with evidence, complete acceptance and no existing task. Use the create-task
   skill.
4. Follow the project's integration and dependency policy before starting. Honor
   explicit stacking and source-availability rules in the task. Coordinate file
   ownership and record actual conflicts on the thread.
5. Assign disjoint files and complete briefs. Put repeated instructions in skills.
   Register a worker handle, reserve the instruction with --for HANDLE, and give
   it only $work SLUG HANDLE (Claude: /work SLUG HANDLE). The work skill owns
   claiming, milestones and finishing. Use a separate checkout per executor.
6. Choose a supported launch method under project policy: an in-process agent,
   a headless run, or just tasks launch SLUG --kind claude|codex. Pass its required
   model and effort. A terminal is for work needing a terminal or outliving your
   session. Never run tests in another agent's terminal. Launch records retain
   conversations for restore.
7. Full-session workers register their own callbacks. In-process workers must
   not bind their parent's callback; they read their board inbox between steps.
   When a headless run ends, read its task thread before resuming its recorded
   conversation. A post to an ended session remains in its inbox.
8. Executors post ready tips and their evidence, the targeted checks and their
   results, before review; a brief asks for focused checks, never the full suite.
   Follow the project's review tiers and stamps. Reviewers must be independent of
   author, requester and lander. Never let an executor approve its own branch: the
   board refuses the verdict of a session that executed the task or a slice of it (a
   subtask filed without --instruction), and, but on a fast fix, the reviewer's role
   and the verdict of one that orchestrates it, orchestrated it or filed it, you
   included; a fresh reviewer gives it, as the one that claimed its --instruction
   review subtask does.
   Land an approved task with just land SLUG --as HANDLE, in the background (it
   merges and checks in a fresh worktree, which can take many minutes), never a
   merge by hand: by default it lands only the tip a fresh reviewer approved, checks
   the merge, closes the task and pushes the default branch; a project's own land
   recipe replaces it. Never waive a check it refuses on: its one line names what to
   fix, and an approve it cannot go by sends the task back to review. Run it again
   after an interrupted land: it finishes the push and the close.
   A bug fix whose risk calls for no reviewer is filed with just tasks create SLUG
   TITLE --fast-fix "the risk, in words"; you, never its executor, approve it on its
   posted evidence, naming the tip, and land it. One you executed yourself is
   approved by a fresh reviewer, as any other task.
9. Post only a review-ready tip, review/blocked move, collision, finding, verdict or
   question. Address action with --to @executor, --to @reviewer or a handle. Never
   acknowledge messages or duplicate milestones on parent and child threads.
10. Name the dependency, verdict or landing you await on the thread, then end the
    turn. Do not poll. Use just tasks restore SLUG to resume recorded work and
    just tasks cleanup --dry-run before board-managed cleanup. Do not remove
    another agent's checkout or close its terminal by hand.

Owner approvals travel with the ask: quote exact words and apply them within scope.
Do not ask again for an approved requirement. Unseen designs and artifacts still
need the project's review. Keep visual procedures, taste, shared numbering and
product-specific resources in project-local skills.
