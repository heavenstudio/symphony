# Symphony server

`just symphony` runs this server. The native read models live in `crates/symphony`, and
their serde-only wire records in `crates/symphony-api`. All routes run natively:
the client, reads, saves and media, under each project's `/<id>/` on the one server and
at `/` on a server of its own.

The public Rust items a consumer calls follow the
[two-release rule](../../docs/workflow/CHECKS.md#consumer-api-changes), held here by
`tests/consumer_api.rs`: keep the working old item without `#[deprecated]` alongside its
replacement for a release, then remove it in a separate later task with consumer migration
evidence and the guard list updated.

## Who a request is from

Every request is judged by its client's address before any route (`src/gate.rs`). Loopback is
allowed without asking, an address of the Tailscale ranges when `tailscale whois` names a node
of this tailnet for it, and anything else is refused (403). The same whois answer says whose
node it is, and the gate keeps who the request is from:

- **The person at the machine**: loopback, one of this machine's own tailnet addresses, and a
  node under the Tailscale login this machine is under (`Self.UserID` in `tailscale status`, the
  call the Host check already makes), such as the owner's phone. Their writes read `owner`.
- **Another login's person**: a node under any other user. Tailscale's word says who: the login
  is a sign-in, `tailscale:LOGIN`, of at most one profile in the machine's `people.json`, and a
  board that gives that profile's short name a power has added them. They see nothing else.
- **Nobody**: a tagged node, or one whose answer names no user.

A status call that gives no answer keeps the login it last gave, as it keeps the names. Until one
has answered, only loopback is the person at the machine: this machine's own tailnet addresses are
known from that answer too, so every node, the Mac's own included, counts as another login's or as
nobody's. A Mac that is itself tagged is under no login, so only the Mac reaches its boards.

What a visitor reaches is each board's word (`Gate::member`, asked of `src/roster.rs`): the
person at the machine is on every board, and another login's person is on a board that gives
their short name any power ([People](../../docs/workflow/TASKS.md#people)), as the actor of that
name, shown by their profile's display name. A failed read of the profiles or of the board adds
nobody. The one server judges each request once and keeps only the
projects whose board has added its visitor: a request under a project's id asks that board
first, and no other once it has added them, while any other request, and one under a board that
has not, asks every board (`src/projects.rs`). To them every other project is one the server does
not hold, in no list, and each path under its id answers as an id nobody registered, the same
bytes. `/api/front` and the list's installation id are the machine's, answered to the person at
it alone. A server of its own refuses a visitor its board has not added, as an address the gate
refuses. A project's router takes the visitor's actor with the request, and every save, workflow
action and attempt control is that actor's.

A write from anyone but the person at the machine is checked against that board's people as it
arrives, before its body is read (`src/powers.rs`, the table of what each route needs, and
`src/write.rs`): read fresh from the board, so a revoke holds from the next write. One whose
power the person lacks is answered 403 with the power it needs, `Forbidden: ana may not decide on
this board`, and nothing is written; a board that cannot be read answers 503. The server's log
line for a refusal names the person's short name, never a sign-in or an email.

## Hosted admission

`symphony_server::hosted::router(shared, config, host)` is an explicit, additive admission
boundary for one configured project at `/`. The ordinary router and CLI keep the local gate.
The hosted router validates the selected project against its configuration and consults no
machine registry. It requires an exact configured HTTPS authority/origin, independent verified
proxy transport, a signed visitor and a current grant bound to that actor, project and action.
The consuming `hosted::Host` implements transport, signature/key verification, membership and
session/project-bound CSRF; it pins issuer, immutable app audience and its key endpoint in
protected deployment configuration. A visitor token or forwarded header cannot substitute for
transport evidence. Host calls must have bounded deadlines and fail closed on outages.
`Transport::RequestBound` also supplies the attested body SHA-256; admission compares actual
bytes before dispatch, after authorization. The host verifies method, full target, signature,
expiry and replay policy. `ProtectedConnection` instead requires proved exclusive transport.

Native read-only routes, assets, documents and uploads use that admission. Media and feeds
need distinct grants from ordinary reads, including HEAD, conditional and range requests.
Every response is `private, no-store`; a conditional hit is authorized before returning 304.
Streams recheck policy before each chunk/event and end on identity/grant expiry or the host's
policy-change event, including while idle. A changed revision needs a new request; a refresh
cannot extend an existing request. The host subscribes policy loss and external revocation to
`Host::changes`, with any external revocation delay separately accepted before deployment.
Server diagnostic notices and detailed native errors are withheld from hosted responses.
The machine-work endpoint is excluded. Native agent/task responses keep the existing wire
shape with empty, unknown machine-work evidence and unavailable probe values; task stages and
counts use that empty projection. Machine-work and agent-probe feed payloads are omitted.
Diagnostics-only events retain their version with an empty changes array, so document section
reads can catch up to the feed cursor. Ordinary project tasks, agents and document deltas remain
available.

Hosted browser writes require exact Origin, same-origin fetch metadata when supplied,
`application/json` and a verified CSRF binding. JSON is capped at 2 MiB; duplicate keys/query
parameters, conflicting project selectors, traversal and encoded scope aliases are refused.
The first profile refuses worktree selectors, machine diagnostics/controls, pane screens/focus,
and actorless plugin dispatch. Saves and `/x/` routes go only to `Host::dispatch`, whose default
is deny, with verified `Context` and canonical input, without credential headers. An adapter
resolves any domain-specific references and rechecks authority before state or provider I/O.
It must not forward hosted mutations into the local owner's save/plugin APIs, and those saves
refuse a request the hosted admission holds: each save is the person the local gate admits. Binary upload
and storage integration need their own actor-aware adapter; a local upload URL is no grant.
Redirects stay relative or on the configured origin, with no login redirects for refusals.

This seam and its fake-host tests do not constitute hosted deployment acceptance. The
[hosted ingress contract](../../docs/operations/analysis/hosted-ingress.md) requires real consumer
transport and membership proof; provider credentials, token/key/session implementations,
foreground lifecycle/readiness, managed database and artifact durability remain separate work.
No production host adapter or CLI switch is supplied here.

## Projects

The one server (`serve --home`) keeps its record (`server.json`), log, lease and lock in the
machine folder: `~/Library/Application Support/Symphony/` on macOS, `$XDG_DATA_HOME/symphony`
elsewhere, `SYMPHONY_MACHINE_DIR` when set (every test sets a temporary one). The registry
beside them (`projects.json`) holds each project's root, id and `symphony.toml`; the people's
profiles (`people.json`, with its lock `people.lock`, both mode 600: they hold sign-ins and
contact details) are there, which `just tasks-people` writes; and the
releases installed on the machine are there too (`releases/NAME/`, with the `current` link); `symphony
project add [DIR]` writes it, and every start runs it. The build and client stay in the
starter's install folder (`--install`, `symphony/server/` in its git common dir), where a record
an older server left is read once; a project's own state (asks, thumbnails,
the queue order, the port record, the notices' record) stays in its artifacts folder.

The one server alone tells each project's people of a card waiting, an agent's question and a
reviewer's verdict, a worker of each project's hub beside its board ticks (`src/live/notices.rs`),
handing each notice to the program the machine folder's `notices/programs.toml` names for its way
([NOTICES](../../docs/engineering/NOTICES.md#the-servers-part)); a private server tells no one.

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

`symphony project add` registers the project before checking its board at
`/<id>/api/task-board?counts=1&work=0`. Only a 200 task-board JSON answer is readiness evidence;
the HTML shell at `/<id>/tasks` can answer 200 while its board is unavailable. A failed check
keeps the registration and reports the API's refusal and the JSON URL to check again.
`symphony init` checks that JSON too for an already registered project while its host answers,
and names an older selected release writer, its schema and the host's required schema in `Next:`.
An unknown local build's schema is not inferred from its path.

A board behind the running host needs a compatible project board command linked first, then
`just tasks migrate` in that project's main checkout under its migration policy. Registration,
init and the reader never migrate it or replace the shared host. Failed board operations retry
after 1, 2, 4, 8, 16, 32, then at most 60 seconds; requests and filesystem changes keep that
deadline. The one exception is a board held stopped (`just tasks database hold`, a move, a
snapshot): an attempt refused by the hold is made again at the first tick or page request after
its release, whatever was left of its deadline. After an external migration, the worker or page reader recovers on its next due
attempt without a server restart (`src/live/retry.rs`). The worker's next board tick publishes
the recovered JSON; page-reader success alone does not refresh that snapshot.

A start judges the running server by the host rule (`symphony::machine::host`). A build serves a
project's plugin when the plugin answers from a process of its own (`[plugins.process]`,
[PLUGIN_PROTOCOL](../../docs/engineering/PLUGIN_PROTOCOL.md#the-host)), or when it compiles the plugin. The
server of a release installed on the machine compiles none: it serves every project whose plugins
all answer from a process, at its pin and board schema or older, and a project one board schema
ahead, whose board it reads and writes nothing to, unless the step that moved it is declared to
keep the Symphony writing ([CHECKS](../../docs/workflow/CHECKS.md#a-schema-step-that-keeps-the-symphony-writing)).
A start that finds no server running starts
the server of the release `current` names, by its own folder's path
(`releases/NAME/bin/symphony-server`), when that release serves every registered project, and its
own build otherwise (`symphony::machine::switch`); the record (`server.json`) names the release
its server is. A start whose own build is a release's (`just symphony` in a project with no build
of its own) starts nothing while a project holds the switch: it registers its project and says,
in one line, which project holds it and that `just symphony` in that project's checkout, by its
own build, starts the machine's server. No start replaces a release's server: a project naming a
linked plugin is registered and refused, its pages naming the `[plugins.process]` command that
lets the release serve it, and a project at a newer pin or schema is refused until `current`
names a release that serves it.

While a project's build is the machine's server, a start judges it by the plugins it compiles:
served, when it holds the project's linked plugins at the project's pin and schema or newer;
waiting, when it lacks one, naming the project whose build holds them; refused, registering
nothing, when the project's pin or schema is newer than the running build's and the start's build
cannot replace it. A project with no plugins, and a start of a release, never replaces it. A start
whose build holds every registered linked plugin replaces it by hand-off: it writes
`handover.json` (its executable, root and install folder) in the machine folder and sends
`SIGUSR1`; the server hands its sockets to that build as a reload does. That signal and a stop's
(SIGTERM, SIGINT, SIGHUP) are watched before a server binds, records its port or prints its URL,
so one sent as it starts is kept and acted on once it has started: a stop then still clears its
port record. While a release is
installed, every project naming a linked plugin says on its pages that it holds the machine's
server on a project's build, naming the command that lets it go, and `just release --status` says
the same ([RELEASES](../../docs/workflow/RELEASES.md#status)); a stop and a start then make the release
the machine's server once nothing holds it.

A release's server builds nothing and watches no source: it watches the machine folder for its
`current` link moving, reads where the link points as it starts, at each move and once a period
(`backstop.rs`, for an event lost), and when it names another release, hands its sockets to that
release's server, by its own folder's path, as a reload below hands them. A release not installed
whole, or whose server does not become ready, leaves the running one serving, said in the log and
on the notice and tried again on the waits below; `current` pointed back at the running release
clears the notice. A starter's request to hand over is refused, said in the log. The release a
server is, and the client it serves, are read once, as it starts.

A server that is not following says so in its log (`server.log` in the machine folder), each line
starting `· Symphony reload:`: one for each hand-off it begins, one for each release that did not
take over, with why and when it is tried again, and one for each reason it begins none (`current`
naming the release it is, or none), said when that reason changes. The follow ends only with the
server: one that ends before, a bug, is said in the log and on the notice and started again once
its supervisor has waited for the next wake or period, so no release waits on a restart by hand
and a follow that dies at every start costs one line a wait. The refresh of a release quotes the
last of those lines when the server did not follow `current` within its wait
([RELEASES](../../docs/workflow/RELEASES.md#the-refresh)). The reload writes its lines without
`eprintln!`, which panics its thread once stderr cannot be written.

The log dates each line of the server's two streams, and of the children that inherit them,
through a socket for each (`src/log.rs`). A socket that ends under its forwarder, as when a child
shuts down the one it inherited, is replaced by a new one in the stream's place, said in the log
(`server log capture: … captured again`); were it left, every later write to that stream would
fail, and each thread that writes one by `eprintln!` would die of it.

A project's build as the shared server watches main's server sources and transitive local crate
sources. It builds through the command main's `[project] cargo` names (when unset,
`tools/cargo.sh` in Symphony's own workspace and plain `cargo` elsewhere) under
`symphony/server/reload-build.lock`, while continuing to serve.
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
at its first check, whatever that wait. A build whose sources changed or could not be read while
it ran is dropped and waits the same way, said in the log alone, as the version read next is
usually another and is built at once; a notice that already says an update failed says the drop
in its place, so the time it names is never one already past. A build kept as the running image
clears the notice and the wait, so the next failure waits 5 min again. A starter's hand-off that
fails is not tried again (the starter asks again), and its notice keeps the time of a retry that
still waits.
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
| `/api/task-diff?slug=…` | Changed paths and added/removed counts, from the branch's merge base or the recorded landing merge's first parent; at most 500 paths and 256 KiB of Git output. Binary counts are null; missing branches or history are said explicitly (`TaskDiff`) |
| `/api/task-diff-file?slug=…&path=…&base=…&head=…` | One file's patch, at most 256 KiB; the comparison must still match the summary's commit ids, otherwise 409. All Git calls in either read share five seconds, and cancellation ends the child (`src/api/task_diff.rs`) |
| `/api/task-gates?slug=…` | The task's gates at its recorded branch's tip (`TaskGates`): the Symphony's review, check and owner gates and the project's plugins' own, each met, missing, waived, held or unknown, what the task waits on first; a plugin that errs or is slow is an unknown gate, a task not on the board 404 ([TASKS](../../docs/workflow/TASKS.md#gates)) |
| `/api/task-offer?slug=…` | Resolved branch and the first offer of the project's board actions that has a payload, on work of any status (each action decides which it offers on); fetched near the controls |
| `/api/task-history?slug=…&before=…` | Thirty structured messages with timestamps and typed blocks; `after=…` catches up a loaded thread after a lost feed |
| `/api/agent-page?cursor=…` | Twenty connected agents and their current work and five latest finished tasks; `health=lost` pages lost agents |
| `/api/task-thread?slug=…` | Alias for the typed paged task history |
| `/api/agents` | The board's agents view, including probe availability |
| `/api/reports?grain=…&days=…` | The Reports page's window (`ReportsPage`): its periods by the hour, day or week, each summed from the project's roll-up, agents and board-health day files, a period no file recorded none of; the commits in the window that changed the settings file, the rules or a skill, each with what it changed (`ReportMark`); what was not read. It reads files only, never the board, and at most 64 MiB of them (`src/api/reports.rs`). The answer is kept between reads with what it was read from (the hour, each day file by its stat, the default branch's tip), so a read that finds none of it moved opens no file and starts no git (`src/api/reports/kept.rs`). Its window ends at the moment `TASKS_NOW` holds the board's clock at, when the server's environment sets it, else at the server's clock (`Hub::recorded_now`), so a server over records written at a held moment reads them, as the product specs' pictures do |
| `/api/settings` | The Settings page's one read (`SettingsPage`): the files read, what does not load, and every entry in one shape with each setting's value in force, source, scopes and class (`crates/symphony/src/settings.rs`). It reads the project's files, the board's store of preferences, the project file's saves waiting for delivery, this machine's `PATH` and the committed `symphony-github.toml`, runs no program, and writes nothing; `/api/setting` is its write. A hosted visitor is refused it, as it says what this machine has |
| `/api/people` | The People page's one read (`PeoplePage`): the board's people, groups and log, each person's powers held (their own and their groups'), display name and git handles from the machine's profiles, and the services each signs in by, never a subject, an email or a chat address; who reads it and whether they administer; and `withheld`, each power the board gives no one yet with its reason (`symphony_board::people::Power::withheld`, the words its refusal of a grant says), which the page draws as not yet givable. A board or profiles that cannot be read are said in `problem` beside what was read, a cause only to the person at the machine and never a value of the profiles' file (`src/people.rs`). A hosted visitor is refused it, as the board's people are this machine's |
| `/api/run-log?run=…&tail=…` | Part of a run's log (`RunLog`): its last 64 KiB by default, its last `tail` bytes, or `from=…&to=…` a range, at most 1 MiB; only a regular file in a day folder of the project's `logs/`, never the board (`src/api/run_log.rs`, [RUNS](../../docs/workflow/RUNS.md#logs)) |
| `/api/activity` | Latest forty document and owner-addressed task entries |
| `/api/pending` | Pending cards in document navigation order, then branch cards |
| `/api/feed?since=…` | The feed after the snapshot's version: a WebSocket to a request that asks to upgrade, Server-Sent Events to any other (`src/api/feed_socket.rs`; both forms are `symphony_api`'s feed contract) |

Before any project the one server answers `/api/projects`, to loopback and this machine's tailnet
names like every route of a project's: each project it serves, with its feed's version, the cards
waiting on the owner and the first of them, why it is refused or its board not well (the line
the project list draws under it, `src/live/health.rs`), and its task
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
cursor left out: it moves only with the document. An outline's check that names the tag of the copy
the hub follows for that page is answered `304` at the feed's cursor without rendering the
document: the hub renders that copy again when anything a render reads that it tracks changed, its
linking tasks among them, which a board read keeps current (`Hub::update_board`), and makes its
outline's tag once for each copy (`Hub::outline_kept`). A check with another tag or none, of a
document not followed yet, or of one that names a file from outside the entrypoints' folders by its
change time and size, which no refresh watches, renders as a read does.
A page whose kept copy is still current takes the cursor from `/api/board`. Every JSON read names
the feed's cursor it was read at in a header (`symphony_api::VERSION_HEADER`), on its `200` and its
`304` alike: the server takes the cursor before it reads the answer, so every event at or before it
is in the answer, and a page drops what its feed says of a read at or before the read's cursor
(`api::cursor`). The browser keeps no copy of its own: the client's cache keeps
the body and sends the tag back (docs/engineering/analysis/client-cache.md). An error carries no tag, and the
feed's stream is never read whole.

The read routes also work under `/@<branch with / replaced by ~>/`. Done and
cancelled columns show work updated in the last day; `days=7` selects the last week,
`older=1` and search reach older work. `parent=…`, `roots=1`, and
`holder=…&role=…&open=1` page child tasks, hierarchy roots and followed work.
The task board opts into read-only Build/Test placement with `activity=1`; its
status filter, counts and cursor use one filtered machine snapshot. The returned
`lifecycle_counts` retain the underlying task totals. Without that opt-in,
queries keep lifecycle status and paging semantics.
Search uses the board's full-text word index over titles, briefs, slugs and thread
messages, ordered by BM25 relevance with an exact slug first. Matching remains
in the database; a message-only match keeps its task in the returned results.
Canonical `/tasks`, `/task/…`, `/reports`, `/settings` and `/agents` routes serve the client shell.
The opaque `next` cursor follows a stable task key or agent handle, so live
insertions and removals do not shift an integer offset past an unread record.

Document and picture links in those snapshots retain the branch prefix. Removed worktrees
return 404. Empty or missing task slugs return 400 before reading the board.
A missing task returns 404; an unavailable board returns 503 while the document
board continues without task counts. A board one schema ahead of the build's (main's build moved
it on before a release caught up) is read, and nothing is written to it from the server: its ticks
dispatch no callback, give no work and keep no owner's asks, the schema rule refuses every other
write, and every page says so on the notice until the board and the build agree. The exception is
a board whose step is declared to keep the Symphony writing: its ticks and saves write it as at
the build's own schema, and the notice says nothing of it (`Hub::read_only_ahead`). A board that
took a read on a connection that worked and did not answer it within the 3 s wait is slow: 504,
"the task board is slow", never "cannot be reached". The wait bounds each write of a request and
each read of its answer, inside the client's 15 s request limit, and a request is never sent
again, as the server may have run it. Native routes apply both
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
bundle keeps its strict errors. A release's build of the server (`SYMPHONY_CLIENT_BESIDE=1`)
embeds no client: run from the release's folder in the machine folder
(`releases/NAME/bin/symphony-server`, through `current` or not), it serves that release's
`client/HASH/` from beside it, whatever else is named
([RELEASES](../../docs/workflow/RELEASES.md#the-install)). `symphony-server --serves` prints, as one line
of JSON, the plugin protocol, the board schema and the Symphony pin the build serves.
`just client-update` in the Symphony workspace refreshes the real distribution before committing
client changes. Its default root is the checkout's top level even when the command starts in a
subfolder; `--root` selects another folder. A private server on the main checkout leaves the
shared app's `artifacts/docs-app.port` record alone. A project's page serves the
shell with revalidation by its ETag, as is any client file not named by its
content. A file named by its content, an `assets/` name whose last `-` part is
the build's sixteen hex digits (the shell script, every page module and stylesheet), is
cached for a year as immutable, so a
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
move one key after another (`key` and `after`). They are `queue-item`,
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
`stamp`; `task-counts` updates the column totals.
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
each second, on a thread of its own beside the document refreshes, so neither waits on the
other: a refresh keeps the board's counts the last tick gave its page, never older ones it read
first, and the hub's stop ends the reader's wait between two checks at once. Each check also
looks for the pictures held cards wait on (a card posted before its picture was on disk, which no
watcher follows), and wakes the document refreshes once one has landed: a held card is posted
within a second of its picture. Clients do no
version polling. Recent deltas are retained for the
snapshot/subscribe gap. An expired cursor returns 409, or closes an already
open stream, so the client fetches a fresh snapshot. `Last-Event-ID` is not a
resume mechanism across server starts. Named `ping` events follow the client
contract: at most five seconds of silence.

Worktree index locks and reflogs do not invalidate document snapshots. A page follows each
document a visit read, and a refresh renders those again only once the page's own files changed
(main's: a file below its documents' folders or what its plugins watch, a lost event, or its
project file edited, whether its settings still load or not): a ref, a worktree's record or a
branch's edit renders none of main's but one whose cards or linking tasks moved. A board read
that finds a task's slug, title, status or links moved renders again, on every page, the
followed documents whose linking tasks it moved and no other, so a page left open follows its
related tasks with no file changed. Text and
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
shutdown releases waiting reads before joining the worker, which ends once its board reader and
document refreshes have. A reload replacement awaits the first board result before
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
`/api/ask-reply`, `/api/task-action` and `/api/setting` run natively through `symphony::save`.
`/api/setting` is the Settings page's write (`SettingWrite`): a preference kept in the board's
store, its entry removed, or its stored value made the project's by a commit of the project file
([PROJECT_FILE](../../docs/operations/PROJECT_FILE.md#the-settings-page)); a hosted visitor is refused it. A task
action is a board action of the project's plugins, named by the offer it answers
(`TaskOffer`: the action, its label, the branch and the payload it hands back);
an action no plugin of the project offers is a 404, and one the action refuses
says and moves nothing on the board.
`/api/people-change` (`PeopleChange`) is one change of the People page, the command's own
(`symphony_board::people::change` for an add, which writes the profile with the board's row under
one lock), and needs `administer`; `/api/people-profile` (`ProfileWrite`) writes a profile alone,
the person at the machine's any, a person's their own display name and git handles, and needs
only being on the board ([TASKS](../../docs/workflow/TASKS.md#the-people-page)). Each answers the
page as it reads after the write, is the project's own board's (a worktree's scope answers 404),
and is refused a hosted visitor; a server that names no machine folder answers 503 to a write of
a profile. Only the person at the machine gives a sign-in, and a failure's cause (which can name
the machine folder's path) is told only to them: anyone else is told what failed and what
stands, and the server's log keeps the cause.
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
[CHECKS](../../docs/workflow/CHECKS.md#checks-with-no-reviewer)), then its `[verdicts] land`
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
serves that has a board, each on its own schedule (`front::maintenance`). While it is the
installed release `current` names, each round first tries that project's held schema update
again, each command of that within the same 240 s, and logs what it did (`· held update of ID:
…`; `held`, [RELEASES](../../docs/workflow/RELEASES.md#a-held-update-tried-again)). After a sleep, that
project's clock tick holds its callback dispatch and its give until a round begun after the wake
has ended, or a round's timeout and the slack it may start late within (240 + 30 s) of awake
time have passed (`live::presence`). Private foreground
servers do no session maintenance, including when they use a copied scratch board, and so never
hold their tick.

Branch questions retain a docs-relative storage path and put the full branch
page URL in the question. Quick chat from a branch uses the chat home and the
same sent-from URL, so an agent can identify the proposal being discussed.

`just symphony` opens the browser only for an interactive human terminal. It prints
the URL without opening a browser when `TASKS_SESSION`, `CODEX_THREAD_ID` or
`CLAUDECODE` is set, or stdout is not a terminal. The native `--open` handler
enforces the same boundary even when called directly.

Old `/app/...` and `/doc/...` links redirect to root routes with their query
and branch scope intact. `/docs/...`, `/tasks`, `/task/...`, `/reports`, `/settings`, `/agents`, a plugin's pages
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
