# apps/symphony-web/

The Symphony's web client in React and TypeScript, built by Vite: every page and everything a
page mounts. Its page kinds are the review queue (`/queue`), Reports (`/reports`), Runs (`/runs`),
home (`/`, the kind `review`), the documents (`/docs/`, each document, folder and HTML document an
entrypoint serves, the kind `docs`), the task board (`/tasks`, the kind `board`), the agents page
(`/agents`) and a task's page with its timeline (`/task/SLUG`, the kind `task`); the menus (a
select's popup and the picture viewer, the persistent kind `overlay`) and the panel (the chat and
its sheets, the toasts and the pending bar, the persistent kind `panel`) are mounted beside every
page. The board's task card is a component of its own (`src/taskcard.tsx`), which the agents page
draws too, and whose status words and machine work a task's page draws. The client reads the
server's JSON API, whose wire types are `crates/symphony-api`, and never runs server code or
inserts server-rendered HTML; no HTML fragment crosses that boundary.

The package holds the kinds (`src/`, an entry each under `src/kinds/`), the stylesheets
(`src/styles/`), the shell every page boots from and the device cache's store (`shell/`), and the
build (`build.sh`, `package.sh`). Its browser tests are a package of their own,
`apps/symphony-web-tests`. The pages were ported from a Dioxus client, retired since: where a
comment in `src/` says a thing is drawn or read as the Dioxus page did, it records where a rule
came from, and the rule is this client's now.

Nothing renders or loads until it is near the viewport: `NearViewport` (`src/viewport.tsx`) for
sections, plus native `loading=lazy`. Once revealed, content stays mounted, to preserve forms and
reading position. Closed panels and collapsed card bodies load only when opened.

## The tools

Each is pinned, and each is the smallest set that does its job; a dependency added is argued on
its task's thread first. The pins are the latest stable releases (no beta, rc, canary or nightly)
of Node and of every package, npm being the one the pinned Node ships, moved together, the
lockfile written again, in a task of their own.

- **Node and npm.** `.node-version` pins Node, which `tools/node.sh` runs as rustup runs the
  pinned Rust: the `node` on PATH when it is that version, else fnm's copy (installed once), its
  folder put first on PATH, else it refuses, naming the version; the command takes the script's
  process, no fnm between (`fnm exec` passes no SIGTERM on), so a signal reaches it. npm is the
  one that Node ships, held to its version by `package.json`'s `engines` under `.npmrc`'s
  `engine-strict`. `npm.sh ARGS` runs npm here on
  that Node after `npm ci` from the committed `package-lock.json` whenever `node_modules` was not
  installed from those very bytes, offline after the first fetch (`--prefer-offline`), two runs in
  one checkout taking turns (`tools/check_lock.sh`); an install that succeeds says nothing, so a
  first run's output is the command's alone, and one that fails says all it said on stderr.
  `.npmrc` saves exact versions, runs no
  package's install script (`ignore-scripts`; `fsevents`, Vite's watcher on macOS, ships its
  binary built, and a script `npm run` names still runs) and asks no outside service anything but
  the registry.
- **React 19** (`react`, `react-dom`), the only run-time dependencies.
- **TypeScript**, strict (`tsconfig.json`: `strict`, `noUncheckedIndexedAccess`,
  `exactOptionalPropertyTypes`, `verbatimModuleSyntax`), checked by `tsc --noEmit` as two
  programs: the code a page runs (`tsconfig.json`: `src/` but its tests and the dev server, the
  shell's `shell/cache.ts` and `shell/shell.ts`, and the Mac app's `notification.ts`), which knows
  no Node, and the code Node runs (`tsconfig.node.json`: the Node scripts, the tests, the dev
  server and `vite.config.ts`). It emits nothing: Vite strips the kinds' types, and the pinned
  Node its scripts' and the shell's (`shell/strip.ts`, its own stripper, which puts blanks where
  each type was, so a line of the result is the source's). So the shell and the scripts are
  written in the syntax a type erases alone (no `enum`, `namespace` or parameter property), which
  the stripper refuses. `skipLibCheck` leaves the packages' own declarations unchecked, Vite's
  among them, which name Node's types the page's program does not take.
- **`@types/node`**, Node's own API typed, pinned to the line of the Node `.node-version` names
  and read by `tsconfig.node.json` alone: the Node scripts and the tests call `fs`, `path` and
  `process` typed, where each would otherwise declare by hand the part it calls, and no page's
  code can reach a Node API, as its program has no such types.
- **Vite** builds the kinds (`vite.config.ts`): one entry per kind, `src/kinds/NAME.tsx`, named in
  `KINDS`; JSX by Vite's own transform, so no React plugin; no public folder and no module
  preload of Vite's, as the Symphony's server and shell serve and load the files. Its dev server
  is a developer's alone ([The dev server](#the-dev-server)).
- **Biome** lints and formats (`biome.json`: its recommended rules with the React and test
  domains on, the formatter at two spaces and 100 columns). It was chosen over ESLint with
  typescript-eslint, whose TypeScript support ends below the TypeScript this package pins, and it
  is one tool and one file for both jobs. `biome ci --error-on-warnings` makes a warning an error.
  Two style rules are off under `src/styles/` alone, the sheets carried whole from the retired
  client ([their README](src/styles/README.md)). Three more keep RULES.md `code/no-timed-waits`:
  `noRestrictedGlobals` refuses a bare `setTimeout` or `setInterval` outside `src/timing.ts`,
  the plugin `no-timed-waits.grit` one reached through an object (`window.setTimeout`), and
  `noRestrictedImports` `src/timing.ts`'s `sleep`. A wait is for its event (a socket's message,
  `pageshow`, a `MutationObserver`'s change, the reader's press); a number paces one only through
  `src/timing.ts`'s helpers, each shaped to its kind so no fixed poll is written with one:
  `retryWait` (the next of a `Backoff`'s waits, `state.ts`, never a length), `Debounce` (a rest
  that a later rest takes the place of), `deadline` (a bound on a wait for an event) and `tick`
  (to the next minute); a read's `pause` (`live.tsx`) takes the wait it is given. A unit test
  waits for a condition with `until` and yields a turn through `turn()`, both in
  `src/testing.ts`. A site kept is a `biome-ignore` whose reason begins
  `no-timed-waits baseline:`.
- **Vitest** runs the unit tests, with `@vitest/coverage-v8` for their coverage.

**Hand-written code here is TypeScript**: the kinds, the shell's store and script, the Node
scripts around Vite (`dev.ts`, `report.ts`, `shell/minify.ts`, `shell/strip.ts`) and the Mac app's
one script in a page (`apps/symphony-desktop/src/notification.ts`). JavaScript is what a build
writes. A script a page runs as it is written is TypeScript with no syntax of its own, every type
inferred, as the Mac app's is; one Node runs is TypeScript, which Node strips. `typescript-only.sh`
refuses a JavaScript file (`.js`, `.jsx`, `.mjs`, `.cjs`) anywhere in the checkout, tracked or
written and not ignored, outside the client's distribution (`apps/symphony-server/client/public/`).

`just client-lint` (`check.sh`) runs `typescript-only.sh`, the wire types' check and the tests of
the example that writes them, Biome, `tsc --noEmit` over both programs and the unit tests with
their coverage, each whatever the ones before it did, and `just lint-rs` runs it, so a
task's gate and the release run's lint both do. `just client-wire` writes the wire types.

## The wire types

