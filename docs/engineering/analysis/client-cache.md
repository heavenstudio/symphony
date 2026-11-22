# The client cache

What it takes for a Symphony page to draw at once from what the browser last held and then ask the
server only for what changed, as proposed on the client-cache card in [DECISIONS](../../DECISIONS.md),
and the measurements it rests on. Each fact about the code names the file and function it was read
in. "Measured 2026-11-11" marks this machine that day.

The ask, as the board task `perf/client-cache` records it (2026-11-11): the client should behave like
Apollo's cache-and-network policy. A page draws at once from what the browser last held, then asks
the server only for what is stale. Not everything is kept: task detail must not fill storage.

## The design

Recommended, in five choices; the card asks the owner for each.

1. **Where it lives.** One IndexedDB database for the server's origin. Every key starts with the
   project's id, as `project::stored` does for the browser's other keys, and every entry records
   its project, kind, size and last use.
2. **How freshness is checked.** Every JSON read the server answers carries an `ETag`, a checksum
   of its body. The client sends it back with the next read of the same thing (`If-None-Match`),
   and an unchanged read answers `304` with no body. The live feed is unchanged: it always starts
   from the version of a fresh `/api/board`.
3. **What is kept.** The board, the queue, the 30 tasks and the 20 documents opened last, each with
   only what its page drew. At most 8 MB a project (tasks 2 MB, documents 6 MB) and 32 MB for the
   origin; an entry unused for 14 days goes; over a cap the least recently used goes first. Search
   results, older thread pages, agents, chat and the landing offer are never kept.
4. **Across versions.** The wire crate names a cache format number; a client with another number
   clears the cache. A cached value the client cannot read counts as missing. A server update needs
   nothing: a body that changed has a new checksum.
5. **A stale draw.** A page drawn from the cache shows a notice that it may be out of date, and its
   verdict, answer, message and task buttons wait until the server has answered. Drafts stay
   editable. The landing offer is always fetched.

### A return visit, step by step

1. The page's module starts and reads from the cache what its route draws first: the board's
   counts and columns, a task's page and newest messages, a document's outline and the bodies of
   the sections near the screen. Each is one IndexedDB read.
2. What it finds is drawn at once, marked stale (choice 5).
3. In the same moment the page reads `/api/board` for the feed's version, and then checks every
   cached read in parallel, each with its `If-None-Match`. Reading the version first keeps today's
   order, so the feed replays anything that changes between the reads. A document page reads no
   board: as built, the server tags a document read without the feed's cursor and names the cursor
   in a header, on its `304` too (`symphony_api::VERSION_HEADER`), and the page opens the feed from
   it; a `304` naming no cursor, as an older server's, is read again whole at once. A `304` keeps
   the copy and marks it used; a `200` replaces it on the page and in the cache.
4. Once every read has answered, the stale mark goes and the feed opens from that version, as today
   (`apps/symphony-ui/src/live.rs`, `keep_live`).
5. If the server does not answer, the page stays drawn and marked, and the reconnect backoff the
   page already has keeps trying.

A first visit, or a read the cache does not hold, goes as today, and its answer is kept.

