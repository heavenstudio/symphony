# Symphony

The app's workspace owns its server and Dioxus client under `apps/`, and the review
library, wire types, task board, working-directory scanner, headless Chrome driver and
the tests' CPU meters and waits (`crates/test-time`, RULES.md `tests/no-test-fails-on-load`)
under `crates/`.
The app's formats and operating contract live in [docs/](docs/README.md).
A consuming project owns its landing policy and shared-service safeguards.

## Rust

`Cargo.toml` and `Cargo.lock` belong to this workspace. Members inherit its package
settings and dependency versions. The containing project's thin host and board-only
packages consume these libraries through an immutable Git dependency pin. The plugin kit, the
helpers and the app's wire types (`symphony-api`) are made to be published on crates.io as well,
each by its version, and every other member keeps the workspace's `publish = false`
([RELEASES](docs/RELEASES.md#packages-on-cratesio)).
A project's settings are
its `symphony.toml` (generic defaults in `crates/symphony/src/project.rs`; the
board itself reads `[board] on_review`, the command a task moving to
review starts detached from the main checkout, in `crates/tasks/src/on_review.rs`, and
`[board] on_land`, the one a landing starts, in `crates/tasks/src/on_land.rs`), and its
own code is a plugin: the traits and block vocabulary in `crates/symphony-plugin`,
the implementation in a crate of the project's that its host links, selected by
the manifest's `[plugins] crates`, or that answers from a process of its own which
`[plugins.process]` starts ([PLUGIN_PROTOCOL](docs/PLUGIN_PROTOCOL.md)). A plugin renders to generic blocks, never HTML,
and the client draws each block with one component. Only the plugins a project
selects answer for it: its card kinds, watches, pages and actions. A project's
documents are served from its `[[entrypoints]]` (`crates/symphony/src/entrypoint.rs`;
none written is `/docs/` over `docs/`). A document is named by its route, its
entrypoint's URL and its path in the folder (`docs/…` by the convention), in every
link and on the wire; its reads, saves and commits name the file `Project::file`
maps it to, and every walk goes through `Project::walk`, so what an entrypoint's
patterns and renderers leave out is never listed or read. The Symphony's own routes
(`symphony_api::SYMPHONY_ROUTES`, which `assets/shell.js` repeats) win over every
entrypoint.

One server serves every project the machine folder's registry holds
(`crates/symphony/src/machine/registry.rs`: `projects.json` beside the server's record,
in `SYMPHONY_MACHINE_DIR` when set), each under `/<id>/` with its own watcher, feed,
board snapshot and branch scan, and, while it has a board, its own idle sweep and embedding
catch-up (`front::maintenance`); the server reads the registry again each second, so a project
registered or removed starts or stops them. A project's state stays in its checkout: paths under
its artifacts folder go through `Project::artifact`, never an `artifacts/` literal.
The projects share the server's origin, so the client keys what it keeps in the
browser by the project's id (`project::stored` in `apps/symphony-ui`).
The host rule (`crates/symphony/src/machine/host.rs`) decides whether a start's build
serves, waits for or replaces the running one. A start in a linked worktree (its git dir has a
`commondir`: a submodule or a `--separate-git-dir` checkout is a main checkout), whichever build it
runs, never starts the one server, registers a project with it or replaces it
(`apps/symphony-server/src/starter.rs`): it prints the running server's page for its worktree when
that server serves the project, and is refused otherwise, naming `just symphony --port 0`, a server
of its own. `--shared` there is the owner's word that the start may register the project and
start the one server when none runs; the host rule never lets a start in a linked worktree replace
a running one, and `project add` run in one registers no project of that worktree's own
repository, though it registers another repository's. A project's look
is its theme package (`crates/symphony/src/theme.rs`), served as `/<id>/theme.css` and inlined in
its pages. Tests use a temporary machine folder.

Use `just lint-rs`, `just test-rs`, `just symphony-ui-dist` and `just symphony-ui-dev`
from this folder. `tools/cargo.sh` runs ordinary Cargo in this workspace, preserving
its arguments, streams, status and inherited limits. A build or test goes through
`tools/record.sh`, as do the `lint-rs`, `test-rs`, `client-check`, `client-update`
and `verify` recipes, each as one operation: the board's recorder (`tasks run`, the
prepared `.target/bin/tasks` of this checkout or of the main checkout a worktree
belongs to) records it for the Build and Test columns (docs/MACHINE_WORK.md), and
with none that answers within 2 s, `TASKS_RECORDER=off`, inside a recorded run or in a
tree without `tools/record.sh` it is exec'd as it is. On macOS the recorder becomes
the build it records and watches it from a process of its own, so descriptors, the
signal mask, ignored signals and a kill reach the build as they would without it.
It uses this folder's `.target/`
unless `CARGO_TARGET_DIR` names another. In a linked worktree it makes a missing target
folder first as a copy-on-write clone of the main checkout's `.target/debug`, less the
workspace's own units (`tools/seed-target.sh`, one `clonefile` call by `tools/seed-clone.rs`
on macOS; `SYMPHONY_SEED_TARGET=off`), so worktrees
share compiled dependencies on disk; never one target folder, where Cargo names two
worktrees' packages alike and each runs the other's build. The board's sweep clears a
finished worktree's build output ([TASKS](docs/TASKS.md)). It supplies no project gate
planner, test selection, certificate or shared slot pool. The toolchain
pin is `rust-toolchain.toml`. The containing project keeps its own source launcher,
slot wrapper and gates, which compile and test its own consumers.

