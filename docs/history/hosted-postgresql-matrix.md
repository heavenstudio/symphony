# The hosted PostgreSQL matrix as run

The [validation matrix](../operations/analysis/hosted-postgresql.md#postgresql-16-and-18-validation-matrix)
of the hosted adapter, run for `tools/hosted-postgresql-adapter` on 2026-11-18. A record of
that day's servers and numbers, not a reference: the tests it names are the proof, and they run
again with `just hosted-check`.

## What it ran on

One machine (aarch64 macOS 25.6), no cloud database: each test binary starts servers of its own
under the temporary folder, on a loopback port, and stops them at its exit.

| | External 16 | External 18 |
| --- | --- | --- |
| Server | PostgreSQL 16.16 (Homebrew, keg-only) | PostgreSQL 18.7 (Homebrew) |
| Cluster | UTF-8, the C library's `en_US.UTF-8` | UTF-8, builtin `C.UTF-8` |
| Transport | TLS 1.3, a certificate authority and server certificate made by the test (`openssl`) | the same |
| Login | SCRAM-SHA-256 with channel binding required; `pg_hba.conf` takes TLS and SCRAM alone | the same |
| Project | a login `NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS`, a UTF-8 database it owns, `CONNECT` revoked from `PUBLIC` | the same |
| Client | `tokio-postgres` 0.7.18, `tokio-postgres-rustls` 0.14.0, `rustls` 0.23.46 (`ring`) | the same |

The servers are not a platform's: no managed service's extensions, proxies, failover or limits
were met. Every cell below is the same test run against both majors, in
`crates/tasks/tests/hosted/`.

## The cells

| Surface | 16 | 18 | The test |
| --- | --- | --- | --- |
| Cluster creation | bypassed | bypassed | the client makes none; `refusals.rs` `a_login_or_database_a_board_is_not_kept_in_is_refused` (a database not there is refused, and none is made) |
| Database and schema initialization | pass | pass | `board.rs` `a_hosted_board_is_made_by_its_migration_alone_and_then_opens_to_every_writer`, `a_database_that_holds_anothers_objects_is_never_taken_for_an_empty_board` |
| Tables, indexes, constraints, upserts | pass | pass | every definition and migration of this build runs in the same tests; on 16 with the analyzer's own definition |
| Functions, DO blocks and triggers; privileges | pass | pass | `board.rs` `a_made_board_is_its_logins_own_with_no_definer_function_extension_or_other_schema` |
| JSONB and paired store semantics | pass | pass | `board.rs` `the_same_work_is_answered_alike_on_hosted_16_hosted_18_and_the_local_board`; `transactions.rs` `a_request_is_one_repeatable_read_transaction_with_variables_of_its_own` |
| Text search and order | pass | pass | `analyzer.rs` `every_code_points_terms_on_16_are_18s_alone_and_inside_a_word`, `a_boards_search_reads_the_same_words_on_16_and_18`; hits and scores in the paired test above |
| Protocol and transaction state | pass | pass | `transactions.rs`, all ten |
| Schema concurrency and fencing | pass | pass | `board.rs` `two_migrations_at_once_make_the_board_once`, `two_processes_migrations_at_once_make_the_board_once_and_neither_is_refused`, `a_migration_runs_as_the_maintenance_class_alone_and_each_class_has_its_own_pool`, `a_writer_behind_holds_the_migration_and_a_board_moved_on_refuses_it`; `transactions.rs` `a_write_that_met_anothers_since_its_snapshot_is_a_conflict_to_decide_again` |
| TLS and login | pass | pass | `refusals.rs`, all eight; `capacity.rs` `a_rotated_secret_is_read_again_and_the_connections_made_with_the_old_one_are_retired` |
| Topology and capacity | pass | pass | `capacity.rs` `a_pool_lends_its_share_and_refuses_the_line_past_its_length_and_its_wait`, `the_deployments_budget_is_counted_in_the_database_across_processes_and_by_class`, `the_servers_cap_for_the_login_is_a_refusal_of_capacity`, each counting the server's own sessions |
| Restart, drop and outage | pass | pass | `transactions.rs` `a_connection_lost_before_its_commit_committed_nothing_and_is_not_sent_again`, `an_idle_connection_the_server_ended_is_replaced_for_the_next_request`; `capacity.rs` `a_request_given_up_on_is_cancelled_uncommitted_and_its_connection_is_not_lent_again`, `a_commit_that_is_not_answered_is_unknown_and_is_never_sent_again`, `a_session_the_server_ends_at_the_commit_is_unknown_and_is_never_sent_again` |
| Isolation | pass | pass | `board.rs` `one_projects_login_reaches_no_other_projects_board` |
| Remote execution integration | no proof | no proof | below |
| Recovery | no proof | no proof | below |

## What has no proof

Reported, and neither skipped in a test nor relaxed:

- **Remote execution** (atomic admission, intent, reservations, leases, revision fencing): no
  board has those rows yet, so nothing of them ran on a hosted connection. What the adapter
  gives them is proved alone: a request is one transaction, its connection is returned before
  the caller does anything else, and nothing is sent twice.
- **Recovery**: export, an older backup's restore and its drill, the project's identity in the
  board's record, and a durable place for a hosted deployment's recovery authority. None is
  built here; export and restore are the lifecycle's. A hosted board does take the recovery
  generation a copy gives any board (the paired test reads it), and nothing more of recovery
  ran on one.
- **A commit that committed and whose answer was lost**: the two commit cells end a commit the
  server then rolls back. That the client says "not known" for both, sends nothing again and
  drops the connection is proved; a commit that took effect behind a lost answer needs a fault
  between the server and the client, which no test here injects.
- **A platform's PostgreSQL**: every server was this machine's. A managed service's own
  certificate chain, connection proxy, failover, connection cap and extensions were not met,
  nor any network between two hosts.
- **Majors other than 16 and 18**: the refusal of another major is the rule
  `pg::hosted::session` applies to the version the server says, tested on that rule alone
  (15, 17, 19); no server of another major was installed to be refused.
- **A server that speaks TLS below 1.2, and a client certificate**: the client certificate's
  settings are read and judged; no test logs in with one.
- **Query plans** under each locale were not compared: the search's hits, order and scores
  were, and are the same.
- **Throughput and load**: no figure is claimed.

## Measurements

The analyzer (`board_terms`), one call of a statement of its own over TLS on loopback, on a
quiet machine:

| Text | 16 | 18 |
| --- | --- | --- |
| a 72-byte ASCII title | 0.22 ms | 0.21 ms |
| a 2.5 KB ASCII body | 0.88 ms | 0.88 ms |
| a 2.6 KB body with accented words | 17.0 ms | 2.9 ms |

16's first definition, a SQL function, cost 6.6 ms for the title and 56 ms for the accented
body: PostgreSQL read the data's thousands of literals again at every call. As a PL/pgSQL
function with a query of its own for ASCII text it costs what the table says.

An earlier sweep by hand, before the test existed, compared `board_terms` on 16 and 18 for
every code point in five places (1,112,063 code points, the two results' MD5 the same,
`c654be222e69edfc973d6cc36a1e5ba3`); the test compares every code point in three places and
every pair of ASCII characters at each run.

The whole hosted target, 43 tests on both majors with its eight servers started and stopped,
took 36 seconds.
