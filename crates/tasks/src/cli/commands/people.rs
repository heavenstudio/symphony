//! `people …`: who is on the board and what each may do there ([`crate::people`],
//! [`crate::Store::add_person`]), and the machine's profiles of them (`people.json` in the
//! machine folder), from the terminal, whose account is the machine's. Every change is recorded
//! as `--as`'s, and nothing it prints or logs names a sign-in's subject, an address or the way a
//! notice goes by: a sign-in is said by its service alone, and `list`, which an agent runs unasked,
//! shows no address and no way.

use super::{Context, Stop};
use crate::Store;
use crate::people::change::{self, Given, Stopped, Writer};
use crate::people::{Change, Holder, People, Power, Profile, Profiles, Signin, ways};
use crate::sys::local_minute;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

/// The recipe a change runs by, which asks first: `symphony.just`'s, which `symphony init`
/// installs.
pub const RECIPE: &str = "just tasks-people";

/// A change to a board's people or a profile, decided from the command line and not made yet.
type Pending<'a> = Box<dyn FnOnce(&mut Context<'_>) -> Result<(), Stop> + 'a>;

/// The two reads, `list` and `log`, run as they are. Every other action is a change, refused
/// under [`crate::confine::MARKER`], which `just tasks` sets as an agent runs it unasked, so a
/// change runs by `just tasks-people`, which no allow rule passes and so asks first. The dispatch
/// decides the action, from the command line as parsed wherever it put the action (after `--as`,
/// an option or a `--`), and only then the guard refuses it: an action added later is refused
/// until it is written as a read, and an unknown one keeps its usage error.
pub(super) fn run(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let action = c.arg("action");
    let words: Vec<String> = c.parsed.all("words").to_vec();
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    let change: Pending<'_> = match (action.as_str(), words.as_slice()) {
        ("list", []) => return list(store, c),
        ("log", []) => return log(store, c),
        ("add", [name]) => Box::new(move |c| add(store, c, name)),
        ("remove", [name]) => Box::new(move |c| {
            let line = store.remove_person(name, &c.actor()?)?;
            c.say(&format!(
                "{name} is on this board no more (change {}): their lines stay theirs, and their \
                 profile stays the machine's",
                line.n
            ));
            Ok(())
        }),
        ("join", [name, group]) => Box::new(move |c| {
            let said = match store.join_group(name, group, &c.actor()?)? {
                Some(line) => format!("{name} is in {group} (change {})", line.n),
                None => format!("{name} is in {group} already: nothing written"),
            };
            c.say(&said);
            Ok(())
        }),
        ("leave", [name, group]) => Box::new(move |c| {
            let line = store.leave_group(name, group, &c.actor()?)?;
            c.say(&format!("{name} is not in {group} now (change {})", line.n));
            Ok(())
        }),
        ("grant", rest) => Box::new(move |c| powers(store, c, true, rest)),
        ("revoke", rest) => Box::new(move |c| powers(store, c, false, rest)),
        ("group", ["add", group, rest @ ..]) => Box::new(move |c| {
            let given = parsed_powers(rest)?;
            let line = store.add_group(group, &given, &c.actor()?)?;
            c.say(&format!(
                "the group {group} is on this board (change {}), holding {}",
                line.n,
                said_powers(&line.powers)
            ));
            Ok(())
        }),
        ("group", ["remove", group]) => Box::new(move |c| {
            let line = store.remove_group(group, &c.actor()?)?;
            c.say(&format!(
                "the group {group} is removed (change {}), and its members are out of it",
                line.n
            ));
            Ok(())
        }),
        ("profile", [name]) => Box::new(move |c| profile(c, name)),
        (other, _) => {
            return Err(Stop::Refused(format!(
                "people takes list, log, add NAME, remove NAME, join NAME GROUP, leave NAME \
                 GROUP, grant NAME POWER…, revoke NAME POWER…, grant --group GROUP POWER…, \
                 revoke --group GROUP POWER…, group add GROUP [POWER…], group remove GROUP or \
                 profile NAME, not {other:?} with those arguments"
            )));
        }
    };
    if c.env.var(crate::confine::MARKER).is_some() {
        return Err(Stop::Refused(format!(
            "`just tasks` refuses people {action}: a change to a board's people or to a profile \
             asks first, and an agent runs `just tasks` unasked. Run it as `{RECIPE} {action} …`, \
             which asks: nothing was written"
        )));
    }
    change(c)
}