`src/wire.ts` holds the TypeScript of the types the client reads from `crates/symphony-api` and
the constants it names (the feed's path and events, the Symphony's routes, the cache's format), as
that crate's `typescript` example prints them through ts-rs (the derives sit behind the crate's
`typescript` feature, which no build of the server turns on). It is never
edited by hand: `just client-wire` writes it, and `just client-lint` fails when the committed file
is not what the Rust types print (`crates/symphony-blocks`' block types too, behind its own
`typescript` feature). `src/wire.cases.json` comes from the same example: what the functions of
both crates the client carries in TypeScript answer for a set of inputs (`here`, `strip`, the
routes of a project's view, an HTML page's name, a card's topic, a viewer's element), with a
sample of each read the client checks at run time (the blocks' holding every kind, which
`blocks.test.tsx` draws), so the unit tests hold the TypeScript to the Rust. The cases only
check the inputs they list, so the file holds the source of each function the client carries as
well (`carried`, from the example's `CARRIED`, each naming the TypeScript that carries it): a
change to one fails `just client-lint` even where no case's input shows it. The change is then
ported to that TypeScript, with cases that show it, before `just client-wire` writes the file
again; a function the client comes to carry is added to `CARRIED` with its cases. What this cannot
see is a port left undone by a `just client-wire` run alone (the review reads the file's diff), a
change outside the functions named (a helper they call that is not listed), and an input no case
lists on which the two disagree. Each function is read from its `fn` line to the line at its
indentation that closes it, rustfmt's form, which `cargo fmt --check` holds, so no brace in a
string, a character or a comment cuts it short (the reader's tests, which `just client-lint` runs:
`just test-rs -p symphony-api --features typescript --example typescript`). TypeScript that carries no Rust
of that crate's (a rule ported from the retired client) is held by its unit tests and the browser tests.

A read is checked where it arrives (`readQueue`, `readPageBoard`, `readReports`, `readRunsPage`,
`document-read.ts`, of the checks `blocks-read.ts` exports): a guard checks the fields the page
draws, each of its type, and nothing else, so a field a newer server adds, or one it drops that
the page does not draw, changes nothing, as a serde read skips
unknown fields. A kind of block, text span or media the client does not know passes too, its
fields unread, and is said where it stands as unsupported content (`blocks-read.ts`, `blocks.tsx`,
`media.tsx`), the rest of the page drawn: the kit's lists grow in a minor, so a newer server may
send one ([PLUGIN_PROTOCOL](../../docs/PLUGIN_PROTOCOL.md#versions)). A word it does not know (a
figure's size, a tag, a shape) and a kind that is no text still fail the read.
A read the guard refuses is a failed read, said and retried as any other. A feed
event of a region or kind the page does not know is not its own, and is left alone.
`JSON.parse` costs no code per field, so a field the page does not draw, a task's plugins' board
fields among them, costs it nothing to receive: its types say what the server sends, and its
guards read what it draws.

A failed read is said in one sentence (`failed`, `src/trouble.tsx`), in a read's words: one that
got no answer in time fails with `NOT_IN_TIME`, never a post's `NO_ANSWER`, which asks the reader
to check before sending again, as a post the server may have acted on does. A page that tries the
read again by itself says "Trying again…" (`failed(why, true)`), and one with nothing drawn yet
does try it again, on the feed's waits (`tryAgain`, `src/state.ts`: 1 s doubling to 30 s), until
it is answered, the server has no such thing (a 404), a newer read starts or the page leaves; its
Retry stays, to try at once. A board column's first page is such a read: the column keeps its
ghost and the page's notice says it, until it answers or the view changes. A 404 said as missing and a read a click asks for (Load more) say no
"Trying again…". The limit a request waits for is `ANSWER_MS`, 15 s, which a page's
`symphonyAnswerMs` shortens for a browser test of a read held past it.

The feed is followed on a WebSocket (`feed.ts`): the events the server sends any other request of
the feed's as Server-Sent Events, each one message, its name, a line feed and its data (the feed
contract, `crates/symphony-api`). A browser keeps six HTTP/1.1 connections to a host, and an event
stream holds one for as long as it is open, so six tabs following the board would leave a seventh
none for its reads; it counts its WebSockets apart. The socket's open is the stream's, its failure
or close a lost stream, and a message the page cannot read lost too, as an event it cannot read
is; a socket the page closed says nothing more. The tests fake it with `testing.ts`'s `feeds` and
`say`.

## The tests

Unit tests sit beside their module (`queue.test.tsx` beside `queue.tsx`) and run in Node, with no
DOM and no DOM library: markup is drawn with `react-dom/server` and compared as text, and a test
that needs a global (`localStorage`, `fetch`, `location`) stubs it with `vi.stubGlobal`, the
browser's storage held in memory by `testing.ts`; what reads the page's elements (the media
layer) is given a small tree of elements of its test's own (`media-layer.test.ts`). Time is
Vitest's fake timers, never the clock. A test's or a hook's one time is the run's backstop of two
minutes (`BACKSTOP_MS`, `src/unit-run.ts`, in place of Vitest's 5 s), which only a stall reaches
(a machine asleep, a process stopped). A test waits for a condition with `until` (`testing.ts`),
whose one time is that backstop too, never with `vi.waitFor` or `vi.waitUntil`, whose own second
of real time a loaded machine reaches and which fails as the condition's assertion would:
`unit-run.test.ts` fails on either. The run's own report, beside Vitest's, says each test or hook
that ran out of its time as a stall, with the load average. It also names each test file the run asked for and did
not run or finish, and fails the run. Such a file is one whose worker never started: Vitest's own
waits for that (60 s and 90 s, which no setting changes) are reached only by a stall too, and its
summary leaves the file out of its count.
What only a browser shows (effects, the feed, the layout, the moves between kinds) is the headless
browser target's (`apps/symphony-web-tests/tests/browser`, [CHECKS](../../docs/CHECKS.md#the-browser-target-by-page)),
which drives headless Chrome over its DevTools protocol against the release build, served as the
server serves it (`just test-rs -p symphony-web-tests --test browser -- FILTER…`: the tests of the
pages and controls a change touched, never the whole target, which the release run runs). It
builds the client once per run into `build/symphony-ui/` unless `SYMPHONY_UI_DIST` names a built
distribution's `public/`; it needs Chrome (installed, or named with `SYMPHONY_CHROME`), and owns
each browser's temporary profile, process group and shutdown. A kind's entry
(`src/kinds/`) and the generated types are left out of the coverage, which `npm.sh run test`
prints.

## The build and the distribution

`just symphony-ui-dist [OUT]` (`build/symphony-ui`, the page in its `public/`) runs `build.sh OUT`:
one Vite build of this package (`npm.sh run build`, into `OUT/.modules/web`), then `package.sh`,
the manifest step. Two builds into one OUT take turns (`tools/check_lock.sh`). `package.sh` copies
Vite's assets into a stage, so one `builds.json` names every file of the build, and writes the
shell's manifest from Vite's (`.vite/manifest.json`): each kind's script, the chunks it imports and
its styles, the shared stylesheet first and then the CSS its chunks and its entry carry. The
shared stylesheet is the build's entry for `src/styles/shared.css`, which imports its sheets in
the order written there (the board's, the controls' theme, the frame's, then the shared
controls'): the order of the cascade is stated in that file, never derived from a file's name. The
shell is the manifest's line, then the device cache's store and `shell/shell.ts`, each with its
types stripped and the two joined (a join that is no module, a name both declare, refused), minified
by the minifier Vite ships (`shell/minify.ts`), named by its bytes, with the server's notice styles'
hash as its last line. The step refuses a build with no Vite manifest, no shared stylesheet or no
kind, and publishes nothing then. `symphony-build publish-client` exchanges the stage with
`OUT/public` whole. Files are named `NAME-HASH16.EXT` with a hex hash, as the server caches such a
name for good. No path, clock or environment enters the bytes: the build script sets
`NODE_ENV=production` (a shell's `development` would build React's development code) and Vite
reads no `.env` file (`envDir: false`), which `apps/symphony-web-tests/tests/build.rs` holds with
the rest of the build, so the build is the same from any checkout and shell.
`tools/client-key.sh` prints the build's key, the hash of every file git lists in this package but
its guides and of the notice styles, so the regenerator rebuilds the distribution when one
changes. The server embeds the files as it embeds the rest, and needs no Node to run. The build
needs Git, Bash, jq, the pinned Node and a Rust toolchain for the publisher.
Vite's `build.license` inventories the packages in emitted chunks with their installed license
text (`.vite/licenses.json`). `notices.sh` combines it with `copied-controls.txt` and the copied
controls' selected MIT text into `assets/third-party-HASH.txt`, named by the shell and retained
in `builds.json` with that build. The server packages, embeds and installs it with the other
client files. A missing inventory, missing text or license other than MIT refuses packaging,
so a dependency with different terms needs an explicit license selection in this step. The
repository's MIT license covers its own code; dependencies retain their own licenses.
`report.sh OUT` prints what each kind of the build in OUT makes a browser fetch
([Budget](#what-the-pages-keep)).

The shell imports a kind with `import()`: its
entry exports `run`, which mounts the kind in `#review-root-NAME` and draws it before it settles,
and `stop`, which aborts the kind's mount and unmounts it, its feed and reads with it. What the
kinds share (React, the frame) Vite puts in a chunk their entries import, named after one of its
modules, so a first visit takes the kind's entry and that chunk, and a move between two kinds of
this client the target's entry alone. What every page kind imports stays that one chunk
(`codeSplitting` in `vite.config.ts`), which the menus' entry imports too, though they use only
part of it: split by who imports each module, it would be two files every first visit takes,
compressed apart. The device cache and the board's live half (`cache.ts`, `live.tsx`) stay one
chunk the same way, which Reports loads for the cache and the page's notices alone: split by
importer, the cache would be a file more on every page that follows the board. The shell asks for the chunks an entry imports beside it, a
module preload each (`imports` in its manifest, which `package.sh` reads from Vite's), so a first
visit fetches them with the entry rather than once the entry has come and been read: a round trip
fewer, which a phone over the tailnet pays.

**Work that outlives its start.** Async work a page starts checks, after each wait and before it
touches the page, the window, the storage or the address, a signal that ends with what started
it: an effect's own (its cleanup aborts it), or, for a handler or a read with no cleanup of its own
(a click's post, a document's versions read), the kind's mount (`useMount()`, `src/mount.ts`, which
`stop` aborts). A kind the shell stopped does nothing more: Later answered after the reader left
moves no one, and a late read writes nothing over the next kind's. The device
cache's store is the shell's (`shell/cache.ts`, `symphonyCache`, which `src/cache.ts` names what
to keep in), and it opens every IndexedDB transaction: this client opens none of its own.

A move between two kinds stops one and runs the other; a move within a kind is the shell's
`review:navigate`, which `route.ts` follows. The header, the panel's `+` and bell and the feed are
the frame's (`src/frame.tsx`), which every page kind draws; the panel and its sheets are this
client's persistent kind `panel` ([The panel](#the-panel)), which the shell mounts beside any page.
The header opens with the Symphony's mark, the fermata, and its wordmark, or in the place of each
the icon and the name the project's theme has of its own (`ProjectView`'s `mark` and `wordmark`,
which the server decides), as the server's own pages open theirs
(`apps/symphony-server/src/mark.rs`; `src/frame.test.tsx` reads its `mark/mark.svg` to hold the two
marks alike, as the published server's tests read only its own files); it is no link, so the
keyboard's first stop stays the project switch.

The menus are this client's persistent kind `overlay` (`src/kinds/overlay.tsx`, `src/overlay.tsx`),
which the shell mounts in `#overlay-root` on the first select or picture and keeps beside every
page after: a select's popup (`select-popup.tsx`, the primitives' listbox, placed by
`popup-position.ts` and matched as typed by `typeahead.ts`) and the picture viewer
(`picture-viewer.tsx`, the primitives' dialog with its focus trap). A request is data, never a
component: the shell's `reviewOverlay({kind, owner, …, callback})`, which a select field
and the media layer send alike, answered through its callback (a select by the value chosen
or none, the viewer by the step asked, 0 to close) and closed by its owner. A page that shows media
mounts the media layer (`media-layer.ts`) before it: started by the first media it shows, it scales
the mockups' frames, loads a viewer's script, runs the views' buttons and opens a picture in the
menus' picture viewer, all of it ending with the kind's mount. Each picture it makes a button is
named by its alt, else its figure's caption (`nameOf`), and takes the focus as it opens, so the
viewer, whose Full size link opens the picture itself in a tab of its own, gives the focus back to
it. A card's compare row (`.compare`, `src/docs.css`) holds up to four pictures on one line and
wraps more into a gallery, each captioned by the server from its source's alt
(`crates/symphony/src/render/cards.rs`). A document's
contents watch (`contents.ts`: the outline folded on its own while a drawn table is wider than its
box, unless the reader chose) is no part of it: it runs from the mount of a body that has the
outline to its unmount, under the signal of that body's own effect (`DocumentBody`).

A read the page gives up is given up on the wire too: every read in `src/` is passed the signal of
what asked (`getJson(url, signal)`, `getIf(url, etag, signal)` and `getDocument`, all through
`exchange`), so a read the feed's news replaces, or one a page left behind, is aborted rather than
answered into nothing, and its caller still checks that signal after each wait, as an answer may
come first. A post takes none: the server may have acted on it. A text box (`textarea.tsx`) hears each
`input` event as its change: React's `onChange` compares the
box with the value it last saw, and so misses a value a script set before firing `input`.

Styles are the existing sheets, used as they are: the project's `theme.css`, inlined by the
server, the shared sheet (`src/styles/shared.css`: the frame, the controls, the board's rules), whose
class names the markup uses, and the kind's own sheet, imported by its module
(`src/queue.css`, `src/docs.css`; the menus' `src/overlay.css`: the popup's and the dialog's rules
as the components' sheets wrote them, their CSS-module suffixes gone, and the picture viewer's,
moved there from the media sheet; the panel's `src/panel.css`: the `+` sheet's tabs and its
collapse button's tip as the components' tabs and tooltip sheets write them, and what the sheets
hold once the panel has mounted, the chat's thread and composer, the activity list, the toasts and
the appearance panel; the boxes the shell draws before that are the shared sheet's, which every
first visit pays for). The media sheet,
`src/styles/media.css`, which the shared sheet leaves out, is the media layer's,
imported by `media-layer.ts`, so home, the documents, a task's page and the chat take it as one
file under the layer's name. The task board imports its own layout sheet
(`src/styles/boardwait.css`) through `src/board.tsx`, emitted as `board-<hash>.css` after the
shared sheet. The agents page has no sheet of its own: its cards use the shared sheet's
task card rules (`src/styles/board.css`). The attach control's sheet
(`src/styles/attach.css`) comes inside `src/attach.tsx`, which the first composer to mount puts in
the page, so no first visit carries a byte of it. The server's own pages inline `board.css`'s
tokens and `frame.css` from copies in its package (`apps/symphony-server/src/`), which it carries
to crates.io: a change to either is copied there too, and `crates/symphony/tests/packages.rs`
fails until it is.

Blocks are drawn by `blocks.tsx` and `media.tsx`, with two rules React does not give. A presentation's style is written as given, by a ref (`writtenStyle`): React's `style`
takes an object, which it sets property by property, and would lose a value marked `!important` or
a declaration after a comment. A property the element
already had and the written style does not set (a centred container's `text-align`) is kept. A
`javascript:` address in an href or a src is replaced by React 19 with one that throws.

## The dev server

`just symphony-ui-dev SERVER [PORT]` shows this client's kinds as their sources stand, with no
client build, the page loaded again on each save: Vite's own dev server (`dev.ts`, the Node around
`src/dev-server.ts`), on the loopback address alone at PORT (0, the default, for any), over a
Symphony server of the developer's own on port SERVER, which `just symphony --port 0` starts and
prints (`--root DIR` another project's). It refuses 8765, the shared server's, as either port, and
starts nothing: Ctrl-C, or a kill of the recipe's process, ends it. Every request but those for
Vite's own files (under `/@symphony-web/`) goes to that server through Vite's proxy, its Host and
Origin as the browser sent them, so a save meets the server's origin check as on the server itself:
the pages, the API, the feed (its WebSocket's upgrade too, `ws`) and the build's files. It sends
no CORS header
(`cors: false`, where Vite's default lets a page on any loopback port read every answer), so a page
of another origin reads through it no more than from the server. What it makes itself is the page's
shell script: this checkout's shell (`shell/cache.ts` and `shell/shell.ts`, their types
stripped, unminified) with Vite's client first, and each of `KINDS` as its entry under Vite (one the
server's build has as another client's or not at all among them) with the server's shared
stylesheet, each kind's own styles kept before the project's theme as the built shell keeps them;
a kind the server's build has and this checkout does not is left out. So a kind added on a branch, or a
change to the shell, shows with no build too. A save to a kind's module or to the shell's sources
loads the page again, a style sheet's change applies in place, and an error in a module is drawn
over the page.

A reload, not React's Fast Refresh (`@vitejs/plugin-react`): that would be a dependency more and
one more script put in a page Vite does not serve, and it keeps a component's state only across an
edit to a module that exports components alone, which few of these do, and most pages here draw
again at once from the device's cache and keep their drafts on every input. What only a build
makes it does not show: the shared stylesheet is the server's build's (an edit to the
sheets under `src/styles/` it joins, `board.css` among them, shows once `just symphony-ui-dist` has built them
and the server serves that build, `SYMPHONY_UI_DIST` naming its `public/` for another project),
React is its development build, and the bytes are no build's. No build, regenerator or check runs
it (its files count in the regenerator's key, as every file of this package does);
`src/dev-server.test.ts` holds what it does.

## Reports

`src/reports.tsx` draws `/api/reports` at `/reports`, the kind `reports`, in the frame, with no
feed of its own: it reads the window this device last chose (`reports-grain`, `reports-days`
through `storage.ts`), else the last 24 hours by the hour (`FIRST_WINDOW`), at once, on each
choice and every five minutes with the last answer's tag (`followReports`, under its effect's
own signal), a read that failed sooner, on the feed's waits. Each window's last answer is kept on
this device (`openReports`: the item `reports` of its checkout, a part a window, whole, as the
largest is some 140 KB and all nine a third of a megabyte; outside the caps as the board's reads
are) and drawn at once on a return or when its window is chosen again, marked `data-cache=device`
and said as the other pages say it, in their notices (`PageNotices`, from the chunk of `live.tsx`
the cache's read already loads: the reserve that keeps the page's end clear of the stack, and the
toasts above it) until a read answers: a `304` for a window no roll-up changed, or a new answer,
drawn and kept in its place. A read that fails leaves the kept copy drawn, said not reachable,
under the failed read's own line, which a choice of window clears until its own read fails. The
periods are read by path as the server writes them (`Bucket`): a source with no record of a period
is none there, a count by status that leaves out its zeros a zero. Four
figures sit over five sections of charts, one column on a phone and two from 960 px. Each figure
links to the chart it sums up (`#chart-done`…) while the charts are drawn. A chart's id is an
anchor of its `NearViewport`, held in its ghost until it mounts; the chart an address names mounts at
once and is landed as a document's part is (the shell's `land`, once a navigation), so a fresh
load's charts, which come after the shell looked for it, land too, until the reader chooses a
window: a first answer that drew no chart (nothing recorded, a failed read) started no landing,
and the window chosen after it lands none (`lands`, `NearViewport`).
A project whose board runs a custom workflow gets a Workflow section after Flow
(`workflowSection`): time by phase and time by state, two charts of the same hours, a series for
each phase or state under each label its hours were spent under (its ID after it where two share
a label), the four largest and Other past five; drawn only while a period of the window records a
workflow, so a board that runs none draws the page it did.
_Tokens and cost_ charts each period's estimated cost and its tokens by role, from the period's
`spend` (`ReportBucket::spend`, the server pricing the day files' tokens at read), a period with
no record hatched, and sums the window under them (`SpendSummary`): tokens and cost all told, the
cost per landed task (`ReportsPage::landed`: the median and the ten dearest, each a link to its
page) and the review rounds after the first against the first, every dollar said as estimated at
the prices read on `ReportsPage::prices`, and how an hour's tokens are counted (`TOLD`). Its two
units, `tokens` and `dollars`, are formats of the one bar chart, not a chart of their own.
`src/chart.tsx` draws each chart by hand, with no charting library: bars as boxes over a column
a period, lines as one SVG whose strokes keep their width, both on one axis with its unit; a
period no file recorded is hatched and its line broken, never a zero. Each entry of a chart's
legend is a toggle button (`aria-pressed`) that hides its series on that chart alone, kept in
every window by its chart's id and name (`reports-hidden`, a key a line): a hidden series leaves
the plot, the readout and the table, and the axis scales to the series shown (`scaleOf`), so
hiding a large one makes the rest larger; a period stays hatched only when no series, hidden or
not, recorded it. A tap or the arrow keys pick a period, the same on every chart, and its readout
says every value as text; each chart's table holds every period. Figures are written with
a tie rounded to the even digit, as Rust's `{:.N}` does (`fixed`,
`words.ts`, which home's percentages use too). The
series take `--chart-1` to `--chart-5` (`src/reports.css`), set at no specificity so a theme's
own win, with their own steps in the dark. Charts below the screen mount through
`NearViewport`. Hours are labelled in UTC.

Every chart carries one dashed line (`.chart-mark`) at each commit that reached the default branch
in the window and changed the project's settings or the instructions its agents work under: the
settings file, the rules or a skill's file (one installed, under `.agents/skills` or
`.claude/skills`, or shipped, under `shipped/skills`), and no guide, which changes too often to
mark. The server names what each changed (`ReportMark::changed`: `settings`, `rules`,
`skill NAME`), from one list of the files that count (`instruction`,
`crates/symphony/src/reports/git.rs`), and the page says it with the commit's subject and hash in
the list over the charts (`MarkedChanges`, the newest three, the rest folded), in the mark's title
and in the readout of the period it fell in. All are drawn alike; a mark that names nothing, as a
server built before it named any wrote it, is a settings change.

## Runs

`src/runs.tsx` draws `/runs`, the kind `runs`, in the frame, with no feed of its own
([RUNS](../../docs/RUNS.md#the-runs-page)). Now is the machine work (`/api/machine-work`,
`followNow`, every 5 s), read by a guard of the fields it draws (`readNow`) rather than
`work.ts`, which only the board's kinds import, so this page splits no shared chunk. The list is
`/api/runs` (`followList`, its first page every 15 s with the last answer's tag), with its
filters kept as a query through `storage.ts` (`runs-filters`) and Load more (`readOlder`) under
the mount's signal. A run is `/runs?run=ID`, the same kind: the shell's move within it says
`review:navigate`, and the page reads `location.search` again. Its read (`followRun`) goes on
every 5 s while the run or a step of it has not ended, reading its log on from what is shown
(`appended`), and from its tail again once the log is shorter than that; a log the server keeps
no more (`404`) ends an ended run's read and is said quietly. The output before it
(`readEarlier`) goes in front (`prepended`). Each reader takes its effect's signal, and each
wait between reads is a parameter (`pause` by default: its time, then on only once the page is
shown, `whenShown` in `feed.ts`), so `runs.test.tsx` runs them with no timer. Nothing is kept
on the device but the filters. The steps are bars on the run's span drawn as boxes
(`stepBars`), with no chart; Trends, a chart of the roll-up, will share `chart.tsx` with
Reports. Times are in UTC.

## The agents page

`src/agents.tsx` draws `/agents`, the kind `agents`, under the board it follows (`keepBoard`,
`live.tsx`): its probes, the tasks happening now as the board's cards, the machines' build and
test context, who is connected and, read when their list opens, the lost agents. Each agent's row
says what is known of it, its Go to session control (`session.tsx`, which the task page's role
chips will share), its builds and tests, how to reach it, what it works on, the open tasks it
orchestrates (its Following list, as many as the agent's `following` counts, `Followed` reading
them a page at a time once opened) and the five it finished last. `src/agents-live.ts` holds its
reads, guarded as serde reads them (a field a server
leaves out reads as serde's default), its rules and the page kept live (`LiveAgents`): the first
page of `/api/agent-page` once the board is read, then each feed event applied where it lands; one
that brings in a task the page lists no record of (`agentsPageTakes`, the server's rule) or says
the board is back reads the first page again, merged into what is drawn, both cursors and the
lost agents kept, and the events heard during a read are replayed onto its answer (`ReadCursor`,
`src/journal.ts`, the board's as well and no other kind's, so they alone carry it). It keeps
nothing on the device. Its text orders and trims as Rust's does (`textOrder` by UTF-8
bytes, `whiteSpace` for `trim`), so its lists and lines are the server's. An agent with tokens
has a line of them under its own (`agentSpent`): how many in how many calls, and their cost
estimated at the prices read on a named day, with the models no price was found for.

## The documents

The kind `docs` (`src/kinds/docs.tsx`) draws every page an entrypoint serves: a document, a
folder with its intro, and an HTML document in a frame that runs its scripts and nothing else
(`sandbox="allow-scripts"`, its file's `?raw` address, which the server answers whatever
fetch-metadata headers a browser sends or leaves out). `src/document-page.tsx` draws them;
`src/document-live.ts` holds what each page reads and follows, a model its loops change under
the page's signal: the outline and the bodies drawn on this device kept and drawn again at once
on a return (`cache-document.ts`), the outline read with its copy's tag, the bodies of the
sections near the screen read and kept, and the document's feed followed. A section that did
not change draws nothing again, and typed words live outside the page's markup: a section's
editor (`editor.tsx`: a card's verdict under its tap, a message, a comment), a question and its
follow-ups (`asks.tsx`), each draft saved on this device on every input with its files
(`attach.tsx`: `POST /api/upload` answers a line for each file, which the posted text carries,
docs/REVIEW.md, "Attaching files"). A link's excerpt, once opened, draws its
blocks whole at once (`ExcerptBody` under `AtOnce`, `viewport.tsx`), no part of it waiting to come
near the screen: opening it asked for them, and its box, which scrolls, would clip the sentinel's
look-ahead, leaving a box opened below the screen empty until it was scrolled to. A card's Later
is the queue's (`postpone.tsx`), the related tasks and a branch's differences are `related.tsx`'s.
Its own styles are `src/docs.css`. Once a landing
has ended, the browser's own scroll anchoring holds the reader's place while the sections around
it draw, and a change to a margin of the part being read (or to any box around what Chrome
anchors on, a focused field first) stops it for that frame, so everything drawn above lands on the
reader. The spacing that stands in for the margins one card's sections collapsed therefore reads a
neighbour not drawn yet as what it will draw as, by its part's classes (`partForm`, `document.ts`):
an open card by the outline's cards, and how its prose opens and closes by the kinds of its first
and last blocks, which the outline carries for each section (`ends`,
`symphony_api::Section::ends_of`). A section in an outline kept from before it carried them reads
as opening and closing with no listed block, so its neighbour may move once as it draws, until
the next read replaces that copy
(`apps/symphony-web-tests/tests/browser/document_landing.rs` reads a paragraph, a list, a table, the intro and a card
beside code, a quote, a mockup, a picture, a heading, a table and a title, on a desk and a phone).

## A task's page

The kind `task` (`src/kinds/task.tsx`) draws `/task/SLUG`: one live task per checkout, which
follows the route from task to task within the kind (`review:navigate`) and keeps one feed. Each part of the page is a read of its own in the page's store
(`src/parts.ts`, `Parts`), named by a key and drawn by the component that names it (`usePart`):
the task's page (head, facts, brief), its thread's newest page, its gates, the landing it offers
and its timeline (`src/task-live.ts`'s `pagePart`, `threadPart`, `gatesPart`, `offerPart`,
`timelinePart`). Each is asked as the page mounts, behind no other read, the board's among them;
the page and the thread are kept on this device (`taskItem`) and drawn at once on a return,
marked stale, then checked with their copy's tag. The store reads a key once however many
components name it (an ask while its read is out joins it), tries a failed read again on the
feed's waits (`tryAgain`) while the other parts draw, and ends a part's reads when the route
leaves its task (`drop`) or the page goes. A part whose read failed with nothing drawn looks on
this device again, so a database that opened after the store's deadline still gives the copy
when the server does not answer.

Every JSON read's answer names the feed's cursor it was read at (`VERSION_HEADER`, read with
`request.ts`'s `getAt`): every event at or before it is in the answer. So the reads and the feed
start in any order, and the store places each against the other: a feed event at or before a
part's answer is dropped, one after it is applied to the part or reads it again, as the part says
(`hear`); an answer is drawn with the events heard since it was asked, and after its cursor,
applied to it again; a part is read once more only where the store cannot tell what it missed
(its cursor is before the version the feed opened from, the server named none while the feed
opened, or the events it would need are no longer among the 128 kept); and when the feed opens
again, only the parts answered at another version than it opens from are read (`since`).
`LiveTask` (`src/task-live.ts`, on `live.tsx`'s `keepBoard`, whose `views.read` gives the board's
loop nothing to wait for) hears the feed into the store, each event with the page as it was drawn
when it came (`TaskEvent`), and each part reads itself again, through its `hear`, for what an
event newer than its answer names without carrying: the gates for a verdict, a card in a
document the task links, a new tip of their branch or a run of the task's (`namesGates`, the
links and the run told by the page as the event found it); the offer, the gates and the timeline
when the task itself changed. A part whose read failed offers its retry (the gates' Retry, the
thread's Load messages, which nothing offers while its first read is out), and says it reads
while a read is out after the failed one, the store's own or the reader's (`Shown.retrying`).
What no key names stays the page's own: the thread's earlier pages and its catch-up
after a gap, and the subtasks, read into the page as their section nears the screen. Its guards
are `src/task-read.ts`.

`src/task-page.tsx` draws the head (the owner's priority and cancel, the
roles with each holder's Go to session, `src/session.tsx`), the gates (`src/gates.tsx`, their
logic `src/gates-model.ts`, the links they may draw `src/links.ts`), the subtasks, the timeline's
box, the brief, the thread and its composer, each from its own part as soon as that is there and
as its own mock-up until then, in the page's one frame: the head and the brief at the height this
browser last drew them, the gates' outline, the timeline's box, the thread's and the composer's
ghosts, and none waits a set time. On a visit after the first a part that comes moves none of
the others: each mock-up is as tall as this browser last drew its part. On a first visit a
mock-up is a guess (the head's and the brief's heights of the last task drawn, else a typical
one's; the gates' outline of four signed gates, one waited on, which the page's own count
replaces if the page comes first), so a head, a brief or gates of another shape move what is
below them once, as its answer comes. The store asks the reads in the order their parts were
opened (the page and the thread, then the gates, the offer and the timeline as they mount), each
after its own lookup on this device alone, so the server hears of the head before what is below
it. A part whose read fails says so in its own place while the rest draw. The timeline (`src/timeline.tsx`, its layout
`src/timeline-layout.ts`) is a chunk of its own, which its box (`src/timeline-slot.tsx`) imports
once it nears the screen and its part has something to draw (an answer, or why there is none),
behind no other read, so the page's first draw carries none of it; its style comes with it. The gates' and the box's styles (`src/gates.css`,
`src/timeline-slot.css`) are the kind's own.

The task's Changes section (`src/task-diff.tsx`, its style `src/task-diff.css`) mounts near the
screen, reading its own live part from `/api/task-diff`: paths and line counts, binary files
said as such, never patch text. It follows branch changes and the task's verdicts. Opening a
file reads `/api/task-diff-file` with the summary's base and head commits; a new comparison
closes the old files and cancels their reads. Patches are scrollable and bounded at 256 KiB,
the file list at 500 files and 256 KiB of Git output; every incomplete or unavailable read
says why. This code belongs only to the task kind, so the board carries none of it.

A task whose page counts execution attempts (`attempts`, which the server leaves out at none)
draws their history under its Changes, newest first (`src/attempts.tsx`, its style
`src/attempts.css`): a chunk the page imports for such a task alone, near the screen, so a task
with none loads and reads nothing of it. It reads `/api/task-attempts` again on each change of the
task and on Refresh, as the feed carries no attempt's change. A cancel, a stop or a resume waits on
its confirmation, then posts the attempt's revision as read (`/api/attempt-control`) and draws the
history it answers; a view opens a blank window within the click, cuts its opener and sends it the
address `/api/attempt-view` resolved, which the page keeps nowhere
([EXECUTION_ATTEMPTS](../../docs/EXECUTION_ATTEMPTS.md#on-a-tasks-page)).

What the task's tokens cost follows its time on its timeline, and nothing is at the page's foot.
The box asks for the tokens' part beside the timeline's (`/api/task-tokens`, `src/task-live.ts`'s
`tokensPart`, read again on a change of the task) and hands the timeline what it answers, which
the timeline guards (`readTaskTokens`) and draws without until it comes: the total after the
task's time on the first line (`.tl-cost`), its words (that it is an estimate and of what day's
prices, its split by role and by kind, the tokens and the calls) said in the legend's place while
it is pointed at or focused, and under the totals on a phone, where the rounds and bounces move to
the second line; and each stage stretch's cost after its time, the fitter trying the word, the
time and the cost, then the time and the cost, then the time alone, a phone's row ending with the
time and the cost. `src/timeline-cost.ts` puts each of the record's rows on exactly one stretch,
by its role and review round and never by a share of time (its header says the rule and where a
row the rule places nowhere goes), so the stretches add up to the total, and a stretch's words
say whose calls it carries, when they ran and how many. As the layout, it imports nothing of the
kind's own. A task with no tokens draws as one with no cost. Every figure is an estimate at API
list prices with the day they were read (`src/spend.ts`, which says them as `just tasks show`
does).

## The panel

The kind `panel` (`src/kinds/panel.tsx`, `src/panel.tsx`) is the shell's persistent kind, which it
mounts in `#quick-root` on the first click of `+`, the bell or Appearance, a toast's arrival or a
pending step (its files warmed ahead once the page is idle) and keeps beside every page after, so
the conversation, the task form and their places stay. It draws the `+` sheet (`src/quick.tsx`:
its Chat and Add task tabs, `src/tabs.tsx`; the conversation, read again when the page's feed says
its thread moved; the composer with its attachments; the new task's form, its priority the menus'
select, its save asked again until the server knows the outcome), the activity list, unread
against what was seen, and its toasts, the pending bar, and the header's Appearance
(`src/appearance.tsx`, the theme file read by `src/theme.ts`). Its pure parts are
`src/panel-model.ts` (the snapshots' readers, the draft, the toasts, a save's outcome, what was
read) and `src/panel-focus.ts` (the focus taken on open, held on the shown tab's panel while a
send disables the sheet's fields and given to the field the reader types in after it, Escape
giving it back to the opener). Until it mounts, the shell draws the
sheet a click opened at once (`#panel-loading`), and the panel's own sheet takes the focus over.

It has no feed of its own: the frame (`src/frame.tsx`) reads the activity and the pending cards
and the two speak through the window, as the frame and the shell do: the snapshots
(`reviewQuickDraft`, `reviewActivityOpen`, `reviewAppearanceOpen`, `reviewPanelFocus`,
`reviewPanelOpener`, `reviewActivitySnapshot`, `reviewPendingSnapshot`, `reviewFrameError`,
`reviewKnownIds`, `reviewPreviousIds`) and their events (`review:quick`, `review:activity`,
`review:appearance`, `review:frame`, `review:frame-seen`, `review:activity-arrived`,
`review:asks`). What it keeps in the browser is under its keys (`review-quick-draft`,
`docs-review-seen`, `studio.cssVars`), the ones a reader's draft, what they saw and their theme are
already kept under, so all three stay. Its media layer (`reachOf`) takes the chat's pictures up on every page,
each opening in the menus' viewer, and on the pages that mount none of their own (the task board,
the agents page, the queue, Reports and Runs) every mockup and viewer as well, the chat's and the
page's, with the page's pictures.

## What the pages keep

What holds on every page, and each page's contract where its section above names only its code.

- **The feed.** The pages follow `crates/symphony-api`'s feed contract (`src/feed.ts` and
  `src/live.tsx`). A stream
  is lost when it errs, when it says nothing (not even the server's ping) for
  two and a half of the board's `ping_ms` (at most 5 s, so 12.5 s: the owner
  reads the board from a phone over the tailnet, where a stream goes silent
  but open on every network change), or when it sends an event the client
  cannot read. Then the client closes it, marks the page stale
  (`main[data-feed=stale]`, a notice), waits 1 s doubling to 30 s (the waits
  start again only after a stream held 30 s), fetches the board again (it
  replaces the one shown whatever its version) and opens a new stream from it;
  the board is live again once that stream opens. It never resumes by
  `Last-Event-ID`. It is followed on a WebSocket (`src/feed.ts`): an event stream
  holds one of the browser's six connections to the server (HTTP/1.1) for as long as it is open,
  and six tabs of one server on streams would leave a seventh none for its reads, where the
  browser counts WebSockets apart (`apps/symphony-web-tests/tests/browser/tabs.rs`). A page that goes away closes its
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
  (`reloadFor` in `src/state.ts`, remembered in `sessionStorage`); while the reader is in the
  middle of something (`engaged` in `src/route.ts`) it is held, and goes at the first event that
  can end that (`whenFree`: the focus leaving the fields, a busy region, list box or details
  changing, or an element added or removed), never on a timer.
- **Cache.** What a page drew is kept on the device so a return visit draws at once
  ([the client cache](../../docs/analysis/client-cache.md)). The store is `shell/cache.ts`
  (`window.symphonyCache`), which `package.sh` puts first in the shell's one script: every page
  kind shares it, and it opens every IndexedDB transaction. `src/cache.ts` and
  `src/cache-document.ts` reach it: an
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
  which the shell changes without a reload, use it once it opens, and so does a part whose read
  failed with nothing drawn (`src/parts.ts`). A page's `symphonyCacheOpenMs` names another
  wait, for a browser test of a page with its cache, which a loaded machine's slow open would
  otherwise run without it. A page that goes away
  (`pagehide`, `freeze`) aborts the store's open transactions and starts none until it is back
  (`pageshow`, `resume`), as it closes its feed's stream: a page kept for back and forward runs
  none of its callbacks, so a transaction left between two of them would hold its locks, and
  Chrome evicts the kept page (IgnoreEventAndEvict) once the next page's first call asks for them.
  `apps/symphony-web-tests/tests/browser/cache_store.rs` drives the store alone in headless Chrome, no release build
  needed, and reports its coverage. Each case starts on a deleted database once the store has
  opened its new one (a scratch item kept, then forgotten), so a loaded machine's slow open
  never runs a case without the cache; the pages that test the deadline keep the store's 500 ms
  against their own opens.
- **A return visit.** Home, the queue and the task board each draw what they kept of
  `Item::board(scope)` before the server answers, then check it. `/api/board` (part `board`) is
  drawn and still read fresh, with no tag, since its `version` starts the feed; nothing else is
  read before it. Every other kept read then goes at once with `If-None-Match` naming the kept
  copy's tag and `cache: "no-store"` (`getIf`, `src/request.ts`): a `304` is the
  copy, a `200` replaces it on the page and on the device. The queue keeps `/api/queue` (`queue`,
  `QUEUE` in `src/cache.ts`). The task board keeps its
  default waiting view only (no search or wait-kind filter, the last 24 hours, stages by the
  machine's activity): the counts
  (`counts`), each column's first page (`column:STATUS`) and the machine's work (`work`), never a
  search, wait-kind filter, another window or a Load more page. Its cache item ends in `waiting`,
  so the former board's columns cannot return in this view; a column drawn from the device is
  checked at once,
  not after its 250 ms. Its listings are read with `work=0`, which places tasks by the machine's
  Build and Test work as before and leaves that work out of the body, whose heartbeats would
  otherwise change every listing's tag; the work is read apart (`/api/machine-work`) and followed
  by the feed. The header's reads are never kept. A check that fails is asked again, waiting
  longer each time (`Backoff`, `src/state.ts`), until it answers: the counts, each column drawn from the
  device and the work (`LiveBoard.check` in `src/boardlive.tsx`), the queue
  (`src/queue.tsx`); the feed's `machine-work`
  event, which carries the whole work, answers for the work's read. Once every column drawn from
  the device has answered, a task it drew that no fresh answer (a column, a feed `task` event)
  listed is dropped, whichever stage the work places it in by then. `Freshness` (`src/cache.ts`) says what
  a page drawn from the device shows: `main[data-feed=stale][data-cache=device]` and the notice
  "Shown from this device, saved 14:32. Checking for changes…" (the oldest copy drawn), "Not
  reachable: …" while a read fails and the feed is not open, cleared once every drawn read has
  answered and the feed is open. A task's page ([below](#a-tasks-page)) keeps `taskItem(scope, slug)` (`src/cache.ts`):
  its page (`page`, `/api/task-page`) and its thread's newest page (`thread`,
  `/api/task-history` without `before`). Opened, it counts a visit and draws them at once unless
  it shows that task already. Both are checked together, each asked again until it answers
  (`LiveTask`, `src/task-live.ts`): once `/api/board` is read on the page's first open, at
  once for a task opened in place from another. A thread answered `200` replaces the messages the
  device drew and keeps those the feed brought meanwhile (`newest`). Until the
  page's check answers, a page drawn from the device reads no other part of the task: the
  newest messages when no thread was kept, the subtasks and earlier messages wait for it, and
  the feed replays what changed after the board's version. The landing offer, the gates and the
  timeline, which it never keeps, it reads at once. A task the server no longer has (`404`) says so and
  is forgotten. Never kept: earlier thread pages, the landing offer (`/api/task-offer`, read on
  every visit), the subtasks' further pages and the brief's later chunks. `/agents` draws
  nothing from the device.
  A document keeps `documentItem(path, false)` (`src/cache-document.ts`): its
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
  `STALE` (`src/shared.ts`), which `PageNotices` (`src/live.tsx`) sets
  while that notice or the reconnecting one
  shows, holds the owner's actions (verdicts, messages, comments, questions, priority, cancel,
  landing, Later):
  disabled, saying "Checking…"; drafts stay editable. `apps/symphony-web-tests/tests/browser/cache_pages.rs`,
  `cache_tasks.rs` and `cache_documents.rs` hold the reads back on the stand-in
  (`Site::hold_route`, which answers `If-None-Match` as the server does and logs each answer;
  `Site::set_document` serves a document's outline, its first part and its sections by their keys
  as the server does).
  Once a page has its data, its feed is open and the browser is idle, the shell reads ahead the
  first 8 task and document links on the screen in its `main` (`ahead` in `shell/shell.ts` and
  `shell/cache.ts`; [Read ahead](../../docs/analysis/client-cache.md#read-ahead)): one after
  another, at `priority: "low"`, marked `Purpose: prefetch`, naming any tag kept, and kept
  unvisited. A task is its page and its thread's newest page; a document its first part
  (`/api/document?view=first`: the head, every heading among it, and the leading sections whole
  within `symphony_api::AHEAD_BYTES`; the server does not follow it), whose contents and bodies a
  visit draws at once, so nothing moves when its outline answers, and keeps as its own
  (`split`, `src/cache-document.ts`). Never on save-data or a 3g or slower link, never a link that acts, a
  search, the page itself, a folder or another checkout's page. The stand-in logs a read of the
  API marked as a prefetch apart from the page's own (`Site::read_ahead`, `asked_ahead`;
  `requests`, `targets` and `answers` leave it out), so a test counting a page's reads is not
  raced by the reads ahead; `apps/symphony-web-tests/tests/browser/cache_ahead.rs` covers them.
- **Budget.** `/` and `/tasks` each have a 300 KB first-visit gzip budget and
  a 20 KB return-visit budget (`apps/symphony-web-tests/tests/browser/`, restored after the port;
  both the owner's, the first-visit cap raised by the owner to 300 KB, and
  never loosened by an agent). Home's first visit is held
  to 115,000 bytes (`HOME_FIRST_VISIT_BYTES`), the task board's to 150,000
  (`TASKS_FIRST_VISIT_BYTES`) and a task's page's to 145,000 (`TASK_FIRST_VISIT_BYTES`), each
  lowered by its move and again once the chat moved and no kind was left to move; the board's
  alone was raised since, from 125,000, by the owner on 2026-11-19. Each is raised only by its
  task's orchestrator's recorded decision once the cut came first, naming the constant
  (`just tasks gate SLUG apps/symphony-web-tests/tests/browser/main.rs:NAME --raise NEW`,
  [TASKS](../../docs/TASKS.md#gate-decisions)), never by an agent's own word.
  A visit counts each response the page loaded at what Chrome's
  network events say it took on the wire, gzip body and response head: the
  HTML, the shell, the kind's scripts, the style sheets, API replies
  (refusals too) and the live feed's head and content messages, as the page
  reads them. The stand-in server sends the build as the server does, gzipped
  at the best level by the first request for each file and kept for the test
  run, and the page and API replies at the default level
  (`apps/symphony-web-tests/tests/browser/site.rs`): a start reads the shell alone, and a run gzips
  the files its pages ask for, the whole published folder, older builds kept
  and all, only for the canonical-entry test's report.
  It leaves out document-content images, browser-cache hits and
  the feed's keep-alive pings, which arrive every `ping_ms` for as long as the
  page is open and are reported beside the total: leaving the pings out is
  the owner's choice, as the caps are.
  Other page kinds are reported separately for review.
  On the fixture (`entry_pages_fit_the_first_visit_budget_and_deferred_kinds_are_reported` prints
  each page's requests) a first visit to `/` takes 112,818 bytes, `/tasks` 126,128 (the cards'
  costs' read among them), `/agents` 116,078, `/queue` 108,694, `/reports` 115,859, `/runs`
  111,747, `/docs/` 139,525, a document 141,339 and a task's page 143,621, and a return that finds
  what it kept unchanged 3,228, 3,786, 3,557, 3,492, 1,756, 2,217, 1,801, 1,975 and 3,622. A first
  visit to a kind is its entry and its
  sheets, the chunk every kind shares (React and the frame) and the pieces it shares with some of
  the others (the board's feed and keep; the card and the task readers of the board and the agents
  page; the media layer's chunk and sheet, home's, the documents', a task's page's and the chat's),
  beside the HTML, the shell and the page's reads; a move to another kind takes its entry and the
  pieces no kind before it took (`apps/symphony-web-tests/tests/browser/preload.rs`). A task's page's timeline, a chunk of
  its own, comes once its box nears the screen and its part has something to draw
  ([A task's page](#a-tasks-page)), and is reported apart from its first visit: 16,825 bytes. The
  reads ahead start only once the page's own reads have answered (`cache_ahead.rs`) and go marked
  as prefetches, so the budget counts them outside the first visit, with the warmed modules
  (`prefetched_bytes`). Once a page has its data and is idle, the shell warms the chat's files
  (its entry, the chunks it imports and its sheets the page has not) and then the menus' (the
  same of theirs): 29,714 bytes outside home's first visit, and neither the chat's first open nor
  the menus' transfers anything more. Later visits reuse cached imports. The home budget test
  is independent of the canonical-entry test, which gives every kind a fresh
  browser profile and reports the complete published output.
  Measure loaded resources, including API data, rather than summing every
  deferred module or an older build's file kept in the published output.
  The 300,000 and 20,000 byte limits are the owner's; a lower first-visit
  line needs an architectural change the owner approves.
  `report.sh OUT` prints the bundle report of the build in OUT, read from Vite's manifest
  (`src/bundle-report.ts`): each kind's first visit, the shared stylesheet, its sheets, its script
  and the chunks it imports, and apart what it imports later, file by file as built and gzipped,
  then the build's every file once. It says what a build's files weigh and why a kind grew; the
  budget test stays the measure of a visit, as it counts the page, the shell and the reads too.
- **Controls and themes.** The shared controls (`src/button.tsx`, `src/input.tsx`,
  `src/textarea.tsx`, `src/select.tsx`, `src/tabs.tsx`, `src/tooltip.tsx`) draw the markup and
  classes of DioxusLabs/dioxus-components at revision 9a758255 (`dx-button`, `dx-input`, …), whose
  sheets this package carries under `src/styles/` with its licences
  ([its README](src/styles/README.md) records the local adaptations). A select's popup and the
  picture dialog are the menus' (`src/overlay.tsx`). The header's Appearance button opens the
  panel's `#appearance-panel` (`src/appearance.tsx`, as the bell opens activity's), whose theme
  menu imports a shadcn/tweakcn registry JSON file's `cssVars.theme`, `cssVars.light` and
  `cssVars.dark`, including fonts and radius. `src/theme.ts` validates token declarations; the
  shell restores their saved CSS before mounting a page. Status tokens such as `like`, `question`
  and `live` keep their defaults unless the imported file sets them. UI colours use the canonical
  shadcn token names; `src/styles/controls-theme.css` maps those names to the copied controls'
  palette. Extend the shared controls for new fields. A text field is 16px on a touch screen
  (`(pointer: coarse)`, `!important` in `src/styles/board.css`, so no page's own sheet linked after
  it undoes it): iOS zooms into a smaller one as it takes focus and does not zoom back out
  (`apps/symphony-web-tests/tests/browser/touch.rs`). A press is the control's own `:active`,
  which sinks it, with no tap flash and no label a long press selects, while an unseen layer
  keeps its whole box a target, and a hover that moves or raises a thing sits under `(hover:
  hover) and (pointer: fine)`, as a tap would leave it raised. A select's popup grows from the
  edge nearest its trigger (`data-side`, `src/popup-position.ts`), stays drawn closed while its
  exit eases out (the overlay host lets it go at its `animationend`, `src/overlay.tsx`), and under
  reduced motion only fades (`apps/symphony-web-tests/tests/browser/polish.rs`).
- **Task views.** The task board (`/tasks`, `src/board.tsx` over `src/boardlive.tsx`, its cards
  and wait lines in `src/boardwait.tsx`), the agents page ([below](#the-agents-page)) and a task's
  page ([below](#a-tasks-page); [TASKS](../../docs/TASKS.md#gates) and [the timeline](../../docs/TASKS.md#the-timeline)).
  The board implements the owner's option B (`docs/analysis/board-design/b-stuck-in-place.html`):
  blocked tasks stay first in their original stage, ordered by named wait kind then age, ordinary
  work oldest first, parked last and finished newest first. An unnamed legacy block is ordinary
  work. Lifecycle, Build and Test columns scroll horizontally at a minimum width of 280 px,
  including beside the chat. Above them a row names every stage (`.stage-jump`), marking the
  lanes the scroller cuts (`laneSide`, read at each scroll and each change of its width); a
  stage's button scrolls its lane whole into view and puts the focus on its heading, so the
  next Tab is in that lane. Backlog opens from the toolbar and cancelled work from Done, each
  as a list in the board's place, with Back to stages. Search and the finished window are behind
  Search and filters, collapsed initially at every width.
  A focused card that moves keeps its title focused, opening its destination where needed;
  unrelated moves keep the chosen stage. At 760 px and below one stage is mounted, Review
  first, with the chosen tab saved per project (`board-waiting-stage`, `src/storage.ts`). There
  the header's pages, the wait strip and the stage tabs are each one row that scrolls across what
  does not fit it (`src/scrollrow.ts`, `.scroll-row` in `src/styles/frame.css`): the chosen one,
  and one that takes the focus, is brought whole into view; an edge fades only while the row
  continues past it, the row itself and no colour over it; and a choice not whole in the row's
  window is drawn as its empty shape, so no label is read in part
  (`apps/symphony-web-tests/tests/browser/phone_labels.rs`, at 320 and 390 px). The Symphony's
  seven pages fit at both widths, and below 375 px the bell is drawn alone, leaving the first row
  to the project's name. The
  complete wait-kind strip filters to one paged list across stages; there is no Blocked column,
  sign-off lane, Status select or column pager. Search and the finished window remain.
  `/api/task-board?view=waiting` supplies complete facets and per-column ordinary/blocked counts;
  no count is inferred from loaded pages. Cards show title, slug/priority, what they wait on,
  stage age and reason, with no role or document chips. A slug, wait or reason too long for
  its line keeps its end, the ellipsis between its two parts (`ends`, `src/boardwait.tsx`), so
  cards that differ only at the end are told apart, and each card's one button
  (`.card-more`, `aria-expanded`) opens it in place to the whole of its title, slug, wait and
  reason, by key or touch, reading nothing. Closed, each of those lines folds every run of white
  space in it, a line break among them, to one space (`fold`), so a reason written on two lines
  stays on its one line in the card's box; opened, each is drawn as written. A positive authoritative `review_rounds`
  adds one `1 review` / `N reviews` mark before the title, on open and finished cards; zero or
  unavailable summaries show no mark. Live fields update known counts; a full replacement clears
  an unavailable summary. No task detail read or subtask count supplies the mark. A card whose
  task and subtasks took tokens shows their estimated cost before its time, on the time's line
  (`$1.23 · 1 h`; `$1.23+` where some had no price, `unpriced` where none had; its title says it
  is an estimate and of what day's prices). The listings carry none of it: each card names its task as it mounts
  (`useCardCost`, `src/card-costs.ts`), and the board asks for the cards named in one turn in one
  read of `/api/task-costs` apart (`CardCosts`, at most `TASK_COSTS_MOST` slugs a read, under the
  mount's signal), each once until its record's `updated` moves, so a heartbeat that adds tokens
  reads nothing. A read that fails leaves its cards without a figure until they change.
  The one orchestrator banner is amber
  for stale holders and red when none is there, absent while one is live or no ready work waits.
  Counts load first, then nearby column pages; finished work defaults to 24 hours with a week switch. A feed event
  that changes what a view lists (a task, a message
  while searching, the counts) reads the counts again 750 ms on, then the columns, each its first
  page as long as the rows it shows (the listing's `rows`), so the pages Load more brought, and the
  reader's place deep in them, stay (`apps/symphony-web-tests/tests/browser/board_place.rs`); the machine's
  work does only when it puts a task in another stage (`MachineWork::places_alike`), not at each
  heartbeat. An answer is dropped only for another view (search, window, connection, board
  version, `BoardData.view` in `src/boardlive.tsx`) or a newer read of the same part, never for an event
  that came while it was read: the events since are replayed onto it, and the refresh reads again
  (app/board-search-slow-to-fill: a loaded machine's work, every second, kept a search's columns
  grey for 5 to 60 s; `apps/symphony-web-tests/tests/browser/board_search.rs` counts a search's reads). Composer drafts are local to
  their component and saved through `src/storage.ts`, never replaced by a
  snapshot. Routes use `src/route.ts`, and requests `src/request.ts`, so a stalled save times
  out with its draft kept.
- **Visibility.** `NearViewport` (`src/viewport.tsx`) uses the shared visibility latch, with a CSS sentinel
  extending 600 px around its placeholder. The latch owns an IntersectionObserver until
  revelation or teardown, and reattaches after props replace its node. Markers include the
  page kind so the persistent panel cannot select a page's sentinel. A slot uses
  `content-visibility: auto` only once no sentinel is left in it, its own or a nested
  slot's (a column's rows): paint containment would clip the look-ahead. A slot that is
  or sits in a list item never does: paint containment would clip the item's
  marker, and layout containment would put it on a line of its own
  (`apps/symphony-web-tests/tests/browser/look_parity/list_and_queue_spacing.rs`). Mounted rows
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
  so its bars are still (`.viewport-placeholder` in `src/styles/board.css`). Chrome runs an animation off the
  screen every frame, under `content-visibility` too, so a page at rest animates nothing
  (`apps/symphony-web-tests/tests/browser/skeleton.rs` counts `document.getAnimations()`). A page says once that it
  loads (one `role=status` line, the gates' outline under `aria-hidden` on a task's ghost) and is
  `aria-busy` while it loads, as a column reading its first page and a `Settle` not yet settled
  are, and no longer once its read failed (`data-ready=failed`), when it says so, drawn:
  `symphony shot` counts a page drawn only once no region busy shows on the screen, so its picture
  is of the page and not of its ghost, and a page left busy after it failed would hold the shot to
  its timeout.
  A ghost uses its own classes, never the real ones, so a test's selector never finds it;
  the one placeholder it shares is a task's gates' outline, the section's own until its
  answer, which `GhostTask` draws for the room this browser kept (`GhostGates`,
  `src/gates.tsx`).
  The board, the queue and a task page draw a return visit from this device's cache (`cache`,
  `main[data-cache=device]`) with no ghost once its store answers; theirs shows on a visit the
  device has no copy for (a first one, a store that opened late or let the page go), and the
  browser tests of their next ghost turn the store off (`NO_STORE` in `apps/symphony-web-tests/tests/browser/main.rs`).
  What a first visit cannot know a ghost keeps from the last one, per project in the
  browser: the board's columns and counts, kept once the server has answered both its counts and
  the machine's work that places its build and test columns (`SHAPE`, `boardShape` and
  `keepShape` in `src/board.tsx`; none kept is every column with a card).
  The board's cards reserve two title lines and their wait line in a fixed 110 px box plus an
  8 px gap (`ROW`, `src/board.tsx`, `src/styles/boardwait.css`), so no height polling or chip-row
  cache is needed: only a card the reader opened grows past it, in a row already drawn, and
  the ghost keeps the stage row's room (`.ghost-jump`); the queue's rows
  per group (`ROWS` in `src/queue.tsx`) and each
  group's height; a task's head, title, facts and brief at the height drawn in a window as wide
  (`drawnHeight`, `rememberDrawn` in `src/viewport.tsx`), and its gates' room
  (`kept`, `remember` in `src/gates.tsx`). A
  task never opened at that width takes the last task's (`recallHeight` falls back to the
  newest part of the same kind); with none, a head with a facts line and
  a title of one line wide, two on a phone, four gates and a brief of seven lines. A brief kept
  shorter than those seven lines draws only the bars its box shows whole (`GhostBrief`,
  `src/task-page.tsx`). The timeline's box takes the height `reserved` gives it
  (`src/timeline-slot.tsx`). What is kept is measured
  as it draws, every 100 ms, and kept again when it changed (the task page's `rememberDrawn`),
  until no region in it still waits within the sentinel's reach of the screen (`waitsNear`), for
  `KEEP_MS` (30 s) at most, a bound on the work alone: on a loaded machine or a slow
  phone rows draw many seconds after their data. A
  region still waiting carries `data-pending=true`: a `NearViewport` placeholder, a document's
  section, a column's first page, a task's thread. What a page already knows shows at once
  (a task's head and brief, a document's head and outline); `Settle` (`src/viewport.tsx`) keeps only
  the part still reading (a task's thread and composer, a document's body) hidden under its
  ghost (`cover`) until no such region shows on its first screen, its read fails, or
  `SETTLE_MS` (3 s) passes; a hidden control takes no focus, so a
  browser test waits for `visible(SELECTOR)` before it focuses one. The task board shows its
  heading and controls at once, each
  column reserving a card's box per row of its first page (`PAGE` in `src/board.tsx`), at the
  fixed pitch `ROW`, with the same title, slug and wait-line bars as its card; it shows Load more
  only once the first page is in. A row not yet near the screen reserves that same box. A first
  read that fails
  with nothing drawn yet (a board not made, say) is said in place of the board's ghost, which
  would wait for ever, in the read's words with a retry (`UnreadBoard`); the page reads
  again by itself too. A live update that adds or
  removes a row calls `holdPlace(ROWS)` (`src/shared.ts`, the shell's
  `reviewHoldPlace`) before it
  sets its data: once the reader has scrolled the box around what they read or the page, the
  focused field, else the first of ROWS on the screen, stays where it was while the update
  and any placeholder within the sentinel's reach draw, until the reader's own scroll, click
  or key; a reader at the top sees what lands at the top.
  `apps/symphony-web-tests/tests/browser/layout_shift.rs` fails a page whose cold or warm load scores a layout
  shift of 0.05 or more, on a phone or wide. That score sees only elements already drawn
  that move, not a ghost's box against the new element in its place. A task's parts are the
  page's own elements from their mock-ups on, so the score does see a part's guess give way to
  its answer there, and the file holds a first visit in each order the page's and the gates'
  answers can come in, one route held until the other's answer is drawn. A task with the gates
  the outline guesses for a task this browser knows nothing of (four signed gates, one waited
  on) stays under 0.05 in both. A task of another shape (two gates, one unsigned) stays under
  0.05 on a return and on a wide first visit, and on a phone's first visit moves what is under
  its gates once, the move the owner kept (the card `task-first-visit-head-first`). Both tasks
  are measured under each colour scheme, which the test sets itself and never takes from the
  machine, as the box a shift moves holds the card's shadow and the light theme's is the
  larger: dark it is held under 0.14 with the page's answer first (it scores 0.1399) and under
  0.064 with the gates' first (0.0636), light under 0.156 (0.1556) and 0.069 (0.0689), the same
  elements moving the same 32 px. Those four limits only go down. For every other ghost:
  `apps/symphony-web-tests/tests/browser/landmarks.rs` compares each region's place on the ghost and on the page,
  on a first visit and the next, on a board, a queue and tasks shaped as the live ones (a
  later card, a later queue group, the board's lower row, a task opened first and after
  another), and prints each offset as a `LANDMARKS` line; it measures the board and the queue
  again with every `IntersectionObserver` callback 800 ms late. No scrollbar takes room there
  (`NO_SCROLLBARS`), so the kind Chrome draws cannot change every region's width. `apps/symphony-web-tests/tests/browser/skeleton.rs`
  covers the rest.
- **The page.** `package.sh` writes the shell, whose single history adapter is
  `shell/shell.ts`: the page kinds' manifest on its first line (`const modules =
  …;`), then the store and that script minified by the minifier Vite ships (`shell/minify.ts`).
  Every kind names its script, the chunks that script imports (`imports`) and its styles:
  the shell imports that script, a module exporting `run` and `stop`, with a module preload of
  each chunk it has not asked for yet beside it. Its
  `html[data-navigation]` is `loading` until the shell's navigation finishes,
  then `ready`, or `failed` if the page cannot load. It preserves a scroll made
  after the new page mounts instead of applying its late top reset. A scroll to the
  element the address names, its own or a web kind's (`route.ts`), is held there while
  the page draws around it (`land`): the sections a landing brings near draw after
  it, often taller than their ghosts, and the browser's own scroll anchoring, which holds
  nothing under a page's ghost (`.settle`), is off while it holds (`overflow-anchor` on the
  root, the root's own value put back after), as an adjustment of its own would scroll the page
  and the element alike, a scroll away to the hold. A landing is one hold for each navigation.
  It ends when it goes quiet (nothing moved and nothing waiting or busy within reach for four
  frames, or 1800 at most), once no element has its id, at the reader's input that scrolls (a
  wheel, a touch that moves, a press on the page's scrollbar, a scroll key; not a click, a tap, a
  selection or another key) and at any scroll that moves the page and the element alike (find in
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
  deferred import finds its build two publications on. The server embeds every file kept, and
  the size check's client line counts each kept build whole
  ([CHECKS](../../docs/CHECKS.md#size-budgets)): a change to a module of the chunk every page
  imports renames every chunk that imports it, so the build after it shares almost no file with
  the builds before it, and the folder weighs up to `KEEP` builds. Once its build is gone, an
  import that fails asks for the page's own shell past the cache: a 404 loads the
  page again, onto the build served, once per build in the tab's storage
  (`gone` in `shell/shell.ts`); anything else says the failure with its retry.
  A kind's `stop` unmounts it and awaits its cleanup before another kind starts, its feed and
  event subscriptions among it. Back and forward use this same adapter. The persistent panel remains mounted once opened,
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
  select's popup and the picture viewer are drawn by the menus, the persistent
  kind `overlay` (`src/overlay.tsx`, mounted in `#overlay-root`), never the
  chat's: the chat's select field and its media layer ask for them by data through the shell's
  `reviewOverlay`, as every page does. The chat is the persistent kind `panel`
  (`src/panel.tsx`), mounted in `#quick-root`. A task's timeline is a chunk its box imports
  ([A task's page](#a-tasks-page)).
  The shell requests a kind's script, a module preload of each chunk it imports and its styles
  together. Once the first page has its data (the shell
  counts the page's reads in flight, its `fetch` reads and its lookups in the device's cache
  (`symphonyCache.read`), which a page awaits before it asks the server for the rest, and waits
  until none has been for half a second, and after a read that got no answer until one does)
  and the browser is
  idle, it prefetches the persistent kinds' files at the lowest priority, nothing evaluated,
  each one the page has not linked: the chat's script, the chunks it imports and its styles
  first, the menus' once the chat's script (or the chat's own load, when a click came first) is
  in, so no two share a slow link. A browser asking to save data, or reporting a
  2g link, warms nothing; the browser tests count those prefetches apart from a
  first visit.
  The style sheets every kind loads
  are one `shared-<hash>.css`, first in each kind's list of the manifest, and the
  kind's own follow it: a first visit asks for them once, and every kind caches
  the same file. Its sheets and their order are `src/styles/shared.css`'s. The pictures' styles
  (`src/styles/media.css`) stay out of it: they are the media
  layer's sheet, which home, the documents, a task's page and the chat take as one file, and the
  queue, Reports and the task board, which show no picture, none. The server's own pages inline
  `board.css`'s tokens and `frame.css` from copies in its package
  (`apps/symphony-server/src/`), which it carries to crates.io: a change to either
  is copied there too, and `crates/symphony/tests/packages.rs` fails until it is.
- **Projects on one origin.** The projects
  of the machine's one server share its origin, so every key the browser
  stores takes the page's project id first (`stored`, `src/project.ts`,
  `alpha:review-quick-draft`; the shell's `stored` for the saved theme); a
  server of its own keeps its keys as they are. Every link the client draws, and
  the address of a document's or a card's picture, is written under the project's
  base (`href`, `src/project.ts`, `/alpha/task/…`), so a new tab, a copied link or image
  address or a long-press opens what a click does; the shell follows a link under
  the base in place and adds nothing to it, leaves any other to the browser, and
  the project switch's `/` alone leaves the project.
- **Media.** Typed document images carry real dimensions and thumbnail URLs from the
  API (home's pictures reserve a square). The shared visibility
  latch defers native media shells and gallery rows. The media layer that sizes mounted frames
  and opens pictures at `data-full` in the menus' viewer is
  `src/media-layer.ts`: each page that shows media mounts it, and the chat
  mounts one for its own pictures on every page and, on the pages that have none of their own
  (the task board, the agents page, the queue, Reports and Runs), for every mockup and viewer
  and the page's pictures; it is in the chunks of each, so it loads, or
  fails, with them. Mockups stay at 1280x800,
  scaled by their card's width, with only `allow-scripts` in their sandbox. Never add
  `allow-same-origin`.
  A file a plugin's renderer draws in a viewer is a `Media::Viewer`, drawn by
  `FileViewer` (`src/media.tsx`) from its `symphony_api::Viewer` data alone: the custom
  element (its markup from `Viewer::element_html`, every value escaped), its
  caption, a button per view and the download link. The client holds no
  viewer's code: the module script the viewer names is loaded once, on the first
  mounted stage naming it, and is absent from the shell's preloads; a view's
  button sets the attributes it lists on the element and calls the method it
  names. A plugin's pages are the
  `ProjectView`'s `nav` items the served page inlines (`PROJECT_ELEMENT`): each
  reads its document through the documents' page (`src/document-page.tsx`), as a `/shelf` page reads
  `docs/SHELF.md`. A `Media::Gallery` holds its items (name, file `src`,
  `poster`), served under the plugin's route, `/x/shelf/`, and the viewer they
  open in. Its grid has only small pictures until an item opens; one selected
  item is mounted at a time, and closing it unmounts the viewer.
  The server warms read snapshots before announcing its URL, and a gallery
  plugin warms the selected checkout's gallery cache at startup and when a
  folder its watch names changes. Startup logs separate gallery, document and
  board time; requests never walk model history or other checkouts to build
  the gallery.
  `DocumentPicture` (`src/media.tsx`) preserves typed document image
  dimensions, presentation and `data-full`; until its latch opens its `img` holds
  the empty picture `WAITING` as its src, never none, so it is a sized picture
  from its first frame and the latch only changes that node's src, keeping a keyboard
  focus on it. A src-less img is drawn as its alt text, and Chrome rebuilds its box
  when a src comes: a rebuild asked for in the frame an ancestor slot takes
  `content-visibility: auto` is lost, so the picture never loads (a folded card
  opened by a click). The picture viewer skips a waiting img. `MediaView` renders native
  media from either `Block::Media` or `Span::Media`.
  A document's questions and follow-ups (`src/asks.tsx`) are read
  with `format=blocks`, no rendered HTML on that path.
  A document's questions and the `+` sheet's chat read again when the page's feed
  says so (the chat by `concernsThread`, `src/panel-model.ts`): its `asks` region names a moved thread (by id
  and document path) or a new chat recipient, or a stream opens, first or after a loss. A
  document page that opens its stream again for other sections, from the cursor of the last
  event it heard, missed nothing, and reads nothing again (`followDocument`). The page
  module forwards that region to every module as a `review:asks` event; a read
  that fails tries again every 3 s until one answers, and none polls. A document's
  question read the next one replaces aborts its fetch (`getJson`'s signal): a fetch whose
  body the page never reads stays in flight in Chrome's eyes for as long as the
  page is open, and `symphony shot` waits on it.
  The media style is `src/styles/media.css`; every observer and listener the layer adds is owned
  by the kind's mount and ends with it.
- **Documents.** The page's contract ([The documents](#the-documents) names its code).
  `document-live.ts` reads `/api/document?view=outline`: section keys,
  source positions and anchors, the kinds of each section's first and last blocks (`ends`), with
  short card summaries. `NearViewport`
  mounts each block once near the screen; a collapsed
  card requests its body only when opened. Mounted blocks request their bodies
  and threads together from `/api/section?format=blocks&keys=JSON_LIST`. Excerpts
  request `format=blocks` too; reads without it retain the legacy HTML/media
  response for compatible API callers. Canonical `/docs/` pages use the client;
  old `/app/docs/` and `/doc/docs/` bookmarks redirect there.
  Only Markdown files, HTML documents and folders are client routes; raw files keep normal links.
  An HTML document (docs/PROJECT_FILE.md "HTML documents") is no fragment: its
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
  what an HTML document does is the server's and the browser tests'.
  The feed opts into
  one `document=PATH` and its requested `sections=JSON_LIST`; a stream without
  a document receives no document payloads. It names the sections near the screen and those
  whose bodies the page holds (`followed`): on a return visit the second are the ones drawn from
  this device, there before the outline's check answers, so a return opens one stream, naming its
  sections, though their slots name themselves only a frame or more later. A section near the
  screen the stream does not name reopens it at its current cursor. A body response waits for that cursor and
  never overwrites a newer document change; a reconnect invalidates outstanding
  reads. Drafts and verdict retry ids are saved locally. Busy sections retain
  their markup while their source positions advance; the queued markup applies
  when the form or excerpt is free. The owner's own saved answer on a card
  releases that card (`release`, `held`, `settle`, `src/document.ts`): its mark,
  line and the register's regroup show at once, an open excerpt beside it or
  not. The release lasts until the card shows one more line of the owner's than
  its latest copy had when the save was sent, so an agent's reply the feed
  brings first does not end it. A closed
  draft holds its card only once the card is gone, never the register's order.
  Content is `symphony_api::Block` and `Span`, drawn by
  `BlocksView` and `SpansView` (`src/blocks.tsx`). Server reads
  prepare media and scoped URLs before `symphony::blocks::parse`; HTML fields
  belong only to that internal pipeline and are excluded from JSON. Links
  carry typed excerpt targets; SVGs are primitive data. Unsupported source
  markup stays visible as an Unknown node with a server warning. `symphony blocks`
  prints a real-document census and checks that the data survives serialization.

## Adding a page kind

1. Draw the page in a module of its own (`src/NAME.tsx`), its reads guarded and its pure parts
   exported for the unit tests, with an entry in `src/kinds/NAME.tsx` and the name in `KINDS`
   (`vite.config.ts`); its route is the shell's (`shell/shell.ts`) and, for a route the Symphony
   answers itself, `symphony_api::SYMPHONY_ROUTES`.
2. Give it browser tests in `apps/symphony-web-tests/tests/browser`, and name it in the build
   test's kinds (`tests/build.rs`) with the sheets it loads.
3. Report its first visit beside the other kinds' (the budget test prints each page's requests),
   and hold it to a cap where the owner sets one.
