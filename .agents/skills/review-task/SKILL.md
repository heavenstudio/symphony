---
name: review-task
description: Review one task waiting on its review, independently of its author: read its diff against its recorded evidence, run only what that leaves open and record a verdict naming the tip read. A reviewer's whole prompt is `/review-task SLUG HANDLE` (Codex `$review-task SLUG HANDLE`), never /work. Trigger on /review-task SLUG or $review-task SLUG.
---
<!-- symphony-stamp: 1776a533e1cce91a files=SKILL.md -->

# /review-task SLUG HANDLE

Judge one task waiting on its review, independently of whoever wrote, executed or requested it.
The project's review rules come first: its guide (`AGENTS.md`), the rules it names, the guide of
each package the change touches, and `project.md` beside this file where the project has one, its
standing instructions for a reviewer. This skill supplies the board's mechanics; it assumes no
certificate or landing system.

1. Reuse HANDLE from the prompt; with none, register with `just tasks register --kind reviewer`.
   Pass it on every board command as `just tasks … --as HANDLE`, the handle last. A session with
   its own channel registers it: `just tasks callback --as HANDLE`.
2. SLUG is the task to judge, itself, never a subtask: take the reviewer role on it unless you
   hold it already, `just tasks role SLUG reviewer --as HANDLE`. Refused the role, stop and say
   why on the thread `--to @orchestrator`; never pass `--beside` yourself.
3. Read `just tasks show SLUG --as HANDLE`, its thread with every earlier review on it, and the
   whole diff of its tip (the `TIP` line of step 4) against the default branch.
4. Read its recorded builds and tests at that tip: `just tasks runs SLUG --as HANDLE` (`--tip
   SHA` for another commit). A run recorded green at the tip is run: never build or run it again,
   and never the full suite. Build or run only what the record does not cover and reading cannot
   settle: a doubt you name, with the smallest target that answers it; a mutation, to see that a
   test can fail; a run the executor left out. Green is the record's to say, never the
   executor's word: a run the evidence names and nothing recorded is asked for in a changes, or
   rerun yourself in a project that records none. A scratch file you write is named by your
   handle.
5. Judge, naming what is missing in a changes:
   - whether the change is correct and does what the brief asks, its documents included;
   - whether its tests cover the edge cases, and whether a test or a check was loosened to pass;
   - whether the evidence is good enough for the change's risk: higher risk asks for stronger
     evidence, never for the whole suite;
   - whether a test it adds could fail on a loaded machine (a wall-clock bound where a CPU bound,
     a count or a wait on its condition belongs): that is a changes;
   - whether the change is bigger than what it solves (a new mechanism where an existing one
     serves, more files, options or pages than the ask needs, a committed tool for a problem met
     once): name the smaller form as a finding;
   - each gate decision on it (`GATES` in `just tasks stamps SLUG --as HANDLE`, recorded by its
     orchestrator alone): for a raise, that the cut came first, that the commit raising the limit
     is its own with a true account (what grew, what was taken back, the number measured), and
     that the tip holds the number decided; for a waiver, that it names the tip you read. A limit
     raised in the diff that no raise names is a changes.

   That the default branch moved since the tip is no finding. Ask for a merge only by naming
   what the default branch gained that breaks the change or that the change breaks.
6. Record the verdict on SLUG: `just tasks verdict SLUG approve "FULL_SHA: why" --as HANDLE`,
   naming the whole tip you read (the default landing lands that commit and no other), or
   `just tasks verdict SLUG changes "what and where" --as HANDLE`. Its text lists what you
   checked, what you ran with its result, each gate decision at the tip, and each defect with its
   file and line and why it matters. Approve only what you would land; a nit is a follow-up named
   in the text, never a reason for a changes. Never judge a task you executed (the board refuses
   it), and never fix the branch yourself: a defect is a changes.
7. After a changes, or a land that asked for SLUG's review again, you stay its reviewer: resumed
   with a new tip, read the thread since your verdict and judge that tip from step 2.

Where a custom workflow holds SLUG, `just tasks role` and `verdict` refuse it as
`explicit_action_required`: `just tasks workflow actions SLUG --as HANDLE` names each edge that
leaves where it stands, and `just tasks workflow apply SLUG EDGE --revision N --as HANDLE` takes
one, N the revision those actions named: the edge that gives you the reviewer role, then its
`approve` or `changes` edge, the verdict's text as the input it names (`--input NAME=TEXT`).
Refused as `stale_task`, read its actions again.

An in-process reviewer reads `just tasks inbox --as HANDLE` between steps; an owner's `MOVE`
ending `it moves no further: stop work on it` means stop. Post the verdict, never an
acknowledgement, and say in one line what you judged and the verdict.
