//! `people …`: who is on the board and what each may do there ([`crate::people`],
//! [`crate::Store::add_person`]), and the machine's profiles of them (`people.json` in the
//! machine folder), from the terminal, whose account is the machine's. Every change is recorded
//! as `--as`'s, and nothing it prints or logs names a sign-in's subject: a sign-in is said by its
//! service alone.

use super::{Context, Stop};
use crate::Store;
use crate::people::{Change, Holder, People, Power, Profile, Profiles, RETIRED_WHY, Signin};
use crate::sys::local_minute;
use serde_json::{Value, json};
use std::ffi::OsString;
use std::path::PathBuf;

pub(super) fn run(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let words: Vec<String> = c.parsed.all("words").to_vec();
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    match (c.arg("action").as_str(), words.as_slice()) {
        ("list", []) => list(store, c),
        ("log", []) => log(store, c),
        ("add", [name]) => add(store, c, name),
        ("remove", [name]) => remove(store, c, Holder::Person(name)),
        ("join", [name, group]) => {
            let said = match store.join_group(name, group, &c.actor()?)? {
                Some(line) => format!("{name} is in {group} (change {})", line.n),
                None => format!("{name} is in {group} already: nothing written"),
            };
            c.say(&said);
            Ok(())
        }
        ("leave", [name, group]) => {
            let line = store.leave_group(name, group, &c.actor()?)?;
            c.say(&format!("{name} is not in {group} now (change {})", line.n));
            Ok(())
        }
        ("grant", rest) => powers(store, c, true, rest),
        ("revoke", rest) => powers(store, c, false, rest),
        ("group", ["add", group, rest @ ..]) => {
            let given = parsed_powers(rest)?;
            let line = store.add_group(group, &given, &c.actor()?)?;
            c.say(&format!(
                "the group {group} is on this board (change {}), holding {}",
                line.n,
                said_powers(&line.powers)
            ));
            Ok(())
        }
        ("group", ["remove", group]) => remove(store, c, Holder::Group(group)),
        ("profile", [name]) => profile(c, name),
        (other, _) => Err(Stop::Refused(format!(
            "people takes list, log, add NAME, remove NAME, join NAME GROUP, leave NAME GROUP, \
             grant NAME POWER…, revoke NAME POWER…, grant --group GROUP POWER…, revoke --group \
             GROUP POWER…, group add GROUP [POWER…], group remove GROUP or profile NAME, not \
             {other:?} with those arguments"
        ))),
    }
}

/// `add NAME`: the person's profile made or given the sign-ins and git handles named, then the
/// person put on the board with the powers and groups given. A name the machine retired, a
/// sign-in another profile holds and anything the board would refuse ([`Store::may_add_person`])
/// are refused before anything is written; a profile that cannot be written puts no one on the
/// board, and a board that refuses the person after all (another change came between) puts the
/// profile back as it was. The profile comes first because a board's removal retires the name: a
/// person once on a board is never taken off it to undo an add.
fn add(store: &Store, c: &mut Context, name: &str) -> Result<(), Stop> {
    let actor = c.actor()?;
    let given = parsed_powers(
        &c.parsed
            .all("power")
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    )?;
    let groups: Vec<String> = c.parsed.all("group").to_vec();
    let signins = parsed_signins(c)?;
    let dir = machine(c)?;
    let addable = |profiles: &Profiles| {
        if profiles.is_retired(name) {
            return Err(format!("{name} is {RETIRED_WHY}: nothing was written"));
        }
        taken_signins(profiles, name, &signins)
    };
    addable(&Profiles::read(&dir).map_err(Stop::Board)?).map_err(Stop::Refused)?;
    store.may_add_person(name, &groups, &given)?;
    let before = Profiles::change(&dir, |profiles| {
        if let Err(why) = addable(profiles) {
            return Ok(Err(why));
        }
        let before = profiles.get(name).cloned();
        fill(profiles, name, c, &signins)?;
        Ok(Ok(before))
    })
    .map_err(|why| {
        Stop::Board(format!(
            "{name}'s profile could not be written ({why}): nothing was written"
        ))
    })?
    .map_err(Stop::Refused)?;
    let line = match store.add_person(name, &groups, &given, &actor) {
        Ok(line) => line,
        Err(error) => {
            let back = Profiles::change(&dir, |profiles| {
                put_back(profiles, name, before.clone());
                Ok(())
            });
            return Err(match back {
                Ok(()) => Stop::from(error),
                Err(why) => Stop::Board(format!(
                    "{error}; and {name}'s profile, written before the board refused, could not \
                     be put back ({why})"
                )),
            });
        }
    };
    let profiles = Profiles::read(&dir).map_err(Stop::Board)?;
    let reach = match profiles.get(name).map(|profile| profile.signins.len()) {
        Some(0) | None => {
            "no sign-in is theirs yet, so they reach no board until `people profile` gives one"
                .to_string()
        }
        Some(_) => "they reach it by their sign-ins".to_string(),
    };
    c.say(&format!(
        "{name} is on this board (change {}), holding {}: {reach}",
        line.n,
        said_powers(&crate::people::Powers::of(given).held())
    ));
    Ok(())
}

