# One runtime for the client's modules

Whether the client's eight page modules can share one copy of the code they all carry, so that a
page's module holds only its own code: what each module holds, the ways the locked toolchain
offers, a prototype of the most promising one, what it would change, and the card it is asked on,
[below](#client-shared-runtime). Measured 2026-11-13 on this Mac with Rust 1.99.0, dx 0.7.10 and
wasm-bindgen 0.2.129, against the client main had committed at 310a8c3, at load averages of 30 to
136 from a release run and other agents' builds: every time below is slower than on a quiet
machine, and moves with the machine's load as much as with the files ([below](#the-prototype)).
The published client's size is main's at c76d0bc4, 20,557,418 bytes (at 310a8c3 it was
20,471,262).

The Reports page costs 557 KB on a first visit, which stays for now. The way chosen to bring it
down, investigated here as a follow-up: share the runtime between modules, one common chunk that
every page module loads, so that a page's own module is only its own code. The 300,000-byte
first-visit caps on `/` and `/tasks` are the owner's; that choice left them as they are, and only
the owner moves them.

## The choice

Sharing works with this toolchain, through the splitter dx already ships (`dx bundle
--wasm-split`), once two faults in that splitter are dealt with ([below](#the-splitters-faults)).
It shrinks the published client and makes Reports cheap. But a shared part that every page loads
holds code a given page never needed, and the board page has about 4 KB of room under its cap: on
the prototype, `/tasks`'s first visit grew from 295,837 to 360,579 bytes. So the card asked which
pages share, and A was chosen:

- **A, recommended: every page but home and the board.** `/` and `/tasks` keep a module of their
  own, as today, and their caps stand. The task pages, `/agents` and Reports share one runtime with
  the chat, the menus and the task timeline; the documents and the queue join them once the shared
  part is trimmed enough that their first visits do not grow.
- **B: all eight**, which needs the board's first-visit cap raised to about 365,000 bytes, the
  owner's alone to do. Home's first visit is estimated at about 291,000, about 9 KB under its cap.
- **C: only within the task pages' module**, so Reports loads the shared part and its own code,
  and nothing is shared between page kinds.

Bytes, gzip on the wire as the first-visit budget test counts them; "measured" is the prototype,
the rest estimated as [the last section](#verified-and-inferred) says:

| | today | A | B | C |
|---|---|---|---|---|
| `/`, first visit | 236,400 | the same | about 291,000, about 9 KB under its 300,000 cap | the same |
| `/tasks`, first visit | 295,837 | the same | 360,579 measured, over its 300,000 cap | the same |
| a task page, first visit | 559,627 | 567,087 | 568,930 measured | 567,087 measured |
| `/agents`, first visit | 559,034 | 566,494 | 568,337 measured | 566,494 measured |
| `/reports`, first visit | 557,536 | 286,100 | 287,976 measured | 286,100 measured |
| `/docs/` and `/queue`, their module and script | 447,267 and 229,641 | the same until they join | about +106 KB and +45 KB until the shared part is trimmed | the same |
| `/tasks`, then Reports | 539,160 more | 267,722 more | 55,963 more, measured | 267,722 more, measured |
| the chat and menus, warmed at idle, their modules and scripts | 742,886 | about 524 KB after a shared page, the same after `/` or `/tasks` | about 524 KB | the same |
| the published client, three builds kept | 20,557,418 at c76d0bc4, of a 20,750,000 line | about −2.4 MB, −4.7 MB once the documents and the queue join | about −7.0 MB | +504,993 (one build's measured +168,331, three kept), past the line |

A is recommended because it keeps both caps the owner set, gives Reports back 271 KB, and frees
the published client a tenth to a quarter of its size where its line has 0.9 % of room. Its costs:
a shared page waits for one request more, its chunk, whose time this machine's load hid
([below](#the-prototype)) and is measured on a quiet machine as the second step's evidence; a task
page grows by 7.5 KB; and the documents and the queue join only after the work of trimming the
shared part. B saves more and makes moves between all pages cheap, but needs the board's cap,
which the owner set two days ago, raised by about 65 KB; home's estimate stays under its cap. C
alone answers Reports but costs the size line half a megabyte it does not have, so it can only
land together with A's next step.

- [r] **client-shared-runtime** — You chose to share the runtime between the client's page modules; it works, but not for every page within your caps. Today 57 % of the client's code is a copy of code another module holds, and a first visit to Reports loads the whole task pages' module, 557,536 bytes. With one shared part, Reports drops to 286,100. But the shared part holds code a given page never uses: shared with the board, `/tasks` grew from 295,837 to 360,579 bytes on a first visit, over its 300,000 cap. Which pages should share? A: every page but home and the board, which keep their own modules and caps. Reports 286,100; a task page and `/agents` 7,460 more; the published client about 2.4 MB smaller, 4.7 MB once the documents and the queue join (it is 20.56 MB of a 20.75 MB line). B: all eight, about 7 MB smaller and cheap moves between every page, but the board's cap raised to about 365,000 and likely home's (estimated 291,000 against its 300,000). C: only inside the task pages' module: Reports the same, nothing else shared, and the published client 0.5 MB larger, past its line, so it cannot land alone. Recommended: A. Approve and say A, B or C in the note, or comment. [[analysis/client-shared-runtime#The choice]]
  [2026-11-13 17:58] agent — Posted for the board task `perf/client-shared-runtime-investigation`. The prototype is the branch `perf/client-shared-runtime-prototype`, never landed. Whichever you choose, two faults in the build tool's splitter come first: it blanks some text in the shared part (every task page showed "Page unavailable" until the prototype repaired it), and it writes its files differently on every build, which the client check refuses. The first step of the work is a step of our build that repairs both, until the tool does; the analysis says how.
  [2026-11-13 18:17] owner · approved — A
  [2026-11-13 18:20] agent — Recorded: A, every page but home and the board ("A"). Since the card was posted, the review of the analysis corrected three things it said. None changes A, and the analysis now says each:

    - **Reports' 557,536 bytes** are its whole first visit; the task pages' module and its script are 534,635 of them.
    - **Home under B** was estimated at about 291,000, about 9 KB under its 300,000 cap, not likely over it as the card said. The board's cap would still have had to rise to about 365,000.
    - **A's cost in time** is one request more on a shared page, for its part. This machine's load hid it, so it is measured on a quiet machine as the evidence of A's second step.

    The analysis lists A's five steps in order, the splitter's repair first.
  [2026-11-13 20:43] agent · resolved

## What each module holds

Each kind is built on its own (`apps/symphony-ui/bundle.sh`, one `dx bundle --features page-KIND`
each), so each carries its own copy of the framework, the frame, the wire types and the standard
library, and of every generic function compiled for its own types. Measured on named copies of
each kind (`cargo symphony-ui size`), function by function over the code section, with the crate
hashes in the names folded so a function compiled into two modules counts once:

| kind | wasm | gzip, with glue | code | in all eight | in another | its own |
|---|---|---|---|---|---|---|
| board (`/tasks`) | 605,448 | 274,083 | 505,432 | 189,691 | 182,871 | 126,166 |
| tasks (task pages, `/agents`, Reports) | 1,307,140 | 536,150 | 1,115,825 | 188,890 | 569,247 | 342,652 |
| docs | 1,134,216 | 449,743 | 989,635 | 188,769 | 508,548 | 277,865 |
| review (`/`) | 484,751 | 214,329 | 402,989 | 190,009 | 166,608 | 40,961 |
| queue | 519,557 | 230,752 | 426,947 | 189,401 | 186,887 | 45,020 |
| panel (the chat) | 1,157,033 | 468,504 | 986,849 | 190,569 | 578,632 | 201,343 |
| overlay (the menus) | 641,413 | 274,382 | 529,098 | 181,534 | 206,267 | 131,331 |
| timeline | 439,919 | 198,459 | 359,208 | 182,021 | 78,876 | 93,389 |

The eight hold 5,315,983 bytes of code between them; one copy of each function is 2,281,626. So
57 % of the client's code is a copy of code another module also holds. What all eight hold is
small, 176,650 to 195,926 bytes (the smallest and the largest copy of each function): the Dioxus
core 77 KB, the standard library 69 KB, `anyhow`'s backtrace support 16 KB, the bindings 7 KB, the
app's own 6 KB and serde 2 KB. The copies are mostly code two to seven modules share: one copy of
what two or more hold is 1,022,899 bytes, of which the Dioxus runtime's generic code is 321 KB, the
wire types and serde 266 KB, the standard library 216 KB, the app's own code 152 KB and the shared
controls 18 KB. A "runtime chunk" of what every
module needs would take only the first 196 KB; sharing has to reach what any two modules hold to
free the rest.

Every module changes in every build: the release snapshot's path follows the client's input bytes
and is written into each symbol, so the three builds the client keeps share one file of their 38
(a small script helper). A shared part shares nothing across builds either; it shares within one
build, between the page kinds a reader opens.

## The ways to share

| way | works today | a first visit loads |
|---|---|---|
| dx's splitter (`--wasm-split`) | yes, with its two faults dealt with | the shared part, then the page's chunk |
| one module for every kind, no split | yes | every kind's code, about 1 MB gzip |
| a shared module the page modules import | no | |
| dynamic linking of wasm | no | |

**dx's splitter.** `dx bundle --wasm-split` (dioxus-cli 0.7.10, whose splitter is `wasm-split-cli`
0.7.10) builds one program linked with `--emit-relocs`, follows every relocation from the
program's entry points (its exports, start function and imports) to find what the main module
needs, and cuts each split point's code into a chunk of its own, which the generated loader
fetches the first time the split point is called. A split point is an import and an export named
by convention (`__wasm_split_00___MODULE___00_import_HASH_NAME`, its `_export_` twin and the loader
`__wasm_split_load_MODULE_HASH_NAME`), which the `wasm_split` macro writes. The client guide
already records that this splitter "retained shared exports and increased the measured initial
payload", which is why each kind is built apart today (`apps/symphony-ui/AGENTS.md`, on
`bundle.sh`); the prototype measures the same on `/tasks` and finds why. The macro's expansion uses
`extern "C"` blocks and `#[no_mangle]` without `unsafe`, which this edition 2024 crate refuses,
so the prototype writes them out. Three limits, read in its source: code two chunks share that the
main module does not reach is copied into each chunk (the shared chunk it means to make,
`build_split_chunks`, collects nothing, in 0.7.10 as in 0.8.0-alpha.3); the program has one
script, with every kind's bindings; and its loader (`makeLoad`) logs a failed fetch and never
answers the page waiting on it.

**One module, no split.** One copy of every function of all eight is 2,281,626 bytes of code,
about 1 MB gzipped, and every first visit would load it. Of the board and the task pages alone it
is 1,249,053: `/tasks` would load about twice its cap.

**A shared module the page modules import.** Each module is a `cdylib` that wasm-ld links whole,
with its own allocator, statics and runtime state, and wasm-bindgen writes one glue per module that
assumes it owns the instance. Nothing in Rust, wasm-bindgen 0.2.129 or dx 0.7.10 builds a module
whose functions another Rust module calls across two instances sharing a memory. And most of what
is copied is generic code compiled for each module's own types, which a library compiled apart
could not hold.

**Dynamic linking.** wasm-ld's `-shared` and `-pie` outputs (the `dylink.0` convention) are
experimental and Emscripten's; wasm-bindgen does not process them. dx patches a running module
only for hot reloading in development, never in a release bundle.

So the splitter is the one way, and it sets the shape: one program holds every kind that shares,
and the shared part is whatever the program reaches before its split points.

## The prototype

Branch `perf/client-shared-runtime-prototype` (never landed). Its feature `split-proto` builds the
board's kind and the task pages' kind as one program cut by `dx bundle --wasm-split`, with three
split points: the board page, the task pages with `/agents`, and Reports
(`apps/symphony-ui/src/split.rs`). One wasm instance runs a Dioxus root for each kind the shell
starts (`module::install`, `KindRoot`), so a move from the board to a task page reuses the runtime
the board loaded. Its feature `split-proto-c` builds the task pages' kind alone, cut the same way:
option C. Each was packaged beside the committed client's newest build, the kinds it holds pointing
at its one script and main module, and measured by the browser harness the first-visit budget test
uses (`tests/browser/proto_split.rs`: the same stand-in server, fixtures and byte count), two
rounds each, the bytes the same in both.

The two-kind program's files, against the two modules it replaces:

| file | bytes | gzip |
|---|---|---|
| main module | 474,428 | 196,245 |
| board chunk | 285,876 | 125,323 |
| task pages chunk | 872,449 | 329,803 |
| Reports chunk | 118,995 | 51,177 |
| shared chunk, empty | 282 | 286 |
| script | 66,891 | 15,571 |
| **in all** | 1,818,921 | |
| today, the board's and the task pages' modules and scripts | 2,006,898 | |

The program is 187,977 bytes smaller (9.4 %), not half: the board's chunk and the task pages'
chunk each carry a copy of what both need and the main module does not reach. Option C's program,
the task pages' kind alone (main module 472,166, task pages chunk 871,657, Reports chunk 118,996,
script 66,541), is 1,529,642 bytes against today's 1,361,311 for that kind: 168,331 more, as its
chunks copy what they share and the main module exports what they call.

First visits, and moves made in place from `/tasks`:

| | today | two-kind program | change | C | change |
|---|---|---|---|---|---|
| `/tasks` | 295,837 | 360,579 | +64,742 | 295,839 | +2, the shell |
| `/agents` | 559,034 | 568,337 | +9,303 | 566,494 | +7,460 |
| `/reports` | 557,536 | 287,976 | −269,560 | 286,100 | −271,436 |
| a task page | 559,627 | 568,930 | +9,303 | 567,087 | +7,460 |
| `/` | 236,400 | | | 236,402 | +2, the shell |
| `/tasks`, then `/agents` | 540,658 more | 336,324 more | −204,334 | 548,116 more | +7,458 |
| `/tasks`, then `/reports` | 539,160 more | 55,963 more | −483,197 | 267,722 more | −271,438 |
| `/tasks`, then a task page | 541,251 more | 336,917 more | −204,334 | 548,709 more | +7,458 |

`/tasks` grows by what the main module holds that the board never needed, 122,814 bytes of code
reached from the shared `App` before its split points: the Dioxus runtime's generic code for the
task pages' types 41 KB, the standard library's 32 KB (float formatting 10 KB of it), the media
layer 19 KB, the keyboard's key names 10 KB, `anyhow`'s error printing 8 KB and the split's own
loaders about 14 KB; and by 4,515 bytes of script on the wire for the task pages' bindings (the
program's one script, 15,839, against the board's own, 11,324). The main module is
nearly the same with or without the board (472,166 against 474,428 bytes): it is the shared
`App`'s reach, not any one page's.

Time until the page drew on the mid-phone profile (CPU four times slower, 10 Mbit/s, 40 ms), cache
off, the median of three, in ms, two rounds each:

| | today | two-kind program | today | C |
|---|---|---|---|---|
| load average | 90 to 136 | | 30 to 45 | |
| `/tasks` | 924, 1,223 | 1,044, 1,158 | 846, 897 | 929, 945 |
| `/agents` | 1,015, 1,129 | 1,235, 1,115 | 1,038, 1,089 | 1,145, 1,123 |
| `/reports` | 994, 1,064 | 829, 922 | 991, 1,018 | 837, 825 |
| a task page | 1,131, 1,239 | 1,213, 1,200 | 1,113, 1,113 | 1,200, 1,212 |
| `/` | | | 754, 760 | 766, 732 |

These times move with the machine as much as with the files. In the same rounds `/tasks` under C
loaded today's bytes but for 2 of the shell, yet took 83 and 48 ms longer, and `/`, the same but
for those 2 bytes, moved by +12 and −28 ms. So the split pages' 34 to 107 ms more at the lighter
load cannot be told from that drift, and is not the cost of sharing. That cost is one request
more: a split page's chunk is asked for only once the main module runs and reaches the split
point. Its time is measured on a quiet machine as the second step's evidence. Reports' 154 to
193 ms less is beyond the drift: an indication, not a figure to rely on. A preload of the route's
chunk beside the main module would overlap the two requests.

Build, at the same loads: the two-kind program took 121 s and C's 61 s, and a second build of C
with nothing changed 6 s. A kind built on its own took 20 to 30 s with its dependencies built,
timed but not logged. One program replaces a build per kind.

## The splitter's faults

**It zeroes text the main module needs.** The prototype's first build drew the board, and every
task page and `/agents` as "Page unavailable": the task pages' route, `format!("/{}",
route().path)` computed in the shared `App`, came out empty. For every data symbol only a chunk
reaches, the splitter writes zeros over its bytes in the main module (`prune_main_symbols`), to be
written back when the chunk loads. The linker merges string constants that end alike, so one
symbol can lie inside another, and Rust 1.99 compiles a `format!` template to such a string (`/{}`
is `01 2F C0 00`, and it lies at the end of `@{}/{}`). When the outer one is a chunk's, the main
module's template reads zeros until that chunk loads, and its text comes out short or empty,
silently. The two-kind program had 21 such pairs (115 bytes), C 18. The prototype writes them back
into the main module after the split (a scratch step), and every page then drew. The fix belongs in
the splitter: zero only bytes no symbol the main module keeps lies over. Until dx has it, the build
has to make that repair itself and fail when a kept symbol would read a zeroed byte.

**Its chunks differ from one build to the next.** Two builds of C from the same source gave the same
main module and different chunk files: the same sizes, other bytes, so other names, and a script
that names them differently. Three runs of the splitter over one linked program gave the same main
module and empty shared chunk each time, and three different files for each page's chunk. It
writes a chunk's table entries and data segments in the order of a hash map, which changes with
every run.
The client check rebuilds the client and compares every byte with the committed distribution, so a
split client would fail it on every run. The fix is to emit them in a fixed order; until dx does, a
step after the split has to sort them.

## What changes

- **The build** (`bundle.sh`, `package.sh`): the sharing kinds become one `dx bundle --wasm-split`
  of one feature set, and the two repairs above run on its output. `package.sh` names the
  program's script for every kind in it and sets each kind's ready promise
  (`reviewModuleReady_KIND`) from its one initializer; its chunks are files of the build like any
  other.
- **The code**: about 220 `cfg(feature = "page-…")` choices pick a kind's code at compile time. In
  one program a kind is chosen at run time (`module::kind` becomes the kind each root was started
  for, as the prototype's `KindRoot` has it) and each kind's pages sit behind a split point. The
  lint's feature matrix gains the program's feature set.
- **One instance for several kinds**: a panic aborts every kind in the program, where today it
  takes down one module. The shell's per-root event scope (`scope` in `assets/shell.js`) stays, as
  each root is still a Dioxus of its own.
- **`assets/shell.js`**: `initialize` preloads a kind's script and main module; it would also
  preload the route's chunk, from a `chunks` entry in the manifest. The persistent modules (the
  chat, the menus, the timeline) become chunks the program mounts in their roots.
- **The gone-build reload**: a page open on a build two publications old finds it gone when an
  import fails (`gone`). A chunk's failed fetch fails no import: the splitter's loader logs it and
  leaves the page waiting. The loader has to reject, and the shell call `gone` on that rejection.
- **`builds.json` and `KEEP`**: unchanged in form; the chunks are listed with their build's files
  and kept with it.
- **The first-visit budgets and their tests**: the canonical-entry test
  (`entry_pages_fit_the_first_visit_budget_and_deferred_kinds_are_reported`) checks that each page
  fetched a script named after its kind; a shared page fetches the program's script and its chunk,
  and the check names those. The caps on `/` and `/tasks` stand under A.
- **The size line**: the commit of a reduction lowers the `client` line in
  `tools/size-budget.toml` to the new size plus 4 %, as its rule says. C on its own would need it
  raised, which is why it does not land alone.
- **Release snapshots and the client check**: `release-inputs` copies the client's package and the
  workspace's lock, which the splitter's runtime crate (`wasm-splitter`) joins; no new kind of
  input. The client check needs the chunks in a fixed order, above.
- **The bytes the server embeds** are the distribution's three builds: they fall with it.

## Building A, in order

1. **Make the splitter's output safe and repeatable.** A step after `dx bundle --wasm-split` that
   writes back every merged constant the main module needs, failing the build when one still reads
   zero, and puts each chunk's table entries and data segments in a fixed order; each with a test
   on a small program built to show it. Report both faults upstream, and drop the step when dx's
   splitter has the fixes.
2. **One program for the task pages, the chat, the menus and the timeline**: the task pages with
   `/agents`, and Reports, as chunks, the three persistent modules as chunks mounted in their roots,
   the shell's chunk preload, and the loader's failure path to `gone`. Reports' first visit falls
   to about 286 KB, the size line by about 2.4 MB.
3. **Trim the shared part** to what every page of the program needs: the media layer, the keyboard
   types and float formatting behind the pages that use them. Built, only the media layer could
   go: a chunk of its own, which a page loads once it shows media, takes the main module from
   216,490 bytes on the wire to 207,514 and a task page's and `/agents`' first visits down by
   7,389 bytes, of the 12,362 step 2 added (against main's build at 2eec23a). A chunk that is no
   kind has no page move or shell module to fail through, so the layer says its own failure
   through the shell: the page loads again when its build is gone, and otherwise stays under a
   notice whose retry, or the next media shown, asks for the chunk again. Against a build of this
   step without it, that costs the main module about 920 bytes of wasm (some 490 on the wire)
   and every page's shell about 60. The other side: the chunks that share code with the layer
   each hold a copy of it, 4,646 bytes on the wire across five chunks, and a task page that shows
   media on its first screen fetches the chunk too, 5,514 bytes more than before and one more
   round trip. The other two stay with dioxus-web and the splitter as they are pinned. The
   keyboard's key names (with `document::eval` and the serde_json it
   reads with, the same way) are functions of tables dioxus-web makes at launch, the event
   converter's and the document's; the splitter keeps every function of a table the main module
   reaches, and the compiler keeps these as the chat's and the menus' controls call them. Float
   formatting's precise branch is chosen inside `Display for f64` by the formatter at run time,
   and the runtime itself formats through it (an attribute's value), so no format string of the
   client's drops it: the board's module, with none, holds it too. Either needs a patched
   dioxus-web or splitter.
4. **Bring in the documents and the queue**, each once its first visit no longer grows by joining;
   the size line falls by about 2.3 MB more.
5. **Lower the size line** in the commit of each reduction.

Each step is a board task with its own measurements, the canonical-entry test and the client check
its evidence.

## Not chosen

- **A part of only the eight's common code**: 196 KB of code at most, the rest still copied.
- **Reports as a kind of its own**: a first visit near the board's, but another module of about
  600 KB a build, 1.8 MB with three kept, where the line has 0.19 MB.
- **Keeping two builds instead of three** (`KEEP`): a third of the line back, at the cost of pages
  open across two publications reloading; the orchestrator's setting, not this choice.
- **dx 0.8.0-alpha.3's splitter**: the same empty shared chunk and the same zeroing.

## Verified and inferred

Verified by measurement: the byte counts in the tables of the modules, of the prototype's files and
of its first visits and moves; the mid-phone times; the merged constants and the zeroed template in
the main module; the chunks differing between two builds and three splits; the three kept builds
sharing one file; the build times. Read in source: the splitter's graph, its zeroing, its empty
shared chunk, its hash-map order, its loader's failure path, the macro's edition, dx's flags.

The logs, file lists and scratch scripts behind them are kept with the task's evidence, in
`perf/client-shared-runtime-investigation/` under the project's artifacts folder. Measured in
scratch runs whose output was not kept: 195,926, the largest copy of each function all eight hold;
the 122,814 bytes of code the prototype's main module holds beyond the board's, with its parts; and
a kind's own build time. The documents', the queue's and the chat and menus' figures (447,267,
229,641 and 742,886) are the gzip at level 9 of their committed modules and scripts with a response
head each, which the committed client recomputes.

Inferred: in the first table, A's rows for a task page, `/agents`, Reports and `/tasks`, then
Reports, which are C's measured ones, as A's program holds the task pages' kind as C does; `/`
under B; `/docs/` and `/queue` under A and B; the chat and menus at idle; and the published client
under A and B. Each takes a kind's chunk to be its code that the prototype's main module does not
hold, at 1.29 bytes of wasm per byte of that code and 0.41 of the wasm gzipped (0.38 to 0.44
measured), beside the measured main module and script. Today's modules hold 1.17 bytes of wasm per
byte of code; the prototype's chunks came out 5 to 10 % above that, and at 1.29 the arithmetic
over its two kinds comes within 5 % of each measured chunk. Also inferred: that one program builds
faster than its kinds one by one.
