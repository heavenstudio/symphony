# Engineering

The contracts a project or a provider builds against, and how the Symphony's own code is built. The other areas are in the [index](../README.md).

- [Writing a plugin and a theme](PLUGIN_WRITING.md) — a first plugin from nothing: its crate, its
  code and test, its settings, its component in a document and how to see it drawn; a theme's
  tokens by name, in the project file alone and from a plugin.
- [The board's JSON APIs](BOARD_API.md) — `tasks api`, the board's methods as a versioned JSON
  contract for a program, and the agents API with what a callback and the PULL digest push.
- [Plugin protocol](PLUGIN_PROTOCOL.md) — a plugin answering from a process of its own: the
  setting, the seams it answers, its process, its commands, a register read in one request, the
  line, each call's minor, the versions and the wire's shape.
- [Plugin calls](PLUGIN_CALLS.md) — each call into a project's plugins timed, and sent through a
  JSON round trip inside the process: the two settings, the record and how to read it.
- [Commands for a project's tools](TOOL_COMMANDS.md) — draw one card, check a branch for a
  landing, read status and triage, and share the document write lock.
- [Working artifact contracts](ARTIFACTS.md) — optional verified publication and scoped private
  storage; the local/fake-provider core and the integrations still required for hosted use.
- [Execution adapter](EXECUTION_ADAPTER.md) — bounded provider operations, scoped identities,
  trusted actor context, capabilities and the independent execution wire contract.
- [Execution attempts](EXECUTION_ATTEMPTS.md) — the board's durable record of each provider
  attempt: admission, fenced recovery, controls, evidence eligibility and host grants; a task's
  page reads its attempts, and no setting reaches them yet.

## Analysis

An analysis holds a proposal and its evidence: an approved design can still await implementation, and its approval alone does not make it current behavior.

- [The client cache](analysis/client-cache.md) — a page drawn at once from what the browser last
  held, then asking only for what changed.
- [Dioxus or JSX for the client](analysis/client-framework-comparison.md) — the web client kept in
  Dioxus or moved to React or Preact, compared on the same pages.
- [The built client out of git](analysis/client-off-git.md) — what it would take for git to hold
  no built web client.
- [One runtime for the client's modules](analysis/client-shared-runtime.md) — whether the client's
  page modules can share one copy of the code they all carry.
- [Code and architecture review, November 2026](analysis/code-review-2026-11.md) — the workspace's
  structure, risks and smells as ranked findings.
- [Symphony behind a versioned contract](analysis/consumer-contract.md) — whether Symphony's
  packages can be released and versioned one by one.
- [GraphQL, its library and the server's framework](analysis/graphql-and-server-framework.md) —
  what the client's data layer and the server are built on.
- [Delivering Symphony without rebuilds](analysis/plugin-delivery.md) — a project's plugin as a
  program of its own, and the split of crates, binaries and client.
- [Durable project artifacts](analysis/remote-artifact-store.md) — a proposed optional storage
  boundary for working evidence, Git revisions and recovery on a fresh application disk.
- [Execution providers and durable attempts](analysis/remote-execution-attempts.md) — a proposal
  for project-owned execution adapters, durable recovery and authorized attempt history.
- [Relax against cargo](analysis/relax-build-trial.md) — Makepad's live Rust compiler tried on
  this workspace and measured against cargo.
- [Worktree build folders](analysis/worktree-build-folders.md) — how agents' worktrees share
  compiled dependencies, with the measurements.
- [Naming inventory and superseded plan](analysis/own-names.md) — historical naming evidence, with
  pointers to the current naming rules and state paths.
- [Publication code boundary review](analysis/public-code-boundaries.md) — a bounded review of
  server request, file and process boundaries.
