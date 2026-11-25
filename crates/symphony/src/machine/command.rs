//! `symphony project add [DIR]` (and `symphony-server project add`, which `just symphony` runs as
//! its start registers): the project of DIR's checkout written into the machine folder's registry
//! (its main checkout, its id and its `symphony.toml`), and what the host rule says of the running
//! server ([`super::host`]): it serves the project, or it lacks the project's plugins and that
//! project's `just symphony` replaces it, or the project's pin is newer and someone must bump.
//! A compatible host is called serving only once its board JSON answers; registration makes and
//! migrates no board. Run
//! in a linked worktree, it registers no project of that worktree's own repository, naming the main
//! checkout; another repository's project it registers as it would from anywhere else.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::host::{Build, Verdict, judge};
use super::{Added, Entry, Registry};

const USAGE: &str = "usage: symphony project add [DIR]";
/// The running server's record in the machine folder, as its starters write it.
pub(crate) const RECORD: &str = "server.json";
/// A project's Symphony install in its git common dir, which also held the record of a server
/// started before the machine folder.
const INSTALL: &str = symphony_board::folders::SERVER;
/// How long the running server is given to say what it is.
pub(crate) const LOOK: Duration = Duration::from_secs(2);

/// `symphony project ARGS`: the exit code.
pub fn main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let dir = match args {
        [add] if add == "add" => ".",
        [add, dir] if add == "add" && !dir.starts_with('-') => dir.as_str(),
        _ => return crate::cli::refuse(err, USAGE, "project", &misnamed(args)),
    };
    let here = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let added = super::dir().and_then(|machine| {
        let front = running(&machine, older_record_folder(Path::new(dir)).as_deref());
        add(Path::new(dir), &here, &machine, front.as_ref())
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

/// Register the project at `dir` (its checkout's main) in `machine`, by the build run in the folder
/// `from`, judged by the host rule against `front`, the running server's status: what to say. A
/// server from before pins is never judged; with none running the project is registered for the
/// next start.
///
/// # Errors
///
/// When `from` is in a linked worktree of the repository `dir` is in ([`own_worktree`]), the
/// project does not load, the registry
/// cannot be read or written, or the host rule refuses it (its pin or board schema is newer than the
/// running server's, which this build cannot replace): a refused project is not registered.
pub fn add(
    dir: &Path,
    from: &Path,
    machine: &Path,
    front: Option<&symphony_api::Front>,
) -> Result<String, String> {
    add_within(dir, from, machine, front, LOOK)
}

/// [`add`], with each HTTP probe given `look` to connect and answer.
fn add_within(
    dir: &Path,
    from: &Path,
    machine: &Path,
    front: Option<&symphony_api::Front>,
    look: Duration,
) -> Result<String, String> {
    if let Some(refusal) = own_worktree(dir, from) {
        return Err(refusal);
    }
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
        Some(Verdict::Serves) => {
            match board_readiness_within(front.expect("a judged host"), &entry.id, look) {
                Ok(()) => format!("{said}; the running Symphony serves it"),
                Err(why) => format!("{said}\n· {why}"),
            }
        }
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

/// Why `project add` of `dir` run in the folder `from` registers nothing: `from` is in a linked
/// worktree of the repository `dir` is in, where a branch is checked out, and its project would be
/// registered by that worktree's build, its pin and board schema standing for the project's on the
/// machine. None for any other pair: from a linked worktree, another repository's project is
/// registered as from anywhere else.
fn own_worktree(dir: &Path, from: &Path) -> Option<String> {
    let worktree = super::host::worktree_of(from)?;
    let common = symphony_board::git::checkout_common_dir(&worktree)?;
    let theirs = symphony_board::git::common_dir(dir)
        .ok()?
        .canonicalize()
        .ok()?;
    if theirs != common {
        return None;
    }
    let main = main_checkout(&worktree);
    let instead = if main == worktree {
        // A bare repository's, or one whose git dir lies apart from its main checkout, which git
        // cannot find from the git dir.
        format!(
            "its repository's git dir {} names no main checkout: run `project add` in that \
             repository's main checkout, if it has one",
            common.display()
        )
    } else {
        format!("run `project add` in {}, the main checkout", main.display())
    };
    Some(format!(
        "this runs in the linked worktree {}, and a linked worktree registers no project of its own \
         repository with the machine's Symphony: {instead}",
        worktree.display()
    ))
}

/// Where a server started before the machine folder recorded itself, for the project at `dir`:
/// [`INSTALL`] in its repository's git common dir.
fn older_record_folder(dir: &Path) -> Option<PathBuf> {
    symphony_board::git::common_dir(dir)
        .ok()
        .map(|common| common.join(INSTALL))
}

/// What the server the machine folder `machine` records says it is, when it answers within
/// [`LOOK`]; with no record there, the server `older` records, one started before the machine
/// folder.
pub fn running(machine: &Path, older: Option<&Path>) -> Option<symphony_api::Front> {
    running_within(machine, older, LOOK)
}

/// [`running`], the server given `look` to connect and to answer.
pub(crate) fn running_within(
    machine: &Path,
    older: Option<&Path>,
    look: Duration,
) -> Option<symphony_api::Front> {
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
    let record: Record = serde_json::from_slice(text.as_bytes()).ok()?;
    let (status, body) = get(record.port, symphony_api::FRONT_PATH, look).ok()?;
    (status == 200)
        .then(|| serde_json::from_slice(body.as_bytes()).ok())
        .flatten()
}

/// Whether the server this machine's folder `machine` records writes each project's board, as its
/// project list says (`/api/projects`, each project's `writes`): by id, `Some(true)` while it writes
/// that board, `Some(false)` while it only reads or refuses it, and `None` while it cannot say (it
/// keeps no board, has not read it yet, or its last read failed). A project it lists with no
/// `writes` at all, from a server built before it said, is left out. None when no server answers
/// there.
pub fn writes(machine: &Path) -> Option<Vec<(String, Option<bool>)>> {
    #[derive(serde::Deserialize)]
    struct Record {
        port: u16,
    }
    let text = std::fs::read_to_string(machine.join(RECORD)).ok()?;
    let record: Record = serde_json::from_slice(text.as_bytes()).ok()?;
    let (status, body) = get(record.port, symphony_api::listing::PROJECTS_PATH, LOOK).ok()?;
    if status != 200 {
        return None;
    }
    let list: serde_json::Value = serde_json::from_slice(body.as_bytes()).ok()?;
    let projects = list.get("projects")?.as_array()?;
    Some(
        (projects.iter())
            .filter_map(|project| {
                let id = project.get("id")?.as_str()?.to_string();
                Some((id, project.get("writes")?.as_bool()))
            })
            .collect(),
    )
}

/// Check the board's JSON, never the HTML shell, without writing or migrating it. A refusal keeps
/// the API's actionable reason. A repaired board may retain that reason until its next retry.
pub(crate) fn board_readiness_within(
    front: &symphony_api::Front,
    id: &str,
    look: Duration,
) -> Result<(), String> {
    let path = format!("/{id}/api/task-board?counts=1&work=0");
    let url = format!("http://127.0.0.1:{}{path}", front.port);
    let checked = get(front.port, &path, look).and_then(|(status, body)| {
        if status != 200 {
            let why = serde_json::from_slice::<serde_json::Value>(body.as_bytes())
                .ok()
                .and_then(|value| value["error"].as_str().map(str::to_string))
                .unwrap_or_else(|| "no board JSON answer".into());
            return Err(format!("HTTP {status}: {why}"));
        }
        serde_json::from_slice::<symphony_api::TaskListing>(body.as_bytes())
            .map(|_| ())
            .map_err(|_| "HTTP 200 did not contain task-board JSON".to_string())
    });
    checked.map_err(|why| {
        format!(
            "board readiness at {url} failed: {why}; the running host requires board schema {}. \
             If the board is behind, first link a board command compatible with that schema, \
             then run `just tasks migrate` in the project's main checkout; registration and \
             readers never migrate it. After repair the running \
             server retries within 60 seconds on its next board tick or request; check this JSON \
             URL again",
            front.schema
        )
    })
}

/// A bounded, local HTTP/1.0 read: asking for close avoids chunked transfer encoding.
fn get(port: u16, path: &str, look: Duration) -> Result<(u16, String), String> {
    let read = || -> std::io::Result<String> {
        let address = SocketAddr::from(([127, 0, 0, 1], port));
        let mut stream = TcpStream::connect_timeout(&address, look)?;
        stream.set_read_timeout(Some(look))?;
        stream.set_write_timeout(Some(look))?;
        write!(
            stream,
            "GET {path} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
        )?;
        let mut answer = String::new();
        stream.read_to_string(&mut answer)?;
        Ok(answer)
    };
    let answer = read().map_err(|error| error.to_string())?;
    let (head, body) = answer
        .split_once("\r\n\r\n")
        .ok_or("incomplete HTTP answer")?;
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse().ok())
        .ok_or("invalid HTTP status")?;
    Ok((status, body.to_string()))
}

#[cfg(test)]
#[path = "command_tests.rs"]
mod tests;
