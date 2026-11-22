# Rare endings

## Covered by another task

Work that landed with another task's branch, leaving none of its own, closes with
`just tasks done SLUG "what landed, where" --covered-by OTHER --as HANDLE`, OTHER the task whose
landing holds it. It is no approve and no review. The board refuses it while the task's branch,
or a branch or worktree it records for an executor, holds commits the default branch lacks or
changes no commit has: those request review. It reads nothing else (the main checkout, earlier
checkouts, stashes, other branches): check those first.

## A refused land

A land that refused your approved task over a conflict or a red check says what to do next; do
that and no more.

- **A conflict in documents alone**, where the land says the approve stands over one merge:
  merge the default branch into the branch once, settle the conflict in those documents and
  change nothing else, run the focused checks at the new tip through the project's recorded
  entrypoints, post the new complete tip, and ask for the land again. Leave the task as it is: a
  request for review again, or any change beyond those conflicts, asks for a fresh approve.
- **Any other conflict, or a red check**: merge the default branch into the branch and fix it
  there, post the new complete tip and its evidence, then request review again
  (`just tasks move SLUG review --as HANDLE`) for a fresh approve.
