//! A board's people, groups and powers, and the machine's profiles of the people its boards name
//! (docs/product/analysis/users-and-identity.md, "Where people are kept" and "The way from one
//! owner", step 3).
//!
//! A person's profile is kept once, with the server: `people.json` in the machine folder
//! ([`crate::project::machine_folder`]), their short name, how they are shown, the sign-ins that
//! are theirs and their git handles, with what they gave of emails, chat and preferences kept as
//! written. It holds sign-ins and contact details, so it and its lock are written mode 600, and
//! nothing a board or a log keeps names a sign-in's subject or an email: they name the short name.
//!
//! Each board keeps who is on it, in the tables of [`crate::schema::PEOPLE`]: a row a person (the
//! groups they are in and the powers given them), a row a group (its powers), and the log of every
//! change, numbered from 1 ([`Change`]); the newest number is the board's people revision, which
//! every change is fenced on ([`crate::Store::change_people`]). People and groups share one name
//! space ([`refused_name`]).
//!
//! What a person may do there is their own powers and their groups' ([`People::powers_of`]): every
//! power includes read, and one who holds none is not on the board. A group owns cards and tasks
//! ([`ownership`]), which its members hand on, but its powers still hold on the whole board: who
//! moves a group's task and whose answer settles are later parts of step 4. The person at the
//! machine is on every board with every power, and no row says so ([`Powers::all`]).

pub mod change;
pub mod ownership;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;
use std::path::Path;

/// The machine's profiles, in the machine folder.
pub const FILE: &str = "people.json";
/// The lock a change of [`FILE`] is made under, beside it.
pub const LOCK: &str = "people.lock";
/// The mode [`FILE`] and [`LOCK`] are written with: the machine's account alone reads them.
pub const MODE: u32 = 0o600;
/// The service of a Tailscale login's sign-in, its subject the login's name
/// (the server's `gate::Login`).
pub const TAILSCALE: &str = "tailscale";

/// What a person may do on a board.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Power {
    /// See the board and its project: every power includes it.
    Read,
    /// Write on its threads and upload.
    Speak,
    /// Give a verdict and its note, move a task and change its fields, and change the board's
    /// settings: the approve that meets the `owner` stamp among them.
    Decide,
    /// Have their words reach the board's agents, and control the agents' attempts. Given to no
    /// one yet: every grant of it is refused until the board holds the words of someone who may
    /// not direct agents (step 5 of docs/product/analysis/users-and-identity.md), so the routes
    /// that need it stay the person at the machine's.
    DirectAgents,
    /// Say who is on the board, in which groups and with what powers: the People page's changes
    /// need it, and the command line is the terminal's.
    Administer,
}

impl Power {
    /// Every power, in the order they are said.
    pub const ALL: [Power; 5] = [
        Power::Read,
        Power::Speak,
        Power::Decide,
        Power::DirectAgents,
        Power::Administer,
    ];

    /// How the board writes it: `read`, `speak`, `decide`, `direct_agents`, `administer`.
    pub fn word(self) -> &'static str {
        match self {
            Power::Read => "read",
            Power::Speak => "speak",
            Power::Decide => "decide",
            Power::DirectAgents => "direct_agents",
            Power::Administer => "administer",
        }
    }

    /// How a sentence says it: `direct agents` for [`Power::DirectAgents`], else its word.
    pub fn said(self) -> &'static str {
        match self {
            Power::DirectAgents => "direct agents",
            other => other.word(),
        }
    }

    /// Why no one is given it yet, where no one is: direct agents, until a board speaks to its
    /// agents for a person (step 5 of docs/product/analysis/users-and-identity.md), so every
    /// route that asks them for work stays the person at the machine's. Every grant of it is
    /// refused in these words, and the People page draws it as not yet givable with them.
    pub fn withheld(self) -> Option<&'static str> {
        match self {
            Power::DirectAgents => Some(
                "until a board speaks to its agents for a person, asking them for work is the \
                 person at the machine's",
            ),
            Power::Read | Power::Speak | Power::Decide | Power::Administer => None,
        }
    }

    /// The power `word` names, its word or with a hyphen for the underscore (`direct-agents`).
    ///
    /// # Errors
    ///
    /// When it names none, saying the ones there are.
    pub fn parse(word: &str) -> Result<Power, String> {
        let word = word.trim().replace('-', "_");
        Power::ALL
            .into_iter()
            .find(|power| power.word() == word)
            .ok_or_else(|| {
                let words: Vec<&str> = Power::ALL.iter().map(|power| power.word()).collect();
                format!("{word:?} is no power: one of {}", words.join(", "))
            })
    }
}

