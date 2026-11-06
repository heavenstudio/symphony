# Symphony server

`just symphony` runs this server. The native read models live in `crates/symphony`, and
their serde-only wire records in `crates/symphony-api`. All routes run natively:
the Dioxus SPA at `/`, reads, saves and media.

The shared server watches main's server sources and transitive local crate sources. It builds
through main's cargo wrapper under `sf-symphony/reload-build.lock`, while continuing to serve.
The replacement inherits every listening socket and the `server.lock` lease, warms its read
hub, then acknowledges readiness on a private pipe. Only then does the old generation stop
accepting, finish its feeds and drain accepted requests and save workers without a timeout.
Unadmitted sockets receive 503, then close their write side and drain input to EOF within the
existing five-second shutdown grace, so unread request bytes cannot reset away the response.
Clients that send nothing or leave a header incomplete cannot hold the old process indefinitely;
admitted request bodies drain normally.
The startup lock serializes publication with starters and stops. A build failure is logged once
per source fingerprint and sent to open clients as a named `server` SSE event containing a JSON
string; a fresh stream receives the current notice too. Editing the sources retries the build.
Only the main checkout on branch `main` is followed; foreground servers do not reload.
Shared installation and live rebuilds both use Cargo's release profile. The debug
server exceeded the idle CPU budget on the normal board; the release build keeps
serving requests throughout its longer rebuild.
Source hashing runs at startup, after filesystem events with a 100 ms debounce, and once every
10 s without one, since FSEvents drops events under load (see the read hub below);
the shutdown timeout performs no source reads or repository commands. Watches cover
workspace source groups so an incomplete dependency edit can recover on a later event.
A successful handover stamps the installed executable's content hash and source fingerprint.
Verdict workers in overlapping generations share a commit lock in the common directory. An edit
whose file includes a still-pending earlier verdict stays queued and retries against fresh main;
it is not discarded as a commit failure. The separate document-write lock keeps reads, edits and
generated-verdict publication from overwriting a save made by another generation.
Existing repositories keep their publication journals, backlog counters and shared locks
in the existing metadata home; fresh stores use `sf-symphony`. Server discovery prefers
the canonical record and reads a predecessor record once when the canonical record is absent.
Once earlier live backlogs drain, a repeated conflict on unchanged main still files the
existing failure task and stops retrying. Dead processes' unlocked markers do not extend retries.
Restarts reuse a matching stamp, so unchanged code needs no extra build or handover; an
unstamped or replaced binary is reconciled once. Ending a subscription also ends its response,
even while the shared server-notice channel remains open.
Shutdown cancels an active build as well as lock and replacement-readiness waits. Each build
owns a process group covering the launcher, cargo and compilers; shutdown sends TERM, allows
two seconds, then sends KILL and reaps the launcher before the server exits. A cancelled build
cannot keep installing after `--stop` returns.

| GET | Response |
| --- | --- |
| `/api/board` | Next five ranked cards, counts, the project's progress regions, build and feed version |
| `/api/queue` | All ranked cards, spot checks, agent work and branch cards |
| `/queue` with `Accept: application/json` | Same queue; ordinary navigation opens the client board |
| `/api/tasks` | Version, task records and sessions without process ids |
| `/api/task-board?q=…&status=…&cursor=…` | Thirty brief-free summaries in the requested column; `counts=1` returns only counts and sign-off slugs |
| `/api/task-page?slug=…` | Task, shared typed brief blocks and child count |
| `/api/task-offer?slug=…` | Resolved branch and the first offer of the project's board actions that has a payload, on work of any status (each action decides which it offers on); fetched near the controls |
| `/api/task-history?slug=…&before=…` | Thirty structured messages with timestamps and typed blocks; `after=…` catches up a loaded thread after a lost feed |
| `/api/agent-page?cursor=…` | Twenty connected agents and their current work and five latest finished tasks; `health=lost` pages lost agents |
| `/api/task-thread?slug=…` | Alias for the typed paged task history |
| `/api/agents` | The board's agents view, including probe availability |
| `/api/activity` | Latest forty document and owner-addressed task entries |
| `/api/pending` | Pending cards in document navigation order, then branch cards |
| `/api/feed?since=…` | Server-Sent Events after the snapshot's version |

