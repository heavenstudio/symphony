# Components draw when their data arrives

The requirement: on every page of the React client, a component draws as soon as the data it
needs is there, the important ones first, and redraws itself when newer data comes, so first and
later loads can happen at different times and no page waits whole on its slowest part. This says
what the client does today, the ways to get there, and which is recommended. The choice is
[its card](../../DECISIONS.md#components-draw-when-data-arrives); nothing in the client changes until
it is answered.

Every claim says how it is known: **measured** (by the run named), **read in code** (main at
`6b223845`), **read in docs** (looked up on 2026-11-15) or **inferred** (reasoned from those, not
tried). The probe is `experiments/keyed-reads/` at commit
`a40751dc2428b708045ed2ad4d6e27c15d94c314`, removed from the tree since; it reads nothing outside
that folder, and a checkout of that revision runs it:
`git worktree add --detach DIR a40751dc2428b708045ed2ad4d6e27c15d94c314`, then `npm ci` and
`npm run measure` in `DIR/experiments/keyed-reads`.

## The choice

**A: one small store of the client's own, each part a read of its own, the feed saying which part
moved.** A component names the part it draws. The store draws the device's copy first, reads the
part once whoever asks, names the copy's tag, tries a failed read again, and applies what the feed
says of it. Where the server joins parts that cost or change differently (Reports alone today), its
answer is split into a read a part.

**B: A, with TanStack Query as the store.** The same reads and the same split; the library holds
the keys, the dedupe and the retries.

**C: A, with the parts travelling on the feed's socket** in place of separate reads: a component
asks on the socket and the server pushes each part when it is ready.

**Recommended: A.** Most pages already read part by part; what they lack is one place that does
it, and A is that place and little else: 143 bytes gzipped in the probe against 10,028 for B, the
device cache and its `304`s kept as they are, no new protocol. B's extra is what this client does
not use or already has. C gives up the `304` and the device's tags for one fewer kind of request,
on a channel that today sends every event to every page.

## What exists

Read in code throughout (`apps/symphony-web/src`).

**The pieces.** `request.ts` is the transport: a read, a read naming a kept copy's tag (`getIf`,
answered `304` with no body when unchanged), a post. `cache.ts` keeps what a page drew on the
device, a named part at a time under an item (`readKept`, `keepLater`), keyed by project, and
holds `Freshness`: one value a page, listing the parts it drew from the device that the server has
not answered. `feed.ts` is the WebSocket feed, one event a message. `live.tsx` is the loop most
pages run (`keepBoard`): read `/api/board`, tell the page's views to read, then follow the feed
from the board's version until it is lost, and start again. A page is marked on its region:
`data-ready` once drawn, `data-cache=device` while it shows a copy the server has not confirmed,
`data-feed` stale or live; while either mark stands its actions wait at "Checking…".

**There is no one way a component gets its data.** Each page hand-writes the order of its reads,
its tags, its retries and its freshness: `task-live.ts` (1,006 lines), `boardlive.tsx` (1,052),
`document-live.ts` (711), `agents-live.ts` (830), and a loop of its own in `reports.tsx`,
`queue.tsx`, `runs.tsx` and `gates.tsx`.

### Page by page

| page | components and the read each needs | drawn from the device | what it waits behind |
|---|---|---|---|
| a task | head, facts, brief: `/api/task-page`; thread: `/api/task-history`; gates: `/api/task-gates`; subtasks: `/api/task-board?parent=`; landing offer: `/api/task-offer`; timeline: `/api/task-timeline` | page and thread | the page read starts after `/api/board` answered; the other five start once the page is drawn and their part nears the screen |
| `/tasks` | counts: `/api/task-board?counts=1`; the machines' work: `/api/machine-work`; each column: `/api/task-board?status=` | counts, work, each column's first page | all start after `/api/board`; each column then draws alone |
| a document | outline: `/api/document?view=outline`; each section near the screen: `/api/section`; asks: `/api/asks`; postponed: `/api/postponed` | outline and sections | sections and the feed wait for the outline's answer |
| the queue | `/api/queue` | board and queue | after `/api/board` |
| home | everything from `/api/board` | board | nothing: one read, answered from memory |
| agents | `/api/agent-page` | nothing | after `/api/board` |
| Reports | four figures and fourteen charts from `/api/reports` | nothing | the one read; no feed, read again every five minutes |
| runs | now: `/api/machine-work`; list: `/api/runs`; a run: `/api/run`, `/api/run-log` | nothing | nothing; each read again on a timer of its own |
| every page's frame | `/api/activity`, `/api/pending` | nothing | nothing |

So a task's page, `/tasks`, a document and the frame already draw part by part. Reports, the queue
and home draw whole.

### What makes parts wait

