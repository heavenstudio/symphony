# Relax against cargo

Whether Relax, Makepad's live Rust compiler, could replace or sit beside cargo in Symphony's build
and test loop, tried on this workspace and measured against cargo, as the relax-not-adopted card
in [DECISIONS](../DECISIONS.md) records. "Measured 2026-11-11" marks this machine that day: an
Apple M3 Max, 16 cores, 64 GB, macOS, shared with a dozen other agents' builds.

The ask: evaluate whether adopting Relax (https://makepad.github.io/relax/#architecture) could
help Symphony's build process, by an empirical test, and adopt it if its numbers are better, with
no need to ask the owner to decide once there is proof it is better.

## The result

Nothing is adopted; cargo stays the build, the test runner and the gate of record.

- **It runs little of our code.** Of the six cases tried, three ran with cargo's results: the
  `symphony-args` and `symphony-api` tests and `tasks list` on a scratch board. The
  `chrome-devtools` tests, `symphony docs-audit --check` and the server did not start, each on a
  gap in Relax's own standard library; with those gaps filled, `docs-audit` stops on the next, its
  missing AArch64 NEON intrinsics. Two more targets tried for a CPU-heavy case
  disagree with cargo: the board's unit tests do not compile, and 7 of the 32 `symphony-schema`
  tests fail under Relax and pass under cargo.
- **After an edit, where it runs, it is faster.** It reaches the test result sooner (0.8 s against
  3.1 s on `symphony-args`, 9.7 s against 18.2 s on `symphony-api`) and the board command sooner
  (12 s against 75 s, with half cargo's CPU). On `symphony-api` it spends twice cargo's CPU to do
  it, on a machine whose limit is CPU (load 150 to 275 during these runs).
- **With nothing changed it is slower.** A board command costs 10 to 13 s and 12 CPU-s each time
  under Relax, against 2 to 6 s for the launcher's no-op cargo build and the built binary; a no-op
  `symphony-api` test run costs 5.6 s against 3.0 s. The common case of the board launcher, every
  agent's every `just tasks`, is a no-op.
- **Short test processes run on its slowest tier.** The `symphony-api` wire tests take 3.7 to 5.0 s
  under a warm Relax against 0.03 to 0.10 s in cargo's unoptimised test build.

- [r] **relax-not-adopted** — Could Relax, Makepad's live Rust compiler, make Symphony's builds faster? Tried on this workspace against cargo: not yet, so nothing is adopted and cargo stays the build and the gate. Of the six cases tried, three ran and matched cargo. `docs-audit`, the server and the `chrome-devtools` tests stop on gaps in its standard library and its ARM vector instructions, and 7 of the 32 `symphony-schema` tests fail under it that pass under cargo. Where it runs, an edit reaches its result sooner; with nothing changed it is slower, and that is every `just tasks` command. Approve to close, or comment. [[analysis/relax-build-trial#The result]]
  [2026-11-11 20:12] agent — Posted for the board task `tools/relax-build-trial`. Medians of three runs each, at load 150 to 275 on 16 cores:

    | | cargo | Relax |
    |---|---|---|
    | `symphony-args` tests, after an edit | 3.1 s | 0.8 s |
    | `symphony-api` tests, clean checkout | 37 s, 22 CPU-s | 28 s, 39 CPU-s |
    | `symphony-api` tests, after an edit | 18 s, 9 CPU-s | 10 s, 20 CPU-s |
    | `symphony-api` tests, nothing changed | 3.0 s | 5.6 s |
    | `tasks list`, after an edit in `crates/tasks` | 75 s, 27 CPU-s | 12 s, 14 CPU-s |
    | `tasks list`, nothing changed | 5 s, 0.3 CPU-s | 11 s, 12 CPU-s |
    | 18 short `symphony-api` tests, run alone | 0.03 to 0.10 s | 3.7 to 5.0 s |
    | peak memory, `tasks list` | 720 MB | 1.05 GB |
    | disk, per worktree | 4.7 GB | 2.5 GB, and 1.2 GB once per machine |

    **Adopted:** nothing: no recipe, pin or doc change. The trial's Relax build stayed in a scratch folder.

    **Why not.** The crates agents edit most (`symphony`, the board's library, the server) do not run under it. Its false failures would send agents after bugs that are not theirs. The board launcher's usual case, nothing changed, gets slower and takes 1 GB per command. And on `symphony-api` it spends twice cargo's CPU on a machine whose limit is CPU. Short test processes never leave its slowest tier, which is where LLVM wins for us.

    **What would change it:** a published `relax` binary to pin, the missing standard library pieces and ARM vector instructions, and test runs that do not start on its slowest tier. [[analysis/relax-build-trial#What would have to change]]
  [2026-11-11 20:20] agent · auto-approved — not adopted: the owner asked for "empirical test, if better numbers should be adopted" and said "if there's proof this is better no need to ask me to decide" (2026-11-11); the measurements leave one option standing, since three of six cases do not start under Relax and 7 of 32 `symphony-schema` tests fail under it that pass under cargo. Cargo stays the build and the gate. Any verdict here overrides this.
  [2026-11-12 08:51] owner · approved
  [2026-11-12 10:54] agent · resolved

## What Relax is

Read in the repository [github.com/makepad/relax](https://github.com/makepad/relax) at
`544158264603dbf90994ac47fc97a2636437b656` (2026-11-11), not the page.

- **What it offers.** A Rust library (`relax_compiler` and the layers below it), MIT, 438k lines, no
  crates.io dependencies. The repository was published the day of this trial: 19 commits, the first
  at 20:13 +0100. Its runners (`tools/run`: the `relax` dev and test runner and the `makepad`
  binary) are not in it, though its own `CONTRIBUTING.md` and `KNOWN_GAPS.md` use them, so there is
  no binary to install or pin: a host has to embed the library ([below](#the-host)). Nothing is
  published to crates.io (`publish = false`).
- **Platforms.** Its own AArch64 and x86-64 code generators, no LLVM and no linker; its tests are
  kept clean on Linux x86-64 and macOS arm64. It ran on this Apple Silicon Mac.
- **Rust it reads.** Every edition (our 2024 crates ran); its standard library is its own port of
  rustc 1.97's, compiled from source, and it answers build scripts as rustc 1.98.0. What that port
  lacks fails our code: `impl FromStr for PathBuf`, `std::os::unix::fs::chown` and several AArch64
  NEON intrinsics in `core::arch` were missing here.
- **Cargo it reads.** It resolves the crate graph from the manifests and `Cargo.lock` itself
  (resolver 2, workspace inheritance, path, git and registry packages); it never writes the lock.
- **Proc macros.** Run in-process on its JIT. By default a third-party macro crate may not use
  `unsafe` or std's fs, net, process, env, thread or time modules, which refuses `serde_derive`; the
  host has to choose the open policy that bare `relax` uses (`pmhost::set_policy(Policy::Open)`).
- **Build scripts.** Compiled and run by Relax itself, which answers their `rustc --version` and
  `--print` probes. A package that compiles C (a `cc` or `cmake` build-dependency: our `rusqlite`
  bundled SQLite, `tract-linalg`, `ring`, `psm`) is refused unless `--allow-c`, which runs the real
  cargo and rustc for those packages and links their archives into one shared library.
- **FFI.** C calls go through its C ABI planner; a symbol the loaded libraries lack fails at the
  call (`psm`'s stack probe, called by `pest_derive` under `tract`, did here).
- **Threads, processes, async.** std threads and child processes worked in the cases that ran. Its
  fixtures cover async and coroutines but no tokio; the server, our one tokio program, never got
  past its build scripts.
- **wasm.** It has its own web target for Makepad applications, not a `wasm32-unknown-unknown`
  compiler `dx` could use: the committed client stays on rustc.
- **Tests.** `relax test PACKAGE [--lib] [--test NAME] [-- FILTER]` runs what `cargo test -p`
  runs, each target in a process of its own, with libtest's output; doc tests are not run.
- **Errors it skips.** It compiles a function when first called, so code rustc would refuse (or
  that Relax cannot compile) only fails when reached: it prints the errors and runs the rest.

### The host

`relax_compiler::cli_main` is the command line driver of the unpublished runners. The trial's host
is a binary crate of its own, outside this repository, depending on the library by path at the pin
and built with this workspace's toolchain (`cargo +1.99.0 build --release`): 9 min 26 s, 1,195 CPU-s,
2.7 GB peak memory, a 55 MB binary and a 1.2 GB target folder, once per machine. Its `main`:

```rust
fn main() {
    // cargo's RUSTC_WRAPPER for `--allow-c` native builds, or a stubbed build script (native.rs)
    if let Some(code) = relax_compiler::native::wrapper() {
        std::process::exit(code);
    }
    // a build script's RUSTC is this executable (`--version`, `--print` probes)
    if relax_compiler::relax_as_rustc() {
        return;
    }
    if let Err(e) = relax_compiler::vfs::mount_from_env() {
        eprintln!("relax: {e}");
        std::process::exit(2);
    }
    // every proc-macro crate is trusted, as cargo trusts it (pmhost.rs, Policy::Open)
    relax_compiler::pmhost::set_policy(relax_compiler::pmhost::Policy::Open);
    let args: Vec<String> = std::env::args().collect();
    let sess = relax_compiler::session_from_env(&args);
    relax_compiler::cli_main(&sess);
}
```

Without the first two hooks `libc`'s build script fails ("Failed to get rustc version"); without
the policy every `serde` derive is refused. The library reads its standard library's sources from
the checkout at run time, so the checkout stays where it was built.

## Does it run our code

Each case ran under cargo first for the reference result, then under Relax with
`MAKEPAD_RUN_HOME` and `MAKEPAD_RUN_CACHE_DIR` in a scratch folder. The board runs used a private
in-memory SurrealDB (the pinned `surreal start --unauthenticated --bind 127.0.0.1:PORT memory`)
named by `TASKS_SERVER`, with `TASKS_DATABASE=relaxtrial` and five scratch tasks; the server run
used a temporary `SYMPHONY_MACHINE_DIR`. Nothing touched the live board or the shared server.

| Case | Command under Relax | Ran | Same as cargo |
|---|---|---|---|
| `symphony-args` tests | `relax test symphony-args` | yes | yes: 5 pass |
| `symphony-api` tests | `relax test symphony-api` | yes | yes: 13 and 18 pass; 20 unresolved NEON intrinsics in `zmij` (serde_json's float printer) reported and not reached |
| `chrome-devtools` tests | `relax test chrome-devtools` | no | the target does not compile: Relax's std has no `std::os::unix::fs::chown` (`tools/test-support/scratch_tests.rs`) |
| `symphony docs-audit --check` | `relax run --allow-c --no-live symphony --bin symphony -- docs-audit --check` | no | `comrak`'s build script does not compile: Relax's std has no `impl FromStr for PathBuf` |
| `tasks list`, scratch board | `relax run --allow-c --no-live symphony-board --bin tasks -- list` | yes | yes: the same five rows; `pest_derive` (under `tract`) panics on `psm`'s missing C symbol and a `time_macros` function fails to compile, neither reached by `list` |
| server, `--port 0` | `relax run --allow-c --no-live symphony-server --bin symphony-server -- --port 0` | no | the same `comrak` build script |

Tried as well, for a heavier test target:

| Case | Ran | Same as cargo |
|---|---|---|
| `symphony-docs` tests | yes | yes: 54 and 34 pass |
| `symphony-board --lib` (483 tests under cargo) | no | does not compile: Relax does not resolve `use super::super as api;` (`crates/tasks/src/cli/api/assessment/context_tests.rs`) |
| `symphony-board --test cli` | no | does not compile: the same missing `chown` as `chrome-devtools` |
| `symphony-schema` tests | yes | **no**: 7 of 32 fail under Relax and pass under cargo; Relax reports `num_rational::BigRational` and two `bytecount` NEON intrinsics unresolved, and prints no panic message for the failures |

### With the two std gaps filled

To see whether the `symphony` binary was one std gap away from running, a copy of Relax's
standard library was given the two missing items (`impl FromStr for PathBuf`, as real std has it,
and `std::os::unix::fs::chown` over libc's `chown`) and named by `MAKEPAD_RUN_STD` for one
`docs-audit --check` run. The build scripts then passed, and the command failed at its start:
`comrak`'s `memchr-n` reaches `fearless_simd`'s NEON kernels, which do not compile
(`vreinterpretq_u32_s8` and others missing from Relax's `core::arch`), and the process exits 101
after 159 s and 158 CPU-s. The same run reports more NEON intrinsics missing in `simdutf8` and
`zlib-rs`, both under `gix`. The std gaps are the first of several: the next is AArch64
`core::arch`, which our markdown, git and JSON dependencies all use.

## Measured

Measured 2026-11-11, 19:42 to 20:05, three runs of each, every run's one-minute load average
recorded before it: 148 to 276 on 16 cores, from other agents' builds this trial could not stop.
Wall and CPU (user + system, the process and the children it waited for) come from
`/usr/bin/time -l`, peak memory is its maximum resident set. The figure given is the median and the
range is all three runs.

Cargo is the project's own command: `tools/cargo.sh test --locked -p PACKAGE` (what `just test-rs`
runs, two build jobs, `TASKS_RECORDER=off` so no recorder is waited on) and `tools/rs-bin.sh tasks
list` (what `just tasks` runs: a cargo build of the binary, then the binary). Relax uses every core
it is given.

- **Clean checkout to test result.** An empty cargo target folder against an empty Relax run
  folder, sources and registry already on disk for both. The Relax binary itself is a one-time
  build per machine ([the host](#the-host)).
- **One-line edit to result.** One line rewritten with a number no earlier run used (`0 * N` folds
  away, so the behaviour is the same and neither tool can answer from a cache of an earlier state):
  `looks_like_option` in `crates/symphony-args/src/lib.rs`, `is_document_route` in
  `crates/symphony-api/src/project.rs`, and `list::run`'s role column in
  `crates/tasks/src/cli/commands/list.rs`, each a function the measured command calls. Both tools
  ran the same tree in turn.
- **No-op.** The same command again with nothing changed: the control, which shows the edit
  measure moves (cargo's `symphony-api` goes from 3.0 s to 18.2 s on an edit, Relax's from 5.6 s
  to 9.7 s).

| Case | cargo | Relax |
|---|---|---|
| `symphony-args` tests, clean | 5.6 s (2.1 to 6.6), 1.2 CPU-s, 125 MB | 2.2 s (1.9 to 2.7), 3.5 CPU-s, 255 MB |
| `symphony-args` tests, after an edit | 3.1 s (2.9 to 3.1), 1.0 CPU-s | 0.8 s (0.7 to 1.1), 0.6 CPU-s, 160 MB |
| `symphony-args` tests, no-op | 0.9 s (0.8 to 3.3), 0.3 CPU-s | 0.4 s (0.4 to 3.8), 0.4 CPU-s |
| `symphony-api` tests, clean | 37.4 s (37.2 to 61.1), 22.0 CPU-s, 300 MB | 28.4 s (22.7 to 30.1), 39.3 CPU-s, 600 MB |
| `symphony-api` tests, after an edit | 18.2 s (13.9 to 21.8), 9.4 CPU-s, 310 MB | 9.7 s (9.4 to 10.9), 20.2 CPU-s, 365 MB |
| `symphony-api` tests, no-op | 3.0 s (2.3 to 3.6), 0.8 CPU-s, 136 MB | 5.6 s (5.1 to 7.4), 11.4 CPU-s, 290 MB |
| `tasks list`, after an edit in `crates/tasks` | 74.8 s (66.6 to 78.9), 26.8 CPU-s, 720 MB | 12.1 s (10.8 to 13.8), 14.3 CPU-s, 1.05 GB |
| `tasks list`, no-op | 5.2 s (1.8 to 6.2), 0.3 CPU-s, 112 MB | 11.4 s (10.4 to 13.4), 11.9 CPU-s, 965 MB |

The launcher ends by `exec`ing the built binary, which hides its cargo build from `/usr/bin/time`,
so cargo's CPU and memory in the two board rows come from timing that build alone
(`tools/cargo.sh build --locked -p symphony-board --bin tasks`): four runs after the same kind of
edit, 65.9 to 92.5 s and 25.2 to 27.3 CPU-s, and three with nothing changed, 1.0 to 5.0 s and 0.3
to 0.4 CPU-s, at load 156 to 229. The binary's own run is 0.03 s.

### Steady state

No CPU-heavy test target of ours runs under Relax: the board's `perf` and unit targets, its
embedding search and the client's browser tests are all among what does not compile or start. So
the steady state is the test run itself, warm, with nothing to build: libtest's own "finished in"
time.

| Target | cargo (unoptimised test build) | Relax, warm |
|---|---|---|
| `symphony-docs` `check`, 54 tests | 0.20 to 0.25 s | 0.23 to 0.30 s |
| `symphony-api` `wire`, 18 tests | 0.03 to 0.10 s | 3.7 to 5.0 s |

That is where LLVM wins for us: not in long computation, which none of our runnable targets does,
but in a short test process running deep generic code. Relax's own report of a warm `wire` run
(`MAKEPAD_RUN_VERBOSE=1`) compiles 25 functions in 253 ms and finds 4,089 of its 4,639 cached code
entries at its base tier, against 398 at O1, 114 at O2 and none at O3: a test process ends before
serde's code gets hot enough to be optimised, and every run starts on the slow tier again. The
README's "1.14 to 1.26 times `--release`" is for code that has settled at O3 in a long-running
process.

### Disk

| What | cargo | Relax |
|---|---|---|
| `symphony-args`, clean run | 3.3 MB target folder | 61 MB run folder |
| `symphony-api`, clean run | 74 MB | 99 MB |
| the three cases that ran and `symphony-docs`, warm | 4.7 GB `.target` (it also holds the three binaries and seven crates' test builds) | 2.5 GB run folder, of which 1.65 GB is the cargo build of the C packages `--allow-c` makes |
| per machine | the toolchain | 1.2 GB host target folder, 55 MB source checkout |

Cargo's warm figure is this worktree's own `.target`, built in full before worktrees began as a
copy-on-write clone of the main checkout's build folder; with that clone a worktree takes disk only
for what its builds change.

## Where it would fit

- **The agents' edit-and-test loop.** Not today. The crates agents edit most (`symphony`, the
  board's library, the server, the client) do not run, and one that runs (`symphony-schema`) gives
  false failures, which an agent would chase as its own bugs. Where it runs, an edit reaches its
  result in half the time or less, at twice the CPU on `symphony-api`, and a no-op run is slower.
- **The board launcher (`tools/rs-bin.sh`).** No. Its common case is the no-op, which Relax makes
  10 to 13 s and 12 CPU-s and 1 GB per command for every agent, against a cargo freshness check and
  an already built binary. Relax wins only the first command after `crates/tasks` changes, and a
  board command run from a branch against the live board already runs main's build. The board's
  search and embedding reach the code Relax cannot compile.
- **The server's foreground preview.** Does not start.
- **Release binaries, the committed wasm client and the nightly verify** stay on rustc: Relax makes
  neither a release binary nor a `wasm32-unknown-unknown` build.

## What would have to change

For a second trial to be worth running:

1. A published runner: a release of the `relax` binary or the `tools/run` crate, so a pin is a
   version and not a host this project keeps.
2. Relax's std and front end closing the gaps our code hit: `impl FromStr for PathBuf`,
   `std::os::unix::fs::chown`, `use super::super as NAME`, the `num_rational::BigRational`
   re-export, `psm`'s C stack probe, and above all the AArch64 NEON intrinsics in `core::arch` that
   `fearless_simd`, `simdutf8`, `zlib-rs`, `zmij` and `bytecount` use: filling the two std gaps
   alone moved `docs-audit` only as far as `fearless_simd`
   ([above](#with-the-two-std-gaps-filled)).
3. A test run that does not start every process on the base tier, such as a warm session per test
   target that keeps settled code across runs (the README's devhub keeps "a live session per
   lane").
4. The no-op case costing less than cargo's freshness check before it could serve the board
   launcher.

To rerun: build [the host](#the-host) at the pin; run each case in the first table under cargo and
then under Relax, with `MAKEPAD_RUN_HOME` and `MAKEPAD_RUN_CACHE_DIR` in a scratch folder; and for
each row of the [Measured](#measured) table, three times in turn per tool, record `uptime`'s
one-minute load and run the command under `/usr/bin/time -l`, emptying the target or run folder
first for a clean row and rewriting the named line with a fresh `0 * N` before an edit row.