The read routes also work under `/@<branch with / replaced by ~>/`. Done and
cancelled columns show work updated in the last day; `days=7` selects the last week,
`older=1` and search reach older work. `parent=…`, `roots=1`, and
`holder=…&role=…&open=1` page child tasks, hierarchy roots and followed work.
Search uses SurrealDB full-text indexes over titles, briefs, slugs and thread
messages, ordered by BM25 relevance with an exact slug first. Matching remains
in the database; a message-only match keeps its task in the returned results.
Canonical `/tasks`, `/task/…` and `/agents` routes serve the client shell.
The opaque `next` cursor follows a stable task key or agent handle, so live
insertions and removals do not shift an integer offset past an unread record.

Document and picture links in those snapshots retain the branch prefix. Removed worktrees
return 404. Empty or missing task slugs return 400 before reading the board.
A missing task returns 404; an unavailable board returns 503 while the document
board continues without task counts. A board that took a read and answered it
later than the 3 s wait, on its one resend too, is slow: 504, "the task board is
slow", never "cannot be reached". The wait bounds the board tick, which shares the
data worker's thread with document refreshes, and bounds each sending's whole
answer, so a read sent twice stays inside the client's 15 s request limit. Native routes apply both
the peer gate and the Host check; sandboxed frames cannot read the API.

`just symphony` installs the release client with the shared server, in
`sf-symphony/client/public` beside `sf-symphony/bin`, built from main. The detached
server uses its `--home` argument to find that bundle for every branch URL.
`just symphony --port N` builds and runs this checkout's client and server without
touching the shared instance; N is never the shared server's port, on any
address (outside Linux a bind of one address would sit beside its wildcard
bind). A foreground server's static directory is
`build/symphony-ui/public` under its root, or `SF_SYMPHONY_UI_DIST` when set.
Direct `symphony-server --port N` needs `just symphony-ui-dist` first. Its default
root is the checkout's top level even when the command starts in a subfolder; `--root`
selects another folder. A private server on the main checkout leaves the
shared app's `artifacts/docs-app.port` record alone. `/` serves the
shell with revalidation; content-hashed assets have immutable caching. Gzip
compression covers snapshots and client files. SSE is never compressed or
buffered. A changed shell script path emits a `client` region, which the
existing client uses to reload once per build.

Only the shared server started with `--home`, holding the server lease, drains
the chat journal. Private servers leave it untouched, even with `ASKS_DB` set,
and log that choice when their read hub starts. This keeps scratch-board previews
from forwarding pending owner chat to their board. Set `TASKS_DATABASE` to a
scratch database for a preview, and set `ASKS_DB` to a private path when exercising
ask or chat writes, which still use that store. Leaving the journal alone is
simpler than creating a second preview journal with its own delivery lifecycle.

Every SSE delta has an increasing `id` and a single JSON line containing
`version` and `changes`. The existing board regions are `card`, `queue` (only
the five visible cards), `counts`, `progress` and `client`. Additional list
regions use keyed upserts/removals (`ItemChange<T>`); their `-order` events
move one key after another (`OrderChange`). They are `queue-item`,
`spot-check`, `agent-queue`, `pending`, `activity`, `task`,
`session` and `agent`. `task-fields` merges changed mutable fields into an
existing task, avoiding its unchanged brief. `message` appends one structured
thread message by id, with its typed `blocks` and display `stamp`. `task-summary` upserts
one brief-free record so a task entering a paged view appears without a board
refetch; `task-counts` and `task-signoff` update the column totals and owner queue.
`docs` replaces navigation order; `agent-probes` reports
probe availability; `tasks-status` reports board availability. Unknown regions
are ignored by the current board client.