- **The page's own order, mostly.** `keepBoard` reads `/api/board` and only then lets the views
  read, so a task's page, the board's counts, the queue and the agents page all start one round
  trip late, though `/api/board` gives them nothing but the feed's version. A document's feed
  waits on its outline the same way. Three tasks in flight each undo one such order by hand:
  `app/task-page-reads-beside-board`, `app/document-feed-beside-outline-check` and
  `app/reports-drawn-from-device`.
- **The server, in one place.** `/api/reports` is the one answer that carries several components'
  data that could come apart: one pass over three kinds of day files, the roll-up (three of the
  four figures and ten charts), the agents' (one figure, three charts) and the board's health
  (one chart) (`apps/symphony-server/src/api/reports.rs`). `/api/task-page` already leaves the
  thread, the gates, the offer, the timeline and the subtasks to reads of their own, and
  `/api/board` is small and held in memory.
- **A part's own cost.** Measured on the live server, reading only, seven reads each on loopback,
  time to the first byte, medians, at a load of 165 to 174 on 16 cores:

  | read | ms |
  |---|---|
  | `/api/board`, `/api/queue`, `/api/machine-work` | 1 to 2 |
  | `/api/task-board?counts=1` | 4 |
  | `/api/task-history` | 71 |
  | `/api/task-offer` | 89 |
  | `/api/task-timeline` | 98 |
  | `/api/reports` (hours, two days) | 105 |
  | `/api/task-page` | 146 |
  | `/api/task-gates` | 252 |

  A task's reads differ a hundredfold, so which waits behind which is most of what a reader
  feels, and it differs by visit:

  - **A first visit to a task the device has not kept** orders them one after another: the board,
    the page, then the thread and the gates. The thread is answered at about 219 ms of server time
    and the gates at about 400; side by side they would be at 71 and 252, the head at 146, each
    about 150 ms sooner (inferred, the measured parts added; a browser's own time is on top).
  - **A return visit** already has them side by side. The page and the kept thread are checked
    together once the board is read (`task-live.ts`, `load`), and the gates read starts as soon as
    its part nears the screen on the page drawn from the device, behind neither (`gates.tsx`). A
    store brings the thread about 2 ms sooner there, the board's read, and the gates no sooner.
    Synced on that visit is bounded by the gates' own 252 ms, which no option here shortens:
    `perf/task-gates-kept-by-tip` does. `symphony-build synced` measured such a visit on main's
    build under load: the copy drawn at 26 to 34 ms, checked at 98 to 127, synced at 352 to 534
    (the thread of `perf/pages-time-to-synced`).

## The pattern wanted

1. **A component names the data it needs:** a part, by its key, its read and how to read it.
2. **It draws as soon as that part is there:** from the device first, marked as possibly out of
   date, as [the client cache](../../engineering/analysis/client-cache.md#a-stale-draw) decided; then from the server.
3. **It redraws itself when newer data arrives:** the feed says the part moved, with its new value
   when it is small, and nothing else on the page is read again.
4. **The important first:** no part waits for another's read unless it needs its value; what is on
   the screen is asked before what is not.
5. **First and later loads at different times:** each part has its own tag and its own moment to
   be read again.

## The options, against Reports and a task's page

### a. The client's own, extended: a keyed store

A store over `cache.ts` and `request.ts`. A part is a key (its item and part name on the device),
a URL and a reader. `usePart(part)` gives a component `{ value, stale, error }` through React's
`useSyncExternalStore`. The store, once for every page:

- draws the device's copy at once, marked stale, and reads with its tag;
- makes one read a key, however many components name it;
- tries a failed read again on `state.ts`'s backoff until it answers;
- takes the feed: a change that carries the part's value is drawn, one that only names it is read
  again;
- holds the page's `Freshness`, which becomes what it can compute: the parts on the page still
  unanswered. The region's marks and "Checking…" follow from it as today.

Every read's answer also names the feed's version it was read at, in the header a document's read
already carries (`VERSION_HEADER`), so the feed can open at once, from any read, and the store
drops what the feed says of a part at or before its answer and applies what comes after. This is
the part that needs the most care, and the reason for doing it once: `task-live.ts` and
`document-live.ts` each do it by hand today (inferred design, not built).

- **A task's page:** six parts. Each starts at mount or as it nears the screen, behind nothing:
  the thread, the gates and the timeline need only the slug. The page's loop keeps what is its own
  (merging a feed event into a task, the thread's paging).
- **Reports:** one part, so (a) alone changes nothing a reader sees beyond the device's copy. Its
  charts come apart only with a split on the server ([c](#c-the-server-sending-parts-as-they-are-ready)).
- **Cost.** Code: the probe's store is 114 lines with its two sample components; the real one,
  with freshness, versions and the near-the-screen start, perhaps 300 and its tests (inferred),
  against the hand-written order it takes out of five modules. Bytes: the probe's store is 143
  gzipped beyond a page read whole (measured, below); the real one's are not measured, and about
  nothing beside the order it replaces (inferred). Server: the version header on every read; and,
  for a part to refresh when its data changes, a feed event when the server writes a report's day
  file or a run, with Reports and runs opening the feed they do not open today. Checks: none
  change.

### b. A library for the client side

Measured: each page of the probe built alone with the web client's Vite settings, gzipped at the
best level, bytes beyond the same page read whole (`experiments/keyed-reads` at commit
`a40751dc2428b708045ed2ad4d6e27c15d94c314`, `npm run measure`, run as the top of this page says):

| way | minified | gzip |
|---|---|---|
| a keyed store of the client's own | 468 | 143 |
| React's `use()` under Suspense | 136 | 2 |
| SWR 2.5.1 | 13,147 | 5,402 |
| TanStack Query 5.107.1, the device cache kept beside it | 33,555 | 10,028 |
| TanStack Query with its own persistence | 36,886 | 10,980 |

A first visit has 17,658 bytes of room on a task's page (142,342 of its 160,000 cap), 28,948 on
`/tasks` and 12,255 on home (`apps/symphony-ui/tests/browser/main.rs`, sizes in
`apps/symphony-ui/AGENTS.md`). TanStack Query would take 57 % of a task page's room, SWR 31 %.

What each gives that (a) lacks, read in docs:

- **TanStack Query** ([its defaults](https://tanstack.com/query/latest/docs/framework/react/guides/important-defaults)):
  dedupe, three retries with exponential backoff, cached data taken as stale and read again when a
  component mounts, the window is focused or the network returns, unused entries dropped after
  five minutes, results shared structurally, paging helpers and developer tools. Of these the
  client lacks only the last three, and the store in (a) has the first two. Its persistence
  writes the whole query cache as one value and restores it before any query runs: no caps an
  item, no least-recently-used, no kept entrypoints, no format number, and a restored query has no
  tag to name, so its check downloads the answer whole. Used with the device cache kept beside it
  (the probe's `tanstack` page), it is a second cache in memory over the first, and the stale mark
  is its `isPlaceholderData`.
- **SWR** ([its options](https://swr.vercel.app/docs/api)): dedupe within two seconds, a retry
  every five seconds with backoff, a read again on focus, fallback data. No persistence of its
  own. Its dedupe and focus throttle are paced by a number.
- **React's `use()` with Suspense** ([its reference](https://react.dev/reference/react/use)): a
  boundary a component whose fallback is its skeleton, and a transition that keeps the old content
  while a fresh read is out. It weighs nothing and gives no cache: a promise must be kept outside
  the component or the fallback shows at every render, so the store in (a) is still written, and
  a failed part needs an error boundary of its own. It can be laid over (a) later; nothing here
  needs it.

Against the two pages a library changes what (a) changes and no more: neither splits Reports'
answer, and neither knows the feed, the device's tags or the page's freshness, which stay the
client's own code around it.

### c. The server sending parts as they are ready

- **A read a part.** What a task's page and `/tasks` already do. Each part keeps its own tag, its
  own `304` and its own copy on the device. The cost is requests: a browser keeps six HTTP/1.1
  connections to the server for every tab of every project, which the feed no longer takes one
  of. **Reports:** `/api/reports` gains `source=rollup|agents|health`, three reads for one, each
  kept by window on the server as the whole is now. Three of the figures and ten charts draw when
  the roll-up answers. The agents' day files are 83 % of what the answer reads (1,043 kB of 1,262
  on this board, measured) and the only source that changes within the hour, so the roll-up's
  charts stop being read again with them. Under load the whole answer already takes about 105 ms
  and a kept window less, so the gain is charts that fail, change and refresh apart more than
  time.
- **One streamed answer** (newline-delimited JSON). One request, parts in the order the server
  finishes them. But one response has one validator, so an unchanged part is sent again whenever
  another moved, unless the request names a tag a part, which is a protocol of the client's own;
  `request.ts` reads a body whole and would need a reader of lines; and the handler must be
  rewritten to yield. It earns its cost where one computation gives its parts in turn and they
  cannot be asked for apart. Neither page is that: Reports' sources are separate files, a task's
  parts separate handlers.
- **Parts on the feed's socket.** No HTTP request at all. But the feed sends every event to every
  page of the project (only a document's sections are filtered, `apps/symphony-server/src/api.rs`),
  so a pushed part needs subscriptions; its events are versioned and kept for resuming, which an
  answer to one page's question is not; a page's reads would stop while its feed reconnects, where
  today they go on; Reports and runs open no feed; and the tag, the `304` and the count of reads
  `symphony-build synced` keeps would each be built again over it.

The feed is right for what it does now: saying which part moved, with the value when it is small.
That is the data channel in its cheapest form, and (a) uses it as it is.

### d. GraphQL with `@defer` and `@stream`

Only what this requirement changes in [the earlier reasoning](../../engineering/analysis/graphql-and-server-framework.md#graphql-or-not),
which stands ([its card](../../DECISIONS.md#graphql)):

- **New: the server has a WebSocket.** The feed moved to one, so subscriptions would no longer
  bring it. The feed's rules would still be rebuilt on top.
- **New: incremental delivery is what `@defer` is for.** One query, its deferred parts sent as
  they are ready. It is still outside the released specification: an RFC in draft, experimental in
  GraphQL.js 17, where incremental delivery is "pending GraphQL specification work"
  ([the RFC](https://rfcs.graphql.org/rfcs/1110/), [GraphQL.js](https://graphql-js.org/docs/defer-stream)).
  Neither Rust library serves it: async-graphql removed both directives after its 1.x line, and
  its maintainer wrote that they return "after these directives have become standards"
  ([async-graphql #403](https://github.com/async-graphql/async-graphql/issues/403), open); Juniper's
  request has been open since 2020 with no pull request
  ([Juniper #734](https://github.com/graphql-rust/juniper/issues/734)).
- **Unchanged:** it is the streamed answer of (c) with a schema: one response, no `304` a part.
  The bytes (10 to 58 kB), the plugins' blocks crossing untyped and the second set of types are as
  the card weighed them.

Nothing here reopens it.

### Which pairs make sense

| | a read a part | one streamed answer | parts on the feed |
|---|---|---|---|
| **the client's own store** | recommended: tags, `304`s and the device cache as they are | only for an answer that cannot be split; none today | a second protocol for what a read does |
| **TanStack Query or SWR** | works; 5 to 10 kB for what the store does in 143 bytes | the library has no part in it | the library has no part in it |
| **`use()` with Suspense** | over the store, later, if boundaries are wanted | | |

## How it meets the standing rules

- **No wait by a number** (`tools/no-timed-waits`). A component waits for its part's event: the
  store's change. The only number is the retry's backoff, through the one helper the rule allows.
  Reports' five-minute read and the runs page's timers become a feed event naming the part that
  moved; the server knows when it writes a day file or a run.
- **Synced, as the client defines it.** The region's marks are computed from the store, so
  `symphony-build synced` reads them as today. It gains one figure: the store marks
  (`performance.mark`) each part's first draw from the server, a page names the part that matters
  most, and the tool prints that part's time beside drawn, checked and synced.
- **One feed per origin.** A page holds its one socket and nothing more: parts come by reads that
  end, and the store opens no stream of its own.
- **The device cache keyed by project.** The store keeps parts through `cache.ts`: the same items,
  caps, format number and read-ahead. A new part is a name under an item.
- **Skeletons that reserve layout.** Each part's placeholder is its own box at its kept size, so
  parts arriving in any order fill their boxes and move nothing
  ([its card](../../DECISIONS.md#skeleton-loading-outline)).
- **Plugin blocks drawn by one component each.** Unchanged: a block is drawn from what its part
  brought and reads nothing itself.
- **Actions wait on what they act on.** While any part on the page shows the device's copy, the
  page's actions wait, as [the client cache](../../DECISIONS.md#client-cache) decided.

## Recommendation

A, in this order, each step a task with `symphony-build synced` run before and after on the same
scratch board under the day's load, posting the first important part's time and synced:

1. **The store and a task's page**, the page with the most parts: the six read side by side, the
   version header on every read. The gain is a first visit's, about 150 ms for the thread and the
   gates under load (inferred); a return visit, whose reads are side by side already, gains about
   nothing in time. It takes in what `app/task-page-reads-beside-board` did by hand.
2. **`live.tsx`'s loop**: the feed opened beside the board's read, so `/tasks`, the queue and the
   agents page stop starting one round trip late.
3. **Reports**: the answer split by source, three parts, and a feed event in place of the timer.
4. **A document**: the outline and its sections as parts.
5. **Runs**: its three timers replaced by feed events.

Home stays one read. No library, no streamed answer and no part on the feed's socket until a page
has data that cannot come by a read a part.

## Measured, read and inferred

- **Measured here:** the probe's bytes; the live server's read times, which are an earlier build's
  than main's and one board's, at one load; the day files' sizes.
- **Measured by others:** a task page's drawn, checked and synced on main's build
  (`perf/pages-time-to-synced`).
- **Read in code:** what each page reads and behind what; in docs, the libraries' behaviour and
  the state of `@defer`.
- **Inferred, not tried:** the store's real size; a task page's first-visit times with its reads
  side by side;
  the version header letting the feed open from any read; what the Reports split gains under
  load. Step 1 measures the second and third before any other page moves.
