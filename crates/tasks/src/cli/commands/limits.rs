//! `limits`: the limits a command run here is held to and the model each role runs on, each with
//! where it came from, the project's `symphony.toml` as it is now ([`crate::limits`],
//! [`Defaults::model_of`]), or the local file beside it for one that file overrides
//! ([`crate::local`]), or the board's store for one it holds ([`crate::settings`]), with the
//! committed value that one stands over. What an agent reads before it starts a subagent, and what tells a
//! project its edit was taken.

use super::{Context, Stop};
use crate::Store;
use crate::alarm;
use crate::launch::defaults::{Chosen, ROLES};
use crate::launch::hc::Binary;
use crate::launch::{Defaults, Machine, running};
use crate::limits::{self, LOW, MACHINE_AGENTS, Read, SLOTS, TASKS};
use crate::local::Local;
use crate::settings::{self, Stored};
use std::path::Path;

/// The width of a setting's name, the longest (`review_alarm_clear_hours`) and a space.
const NAME: usize = 25;
/// The least width of a value before the space that ends it: wider when a model's name is longer,
/// so every row's source starts in one column.
const VALUE: usize = 7;

pub(super) fn run(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let file = limits::file_of(c.env);
    let text = file
        .as_deref()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    let read = store.limits_read().unwrap_or_else(|| Read {
        limits: store.limits(),
        ..Read::default()
    });
    let running = read
        .limits
        .machine_agents
        .map(|_| running::running(store, c.env, &Binary::of(c.env)));
    let stored = store.stored();
    let models = Defaults::configured_with(&Machine::of(c.env).root, &stored);
    if let Err(problem) = &models {
        let _ = writeln!(c.err, "⚠ {problem}");
    }
    // What is wrong with a local file the store's own read of the limits said already.
    let local = file
        .as_deref()
        .and_then(|path| crate::local::beside(path).ok().flatten());
    for line in lines(
        file.as_deref(),
        &text,
        &read,
        models.ok().as_ref(),
        running,
        local.as_ref(),
        &stored,
    ) {
        c.say(&line);
    }
    Ok(())
}

