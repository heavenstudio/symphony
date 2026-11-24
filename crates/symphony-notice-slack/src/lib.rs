//! The notice plugin for Slack (`docs/operations/NOTICE_SLACK.md`): one person's notice, handed on
//! stdin by the notice message's contract (docs/engineering/NOTICES.md), sent as a direct message
//! from the workspace's Slack app to the person's member id.
//!
//! [`send`] is the whole of it, which the program's `main` hands to [`symphony_notice::take`]:
//! it refuses a notice of another way and an address that is no member id before anything else,
//! reads the app's bot token from the machine folder ([`token`]), posts Slack's
//! `chat.postMessage` through `curl` ([`curl`]) and reads Slack's answer into the exit status
//! ([`slack`]). Nothing it says names the address, the link or the token.

#![forbid(unsafe_code)]

mod curl;
mod slack;
mod token;

#[cfg(test)]
mod stand_in;

use std::path::PathBuf;
use symphony_notice::{Notice, SLACK, Unsent};

pub use token::{NOTICES, TOKEN_FILE};

/// Slack's Web API, the one host the program reaches.
pub const API: &str = "https://slack.com/api";

/// Sends `notice` to its person's Slack member id, as the app whose bot token is
/// `NOTICES/TOKEN_FILE` in the machine `folder` ([`symphony_notice::machine_folder`]), through
/// the Web API at `api`.
///
/// # Errors
///
/// [`Unsent::Never`] for a notice of another way than [`SLACK`], an address that is no member id,
/// no token to read, and what Slack refuses; [`Unsent::Later`] where Slack did not answer, asked to
/// be called later, or answered so that whether it sent is not known.
pub fn send(notice: &Notice, folder: Result<PathBuf, String>, api: &str) -> Result<(), Unsent> {
    let to = notice.to();
    if to.way() != SLACK {
        return Err(Unsent::Never(format!(
            "this program sends by {SLACK}, and the notice goes by {}",
            to.way()
        )));
    }
    if !symphony_notice::ways::slack(to.address()) {
        return Err(Unsent::Never(
            "the address is no Slack member id (U or W, then capital letters and digits)".into(),
        ));
    }
    let folder = folder.map_err(|why| Unsent::Never(format!("no token was read: {why}")))?;
    let token = token::read(&folder).map_err(Unsent::Never)?;
    let answer = curl::Curl::new(api, token).post(slack::POST_MESSAGE, &slack::body(notice))?;
    slack::answered(&answer)
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
