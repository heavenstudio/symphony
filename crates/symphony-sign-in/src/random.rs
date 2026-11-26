//! The system's randomness, read from `/dev/urandom`: the key's seed, and every token a sign-in
//! and a session carry.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use std::fs::File;
use std::io::{self, Read as _};

/// `N` bytes of the system's randomness.
pub(crate) fn bytes<const N: usize>() -> io::Result<[u8; N]> {
    let mut bytes = [0; N];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// A token of 32 random bytes, base64url with no padding: 43 characters, none a URL, a form or a
/// cookie escapes.
pub(crate) fn token() -> io::Result<String> {
    Ok(URL_SAFE_NO_PAD.encode(bytes::<32>()?))
}
