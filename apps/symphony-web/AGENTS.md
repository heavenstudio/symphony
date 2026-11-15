# apps/symphony-web/

The Symphony's web client in React and TypeScript, built by Vite. It takes the pages over from
the Dioxus client ([its guide](../symphony-ui/AGENTS.md)) a page kind at a time: a kind moves
whole, feature for feature, in one landing, and its Dioxus sources leave in the same commit. The
review queue (`/queue`) is the first kind it serves, Reports (`/reports`) the second, home
(`/`, the kind `review`) the third, the documents (`/docs/`, each document, folder and HTML
document an entrypoint serves, the kind `docs`) the fourth, the task board (`/tasks`, the kind
`board`) the fifth, the agents page (`/agents`, the kind `agents`) the sixth and a task's page with
its timeline (`/task/SLUG`, the kind `task`) the seventh: no page is left the Dioxus program's,
which keeps the chat, the menus and the media layer every page mounts. The board's task card is a
component of its own (`src/taskcard.tsx`), which the agents page draws too, and whose status words
and machine work a task's page draws. Like that
client it reads the server's JSON API and never runs server code or inserts server-rendered HTML.

**Where a UI improvement is built.** A UI improvement (a new control, a redesign, a change of
look or behaviour) is built on its page once that page's kind has moved here, in this package,
never on its Dioxus page: until the kind moves, the improvement waits for it. The one exception is
a fix on a Dioxus page for a bug that loses data or blocks use, which is made where the page is.

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
  one checkout taking turns (`tools/check_lock.sh`). `.npmrc` saves exact versions, runs no
  package's install script (`ignore-scripts`; `fsevents`, Vite's watcher on macOS, ships its
  binary built, and a script `npm run` names still runs) and asks no outside service anything but
  the registry.
- **React 19** (`react`, `react-dom`), the only run-time dependencies.
- **TypeScript**, strict (`tsconfig.json`: `strict`, `noUncheckedIndexedAccess`,
  `exactOptionalPropertyTypes`, `verbatimModuleSyntax`), checked by `tsc --noEmit`. It emits
  nothing: Vite strips the types. `skipLibCheck` leaves the packages' own declarations unchecked,
  which name Node's types this package does not take.
- **Vite** builds the kinds (`vite.config.ts`): one entry per kind, `src/kinds/NAME.tsx`, named in
  `KINDS`; JSX by Vite's own transform, so no React plugin; no public folder and no module
  preload of Vite's, as the Symphony's server and shell serve and load the files. Its dev server
  is a developer's alone ([The dev server](#the-dev-server)).
- **Biome** lints and formats (`biome.json`: its recommended rules with the React and test
  domains on, the formatter at two spaces and 100 columns). It was chosen over ESLint with
  typescript-eslint, whose TypeScript support ends below the TypeScript this package pins, and it
  is one tool and one file for both jobs. `biome ci --error-on-warnings` makes a warning an error.
- **Vitest** runs the unit tests, with `@vitest/coverage-v8` for their coverage.

`just client-lint` (`check.sh`) runs the wire types' check and the tests of the example that
writes them, Biome, `tsc --noEmit` and the unit tests with their coverage, each whatever the ones
before it did, and `just lint-rs` runs it, so a
task's gate and the release run's lint both do. `just client-wire` writes the wire types.

## The wire types

`src/wire.ts` holds the TypeScript of the types the client reads from `crates/symphony-api` and
the constants it names (the feed's path and events, the Symphony's routes, the cache's format), as
that crate's `typescript` example prints them through ts-rs (the derives sit behind the crate's
`typescript` feature, which no build of the server or the Dioxus client turns on). It is never
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
`just test-rs -p symphony-api --features typescript --example typescript`). TypeScript that carries the Dioxus client's own Rust
(`apps/symphony-ui`), not this crate's, is held by the browser parity tests alone.

A read is checked where it arrives (`readQueue`, `readPageBoard`, `readReports`,
`document-read.ts`, of the checks `blocks-read.ts` exports): a guard checks the fields the page
draws, each of its type, and nothing else, so a field a newer server adds, or one it drops that
the page does not draw, changes nothing, as the Dioxus client's serde reads skip
unknown fields. A read the guard refuses is a failed read, said and retried as any other. A feed
event of a region or kind the page does not know is not its own, and is left alone.
`JSON.parse` costs no code per field, so a field the page does not draw, a task's plugins' board
fields among them, costs it nothing to receive: its types say what the server sends, and its
guards read what it draws.

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
Vitest's fake timers, never the clock.
What only a browser shows (effects, the feed, the layout, the moves between kinds) is the headless
browser target's (`apps/symphony-ui/tests/browser`, [CHECKS](../../docs/CHECKS.md#the-browser-target-by-page)),
which opens a moved page as it opened the Dioxus one and is its parity check. A kind's entry
(`src/kinds/`) and the generated types are left out of the coverage, which `npm.sh run test`
prints.

