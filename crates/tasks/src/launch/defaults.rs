//! What a launch starts when its caller names no agent or model: the project's `[agents] launch` in
//! the main checkout's `symphony.toml` (`default`, the agent, `codex` or `claude`; `model`; `effort`),
//! read there as `[board] on_review` is, else the board's own: Codex on [`CODEX_MODEL`] at effort
//! [`CODEX_EFFORT`], Claude on [`CLAUDE_MODEL`] at its own effort. A `--kind` or `--model` the caller
//! gives wins. The project's model and effort go with its agent (Codex when it names none): a caller
//! naming the other agent gets the board's model and effort for that one, never a model the other
//! agent does not run.
//!
//! The model each role runs on is `[agents.models]` (`orchestrator`, `executor`, `reviewer`,
//! `assessor`, each a model of the project's agent): a launch of an executor or a reviewer takes
//! its role's, else `[agents] launch`'s model, else the board's own; `just tasks limits` prints each
//! role's and where it came from ([`Defaults::model_of`]), so an agent starting a subagent reads the
//! setting. A board launch never starts an orchestrator: with no model written for it, whoever
//! starts one chooses. The assessor ([`crate::stamps`]), whose decision is which of a few stamps
//! apply, runs with none written on the smallest model its agent offers ([`smallest`]: Claude's
//! [`CLAUDE_SMALLEST`]; for Codex the board names none, so `[agents] launch`'s model, else the
//! board's own). Only those two tables are read; the rest of the file is the Symphony's, which
//! refuses a role it does not know.

use crate::on_review::MANIFEST;
use serde::Deserialize;
use std::path::Path;

/// The roles a model is written for, in the order `tasks limits` prints them.
pub const ROLES: [&str; 4] = ["orchestrator", "executor", "reviewer", ASSESSOR];
/// The role that writes a task's needs ([`crate::stamps`]), a role in name only.
pub const ASSESSOR: &str = "assessor";
/// Claude's smallest model, the assessor's with none written.
pub const CLAUDE_SMALLEST: &str = "haiku";

/// The agents a launch can start, the first the board's own.
pub const AGENTS: [&str; 2] = ["codex", "claude"];
/// The board's model for Codex.
pub const CODEX_MODEL: &str = "gpt-6.1-sol";
/// The board's model for Claude.
pub const CLAUDE_MODEL: &str = "sonnet";
/// The board's effort for Codex; Claude keeps its own unless the project names one.
pub const CODEX_EFFORT: &str = "xhigh";

/// The project's `[agents] launch` and `[agents.models]`: each `None` leaves the board's own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Defaults {
    /// One of [`AGENTS`].
    pub agent: Option<&'static str>,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// `[agents.models]`: the model each role runs on.
    pub models: Models,
}

/// `[agents.models]`: the model of the project's agent each role runs on; `None` is no model
/// written for it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Models {
    pub orchestrator: Option<String>,
    pub executor: Option<String>,
    pub reviewer: Option<String>,
    pub assessor: Option<String>,
}

impl Models {
    /// The model written for `role`, one of [`ROLES`]; none for another word.
    pub fn of(&self, role: &str) -> Option<&str> {
        match role {
            "orchestrator" => self.orchestrator.as_deref(),
            "executor" => self.executor.as_deref(),
            "reviewer" => self.reviewer.as_deref(),
            ASSESSOR => self.assessor.as_deref(),
            _ => None,
        }
    }
}

/// The smallest model `agent` offers, as the board names it: the assessor's with none written.
/// None for Codex, whose smallest the board does not name.
pub fn smallest(agent: &str) -> Option<&'static str> {
    (agent == "claude").then_some(CLAUDE_SMALLEST)
}

/// Where a role's model comes from ([`Defaults::model_of`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chosen {
    /// `[agents.models] ROLE`.
    Role,
    /// `[agents] launch`'s model: what a board launch starts.
    Launch,
    /// The board's own model for the agent ([`CODEX_MODEL`], [`CLAUDE_MODEL`]).
    Board,
    /// The smallest the agent offers ([`smallest`]): the assessor's with none written.
    Smallest,
    /// None: whoever starts it chooses (an orchestrator, which no board launch starts).
    Starter,
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

