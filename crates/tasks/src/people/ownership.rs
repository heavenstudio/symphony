//! Which group a card or a task belongs to (docs/product/analysis/users-and-identity.md, "Which
//! group a thing belongs to", step 4 of "The way from one owner"): the place it is in, unless a
//! name is written on it.
//!
//! The board keeps a map of places to groups ([`Map`]): a folder of documents, a task's area, the
//! Settings page and the board's default ([`Place`]). An admin keeps it, each change a line of the
//! board's `ownership` log ([`Line`]), whose newest line for a place says its group. A name written
//! on a thing ([`owning`]'s `written`: a card's `*For:*`, a task's `fields._board.group`) overrides
//! the map when the board holds a group or a person by it, or it is `owner`, the person at the
//! machine; a name it holds nothing by is read as none, and the map decides. People and groups
//! share one name space on a board ([`super::refused_name`]), so a name is a group's or a person's
//! by the board's rows alone. A place the map does not name, or maps to a group the board keeps no
//! more, goes to the default, and with no default mapped to the person at the machine. On a board
//! with nobody named every thing is the person at the machine's, whatever is mapped or written.
//!
//! Any member of a thing's group hands it to another group or a person ([`may_hand`]): the
//! person a thing is written to, a member of its group, and the person at the machine always. The
//! log keeps who did ([`Did::Hand`]).

use super::People;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// The name of the person at the machine, as a name written on a thing and as the read answers.
pub const MACHINE: &str = "owner";

/// A place a thing is in, which the map gives a group.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Place {
    /// A folder of documents, by the route a link names it (`docs/design`): a card's document in
    /// it or in a folder under it, the deepest mapped one deciding.
    Folder(String),
    /// A task's area, its slug's first part (`app`).
    Area(String),
    /// The Settings page.
    Settings,
    /// Every place the map does not name.
    Default,
}

impl Place {
    /// The place `text` names: `folder:ROUTE`, `area:NAME`, `settings` or `default`.
    ///
    /// # Errors
    ///
    /// When it names none, saying the forms there are.
    pub fn parse(text: &str) -> Result<Place, String> {
        let text = text.trim();
        let place = match text.split_once(':') {
            Some(("folder", route)) => folder(route).map(Place::Folder),
            Some(("area", area)) => is_area(area)
                .then(|| Place::Area(area.to_string()))
                .ok_or_else(|| {
                    format!(
                        "{area:?} is no task's area: lowercase letters, digits and hyphens, as a \
                         slug's first part"
                    )
                }),
            None if text == "settings" => Ok(Place::Settings),
            None if text == "default" => Ok(Place::Default),
            _ => Err(String::new()),
        };
        place.map_err(|why| {
            let forms = "folder:ROUTE (a folder of documents, as docs/design), area:NAME (a \
                         task's area, as app), settings or default";
            if why.is_empty() {
                format!("{text:?} is no place: one of {forms}")
            } else {
                why
            }
        })
    }

    /// How the board writes it, the form [`Place::parse`] reads.
    pub fn text(&self) -> String {
        match self {
            Place::Folder(route) => format!("folder:{route}"),
            Place::Area(area) => format!("area:{area}"),
            Place::Settings => "settings".into(),
            Place::Default => "default".into(),
        }
    }

    /// How a sentence says it: `the folder docs/design`, `the area app`, `the Settings page`,
    /// `the board's default`.
    pub fn said(&self) -> String {
        match self {
            Place::Folder(route) => format!("the folder {route}"),
            Place::Area(area) => format!("the area {area}"),
            Place::Settings => "the Settings page".into(),
            Place::Default => "the board's default".into(),
        }
    }
}

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text())
    }
}

/// `route` as a folder's: its parts joined by single slashes, none empty, `.` or `..`.
fn folder(route: &str) -> Result<String, String> {
    let parts: Vec<&str> = route.trim().trim_matches('/').split('/').collect();
    let well_formed = parts.iter().all(|part| {
        !part.is_empty() && *part != "." && *part != ".." && !part.contains(char::is_whitespace)
    });
    if well_formed {
        Ok(parts.join("/"))
    } else {
        Err(format!(
            "{route:?} is no folder: a route's folders joined by slashes, as docs/design"
        ))
    }
}

