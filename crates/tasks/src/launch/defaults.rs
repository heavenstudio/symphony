//! What a launch starts when its caller names no agent or model: the project's `[agents] launch` in
//! the main checkout's `symphony.toml` (`default`, the agent, `codex` or `claude`; `model`; `effort`),
//! read there as `[board] on_review` is, else the board's own: Codex on [`CODEX_MODEL`] at effort
//! [`CODEX_EFFORT`], Claude on [`CLAUDE_MODEL`] at its own effort. A `--kind` or `--model` the caller
//! gives wins. The project's model and effort go with its agent (Codex when it names none): a caller
//! naming the other agent gets the board's model and effort for that one, never a model the other
//! agent does not run. Only that table is read; the rest of the file is the Symphony's.

use crate::on_review::MANIFEST;
use serde::Deserialize;
use std::path::Path;

/// The agents a launch can start, the first the board's own.
pub const AGENTS: [&str; 2] = ["codex", "claude"];
/// The board's model for Codex.
pub const CODEX_MODEL: &str = "gpt-6.1-sol";
/// The board's model for Claude.
pub const CLAUDE_MODEL: &str = "sonnet";
/// The board's effort for Codex; Claude keeps its own unless the project names one.
pub const CODEX_EFFORT: &str = "xhigh";

/// The project's `[agents] launch`: each `None` leaves the board's own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Defaults {
    /// One of [`AGENTS`].
    pub agent: Option<&'static str>,
    pub model: Option<String>,
    pub effort: Option<String>,
}

/// What a launch starts: the agent, its model and its effort, `None` leaving the agent's own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    /// One of [`AGENTS`].
    pub agent: &'static str,
    pub model: String,
    pub effort: Option<String>,
}

impl Resolved {
    /// `claude on opus`, `codex on gpt-6.1-sol (effort xhigh)`.
    pub fn said(&self) -> String {
        match &self.effort {
            Some(effort) => format!("{} on {} (effort {effort})", self.agent, self.model),
            None => format!("{} on {}", self.agent, self.model),
        }
    }
}

#[derive(Default, Deserialize)]
struct Manifest {
    #[serde(default)]
    agents: Agents,
}

#[derive(Default, Deserialize)]
struct Agents {
    #[serde(default)]
    launch: Launch,
}

#[derive(Default, Deserialize)]
struct Launch {
    default: Option<String>,
    model: Option<String>,
    effort: Option<String>,
}

/// A value written empty is none.
fn given(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}

