# GraphQL, its library and the server's framework

With the web client moving to React, TypeScript and Vite ([its card](client-framework-comparison.md#client-framework)),
three questions about what the port's data layer and the server are built on: whether the client
should read the server through GraphQL, with Apollo or another client cache, in place of today's
JSON reads and live feed; if so, which Rust GraphQL library; and whether the server should stay
on Axum or move to Actix Web, Rocket, Warp, bare Hyper or another framework. Each is a card of its
own in [DECISIONS](../../DECISIONS.md), so each can be answered alone.

Every claim says how it is known: **measured** (on this machine, by the runs named), **read in
code** (the file named) or **read in docs** (the source linked, looked up on 2026-11-14), or
**inferred** (reasoned from those, not tried). The probes are in `experiments/graphql-stack/` at
commit `a40751dc2428b708045ed2ad4d6e27c15d94c314`, removed from the tree since; nothing the
Symphony built, tested or served read them, and [the probe](#the-probe) says how to run them
from that revision.

## The choices

### GraphQL or not

**A: keep the JSON reads, the feed and the hand-written device cache.** The React port goes on as
its foundation built it: reads, feed and cache are three small modules of `apps/symphony-web`,
the wire's types generated from `crates/symphony-api`. **B: GraphQL**, a schema served beside or
in place of the reads, with a GraphQL client (Apollo, or urql) and its cache in the browser.

**Recommended: A.** The reasons, each from the sections below:

- **What GraphQL saves, this client mostly has.** Its one real saving is round trips that wait on
  each other, and those are the client's own ordering, not REST's: a task's page waits for
  `/api/board` before it asks for the task because it opens the feed from the board's version,
  and the server already names that version in a header on a document's read, so a task's read
  could start at once too. On this machine a round trip costs nothing; on a phone over a slow
  network, about 0.6 s a step ([Round trips](#requests-and-round-trips)).
- **The bytes go the wrong way.** A page's own data is 5 to 23 kB before it is drawn and about
  0.3 kB a read on a return visit, when nothing changed. A GraphQL client adds 10 kB (urql) to
  58 kB (Apollo) and 68 kB (Relay) to every first visit, against the queue's whole first visit of
  101 kB on React, which is more than any field it leaves out could save ([Bytes](#bytes-on-the-wire)).
- **The cache it brings is weaker here than the one built.** The device cache keeps each read
  across visits in IndexedDB, keyed by project, and asks again with its checksum so an unchanged
  read answers `304` with no body. Apollo's cache lives in the page's memory, keeps nothing
  across visits without a further package last published in March 2024, and a GraphQL query sent
  by `POST` is never answered `304` ([Caching](#caching)).
- **"Faster responses" would not come from it.** The server answers most reads in 1 to 2 ms and
  its slowest (a task's page, a large register's outline) in 20 to 200 ms, all of it the board,
  the store and the Markdown behind them; a GraphQL layer runs the same work and adds about
  0.06 ms a query ([Where faster would come from](#where-faster-responses-would-come-from)).
- **Its typing stops where the plugins start.** The blocks a plugin renders are a tree of any
  depth, and a GraphQL query cannot ask for a tree of any depth; the plugins' board fields are
  maps keyed by names a project picks. Both would cross a schema as untyped JSON
  ([Plugins](#plugins-answers-in-a-schema)).

**Its weakest point.** A normalized cache (one task kept once, every page that shows it updated
from that one record) is what this client would gain and does not have; the live feed covers most
of it today, as each page applies the changes it is sent, but a client that grows many views of
the same tasks would feel its lack. TanStack Query is the smaller way to that (10 kB, the REST
reads unchanged), if it is ever wanted.

- [r] **graphql** — Should the React client read the server through GraphQL, with Apollo or another GraphQL client and its cache, instead of today's JSON reads and live feed? The hope is better caching in the browser and faster responses; measured here, the caching is mostly built already, and the speed would come from elsewhere. A: keep the JSON reads, the live feed and the device cache the React port is built on. B: GraphQL: a schema on the server, a GraphQL client and its cache in the browser, live updates as subscriptions. Recommended: A. Approve and say A or B in the note, or comment. [[analysis/graphql-and-server-framework#GraphQL or not]]
  [2026-11-14 11:01] agent — Posted for the board task `inbox/evaluate-graphql-and-other-web-stacks`.

    **What a page reads today,** measured on the live board: the board 12 reads and 23 kB of data before it is drawn, a task's page 4 reads and 10 kB, this register 6 reads and 17 kB. A return visit makes 3 to 5 reads of about 5 kB, every unchanged read answered empty (`304`). The server answers most reads in 1 to 2 ms and its slowest in 20 to 200 ms, all of it the work behind them.

    **A, keep them.** Nothing changes in the port. Two small changes would give what one GraphQL query saves, if wanted later: a task's page asking for the task and the board together (the server already does this for documents), and the board asking for its counts and columns with the board. Over a slow phone network each saves about 0.6 s; on this machine, nothing.

    **B, GraphQL.** One query a page in place of several reads. Its costs:
    - 10 kB (urql) to 58 kB (Apollo) more on every first visit, against the queue's whole 101 kB on React.
    - Apollo's cache lives in the page's memory and keeps nothing across visits without an add-on last released in March 2024. A query sent by POST never gets the empty `304` that makes return visits cheap today.
    - Subscriptions need a WebSocket the server does not have, with the feed's rules rebuilt on top.
    - A second set of generated types beside today's.
    - The plugins' blocks and fields cross as untyped JSON, since a query cannot ask for a tree of any depth.
    - The port's reads, feed and cache modules are replaced before its nine page tasks start.

    **Why A.** Where GraphQL saves time, the same saving is open without it. Weighed in bytes, it costs more than it saves. The cache it brings is weaker here than the one built. The server's time would not change: about 0.06 ms more a query.

    **The weakest point.** A normalized cache (one task kept once, every view of it updated together) is the one thing the client would gain. The live feed covers most of it today; if it is ever needed, TanStack Query gives it over the reads as they are, for 10 kB.

    **Earlier verdicts checked.** **client-framework** (React, TypeScript, Vite) moved the front end and named no data layer, so it does not decide this. **client-cache** (the device cache, checked by checksum) would be partly replaced under B; that would change an approved design, so it is asked here, not assumed.
  [2026-11-14 11:17] owner · approved — A let's keep it simple for now. Agree this seems to be solving a problem we don't have (yet?)
  [2026-11-15 11:13] agent — Recorded: A. The React client keeps the JSON reads, the live feed and the device cache, with no GraphQL. Nothing changes: the React client is being built on them as they are.
  [2026-11-15 11:14] agent · resolved

### Which GraphQL library, if GraphQL is taken

**A: Juniper. B: async-graphql.** These are the two maintained Rust GraphQL servers; both serve
a query on Axum 0.8 in the probe, speak subscriptions over WebSocket, and derive a schema from
Rust types with derives of their own, so the wire types would each take a second derive in
`symphony-api`, behind a feature as the TypeScript one is.

**Recommended: A, Juniper, by a narrow margin.** The reasons ([The library](#the-rust-graphql-library)):

- **It is the lighter.** Measured in a minimal Axum server against a plain JSON one, compiled
  from nothing for release: Juniper 54 s more CPU and 0.8 MiB more binary, async-graphql 158 s
  and 1.5 MiB. The server's size budget has about 0.6 MiB of room, so either needs its line
  raised, Juniper by less.
- **It is the one released lately.** Juniper 0.18.0 and its Axum integration came out on
  2026-10-30, and its last commit is from 2026-11-09. async-graphql's newest stable, 7.2.1, is
  from 2026-01-20; nothing has been pushed to it since 2026-04-21, and its 8.0 has been a release
  candidate for ten months. It is not abandoned: asked in November 2025 whether it was still
  maintained ([#1762](https://github.com/async-graphql/async-graphql/issues/1762)), it took on a
  second maintainer, who answered in December 2025 and made most of its commits since; but that
  maintainer's work stops in April too.
- **Speed is level:** 74 µs a query on Juniper, 62 µs on async-graphql, for the board and 30
  tasks; both nothing beside a read's own work.

With the maintenance question answered by a maintainer, the release record leans to Juniper by
less than a stall would suggest: seven quiet months against a release two weeks old. What
decides it is mostly the measured weight, compile time and binary, which grows with each of the
wire's 158 types derived (inferred).

**Its weakest point.** async-graphql has about thirteen times the users (4.4 million downloads in
90 days against 0.35 million), two maintainers, and more conveniences: it takes any serde type
whole as a JSON value (`Json<T>`), which the blocks and the plugins' fields would need, where
Juniper needs a scalar of the Symphony's own for it. Juniper is a 0.x line whose minors break
(0.17.2 and 0.18.0 two days apart), with seven people committing in the year.

- [~] **graphql-library** — If the client reads the server through GraphQL (**graphql**), which Rust library serves it? It matters only if that card is B. A: Juniper. B: async-graphql. Recommended: A, by a narrow margin. Approve and say A or B in the note, or comment. [[analysis/graphql-and-server-framework#Which GraphQL library, if GraphQL is taken]]
  [2026-11-14 11:01] agent — Posted for the board task `inbox/evaluate-graphql-and-other-web-stacks`.

    **Both work.** Each served a query on Axum in a probe and speaks subscriptions over WebSocket. Each needs a derive of its own on every wire type, as neither reads serde's.

    **A, Juniper.** Released on 2026-10-30 with its Axum integration; last commit 2026-11-09. The lighter: compiled from nothing it took 54 s more CPU than a plain JSON server and 0.8 MiB more binary. Its costs: about a thirteenth of the users; no JSON value built in, so the blocks and the plugins' fields need a small scalar of the Symphony's own; a 0.x line whose minor versions break.

    **B, async-graphql.** About 13 times the users, two maintainers, and it takes any serde type whole as JSON. It is maintained: asked in November 2025 whether it still was, it took on a second maintainer, who answered in December and made most of its commits since. Its costs: its newest stable is from 2026-01-20, nothing has been pushed since 2026-04-21, and its 8.0 has been a release candidate for ten months. It took 158 s more CPU and 1.5 MiB more binary.

    **Why A, by a narrow margin.** Mostly the measured weight: it is the lighter to compile and to ship, a gap that grows with each of the wire's 158 types. Its recent release counts for less than it would against an abandoned library: seven quiet months, not a stall. Speed is level: 74 µs a query against 62.

    **The size budget.** The server has about 0.6 MiB of room under its line, so either needs the line raised, A by less.

    **Earlier verdicts checked.** No card speaks of a GraphQL library.
  [2026-11-14 11:18] owner · dismissed

### Which server framework

**A: keep Axum. B: move,** Actix Web being the strongest alternative.

**Recommended: A.** The reasons ([The framework](#the-server-framework)):

- **Nothing about Axum is failing this server.** No task, bug or incident on the board names it,
  and it is 0.03 MiB of the server's binary.
- **The server uses it thinly,** as an HTTP layer under a dispatch of its own: one Axum route
  and a fallback. It leans on the parts that are hardest to move: its own listener under
  `axum::serve`, which the reload's socket hand-over and the drain need, and `tower` and
  `tower-http` for the projects' dispatch and the compression.
- **A move buys only speed, which is not this server's bottleneck.** Every alternative that is
  maintained offers what the server needs (Rocket cannot yet take the reload's sockets). At this
  load, one machine and a handful of browsers, the framework's share of a read is microseconds,
  and the read's own work is milliseconds.

**Its weakest point.** Actix Web has scored above Axum in published benchmarks, and is released
more often. At this load that buys nothing measurable, and the move would touch 14 of the
server's source files and 25 of its test files. Staying costs one change when Axum's next
version comes: it changes `axum::serve`, which the server calls in one place, and applies a
30-second limit on reading a request's head that the server's listener does not have today
([Is anything about Axum failing](#is-anything-about-axum-failing)).

- [r] **server-framework** — Should the server stay on Axum, or move to another Rust web framework (Actix Web, Rocket, Warp, bare Hyper, Poem, Salvo)? Nothing you see changes; this decides what the server is built on. A: keep Axum. B: move to Actix Web, the one serious alternative. Recommended: A. Approve and say A or B in the note, or comment. [[analysis/graphql-and-server-framework#Which server framework]]
  [2026-11-14 11:01] agent — Posted for the board task `inbox/evaluate-graphql-and-other-web-stacks`.

    **Is Axum failing?** No. No task or bug names it, and it is 0.03 MiB of the server's binary. The server uses it thinly: one route and a fallback, with its own dispatch picking among 30 reads and 15 saves.

    **The others.** Rocket cannot take the listening sockets the reload hands a new build. Poem has had no release in 15 months. Warp moves slowly, under one person's account. Salvo and ntex each rest on one person. Bare Hyper is what Axum already runs on. Actix Web is maintained, released often, and offers everything the server needs.

    **Speed.** Actix Web has scored above Axum in published benchmarks; Axum's last published JSON figure is 2.7 million requests a second, about 21 µs of a core each. This server's reads take 1 to 200 ms of their own work, for a handful of browsers, so a faster framework would save microseconds no page could show.

    **B's cost.** 14 of the server's files and 25 of its test files: its own listener, which the reload's hand-over and drain rely on; its layers; the projects' dispatch; the streams.

    **A's cost.** Axum's next version changes `axum::serve`, which the server calls in one file: a small compile fix, plus a 30-second limit on reading a request's head that it applies, with no setting to turn it off. That would close a connection that sends nothing for 30 s, an idle browser connection among them, where today it is kept, so the server's drain on reload needs checking when it comes.

    **Why A.** Nothing to fix, nothing to win at this load, and the move would rewrite the parts that are hardest to test.

    **Earlier verdicts checked.** No card speaks of the server's framework; **client-framework** left the server as it is.
  [2026-11-14 11:19] owner · approved — A
  [2026-11-15 11:13] agent — Recorded: A, the server stays on Axum. Nothing changes, and no document said otherwise.
  [2026-11-15 11:14] agent · resolved

### What it changes in the port

Under A on the first card, nothing: the foundation (`app/client-moves-to-react/foundation-and-first-page`,
in review at `a8b379be`) stands as built, and its nine port tasks extend its reads, feed and
cache. Under B, the port's data layer is replaced before those tasks start
([What GraphQL would replace](#what-graphql-would-replace-in-the-react-client)), and the server
gains a schema, a route and a WebSocket. The other two cards change nothing in the port.

## What a page reads today

Measured on 2026-11-14 on the live board (this repository's, served by the machine's server and
its Dioxus client), each page opened once in a fresh headless Chrome by `symphony shot`, then
opened again in the same browser for a return visit; the browser's Resource Timing entries read
by `--eval` 1.5 s and 3 s after each load. One run of each on a machine at a one-minute load of 100
to 150, so the times are indicative only; the counts and bytes are not the load's, but they are a
live board's, which changed while it was read. Bytes are on the wire, gzipped, headers included.

| page | before it is drawn | in its first 1.5 s | return visit, before it is drawn |
|---|---|---|---|
| the board `/tasks` | 12 reads, 22.9 kB, drawn at 494 ms | 44 reads, 62.5 kB, 14 of them the 7 linked tasks read ahead (30.6 kB) | 5 reads, 5.5 kB, drawn at 115 ms; every column `304` |
| a task's page (`app/client-moves-to-react/foundation-and-first-page`) | 4 reads, 9.7 kB; its task read answered at 219 ms | 6 reads, 12.7 kB | 4 reads, 9.7 kB; the task read answered at 82 ms |
| this register `docs/DECISIONS.md` | 6 reads, 17.1 kB, drawn at 674 ms | 11 reads, 30.1 kB | 4 reads, 5.0 kB, drawn at 61 ms; the outline `304` |
| home `/` | 3 reads, 4.9 kB, drawn at 88 ms | 5 reads, 11.8 kB | 3 reads, 4.9 kB, drawn at 23 ms |

Beside the data, the same cold visits fetched 1.0 MB (the board) to 1.5 MB (the task's page) of
Dioxus code on the wire, the modules warmed at idle among them: the data is 1 % to 6 % of a cold
visit today. The React port changes that share: the queue's first visit on React is 101,350
bytes against 255,310 on Dioxus, of which its script, style sheets and shell are 98,059 and the
page and its data the rest (measured by the port's budget tests at `a8b379be`, read on its
thread).

**The chains.** What waits on what, from the same runs (read in the timings, and in the client's
code, `apps/symphony-ui/src/live.rs`, `keep_live`; `board_live.rs`, `column`, now the React
client's `apps/symphony-web/src/boardlive.tsx`, `LiveBoard.column`):

- **The board:** `/api/board` (78 to 80 ms), then the header's `/api/activity` and
  `/api/pending`, then the counts and the machine's work (170 to 177 ms), then each column near
  the screen, after the 250 ms the client waits before a column's first read.
- **A task's page:** `/api/board` (97 to 99 ms), and only then `/api/task-page` (159 to 219 ms),
  then the gates (252 to 375 ms); the timeline's read comes a second later, once its module is in.
  The page waits for the board because it opens the feed from the board's version, so that the
  feed replays whatever changes between the reads.
- **A document:** the outline first, then the sections near the screen, then the feed, which
  opens from the version the outline's read names in a header (`symphony_api::VERSION_HEADER`,
  set in `apps/symphony-server/src/api.rs` for a document, its sections and a folder alone), so a
  document reads no board.

**What a read costs the server.** Measured on 2026-11-11 by curl on loopback, five reads each
([the client cache's analysis](client-cache.md#measured)): `/api/board`, the counts, a column,
the queue, the header's two reads 1 to 2 ms; a task's page 13 to 31 ms; its newest messages 12 to
14 ms; this register's outline 27 to 67 ms and a 128-card register's 152 to 317 ms. In today's
runs, at a higher load, a task's page took 52 to 60 ms and its gates 70 to 123 ms. That time is
the work behind each read: the board's snapshot and its store, the git history, the Markdown
rendered (read in code, `apps/symphony-server/src/api.rs`).

**What a read carries.** A column's read is 15.7 kB raw and 3.9 kB gzipped for 30 tasks; its
largest parts are the titles (24 % of the raw bytes) and the agents on each task (22 %); a field
the board's card may not draw, such as `created_by`, `created`, `updated` and `depends`, is about
5 % each (measured on the live read; which fields the card draws was not traced). Repeated keys
compress well, so leaving fields out saves less gzipped than raw: at most about 1 kB of a
column's 3.9 (inferred).

## GraphQL for this client

### Requests and round trips

One GraphQL query can ask for what a page needs at once: the board's version, the counts and a
column, or a task with its gates and its newest messages. That would turn the board's 12 reads
before it is drawn into one or two, and a task page's three steps into one (inferred).

What that is worth depends on the network. On loopback, as above, a step is a millisecond or two
and the waits are the client's own (the column's 250 ms, a module's load). At Chrome's Fast 3G
profile, measured for the client cache on 2026-11-11, each step that waits on another cost about
0.58 s: a task page's board, then task, 1.2 s; the board's board, counts, wait and columns, 2 s.
So one query would save about 0.6 s on a task page and about 1.2 s on the board over a slow
network (inferred from those steps), and nothing measurable on this machine. The phone on the
tailnet sits between the two; its round trip was not measured.

The same saving is open without GraphQL:

- A task's read can carry the feed's version in a header, as a document's already does, so the
  task page asks for the task and the board together: one step fewer on every task page (read in
  code: `apps/symphony-server/src/api/validated.rs`, `without_cursor`).
- The board's counts and its first columns can be asked for with `/api/board`, not after it; the
  250 ms wait before a column's first read is the client's choice
  (`apps/symphony-web/src/boardlive.tsx`, `LiveBoard.column`).

Either is a change to two or three reads and the pages that make them, not a new layer (inferred;
not built). The number of parallel reads matters a little more here than elsewhere: the server
speaks HTTP/1.1 alone (axum's `http1` feature, `apps/symphony-server/Cargo.toml`), so a browser
holds six connections to it, the feed takes one per page, and the board's seven column reads
queue behind the rest (read in code: `apps/symphony-web/src/feed.ts` at `a8b379be` says so of the
feed). In today's run the seven columns all answered within 6 ms of each other, so the queue cost
nothing measurable on loopback.

### Bytes on the wire

What each data library adds to a React 19 page's first visit, measured by the probe
(`experiments/graphql-stack/web` at commit `a40751dc2428b708045ed2ad4d6e27c15d94c314`,
[the probe](#the-probe)): one page per library, each drawing the board's counts and a
column from one read, built alone by Vite 8.3.8 with the web client's settings, its script
gzipped at the best level as the server sends it. The baseline is React and a hand-written
`fetch`, as the foundation reads today: 67,720 bytes.

| library | version | what it is | added, gzip | added, Brotli |
|---|---|---|---:|---:|
| TanStack Query | 5.107.1 | a cache keyed by read, over the REST reads as they are; no GraphQL | 10,021 | 8,704 |
| urql | 5.0.4 (`@urql/core` 6.0.3) | GraphQL, a cache of answers by query and variables | 10,367 | 9,301 |
| graphql-request | 7.4.0 (with `graphql` 16.15.0) | GraphQL, no cache | 13,424 | 11,746 |
| urql with Graphcache | 9.0.2 | GraphQL, a normalized cache | 17,565 | 15,812 |
| Apollo Client | 4.3.8 (with `graphql` 16.15.0, `rxjs` 7.8.2) | GraphQL, a normalized cache | 58,313 | 51,176 |
| Apollo with subscriptions | the same, with `graphql-ws` 6.4.2 | the live feed as GraphQL subscriptions over a WebSocket | 62,120 | 54,452 |
| Relay | 21.0.1 (`react-relay`, `relay-runtime`) | GraphQL, a normalized store; its compiler's artifact written by hand | 68,349 | 56,036 |

The pages were built, not run, and each library's bytes are its code with the page's few lines
of setup; the GraphQL pages also carry their query's text, a few hundred bytes. The versions are
each package's newest on npm on 2026-11-14 (`npm view`); `graphql` 16.15.0, the newest 16, is
what graphql-request 7.4.0 accepts (its peer range is 14 to 16) and Apollo accepts 16 or 17.

**Against the caps.** The first visits of `/tasks` and `/` are capped at 300,000 bytes, and a cap
only ever goes down ([the client guide](../../apps/symphony-ui/AGENTS.md)). The queue on React is
101,350 bytes; with urql it would be about 111,700, with Apollo about 159,700, with Relay about
169,700 (inferred: the measured parts added). The board on React measured 103.8 kB on the
comparison's larger fixture, 10 to 30 kB more with what that page left out
([its Size](client-framework-comparison.md#size)), so about 114 to 134 kB, and about 172 to
192 kB with Apollo (inferred). Every option stays under the caps; the cost is the share, not the
cap: Apollo adds more than half again to a React page's first visit, and more than any page's
whole data before it is drawn (5 to 23 kB, above).

### Caching

**Today** (read in code: `apps/symphony-web/src/cache.ts` and `request.ts` at `a8b379be`,
`apps/symphony-ui/assets/cache.js`, [the client cache](client-cache.md#the-design)). The device
cache keeps what each page drew in one IndexedDB store the shell owns (`symphonyCache`), which
every page kind of both clients shares while the port runs. Every key starts with the project's
id; a format number in the wire crate clears it; caps of 8 MB a project and 32 MB in all, 14 days,
least recently used out first, a project's entrypoint pages kept; linked tasks and documents read
ahead at idle. A return visit draws from it at once, marked as possibly out of date, then asks
each read again with its checksum (`If-None-Match`); an unchanged read answers `304` with no
body. Measured above: a return visit's columns all `304`, its outline `304`.

**Apollo Client** "stores the results of your GraphQL queries in a local, normalized, in-memory
cache": each object once by its key, every query that names it answered from that record, with
fetch policies such as cache-and-network ([its docs](https://www.apollographql.com/docs/react/caching/overview)).
Across visits it keeps nothing unless `apollo3-cache-persist` is added: a separate package
written for Apollo Client 3, whose newest version, 0.15.0, was published on 2024-03-26 (`npm
view`); it writes the cache on every change, up to 1 MB by default, and past that "persistence
will pause and app will start up cold on next launch"
([its docs](https://github.com/apollographql/apollo-cache-persist/blob/master/docs/advanced-usage.md)).
It does not revalidate a kept answer by its checksum: a query goes by `POST` unless its link is
told `useGETForQueries` ([its docs](https://www.apollographql.com/docs/react/api/link/apollo-link-http)),
and a `304` answers only a `GET` or a `HEAD`
([RFC 9110, 15.4.5](https://www.rfc-editor.org/rfc/rfc9110#section-15.4.5)), so every refetch
downloads its answer whole (inferred, from those). The board, a task's page and a document would
each come back in full where today they come back empty.

**urql** keeps, by default, each answer under its query and variables, "indefinitely", dropped
when a mutation returns a type it holds
([its docs](https://github.com/urql-graphql/urql/blob/main/docs/basics/document-caching.md)).
**Graphcache**, its normalized cache, can keep its records on the device: its default storage
"uses IndexedDB to persist the cache's data", through its offline exchange
([its docs](https://github.com/urql-graphql/urql/blob/main/docs/graphcache/offline.md)).
**Relay** keeps a normalized store in memory, needs its compiler for every query, and for its
refetching and paging expects the server's schema to give every object an id it can be fetched
by (a `Node` interface) and paged lists in its connection shape
([its server spec](https://relay.dev/docs/guides/graphql-server-specification/)); the Symphony's
schema would have to be shaped for it. **TanStack Query**, the non-GraphQL option the
comparison's probe already drove on React (reads, Load more, a feed event's invalidation:
[its probe](client-framework-comparison.md#the-probe)), keeps each read under a key the page
names, in memory, and on the device through a persister, IndexedDB among them, for 24 hours by
default ([its docs](https://tanstack.com/query/latest/docs/framework/react/plugins/persistQueryClient));
its `queryFn` is the page's own code, so it would keep today's `If-None-Match` and `304` as they
are (inferred).

**What a client cache would add here.** Normalization is the one thing the device cache does not
do: a task drawn on the board, on its own page and in the panel is three copies, each read and
kept apart. Today the feed sends each page the regions that changed with their new data, and a
page applies them in place, so a copy is stale only between pages, and a return visit checks
every copy it draws (read in code: `crates/symphony-api/src/lib.rs`, the feed's contract). With
the two clients sharing the shell's store during the port, a GraphQL cache would also be a second
cache beside it, which the Dioxus kinds could not read, until the last of them leaves (inferred).

### Live updates

**Today** (read in code: `apps/symphony-server/src/api.rs`, `feed`; `crates/symphony-api/src/lib.rs`;
`apps/symphony-web/src/feed.ts` at `a8b379be`). One stream of Server-Sent Events a page,
`/api/feed?since=VERSION`: each event names the regions that moved, with their new data, and its
version; a ping at least every 5 s; a stream silent for two and a half ping intervals is lost; the
last 128 events are kept, and a `since` older than them answers `409`; after a loss the page reads
a fresh board and opens a new stream from it. A document's stream carries only the sections in
view. Server notices ride the same stream. A page closes its stream when it is left or frozen, so
a page kept for Back does not hold one of the browser's six connections. The reload's hand-over
ends every open stream as the old generation drains, and each page opens a new one on the new
generation.

**GraphQL subscriptions** would carry the same events over a WebSocket: the
`graphql-transport-ws` protocol of the `graphql-ws` package, which Apollo's and urql's clients
take and which both Rust libraries serve, as they do the older `graphql-ws` protocol
([the protocol](https://github.com/enisdenjo/graphql-ws/blob/master/PROTOCOL.md);
[async-graphql](https://github.com/async-graphql/async-graphql/blob/master/src/http/websocket.rs),
[Juniper](https://github.com/graphql-rust/juniper/blob/master/juniper_graphql_ws/README.md)).
Over Server-Sent Events, as the feed is today, there is a protocol (`graphql-sse`, 2.6.3 on npm)
that neither Rust library serves (read in their code, by search). Neither form adds what the feed
lacks; each would need the feed's rules again on top of it: the version to resume from, the loss
after a silence, the reset from a fresh read, the stream closed when a page is left, the drain on
reload (inferred). The server has
no WebSocket today (axum is built without its `ws` feature); a subscription over one adds it and
its dependencies to the binary (read in code: `apps/symphony-server/Cargo.toml`). The events
themselves are a tagged union of regions, a project's plugin regions among them, so they would
cross as JSON, and the page would still apply each change by hand, or answer each event by
reading the page's query again (the probe's `apollo-live.jsx`), which reads more than today's
changes carry (inferred).

### Typed clients

**Today** (read in code at `a8b379be`: `apps/symphony-web/AGENTS.md`, `wire.sh`,
`src/wire.ts`). ts-rs 12.0.1 derives the TypeScript of the types the React client reads, behind a
`typescript` feature of `symphony-api` and `symphony-blocks` that no running build turns on;
`just client-wire` writes `src/wire.ts` (18 types and the constants the client names: the feed's
path and events, the Symphony's routes, the cache's format); `just client-lint` fails when the
committed file is not what the Rust prints. A read is checked where it arrives, the fields the
page draws and no others, so a field a newer server adds changes nothing.

**Under GraphQL** the schema becomes the contract: the Rust types derive GraphQL objects, the
server prints its schema (SDL), and the TypeScript of each query's answer is generated from the
schema and the query, by GraphQL Code Generator (`@graphql-codegen/cli` 8.0.0, a build step,
[its docs](https://the-guild.dev/graphql/codegen)) or inferred inside TypeScript by gql.tada
(1.11.5, [its docs](https://gql-tada.0no.co/)). That types each answer by exactly the fields its
query asks for, and checks every query against the schema at build time, which today's guards do
at run time. It costs a second chain beside the first, not one in its place (inferred, from the
sections around this one):

- The derives would sit on the same types in `symphony-api`, behind a feature, as ts-rs's do: a
  derive goes on a type's own definition, and Rust lets no third crate implement a library's
  trait for another crate's type.
- The feed's events, the blocks, the plugins' fields and the constants stay outside the schema
  or cross it as JSON, so ts-rs and its check stay for them.
- The check becomes two: the committed schema is what the server prints, and the generated types
  are what the schema and the queries give.
- GraphQL's `Int` is 32 bits ([the spec](https://spec.graphql.org/October2021/#sec-Int)); the
  feed's version and the board's are `u64`, so they would cross as strings or a scalar of the
  Symphony's own.

### Plugins' answers in a schema

A plugin answers typed data that the server passes on (read in code: `crates/symphony-plugin`,
`crates/symphony-blocks/src/blocks.rs`, [PLUGIN_PROTOCOL](../PLUGIN_PROTOCOL.md)): blocks, which
the client draws with one component each; a task's board fields, a map of plugin to field name to
value; the regions a plugin adds to home. In a GraphQL schema:

- **Blocks are a tree of any depth.** Eleven kinds of block hold blocks of their own
  (`Vec<Block>`: a list's items, a quote, a callout, a card's body and seven more), and a table's
  cells hold them through a span (`Span::Blocks`). A GraphQL query names
  every field it wants at every level, and a fragment cannot include itself
  ([the spec, 5.5.2.2](https://spec.graphql.org/October2021/#sec-Fragment-spreads-must-not-form-cycles)),
  so a query can ask for a block tree only to a depth it spells out. A document's blocks would
  cross as one JSON value (read in docs; inferred for these types).
- **Board fields are maps** keyed by names each project's plugins pick; GraphQL's types are
  scalars, objects, interfaces, unions, enums, input objects and lists, with no map
  ([the spec, 3](https://spec.graphql.org/October2021/#sec-Type-System)), so they cross as JSON
  or as a list of name and value pairs.
- **Tagged unions** (`#[serde(tag = "type")]`), such as `Block` and the feed's `Change`, become
  GraphQL unions, whose members must be object types
  ([the spec, 3.8](https://spec.graphql.org/October2021/#sec-Unions)): a kind with no fields,
  such as `Block::Rule`, needs an object of its own.
- **A schema that grew with each project's plugins** would differ from one project to the next,
  and the client's generated types are built once for every project (inferred).

So GraphQL would type the Symphony's own fields and pass the plugins' answers on as untyped JSON:
its types would stop at the plugin seam, where the client draws the most generically.

### Where faster responses would come from

- **Round trips that wait on each other:** yes, over a slow network, about 0.6 s a step; nothing
  on this machine. The same steps can go without GraphQL ([above](#requests-and-round-trips)).
- **Fewer bytes of data:** a little. A page reads 5 to 23 kB before it is drawn; fields left out
  would save a fraction of that, and the library costs 10 to 68 kB more on every first visit.
- **The server's time:** no. The time of a read is the work behind it, which resolvers would do
  the same way; the probe measured async-graphql at 62 µs and Juniper at 74 µs a query for the
  board and a column of 30 tasks, where serializing the same data as JSON took 5 to 7 µs: 0.06 ms
  more a query, against reads of 1 to 200 ms (measured, medians of 15 batches of 1,000 at a load
  of about 18; [the probe](#the-probe)).
- **Return visits:** no. They already draw from the device and come back as empty `304`s, which a
  `POST` query never does.
- **The client's cache between pages:** some, for a task opened again within one page's life,
  which an in-memory cache answers at once; the device cache already draws it at once from
  IndexedDB and checks it after.

What is making the client faster now is the port itself: the queue's first visit fell from
255,310 to 101,350 bytes, the Dioxus wasm gone (measured at `a8b379be`, read on its thread).

### What GraphQL would replace in the React client

The foundation's three modules, as the React port's orchestrator asked (read in code at
`a8b379be`):

- **Reads** (`src/request.ts`, 178 lines): the transport, under its policy. Apollo's or urql's
  link would send the queries; what the module decides stays and moves into a link of its own: a
  failure's three kinds (refused, uncertain, missing), the 15 s wait for an answer, the
  performance marks each save leaves for the browser tests, and the saves, which would become
  mutations or stay `POST`s beside the schema.
- **The feed** (`src/feed.ts`, 158 lines, and the loop in `src/live.tsx`): the `EventSource`
  would become a subscription client; the page's lifecycle (closing the stream when the page is
  left or frozen), the loss after a silence and the reset from a fresh read stay, rebuilt on the
  new client, which has its own reconnects and pings to reconcile with them.
- **The device cache** (`src/cache.ts`, 176 lines, over the shell's `cache.js`): replaced only in
  part. The cache in the page would be Apollo's or Graphcache's; keeping it across visits, the
  project's keys, the caps, the format number and the stale draw would be rebuilt on its
  persistence, or kept as they are beside it; the `304` that makes a return visit cheap is lost
  for every query sent by `POST`.
- **The generated types and their check:** kept for the feed, the blocks and the constants, and
  joined by the schema's types and a second check ([Typed clients](#typed-clients)).
- **The nine port tasks** (`app/client-moves-to-react/*`, in backlog): each brief says to extend
  those modules, never fork them (their orchestrator's post on this task's thread); under GraphQL
  they wait for a task that replaces them first.

## The Rust GraphQL library

The two that are maintained, looked up on 2026-11-14 on crates.io (its API: versions, dates,
downloads) and GitHub (`gh api`: commits since 2025-11-14 on the default branch without those
whose author's account GitHub marks as a bot, Dependabot's chiefly, and the people who made
them; and the open issues and pull requests); the probe's rows measured as [below](#the-probe). A search of
crates.io found no other maintained, general-purpose Rust GraphQL server: seaography and
dynamic-graphql are built on async-graphql, juniper-from-schema was last released in 2020, and
cynic, graphql_client and apollo-rs are clients or compilers.

| | Juniper | async-graphql |
|---|---|---|
| newest | 0.18.0, 2026-10-30 | 7.2.1, 2026-01-20; 8.0.0-rc.5, 2026-04-21 |
| before it | 0.17.2 (2026-10-28), 0.17.1 (2026-01-26), 0.17.0 (2025-09-08), 0.16.2 (2025-06-25) | 7.2.0 (2026-01-16), 7.1.0 (2026-01-02), 7.0.19 (2026-01-01), 7.0.17 (2025-05-24): none from May 2025 to January 2026 |
| who | the graphql-rust organization: 53 commits by 7 people in the year (71 with bots') | the async-graphql organization, its author and, since December 2025, a second maintainer, taken on after [#1762](https://github.com/async-graphql/async-graphql/issues/1762) ("Is this library still being maintained?", opened 2025-11-03, answered by that maintainer on 2025-12-24 and closed on 2025-12-26): 87 commits by 24 people in the year, none of them bots', 54 of them that maintainer's |
| last commit | 2026-11-09 | 2026-04-21, and nothing pushed since |
| open issues, pull requests | 48, 13 | 228, 31 |
| downloads, all and the last 90 days | 3.7 million, 0.35 million | 34.8 million, 4.4 million |
| subscriptions | WebSocket, both protocols (`juniper_graphql_ws` 0.6.0) | WebSocket, both protocols; also over HTTP as `multipart/mixed` |
| Server-Sent Events | no | no |
| integrations | `juniper_axum` 0.4.0 (axum 0.8), `juniper_actix` 0.8.0, `juniper_warp` 0.10.0, `juniper_rocket` 0.11.0, `juniper_hyper` 0.11.0, all 2026-10-30 | axum (0.8), Actix Web, Poem, Warp, Rocket, all 7.2.1 (2026-01-20) |
| the wire's serde types | a derive of its own on each (`GraphQLObject`), which reads no serde attribute | a derive of its own on each (`SimpleObject`), which reads no serde attribute |
| a serde type taken whole | no JSON value built in: a scalar of the Symphony's own (`#[graphql_scalar]`) | `Json<T>` for any serde type, and `scalar!` |
| compile, release from nothing, CPU beyond plain Axum (measured) | 54 s (98 s against 44 s) | 158 s (202 s against 44 s) |
| the same, wall time on 16 cores (measured) | 43 s against 20 s | 79 s against 20 s |
| stripped binary beyond plain Axum (measured) | 0.77 MiB (1.85 MB against 1.04 MB) | 1.54 MiB (2.65 MB) |
| a query, the board and 30 tasks (measured) | 74 µs | 62 µs |

The sources: [crates.io](https://crates.io/crates/juniper) and
[the repository](https://github.com/graphql-rust/juniper) with
[its changelog](https://github.com/graphql-rust/juniper/blob/master/juniper/CHANGELOG.md) for
Juniper; [crates.io](https://crates.io/crates/async-graphql) and
[the repository](https://github.com/async-graphql/async-graphql) with
[its integrations](https://github.com/async-graphql/async-graphql/tree/master/integrations) for
async-graphql. That neither derive reads serde's attributes was read by searching their code,
not tried; so every renamed field and every serde-tagged union would be named again for GraphQL
(inferred).

What else the record says (read in docs):

- **Juniper 0.18.0** follows the September 2025 GraphQL spec and limits a query, by default, to 64
  levels of nesting and 15,000 tokens and fields, the answer to two security advisories (its
  changelog). Its minors break: 0.17 to 0.18 is a new major in all but name.
- **async-graphql's compile time** is its executor's generic code built again for every type and
  field, says an open issue ([#1846](https://github.com/async-graphql/async-graphql/issues/1846),
  2026-10-27, whose fork cut one service's rebuild from 3 min 8 s to 37 s, not merged), and a
  change in the 8.0 candidates cut the code its derives expand to
  ([#1796](https://github.com/async-graphql/async-graphql/pull/1796)), which 7.2.1 does not have.
  For the Symphony, whose wire has 158 types, the cost would grow with every type derived
  (inferred; the probe derives three).
- **The size budget.** `symphony-server` is held to 16,150,000 bytes (`tools/size-budget.toml`)
  and was 14.8 MiB at `a8b379be` (its `just size-check`, read on its thread): about 0.6 MiB of
  room. Either library's measured weight is over it, so GraphQL would raise the server's line.
  The probe's figures are an upper bound: a minimal binary counts crates the server already links
  (inferred).

## The server framework

### What this server asks of its framework

Read in code (`apps/symphony-server/src`, 38,787 lines with its unit tests):

- **Axum 0.8 with three features,** `http1`, `json` and `tokio` (`Cargo.toml`): HTTP/1.1 alone,
  no WebSocket.
- **One Axum route and a fallback** (`front.rs`, `router`): `/api/front`, and everything else
  goes to the server's own dispatch, which picks among 30 reads (`api.rs`, `READS`), 15 saves
  (`crates/symphony/src/save.rs`, `ROUTES`, with the upload and an agent's focus), and the
  pages, media, uploads, plugin routes and old bookmarks. So the framework's router is barely
  used.
- **Three layers:** compression (`tower-http`'s gzip, with a predicate of the server's own per
  request), a HEAD that runs as a GET and drops the body, and the judge that refuses any address
  but this machine's and its tailnet's, and any path into git's metadata
  (`middleware::from_fn`).
- **The projects' dispatch:** the one server hands each request to its project's router with
  `tower`'s `ServiceExt::oneshot` (`projects.rs`), as the router's tests do without a socket.
- **A listener of its own under `axum::serve`** (`front/connections.rs`, `impl Listener for
  Connections`): it stops accepting when the server stops, answers a socket Hyper drops with a
  `503`, admits each connection before its first body read, and gives the handlers the peer's
  address (`ConnectInfo`) and a graceful shutdown.
- **The reload's hand-over** (`reload.rs`): the serving generation passes its listening sockets
  to the new build by descriptor, which takes them as `std::net::TcpListener::from_raw_fd`, then
  `tokio::net::TcpListener::from_std`; accepted connections stay with the old generation, which
  drains them, its feed streams ended.
- **Streams it writes itself:** the feed is a `Body::from_stream` of the event text the server
  formats (`api.rs`, `feed`), not Axum's `Sse`; a file streams through `file_body.rs`.
- **Its own static files:** the client's files are embedded at build time and served by the
  server's own handler, each gzipped once and kept, with its own validators and cache headers
  (`api/client.rs`, `api/client/gzipped.rs`): no framework's static files.

So a framework here must run on tokio, take an already-bound listener (better, an accept loop of
the server's own), give a middleware seam and streaming bodies, and compress.

### The candidates

Looked up on 2026-11-14 as the libraries were ([crates.io](https://crates.io), their repositories
and docs.rs at each version named), the commits counted without bots' the same way; Hyper's,
Poem's, Warp's and ntex's had none.

| | newest | who, and the last year | runtime | SSE | WebSocket | compression | an inherited listener | tower |
|---|---|---|---|---|---|---|---|---|
| **Axum** | 0.8.9, 2026-04-14; last commit 2026-11-13 | tokio-rs: 233 commits by 83 people (259 with bots') | tokio | `response::sse` | `ws` feature | `tower-http` | `serve` over any `Listener`: the server's own today | its own |
| **Actix Web** | 4.19.0, 2026-11-09; six releases since 2026-10-12 | the actix organization: 295 commits by 27 people (504 with Dependabot's) | `actix-rt`, a single-threaded tokio runtime per worker | in `actix-web-lab` | `actix-ws` | built in (`Compress`) | `HttpServer::listen`, a `std::net::TcpListener` as it is | no: its own service trait |
| **Rocket** | 0.5.2, 2026-10-23, 29 months after 0.5.1; its main line 4 commits in the year | rwf2 | tokio | `EventStream` | `rocket_ws` 0.1.1, 2024-05-23 | none built in | none in 0.5.2 (`launch_on` is on its unreleased main line) | no: fairings |
| **Warp** | 0.4.4, 2026-11-13 | its author's own account: 11 commits by 7 people; 179 open issues | tokio | a filter | a filter (feature) | a filter | `Server::incoming`, over a tokio listener | `warp::service` |
| **Hyper** (bare, with `hyper-util`) | 1.12.0, 2026-10-06 (`hyper-util` 0.1.21) | hyperium: 137 commits by 62 people | tokio, through `hyper-util` | none | the upgrade alone | none | any socket the server accepts | through `hyper-util` |
| **Poem** | 3.1.12, 2025-07-28: no release in 15 months, a 4.0 unreleased | poem-web: 62 commits by 18 people | tokio, Hyper 1 | feature | feature | feature | `TcpAcceptor::from_std` | `tower-compat` |
| **Salvo** | 1.1.2, 2026-11-10 | salvo-rs: 651 commits, 602 of them one person's (679 with bots') | tokio, Hyper 1 | feature | feature | feature | `TcpAcceptor` from a tokio listener | `tower-compat` |

Beside them, ntex (4.8.0, 2026-11-12) runs on a runtime of its own by default, one person wrote
680 of its 866 commits in the year, and no Server-Sent Events module was found in it; loco is a
framework built on Axum. The sources: docs.rs for [Axum's
`Listener`](https://docs.rs/axum/0.8.9/axum/serve/trait.Listener.html), [Actix Web's
`HttpServer`](https://docs.rs/actix-web/4.19.0/actix_web/struct.HttpServer.html),
[Rocket](https://docs.rs/rocket/0.5.2/rocket/struct.Rocket.html) with
[its changelog](https://github.com/rwf2/Rocket/blob/v0.5/CHANGELOG.md),
[Warp's `Server`](https://docs.rs/warp/0.4.4/warp/struct.Server.html) with
[its changelog](https://github.com/seanmonstar/warp/blob/master/CHANGELOG.md),
[`hyper-util`](https://docs.rs/hyper-util/0.1.21/hyper_util/service/index.html),
[Poem's `TcpAcceptor`](https://docs.rs/poem/3.1.12/poem/listener/struct.TcpAcceptor.html) with
[its changelog](https://github.com/poem-web/poem/blob/master/poem/CHANGELOG.md),
[Salvo's `TcpAcceptor`](https://docs.rs/salvo_core/1.1.2/salvo_core/conn/tcp/struct.TcpAcceptor.html)
and [ntex](https://docs.rs/ntex/4.8.0/ntex/index.html).

What follows (inferred from the table and the section above): Rocket cannot take the reload's
sockets today; Poem is unreleased for over a year; Warp moves slowly under one account; Salvo and
ntex rest on one person; bare Hyper is what Axum already is underneath, so going to it means the
server writing its own serve loop, shutdown and connection info, for no feature it lacks. Actix
Web is the one serious alternative: maintained, released often, with everything the server
needs; it would replace the `tower` parts (the projects' dispatch, the compression's predicate,
the router's socket-free tests) with its own, and run its handlers on a runtime per worker.

### Performance, and whether it matters here

**Published.** TechEmpower ended its benchmarks on 2026-03-24 and archived them
([its announcement](https://github.com/TechEmpower/FrameworkBenchmarks/issues/10932)); its last,
Round 23, ran in early 2025 ([its post](https://www.techempower.com/blog/2025/03/17/framework-benchmarks-round-23/)).
From that round's results on physical hardware (56 cores), ranked from its raw data
([ph.json](https://www.techempower.com/benchmarks/results/round23/ph.json)), not read off its
site: in its JSON test Hyper answered 2.89 million requests a second, Axum 2.71 million (87 % of
the fastest of all), Salvo 2.09 million and Rocket 0.54 million; Actix Web and Warp gave no
results that round. In Round 22 (2023) Actix Web's JSON entry ranked 19th and Axum's 95th.
HttpArena, a live benchmark whose entries different people write, has Actix Web ahead of Axum
([its repository](https://github.com/MDA2AV/HttpArena)).

**At this server's load.** 2.71 million requests a second on 56 cores is about 21 µs of a core
a request for Axum, the test's own work included (inferred: 56 ÷ 2.71 million). This server
answers a handful of browsers on one machine, and its reads take 1 to 200 ms of their own work
([above](#what-a-page-reads-today)). A framework twice as fast would save about 10 µs a read:
nothing a page could show (inferred). Its HTTP stack is about 0.35 MiB of the server's binary
and Axum itself 0.03 MiB (measured for the board task `perf/binary-size-budget`, read on its
thread), so there is no size to win either.

### The cost of moving

Read in code: 14 of the server's source files, 6,873 lines, name Axum (68 lines name it), and 25
of its test files. The work is in the parts above that are not routes: the listener
(`front/connections.rs`, 595 lines with its tests), the three layers, the projects' dispatch, the
feed's and the files' streams, and the tests that drive a router without a socket. Under Actix
Web each changes shape; under any framework the reload's hand-over and drain are tested again
end to end (inferred).

### Is anything about Axum failing

No. Its one limit seen here, HTTP/1.1 alone, is the server's choice of features, and would hold
on any framework: no browser speaks HTTP/2 without TLS
([HTTP/2's FAQ](https://http2.github.io/faq/#does-http2-require-encryption)), and the server
serves plain HTTP on loopback and the tailnet. The one cost of staying is ahead, in Axum's
next version, 0.9, whose breaking changes are to `axum::serve`, not to the `Listener` trait the
server implements, which is the same on Axum's main line as in 0.8.9
([its changelog, unreleased](https://github.com/tokio-rs/axum/blob/main/axum/CHANGELOG.md)):

- **Its future's output** is no longer an `io::Result`, and **it takes a generic for the
  response body**. The server calls it once (`front/connections.rs`, `Connections::serve`) and
  discards the output, so these are a compile fix in that file (inferred).
- **It applies hyper's default `header_read_timeout`,** 30 seconds (hyper's
  `http1::Builder::header_read_timeout`; Axum 0.8.9's `serve/mod.rs` sets no timer, so none
  applies today). hyper
  starts that clock whenever it begins reading a request's head, and closes the connection when
  no complete head has come within it (read in hyper 1.12.0's `proto/h1/conn.rs`). That is a
  change of behaviour for this listener, which admits each connection before its first read and
  answers the sockets Hyper drops on a drain with a `503`: a connection that sends nothing for 30
  s, a browser's idle kept-alive one among them, would be closed where today it is kept, and the
  listener's tests that hold idle peers would show what that does to the drain (inferred, not
  tried). Axum added it so that a graceful shutdown cannot hang on a request stuck in its head
  ([#3478](https://github.com/tokio-rs/axum/pull/3478)), which this listener already prevents
  by cancelling unadmitted reads; `axum::serve` on the main line offers no setting for it (read
  in its `serve/mod.rs`), so taking 0.9 means keeping the limit, those paths checked, or serving
  through `hyper-util` directly.

## The probe

`experiments/graphql-stack/` at commit `a40751dc2428b708045ed2ad4d6e27c15d94c314`, removed from
the tree since, its README naming the commands. Its two probes read nothing outside that folder;
a checkout of that revision runs them:
`git worktree add --detach DIR a40751dc2428b708045ed2ad4d6e27c15d94c314`, then the README in
`DIR/experiments/graphql-stack`.

- **The client's bytes** (`web/`): a React 19.3.1 page per library, built alone by Vite 8.3.8
  with the web client's settings (`es2023`, no module preload), its script gzipped at the best
  level and Brotli at quality 11; the table is [above](#bytes-on-the-wire). The pages were built,
  not run.
- **The server's cost** (`server/`): the same read on Axum 0.8.9 three ways, one Cargo feature
  each, a plain JSON read, async-graphql 7.2.1 with `async-graphql-axum` and Juniper 0.18.0 with
  `juniper_axum` 0.4.0, on Rust 1.99.0. `measure.sh` built each from nothing in a target folder
  of its own, release with one codegen unit as the shipped server is, its crates fetched first;
  the one-minute load was 17 to 18 when each began. Then each binary timed 15 batches of 1,000
  plain reads (the data serialized, as the server's reads are) and 1,000 GraphQL queries (parsed,
  validated, run and serialized), in turn, and answered one query on a private loopback port.

| build | CPU, user and system | wall | stripped binary | a plain read | a query | the query's answer |
|---|---:|---:|---:|---:|---:|---:|
| plain JSON | 43.8 s | 20.2 s | 1,038,312 bytes | 6.7 µs | – | 5,976 bytes, every field |
| async-graphql | 201.6 s | 79.3 s | 2,654,840 bytes | 5.3 µs | 61.7 µs | 3,494 bytes |
| Juniper | 97.8 s | 42.8 s | 1,846,040 bytes | 6.0 µs | 73.9 µs | 3,485 bytes |

The query asks for three of a task's six fields, so its answer is 42 % smaller raw than the plain
read's: the saving GraphQL's field selection gives, on a stand-in for a column (raw bytes;
gzipped it would be less, as above).

**Not measured.** A GraphQL client running against a GraphQL server in a browser; a subscription;
the debug and incremental builds the gates run, and the derives on the real 158 types; either
library's weight inside the real server, which shares some of its crates; a phone over the
tailnet, whose round trip decides what one query saves; a quiet machine's times for the live
board's reads.

## Measured, read and inferred

**Measured here, on 2026-11-14.** The live board's reads, requests and bytes on four pages, cold
and back again (one run each, at a load of 100 to 150); a column's fields' share of its bytes; the
bytes each data library adds to a React page; the GraphQL libraries' compile time, binary size and
time a query against a plain JSON read (at a load of about 18). Measured earlier and cited: the
reads' server times and the Fast 3G steps ([the client cache](client-cache.md#measured),
2026-11-11), the React and Dioxus first visits ([the comparison](client-framework-comparison.md#size)
and the port's budget tests at `a8b379be`), the server binary's size by crate (`perf/binary-size-budget`).

**Read in code.** What each page reads and in what order; the feed's contract and the device
cache's design; the foundation's three modules and its wire types at `a8b379be`; the blocks'
recursion and the plugins' field maps; the server's one route, layers, listener, hand-over,
streams and static files; the count of files that name Axum.

**Read in docs, 2026-11-14.** Every version, date, maintainer count and download count, each from
crates.io, npm or GitHub as named; the libraries' caching, persistence, subscription and
integration behaviour, each from its own docs or code; the GraphQL spec's limits; the benchmarks.
That neither GraphQL derive reads serde's attributes, and that neither Rust library serves
`graphql-sse`, rests on searching their code.

**Inferred, not tried.** What one query a page would save over a network (from the measured Fast
3G steps); that the same steps can be saved without GraphQL (a task's read carrying the feed's
version, the board's reads together); the React board's first visit with each library; that a
GraphQL cache would be a second one beside the shell's during the port; that subscriptions would
need the feed's rules rebuilt; the framework's share of a read at this load; the cost of moving
the server.
