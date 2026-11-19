# Execution attempts

[`symphony_board::execution`](../crates/tasks/src/execution.rs) is the board's record of each time
a task's role ran on a project's execution provider, and the coordinator that admits, recovers and
judges those attempts. It is stage 2 of the [execution design](analysis/remote-execution-attempts.md#delivery-after-approval)
under [its card](DECISIONS.md#execution-provider-attempts), built on the kit's
[execution adapter contract](EXECUTION_ADAPTER.md), whose types it reuses. A project supplies the
provider as a kit `Adapter`, its credentials and its storage; the host that runs Symphony builds
the grants and runs the coordinator. Nothing starts it: local terminals launch as before, and no
setting reaches it; a task's page reads its attempts and its owner's controls reach the
coordinator ([on a task's page](#on-a-tasks-page), [what is not delivered](#what-this-does-not-deliver)).

## The record

An attempt is a row of `execution_attempt` (schema 23): its identity, written once at admission
and never again, beside its state, which each write replaces under the row's revision. Its facts
are appended to `execution_fact` and never changed; `execution_fence` holds the fences admission
writes (a request key, a task's lane, a capacity scope) and `execution_artifact` its collected
artifacts. The four are schemaless, with no event: no reader caches them and no write of theirs
moves the board's marker. A whole-board copy and dump carry them (`rows::Dump`'s `execution_*`
rows, each holding its own record's id), so a restored board recovers its attempts, their
reservations and their request keys.

The identity pins the project, board, task, lane, role and handle, the task's input (its revision,
digest, repository revision and manifest), the provider configuration with the contract and
capabilities negotiated then, who asked and under which policy revision, the request key, the
runtime epoch and the capacity scope, and the predecessor a retry or a resume follows. Changing any
of it is another attempt. The state keeps each dimension apart: dispatch (pending, starting,
unknown, running, paused, held, terminal, with how it ended), cancellation, collection, cleanup, the
bound resource and its generation, the conversation, the operation owed, and the evidence
references the provider reported. `Store::live_attempts` names a task's attempts whose runtime may
still be alive, held ones included: what a workflow change waits on.

## Admission

`Coordinator::admit` writes the attempt, its first facts, its lane, its request key and its
reservation in one transaction. A request key admits the same attempt however often it is sent,
and refuses a different request under the same key; a lane holds one live attempt; a full
capacity scope refuses with the attempts that hold it. The start it owes is recorded, with its
key, before anything is sent. A task the board does not hold, or a closed one, takes no attempt.

## The worker

`Coordinator::step` acts for one attempt under a lease the database's clock times, each lease the
next epoch, and `run_due` steps the attempts due under its configuration: a pass the host calls,
starting no thread and no scheduler of its own. Before an operation is sent, the step records it
as sent; the answer is written only while that lease holds, else kept as a stale fact that
changes nothing. An operation's key is a function of its project, board, configuration, attempt,
kind and sequence alone, so a repeat sends the same key.

An answer that settles nothing (lost contact, a deadline, an invalid reply) never fails the
attempt and never starts another: the operation is reconciled by its key, after a backoff that
doubles to a cap. A start the provider says it never received, with an authoritative absence, is
sent again by the same key, unless a cancel was asked, when it ends with nothing provisioned. Two
resources for one key are quarantined, not promoted. After a bound of unsettled answers in a row,
an expired key or a provider that cannot reconcile, the attempt is held for a person, with no I/O.
A worker that dies leaves its lease to run out, and the attempt is due again then.

## Controls

A cancel bars every result of the attempt from the moment it is written. One asked before
anything was sent ends the attempt at once; one asked while the start is in flight is sent once
the start's resource is known; one the provider refuses leaves the cancellation unconfirmed. A
stop asks the bound resource to pause or terminate, in a mode its provider offers; a cancel asked
while a stop is in flight is sent once the stop is answered, whether it was taken or refused,
and only a cancel, or the terminate that stands in for one on a provider that cannot cancel,
settles it. A resume admits another attempt continuing a paused or ended one, from a checkpoint
or a conversation, at the next runtime epoch, so nothing of the predecessor's runtime writes for
it; a retry admits a new execution after a held or ended one, and is a person's decision alone,
naming the evidence that the predecessor was found or isolated. Either is admitted in the same
transaction that links the two and ends a still-live predecessor superseded, its capacity place
handed to the new attempt, so following an attempt never needs a free place and is never half
done. `external_link` resolves a view for its caller, never kept.

## Evidence and what may count

Once an attempt has ended or paused, the host reports its collection and the artifacts the
project's storage collected. An artifact is kept only when the attempt's current resource
generation and runtime epoch made it; any other is a stale fact alone. Its name is a relative path
that leads nowhere above the manifest; the same artifact reported again changes nothing, and
another under the same name is refused. Cleanup begins once the attempt has ended and its
collection has, under a key a late report of an earlier cleanup cannot match.

`Coordinator::eligibility` says whether an attempt's result may be put forward as the task's
evidence, and every reason it may not: it holds its lane, it succeeded, no cancel was asked, its
collection is complete and verified, and it ran on the task revision named. Eligible is all it
says. A process exit or a released run is not task success, and no provider answer completes a
task, grants a stamp, approves a workflow step or lands anything.

## Authority

Every call takes a `Grant`, which the host builds with `Grant::from_host` after it authenticated
its caller: the principal, the board, the tasks and acts its policy allows and that policy's
revision. A grant is never deserialized, so no payload, query or claimed handle becomes one. A
`Principal` in a grant or a fact is who acted, never what they may do. A grant of another project
or board, an attempt of one, and a worker grant on some tasks alone are refused; a coordinator
steps only its own configuration's attempts and refuses another's before touching its lease.

## On a task's page

A task's attempts are read with no provider at hand: `execution::Reader` reads a task's history,
an attempt and its facts under a grant on that one task, in the board scope of the project the
caller serves, which `execution::board_scope` makes from the registered project's id and the
board's database, the one constructor every caller uses for a project's board. A history holds
every attempt the board kept for the task, after its handle left too, with the resource,
generation and conversation each was bound to, as references, never as addresses; an attempt of
another project or board refuses the whole read, however its id came.

- **The read.** The server answers `GET /api/task-attempts?slug=SLUG` with
  `symphony_api::TaskAttempts` (`apps/symphony-server/src/attempts.rs`): each attempt's states in
  the board's snake-case words, a word a newer server adds drawn as said, and what may be asked of
  it here (`controls`). On this machine's server the asker is the local owner
  (`workflow::local_owner`), as a workflow edge is taken; under hosted admission it is the
  principal and policy revision the host verified, which reads alone and is offered no control. A
  task's page counts its attempts (`TaskPage.attempts`, left out at none) and draws their history,
  newest first, from a chunk it imports for a task with some alone
  (`apps/symphony-web/src/attempts.tsx`), read again on each change of the task and on Refresh, as
  the feed carries no attempt's change.
- **The controls** are the local owner's posts: `POST /api/attempt-control` with the task, the
  attempt, the revision the page read, the act (`cancel`, `stop` or `resume`), its mode and a
  resume's request key, and `POST /api/attempt-view` with the task and the attempt. A request
  takes its own fields alone, so one that names an owner, a principal or a grant is refused; an
  attempt of another task or project is refused; a revision the attempt has moved past is refused
  as stale; and a hosted visitor's post is its host's dispatch, never these. Each goes through the
  coordinator, under a grant on that one task. A cancel and a stop are intent the provider's
  worker sends wherever it runs, so the server records them with no provider of its own; a resume
  admits under its provider's own profile and capacity, and a view is that provider's I/O now, so
  both go only through the provider the attempt's configuration names, and with none here are not
  offered (`controls.unserved` says why) and are refused.
- **A view's address** is answered to the one who asked, `no-store`, and kept nowhere: not on the
  board, on a thread, in a log, a cache or the page. The page opens a blank window within the
  click, cuts its opener and sends it the address alone, an http or https one only.
- **A session is as it was.** The live `session_url` and Go to session keep their behaviour. An
  attempt runs on its provider and has no pane, so a session's focus and screen stay local.
- **`tasks api`'s `attempts`** (2.5, `{"slug": S}`) reads a task's history as the board's own
  service, under the project the call runs in, with no control offered
  ([the tasks API](TASKS.md#the-tasks-api)).

### The provider seam

A host binary that links a provider installs it beside its plugins, by its configuration name:
`symphony_server::binary::run_with_providers(plugins, providers, args)`, each
`symphony_server::execution::Provider::new(configuration, capabilities, adapter)`, with
`.with_capacity(scope, limit)` naming the capacity its own worker admits under. Nothing is
discovered or loaded at run time, a configuration answers once and a second install is refused
(`apps/symphony-server/src/execution.rs`). The Symphony's own server installs none: a page reads,
cancels and stops as above and is offered no resume or view, so a host need do nothing to keep
working.

## What this does not deliver

This is the board's half alone; full provider integration is not complete.

- **Local terminals are unchanged.** hc's launch, sessions and links keep their own calls;
  running them as a provider of attempts is the board task `tools/local-provider-attempts`.
- **No provider runs here.** The Symphony's server installs none, so a resume or a view is offered
  only by a host that installs its own ([the seam](#the-provider-seam)), and nothing here steps an
  attempt: its provider's worker does, wherever the project runs it.
- **No provider, credentials or storage.** Each project supplies its own adapter and storage, in
  its own tasks; this crate links no provider SDK.

## Tests

`crates/tasks/tests/store/execution.rs` runs the coordinator on each database against a
deterministic provider (`tests/store/execution/fake.rs`) that scripts lost responses, outages,
crashes before and after the provider acted, refusals, conflicts and calls that race a second
worker or a cancel. `execution/rules_tests.rs` holds the transitions without a database, and
`tests/consumer_api.rs` the shapes a host calls. `tests/store/attempt_reads.rs` reads a history
with no provider; `tests/cli/api/attempts.rs` is `tasks api`'s read; the server's routes run on a
private board with that provider and a worker the test steps
(`apps/symphony-server/src/attempts_tests.rs`), each refusal among them; and the page is
`apps/symphony-web/src/attempts.test.tsx` and the browser's
`apps/symphony-web-tests/tests/browser/task_attempts.rs`.
