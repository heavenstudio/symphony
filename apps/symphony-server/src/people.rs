//! The People page's read and its two writes (`symphony_api::people`, docs/workflow/TASKS.md,
//! "People"): who is on the project's board, its groups and the log of every change, read by
//! everyone on the board; a change of them by the person at the machine or a person holding
//! administer ([`crate::powers`]); and a profile written as `symphony_board::people::change` lets
//! its writer. Both writes run the command line's code, answer the page as they left it, and are
//! the main checkout's: a worktree's page changes none.
//!
//! No answer names a sign-in's subject, an email or a chat address: a sign-in is said by its
//! service. A hosted visitor reads none of it, as the board's people are this machine's gate's.

use crate::api::{answered, error};
use crate::front::{Admitted, Shared};
use axum::Json;
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use symphony_api::{
    GroupRow, PEOPLE_CHANGE_PATH, PeopleAction, PeopleChange, PeopleLogLine, PeoplePage, PersonRow,
    ProfileWrite, WithheldPower,
};
use symphony_board::Store;
use symphony_board::people::change::{self, Given, Stopped, Writer};
use symphony_board::people::{Change, Holder, People, Power, Profiles, Signin};

/// A route's refusal: its status and its words.
type Failed = (StatusCode, String);

/// A hosted visitor's refusal: the board's people are the local gate's.
const HOSTED: &str = "Forbidden: the board's people are this machine's, never a hosted visitor's";

/// What a server that names no machine folder says of the profiles.
const NO_MACHINE: &str = "this server names no machine folder, so no profile is read or written";

/// Who asks: their short name (`owner` for the person at the machine) and whether they are that
/// person.
#[derive(Clone, Debug)]
pub(crate) struct Asker {
    pub(crate) name: String,
    pub(crate) machine: bool,
}

impl Asker {
    fn of(shared: &Shared, extensions: &axum::http::Extensions) -> Asker {
        Asker {
            name: Admitted::actor(shared, extensions).name().to_string(),
            machine: Admitted::machine(extensions),
        }
    }
}

/// `GET` [`symphony_api::PEOPLE_PATH`]: the page, the board read on the store the pages share.
pub(crate) async fn read(shared: &Shared, request: &axum::http::Request<()>) -> Response {
    if crate::hosted::admission(request).is_some() {
        return error(StatusCode::FORBIDDEN, HOSTED);
    }
    let hub = match shared.hub() {
        Ok(hub) => hub,
        Err(refusal) => return *refusal,
    };
    let asker = Asker::of(shared, request.extensions());
    let machine = shared.gate.machine().map(Path::to_path_buf);
    let read = crate::spawn::blocking(move || {
        let board = hub.on_reader(|store| Ok((store.people()?, store.people_log()?)));
        Ok::<_, Failed>(Json(page(board, profiles(machine.as_deref()), &asker)))
    });
    answered(read.await, "the people could not be read: ")
}

/// `POST` of a change or a profile (`route`), asked by `asker` of the worktree `scope`: the page
/// as it left it, or its refusal with nothing written.
pub(crate) async fn write(
    shared: &Arc<Shared>,
    scope: &str,
    route: &str,
    payload: Value,
    asker: Asker,
) -> Response {
    if !scope.is_empty() {
        return error(
            StatusCode::NOT_FOUND,
            "the board's people are changed from the project's own People page, never a \
             worktree's",
        );
    }
    let (root, machine) = (
        shared.root.clone(),
        shared.gate.machine().map(Path::to_path_buf),
    );
    let route = route.to_string();
    let written = crate::spawn::blocking(move || {
        let store = symphony::taskview::connect(&root).map_err(|why| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                format!("the task board cannot be reached: {why}"),
            )
        })?;
        let machine = machine.as_deref();
        if route == PEOPLE_CHANGE_PATH {
            let asked = serde_json::from_value(payload).map_err(|e| bad(format!("{e}")))?;
            changed(&store, machine, &asked, &asker)?;
        } else {
            let asked = serde_json::from_value(payload).map_err(|e| bad(format!("{e}")))?;
            profiled(machine, &asked, &asker)?;
        }
        let board = (store.people()).and_then(|people| Ok((people, store.people_log()?)));
        Ok::<_, Failed>(page(
            board.map_err(|why| why.to_string()),
            profiles(machine),
            &asker,
        ))
    })
    .await;
    answered(
        written.map(|done| done.map(Json)),
        "the people could not be changed: ",
    )
}

/// The machine's profiles, read from `machine`.
fn profiles(machine: Option<&Path>) -> Result<Profiles, String> {
    machine.map_or_else(|| Err(NO_MACHINE.to_string()), Profiles::read)
}