/// `add NAME`: the person put on the board with the powers and groups given, and their profile
/// made or given the display name, sign-ins and git handles named, as the person at the machine
/// writes it ([`change::add`], which the People page's add is too): a sign-in another profile holds
/// is refused before anything is written, on the board or in the machine folder, and a profile that
/// cannot be written once the board took the person takes the board's change back.
fn add(store: &Store, c: &mut Context, name: &str) -> Result<(), Stop> {
    let actor = c.actor()?;
    let powers = parsed_powers(
        &c.parsed
            .all("power")
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    )?;
    let groups: Vec<String> = c.parsed.all("group").to_vec();
    let given = given(c)?;
    let dir = machine(c)?;
    let added = change::add(
        store,
        &dir,
        name,
        &groups,
        &powers,
        &given,
        Writer::Machine,
        &actor,
    )
    .map_err(stopped)?;
    let reach = match added.signins {
        0 => format!(
            "no sign-in is theirs yet, so they reach no board until `{RECIPE} profile` gives one"
        ),
        _ => "they reach it by their sign-ins".to_string(),
    };
    c.say(&format!(
        "{name} is on this board (change {}), holding {}: {reach}",
        added.line.n,
        said_powers(&crate::people::Powers::of(powers).held())
    ));
    Ok(())
}

/// `profile NAME`: the machine's profile of `name` made or given what the options name, on no
/// board, as the person at the machine writes it ([`change::profile`]). A name no board could add
/// ([`crate::people::refused_person_name`]) is refused.
fn profile(c: &mut Context, name: &str) -> Result<(), Stop> {
    let given = given(c)?;
    let dir = machine(c)?;
    let changed = change::profile(&dir, name, &given, Writer::Machine).map_err(stopped)?;
    c.say(&if changed {
        format!("{name}'s profile is written in the machine folder")
    } else {
        format!("{name}'s profile holds those already: nothing written")
    });
    Ok(())
}

/// What `--display`, `--signin`, `--git`, `--email`, `--slack`, `--whatsapp` and `--notice` give a
/// profile.
fn given(c: &Context) -> Result<Given, Stop> {
    let addresses = (ways::WAYS.into_iter())
        .filter_map(|way| Some((way.to_string(), c.parsed.get(way)?.to_string())))
        .collect();
    Ok(Given {
        display: c.parsed.get("display").map(str::to_string),
        signins: parsed_signins(c)?,
        git: c.parsed.all("git").to_vec(),
        addresses,
        notices: parsed_notices(c)?,
    })
}

/// The ways `--notice KIND=WAY` names, by kind: `KIND=` sends that kind by none.
fn parsed_notices(c: &Context) -> Result<BTreeMap<String, String>, Stop> {
    (c.parsed.all("notice").iter())
        .map(|text| {
            let (kind, way) = text.split_once('=').ok_or_else(|| {
                Stop::Refused(
                    "a notice is KIND=WAY, as card=slack, or KIND= for none: nothing was written"
                        .into(),
                )
            })?;
            Ok((kind.trim().to_string(), way.trim().to_string()))
        })
        .collect()
}

/// A change's stop as the command's: a refusal exits 1, a failure 2, said with its cause, as the
/// terminal is the person at the machine's, and with the removal that takes back a person it left
/// on the board.
fn stopped(stopped: Stopped) -> Stop {
    match stopped {
        Stopped::Refused(why) => Stop::Refused(why),
        Stopped::Failed(failure) => Stop::Board(failure.told(true, |name| {
            format!("`{RECIPE} remove {name}` takes it back")
        })),
    }
}

/// `grant` and `revoke`: `NAME POWER…`, or `--group GROUP POWER…`.
fn powers(store: &Store, c: &mut Context, grant: bool, rest: &[&str]) -> Result<(), Stop> {
    let group = c.parsed.all("group").to_vec();
    let (holder, powers) = match (group.as_slice(), rest) {
        ([group], powers) => (Holder::Group(group.as_str()), powers),
        ([], [name, powers @ ..]) => (Holder::Person(name), powers),
        _ => {
            return Err(Stop::Refused(
                "grant and revoke take NAME POWER…, or --group GROUP POWER…".into(),
            ));
        }
    };
    let powers = parsed_powers(powers)?;
    let actor = c.actor()?;
    let whose = match holder {
        Holder::Person(name) => name.to_string(),
        Holder::Group(name) => format!("the group {name}"),
    };
    let said = if grant {
        match store.grant(holder, &powers, &actor)? {
            Some(line) => format!(
                "{whose} is given {} (change {})",
                said_powers(&line.powers),
                line.n
            ),
            None => format!("{whose} holds those already: nothing written"),
        }
    } else {
        let line = store.revoke(holder, &powers, &actor)?;
        format!(
            "{whose} holds {} no more (change {})",
            said_powers(&line.powers),
            line.n
        )
    };
    c.say(&said);
    Ok(())
}

