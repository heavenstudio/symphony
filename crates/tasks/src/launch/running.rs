//! How many agents this machine runs, whoever started them: what `[limits] machine_agents`
//! ([`crate::limits`]) holds `launch` and `next` to, and the staffing pass's session cap
//! ([`super::staff`]); and how many of the board's sessions each agent runs ([`by_agent`]), which
//! the Settings page says beside its switch.

use super::hc::{self, Hc, Object, text_of};
use crate::cli::agents::{self, Agent};
use crate::cli::{Env, probes};
use crate::{Error, Health, Store};

/// The agents `agents` (a board's agents view) and `terminals` (every live terminal of the
/// machine) show: each terminal running an agent, and each live worker, reviewer or orchestrator
/// session with no agent terminal, matched to none of those by name or pane (one started outside
/// hc, a subagent).
pub fn counted(agents: &[Agent], terminals: &[Object]) -> usize {
    let running: Vec<&Object> = terminals
        .iter()
        .filter(|t| hc::agent(t).is_some())
        .collect();
    running.len()
        + agents
            .iter()
            .filter(|a| {
                a.health == Health::Live
                    && a.agent.is_none()
                    && matches!(a.kind.as_str(), "worker" | "reviewer" | "orchestrator")
                    && !running.iter().any(|t| {
                        text_of(t, "name") == a.handle
                            || a.pane.as_deref() == Some(text_of(t, "address").as_str())
                    })
            })
            .count()
}

/// The agents this machine runs now ([`counted`]), or why they cannot be counted.
pub fn running(store: &Store, env: &Env, hc: &dyn Hc) -> Result<usize, Error> {
    let view = agents::describe(store, &probes::machine(env), store.now(), false)?;
    if !view.herdr {
        return Err(Error::Refused(
            "hc cannot be asked, so the agents this machine runs cannot be counted".into(),
        ));
    }
    Ok(counted(&view.agents, &hc.live()?))
}

/// How many of the board's sessions each agent runs now, in [`super::defaults::AGENTS`]' order:
/// each session that holds (live, stale or awaiting a sweep) and has an open launch record, counted
/// once, as the agent its newest such record names. From the board alone: no terminal is asked.
pub fn by_agent(store: &Store) -> Result<[usize; 2], Error> {
    let holding: std::collections::HashSet<String> = (store.sessions()?.into_iter())
        .filter(|s| s.health.holds())
        .map(|s| s.row.handle)
        .collect();
    let mut agent = std::collections::BTreeMap::new();
    for record in store.open_task_sessions()? {
        if holding.contains(&record.handle) {
            agent.insert(record.handle, super::agent_of(&record.claude_session));
        }
    }
    let mut counts = [0; 2];
    for (count, name) in counts.iter_mut().zip(super::defaults::AGENTS) {
        *count = agent.values().filter(|of| **of == name).count();
    }
    Ok(counts)
}

/// Why a launch from `env` is refused now ([`over`]), if it is: only under `[limits]
/// machine_agents` is anything counted.
pub fn refusing_launch(store: &Store, env: &Env, hc: &dyn Hc) -> Option<String> {
    let most = store.limits().machine_agents?;
    over(running(store, env, hc), Some(most), &written_in(env))
}

/// What `next` from `env` warns of now ([`past`]), if anything.
pub fn warning_next(store: &Store, env: &Env, hc: &dyn Hc) -> Option<String> {
    let most = store.limits().machine_agents?;
    past(running(store, env, hc), Some(most), &written_in(env))
}

/// The project file the limits of `env` are written in, as a refusal names it.
fn written_in(env: &Env) -> String {
    crate::limits::file_of(env).map_or_else(
        || crate::on_review::MANIFEST.to_string(),
        |path| path.display().to_string(),
    )
}

/// What `[limits] machine_agents` says of a launch: `None` while `running` stays under `most` (or
/// no limit is written), else why it is refused. `file` names where the limit is written.
pub fn over(running: Result<usize, Error>, most: Option<usize>, file: &str) -> Option<String> {
    let most = most?;
    match running {
        Ok(count) if count < most => None,
        Ok(count) => Some(format!(
            "this machine runs {count} agents, at its limit of {most} ([limits] machine_agents in \
             {file})"
        )),
        Err(problem) => Some(format!(
            "{problem}: [limits] machine_agents in {file} holds it to {most}"
        )),
    }
}

/// What `[limits] machine_agents` says to `next`: `None` while `running` stays within `most` (or no
/// limit is written), else a warning. Whoever asks runs already, and is counted: refusing it work
/// would free nothing, so `next` still names its task.
pub fn past(running: Result<usize, Error>, most: Option<usize>, file: &str) -> Option<String> {
    let most = most?;
    match running {
        Ok(count) if count <= most => None,
        Ok(count) => Some(format!(
            "this machine runs {count} agents, past its limit of {most} ([limits] machine_agents \
             in {file}): no more start until some finish"
        )),
        Err(problem) => Some(format!(
            "{problem}: [limits] machine_agents in {file} holds it to {most}"
        )),
    }
}

#[cfg(test)]
mod tests;
