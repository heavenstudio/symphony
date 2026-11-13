---
name: review-task
description: Review one task in the board's review column, independently of its author: rerun its focused tests, judge its evidence and record a verdict naming the tip read. A reviewer's whole prompt is `/review-task SLUG HANDLE` (Codex `$review-task SLUG HANDLE`), never /work. Trigger on /review-task SLUG or $review-task SLUG.
---
<!-- symphony-stamp: ad7fae9d93e86b7a files=SKILL.md -->

# /review-task SLUG HANDLE

Judge one task in review, independently of whoever wrote, executed or requested it. The
destination project's review rules come first: its reviewer families and models (`just tasks
limits` names the reviewer's), its stamps, any checklist it provides for the changed languages or
domains, and any further verification it requires. This skill supplies the board's mechanics; it
assumes no certificate or landing system.

1. Reuse HANDLE from the prompt or the task's reservation; with none, register with
   `just tasks register --kind reviewer`. Pass it on every board command as
   `just tasks … --as HANDLE`, the handle last.
2. SLUG is the task to judge, TARGET, or a review instruction (an `--instruction` subtask) whose
   text names TARGET, its parent unless it says another. Claim an instruction with
   `just tasks claim SLUG --as HANDLE`: claiming it is not executing TARGET. Otherwise take the
   role, unless you hold it already: `just tasks role TARGET reviewer --as HANDLE`.
3. Read `just tasks show TARGET --as HANDLE`, its thread, and the diff of the tip it names.
4. Rerun the focused tests its evidence names yourself, never the full suite; never take the
   executor's word that they are green.
5. Judge, naming what is missing in a changes:
   - whether the evidence is good enough for the change's risk: higher risk asks for stronger
     evidence, never for the whole suite;
   - whether a test it adds could fail on a loaded machine (a wall-clock bound where a CPU bound,
     a count or a wait on its condition belongs): that is a changes;
   - whether the change is bigger than what it solves (a new mechanism where an existing one
     serves, more files, options or pages than the ask needs, a committed tool for a problem met
     once): name the smaller form, which may be "once and done", as a finding.
6. Record the verdict: `just tasks verdict TARGET approve "FULL_SHA: why" --as HANDLE`, naming the
   whole tip you read (the approve is the task's stamp: TARGET becomes `done`, and the default
   landing lands that commit and no other), or
   `just tasks verdict TARGET changes "what and where" --as HANDLE`. The board refuses the
   verdict of a session that ever executed TARGET or a slice of it (a subtask without
   `--instruction`). Never judge a task you executed.
7. Close an instruction with `just tasks done SLUG "the verdict and the tip" --as HANDLE`.

An in-process reviewer reads `just tasks inbox --as HANDLE` between steps; an owner's `MOVE`
ending `it moves no further: stop work on it` means stop. Post the verdict, never an
acknowledgement, and say in one line what you judged and the verdict.