/// Each person with what they hold, theirs and by their groups, and each group.
fn list(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let people = store.people()?;
    let dir = machine(c)?;
    let profiles = Profiles::read(&dir).map_err(Stop::Board)?;
    if c.parsed.flag("json") {
        let listed = listed(&people, &profiles);
        c.say(&serde_json::to_string_pretty(&listed).expect("people are JSON"));
        return Ok(());
    }
    for line in lines(&people, &profiles) {
        c.say(&line);
    }
    Ok(())
}

/// Every change made to the board's people, oldest first.
fn log(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let changes = store.people_log()?;
    if c.parsed.flag("json") {
        c.say(&serde_json::to_string_pretty(&changes).expect("changes are JSON"));
        return Ok(());
    }
    if changes.is_empty() {
        c.say("no change was made to this board's people");
    }
    for change in &changes {
        c.say(&logged(change));
    }
    Ok(())
}

/// What `people list --json` prints.
pub fn listed(people: &People, profiles: &Profiles) -> Value {
    let members: Vec<Value> = (people.members.iter())
        .map(|member| {
            let profile = profiles.get(&member.name);
            json!({
                "name": member.name,
                "display": profile.map(Profile::shown),
                "powers": people.powers_of(&member.name).held(),
                "own": member.powers,
                "groups": member.groups,
                "signins": profile.map(services).unwrap_or_default(),
            })
        })
        .collect();
    json!({
        "revision": people.revision,
        "people": members,
        "groups": people.groups.iter().map(|group| json!({
            "name": group.name,
            "powers": group.powers,
        })).collect::<Vec<_>>(),
    })
}

/// What `people list` prints.
pub fn lines(people: &People, profiles: &Profiles) -> Vec<String> {
    if people.members.is_empty() && people.groups.is_empty() {
        return vec![
            "nobody is on this board but the person at the machine, who holds every power".into(),
        ];
    }
    let mut lines = Vec::new();
    for member in &people.members {
        let profile = profiles.get(&member.name);
        let shown = profile
            .map(Profile::shown)
            .filter(|shown| *shown != member.name)
            .map(|shown| format!(" ({shown})"))
            .unwrap_or_default();
        let held = people.powers_of(&member.name).held();
        let mut line = format!("{}{shown}  {}", member.name, said_powers(&held));
        if !member.groups.is_empty() {
            line.push_str(&format!("  groups: {}", member.groups.join(", ")));
        }
        let by = profile.map(services).unwrap_or_default();
        line.push_str(&if by.is_empty() {
            "  no sign-in: reaches no board".to_string()
        } else {
            format!("  signs in by {}", by.join(", "))
        });
        lines.push(line);
    }
    for group in &people.groups {
        lines.push(format!(
            "group {}  {}",
            group.name,
            said_powers(&group.powers)
        ));
    }
    lines
}

/// One line of the log.
pub fn logged(change: &Change) -> String {
    let mut line = format!(
        "{}  {}  {}: {} {}",
        local_minute(change.at),
        change.n,
        change.by,
        change.did.word(),
        change.name
    );
    if let Some(group) = change.group.as_ref().filter(|group| **group != change.name) {
        line.push_str(&format!(" {group}"));
    }
    if !change.powers.is_empty() {
        line.push_str(&format!(" {}", words(&change.powers)));
    }
    line
}

/// `powers` said, `no power` for none.
fn said_powers(powers: &[Power]) -> String {
    if powers.is_empty() {
        return "no power: listed, and sees nothing".into();
    }
    let said: Vec<&str> = powers.iter().map(|power| power.said()).collect();
    said.join(", ")
}

/// `powers` as the board writes them.
fn words(powers: &[Power]) -> String {
    let words: Vec<&str> = powers.iter().map(|power| power.word()).collect();
    words.join(",")
}

/// The services `profile` signs in by, each once: never a subject.
fn services(profile: &Profile) -> Vec<String> {
    let mut services: Vec<String> = Vec::new();
    for signin in &profile.signins {
        if !services.contains(&signin.service) {
            services.push(signin.service.clone());
        }
    }
    services
}

/// The powers `words` name.
fn parsed_powers(words: &[&str]) -> Result<Vec<Power>, Stop> {
    (words.iter())
        .map(|word| {
            Power::parse(word).map_err(|why| Stop::Refused(format!("{why}: nothing was written")))
        })
        .collect()
}

/// The sign-ins `--signin` names.
fn parsed_signins(c: &Context) -> Result<Vec<Signin>, Stop> {
    (c.parsed.all("signin").iter())
        .map(|text| {
            Signin::parse(text).map_err(|why| Stop::Refused(format!("{why}: nothing was written")))
        })
        .collect()
}

/// The machine folder the command's environment names.
fn machine(c: &Context) -> Result<PathBuf, Stop> {
    crate::project::machine_folder(
        |name| c.env.var(name).map(OsString::from),
        cfg!(target_os = "macos"),
    )
    .map_err(Stop::Board)
}

#[cfg(test)]
mod tests;
