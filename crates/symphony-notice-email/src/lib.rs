//! The email notice plugin: one person's notice, handed on stdin by the server
//! (docs/engineering/NOTICES.md), sent as a mail to their address
//! (`docs/operations/NOTICE_EMAIL.md`).
//!
//! It reaches the mail server by SMTP through `curl`, which reads the whole request on its stdin
//! ([`curl`]), so the password, the address and the link are in no argument and no environment of
//! any process. One way covers a mail server and its password and a mail service's key alike, as a
//! mail service takes its key as the SMTP password. Its settings and its password are its own, in
//! the machine folder's `notices/` ([`settings`]), never in a project's `symphony.toml`; the mail
//! says what waits on the person, in which project, and the link ([`mail`]).
//!
//! [`send`] is the plugin's `send`, which its `main` hands [`symphony_notice::take`]: a notice by
//! another way than [`EMAIL`], to an address that is no plain email address, or with no settings to
//! send it by is refused and never sent.

#![forbid(unsafe_code)]

mod curl;
mod mail;
mod settings;

use std::path::PathBuf;
use std::time::SystemTime;
use symphony_notice::{EMAIL, Notice, Unsent};

/// Sends `notice` by mail, by the settings the machine folder's `notices/` holds, through the
/// `curl` on `PATH`.
///
/// # Errors
///
/// [`Unsent::Later`] where the mail server did not answer or asked to be called later;
/// [`Unsent::Never`] where it refused the login, the address or the message, where the notice is
/// not one this plugin sends, and where its settings are missing or do not read. The reason names
/// neither the address, the link nor the password.
pub fn send(notice: &Notice) -> Result<(), Unsent> {
    let folder = settings::machine_folder(|name| std::env::var_os(name), cfg!(target_os = "macos"))
        .map_err(Unsent::Never)?;
    Sender {
        folder,
        curl: "curl".into(),
    }
    .send(notice, SystemTime::now())
}

/// What a notice is sent with: the machine folder its settings are read from, and the `curl` run.
struct Sender {
    folder: PathBuf,
    curl: String,
}

impl Sender {
    fn send(&self, notice: &Notice, now: SystemTime) -> Result<(), Unsent> {
        let way = notice.to().way();
        if way != EMAIL {
            return Err(Unsent::Never(format!(
                "this plugin sends a notice by `{EMAIL}`, and this one is by `{way}`"
            )));
        }
        if !mail::address(notice.to().address()) {
            return Err(Unsent::Never(format!(
                "the notice's address is no plain email address: {}",
                mail::ADDRESS_RULE
            )));
        }
        let settings = settings::read(&self.folder).map_err(Unsent::Never)?;
        let mail = mail::Mail::of(notice, settings.from(), now);
        curl::send(&self.curl, &settings, notice, &mail)
    }
}

#[cfg(test)]
mod tests;
