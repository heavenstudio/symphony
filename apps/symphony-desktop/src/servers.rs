//! The servers the app knows: this Mac's, found by itself from the machine folder's record of its
//! running server, and the others added by their address, kept in the app's own list file. The
//! machine folder is only read; the list file is the one file the app writes.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::address::Server;
use crate::read::Answer;

/// The app's list of the servers added to it, in its settings folder.
pub const LIST: &str = "servers.json";
/// The running server's record in the machine folder (`symphony project add` reads it the same
/// way): its port is this Mac's server.
const RECORD: &str = "server.json";
/// Names the machine folder in place of the platform's, as the server's own tests name theirs
/// (`symphony_board::project::MACHINE_DIR`).
pub const MACHINE_DIR: &str = "SYMPHONY_MACHINE_DIR";
/// Names the app's settings folder in place of the platform's: a test's own.
pub const SETTINGS_DIR: &str = "SYMPHONY_DESKTOP_DIR";

/// A server added to the app.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Saved {
    /// Its address.
    pub server: Server,
    /// What the app calls it.
    pub name: String,
    /// The installation id it answered when it was added: one server reached two ways is listed
    /// once.
    pub installation: Option<String>,
}

/// The servers added to the app, in the order they were added.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct List {
    pub servers: Vec<Saved>,
}

impl List {
    /// The list in `folder`: none when it has no file yet. A file that does not read as a list is
    /// an error naming it, and is kept as it is.
    pub fn read(folder: &Path) -> Result<List, String> {
        let path = folder.join(LIST);
        match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(|error| {
                format!(
                    "{} does not read as the app's list of servers ({error}): it is kept as it is, and nothing is saved until it reads",
                    path.display()
                )
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(List::default()),
            Err(error) => Err(format!("{} cannot be read: {error}", path.display())),
        }
    }

    /// The list written whole into `folder`, made if need be: a file beside it renamed over the
    /// old, so a reader never meets half a list. A file there that does not read as a list is never
    /// written over.
    pub fn write(&self, folder: &Path) -> Result<(), String> {
        List::read(folder)?;
        let place = |error: io::Error| format!("{} cannot be written: {error}", folder.display());
        fs::create_dir_all(folder).map_err(place)?;
        let text = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
        let pending = folder.join(format!(".{LIST}.{}", std::process::id()));
        let written = fs::File::create(&pending).and_then(|mut file| {
            file.write_all(text.as_bytes())?;
            file.write_all(b"\n")?;
            file.sync_all()
        });
        let renamed = written.and_then(|()| fs::rename(&pending, folder.join(LIST)));
        if renamed.is_err() {
            let _ = fs::remove_file(&pending);
        }
        renamed.map_err(place)
    }
}

/// Where the app's settings and the machine folder are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Places {
    /// The app's own folder, which holds [`LIST`].
    pub settings: PathBuf,
    /// The machine folder, read for this Mac's server alone; none when it cannot be named.
    pub machine: Option<PathBuf>,
}

impl Places {
    /// The places `var` names ([`SETTINGS_DIR`], [`MACHINE_DIR`], `HOME`), each an absolute folder
    /// or unset: else the platform's, `~/Library/Application Support/Symphony Desktop/` and
    /// `~/Library/Application Support/Symphony/`.
    pub fn from(var: impl Fn(&str) -> Option<String>) -> Result<Places, String> {
        let named = |name: &str| {
            var(name)
                .filter(|value| !value.is_empty())
                .map(|value| {
                    let path = PathBuf::from(&value);
                    if path.is_absolute() {
                        Ok(path)
                    } else {
                        Err(format!("{name} names {value}, not an absolute folder"))
                    }
                })
                .transpose()
        };
        let support = named("HOME")?.map(|home| home.join("Library/Application Support"));
        let settings = match named(SETTINGS_DIR)? {
            Some(folder) => folder,
            None => support
                .as_ref()
                .map(|support| support.join("Symphony Desktop"))
                .ok_or_else(|| format!("neither {SETTINGS_DIR} nor HOME names a folder"))?,
        };
        let machine =
            named(MACHINE_DIR)?.or_else(|| support.map(|support| support.join("Symphony")));
        Ok(Places { settings, machine })
    }
}

/// This Mac's server, as the machine folder's record of the running one names its port; none when
/// no server has run here.
pub fn this_mac(machine: &Path) -> Option<Server> {
    #[derive(Deserialize)]
    struct Record {
        port: u16,
    }
    let text = fs::read_to_string(machine.join(RECORD)).ok()?;
    let record: Record = serde_json::from_str(&text).ok()?;
    Server::parse(&format!("http://127.0.0.1:{}", record.port)).ok()
}

/// The server at `pasted` added to the list in `settings`, once `read` finds a Symphony there:
/// refused when the rule refuses its address, when it is already listed (by its address, or by the
/// installation id it answers, this Mac's `here` among them) or when it answers no list.
pub fn add(
    settings: &Path,
    pasted: &str,
    here: Option<&Server>,
    read: impl Fn(&Server) -> Answer,
) -> Result<Saved, String> {
    let server = Server::parse(pasted).map_err(|refused| refused.to_string())?;
    let mut list = List::read(settings)?;
    if let Some(saved) = list.servers.iter().find(|saved| saved.server == server) {
        return Err(format!("{server} is listed already, as {}", saved.name));
    }
    if here == Some(&server) {
        return Err(format!(
            "{server} is this Mac's: its server is found by itself, from the machine folder"
        ));
    }
    let installation = match read(&server) {
        Answer::Listed(listed) => listed.installation,
        Answer::Older => {
            return Err(format!(
                "{server} has no project list: it needs a newer Symphony to be added"
            ));
        }
        Answer::Lost(why) => return Err(format!("{why}: nothing is added")),
    };
    if let Some(id) = &installation {
        if let Some(saved) = list
            .servers
            .iter()
            .find(|saved| saved.installation.as_ref() == Some(id))
        {
            return Err(format!(
                "{server} is {}, listed already at {}: one server is listed once",
                saved.name, saved.server
            ));
        }
        if let Some(here) = here
            && let Answer::Listed(mine) = read(here)
            && mine.installation.as_ref() == Some(id)
        {
            return Err(format!(
                "{server} is this Mac's own server, reached by another name: it is listed once"
            ));
        }
    }
    let saved = Saved {
        name: server.default_name(),
        server,
        installation,
    };
    list.servers.push(saved.clone());
    list.write(settings)?;
    Ok(saved)
}

#[cfg(test)]
#[path = "servers_tests.rs"]
mod tests;