/// `[agents.models]` as written, read apart from `[agents] launch`, so a problem with either names
/// its own table.
#[derive(Default, Deserialize)]
struct ModelsManifest {
    #[serde(default)]
    agents: ModelsAgents,
}

#[derive(Default, Deserialize)]
struct ModelsAgents {
    #[serde(default)]
    models: WrittenModels,
}

#[derive(Default, Deserialize)]
struct WrittenModels {
    orchestrator: Option<String>,
    executor: Option<String>,
    reviewer: Option<String>,
    assessor: Option<String>,
}

/// A value written empty is none.
fn given(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}

/// The setting [`agent`] judges, as the project file names it in a refusal.
pub const AGENT_KEY: &str = "[agents] launch default";

/// `written`, the agent `[agents] launch default` names, as every reader of it takes it: a launch
/// here, the local file's and the board store's reader ([`crate::local`]) and the Symphony's load
/// of the project file. None when it names none or a blank one, which leaves the board's own
/// ([`AGENTS`]' first); else one of [`AGENTS`].
///
/// # Errors
///
/// For an agent no launch starts, naming `key`, the setting as its reader names it.
pub fn agent(key: &str, written: Option<&str>) -> Result<Option<&'static str>, String> {
    let Some(named) = written.filter(|named| !named.trim().is_empty()) else {
        return Ok(None);
    };
    match AGENTS.into_iter().find(|known| *known == named) {
        Some(known) => Ok(Some(known)),
        None => Err(format!(
            "{key} {named:?} is not one of {}",
            AGENTS.join(", ")
        )),
    }
}

/// The board's own model for `agent`.
fn board_model(agent: &str) -> &'static str {
    if agent == "codex" {
        CODEX_MODEL
    } else {
        CLAUDE_MODEL
    }
}

impl Defaults {
    /// The `[agents] launch` of the manifest at `root`, with the local file beside it over it
    /// ([`crate::local`]): the board's own when the file, the table or a key is missing. A file
    /// that does not parse, or a `default` that is no agent a launch starts, is an error naming
    /// the file.
    pub fn configured(root: &Path) -> Result<Defaults, String> {
        Self::configured_with(root, &crate::settings::Stored::default())
    }

