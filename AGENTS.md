# Symphony

The app's workspace owns its server and Dioxus client under `apps/`, and the review
library, wire types, task board, working-directory scanner and headless Chrome driver
under `crates/`.
The app's formats and operating contract live in [docs/](docs/README.md).
A consuming project owns its landing policy and shared-service safeguards.

## Rust

`Cargo.toml` and `Cargo.lock` belong to this workspace. Members inherit its package
settings and dependency versions. The containing project's thin host and board-only
packages consume these libraries through an immutable Git dependency pin. A project's settings are
its `symphony.toml` (generic defaults in `crates/symphony/src/project.rs`; the
board itself reads `[board] on_review`, the command a task moving to
review starts detached from the main checkout, in `crates/tasks/src/on_review.rs`, and
`[board] on_land`, the one a landing starts, in `crates/tasks/src/on_land.rs`), and its
own code is a plugin: the traits and block vocabulary in `crates/symphony-plugin`,
the implementation in a crate of the project's that its host links, selected by
the manifest's `[plugins] crates`. A plugin renders to generic blocks, never HTML,
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
board snapshot and branch scan. A project's state stays in its checkout: paths under
its artifacts folder go through `Project::artifact`, never an `artifacts/` literal.
The projects share the server's origin, so the client keys what it keeps in the
browser by the project's id (`project::stored` in `apps/symphony-ui`).
The host rule (`crates/symphony/src/machine/host.rs`) decides whether a start's build
serves, waits for or replaces the running one. A project's look is its theme
package (`crates/symphony/src/theme.rs`), served as `/<id>/theme.css` and inlined in
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
on macOS; `SF_SEED_TARGET=off`), so worktrees
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

