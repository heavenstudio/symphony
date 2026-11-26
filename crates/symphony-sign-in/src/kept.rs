//! What the service keeps, in memory alone: the sign-ins under way, each under its `state`, and the
//! sessions, each under its id. A restart forgets them all, and a visitor signs in again.
//!
//! Both are kept by the SHA-256 of their token, so a lookup's timing says nothing of a token, and
//! the browser's flow cookie is compared with the `state` the provider hands back in constant time
//! ([`Kept::take`]). A sign-in under way is taken once: a second way back with its `state` finds
//! nothing. At most [`FLOWS`] are kept, the oldest dropped first, as anyone may begin one; a session
//! is made only by a sign-in whose way back and id token were taken ([`Kept::open`]).

use crate::random::token;
use crate::word::Who;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest as _, Sha256};
use std::collections::{HashMap, VecDeque};
use std::io;
use subtle::ConstantTimeEq as _;

/// How long a sign-in under way waits for its way back, in seconds.
pub(crate) const FLOW_SECONDS: u64 = 10 * 60;
/// The most sign-ins under way kept at once.
pub(crate) const FLOWS: usize = 1000;
/// How long a session holds from its sign-in, in seconds, however it is used.
pub(crate) const SESSION_SECONDS: u64 = 12 * 60 * 60;

/// Why a way back is not taken: its `state` is not the browser's own.
pub(crate) const UNBOUND: &str = "its state is not the one this browser's sign-in began with";
/// Why a way back is not taken: no sign-in under way has its `state`.
pub(crate) const UNKNOWN: &str =
    "its state names no sign-in under way, as one forged or taken already";
/// Why a way back is not taken: its sign-in began too long ago.
pub(crate) const STALE: &str = "its sign-in began more than ten minutes ago";

/// A sign-in under way: what the provider is sent, and what its way back is checked against.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Flow {
    /// Sent to the provider and handed back, and kept in the browser's flow cookie.
    pub(crate) state: String,
    /// Sent to the provider, which puts it in the id token.
    pub(crate) nonce: String,
    /// PKCE's verifier, sent with the code: the provider was sent its [`Flow::challenge`].
    pub(crate) verifier: String,
    /// The path of this origin the visitor asked for, where they are sent once signed in.
    pub(crate) to: String,
}

impl Flow {
    /// PKCE's S256 challenge of the verifier: the base64url of its SHA-256.
    pub(crate) fn challenge(&self) -> String {
        URL_SAFE_NO_PAD.encode(Sha256::digest(self.verifier.as_bytes()))
    }
}

/// A sign-in under way as kept, under its state's digest.
struct Pending {
    digest: [u8; 32],
    began: u64,
    nonce: String,
    verifier: String,
    to: String,
}

/// A session: who signed in, and until when it holds.
struct Session {
    who: Who,
    until: u64,
}

/// The sign-ins under way and the sessions.
#[derive(Default)]
pub(crate) struct Kept {
    flows: VecDeque<Pending>,
    sessions: HashMap<[u8; 32], Session>,
}

impl Kept {
    /// A sign-in begun at `now` for a visitor who asked for `to`, kept until its way back: those
    /// past [`FLOW_SECONDS`] dropped, and the oldest while [`FLOWS`] are kept.
    pub(crate) fn begin(&mut self, to: String, now: u64) -> io::Result<Flow> {
        let flow = Flow {
            state: token()?,
            nonce: token()?,
            verifier: token()?,
            to,
        };
        self.flows
            .retain(|pending| pending.began.saturating_add(FLOW_SECONDS) > now);
        while self.flows.len() >= FLOWS {
            self.flows.pop_front();
        }
        self.flows.push_back(Pending {
            digest: digest(&flow.state),
            began: now,
            nonce: flow.nonce.clone(),
            verifier: flow.verifier.clone(),
            to: flow.to.clone(),
        });
        Ok(flow)
    }

    /// The sign-in under way the provider's way back names by `state`, taken at `now` from a
    /// browser whose flow cookie holds `cookie`: taken out, so it is taken once.
    ///
    /// # Errors
    ///
    /// [`UNBOUND`] where the cookie is not the state, which leaves the sign-in under way for its
    /// own browser; [`UNKNOWN`] where none is kept under it; [`STALE`] where it began
    /// [`FLOW_SECONDS`] or more before `now`.
    pub(crate) fn take(
        &mut self,
        state: &str,
        cookie: Option<&str>,
        now: u64,
    ) -> Result<Flow, &'static str> {
        let bound =
            cookie.is_some_and(|cookie| bool::from(cookie.as_bytes().ct_eq(state.as_bytes())));
        if !bound {
            return Err(UNBOUND);
        }
        let digest = digest(state);
        let pending = (self.flows.iter())
            .position(|pending| pending.digest == digest)
            .and_then(|at| self.flows.remove(at))
            .ok_or(UNKNOWN)?;
        if pending.began.saturating_add(FLOW_SECONDS) <= now {
            return Err(STALE);
        }
        Ok(Flow {
            state: state.into(),
            nonce: pending.nonce,
            verifier: pending.verifier,
            to: pending.to,
        })
    }

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
