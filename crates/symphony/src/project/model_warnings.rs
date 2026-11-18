//! Advisory cross-agent model checks shared by the model editor and settings check. Only names
//! recognizable from the board's agent defaults are classified; unknown text is always accepted.

use std::io::Write;
use std::path::Path;

use symphony_board::launch::defaults::{AGENTS, CLAUDE_MODEL, CLAUDE_SMALLEST, ROLES};
use toml::de::DeTable;

/// Warn for each written model that clearly names the other agent, locating its key by source
/// span, including inline and dotted tables. This never decides whether a file is accepted.
pub(crate) fn warn(text: &str, file: &Path, err: &mut dyn Write) {
    let Ok(root) = DeTable::parse(text) else {
        return;
    };
    let Some(agents) = root
        .get_ref()
        .get("agents")
        .and_then(|v| v.get_ref().as_table())
    else {
        return;
    };
    let launch = agents.get("launch").and_then(|v| v.get_ref().as_table());
    let named = launch
        .and_then(|table| table.get("default"))
        .and_then(|v| v.get_ref().as_str())
        .filter(|agent| !agent.trim().is_empty())
        .unwrap_or(AGENTS[0]);
    if !AGENTS.contains(&named) {
        return;
    }
    let models = agents.get("models").and_then(|v| v.get_ref().as_table());
    for (table, key, at) in ROLES
        .map(|role| (models, role, "[agents.models]"))
        .into_iter()
        .chain([(launch, "model", "[agents.launch]")])
    {
        let Some((key, value)) = table.and_then(|table| table.get_key_value(key)) else {
            continue;
        };
        let Some(model) = value.get_ref().as_str() else {
            continue;
        };
        let Some(agent) = agent_of(model).filter(|agent| *agent != named) else {
            continue;
        };
        let line = text[..key.span().start].matches('\n').count() + 1;
        let _ = writeln!(
            err,
            "warning: {}: {at} {} (line {line}) model {model:?} names {agent}, but [agents.launch] default is {named:?}",
            file.display(),
            key.get_ref()
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
