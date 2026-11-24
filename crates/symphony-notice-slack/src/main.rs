//! `symphony-notice-slack`: the one notice on stdin sent by Slack, its exit status saying what came
//! of it (`docs/operations/NOTICE_SLACK.md`).

use std::process::ExitCode;
use symphony_notice_slack::{API, machine_folder, send};

fn main() -> ExitCode {
    let folder = machine_folder(|name| std::env::var_os(name), cfg!(target_os = "macos"));
    symphony_notice::take(|notice| send(notice, folder, API))
}
