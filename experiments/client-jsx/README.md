# A JSX client of three pages

An experiment, not a client: three of the Symphony's pages written in JSX, built for React 19 and,
from the same source, for Preact through its compat alias, to compare with the Dioxus client.
What it found is `docs/analysis/client-framework-comparison.md`. Nothing the Symphony builds,
tests, ships or serves reads this folder.

The pages are the board (`/tasks`), one task (`/task/SLUG`) and one document (a Markdown file
under an entrypoint), each matched with the Dioxus page's DOM so that the Dioxus client's own style
sheets, copied byte for byte from a built distribution, draw it. Each page's file says in its
first comment what it does and what it leaves out. Any other route draws a line naming the
Dioxus client.

## Build

```sh
npm ci
npm run build          # dist/react and dist/preact
```

`SYMPHONY_DIOXUS_DIST=…/public` picks the Dioxus distribution whose style sheets the build copies
(by default the committed one, `apps/symphony-server/client/public`). Each build is laid out as
the server expects a client's: `index.html` and content-hashed `assets/`.

## Serve

The Symphony server serves either build as it serves the Dioxus client, so both are measured
through the same static handler, compression and cache rules:

```sh
SYMPHONY_UI_DIST=$PWD/dist/react just symphony --port 0 --root PROJECT
```

`npm run dev` runs Vite's dev server instead, its `/api` proxied to `SYMPHONY_URL`.

## The fixture

`fixture/make.mjs` makes a scratch project with a board of its own (`SYMPHONY_MACHINE_DIR` a
temporary folder, `SYMPHONY_SURREAL` the board's database, `TASKS_BIN` and `SERVER_BIN` the
binaries):

```sh
node fixture/make.mjs bench FOLDER     # 120 tasks, a task with an 80-message thread, three documents
node fixture/make.mjs big500 FOLDER    # 500 tasks
node fixture/make.mjs big2000 FOLDER   # 2,000 tasks
```

## Measure

`bench/` is a Rust program over the workspace's headless Chrome driver
(`crates/chrome-devtools`), a Cargo workspace of its own; `--target-dir` keeps its build out of the
Symphony's `.target/`, which the repository's Cargo configuration names for every build under it:

```sh
cargo build --release --manifest-path bench/Cargo.toml --target-dir bench/target
bench/target/release/client-bench visit-board --runs 15 --client dioxus=URL --client react=URL --client preact=URL
bench/target/release/client-bench visit-board --cpu 4 --screen phone --client …
bench/target/release/client-bench feed --fixture FOLDER --handle HANDLE --client …
node bench/summarize.mjs RESULTS.jsonl… --assets dioxus=URL --assets react=URL --assets preact=URL
```

The scenarios are `visit-board`, `visit-task`, `visit-doc`, `feed`, `type`, `scroll`, `load-more`
and `memory`; the program's first comment says what each measures. Each round runs every client
in turn, a fresh Chrome each, so the clients of a round share the machine's load; the summary
gives each figure's median and range, and each client's ratio to the first, overall and by batch
of five rounds. The task page's and the document's ready conditions are passed with
`--usable-task` and `--usable-doc` (the analysis names the ones used).
