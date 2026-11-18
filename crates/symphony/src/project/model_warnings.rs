//! Advisory cross-agent model checks shared by the model editor and settings check, made on the
//! settings in force: the project file's with the local settings file's over them. Only names
//! recognizable from the board's agent defaults are classified; unknown text is always accepted.

use std::io::Write;
use std::path::Path;

use symphony_board::launch::defaults::{AGENTS, CLAUDE_MODEL, CLAUDE_SMALLEST, ROLES};
use symphony_board::local::Local;
use toml::de::DeTable;

/// Warn for each model in force that clearly names the other agent than the one in force: the
/// settings of `text`, the project file `file`, with what the local settings file `local` writes
/// over them (`symphony_board::local`), so a local preference that changes the pairing is judged as
/// a launch takes it. Each warning names the file, key and line that supply the model, located by
/// source span (inline and dotted tables included), and the file supplying the agent when it is
/// another. This never decides whether a file is accepted.
pub(crate) fn warn(text: &str, file: &Path, local: Option<&Local>, err: &mut dyn Write) {
    let Ok(root) = DeTable::parse(text) else {
        return;
    };
    let agents = root
        .get_ref()
        .get("agents")
        .and_then(|v| v.get_ref().as_table());
    let table = |name: &str| {
        agents
            .and_then(|agents| agents.get(name))
            .and_then(|v| v.get_ref().as_table())
    };
    let (launch, models) = (table("launch"), table("models"));
    // The committed text of `key` in `table`, and its line.
    let committed = |table: Option<&DeTable<'_>>, key: &str| {
        let (key, value) = table?.get_key_value(key)?;
        let line = text[..key.span().start].matches('\n').count() + 1;
        Some((value.get_ref().as_str()?.to_string(), line))
    };
    // The agent in force and the file that names it, none when neither does: a name the local
    // file writes holds, an empty one being none, as a launch reads it.
    let (named, names) = match local.and_then(|local| Some((local.launch[0].clone()?, local))) {
        Some((agent, local)) => (agent, Some(local.path.as_path())),
        None => committed(launch, "default")
            .map_or((String::new(), None), |(agent, _)| (agent, Some(file))),
    };
    let (named, names) = if named.trim().is_empty() {
        (AGENTS[0], None)
    } else {
        match AGENTS.into_iter().find(|agent| *agent == named) {
            Some(agent) => (agent, names),
            None => return,
        }
    };
    let roles = ROLES
        .iter()
        .enumerate()
        .map(|(at, role)| (models, *role, "[agents.models]", "models", at));
    for (table, key, at, part, index) in
        roles.chain([(launch, "model", "[agents.launch]", "launch", 1)])
    {
        let written = local.and_then(|local| {
            let model = match part {
                "launch" => local.launch[index].clone(),
                _ => local.models[index].clone(),
            }?;
            Some((
                model,
                local.path.as_path(),
                local.line(&["agents", part], key),
            ))
        });
        let Some((model, supplier, line)) = written
            .or_else(|| committed(table, key).map(|(model, line)| (model, file, Some(line))))
        else {
            continue;
        };
        let Some(agent) = agent_of(&model).filter(|agent| *agent != named) else {
            continue;
        };
        let line = line.map_or_else(String::new, |line| format!(" (line {line})"));
        let whose = names
            .filter(|names| *names != supplier)
            .map_or_else(String::new, |names| format!(" in {}", names.display()));
        let _ = writeln!(
            err,
            "warning: {}: {at} {key}{line} model {model:?} names {agent}, but [agents.launch] default is {named:?}{whose}",
            supplier.display()
        );
    }
}

fn agent_of(model: &str) -> Option<&'static str> {
    if model.starts_with("gpt-") {
        Some("codex")
    } else if model.starts_with("claude-")
        || [CLAUDE_MODEL, CLAUDE_SMALLEST, "opus", "fable"].contains(&model)
    {
        Some("claude")
    } else {
        None
    }
}
