# apps/symphony-ui/

The workspace, the cargo wrapper and the toolchain pin: [the Rust section of
the root guide](../../AGENTS.md#rust).

`apps/symphony-ui` is one Dioxus app, the framework the owner chose to experiment
further with, the browser now and the desktop window later, from the same code.
Its pages are moving to the React client, a page kind at a time
([its guide](../symphony-web/AGENTS.md), which says where a UI improvement is built); a kind
moved leaves this package.
It reads the server's JSON API, whose wire types are
`crates/symphony-api` (tested against its `fixtures/`), and never runs server
code: no fullstack server functions. Dioxus is pinned `=0.7.10` and built by
dx 0.7.10.

The server sends typed data from `/api/`; the client renders it with components.
It never requests or inserts server-rendered HTML fragments. Nothing renders or
loads until it is near the viewport: use the shared `viewport::NearViewport`
for sections, or `viewport::use_visibility` for media with their own reserved
layout, plus native `loading=lazy`. Once revealed, keep content mounted to
preserve forms and reading position. Closed panels and collapsed card bodies
load only when opened.

The native browser target owns headless Chrome sessions and private loopback
fixtures. Run `tools/cargo.sh test -p symphony-ui --test browser --locked` from
this workspace. It builds the real release client unless `SYMPHONY_UI_DIST`
explicitly names a built distribution. Install Chrome or name its executable
with `SYMPHONY_CHROME`; CI requires it. The harness owns each browser's temporary
profile, process group and shutdown, and checks keyboard interaction, page
geometry, stream recovery and transfer budgets without a display server.
A client change runs the browser tests of the pages and controls it touched, by
filter after `--` ([CHECKS](../../docs/CHECKS.md#the-browser-target-by-page)),
never the whole target, which the release run runs.

- **Build.** `just symphony-ui-dist [OUT] [KIND]` (`build/symphony-ui`, the page in its
  `public/`): cargo's subcommand `symphony-ui dist OUT` (`bin/cargo-symphony-ui`,
  which the recipe puts on `PATH`; the browser tests run it too) runs
  `bundle.sh OUT` through the cargo wrapper: `dx bundle --web --release
  --debug-symbols false`, on the root manifest's `wasm-release` profile
  (opt-level z, fat LTO, one codegen unit, abort on panic) and dx's wasm-opt at
  `-Oz` (`Dioxus.toml`). `KIND` defaults to `all`: independently compile the
  board, document and review-home modules and the program, build the React client's
  kinds (`web`: one Vite build of `apps/symphony-web`, [its guide](../symphony-web/AGENTS.md)),
  then assemble their content-hashed assets with `package.sh`, which refuses a kind both
  clients build. Naming a kind (`program` among
  them) rebuilds it against the other kinds already in that output folder. The program
  (`page-program`, `src/split.rs`; [its card](../../docs/DECISIONS.md#client-shared-runtime)) is one
  `dx bundle --wasm-split` of the task pages with `/agents`, Reports, the chat, the menus and
  the task timeline: one script and main module, and a chunk for each and one for the media
  layer (no kind: a page of the program loads it once it shows media, Media below), which
  `symphony-build split-repair` puts right after dx
  (crates/symphony/src/cli/commands/split_repair.rs)
  and `assets/chunks.js` loads (`window.reviewChunk`), rejecting a chunk that does not load so
  the shell's `gone` state, never a page left waiting, follows. The shell awaits a kind's chunk
  before it stops the page shown, which stays until the chunk is in, and under the shell's
  notice when it does not load. A chunk mounts its kind's root
  in the one instance (a `VirtualDom` per root, `module.rs`); home and the board keep modules
  of their own, under their 300,000-byte caps, which a program shared with them would pass. The
  shell's manifest names each program kind's script, wasm, `chunk` and styles; package.sh gives
  each the program's styles another kind loads too and its own by name, and refuses a style no
  kind takes. It publishes no shared chunk (`chunk_0_split`, which dx's splitter writes and
  never fills, and no kind loads), and refuses one that holds more than custom sections.
  Without `--debug-symbols false` dx 0.7 emits 260 KB of
  DWARF. dx and wasm-bindgen (the lockfile's version) are built once per
  repository into `symphony/tools/` in the git common dir by `tools.sh`, which
  bundle.sh runs when one is missing (`tools.sh --folder` names that folder,
  `tests/tools.rs`). A build does not run dx from there: dx makes itself the rustc wrapper of
  the workspace's own crates by its canonical path, which Cargo mixes into their
  symbol names, and so into every byte of the wasm and every asset name. So
  `bundle.sh` runs it as `/private/tmp/symphony-client-tools/dx-0.7.10/bin/dx`,
  in a folder private to the user, the same path in every checkout and common dir:
  `dx-path.sh` keeps there a hard link of the installed dx (a copy across volumes),
  replaced whole when it is not that file or its bytes (`tests/tools.rs`).
  A new tree's folder starts from a seed
  another tree made, its wasm32 builds among it. The wrapper clears workspace
  crates from every seeded target/profile and records that cleanup for
  `wasm-fresh.sh`. Without a matching
  record, that guard cleans both wasm32 profiles (`dev`, `wasm-release`); its
  record stays inside the folder. `lint-rs` also runs clippy for
  wasm32 over the client (`cargo symphony-ui clippy`): the host build never
  compiles its browser half (`live.rs`, `platform::Browser`).
- **A build kept off the repository's common dir.** `just client-update`,
  `just client-check`, `just symphony-ui-dist` and the browser target without
  `SYMPHONY_UI_DIST` run `tools.sh` in the repository's common dir; its other
  tests (`tests/tools.rs`, `wasm_build`, `bundle_concurrent`) use folders of
  their own. A reviewer or executor whose branch changes the tools, their
  folder or the scripts that find them names another common dir, so nothing
  is installed into or read from the repository's:
  `SYMPHONY_GIT_COMMON_DIR=DIR`, `DIR/symphony/tools/` holding copy-on-write
  clones (`cp -cR` on macOS) of `dx-0.7.10` and `dx-home` from the folder
  `tools.sh --folder` names. In that common dir the browser target also needs
  `SYMPHONY_SURREAL` naming the pinned SurrealDB, which `tools/surreal.sh`
  otherwise looks for there, and `SYMPHONY_UI_DIST` naming the checkout's
  `build/shipped-client/public` by its absolute path tests the client
  `just client-update` built, building none.
- **The feed.** The client keeps `crates/symphony-api`'s feed contract. A stream
  is lost when it errs, when it says nothing (not even the server's ping) for
  two and a half of the board's `ping_ms` (at most 5 s, so 12.5 s: the owner
  reads the board from a phone over the tailnet, where a stream goes silent
  but open on every network change), or when it sends an event the client
  cannot read. Then the client closes it, marks the page stale
  (`main[data-feed=stale]`, a notice), waits 1 s doubling to 30 s (the waits
  start again only after a stream held 30 s), fetches the board again (it
  replaces the one shown whatever its version) and opens a new stream from it;
  the board is live again once that stream opens. It never resumes by
  `Last-Event-ID`. A page that goes away closes its stream (`live/feed.rs`): on
  `pagehide`, which every browser fires as a page is left, for good or into
  its back/forward cache, and on Chrome's `freeze` of a background tab. A kept
  page's open stream would hold one of the browser's six connections to the
  server (HTTP/1.1) for as long as it is kept, and the sixth full load in a tab
  would wait for its reads behind streams that never end. Once back
  (`pageshow`, `resume`) it reads at once, with no wait, from a fresh board as
  after a lost stream: the server it left may have restarted meanwhile. Hiding
  alone (another tab in front) closes nothing: the home page notifies from the
  background. Six live tabs of one server still take every connection. A board
  or a `client` change naming another build than the
  page's own module script loads the page again, once per build
  (`state::reload_for`, remembered in `sessionStorage`).
- **Cache.** What a page drew is kept on the device so a return visit draws at once
  ([the client cache](../../docs/analysis/client-cache.md)). The store is `assets/cache.js`
  (`window.symphonyCache`), which `package.sh` puts first in the shell's one script: every page
  kind shares it and no kind's wasm carries IndexedDB bindings. It adds 1,433 bytes to each
  kind's first visit and none to a return. `src/cache.rs` reaches it: an
  `Item` is what a visit opens (`board(scope)`, `task(scope, slug)`, `document(path,
  entrypoint)`) and goes whole; its parts are the reads its page drew, each kept with the
  server's tag (`ETag`) and the time it was kept (`Kept`, never later than the page's clock).
  One IndexedDB database for the origin, keyed by project. `cache::visit` once per page open
  counts a visit: one, halving every three days; a visited page's first keep counts that visit
  too, so its item ranks the same whichever of the two comes first. Each project keeps 30 tasks
  in 2 MB and 20 documents in 6 MB, 8 MB in all; the board's reads are outside them, and the
  origin keeps 32 MB in all. An item unused for 14 days goes (one last used by a clock ahead of
  the page's is taken as used now), and one larger alone than its caps is never kept. Over a cap
  the lowest ranked goes first: a page read ahead (`keep(…, ahead)`) before any visited page, a
  project's entrypoint page (`pinned`) last, otherwise the task opened longest ago (a newly
  opened task always enters), the least and least lately visited document, and over the
  origin's cap the item used longest ago. Every call names `symphony_api::CACHE_FORMAT`: a store
  of another format is cleared when it opens, a part kept under another is a miss, and a value
  this client cannot read is forgotten (`cache::read`). Without IndexedDB, or in the development
  runtime, which has no shell, every read misses and nothing is kept. A database that does not
  open within 500 ms leaves the calls until it opens without the cache; the pages after it,
  which the shell changes without a reload, use it once it opens. A page that goes away
  (`pagehide`, `freeze`) aborts the store's open transactions and starts none until it is back
  (`pageshow`, `resume`), as it closes its feed's stream: a page kept for back and forward runs
  none of its callbacks, so a transaction left between two of them would hold its locks, and
  Chrome evicts the kept page (IgnoreEventAndEvict) once the next page's first call asks for them.
  `tests/browser/cache_store.rs` drives the store alone in headless Chrome, no release build
  needed, and reports its coverage. Each case starts on a deleted database once the store has
  opened its new one (a scratch item kept, then forgotten), so a loaded machine's slow open
  never runs a case without the cache; the pages that test the deadline keep the store's 500 ms
  against their own opens.
- **A return visit.** Home and the task board (and the React client's queue, the same way) draw what they kept of
  `Item::board(scope)` before the server answers, then check it. `/api/board` (part `board`) is
  drawn and still read fresh, with no tag, since its `version` starts the feed; nothing else is
  read before it. Every other kept read then goes at once with `If-None-Match` naming the kept
  copy's tag and `cache: "no-store"` (`request::get_if`): a `304` is the copy, a `200` replaces it
  on the page and on the device. The queue keeps `/api/queue` (`queue`, `cache::board::QUEUE`). The task board keeps its
  default view only (no search, the last 24 hours, stages by the machine's activity): the counts
  (`counts`), each column's first page (`column:STATUS`) and the machine's work (`work`), never a
  search, another window or a Load more page; a column drawn from the device is checked at once,
  not after its 250 ms. Its listings are read with `work=0`, which places tasks by the machine's
  Build and Test work as before and leaves that work out of the body, whose heartbeats would
  otherwise change every listing's tag; the work is read apart (`/api/machine-work`) and followed
  by the feed. The header's reads are never kept. A check that fails is asked again, waiting
  longer each time (`state::Backoff`), until it answers: the counts, each column drawn from the
  device and the work (`board_live::check`), the queue (the React client's `src/queue.tsx`); the feed's `machine-work`
  event, which carries the whole work, answers for the work's read. Once every column drawn from
  the device has answered, a task it drew that no fresh answer (a column, a feed `task` event)
  listed is dropped, whichever stage the work places it in by then. `cache::Freshness` says what
  a page drawn from the device shows: `main[data-feed=stale][data-cache=device]` and the notice
  "Shown from this device, saved 14:32. Checking for changes…" (the oldest copy drawn), "Not
  reachable: …" while a read fails and the feed is not open, cleared once every drawn read has
  answered and the feed is open. A task page keeps `Item::task(scope, slug)`: its page (`page`,
  `/api/task-page`) and its thread's newest page (`thread`, `/api/task-history` without
  `before`). Opened, it counts a visit and draws them at once unless it shows that task already
  (`task_live::opened`). Both are checked together, each asked again until it answers
  (`task_live::load`, `answer`): once `/api/board` is read on the page's first open, at once
  for a task opened in place from another. A thread answered `200` replaces the messages the
  device drew and keeps those the feed brought meanwhile (`cache::task::newest`). Until the
  page's check answers, a page drawn from the device reads no other part of the task: the
  newest messages when no thread was kept, the subtasks and earlier messages wait for it, and
  the feed replays what changed after the board's version. The landing offer, the gates and the
  timeline, which it never keeps, it reads at once. A task the server no longer has (`404`) says so and
  is forgotten. Never kept: earlier thread pages, the landing offer (`/api/task-offer`, read on
  every visit), the agents, the subtasks' further pages and the brief's later chunks. `/agents`
  draws nothing from the device.
  A document keeps `Item::document(path, false)`: its outline (`outline`,
  `/api/document?view=outline`) and the bodies of the sections it drew (`sections`, one map by
  key, each with its card and thread and the validator it was taken under,
  `cache::document::KeptBody`). A section's validator is its outline `hash` and, for a card, its
  thread's hash (`DocumentOutline::thread_hashes`: a thread is no part of its section's hash); a
  section showing a difference from main is never kept. Opened, the page counts a visit and draws
  the outline and every body current by it at once (`docs_live::from_device`), then reads the
  outline naming its copy's tag (`request::get_document`). The server tags a document read
  without the feed's cursor, so an unchanged document is a `304` however the board moved, and
  names the cursor in a header (`symphony_api::VERSION_HEADER`), on the `304` too, from which the
  feed opens. The bodies the server's outline names alike stay and are read no more
  (`cache::document::Bodies`), on a reconnect as on a return; the others are read as the page
  wants them, and no body is read before the outline answers. A body the feed changed is kept
  again only once a later outline names it. An entrypoint's own page, the main checkout's, is kept
  pinned (`Item::document("docs/", true)`): its folder (`folder`, a `304` while unchanged, the
  server's tag leaving out its intro's cursor) and its intro's outline and bodies in the one item,
  so it outlives the documents visited once. No other folder page is kept. A document the server no
  longer has says so and is forgotten. Never kept: versions, diffs, asks and the history.
  `cache::STALE`, which `PageNotices` sets while that notice or the reconnecting one shows, holds
  the owner's actions (verdicts, messages, comments, questions, priority, cancel, landing, Later):
  disabled, saying "Checking…"; drafts stay editable. `tests/browser/cache_pages.rs`,
  `cache_tasks.rs` and `cache_documents.rs` hold the reads back on the stand-in
  (`Site::hold_route`, which answers `If-None-Match` as the server does and logs each answer;
  `Site::set_document` serves a document's outline, its first part and its sections by their keys
  as the server does).
  Once a page has its data, its feed is open and the browser is idle, the shell reads ahead the
  first 8 task and document links on the screen in its `main` (`ahead` in `assets/shell.js` and
  `assets/cache.js`; [Read ahead](../../docs/analysis/client-cache.md#read-ahead)): one after
  another, at `priority: "low"`, marked `Purpose: prefetch`, naming any tag kept, and kept
  unvisited. A task is its page and its thread's newest page; a document its first part
  (`/api/document?view=first`: the head, every heading among it, and the leading sections whole
  within `symphony_api::AHEAD_BYTES`; the server does not follow it), whose contents and bodies a
  visit draws at once, so nothing moves when its outline answers, and keeps as its own
  (`cache::document::split`). Never on save-data or a 3g or slower link, never a link that acts, a
  search, the page itself, a folder or another checkout's page. The stand-in logs a read of the
  API marked as a prefetch apart from the page's own (`Site::read_ahead`, `asked_ahead`;
  `requests`, `targets` and `answers` leave it out), so a test counting a page's reads is not
  raced by the reads ahead; `tests/browser/cache_ahead.rs` covers them.
- **Budget.** Both `/` and `/tasks` have a 300 KB first-visit gzip budget and
  a 20 KB return-visit budget (`tests/browser/`, restored after the port;
  both the owner's, the first-visit cap raised by the owner to 300 KB, and
  never loosened by an agent). A visit counts each response the page loaded at what Chrome's
  network events say it took on the wire, gzip body and response head: the
  HTML, the shell, the module's glue and WASM, the style sheets, API replies
  (refusals too) and the live feed's head and content messages, as the page
  reads them. The stand-in server sends the build as the server does, gzipped
  at the best level by the first request for each file and kept for the test
  run, and the page and API replies at the default level
  (`tests/browser/site.rs`): a start reads the shell alone, and a run gzips
  the files its pages ask for, the whole published folder, older builds kept
  and all, only for the canonical-entry test's report.
  It leaves out document-content images, browser-cache hits and
  the feed's keep-alive pings, which arrive every `ping_ms` for as long as the
  page is open and are reported beside the total: leaving the pings out is
  the owner's choice, as the caps are.
  Other page kinds are reported separately for review.
  On the fixture a first visit to `/tasks` takes 261,350 bytes, 38,650 under the
  cap: the board WASM 230,907, its glue 11,086, the shared style sheet 9,175,
  the shell 6,151 (the client cache's store about 1,430 of it), the HTML 1,142, API
  replies 2,804 (`/api/machine-work` 247 of them) and the feed's head and
  opening comment 85. `/` takes 232,052; cached returns take 1,986 and 2,397.
  Drawing a return visit from the device costs the first visit its code: about
  14.0 KB of the board's WASM and 6.9 KB of home's, most
  of the board's in `board_live` (the draw, the check and its retries) and the
  cache's bindings (`wasm_size` on the named copies); about 13.6 KB of the task page's and
  `/agents`' kind, 11.0 KB of it the task page's draw, checks and retries (`task_live`), and
  the documents' 2.2 KB. The pages' ghosts and what they keep (Loading, below) cost it
  about 13.3 KB of `/tasks`, 2.4 KB of `/`, 13.0 KB of a task page and
  `/agents` and 6.8 KB of the documents. Documents
  drawn from the device, an entrypoint's page kept and the reads ahead
  (`perf/client-cache/documents` and `/ahead`, against main's build at 8ef025d) cost every page
  about 0.9 KB of the shell and the documents' kind about 30.4 KB (`wasm_size` on the named
  copies: `docs_live`'s draw from the device, reads and keeping 9.0 KB, the cache's bindings
  3.5 KB, and the maps it adds with their trees and sorts about 9.8 KB of `alloc` and
  `core::slice`, a stable sort compiled for each `collect` into a map): `/tasks` takes 296,924 bytes
  (3,076 under the cap, from 295,902), `/` 237,202 (from 236,322) and `/docs/` 505,402 (from
  474,965); a document's return takes 1,633 (from 2,371), the budget's stand-in naming no cursor
  on its document reads, so its outline's `304` is read again whole. The reads ahead start only
  once the page's own reads have answered (`cache_ahead.rs`) and go marked as prefetches, so the
  budget counts them outside the first visit, with the warmed modules (`prefetched_bytes`): on
  the stand-in's links page, a task and seven documents, 5,511 bytes on the wire. The task
  pages, `/agents` and Reports are kinds of the program (Build, above;
  `perf/client-shared-runtime/task-pages-program`, against main's build at 5352c2d), which costs
  every page the shell's 38 bytes that name its chunks (`/tasks` 295,939 from 295,902, `/`
  236,351 from 236,322): a first visit to one takes the program's script (17,907 bytes on the
  wire) and main module (216,782) and its kind's chunk (the task pages' 315,343, Reports'
  48,996), so `/agents` and a task page take 572,269 and 572,862 (from 559,770 and 560,363,
  2,241 on a return that finds a task unchanged, its page and thread each a `304` of 82) and
  `/reports` 306,609 (from 558,271), and a move between them takes the target's chunk alone. A
  task page's timeline, a chunk of the same program, comes once the page has drawn and is
  idle: 54,357 bytes outside its first visit (from 202,226; its chunk 50,648, style sheet 2,604
  and first read 1,105), and `/agents` never loads it. Once a page has its data and is idle,
  the shell warms the chat's and the menus' chunks, with the program's script and main module
  where the page is not one of the program's: 609,120 bytes outside home's first visit (from
  748,701), about 374,600 outside a task page's, and the chat's first open transfers nothing
  more. The media layer leaving the main module for a chunk of its own
  (`perf/client-shared-runtime/trim-shared-part`, against main's build at 2eec23a) takes it to
  207,514 bytes on the wire (from 216,490; 492,237 bytes of wasm from 515,290), and the task
  pages' chunk to 318,193 (from 316,744), the code they shared with it now theirs: `/agents` and
  a task page take 567,190 and 567,783 (from 574,579 and 575,172), `/reports` 298,785 (from
  307,557), and every other page 71 to 94 bytes more, about 60 of them the shell's for a media
  chunk that does not load (`/tasks` 299,205 from 299,134). The chunk, 31,502 bytes of wasm and
  12,903 on the wire, comes only once a page shows media: a task page that shows media on its
  first screen takes it too, 580,686 bytes, 5,514 more than main's, and one more round trip.
  The timeline then takes 55,938 outside a task page's first visit (from 55,208), and the warmed
  chat and menus 604,545 outside home's (from 611,040) and about 377,800 outside a task page's:
  the chunks that share code with the media layer (the task pages', the chat's, the menus', the
  timeline's and Reports') each hold a copy of it, 4,646 bytes on the wire in all, 19,295 bytes
  a build. The keyboard's key names, `document::eval` with serde_json, and float formatting stay
  in the main module: dioxus-web makes at launch the event converter and the document that
  reach them, and the splitter keeps there every function of a table the main module reaches
  ([the analysis](../../docs/analysis/client-shared-runtime.md#building-a-in-order)). The
  client reading none of a task's plugin fields (`symphony_api::Task::fields`, which it does
  not draw: `symphony-api`'s feature `skip-board-fields`, which `Cargo.toml` turns on for the
  wasm32 build alone) and the media layer's notice moving to the program's script
  (`perf/tasks-first-visit-gets-room`, against main's build at 029d581) take `/tasks` to
  281,222 bytes (from 299,662: the board's WASM 247,735 from 266,112, the shell 7,880 from
  7,928), `/agents` and a task page to 548,973 and 549,566 (from 567,204 and 567,797, the task
  pages' chunk 18 KB less) and `/reports` to 298,951 (from 298,790), the program's script now
  holding the notice; every other page stays within 50 bytes of main's. Each
  module pays for its own compiled framework, the program once for all its kinds;
  later visits reuse cached imports. The home budget test
  is independent of the canonical-entry test, which gives every kind a fresh
  browser profile and reports the complete published output.
  Measure loaded resources, including API data, rather than summing every
  deferred module or an older build's file kept in the published output.
  The 300,000 and 20,000 byte limits are the owner's; a lower first-visit
  line needs an architectural change the owner approves, and the first-visit
  analysis below prices those the owner left to revisit.
  `cargo symphony-ui size OUT NAME...` (`probes/size.sh`) builds a page kind or
  a size probe as the release does and names a copy for
  `examples/wasm_size.rs`, which says where its bytes go and, with
  `--functions GROUP`, which of a group's functions hold them
  ([the first-visit analysis](../../../docs/engineering/analysis/review-ui-first-visit.md)).
  Measured prices: healing a lost feed 1.4 KB, then the pings,
  unreadable events, the build check and the backoff 0.9 KB, the `mounted` feature
  (`onmounted`) 26 KB, `router` with the `document` it needs 55 KB, serde_json's
  parser 11 KB, three `BTreeMap`s of the board's own 5.7 KB (each key and value
  type compiles its own tree code; a handful of keyed values go in
  `state::FewMap`). So the app has `minimal` and `asset` only, the browser parses
  JSON and serde-wasm-bindgen reads it, and a slice that needs more pays for it
  here first.
- **Components.** `#[component] fn Hero(card: Option<Card>, now: Moment) ->
  Element { rsx! { section { class: "hero", h2 { "{card.title}" } } } }`. Props
  are `Clone + PartialEq`, and a component whose props did not change is not
  rerun, so each region of a page is a component of its own data. In `rsx!`:
  `for card in cards { NextCard { key: "{card.slug}", card } }`, `if waiting {
  ... }`, attributes as format strings (`href: "/{card.doc}"`), `class: if
  waiting { "waiting" }`.
- **Controls and themes.** The controls under `src/components/` are copied from
  DioxusLabs/dioxus-components at the pinned primitive revision; their README
  records the local adaptations and stylesheet hashes. Buttons, text fields,
  selects, picture dialogs and the conversation tabs use them. The header's
  Appearance button opens a panel of the deferred module (`#appearance-panel`,
  as the bell opens activity's) whose theme menu imports a shadcn/tweakcn registry JSON file's
  `cssVars.theme`, `cssVars.light` and `cssVars.dark`, including fonts and radius.
  `theme.rs` validates token declarations; the shell restores their saved CSS
  before mounting a page. Status tokens such as `like`, `question` and `live`
  keep their defaults unless the imported file sets them. UI colours use the
  canonical shadcn token names; `dx-components-theme.css` maps those names to
  the copied components' palette. Extend the shared controls for new fields.
- **State.** `let mut board = use_signal(|| None::<Board>)`; read with
  `board()` (a clone) or `board.read()`, write with `board.set(..)` or
  `board.write()`. Signals are `Copy`: move them into closures and tasks.
  `use_hook(|| spawn(async move { .. }))` starts a task once per mount, which
  lives as long as the component. A browser callback (an `EventSource`
  listener) never writes a signal: it sends on a `futures_channel` that a
  spawned task drains. `use_effect` runs after the frame's edits are in the
  page and again whenever a signal it read changes.
- **Task views.** `tasks.rs` draws columns and hierarchy; `board_live.rs` owns
  summary-only counts, columns and deltas. `task_detail.rs` and `task_live.rs`
  own task pages and agents, compiled only into their page kind. A task page's
  gates (`task_gates.rs`, docs/TASKS.md#gates) are one section under its head,
  read from `/api/task-gates` when it nears the screen and again when the one
  counter `task_live` keeps of the events that concern the shown task grows
  (`Data::gates_nudged`: a verdict, its gates' branch moved, a linked card) or
  the task or its runs change, a newer read aborting the one before. Until then
  it is its own outline, the shape the server or this browser last saw (`Room`);
  a task nothing gates shows none, and a failed read says so in its place. In the
  program alone (the task pages, Reports, the chat, the menus and the timeline, which
  share its runtime), a read dropped before its answer aborts its fetch (`request.rs`,
  `page-tasks`, which `page-program` turns on), so the board's, home's and the documents'
  transport is unchanged; a post never does. As attach.rs does, it puts its own style
  (`assets/task-gates.css`) in the page, so the shell names no sheet of it. The board fetches counts
  first, then nearby column pages; finished work defaults to 24 hours, with
  a week switch and older paging. A feed event that changes what a view lists (a task, a message
  while searching, the counts) reads the counts again 750 ms on, then the columns; the machine's
  work does only when it puts a task in another stage (`MachineWork::places_alike`), not at each
  heartbeat. An answer is dropped only for another view (search, window, connection, board
  version, `board_live::BoardData::view`) or a newer read of the same part, never for an event
  that came while it was read: the events since are replayed onto it, and the refresh reads again
  (app/board-search-slow-to-fill: a loaded machine's work, every second, kept a search's columns
  grey for 5 to 60 s; `tests/browser/board_search.rs` counts a search's reads). Thread history, subtasks, hierarchy groups
  and agents page separately. A reconnect catches up a loaded thread by
  message id before following the feed again. Composer drafts are local to
  their keyed component and saved through `Platform`, never replaced by a
  snapshot. The optional owner offer is pinned to the payload the server showed,
  labelled and posted to `/api/task-action` by the action the offer names.
  It is fetched only near the controls, using the task's recorded branch;
  preparing the landing tool must not hold up the brief or the message box.
  The task page compiles only the timeline's box (`timeline_slot.rs`, its small
  `assets/timeline-slot.css` put in the page as `attach.rs` puts its own): as tall
  as this browser last drew the task's timeline (`project::stored`), with placeholder
  rows, so nothing outside it moves when the timeline comes. On a phone it is at most
  240 px (the owner's), so the brief starts near the page's top, and "Show all N steps"
  grows it; from 900 px, the lanes' width, it grows to the whole timeline once drawn, with
  no "Show all" (the owner's), keeping that height, uncapped, for the next visit
  (`timeline_slot::WIDE`, a typical whole timeline, for a task never drawn there). Near
  the screen it
  asks the shell (`reviewTimeline`) for the timeline, the page kind `timeline`
  (`task_timeline.rs`, `page-timeline`), whose root the shell moves into the box,
  and only a box whose `data-slug` is the task asked for. Boxes are numbered as
  they mount, so the shell drops an ask from a box older than the one it holds or
  from one that closed.
  It reads `/api/task-timeline` with the last answer's tag, again at once when the
  task changes (the box's `revision`) or Try again is pressed, and each `every`
  seconds while the task is open and the page shown; a failed read keeps the
  answer shown, or with none says why in a sentence from the error's message
  (`failed`), never its stack, grows the box to show Try again, and reads again
  a minute on. Dioxus compares keys only in a list and ignores a `key` anywhere
  else, so each read, box and task that must start afresh is the one item of a
  keyed list (`for read in std::iter::once(…)`). The module's pure `rows` and `lanes` lay out the phone's list and the wide
  lanes, both drawn, the lanes' words shown by container queries only at the track
  widths that hold them; `assets/task-timeline.css`, the module's own, shows one
  view by width and keeps the list for assistive technology
  ([TASKS](../../docs/TASKS.md#the-timeline)).
  Briefs and agent messages use the shared `block_view::BlocksView`; owner and
  system messages remain literal text, but for an owner message naming an
  upload, whose blocks the server sends with the words kept literal and the
  files drawn. Task routes use `navigation.rs`, and
  requests use `request.rs` so a stalled save times out with its draft kept.
- **Attach.** Every owner composer (the `+` sheet, asks, a card's editor, a
  task's message box) takes files through `attach.rs`: `use_attach(key)`, the
  form marked `data-attach` with `Attach::zone()`, `AttachControls` inside it,
  and `Attach::compose(text)` for the posted text, which adds the line
  `POST /api/upload` answered for each file (docs/workflow/REVIEW.md,
  "Attaching files"). One document listener per module takes paste, drop and
  the picker's change for the zone holding the event's target, and sends the
  files on that composer's channel; uploads run one at a time, and the chips
  are saved with the draft. The module compiles into the task, document and
  panel kinds only, and brings `assets/attach.css` inside it (the first
  composer to mount puts it in the page): a stylesheet asset would be named in
  the shell's manifest, which every first visit loads, so `/` and `/tasks`
  carry no byte of either.
- **Visibility.** `viewport::NearViewport` uses the shared visibility latch, with a CSS sentinel
  extending 600 px around its placeholder. The latch owns an IntersectionObserver until
  revelation or teardown, and reattaches after props replace its node. Markers include the
  page kind so the persistent panel cannot select a page's sentinel. No `mounted` feature
  is needed. Plain Dioxus action listeners and own-node form reads avoid retaining rich
  browser event converters the entry controls do not use. A slot uses
  `content-visibility: auto` only once no sentinel is left in it, its own or a nested
  slot's (a column's rows): paint containment would clip the look-ahead. A slot that is
  or sits in a list item never does: paint containment would clip the item's
  marker, and layout containment would put it on a line of its own
  (`tests/browser/look_parity/list_and_queue_spacing.rs`). Mounted rows
  and sections keep an intrinsic size; a region keeps its children once revealed so
  scrolling preserves forms and reading position.
- **Loading.** A region is reserved before its read arrives, never left to grow when it
  lands, and drawn meanwhile as a ghost of what it will hold: each box as the real one
  (border, radius, padding, size: a `ghost-*` class shares the real class's rule) with a
  `span.bone` bar at each line of text in it, at that line's height, not one grey slab. A
  card, a row, a message, a column, a page each has its ghost beside its component (`GhostCard`,
  `GhostRow`, `GhostEntry`, `GhostTask`, `GhostDocument`, `GhostQueue`, `GhostBoard`); a
  `NearViewport` takes its `ghost`, else draws the lines of text its height holds
  (`viewport::Skeleton`, `lines_for`), and a picture still to come is its box, filled. Bars
  take the theme's `--skeleton` colour, shimmering in `--skeleton-shine` only under
  `prefers-reduced-motion: no-preference`. A page says once that it loads (one `role=status`
  line, the gates' outline under `aria-hidden` on a task's ghost) and is `aria-busy` while
  its ghost shows, as a column reading its first page and a `Settle` not yet settled are.
  A ghost uses its own classes, never the real ones, so a test's selector never finds it;
  the one placeholder it shares is a task's gates' outline, the section's own until its
  answer, which `GhostTask` draws for the room this browser kept (`task_gates::GhostGates`).
  The board, the queue and a task page draw a return visit from this device's cache (`cache`,
  `main[data-cache=device]`) with no ghost once its store answers; theirs shows on a visit the
  device has no copy for (a first one, a store that opened late or let the page go), and the
  browser tests of their next ghost turn the store off (`NO_STORE` in `tests/browser/main.rs`).
  What a first visit cannot know a ghost keeps from the last one, per project in the
  browser: the board's columns and counts, kept once the server has answered both its counts and
  the machine's work that places its build and test columns (`tasks::SHAPE`, `board_shape`,
  `board_live::keep_shape`; none kept is every column with a card) and the rows of each
  column's first page as they stood in a window as wide, each one's pitch and its rows of chips
  (phases, links, roles), a drawn card's or the place a row still waiting holds (`tasks::CARDS`,
  `kept_cards`, `drawn_rows`; a card's ghost draws a pill per row of chips and the title lines
  its pitch has room for); the queue's rows
  per group (`ROWS` in the React client's `queue.tsx`) and each
  group's height; a task's head, title, facts and brief at the height drawn in a window as wide
  (`viewport::drawn_height`, `remember_drawn`), and its gates' room (`task_gates::KEPT`). A
  task never opened at that width takes the last task's (`recall_height` falls back to the
  newest part of the same kind, `task_gates::latest`); with none, a head with a facts line and
  a title of one line wide, two on a phone, four gates and a brief of seven lines. A brief kept
  shorter than those seven lines draws only the bars its box shows whole (`GhostBrief`). The
  timeline's box takes the height `timeline_slot::reserved` gives it. What is kept is measured
  as it draws, every 100 ms, and kept again when it changed (`tasks::keep_rows`,
  `viewport::remember_drawn`), until no region in it still waits within the sentinel's reach of
  the screen (`within_reach`; for a column, of the board's own scroller: `waits_near`,
  `clip_to`) or every row of a column has drawn (`tasks::keeps_watching`), for
  `viewport::KEEP_MS` (30 s) at most, a bound on the work alone: on a loaded machine or a slow
  phone rows draw many seconds after their data. A
  region still waiting carries `data-pending=true`: a `NearViewport` placeholder, a document's
  section, a column's first page, a task's thread. What a page already knows shows at once
  (a task's head and brief, a document's head and outline); `viewport::Settle` keeps only
  the part still reading (a task's thread and composer, a document's body) hidden under its
  ghost (`cover`) until no such region shows on its first screen, its read fails, or
  `SETTLE_MS` (3 s) passes; a hidden control takes no focus, so a
  browser test waits for `visible(SELECTOR)` before it focuses one. The task board shows its
  heading (one node from ghost to board, so its link keeps focus) and controls at once, each
  column reserving a card's box per row of its first page (`tasks::PAGE`), at the pitch kept
  for its place, else `tasks::ROW`, a typical card (a title of two lines, a row of links and one
  of roles; its ghost draws no role chip in a column no agent has taken up, `tasks::unstaffed`),
  and showing Load more only once its first
  page is in; a row of that page still waiting to be drawn holds what its own card will take,
  its ghost as tall (`tasks::row_card`, `card_height::row_height`). A first read that fails
  with nothing drawn yet (a board not made, say) is said in place of the board's ghost, which
  would wait for ever, in the read's words with a retry (`tasks::UnreadBoard`); the page reads
  again by itself too. A live update that adds or
  removes a row calls `viewport::hold_place(ROWS)` (the shell's `reviewHoldPlace`) before it
  sets its data: once the reader has scrolled the box around what they read or the page, the
  focused field, else the first of ROWS on the screen, stays where it was while the update
  and any placeholder within the sentinel's reach draw, until the reader's own scroll, click
  or key; a reader at the top sees what lands at the top.
  `tests/browser/layout_shift.rs` fails a page whose cold or warm load scores a layout
  shift of 0.05 or more, on a phone or wide. That score sees only elements already drawn
  that move, not a ghost's box against the new element in its place:
  `tests/browser/landmarks.rs` compares each region's place on the ghost and on the page,
  on a first visit and the next, on a board, a queue and tasks shaped as the live ones (a
  later card, a later queue group, the board's lower row, a task opened first and after
  another), and prints each offset as a `LANDMARKS` line; it measures the board and the queue
  again with every `IntersectionObserver` callback 800 ms late. No scrollbar takes room there
  (`NO_SCROLLBARS`), so the kind Chrome draws cannot change every region's width. `tests/browser/skeleton.rs`
  covers the rest.
- **Queue.** `/queue` is the React client's, the first page kind it took over
  ([its guide](../symphony-web/AGENTS.md)): this client builds none of it. Home's preview of the
  queue stays here, in `view.rs` and `live.rs`, until home moves.
- **Reports.** `reports.rs` renders `/api/reports` at `/reports`, the program's `reports`
  kind: a chunk of its own over the runtime, frame and components the program's main module
  holds for the task pages too, so a move between them fetches the other's chunk alone. Its
  style is `assets/reports.css`, which only its kind loads. It reads
  the window this device last chose (`reports-grain`, `reports-days` through `Platform`), else
  the last 24 hours by the hour (`reports::FIRST_WINDOW`), at
  once, on each new choice and every five minutes with the last answer's tag. Four figures sit over five sections of charts, one column on a
  phone and two from 960 px. `reports/chart.rs` draws each chart as one component: bars as
  boxes over a column a period, lines as one SVG whose strokes keep their width, both on one
  axis with its unit; a period no file recorded is hatched and its line broken, never a zero.
  Each entry of a chart's legend is a toggle button (`aria-pressed`) that hides its series on
  that chart alone, kept in every window by its chart's id and name (`reports-hidden`, a key a
  line, through `Platform`): a hidden series leaves the plot, the readout and the table, and the
  axis scales to the series shown (`Scale::of`), so hiding a large one makes the rest larger; a
  period stays hatched only when no series, hidden or not, recorded it.
  A tap or the arrow keys pick a period, the same on every chart, and its readout says every
  value as text; each chart's table holds every period. The series take `--chart-1` to
  `--chart-5`, set at no specificity so a theme's own win, with their own steps in the dark.
  Charts below the screen mount through `NearViewport`. Hours are labelled in UTC.
- **Documents.** `docs_live` reads `/api/document?view=outline`: section keys,
  source positions and anchors, with short card summaries. `viewport::NearViewport`
  uses the shared visibility latch to mount each block once near the screen; a collapsed
  card requests its body only when opened. Mounted blocks request their bodies
  and threads together from `/api/section?format=blocks&keys=JSON_LIST`. Excerpts
  request `format=blocks` too; reads without it retain the legacy HTML/media
  response for compatible API callers. Canonical `/docs/` pages use the client;
  old `/app/docs/` and `/doc/docs/` bookmarks redirect there.
  Only Markdown files, HTML documents and folders are client routes; raw files keep normal links.
  An HTML document (`html_page`, docs/PROJECT_FILE.md "HTML documents") is no fragment: its
  page loads the file whole into one frame whose sandbox allows its scripts alone
  (`html_page::SANDBOX`; never `allow-same-origin`, top navigation, forms or popups), so it runs
  in an opaque origin with no reach into the app's storage, cookies, feed, API or page. The
  document's route is the app's page whatever headers ask for it (a browser sends no fetch
  metadata to a plain-http address but loopback's); the frame, and a Markdown document's mockup
  frame, load the file's address, the route with `?raw` (`symphony_api::file_address`), which the
  server answers with the file under a policy that keeps its loads to its entrypoint and opens no
  connection (`html_document`, `apps/symphony-server/src/media_files.rs`), and a route with no
  such document with its Not found, which the frame shows. The frame is as wide as the page
  and as tall as the window below the header, scrolling inside itself; the page has the header and nothing of the
  Markdown view (outline, cards, notes, editing, versions, cache or feed), and no reads of its own:
  what an HTML document does is the server's and the browser tests', so a port of the documents'
  page carries it over as one frame.
  The feed opts into
  one `document=PATH` and its requested `sections=JSON_LIST`; a stream without
  a document receives no document payloads. Changing the requested keys reopens
  the stream at its current cursor. A body response waits for that cursor and
  never overwrites a newer document change; a reconnect invalidates outstanding
  reads. Drafts and verdict retry ids are saved locally. Busy sections retain
  their markup while their source positions advance; the queued markup applies
  when the form or excerpt is free. The owner's own saved answer on a card
  releases that card (`docs::release`, `docs::held`, `docs::settle`): its mark,
  line and the register's regroup show at once, an open excerpt beside it or
  not. The release lasts until the card shows one more line of the owner's than
  its latest copy had when the save was sent, so an agent's reply the feed
  brings first does not end it. A closed
  draft holds its card only once the card is gone, never the register's order.
  Content is `symphony_api::Block` and `Span`, drawn by
  `block_view::BlocksView { blocks }` and `SpansView { spans }`. Server reads
  prepare media and scoped URLs before `symphony::blocks::parse`; HTML fields
  belong only to that internal pipeline and are excluded from JSON. Links
  carry typed excerpt targets; SVGs are primitive data. Unsupported source
  markup stays visible as an Unknown node with a server warning. `symphony blocks`
  prints a real-document census and checks that the data survives serialization.
- **The page.** `package.sh` writes the shell, whose single history adapter is
  `assets/shell.js`: the page kinds' manifest on its first line (`const modules =
  …;`), then that script minified by the esbuild dx fetched. A kind of the React client's
  names its script and styles and no wasm: the shell imports that script, a module exporting
  `run` and `stop`, in place of the glue's init, and a move between it and a Dioxus kind stops
  one and starts the other as between two Dioxus kinds. Its
  `html[data-navigation]` is `loading` until the shell's navigation finishes,
  then `ready`, or `failed` if the page cannot load. It preserves a scroll made
  after the new page mounts instead of applying its late top reset.
  Compilation leaves `OUT/public` serving its last build;
  packaging assembles beside it and `symphony-build publish-client` atomically exchanges the
  complete directories (macOS/Linux). Failed builds leave the published client
  intact; the next package removes abandoned staging folders. The stage holds its
  build alone, its files named in its `builds.json` by its shell; `publish-client`
  keeps it and the two builds published before it over the folder, newest first in
  the folder's `builds.json`, copying in what the stage lacks and removing every
  other file (`KEEP`, `crates/symphony/src/cli/publish.rs`), so an open page's
  deferred import finds its build two publications on. Once its build is gone, an
  import that fails asks for the page's own shell past the cache: a 404 loads the
  page again, onto the build served, once per build in the tab's storage
  (`gone` in `assets/shell.js`); anything else says the failure with its retry.
  Cached page kinds retain an empty Dioxus root;
  `module.rs` unmounts all page children and awaits cleanup before another
  kind starts, including their feed and event subscriptions. Back and forward
  use this same adapter. The persistent panel remains mounted once opened,
  retaining chat, task form and conversation state across kinds. The initial
  frame stores raw API snapshots until a deferred component needs their
  typed data; activity and pending controls load on interaction, and new
  activity can load the panel to show a toast. No HTML fragments cross this
  boundary. A click on `+` or the bell draws its sheet at once from the shell
  (`#panel-loading`: the frame's own `#quick-sheet` or `#activity-panel`, with a
  status line and no spinner) until the panel mounts or fails. The sheet takes the
  focus (`+` hides at desktop width) and Escape closes it, giving the focus back to
  its opener; its status is filled a frame after the sheet is in the page, so a
  screen reader announces it. The panel's own sheet takes the focus over. A
  select's popup and the picture viewer are drawn by the overlay kind
  (`page-overlay`, `components::deferred::OverlayApp`, mounted in `#overlay-root`),
  never the chat's. The chat, the menus and the timeline are kinds of the program, each a
  chunk the shell names in the manifest (`chunk`) and mounts in its root; a page of the
  program that has them already fetches the chunk alone. A task page's timeline is a
  persistent kind too (`page-timeline`, `task_timeline::TimelineApp`, in `#timeline-root`), but cold:
  never warmed, loaded only once a task page's box asks for it and the page has its
  data and is idle, its root moved into that box and parked hidden at the body's
  end when the box goes. What listens on a module's root, as each module's Dioxus
  does, hears only the events of that module's own part of the page (`assets/shell.js`
  `scope`): Dioxus finds an event's element by a number each module counts on its
  own, so without it, a click in the timeline would run the page's control of the same number.
  The shell requests a module's script, its wasm and a program kind's chunk (each a `fetch`
  preload) and its styles together. Once the first page has its data (the shell
  counts the page's `fetch` reads in flight, and waits until none has been for half
  a second, and after a read that got no answer until one does) and the browser is
  idle, it prefetches the persistent kinds' script,
  wasm, chunk and styles at the lowest priority, nothing evaluated: the chat's first, the
  menus' once the chat's chunk (or the chat's own load, when a click came first) is
  in, so no two share a slow link. A browser asking to save data, or reporting a
  2g link, warms nothing; the browser tests count those prefetches apart from a
  first visit. `index.html` in the crate is dx's build template (`{app_title}`), and
  `base_path = ""` serves the app at `/`. A style sheet is an
  `asset!("/assets/board.css", AssetOptions::css().with_static_head(true))`:
  dx hashes it and links it from the shell's head, if the code keeps the
  constant (`std::hint::black_box(&STYLE)`). The style sheets every kind loads
  are one `shared-<hash>.css`, first in each kind's list of the manifest, and the
  kind's own follow it: a first visit asks for them once, and every kind caches
  the same file. The task timeline's few styles do not narrow it: it is drawn only
  in a task page, which has the shared file already. The server's own pages inline
  `board.css`'s tokens and `frame.css` from copies in its package
  (`apps/symphony-server/src/`), which it carries to crates.io: a change to either
  is copied there too, and `crates/symphony/tests/packages.rs` fails until it is.
- **Routes**, when a slice pays for them: `#[derive(Routable, Clone,
  PartialEq)] enum Route { #[route("/")] Front {} }` and `rsx! {
  Router::<Route> {} }`, one component per variant, named as it is.
- **Tests.** Views render on the host with dioxus-ssr
  (`dioxus_ssr::render_element(rsx! { BoardView { board, now } })`), pure state
  in plain Rust, and `tests/browser/` drives headless Chrome over its DevTools
  protocol against the release build, served as the server serves it.
- **Development.** `just symphony-ui-dev [PORT]` starts an owned loopback API and
  dx 0.7.10 with `serve --web --open false --interactive false`.
  PORT defaults to 0 (an available port); 8765 is refused. Use the printed front
  door URL, which forwards APIs, saves, media and SSE to that checkout's API and
  pages, assets and `/_dioxus` to DX. Host and Origin pass through together, so
  saves keep the production origin check. Ctrl-C stops the API and DX's own
  process groups and waits for their compiler descendants before returning,
  escalating after 3 s if needed. Losing the starter also triggers cleanup.
  DX build outputs stay in `.target/symphony-ui-dev`, isolated from check, lint,
  release builds and shared seeds. The native launcher uses the ordinary tree
  folder. Build tools are installed by `tools.sh` into the common directory as
  for releases.
  Only DX gets `CARGO_INCREMENTAL=1` (the workspace's normal dev profile disables
  it), opt-level 1, no debug info and panic=abort on its `wasm-dev` profile.
  The `devtools` feature is opt-in and enables every page kind in the dev binary.
  `src/dev.rs` mounts the client library directly. RSX and CSS update in place;
  Rust logic edits incrementally rebuild the module. Its page reload, or a
  reconnect, waits while a form, draft or open
  details/dialog is engaged. The API's release hash does not reload this dev
  page. Changes to dependencies, feature sets or static initializers need a
  restart. The release shell, hashed page-kind builds and shared server never
  start this development runtime. Subsecond remains disabled: dx 0.7.10's
  hot-patch parser cannot read wasm-bindgen 0.2.129's exception tags. Updating
  the CLI to a version with [Dioxus #5843](https://github.com/DioxusLabs/dioxus/pull/5843)
  is an owner decision; no patched CLI is installed here.
- **Platforms.** Every platform hook goes through `platform::Platform`; the
  desktop slice adds its implementation and a `desktop` feature. The projects
  of the machine's one server share its origin, so every key the browser
  stores takes the page's project id first (`project::stored`,
  `alpha:review-quick-draft`; the shell's `stored` for the saved theme); a
  server of its own keeps its keys as they are. Every link the client draws, and
  the address of a document's or a card's picture, is written under the project's
  base (`project::href`, `/alpha/task/…`), so a new tab, a copied link or image
  address or a long-press opens what a click does; the shell follows a link under
  the base in place and adds nothing to it, leaves any other to the browser, and
  the project switch's `/` alone leaves the project.
- **Media.** `PictureView(src, caption, thumb)` reserves a square for the board;
  typed document images carry real dimensions and thumbnail URLs from the
  API. The shared visibility latch defers native media shells, gallery rows, document blocks
  and ask threads. Mount `media_live::MediaRuntime` once beside `main`: its
  adapter sizes mounted frames and its component dialog opens
  pictures at `data-full`. A page draws it once it shows media (`media_live::use_activation`),
  and the program's from a chunk of its own (`split::MediaPart`, `media` in the program's map of
  chunks and in no manifest), so the program's main module holds none of it: the task pages, the
  chat on the board and Reports fetch it only when media first shows. A chunk that does not load
  leaves the page as it is and is asked for again by the next media shown: the shell loads the
  page again when its build is gone, and otherwise puts up `#media-error` with a retry that
  shows the media again, taken down once the chunk is in or the page moves on
  (`reviewMediaFailed` and `reviewMediaLoaded` in the program's `assets/chunks.js`, over the
  shell's `reviewShell`, so no other page's first visit carries them). Mockups stay at 1280x800,
  scaled by their card's width, with only `allow-scripts` in their sandbox. Never add
  `allow-same-origin`.
  A file a plugin's renderer draws in a viewer is a `Media::Viewer`, drawn by
  `viewer::FileViewer` from its `symphony_api::Viewer` data alone: the custom
  element (its markup from `Viewer::element_html`, every value escaped), its
  caption, a button per view and the download link. The client holds no
  viewer's code: the module script the viewer names is loaded once, on the first
  mounted stage naming it, and is absent from the shell's preloads; a view's
  button sets the attributes it lists on the element and calls the method it
  names. A plugin's pages are the
  `ProjectView`'s `nav` items the served page inlines (`PROJECT_ELEMENT`): each
  reads its document through `DocumentPage`, as a `/shelf` page reads
  `docs/SHELF.md`. A `Media::Gallery` holds its items (name, file `src`,
  `poster`), served under the plugin's route, `/x/shelf/`, and the viewer they
  open in. Its grid has only small pictures until an item opens; one selected
  item is mounted at a time, and closing it unmounts the viewer.
  The server warms read snapshots before announcing its URL, and a gallery
  plugin warms the selected checkout's gallery cache at startup and when a
  folder its watch names changes. Startup logs separate gallery, document and
  board time; requests never walk model history or other checkouts to build
  the gallery.
  `DocumentPicture(image: symphony_api::Image)` preserves typed document image
  dimensions, presentation and `data-full`; until its latch opens its `img` holds
  the empty picture `media::WAITING` as its src, never none, so it is a sized picture
  from its first frame and the latch only changes that node's src, keeping a keyboard
  focus on it. A src-less img is drawn as its alt text, and Chrome rebuilds its box
  when a src comes: a rebuild asked for in the frame an ancestor slot takes
  `content-visibility: auto` is lost, so the picture never loads (a folded card
  opened by a click). The picture viewer skips a waiting img. `MediaView(media)` renders native
  media from either `Block::Media` or `Span::Media`.
  `asks::Asks(path, scope, render_blocks)` reads and posts ephemeral document
  questions and follow-ups. Key both pages by scope and path when navigating so
  their tasks end with the old page. Ask reads request `format=blocks`; the
  `render_blocks: Callback<Vec<Block>, Element>` prop supplies the shared
  document renderer for answers. No rendered HTML is sent on that path.
  A document's questions and the `+` sheet's chat read again when the page's feed
  says so (`frame::use_asks_heard`): its `asks` region names a moved thread (by id
  and document path) or a new chat recipient, or a stream opens again. The page
  module forwards that region to every module as a `review:asks` event; a read
  that fails tries again every 3 s until one answers, and none polls. A read the
  next one replaces aborts its fetch (`media_live/requests.rs`): a fetch whose
  body the page never reads stays in flight in Chrome's eyes for as long as the
  page is open, and `symphony shot` waits on it.
  The media style is `assets/media.css`; the browser adapter uses web-sys and
  keeps every observer and listener owned by the mounted runtime.
