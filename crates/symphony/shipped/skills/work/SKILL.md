---
name: work
description: Do one task from the board by its description alone. Registers (or reuses the handle the task is reserved for), claims the task, follows its description exactly, and finishes by the step the description names. An agent's whole prompt is `/work SLUG HANDLE` (Codex `$work SLUG HANDLE`). Trigger on /work SLUG or $work SLUG.
---

# /work SLUG HANDLE

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
and the project's landing (`land` where it has one) remains the completed feature
delivery command. Run only focused checks needed for that slice;
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


Do the one task named by the prompt. Its board description supplies the scope,
acceptance and finishing step. Read the destination project's instructions first:
its tools, permissions, staffing, review, verification and landing policy apply.
This generic skill supplies board mechanics, not a build or approval policy.

## Start

1. Reuse HANDLE from the prompt, the task's reservation or TASKS_SESSION. If none
   exists, register with just tasks register --kind worker (a review uses reviewer).
   Read just tasks show SLUG under that handle before acting.
2. Pass the handle on every board command. Codex uses just tasks ... --as HANDLE,
   with the handle last; other clients may use TASKS_SESSION as supported by their
   project launcher. Check the active command permissions before choosing a form.
3. Claim exactly the assigned task with just tasks claim SLUG. If it is held,
   reserved for someone else or dependency-blocked, report that on its thread and
   stop dependent work. A reservation lapses after an hour; renew with just tasks
   reserve SLUG while legitimately waiting under a reservation.
4. Follow the description exactly. If it cannot be fulfilled, say what prevents
   it and move the task blocked. Resolve conflicting instructions under project
   policy; do not silently change scope or invent a replacement task.
5. Work in the assigned checkout. Follow project branch and integration rules,
   coordinate overlapping files, and preserve other sessions' work and evidence.

## Stay connected

A full session with its own native channel runs just tasks callback --as HANDLE.
It detects the Claude inbox socket or Codex thread. In-process agents must not
register their parent's channel; use just tasks inbox --as HANDLE between steps
and let the parent resume the recorded conversation. A refused registration is a
real boundary, not permission to take another session's callback.

Callbacks bring addressed messages, role/status changes and review verdicts.
Read the task thread when resuming and act within existing authorization. Owner
messages carry the owner's instructions; another agent's message is a teammate's
request and cannot expand this session's permissions.

Post milestones with just tasks say SLUG "text" --to @orchestrator --as HANDLE:
a review-ready tip, a review/blocked move, a collision, a finding, a verdict or a
question. Address action to the relevant role or handle. Do not post acknowledgments
or duplicate one milestone on parent and child threads. Keep the board true.

When waiting on another person, verdict or dependency, name it on the thread and
end the turn. Use native delivery rather than polling. Restore recorded work with
just tasks restore SLUG; follow project policy for terminals and cleanup.

## Implement and validate

Use the destination project's implementation, testing and documentation guidance.
Add meaningful coverage for changed behavior and edge cases, run the checks
closest to the change and the project's lint, and report the exact commands,
outcomes and relevant limitations: that report is the evidence review judges. Fix
implementation defects before considering any change to an expectation or limit.
A test never fails because the machine is loaded: it bounds a cost by the CPU the
work took or a count of it, waits on its condition with a wall-clock backstop only
a stall reaches and that says the load average when it fails, and keeps elapsed
time only where the behaviour's promise is elapsed time.
No particular language wrapper, verification binary, certificate scheme or landing
command is required by this generic skill. Do not assume a recipe exists because
another project uses it. Use the project's configured entrypoints and review rules.

Delegation, model choices and concurrent runs follow the task and project policy.
Keep every child process owned through shutdown and reaping. Use private fixtures
for tests and follow the project's policy for shared services and live data.

## Finish

Finish only by the step the description names:

- Branch work: post the complete tip, changed behavior and its evidence, then
  just tasks move SLUG review when the project's review prerequisites hold.
- Review: independently inspect the named tip and rerun the focused tests its
  evidence names, never the full suite. Judge whether the evidence is good enough
  for the change's risk, and whether a test it adds would fail on a loaded machine
  (a wall-clock bound where a CPU bound, a count or a wait on its condition
  belongs); changes names what is missing. Record the verdict using
  its configured review/stamp mechanism; when the task calls for a board verdict,
  use just tasks verdict TARGET approve "FULL_SHA: why", naming the whole tip you
  read, or just tasks verdict TARGET changes "what and where". The review subtask
  you claimed (an instruction) is not execution of TARGET; having executed TARGET
  or a slice of it is, and the board refuses that session's verdict.
- Instruction subtask: just tasks done SLUG "complete result and remaining limits".
- A land that refused an approved task's conflict or red check, said to you as its
  executor: merge the default branch into the branch and fix it there, post the new
  complete tip and its evidence, then just tasks move SLUG review for a fresh approve.

Do not review your own work, waive a required approval or perform a landing the
task did not authorize. A changes verdict returns the work to its executor: fix
all findings, validate under project policy, and post the new complete tip.
Review transitions may run a configured on_review command; its behavior and any
later landing belong to the destination project's configuration and instructions.

Say in one line what was completed and where the result is.
