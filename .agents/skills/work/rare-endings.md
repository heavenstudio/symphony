# Rare endings

## Covered by another task

Work that landed with another task's branch, leaving none of its own, closes with
`just tasks done SLUG "what landed, where" --covered-by OTHER --as HANDLE`, OTHER the task whose
landing holds it. It is no approve and no review. The board refuses it while the task's branch,
or a branch or worktree it records for an executor (an in-process one's too, as
`just tasks agents` shows it), holds commits the default branch lacks or changes no commit has:
those go to review. It reads nothing else (the main checkout, earlier checkouts, stashes, other
branches): check those first.

## A refused land

A land that refused your approved task over a conflict or a red check, said to you as its
executor: merge the default branch into the branch and fix it there, post the new complete tip
and its evidence, then `just tasks move SLUG review --as HANDLE` for a fresh approve.
