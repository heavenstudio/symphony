//! `symphony model ROLE MODEL`: edit the main checkout's role settings in place, preserving
//! comments and order. `all MODEL --agent AGENT --assessor MODEL` also switches the launch agent
//! and its fallback model, with an explicit assessor override. Recognizable cross-agent names
//! warn, judged on the settings in force; model names remain arbitrary text.
//! Checking and landing a settings change follow the project's settings-only policy. With
//! `--local` the same edit goes to the local settings file beside the project file instead
//! (`symphony_board::local`), which git does not version and nothing lands: made when it is not
//! there, and never left as its readers would refuse it.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use symphony_board::launch::defaults::{AGENTS, ASSESSOR, ROLES};
use symphony_board::local;

const USAGE: &str = "usage: symphony model ROLE MODEL [--agent AGENT] [--assessor MODEL] [--local]";
const LOCAL: &str = "--local";

struct Edit<'a> {
    role: &'a str,
    model: &'a str,
    agent: Option<&'a str>,
    assessor: Option<&'a str>,
    /// The local settings file is edited, in the project file's place.
    local: bool,
}

impl<'a> Edit<'a> {
    fn parsed(parsed: &'a super::Parsed) -> Result<Self, String> {
        let [role, model] = parsed.positionals.as_slice() else {
            return Err("expected ROLE and MODEL".into());
        };
        let edit = Self {
            role,
            model,
            agent: parsed.value("--agent"),
            assessor: parsed.value("--assessor"),
            local: parsed.flag(LOCAL),
        };
        edit.validate()?;
        Ok(edit)
    }

    fn validate(&self) -> Result<(), String> {
        if self.role != "all" && !ROLES.contains(&self.role) {
            return Err(format!("unknown role {:?}", self.role));
        }
        if let Some(agent) = self.agent
            && !AGENTS.contains(&agent)
        {
            return Err(format!(
                "unknown agent {agent:?}; expected {}",
                AGENTS.join(", ")
            ));
        }
        if self.role != "all" && (self.agent.is_some() || self.assessor.is_some()) {
            return Err("--agent and --assessor are only available with all".into());
        }
        Ok(())
    }
}

pub(super) fn main(
    args: &[String],
    env: &BTreeMap<String, String>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let flags = [
        super::Flag {
            name: "--agent",
            value: true,
        },
        super::Flag {
            name: "--assessor",
            value: true,
        },
        super::Flag {
            name: LOCAL,
            value: false,
        },
    ];
    let parsed = match super::parse(args, &flags) {
        Ok(parsed) => parsed,
        Err(problem) => return super::refuse(err, USAGE, "model", &problem),
    };
    let edit = match Edit::parsed(&parsed) {
        Ok(edit) => edit,
        Err(problem) => return super::refuse(err, USAGE, "model", &problem),
    };
    let result = std::env::current_dir()
        .map_err(|error| error.to_string())
        .and_then(|here| change(&here, &edit, env, err));
    match result {
        Ok(line) => {
            let _ = writeln!(out, "{line}");
            0
        }
        Err(problem) => {
            let _ = writeln!(err, "symphony model: {problem}");
            1
        }
    }
}

/// The edit in the main checkout's project file, or with `--local` in the local settings file
/// beside it, which starts empty when it is not there.
fn change(
    here: &Path,
    edit: &Edit<'_>,
    env: &BTreeMap<String, String>,
    err: &mut dyn Write,
) -> Result<String, String> {
    let (top, _) = symphony_board::git::checkout(here)
        .ok_or_else(|| format!("{} is in no git checkout", here.display()))?;
    let project = symphony_board::builds::main_of(&top).join(crate::project::FILE);
    let file = if edit.local {
        local::path_beside(&project)
    } else {
        project.clone()
    };
    super::unasked_inside(&file, env)?;
    let before = match crate::text::read_text(&file) {
        Ok(text) => text,
        Err(e) if edit.local && e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(crate::text::os_error(&e, &file)),
    };
    let after = if edit.local {
        // A file its readers leave out is mended by hand first, each problem named on the line
        // it stands on, before any edit moves it.
        local::parse(&file, &before).map_err(|problems| problems.join("\n"))?;
        // The edit names the project file in what it refuses: here it is the local one.
        edited(&before, edit)
            .map_err(|problem| problem.replacen(crate::project::FILE, local::FILE, 1))?
    } else {
        edited(&before, edit)?
    };
    let Edit {
        role,
        model,
        agent,
        assessor,
        ..
    } = edit;
    let details = format!(
        "{}{}",
        agent.map_or_else(String::new, |agent| format!(
            "; launch agent set to {agent:?}"
        )),
        assessor.map_or_else(String::new, |model| format!(
            "; assessor model set to {model:?}"
        ))
    );
    // A cross-agent name is warned of in the settings in force once the edit is made: the local
    // file's over the project file's, whichever of the two was edited.
    let warn = |err: &mut dyn Write| {
        if edit.local {
            let committed = crate::text::read_text(&project).unwrap_or_default();
            let over = local::parse(&file, &after).ok();
            crate::project::warn_models(&committed, &project, over.as_ref(), err);
        } else {
            let over = local::beside(&project).ok().flatten();
            crate::project::warn_models(&after, &project, over.as_ref(), err);
        }
    };
    if before == after {
        warn(err);
        return Ok(format!(
            "{}: {role} already {model:?}{details}",
            file.display()
        ));
    }
    crate::text::write_whole(&file, &after).map_err(|e| crate::text::os_error(&e, &file))?;
    warn(err);
    Ok(format!(
        "{}: {role} model set to {model:?}{details}",
        file.display()
    ))
}

/// Replace only each value's source span ([`crate::project::edit::set`]).
fn edited(text: &str, edit: &Edit<'_>) -> Result<String, String> {
    edit.validate()?;
    let roles = if edit.role == "all" {
        ROLES.as_slice()
    } else {
        std::slice::from_ref(&edit.role)
    };
    let mut text = text.to_string();
    if let Some(agent) = edit.agent {
        set(&mut text, &["agents", "launch", "default"], agent)?;
        set(&mut text, &["agents", "launch", "model"], edit.model)?;
    }
    for role in roles {
        let model = if *role == ASSESSOR {
            edit.assessor.unwrap_or(edit.model)
        } else {
            edit.model
        };
        set(&mut text, &["agents", "models", role], model)?;
    }
    Ok(text)
}

fn set(text: &mut String, path: &[&str], model: &str) -> Result<(), String> {
    crate::project::edit::set(text, path, &toml::Value::String(model.into()))
}

#[cfg(test)]
mod tests;
