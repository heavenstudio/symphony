//! What the service keeps, in memory alone: the sessions, each under the SHA-256 of its id, so a
//! lookup's timing says nothing of an id. A restart forgets them all, and a visitor signs in again.
//! A session is made only by a sign-in whose way back and provider's answer were taken
//! ([`Kept::open`]); a sign-in under way is kept by nobody (`crate::flow`).

use crate::random::token;
use crate::word::Who;
use sha2::{Digest as _, Sha256};
use std::collections::HashMap;
use std::io;

/// How long a session holds from its sign-in, in seconds, however it is used.
pub(crate) const SESSION_SECONDS: u64 = 12 * 60 * 60;

/// A session: who signed in, and until when it holds.
struct Session {
    who: Who,
    until: u64,
}

/// The sessions.
#[derive(Default)]
pub(crate) struct Kept {
    sessions: HashMap<[u8; 32], Session>,
}

impl Kept {
    /// A session for `who`, signed in at `now`, held for [`SESSION_SECONDS`]: its id, which the
    /// browser's session cookie holds. Sessions past their end are dropped.
    pub(crate) fn open(&mut self, who: Who, now: u64) -> io::Result<String> {
        let id = token()?;
        self.sessions.retain(|_, session| session.until > now);
        self.sessions.insert(
            digest(&id),
            Session {
                who,
                until: now.saturating_add(SESSION_SECONDS),
            },
        );
        Ok(id)
    }

    /// Who the session `id` is at `now`: none where none is kept under it, or it has ended.
    pub(crate) fn who(&self, id: &str, now: u64) -> Option<Who> {
        (self.sessions.get(&digest(id)))
            .filter(|session| session.until > now)
            .map(|session| session.who.clone())
    }

    /// Ends the session `id`: whether one was kept under it.
    pub(crate) fn close(&mut self, id: &str) -> bool {
        self.sessions.remove(&digest(id)).is_some()
    }
}

/// The digest a token is kept under.
fn digest(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

#[cfg(test)]
#[path = "kept_tests.rs"]
mod tests;
