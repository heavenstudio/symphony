# Worktree build folders

How agents' worktrees share compiled dependencies, so ten agents do not hold ten copies of them,
the measurements the choice rests on, and the card it is asked on in [DECISIONS](../DECISIONS.md).
Measured 2026-11-11 on this Mac (APFS, 1.95 TB, 41 to 64 GB free while it ran), at load averages
of 250 to 600 from the other agents' builds: every time below is slower than on a quiet machine,
and each pair or trio ran at the same time on the same load.

## The choice

Each worktree keeps a build folder of its own, and:

- **Its first build starts from the main checkout's.** `tools/cargo.sh`, which every build of this
  workspace goes through, makes a linked worktree's missing build folder as a copy-on-write clone
  of the main checkout's `.target/debug` (`tools/seed-target.sh`: `clonefile` on APFS, a reflink on
  Linux, nothing on a volume that cannot clone). The clone is taken under Cargo's own lock on that
  folder, waiting at most two minutes for a build there to end, and leaves out the workspace's own
  packages, so the worktree compiles those from its own sources and reuses every dependency.
- **The board's sweep clears the build output of finished work.** A worktree the sweep keeps loses
  `.target/` and `symphony/.target/` once every task tied to it is done or cancelled, one of them
  done, its HEAD is merged work, and no agent beat there or touched its git files within the hour
  (`crates/tasks/src/sweep/build_output.rs`). A branch with no commits ahead of main is not merged
  work, and a task in progress, in review, blocked or ready keeps it.
- **The board warns when the disk runs low**: under `SF_LOW_DISK_GB` gigabytes free (20), in
  `just tasks status` and on every page of the project.

## Measurements

