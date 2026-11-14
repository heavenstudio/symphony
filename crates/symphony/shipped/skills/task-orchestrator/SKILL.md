---
name: task-orchestrator
description: Keep the development pipeline healthy through the task board: find approved work, file complete briefs, staff executors and independent reviewers, and follow the project's landing contract.
---

# Orchestrate development from the board

Load the tasks skill and the project's AGENTS.md first. Follow its staffing limits,
model choices, verification and landing rules. The limits and each role's model are
settings, `[limits]` and `[agents.models]` in its `symphony.toml`: `just tasks limits`
prints those in force and where each came from. Generic integration supplies no
product-specific policy or taste data. Staff, brief and land by how an executor carries
the work: [fast bug fixes](../work/start.md#fast-bug-fixes) (one agent, no review or gate
subtasks, a postmortem by another agent after recovery),
[parent feature delivery](../work/finish.md#parent-feature-delivery) (one agent per
feature, slices with no reviewer of their own, the parent reviewed and landed once) and
[evidence is the gate](../work/build.md#evidence-is-the-gate) (focused checks, never the
full suite).

1. Reuse your handle or register as an orchestrator. Take the orchestrator role
   with just tasks role SLUG orchestrator --as HANDLE; a task you file with just
   tasks create is yours from that write. Every open task has an orchestrator:
   once you have been live past the first live line and wait, watch or hold a task,
   the board gives you open work that had none (a leaving or lost orchestrator's
   among it) at your next look, saying so in one GIVEN line; follow those as your
   own. A session that only registered is given nothing. To hear the board, a session
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
   verdict nobody received. Staff a red (tests/main-red-*, tests/release-red-*,
   tests/nightly-red-*) before anything else. One filed as a fast fix is yours to
   approve when its fix changes no test or check, the failing tests unchanged and
   green at its tip; the land refuses your approve on a fix that changes a test, a
   check, a budget or a run step ([land] check_paths), and you staff a fresh reviewer
   for it. A test red on load (it passes alone, or its failure is a wall-clock bound,
   a sleep-ordered step or a timeout under load) is discarded in its red's own fix,
   with a line on the red's thread naming it, the evidence and what it proved; the
   release waits for no card, rerun or rewrite of it, and you hold no other work back
   to keep the load down for a run. The owner, 2026-11-13:
   "A rerun should not fail or pass based on machine load. If it does, discard the tests and move on".
   A red whose release run held its commit as the release candidate says so in
   its text: brief its executor to start from the local branch release-candidate, never
   main, and to merge no main before its approve; its own land moves the candidate, and
   the land that answers the last red starts its rerun. Your approve of its fast fix
   names the fix's full tip, the commit the candidate moves to. Rerunning or dropping a
   candidate by hand is an orchestrator's retry the project's guidance names. A red
   whose fix landed with another red's is closed as covered by that landing, which
   answers it once that land moved the candidate; closed as covered by any other
   landing, it drops the candidate, its fix on main and not in it. A red fixed outside
   the repository is answered by cancelling its task, saying where; an approved one
   still waits for its land. Staff ready work, restore
   orphaned work and arrange independent reviews. File gaps between approved docs and
   implementation only with evidence, complete acceptance and no existing task. Use
   the create-task skill.
4. Follow the project's integration and dependency policy before starting. Honor
   explicit stacking and source-availability rules in the task. Coordinate file
   ownership and record actual conflicts on the thread.
5. Assign disjoint files and complete briefs. Put repeated instructions in skills.
   Register a worker handle, reserve the instruction with --for HANDLE, and give
   it only $work SLUG HANDLE (Claude: /work SLUG HANDLE), a reviewer only
   $review-task SLUG HANDLE (Claude: /review-task SLUG HANDLE). The work skill owns
   claiming, milestones and finishing, the review-task skill the verdict. Use a
   separate checkout per executor.
6. Choose a supported launch method under project policy: an in-process agent,
   a headless run, or just tasks launch SLUG --kind claude|codex. Pass the model
   just tasks limits names for its role, and its required effort. A terminal is for
   work needing a terminal or outliving your
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
   A task whose work landed with another task's branch, leaving none of its own,
   is closed by its executor, or by you holding its orchestrator role, with just
   tasks done SLUG "what landed, where" --covered-by OTHER, never a verdict of
   yours: it is no approve, and a land refuses it. Cancel work no longer wanted
   with just tasks move SLUG cancelled --reason "why".
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
