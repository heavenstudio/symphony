# Managed PostgreSQL for a hosted board

A hosted board needs a database that survives its application process. The proposal is an
explicit external connection to a pre-created, dedicated PostgreSQL database, beside the local
socket path. The application never starts a replacement database when that connection fails.
The [database boundary](#hosted-postgresql-database-boundary) and the independent
[server-version choice](#hosted-postgresql-versions) are decided: a dedicated
database for each project, and external PostgreSQL 16 and 18. The connection adapter is built
behind the board's `hosted-postgresql` feature ([what a hosted board is](../POSTGRESQL.md#a-hosted-board),
[the adapter as built](#the-adapter-as-built)); nothing here gives permission to move data.

## Delivery boundary

The available `release-2026-11-16` uses SurrealDB and board schema 18. The inspected main at
`3b460adb6` has board schema 20 and a socket PostgreSQL client/store, with PostgreSQL startup
limited to scratch boards. Neither is a released hosted PostgreSQL capability. PostgreSQL's
server major version and the board's schema number are separate contracts. The exact source
pins and findings are in the [source inspection](../history/hosted-postgresql-source-audit.md).

This is a generic Symphony proposal. A consuming application's deployment design does not
approve it. A future release must name its supported server majors, driver/TLS dependencies,
board schema, old-reader behavior and tested deployment budget. Consumers take that release
through their own integration and landing. A pin to main is not evidence of hosted support.

Local startup retains `[board] postgresql`, the machine-installed programs, the PostgreSQL 18
minimum and the socket behavior chosen in the
[installation card](postgresql-binary-source.md#postgresql-binary-source). Remote mode skips local program
discovery, `initdb`, server supervision and database creation altogether. It does not weaken
their checks. Local activation, snapshots and version fixes belong to `tools/board-pg-lifecycle`;
live cutover and its permission belong to `docs/board-pg-cutover-card`.

## Configuration and secrets

The keys are read today ([PROJECT_FILE](../PROJECT_FILE.md#boardexternal_postgresql)). A
process's pool is its class's in the budget, so the table has no `pool_max` of its own:

```toml
[board.external_postgresql]
url_env = "SYMPHONY_DATABASE_URL"
ca_file_env = "SYMPHONY_DATABASE_CA_FILE"
class_env = "SYMPHONY_DATABASE_CLASS"
connect_timeout_seconds = 10
acquire_timeout_seconds = 10
request_timeout_seconds = 300

[board.external_postgresql.budget]
role_connection_cap = 12
maintenance = 1
headroom = 2

[board.external_postgresql.budget.classes]
web = { processes = 2, pool_max = 3 }
worker = { processes = 1, pool_max = 3 }
```

The platform injects the URL as an environment secret, or an alternative `url_file_env` names
an environment variable containing a mounted secret-file path. Exactly one URL source is
allowed. The project file contains only secret references. A consuming adapter may map its
existing environment variable into this configuration; Symphony does not silently read a
generic `DATABASE_URL` or an ambient libpq service/password file. CA and optional client
certificate/key files are deployment inputs, never committed credentials. A missing source,
invalid URL, conflicting explicit backend/address override or unsupported key fails startup
before a connection. External selection is authoritative even when the database is unavailable.

Accept one PostgreSQL URI with a host, explicit database and login role; accept IPv6 and escaped
credentials through the chosen library's parser. Require UTF-8. Refuse an empty database, a
maintenance database, multiple hosts, arbitrary startup `options`, and options that select an
unverified TLS mode. No database name is derived from a checkout path. Bound parsing and file
reads; report the configuration key and failure class without its value. Store address methods,
board reach commands, health logs, pool labels, panic/debug output and recorder arguments use a
nonsecret project/connection identifier. They never return the URL, even with its password
removed. Suppress raw driver/server diagnostics that may contain SQL, parameters or credentials;
retain SQLSTATE, a static explanation and a correlation id. Redaction tests cover parse errors,
authentication errors, URL percent escapes, notices and nested error chains.

Read the secret when creating a new connection generation. Rotation replaces the pool generation
and drains old connections; it does not mutate a connection under an active transaction. A
revoked credential fails closed. A restart reloads the configuration and reaches the same
database; losing the container filesystem creates no empty board.

## Transport and authentication

Use direct TCP with mandatory certificate-chain and endpoint-name verification, equivalent to
libpq `verify-full`; pin the deployment's CA source. An IP endpoint needs a matching certificate
identity. `require`, opportunistic encryption and accepting an unknown certificate are
insufficient. PostgreSQL documents the distinction in
[SSL support](https://www.postgresql.org/docs/16/libpq-ssl.html). Network reachability, DNS and
firewall policy remain the deployer's work; this task creates no tunnel or proxy.

The first proposal was maintained libpq through a Rust binding; both were then measured, and
the adapter is built on the Rust driver instead ([the driver](#the-driver)), rather than
extending the socket client's authentication state machine. Initially support SCRAM password login over verified TLS and an optional client
certificate when the endpoint requires one. Refuse unsupported authentication, including an
unimplemented token-refresh mechanism, without downgrading. IAM/connector authentication would
need its own tested adapter. Use explicit connection parameters and disable ambient connection
defaults; libpq's [connection control](https://www.postgresql.org/docs/18/libpq-connect.html)
describes both parameter handling and the need for a trusted `search_path`.

The local socket build keeps its dependency path: the driver is optional, and no default build
or installed release compiles it.

### The driver

Two were built into the same small program and measured on this machine (aarch64 macOS,
release profile, 2026-11-18), over a baseline of 342,816 bytes:

| Driver | Binary | Build | What the image must carry |
| --- | --- | --- | --- |
| libpq through a thin binding | +53,088 bytes | 20.5 CPU-seconds | libpq and its TLS library, dynamically linked, at a version the image pins |
| `tokio-postgres` with `rustls` (`ring`) | +1,893,824 bytes | 113.7 CPU-seconds, 95 crates | nothing: linked statically |

The static driver was chosen, behind the off-by-default feature `hosted-postgresql`: a hosted
image then carries one binary and no native library whose version, TLS build and search path
are a second thing to pin and patch, and its certificate verification is the build's own and
not the image's OpenSSL configuration. Its cost is the larger binary and build, which only a
build that names the feature pays: with the feature off the dependency tree of the board and
of the workspace is what it was (36 and 312 crates), and the shipped binaries' size budgets
are unchanged. What every build does carry is the settings table's reading and judging, so a
project file is checked the same anywhere: against the commit this was built on, `tasks` and
`agent-send` are 16 bytes larger, `symphony` 33,088 and `symphony-server` 49,600, each within
its budget. The libpq binding stays the alternative where a platform already ships and
patches libpq and a hosted binary's size matters more than a second moving part. The lockfile
gained 51 packages for it, the platform-conditional ones among them (`tokio-postgres` 0.7.18,
`tokio-postgres-rustls` 0.14.0, `rustls` 0.23.46 with `ring` 0.17.14 and `rustls-webpki`
0.103.17, `postgres-protocol` and `postgres-types`, and what they need); no package it held
moved. `tokio-postgres-rustls` 0.13 read no channel binding from a server certificate, so a
login that requires it was refused; 0.14 does, and the matrix logs in with it required.

## Transactions before pooling

The source inspection found unnamed Parse/Bind/Execute messages ending in one Sync, which
commits the complete implicit transaction. Splitting those statements into individual autocommit
calls loses both atomicity and `set_config(..., true)` variables. The hosted adapter instead
owns one connection for an explicit REPEATABLE READ transaction around all request steps,
binds the JSON parameter as JSONB, collects text/null rows and command tags, and commits before
returning success. Simple-query migration scripts already contain BEGIN/COMMIT and retain
their explicit READ COMMITTED boundary; they are not wrapped in another transaction.
The protocol's [message-flow contract](https://www.postgresql.org/docs/16/protocol-flow.html)
and libpq's [execution interface](https://www.postgresql.org/docs/18/libpq-exec.html) are the
references for this adapter, including complete result consumption.

The SQL uses transaction-local `board.vars`, `board.answer` and `board.bumped`; persistent
SQL/PLpgSQL functions and triggers; and a transaction-level migration advisory lock. No
`pg_temp` function definition was found in the inspected store. This is a source finding, not
proof about future SQL. The integration fixture must inventory temporary functions/tables,
prepared statements, session settings and locks again at the implementation tip. A session
that needs initialization is initialized on every new connection. There is no reliance on
another borrower's temporary object or setting.

Start with direct connections pooled inside the process, with exclusive ownership from BEGIN
through COMMIT/ROLLBACK and all results drained. No statement or transaction proxy is required.
PgBouncer explicitly distinguishes session support from the restricted
[transaction pooling feature set](https://www.pgbouncer.org/features); a future proxy must prove
these semantics before being accepted. A pool checkout cannot cross a process/fork, and nested
store calls may not acquire a second connection while retaining their first. The API takes its
borrowed transaction where needed. Return only a confirmed idle connection with the known
session baseline restored. Drop it on a protocol error, failed cleanup, cancellation uncertainty
or an unknown transaction state. Test isolation with different variables, failures and concurrent
borrowers before choosing any more aggressive pooling mode.

Remote execution follows its [commit-intent contract](remote-execution-attempts.md#commit-intent-then-perform-io).
Commit and release the borrowed connection before provider I/O. Atomic admission, keyed intent,
active-attempt pointers and durable execution reservations remain one database transaction;
database-clock leases, worker epochs and revision comparisons fence later writes. Runtime
execution epochs and project/board references retain their separate scopes. Authorized cancel
intent does not require the worker lease. The hosted adapter must preserve these guarantees;
this proposal does not claim the current connection code implements them.

## Capacity across revisions and workers

All clients of a project and credential generation within one process share one pool registry;
`another`, bounded lookups, callbacks, health reads and dispatch do not each make a new pool.
The registry verifies endpoint, database and role against the same connection identity. Minimum
connections is zero, maximum is explicit, and the waiting queue is bounded. A checkout waits on
release/cancellation in FIFO order with its own deadline. It returns a capacity error when the
queue is full or its deadline expires; it never opens a connection outside the pool. Connect attempts and
connections being replaced count toward the maximum until the old connection is closed.

A per-process maximum is not the platform budget. For each login role, deployment admission
must prove this conservative bound from enforced process/instance/job maxima:

```text
sum(maximum simultaneous processes of class i * pool maximum of class i)
  + other direct users of this role + maintenance allowance + reserved headroom
  <= platform role connection cap
```

Classes include old and new web revisions during a rollout, background workers, child command
processes, a migration job and any other application using the same login. Cloud instances with
several processes count each one. An example cap of 12 permits old web 1×3, new web 1×3, worker
1×3, maintenance 1 and headroom 2. This is an example topology, not a measured deployment or a
default scaling entitlement. Two old and two new web processes at three each already consume
12, leaving nothing for a worker or recovery.

The deployer must enforce these maxima across all simultaneously live revisions, including
draining instances and rollout/job overlap. A manifest value that merely states a bound is not
enforcement. Migration and export jobs share the maintenance allowance and cannot overlap if
it only has one connection. Shell/worker commands use that budget or the application's existing
service boundary; there is no unlimited direct CLI access. If an existing application pool cannot
share the registry, its full maximum is an additional term. An unbounded or unprovable topology
is unsupported and blocks hosted activation. Arbitrary autoscaling would need a separately
designed global admission service or shared session pooler, not one sidecar pooler per instance.

The platform retains the login's connection limit as containment; Symphony's application login
cannot raise it. PostgreSQL calls this limit approximate and exempts superusers
([role limits](https://www.postgresql.org/docs/16/sql-createrole.html)), so it is not the scheduler
or proof of the bound above. Exhaustion (`53300`) is unavailable/capacity, never permission to
retry forever or bypass the queue. Expose active, idle, connecting and waiting counts without
secret labels. Tests must hold the complete old/new/worker topology concurrently and check
actual server sessions as well as per-process counters.

## Database and privileges

The supported boundary is one pre-created database per Symphony project, with one project
login and objects owned by that login. The platform provisions it and its cap out of band.
The login has CONNECT, the schema USAGE/CREATE and object rights needed for its own tables,
indexes, functions, triggers and DML; SQL/PLpgSQL language USAGE is required. It is
NOSUPERUSER, NOCREATEDB, NOCREATEROLE, NOREPLICATION and NOBYPASSRLS, with no membership that
can assume a stronger or another project's role. Function creation requires ownership and
language privileges, not superuser privileges for these trusted languages
([functions](https://www.postgresql.org/docs/16/sql-createfunction.html),
[anonymous blocks](https://www.postgresql.org/docs/16/sql-do.html)).

Use the dedicated database's `public` schema with a fixed trusted search path, `public` alone,
which PostgreSQL searches after `pg_catalog`, so no schema named for the login comes first; revoke public object-creation rights from other logins. Deny other
project roles CONNECT to this database, revoke inherited PUBLIC CONNECT/TEMP where appropriate,
and remove cross-project grants/default privileges. Database ownership is intentionally broad
inside this one database. A shared application database with name prefixes or only a project-id
column is not supported by this design. It would need a full schema/RLS, ownership, migration,
backup and isolation design before use. If the platform supplies only one shared application
database and cannot provision this boundary, hosted activation waits on that platform work.

In external mode a missing database (`3D000`) is a provisioning error. It never calls
`Client::make_database`, connects to the maintenance database or issues CREATE DATABASE/ROLE.
An empty, correctly provisioned database is initialized only by the authorized migration job.
The existing local maker remains unchanged. This cleanly bypasses database-creation privilege
without changing local semantics. The implementation must assert the call sequence with a
restricted real role, not just a mock successful connection.

- [r] **hosted-postgresql-database-boundary** — Each hosted Symphony project needs a database boundary that keeps another project's data and migrations out. The platform may currently supply only the application's existing database, so a separate board database could require provisioning work. Which boundary should the first hosted adapter require? A: a pre-created database dedicated to each Symphony project, owned by its restricted project login; sharing another application's tables is unsupported. B: defer hosted activation until a separate design proves safe sharing of an application database through schemas and permissions. Recommended: A. Approve and say A or B in the note, or comment. [[analysis/hosted-postgresql#Database and privileges]]
  [2026-11-17 19:42] agent — Proposed by `tools/hosted-postgresql`. A reuses the store's database-wide namespace and migration lock, but needs the platform to provision a database per project and deny cross-project connections. The application has no CREATE DATABASE/ROLE privilege. B may fit existing provisioning but needs schema ownership, qualified SQL, migration locks, export/restore and isolation designed before activation; prefixes or a project-id column alone are insufficient. Additional provisioning cost and availability are unverified. Both preserve the requested managed connection and unchanged local socket behavior. Neither authorizes a live migration, cutover, restore or shared-server restart.

    **Earlier verdicts checked.** **postgresql-binary-source** (C) chooses machine-installed local PostgreSQL and the latest local Homebrew version; it does not decide managed database tenancy. **board-schema-at-update** (C) preserves recent-writer checks before migration, and **consumer-api-break-release** preserves old public calls for a release; both options keep them. Neither chooses a database boundary. The live cutover remains `docs/board-pg-cutover-card`'s separate decision.
  [2026-11-17 20:43] owner · approved — A
  [2026-11-17 20:48] agent — Your “A” requires a pre-created database dedicated to each hosted Symphony project, owned by its restricted project login. Sharing application tables is unsupported, and the application gains no database or role creation privileges. Implementation can now target this boundary and the approved external PostgreSQL 16/18 matrix; live provisioning, migration and cutover remain separate operations.
  [2026-11-17 20:48] agent · resolved

## Migration and stale writers

Reuse the landed schema/store/copy and lifecycle contracts. The lifecycle task owns the local
guard repair; the hosted task consumes its landed result and proves the same behavior remotely.
A migration job uses the selected release artifact and an explicit deployer-granted migration
mode. Ordinary hosted application opens do not auto-migrate an empty or older board. The
artifact/release identity replaces the local checkout's main-build authority; it is not an
implicit exemption for every network client. Keep local main-only and scratch safeguards.

Within the dedicated database, serialize migrations on the existing transaction advisory key
at READ COMMITTED. After taking the lock, read the actual version again, verify the expected
predecessor and recognized schema identity, apply only its named additive steps, and update
the version in the same transaction. Lock an existing metadata row FOR UPDATE before DDL,
so existing writers drain before schema changes begin. A concurrent loser reopens and observes
the winner.
A killed or failed migration leaves the predecessor intact. Same-version reopening performs
no schema DDL. A partial/unrecognized set of objects with no valid metadata is refused, not
adopted as an empty board. No extension installation, owner reassignment, role/database creation,
server setting changes or superuser-only operation belongs in application migrations.

Before opening data, verify `server_version_num` against the release's tested external majors,
UTF-8, the connected database/login and the recorded project identity. An unsupported engine
major refuses before DDL; the proposed matrix authorizes no untested major. The authorized
initial migration binds an empty database to its project and recovery generation. Later opens
cannot silently rebind that identity. Include project identity and generation in the same
transactional metadata guard as the schema, so a pool or credential mix-up fails before work.

Every write, including heartbeat/raw/copy/callback paths, checks the board schema under the
same transaction's row lock (`FOR SHARE` on the metadata row). The migration's version update
must conflict with that lock. An old snapshot taken before a completed migration cannot commit
a stale write afterwards: serialization failure causes a fresh decision and then version refusal.
The empty-board case is handled only by the migration lock, never by ordinary writers.
One-version-behind reads retain the existing read-only contract where proved compatible;
older/newer unknown versions refuse. A SQL/schema digest mismatch at the same version refuses.
The [isolation rules](https://www.postgresql.org/docs/16/transaction-iso.html) motivate the
old-snapshot regression; the lock alone must not be taken on trust.

The rollout must account for every writer, including workers, and honor the existing recent-writer
gate before a schema raise. Compatible releases can overlap at the same schema. A schema-changing
deploy drains/fences the old writers first; one-behind reading never permits old writes.
Duplicate authorized migration jobs are harmless under the lock. Their presence is still
counted in capacity. Unknown versions and migration failure keep the application unready.

## Failure and recovery behavior

Before any SQL is sent, a connect/auth/TLS failure reports unavailable or configuration refusal.
Connect/reconnect uses bounded exponential backoff with jitter, cancellation and an overall
deadline; no hot connection loop. A broken idle connection is replaced for a subsequent request.
During a request, connection loss, a read/write deadline or an uncertain COMMIT produces an
unknown-outcome error and discards the connection. It never automatically replays the request,
including after Sync or an explicit COMMIT whose acknowledgement was lost. Confirmed `40001`
or `40P01` rollback retains the store's bounded re-read/re-decide retry; a transport error never
masquerades as those codes. If COMMIT reports rollback, return failure rather than the previously
collected answer. Only confirmed commit returns mutation success.

An operation with a durable idempotency key may reconcile by reading that key on a fresh
connection; absence is not proof a still-running old transaction cannot commit. A later replay
requires uniqueness/fencing in the same mutation and belongs to that operation's API. Never
invent a key from the transient connection. During an outage writes fail visibly, reads report
unavailable, and readiness remains false until the schema and database identity are verified.
Shutdown stops admission, cancels waiting borrowers, bounds draining and closes owned connections;
it stops no managed database. Restart preserves committed rows and durable delivery state.

For a remote attempt, an unknown intent or lease commit prohibits provider I/O until a fresh
read confirms the stable key, revision and ownership epoch. An unknown result commit is
reconciled against its durable fact before another append or application. Connection recovery
does not itself confer worker ownership or permission to dispatch.

Provider backup/PITR is the durable disaster-recovery mechanism; the deployer owns its schedule,
retention, access and a measured recovery-point/recovery-time objective. No objective is claimed
before a restore drill. Add portable logical export from the landed copy code, with a consistent
snapshot and schema/release/project identity, row counts and a checksum manifest. Database-native
export uses a compatible `pg_dump` client and secret source, never a URL argument; a single
nonparallel dump fits the reserved maintenance connection. PostgreSQL documents dump version
constraints and consistent snapshots in [pg_dump](https://www.postgresql.org/docs/16/app-pgdump.html).

The recovery handoff contains the backup/export id and verified checksum, source engine/schema
and release, target identity, authorization to freeze writers and switch, credential owner,
expected data-loss interval and restore-validation results. The operator first fences all old
writers and retains the damaged source. Restore into a different pre-created database, using
no-owner/no-ACL logical restore or the platform's restore service; do not overwrite an active
board. The target owner creates its objects without superuser rights. Verify identity, schema,
exact logical rows/counters, nested nulls, search, delivery/callback deduplication and isolation.
Then the operator grants the deployment its new secret/endpoint and explicitly activates it.

A durable project control record outside the restored backup must retain the accepted schema
high-water mark and a monotonically increasing recovery generation. The deployer provisions
that record alongside the secret configuration. Restoring an older database cannot lower it.
Processes verify the configured generation against database metadata before reads/writes; a
recovery fences/revokes the old database access as well as draining revisions, since an isolated
old process cannot learn a new generation. The lifecycle task's local rollback guard is reused
where applicable, but its checkout file cannot serve as this record in an ephemeral container.
The future hosted implementation includes this small control-record interface and scratch
recovery tests. A provider restore alone is not a safe automatic rollback. The board's side of
that interface is its recovery authority
([TASKS](../TASKS.md#a-board-that-was-copied-or-restored)): a store is handed the generation
the control record names and refuses every bound write where the database holds another or the
record names none. The generation it names is the database's own text, replaced at each copy and
rebind, and a board home's file of it on one machine is not this record.

If a post-switch rollback is needed, freeze again and reconcile writes made since switching;
changing the endpoint to an older snapshot silently loses those writes and is refused. Local
stopped-cluster snapshots/hold/reclaim results remain useful local evidence. Physical cluster
copying, `pg_ctl` and filesystem locks are not a managed backup or distributed hold. No live
cutover, restore or shared-server restart is authorized by these steps.

## PostgreSQL 16 and 18 validation matrix

The proposed target is external PostgreSQL 16 and 18, while local startup keeps 18 or later.
The version card may instead require the platform to upgrade. SQL availability in documentation
is only a reason to try 16, not a supported-version claim. The cells were run on scratch
servers of both majors on one machine: [the matrix as run](../history/hosted-postgresql-matrix.md)
says which passed, by which test, and which rows have no proof yet.
Use disposable databases on both majors, the same release artifact, and the restricted owner
role described above. Record server/client versions, encoding, collation provider and privileges.

| Surface | External 16 | External 18 | Local 18 regression / privilege assertion |
| --- | --- | --- | --- |
| Cluster creation | Bypassed; 16 lacks the local builtin locale provider | Bypassed | Keep current `initdb` flags, socket trust and minimum-version refusal |
| Database/schema initialization | Pre-created UTF-8 database; approved migration only | Same | Remote role cannot create a database/role; local maker still can |
| Tables, indexes, constraints, upserts | Execute every generated definition and migration | Same | Own tables/indexes; no extension or superuser privilege |
| Functions, DO blocks and triggers | Prove invoker SQL/PLpgSQL and CREATE OR REPLACE TRIGGER | Same | No SECURITY DEFINER escalation; denied language/schema rights fail clearly |
| JSONB and paired store semantics | Null/missing, Unicode, arrays, counters, raw/read/copy parity | Same | Exact paired assertions, including nested nulls and rollback |
| Text search/order | Re-run landed search/copy vectors and query plans under provisioned locale | Same | Compare Latin/non-ASCII case, ordering and rank; no broadened expectation |
| Protocol/transaction state | Parameter typing, multistep atomicity, abort, commit status, settings, temp-object inventory | Same | Local wire behavior unchanged; borrower state cannot leak |
| Schema concurrency/fencing | Fresh and upgrade races, old snapshot, unknown/digest-mismatched schema | Same | Old writes fail; compatible old reader only; migration rollback atomic |
| TLS and login | Verified CA/name, SCRAM, wrong/expired cert, rejected password, rotation | Same | Bad/missing secrets and forbidden TLS options expose no secret |
| Topology/capacity | Old+new+worker+maintenance held together, queue exhaustion and cancellation | Same | No bypass pool, overflow or starvation; inspect actual backend count |
| Restart/drop/outage | Process restart, idle/mid-write drop, ambiguous commit, DB unavailable | Same | No automatic transport replay, no local fallback, bounded attempts |
| Remote execution integration | Atomic admission/intent/reservation, lease and revision fencing, uncertain commit reconciliation | Same | No provider I/O while a connection is held or intent/ownership is unknown |
| Isolation/recovery | Two projects/roles, cross-connect denial, export and older-backup restore drill | Same | No cross-read/write; generation/high-water guard survives container loss |

The source-backed SQL hypotheses are PostgreSQL 16's
[JSON operators](https://www.postgresql.org/docs/16/functions-json.html),
[trigger syntax](https://www.postgresql.org/docs/16/sql-createtrigger.html) and
[text search](https://www.postgresql.org/docs/16/textsearch.html). The local locale setup cannot
be copied onto 16: its [initdb providers](https://www.postgresql.org/docs/16/app-initdb.html)
differ from [18's](https://www.postgresql.org/docs/18/app-initdb.html). Provisioned 16 locale
behavior must meet the same semantic assertions or receive a bounded SQL fix; otherwise 16
stays unsupported and the platform upgrade alternative returns to its card. No relaxed local
minimum, test assertion or performance budget establishes compatibility.

- [r] **hosted-postgresql-versions** — The proposed hosted connection needs a declared PostgreSQL server target. The consuming deployment reports PostgreSQL 16; Symphony's local launcher requires 18 and uses a locale setup unavailable on 16. Which target should the hosted implementation prove? A: external 16 and 18, with the same SQL/privilege, search, failure and recovery assertions on both; local startup stays at 18 or later. B: external 18 only; the platform must separately approve and complete an upgrade before hosting this board. Recommended: A. Approve and say A or B in the note, or comment. [[analysis/hosted-postgresql#PostgreSQL 16 and 18 validation matrix]]
  [2026-11-17 19:42] agent — Proposed by `tools/hosted-postgresql`, independently of **hosted-postgresql-database-boundary**. A adds a supported-major validation lane and possible bounded SQL/locale work; B avoids that compatibility lane but requires platform work whose cost and timing are unverified. Neither major has been tested by this design task. Documented SQL availability is not a compatibility pass. A failure is fixed in the code or keeps that major unsupported; local requirements and assertions are not relaxed. No platform upgrade is authorized here.

    **Earlier verdicts checked.** **postgresql-binary-source**'s latest-local-version choice does not say which managed server majors Symphony supports. **symphony-update-command** (B) has consumers take release tags; a released SurrealDB/schema-18 build and inspected main's socket PostgreSQL/schema-20 build do not provide hosted support under either option. This choice authorizes a future validation target, not deployment acceptance.
  [2026-11-17 20:37] owner · approved — A
  [2026-11-17 20:59] agent — Your “A” sets external PostgreSQL 16 and 18 as implementation and validation targets, with the same SQL, privilege, search, failure and recovery assertions on both. Local startup remains PostgreSQL 18 or later. A failing compatibility case must be fixed or remain unsupported; this is not yet a runtime compatibility result.
  [2026-11-17 20:59] agent — Recovered the exact callback reply supplied by its writer after a concurrent save omitted it; original owner verdict retained.
  [2026-11-17 20:59] agent · resolved

## Implementation and release gates

The smallest implementation extends private store dispatch with a hosted connection adapter and
adds hosted opening/configuration APIs. Keep the existing public `pg::Client::new`, `socket`,
`of`, local address semantics and exhaustive `Database` enum working. A private dispatch enum
can hold the old public `Database` or the new hosted pool without adding a public enum variant
or making a socket getter invent a path. Preserve public struct construction and raw-store API
compile guards. Consumer-visible replacements ship beside the old calls, without deprecation;
removal needs a later release and demonstrated migration, as
[consumer API checks](../CHECKS.md#consumer-api-changes) require.

Implementation consumes the reviewed, landed search/copy and lifecycle sources; it does not
copy unlanded branches. Confirm file ownership before editing `pg`, store opening/database/
transaction, schema guards, project configuration or CLI reaching. Remote execution attempts
remain `tools/remote-execution-attempts`'s contract; database capacity is a separate resource.
Its execution reservations live in durable board rows, never pool/process counters, and remain
held for uncertain or cancelled resources until authoritative absence or quiescence is confirmed.
Configurable workflow remains `tools/configurable-delivery-workflow`'s contract. No frontend
or distribution input changes are proposed here.

Before release, require the focused paired store/connection regressions, supported-major matrix,
coverage analysis of new parsing/error/transaction/pool code, lint and independent review under
the normal [evidence gate](../CHECKS.md#evidence-by-kind-of-change). Add cases for uncovered
branches rather than changing expectations to fit failures. Show the optional dependency's
binary/image cost. The release run performs the repository's full verification. Separately,
the deployer proves network/certificate reachability, enforced overlap budget, backup restore
and project isolation on authorized scratch resources. No runtime test, target-version
integration, throughput figure, restore drill or cloud deployment is claimed by this design task.

## The adapter as built

Built, behind the feature, and proved on scratch PostgreSQL 16 and 18 servers
([the matrix as run](../history/hosted-postgresql-matrix.md)): the settings and their check;
the secret references, read at each connection, and refusals that say none of them; verified
TLS with channel-bound SCRAM; the server, login and database checks; one explicit transaction
a request with nothing sent twice and a lost commit said as unknown; the process's pool, its
line and its refusals; the budget's rule in the settings and its slots in the database; the
store's private dispatch beside the socket client, the public `Database` and `pg::Client`
unchanged; a board made and moved by the migration opening alone; and PostgreSQL 16's
analyzer.

Not built, each still this design's and none claimed by the adapter:

- a command's reach of a hosted board: the library opens one, and the board's commands and
  the server still reach the machine's own board;
- the project's identity bound into the board's record, and a hosted deployment's durable
  place for the recovery authority: the board's recovery generation and the writes bound to it
  ([TASKS](../TASKS.md#a-board-that-was-copied-or-restored)) are the board's own and a hosted
  board has them as any, but the authority that says which generation a board is bound to is
  handed to a store by its caller, and nothing here keeps one outside a container;
- a refusal of a schema whose definitions differ at the same version (a digest);
- reconnection with backoff: a request makes one connection attempt within its deadline and
  is refused, and the next request tries again;
- export, restore and their drill, which are the lifecycle's;
- the remote execution rows (admission, intent, leases), which no board has yet;
- anything on a platform's own PostgreSQL: no cloud database was made, reached or read.