impl Defaults {
    /// The `[agents] launch` of the manifest at `root`: the board's own when the file, the table or
    /// a key is missing. A file that does not parse, or a `default` that is no agent a launch
    /// starts, is an error naming the file.
    pub fn configured(root: &Path) -> Result<Defaults, String> {
        let path = root.join(MANIFEST);
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&path.display().to_string(), &text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Defaults::default()),
            Err(error) => Err(format!("cannot read {}: {error}", path.display())),
        }
    }

    /// `text`, the manifest at `path`, as [`Defaults::configured`] reads it.
    fn parse(path: &str, text: &str) -> Result<Defaults, String> {
        let manifest: Manifest =
            toml::from_str(text).map_err(|error| format!("{path}: [agents] launch: {error}"))?;
        let launch = manifest.agents.launch;
        let agent = given(launch.default)
            .map(|named| {
                AGENTS
                    .into_iter()
                    .find(|known| *known == named)
                    .ok_or_else(|| {
                        format!(
                            "{path}: [agents] launch default {named:?} is not one of {}",
                            AGENTS.join(", ")
                        )
                    })
            })
            .transpose()?;
        Ok(Defaults {
            agent,
            model: given(launch.model),
            effort: given(launch.effort),
        })
    }

    /// Whether the project names any of the three.
    fn named(&self) -> bool {
        self.agent.is_some() || self.model.is_some() || self.effort.is_some()
    }

    /// What a launch asking for `kind` (`--kind`: `claude`, `worker` and `reviewer` are Claude,
    /// any other Codex) and `model` (`--model`; empty is none) starts here.
    pub fn resolve(&self, kind: Option<&str>, model: Option<&str>) -> Resolved {
        let own = self.agent.unwrap_or(AGENTS[0]);
        let agent = match kind {
            None => own,
            Some("claude" | "worker" | "reviewer") => "claude",
            Some(_) => "codex",
        };
        let ours = agent == own;
        let model = model
            .filter(|m| !m.trim().is_empty())
            .map(str::to_string)
            .or_else(|| self.model.clone().filter(|_| ours))
            .unwrap_or_else(|| {
                if agent == "codex" {
                    CODEX_MODEL
                } else {
                    CLAUDE_MODEL
                }
                .to_string()
            });
        let effort = self
            .effort
            .clone()
            .filter(|_| ours)
            .or_else(|| (agent == "codex").then(|| CODEX_EFFORT.to_string()));
        Resolved {
            agent,
            model,
            effort,
        }
    }

    /// What `launch --help` says a bare launch in the project at `root` starts, and where that
    /// comes from.
    pub fn said_at(root: &Path) -> String {
        let path = root.join(MANIFEST);
        match Self::configured(root) {
            Ok(defaults) if defaults.named() => format!(
                "here a bare launch starts {}, from [agents] launch in {}\n",
                defaults.resolve(None, None).said(),
                path.display()
            ),
            Ok(defaults) => format!(
                "here a bare launch starts {}, the board's own: no [agents] launch in {}\n",
                defaults.resolve(None, None).said(),
                path.display()
            ),
            Err(problem) => format!("here a launch is refused: {problem}\n"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> Defaults {
        Defaults::parse("symphony.toml", text).unwrap()
    }

    fn resolved(agent: &'static str, model: &str, effort: Option<&str>) -> Resolved {
        Resolved {
            agent,
            model: model.into(),
            effort: effort.map(str::to_string),
        }
    }

    #[test]
    fn the_table_names_each_default_or_leaves_the_boards_own() {
        for (text, expected) in [
            ("", Defaults::default()),
            ("[project]\nname = \"x\"\n", Defaults::default()),
            ("[agents]\npanes = \"hc\"\n", Defaults::default()),
            (
                "[agents]\nlaunch = { default = \"\", model = \" \", effort = \"\" }\n",
                Defaults::default(),
            ),
            (
                "[agents]\npanes = \"hc\"\nlaunch = { default = \"claude\", model = \"opus\" }\n",
                Defaults {
                    agent: Some("claude"),
                    model: Some("opus".into()),
                    effort: None,
                },
            ),
            (
                "[agents.launch]\ndefault = \"codex\"\neffort = \"high\"\nother = 1\n",
                Defaults {
                    agent: Some("codex"),
                    model: None,
                    effort: Some("high".into()),
                },
            ),
        ] {
            assert_eq!(parsed(text), expected, "{text}");
        }
    }

    #[test]
    fn an_unknown_agent_or_a_broken_file_is_an_error_naming_the_file() {
        assert_eq!(
            Defaults::parse(
                "/r/symphony.toml",
                "[agents.launch]\ndefault = \"gemini\"\n"
            ),
            Err(
                "/r/symphony.toml: [agents] launch default \"gemini\" is not one of codex, claude"
                    .into()
            )
        );
        for text in ["[agents.launch]\nmodel = 4\n", "[agents\n"] {
            let problem = Defaults::parse("/r/symphony.toml", text).unwrap_err();
            assert!(
                problem.starts_with("/r/symphony.toml: [agents] launch: "),
                "{problem}"
            );
        }
    }

    #[test]
    fn a_project_naming_nothing_starts_the_boards_own_agent_and_model() {
        let none = Defaults::default();
        for (kind, model, expected) in [
            (
                None,
                None,
                resolved("codex", CODEX_MODEL, Some(CODEX_EFFORT)),
            ),
            (
                Some("codex"),
                None,
                resolved("codex", CODEX_MODEL, Some("xhigh")),
            ),
            (Some("claude"), None, resolved("claude", CLAUDE_MODEL, None)),
            (Some("worker"), None, resolved("claude", "sonnet", None)),
            (
                Some("reviewer"),
                Some("opus"),
                resolved("claude", "opus", None),
            ),
            (None, Some("m"), resolved("codex", "m", Some("xhigh"))),
            (
                None,
                Some(""),
                resolved("codex", CODEX_MODEL, Some("xhigh")),
            ),
        ] {
            assert_eq!(none.resolve(kind, model), expected, "{kind:?} {model:?}");
        }
    }

    #[test]
    fn the_projects_agent_model_and_effort_win_over_the_boards_and_flags_over_both() {
        let claude = Defaults {
            agent: Some("claude"),
            model: Some("opus".into()),
            effort: Some("high".into()),
        };
        for (kind, model, expected) in [
            (None, None, resolved("claude", "opus", Some("high"))),
            (
                Some("claude"),
                None,
                resolved("claude", "opus", Some("high")),
            ),
            (
                Some("reviewer"),
                None,
                resolved("claude", "opus", Some("high")),
            ),
            (
                None,
                Some("sonnet"),
                resolved("claude", "sonnet", Some("high")),
            ),
            // Another agent never takes the project's model or effort: they are Claude's.
            (
                Some("codex"),
                None,
                resolved("codex", CODEX_MODEL, Some("xhigh")),
            ),
            (
                Some("codex"),
                Some("m"),
                resolved("codex", "m", Some("xhigh")),
            ),
        ] {
            assert_eq!(claude.resolve(kind, model), expected, "{kind:?} {model:?}");
        }
    }

    #[test]
    fn a_model_or_effort_without_an_agent_goes_with_the_boards_agent() {
        let codex = parsed("[agents.launch]\nmodel = \"gpt-x\"\neffort = \"medium\"\n");
        assert_eq!(
            codex.resolve(None, None),
            resolved("codex", "gpt-x", Some("medium"))
        );
        assert_eq!(
            codex.resolve(Some("claude"), None),
            resolved("claude", CLAUDE_MODEL, None)
        );
        let agent_only = parsed("[agents.launch]\ndefault = \"claude\"\n");
        assert_eq!(
            agent_only.resolve(None, None),
            resolved("claude", CLAUDE_MODEL, None)
        );
    }

    #[test]
    fn a_resolved_launch_is_said_with_its_effort_when_it_has_one() {
        assert_eq!(resolved("claude", "opus", None).said(), "claude on opus");
        assert_eq!(
            resolved("codex", "gpt-6.1-sol", Some("xhigh")).said(),
            "codex on gpt-6.1-sol (effort xhigh)"
        );
    }

    #[test]
    fn the_help_says_what_a_bare_launch_starts_and_where_that_comes_from() {
        let dir = std::env::temp_dir().join(format!("launch-defaults-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(MANIFEST).display().to_string();
        assert_eq!(
            Defaults::said_at(&dir),
            format!(
                "here a bare launch starts codex on gpt-6.1-sol (effort xhigh), the board's own: \
                 no [agents] launch in {path}\n"
            )
        );
        std::fs::write(
            dir.join(MANIFEST),
            "[agents]\nlaunch = { default = \"claude\", model = \"opus\" }\n",
        )
        .unwrap();
        assert_eq!(
            Defaults::said_at(&dir),
            format!("here a bare launch starts claude on opus, from [agents] launch in {path}\n")
        );
        // A file there that cannot be read is no file missing.
        let unreadable = dir.join("unreadable");
        std::fs::create_dir_all(unreadable.join(MANIFEST)).unwrap();
        let problem = Defaults::configured(&unreadable).unwrap_err();
        assert!(
            problem.starts_with(&format!(
                "cannot read {}: ",
                unreadable.join(MANIFEST).display()
            )),
            "{problem}"
        );
        std::fs::write(dir.join(MANIFEST), "[agents.launch]\ndefault = \"x\"\n").unwrap();
        assert_eq!(
            Defaults::said_at(&dir),
            format!(
                "here a launch is refused: {path}: [agents] launch default \"x\" is not one of \
                 codex, claude\n"
            )
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
