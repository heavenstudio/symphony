# apps/symphony-web/

The Symphony's web client in React and TypeScript, built by Vite: every page and everything a page
mounts. It reads the server's JSON API, whose wire types are `crates/symphony-api`, and never runs
server code or inserts server-rendered HTML: no HTML fragment crosses that boundary. Its browser
tests are a package of their own, `apps/symphony-web-tests`.

How a module works is its own header comment, and a behaviour a change makes is said there or
in the document that owns the page: this guide changes only when a job moves or a rule does,
and quotes no measured figure, which its test prints.

## Where each job lives

Paths are under `src/` unless they say otherwise. Every module opens with a comment saying what it
is and how it works: read the one you change, and the ones it names.

A page kind is an entry under `kinds/` that exports `run` and `stop`, named in `KINDS`
(`vite.config.ts`):

- `review`, `/`: `home.tsx`, `home-board.ts`
  ([REVIEW](../../docs/workflow/REVIEW.md#the-web-app)).
- `queue`, `/queue`: `queue.tsx`, `postpone.tsx`
  ([REVIEW](../../docs/workflow/REVIEW.md#queue-categories)).
- `docs`, `/docs/` and every page an entrypoint serves: `document-page.tsx`, `document-live.ts`,
  `document.ts`, `editor.tsx`, `asks.tsx` ([REVIEW](../../docs/workflow/REVIEW.md#commenting-in-place)).
- `board`, `/tasks`: `board.tsx`, `boardlive.tsx`, `boardwait.tsx`, `flow.tsx`
  ([the board's design](../../docs/product/analysis/board-design.md)).
- `task`, `/task/SLUG`: `task-page.tsx`, `task-live.ts`, `parts.ts`, `gates.tsx`, `kind.ts`,
  `timeline.tsx`, `timeline-cost.ts`, `task-diff.tsx`, `task-diff-files.tsx`, `attempts.tsx`, and
  for a custom workflow's task `nextactions.tsx`, `guards.tsx` ([TASKS](../../docs/workflow/TASKS.md#gates),
  [the timeline](../../docs/workflow/TASKS.md#the-timeline),
  [WORKFLOW](../../docs/workflow/WORKFLOW.md#the-app)).
- `agents`, `/agents`: `agents.tsx`, `agents-live.ts`, `session.tsx` (what a press does,
  `session-press.ts`)
  ([TASKS](../../docs/workflow/TASKS.md#the-agents-api)).
- `reports`, `/reports`: `reports.tsx`, its Landed tasks tab `reports-landed.tsx`, `chart.tsx`
  ([REPORTS](../../docs/product/REPORTS.md)).
- `runs`, `/runs`: `runs.tsx` ([RUNS](../../docs/workflow/RUNS.md#the-runs-page)).
- `settings`, `/settings`: `settings.tsx`
  ([PROJECT_FILE](../../docs/operations/PROJECT_FILE.md#the-settings-page)).
- `people`, `/people`: `people.tsx` ([TASKS](../../docs/workflow/TASKS.md#the-people-page)).
- `panel`, persistent (the chat, add task, activity, toasts, the pending bar, Appearance):
  `panel.tsx`, `quick.tsx`, `panel-model.ts`, `panel-focus.ts`, `appearance.tsx`
  ([REVIEW](../../docs/workflow/REVIEW.md#chat)).
- `overlay`, persistent (a select's popup and the picture viewer): `overlay.tsx`,
  `select-popup.tsx`, `picture-viewer.tsx`.

What the kinds share:

- **The shell** (`shell/shell.ts`, with the device cache's store `shell/cache.ts`): the one
  history adapter, which mounts and stops kinds, lands a navigation, reads ahead and reloads onto
  a new build. `package.sh` writes it with the kinds' manifest.
- **The frame** (`frame.tsx`): the header, the panel's `+` and bell, and the frame's reads, which
  every page kind draws. The frame, the shell and the panel speak through the window
  (`shared.ts`).
- **The feed and the board, live** (`feed.ts`, `live.tsx`, `state.ts`, `journal.ts`): a WebSocket,
  a lost stream read again from a fresh board, never resumed. Home's and the queue's page, which
  draws the whole board over it, is `board-page.tsx`. The tests fake it with
  `testing.ts`'s `feeds` and `say`.
- **Reads** (`request.ts`), **routes** (`route.ts`), **the project's base and keys**
  (`project.ts`, `storage.ts`), **async work's signal** (`mount.ts`).
- **The device cache** (`cache.ts`, `cache-document.ts`;
  [the client cache](../../docs/engineering/analysis/client-cache.md)) and a page's parts, each a read of its
  own (`parts.ts`;
  [its analysis](../../docs/design/analysis/components-draw-when-their-data-arrives.md)).
- **Loading and visibility** (`viewport.tsx`: `NearViewport`, `Settle`, the remembered heights).
- **Content** (`blocks.tsx`, `blocks-read.ts`, `media.tsx`, `media-layer.ts`): blocks and spans,
  pictures, mockups and a plugin's viewers; a board's picture and a mockup's frame, which home
  draws as the blocks do, are `media-picture.tsx`.
- **The shared controls** (`button.tsx`, `input.tsx`, `textarea.tsx`, `select.tsx`, `tabs.tsx`,
  `tooltip.tsx`), the attach control (`attach.tsx`, its sending `attach-upload.ts`), the task card
  (`taskcard.tsx`) and a task's work phases (`work-phases.tsx`).
- **Styles** (`styles/`, [their README](src/styles/README.md)): the shared sheet
  `styles/shared.css`, which states the cascade's order, the media layer's `styles/media.css`,
  and a kind's own sheet, imported by its module.
- **Timers** (`timing.ts`) and what the unit tests share (`testing.ts`, `unit-run.ts`).

## Rules a change keeps

Each is one a lint or a test does not hold by itself, or names the one that does.

- **Nothing renders or loads until it is near the viewport**: `NearViewport` for sections, native
  `loading=lazy` for media; closed panels and collapsed bodies load when opened. Once revealed,
  content stays mounted, so forms and the reading position stay.
- **A region is reserved before its read arrives** and drawn as a ghost of what it will hold,
  each box as the real one's; it never grows when its read lands. A ghost uses its own `ghost-*`
  classes, never the real ones, so no test's selector finds it. A live update that adds or removes
  a row calls `holdPlace(ROWS)` (`shared.ts`) before it sets its data.
  `layout_shift.rs` fails a load that scores 0.05 or more, and its four limits for a task's first
  visit only go down; `landmarks.rs` and `skeleton.rs` hold the ghosts' places.
- **A page at rest animates nothing** (`skeleton.rs` counts the animations), and is `aria-busy`
  while it loads and no longer once its read failed: `symphony shot` takes a page as drawn only
  once no busy region shows.
- **Async work checks its signal after each wait**, before it touches the page, the window, the
  storage or the address: an effect's own, or the kind's mount (`useMount()`, `mount.ts`) for a
  handler with no cleanup. A stopped kind does nothing more.
- **Every read is passed the signal of what asked** (`getJson(url, signal)`, `getIf`,
  `getDocument`), so a read given up is aborted on the wire. A post takes none: the server may
  have acted on it.
- **A read is guarded where it arrives**, for the fields the page draws and nothing else, so a
  field a newer server adds changes nothing; a block, span or media kind the client does not know
  is drawn as unsupported content, the rest of the page with it.
- **A failed read is said through `failed`** (`trouble.tsx`) in a read's words, never a post's
  `NO_ANSWER`, and is tried again through `tryAgain` (`state.ts`) while nothing is drawn. Such a
  part looks on this device again (`parts.ts`), so a database that opened after the store's
  500 ms deadline still gives the copy when the server does not answer.
- **A page explains nothing of itself** (RULES.md `code/pages-explain-nothing`): no lead, legend
  or note says what it shows, a heading is a short label, and a control that cannot act is not
  drawn.
- **No wait by a number** (RULES.md `code/no-timed-waits`): a wait is for its event, and a number
  paces one only through `timing.ts`'s helpers. Biome refuses the rest.
- **Every key the browser stores goes through `stored`, and every link through `href`**
  (`project.ts`): the projects of the machine's one server share its origin.
- **A page links only what `links.ts` allows**, as the addresses come from text it did not write.
- **A frame's sandbox is `allow-scripts` alone**, a mockup's and an HTML document's: never
  `allow-same-origin`, top navigation, forms or popups.
- **The client holds no viewer's code**: a plugin's viewer is drawn from its `Viewer` data, its
  script loaded once by the media layer.
- **A request for a popup or the picture viewer is data**, the shell's `reviewOverlay`, never a
  component of the page's; a page that shows media mounts the media layer.
- **This client opens no IndexedDB transaction of its own**: the store is the shell's.
- **Drafts are local to their component**, saved through `storage.ts` on every input, and never
  replaced by a snapshot.
- **New fields extend the shared controls.** Colours use the canonical shadcn token names
  (`styles/controls-theme.css` maps them); a hover that moves or raises a thing sits under
  `(hover: hover) and (pointer: fine)`.
- **A chunk imported later imports nothing of its kind's own** (the timeline, a task's attempts):
  one that did splits the kind's entry in two, a file more on every first visit. It brings its
  own sheet as `?inline` and puts it in the page, as the build links no chunk's sheet and a plain
  CSS import there builds and never loads; when its import fails it asks the shell's `gone()`
  before offering Retry (`chunk.tsx`). What runs only once the owner presses or attaches, or once
  a comparison has files to list, is such a chunk too (`session-press.ts`, `attach-upload.ts`,
  `task-diff-files.tsx`).
- **A change to `styles/board.css`'s tokens or `styles/frame.css` is copied to the server's
  copies** (`apps/symphony-server/src/`); `crates/symphony/tests/packages.rs` fails until it is.
- **A dependency added is argued on its task's thread first.** The pins are the latest stable
  releases of Node and every package, moved together, the lockfile written again, in a task of
  their own.

### Budget

`/` and `/tasks` each have a 300 KB first-visit gzip cap and a 20 KB return-visit cap, the
owner's, which no agent loosens. Under them the first visits of home, the task board and a task's
page are held to `HOME_FIRST_VISIT_BYTES`, `TASKS_FIRST_VISIT_BYTES` and `TASK_FIRST_VISIT_BYTES`
(`apps/symphony-web-tests/tests/browser/main.rs`, whose `budget` tests print each page's requests
and say what a visit counts). One is raised only by its task's orchestrator's recorded decision
once the cut came first, naming the constant (`just tasks gate SLUG
apps/symphony-web-tests/tests/browser/main.rs:NAME --raise NEW`,
[TASKS](../../docs/workflow/TASKS.md#gate-decisions)), never by an agent's own word. `report.sh OUT` says
what each kind of a build makes a browser fetch and why a kind grew; the budget test stays the
measure of a visit.

## The tools

`just client-lint` (`check.sh`) runs `typescript-only.sh`, the wire types' check, Biome
(`biome ci --error-on-warnings`), `tsc --noEmit` over both programs and the unit tests with their
coverage; `just lint-rs` runs it.

- **Node and npm** are pinned (`.node-version`, `package.json`'s `engines`): run Node through
  `tools/node.sh` and npm through `npm.sh ARGS`, which installs from the committed lockfile when
  it must. `.npmrc` runs no package's install script.
- **React 19** is the only run-time dependency; **Vite** builds one entry per kind, **Biome**
  lints and formats, **Vitest** runs the unit tests, and there is no React plugin, no charting
  library and no DOM library.
- **Hand-written code is TypeScript**, strict, in two programs: what a page runs
  (`tsconfig.json`), which knows no Node, and what Node runs (`tsconfig.node.json`).
  `typescript-only.sh` refuses a JavaScript file anywhere in the checkout outside the client's
  distribution. The shell and the Node scripts use only syntax a type erases (no `enum`,
  `namespace` or parameter property), which Node's stripper refuses otherwise (`shell/strip.ts`
  for the shell). The Mac app's `notification.ts` has no TypeScript syntax at all
  ([its guide](../symphony-desktop/AGENTS.md), `src/notification-stub.test.ts`).
- **`@types/node`** stays on the line of the Node `.node-version` names, read by
  `tsconfig.node.json` alone.

## The wire types

`src/wire.ts` and `src/wire.cases.json` are never edited by hand: `just client-wire` writes them
from `crates/symphony-api`, and `just client-lint` fails when the committed files are not what the
Rust prints. The cases hold the TypeScript that carries a Rust function (`CARRIED` in the crate's
`typescript` example) to the Rust: when one of those functions changes, port the change to the
TypeScript it names, with cases that show it, and only then run `just client-wire`; a function the
client comes to carry is added to `CARRIED` with its cases. The check cannot see a port left
undone by `just client-wire` run alone, a change in a helper `CARRIED` does not list, or an
input no case lists: a reviewer reads the file's diff for those.

## The tests

- **Unit tests** sit beside their module and run in Node with no DOM: markup is drawn with
  `react-dom/server` and compared as text, a global is stubbed with `vi.stubGlobal`, and time is
  Vitest's fake timers. A test waits for a condition with `until` and yields a turn with `turn()`
  (`testing.ts`), never `vi.waitFor` or `vi.waitUntil`, which `unit-run.test.ts` fails on.
  `npm.sh run test` prints the coverage.
- **What only a browser shows** is the headless browser target's:
  `just test-rs -p symphony-web-tests --test browser -- FILTER…`, the tests of the pages and
  controls a change touched ([by page](../../docs/workflow/CHECKS.md#the-browser-target-by-page)), never
  the whole target, which the release run runs. It builds the client once per run unless
  `SYMPHONY_UI_DIST` names a built distribution's `public/`, and needs Chrome (`SYMPHONY_CHROME`).
  A hidden control takes no focus, so a test waits for `visible(SELECTOR)` before it focuses one;
  a test of a ghost turns the device's store off (`NO_STORE`). A test of a page with its cache
  names the store's wait (`symphonyCacheOpenMs`, `shell/cache.ts`), or a loaded machine's slow
  open runs it without the cache.

## The build and the distribution

`just symphony-ui-dist [OUT]` runs `build.sh OUT`: one Vite build, then `package.sh`, which writes
the shell and its manifest and has `symphony-build publish-client` exchange `OUT/public` whole,
keeping the two builds published before it so an open page's later import still finds its file.
The scripts' comments say the rest. What a change keeps:

- **No path, clock or environment enters the bytes**, so the build is the same from any checkout
  and shell (`apps/symphony-web-tests/tests/build.rs`).
- **`tools/client-key.sh`** prints the build's key, the hash of every file git lists in this
  package but its guides: the regenerator rebuilds the distribution when it changes, so a change
  to any other file here is a release input. A branch commits no distribution
  ([the root guide](../../AGENTS.md#the-clients-distribution)).
- **A dependency's licence other than MIT refuses packaging** (`notices.sh`), until this step
  selects its licence explicitly.

## The dev server

`just symphony-ui-dev SERVER [PORT]` shows this client's kinds as their sources stand, the page
loaded again on each save: Vite's dev server on loopback at PORT (0 for any), over a Symphony
server of your own on port SERVER, which `just symphony --port 0` starts and prints. It refuses
8765, the shared server's, as either port. The shared stylesheet it shows is the server's build's,
so an edit under `src/styles/` shows once `just symphony-ui-dist` has built it.
`src/dev-server.ts` says the rest.

## Adding a page kind

1. Draw the page in a module of its own (`src/NAME.tsx`), its reads guarded and its pure parts
   exported for the unit tests, with an entry in `src/kinds/NAME.tsx` and the name in `KINDS`
   (`vite.config.ts`); its route is the shell's (`shell/shell.ts`) and, for a route the Symphony
   answers itself, `symphony_api::SYMPHONY_ROUTES`.
2. Give it browser tests in `apps/symphony-web-tests/tests/browser`, and name it in the build
   test's kinds (`tests/build.rs`) with the sheets it loads.
3. Report its first visit beside the other kinds' (the budget test prints each page's requests),
   and hold it to a cap where the owner sets one.
