//! A sign-in under way, kept by nobody: what its way back is checked against travels in the
//! browser's flow cookie, signed by the service, so nothing is kept for it and nobody's sign-in can
//! push another's out, however many anyone begins.
//!
//! The cookie holds the sign-in's `state`, `nonce` and PKCE verifier, the path to come back to and
//! when it ends, with an HMAC-SHA256 tag of them under a key the service makes at each start and
//! never writes ([`Seal`]): a restart ends every sign-in under way, as it ends every session. A way
//! back is taken only with a cookie whose tag is the service's, checked in constant time, whose
//! `state` is the one the provider hands back, also in constant time, and within its ten minutes
//! ([`Seal::take`]).
//!
//! A sign-in is taken once because the provider takes its code once: a way back repeated, cookie
//! and all, is exchanged again and refused by the provider, and a session is made only after a good
//! exchange. The browser holding its own verifier loses nothing: PKCE keeps a code from anyone who
//! did not begin its sign-in, not from the browser that did.

use crate::random::{bytes, token};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit as _, Mac as _};
use sha2::{Digest as _, Sha256};
use std::fmt;
use std::io;
use subtle::ConstantTimeEq as _;

/// How long a sign-in under way waits for its way back, in seconds.
pub(crate) const FLOW_SECONDS: u64 = 10 * 60;
/// The longest path a visitor comes back to, in bytes, with its query: a longer one comes back to
/// `/`, so the flow cookie stays well inside the 4 KiB a browser keeps of one.
pub(crate) const TO_BYTES: usize = 2048;

/// Why a way back is not taken: its `state` is not the browser's own.
pub(crate) const UNBOUND: &str = "its state is not the one this browser's sign-in began with";
/// Why a way back is not taken: its browser's flow cookie is not one the service signed, as one
/// forged, altered, or signed before a restart.
pub(crate) const FORGED: &str = "its flow cookie is not one this service signed";
/// Why a way back is not taken: its sign-in began too long ago.
pub(crate) const STALE: &str = "its sign-in began more than ten minutes ago";

/// A sign-in under way: what the provider is sent, and what its way back is checked against.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Flow {
    /// Sent to the provider and handed back, and kept in the browser's flow cookie.
    pub(crate) state: String,
    /// Sent to Google, which puts it in the id token.
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

/// The key flow cookies are signed with: 64 bytes of the system's randomness, SHA-256's block, made
/// at start and never written, and never the word's key, so nothing signed for a flow passes as a
/// word. Its `Debug` says only that it is one.
pub(crate) struct Seal([u8; 64]);

impl fmt::Debug for Seal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Seal(hidden)")
    }
}

impl Seal {
    /// A fresh key.
    ///
    /// # Errors
    ///
    /// The system's randomness did not read.
    pub(crate) fn new() -> io::Result<Seal> {
        bytes().map(Seal)
    }

    /// A sign-in begun at `now` for a visitor who asked for `to`, and its flow cookie's value. A
    /// path longer than [`TO_BYTES`] comes back to `/`.
    ///
    /// # Errors
    ///
    /// The system's randomness did not read.
    pub(crate) fn begin(&self, to: &str, now: u64) -> io::Result<(Flow, String)> {
        let to = if to.len() > TO_BYTES { "/" } else { to };
        let flow = Flow {
            state: token()?,
            nonce: token()?,
            verifier: token()?,
            to: to.into(),
        };
        let until = now.saturating_add(FLOW_SECONDS);
        // The tokens are base64url, so no `.` but those between the parts, and the path is last.
        let said = URL_SAFE_NO_PAD.encode(format!(
            "{until}.{}.{}.{}.{}",
            flow.state, flow.nonce, flow.verifier, flow.to
        ));
        let tag = URL_SAFE_NO_PAD.encode(self.mac(&said).finalize().into_bytes());
        Ok((flow, format!("{said}.{tag}")))
    }

    /// The sign-in under way the provider's way back names by `state`, taken at `now` from a
    /// browser whose flow cookie holds `cookie`.
    ///
    /// # Errors
    ///
    /// [`UNBOUND`] where there is no cookie, or its state is not `state`; [`FORGED`] where its tag
    /// is not the service's; [`STALE`] where it began [`FLOW_SECONDS`] or more before `now`.
    pub(crate) fn take(
        &self,
        state: &str,
        cookie: Option<&str>,
        now: u64,
    ) -> Result<Flow, &'static str> {
        let cookie = cookie.ok_or(UNBOUND)?;
        let (said, tag) = cookie.rsplit_once('.').ok_or(FORGED)?;
        let tag = URL_SAFE_NO_PAD.decode(tag).ok().ok_or(FORGED)?;
        self.mac(said).verify_slice(&tag).ok().ok_or(FORGED)?;
        // Past the tag, the cookie is one this service made, so what follows reads as it was
        // written; it is still refused, never trusted, where it would not.
        let text = (URL_SAFE_NO_PAD.decode(said).ok())
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .ok_or(FORGED)?;
        let mut parts = text.splitn(5, '.');
        let (Some(until), Some(kept), Some(nonce), Some(verifier), Some(to)) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            return Err(FORGED);
        };
        let until: u64 = until.parse().ok().ok_or(FORGED)?;
        if !bool::from(kept.as_bytes().ct_eq(state.as_bytes())) {
            return Err(UNBOUND);
        }
        if until <= now {
            return Err(STALE);
        }
        Ok(Flow {
            state: state.into(),
            nonce: nonce.into(),
            verifier: verifier.into(),
            to: to.into(),
        })
    }

    /// The tag of `said`, under this key.
    fn mac(&self, said: &str) -> Hmac<Sha256> {
        let mut mac = Hmac::<Sha256>::new(&self.0.into());
        mac.update(said.as_bytes());
        mac
    }
}

#[cfg(test)]
#[path = "flow_tests.rs"]
mod tests;
