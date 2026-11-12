//! `symphony project add [DIR]` (and `symphony-server project add`, which `just symphony` runs as
//! its start registers): the project of DIR's checkout written into the machine folder's registry
//! (its main checkout, its id and its `symphony.toml`), and what the host rule says of the running
//! server ([`super::host`]): it serves the project, or it lacks the project's plugins and that
//! project's `just symphony` replaces it, or the project's pin is newer and someone must bump.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::host::{Build, Verdict, judge};
use super::{Added, Entry, Registry};

const USAGE: &str = "usage: symphony project add [DIR]";
/// The running server's record in the machine folder, as its starters write it.
const RECORD: &str = "server.json";
/// A project's Symphony install in its git common dir, which also held the record of a server
/// started before the machine folder.
const INSTALL: &str = "symphony/server";
/// How long the running server is given to say what it is.
const LOOK: Duration = Duration::from_secs(2);

/// `symphony project ARGS`: the exit code.
pub fn main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let dir = match args {
        [add] if add == "add" => ".",
        [add, dir] if add == "add" && !dir.starts_with('-') => dir.as_str(),
        _ => return crate::cli::refuse(err, USAGE, "project", &misnamed(args)),
    };
    let added = super::dir().and_then(|machine| {
        let front = running(&machine, older_record_folder(Path::new(dir)).as_deref());
        add(Path::new(dir), &machine, front.as_ref())
    });
    match added {
        Ok(said) => {
            let _ = writeln!(out, "{said}");
            0
        }
        Err(why) => {
            let _ = writeln!(err, "✖ Symphony: {why}");
            2
        }
    }
}

/// Why a `project` command line is refused, after its usage: no command, one other than `add`
/// (with it, when it is near), or words `add` does not take.
fn misnamed(args: &[String]) -> String {
    match args {
        [] => "the following arguments are required: command".into(),
        [command, ..] if command != "add" => {
            let near = if symphony_args::nearest(command, ["add"]).is_empty() {
                ""
            } else {
                "; did you mean 'add'?"
            };
            format!("argument command: invalid choice: {command:?} (choose from 'add'){near}")
        }
        [_, rest @ ..] => {
            let taken = usize::from(rest.first().is_some_and(|dir| !dir.starts_with('-')));
            format!("unrecognized arguments: {}", rest[taken..].join(" "))
        }
    }
}

/// The main checkout of the repository `dir` is in, else `dir` itself.
pub fn main_checkout(dir: &Path) -> PathBuf {
    symphony_board::git::common_dir(dir)
        .ok()
        .filter(|common| common.file_name().is_some_and(|name| name == ".git"))
        .and_then(|common| common.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| dir.to_path_buf())
}

/// Register the project at `dir` (its checkout's main) in `machine`, judged by the host rule
/// against `front`, the running server's status: what to say. A server from before pins is never
/// judged; with none running the project is registered for the next start.
///
/// # Errors
///
/// When the project does not load, the registry cannot be read or written, or the host rule
/// refuses it (its pin or board schema is newer than the running server's, which this build cannot
/// replace): a refused project is not registered.
pub fn add(
    dir: &Path,
    machine: &Path,
    front: Option<&symphony_api::Front>,
) -> Result<String, String> {
    let entry = Entry::of(&main_checkout(dir))?;
    let ours = Build::this(&entry.root);
    let verdict = match front.filter(|front| front.pin > 0) {
        Some(front) => Some(judge(
            &Build::of(front),
            &ours,
            &entry,
            &Registry::read(machine)?,
        )),
        None => None,
    };
    if let Some(Verdict::Refuse(why)) = verdict {
        return Err(why);
    }
    let added = Registry::change(machine, |registry| registry.add(entry.clone()))?;
    let what = match added {
        Added::New => "registered".to_string(),
        Added::Same => "already registered".to_string(),
        Added::Updated => "registered again".to_string(),
        Added::Moved(from) => format!("moved from {}", from.display()),
    };
    let said = format!(
        "· {} {what} at /{}/ ({})",
        entry.name,
        entry.id,
        entry.root.display()
    );
    Ok(match verdict {
        Some(Verdict::Serves) => format!("{said}; the running Symphony serves it"),
        Some(Verdict::Replace(why)) => format!(
            "{said}\n· {why}: `just symphony` in {} hands it over",
            entry.root.display()
        ),
        Some(Verdict::Waits(why)) => format!("{said}\n· {why}"),
        Some(Verdict::Refuse(_)) => unreachable!("a refused project is not registered"),
        None if front.is_some() => format!(
            "{said}; the running Symphony predates the registry: `just symphony --stop`, then \
             `just symphony`, serves every project"
        ),
        None => format!("{said}; `just symphony` starts the Symphony"),
    })
}

/// Where a server started before the machine folder recorded itself, for the project at `dir`:
/// [`INSTALL`] in its repository's git common dir.
fn older_record_folder(dir: &Path) -> Option<PathBuf> {
    symphony_board::git::common_dir(dir)
        .ok()
        .map(|common| common.join(INSTALL))
}

/// What the server the machine folder `machine` records says it is, when it answers; with no
/// record there, the server `older` records, one started before the machine folder.
pub fn running(machine: &Path, older: Option<&Path>) -> Option<symphony_api::Front> {
    #[derive(serde::Deserialize)]
    struct Record {
        port: u16,
    }
    let text = match std::fs::read_to_string(machine.join(RECORD)) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::read_to_string(older?.join(RECORD)).ok()?
        }
        Err(_) => return None,
    };
    let record: Record = serde_json::from_str(&text).ok()?;
    let address = SocketAddr::from(([127, 0, 0, 1], record.port));
    let mut stream = TcpStream::connect_timeout(&address, LOOK).ok()?;
    stream.set_read_timeout(Some(LOOK)).ok()?;
    write!(
        stream,
        "GET {} HTTP/1.0\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
        symphony_api::FRONT_PATH,
        record.port
    )
    .ok()?;
    let mut answer = String::new();
    stream.read_to_string(&mut answer).ok()?;
    let (head, body) = answer.split_once("\r\n\r\n")?;
    head.lines()
        .next()
        .is_some_and(|status| status.split_whitespace().nth(1) == Some("200"))
        .then(|| serde_json::from_str(body).ok())
        .flatten()
}

#[cfg(test)]
#[path = "command_tests.rs"]
mod tests;
