# Hosted PostgreSQL source inspection

Read-only inspection for `tools/hosted-postgresql`, 2026-11-17. This is a source/link record,
not a runtime compatibility, cloud connection, load or recovery test.

| Source | Finding |
| --- | --- |
| Available tag `release-2026-11-16`, commit `672d99abdad8de30a894a069dc7738d8894edd87`, `crates/tasks/src/schema.rs` | Board schema 18; released board path is SurrealDB. |
| Main `3b460adb62827f674669e8e99f3090561107264f`, `crates/tasks/src/schema.rs` | Board schema 20; board schema and PostgreSQL major are independent numbers. |
| [pg.rs](../../crates/tasks/src/pg.rs), Client/Connection | UnixStream only; trusted `postgres` startup; each client retains one connection. Mid-request failure drops it without replay. |
| [pg/wire.rs](../../crates/tasks/src/pg/wire.rs), parse/bind/sync | Unnamed statement/portal, JSONB parameter OID 3802, text results, a single Sync for all request steps. |
| [CLI reaching](../../crates/tasks/src/cli/mod.rs), Reached | `postgresql:/absolute/socket` accepted; `postgresql://host/database` explicitly rejected in regression cases. No hosted secret reader. |
| [Local PostgreSQL](../../crates/tasks/src/server/postgres.rs), version/ensure/make | Scratch-only startup; minimum major 18; builtin C.UTF-8 initialization; no TCP. |
| [Database dispatch](../../crates/tasks/src/store/database.rs), define/another | Public exhaustive enum; clones create separate clients; PostgreSQL define calls make_database against maintenance `postgres`. |
| [Store opening](../../crates/tasks/src/store/opening.rs), opening_on/migrate | Opens may migrate; the remote design needs explicit provisioning/migration authority rather than bypassing local live guards. |
| [PostgreSQL opening](../../crates/tasks/src/store/postgres/opening.rs), migration | Explicit READ COMMITTED transaction with pg_advisory_xact_lock, predecessor recheck and version upsert. |
| [Request wrapper](../../crates/tasks/src/store/postgres/mod.rs), VARS/SET/ANSWER/GUARD | Transaction-local board.vars/answer; DO blocks. Metadata version read lacks FOR SHARE in this inspected tip. |
| [Generated schema](../../crates/tasks/src/store/postgres/schema.rs), FUNCTIONS/triggers | Persistent invoker SQL/PLpgSQL functions and triggers; board.bumped is transaction-local; marker uses pg_backend_pid. No CREATE EXTENSION requirement seen. |
| [Live guard](../../crates/tasks/src/live.rs) and [build digest](../../crates/tasks/build.rs) | Local repository authority must be extended by lifecycle before PostgreSQL cutover; it is not a hosted release authority. |

The store source scan found no `pg_temp` definition. This does not prove temporary/session
features absent in a later implementation. The protocol adapter and pool must be checked again
against the landed tip used for implementation, including all result/cleanup paths.

Board coordination read `tools/board-pg-lifecycle` and `docs/board-pg-cutover-card`. Lifecycle's
executor owns scratch snapshot/hold/recovery work and the metadata-lock repair; search/copy's
executor retains its integration/schema work. A task marked done was not assumed landed:
the source pin above, rather than another worktree's files, defines these findings. The lifecycle
measurements, physical snapshots and cutover approval were not available evidence for hosted
operation. `tools/postgresql-setup-portable` retains installed executable discovery and the local
18 minimum. No runtime source is edited for this proposal.

The reporting brief supplied an example pool maximum 3, platform role cap 12 and a managed
PostgreSQL 16 target. These are planning inputs, not a fresh deployment inspection by this task.
No consumer credentials, database, cloud resource or consumer checkout was changed. No hosted
team URL was verified.

Primary documentation checked for the proposal: PostgreSQL 16 SSL, role limits, functions,
anonymous blocks, grants, protocol, isolation, JSON, triggers, text search, initdb and pg_dump;
PostgreSQL 18 libpq connection/execution and initdb; PgBouncer's feature matrix. The
[analysis](../operations/analysis/hosted-postgresql.md) links the precise pages next to the claims they
support. Documented SQL availability is not an integration pass on either major.
