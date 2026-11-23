//! `describe`: the exchange's entry on the project's Settings page, in the shape every entry
//! takes (`symphony_blocks::SettingsEntry`, spelt here as [`shape`]), printed as JSON for the
//! server that runs this command to read it. It says what `status` says, from the same two
//! places: the settings the default branch commits, each with the line that writes it, and what
//! this program keeps. It reads no remote and writes nothing, not even its folder.
//!
//! Every setting is reviewed: the file decides what lands and what is written to an outside
//! service, so a change of it is a reviewed task's, and the page that shows it writes none.

use crate::config::{self, Config, Loaded, PullRequests, Releases, Verification};
use crate::reconcile::Context;
use crate::state::Store;
pub mod shape;

use shape::{
    EntryNeed, EntryState, Setting, SettingClass, SettingKind, SettingScope, SettingSource,
    SettingsEntry,
};

/// The entry's id on the page.
pub const ID: &str = "github";

/// The exchange's entry, as the committed settings and what is kept say it now.
#[must_use]
pub fn entry(ctx: &Context<'_>) -> SettingsEntry {
    let loaded = ctx.source.load();
    let (state, settings, needs) = match &loaded {
        Ok(loaded) => (
            state(loaded),
            settings(loaded),
            needs(&loaded.config).into_iter().collect(),
        ),
        Err(why) => (
            EntryState {
                word: "Off".into(),
                on: Some(false),
                why: format!("Off: its settings do not read: {why}."),
            },
            Vec::new(),
            Vec::new(),
        ),
    };
    let kept = kept(ctx);
    SettingsEntry {
        id: ID.into(),
        name: "GitHub".into(),
        about: "Issues exchanged, lands delivered by pull request, releases published.".into(),
        state: Some(EntryState {
            why: format!("{} {kept}", state.why),
            ..state
        }),
        needs,
        settings,
    }
}

/// On where the committed settings enable it, with what they turn on; off otherwise, and why.
fn state(loaded: &Loaded) -> EntryState {
    let config = &loaded.config;
    let place = &loaded.place;
    if config.enabled {
        let repository = config
            .repository
            .as_ref()
            .map_or_else(|| "no repository".into(), config::Repository::full_name);
        EntryState {
            word: "On".into(),
            on: Some(true),
            why: format!(
                "Enabled by {place} for {repository}: issues {}, pull requests {}, releases {}.",
                config.issues.word(),
                config.pull_requests.word(),
                config.releases.word()
            ),
        }
    } else if loaded.text.is_none() {
        EntryState {
            word: "Off".into(),
            on: Some(false),
            why: format!("Off: no {place}, so nothing is read from or written to GitHub."),
        }
    } else {
        EntryState {
            word: "Off".into(),
            on: Some(false),
            why: format!(
                "Off: {place} does not enable it, so nothing is read from or written to GitHub."
            ),
        }
    }
}

/// What is kept, in a sentence: the repository it is bound to, a pause, what is mapped.
fn kept(ctx: &Context<'_>) -> String {
    let folder = match ctx.board.state_folder() {
        Ok(folder) => folder,
        Err(why) => return format!("What it keeps cannot be read: {}.", why.said()),
    };
    let Some(store) = Store::existing(&folder) else {
        return "Nothing is kept.".into();
    };
    let state = match store.state() {
        Ok(state) => state.unwrap_or_default(),
        Err(why) => return format!("What it keeps does not read: {why}."),
    };
    let bound = match &state.binding {
        Some(binding) => format!("Bound to {}", binding.full_name),
        None => "Bound to no repository yet".into(),
    };
    let mut said = vec![format!(
        "{bound}, {} mapped, {} curated texts, {} delivered by pull request.",
        state.mappings.len(),
        state.drafts.len(),
        state.pulls.len()
    )];
    if let Some(why) = &state.paused {
        said.push(format!("Paused: {why}."));
    }
    if let Some(until) = state.rate_limited_until.filter(|until| *until > ctx.now) {
        said.push(format!("Rate limited until {until} (seconds since 1970)."));
    }
    said.join(" ")
}

