//! The optional exchange of selected tasks with GitHub issues (docs/operations/GITHUB.md): turned off
//! unless the project's default branch commits `symphony-github.toml` and it says otherwise.
//!
//! It is a program of its own. It reaches the board through `tasks api` ([`board`]), GitHub
//! through `curl` ([`transport`]), and keeps what it must remember in the folder the board names
//! for it ([`state`]). Nothing here is linked into the board, the library or the server, and
//! nothing runs it but an operator: `preview` shows what a reconcile would do, and `reconcile`
//! does the preview it is handed by digest ([`reconcile`]). `deliver` answers a land delivered by
//! a pull request (`[land] delivery`, [`deliver`]), which the land runs and no one else, and
//! reads the repository's protection before it does ([`preflight`]); `rulesets` verifies who
//! bypasses it, with the gh CLI of someone who administers the repository ([`rulesets`]). `release` answers the release
//! run with a GitHub Release for a tag it pushed ([`release`]), and `verify` with its commit's
//! verification run on GitHub's machines ([`verify`]).

#![forbid(unsafe_code)]

pub mod board;
pub mod cli;
pub mod config;
pub mod credential;
pub mod deliver;
pub mod describe;
pub mod github;
pub mod mint;
pub mod plan;
pub mod preflight;
pub mod reconcile;
pub mod release;
pub mod rulesets;
pub mod state;
pub mod transport;
pub mod verify;

/// The integration's name: its state folder's, `symphony/integrations/github`.
pub const NAME: &str = "github";

/// The SHA-256 of `parts`, each ended so that no two lists of parts read the same.
#[must_use]
pub fn digest(parts: &[&str]) -> String {
    use sha2::{Digest as _, Sha256};
    use std::fmt::Write as _;
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.len().to_le_bytes());
        hasher.update(part.as_bytes());
    }
    hasher
        .finalize()
        .iter()
        .fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}