/// Whether `area` has a task area's form: what a slug's first part may be.
fn is_area(area: &str) -> bool {
    !area.is_empty()
        && area.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && (area.bytes()).all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A thing a group owns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Thing {
    /// A card, by its slug, one in the project's documents, in the document of `route`.
    Card { slug: String, route: String },
    /// A task, by its slug.
    Task(String),
}

impl Thing {
    /// How the log writes it: `card SLUG` or `task SLUG`.
    pub fn text(&self) -> String {
        match self {
            Thing::Card { slug, .. } => format!("card {slug}"),
            Thing::Task(slug) => format!("task {slug}"),
        }
    }

    /// The places it is in, the one deciding first: a card's document's folders, deepest first;
    /// a task's area. The default comes after them all ([`owning`]).
    pub fn places(&self) -> Vec<Place> {
        match self {
            Thing::Card { route, .. } => {
                let parts: Vec<&str> = (route.trim_matches('/').split('/'))
                    .filter(|part| !part.is_empty())
                    .collect();
                (1..parts.len())
                    .rev()
                    .map(|depth| Place::Folder(parts[..depth].join("/")))
                    .collect()
            }
            Thing::Task(slug) => (slug.split_once('/'))
                .map(|(area, _)| area)
                .filter(|area| is_area(area))
                .map(|area| vec![Place::Area(area.to_string())])
                .unwrap_or_default(),
        }
    }
}

/// What a line of the ownership log did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Did {
    /// A place given a group.
    Map,
    /// A place taken off the map: its things go to the default.
    Unmap,
    /// A thing handed to another group or a person.
    Hand,
}

impl Did {
    /// How the log writes it.
    pub fn word(self) -> &'static str {
        match self {
            Did::Map => "map",
            Did::Unmap => "unmap",
            Did::Hand => "hand",
        }
    }
}

/// A line of the board's ownership log, the row of `ownership` numbered `n`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Line {
    /// Its number, from 1.
    pub n: i64,
    /// When, in seconds since the epoch, by the store's clock.
    pub at: f64,
    /// Who made it: a map's session, a hand-over's person.
    pub by: String,
    pub did: Did,
    /// The place a map or an unmap names, as [`Place::text`] writes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    /// The thing a hand-over names, as [`Thing::text`] writes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thing: Option<String>,
    /// The group a map gives, or the group or person a thing is handed to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Whose a handed thing was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

impl Line {
    /// What it says, after its time and number: `ana: hand task app/x design (was engineering)`.
    pub fn said(&self) -> String {
        let mut said = format!("{}: {}", self.by, self.did.word());
        for part in [&self.place, &self.thing, &self.name].into_iter().flatten() {
            said.push(' ');
            said.push_str(part);
        }
        if let Some(from) = &self.from {
            said.push_str(&format!(" (was {from})"));
        }
        said
    }
}

/// The board's map of places to groups, as its log's lines left it, and its newest line's number.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Map {
    groups: BTreeMap<Place, String>,
    /// The number of the newest line of the log: 0 on a board with none.
    pub revision: i64,
}

impl Map {
    /// The map `lines` leave, oldest first: each place's newest map or unmap. A line naming no
    /// place this build reads is passed over.
    pub fn of(lines: &[Line]) -> Map {
        let mut map = Map::default();
        for line in lines {
            map.revision = map.revision.max(line.n);
            let Some(place) = (line.place.as_deref()).and_then(|place| Place::parse(place).ok())
            else {
                continue;
            };
            match (line.did, &line.name) {
                (Did::Map, Some(group)) => {
                    map.groups.insert(place, group.clone());
                }
                (Did::Unmap, _) => {
                    map.groups.remove(&place);
                }
                _ => {}
            }
        }
        map
    }

    /// The group `place` maps to.
    pub fn group(&self, place: &Place) -> Option<&str> {
        self.groups.get(place).map(String::as_str)
    }

    /// Each mapped place and its group, in the places' order.
    pub fn entries(&self) -> impl Iterator<Item = (&Place, &str)> {
        self.groups
            .iter()
            .map(|(place, group)| (place, group.as_str()))
    }

    /// Whether it maps nothing.
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
}

/// What owns a thing: a group, one person, or the person at the machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Group,
    Person,
    Machine,
}

/// What gave a thing its owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum By {
    /// The name written on it.
    Named,
    /// The map, by the place [`Owning::place`] names.
    Map,
    /// The board's default: its default group, or the person at the machine.
    Default,
}

/// Whose a thing is, and what said so.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Owning {
    /// The group's or the person's short name, or [`MACHINE`].
    pub name: String,
    pub kind: Kind,
    pub by: By,
    /// The place the map gave it by, as [`Place::text`] writes it: where [`By::Map`], and for
    /// [`By::Default`] the default where the map names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    /// A name written on it that the board holds nothing by, so the map decided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unheld: Option<String>,
}

impl Owning {
    fn machine(by: By) -> Owning {
        Owning {
            name: MACHINE.into(),
            kind: Kind::Machine,
            by,
            place: None,
            unheld: None,
        }
    }

    /// What a page or a command says of what gave it: `named on it`, `from the area app`, `the
    /// board's default`, and the name written on it the board does not hold.
    pub fn said(&self) -> String {
        let place = (self.place.as_deref()).and_then(|place| Place::parse(place).ok());
        let mut said = match (self.by, place) {
            (By::Named, _) => "named on it".to_string(),
            (By::Map, Some(place)) => format!("from {}", place.said()),
            (By::Map, None) => "from the map".to_string(),
            (By::Default, Some(_)) => "the board's default group".to_string(),
            (By::Default, None) => "the board's default".to_string(),
        };
        if let Some(unheld) = &self.unheld {
            said.push_str(&format!(
                ", as this board holds no group or person {unheld}"
            ));
        }
        said
    }
}