/// The token file an enabled exchange sends from: whether this machine holds it.
fn needs(config: &Config) -> Option<EntryNeed> {
    let file = config.token_file.as_ref().filter(|_| config.enabled)?;
    let met = file.is_file();
    Some(EntryNeed {
        what: "the token file".into(),
        met,
        said: if met {
            format!("Found on this machine: {}.", file.display())
        } else {
            format!(
                "Not found on this machine: {}; nothing is sent from here until it holds a \
                 token (docs/operations/GITHUB_SETUP.md).",
                file.display()
            )
        },
    })
}

/// Each setting the page shows: what the exchange does, and with which repository and token.
fn settings(loaded: &Loaded) -> Vec<Setting> {
    let config = &loaded.config;
    let words = |words: &[&str]| words.iter().map(|word| (*word).to_string()).collect();
    let mut settings = vec![
        setting(
            loaded,
            "enabled",
            "Whether the exchange runs",
            SettingKind::Switch,
            if config.enabled { "on" } else { "off" },
            Vec::new(),
        ),
        setting(
            loaded,
            "repository",
            "The one GitHub repository it reaches",
            SettingKind::Text,
            &config
                .repository
                .as_ref()
                .map_or_else(|| "none".into(), config::Repository::full_name),
            Vec::new(),
        ),
        setting(
            loaded,
            "issues",
            "What it does with issues",
            SettingKind::Choice,
            config.issues.word(),
            words(&["off", "import", "export", "exchange"]),
        ),
        setting(
            loaded,
            "pull_requests",
            "Whether a land is delivered by a pull request",
            SettingKind::Choice,
            config.pull_requests.word(),
            words(&[
                PullRequests::Off.word(),
                PullRequests::Track.word(),
                PullRequests::Publish.word(),
            ]),
        ),
        setting(
            loaded,
            "releases",
            "Whether a release is published as a GitHub Release",
            SettingKind::Choice,
            config.releases.word(),
            words(&[Releases::Off.word(), Releases::Publish.word()]),
        ),
        setting(
            loaded,
            "verification",
            "Whether a release is verified on GitHub's machines",
            SettingKind::Choice,
            config.verification.word(),
            words(&[Verification::Off.word(), Verification::Hosted.word()]),
        ),
        setting(
            loaded,
            "token_file",
            "Where its token is kept, never the token",
            SettingKind::Text,
            &config
                .token_file
                .as_ref()
                .map_or_else(|| "none".into(), |file| file.display().to_string()),
            Vec::new(),
        ),
    ];
    if let Some(app) = config.app_id {
        settings.push(setting(
            loaded,
            "app_id",
            "The GitHub App its token is of",
            SettingKind::Number,
            &app.to_string(),
            Vec::new(),
        ));
    }
    settings
}

/// The setting `key`, its value in force `value`, written where the settings' text writes it, or
/// the default.
fn setting(
    loaded: &Loaded,
    key: &str,
    name: &str,
    kind: SettingKind,
    value: &str,
    choices: Vec<String>,
) -> Setting {
    let line = (loaded.text.as_deref()).and_then(|text| config::line(text, key));
    let source = match line {
        Some(line) => SettingSource {
            scope: Some(SettingScope::Project),
            said: format!("{} {key}, line {line}", loaded.place),
            file: Some(config::FILE.into()),
            line: Some(line),
        },
        None => SettingSource {
            scope: None,
            said: format!("default: {} does not write it", loaded.place),
            file: None,
            line: None,
        },
    };
    Setting {
        key: key.into(),
        name: name.into(),
        kind,
        value: value.into(),
        choices,
        source,
        scopes: vec![SettingScope::Project],
        class: SettingClass::Reviewed,
    }
}
