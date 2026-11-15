# Symphony server

`just symphony` runs this server. The native read models live in `crates/symphony`, and
their serde-only wire records in `crates/symphony-api`. All routes run natively:
the Dioxus SPA, reads, saves and media, under each project's `/<id>/` on the one server and
at `/` on a server of its own.

## Projects

The one server (`serve --home`) keeps its record (`server.json`), log, lease and lock in the
machine folder: `~/Library/Application Support/Symphony/` on macOS, `$XDG_DATA_HOME/symphony`
elsewhere, `SYMPHONY_MACHINE_DIR` when set (every test sets a temporary one). The registry
beside them (`projects.json`) holds each project's root, id and `symphony.toml`; `symphony
project add [DIR]` writes it, and every start runs it. The build and client stay in the
starter's install folder (`--install`, `symphony/server/` in its git common dir), where a record
an older server left is read once; a project's own state (asks, thumbnails,
the queue order, the port record) stays in its artifacts folder.

Each registered project is served at `/<id>/` by its own read hub: its watcher, feed cursor,
board snapshot and branch scan (`src/projects.rs`). `/<id>/theme.css` serves its theme
package's tokens with an ETag; its pages inline the same tokens in the head, then its
stylesheets, its name and icon, and each layout region's variant as a `data-layout-*`
attribute. `/` always lists the projects with their counts, including when there is only
one project or the browser carries an old project-selection cookie. The header's Projects link,
top left, opens this picker; its Home link opens the current project's home.
A navigation to a path of today
(`/docs/…`, `/tasks`, a bookmark) redirects to the project a same-origin referrer names, else
the starter's; other requests on such a path are answered by that project in place. A
project whose root is gone, or whose plugins the running build lacks, answers 503 saying why.

The running server compiles every registered project's plugins. A start judges it
(`symphony::machine::host`): served, when it holds the project's plugins at the project's pin
and schema or newer; waiting, when it lacks a plugin, naming the project whose build holds them;
refused, registering nothing, when the project's pin or schema is newer than the running
build's and the start's build cannot replace it. A project with no plugins never replaces it. A start whose build holds every registered plugin replaces it by
hand-off: it writes `handover.json` (its executable, root and install folder) in the machine
folder and sends `SIGUSR1`; the server hands its sockets to that build as a reload does.

The shared server watches main's server sources and transitive local crate sources. It builds
through the command main's `[project] cargo` names (when unset, `tools/cargo.sh` in Symphony's
own workspace and plain `cargo` elsewhere) under `symphony/server/reload-build.lock`, while
continuing to serve.
The replacement inherits every listening socket and the `server.lock` lease, warms its read
hub, then acknowledges readiness on a private pipe. Only then does the old generation stop
accepting, finish its feeds, and drain its save workers without a timeout and its accepted
requests for two minutes at most (`REPLACED_DRAIN`): it then drops what is still open, so a
peer that stopped reading a response (a suspended page, a sleeping network) cannot keep the
replacement's parent running.
Unadmitted sockets receive 503, then close their write side and drain input to EOF within the
existing five-second shutdown grace, so unread request bytes cannot reset away the response.
Clients that send nothing or leave a header incomplete cannot hold the old process indefinitely;
admitted request bodies drain normally.
The startup lock serializes publication with starters and stops. A failed update, its build or
its hand-over, is logged and sent to open clients as the feed's named `server` event containing a
JSON string; a fresh stream receives the current notice too. The notice says when that source
fingerprint is built and handed over again: 5 min after its first failure, the wait doubling with
each failure of it in a row up to 1 h (`src/reload/retry.rs`), on either clock, so the machine
never stays on the old build for want of a retry. Editing the sources builds the new fingerprint
at its first check, whatever that wait.
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
Publication journals, backlog counters and their shared locks live in `symphony/server/` in
the common directory. Server discovery prefers
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
| `/api/task-page?slug=…` | Task, shared typed brief blocks and child count; `gates`, once the server has answered the task's gates since it started, says how many there were, how many the task waited on and how many had no who and when (`GatesSeen`), the room the page holds while it reads them again |
| `/api/task-gates?slug=…` | The task's gates at its recorded branch's tip (`TaskGates`): the Symphony's review, check and owner gates and the project's plugins' own, each met, missing, waived, held or unknown, what the task waits on first; a plugin that errs or is slow is an unknown gate, a task not on the board 404 ([TASKS](../../docs/TASKS.md#gates)) |
| `/api/task-offer?slug=…` | Resolved branch and the first offer of the project's board actions that has a payload, on work of any status (each action decides which it offers on); fetched near the controls |
| `/api/task-history?slug=…&before=…` | Thirty structured messages with timestamps and typed blocks; `after=…` catches up a loaded thread after a lost feed |
| `/api/agent-page?cursor=…` | Twenty connected agents and their current work and five latest finished tasks; `health=lost` pages lost agents |
| `/api/task-thread?slug=…` | Alias for the typed paged task history |
| `/api/agents` | The board's agents view, including probe availability |
| `/api/reports?grain=…&days=…` | The Reports page's window (`ReportsPage`): its periods by the hour, day or week, each summed from the project's roll-up, agents and board-health day files, a period no file recorded none of; the settings file's commits in the window; what was not read. It reads files only, never the board, and at most 64 MiB of them (`src/api/reports.rs`). The answer is kept between reads with what it was read from (the hour, each day file by its stat, the default branch's tip), so a read that finds none of it moved opens no file and starts no git (`src/api/reports/kept.rs`) |
| `/api/run-log?run=…&tail=…` | Part of a run's log (`RunLog`): its last 64 KiB by default, its last `tail` bytes, or `from=…&to=…` a range, at most 1 MiB; only a regular file in a day folder of the project's `logs/`, never the board (`src/api/run_log.rs`, [RUNS](../../docs/RUNS.md#logs)) |
| `/api/activity` | Latest forty document and owner-addressed task entries |
| `/api/pending` | Pending cards in document navigation order, then branch cards |
| `/api/feed?since=…` | The feed after the snapshot's version: a WebSocket to a request that asks to upgrade, Server-Sent Events to any other (`src/api/feed_socket.rs`; both forms are `symphony_api`'s feed contract) |

Before any project the one server answers `/api/projects`, to loopback and this machine's tailnet
names like every route of a project's: each project it serves, with its feed's version, the cards
waiting on the owner and the first of them, why it is refused or its board unwell, and its task
asks, beside the server's installation id and pin (`symphony_api::ProjectList`, what the desktop
app reads). The task asks are the questions agents asked each person on open tasks that still wait
on them, and the board's newest reviewer verdict; a project's feed sends them again each time they
change, as the region `task-asks` (`src/live/task_asks.rs`).

Every JSON read, these and the document reads and a plugin's JSON alike, is answered
`no-store` with a weak `ETag`: the first 16 hex digits of the SHA-256 of the JSON it sends,
before compression. A request whose `If-None-Match` names it is answered `304` with no body,
keeping the tag and `no-store` (`src/api/validated.rs`, applied where every read leaves
`api::answer`). A document, its outline and its sections' bodies carry the feed's cursor
(`version`), which moves with every change on the board, so their tag is of the JSON with the
cursor left out: it moves only with the document. A page whose kept copy is still current takes
the cursor from `/api/board`. The browser keeps no copy of its own: the client's cache keeps
the body and sends the tag back (docs/analysis/client-cache.md). An error carries no tag, and the
feed's stream is never read whole.

The read routes also work under `/@<branch with / replaced by ~>/`. Done and
cancelled columns show work updated in the last day; `days=7` selects the last week,
`older=1` and search reach older work. `parent=…`, `roots=1`, and
`holder=…&role=…&open=1` page child tasks, hierarchy roots and followed work.
The task board opts into read-only Build/Test placement with `activity=1`; its
status filter, counts and cursor use one filtered machine snapshot. The returned
`lifecycle_counts` retain the underlying task totals. Without that opt-in,
queries keep lifecycle status and paging semantics.
Search uses SurrealDB full-text indexes over titles, briefs, slugs and thread
messages, ordered by BM25 relevance with an exact slug first. Matching remains
in the database; a message-only match keeps its task in the returned results.
Canonical `/tasks`, `/task/…`, `/reports` and `/agents` routes serve the client shell.
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
`symphony/server/client/public` beside `symphony/server/bin`, built from main. The detached
server uses its `--install` argument to find that bundle for every project and branch URL.
`just symphony --port N` builds and runs this checkout's client and server without
touching the shared instance; N is never the shared server's port, on any
address (outside Linux a bind of one address would sit beside its wildcard
bind). A foreground server's static directory is
`build/symphony-ui/public` under its root, or `SYMPHONY_UI_DIST` when set.
The server package embeds its committed `client/public` distribution: the current build and
the two published before it, as its `builds.json` names them. A direct
`symphony-server --port N` uses it when the default local build directory is absent.
An explicit install or `SYMPHONY_UI_DIST` never falls back, and a present local
bundle keeps its strict errors. `just client-update` in the Symphony workspace
refreshes the real distribution before committing client changes. Its default
root is the checkout's top level even when the command starts in a subfolder; `--root`
selects another folder. A private server on the main checkout leaves the
shared app's `artifacts/docs-app.port` record alone. A project's page serves the
shell with revalidation by its ETag, as is any client file not named by its
content. A file named by its content, an `assets/` name whose last `-` part is
dx's `dxh…` hash or the packager's sixteen hex digits (the shell script, every
page module, wasm and stylesheet), is cached for a year as immutable, so a
return visit revalidates the page alone (`src/api/client.rs`). Snapshots and the
page are gzipped per request. A client file is gzipped once per content, at the
best level, by the first request that takes gzip; later requests, from any
project, send those stored bytes. The server keeps the most recently asked-for
gzips, up to 16 MiB (about eight builds of every page's files), and drops the
least recently asked-for past that, gzipping it again should it be asked for
again (`src/api/client/gzipped.rs`). A file gzip cannot shrink goes as it is, as
do PNG and WOFF2 files. Gzip is the one encoding offered: `br` and `zstd` get
the file as it is. The feed is never compressed or buffered. A changed shell script path
emits a `client` region, which the existing client uses to reload once per build.

Only the shared server started with `--home`, holding the server lease, drains
the chat journal. Private servers leave it untouched, even with `ASKS_DB` set,
and log that choice when their read hub starts. This keeps scratch-board previews
from forwarding pending owner chat to their board. Set `TASKS_DATABASE` to a
scratch database for a preview, and set `ASKS_DB` to a private path when exercising
ask or chat writes, which still use that store. Leaving the journal alone is
simpler than creating a second preview journal with its own delivery lifecycle.

Every delta's data is a single JSON line containing `version` and `changes`; as a
Server-Sent Event it has an increasing `id` too, and on the WebSocket none (how each
form says an event: `symphony_api`'s feed contract). The existing board regions are
`card`, `queue` (only the five visible cards), `counts`, `progress` and `client`. Additional list
regions use keyed upserts/removals (`ItemChange<T>`); their `-order` events
move one key after another (`OrderChange`). They are `queue-item`,
`spot-check`, `agent-queue`, `pending`, `activity`, `task`,
`session` and `agent`. A `task` upsert is a new task; an existing task that
changed is sent once, as `task-fields`, its changed mutable fields alone,
never its unchanged brief or record again: a view that does not hold the task
(a column page, a task page's subtasks, `/agents`) reads it again when the
change brings it in, `/agents` merging that read into the agents it drew. A role's holder's health is no task's change: it is sent
once, as that holder's `session` change, and the client sets each role of that
handle from it (`lost` when the session is gone, as the board reads a role of
no session). A `session` or `agent` change is sent only when something a page
shows changed, never for a heartbeat or a tool call alone (`last_seen`,
`last_tool`, `last_tool_call`), and carries the record as it is now. `message`
appends one structured thread message by id, with its typed `blocks` and display
`stamp`; `task-counts` and `task-signoff` update the column totals and owner queue.
`docs` replaces navigation order; `agent-probes` reports
probe availability; `tasks-status` reports board availability, and of a board
that went whether it is `slow` (it took the read and answered late) or
unreachable, again when it turns from one to the other. `asks` names each
ask thread with a message the hub has not announced yet (`threads`, each `id` and
`path`), or the board's new chat recipient (`recipient`): an open chat or a
document's questions read their thread again only then, and once after a
reconnect, instead of polling. The hub compares the asks file's size and time each
board interval and reads it only when they changed; `/api/ask` and `/api/asks` take
the chat recipient from the hub's last board read. `task-asks` is what the task
board asks of each person, the questions waiting on them and the newest reviewer
verdict, sent each time it changes, for the desktop app (`src/live/task_asks.rs`):
no page reads it. `branches` names the branches of the
project's repository whose tip moved, as the watcher saw their refs change: an open task page
reads its gates again when the branch they were answered for is among them. Unknown regions
are ignored by the current board client.

One filesystem watcher handles registers, branch discovery and the client
build for every connection. One shared board reader checks database changes
each second; clients do no version polling. Recent deltas are retained for the
snapshot/subscribe gap. An expired cursor returns 409, or closes an already
open stream, so the client fetches a fresh snapshot. `Last-Event-ID` is not a
resume mechanism across server starts. Named `ping` events follow the client
contract: at most five seconds of silence.

Worktree index locks and reflogs do not invalidate document snapshots. A page follows each
document a visit read, and a refresh renders those again only once the page's own files changed
(main's: a file below its documents' folders or what its plugins watch, a lost event, or its
project file edited, whether its settings still load or not): a ref, a worktree's record or a
branch's edit renders none of main's but one whose cards or linking tasks moved. Text and
parsed registers reuse unchanged files, checking inode, size, modification and
change times so atomic replacement and a restored mtime still invalidate them.
Schema validators are reused by schema contents. Unchanged feed lists skip diff
construction, and branch discovery reuses parsed immutable register contents.
Task activity and sign-off reads follow their message/task/document
inputs. The agents view indexes task roles once per snapshot instead of scanning
all tasks for each historical session, and the live reader reuses its repository
location. `SYMPHONY_PROFILE=1` logs wall time for each reader stage and maintenance
pass, along with process CPU time during that stage; ordinary serving leaves those
diagnostics off. It reaches a foreground server only: the shared one keeps none of its
starter's environment but `HOME`, `PATH`, `USER`, `LANG` and `SYMPHONY_TEST_PID`. The
server reads no process table for other copies of itself and reports none: a test that starts a
server or a task database ends it, and a scratch board's database ends with the process that
started it (`symphony_board::server::scratch`). Its status still sends `orphans`, always 0,
for starters built when it counted them. The callback clock's
sequenced board snapshot also supplies the board and agents views. Unchanged raw records reuse their wire snapshot and version hash.
That snapshot also names the tasks that link a document or a folder's introduction, and the
work beside each agent on the agents page, so those reads read no board; while the board cannot be
read, the agents views answer unavailable, as its tasks do. The task page's reads (`task-page`, `task-offer`,
`task-history`, `task-brief`, `task-thread`) and the task board's search share one store of the
board, opened by the first and kept until the board fails a read on it, so no read finds the
board's server and asks `ps` and `lsof` about it again. A document renders without the lock the
board, the feed and the project list read under, which it takes only to place its copy in its
page. Renders run side by side, a visit's on tokio's blocking pool, which bounds how many run at
once, so a reader's waits on no refresh's; a copy is placed only over one whose read began before
its own, so the copy kept is the last read begun, and a visit's first read of a document begun
before a refresh that renders anew reads again, as that refresh rendered only what was followed.

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
Writes require an allowed Host and, when supplied, the app's exact Origin. So does the feed's
WebSocket upgrade, as a page of any site may open a WebSocket and read what it says: its Origin
must be the server's own page (`src/api/feed_socket.rs`).
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
three-way merge, marking the commit as the owner's verdict in the folder the project's
`[verdicts] record` names, when it names one, before updating main. Verdicts that change
generated content take a private branch with their outputs through landing. A separate worker
handles these branches so ordinary verdicts can proceed. Each generated branch runs the project's
`[verdicts] check` (this repository's: `just symphony-cli docs-audit --check`,
[CHECKS](../../docs/CHECKS.md#checks-with-no-reviewer)), then its `[verdicts] land`
command with the branch, in its private checkout as the same app actor, regardless
of the legacy migration key. Land handles a moving main through its ordinary
candidate verification and reuse. A bounded land retry keeps the immutable edit,
branch, checkout and verifier records in a locked publication journal. The next
save supplies another publication pass that calls land again without regenerating
or running the check again; a restarted worker resumes the same journal.
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
shared server calls the board's locked `reap` every five minutes for each project it
serves that has a board, each on its own schedule (`front::maintenance`). Private foreground
servers do no session maintenance, including when they use a copied scratch board.

Branch questions retain a docs-relative storage path and put the full branch
page URL in the question. Quick chat from a branch uses the chat home and the
same sent-from URL, so an agent can identify the proposal being discussed.

`just symphony` opens the browser only for an interactive human terminal. It prints
the URL without opening a browser when `TASKS_SESSION`, `CODEX_THREAD_ID` or
`CLAUDECODE` is set, or stdout is not a terminal. The native `--open` handler
enforces the same boundary even when called directly.

Old `/app/...` and `/doc/...` links redirect to root routes with their query
and branch scope intact. `/docs/...`, `/tasks`, `/task/...`, `/reports`, `/agents`, a plugin's pages
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