`tools/rs-bin.sh NAME` builds a declared workspace binary with Cargo before every
run and runs `rs-bin/NAME` in the target folder, a hard link renewed only when Cargo
relinked, so a no-op build costs no new executable's first run. `--build NAME` prints
that path, `--where NAME` names Cargo's own copy without building or attesting
freshness, and `--link NAME` builds and points `.target/bin/NAME` at it; that build
is never recorded. A board
binary run from a linked worktree against the live board runs the main checkout's
launcher instead ([the board guide](crates/tasks/AGENTS.md)), and the board itself
moves its live schema forward only for a build of the schema source the main checkout
holds, or `just tasks migrate` run there where it holds none. None searches a
containing project. `just symphony
--port 0` runs a server of its own; the server CLI retains shared-server selection.

`symphony-build` (`crates/symphony/src/bin/symphony-build.rs`, `symphony::cli::build_main`) holds
the commands only this repository's own build and release run use: `publish-client`,
`release-inputs`, `size-budget`, `split-repair` and `release`. The recipes and scripts run it by
`tools/rs-bin.sh symphony-build`; it is never shipped or installed (`init::BUILDS` does not name
it), so neither the shipped `symphony` nor a consuming host that calls `symphony::cli::main`
links them ([CHECKS](docs/CHECKS.md#size-budgets)).

`just client-update` builds the client's four page modules and its program (the pages that
share one runtime, `apps/symphony-ui/src/split.rs`) and publishes their bundle
under `apps/symphony-server/client/public/`. A branch commits no distribution, whatever
it changed: the regenerator a land starts rebuilds it from main's newest tip and lands it
as a commit of its own ([TASKS](docs/TASKS.md#the-regenerator)), so a task builds the
client for its tests and pictures and puts the distribution back (`git checkout --
apps/symphony-server/client/public`). The one exception is a release candidate's fix that
changes what the client is built from: no regenerator builds the candidate, so that branch
commits the distribution its own sources make, and the land keeps main's side of it
([RELEASES](docs/RELEASES.md#the-release-candidate)). Release snapshots use stable paths
under /private/tmp, copy the locked component source without changing its version or features, and
retain the caller's compiler flags, wrappers and resource settings. Snapshot
configuration patches and lock accounting affect only those private copies.
A snapshot holds only what the client's build reads (`symphony-build release-inputs`,
`crates/symphony/src/cli/commands/release_inputs.rs`): the client's package and the local
packages it builds with, the workspace manifest, lock, Cargo configuration, toolchain
pin and the server's notice styles, the sources of the split repair the program's build
runs (`symphony-build split-repair`), and the other members' manifests over empty targets. Its
path, and so the asset names, follow those bytes alone: a change anywhere else needs no new
distribution. dx runs from one path in every checkout and common
dir as well (`apps/symphony-ui/dx-path.sh`), since Cargo mixes its path, as the workspace's
rustc wrapper, into every byte of the client's wasm.
The server package embeds those bytes at build time, so an
installed binary serves them without dx, a source checkout or a runtime extraction.
A source kit's local preview build wins over the embedded default. Explicit source-kit
install directories and `SYMPHONY_UI_DIST` retain their own missing-file errors.
A consuming host uses its pinned embedded client for installed and foreground defaults;
retired implicit bundles cannot replace it. A foreground preview override remains explicit.

`just client-check` rebuilds into a separate directory and compares every freshly
built path and byte with that committed distribution. The distribution keeps the build
`symphony-build publish-client` publishes and the two published before it, newest first in its
`builds.json`, for pages opened on them (`KEEP` in `crates/symphony/src/cli/publish.rs`: two
previous builds is the orchestrator's choice, not the owner's, who can change it); a page whose
build is gone loads again onto the current one. The check allows those older builds and refuses
a build or a file beyond them. `just verify` runs lint, that real client check, the shipped
binaries' size budgets ([CHECKS](docs/CHECKS.md#size-budgets)), the docs audit and the
workspace's native/headless browser tests locally, each step whatever the ones before it did and
with a log of its own (`tools/verify.sh`): the release run's full verification, never a
task's gate ([CHECKS](docs/CHECKS.md#the-release-run)). Local acceptance requires a working
Chrome executable; the consuming project's game, display and gate binaries are not prerequisites.
This repository has no hosted GitHub CI requirement.

Native board fixtures use the fixture source-root function to find
`tools/surreal.sh`, or the explicit `SYMPHONY_SURREAL` executable. Resolution and
downloads finish before server readiness is measured. `tools/fixture_process.sh`
closes inherited descriptors and supplies the owned EOF watchdog. These helpers
do not start the shared board or Symphony server.

Read [the client guide](apps/symphony-ui/AGENTS.md) and
[the board guide](crates/tasks/AGENTS.md) before editing those packages. A consuming project applies its own Rust, review and resource policy.
Never run a shared-service restart as a test: use a private port and scratch board.

## This repository as its own project

`symphony.toml` registers this checkout as the project `symphony`: its docs and its
board are served under `/symphony/` by the machine's one server, and its board is this
repository's own, in its git common dir. Symphony work is filed, staffed and reviewed
on that board, never on a consuming project's. Every agent working here follows the
standing rules in [RULES.md](RULES.md).

**Its own names.** Symphony names only its own variables (`SYMPHONY_`), state folders
(`symphony/` in the git common dir), formats and settings, never a consuming project's
tools: `crates/symphony/tests/hooks/own_names.rs` fails on an old state or tool name
outside the fences it lists, each with its reason (a past incident's record in
`docs/history/`), and on the first consuming project's name, in any spelling or identifier
built from it, in a tracked file's text or path: say "the consuming project". The state
folders are named once, in `crates/tasks/src/folders.rs`.

**Memory lives in the repository.** Nothing goes in an agent's own memory folder
(`~/.claude/projects/…/memory/`): what must outlast a session is guidance here or in a
package's AGENTS.md, a skill, a doc or a board task, and it is committed, so every
agent sees it. In a Claude Code session the memory guard refuses the writes there it
can read (below), and `.claude/settings.json` denies Edit there as well.

**Models.** The model each role runs on is a setting, so a change of mind is one edit:
`[agents.models]` in `symphony.toml` (`orchestrator`, `executor`, `reviewer`), which a board
launch takes and `just tasks limits` prints with where each came from
([TASKS](docs/TASKS.md#limits-and-models)). Until this file names them (it waits for the
machine to run a build that reads the table), the orchestrator may run on the most capable
model, for diagnosis and design, and executors and reviewers run on Opus, as `[agents] launch`
says for a board launch. An Agent launch passes the model `just tasks limits` names for the
role, `model: "opus"` today (a subagent otherwise inherits its orchestrator's model). A
resumed agent keeps the model it started on. The work-in-progress limits are settings the same
way, in `[limits]`.

**Landing and pushing main.** An orchestrator lands an approved task with `just land SLUG --as
HANDLE`, never a merge by hand: it merges, closes the task and pushes `main` to `origin` as its
last step, without asking each time (the owner, 2026-11-11: "just land should do the push"; before
it, "do push it yes"). The push fetches first, merges what `origin/main` gained, never forces,
and pushes no other branch and no tag; one that fails leaves the landing done here and names the
command to retry. A land here takes seconds: it runs no generator and, with `[land] check =
false` in `symphony.toml`, no check, the docs audit being evidence at the tip and part of the
release run's `just verify`. It keeps main's side of the client distribution a branch changed, and
the regenerator it starts detached rebuilds that from main's newest tip and lands it after
([TASKS](docs/TASKS.md#the-regenerator)); one interrupted is run again, which finishes its push
and close. It lands
only a fresh reviewer's approve naming the tip, or the orchestrator's on a fast fix (`just tasks
create --fast-fix WHY`); any other approve, a conflict or a red goes back on the thread
([the default landing](docs/TASKS.md#the-default-landing)). An orchestrator lands an approved
task without asking the owner, and `just land` runs with no permission prompt: the fresh
reviewer's approve is what decides it (the owner, 2026-11-12: "asks to lands should be auto
approved by orchestrators"). A decision the owner must make goes through the decision docs, a
card in [DECISIONS](docs/DECISIONS.md), not a question in chat. The one thing that pushes a tag
is the release run (`just release`, [RELEASES](docs/RELEASES.md#who-runs-it)), the release that run
cut; running it or `--commit` by hand and installing its schedule are the owner's call, and an
orchestrator does either only on the owner's word. The owner, 2026-11-12: "It would be nice to
release a new version every given number of tasks done, say 20 and refresh server/clients", then
"yes, a green release should refresh". So a land also starts a release run once 20 tasks have landed
since the last release or the last run that checked something (`[releases] every_landings`, run by
`[board] on_land` as the linked `.target/bin/symphony-build release --due`, a link that any
`just release` in the main checkout makes or renews, which counts with git and builds nothing
short of due; [the cadence](docs/RELEASES.md#the-cadence)), one run at a time; with
no release and no run on record the first landing counts the line's whole history and starts a
run at once. A green release then refreshes the machine
([the refresh](docs/RELEASES.md#the-refresh)): each consuming project's pin moved to it on a
branch of its own through its own commit gate, its main fast-forwarded to that branch's one
commit, by its hash, in its main checkout through its own hooks when that commit is the move and
nothing more and that checkout is on its default branch with no tracked edits (never pushed, so
the project's own next push carries it; no merge commit, no hook bypassed), and its board moved to
the schema of the pin its main locks, after a snapshot. The shared server takes it by its own
reload once the main
checkout it is built from has the new `Cargo.lock` (`apps/symphony-server/src/reload.rs`, which
builds the new server through `crates/symphony/src/reload.rs` and hands it its sockets), and the
refresh waits for that, bounded. What stays by hand: a consumer whose gate or hooks refuse the move
lands `symphony/RELEASE` through its own landing, and a main checkout off its default branch or with
tracked edits is its owner's to put right; then an orchestrator reruns `just release --refresh`
without asking. Pushing a consumer's branch and restarting the shared server stay separate acts
that need their own word. A pin moved by hand to a commit of main waits until `just tasks
regenerate --status --wait` exits 0 and takes the commit it names, so it never carries a client
older than its sources ([TASKS](docs/TASKS.md#the-regenerator)); a release run waits for the
regenerator and releases only a main its own `just client-check` (in `just verify`) passes, so a
refresh's pin carries none older either.

**Board commands.** Run them as `just tasks … --as HANDLE`: a subagent's tool calls
count as its heartbeat only after a command of that shape binds it, and one that runs
the binary by its path reads lost while it works. A build or test is recorded for the
task its session holds; an orchestrator running one for a task, or a subagent without
a session of its own, names it: `TASKS_TASK=SLUG just verify`, with
`TASKS_SESSION=HANDLE` when it has a handle (docs/MACHINE_WORK.md).

**Evidence, not the suite.** A task's gate is the tests closest to what it changed and
`just lint-rs`, with the client built for a client change (its distribution never committed),
and the docs audit for a docs change, posted on its thread as evidence
a reviewer judges and stamps (RULES.md `workflow/evidence-is-the-gate`; how to pick the tests and
what each kind of change shows: [CHECKS](docs/CHECKS.md)). No task runs `just verify`, whatever
its brief asks: it is the release run's (`just release`), on main's tip in a clean checkout outside
this folder (`../symphony-wt/release`), which tags a green tip as a release
([RELEASES](docs/RELEASES.md)).

## Agent integration

`symphony init --root ROOT` installs the eight generic skills, their Claude links,
Claude allow rules, heartbeat and memory-guard commands, Codex rules and the recipes
the skills name (`crates/symphony/shipped/symphony.just`: `tasks`, `agent-send`,
`symphony`, `symphony-cli`, `symphony-status`, `symphony-reply`, `symphony-ask`, and the
default landing `land`). A project's justfile imports `symphony.just`, which sets
`allow-duplicate-recipes`, so a recipe of the project's own of the same name wins wherever
the import stands, with no setting of its own; where the project's justfile (or a file it
imports) makes the setting itself, init writes `symphony.just` without it, as `just` refuses
the setting made twice and every recipe would fail. Init writes that justfile, its import a
stamped line, only where there is none, and adds `/.target/` and the artifacts folder to
`.gitignore`. Retiring the recipes removes the stamped `symphony.just` and that stamped
line. Each recipe runs
the project's build linked at `.target/bin/NAME`, else the main checkout's link from a
linked worktree, else NAME on PATH (`crates/symphony/src/builds.rs`, which the
Symphony's board clients share). Stamped copies are kept current; one that differs from
its stamp, an edit since, is written over all the same, its line saying so and where the
project's own goes (a recipe in its justfile); an unstamped project copy wins.
Project-specific skills and taste data stay in the project.
Retirement removes managed files and links, retaining project data. It writes no
permission rule but the allow rules. A rule carries no stamp, so init removes only one a
landed init wrote and it no longer does: an allow rule (`RETIRED`: the env-prefixed
`Bash(TASKS_SESSION=* just tasks *)`, below) or an ask
rule (`RETIRED_ASK`: those that asked before a land, `Bash(*SYMPHONY_LAND*)` and
`Bash(just tasks land *)` and `Bash(just tasks -* land *)` bare and after `TASKS_SESSION=*`),
with an ask list they leave empty; any other it finds is the project's own and stays. The
allow rules, like the Codex rules, allow the board and reply recipes (`tasks`, `agent-send`,
`symphony-status`, `symphony-reply`, `symphony-ask`) and `land`, and not the two server
recipes: `just symphony` starts, replaces or stops the machine's shared server and
`just symphony-cli` reaches `serve` and `init`, so an agent is asked first.
`just land` needs no prompt: what the default land lands is decided by its approval check, a
fresh reviewer's approve standing on the tip ([TASKS](docs/TASKS.md#the-default-landing)), and
a prompt would add only a wait on the owner (the owner, 2026-11-12: "should not require
permission from me"). A project whose own `land` recipe replaces the default gets the same
allow rule, and its own landing policy decides what lands. The `land` recipe sets
`SYMPHONY_LAND`, without which `tasks land` refuses, so `just tasks land` run by itself, an
option such as `--as H` before `land` or not, never lands: a guard against accident, not the
gate. No Codex rule prompts, and no rule, Claude's or Codex's, allows a command with an env
prefix: each Claude rule is a recipe's words and then its one `*`, as Claude Code's `*` matches
any text, spaces included, and only the words before the first one bound a rule
(code.claude.com/docs/en/permissions). The retired `Bash(TASKS_SESSION=* just tasks *)` so
passed `TASKS_SESSION=x sh -c '…' just tasks list` and `TASKS_SESSION=x JUST_JUSTFILE=…
just tasks …`: any program. An agent names its handle with `just tasks … --as HANDLE`, which
binds its heartbeat as the prefix did (`named_handle` in `crates/tasks/src/cli/beat.rs`); a
`TASKS_SESSION=` prefix asks first.
`just tasks` runs every board command but the recorder: its recipe refuses `tasks run`, the
command first after any `--database NAME` as `tasks` reads it, since the recorder runs any
command it is handed, and a build records itself by the board's binary (`tools/record.sh`),
which no rule allows ([MACHINE_WORK](docs/MACHINE_WORK.md#recording-builds-and-tests)); this
repository's own `tasks` recipe refuses it the same way.
The board and reply recipes (`tasks`, `symphony-status`, `symphony-reply`, `symphony-ask`;
this repository's own `tasks` too) set `SYMPHONY_UNASKED`, and under it a place their command
writes must lead, its links and `..` resolved, into the checkout it runs in or, from a linked
worktree its repository lists as its own, the main checkout's artifacts folder (the ask store's,
`project::artifacts`), and into no git folder: not a `.git` below either, in any case of its
letters (git takes a new `.GIT` as its own where the volume ignores case), the checkout's git dir
or the common dir (`crates/tasks/src/confine.rs`). A hook or the config written there would run,
or misdirect, every later git command, and the rest of the main checkout is not a worktree's to
write. The worktree's `.git` file, untracked, cannot lead it into another repository: the main
checkout counts only when its `worktrees/NAME/gitdir` names that worktree back. So `tasks export
--out`, `symphony reply PATH`, `symphony ask --db` or `--root` (the store it makes) and `symphony
status --tidy --root` refuse any other place with one line naming the path, where it leads and
the places allowed, and write nothing; under the mark `tasks export` also replaces only an
earlier snapshot (a file whose first line is `# Task board snapshot`). An agent the board
starts there, in auto mode, stays in those places or under the project's worktree base the same
way: `tasks launch --worktree` and `restore` refuse any other folder, a launch takes no
`--prompt` (an agent's first message is its person's word: the rest goes on the thread),
`tasks api`'s `record_launch` records no other folder and no conversation id, its `bind_agent`
and `tasks beat` bind no agent id (the hook's beat binds the session's own), and `tasks
callback` registers a Codex thread only as the caller's own (`CODEX_THREAD_ID`), what a restore
then resumes; `--as` names any handle, so an agent's own conversation may be given to another
(`crates/tasks/src/launch/bounds.rs`, [TASKS](docs/TASKS.md#where-it-lives)). The binaries run by
themselves, which no rule allows, write where they are told: the owner's way to such a place. A
project's own `tasks` or `symphony-*` recipe, which the allow rule still passes, gets the guard
only by setting the marker itself (starting with `{{ _symphony_unasked }}`, as the comment above
it in `symphony.just` says).
Init refuses a `--root` that is not a folder or is in no git repository, making nothing. It
ends by printing what is left (`Next:`): the project file, its document folders, the
justfile's import, the links to the project's builds the recipes and hooks find nowhere (with
the `ln -s` for those beside the running `symphony`, and the `cargo build` in a Symphony
checkout for the rest), `just` when it is not on PATH, the board's SurrealDB (the line that
fetches the pinned tarball and checks its SHA-256), the board's making while it is not made
(`just tasks list`: the Symphony only reads a board) and the machine's registration; then that Claude
Code counts the allow rules only once the folder is trusted. It warns on stderr while
the heartbeat has no build of `symphony` or `tasks` to run. Every `just NAME` in
the shipped skills, the docs, the guides and the binaries' strings is an installed recipe or
the consumer's landing, this repository's own where its lists (`tools/named-recipes.txt`)
name it for that file, or in a guide a recipe of its justfile: one rule and one reader
(`crates/symphony/src/docs_audit/recipes.rs`), by which the docs audit judges, in this
checkout, its documents but the records, what it delivers and its guides, and
`crates/symphony/tests/hooks/named_recipes.rs` what it delivers and the binaries' strings
([CHECKS](docs/CHECKS.md#the-recipes-a-text-names)).

`symphony hook tasks-beat` reads the tool event from stdin and finds its board through
the board's project lookup. Its bound keys belong to that repository's common directory;
scratch overrides remain explicit. It runs `TASKS_BIN`, else the checkout's
`.target/bin/tasks`, else, from a linked worktree, its main checkout's, and makes no link.
It starts no server, builds or
hashes no sources, prints nothing and never refuses a tool call. Settings run the
project's `symphony` as the recipes find it: the session's `.target/bin/symphony` link,
else its main checkout's (a session in a linked worktree with none of its own), else
`symphony` on PATH; with none the call passes quietly. The beat finds `tasks` the same
way. Project build setup refreshes those links explicitly, alongside its own policy hooks.

`symphony hook memory-guard` (`crates/symphony/src/hook/memory_guard.rs`) runs before a
Claude Code session's Write, Edit, MultiEdit, NotebookEdit or Bash call. It refuses,
with exit 2 and one line naming where the guidance goes instead, a write into an
agent's memory folder (`~/.claude/projects/*/memory/`, `~/.codex/memories/`): a write
tool's path there, or a shell command that names the folder and writes anything. It
reads the command as text, with `.` and `NAME/..` parts folded. A command names the
folder by its path, by running in it, or by a folder above it (`.claude`,
`.claude/projects/NAME`, `.codex`, in the command or as the folder it runs in) beside a
`memory` or `memories` part. A write is a redirection into a file (not a descriptor or
`/dev/null`), `sed -i`, `find -delete` or `-exec`, or a writer, unpacker, downloader,
interpreter or `xargs` named by its word, path or `\` escape. Reads pass. It misses
`git clone` or `git -C … init`, `sort -o`, `uniq IN OUT`, `mktemp`, `ed`,
`awk -i inplace`, `xattr -w` and a write through a link made before. Text that names
the folder beside a write (a commit message, a board post, a heredoc) is refused:
say "the memory folder", or put the text in a file with Write. A Codex session runs
no Claude hook and is not guarded. Its installed command passes on that refusal
alone: with no link, or a link to a build older than the guard, the call goes through.

**Looking at a page.** An agent sees a page with `just symphony-cli shot URL --out FILE.png`:
a private headless Chrome (`crates/chrome-devtools`, the driver the client's browser tests
use, on a profile and process of its own that die with the run) opens URL, waits until
the page has drawn, its frames' and dedicated workers' loads among it, dismissing each dialog
as it opens (a confirm or a prompt as by its Cancel), runs the steps `--click CSS`,
`--type CSS=TEXT`, `--key NAME`, `--hover CSS`, `--eval JS` (its JSON on stdout),
`--pause MS` and `--shot FILE.png` in the order given, then saves the PNG. `--phone`, `--size WxH`,
`--full-page` and `--scheme dark` set the view, `--wait-for CSS` and `--timeout SECS`
bound the waits, and `--report` prints the console errors (a frame's naming its address),
failed requests, dialogs dismissed and horizontal overflow it saw; `--help` says the
rest. It only reads the page, so the live board
(`http://127.0.0.1:8765/symphony/tasks`) is fair to shoot; it never starts, stops or
reconfigures a server. Pictures kept as a task's evidence go under the project's artifacts
folder in the task's slug (`artifacts/SLUG/` by default) and are named on its thread.
The shot leashes Chrome by running its own program again, so a host binary that calls
`symphony::cli` runs `chrome_devtools::leash_main()` first, as
`crates/symphony/src/bin/symphony.rs` does; without it a shot fails with "leash exited
before DevTools readiness".

## Command ownership

`just symphony` runs the server, `just symphony-cli ARGS` the document CLI, and
`just tasks ARGS` and `just agent-send ARGS` the board entrypoints, and `just land SLUG`
the default landing (`tasks land`, `crates/tasks/src/land.rs`): a task's approved branch
merged on the default branch, checked by `[verdicts] check` in a worktree of its own
unless `[land] check = false`, the task closed and its worktrees swept, the branch pushed,
and the regenerator (`tasks regenerate`) started detached for what the project's `[land]
generators` make (docs/TASKS.md). `tasks land`
runs only under that recipe, and never where the project's justfile runs a `land` of its
own. This repository's justfile imports the installed `symphony.just` like any project's,
and its own recipes of those names win: they execute this workspace's declared binaries
through ordinary Cargo; `land` it takes as installed, through its own `tasks`. The
standalone workspace requires no project gate or landing executable.

`just release` cuts this repository's releases ([RELEASES](docs/RELEASES.md)): main's tip
verified in full in a clean checkout outside this folder, judged by the checks this repository
owns and never by a consuming project's suite, each red check filed on this board, a green tip
tagged `release-YYYY-MM-DD` and that tag alone pushed, then the machine refreshed to it. A red
tip is held as the release candidate, the local branch `release-candidate` that is never pushed,
until its reds' own tasks, started there, land; then it is rerun and released, and main's tip is
not run meanwhile.
`--commit COMMIT` releases a commit of main out of cycle, `--due` starts a run only once
`[releases] every_landings` tasks have landed or the release candidate's rerun or drop is due
(what a land's `[board] on_land` runs here), `--refresh` refreshes the machine to the newest
release again, `--rerun` reruns the release candidate and `--drop` drops it (each an
orchestrator's retry), and `--status` runs nothing. Running a release run or
`--commit` by hand and installing its schedule (`--schedule` prints it) are the owner's call. It
is Symphony's own release command, not a recipe `symphony init` installs.

Board roles, verdicts and configured review callbacks belong to Symphony, and so does the
default landing, convention over configuration. A consumer with a landing of its own keeps
it: its `land` recipe replaces the Symphony's everywhere for that project, and the workflow
examples' `land-approve`, `land-stamps`, `land-request`, `merge-risk` and `merge-feature`
recipes, with its verification, certificates and approval policy, are consumer integration,
not standalone product commands. This workspace supplies no stamp backend or aliases
forwarding to a containing project.