/// The page `asker` is shown of `board`'s people and log and the machine's `profiles`: each
/// person with their profile's display name, git handles and the services they sign in by, and
/// what `asker` may change. A board or profiles not read are said, the people listed without
/// what was not read.
pub(crate) fn page(
    board: Result<(People, Vec<Change>), String>,
    profiles: Result<Profiles, String>,
    asker: &Asker,
) -> PeoplePage {
    let mut problems = Vec::new();
    let (people, log) = board.unwrap_or_else(|why| {
        problems.push(format!("the board's people could not be read: {why}"));
        Default::default()
    });
    let profiles = profiles.unwrap_or_else(|why| {
        problems.push(format!("the machine's profiles could not be read: {why}"));
        Profiles::default()
    });
    let words = |powers: &[Power]| {
        powers
            .iter()
            .map(|power| power.word().to_string())
            .collect()
    };
    let rows = (people.members.iter())
        .map(|member| {
            let profile = profiles.get(&member.name);
            let mut signins: Vec<String> = Vec::new();
            for signin in profile.iter().flat_map(|profile| &profile.signins) {
                if !signins.contains(&signin.service) {
                    signins.push(signin.service.clone());
                }
            }
            PersonRow {
                name: member.name.clone(),
                display: profile.map_or(&member.name[..], |p| p.shown()).to_string(),
                powers: words(&people.powers_of(&member.name).held()),
                own: words(&member.powers),
                groups: member.groups.clone(),
                signins,
                git: profile
                    .map(|profile| profile.git.clone())
                    .unwrap_or_default(),
                profile: profile.is_some(),
            }
        })
        .collect();
    let groups = (people.groups.iter())
        .map(|group| GroupRow {
            name: group.name.clone(),
            powers: words(&group.powers),
        })
        .collect();
    let log = (log.into_iter())
        .map(|line| PeopleLogLine {
            n: line.n,
            at: line.at,
            by: line.by,
            did: line.did.word().to_string(),
            name: line.name,
            group: line.group,
            powers: words(&line.powers),
        })
        .collect();
    PeoplePage {
        administers: asker.machine || people.powers_of(&asker.name).holds(Power::Administer),
        you: (!asker.machine).then(|| asker.name.clone()),
        machine: asker.machine,
        revision: people.revision,
        people: rows,
        groups,
        log,
        withheld: (Power::ALL.into_iter())
            .filter_map(|power| {
                Some(WithheldPower {
                    power: power.word().into(),
                    why: power.withheld()?.into(),
                })
            })
            .collect(),
        problem: (!problems.is_empty()).then(|| problems.join("; ")),
    }
}

fn bad(why: String) -> Failed {
    (StatusCode::BAD_REQUEST, why)
}

/// A board's refusal (400) or failure (503), in its words.
fn board(error: symphony_board::Error) -> Failed {
    match error {
        symphony_board::Error::Refused(why) => bad(why),
        failed => (StatusCode::SERVICE_UNAVAILABLE, failed.to_string()),
    }
}

/// A profile's change refused (400) or failed (503), in its words.
fn stopped(stopped: Stopped) -> Failed {
    match stopped {
        Stopped::Refused(why) => bad(why),
        Stopped::Failed(why) => (StatusCode::SERVICE_UNAVAILABLE, why),
    }
}

/// The powers `words` names, each refused in its words.
fn powers(words: &[String]) -> Result<Vec<Power>, Failed> {
    (words.iter())
        .map(|word| Power::parse(word).map_err(|why| bad(format!("{why}: nothing was written"))))
        .collect()
}

/// The sign-ins `texts` names, each `SERVICE:SUBJECT`.
fn signins(texts: &[String]) -> Result<Vec<Signin>, Failed> {
    (texts.iter())
        .map(|text| Signin::parse(text).map_err(|why| bad(format!("{why}: nothing was written"))))
        .collect()
}

/// The machine folder a profile's write needs.
fn folder(machine: Option<&Path>) -> Result<PathBuf, Failed> {
    (machine.map(Path::to_path_buf))
        .ok_or_else(|| (StatusCode::SERVICE_UNAVAILABLE, NO_MACHINE.to_string()))
}

/// `asked` made on the board `store` as `asker`'s change: an add as `symphony_board::people::change::add`
/// writes it, with its profile in `machine`, the rest as the command line's changes are.
fn changed(
    store: &Store,
    machine: Option<&Path>,
    asked: &PeopleChange,
    asker: &Asker,
) -> Result<(), Failed> {
    let given = powers(&asked.powers)?;
    let by = asker.name.as_str();
    let name = asked.name.trim();
    let group = || {
        (asked.group.as_deref().map(str::trim))
            .filter(|group| !group.is_empty())
            .ok_or_else(|| bad("a join or a leave names its group: nothing was written".into()))
    };
    let holder = match asked.group.as_deref().map(str::trim) {
        Some(group) if !group.is_empty() => Holder::Group(group),
        _ => Holder::Person(name),
    };
    match asked.action {
        PeopleAction::Add => {
            let profile = Given {
                display: asked.display.clone(),
                signins: signins(&asked.signins)?,
                git: asked.git.clone(),
            };
            let writer = if asker.machine {
                Writer::Machine
            } else {
                Writer::Other
            };
            let machine = folder(machine)?;
            change::add(
                store,
                &machine,
                name,
                &asked.groups,
                &given,
                &profile,
                writer,
                by,
            )
            .map_err(stopped)?;
        }
        PeopleAction::Remove => drop(store.remove_person(name, by).map_err(board)?),
        PeopleAction::Join => drop(store.join_group(name, group()?, by).map_err(board)?),
        PeopleAction::Leave => drop(store.leave_group(name, group()?, by).map_err(board)?),
        PeopleAction::Grant => drop(store.grant(holder, &given, by).map_err(board)?),
        PeopleAction::Revoke => drop(store.revoke(holder, &given, by).map_err(board)?),
        PeopleAction::GroupAdd => drop(store.add_group(name, &given, by).map_err(board)?),
        PeopleAction::GroupRemove => drop(store.remove_group(name, by).map_err(board)?),
    }
    Ok(())
}

/// `asked` written to the machine's profile in `machine` as `asker` may: any for the person at
/// the machine, their own for the person themself, none for anyone else.
fn profiled(machine: Option<&Path>, asked: &ProfileWrite, asker: &Asker) -> Result<(), Failed> {
    let writer = if asker.machine {
        Writer::Machine
    } else if asked.name == asker.name {
        Writer::Own
    } else {
        Writer::Other
    };
    let given = Given {
        display: asked.display.clone(),
        signins: signins(&asked.signins)?,
        git: asked.git.clone(),
    };
    change::profile(&folder(machine)?, &asked.name, &given, writer).map_err(stopped)?;
    Ok(())
}

#[cfg(test)]
#[path = "people_tests.rs"]
mod tests;
