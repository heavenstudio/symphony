---
name: review-task
description: Review one task in the board's review column, independently of its author: rerun its focused tests, judge its evidence and record a verdict naming the tip read. A reviewer's whole prompt is `/review-task SLUG HANDLE` (Codex `$review-task SLUG HANDLE`), never /work. Trigger on /review-task SLUG or $review-task SLUG.
---
<!-- symphony-stamp: 0e12501c6db2cefa files=SKILL.md -->

# /review-task SLUG HANDLE

Judge one task in review, independently of whoever wrote, executed or requested it. The
destination project's review rules come first: its reviewer families and models (`just tasks
limits` names the reviewer's), its stamps, any checklist it provides for the changed languages or
domains, and any further verification it requires. This skill supplies the board's mechanics; it
assumes no certificate or landing system.

1. Reuse HANDLE from the prompt; with none, register with `just tasks register --kind reviewer`.
   Pass it on every board command as `just tasks … --as HANDLE`, the handle last. A session with
   its own channel (a terminal of its own) registers it: `just tasks callback --as HANDLE`.
2. SLUG is the task to judge, itself, never a subtask: take the reviewer role on it, unless you
   hold it already: `just tasks role SLUG reviewer --as HANDLE`.
3. Read `just tasks show SLUG --as HANDLE`, its thread, and the diff of the tip it names.
4. Rerun the focused tests its evidence names yourself, never the full suite; never take the
   executor's word that they are green. A log or scratch file you write in your scratchpad is
   named by your handle: every in-process agent of one orchestrator shares that folder, and two
   writing one name read each other's output.
5. Judge, naming what is missing in a changes:
   - whether the evidence is good enough for the change's risk: higher risk asks for stronger
     evidence, never for the whole suite;
   - whether a test it adds could fail on a loaded machine (a wall-clock bound where a CPU bound,
     a count or a wait on its condition belongs): that is a changes;
   - whether the change is bigger than what it solves (a new mechanism where an existing one
     serves, more files, options or pages than the ask needs, a committed tool for a problem met
     once): name the smaller form, which may be "once and done", as a finding.
6. Record the verdict on SLUG, a round of its review: `just tasks verdict SLUG approve
   "FULL_SHA: why" --as HANDLE`, naming the whole tip you read (the approve is the task's stamp:
   SLUG becomes `done`, and the default landing lands that commit and no other), or
   `just tasks verdict SLUG changes "what and where" --as HANDLE`. The board refuses the verdict
   of a session that ever executed SLUG or a slice of it (a subtask without `--instruction`).
   Never judge a task you executed.
7. After a changes, or a land that sent SLUG back to review because your approve does not name
   its tip, you stay its reviewer: the next round is yours. Resumed with a new tip, read the
   thread since your verdict and judge that tip from step 2.

An in-process reviewer reads `just tasks inbox --as HANDLE` between steps; an owner's `MOVE`
ending `it moves no further: stop work on it` means stop. Post the verdict, never an
acknowledgement, and say in one line what you judged and the verdict.