`just client-update` builds all eight real client modules and publishes their bundle
under `apps/symphony-server/client/public/`. A branch commits no distribution, whatever
it changed: the regenerator a land starts rebuilds it from main's newest tip and lands it
as a commit of its own ([TASKS](docs/TASKS.md#the-regenerator)), so a task builds the
client for its tests and pictures and puts the distribution back (`git checkout --
apps/symphony-server/client/public`). Release snapshots use stable paths under /private/tmp,
copy the locked component source without changing its version or features, and
retain the caller's compiler flags, wrappers and resource settings. Snapshot
configuration patches and lock accounting affect only those private copies.
A snapshot holds only what the client's build reads (`release-inputs`,
`crates/symphony/src/cli/commands/release.rs`): the client's package and the local
packages it builds with, the workspace manifest, lock, Cargo configuration, toolchain
pin and the server's notice styles, and the other members' manifests over empty
targets. Its path, and so the asset names, follow those bytes alone: a change
anywhere else needs no new distribution.
The server package embeds those bytes at build time, so an
installed binary serves them without dx, a source checkout or a runtime extraction.
A source kit's local preview build wins over the embedded default. Explicit source-kit
install directories and `SF_SYMPHONY_UI_DIST` retain their own missing-file errors.
A consuming host uses its pinned embedded client for installed and foreground defaults;
retired implicit bundles cannot replace it. A foreground preview override remains explicit.

`just client-check` rebuilds into a separate directory and compares every freshly
built path and byte with that committed distribution. The distribution keeps the build
`publish-client` publishes and the two published before it, newest first in its
`builds.json`, for pages opened on them (`KEEP` in `crates/symphony/src/cli/publish.rs`: two
previous builds is the orchestrator's choice, not the owner's, who can change it); a page whose
build is gone loads again onto the current one. The check allows those older builds and refuses
a build or a file beyond them. `just verify` runs lint, that real client check, the shipped
binaries' size budgets ([CHECKS](docs/CHECKS.md#size-budgets)), the docs audit and the
workspace's native/headless browser tests locally: the nightly's full verification, never a
task's gate ([CHECKS](docs/CHECKS.md#the-nightly)). Local acceptance requires a working
Chrome executable; the consuming project's game, display and gate binaries are not prerequisites.
This repository has no hosted GitHub CI requirement.

Native board fixtures use the fixture source-root function to find
`tools/surreal.sh`, or the explicit `SF_SURREAL` executable. Resolution and
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

**Memory lives in the repository.** Nothing goes in an agent's own memory folder
(`~/.claude/projects/…/memory/`): what must outlast a session is guidance here or in a
package's AGENTS.md, a skill, a doc or a board task, and it is committed, so every
agent sees it. In a Claude Code session the memory guard refuses the writes there it
can read (below), and `.claude/settings.json` denies Edit there as well.

**Models.** The orchestrator may run on the most capable model, for diagnosis and
design. Executors and reviewers run on Opus: an Agent launch passes `model: "opus"`
(a subagent otherwise inherits its orchestrator's model), and `[agents] launch` in
`symphony.toml` says the same for a board launch. A resumed agent keeps the model it
started on.

**Landing and pushing main.** An orchestrator lands an approved task with `just land SLUG --as
HANDLE`, never a merge by hand: it merges, closes the task and pushes `main` to `origin` as its
last step, without asking each time (the owner, 2026-11-11: "just land should do the push"; before
it, "do push it yes"). The push fetches first, merges what `origin/main` gained, never forces,
and pushes no other branch and no tag; one that fails leaves the landing done here and names the
command to retry. A land here takes seconds: it runs no generator and, with `[land] check =
false` in `symphony.toml`, no check, the docs audit being evidence at the tip and part of the
nightly's `just verify`. It keeps main's side of the client distribution a branch changed, and
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
is the nightly (`just nightly`, [RELEASES](docs/RELEASES.md#who-runs-it)), the release that run
cut; running it or `--release` by hand and installing its schedule are the owner's call, and an
orchestrator does either only on the owner's word. The owner, 2026-11-12: "It would be nice to
release a new version every given number of tasks done, say 20 and refresh server/clients", then
"yes, a green release should refresh". So a land also starts the nightly once 20 tasks have landed
since the last release or the last run that checked something (`[releases] every_landings`, run by
`[board] on_land` as the linked `.target/bin/symphony nightly --due`, which counts with git and
builds nothing short of due; [the cadence](docs/RELEASES.md#the-cadence)), one run at a time; with
no release and no run on record the first landing counts the line's whole history and starts a
night at once. A green release then refreshes the machine
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
tracked edits is its owner's to put right; then an orchestrator reruns `just nightly --refresh`
without asking. Pushing a consumer's branch and restarting the shared server stay separate acts
that need their own word. A pin moved by hand to a commit of main waits until `just tasks
regenerate --status --wait` exits 0 and takes the commit it names, so it never carries a client
older than its sources ([TASKS](docs/TASKS.md#the-regenerator)); a night waits for the
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
its brief asks: it is the nightly's (`just nightly`), on main's tip in a clean checkout outside
this folder (`../symphony-wt/nightly`), which tags a green tip as a release
([RELEASES](docs/RELEASES.md)).

## Agent integration

`symphony init --root ROOT` installs the seven generic skills, their Claude links,
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
Symphony's board clients share). Stamped copies are kept current; an unstamped
project copy wins. Project-specific skills and taste data stay in the project.
Retirement removes managed files and links, retaining project data. It writes no
permission rule but the allow rules. A rule carries no stamp, so init removes only one a
landed init wrote and it no longer does: an allow rule (`RETIRED`, none so far) or an ask
rule (`RETIRED_ASK`: those that asked before a land, `Bash(*SYMPHONY_LAND*)` and
`Bash(just tasks land *)` and `Bash(just tasks -* land *)` bare and after `TASKS_SESSION=*`),
with an ask list they leave empty; any other it finds is the project's own and stays. The
allow rules, like the Codex rules, allow the board and reply recipes (`tasks`, `agent-send`,
`symphony-status`, `symphony-reply`, `symphony-ask`) and `land`, and not the two server
recipes: `just symphony` starts, replaces or stops the machine's shared server and
`just symphony-cli` reaches `serve`, `init` and `publish-client`, so an agent is asked first.
`just land` needs no prompt: what the default land lands is decided by its approval check, a
fresh reviewer's approve standing on the tip ([TASKS](docs/TASKS.md#the-default-landing)), and
a prompt would add only a wait on the owner (the owner, 2026-11-12: "should not require
permission from me"). A project whose own `land` recipe replaces the default gets the same
allow rule, and its own landing policy decides what lands. The `land` recipe sets
`SYMPHONY_LAND`, without which `tasks land` refuses, so `just tasks land` run by itself, an
option such as `--as H` before `land` or not, never lands: a guard against accident, not the
gate. No Codex rule prompts, and none allows a command with an env prefix.
`just tasks` runs every board command but the recorder: its recipe refuses `tasks run`, the
command first after any `--database NAME` as `tasks` reads it, since the recorder runs any
command it is handed, and a build records itself by the board's binary (`tools/record.sh`),
which no rule allows ([MACHINE_WORK](docs/MACHINE_WORK.md#recording-builds-and-tests)); this
repository's own `tasks` recipe refuses it the same way.
Init refuses a `--root` that is not a folder, making nothing. It ends by printing what is
left (`Next:`): the project file, its document folders, the justfile's import, the links
to the project's builds the recipes and hooks find nowhere (with the `ln -s` for those
beside the running `symphony`, and the `cargo build` in a Symphony checkout for the
rest), `just` when it is not on PATH, the board's SurrealDB (the line that fetches the
pinned tarball and checks its SHA-256) and the machine's registration; then that Claude
Code counts the allow rules only once the folder is trusted. It warns on stderr while
the heartbeat has no build of `symphony` or `tasks` to run. Every `just NAME` in
the shipped skills, the docs and the binaries' strings is an installed recipe or the
consumer's landing (`crates/symphony/tests/hooks/named_recipes.rs`).

`symphony hook tasks-beat` reads the tool event from stdin and finds its board through
the board's project lookup. Keys and live-build records belong to that repository's
common directory; scratch overrides remain explicit. It starts no server, builds or
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
the page has drawn, runs the steps `--click CSS`, `--type CSS=TEXT`, `--key NAME`,
`--hover CSS`, `--eval JS` (its JSON on stdout), `--pause MS` and `--shot FILE.png` in
the order given, then saves the PNG. `--phone`, `--size WxH`, `--full-page` and
`--scheme dark` set the view, `--wait-for CSS` and `--timeout SECS` bound the waits, and
`--report` prints the console errors, failed requests and horizontal overflow it saw;
`--help` says the rest. It only reads the page, so the live board
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

`just nightly` cuts this repository's releases ([RELEASES](docs/RELEASES.md)): main's tip
verified in full in a clean checkout outside this folder and conciliated with the projects the
machine serves, each red check filed on this board, a green tip tagged `release-YYYY-MM-DD` and
that tag alone pushed, then the machine refreshed to it; `--release COMMIT` releases a commit of
main out of cycle, `--due` runs a night only once `[releases] every_landings` tasks have landed
(what a land's `[board] on_land` runs here), `--refresh` refreshes the machine to the newest
release again (an orchestrator's retry), and `--status` runs nothing. Running a night or
`--release` by hand and installing its schedule (`--schedule` prints it) are the owner's call. It
is Symphony's own release command, not a recipe `symphony init` installs.

Board roles, verdicts and configured review callbacks belong to Symphony, and so does the
default landing, convention over configuration. A consumer with a landing of its own keeps
it: its `land` recipe replaces the Symphony's everywhere for that project, and the workflow
examples' `land-approve`, `land-stamps`, `land-request`, `merge-risk` and `merge-feature`
recipes, with its verification, certificates and approval policy, are consumer integration,
not standalone product commands. This workspace supplies no stamp backend or aliases
forwarding to a containing project.
