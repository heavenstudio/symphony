//! The word the service signs on every request it passes on (`docs/operations/SIGNED_IN.md`): a JWT
//! of `{"alg":"EdDSA","typ":"JWT"}` and the claims `iss`, `sub`, `aud`, `iat` and `exp`, signed
//! with Ed25519 over its first two parts. It is made afresh for each request and holds
//! [`LIFETIME`] seconds, well inside the server's bound; it carries no `name`, no group and no role.

use crate::settings::Key;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::Signer as _;

/// The header the word goes in, lowercase as HTTP names it: `symphony::signed_in::HEADER`.
pub(crate) const HEADER: &str = "symphony-signed-in";
/// How long a word holds from its issue, in seconds, and so how long a feed it opened stays open.
pub(crate) const LIFETIME: u64 = 300;

/// Who a session is: the service's name, the SERVICE of a profile's `SERVICE:SUBJECT` sign-in,
/// and the provider's subject for them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Who {
    pub(crate) service: &'static str,
    pub(crate) subject: String,
}

impl Who {
    /// The sign-in a profile names them by: `SERVICE:SUBJECT`.
    pub(crate) fn signin(&self) -> String {
        format!("{}:{}", self.service, self.subject)
    }
}

/// The word `key` signs for `who`, made for `origin` and issued at `now`, in seconds since 1970.
pub(crate) fn signed(key: &Key, who: &Who, origin: &str, now: u64) -> String {
    let part = |bytes: &[u8]| URL_SAFE_NO_PAD.encode(bytes);
    let claims = serde_json::json!({
        "iss": who.service,
        "sub": who.subject,
        "aud": origin,
        "iat": now,
        "exp": now + LIFETIME,
    });
    let input = format!(
        "{}.{}",
        part(br#"{"alg":"EdDSA","typ":"JWT"}"#),
        part(claims.to_string().as_bytes())
    );
    let signature = key.signing().sign(input.as_bytes());
    format!("{input}.{}", part(&signature.to_bytes()))
}

#[cfg(test)]
#[path = "word_tests.rs"]
mod tests;
