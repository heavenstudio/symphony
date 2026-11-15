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
  program, the one kind dx builds since every other moved to the React client, build the React
  client's kinds (`web`: one Vite build of `apps/symphony-web`, home, the queue, Reports, the
  documents, the task board, the agents page and a task's page, [its
  guide](../symphony-web/AGENTS.md)),
  then assemble their content-hashed assets with `package.sh`, which refuses a kind both
  clients build. Naming a kind (`program` among
  them) rebuilds it against the other kinds already in that output folder. The program
  (`page-program`, `src/split.rs`; [its card](../../docs/DECISIONS.md#client-shared-runtime)) is one
  `dx bundle --wasm-split` of the chat and the menus every page mounts, no page kind: one
  script and main module, and a chunk for each and one for the media
  layer (no kind: the chat loads it once it shows media, Media below), which
  `symphony-build split-repair` puts right after dx
  (crates/symphony/src/cli/commands/split_repair.rs)
  and `assets/chunks.js` loads (`window.reviewChunk`), rejecting a chunk that does not load so
  the shell's `gone` state, never a page left waiting, follows. The shell awaits a kind's chunk
  before it stops the page shown, which stays until the chunk is in, and under the shell's
  notice when it does not load. A chunk mounts its kind's root
  in the one instance (a `VirtualDom` per root, `module.rs`). The
  shell's manifest names each program kind's script, wasm, `chunk` and styles; package.sh gives
  each the styles the program's page links in its head (dx's static head: the frame's, the board's,
  the pictures' and the controls every page draws; no other Dioxus kind is left to load them too)
  and its own by name (for the chat and the menus the CSS modules, `style-`, that their code puts
  in the page as it draws), and refuses a style no kind takes. It publishes no shared chunk (`chunk_0_split`, which dx's splitter writes and
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
- **The feed.** The web client's pages follow `crates/symphony-api`'s feed contract (its
  `src/feed.ts` and `src/live.tsx`; this client follows no feed). A stream
  is lost when it errs, when it says nothing (not even the server's ping) for
  two and a half of the board's `ping_ms` (at most 5 s, so 12.5 s: the owner
  reads the board from a phone over the tailnet, where a stream goes silent
  but open on every network change), or when it sends an event the client
  cannot read. Then the client closes it, marks the page stale
  (`main[data-feed=stale]`, a notice), waits 1 s doubling to 30 s (the waits
  start again only after a stream held 30 s), fetches the board again (it
  replaces the one shown whatever its version) and opens a new stream from it;
  the board is live again once that stream opens. It never resumes by
  `Last-Event-ID`. The web client follows it on a WebSocket (`src/feed.ts`): an event stream
  holds one of the browser's six connections to the server (HTTP/1.1) for as long as it is open,
  and six tabs of one server on streams would leave a seventh none for its reads, where the
  browser counts WebSockets apart (`tests/browser/tabs.rs`). A page that goes away closes its
  stream: on
  `pagehide`, which every browser fires as a page is left, for good or into
  its back/forward cache, and on Chrome's `freeze` of a background tab. A kept
  page's open stream would hold its socket, and the server its subscription, for as long as it is
  kept. Once back
  (`pageshow`, `resume`) it reads at once, with no wait, from a fresh board as
  after a lost stream: the server it left may have restarted meanwhile. Hiding
  alone (another tab in front) closes nothing: the home page notifies from the
  background. A board
  or a `client` change naming another build than the
  page's own module script loads the page again, once per build
  (`reloadFor` in the web client's `src/state.ts`, remembered in `sessionStorage`).
- **Cache.** What a page drew is kept on the device so a return visit draws at once
  ([the client cache](../../docs/analysis/client-cache.md)). The store is `assets/cache.js`
  (`window.symphonyCache`), which `package.sh` puts first in the shell's one script: every page
  kind shares it and no kind's wasm carries IndexedDB bindings. It adds 1,433 bytes to each
  kind's first visit and none to a return. The React client's `src/cache.ts` and
  `src/cache-document.ts` reach it (this client keeps nothing there): an
  `Item` is what a visit opens (`board(scope)`, `task(scope, slug)`, `document(path,
  entrypoint)`) and goes whole; its parts are the reads its page drew, each kept with the
  server's tag (`ETag`) and the time it was kept (`Kept`, never later than the page's clock).
  One IndexedDB database for the origin, keyed by project. A visit (`visitLater`) once per page open
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
  the page cannot read is forgotten (`readKept`). Without IndexedDB, or in the development
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
- **A return visit.** The React client's home, queue and task board each draw what they kept of
  `Item::board(scope)` before the server answers, then check it. `/api/board` (part `board`) is
  drawn and still read fresh, with no tag, since its `version` starts the feed; nothing else is
  read before it. Every other kept read then goes at once with `If-None-Match` naming the kept
  copy's tag and `cache: "no-store"` (`getIf`, the React client's `src/request.ts`): a `304` is the
  copy, a `200` replaces it on the page and on the device. The queue keeps `/api/queue` (`queue`,
  `QUEUE` in `src/cache.ts`). The task board keeps its
  default view only (no search, the last 24 hours, stages by the machine's activity): the counts
  (`counts`), each column's first page (`column:STATUS`) and the machine's work (`work`), never a
  search, another window or a Load more page; a column drawn from the device is checked at once,
  not after its 250 ms. Its listings are read with `work=0`, which places tasks by the machine's
  Build and Test work as before and leaves that work out of the body, whose heartbeats would
  otherwise change every listing's tag; the work is read apart (`/api/machine-work`) and followed
  by the feed. The header's reads are never kept. A check that fails is asked again, waiting
  longer each time (`Backoff`, the React client's `src/state.ts`), until it answers: the counts, each column drawn from the
  device and the work (`LiveBoard.check` in the React client's `src/boardlive.tsx`), the queue
  (its `src/queue.tsx`); the feed's `machine-work`
  event, which carries the whole work, answers for the work's read. Once every column drawn from
  the device has answered, a task it drew that no fresh answer (a column, a feed `task` event)
  listed is dropped, whichever stage the work places it in by then. `Freshness` (`src/cache.ts`) says what
  a page drawn from the device shows: `main[data-feed=stale][data-cache=device]` and the notice
  "Shown from this device, saved 14:32. Checking for changes…" (the oldest copy drawn), "Not
  reachable: …" while a read fails and the feed is not open, cleared once every drawn read has
  answered and the feed is open. A task's page, the React client's since it moved ([its
  guide](../symphony-web/AGENTS.md#a-tasks-page)), keeps `taskItem(scope, slug)` (`src/cache.ts`
  there): its page (`page`, `/api/task-page`) and its thread's newest page (`thread`,
  `/api/task-history` without `before`). Opened, it counts a visit and draws them at once unless
  it shows that task already. Both are checked together, each asked again until it answers
  (`LiveTask`, `src/task-live.ts` there): once `/api/board` is read on the page's first open, at
  once for a task opened in place from another. A thread answered `200` replaces the messages the
  device drew and keeps those the feed brought meanwhile (`newest` there). Until the
  page's check answers, a page drawn from the device reads no other part of the task: the
  newest messages when no thread was kept, the subtasks and earlier messages wait for it, and
  the feed replays what changed after the board's version. The landing offer, the gates and the
  timeline, which it never keeps, it reads at once. A task the server no longer has (`404`) says so and
  is forgotten. Never kept: earlier thread pages, the landing offer (`/api/task-offer`, read on
  every visit), the subtasks' further pages and the brief's later chunks. `/agents`, the React
  client's, draws nothing from the device.
  The documents are the React client's since they moved ([its guide](../symphony-web/AGENTS.md)),
  which keeps them in the same store under the same names, so a copy kept by either reads back
  on the other. A document keeps `documentItem(path, false)` (`src/cache-document.ts` there): its
  outline (`outline`, `/api/document?view=outline`) and the bodies of the sections it drew
  (`sections`, one map by key, each with its card and thread and the validator it was taken
  under, `KeptBody`). A section's validator is its outline `hash` and, for a card, its thread's
  hash (`DocumentOutline::thread_hashes`: a thread is no part of its section's hash); a section
  showing a difference from main is never kept. Opened, the page counts a visit and draws the
  outline and every body current by it at once (`fromDevice`, `src/document-live.ts`), then
  reads the outline naming its copy's tag (`getDocument`, `src/request.ts`). The server tags a document read
  without the feed's cursor, so an unchanged document is a `304` however the board moved, and
  names the cursor in a header (`symphony_api::VERSION_HEADER`), on the `304` too, from which the
  feed opens. The bodies the server's outline names alike stay and are read no more
  (`Bodies`), on a reconnect as on a return; the others are read as the page
  wants them, and no body is read before the outline answers. A body the feed changed is kept
  again only once a later outline names it. An entrypoint's own page, the main checkout's, is kept
  pinned (`documentItem("docs/", true)`): its folder (`folder`, a `304` while unchanged, the
  server's tag leaving out its intro's cursor) and its intro's outline and bodies in the one item,
  so it outlives the documents visited once. No other folder page is kept. A document the server no
  longer has says so and is forgotten. Never kept: versions, diffs, asks and the history.
  `STALE` (the React client's `src/shared.ts`), which `PageNotices` (its `src/live.tsx`) sets
  while that notice or the reconnecting one
  shows, holds the owner's actions (verdicts, messages, comments, questions, priority, cancel,
  landing, Later):
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
  (`split`, the React client's `src/cache-document.ts`). Never on save-data or a 3g or slower link, never a link that acts, a
  search, the page itself, a folder or another checkout's page. The stand-in logs a read of the
  API marked as a prefetch apart from the page's own (`Site::read_ahead`, `asked_ahead`;
  `requests`, `targets` and `answers` leave it out), so a test counting a page's reads is not
  raced by the reads ahead; `tests/browser/cache_ahead.rs` covers them.
- **Budget.** `/` and `/tasks` each have a 300 KB first-visit gzip budget and
  a 20 KB return-visit budget (`tests/browser/`, restored after the port;
  both the owner's, the first-visit cap raised by the owner to 300 KB, and
  never loosened by an agent). Home's first visit, the web client's since it moved, is held
  to 125,000 bytes (`HOME_FIRST_VISIT_BYTES`), the task board's, the same way, to 150,000
  (`TASKS_FIRST_VISIT_BYTES`) and a task's page's to 160,000 (`TASK_FIRST_VISIT_BYTES`), each
  lowered by its move, never raised. A visit counts each response the page loaded at what Chrome's
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
  On the fixture (`entry_pages_fit_the_first_visit_budget_and_deferred_kinds_are_reported` prints
  each page's requests) a first visit to `/` takes 112,577 bytes, `/tasks` 120,489, `/agents`
  115,526, `/queue` 108,096, `/reports` 108,512, `/docs/` 138,845, a document 140,506 and a task's
  page 142,260, and a return that finds what it kept unchanged 2,031, 2,442, 2,307, 2,272, 765,
  574, 1,677 and 2,285. A first visit to a kind of the web client is its entry and its sheets, the
  chunk every kind shares (React and the frame) and the pieces it shares with some of the others
  (the board's feed and keep; the card and the task readers of the board and the agents page; the
  media layer's chunk and sheet, home's, the documents' and a task's page's), beside the HTML, the
  shell and the page's reads; a move to another kind takes its entry and the pieces no kind
  before it took (`tests/browser/preload.rs`). A task's page's timeline, a chunk of its own, comes
  once the page has drawn and is idle: 12,431 bytes after its first visit. The reads ahead start
  only once the page's own reads have answered (`cache_ahead.rs`) and go marked as prefetches, so
  the budget counts them outside the first visit, with the warmed modules (`prefetched_bytes`).
  Once a page has its data and is idle, the shell warms the chat's and the menus' files, the
  program's script and main module and their chunks: about 584 KB outside home's first visit,
  and the chat's first open transfers nothing more. The program's main module keeps the
  keyboard's key names, `document::eval` with serde_json, and float formatting: dioxus-web makes
  at launch the event converter and the document that reach them, and the splitter keeps there
  every function of a table the main module reaches
  ([the analysis](../../docs/analysis/client-shared-runtime.md#building-a-in-order)). How each
  change of the client moved these figures is [on record](../../docs/history/client-first-visits.md).
  Each module pays for its own compiled framework, the program once for all its kinds;
  later visits reuse cached imports. The home budget test
  is independent of the canonical-entry test, which gives every kind a fresh
  browser profile and reports the complete published output.
  Measure loaded resources, including API data, rather than summing every
  deferred module or an older build's file kept in the published output.
  The 300,000 and 20,000 byte limits are the owner's; a lower first-visit
  line needs an architectural change the owner approves, and the first-visit
  analysis below prices those the owner left to revisit.
  `cargo symphony-ui size OUT NAME...` (`probes/size.sh`) builds a size probe as
  the release builds a page kind (no page kind is built on its own since the last
  one moved to the React client) and names a copy for
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
- **Task views.** The task board (`/tasks`) and a task's page (`/task/SLUG`, with its gates and
  its timeline) are the React client's, the fifth and sixth page kinds it took over: this client
  builds none of them. `src/board.tsx` draws the board's
  columns and groups, `src/boardlive.tsx` owns its summary-only counts, columns and deltas, and
  its card is `src/taskcard.tsx`; the agents page (`/agents`) is the seventh, `src/agents.tsx`
  over `src/agents-live.ts`, its cards the board's ([its guide](../symphony-web/AGENTS.md)); a
  task's page is `src/task-page.tsx` over `src/task-live.ts`, its gates `src/gates.tsx`, its
  timeline `src/timeline.tsx` in its box `src/timeline-slot.tsx` ([its
  guide](../symphony-web/AGENTS.md#a-tasks-page); the gates' and the timeline's contract,
  [TASKS](../../docs/TASKS.md#gates) and [the timeline](../../docs/TASKS.md#the-timeline)). In
  the program (the chat and the menus, which share its runtime), a read dropped before its answer
  aborts its fetch (`request.rs`, `page-program`); a post never does. The board fetches counts
  first, then nearby column pages; finished work defaults to 24 hours, with
  a week switch and older paging. A feed event that changes what a view lists (a task, a message
  while searching, the counts) reads the counts again 750 ms on, then the columns; the machine's
  work does only when it puts a task in another stage (`MachineWork::places_alike`), not at each
  heartbeat. An answer is dropped only for another view (search, window, connection, board
  version, `BoardData.view` in `src/boardlive.tsx`) or a newer read of the same part, never for an event
  that came while it was read: the events since are replayed onto it, and the refresh reads again
  (app/board-search-slow-to-fill: a loaded machine's work, every second, kept a search's columns
  grey for 5 to 60 s; `tests/browser/board_search.rs` counts a search's reads). Composer drafts are local to
  their keyed component and saved through `Platform`, never replaced by a
  snapshot. Dioxus compares keys only in a list and ignores a `key` anywhere
  else, so a read or a view that must start afresh is the one item of a
  keyed list (`for read in std::iter::once(…)`). Routes use `navigation.rs`, and
  requests use `request.rs` so a stalled save times out with its draft kept.
- **Attach.** This client's owner composers, the `+` sheet's chat and new task, take files
  through `attach.rs` (the React client's, asks, a card's editor and a task's message box, through
  its `src/attach.tsx`, keeping the same rows): `use_attach(key)`, the
  form marked `data-attach` with `Attach::zone()`, `AttachControls` inside it,
  and `Attach::compose(text)` for the posted text, which adds the line
  `POST /api/upload` answered for each file (docs/workflow/REVIEW.md,
  "Attaching files"). One document listener per module takes paste, drop and
  the picker's change for the zone holding the event's target, and sends the
  files on that composer's channel; uploads run one at a time, and the chips
  are saved with the draft. The module compiles into the panel kind only (`page-panel`),
  and brings `assets/attach.css` inside it (the first
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
  `prefers-reduced-motion: no-preference`, and only in a ghost that waits for its read (a page's
  `Settle` cover, a column reading its first page, a section, a thread): a `NearViewport`
  placeholder waits only to come within its sentinel's reach, where its content takes its place,
  so its bars are still (`.viewport-placeholder` in `board.css`). Chrome runs an animation off the
  screen every frame, under `content-visibility` too, so a page at rest animates nothing
  (`tests/browser/skeleton.rs` counts `document.getAnimations()`). A page says once that it
  loads (one `role=status` line, the gates' outline under `aria-hidden` on a task's ghost) and is
  `aria-busy` while its ghost shows, as a column reading its first page and a `Settle` not yet
  settled are.
  A ghost uses its own classes, never the real ones, so a test's selector never finds it;
  the one placeholder it shares is a task's gates' outline, the section's own until its
  answer, which `GhostTask` draws for the room this browser kept (`GhostGates`, the React
  client's `src/gates.tsx`).
  The board, the queue and a task page draw a return visit from this device's cache (`cache`,
  `main[data-cache=device]`) with no ghost once its store answers; theirs shows on a visit the
  device has no copy for (a first one, a store that opened late or let the page go), and the
  browser tests of their next ghost turn the store off (`NO_STORE` in `tests/browser/main.rs`).
  What a first visit cannot know a ghost keeps from the last one, per project in the
  browser: the board's columns and counts, kept once the server has answered both its counts and
  the machine's work that places its build and test columns (`SHAPE`, `boardShape` and
  `keepShape` in the React client's `src/board.tsx`; none kept is every column with a card) and
  the rows of each column's first page as they stood in a window as wide, each one's pitch and
  its rows of chips (phases, links, roles), a drawn card's or the place a row still waiting holds
  (`CARDS`, `keptCards`, `drawnRows` there; a card's ghost draws a pill per row of chips and the
  title lines its pitch has room for); the queue's rows
  per group (`ROWS` in the React client's `queue.tsx`) and each
  group's height; a task's head, title, facts and brief at the height drawn in a window as wide
  (`drawnHeight`, `rememberDrawn` in the React client's `src/viewport.tsx`), and its gates' room
  (`kept`, `remember` in its `src/gates.tsx`). A
  task never opened at that width takes the last task's (`recallHeight` falls back to the
  newest part of the same kind); with none, a head with a facts line and
  a title of one line wide, two on a phone, four gates and a brief of seven lines. A brief kept
  shorter than those seven lines draws only the bars its box shows whole (`GhostBrief`,
  `src/task-page.tsx`). The timeline's box takes the height `reserved` gives it
  (`src/timeline-slot.tsx`). What is kept is measured
  as it draws, every 100 ms, and kept again when it changed (the board's `keepRows`, the task
  page's `rememberDrawn`), until no region in it still waits within the sentinel's reach of
  the screen (the React client's `waitsNear`, for a column of the board's own scroller) or
  every row of a column has drawn (the board's `keepsWatching`), for
  `KEEP_MS` (30 s) at most, a bound on the work alone: on a loaded machine or a slow
  phone rows draw many seconds after their data. A
  region still waiting carries `data-pending=true`: a `NearViewport` placeholder, a document's
  section, a column's first page, a task's thread. What a page already knows shows at once
  (a task's head and brief, a document's head and outline); `Settle` (the React client's
  `src/viewport.tsx`) keeps only
  the part still reading (a task's thread and composer, a document's body) hidden under its
  ghost (`cover`) until no such region shows on its first screen, its read fails, or
  `SETTLE_MS` (3 s) passes; a hidden control takes no focus, so a
  browser test waits for `visible(SELECTOR)` before it focuses one. The task board shows its
  heading (one node from ghost to board, so its link keeps focus) and controls at once, each
  column reserving a card's box per row of its first page (`PAGE` in `src/board.tsx`), at the
  pitch kept for its place, else `ROW`, a typical card (a title of two lines, a row of links and
  one of roles; its ghost draws no role chip in a column no agent has taken up, `unstaffed`),
  and showing Load more only once its first
  page is in; a row of that page still waiting to be drawn holds what its own card will take,
  its ghost as tall (`rowCard`, `rowHeight` in `src/cardheight.ts`). A first read that fails
  with nothing drawn yet (a board not made, say) is said in place of the board's ghost, which
  would wait for ever, in the read's words with a retry (`UnreadBoard`); the page reads
  again by itself too. A live update that adds or
  removes a row calls `holdPlace(ROWS)` (the React client's `src/shared.ts`, the shell's
  `reviewHoldPlace`) before it
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
  ([its guide](../symphony-web/AGENTS.md)): this client builds none of it.
- **Reports.** `/reports` is the React client's, the second page kind it took over
  ([its guide](../symphony-web/AGENTS.md#reports)): this client builds none of it, and the
  program has no chunk of it.
- **Home.** Home (`/`, the kind `review`) is the React client's, the third page kind it took
  over ([its guide](../symphony-web/AGENTS.md)): this client builds none of it. Its media layer
  (frames scaled, a viewer's script loaded, its view buttons, the picture viewer's opener) is
  the web client's own (`media-layer.ts`); the picture viewer it opens is still the overlay
  module's here.
- **Documents.** The documents (`/docs/`, each document, folder and HTML document an
  entrypoint serves, the kind `docs`) are the React client's, the fourth page kind it took over
  ([its guide](../symphony-web/AGENTS.md#the-documents)): this client builds none of them, and
  its document page's sources (`docs.rs`, `docs_live.rs`, `html_page.rs`, `asks.rs`,
  `postpone_live.rs`, the `page-docs` feature) left with them. What follows is the page's
  contract, which the React page keeps, its code named there.
  `document-live.ts` reads `/api/document?view=outline`: section keys,
  source positions and anchors, with short card summaries. `NearViewport`
  mounts each block once near the screen; a collapsed
  card requests its body only when opened. Mounted blocks request their bodies
  and threads together from `/api/section?format=blocks&keys=JSON_LIST`. Excerpts
  request `format=blocks` too; reads without it retain the legacy HTML/media
  response for compatible API callers. Canonical `/docs/` pages use the client;
  old `/app/docs/` and `/doc/docs/` bookmarks redirect there.
  Only Markdown files, HTML documents and folders are client routes; raw files keep normal links.
  An HTML document (`HtmlPage`, docs/PROJECT_FILE.md "HTML documents") is no fragment: its
  page loads the file whole into one frame whose sandbox allows its scripts alone
  (`SANDBOX`, `document-page.tsx`; never `allow-same-origin`, top navigation, forms or popups), so it runs
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
  releases that card (`release`, `held`, `settle`, the React client's `document.ts`): its mark,
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
  names its script, the chunks that script imports (`imports`) and its styles, and no wasm: the
  shell imports that script, a module exporting `run` and `stop`, in place of the glue's init,
  with a module preload of each chunk it has not asked for yet beside it, and a move between it and a Dioxus kind stops
  one and starts the other as between two Dioxus kinds. Its
  `html[data-navigation]` is `loading` until the shell's navigation finishes,
  then `ready`, or `failed` if the page cannot load. It preserves a scroll made
  after the new page mounts instead of applying its late top reset. A scroll to the
  element the address names, its own or a web kind's (`route.ts`), is held there while
  the page draws around it (`land`): the sections a landing brings near draw after
  it, often taller than their ghosts, and the browser's own scroll anchoring holds nothing
  under a page's ghost (`.settle`). A landing is one hold for each navigation. It ends
  when it goes quiet (nothing moved and nothing waiting or busy within reach for four
  frames, or 1800 at most), at the reader's input that scrolls (a wheel, a touch that
  moves, a press on the page's scrollbar, a scroll key; not a click, a tap, a selection
  or another key) and at any scroll that moves the page and the element alike (find in
  the page, a focus, a script). Only the next navigation lands again: a later call (a
  section's read in, an outline read again) does nothing once it has ended.
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
  never the chat's. The chat and the menus are kinds of the program, each a
  chunk the shell names in the manifest (`chunk`) and mounts in its root; a page of the
  program that has them already fetches the chunk alone. A task's timeline, once a cold
  persistent kind of the program moved into its page's box, is the React client's, a chunk its
  box imports ([its guide](../symphony-web/AGENTS.md#a-tasks-page)). What listens on a module's
  root, as each module's Dioxus does, hears only the events of that module's own part of the
  page (`assets/shell.js` `scope`): Dioxus finds an event's element by a number each module
  counts on its own, so without it, a click in a root placed inside another's would run the
  outer page's control of the same number.
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
  the same file. The pictures' styles (`media-`), which
  every kind of this client loads, stay out of it: the React client's kinds take the shared file
  and their own, home, the documents and a task's page the media layer's copy of them, and the
  queue, Reports and the task board, which show no picture, none. The server's own pages inline
  `board.css`'s tokens and `frame.css` from copies in its package
  (`apps/symphony-server/src/`), which it carries to crates.io: a change to either
  is copied there too, and `crates/symphony/tests/packages.rs` fails until it is.
- **Routes**, when a slice pays for them: `#[derive(Routable, Clone,
  PartialEq)] enum Route { #[route("/")] Front {} }` and `rsx! {
  Router::<Route> {} }`, one component per variant, named as it is.
- **Tests.** Views render on the host with dioxus-ssr
  (`dioxus_ssr::render_element(rsx! { BlocksView { blocks } })`), pure state
  in plain Rust, and `tests/browser/` drives headless Chrome over its DevTools
  protocol against the release build, served as the server serves it.
- **Development.** The web client's pages are Vite's dev server's (`just symphony-ui-dev SERVER
  [PORT]`, [its guide](../symphony-web/AGENTS.md#the-dev-server)), over a server of the
  developer's own whose build serves the program's kinds. This client's dx front door
  (`apps/symphony-server/src/development.rs`) stays for the consuming project, whose host's own
  `symphony-ui-dev` calls `development::main` (`apps/symphony-server/tests/consumer_api.rs`) and
  whose recipe runs `cargo symphony-ui dev` from a Symphony checkout: it leaves with this client,
  under a consumer release plan. Here it runs as `PATH="$PWD/apps/symphony-ui/bin:$PATH"
  tools/cargo.sh symphony-ui dev [PORT]` (`dev.sh`), which starts an owned loopback API and
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
  The `devtools` feature is opt-in and enables the chat's and the menus' kinds in the dev
  binary. `src/dev.rs` mounts the client library directly. RSX and CSS update in place;
  Rust logic edits incrementally rebuild the module. The API's release hash does not reload
  this dev page. Changes to dependencies, feature sets or static initializers need a
  restart. The release shell, hashed page-kind builds and shared server never
  start this development runtime. Subsecond remains disabled: dx 0.7.10's
  hot-patch parser cannot read wasm-bindgen 0.2.129's exception tags. Updating
  the CLI to a version with [Dioxus #5843](https://github.com/DioxusLabs/dioxus/pull/5843)
  is an owner decision; no patched CLI is installed here.
  The development runtime draws the Dioxus kinds alone: the web client's pages (`/`, `/queue`,
  `/reports`, `/tasks`, `/agents`, `/docs/` and the documents, a task's page) are no part of it:
  it draws the chat and the menus alone. The web client's dev server serves those pages with hot
  reload ([its guide](../symphony-web/AGENTS.md), `just symphony-ui-dev`).
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
- **Media.** Typed document images carry real dimensions and thumbnail URLs from the
  API (home's pictures, which reserve a square, are the web client's). The shared visibility latch defers native media shells and gallery rows. Mount `media_live::MediaRuntime` once beside `main`: its
  adapter sizes mounted frames and its component dialog opens
  pictures at `data-full`. A page draws it once it shows media (`media_live::use_activation`),
  and the program's from a chunk of its own (`split::MediaPart`, `media` in the program's map of
  chunks and in no manifest), so the program's main module holds none of it: `/agents` and the
  chat fetch it only when media first shows. A chunk that does not load
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
  A document's questions and follow-ups are the React client's (`asks.tsx`), which reads them
  with `format=blocks`, no rendered HTML on that path.
  A document's questions and the `+` sheet's chat read again when the page's feed
  says so (the chat by `frame::use_asks_heard`): its `asks` region names a moved thread (by id
  and document path) or a new chat recipient, or a stream opens, first or after a loss. A
  document page that opens its stream again for other sections, from the cursor of the last
  event it heard, missed nothing, and reads nothing again (`followDocument`). The page
  module forwards that region to every module as a `review:asks` event; a read
  that fails tries again every 3 s until one answers, and none polls. A document's
  question read the next one replaces aborts its fetch (`getJson`'s signal): a fetch whose
  body the page never reads stays in flight in Chrome's eyes for as long as the
  page is open, and `symphony shot` waits on it.
  The media style is `assets/media.css`; the browser adapter uses web-sys and
  keeps every observer and listener owned by the mounted runtime.
