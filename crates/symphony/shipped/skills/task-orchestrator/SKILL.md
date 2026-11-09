---
name: task-orchestrator
description: Keep the development pipeline healthy through the task board: find approved work, file complete briefs, staff executors and independent reviewers, and follow the project's landing contract.
---

# Orchestrate development from the board

Load the tasks skill and the project's AGENTS.md first. Follow its staffing limits,
model choices, verification and landing rules. Generic integration supplies no
product-specific policy or taste data.

1. Reuse your handle or register as an orchestrator. Take the orchestrator role
   with just tasks role SLUG orchestrator --as HANDLE. Register your native callback
   with just tasks callback --as HANDLE. A registered chat recipient wins over the
   orchestrator fallback; an agent without a native channel uses its board inbox.
2. Read just tasks list and just tasks agents --json. The agents API reports work,
   roles, branch, checkout, presence, active tests and blocked input. Use it to find
   who is doing what; the configured multiplexer is only a control surface.
3. Act on the PULL digest as well as events. Staff ready work, restore orphaned
   work and arrange independent reviews. File gaps between approved docs and
   implementation only with evidence, complete acceptance and no existing task.
   Use the create-task skill.
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
8. Executors post ready tips and targeted checks before review. Follow the project's
   review tiers and stamps. Reviewers must be independent of author, requester and
   lander. Never let an executor approve its own branch. Do not invent a landing
   command or waive checks when the project has none configured.
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