/// What a name written on a thing names on a board of `people`: the person at the machine for
/// `owner`, a group it keeps, a person on it; none for a name it holds nothing by.
pub fn named(people: &People, name: &str) -> Option<Kind> {
    if name == MACHINE {
        Some(Kind::Machine)
    } else if people.group(name).is_some() {
        Some(Kind::Group)
    } else if people.member(name).is_some() {
        Some(Kind::Person)
    } else {
        None
    }
}

/// Whose a thing in `places` (the deciding first, [`Thing::places`]) is on a board of `people`
/// with the map `map`, `written` the name written on it. On a board with nobody named, the person
/// at the machine's. Else the name written on it, where the board holds it; else the group the
/// deepest of its places maps to, where the board keeps it; else the default group, where one is
/// mapped and kept; else the person at the machine.
pub fn owning(people: &People, map: &Map, written: Option<&str>, places: &[Place]) -> Owning {
    if people.members.is_empty() {
        return Owning::machine(By::Default);
    }
    let written = written.map(str::trim).filter(|name| !name.is_empty());
    let mut unheld = None;
    if let Some(name) = written {
        match named(people, name) {
            Some(kind) => {
                return Owning {
                    name: name.into(),
                    kind,
                    by: By::Named,
                    place: None,
                    unheld: None,
                };
            }
            None => unheld = Some(name.to_string()),
        }
    }
    let kept = |place: &Place| {
        (map.group(place))
            .filter(|group| people.group(group).is_some())
            .map(|group| (group.to_string(), place.text()))
    };
    let mapped = (places.iter())
        .find_map(|place| kept(place).map(|found| (found, By::Map)))
        .or_else(|| kept(&Place::Default).map(|found| (found, By::Default)));
    let mut owning = match mapped {
        Some(((name, place), by)) => Owning {
            name,
            kind: Kind::Group,
            by,
            place: Some(place),
            unheld: None,
        },
        None => Owning::machine(By::Default),
    };
    owning.unheld = unheld;
    owning
}

/// Who is told of a thing `owning` says whose it is on a board of `people`, by short name, as a
/// notice reaches each by their profile's way (docs/engineering/NOTICES.md): each member of its
/// group, in the board's order; the person it is written to, while the board holds them; nobody for
/// the person at the machine, whom no profile names, so a board that names nobody tells no one.
pub fn told(people: &People, owning: &Owning) -> Vec<String> {
    match owning.kind {
        Kind::Group => (people.members.iter())
            .filter(|member| member.groups.contains(&owning.name))
            .map(|member| member.name.clone())
            .collect(),
        Kind::Person => (people.member(&owning.name))
            .map(|member| vec![member.name.clone()])
            .unwrap_or_default(),
        Kind::Machine => Vec::new(),
    }
}

/// Whether `actor` may hand a thing `owning` says whose it is on a board of `people`: the person
/// at the machine always (`machine`), the person it is written to, and a member of its group.
pub fn may_hand(people: &People, owning: &Owning, actor: &str, machine: bool) -> bool {
    if machine {
        return true;
    }
    match owning.kind {
        Kind::Machine => false,
        Kind::Person => owning.name == actor,
        Kind::Group => people
            .member(actor)
            .is_some_and(|member| member.groups.contains(&owning.name)),
    }
}

/// Why `actor` may not hand a thing `owning` says whose it is to `to` on a board of `people`
/// (`machine`: the actor is the person at the machine): none where they may. Refused: a board
/// with nobody named, where every thing is the person at the machine's; a name the board holds
/// nothing by; an actor who is no member of its group, or not the person it is written for.
pub fn refused_hand(
    people: &People,
    owning: &Owning,
    to: &str,
    actor: &str,
    machine: bool,
) -> Option<String> {
    if people.members.is_empty() {
        return Some(
            "this board names nobody, so every thing is the person at the machine's and none is \
             handed"
                .into(),
        );
    }
    if named(people, to).is_none() {
        return Some(format!(
            "this board holds no group or person {to} to hand it to"
        ));
    }
    if !may_hand(people, owning, actor, machine) {
        let name = &owning.name;
        return Some(match owning.kind {
            Kind::Group => {
                format!(
                    "{actor} is no member of the group {name}, whose it is, so may not hand it on"
                )
            }
            Kind::Person => format!(
                "it is {name}'s, so {actor} may not hand it on: only {name} or the person at the \
                 machine does"
            ),
            Kind::Machine => format!(
                "it is the person at the machine's, so {actor} may not hand it on: only the person \
                 at the machine does"
            ),
        });
    }
    None
}

#[cfg(test)]
#[path = "ownership_tests.rs"]
mod tests;