## The build and the distribution

`apps/symphony-ui/bundle.sh` builds this client as its kind `web` (`npm.sh run build`, into the
build's own folder), beside the Dioxus kinds, and `package.sh` puts both in one distribution: it
copies Vite's assets into the stage, so one `builds.json` names every file of the build, and adds
each kind to the shell's manifest from Vite's (`.vite/manifest.json`): its script and its styles,
the shared sheet first and then the CSS its entry imports, and no wasm. It refuses a kind both
clients build, and a build whose web kind left no Vite manifest. Files are named `NAME-HASH16.EXT`
with a hex hash, as the server caches such a name for good. No path, clock or environment enters
the bytes: the build script sets `NODE_ENV=production` (a shell's `development` would build
React's development code) and Vite reads no `.env` file (`envDir: false`), which
`tests/wasm_build.rs` holds, so the build is the same from any checkout and shell. It builds from
the checkout, not from the release snapshot the Dioxus build uses, and `tools/client-key.sh` adds
this package's hash (every file git lists here but its guides) to the snapshot's name, so the
regenerator rebuilds the distribution when either changes. The server embeds the files as it
embeds the rest, and needs no Node to run.

The shell imports a kind of this client with `import()` in place of the Dioxus glue's init: its
entry exports `run`, which mounts the kind in `#review-root-NAME` and draws it before it settles,
and `stop`, which aborts the kind's mount and unmounts it, its feed and reads with it. What the
kinds share (React, the frame) Vite puts in a chunk their entries import, named after one of its
modules, so a first visit takes the kind's entry and that chunk, and a move between two kinds of
this client the target's entry alone. The shell asks for the chunks an entry imports beside it, a
module preload each (`imports` in its manifest, which `package.sh` reads from Vite's), so a first
visit fetches them with the entry rather than once the entry has come and been read: a round trip
fewer, which a phone over the tailnet pays.

**Work that outlives its start.** Async work a page starts checks, after each wait and before it
touches the page, the window, the storage or the address, a signal that ends with what started
it: an effect's own (its cleanup aborts it), or, for a handler or a read with no cleanup of its own
(a click's post, a document's versions read), the kind's mount (`useMount()`, `src/mount.ts`, which
`stop` aborts). A kind the shell stopped does nothing more: Later answered after the reader left
moves no one, and a late read writes nothing over the next kind's. A Dioxus page had this from
its framework, a spawned task dying with its component; here each such wait says it. The device
cache's store is the shell's (`assets/cache.js`, `symphonyCache`, which `src/cache.ts` names what
to keep in), and it opens every IndexedDB transaction: this client opens none of its own.

A move between a kind of this client and a Dioxus kind stops one and runs the other as between
two Dioxus kinds; a move within a kind is the shell's `review:navigate`, which `route.ts` follows. The header, the panel's `+` and bell
and the feed are drawn here as the Dioxus frame draws them; the panel, its sheets and the overlay
stay the Dioxus program's kinds, which the shell mounts beside any page. A page that shows media
mounts the media layer (`media-layer.ts`) before it: started by the first media it shows, it scales
the mockups' frames, loads a viewer's script, runs the views' buttons and opens a picture in the
overlay's picture viewer, which it reaches through the shell's `reviewOverlay` as the Dioxus
pages do, all of it ending with the kind's mount: one runtime for both kinds. A document's
contents watch (`contents.ts`: the outline folded on its own while a drawn table is wider than its
box, unless the reader chose) is no part of it: it runs from the mount of a body that has the
outline to its unmount, under the signal of that body's own effect (`DocumentBody`).

A read the page gives up is given up on the wire too: every read in `src/` is passed the signal of
what asked (`getJson(url, signal)`, `getIf(url, etag, signal)` and `getDocument`, all through
`exchange`), so a read the feed's news replaces, or one a page left behind, is aborted rather than
answered into nothing, and its caller still checks that signal after each wait, as an answer may
come first. A post takes none: the server may have acted on it. A text box (`textarea.tsx`) hears each
`input` event as its change, as the Dioxus box's `oninput` did: React's `onChange` compares the
box with the value it last saw, and so misses a value a script set before firing `input`.

Styles are the existing sheets, used as they are: the project's `theme.css`, inlined by the
server, the shared sheet of the Dioxus build (the frame, the components, the board's rules), whose
class names this client's markup uses, and the kind's own sheet, imported by its module
(`src/queue.css`, `src/docs.css`). The Dioxus client's media sheet,
`apps/symphony-ui/assets/media.css`, which the shared sheet leaves out and the program's media
layer loads as well, is the media layer's, imported by `media-layer.ts`, so home, the documents and
a task's page take it as one file under the layer's name. The task board and the agents page have
no sheet of their own: their rules are the shared sheet's (`apps/symphony-ui/assets/board.css`),
whose task card rules the agents page's cards use too. A component drawn by a Dioxus component
library keeps its markup and class
(`button.tsx` draws the shared Button's `dx-button`).

Blocks are drawn as the Dioxus client draws them (`blocks.tsx`, `media.tsx`) but in two ways React
has its own. A presentation's style is written as given, by a ref (`writtenStyle`): React's `style`
takes an object, which it sets property by property, and would lose a value marked `!important` or
a declaration after a comment. As the Dioxus interpreter writes a style, a property the element
already had and the written style does not set (a centred container's `text-align`) is kept. A
`javascript:` address in an href or a src is replaced by React 19 with one that throws, where the
Dioxus client writes it as given: a difference in the safe direction, kept.

## The dev server

`just symphony-ui-dev SERVER [PORT]` shows this client's kinds as their sources stand, with no
client build, the page loaded again on each save: Vite's own dev server (`dev.js`, the Node around
`src/dev-server.ts`), on the loopback address alone at PORT (0, the default, for any), over a
Symphony server of the developer's own on port SERVER, which `just symphony --port 0` starts and
prints (`--root DIR` another project's). It refuses 8765, the shared server's, as either port, and
starts nothing: Ctrl-C, or a kill of the recipe's process, ends it. Every request but those for
Vite's own files (under `/@symphony-web/`) goes to that server through Vite's proxy, its Host and
Origin as the browser sent them, so a save meets the server's origin check as on the server itself:
the pages, the API, the feed (its WebSocket's upgrade too, `ws`) and the files of the Dioxus
program's kinds. It sends no CORS header
(`cors: false`, where Vite's default lets a page on any loopback port read every answer), so a page
of another origin reads through it no more than from the server. What it makes itself is the page's
shell script: this checkout's shell (`apps/symphony-ui/assets/cache.js` and `shell.js`,
unminified) with Vite's client first, the program's kinds as the server's build names them, and
each of `KINDS` as its entry under Vite with the server's shared stylesheet, each kind's own styles
kept before the project's theme as the built shell keeps them. So a kind added on a branch, or a
change to the shell, shows with no build too. A save to a kind's module or to the shell's sources
loads the page again, a style sheet's change applies in place, and an error in a module is drawn
over the page.

A reload, not React's Fast Refresh (`@vitejs/plugin-react`): that would be a dependency more and
one more script put in a page Vite does not serve, and it keeps a component's state only across an
edit to a module that exports components alone, which few of these do, and most pages here draw
again at once from the device's cache and keep their drafts on every input. What only a build
makes it does not show: the shared stylesheet and the program's kinds are the server's build's (an edit to the
Dioxus client's sheets, `board.css` among them, shows once `just symphony-ui-dist` has built them
and the server serves that build, `SYMPHONY_UI_DIST` naming its `public/` for another project),
React is its development build, and the bytes are no build's. No build, regenerator or check runs
it (its files count in the regenerator's key, as every file of this package does);
`src/dev-server.test.ts` holds what it does.

## Reports

`src/reports.tsx` draws `/api/reports` at `/reports`, the kind `reports`, in the frame, with no
feed of its own: it reads the window this device last chose (`reports-grain`, `reports-days`
through `storage.ts`), else the last 24 hours by the hour (`FIRST_WINDOW`), at once, on each
choice and every five minutes with the last answer's tag (`followReports`, under its effect's
own signal). The periods are read by path as the server writes them (`Bucket`): a source with no
record of a period is none there, a count by status that leaves out its zeros a zero. Four
figures sit over five sections of charts, one column on a phone and two from 960 px. Each figure
links to the chart it sums up (`#chart-done`…) while the charts are drawn. A chart's id is an
anchor of its `NearViewport`, held in its ghost until it mounts; the chart an address names mounts at
once and is landed as a document's part is (the shell's `land`, once a navigation), so a fresh
load's charts, which come after the shell looked for it, land too, until the reader chooses a
window: a first answer that drew no chart (nothing recorded, a failed read) started no landing,
and the window chosen after it lands none (`lands`, `NearViewport`).
`src/chart.tsx` draws each chart by hand, with no charting library: bars as boxes over a column
a period, lines as one SVG whose strokes keep their width, both on one axis with its unit; a
period no file recorded is hatched and its line broken, never a zero. Each entry of a chart's
legend is a toggle button (`aria-pressed`) that hides its series on that chart alone, kept in
every window by its chart's id and name (`reports-hidden`, a key a line): a hidden series leaves
the plot, the readout and the table, and the axis scales to the series shown (`scaleOf`), so
hiding a large one makes the rest larger; a period stays hatched only when no series, hidden or
not, recorded it. A tap or the arrow keys pick a period, the same on every chart, and its readout
says every value as text; each chart's table holds every period. Figures are written as the
Dioxus page wrote them, a tie rounded to the even digit as Rust's `{:.N}` does (`fixed`,
`words.ts`, which home's percentages use too). The
series take `--chart-1` to `--chart-5` (`src/reports.css`), set at no specificity so a theme's
own win, with their own steps in the dark. Charts below the screen mount through
`NearViewport`. Hours are labelled in UTC.

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
bytes, `whiteSpace` for `trim`), so its lists and lines are the Dioxus page's.

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
(`attach.tsx`, the same rows the Dioxus composers keep). A link's excerpt, once opened, draws its
blocks whole at once (`ExcerptBody` under `AtOnce`, `viewport.tsx`), no part of it waiting to come
near the screen: opening it asked for them, and its box, which scrolls, would clip the sentinel's
look-ahead, leaving a box opened below the screen empty until it was scrolled to. A card's Later
is the queue's (`postpone.tsx`), the related tasks and a branch's differences are `related.tsx`'s.
Its own styles are `src/docs.css`, the Dioxus client's documents sheet moved here.

## A task's page

The kind `task` (`src/kinds/task.tsx`) draws `/task/SLUG`: one live task per checkout, which
follows the route from task to task within the kind (`review:navigate`) and keeps one feed, as the
Dioxus program's page did. `src/task-live.ts` holds what it reads and follows (`LiveTask`, on
`live.tsx`'s `keepBoard`): the page and the thread's newest page kept on this device
(`taskItem`) and drawn at once on a return, each checked with its copy's tag once the board's
version is read and asked again until it answers; the rest of the thread, the subtasks and the
landing the task offers read when the page asks; each feed event applied as one change, the page
read again only when its brief, status or subtasks moved, those reads one after another
(`refreshAfter`), and the events heard while a read was out replayed onto its answer. Its guards
are `src/task-read.ts`. `src/task-page.tsx` draws the head (the owner's priority and cancel, the
roles with each holder's Go to session, `src/session.tsx`), the gates (`src/gates.tsx`, their
logic `src/gates-model.ts`, the links they may draw `src/links.ts`), the subtasks, the timeline's
box, the brief, the thread and its composer, and before its read the page's ghost, each part at
the height this browser last drew it. The timeline (`src/timeline.tsx`, its layout
`src/timeline-layout.ts`) is a chunk of its own, which its box (`src/timeline-slot.tsx`) imports
once it nears the screen and the page has settled (the shell's `reviewSettled`), so a first visit
carries none of it; its style comes with it. The gates' and the box's styles (`src/gates.css`,
`src/timeline-slot.css`) are the kind's own.

## Moving a page kind

1. Draw the page in a module of its own (`src/NAME.tsx`), its reads guarded and its pure parts
   exported for the unit tests, with an entry in `src/kinds/NAME.tsx` and the name in `KINDS`.
2. Take the Dioxus kind out of `apps/symphony-ui` (its sources where nothing else uses them, its
   feature, its place in `bundle.sh`'s and `package.sh`'s kinds and `tests/wasm_build.rs`), and
   move its style sheet here.
3. Run its browser tests against the built distribution, change none to pass, and take its
   pictures beside the Dioxus page's, desktop and phone, light and dark. Where a list reorders
   under the reader, Chrome's scroll anchoring may leave the two pages at different places, as
   each framework's keyed diff moves other nodes (React keeps the one it moves least, so the page
   follows it): that is the browser's, not a parity break, and `overflow-anchor: none` shows the
   two alike.
4. Hold its first visit to the cap its page has, and lower a cap the move earns, in that commit.
