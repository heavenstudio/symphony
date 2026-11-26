//! The signed word (docs/operations/SIGNED_IN.md): a sign-in service in front of the server says
//! who a request is from in one header, [`HEADER`], a JWT it signs with Ed25519, and the server
//! verifies it and nothing more (docs/product/analysis/users-and-identity.md, "How a provider plugs
//! in"): the signature against the key the machine folder's [`FILE`] names, that the word is fresh,
//! and that it was made for this server, the origin that file names ([`Settings`]).
//!
//! [`carried`] and [`verify`] read nothing and keep nothing: the clock is given. What a verified
//! word says is who signed the person in and as whom ([`SignedIn`]); nothing in it gives a power,
//! and a claim of groups or roles is never read. The server matches the service and the subject
//! against the sign-ins of the machine's profiles, as it matches a Tailscale login.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::Deserialize;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// The header a signed request carries its word in, lowercase as HTTP names it.
pub const HEADER: &str = "symphony-signed-in";
/// The machine folder's file naming the key and the origin.
pub const FILE: &str = "sign-in.toml";
/// The longest word taken, in bytes.
pub const MAX_BYTES: usize = 4096;
/// The longest a word may hold, from its issue to its expiry, in seconds.
pub const MAX_LIFETIME: u64 = 900;
/// How far ahead of the server's clock a word may say it was issued, or starts, in seconds.
pub const SKEW: u64 = 30;
/// The one algorithm a word is signed with: Ed25519 (RFC 8037).
pub const ALGORITHM: &str = "EdDSA";
/// The service no signed word may name: a Tailscale login is the machine's daemon's word alone.
pub const DAEMONS: &str = "tailscale";
/// The longest service name taken, in bytes.
const SERVICE_BYTES: usize = 64;
/// What a refusal of the file says follows from it.
const REFUSING: &str = "no signed word is verified by it";

/// Who a verified word says a request is from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedIn {
    /// The service's name (`iss`): the SERVICE of a profile's `SERVICE:SUBJECT` sign-in.
    pub service: String,
    /// The service's subject for the person (`sub`).
    pub subject: String,
    /// How the service shows them (`name`), when it says.
    pub display: Option<String>,
    /// The origin the word was made for (`aud`), this server's as [`Settings`] names it.
    pub origin: String,
}

/// Why a word is refused. The server answers every one alike; the kinds are for its tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The request carries the header more than once.
    Repeated,
    /// The word is longer than [`MAX_BYTES`].
    Oversized,
    /// It is not three base64url parts of JSON the format holds, or a claim breaks its rule.
    Malformed,
    /// Its header names another algorithm, or a member the format does not hold.
    Algorithm,
    /// Its signature is not the key's over its first two parts.
    Signature,
    /// It names no service a profile could sign in by, or the daemon's.
    Service,
    /// It was made for another origin.
    Audience,
    /// It holds longer than [`MAX_LIFETIME`], or expires before it is issued.
    Lifetime,
    /// It is issued, or starts, more than [`SKEW`] ahead of the server's clock.
    Early,
    /// It has expired.
    Stale,
}

/// What the server checks a word against: the public key the service signs with and the origin
/// browsers reach the server at through it, as [`FILE`] names them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    key: VerifyingKey,
    origin: String,
}

impl Settings {
    /// The key `key`, `ed25519:` and 64 lowercase hex digits, and the origin `origin`.
    ///
    /// # Errors
    ///
    /// The key is not that form or no Ed25519 public key, or a weak one; the origin is not `http://`
    /// or `https://` and a lowercase host, with a port or none and nothing after it. Said by the
    /// rule, never the value.
    pub fn new(key: &str, origin: &str) -> Result<Settings, String> {
        Ok(Settings {
            key: verifying_key(key)?,
            origin: checked_origin(origin)?,
        })
    }