impl fmt::Display for Power {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

/// The powers one holds on a board, theirs and their groups'.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Powers(BTreeSet<Power>);

impl Powers {
    /// Every power: the person at the machine's, on every board.
    pub fn all() -> Powers {
        Powers(Power::ALL.into_iter().collect())
    }

    /// Exactly `powers`.
    pub fn of(powers: impl IntoIterator<Item = Power>) -> Powers {
        Powers(powers.into_iter().collect())
    }

    /// Whether these hold `power`: read is held with any power.
    pub fn holds(&self, power: Power) -> bool {
        self.0.contains(&power) || (power == Power::Read && self.any())
    }

    /// Whether these hold any power, which puts one on the board.
    pub fn any(&self) -> bool {
        !self.0.is_empty()
    }

    /// The first of `needs` these do not hold: none where they hold every one.
    pub fn lacking(&self, needs: &[Power]) -> Option<Power> {
        needs.iter().copied().find(|power| !self.holds(*power))
    }

    /// The powers held, read among them where any is, in the order they are said.
    pub fn held(&self) -> Vec<Power> {
        (Power::ALL.into_iter())
            .filter(|power| self.holds(*power))
            .collect()
    }
}

/// A person on a board: the row of `people` named by their short name.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub name: String,
    /// The groups they are in, each a row of `people_group` while it is kept.
    #[serde(default)]
    pub groups: Vec<String>,
    /// The powers given them, beside their groups'.
    #[serde(default)]
    pub powers: Vec<Power>,
    /// The number of the change that last wrote the row ([`Change::n`]).
    #[serde(default)]
    pub revision: i64,
}

/// A group of a board: the row of `people_group` named by its short name.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub name: String,
    /// The powers its members hold by it.
    #[serde(default)]
    pub powers: Vec<Power>,
    /// The number of the change that last wrote the row ([`Change::n`]).
    #[serde(default)]
    pub revision: i64,
}

/// What a change of a board's people did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Did {
    /// A person put on the board.
    Add,
    /// A person taken off it, their row gone.
    Remove,
    /// A person put in a group.
    Join,
    /// A person taken out of one.
    Leave,
    /// Powers given a person or a group.
    Grant,
    /// Powers taken from one.
    Revoke,
    /// A group made.
    GroupAdd,
    /// A group removed, its members taken out of it.
    GroupRemove,
}

impl Did {
    /// How the log writes it.
    pub fn word(self) -> &'static str {
        match self {
            Did::Add => "add",
            Did::Remove => "remove",
            Did::Join => "join",
            Did::Leave => "leave",
            Did::Grant => "grant",
            Did::Revoke => "revoke",
            Did::GroupAdd => "group-add",
            Did::GroupRemove => "group-remove",
        }
    }
}

/// Whose own powers a grant or a revoke changes ([`crate::Store::grant`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holder<'a> {
    Person(&'a str),
    Group(&'a str),
}

/// A line of a board's people log, the row of `people_log` numbered `n`: who changed what, by
/// short names alone.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    /// Its number, from 1: the board's people revision once it is written.
    pub n: i64,
    /// When, in seconds since the epoch, by the store's clock.
    pub at: f64,
    /// Who made it: the session whose command it was.
    pub by: String,
    pub did: Did,
    /// The person, or the group of [`Did::GroupAdd`], [`Did::GroupRemove`] and a group's grant
    /// or revoke.
    pub name: String,
    /// The group a person joined or left.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// The powers given or taken, and those an add or a group's add gave.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub powers: Vec<Power>,
}

/// Who is on a board: its people and groups as one read found them, and its people revision then.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct People {
    pub members: Vec<Member>,
    pub groups: Vec<Group>,
    /// The number of the newest change ([`Change::n`]): 0 on a board none was made on.
    pub revision: i64,
}

impl People {
    /// The person named `name` on the board.
    pub fn member(&self, name: &str) -> Option<&Member> {
        self.members.iter().find(|member| member.name == name)
    }

    /// The group named `name` on the board.
    pub fn group(&self, name: &str) -> Option<&Group> {
        self.groups.iter().find(|group| group.name == name)
    }

    /// What the person named `name` may do on the board: their own powers and those of each of
    /// their groups the board keeps; no power for a name it holds no person by.
    pub fn powers_of(&self, name: &str) -> Powers {
        let Some(member) = self.member(name) else {
            return Powers::default();
        };
        let by_groups = (member.groups.iter())
            .filter_map(|group| self.group(group))
            .flat_map(|group| group.powers.iter().copied());
        Powers::of(member.powers.iter().copied().chain(by_groups))
    }

    /// The people who hold `power`, by short name.
    pub fn holding(&self, power: Power) -> Vec<String> {
        (self.members.iter())
            .filter(|member| self.powers_of(&member.name).holds(power))
            .map(|member| member.name.clone())
            .collect()
    }
}

