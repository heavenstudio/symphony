//! The board's cost at today's scale and above, a test target of its own (`--test perf`): a
//! generated board ([`generated`]: 50k messages, 36k deliveries, 2.6k sessions, 2.8k roles, 2.2k
//! tasks and 100 recipients with a native callback) on the engine the live board runs on,
//! `surrealkv://` ([`board`]).
//!
//! - [`copied`]: the generated board is copied whole onto a store in the run's folder, on a server
//!   the store's fixture starts as committed: its rows and its store, never a time.
//! - [`plans`]: every query the board's commands on a per-write, per-recipient or per-tick path send
//!   (a say, a claim, an inbox, the dispatch passes to every recipient, the `tasks api` calls land
//!   makes, the pull digest and the agents view a recipient's look reads), recorded on their way to
//!   the server, is planned with no filtered scan of a whole table among message, delivery, role,
//!   session and callback_state, and no subquery in a predicate, which the server runs again for
//!   every row it tests; and so is every query the schema's events run in the write that fires
//!   them. A statement of a request that the server plans as a read of rows, and the check read
//!   no statement out of, fails it too: a spelling it does not know is reported, not skipped.
//!   Deterministic: the plans, never a time. [`sql`] reads the statements out of a request or the
//!   schema as the server's lexer does; [`findings`] reads what is wrong out of a plan.
//! - [`timings`]: say, claim, inbox, looks of `tasks wait` and the api answers, each run as the
//!   `tasks` binary an agent runs, the CPU it costs (its own and the server's, [`cpu`]) within a
//!   bound stated there, which the machine's load does not move as it moves a wall.
//! - [`sessions`]: the session rows show, list, next, who and a claim read: the sessions on the
//!   tasks a read shows, the whole list only where every session is asked for. Rows, never a time.
//! - [`unanswered`]: the unanswered-post lookup a PULL digest runs reads the open tasks' threads
//!   alone, by record range, within a bound of the server's CPU.
//! - [`words`]: each sender's last word kept by its event where the board is biggest, a rare
//!   sender's newest word deleted and every message deleted at once, within a bound of the
//!   server's CPU stated there.
//!
//! On 2026-11-07 one board write took minutes on the live board: each of 95 recipients' inbox read
//! ran `session WHERE handle IN (SELECT … FROM role)`, the role scan again for every session, 7 s a
//! recipient. The suite is how a change to the board's queries shows what it costs at that scale.

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
mod sql;
mod timings;
mod unanswered;
mod words;
