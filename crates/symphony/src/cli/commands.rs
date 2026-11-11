//! The commands the binary answers to: the Symphony's own, `serve` last, with the commands the
//! plugins this process holds add before it. A name none of them has is refused with them all.

use std::io::Write;

mod release;
mod shot;

/// The Symphony's own commands, in the order its usage names them.
const OWN: [&str; 13] = [
    "status",
    "reply",
    "docs-audit",
    "blocks",
    "publish-client",
    "release-inputs",
    "triage",
    "verdict-tasks",
    "ask",
    "init",
    "hook",
    "shot",
    "serve",
];

/// `name`, a command the Symphony does not have, run by the first plugin that adds it with `args`;
/// a name no plugin adds is refused. Its exit code.
pub(super) fn plugin(name: &str, args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if name == "release-inputs" {
        return release::main(args, out, err);
    }
    if name == "init" {
        return crate::init::main(args, out, err);
    }
    if name == "shot" {
        return shot::main(args, out, err);
    }
    if let Some(command) = crate::plugins::commands()
        .into_iter()
        .find(|command| command.name() == name)
    {
        return command.run(args, out, err);
    }
    let names = names();
    let (last, rest) = names.split_last().expect("the Symphony's own commands");
    let _ = writeln!(
        err,
        "symphony: unknown command {name:?}; use {} or {last}",
        rest.join(", ")
    );
    2
}

/// A command line naming no command: the usage. Exit 2.
pub(super) fn usage(err: &mut dyn Write) -> i32 {
    let _ = writeln!(err, "usage: symphony {} …", names().join("|"));
    2
}

/// Every command name, in the order the usage names them.
fn names() -> Vec<String> {
    let (serve, own) = OWN.split_last().expect("the Symphony's own commands");
    own.iter()
        .map(|name| (*name).to_string())
        .chain(
            crate::plugins::commands()
                .into_iter()
                .map(|command| command.name().to_string()),
        )
        .chain([(*serve).to_string()])
        .collect()
}