    /// The origin words are made for.
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// The settings `text`, a [`FILE`], names: its `key` and `origin`, and nothing else.
    ///
    /// # Errors
    ///
    /// It does not read as TOML, lacks either, names another key, or breaks [`Settings::new`]'s
    /// rules. Said by the rule, never the value.
    pub fn parse(text: &str) -> Result<Settings, String> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Named {
            key: String,
            origin: String,
        }
        let named: Named = toml::from_str(text).map_err(|error| error.message().to_string())?;
        Settings::new(&named.key, &named.origin)
    }

    /// The settings the machine folder `machine` names in its [`FILE`]: none where it holds none.
    ///
    /// # Errors
    ///
    /// The file is a link, another account's, or another account may write it; it does not read;
    /// it breaks [`Settings::parse`]'s rules. Said by its path and the rule.
    pub fn read(machine: &Path) -> Result<Option<Settings>, String> {
        let path = machine.join(FILE);
        let Some(text) = crate::notices::kept_text(&path, REFUSING)? else {
            return Ok(None);
        };
        Settings::parse(&text)
            .map(Some)
            .map_err(|why| format!("{}: {why}", path.display()))
    }
}

/// The word a request carries, given each value of its [`HEADER`] as sent: none when it carries
/// none.
///
/// # Errors
///
/// Two values or more, one longer than [`MAX_BYTES`], or one that is not visible ASCII.
pub fn carried<'a>(values: &[&'a [u8]]) -> Option<Result<&'a str, Refused>> {
    match values {
        [] => None,
        [value] if value.len() > MAX_BYTES => Some(Err(Refused::Oversized)),
        [value] => Some(
            (value.iter().all(u8::is_ascii_graphic))
                .then(|| std::str::from_utf8(value).ok())
                .flatten()
                .ok_or(Refused::Malformed),
        ),
        _ => Some(Err(Refused::Repeated)),
    }
}

/// Who the word `token` says a request is from, as `settings` verify it at `now`.
///
/// # Errors
///
/// It is longer than [`MAX_BYTES`]; it is not three base64url parts, unpadded; its header is not
/// `alg` `EdDSA` with at most `typ` `JWT` and a `kid`; its signature is not `settings`' key's over
/// its first two parts; its claims are not JSON with `iss`, `sub`, `aud`, `iat` and `exp`, the
/// times integer seconds; it names no service a profile could sign in by, or `tailscale`; an
/// empty subject; another origin; a life over [`MAX_LIFETIME`] or none; an issue or start more than
/// [`SKEW`] ahead of `now`; an expiry at or before `now`. The signature is checked before any claim
/// is read.
pub fn verify(token: &str, settings: &Settings, now: SystemTime) -> Result<SignedIn, Refused> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Header {
        alg: String,
        #[serde(default)]
        typ: Option<String>,
        /// A key's id is taken and not read: one key is named.
        #[serde(default, rename = "kid")]
        _kid: Option<String>,
    }
    #[derive(Deserialize)]
    struct Claims {
        iss: String,
        sub: String,
        aud: String,
        iat: u64,
        exp: u64,
        #[serde(default)]
        nbf: Option<u64>,
        #[serde(default)]
        name: Option<String>,
    }
    if token.len() > MAX_BYTES {
        return Err(Refused::Oversized);
    }
    let mut parts = token.split('.');
    let (Some(head), Some(body), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Refused::Malformed);
    };
    let header: Header = json(head)?;
    if header.alg != ALGORITHM || header.typ.as_deref().is_some_and(|typ| typ != "JWT") {
        return Err(Refused::Algorithm);
    }
    let signature: [u8; 64] = (decoded(signature)?.try_into())
        .ok()
        .ok_or(Refused::Malformed)?;
    let signed = (token.as_bytes())
        .get(..head.len() + 1 + body.len())
        .ok_or(Refused::Malformed)?;
    (settings.key)
        .verify_strict(signed, &Signature::from_bytes(&signature))
        .ok()
        .ok_or(Refused::Signature)?;
    let claims: Claims = json(body)?;
    if !service(&claims.iss) {
        return Err(Refused::Service);
    }
    if claims.sub.is_empty() {
        return Err(Refused::Malformed);
    }
    if claims.aud != settings.origin {
        return Err(Refused::Audience);
    }
    if claims.exp <= claims.iat || claims.exp - claims.iat > MAX_LIFETIME {
        return Err(Refused::Lifetime);
    }
    let now = now
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    if claims.iat.max(claims.nbf.unwrap_or(0)) > now.saturating_add(SKEW) {
        return Err(Refused::Early);
    }
    if now >= claims.exp {
        return Err(Refused::Stale);
    }
    Ok(SignedIn {
        service: claims.iss,
        subject: claims.sub,
        display: claims.name,
        origin: claims.aud,
    })
}