/// `remove NAME` and `group remove GROUP`: the name retired in the machine's profiles, then the
/// person or group taken off the board, whose log retires it there. A name the board holds no
/// person or group of is refused by the board, retiring nothing; a removal the board refuses after
/// the machine retired the name leaves it retired there, never the other way round.
fn remove(store: &Store, c: &mut Context, holder: Holder<'_>) -> Result<(), Stop> {
    let actor = c.actor()?;
    let people = store.people()?;
    let name = match holder {
        Holder::Person(name) | Holder::Group(name) => name,
    };
    let kept = match holder {
        Holder::Person(name) => people.member(name).is_some(),
        Holder::Group(name) => people.group(name).is_some(),
    };
    if kept {
        let dir = machine(c)?;
        Profiles::change(&dir, |profiles| {
            profiles.retire(name);
            Ok(())
        })
        .map_err(|why| {
            Stop::Board(format!(
                "{name} could not be retired in the machine's profiles ({why}): nothing was \
                 written"
            ))
        })?;
    }
    let said = match holder {
        Holder::Person(name) => format!(
            "{name} is on this board no more (change {}): their lines stay theirs, their profile \
             stays the machine's, and {name} is retired, never given to anyone again",
            store.remove_person(name, &actor)?.n
        ),
        Holder::Group(group) => format!(
            "the group {group} is removed (change {}), its members are out of it, and {group} is \
             retired, never given to anyone again",
            store.remove_group(group, &actor)?.n
        ),
    };
    c.say(&said);
    Ok(())
}

/// `profile NAME`: the machine's profile of `name` made or given what the options name, on no
/// board. A retired name makes no new profile; one kept stays that person's to change.
fn profile(c: &mut Context, name: &str) -> Result<(), Stop> {
    if !symphony_docs::syntax::is_short_name(name) {
        return Err(Stop::Refused(format!(
            "{name:?} is no short name: nothing was written"
        )));
    }
    let signins = parsed_signins(c)?;
    let dir = machine(c)?;
    let changed = Profiles::change(&dir, |profiles| {
        if profiles.is_retired(name) && profiles.get(name).is_none() {
            return Err(format!("{name} is {RETIRED_WHY}: nothing was written"));
        }
        taken_signins(profiles, name, &signins)?;
        let before = profiles.clone();
        fill(profiles, name, c, &signins)?;
        Ok(*profiles != before)
    })
    .map_err(Stop::Refused)?;
    c.say(&if changed {
        format!("{name}'s profile is written in the machine folder")
    } else {
        format!("{name}'s profile holds those already: nothing written")
    });
    Ok(())
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
        "retired": people.retired,
    })
}

/// What `people list` prints.
pub fn lines(people: &People, profiles: &Profiles) -> Vec<String> {
    let mut lines = Vec::new();
    if people.members.is_empty() && people.groups.is_empty() {
        lines.push(
            "nobody is on this board but the person at the machine, who holds every power".into(),
        );
    }
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
    if !people.retired.is_empty() {
        lines.push(format!(
            "retired, never given again: {}",
            people.retired.join(", ")
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

/// Refused where one of `signins` is another profile's than `name`'s: a sign-in is one person's.
fn taken_signins(profiles: &Profiles, name: &str, signins: &[Signin]) -> Result<(), String> {
    for signin in signins {
        if let Some(holder) = profiles.signed_in(&signin.service, &signin.subject)
            && holder.name != name
        {
            return Err(format!(
                "that {} sign-in is {}'s: a sign-in is one person's, so nothing was written",
                signin.service, holder.name
            ));
        }
    }
    Ok(())
}

/// `name`'s profile among `profiles`, made where there is none, given `--display`, `signins` and
/// `--git` where it does not hold them.
fn fill(
    profiles: &mut Profiles,
    name: &str,
    c: &Context,
    signins: &[Signin],
) -> Result<(), String> {
    let at = match profiles
        .people
        .iter()
        .position(|profile| profile.name == name)
    {
        Some(at) => at,
        None => {
            profiles.people.push(Profile {
                name: name.into(),
                ..Profile::default()
            });
            profiles.people.len() - 1
        }
    };
    let profile = &mut profiles.people[at];
    if let Some(display) = c
        .parsed
        .get("display")
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        profile.display = display.into();
    }
    for signin in signins {
        if !profile
            .signins
            .iter()
            .any(|held| held.is(&signin.service, &signin.subject))
        {
            profile.signins.push(signin.clone());
        }
    }
    for git in c.parsed.all("git").iter().map(|git| git.trim()) {
        if !git.is_empty() && !profile.git.iter().any(|held| held == git) {
            profile.git.push(git.into());
        }
    }
    Ok(())
}

/// `name`'s profile put back as it was `before` an add the board refused: removed where there was
/// none.
fn put_back(profiles: &mut Profiles, name: &str, before: Option<Profile>) {
    let at = (profiles.people.iter()).position(|profile| profile.name == name);
    match (at, before) {
        (Some(at), Some(before)) => profiles.people[at] = before,
        (Some(at), None) => {
            profiles.people.remove(at);
        }
        (None, Some(before)) => profiles.people.push(before),
        (None, None) => {}
    }
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
