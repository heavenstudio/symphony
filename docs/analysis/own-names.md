# Symphony's own names

**Decided: B, at once** (the owner, 2026-11-11: "just remove the bandaid quickly. Break things
then fix them ASAP"). Every old name goes in one pass, with no bridge: no build reads an old name.
The old-name reads, the fences, the refusal of an unmoved folder and the bridge slices this page
plans are void. An existing checkout's state moves with
`tools/move-state.sh [--dry-run] [CHECKOUT]`, which renames its folders into the `symphony/`
folder of its git common dir and refuses while that repository's board database, a board process
or the machine's shared server runs; the owner runs it once per repository, with the shared server
stopped. The board database's namespace stays `sf`. The rest of this page is the inventory and
plan as they stood before the verdict.

Symphony grew inside Star Foundry and still carries its names: variables, state folders, the
names of Star Foundry's own tools, and documents that name it. This is the inventory behind the
names card in [DECISIONS](../DECISIONS.md), for the board task `tools/no-star-foundry-names`, and
the plan for removing them. Nothing here renames anything.

Read 2026-11-11 on main `aaa4aed`. Each fact about the code names the file it was read in. Star
Foundry was read, never changed, at its main `f309cad`, which pins Symphony at `c9a0af8`. Live
state was only listed (`ls`, `du`, `ps`), never opened or changed.

## The choices

A second project adopting Symphony should meet no other project's name. Three things are the
owner's: the new names, how long the old ones are still read, and how the state on disk moves.

**The new names (recommended).**

- **Variables:** `SYMPHONY_` in place of `SF_`, with a doubled prefix dropped
  (`SF_SYMPHONY_PORT` becomes `SYMPHONY_PORT`, `SF_SURREAL` becomes `SYMPHONY_SURREAL`).
  The board's existing `TASKS_` variables stay as they are.
- **State:** one `symphony/` folder in each repository's git common dir. It holds
  `tasks/` (the board, its keys and its records), `surreal/` (the pinned SurrealDB),
  `server/` (the shared server's build, client, logs and verdict journals), `models/` (the
  board's embedding model) and `tools/` (the client's build tools).
- **Other names:** `symphony-` for the files in a worktree's git folder,
  `symphony.codexSteer` for the git setting, and `symphony-component` for the HTML element.
- **Star Foundry's tools:** Symphony stops naming them. Each one becomes a `symphony.toml`
  setting or a check that names no tool ([below](#star-foundrys-names-in-symphonys-code)).

**How the old names go:**

- **A, a bridge (recommended).** One release reads both names, new first, and says once per run
  when it used an old one. It also ships the command that moves a project's state. Each project
  moves on a day the owner picks. The first release after both registered projects have moved
  stops reading the old names. Cost: a small helper for variables and folders, and one or two
  releases that carry it. Star Foundry keeps working until its pin and wrappers move.
- **B, at once.** One release renames everything with no bridge, and both projects move the
  same day. Cost: on that day, Star Foundry's pin, its wrappers (`tools/tasks.sh`,
  `tools/surreal.sh`, the `symphony` recipe), its tests and both moves all land together. Every
  older build on the machine fails until it is rebuilt, and Star Foundry keeps 46 `tasks` builds
  in `sf-bin/`.
- **C, new projects only.** New boards and installs get the new names. The two existing projects
  keep their `sf-` folders, read as legacy for good. Cost: both schemes stay in the code, and the
  grep for the old names never comes clean. Nothing moves and nothing stops.

**Why A.** Only A lets each project move on its own day with nothing broken in between. It ends
with no old name in the code except the fences and the refusal ([Moving live state](#moving-live-state)).
The brief's period of "one tagged release" is counted from the second move, not the first
release: releases are cut nightly ([symphony-update](symphony-update.md#releases)), so one
release could otherwise last a day.

### Smaller choices

These are defaults that a note on the card can change.

1. **The board database's namespace.** It is `sf`: `NAMESPACE` in `crates/tasks/src/lib.rs`, 1
   constant, plus 94 literals in tests. Renaming it means exporting and importing the whole
   store: 6.3 GB of data on Star Foundry's board, with the time not measured. Default: the two
   existing boards keep `sf`, recorded in their folder by the move. New boards get `symphony`.
   An existing board takes the new namespace at its next `tasks-reclaim`, which exports and
   imports every database anyway (`crates/tasks/src/server/reclaim.rs`).
2. **Star Foundry's tool names** leave the code, as the table below says for each.
3. **Prose.** Guides, AGENTS files and skills say "a consuming project". Analysis documents
   keep Star Foundry's name where they record a measurement of its board, since they are dated
   records. The dated history in [DECISIONS](../DECISIONS.md) stays as written.
4. **Board variables** take `SYMPHONY_` rather than the board's own `TASKS_` (`TASKS_SURREAL`,
   `TASKS_SWEEP_RECENT_HOURS`, `TASKS_WT_DIR`). This gives one prefix to look for, at the cost
   of two prefixes in the board's own guide.
5. **`tasks-cutover` is retired, not renamed.** It moved a SQLite board to SurrealDB. Star
   Foundry's board moved 2026-10-21 (its `star-foundry-tasks.db.moved`). This repository never
   had a SQLite board. Its only old name is the `sf_moved_` prefix of the freeze triggers, which
   exist only in the frozen SQLite backup.
6. **The standalone guard keeps Star Foundry's tool names.**
   `apps/symphony-server/tests/standalone/main.rs` puts fake `sf-slots`, `sf-rust-tests`,
   `sf-land`, `sf-verify`, `sf-nightly`, `sf-hooks` and every `sf-*` on `PATH` to prove the
   standalone workspace runs none of them. It is the one allowed exception to the grep.

## The count

```
git grep -nIiE 'sf[-_]|star.?foundry' -- . ':(exclude)apps/symphony-server/client/public'
```

That finds **825 lines in 213 files**. They break down as:

- 760 lines in 208 files match `sf[-_]`. On one of them the match is not a name: a mangled Rust
  symbol in `apps/symphony-ui/examples/wasm_size.rs`.
- 67 lines in 6 files name Star Foundry.
- 2 lines do both.

The generated client has no match. The lines hold **857 occurrences**, each sorted by the name it
carries:

| Category | What it is | Occurrences | Outside tests | Tests | Docs | Shell, justfile |
|---|---|---:|---:|---:|---:|---:|
| a1 | Variables Symphony reads outside tests (19) | 182 | 54 | 100 | 14 | 14 |
| a2 | Variables only Symphony's tests set and read (88, plus 3 `SF_` prefix filters) | 185 | 0 | 184 | 1 | 0 |
| a3 | Folders and files Symphony writes | 193 | 43 | 100 | 33 | 17 |
| a4 | Names inside formats Symphony reads and writes | 19 | 13 | 6 | 0 | 0 |
| a5 | Tests' temp folders, sockets, fixtures and shell variables | 96 | 0 | 96 | 0 | 0 |
| b | Star Foundry's own names that Symphony mentions or reads | 109 | 17 | 82 | 7 | 3 |
| c | "Star Foundry" by name | 72 | 0 | 0 | 72 | 0 |
| | Not a name (the mangled symbol) | 1 | 0 | 1 | 0 | 0 |

"Outside tests" means source that is not a test file and not inside an inline `#[cfg(test)]`
module, comments included.

By package, as lines and files from `git grep -c`, and as occurrences by category:

| Package | Lines | Files | a1 | a2 | a3 | a4 | a5 | b | c |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `crates/tasks` | 248 | 85 | 51 | 44 | 75 | 4 | 38 | 38 | 0 |
| `apps/symphony-server` | 176 | 46 | 45 | 68 | 28 | 0 | 12 | 29 | 0 |
| `crates/symphony` | 175 | 40 | 40 | 53 | 41 | 14 | 5 | 27 | 0 |
| `docs/` | 88 | 8 | 2 | 0 | 17 | 0 | 0 | 6 | 72 |
| `apps/symphony-ui` | 64 | 17 | 26 | 1 | 25 | 0 | 5 | 6 | 0 |
| `tools/` | 32 | 7 | 8 | 0 | 7 | 0 | 24 | 3 | 0 |
| `crates/process-cwd` | 21 | 3 | 0 | 9 | 0 | 0 | 12 | 0 | 0 |
| `crates/chrome-devtools` | 14 | 3 | 4 | 10 | 0 | 0 | 0 | 0 | 0 |
| `crates/repository-read` | 3 | 1 | 2 | 0 | 0 | 1 | 0 | 0 | 0 |
| `AGENTS.md`, `README.md`, `justfile` | 4 | 3 | 4 | 0 | 0 | 0 | 0 | 0 | 0 |
| **Total** | **825** | **213** | **182** | **185** | **193** | **19** | **96** | **109** | **72** |

The `apps/symphony-ui` row also holds the 1 occurrence that is not a name. `RULES.md`, the
skills, `symphony.just`, `.claude/settings.json`, `codex.rules` and `symphony.toml` have none.

**Outside the grep.** Three names the brief's pattern does not match:

- The namespace `sf` ([Smaller choices](#smaller-choices) 1).
- The git setting `sf.codexSteer`: 7 lines. It is read in `crates/tasks/src/store/codex_delivery.rs`
  and named in the `tasks` skill that `symphony init` installs (`crates/symphony/shipped/skills/tasks/SKILL.md`).
- Paths from when Symphony lived in Star Foundry's tree: `symphony/crates/…` in 64 lines and 13
  files, and Star Foundry's launchers `tools/tasks.sh` and `tools/tasks.py` in 22 lines and 14
  files. These are prose and belong with the docs slice.

## Variables

**The 19 that Symphony reads outside tests.** Category a1. A bridge release reads the new name,
then the old one.

| Variable | Read by | Set by | New name |
|---|---|---|---|
| `SF_SURREAL` | the board's `Home::in_common` callers: `tasks` (`cli/mod.rs`), `tasks-reclaim` (`lib.rs`), `agent-send` passes it on, `symphony init`'s next steps (`init.rs`) | a person or a project's wrapper naming another SurrealDB; Star Foundry's gate passes it through; tests | `SYMPHONY_SURREAL` |
| `SF_GIT_COMMON_DIR` | `tools/surreal.sh`; `apps/symphony-ui/{bundle,dev,tools}.sh`, `probes/size.sh` (`bundle.sh` exports it); named in a `server.rs` message | Star Foundry's own tools, 157 lines of its own; tests | `SYMPHONY_GIT_COMMON_DIR` |
| `SF_CHROME` | `crates/chrome-devtools/src/lib.rs` (the shot, the browser tests) | a person; tests | `SYMPHONY_CHROME` |
| `SF_SYMPHONY_UI_DIST` | `apps/symphony-server/src/client.rs` | the `justfile`'s `_verify`; tests, Star Foundry's too | `SYMPHONY_UI_DIST` |
| `SF_SYMPHONY_PORT` | `apps/symphony-server/src/cli.rs`; `crates/tasks/src/cli/field.rs` | tests, Star Foundry's too | `SYMPHONY_PORT` |
| `SF_HOOKS_ROOT` | `crates/symphony/src/hook/tasks_beat.rs`, after `SYMPHONY_HOOK_ROOT` | Star Foundry's `.claude/hooks/sf-hook`, on a path its settings do not use | dropped: `SYMPHONY_HOOK_ROOT` exists |
| `SF_TASKS_CLI` | `crates/symphony/src/taskcli.rs` | tests; Star Foundry's gate reads its own copy | `SYMPHONY_TASKS_CLI` |
| `SF_SYMPHONY_PROFILE` | `apps/symphony-server/src/live.rs` | a person profiling; tests, Star Foundry's too | `SYMPHONY_PROFILE` |
| `SF_SYMPHONY_STARTUP_TRACE` | `apps/symphony-server/src/cli.rs` | tests, Star Foundry's too | `SYMPHONY_STARTUP_TRACE` |
| `SF_SYMPHONY_PUBLICATION_TRACE` | `crates/symphony/src/commit.rs` | a person; a test fixture | `SYMPHONY_PUBLICATION_TRACE` |
| `SF_SWEEP_RECENT_HOURS` | `crates/tasks/src/sweep.rs`, and `tasks` usage | a person; tests | `SYMPHONY_SWEEP_RECENT_HOURS` |
| `SF_WT_DIR` | `crates/tasks/src/launch/mod.rs` | a person; a test | `SYMPHONY_WT_DIR` |
| `SF_MACHINE_WORK_READER` | `apps/symphony-server/src/live/machine_work.rs` | a project's host process | `SYMPHONY_MACHINE_WORK_READER` |
| `SF_READ_AUDIT_ROOT`, `SF_READ_AUDIT_REPORT` | `crates/repository-read/src/lib.rs`, a library crate; today only `symphony-api`'s wire tests use it | a read-audit verifier; Star Foundry's gate has one | `SYMPHONY_READ_AUDIT_ROOT`, `…_REPORT` |
| `SF_SYMPHONY_HANDOVER` | `apps/symphony-server/src/reload.rs` | the serving server, for its successor | `SYMPHONY_HANDOVER` |
| `SF_SYMPHONY_CHROME_LEASH` | `crates/chrome-devtools/src/lib.rs` | the shot, for itself run again | `SYMPHONY_CHROME_LEASH` |
| `SF_SYMPHONY_TEST_PID` | `apps/symphony-server/src/front.rs`, kept in the server's environment (`home.rs`, `KEPT`) | tests, Star Foundry's too | `SYMPHONY_TEST_PID` |
| `SF_HOOKS_BEAT_CLOCK` | `crates/symphony/src/hook/tasks_beat.rs` | tests, Star Foundry's too | `SYMPHONY_HOOK_BEAT_CLOCK` |

`SF_SYMPHONY_HANDOVER` passes from one server generation to the next, so a bridge release both
sets and reads both names. Otherwise a reload from an older generation would lose its listener
and restart instead of handing over.

`SF_HOOKS_ROOT` already has its replacement. It is the existing form of the bridge: a new name
read first and an old one after it.

**Star Foundry's 6.** Category b. Symphony reads or mentions these variables, but they belong to
Star Foundry:

- `SF_CONTENT_DIR`: removed from the environment of the server's own build
  (`crates/symphony/src/reload.rs`).
- `SF_REVIEW`: a comment in `taskcli.rs`.
- `SF_TEST_SLOT_HELD`: unset by `tools/fixture_process.sh`.
- `SF_TEST_TASKS_BINARY`, `SF_TEST_SYMPHONY_PREBUILT` and `SF_TOOL_TEST_WAIT_HELPER`: Star
  Foundry's test runner hands these to tests, which Symphony's tests and `tools/check_lock.sh`
  still read.

None of them is set when Symphony runs alone.

**The 88 that only Symphony's tests set and read.** Category a2. These are renamed outright, with
no bridge: nothing outside the tests sets them. Grouped by family:

- **`SF_AFTER_*`:**
  - `ACCESS_{CASE,ROOT}`
  - `COMMIT_{FILING,GATE,JUST,NOTICE,READY,ROOT,SWAP_BRANCH,SWAP_ROOT,TASKS}`
  - `HOST_ORIGIN_{CHILD,FINISH,LOOKUP,NOTICE}`
  - `HTTP_CHILD`
  - `RELOAD_{CENSUS,COMMANDS,GATE,PROCESSES,ROOT,SERVER}`
  - `WORKTREES_CHILD`
- **Children and cases:**
  - `SF_ASK_FLUSH_STOP_CHILD`, `SF_BLOCKED_PANES_CHILD`, `SF_BOARD_CHILDREN_CASE`,
    `SF_BOARD_KEPT`, `SF_BROKEN_STDERR_STOP_CHILD`, `SF_BROWSER_BINARIES_CHILD`
  - `SF_CENSUS_INHERITED_FD`, `SF_CHAT_JOURNAL_TEST`, `SF_CHROME_CLEANUP_SPINNER`,
    `SF_CHROME_QUIET_LEASH`, `SF_CHROME_SUITE_CHILD`, `SF_CHROME_SUITE_SIGNAL_MODE`
  - `SF_CLEAN_PROOF_CHILD`, `SF_CODEX_QUEUE_CASE`, `SF_CODEX_QUEUE_KEPT`
  - `SF_CUTOVER_FIXTURE_{EVENTS,ROUTE,SLOT}`, `SF_CWD_GIVEN`
  - `SF_DEV_FAKE_LISTENER`, `SF_DEV_SESSION_DIR`, `SF_EMBEDDING_RESOURCE_PROBE`
  - `SF_EVENT_OF_EVENTS`, `SF_EVENT_OF_ROUND`, `SF_FLUSH_CONTROL`, `SF_GALLERY_REFRESH_CHILD`
  - `SF_HOME_FIXTURE_{EVENTS,OWNER}`, `SF_HUB_BACKSTOP_CHILD`, `SF_IGNORE_HOME_CASE`
- **`SF_LIFECYCLE_*` and later:**
  - `SF_LIFECYCLE_{BINARY,LOG,PREPARED_APP,SCRATCH_ROLE}`, `SF_LOG_TEST_HOME`,
    `SF_NO_DOCUMENTS_CHILD`, `SF_OWNER_SAVE_{PHASE,PROBE}`
  - `SF_PERF_CLEANUP_RECORD`, `SF_PERF_TASKS` (the board guide names it), `SF_REAL_GIT`,
    `SF_REAL_SQLITE`
  - `SF_SCAN_ROOT`, `SF_SCRATCH_RUN_SERVER`, `SF_SESSIONLESS_CHILD`, `SF_SHOT_MISSING_CHROME`
  - `SF_SPAWN_{EARLIER,NEVER,SET}`, `SF_STALE_CHILD_FIXTURE`
- **`SF_SYMPHONY_*` and `SF_TEST*`:**
  - `SF_SYMPHONY_{AGENTS_PROBE_ROOT,BUILD_CASE,CHILDREN_CASE,FAKE_BROWSER,FIXTURE_BASH_ENV_CHILD,KEPT,METADATA_MODE,METADATA_ROOT,PROFILES_ROLE,SIPS_ARGS,SIPS_CASE}`
  - `SF_TASKS_CLI_SCRATCH_ROLE`
  - `SF_TEST_SERVER_DIR`, `SF_TEST_SESSION_DIR`, `SF_TEST_SWEEP_SESSION_DIR`

Three tests also strip every `SF_` variable from a child's environment by prefix:

- `apps/symphony-server/tests/after_reload.rs`
- `crates/symphony/tests/after_commit.rs`
- `crates/symphony/tests/common/commit_fixture.rs`

During the bridge they strip both prefixes.

**Landed after this count.** `tools/one-build-folder-for-worktrees` landed three more before it
took the new names: `SF_LOW_DISK_GB` (the board's and the pages' low-disk threshold) and
`SF_SEED_TARGET` and `SF_SEED_WAIT` (`tools/seed-target.sh`). The variables slice reads them as
`SYMPHONY_LOW_DISK_GB`, `SYMPHONY_SEED_TARGET` and `SYMPHONY_SEED_WAIT` first, with the same
bridge.

## Folders and files

Category a3, by where it lives. Everything in the git common dir is shared by every worktree of
its repository. Each repository has its own copy: this one at `symphony/.git/`, and Star
Foundry's at `star-foundry/.git/`.

**`sf-tasks/`, the board. Becomes `symphony/tasks/`.**

- **Named in:** `crates/tasks/src/server.rs` (`FOLDER`, `Home::in_common`). Also joined by hand
  in:
  - `launch/staff.rs` (`staff.lock`)
  - `launch/reap/state.rs` and `cutover.rs` (`reap.json`)
  - `cli/beat.rs` and the hook's `tasks_beat.rs` (`keys/`)
  - `cli/machine_work/recorded.rs` (`work/`)
  - `board_health.rs` (`health.json`)
  - the sweep (`sweep.*`)
  - `on_review.rs` (`on-review.log`)
  - `tools/rs-bin.sh` (`server.json`)
- **Holds:**
  - `data/`, the store
  - `server.json`, `lock` and `server.log`
  - `keys/`
  - `health.json`
  - `dispatch.log`, `on-review.log`
  - `reap.json` and its lock, `staff.lock`
  - `sweep.{lock,log,requests,served,stamp}`
  - `work/` and `reclaim/`
- **Writers:**
  - the board's SurrealDB (`data/`, `server.log`)
  - every build of the board library: `tasks`, `agent-send`, `symphony` and the heartbeat
    hook's `tasks beat`
  - the shared server (health, the five-minute sweep, callback dispatch, review callbacks)
- **Readers:** the same, plus `tools/rs-bin.sh`, and Star Foundry's `tools/tasks.sh`, which reads
  `server.json` with `jq`.

**`sf-surreal/VERSION/surreal`, the pinned SurrealDB. Becomes `symphony/surreal/`.**

- **Named in:** `server.rs` (`PINNED`) and `tools/surreal.sh` (`FOLDER`).
- **Writers:** `tools/surreal.sh`. In Star Foundry, its own copy of that script writes it.
- **Readers:** the board library, and `symphony init`, whose next step tells a new project to
  put the binary at this old path.

**`sf-symphony/`, the shared server's install and verdict journals. Becomes `symphony/server/`.**

- **Named in:**
  - `apps/symphony-server/src/home.rs` (`FOLDER`)
  - `crates/symphony/src/machine/command.rs` (`INSTALL`)
  - `crates/tasks/src/cli/field.rs` (`FOLDER`)
  - `crates/symphony/src/commit.rs` (the verdict home)
- **Holds:**
  - `bin/symphony-server` and `client/`
  - `installed.json` and Cargo's install records
  - `lock`, `server.lock`, `reload-build.lock` and `server.log`
  - the record of a server started before the machine folder, which is read once
  - `verdict-backlog/` and `verdict-publications/`
  - `verdict-{commits,sequence,writes}.lock`
- **Writers:**
  - the server's own start and reload (`--install` names the folder)
  - Star Foundry's `symphony` recipe, which runs `cargo install --root …/sf-symphony` and
    starts `sf-symphony/bin/symphony-server`
  - each owner verdict, through `symphony::commit`
- **Readers:** the starter. Also `tasks field`, which still looks there for the server's port
  although the record now lives in the machine folder. The rename slice reads the machine folder
  instead. Star Foundry's gate reads `verdict-publications` too (`tools/gate/src/verify/retention.rs`).

**`sf-docs/`, the verdict journals of a repository that had them before `sf-symphony/`. Moves into `symphony/server/`.**

- **Named in:** `commit.rs`. The old folder is used wherever it exists, so Star Foundry's
  verdicts still go there.
- **Holds:** in Star Foundry, the same verdict files plus an older install.
- **Background:** `tools/studio/state-migration` planned to move it to `sf-studio`. That move
  folds into this one.

**`sf-task-models/`, the embedding model and tokenizer per revision, with maintenance locks. Becomes `symphony/models/`.**

- **Named in:** `crates/tasks/src/embedding.rs` and `cli/embedding.rs`.
- **Fetched:** on first use, 128 MB.

**`sf-cargo/dx-0.7.10/`, `sf-cargo/dx-home/`, the client's build tools. Becomes `symphony/tools/`.**

- **Named in:** `apps/symphony-ui/{tools,bundle,dev}.sh` and `probes/size.sh`.
- **Shared:** in Star Foundry, `sf-cargo/` is Star Foundry's own folder, and these two entries
  sit inside it.

**A worktree's git folder** (`.git/worktrees/NAME/`):

- `sf-sweep-remove.lock` (`sweep/coalesce.rs`, `sweep/guards.rs`)
- `sf-sweep-verdict.json`, a cache (`sweep/verdicts.rs`)
- `.sf-sweep-lock-*`, saved locks (`sweep/git/locks.rs`)

These become `symphony-sweep-…`.

**Elsewhere:**

- `$TMPDIR/sf-tasks-reap/`, test boards' sweep state, in non-test code
  (`launch/reap/state.rs`).
- `$TMPDIR/sf-command-*`, unlinked as soon as it is made (`apps/symphony-server/src/sys.rs`).
- `.target/sf-tree-wasm32-unknown-unknown`, a freshness record (`apps/symphony-ui/wasm-fresh.sh`).

All three are renamed outright. Losing one costs at most a rebuild.

**`sf-runs/`.** No code on main writes it. On main, the recorder writes `sf-tasks/work/`. A
branch's recorder left 22 `.run` files there in this repository's common dir on 2026-11-11, and
they were gone by 19:20. [reports-and-monitoring](reports-and-monitoring.md) plans `sf-runs/` for
run records. Under A, `tools/run-record` should name it `symphony/runs/` from the start.

**What is on disk** (read 2026-11-11):

| | This repository | Star Foundry |
|---|---|---|
| `sf-tasks/` | 80 MB, nearly all of it the store; 2 keys | 7.2 GB, store 6.3 GB; 1,440 keys |
| `sf-surreal/` | 280 MB | 280 MB |
| `sf-symphony/` | 4 KB, verdict files only | 165 MB: the running server's build, client, record and log |
| `sf-docs/` | none | 103 MB: verdict files and an older install |
| `sf-task-models/` | none | 128 MB |
| `sf-cargo/` | 103 MB, dx only | 569 GB, Star Foundry's own |

**Running** (read 2026-11-11):

- one SurrealDB per board, each started from its own `sf-surreal/3.3.0/surreal` on
  `surrealkv://…/sf-tasks/data`
- the shared server on port 8765, started from
  `star-foundry/.git/sf-symphony/bin/symphony-server` with `--install …/star-foundry/.git/sf-symphony`

Its record and the registry are already in the machine folder
(`~/Library/Application Support/Symphony`), which carries no old name.

**Star Foundry's own folders** are not Symphony's and stay as they are: `sf-land/`, `sf-bin/`,
`sf-cargo/` (but for the two dx entries), `sf-test-slots/`, `sf-verify-summaries/`,
`sf-nightly/`, `sf-browser/` and the rest.

## Formats and other names

Category a4, plus the two names outside the grep:

- **`sf-component`.** The HTML element that carries a plugin component's blocks through the
  document pipeline: written in `crates/symphony/src/blocks/write.rs`, read back in
  `blocks.rs`, named in `render.rs`. It is internal; no stored document uses it, and Star
  Foundry has 0. Becomes `symphony-component`, with no bridge.
- **`sf-read-audit-v1`.** The header of a read-audit report (`crates/repository-read`), shared
  with whatever verifier reads it. Becomes `symphony-read-audit-v1`. During the bridge, a
  verifier's report under the old header is still accepted.
- **`sf_moved_`.** The freeze triggers' prefix in `crates/tasks/src/cutover.rs`. Retired with
  `tasks-cutover` ([Smaller choices](#smaller-choices) 5).
- **The namespace `sf`.** See [Smaller choices](#smaller-choices) 1.
- **`sf.codexSteer`.** Becomes `symphony.codexSteer`, read first, with the old key read during
  the bridge. The shipped `tasks` skill names the new key, so `symphony init` installs no old
  name.

**Binaries and scripts.** Symphony ships none with an old name:

- **Binaries:** `symphony`, `symphony-server`, `symphony-shelf`, `symphony-ui-dev`, `tasks`,
  `agent-send`, `tasks-copy`, `tasks-reclaim`, `tasks-cutover` and `fake-hc`.
- **Scripts:** `tools/*.sh`, `apps/symphony-ui/*.sh` and the test fixtures' scripts.
- **Recipes:** none in `symphony.just` or the `justfile`.

The only old names in scripts are shell variables in `tools/test-support/script*.sh`
(`sf_fixture_source`, `sf_fixture_link`; category a5) and the paths and variables above.

## Star Foundry's names in Symphony's code

Category b. The names that matter are those Symphony's own code uses outside tests. Star Foundry
builds its `symphony`, `symphony-server`, `tasks` and `agent-send` from these libraries, so each
of them runs inside Star Foundry's binaries too.

| Name | Where | What Symphony does with it | Recommendation |
|---|---|---|---|
| `sf-slots cargo` | `crates/symphony/src/reload.rs` | outside its own workspace, the server rebuilds itself through `tools/rs-bin.sh sf-slots cargo install …` | a `symphony.toml` setting for the command that runs Cargo (Star Foundry writes its wrapper there); plain `cargo` when unset |
| `.target/bin/sf-slots` | `apps/symphony-server/src/live/machine_work.rs` | the project's own machine-work reader, at its prepared path | a `symphony.toml` setting for the reader's path; none when unset |
| `sf-test-slots/` | `crates/tasks/src/cli/probes.rs` (`SLOTS_DIR`) | the board's machine probes read the project's slot locks in the common dir | a `symphony.toml` setting for the folder; no probe when unset |
| `sf-land/assessments`, `sf-land-submission/1`, `sf-land-assessment/1`, `sf-land-binding/1` | `crates/tasks/src/cli/api/assessment/publication{,/read}.rs` | the board's API reads Star Foundry's landing submissions from that folder, by those protocol names | a `symphony.toml` setting for the folder; protocol names of Symphony's own, the old ones accepted during the bridge |
| `sf-cargo/tasks-main` | `crates/symphony/src/hook/tasks_beat.rs` (`LIVE`) | the heartbeat hook falls back to the build Star Foundry's `tools/tasks.sh` records | dropped: the hook's own links (`.target/bin/tasks`, the main checkout's) cover it once Star Foundry links its build there |
| `sf-cargo/bin/tasks-<hash>`, `sf-bin/tasks-<hash>` | `crates/tasks/src/launch/procs.rs` (`board`) | a process running one of Star Foundry's `tasks` builds counts as a board command | any `…/tasks-<16 hex>` executable, by name alone |
| `SF_CONTENT_DIR` | `crates/symphony/src/reload.rs` | removed from the build's environment | dropped. The server keeps only `HOME`, `PATH`, `USER`, `LANG`, the test pid and the machine folder's variable (`home.rs`, `KEPT`), so it is likely already absent. The slice confirms this. |
| `SF_TEST_SLOT_HELD`, `SF_TEST_TASKS_BINARY`, `SF_TEST_SYMPHONY_PREBUILT`, `SF_TOOL_TEST_WAIT_HELPER` | `tools/fixture_process.sh`, `tools/check_lock.sh`, three test helpers | hooks for Star Foundry's test runner, which no longer runs Symphony's tests | dropped, or renamed `SYMPHONY_TEST_…` if Symphony's own runner (`tools/nextest`) wants them |

Mentions only, which are reworded with the docs:

- `sf-land/verdicts` and `on_review = [… "sf-land" …]` in tests and the server README. These are
  examples of `[verdicts] record` and `[board] on_review`, which already take Star Foundry's
  paths from its `symphony.toml`.
- `sf-hook`, a project's own hook in `crates/symphony/src/init_tests.rs`.
- `sf-tool-tests`, `sf-rust-tests` and `sf-hooks` in test comments.
- `sf-verify-summaries/` in an analysis document.
- `SF_REVIEW`.

## Prose that names Star Foundry

Category c: 67 lines, 72 occurrences, all in `docs/`.

- **[DECISIONS](../DECISIONS.md): 14 lines.** The cards' dated history. It stays as written.
- **[reports-and-monitoring](reports-and-monitoring.md): 33 lines.** Measurements of Star
  Foundry's board and verify, and the gates adapter task. It keeps the name, as a dated record.
- **[client-cache](client-cache.md): 10 lines.** Its register measured as "the large register".
  It keeps the name.
- **[symphony-update](symphony-update.md): 8 lines.** Star Foundry's pin and its `verify`, as
  the consuming project of the day. It keeps the name.
- **[desktop-client](desktop-client.md): 1 line.** The registry's two projects. It keeps the
  name.
- **`reports-gates-b-wide.svg`: 1 line.** A made-up task in Star Foundry's shape. It keeps the
  name.

The guides (AGENTS, the package guides, READMEs, `docs/TASKS.md`, `docs/MACHINE_WORK.md`) never
say "Star Foundry". They do carry its tool names and the old variables and folders, so they are
reworded with the names they describe.

## How Star Foundry meets these names

Read at Star Foundry `f309cad`.

**How it pins Symphony.** Its `Cargo.toml` names ten Symphony crates on `branch = "main"`, and
its `Cargo.lock` binds `c9a0af8`. Its `symphony.toml` takes these from its own settings:

- `[verdicts] record = ".git/sf-land/verdicts"`
- `[board] on_review = ["tools/rs-bin.sh","sf-land","review"]`

**What depends on Symphony's names.** A rename on Symphony's side alone breaks:

- **`tools/tasks.sh`** reads `sf-tasks/server.json`.
- **`tools/surreal.sh`** writes `sf-surreal/`.
- **The `symphony` recipe** installs into and starts from `sf-symphony/`.
- **The gate** reads `sf-symphony/` and `sf-docs/` verdict publications.
- **Its tests** set `SF_SYMPHONY_PORT`, `SF_SYMPHONY_PROFILE`, `SF_SYMPHONY_STARTUP_TRACE`,
  `SF_SYMPHONY_TEST_PID`, `SF_SYMPHONY_UI_DIST`, `SF_HOOKS_BEAT_CLOCK` and the namespace `sf`.
- **Its guides:** `AGENTS.md`, `docs/workflow/TASKS.md` and `tools/AGENTS.md`.
- **The `.claude/hooks/sf-hook` launcher** exports `SF_HOOKS_ROOT`, on a path its settings do
  not use.

**Where both use the same name.** `SF_GIT_COMMON_DIR` and `SF_TASKS_CLI` are its own names too,
and it keeps them.

**Its own names** (`sf-slots`, `sf-land`, `sf-cargo`, `sf-bin`, `sf-verify`, `sf-nightly`,
`sf-hooks`, `sf-rust-tests`, `sf-tool-tests`) stay its own. Only Symphony's code stops naming
them.

**What Star Foundry needs to do.** This is a note for Star Foundry's board, not a change made
from here. Under A, Star Foundry:

- moves its pin to the bridge release
- makes its wrappers read the new place first (`tools/tasks.sh`, `tools/surreal.sh`, the
  `symphony` recipe, the gate's verdict reader)
- refreshes its hook link (`just hooks-build`)
- adds the `symphony.toml` settings from the table above

all before its own move. Until then, the bridge keeps its old names working.

## Moving live state

A project's state moves only when nothing can write it. That needs the owner's word at the time,
since it stops that project's board and the shared server.

**What must stop, and why.**

- **The board's SurrealDB.** It holds `data/` and its log open. The board also recognises its
  own server by the data path on that server's command line (`Home::serves` in `server.rs`).
  After a move, a build would not see the running server as its board's, and would start a
  second one on the moved store.
- **The shared server.** It serves both projects' boards and writes `health.json`. It runs the
  sweep, callback dispatch and review callbacks, and publishes verdicts into the verdict
  journals. Its executable is in the install folder that moves.
- **Every board process of that repository:** agents' `tasks` commands, detached sweeps, review
  callbacks and recorders. The move finds them with the board's own match for board commands
  (`launch::procs::board`). It refuses while any runs, naming their pids. This tells the owner
  which agents to stop.

**How the move goes.** The command (`symphony move-state`, run from the main checkout) follows
`tasks-cutover`'s shape: a lock with its pid, steps a second run can resume, and a done marker
written last.

1. Read the board's counts (tasks, messages, sessions) through the running server, then stop it
   (`Home::stop`).
2. For each old folder, put a one-line fence file in its place in one atomic step, swapping the
   file and the folder. Then rename the folder to its new place under `symphony/`. At every
   instant the old name holds either the state or the fence, and the new name holds nothing
   until the state is whole there. All of these moves are renames on one disk, not copies.
3. Move the verdict files of `sf-docs/` (Star Foundry) into `symphony/server/` as one group,
   while nothing publishes. A pending verdict then publishes exactly once, from the new place.
4. Record the board's namespace (`sf`) in `symphony/tasks/`.
5. Start the board's server from the new place, check that it answers with the same counts, and
   write the done marker. Then the owner restarts the shared server.

A run killed at any step is run again. Each step looks at what is where (the old folder, the
fence, the folder in flight, the new folder) and goes on. It refuses only a state it does not
recognise, such as both an old and a new folder holding state.

**One build reads the board at every step.**

- **Before the move:** older builds and the bridge release read the same old folder, and the
  board's schema record already keeps writers of different versions apart.
- **During the move:** nothing reads it. Both servers are stopped. A bridge build that finds a
  fence and no new folder refuses ("a move is in progress or stopped: run it again") and never
  makes a board.
- **After the move:** the bridge and later builds read the new folder. An older build meets the
  fence. Every path under it fails, and the board's start cannot make its folder there, so the
  older build says so instead of starting an empty board. Fences stay for good: they cost a few
  bytes, and older `tasks` builds linger.

**In a worktree's git folder**, nothing moves:

- During the bridge the sweep takes both removal locks, the old name and the new, so an older
  sweep and a newer one never split a lock.
- The verdict cache is read under the new name only. A miss just recomputes it.

**The order**, under A:

1. The slices below land, and the bridge release is tagged
   (`tools/release-nightly` makes releases).
2. Star Foundry takes it (its board).
3. This repository's state moves first: a small board, and a rehearsal.
4. Star Foundry's moves, each on a day the owner picks.
5. The next release drops the bridge's old reads, but keeps the refusal of an old folder that
   never moved, which names the command.

Before each live move, the command runs on a clone of that repository's folders (`cp -c`, as
`tasks-reclaim --dir` rehearses) in a scratch common dir, with private ports, and its time is
recorded.

## Slices

These are subtasks of `tools/no-star-foundry-names`, filed after the verdict. Each carries the
compatibility read the card asks for.

1. **`…/variables`** (independent):
   - one helper that reads the new name, then the old, and says once when it used the old
   - the 19 variables of a1; the 88 of a2 and the prefix filters
   - `sf.codexSteer` and the shipped skill
   - the setting and reading of `SF_SYMPHONY_HANDOVER`
2. **`…/state-paths`** (first of the state chain; it lands before what reads it):
   - every folder resolved as the new one, else the old one, else a refusal at a fence, else
     made new
   - the worktree files under both lock names
   - the temp and target names
   - `symphony init`'s next steps naming the new SurrealDB path
   - `tasks field` reading the machine folder's record
3. **`…/state-move`** (after 2; it absorbs `tools/studio/state-migration`):
   - the move command, tested on private fixture common dirs: killed at each step and run again;
     an older build after it meeting the fence; a pending verdict published once; a live board
     process making it refuse
   - then the rehearsal on a clone of each board
4. **`…/integration-points`** (independent):
   - Star Foundry's tool names out of the code, per the table above
   - the `symphony.toml` settings, with tests
5. **`…/formats-and-tests`** (independent):
   - `symphony-component` and the read-audit header
   - `tasks-cutover` retired
   - the 96 test temp names and the fixtures' shell variables
6. **`…/docs-and-skills`** (after 1, 2 and 4, whose names it describes):
   - guides, READMEs, `docs/TASKS.md`, `docs/MACHINE_WORK.md` and the skills
   - the stale `symphony/crates/…`, `tools/tasks.sh` and `tools/tasks.py` mentions
7. **`…/bridge-removal`** (after both moves, in the next release):
   - the old reads go
   - the fences and the refusal of an unmoved folder stay
   - after this, `git grep -iE "sf[-_]|star.?foundry"` returns only those, the standalone guard
     and the dated records

The moves themselves are not slices. They are the owner's word on a day each. Slices 1, 5 and 6
touch most packages' tests, so they go when few branches are open.

**Other work that names these.**

- `tools/one-build-folder-for-worktrees`: `SYMPHONY_LOW_DISK_GB`.
- `tools/run-record`: `symphony/runs/`.
- `tools/studio/state-migration`: folded into slice 3.
- `tools/release-nightly`: the bridge needs a tagged release.

## Verified and inferred

**Read:**

- the counts, by the commands above
- every code location named
- the folders, sizes and processes, read-only
- Star Foundry's uses, from its tree

**Inferred, not tried:**

- **The atomic swap.** macOS swaps a file and a folder in one step with `renamex_np`'s
  `RENAME_SWAP`, and Linux with `renameat2`'s `RENAME_EXCHANGE`. Slice 3's tests try it on APFS.
- **The fence.** Rust's `create_dir_all` refuses a path that is a file, which is what makes an
  older build fail at a fence instead of starting a board.
- **The shared server after a move.** Whether a bridge-release server finds a moved board
  without a restart is not read here. The plan stops it.
- **`SF_CONTENT_DIR`.** That it is already absent from the server's builds.
- **The cost of a namespace export and import** on Star Foundry's 6.3 GB store. Not measured.
