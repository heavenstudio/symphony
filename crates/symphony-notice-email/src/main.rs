//! `symphony-notice-email`: the notice on stdin, sent by mail (`docs/operations/NOTICE_EMAIL.md`).
//! Its exit status says what came of it, and stderr why it was not sent
//! (docs/engineering/NOTICES.md, "The hand-off").

#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    symphony_notice::take(symphony_notice_email::send)
}
