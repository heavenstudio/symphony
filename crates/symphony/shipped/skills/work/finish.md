# Finish

Finish only by the step the description names:

- Branch work: post the complete tip, the changed behaviour and its evidence
  ([build.md](build.md#evidence-is-the-gate)), with what the description asked for that was
  left out and why, then request review: `just tasks move SLUG review --as HANDLE`.
- A slice of a feature: as [below](#parent-feature-delivery).
- An instruction subtask:
  `just tasks done SLUG "complete result and remaining limits" --as HANDLE`.
- Work that landed with another task's branch, or a land that refused your approved task:
  [rare-endings.md](rare-endings.md).

A changes verdict returns the work to you: fix every finding, post the new complete tip and
request review again. Never review your own work, waive a required approval or perform a landing
the task did not authorize.

## The default branch, once

Merge the default branch into the branch once, when the work is complete and you are about to
ask for review, so the review is of the change on the default branch as it is. Run the focused
checks again at that tip and name it in the evidence. From then on never merge it because it
moved. Merge again only when a land reports a conflict
([rare-endings.md](rare-endings.md#a-refused-land)) or a reviewer asks for it by naming what the
default branch gained. A figure a document quotes (a size, a count of bytes) is measured once, at
that tip. Where the project names another branch for a task to merge, that branch stands in the
default branch's place.

## Evidence at the tip, once

Before review, read the owner's dated lines saved on your branch's cards and commit them on
their own cards in that branch. A land carries commits, so an answer left only in a worktree
cannot go with it. An answer in another worktree is committed on that worktree's own branch and
card; never move it onto a different card to let this branch land.

Run the focused builds and tests through the project's recorded entrypoints, at the tip you ask
review for: the runs recorded there are what a reviewer reads in place of running them again. A
run made before your last commit is of another commit, so run it again before you request
review.

## Parent feature delivery

A top-level task owns one feature branch and one delivery cycle; its subtasks are small slices of
it, not releases. One agent carries the feature unless another, on disjoint files, demonstrably
shortens it. A slice commits into the feature branch (or is merged into it with the project's
`just merge-feature FEATURE [SOURCE]` where it has one), runs only its focused checks and closes
with `just tasks done SLUG "integration commit and focused result" --as HANDLE`: no slice-level
full suite, reviewer, stamps or landing. Only the parent requests review, once, with its
evidence, and keeps its review fixes. This supersedes a per-subtask gate instruction.

Say in one line what was completed and where the result is.
