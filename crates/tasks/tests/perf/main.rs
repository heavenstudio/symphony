//! The board's cost at today's scale and above, a test target of its own (`--test perf`): a
//! generated board ([`generated`]: 50k messages, 36k deliveries, 2.6k sessions, 2.8k roles, 2.2k
//! tasks and 100 recipients with a native callback) on a PostgreSQL of the test binary's own, in
//! the run's folder ([`board`]), as the live board runs on its own.
//!
//! - [`copied`]: the generated board is copied whole onto the suite's server, its data in the run's
//!   folder: its rows and its server, never a time.
//! - [`plans`]: every statement the board's commands on a per-write, per-recipient or per-tick path
//!   run (a say, a claim, a take of the orchestrator role, an inbox, the dispatch passes to every
//!   recipient and their first looks, the `tasks api` calls land makes, the pull digest and the
//!   agents view a recipient's look reads), recorded on its way to the server ([`proxy`]) and
//!   planned by the server as it runs, each a block or a trigger runs too (`auto_explain`, each
//!   plan a notice to the session that ran the statement, kept by the relay), is planned with no
//!   scan of message, delivery, role, session or callback_state under a condition no index
//!   serves, and no subquery run again for every row that reads a table by no index each time
//!   ([`findings`]). A statement sent that no plan was given of, and a plan the check cannot read,
//!   fail it too: neither is skipped.
//!   Deterministic: the plans, never a time.
//! - [`timings`]: say, claim, inbox, looks of `tasks wait` and the api answers, each run as the
//!   `tasks` binary an agent runs, the CPU it costs (its own and the server's, the postmaster and
//!   every process under it, [`cpu`]) within a bound stated there, which the machine's load does
//!   not move as it moves a wall.
//! - [`sessions`]: the session rows show, list, next, who and a claim read: the sessions on the
//!   tasks a read shows, the whole list only where every session is asked for. Rows, never a time.
//! - [`unanswered`]: the unanswered-post lookup a PULL digest runs reads the open tasks' threads
//!   alone, each read on an index, within a bound of the server's CPU.
//! - [`words`]: each sender's last word kept by its trigger where the board is biggest, a rare
//!   sender's newest word deleted and every message deleted at once, within a bound of the
//!   server's CPU stated there.
//! - [`reports`]: the hourly roll-up's one read of the board, within a bound of the server's CPU.
//! - [`cleanup`]: the suite's server stopped and ended at the binary's exit, before its process
//!   is.
//!
//! On 2026-11-07 one board write took minutes on the live board, then on SurrealDB: each of 95
//! recipients' inbox read ran `session WHERE handle IN (SELECT … FROM role)`, the role scan again
//! for every session, 7 s a recipient. The suite is how a change to the board's statements shows
//! what it costs at that scale. Its bounds were measured on SurrealDB and are kept on PostgreSQL;
//! each run prints its figures beside them.

// How a test starts its own children ([`children::Child`]).
#[path = "../support/children.rs"]
mod children;

#[path = "../store/common.rs"]
#[allow(dead_code)]
mod store;

mod board;
mod cleanup;
mod copied;
mod cpu;
mod findings;
mod generated;
mod listeners;
mod plans;
mod proxy;
mod reports;
mod sessions;
mod timings;
mod unanswered;
mod words;