/// What `limits` prints: the limits `read` holds from `file` (none outside a repository) whose
/// `text` names their lines, then the model each role runs on as `models` says (none when the file
/// cannot be read for them), then the keys of `[limits]` left out. `running` is the machine's count
/// of agents, made only under a limit on it. Where `read` and `models` hold what the local file
/// `local` writes over `file` ([`crate::local`]), that file is named under the project file, and a
/// model it writes, as the agent it names, is said to come from it, with its line. What `stored`,
/// the board's store, holds in force and no local key replaces is said to come from the store,
/// with the committed value it stands over, and `[agents] panes` is a row only then.
pub fn lines(
    file: Option<&Path>,
    text: &str,
    read: &Read,
    models: Option<&Defaults>,
    running: Option<Result<usize, crate::Error>>,
    local: Option<&Local>,
    stored: &Stored,
) -> Vec<String> {
    let mut lines = vec![match file {
        Some(path) => format!("LIMITS from {}", path.display()),
        None => "LIMITS: no project file outside a repository, so every default".into(),
    }];
    if !stored.is_empty() {
        let count = stored.keys.len();
        lines.push(format!(
            "STORED over it {count} setting{} the board keeps (just tasks settings list)",
            if count == 1 { "" } else { "s" }
        ));
    }
    if let Some(local) = local {
        lines.push(format!(
            "LOCAL  over it {}, which git does not version",
            local.path.display()
        ));
    }
    // A value the local file writes that is not empty: one written empty is none, and its source
    // is whatever stands in its place.
    let wrote = |written: Option<&str>| written.is_some_and(|value| !value.trim().is_empty());
    let source = |key: &str| {
        let from =
            (read.written.get(key)).map_or_else(|| "default".to_string(), limits::Written::said);
        match read.under.get(key) {
            Some(under) => format!("{from}, over {under}"),
            None => from,
        }
    };
    // What the project file alone says of the launch and the models, which a stored one is over.
    let committed = Defaults::parse("", text).unwrap_or_default();
    // `KEY` of the board's store as a source, where it holds the entry `key` in force: over the
    // committed `value` written at the key `leaf` of the table at `path`, or over none.
    let kept = |key: &str, value: Option<&str>, path: &[&str], leaf: &str| {
        stored.holds(key).then(|| {
            let at = settings::named(key);
            let under = match value {
                Some(value) => format!("{value:?} ({})", written(text, at, path, leaf)),
                None => "none written".into(),
            };
            format!("{}, over {under}", Stored::written(key).said())
        })
    };
    let width = models.map_or(VALUE, |defaults| {
        (ROLES.iter().filter_map(|role| defaults.model_of(role).1))
            .map(|model| model.chars().count())
            .fold(VALUE, usize::max)
    });
    let row =
        |key: &str, value: String, from: String| format!("{key:<NAME$}{value:<width$} {from}");
    let held = read.limits;
    let none = |value: Option<usize>| value.map_or_else(|| "none".to_string(), |n| n.to_string());
    lines.push(row(SLOTS, none(held.orchestrator_slots), source(SLOTS)));
    lines.push(row(
        TASKS,
        held.orchestrator_tasks.to_string(),
        source(TASKS),
    ));
    lines.push(row(LOW, held.low_volume.to_string(), source(LOW)));
    let count = match running {
        None => String::new(),
        Some(Ok(count)) => format!("; this machine runs {count} now"),
        Some(Err(problem)) => format!("; {problem}"),
    };
    lines.push(row(
        MACHINE_AGENTS,
        none(held.machine_agents),
        format!("{}{count}", source(MACHINE_AGENTS)),
    ));
    let alarm = held.alarm;
    for (key, value) in alarm::KEYS
        .iter()
        .zip([alarm.ratio, alarm.raise_hours, alarm.clear_hours])
    {
        lines.push(row(key, value.to_string(), source(key)));
    }
    match models {
        Some(defaults) => {
            let (agent, _, _) = defaults.model_of(ROLES[0]);
            const LAUNCH: [&str; 2] = ["agents", "launch"];
            let [was_agent, was_model, _] = committed.launch();
            let stored_agent = (wrote(stored.held.launch[0].as_deref()))
                .then(|| kept("agents.launch.default", was_agent, &LAUNCH, "default"))
                .flatten();
            lines.push(
                match local.filter(|local| wrote(local.launch[0].as_deref())) {
                    Some(local) => format!(
                        "MODELS of {agent}, from {}",
                        local
                            .written("[agents] launch default", &LAUNCH, "default")
                            .said()
                    ),
                    None => match stored_agent {
                        Some(from) => format!("MODELS of {agent}, from {from}"),
                        None => format!("MODELS of {agent}"),
                    },
                },
            );
            for role in ROLES {
                let (_, model, chosen) = defaults.model_of(role);
                let from = match chosen {
                    Chosen::Role => {
                        let at = format!("[agents.models] {role}");
                        let (_, was, chose) = committed.model_of(role);
                        let was = was.filter(|_| chose == Chosen::Role);
                        let stored = (wrote(stored.held.model_of(role)))
                            .then(|| {
                                let key = format!("agents.models.{role}");
                                kept(&key, was.as_deref(), &["agents", "models"], role)
                            })
                            .flatten();
                        match local.filter(|local| wrote(local.model_of(role))) {
                            Some(local) => local.written(&at, &["agents", "models"], role).said(),
                            None => stored
                                .unwrap_or_else(|| written(text, at, &["agents", "models"], role)),
                        }
                    }
                    Chosen::Launch => {
                        let at = "[agents] launch model";
                        let stored = (wrote(stored.held.launch[1].as_deref()))
                            .then(|| kept("agents.launch.model", was_model, &LAUNCH, "model"))
                            .flatten();
                        match local.filter(|local| wrote(local.launch[1].as_deref())) {
                            Some(local) => local.written(at, &LAUNCH, "model").said(),
                            None => {
                                stored.unwrap_or_else(|| written(text, at.into(), &LAUNCH, "model"))
                            }
                        }
                    }
                    Chosen::Board => format!("the board's own for {agent}"),
                    Chosen::Smallest => format!("the smallest {agent} offers"),
                    Chosen::Starter => "whoever starts one chooses".into(),
                };
                lines.push(row(role, model.unwrap_or_else(|| "-".into()), from));
            }
        }
        None => lines.push("MODELS: the project file cannot be read for them".into()),
    }
    // The panes setting is the Symphony's to read, and a row here only when the store holds it.
    if let Some(panes) = &stored.held.panes {
        let was = toml::from_str::<toml::Table>(text)
            .ok()
            .and_then(|file| Some(file.get("agents")?.get("panes")?.as_str()?.to_string()));
        if let Some(from) = kept("agents.panes", was.as_deref(), &["agents"], "panes") {
            lines.push(row("panes", panes.clone(), from));
        }
    }
    for key in &read.unknown {
        lines.push(format!("LEFT OUT {key}: this build does not know it"));
    }
    lines
}

/// `[agents.models] reviewer, line 14`: the setting `at`, the key `key` of the table at `path` in
/// `text`, with its line.
fn written(text: &str, at: String, path: &[&str], key: &str) -> String {
    limits::Written {
        at,
        line: limits::line(text, path, key),
    }
    .said()
}

#[cfg(test)]
mod tests;