/// Why `name` cannot be a person's or a group's, on any board: none where it can. It takes a short
/// name's form ([`symphony_docs::syntax::is_short_name`], which keeps out the board's own words),
/// and is no handle a session registering can be given ([`crate::store::could_be_handle`]). A
/// register and an add of one name at the same instant then never both take it, as each checks the
/// other's names outside the transaction that writes its own: the handles a register gives and the
/// names a person or a group takes never meet.
pub fn refused_person_name(name: &str) -> Option<String> {
    if !symphony_docs::syntax::is_short_name(name) {
        return Some(format!(
            "{name:?} is no short name: lowercase letters and digits in parts joined by single \
             hyphens, starting with a letter, at most {} long, its last part no number alone, and \
             none of {}",
            symphony_docs::syntax::SHORT_NAME_MAX,
            symphony_docs::syntax::RESERVED_NAMES.join(", ")
        ));
    }
    if crate::store::could_be_handle(name) {
        return Some(format!(
            "{name} is a name a session's handle can take, an adjective and a noun the board names \
             sessions by, so a line of either could read as the other's: name a person or a \
             group otherwise"
        ));
    }
    None
}

/// Why `name` cannot name a new person or group on a board that holds `people`, where `handles`
/// are the board's sessions' handles: none where it can. It is a person's name
/// ([`refused_person_name`]), and no person's or group's there already, nor a session's handle.
pub fn refused_name(name: &str, people: &People, handles: &[String]) -> Option<String> {
    if let Some(why) = refused_person_name(name) {
        return Some(why);
    }
    if people.member(name).is_some() {
        return Some(format!("{name} is a person on this board already"));
    }
    if people.group(name).is_some() {
        return Some(format!("{name} is a group of this board"));
    }
    if handles.iter().any(|handle| handle == name) {
        return Some(format!(
            "{name} is a session's handle on this board, and a line of either would read as the \
             other's"
        ));
    }
    None
}

/// A sign-in that is a person's: a service, and the subject it knows them by.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signin {
    pub service: String,
    pub subject: String,
}

impl Signin {
    /// `SERVICE:SUBJECT`, the command line's form of one.
    ///
    /// # Errors
    ///
    /// When either side is empty.
    pub fn parse(text: &str) -> Result<Signin, String> {
        match text.split_once(':') {
            Some((service, subject))
                if !service.trim().is_empty() && !subject.trim().is_empty() =>
            {
                Ok(Signin {
                    service: service.trim().to_ascii_lowercase(),
                    subject: subject.trim().to_string(),
                })
            }
            _ => Err("a sign-in is SERVICE:SUBJECT, as tailscale:ana@example.com".into()),
        }
    }

    /// Whether this is the sign-in `service` gives for `subject`: the service by its word, the
    /// subject as written, both ignoring ASCII case (a login's name is an email, say).
    pub fn is(&self, service: &str, subject: &str) -> bool {
        self.service.eq_ignore_ascii_case(service) && self.subject.eq_ignore_ascii_case(subject)
    }
}

/// A person's profile, kept once for every board the machine serves.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    /// Their short name, the one every board and every line knows them by.
    pub name: String,
    /// How they are shown: their short name where they gave none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub display: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signins: Vec<Signin>,
    /// Their git handles, the first the one a commit of theirs is authored by.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub git: Vec<String>,
    /// Kept as written: nothing reads them yet.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emails: Vec<String>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub chat: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub preferences: Map<String, Value>,
}

impl Profile {
    /// How they are shown: their display name, else their short name.
    pub fn shown(&self) -> &str {
        if self.display.trim().is_empty() {
            &self.name
        } else {
            &self.display
        }
    }
}

/// `people.json`: the machine's profiles, in the order they were made.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profiles {
    #[serde(default)]
    pub people: Vec<Profile>,
}

