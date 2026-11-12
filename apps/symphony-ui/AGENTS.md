# apps/symphony-ui/

The workspace, the cargo wrapper and the toolchain pin: [the Rust section of
the root guide](../../AGENTS.md#rust).

`apps/symphony-ui` is one Dioxus app (the owner: "I want to experiment more with
dioxus, so let's switch to it"), the browser now and the desktop window later,
from the same code. It reads the server's JSON API, whose wire types are
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
this workspace. It builds the real release client unless `SF_SYMPHONY_UI_DIST`
explicitly names a built distribution. Install Chrome or name its executable
with `SF_CHROME`; CI requires it. The harness owns each browser's temporary
profile, process group and shutdown, and checks keyboard interaction, page
geometry, stream recovery and transfer budgets without a display server.
A client change runs the browser tests of the pages and controls it touched, by
filter after `--` ([CHECKS](../../docs/CHECKS.md#the-browser-target-by-page)),
never the whole target, which the nightly runs.

- **Build.** `just symphony-ui-dist [OUT] [KIND]` (`build/symphony-ui`, the page in its
  `public/`): cargo's subcommand `symphony-ui dist OUT` (`bin/cargo-symphony-ui`,
  which the recipe puts on `PATH`; the browser tests run it too) runs
  `bundle.sh OUT` through the cargo wrapper: `dx bundle --web --release
  --debug-symbols false`, on the root manifest's `wasm-release` profile
  (opt-level z, fat LTO, one codegen unit, abort on panic) and dx's wasm-opt at
  `-Oz` (`Dioxus.toml`). `KIND` defaults to `all`: independently compile the
  board, task/detail, document, review-home, queue, deferred panel, overlay and task timeline modules, then
  assemble their content-hashed assets with `package.sh`. Naming a kind rebuilds
  it against the other kinds already in that output folder. Dioxus's native
  splitter retained shared exports and increased the measured initial payload;
  these builds use the page-kind fallback. Without `--debug-symbols false` dx 0.7 emits 260 KB of
  DWARF. dx and wasm-bindgen (the lockfile's version) are built once per
  machine into `sf-cargo/` by `tools.sh`, which bundle.sh runs when one is
  missing. A new tree's folder starts from a seed another tree made, its
  wasm32 builds among it. The wrapper clears workspace crates from every seeded
  target/profile and records that cleanup for `wasm-fresh.sh`. Without a matching
  record, that guard cleans both wasm32 profiles (`dev`, `wasm-release`); its
  record stays inside the folder. `lint-rs` also runs clippy for
  wasm32 over the client (`cargo symphony-ui clippy`): the host build never
  compiles its browser half (`live.rs`, `platform::Browser`).
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
- **Budget.** Both `/` and `/tasks` have a 300 KB first-visit gzip budget and
  a 20 KB return-visit budget (`tests/browser/`, restored after the port;
  the owner's, never loosened by an agent; the first-visit cap is the owner's
  of 2026-11-11: "lets increase the budget to 300"). A visit counts each response the page loaded at what Chrome's
  network events say it took on the wire, gzip body and response head: the
  HTML, the shell, the module's glue and WASM, the style sheets, API replies
  (refusals too) and the live feed's head and content messages, as the page
  reads them. The stand-in server sends the build as the server does, gzipped
  at the best level by the first request for each file and kept for the test
  run, and the page and API replies at the default level
  (`tests/browser/site.rs`): a start reads the shell alone, and a run gzips
  the files its pages ask for, the whole published folder, retained hashes
  and all, only for the canonical-entry test's report.
  It leaves out document-content images, browser-cache hits and
  the feed's keep-alive pings, which arrive every `ping_ms` for as long as the
  page is open and are reported beside the total (the owner's, 2026-11-11).
  Other page kinds are reported separately for review.
  On the fixture a first visit to `/tasks` takes 256,281 bytes, 43,719 under the
  cap: the board WASM 226,456, its glue 10,831, the shared style sheet 10,015,
  the shell 5,203, the HTML 1,143, API replies 2,548 and the feed's head and
  opening comment 85. `/` takes 225,126; cached returns take 1,981 and 2,713.
  The same run reports `/agents` at 488,668, `/docs/` at 465,525, `/queue` at
  240,436, a task page at 489,214 and a document at 467,678. A task page's
  timeline, its own module, comes once the page has drawn and is idle: 202,142
  bytes outside its first visit (its WASM 186,092, glue 12,344, style sheet 2,603
  and first read 1,103), and `/agents` never loads it. Once a page has its data and is
  idle, the shell warms the chat's and the menus' modules, 746,062 bytes outside
  its first visit, and the chat's first open transfers nothing more. Each kind pays for its own
  compiled framework; later visits reuse cached imports. The home budget test
  is independent of the canonical-entry test, which gives every kind a fresh
  browser profile and reports the complete published output.
  Measure loaded resources, including API data, rather than summing every
  deferred module or retained historical asset hash in the published output.
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
  tasks module alone, a read dropped before its answer aborts its fetch
  (`request.rs`, `page-tasks`), so every other module's transport is unchanged; a
  post never does. As attach.rs does, it puts its own style
  (`assets/task-gates.css`) in the page, so the shell names no sheet of it. The board fetches counts
  first, then nearby column pages; finished work defaults to 24 hours, with
  a week switch and older paging. Thread history, subtasks, hierarchy groups
  and agents page separately. A reconnect catches up a loaded thread by
  message id before following the feed again. Composer drafts are local to
  their keyed component and saved through `Platform`, never replaced by a
  snapshot. The optional owner offer is pinned to the payload the server showed,
  labelled and posted to `/api/task-action` by the action the offer names.
  It is fetched only near the controls, using the task's recorded branch;
  preparing the landing tool must not hold up the brief or the message box.
  The task page compiles only the timeline's box (`timeline_slot.rs`, its small
  `assets/timeline-slot.css` put in the page as `attach.rs` puts its own): as tall
  as this browser last drew the task's timeline (`project::stored`), else a
  default, with placeholder rows, so nothing outside it moves. Near the screen it
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
  What a first visit cannot know a ghost keeps from the last one, per project in the
  browser: the board's columns and counts (`tasks::SHAPE`, `board_shape`; none kept is every
  column with a card) and each column's first 16 cards as drawn in a window as wide, each one's
  pitch and whether it has a chip row (`tasks::CARDS`, `kept_cards`; a card's ghost draws the
  title lines its pitch has room for); the queue's rows per group (`queue::ROWS`) and each
  group's height; a task's head, title, facts and brief at the height drawn in a window as wide
  (`viewport::drawn_height`, `remember_drawn`), and its gates' room (`task_gates::KEPT`). A
  task never opened at that width takes the last task's (`recall_height` falls back to the
  newest part of the same kind, `task_gates::latest`); with none, a head with a facts line and
  a title of one line wide, two on a phone, four gates and a brief of seven lines. A brief kept
  shorter than those seven lines draws only the bars its box shows whole (`GhostBrief`). The
  timeline's box takes the height `timeline_slot::reserved` gives it. What is kept is measured
  as it draws, every 100 ms for `SETTLE_MS` at most, and kept again when it changed: a column's
  cards from its top to the first row not drawn yet, until every row has (`tasks::keep_drawn`),
  and a part's height until no region in it waits within the sentinel's reach of the screen
  (`viewport::remember_drawn`, `within_reach`), as rows draw well after their data on a loaded
  machine or a slow phone. A
  region still waiting carries `data-pending=true`: a `NearViewport` placeholder, a document's
  section, a column's first page, a task's thread. What a page already knows shows at once
  (a task's head and brief, a document's head and outline); `viewport::Settle` keeps only
  the part still reading (a task's thread and composer, a document's body) hidden under its
  ghost (`cover`) until no such region shows on its first screen, its read fails, or
  `SETTLE_MS` (3 s) passes; a hidden control takes no focus, so a
  browser test waits for `visible(SELECTOR)` before it focuses one. The task board shows its
  heading (one node from ghost to board, so its link keeps focus) and controls at once, each
  column reserving a card's box per task it counts, at the pitch kept for its place, else
  `tasks::CARD`, or `BARE` with no chip row where no agent has taken its tasks up (a title of
  two lines), and showing Load more only once its first page is in. A live update that adds or
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
  again with every `IntersectionObserver` callback 800 ms late. `tests/browser/skeleton.rs`
  covers the rest.
- **Queue.** `queue.rs` renders `/api/queue` at `/queue`, in its own page kind.
  The existing feed refreshes its owner decisions, spot checks, agent work and
  branch cards on queue-region deltas and reconnects. Branch cards share the ranked
  owner queue. Section anchors remain `#you`, `#spot` and `#agent`; row bodies use `NearViewport`.
  Owner decisions arrive in mixed document-folder order, also used by the home preview.
  `queue_filter.rs` derives counts from the complete snapshot and stores the selected
  owner category through `Platform` on this device. Chips use the shared Button;
  an absent saved category displays All without hiding new work.
- **Documents.** `docs_live` reads `/api/document?view=outline`: section keys,
  source positions and anchors, with short card summaries. `viewport::NearViewport`
  uses the shared visibility latch to mount each block once near the screen; a collapsed
  card requests its body only when opened. Mounted blocks request their bodies
  and threads together from `/api/section?format=blocks&keys=JSON_LIST`. Excerpts
  request `format=blocks` too; reads without it retain the legacy HTML/media
  response for compatible API callers. Canonical `/docs/` pages use the client;
  old `/app/docs/` and `/doc/docs/` bookmarks redirect there.
  Only Markdown files and folders are client routes; raw files keep normal links.
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
  …;`), then that script minified by the esbuild dx fetched. Its
  `html[data-navigation]` is `loading` until the shell's navigation finishes,
  then `ready`, or `failed` if the page cannot load. It preserves a scroll made
  after the new page mounts instead of applying its late top reset.
  Compilation leaves `OUT/public` serving its last build;
  packaging assembles beside it and `symphony publish-client` atomically exchanges the
  complete directories (macOS/Linux). Failed builds leave the published client
  intact; the next package removes abandoned staging folders. Old hashed assets
  remain available for open pages' deferred imports. Cached page kinds retain an empty Dioxus root;
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
  select's popup and the picture viewer are drawn by the overlay module
  (`page-overlay`, `components::deferred::OverlayApp`, mounted in `#overlay-root`),
  never the chat's. A task page's timeline is a persistent module too
  (`page-timeline`, `task_timeline::TimelineApp`, in `#timeline-root`), but cold:
  never warmed, loaded only once a task page's box asks for it and the page has its
  data and is idle, its root moved into that box and parked hidden at the body's
  end when the box goes. The shell requests a module's script, its wasm (a `fetch`
  preload) and its styles together. Once the first page has its data (the shell
  counts the page's `fetch` reads in flight, and waits until none has been for half
  a second, and after a read that got no answer until one does) and the browser is
  idle, it prefetches the persistent modules' script,
  wasm and styles at the lowest priority, nothing evaluated: the chat's first, the
  menus' once the chat's wasm (or the chat's own load, when a click came first) is
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
  in a task page, which has the shared file already.
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
  pictures at `data-full`. Mockups stay at 1280x800, scaled by their card's width,
  with only `allow-scripts` in their sandbox. Never add `allow-same-origin`.
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
  dimensions, presentation and `data-full`; `MediaView(media)` renders native
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
  that fails tries again every 3 s until one answers, and none polls.
  The media style is `assets/media.css`; the browser adapter uses web-sys and
  keeps every observer and listener owned by the mounted runtime.