One filesystem watcher handles registers, branch discovery and the client
build for every connection. One shared board reader checks database changes
each second; clients do no version polling. Recent deltas are retained for the
snapshot/subscribe gap. An expired cursor returns 409, or closes an already
open stream, so the client fetches a fresh snapshot. `Last-Event-ID` is not a
resume mechanism across server starts. Named `ping` events follow the client
contract: at most five seconds of silence.

Worktree index locks and reflogs do not invalidate document snapshots. Text and
parsed registers reuse unchanged files, checking inode, size, modification and
change times so atomic replacement and a restored mtime still invalidate them.
Schema validators are reused by schema contents. Unchanged feed lists skip diff
construction, and branch discovery reuses parsed immutable register contents.
Task activity and sign-off reads follow their message/task/document
inputs. The agents view indexes task roles once per snapshot instead of scanning
all tasks for each historical session, and the live reader reuses its repository
location. `SF_SYMPHONY_PROFILE=1` logs wall time for each reader stage and maintenance
pass, along with process CPU time during that stage; ordinary serving leaves those
diagnostics off. It reaches a foreground server only: the shared one keeps none of its
starter's environment but `HOME`, `PATH`, `USER`, `LANG` and `SF_SYMPHONY_TEST_PID`. The orphan
census logs one line per round, as its scan answers or as the server's stop ends it, so a
`ps` that never answers still shows how long it was pending. The callback clock's
sequenced board snapshot also supplies the board and agents views. Unchanged raw records reuse their wire snapshot and version hash.

A cold server publishes its document hub and URL, then activates the first board
read in the hub's owned worker. Data reads activate and await that first result; an unavailable board
keeps its existing error response, a panic in that read answers as an unavailable board, and
shutdown releases waiting reads before joining the worker. A reload replacement awaits the first board result before
acknowledging handover. The server publishes its bound-port hint before warmup.
Auto-port discovery also reads bounded owned announcements and probes the known
ports before scanning processes. Cold homes and known free ports need no census;
unresolved startup history retains the recovery scan and its process identity checks.

On macOS, notify filters nonrecursive watches from a recursive FSEvents stream,
so writes below a watched root still cost event filtering. Foreground preview
measurements exclude the shared server's source watcher.
Under load FSEvents can drop an event with no rescan flag, so both watching loops also check on
their own every 10 s (`src/backstop.rs`). The reload worker rehashes its sources; the read hub
compares the stamps of main's watched files and of each served tree's documents, models and
concepts with the last sweep, and feeds what changed through the watcher's own path, so a missed
event costs at most one period. An unserved tree is followed by its events alone. A folder or
file that cannot be read keeps its last stamps rather than reading as removed.
Creating a referenced crate's missing manifest also wakes source discovery, so a
failed reload retries once that dependency exists. An unrelated manifest leaves
the source fingerprint unchanged and does not start a build.
Worker callback state omits the global task-status map used only for orchestrator
PULL digests. Unchanged observations advance their sequence in the durable outbox read
without rewriting seen/statuses, so an older concurrent dispatcher still refreshes its
snapshot; changed observations commit their sequence with their queued events.

Owner POSTs to `/api/decision`, `/api/comment`, `/api/message`,
`/api/task-message`, `/api/quick-chat`, `/api/quick-task`, `/api/ask`,
`/api/ask-reply` and `/api/task-action` run natively through `symphony::save`. A task
action is a board action of the project's plugins, named by the offer it answers
(`TaskOffer`: the action, its label, the branch and the payload it hands back);
an action no plugin of the project offers is a 404, and one the action refuses
says and moves nothing on the board.
Writes require an allowed Host and, when supplied, the app's exact Origin.
One register lock covers read, edit, no-loss validation, write and tap
deduplication across all worktrees. Asks share the existing `ASKS_DB` store.

