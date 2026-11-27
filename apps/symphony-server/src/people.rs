//! The People page's read and its two writes (`symphony_api::people`, docs/workflow/TASKS.md,
//! "People"): who is on the project's board, its groups and the log of every change, read by
//! everyone on the board; a change of them by the person at the machine or a person holding
//! administer ([`crate::powers`]); and a profile written as `symphony_board::people::change` lets
//! its writer. Both writes run the command line's code, answer the page as they left it, and are
//! the main checkout's: a worktree's page changes none.
//!
//! No answer names a sign-in's subject: a sign-in is said by its service. A person's addresses go
//! to the person themself, the person at the machine and the board's admins, and the ways their
//! notices go by to the person and the person at the machine alone: anyone else's answer carries
//! neither field ([`page`]). A hosted visitor reads none of it, as the board's people are this
//! machine's gate's. A failure is told to anyone but the person at the machine without its cause,
//! which can name the machine folder's path or the board's ([`told`]): the server's log keeps it
//! whole.

use crate::api::{answered, error};
use crate::front::{Admitted, Shared};
use axum::Json;
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use symphony_api::{
    Addresses, GroupRow, NoticeWay, PEOPLE_CHANGE_PATH, PeopleAction, PeopleChange, PeopleLogLine,
    PeoplePage, PersonRow, ProfileWrite,
};
use symphony_board::Store;
use symphony_board::people::change::{self, Given, Stopped, Writer};
use symphony_board::people::{Change, Holder, People, Power, Profile, Profiles, Signin, ways};

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
    /// Who asks a request a gate let in as the person at the machine or a person a board named;
    /// none for a hosted visitor, and for a request no gate marked.
    fn of(extensions: &axum::http::Extensions) -> Option<Asker> {
        Admitted::actor(extensions).map(|actor| Asker {
            name: actor.name().to_string(),
            machine: Admitted::machine(extensions),
        })
    }
}

