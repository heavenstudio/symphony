//! Notices outside the page (docs/engineering/NOTICES.md, "The server's part"): the machine's one
//! server tells a thing's people of it by the way each chose, handing one notice at a time to that
//! way's program, which the machine folder's `notices/programs.toml` names
//! ([`symphony::notices::Programs`]). A private server, a linked worktree's among them, tells no one,
//! so two servers never tell one event twice.
//!
//! - **What tells**: a card that starts waiting ([`symphony::owners::waiting`]), each person of its
//!   group told once while it waits and again once it waits again; an agent's question to the
//!   person at the machine on a task, and a reviewer's verdict, each line once. Who is told is
//!   [`symphony_board::people::ownership::told`]: the members of the thing's group, or the person it
//!   is written to. Each is reached by the way their profile chose for that kind and their address
//!   for it ([`symphony_board::people::Profiles::reach`]); a kind with no way, a way with no
//!   address, or one no program is named for, sends nothing, as the page and the Mac app show every
//!   notice anyway.
//! - **Once**: the project's record ([`record`]) keeps the newest board line looked at, the cards
//!   waiting with who was told of each, and each notice not settled yet. The first look takes what
//!   stands as told: turning notices on sends nothing of the past.
//! - **Again**: a `75` is handed again after a minute, doubling to an hour, at most
//!   [`runner::MOST_HANDS`] hands; any other status that is not `0` is said once and dropped. A
//!   program is ended, with its group, after [`hand::MOST`]. A restart takes the record up where it
//!   was: a notice due again is handed once when due, and one a restart cut mid-hand is handed again
//!   with the same id, which a plugin keeping its ids answers as sent.
//! - **No address but on the program's stdin**: the record keeps a digest of it; the log names the
//!   project, the notice's id, kind, person and way; what a program says is repeated with the address
//!   and the link replaced; no agent, plugin of a project, page or feed is handed one.

pub(crate) mod hand;
pub(crate) mod record;
pub(crate) mod runner;