    /// As [`Defaults::configured`], with what the board's store holds (`stored`,
    /// [`crate::Store::stored`]) over the committed settings and under the local file: the order
    /// every setting is read in ([`crate::settings`]).
    pub fn configured_with(
        root: &Path,
        stored: &crate::settings::Stored,
    ) -> Result<Defaults, String> {
        let path = root.join(MANIFEST);
        let committed = match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&path.display().to_string(), &text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Defaults::default()),
            Err(error) => Err(format!("cannot read {}: {error}", path.display())),
        }?;
        // A local file that is refused is read by nobody, and said where the limits are read
        // ([`crate::limits::read_at`]), on every command.
        let committed = committed.over(&stored.held);
        Ok(match crate::local::beside(&path) {
            Ok(Some(local)) => committed.over(&local),
            _ => committed,
        })
    }

    /// These, the committed settings, with what the local file `local` writes in their place
    /// ([`crate::local::Local::over`]); a value it writes empty is none, as in the project file.
    pub fn over(mut self, local: &crate::local::Local) -> Defaults {
        let mut named = self.agent.map(String::from);
        let models = &mut self.models;
        local.over(
            [&mut named, &mut self.model, &mut self.effort],
            [
                &mut models.orchestrator,
                &mut models.executor,
                &mut models.reviewer,
                &mut models.assessor,
            ],
        );
        // The local file's agent is one [`agent`] takes, as its reader checked.
        self.agent = agent(AGENT_KEY, named.as_deref()).ok().flatten();
        for value in [
            &mut self.model,
            &mut self.effort,
            &mut models.orchestrator,
            &mut models.executor,
            &mut models.reviewer,
            &mut models.assessor,
        ] {
            *value = given(value.take());
        }
        self
    }

    /// `text`, the manifest at `path`, as [`Defaults::configured`] reads it.
    pub(crate) fn parse(path: &str, text: &str) -> Result<Defaults, String> {
        let manifest: Manifest =
            toml::from_str(text).map_err(|error| format!("{path}: [agents] launch: {error}"))?;
        let launch = manifest.agents.launch;
        let agent =
            agent(AGENT_KEY, launch.default.as_deref()).map_err(|why| format!("{path}: {why}"))?;
        let written: ModelsManifest =
            toml::from_str(text).map_err(|error| format!("{path}: [agents.models]: {error}"))?;
        let models = written.agents.models;
        Ok(Defaults {
            agent,
            model: given(launch.model),
            effort: given(launch.effort),
            models: Models {
                orchestrator: given(models.orchestrator),
                executor: given(models.executor),
                reviewer: given(models.reviewer),
                assessor: given(models.assessor),
            },
        })
    }

    /// `[agents] launch`'s `default`, `model` and `effort` as these hold them, in
    /// [`crate::local::LAUNCH`]'s order: what `tasks limits` says a stored one stands over.
    pub(crate) fn launch(&self) -> [Option<&str>; 3] {
        [self.agent, self.model.as_deref(), self.effort.as_deref()]
    }

    /// Whether the project names any of the three.
    fn named(&self) -> bool {
        self.agent.is_some() || self.model.is_some() || self.effort.is_some()
    }

    /// What a launch asking for `kind` (`--kind`: `claude`, `worker` and `reviewer` are Claude,
    /// any other Codex) and `model` (`--model`; empty is none) starts here.
    pub fn resolve(&self, kind: Option<&str>, model: Option<&str>) -> Resolved {
        self.resolve_as(None, kind, model)
    }

    /// [`Defaults::resolve`] for a launch taking `role` on its task: the model `[agents.models]`
    /// writes for the role, when the launch starts the project's agent and its caller names none,
    /// else, for an assessor, the smallest its agent offers ([`smallest`]), else `[agents]
    /// launch`'s, else the board's own.
    pub fn resolve_as(
        &self,
        role: Option<&str>,
        kind: Option<&str>,
        model: Option<&str>,
    ) -> Resolved {
        let own = self.agent.unwrap_or(AGENTS[0]);
        let agent = match kind {
            None => own,
            Some("claude" | "worker" | "reviewer") => "claude",
            Some(_) => "codex",
        };
        let ours = agent == own;
        let role_model = role
            .and_then(|role| self.models.of(role))
            .map(str::to_string);
        let model = model
            .filter(|m| !m.trim().is_empty())
            .map(str::to_string)
            .or_else(|| role_model.filter(|_| ours))
            .or_else(|| {
                (role == Some(ASSESSOR))
                    .then(|| smallest(agent).map(str::to_string))
                    .flatten()
            })
            .or_else(|| self.model.clone().filter(|_| ours))
            .unwrap_or_else(|| board_model(agent).to_string());
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

    /// The project's agent, the model `role` (one of [`ROLES`]) runs on with it, and where that
    /// comes from: `[agents.models] ROLE`; else, for an assessor, the smallest the agent offers
    /// ([`smallest`]); else, for an executor, a reviewer or an assessor, `[agents] launch`'s model,
    /// then the board's own; for an orchestrator, none, as whoever starts one chooses. What `just
    /// tasks limits` prints, and an agent starting a subagent passes.
    pub fn model_of(&self, role: &str) -> (&'static str, Option<String>, Chosen) {
        let agent = self.agent.unwrap_or(AGENTS[0]);
        if let Some(model) = self.models.of(role) {
            return (agent, Some(model.to_string()), Chosen::Role);
        }
        if role == ROLES[0] {
            return (agent, None, Chosen::Starter);
        }
        if let Some(model) = smallest(agent).filter(|_| role == ASSESSOR) {
            return (agent, Some(model.to_string()), Chosen::Smallest);
        }
        match &self.model {
            Some(model) => (agent, Some(model.clone()), Chosen::Launch),
            None => (agent, Some(board_model(agent).to_string()), Chosen::Board),
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
                    models: Models::default(),
                },
            ),
            (
                "[agents.launch]\ndefault = \"codex\"\neffort = \"high\"\nother = 1\n",
                Defaults {
                    agent: Some("codex"),
                    model: None,
                    effort: Some("high".into()),
                    models: Models::default(),
                },
            ),
        ] {
            assert_eq!(parsed(text), expected, "{text}");
        }
    }

    #[test]
    fn the_launch_agent_is_one_of_the_agents_none_unwritten_or_blank_and_any_other_refused() {
        assert_eq!(agent(AGENT_KEY, None), Ok(None));
        for blank in ["", " ", "\t"] {
            assert_eq!(agent(AGENT_KEY, Some(blank)), Ok(None), "{blank:?}");
        }
        for known in AGENTS {
            assert_eq!(agent(AGENT_KEY, Some(known)), Ok(Some(known)));
        }
        for unknown in ["gemini", "Claude", " codex", "codex ", "claude-code"] {
            assert_eq!(
                agent(AGENT_KEY, Some(unknown)),
                Err(format!(
                    "[agents] launch default {unknown:?} is not one of codex, claude"
                )),
                "{unknown:?}"
            );
        }
        // The setting is named as its reader names it.
        assert_eq!(
            agent("[agents.launch] default (line 2)", Some("gemini")),
            Err("[agents.launch] default (line 2) \"gemini\" is not one of codex, claude".into())
        );
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
            models: Models::default(),
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

    #[test]
    fn the_models_table_names_each_roles_model_and_an_empty_one_is_none() {
        assert_eq!(parsed("").models, Models::default());
        let models = parsed(
            "[agents.models]\norchestrator = \"fable\"\nexecutor = \"opus\"\nreviewer = \" \"\nassessor = \"haiku-x\"\nother = \"x\"\n",
        )
        .models;
        assert_eq!(
            models,
            Models {
                orchestrator: Some("fable".into()),
                executor: Some("opus".into()),
                reviewer: None,
                assessor: Some("haiku-x".into()),
            }
        );
        assert_eq!(models.of("executor"), Some("opus"));
        assert_eq!(models.of("reviewer"), None);
        assert_eq!(models.of("assessor"), Some("haiku-x"));
        assert_eq!(models.of("worker"), None, "no role of that name");
        let problem =
            Defaults::parse("/r/symphony.toml", "[agents.models]\nexecutor = 4\n").unwrap_err();
        assert!(
            problem.starts_with("/r/symphony.toml: [agents.models]: "),
            "{problem}"
        );
    }

    #[test]
    fn a_launch_takes_its_roles_model_then_the_launch_model_then_the_boards() {
        let project = parsed(
            "[agents]\nlaunch = { default = \"claude\", model = \"opus\" }\n[agents.models]\nexecutor = \"fable\"\n",
        );
        for (role, kind, model, expected) in [
            (
                Some("executor"),
                None,
                None,
                resolved("claude", "fable", None),
            ),
            (
                Some("reviewer"),
                None,
                None,
                resolved("claude", "opus", None),
            ),
            (None, None, None, resolved("claude", "opus", None)),
            (
                Some("executor"),
                None,
                Some("sonnet"),
                resolved("claude", "sonnet", None),
            ),
            (
                Some("executor"),
                Some("worker"),
                None,
                resolved("claude", "fable", None),
            ),
            // Another agent never takes the project's role model: it is the project's agent's.
            (
                Some("executor"),
                Some("codex"),
                None,
                resolved("codex", CODEX_MODEL, Some(CODEX_EFFORT)),
            ),
        ] {
            assert_eq!(
                project.resolve_as(role, kind, model),
                expected,
                "{role:?} {kind:?} {model:?}"
            );
        }
        // No model written for the role nor the launch: the board's own.
        let bare = parsed("[agents.models]\nreviewer = \"gpt-x\"\n");
        assert_eq!(
            bare.resolve_as(Some("executor"), None, None),
            resolved("codex", CODEX_MODEL, Some(CODEX_EFFORT))
        );
        assert_eq!(
            bare.resolve_as(Some("reviewer"), None, None),
            resolved("codex", "gpt-x", Some(CODEX_EFFORT))
        );
    }

    #[test]
    fn each_roles_model_is_said_with_where_it_comes_from() {
        let project = parsed(
            "[agents]\nlaunch = { default = \"claude\", model = \"opus\" }\n[agents.models]\nreviewer = \"sonnet\"\n",
        );
        assert_eq!(
            project.model_of("orchestrator"),
            ("claude", None, Chosen::Starter)
        );
        assert_eq!(
            project.model_of("executor"),
            ("claude", Some("opus".into()), Chosen::Launch)
        );
        assert_eq!(
            project.model_of("reviewer"),
            ("claude", Some("sonnet".into()), Chosen::Role)
        );
        let none = Defaults::default();
        assert_eq!(
            none.model_of("executor"),
            ("codex", Some(CODEX_MODEL.into()), Chosen::Board)
        );
        let claude = parsed(
            "[agents.launch]\ndefault = \"claude\"\n[agents.models]\norchestrator = \"fable\"\n",
        );
        assert_eq!(
            claude.model_of("reviewer"),
            ("claude", Some(CLAUDE_MODEL.into()), Chosen::Board)
        );
        assert_eq!(
            claude.model_of("orchestrator"),
            ("claude", Some("fable".into()), Chosen::Role)
        );
    }

    #[test]
    fn the_assessor_runs_on_the_smallest_model_its_agent_offers_unless_one_is_written() {
        // Claude's smallest, over [agents] launch's model, which an executor or a reviewer takes.
        let claude = parsed("[agents]\nlaunch = { default = \"claude\", model = \"opus\" }\n");
        assert_eq!(
            claude.model_of("assessor"),
            ("claude", Some(CLAUDE_SMALLEST.into()), Chosen::Smallest)
        );
        assert_eq!(
            claude.resolve_as(Some("assessor"), None, None),
            resolved("claude", CLAUDE_SMALLEST, None)
        );
        assert_eq!(
            claude.resolve_as(Some("reviewer"), None, None),
            resolved("claude", "opus", None)
        );
        // Written for the role, it wins; a model the caller names wins over both.
        let written = parsed(
            "[agents]\nlaunch = { default = \"claude\" }\n[agents.models]\nassessor = \"sonnet\"\n",
        );
        assert_eq!(
            written.model_of("assessor"),
            ("claude", Some("sonnet".into()), Chosen::Role)
        );
        assert_eq!(
            written.resolve_as(Some("assessor"), None, Some("opus")),
            resolved("claude", "opus", None)
        );
        // Codex's smallest the board does not name: [agents] launch's model, else the board's own.
        assert_eq!(smallest("codex"), None);
        assert_eq!(
            Defaults::default().model_of("assessor"),
            ("codex", Some(CODEX_MODEL.into()), Chosen::Board)
        );
        let codex = parsed("[agents.launch]\nmodel = \"gpt-x\"\n");
        assert_eq!(
            codex.model_of("assessor"),
            ("codex", Some("gpt-x".into()), Chosen::Launch)
        );
        assert_eq!(
            codex.resolve_as(Some("assessor"), None, None),
            resolved("codex", "gpt-x", Some(CODEX_EFFORT))
        );
        // A Codex project's assessor started on Claude runs on Claude's smallest.
        assert_eq!(
            codex.resolve_as(Some("assessor"), Some("claude"), None),
            resolved("claude", CLAUDE_SMALLEST, None)
        );
    }
}
