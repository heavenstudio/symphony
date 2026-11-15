//! `symphony-desktop [add URL]`: the app's command line. With no words it opens the app; `add URL`
//! adds the server at URL to its list, the way to add one until the app has screens of its own.

use std::io::Write;
use std::time::Duration;

use crate::read::{self, Answer};
use crate::servers::{self, Places};

const USAGE: &str = "usage: symphony-desktop [add URL]\n\n\
With no words, opens a window on each server the app follows: this Mac's, found by itself,\n\
and each added one. `add URL` adds the server at URL (http:// to this Mac, the tailnet's\n\
addresses or a *.ts.net name) once it answers as a Symphony.";

/// How long a read for `add` waits at each step.
const ADD_WAIT: Duration = Duration::from_secs(10);

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Asked {
    /// Open the app on the servers these places hold.
    Open(Places),
    /// Exit with this code: the command is done, or refused.
    Exit(i32),
}

/// What `args` (the words after the program's name) ask for, with the places `var` names, the
/// command's lines written to `out` and its refusals to `err`.
pub fn main(
    args: &[String],
    var: impl Fn(&str) -> Option<String>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Asked {
    main_reading(args, var, out, err, |server| {
        read::projects(server, ADD_WAIT)
    })
}

/// [`main`], each server read by `read`.
pub(crate) fn main_reading(
    args: &[String],
    var: impl Fn(&str) -> Option<String>,
    out: &mut dyn Write,
    err: &mut dyn Write,
    read: impl Fn(&crate::Server) -> Answer,
) -> Asked {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let refuse = |err: &mut dyn Write, why: &str| {
        let _ = writeln!(err, "✖ symphony-desktop: {why}");
        Asked::Exit(2)
    };
    let places = match words.as_slice() {
        ["-h" | "--help"] => {
            let _ = writeln!(out, "{USAGE}");
            return Asked::Exit(0);
        }
        [] | ["add", _] => match Places::from(var) {
            Ok(places) => places,
            Err(why) => return refuse(err, &why),
        },
        _ => return refuse(err, &format!("{USAGE}\n\nit takes no words {words:?}")),
    };
    let ["add", pasted] = words.as_slice() else {
        return Asked::Open(places);
    };
    let here = places.machine.as_deref().and_then(servers::this_mac);
    match servers::add(&places.settings, pasted, here.as_ref(), read) {
        Ok(saved) => {
            let _ = writeln!(
                out,
                "added {} as {}, in {}",
                saved.server,
                saved.name,
                places.settings.join(servers::LIST).display()
            );
            Asked::Exit(0)
        }
        Err(why) => refuse(err, &why),
    }
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