`POST /api/upload?name=FILE` takes an owner's attachment as the raw body, behind
the same Host and Origin check, refusing a declared or streamed body past 10 MB
before reading the rest (`write.rs`). `symphony::uploads` reads its kind from its
bytes and stores it by SHA-256 in `uploads/` of this server's project artifacts
folder (`artifacts/uploads/` by default), whatever branch the page is on. `GET /artifacts/uploads/<sha256>.<ext>` serves a
stored name only, from every scope, with its fixed type, `nosniff`, immutable
caching and a script-free sandboxing CSP, PDF and Office files as downloads
(`media_files.rs`); docs/workflow/REVIEW.md,
"Attaching files", has the kinds and limits. An owner line that is only such a
reference is drawn as the picture or link in asks, card threads and task
messages; the rest of the owner's text stays literal.

Each document save checks whether its checkout is currently on main, so
switching back to main enables verdict commits without restarting the server.
`symphony::commit` commits each main-checkout save through a private index and
three-way merge, recording the unchanged `sf-land/verdicts/<commit>` record
before updating main. Verdicts that change generated content take a private
branch with their outputs through landing. A separate worker handles these
branches so ordinary verdicts can proceed. Each generated branch runs `just verify`
then `just land BRANCH` in its private checkout as the same app actor, regardless
of the legacy migration key. Land handles a moving main through its ordinary
candidate verification and reuse. A bounded land retry keeps the immutable edit,
branch, checkout and verifier records in a locked publication journal. The next
save supplies another publication pass that calls land again without regenerating
or running another whole verify; a restarted worker resumes the same journal.
Graceful shutdown drains both workers and preserves retryable publications.
Every save has an atomic draft journal before publication can temporarily
release its page. An unverified terminal failure retains that immutable draft
in a `.failed` file named in the filed task.
Tracked-dirty main refusals also retain the verified publication for a later
pass. Repeated refusals report once, persisted across restart; unrelated tracked
dirty paths report on the first refusal. Only output-worker pages are temporarily
released, so a plain save still being prepared keeps its source page in place.
The final checkout swap shares the app's write lock, so saves cannot race
that swap. Invalid journals are reported once and left in place while recovery
continues with the other publications. Terminal failures are logged and filed on
the board; generated branches that fail to land remain available.
Branch-checkout saves remain for that
branch's executor to commit. Verdict filing runs off the save path, and the
shared server calls the board's locked `reap` every five minutes. Private foreground
servers do no session maintenance, including when they use a copied scratch board.

Branch questions retain a docs-relative storage path and put the full branch
page URL in the question. Quick chat from a branch uses the chat home and the
same sent-from URL, so an agent can identify the proposal being discussed.

`just symphony` opens the browser only for an interactive human terminal. It prints
the URL without opening a browser when `TASKS_SESSION`, `CODEX_THREAD_ID` or
`CLAUDECODE` is set, or stdout is not a terminal. The native `--open` handler
enforces the same boundary even when called directly.

Old `/app/...` and `/doc/...` links redirect to root routes with their query
and branch scope intact. `/docs/...`, `/tasks`, `/task/...`, `/agents`, a plugin's pages
and scoped `/@<tree>/...` pages use the same shell. `/queue` shows the complete
typed queue: owner decisions, spot checks, agent work and pending branch cards;
`#you`, `#spot` and `#agent` are section anchors. The feed refreshes
the queue when those records change, and row contents mount near the viewport.
Old standalone `/history/...` and `/diff/...` bookmarks open their document,
whose Versions and diffs control reads `/api/versions` and `/api/diff`.

A plugin's gallery snapshots warm once per selected tree and again only when a
folder its watch names changes in that tree. Document-only refreshes leave the gallery cache alone.
The backstop sweep above covers model files too, so a model added or removed without a
filesystem notification still shows within a period. A page that goes, or follows its tree to
another folder, leaves that folder's gallery cold, so serving it again reads its models afresh.
An installed server missing its client instructs the user to run
`just symphony --stop`, then `just symphony` from the main checkout.