- [r] **client-cache** — A page draws at once from what this device last held, marked as possibly out of date, while it asks the server whether anything changed; an unchanged read answers with no body. Measured today, a return visit to a task page draws its brief after 192 ms on this Mac and 2.9 s at phone speed, waiting on data the device already had; with the cache, about 85 ms and 1.8 s. Five choices, each with its default: 1. Where: IndexedDB, keys by project. 2. Freshness: a checksum per read, answered `304` with no body when unchanged; the live feed as today. 3. Kept: the board, the queue, the 30 tasks and 20 documents opened last, each with what its page drew; 8 MB a project (tasks 2, documents 6), 32 MB in all, 14 days, least recently used out first; never searches, older messages or the landing offer. 4. Versions: a format number in the wire crate clears the cache when it changes; a server update needs nothing. 5. Stale: a notice, and the verdict, answer and task buttons wait until the server answers. Approve to take all five defaults, or comment with the ones to change. [[analysis/client-cache#The design]]
  [2026-11-11 18:30] agent — Posted for the board task `perf/client-cache/design`.

    **Today.** Every data read is answered `no-store`, so a return visit downloads its data again and draws nothing until it arrives. Measured on a private server with a copy of this board, five runs each, warm: home 107 ms, `/tasks` 401 ms, a task 192 ms, this register 194 ms, the consuming project's register 296 ms; at phone speed (Fast 3G) 2.3 to 3.8 s. The page's code starts asking for data at 50 to 75 ms, and at 1.8 s on the phone.

    **1.** IndexedDB works on the tailnet's plain `http://`, reads off the page's thread and has room. localStorage holds about 5 MB for the whole origin, shared with drafts; a service worker needs a secure page, which the tailnet is not.

    **2.** A checksum survives a server restart. The feed's version moves with every agent's health, and the feed keeps only its last 128 events, a few minutes on a busy board.

    **3.** Thirty tasks take about 1.3 MB; separate caps keep a week of tasks from pushing out the documents.

    **4.** Clearing on every client build would cost a slow visit per page after each update; main's client changed 7 times today.

    **5.** Nothing that acts is given on content the server has not confirmed. The landing offer is always read fresh.

    **Gain.** Every page draws in about 60 to 85 ms here and 1.8 s on the phone, about 0.6 s once `perf/asset-caching` lands too. An unchanged visit's data drops from 13.5 to 6.4 KB on `/tasks` and from 26.7 to 9.9 KB on the large register. A first visit is unchanged but for the store's code, about 1 to 2 KB. [[analysis/client-cache#Expected gain]]
  [2026-11-11 18:48] owner · approved
  [2026-11-11 19:00] agent — Recorded: the design as recommended, built by the board task `perf/client-cache/build`. The owner added two things in the terminal (2026-11-11), now in that task's brief. Documents are kept by how often they are visited, not only how recently: a project's entrypoint pages stay, a page opened once goes first. And pages are fetched ahead: the tasks and documents a drawn page links to are read into the cache while the device is idle, within the same caps, never on a metered or slow connection, and never for a link that acts.
  [2026-11-11 19:00] agent · resolved

## What a visit loads today

Every page kind loads the same way (`apps/symphony-ui/assets/shell.js`, `show`): the HTML, the
shell script, the page kind's script, its wasm, its stylesheets, and only then the data. Nothing
the page draws comes before its first API answer.

- **Home `/`** reads `/api/board` (`live.rs`, `fetch_board`).
- **The task board `/tasks`** reads `/api/board`, then the counts
  (`/api/task-board?counts=1`), then each column near the screen. Each column waits 250 ms before
  its read (`apps/symphony-web/src/boardlive.tsx`, `LiveBoard.column`), on the first visit as on a filter
  change.
- **A task page** reads `/api/board`, then, only once it has answered, `/api/task-page`, then the
  thread's newest page (`/api/task-history`) and the subtasks when they near the screen
  (`apps/symphony-web/src/task-live.ts`, `LiveTask.load`).
- **A document or the register** reads `/api/document?view=outline`, then the bodies of the
  sections near the screen (`/api/section?keys=…`), then opens the feed for those sections and
  reopens it when the set of sections changes (`apps/symphony-ui/src/docs_live.rs`,
  `document_live`, `load_sections`, `follow_document`).
- **Every page** also reads `/api/activity` and `/api/pending` for its header, and again each
  time its feed opens after the first (`apps/symphony-web/src/frame.tsx`, `FrameRefresh.connected`;
  `apps/symphony-web/src/state.ts`, `MetadataReads.connected`): a document's feed reopens once its
  sections are known, so a document reads them twice, and `/api/postponed` with them
  (`docs_live.rs`, `document_live`).

Every API read is answered `Cache-Control: no-store` with no validator
(`apps/symphony-server/src/api.rs`, `no_store`), so a return visit downloads every byte of data
again and waits for it. Only the hashed `.wasm` and `.css` files and media are cached for good
(`apps/symphony-server/src/api/client.rs`; `apps/symphony-server/src/media_files.rs`); the hashed
scripts revalidate, which `perf/asset-caching` is changing.

### What a read carries to compare by

| read | in the body today | moves when |
|---|---|---|
| `/api/board` | `version`, the feed's cursor; `build` | any change in any region of the board, agents' health included; a restarted server counts from its own start |
| `/api/task-board` (counts, a column's page) | nothing | any field of a listed task, its agents' health included; the machine's work, unless read with `work=0` |
| `/api/task-page` | the task's `updated`, `brief_revision` | the task or its brief changes, or an agent on it changes health |
| `/api/task-history` | message ids, as the `before` and `after` cursors | a message is posted |
| `/api/document?view=outline` | `version`, the feed's cursor at the read; each section's `hash` | the document's text; the version also moves with anything else |
| `/api/section?keys=…` | each section's `hash` | that section's rendered text |
| the feed | each event's version | every change; the last 128 events are kept in memory |

A section's `hash` is the first 16 hex digits of the SHA-256 of its rendered HTML and media
(`crates/symphony/src/render.rs`, `hash`), so a document's unchanged sections can be recognised
without asking the server. The feed keeps its last 128 events in memory
(`apps/symphony-server/src/live.rs`, `HISTORY`); a `since` older than them answers `409`
(`apps/symphony-server/src/api.rs`, `events_since`), and a version is never compared across a
server restart (`crates/symphony-api/src/lib.rs`, the feed contract).

## Measured

Measured 2026-11-11 on a private server: the release `symphony-server` of `c9a0af8` (the consuming
project's installed host, this branch's base) run with `--port 0` and a temporary machine folder, never the
shared server. It served a copy of this repository's board (146 tasks, 547 messages, read once from
the live board and loaded into a private in-memory database) and this repository's `docs/`, plus
the consuming project's `docs/DECISIONS.md` (128 KB, 128 cards) as a large register. Each page was loaded in
headless Chrome 157, driven over its DevTools protocol, at 1280 × 900. **Cold** is a new profile;
**warm** is the same profile in a new browser, so only the disk cache is left, as on a return visit.
Five runs of each, medians; loopback and Chrome's Fast 3G profile (562.5 ms latency, 180,000 B/s
down), the profile `perf/panel-first-open` uses for the phone. Other agents were loading the machine
(load average 73 when the page runs began, 49 when they ended), so times are upper bounds; bytes
and requests do not depend on load.

**First content** is the time from navigation to the page's own data on screen: the first task
card on `/tasks` (the client's `tasks-ready` mark), a task's brief, a document's outline with its
first sections (`document-ready`), the home page's next card. **Asks for data** is when the page's
code first runs a read: the start of its first API request.

| page | network | first content, cold | first content, warm | warm: asks for data at |
|---|---|---|---|---|
| home `/` | loopback | 198 ms | 107 ms | 51 ms |
| task board `/tasks` | loopback | 539 ms | 401 ms | 49 ms |
| a task (`perf/panel-first-open`) | loopback | 254 ms | 192 ms | 73 ms |
| this register | loopback | 339 ms | 194 ms | 65 ms |
| the consuming project's register | loopback | 412 ms | 296 ms | 63 ms |
| home `/` | Fast 3G | 5.2 s | 2.3 s | 1.8 s |
| task board `/tasks` | Fast 3G | 6.8 s | 3.8 s | 1.8 s |
| a task | Fast 3G | 7.1 s | 2.9 s | 1.8 s |
| this register | Fast 3G | 7.2 s | 3.0 s | 1.8 s |
| the consuming project's register | Fast 3G | 7.3 s | 3.1 s | 1.8 s |

| page | sent, cold | sent, warm | of it, data | requests | warm: on the wire, from disk |
|---|---|---|---|---|---|
| home `/` | 225 KB | 5.5 KB | 4.6 KB in 3 reads | 15 | 6, 9 |
| task board `/tasks` | 262 KB | 14.4 KB | 13.5 KB in 9 reads | 20 | 12, 8 |
| a task | 450 KB | 9.0 to 16.4 KB | 8.1 to 15.5 KB in 5 or 6 reads | 17 or 18 | 8 or 9, 9 |
| this register | 462 KB | 13.6 to 17.7 KB | 12.7 to 16.8 KB in 11 or 14 reads | 24 or 27 | 14 or 17, 10 |
| the consuming project's register | 476 KB | 27.6 KB | 26.7 KB in 9 reads | 22 | 12, 10 |

Bytes are what crossed the network, gzipped, headers included (Resource Timing's `transferSize`,
and the navigation's for the HTML), besides the feed's open stream. Requests count the HTML and
every read but the feed. A task reads its thread's newest page only when the thread nears the
screen, so some runs read it and some did not. This register at Fast 3G read its header three
times, not twice, as its feed reopened once more while its sections arrived; the lower figures are
loopback's.

**What a warm visit waits for.** On loopback the page's code asks for data within 50 to 75 ms; the
rest is data the device already had. `/tasks` spends 250 ms of its 401 in the columns' wait. At Fast
3G, 1.8 s go before the code runs: the HTML's revalidation, then the shell script's, then the page
kind's script's, three round trips of about 0.58 s in a row; the wasm and the stylesheets come from
the disk cache. Then each page waits for its data in round trips one after another: a task page
reads `/api/board` and only then `/api/task-page`, 1.2 s; `/tasks` reads the board, then the
counts, then 250 ms, then the columns, 2 s.

**The meter tells.** The same measure reads 0 bytes for the cached wasm and stylesheets, 300 for
each revalidated script, and the full size for each API read, cold and warm alike: nothing of the
data is cached today. The five runs of each Fast 3G cell agree within 3%; on loopback a run caught
by the load can be ten times slower (`/tasks` warm: 386 ms to 3.3 s), which the median sets aside.

**The reads one by one**, raw and gzipped, and the time the server took to its first byte (five
reads each, min / median / max, curl on loopback; load average 25). Every one is answered
`Cache-Control: no-store` with no `ETag`.

| read | raw | gzipped | server |
|---|---|---|---|
| `/api/board` | 0.8 KB | 0.5 KB | 1 / 1 / 1 ms |
| `/api/task-board?counts=1` | 0.5 KB | 0.2 KB | 1 / 1 / 1 ms |
| `/api/task-board?status=ready` (30 tasks) | 13.2 KB | 3.3 KB | 1 / 1 / 2 ms |
| `/api/task-page` | 9.0 KB | 2.5 KB | 13 / 21 / 31 ms |
| `/api/task-history` (10 messages) | 31.9 KB | 7.0 KB | 12 / 14 / 14 ms |
| `/api/document?view=outline`, this register | 6.1 KB | 1.7 KB | 27 / 44 / 67 ms |
| `/api/document?view=outline`, the consuming project's register | 56.4 KB | 15.2 KB | 152 / 204 / 317 ms |
| every section body, this register | 77.5 KB | 16.9 KB | |
| every section body, the consuming project's register | 463.5 KB | 84.5 KB | |
| `/api/queue` | 0.6 KB | 0.4 KB | 1 / 1 / 2 ms |
| `/api/activity` | 13.4 KB | 2.9 KB | 1 / 1 / 9 ms |
| `/api/pending` | 0.4 KB | 0.2 KB | 1 / 1 / 1 ms |

**The browser's storage**, on the private server's origin in the same headless Chrome, three runs:
opening an existing IndexedDB database in a new browser took 2.4 to 3.6 ms, and reading the large
register's outline from it 0.1 to 0.5 ms; writing it 0.7 to 3.3 ms. localStorage refused its tenth
half-million-character item, so it holds about 5 million characters for the whole origin; it
parsed the same outline in 0.1 ms and wrote it in 0.7 to 0.9 ms. IndexedDB offered 10.7 GB. A
`fetch` that sent `If-None-Match` itself, with the browser's cache off, was handed the `304` with an
empty body, as the design needs.

## Where the cache lives

| | IndexedDB (recommended) | localStorage | a service worker's Cache Storage | the browser's HTTP cache |
|---|---|---|---|---|
| draws before the server answers | yes | yes | yes | no: a copy that must be checked waits for the check |
| over the tailnet's plain `http://` | yes | yes | no: secure pages only | yes |
| room | a share of the disk: 10.7 GB offered here | about 5 MB for the whole origin | a share of the disk | the browser's |
| on the page's thread | no: reads answer later | yes: each read parses a string | no | no |
| kept apart by kind, capped, least used out first | by the client | by the client, by hand | by the client | by the browser alone |
| tells the page a copy is old | yes | yes | yes | no |

- **localStorage** is one small quota for the whole origin, which every project of the one server
  shares with the drafts, verdict retry ids and theme the app keeps there today
  (`platform::Platform::save`, `shell.js`). A cache that filled it would make a draft's save fail
  without a word. Every read parses a string on the page's thread, though at these sizes that is
  quick: 0.1 ms for the large register's outline here, and 0.7 to 0.9 ms to write it.
- **A service worker and Cache Storage** exist only on secure pages. `127.0.0.1` is one, but the
  phone reaches the server over the tailnet at `http://<machine>.<tailnet>.ts.net:8765`, which is
  not, and the server's save check accepts only an `http://` origin today
  (`apps/symphony-server/src/write.rs`, the Origin check). Serving the tailnet over HTTPS would
  open this option; that is a change of its own.
- **The browser's HTTP cache**, with an `ETag` and `no-cache` in place of `no-store`, would save
  the bytes, but the page would still wait for each check before drawing anything, and the browser
  alone decides what it keeps and for how long.
- **IndexedDB** reads and writes off the page's thread, keeps the JSON as the browser parsed it
  (no string to parse again), has indexes for least-recently-used eviction, and works on plain
  `http://`. Opening it and reading the large register's outline took about 3 to 4 ms in a new
  browser here, against the 50 to 75 ms a warm page takes to start asking. Its cost is code: kept
  in `shell.js` as a small module, about 1 to 2 KB gzipped (estimated, not built), every page kind
  shares it, where IndexedDB's bindings compiled into each page kind's wasm would cost each of
  them, under the first-visit cap. The desktop app decided on the desktop-client card shows the
  server's own pages in a web view, which has IndexedDB too.

## How freshness is checked

**A, a checksum per read (recommended).** The server answers each JSON read with a weak `ETag`,
the first 16 hex digits of a hash of the JSON it sends, and `304` with no body when the request's
`If-None-Match` names it, as the server already answers for media and for the client's own files
(`apps/symphony-server/src/api.rs`, `matches_etag`, used by `media_files.rs` and
`api/client.rs`). The answer keeps `Cache-Control: no-store`, so the browser holds no second copy:
the client keeps the body and its checksum in its own cache and sends the checksum itself. The checks go in parallel once `/api/board` has answered, so on the phone the page is
current two round trips after its code runs, as soon as today's task page draws, and it is already
drawn while it waits.

- It survives a server restart, because the checksum is of the content, not of a counter.
- The server still builds each body to hash it, so it saves the transfer and the client's parsing,
  not the server's work. The large register's outline took the server 204 ms to its first byte
  (median of five, 152 to 317), and a `304` for it would still wait that long; the page is drawn
  meanwhile. A later step can check before building: a document by its file's content, a task by
  its `updated`. As built, a document's outline is checked before building, against the copy the
  server's hub follows ([the server guide](../../../apps/symphony-server/README.md)).
- The feed's `version` cannot be the checksum: it moves with every change anywhere on the board,
  so on a busy board nothing would ever answer `304`. On the private server it went from 2 to 24
  in its first minutes with no task edited: all 22 events were agents' health changing as the
  copied board's sessions aged, and each one also resent the tasks those agents held.
- A column page of `/tasks` changes whenever an agent on one of its tasks changes health, because
  each task record carries its agents' health. `perf/feed-deltas` takes that health out of task
  records; until it lands, those pages answer `200` more often than they changed, which costs what
  it costs today and no more.
- The task board's listings also carried the machine's Build and Test work (`machine_work`), whose
  sample number, ages and reasons move with every recorder's heartbeat, so a listing with it never
  answered `304` while a build ran. The task board reads its counts and columns with `work=0`,
  which places each task by the machine's work as before and leaves the work out of the body, and
  reads the work apart (`/api/machine-work`, kept beside them and followed by the feed's
  `machine-work` events). A task page still carries it (`/api/task-page`, `machine_work`), which
  the server cannot leave out yet: on a private server over a scratch board, with this
  machine's builds running, a scratch task's page read every 2 s changed body on 291 of 299
  reads in ten minutes, and on 87 of 89 in three more, and without its `machine_work` on none.
  The work is 4.3 KB of that page's 4.8 KB (1.4 KB gzipped, the rest 0.3 KB). So while a build
  runs anywhere on the machine a task page's check answers `200`, costing what a read without the
  cache costs; its thread's still answers `304`. Splitting it out is `perf/feed-deltas`'s, the
  task board's way: `work=0` on `/api/task-page`, the work read apart and followed by the feed.
  The agents' views are never kept.

**B, the feed says which keys moved.** The client would reopen the feed from the version it cached
and drop what the events name. The feed keeps only its last 128 events, in memory (`HISTORY`), and
answers `409` to anything older; the polish sweep counted 37 agent changes a minute on the shared
server's `/symphony/` (`perf/feed-deltas`), so 128 events last a few minutes, and a restart loses
them all. A return after lunch, or after any server update, would fall back to a full read. To work
across hours, the server would keep each key's last-changed version and an id for its own run, and
after a restart it would still need A. B could come later, on top of A, to save the check's round
trip on the phone.

## What is kept

The rule is per kind: what a page draws first is kept; what it loads on a click or a scroll is not.
Sizes are the raw JSON of each read, measured on the private server.

| kind | kept | never kept | size measured |
|---|---|---|---|
| board | `/api/board`; the counts and the first page of each column, for the default filter (24 h) | searches, the week and older windows, *Load more* pages | `/tasks` reads 38 KB in all |
| queue | `/api/queue` | | 0.6 KB with one card waiting; it grows with the owner's queue |
| task | its `/api/task-page` and its newest thread page, for the 30 tasks opened last | older thread pages, the landing offer (`/api/task-offer`), agents, subtasks' further pages, the brief's later chunks (`/api/task-brief`) | `perf/panel-first-open`: 9 KB page, 32 KB thread |
| document | its outline, and the section bodies it drew, each under its `hash` (a card's with its thread's hash, which the outline names), for the 20 documents visited most; a project's entrypoint page, its folder and its intro, pinned | versions, diffs, asks, other folders | this register: 6 KB outline, 78 KB of bodies if read to the end; the consuming project's register: 56 KB outline, 464 KB of bodies |
| reports | `/api/reports`, each window the page drew (the grain and its days) a part of one item | | this board: 4 to 24 KB a window by the day or the week, 43, 86 and 141 KB by the hour; all nine windows 337 KB |
| header | nothing | `/api/activity`, `/api/pending`, a document's `/api/postponed`: 14 KB together, and the activity toasts compare against what the page last read (`apps/symphony-web/src/state.ts`, `keepsPanelBaseline`) | |

**Caps.** At most 8 MB a project: 2 MB for tasks, 6 MB for documents, and the board and queue
outside both, at well under 100 KB here (each column's first read is one page, so a larger board
does not grow it much). The reports are outside both as well: nine windows at most, the largest
bounded by its 168 hours, a third of a megabyte in all here. At most 32 MB for the origin, which the projects of the one
server share. An entry not used for 14 days goes; over a cap, the least recently used entry of that
kind goes first. A task is kept once opened, or read ahead from a page linking to it, which ranks
below every opened one ([Read ahead](#read-ahead)). At the sizes above, 30
tasks take about 1.3 MB; documents fill their 6 MB at about eleven registers the size of the
consuming project's read to the end, or some seventy of this repository's. A busy week of opened
tasks cannot push the board or the documents out, and a long reading session cannot push out the
tasks.

As built, a document's section bodies are kept with its outline, in its own item, and go with it:
a body whose hash the next outline no longer names is read again and replaces it. A page of a
branch (`/@<tree>/…`) is kept under its own scope, and goes by the same age rule once its branch is
gone.

## Read ahead

The rulings on the build's thread: a page loads eagerly the tasks and documents of related and
adjacent information its links name, and a large document is never loaded whole that way, only a
given amount of its content, a screen's worth or a little more. As built (`perf/client-cache/ahead`):

- **When.** Once a page has its data and the browser is idle (the shell's `settled`, which its
  warm-up of the persistent modules waits for too) and its feed is open, the shell reads ahead
  (`apps/symphony-ui/assets/shell.js`, `ahead`); a page whose feed is not open within a minute
  reads nothing. Never when the browser asks to save data or reports a 3g, 2g or slow-2g link.
- **What.** The first 8 links in the page's `main` that are on the screen and open a task
  (`/task/SLUG`) or an entrypoint's Markdown document, in the page's order, each once. Never a
  link that acts (a `download`, `data-native`, another window), one with a search, the page
  itself, a folder, another origin's or another checkout's (`/@tree/…`).
- **How.** One read after another, each with the shell's own `fetch`, which the page's count of
  its reads in flight does not see, at `priority: "low"`, past the HTTP cache, marked
  `Purpose: prefetch` and naming the tag kept for it (`assets/cache.js`, `ahead`), so an item read
  ahead before costs a `304`. A task: its `/api/task-page` and its thread's newest page, the two
  reads its visit keeps. A document: its first part, `/api/document?view=first`. Every answer's
  body is read to its end, a refusal's too (a link to a document the project lacks answers
  `404`): Chrome ends a read only once the page has read its body, and a read left unread stays in
  flight for as long as the page is open, so `symphony shot` would wait on it.
- **A document's first part.** The outline's head with all its headings, so a visit draws the
  contents whole at once and nothing moves when the outline answers, and, whole, the leading
  sections that fit with it in `symphony_api::AHEAD_BYTES`, 16,000 bytes of JSON, each with its
  card, its thread and its anchors (`DocumentOutline::first`; the server's
  `crates/symphony/src/docview.rs`, `first`). A small document comes whole. A head alone over the
  cap answers `413`, and nothing is kept. The server renders it as it renders an outline but does
  not follow it (`Hub::document_ahead`): it publishes no change and the documents' refresh never
  renders it again; the visit's own outline read follows it. Measured on this repository's
  documents (2026-11-13), the headings take 319 to 1,441 bytes and the leading sections within the
  cap hold 990 to 7,300 characters of text: `docs/workflow/TASKS.md` 6,627 in its first 10 sections (13,607
  bytes, of a 27,415-byte outline), `docs/workflow/RELEASES.md` 6,415 in 8, `docs/workflow/CHECKS.md` 3,084 in 10,
  `docs/DECISIONS.md` 990 in 6, its next a 12 KB card. A phone's screen shows about 1,200
  characters of a document and a desktop's about 2,700: the cap is a screen or less on a register
  of large cards, a few screens of prose.
- **Kept.** Unvisited (`keep(…, ahead)`): over a cap an item read ahead goes before any visited
  one, and a read ahead never replaces what a visit kept, nor reads an item a visit kept.
  Opened, a task draws from it as from a visit's copy and checks it with a `304`. A document
  draws the head and the first part's bodies (`cache::document::split`), then reads its outline
  naming the first part's tag; the outline's tag is another, so the answer is a `200`, and the
  bodies it names alike stay: only the sections past the first part are read
  (`tests/browser/cache_ahead.rs`).
- **Outside the first visit.** Every read ahead starts after the page's own reads have finished
  and is marked as a prefetch, which the budget tests count apart (`prefetched_bytes`), as the
  shell's warmed modules are.
- **Measured** (`tests/browser/cache_ahead.rs`, the release build on the stand-in). A page linking
  a task and seven documents on the screen reads them ahead in 9 reads, 5,511 bytes on the wire
  (8,661 of JSON), after its own 10 reads (2,510 bytes). A 60-section document under 60 headings
  has a first part of 13 sections and every heading, 7,682 bytes on the wire (15,078 of JSON, of
  58,265 for the whole). Eight stays the number: the most it can cost is eight first parts at the
  cap, 128,000 bytes of JSON, about 65 KB on the wire at that document's ratio, once per page, at
  low priority and after the page; read again, each costs a `304`.

## Across versions

- **A client update.** The wire crate (`crates/symphony-api`) names a cache format number. Each
  entry carries the number it was written under; a client whose number differs clears the cache
  when it opens it. A test ties the number to the kept types' fixtures, so a change to one of them
  cannot land without it. A page reloaded for a new client build (`state::reload_for`) keeps its
  cache while the number holds.
- **A value the client cannot read** is a miss: it is deleted and fetched.
- **A server update or restart** needs nothing. A body the new server renders differently has a
  new checksum, so its first read answers `200`; one it renders the same still answers `304`.
- **The alternative** is to clear the cache on every client build. It is simpler, and it costs one
  slow visit per page after each update. Main's client changed 7 times on 2026-11-11 (`git log` of
  `apps/symphony-server/client/public`); once projects follow nightly releases, at most once a day.

## A stale draw

- **Shown.** A page drawn from the cache takes the stale state the page already has while it
  reconnects (`main[data-feed=stale]` and its notice, `apps/symphony-ui/src/live.rs`,
  `PageNotices`), worded "Shown from this device, saved 14:32. Checking for changes…". It clears
  when every read has answered and the feed is open. On loopback that is tens of milliseconds, or
  the 204 ms the large register's outline takes to build; on the phone, two round trips once the
  code runs, about 1.2 s at Fast 3G.
- **Buttons.** While stale, and while a page reconnects (the same rule), the verdict, answer,
  message, comment, question, priority, cancel, landing and Later buttons are disabled and say
  "Checking…" (`cache::STALE`, which `PageNotices` sets). Drafts stay editable and are saved as
  today. A page drawn from the cache also takes `main[data-cache=device]`, which a skeleton reads:
  a warm draw needs none.
- **Never from the cache.** The landing offer: approving it lands the exact patch the server
  showed (`apps/symphony-ui/AGENTS.md`, task views), so it is always read fresh.
- **No answer.** The page stays drawn, the notice says "Not reachable: shown from this device,
  saved 14:32" while a read fails and the feed is not open, the buttons stay disabled, and the
  reconnect backoff keeps trying. A read that fails once the feed is open (an error, a 15 s
  timeout) is asked again with the same backoff until it answers, the notice saying it is
  checking; the feed's `machine-work` event answers for the work's read.
- **A task gone since.** Once every column drawn from the device has answered, a task one of
  them drew that no fresh answer listed is dropped: the machine's work may since place it in a
  column whose answer never named it.
- **A size changed since.** A copy from the device is drawn in the shape it was kept in, so what
  changed size while away moves what follows it when its answer comes. A task page's gates hold
  the room the drawn page counts: gates that went from 1 to 4 while away move the brief when they
  answer, 317 px on a phone and 130 px wide, where a page drawn from the server, which counts
  them before they come, moves it 41 and 0 (measured on `perf/client-cache/task`'s review). An
  unchanged copy moves nothing.
- **Not in the default.** A verdict could carry the `hash` of the card it was given on, and the
  server refuse it when the card changed since. That would also cover today's case of a feed that
  lags; it changes `/api/decision`, so it is the owner's to ask for.

## Expected gain

Inferred from the measures above, not built: a warm page with the cache draws when its code starts
asking for data, plus the IndexedDB read (about 4 ms) and the draw (2 to 19 ms after the last read,
medians, today). Times are first content on a return visit.

| page | loopback today | with the cache | Fast 3G today | with the cache | with the cache and `perf/asset-caching` |
|---|---|---|---|---|---|
| home `/` | 107 ms | about 60 ms | 2.3 s | about 1.8 s | about 0.6 s |
| task board `/tasks` | 401 ms | about 60 ms | 3.8 s | about 1.8 s | about 0.6 s |
| a task | 192 ms | about 85 ms | 2.9 s | about 1.8 s | about 0.6 s |
| this register | 194 ms | about 80 ms | 3.0 s | about 1.8 s | about 0.6 s |
| the consuming project's register | 296 ms | about 85 ms | 3.1 s | about 1.8 s | about 0.6 s |

- **On the phone** the cache takes out every wait for data: 0.5 to 2 s a page. What is left is
  the code's start, 1.8 s, of which the two scripts' revalidations in a row are about 1.2 s;
  `perf/asset-caching` takes those out, so the two together bring a return visit near 0.6 s, the
  HTML's own round trip.
- **On loopback** the gain is largest where the page waits longest: `/tasks`, whose cached columns
  no longer wait the 250 ms, and the large register, whose outline the server takes 204 ms to build.
- **Bytes**, warm, gzipped, if nothing changed since the last visit: `/api/board` is read fresh,
  each other kept read answers a `304` of about 300 B with its headers, and the header reads are
  not kept.

  | page | data today | with the cache |
  |---|---|---|
  | home `/` | 4.6 KB | 4.6 KB |
  | task board `/tasks` | 13.5 KB | 6.4 KB |
  | a task | 8.1 KB, 15.5 KB with its thread | 5.2 KB, 5.5 KB with its thread |
  | this register | 12.7 KB | 10.5 KB |
  | the consuming project's register | 26.7 KB | 9.9 KB |

  What is left is mostly the header: `/api/activity` is 2.9 KB, and a document reads it twice. A
  header read once per page would take this register to about 6.4 KB (inferred). A task's row
  holds only while no build runs on the machine: its page carries the machine's work, so with
  one running the page answers `200` and costs what it costs today, its thread still a `304`
  ("How freshness is checked", the machine's Build and Test work).
- **The first visit** is unchanged but for the store's code, about 1 to 2 KB gzipped (estimated),
  and writing what it read to the cache, about 1 to 3 ms a read, off the page's thread. Measured
  once built: the store is 1,433 bytes of the shell, and the code that draws from it, checks what
  it drew and asks a failed check again costs each kind more, about 14.0 KB of the board's WASM,
  6.9 KB of home's and 8.5 KB of the queue's and 13.6 KB of the task page's, so a first visit to
  `/tasks` takes 261,350 bytes and one to a task page 493,470 (the client guide's Budget).
- **A change seen while away** arrives as today: its read answers `200` with the new body, a round
  trip after the page drew the old one, and the page redraws it.

## What the build touches

The slices of `perf/client-cache/build`, refined from the verdict, in the order they land:

1. **Checksums on the server.** Every JSON read now answered by `no_store` in
   `apps/symphony-server/src/api.rs` gains the weak `ETag` and the `304`, with tests on the headers
   and on a matching read sending no body. It lands first, as the data the client reads (RULES
   `workflow/shared-data-lands-first`).
2. **The store.** The cache in the shell (`apps/symphony-ui/assets/shell.js`), so no page kind's
   wasm carries IndexedDB bindings, reached from Rust beside `platform::Platform`; the format
   number in `crates/symphony-api`; the caps, the age rule and eviction, with tests.
3. **The pages**, one kind per slice: the board, the queue and home; the task page; documents,
   their outlines and section bodies by hash. Each draws from the cache, revalidates in parallel
   and opens the feed as today.
4. **The stale rule**: the notice's wording, and the buttons that wait.
5. **Read ahead**, the owner's addition: the tasks and documents a page links to, a document in its
   first part ([Read ahead](#read-ahead)).

Tests: a warm visit draws before any response arrives (the browser test holds the server's answers
back); an unchanged read costs one `304` with no body; storage stays under the caps with eviction
exercised; a format change clears the cache; the stale notice and the waiting buttons; and the
first-visit budget test still passes.

Independent of the cache, and cheaper: the 250 ms each `/tasks` column waits before its first read
(`boardlive.tsx`, `LiveBoard.column`) is most of that page's warm time on loopback. The wait folds repeated
requests for one column into one read; a column's first read could go at once and fold the
repeats that follow it (inferred, not tried).

**Other tasks it meets.**

- `perf/feed-deltas`: once task records stop carrying agents' health, the board's column pages
  answer `304` between real edits.
- `perf/asset-caching`: makes the hashed scripts immutable. A warm visit then starts its code
  without the two revalidations in a row that it waits for today, which on the phone are most of
  the time before the code runs (see [Expected gain](#expected-gain)). The two gains add up.
- `perf/panel-first-open`: the chat panel's module and its polling; the cache does not hold the
  chat.
- `tests/first-visit-budget-counts-pings`: done on main; the `/tasks` first visit is 239,313 bytes
  under the cap the owner raised to 300,000 (the client-size-budget card). The store's code counts
  against it.

## Not chosen

- **localStorage** for the cache: one small quota for the whole origin, shared with the drafts
  kept there, and every read parsed on the page's thread
  ([Where the cache lives](#where-the-cache-lives)).
- **A service worker and Cache Storage**: unavailable on the tailnet's plain `http://` pages,
  which are not a secure context.
- **The browser's HTTP cache** with `stale-while-revalidate`: it would hand the page an old copy
  without saying it is old, and the page would not be told when the new one arrives.
- **Reopening the feed from a cached version**: the feed's contract never compares a version across
  streams, and a cached version is usually past its 128 events
  ([How freshness is checked](#how-freshness-is-checked), B).
- **Fetching tasks ahead** of being opened, in the design the card proposed, since it would fill
  storage with tasks the owner never reads. The owner asked for it on the build's thread; as
  built, what is read ahead ranks below every opened item and goes first
  ([Read ahead](#read-ahead)).

## Verified and inferred

- **Read in code:** each fact above that names a file and function, at `c9a0af8`. Main has moved
  since; the facts the design rests on were checked unchanged there (every data read `no_store`,
  the feed's 128 events, the columns' 250 ms wait, the save's Origin check). Main's page reads now
  share one board connection (`apps/symphony-server/src/live.rs`, `Hub::reader`), which may shorten
  the server's time for a task page; not measured.
- **Measured:** the tables under [Measured](#measured), the read sizes, the feed's events on the
  private server, IndexedDB and localStorage on that server's origin in headless Chrome, and a
  conditional request answered `304` to the page's own `fetch`, all on 2026-11-11, on a machine
  other agents were loading. The private server's board is a copy of this repository's; the
  consuming project's board, many times larger, was not measured.
- **Read in the web's specifications, not tried on the tailnet:** that IndexedDB and localStorage
  work on a plain `http://` page and Cache Storage and service workers do not. The private server
  answers only `localhost` and the tailnet's names (`apps/symphony-server/src/gate.rs`,
  `allows_host`), and `127.0.0.1` counts as a secure page, so no plain-`http` page was measured.
- **Inferred, not tried:** the warm times with the cache (from the measured time the code starts
  asking for data plus the measured IndexedDB read); the gain from `perf/asset-caching`; the size of
  the store's code; IndexedDB's speed on the owner's phone, which is slower than this Mac's;
  Safari's own storage rules on the phone; that a column's first read needs no wait.
- **Not built:** anything. This is a proposal; the card decides it.
