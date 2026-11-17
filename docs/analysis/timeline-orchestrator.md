# Orchestrators and messages on a task's timeline

A proposed addition to [the task timeline](../TASKS.md#the-timeline): show who orchestrated the
task and which way its messages were addressed, on the same clock as execution, review and
machine work. [Its card](../DECISIONS.md#timeline-orchestrator-messages) asks about the drawing
and selection behavior. The [interactive mockup](timeline-orchestrator/mockup.html) uses invented
handles and times; it is a design example, not a working task page.

## The drawing

On a wide screen an Orchestrator lane sits between Stage and Executor. A named span means the
session held that task's role, not that it worked continuously or was connected throughout.
An assignment, join, takeover or handover starts a span; release ends it. A handover changes the
name at its recorded time. Gaps stay empty. All lanes share the same horizontal time scale.

Messages are instants, drawn as small selectable marks at their send times. Selecting a mark
draws one arrow from its sender's lane to the addressed lane, at that same time, and opens its
details below the lanes. The arrowhead says the direction. It does not join a message to a later
reply or represent time spent waiting. Selecting a different message replaces that arrow.
The details show the message number, full sender, original recipient, exact time, full text and
a link to the original thread entry. Text identifies both endpoints without relying on color.

Every message stays reachable in an ordered message chooser beside the details, with Previous
and Next controls and keyboard access. Marks at the same position retain their own chooser
entries. Selecting one scrolls or zooms the common time window to include it. A minimum hit area
around a mark is separate from the instant it represents. No event's time or duration is stretched
to make room for its words.

The mockup's Whole task and Work period views demonstrate one shared scale. Sequential machine
runs reuse a horizontal row; only actual overlaps need another. Choosing a shorter period changes
every lane together. This proposal adds no burst folding and does not ask the run layout choice
again. Its production zoom and pan controls belong to the coordinated horizontal timeline work.

## On a phone

The chronological list stays in its existing 240 px step box until Show all. An orchestrator
assignment, release or handover is a step, and a message row names its sender and recipient with
a right arrow between the words. Selecting it opens the same details below the box. Show all
reveals the full list; it does not stretch a small message mark into a duration bar. The selected
message stays visible when the box is collapsed. The mockup shows the latest steps initially.

## What an arrow can say

| Recorded address or history | Drawing and details |
| --- | --- |
| A handle with one known role on this task at that time | Arrow to that role's span, naming the handle. |
| `@orchestrator`, `@executor` or `@reviewer` | Arrow to the addressed role. Name its holder only where the recorded history identifies one, labelled as the holder at send time. Keep the role address in the details. |
| No recipient | A message to Everyone on task, not a private arrow to the executor. |
| `owner`, `board`, an outside handle, or a sender with no known role | A mark on an Other / task messages row, with the exact endpoint named in details. Do not assign it a role from today's session list. |
| Missing history or several possible holders/roles | Keep the role address or handle as recorded and say the individual endpoint is unknown or ambiguous. Do not choose one. |
| Several messages at the same time | Separate chooser entries ordered by message id. Selection draws one directed message at a time. |
| A message after task closure | Extend the displayed time range to include it and leave the task's closed stage unchanged. Mark its time as after closure. |

An arrow means **addressed to**, not delivered, read, answered or caused. The board's message
record does not identify a reply parent or the historical set of actual recipients. A role's
holder inferred from its recorded span is context, never proof that a callback reached it.
Direct agent messages outside the task thread are outside this timeline's evidence.

For a subtask inheriting its orchestrator from a parent, the current parent holder cannot stand
in for history. Resolve the nearest recorded parent assignment at the message's time, with its
source task identified, or leave the individual unknown. Never backfill the task's whole life
with its current orchestrator. A closed task's role span ends at closure; a later message keeps
its sender's name without implying that the role remained active after closure. Reopening starts
another recorded stretch.

## Data and implementation boundary

The timeline wire type (`crates/symphony-api/src/timeline.rs`) currently carries stages,
executor claims, review rounds, runs and gates. Its builder (`crates/symphony/src/timeline.rs`)
does not include orchestrator holds or messages. The board's `stages::holds` replays only
executors and reviewers. The task thread already keeps each message's id, sender, recipient,
kind, text and timestamp, and `orchestration::moved` reads recorded handovers, takeovers and gives.

Implementation needs additive defaulted wire fields for orchestrator spans and message metadata,
replayed from recorded role events in message-id order. Joins and releases need handling beside
the handover reader. Role history must remain distinct from execution-stage claims. Messages
should reuse thread rendering and fetching for their full text rather than create a second
copy of the thread in the timeline response. A metadata range and explicit loading/failure state
must make any pagination visible, so an incomplete history never reads as no messages.

This design changes neither board routing nor its storage schema. Its implementation should
prove assignment, release, handover, duplicate and out-of-order lines, gaps, multiple roles,
role-addressed and direct messages, broadcasts, outside actors, inherited and missing history,
equal-time events, closure and reopening. Client checks should cover both directions, selected
details, colliding marks, keyboard selection, phone Show all, and one time scale across lanes.
Desktop and phone browser evidence should use a scratch board with a handover and both message
directions. Test coverage belongs to that implementation; no production behavior is changed by
these mockups.