/// `GET` [`symphony_api::PEOPLE_PATH`]: the page, the board read on the store the pages share.
pub(crate) async fn read(shared: &Shared, request: &axum::http::Request<()>) -> Response {
    if crate::hosted::admission(request).is_some() {
        return error(StatusCode::FORBIDDEN, HOSTED);
    }
    let Some(asker) = Asker::of(request.extensions()) else {
        return crate::front::unmarked();
    };
    let hub = match shared.hub() {
        Ok(hub) => hub,
        Err(refusal) => return *refusal,
    };
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
                told(
                    &asker,
                    format!("the task board cannot be reached: {why}"),
                    "the task board cannot be reached",
                ),
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
/// person with their profile's display name, git handles and the services they sign in by, their
/// addresses where `asker` is them, the person at the machine or an admin of the board, the ways
/// their notices go by where `asker` is them or the person at the machine, and what `asker` may
/// change. A board or profiles not read are said, the people listed without what was not read.
pub(crate) fn page(
    board: Result<(People, Vec<Change>), String>,
    profiles: Result<Profiles, String>,
    asker: &Asker,
) -> PeoplePage {
    let mut problems = Vec::new();
    let (people, log) = board.unwrap_or_else(|why| {
        let unread = "the board's people could not be read";
        problems.push(told(asker, format!("{unread}: {why}"), unread));
        Default::default()
    });
    let profiles = profiles.unwrap_or_else(|why| {
        let unread = "the machine's profiles could not be read";
        problems.push(told(asker, format!("{unread}: {why}"), unread));
        Profiles::default()
    });
    let words = |powers: &[Power]| {
        powers
            .iter()
            .map(|power| power.word().to_string())
            .collect()
    };
    let administers = asker.machine || people.powers_of(&asker.name).holds(Power::Administer);
    let rows = (people.members.iter())
        .map(|member| {
            let profile = profiles.get(&member.name);
            let theirs = asker.machine || asker.name == member.name;
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
                addresses: (theirs || administers).then(|| addresses(profile)),
                notices: theirs.then(|| notices(profile)),
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
        administers,
        you: (!asker.machine).then(|| asker.name.clone()),
        machine: asker.machine,
        revision: people.revision,
        people: rows,
        groups,
        log,
        problem: (!problems.is_empty()).then(|| problems.join("; ")),
    }
}

/// The addresses `profile` holds, each the one a notice by its way goes to: none without one.
fn addresses(profile: Option<&Profile>) -> Addresses {
    let address = |way| profile.and_then(|profile| profile.address(way));
    Addresses {
        email: address(ways::EMAIL),
        slack: address(ways::SLACK),
        whatsapp: address(ways::WHATSAPP),
    }
}

/// The way each kind of notice goes by for `profile`, every kind in its order: none without one.
fn notices(profile: Option<&Profile>) -> Vec<NoticeWay> {
    (ways::KINDS.into_iter())
        .map(|kind| NoticeWay {
            kind: kind.into(),
            way: profile
                .and_then(|profile| profile.way(kind))
                .map(Into::into),
        })
        .collect()
}

fn bad(why: String) -> Failed {
    (StatusCode::BAD_REQUEST, why)
}

/// What `asker` is told of a failure: `whole`, its cause in it, for the person at the machine;
/// for anyone else `bare`, as a cause can name the machine folder's path or the board's, and the
/// server's log keeps the whole for the person at the machine.
fn told(asker: &Asker, whole: String, bare: impl Into<String>) -> String {
    if asker.machine {
        return whole;
    }
    eprintln!("· told {} no more than it failed: {whole}", asker.name);
    bare.into()
}

/// A board's refusal (400) or failure (503), as `asker` is told it.
fn board(asker: &Asker) -> impl Fn(symphony_board::Error) -> Failed + '_ {
    move |error| match error {
        symphony_board::Error::Refused(why) => bad(why),
        failed => (
            StatusCode::SERVICE_UNAVAILABLE,
            told(asker, failed.to_string(), "the board could not be changed"),
        ),
    }
}

/// A profile's change refused (400) or failed (503), as `asker` is told it: a person it left on
/// the board is taken off from this page.
fn stopped(asker: &Asker) -> impl Fn(Stopped) -> Failed + '_ {
    move |stopped| match stopped {
        Stopped::Refused(why) => bad(why),
        Stopped::Failed(failure) => {
            let remedy = |name: &str| format!("taking {name} off the board takes it back");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                told(
                    asker,
                    failure.told(true, remedy),
                    failure.told(false, remedy),
                ),
            )
        }
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
                ..Given::default()
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
            .map_err(stopped(asker))?;
        }
        PeopleAction::Remove => drop(store.remove_person(name, by).map_err(board(asker))?),
        PeopleAction::Join => drop(store.join_group(name, group()?, by).map_err(board(asker))?),
        PeopleAction::Leave => drop(
            store
                .leave_group(name, group()?, by)
                .map_err(board(asker))?,
        ),
        PeopleAction::Grant => drop(store.grant(holder, &given, by).map_err(board(asker))?),
        PeopleAction::Revoke => drop(store.revoke(holder, &given, by).map_err(board(asker))?),
        PeopleAction::GroupAdd => drop(store.add_group(name, &given, by).map_err(board(asker))?),
        PeopleAction::GroupRemove => drop(store.remove_group(name, by).map_err(board(asker))?),
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
    let addresses = [
        (ways::EMAIL, &asked.email),
        (ways::SLACK, &asked.slack),
        (ways::WHATSAPP, &asked.whatsapp),
    ];
    let (mut said, mut notices) = (
        std::collections::BTreeMap::new(),
        std::collections::BTreeMap::new(),
    );
    said.extend(
        (addresses.into_iter()).filter_map(|(way, text)| Some((way.to_string(), text.clone()?))),
    );
    notices.extend(
        (asked.notices.iter())
            .map(|notice| (notice.kind.clone(), notice.way.clone().unwrap_or_default())),
    );
    let given = Given {
        display: asked.display.clone(),
        signins: signins(&asked.signins)?,
        git: asked.git.clone(),
        addresses: said,
        notices,
    };
    change::profile(&folder(machine)?, &asked.name, &given, writer).map_err(stopped(asker))?;
    Ok(())
}

#[cfg(test)]
#[path = "people_tests.rs"]
mod tests;