impl Profiles {
    /// The profiles in the machine folder `dir`; none written is none.
    ///
    /// # Errors
    ///
    /// When the file cannot be read or does not parse: profiles that are not read are never
    /// written over. One that does not parse is said by where and what kind ([`unparsed`]), never
    /// by a value found in it.
    pub fn read(dir: &Path) -> Result<Profiles, String> {
        let path = dir.join(FILE);
        match fs::read_to_string(&path) {
            Ok(text) => parse(&text).map_err(|(field, e)| unparsed(&path, &field, &e)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(Profiles::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// Writes the profiles whole into `dir`, mode [`MODE`], so a reader never sees half of them
    /// and no other account reads them.
    ///
    /// # Errors
    ///
    /// When the folder or the file cannot be written.
    pub fn write(&self, dir: &Path) -> Result<(), String> {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = dir.join(FILE);
        let next = dir.join(format!("{FILE}.{}", std::process::id()));
        let text = serde_json::to_string_pretty(self).expect("profiles serialize") + "\n";
        let written = private(&next).and_then(|mut file| {
            std::io::Write::write_all(&mut file, text.as_bytes())?;
            file.sync_all()?;
            fs::rename(&next, &path)
        });
        if written.is_err() {
            let _ = fs::remove_file(&next);
        }
        written.map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Reads the profiles in `dir`, `change`s them and writes them back, under [`LOCK`]: what
    /// `change` returns. A change that fails, or changes nothing, writes nothing.
    ///
    /// # Errors
    ///
    /// When the profiles cannot be locked, read or written, or `change` fails.
    pub fn change<T>(
        dir: &Path,
        change: impl FnOnce(&mut Profiles) -> Result<T, String>,
    ) -> Result<T, String> {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = dir.join(LOCK);
        let lock = private_lock(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        lock.lock()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let mut profiles = Profiles::read(dir)?;
        let before = profiles.clone();
        let result = change(&mut profiles)?;
        if profiles != before {
            profiles.write(dir)?;
        }
        Ok(result)
    }

    /// The profile named `name`.
    pub fn get(&self, name: &str) -> Option<&Profile> {
        self.people.iter().find(|profile| profile.name == name)
    }

    /// The profile `service` signs in as `subject` ([`Signin::is`]).
    pub fn signed_in(&self, service: &str, subject: &str) -> Option<&Profile> {
        (self.people.iter()).find(|profile| profile.signins.iter().any(|s| s.is(service, subject)))
    }
}

/// `people.json`'s text as profiles; else the field it does not parse at ([`field`]) and why.
fn parse(text: &str) -> Result<Profiles, (String, serde_json::Error)> {
    let mut json = serde_json::Deserializer::from_str(text);
    let profiles = serde_path_to_error::deserialize(&mut json)
        .map_err(|e| (field(e.path()), e.into_inner()))?;
    json.end().map_err(|e| (String::new(), e))?;
    Ok(profiles)
}

/// The names a profile's fields have in `people.json`, the only keys said of a fault in it.
const FIELDS: [&str; 10] = [
    "people",
    "name",
    "display",
    "signins",
    "service",
    "subject",
    "git",
    "emails",
    "chat",
    "preferences",
];

/// The field a fault in `people.json` is at, as `people[0].signins[1].subject`: a profile's own
/// field names and the places in its lists, up to the first key the file chose, which is never
/// said: a field no profile has, or one in `chat` or `preferences`, whose keys are the file's.
fn field(path: &serde_path_to_error::Path) -> String {
    use serde_path_to_error::Segment;
    let mut said = String::new();
    for segment in path {
        match segment {
            Segment::Seq { index } => said.push_str(&format!("[{index}]")),
            Segment::Map { key } if FIELDS.contains(&key.as_str()) => {
                if !said.is_empty() {
                    said.push('.');
                }
                said.push_str(key);
                if key == "chat" || key == "preferences" {
                    break;
                }
            }
            Segment::Map { .. } | Segment::Enum { .. } | Segment::Unknown => break,
        }
    }
    said
}

/// What a `people.json` at `path` that does not parse is said as: its line and column, the
/// `field` the fault is at, and its kind, never a value found in it. serde's own words for a value
/// of the wrong type or form quote it (`invalid type: string "ana@example.com", expected a
/// sequence`), a sign-in's subject among them, and this is said in the server's log and to the
/// person at the machine. A syntax error's words quote nothing (`expected `,` or `}``, `EOF while
/// parsing an object`), and a missing or doubled field's name only one of a profile's own, so those
/// are kept.
fn unparsed(path: &Path, field: &str, error: &serde_json::Error) -> String {
    let said = error.to_string();
    // serde_json ends its words with where; that is said once, before them.
    let said = said.split(" at line ").next().unwrap_or_default();
    let what = match error.classify() {
        serde_json::error::Category::Data
            if !(said.starts_with("missing field `") || said.starts_with("duplicate field `")) =>
        {
            "a value of a type or form a profile does not take"
        }
        _ => said,
    };
    let at = if field.is_empty() {
        String::new()
    } else {
        format!("{field}: ")
    };
    format!(
        "{}: line {}, column {}: {at}{what}, so none of its profiles is read",
        path.display(),
        error.line(),
        error.column()
    )
}

/// A new file at `path` the machine's account alone reads and writes, replacing none.
fn private(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, MODE);
    options.open(path)
}

/// The lock at `path`, made mode [`MODE`] where it is not there, and held to it where it is.
fn private_lock(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, MODE);
    let lock = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        lock.set_permissions(fs::Permissions::from_mode(MODE))?;
    }
    Ok(lock)
}

#[cfg(test)]
#[path = "people_tests.rs"]
mod tests;