The build: `tools/cargo.sh test --locked -p symphony-board --no-run`, in new worktrees of this
repository at one commit, each with a different edit to `crates/tasks/src/lib.rs` so their builds
differ. Disk is measured three ways: `du` (which counts a clone in full), the bytes no other file
shares (APFS's private size, `ATTR_CMNEXT_PRIVATESIZE`, read for every file), and what `df` gained
when the folder was deleted, the only figure the machine's other writers cannot move.

| | today: a build from nothing | seeded from main's | one shared build folder |
|---|---|---|---|
| two builds at once, wall time | 495 s and 499 s, both ok | 160 s and 167 s, both ok | 70 s and 71 s, both ok, one wrong (below) |
| three seeded builds at once | | 145, 156 and 174 s, all ok | |
| crates compiled per build | 187 | 2 | 1 for the two |
| `du` per worktree | 2.91 GB | 6.35 GB | |
| private bytes per worktree | 2.02 GB | 0.88 GB, the same in all five | |
| freed when deleted | 2.44 GB | 1.26 to 1.35 GB | |
| making the folder | | 17 s alone, 56 s on a first try under load | |

**Five seeded worktrees**, two built at once and then three at once: 0.88 GB private each,
4.4 GB together, against 2.02 GB each, 10.1 GB, built from nothing (inferred from the two
measured). Deleting one freed 1.26 to 1.35 GB, more than its private bytes: Cargo's copy of a
binary out of `deps/` is itself a clone, its blocks shared by two files of the same worktree and
private to neither.

**This worktree**, the one the change was made in, seeded and then building the board's lib and
test targets and the server's lib tests: `du` 6.20 GB, 1.16 GB private.

**Before**: 15 build folders under `.claude/worktrees/` and `../symphony-wt/` held 118 GB by `du`,
7.9 GB on average, of which the dependencies were 3.6 to 6.3 GB each.

## Not chosen

**One build folder all worktrees share** (`CARGO_TARGET_DIR` pointed at one place) builds the
wrong code. Cargo names a workspace package's output by a hash of its path from the workspace's
top, the same in every worktree, and calls an output fresh when it is newer than its sources. So
two worktrees build into the same files: measured, one build compiled its worktree's edit while
the other waited 71 s on the folder's lock and then, its own sources older than that output, ran
the first worktree's code as its own. Builds also run one at a time on that lock. It would save
the most disk, and no setting makes it correct.

**sccache** saves compile time, not disk: each worktree still holds its full build, and the cache
holds another copy. It is not installed here.

## The seed with sccache

Measured 2026-11-11 17:46 to 17:58, load average 60 to 240, with sccache 0.10.0 run from a scratch
folder (its released binary, checked against its published SHA-256; nothing installed), one cache
folder and one server on a private port. Each build is the board's tests again
(`tools/cargo.sh test --locked -p symphony-board --no-run`) from an empty build folder, the seed
turned off, so every dependency compiles: the most a worktree can diverge from main's build.

| build | time | crates compiled | Rust compiles from the cache |
|---|---|---|---|
| worktree 1, cold cache | 178 s | 187 | 0 of 187 (filling it, 190 MiB) |
| worktree 2, warm cache, at the same time as the next | 188 s | 187 | 0 of 187 |
| worktree 1, no sccache | 233 s | 187 | |
| worktree 1 again, warm cache, a new build folder | 251 s | 187 | 0 of 187 |

Nothing hit. sccache reuses a compile only when its absolute paths match, and every compile names
its build folder (`-L dependency=…/deps`, each `--extern …rlib`), so a build folder never hits
another's entries, even in the same worktree. The cache grew by the same 190 MiB with each folder.
Beyond that, sccache never caches the workspace's own crates, which dev and test builds compile
incrementally, nor what is linked: proc-macro crates and build scripts (74 calls of this build).

So on top of the seed, sccache would hit only for a build folder rebuilt at its own path, after a
`cargo clean` or after the sweep cleared it, where the seed already starts it from main's build.
Its costs: the cache's copy (capped by `SCCACHE_CACHE_SIZE`), a pinned download in the toolchain
setup, a server process, and `RUSTC_WRAPPER`, which the client bundle already sets to its own
wrapper and chains a parent under (`apps/symphony-ui/remap-rustc.sh`). The build recorder wraps
Cargo, not rustc, so it does not meet sccache there; both would change `tools/cargo.sh`.

What the seed leaves to compile is what main has not built: a dependency a `Cargo.lock` change
brought in, or a profile or flag main does not build with. Building main's checkout after each
landing that changes `Cargo.lock` would keep that small; that is not built.

## Costs of the choice

- `du` counts each clone in full: a seeded worktree reads 6.35 GB by `du` and holds 0.88 GB. `df`
  says what the disk holds.
- The clone is of main's build as it is. Dependencies main has not built, or built with other
  features, the worktree compiles itself, as it does today.
- A worktree's clone of a dependency comes to hold disk of its own when main rebuilds that
  dependency, as a lock bump does; until the sweep clears the worktree.
- A seed waits while main's build holds its lock, at most two minutes (`SF_SEED_WAIT`), and then
  the worktree builds from nothing.
- Only builds through `tools/cargo.sh` seed (every `just` recipe and the launcher); a bare `cargo`
  in a new worktree builds from nothing.
- A worktree tied to no task (a `just verify` checkout under `../symphony-wt/`) is not cleared by
  the sweep's build-output rule; the worktree sweep's own rules still remove it when it goes.

## Verified and inferred

Verified by measurement: the times, the private bytes and the freed bytes in the table; the
shared folder's wrong build (its test binary ran the other worktree's edit); the five seeded
worktrees' private bytes; sccache's hits, none of 561 Rust compiles across three build folders. Verified by tests: the seed (`crates/tasks/tests/launcher/seed.rs`), the
sweep's rule (`crates/tasks/tests/sweep/build_output.rs`) and the warning
(`crates/tasks/src/board_health/disk.rs`, `crates/tasks/src/cli/status.rs`,
`apps/symphony-server/src/live/health_tests.rs`). Inferred: five worktrees built from nothing
(from two), and that a seeded worktree's own bytes grow toward a fresh one's as main rebuilds what
it cloned.
