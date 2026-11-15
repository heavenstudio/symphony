# Finish

Finish only by the step the description names:

- Branch work: post the complete tip, the changed behaviour and its evidence
  ([build.md](build.md#evidence-is-the-gate)), with what the description asked for that was
  left out and why, then `just tasks move SLUG review --as HANDLE` once the project's review
  prerequisites hold.
- A slice of a feature: as [below](#parent-feature-delivery).
- An instruction subtask: `just tasks done SLUG "complete result and remaining limits" --as HANDLE`.
- Work that landed with another task's branch, or a land that refused your approved task:
  [rare-endings.md](rare-endings.md).

A changes verdict returns the work to you: fix every finding, validate under project policy,
post the new complete tip and move it to review again. Never review your own work, waive a
required approval or perform a landing the task did not authorize. A move to review may run the
project's configured `on_review` command; what it does, and any later landing, belong to the
project's configuration and instructions.

## The default branch, once

Merge the default branch into the branch once: when the work is complete and you are about to
ask for review, so the review is of the change on the default branch as it is. Run the focused
checks again at that tip and name it in the evidence. From then on never merge it because it
moved: it always moves, and merge, build, measure and merge again has no end. The task lands
once its approvals stand and it merges without conflict. Merge again only when a land reports a
conflict ([rare-endings.md](rare-endings.md#a-refused-land)) or a reviewer asks for it by naming
what the default branch gained. A figure a document quotes (a size, a count of bytes) is measured
once, at that tip. Where the project names another branch for a task to merge, that branch
stands in the default branch's place.

## Evidence at the tip, once

Run the focused builds and tests through the project's recorded entrypoints, at the tip you ask
review for: the runs recorded there are what a reviewer reads in place of running them again. A
run made before your last commit is of another commit, so run it again yourself before the move
to review; a reviewer builds or runs only what your record leaves open.

## Parent feature delivery

A top-level task owns one feature branch and one delivery cycle. Subtasks are small
implementation slices, not independent releases. One agent continues the feature by default;
another only for disjoint work that demonstrably shortens the critical path. Commit slices into
the feature branch, or merge their worktree commits into it with the project's
`just merge-feature FEATURE [SOURCE]` where it has one: that refuses the default branch and runs
no delivery hooks or stamps, and the project's landing (`land` where it has one) remains the
completed feature's delivery command. A slice runs only the focused checks it needs: no
slice-level full build, full suite, reviewer staffing, certificate, stamps or landing. Close the
slice with its integration commit and focused result using
`just tasks done SLUG "integration commit and focused result" --as HANDLE`. When the complete
feature is ready, the parent goes to review once with its evidence, then lands. Review fixes stay
with that parent and do not create independent delivery rounds. The board shows parents by
default, their subtasks nested under them. This rule supersedes conflicting per-subtask gate
instructions.

Say in one line what was completed and where the result is.