/// Whether `name` is a service a profile's sign-in can name and a signed word may: 1 to 64 of
/// lowercase letters, digits, `.`, `_` and `-`, starting with a letter or a digit, and not the
/// daemon's ([`DAEMONS`]).
pub fn service(name: &str) -> bool {
    name.len() <= SERVICE_BYTES
        && name != DAEMONS
        && name
            .bytes()
            .next()
            .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
        && (name.bytes()).all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'_' || b == b'-'
        })
}

/// One part of a word as bytes: base64url with no padding, and no bit set past its last byte.
fn decoded(part: &str) -> Result<Vec<u8>, Refused> {
    URL_SAFE_NO_PAD.decode(part).ok().ok_or(Refused::Malformed)
}

/// One part of a word read as the JSON object `T`: an array, which serde would read field by field,
/// refused, and a duplicated member too, as serde refuses one.
fn json<T: serde::de::DeserializeOwned>(part: &str) -> Result<T, Refused> {
    let bytes = decoded(part)?;
    if bytes.trim_ascii_start().first() != Some(&b'{') {
        return Err(Refused::Malformed);
    }
    serde_json::from_slice(&bytes)
        .ok()
        .ok_or(Refused::Malformed)
}

/// The public key `text` names: `ed25519:` and 64 lowercase hex digits, a point that is no weak
/// key.
fn verifying_key(text: &str) -> Result<VerifyingKey, String> {
    let bytes: [u8; 32] = (text.strip_prefix("ed25519:"))
        .and_then(|hex| (hex.len() == 64).then_some(hex))
        .and_then(|hex| {
            (0..32)
                .map(|at| {
                    let pair = hex.get(at * 2..at * 2 + 2)?;
                    let lower = pair
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
                    lower.then(|| u8::from_str_radix(pair, 16).ok()).flatten()
                })
                .collect::<Option<Vec<u8>>>()
        })
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or("the key is not ed25519: and 64 lowercase hex digits")?;
    let key = VerifyingKey::from_bytes(&bytes)
        .ok()
        .ok_or("the key is no Ed25519 public key")?;
    if key.is_weak() {
        return Err("the key is a weak Ed25519 key, which a forged signature passes".into());
    }
    Ok(key)
}

/// `text` as an origin words are made for: `http://` or `https://`, then a host of lowercase
/// letters, digits, `.` and `-` or an IPv6 address in brackets, then a port or none, and nothing
/// after it.
fn checked_origin(text: &str) -> Result<String, String> {
    const RULE: &str = "the origin is not http:// or https:// and a lowercase host, with a port or \
                        none and nothing after it, as https://board.example.com";
    let authority = (text.strip_prefix("https://"))
        .or_else(|| text.strip_prefix("http://"))
        .ok_or(RULE)?;
    let (host, port) = match authority.strip_prefix('[') {
        Some(rest) => {
            let (address, after) = rest.split_once(']').ok_or(RULE)?;
            let v6 = !address.is_empty()
                && (address.bytes())
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || b == b':');
            (v6.then_some(address).ok_or(RULE)?, after)
        }
        None => match authority.find(':') {
            Some(at) => (
                authority.get(..at).ok_or(RULE)?,
                authority.get(at..).ok_or(RULE)?,
            ),
            None => (authority, ""),
        },
    };
    let named = !host.is_empty()
        && (host.bytes()).all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-' || b == b':'
        });
    let ported = port.is_empty()
        || port
            .strip_prefix(':')
            .filter(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|digits| digits.parse::<u16>().ok())
            .is_some_and(|port| port != 0);
    (named && ported)
        .then(|| text.to_string())
        .ok_or_else(|| RULE.into())
}

#[cfg(test)]
#[path = "signed_in_tests.rs"]
mod tests;
