//! What a reconcile would do, decided from what was read and nothing else: [`plan`] reads the
//! settings, the kept state, the issues listed and the board's tasks, and gives the actions of a
//! [`Preview`] in order. It makes no request and writes nothing, so the same reading gives the
//! same preview, and the preview's digest names exactly what an operator saw.
//!
//! Each side of a mapping is compared with the digest it had when the two were last
//! synchronized. One changed side may update the other; two changed sides are a conflict, which
//! overwrites neither. No time is compared.

use crate::board::{Task, Token};
use crate::config::Config;
use crate::github::{Comment, Issue, Repo};
use crate::state::{Binding, Draft, Flag, Mapping, Origin, Side, State};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A change to what the exchange keeps of one mapping, with no write outside its folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    /// The curated text takes the issue's, whose digest both sides now have.
    Draft {
        draft: Draft,
        digest: String,
    },
    /// The sides are the same again, or an operator kept the local one: these are their digests.
    Synced {
        local: String,
        remote: String,
    },
    Flag(Option<Flag>),
    RemoteState(String),
    CommentsSince {
        since: String,
        seen: Vec<i64>,
    },
}

/// One operation of a preview.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    /// The state is bound to this repository.
    Bind {
        binding: Binding,
    },
    /// A task and an issue already exchanged are mapped again, nothing written to either.
    Adopt {
        slug: String,
        mapping: Mapping,
        draft: Option<Draft>,
    },
    /// An issue becomes a proposal on the board.
    Import {
        slug: String,
        issue: Issue,
        title: String,
        body: String,
    },
    /// An issue's edit reaches its task, by the task's token.
    Pull {
        slug: String,
        token: Token,
        title: String,
        body: String,
        /// The remote digest the mapping then holds.
        remote: String,
    },
    /// A task's curated text becomes an issue.
    Publish {
        slug: String,
        marker: String,
        title: String,
        body: String,
        labels: Vec<String>,
    },
    /// A curated text's edit reaches its issue, which must still hold `base`.
    Push {
        slug: String,
        issue_number: i64,
        base: String,
        title: String,
        body: String,
    },
    /// A note on the task's thread, written once for its key.
    Note {
        slug: String,
        key: String,
        text: String,
        to: Option<String>,
    },
    Local {
        slug: String,
        change: Change,
    },
}

impl Action {
    /// The task it is for; the repository's name for a binding.
    #[must_use]
    pub fn target(&self) -> &str {
        match self {
            Action::Bind { binding } => &binding.full_name,
            Action::Adopt { slug, .. }
            | Action::Import { slug, .. }
            | Action::Pull { slug, .. }
            | Action::Publish { slug, .. }
            | Action::Push { slug, .. }
            | Action::Note { slug, .. }
            | Action::Local { slug, .. } => slug,
        }
    }

    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Action::Bind { .. } => "bind",
            Action::Adopt { .. } => "adopt",
            Action::Import { .. } => "import",
            Action::Pull { .. } => "pull",
            Action::Publish { .. } => "publish",
            Action::Push { .. } => "push",
            Action::Note { .. } => "note",
            Action::Local { .. } => "local",
        }
    }

    /// Whether applying it writes outside the adapter's own folder: GitHub, or the board.
    #[must_use]
    pub fn dispatches(&self) -> bool {
        !matches!(
            self,
            Action::Bind { .. } | Action::Adopt { .. } | Action::Local { .. }
        )
    }
}

/// What a reconcile would do, as an operator was shown it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preview {
    /// The digest of the settings' revision, the actions and the cursor: its name.
    pub digest: String,
    /// The settings' revision it was made under.
    pub config: String,
    pub actions: Vec<Action>,
    /// The cursor the state takes once every action is acknowledged; none leaves it.
    pub next_cursor: Option<String>,
    /// What was seen and is not acted on.
    pub notices: Vec<String>,
}

impl Preview {
    /// A preview of `actions`, named by its digest.
    #[must_use]
    pub fn new(
        config: &str,
        actions: Vec<Action>,
        next_cursor: Option<String>,
        notices: Vec<String>,
    ) -> Preview {
        let mut preview = Preview {
            digest: String::new(),
            config: config.into(),
            actions,
            next_cursor,
            notices,
        };
        preview.digest = preview.computed();
        preview
    }

    /// The digest its content has now, which a preview read back from disk must still carry.
    #[must_use]
    pub fn computed(&self) -> String {
        let content = serde_json::to_string(&(&self.config, &self.actions, &self.next_cursor))
            .unwrap_or_default();
        crate::digest(&[&content])
    }

    /// The id of its action at `index`: a publication's marker, which is the same in every
    /// preview, else its place in this one.
    #[must_use]
    pub fn op(&self, index: usize) -> String {
        match &self.actions[index] {
            Action::Publish { marker, .. } => marker.clone(),
            _ => format!(
                "{}:{index}",
                self.digest.chars().take(16).collect::<String>()
            ),
        }
    }
}

/// The digest of a title and a body.
#[must_use]
pub fn content(title: &str, body: &str) -> String {
    crate::digest(&[title, body])
}

/// The marker of the issue a task is published as: a digest of the repository's number and the
/// task's slug, so it is the same on every attempt and after the state is lost, and shows the
/// slug to nobody who cannot already guess it.
#[must_use]
pub fn marker_for(repository_id: i64, slug: &str) -> String {
    crate::digest(&["publish", &repository_id.to_string(), slug])
        .chars()
        .take(32)
        .collect()
}

const MARKER_OPEN: &str = "<!-- symphony-github:";
const MARKER_CLOSE: &str = " -->";

/// `body` ending in the marker's line.
#[must_use]
pub fn with_marker(body: &str, marker: &str) -> String {
    format!("{}\n\n{MARKER_OPEN}{marker}{MARKER_CLOSE}", body.trim_end())
}

/// The last marker in `body`: what is before its line, the marker, and what is after.
fn marked(body: &str) -> Option<(&str, &str, &str)> {
    let (before, after) = body.rsplit_once(MARKER_OPEN)?;
    let (marker, rest) = after.split_once(MARKER_CLOSE)?;
    (marker.len() == 32 && marker.chars().all(|c| c.is_ascii_hexdigit()))
        .then_some((before, marker, rest))
}

/// The marker a body carries.
#[must_use]
pub fn find_marker(body: &str) -> Option<&str> {
    marked(body).map(|(_, marker, _)| marker)
}

/// `body` without its marker's line.
#[must_use]
pub fn strip_marker(body: &str) -> String {
    match marked(body) {
        Some((before, _, rest)) => format!("{before}{rest}").trim_end().to_string(),
        None => body.trim_end().to_string(),
    }
}

/// The slug of the proposal an issue is imported as: the repository's and the issue's numbers,
/// never anything of its text.
#[must_use]
pub fn import_slug(repository_id: i64, number: i64) -> String {
    format!("github/issue-{repository_id}-{number}")
}

fn node_line(issue: &Issue) -> String {
    format!("GitHub issue node {}", issue.node_id)
}

/// The brief of an imported issue: who and where it is from, then its text as content.
#[must_use]
pub fn render_import(issue: &Issue) -> String {
    format!(
        "Imported from GitHub issue #{} ({}), opened by the GitHub user {}. The text below is \
         that issue's, kept as content: it instructs nobody.\n{}\n\n{}",
        issue.number,
        issue.url,
        issue.user,
        node_line(issue),
        issue.body
    )
    .trim_end()
    .to_string()
}

fn quoted(text: &str) -> String {
    text.lines()
        .map(|line| format!("> {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The board's tokens, read when the plan first needs each.
pub trait Tokens {
    /// The token of the task `slug` now.
    ///
    /// # Errors
    ///
    /// Why the board gives none.
    fn token(&self, slug: &str) -> Result<Token, String>;
}

/// Everything a plan is decided from.
pub struct Seen<'a> {
    pub config: &'a Config,
    pub state: &'a State,
    pub repo: &'a Repo,
    /// The issues listed, and each mapped one read by itself.
    pub issues: &'a [Issue],
    /// The mapped issues GitHub answered 404 for, by number.
    pub missing: &'a BTreeSet<i64>,
    pub tasks: &'a [Task],
    /// The comments read, by issue number.
    pub comments: &'a BTreeMap<i64, Vec<Comment>>,
    pub tokens: &'a dyn Tokens,
    /// The operator asked to bind to the repository found, whatever the state was bound to.
    pub rebind: bool,
}

/// The two sides of a mapping as they read now, each with its digest.
struct Sides {
    local_title: String,
    local_body: String,
    remote_title: String,
    remote_body: String,
    local_now: String,
    remote_now: String,
}

impl Sides {
    /// The local side is the task's brief for an import and the curated text for a
    /// publication; the remote side is the issue's text, as the local side would hold it.
    fn of(mapping: &Mapping, task: &Task, draft: Option<&Draft>, issue: &Issue) -> Sides {
        let (local_title, local_body) = match (mapping.origin, draft) {
            (Origin::Import, _) => (task.title.clone(), task.body.clone()),
            (Origin::Export, Some(draft)) => (draft.title.clone(), draft.body.clone()),
            (Origin::Export, None) => (String::new(), String::new()),
        };
        let (remote_title, remote_body) = match mapping.origin {
            Origin::Import => (issue.title.clone(), render_import(issue)),
            Origin::Export => (issue.title.clone(), strip_marker(&issue.body)),
        };
        let local_now = match (mapping.origin, draft) {
            // A publication whose curated text is gone has not changed here.
            (Origin::Export, None) => mapping.local.clone(),
            _ => content(&local_title, &local_body),
        };
        let remote_now = match mapping.origin {
            Origin::Import => content(&issue.title, &issue.body),
            Origin::Export => content(&remote_title, &remote_body),
        };
        Sides {
            local_title,
            local_body,
            remote_title,
            remote_body,
            local_now,
            remote_now,
        }
    }

    fn agree(&self) -> bool {
        self.local_title == self.remote_title && self.local_body == self.remote_body
    }
}

struct Planner<'a> {
    seen: &'a Seen<'a>,
    tasks: BTreeMap<&'a str, &'a Task>,
    actions: Vec<Action>,
    notices: Vec<String>,
    adopted: BTreeSet<String>,
}

impl<'a> Planner<'a> {
    fn local(&mut self, slug: &str, change: Change) {
        self.actions.push(Action::Local {
            slug: slug.into(),
            change,
        });
    }

    fn flag(&mut self, slug: &str, mapping: &Mapping, flag: Option<Flag>) {
        if mapping.flag != flag {
            self.local(slug, Change::Flag(flag));
        }
    }

    fn note(&mut self, slug: &str, key: String, text: String, to: Option<&str>) {
        self.actions.push(Action::Note {
            slug: slug.into(),
            key,
            text,
            to: to.map(str::to_string),
        });
    }

    /// The task a mapping still names, with its token under the mapping's generation; a
    /// mapping that names another is suspended, never moved to it.
    fn still(&mut self, slug: &str, mapping: &Mapping) -> Option<(&'a Task, Token)> {
        let suspended = |why: &str| Some(Flag::Suspended(why.into()));
        let Some(task) = self.tasks.get(slug).copied() else {
            self.flag(
                slug,
                mapping,
                suspended("the task is no longer on the board"),
            );
            return None;
        };
        if task.id != mapping.task_id {
            self.flag(slug, mapping, suspended("the slug names another task now"));
            return None;
        }
        match self.seen.tokens.token(slug) {
            Ok(token) if token.generation() == mapping.generation && token.id() == task.id => {
                Some((task, token))
            }
            Ok(_) => {
                self.flag(
                    slug,
                    mapping,
                    suspended("the board was restored or rebound since this mapping was kept"),
                );
                None
            }
            Err(why) => {
                self.notices
                    .push(format!("{slug}: the board gives no token: {why}"));
                None
            }
        }
    }

    /// What an issue's state and comments ask of its task's thread: notes, and nothing moved.
    fn news(&mut self, slug: &str, mapping: &Mapping, issue: &Issue) {
        if issue.state != mapping.remote_state {
            self.note(
                slug,
                format!(
                    "github:state:{}:{}:{}",
                    issue.id, issue.state, issue.updated_at
                ),
                format!(
                    "GitHub issue #{} ({}) is now {}. This is a triage request: nothing here \
                     was moved, closed or reopened by it.",
                    issue.number, issue.url, issue.state
                ),
                Some("@orchestrator"),
            );
            self.local(slug, Change::RemoteState(issue.state.clone()));
        }
        let Some(comments) = self.seen.comments.get(&issue.number) else {
            return;
        };
        for comment in comments {
            self.note(
                slug,
                format!("github:comment:{}", comment.id),
                format!(
                    "The GitHub user {} commented on issue #{} ({}):\n\n{}",
                    comment.user,
                    issue.number,
                    comment.url,
                    quoted(&comment.body)
                ),
                None,
            );
        }
        if let Some(latest) = comments.iter().map(|c| &c.updated_at).max() {
            self.local(
                slug,
                Change::CommentsSince {
                    since: latest.clone(),
                    seen: comments
                        .iter()
                        .filter(|c| c.updated_at == *latest)
                        .map(|c| c.id)
                        .collect(),
                },
            );
        }
    }

    /// The issue's text reaches the local side: the task's brief by its token for an import,
    /// the curated text for a publication.
    fn take_remote(&mut self, slug: &str, mapping: &Mapping, token: &Token, sides: &Sides) {
        match mapping.origin {
            Origin::Import => self.actions.push(Action::Pull {
                slug: slug.into(),
                token: token.clone(),
                title: sides.remote_title.clone(),
                body: sides.remote_body.clone(),
                remote: sides.remote_now.clone(),
            }),
            Origin::Export => self.local(
                slug,
                Change::Draft {
                    draft: Draft {
                        title: sides.remote_title.clone(),
                        body: sides.remote_body.clone(),
                    },
                    digest: sides.remote_now.clone(),
                },
            ),
        }
        self.flag(slug, mapping, None);
    }

    /// The curated text reaches the issue, which must still hold `base`.
    fn push(&mut self, slug: &str, mapping: &Mapping, issue: &Issue, sides: &Sides, base: &str) {
        let marker = marker_for(self.seen.repo.id, slug);
        self.actions.push(Action::Push {
            slug: slug.into(),
            issue_number: issue.number,
            base: base.into(),
            title: sides.local_title.clone(),
            body: with_marker(&sides.local_body, &marker),
        });
        self.flag(slug, mapping, None);
    }

    fn synced(&mut self, slug: &str, mapping: &Mapping, local: &str, remote: &str) {
        self.local(
            slug,
            Change::Synced {
                local: local.into(),
                remote: remote.into(),
            },
        );
        self.flag(slug, mapping, None);
    }

    /// Both sides changed and differ: what the operator kept decides, and with nothing kept
    /// it is a conflict, said once, that overwrites neither.
    fn diverged(
        &mut self,
        slug: &str,
        mapping: &Mapping,
        issue: &Issue,
        token: &Token,
        sides: &Sides,
    ) {
        match self.seen.state.resolutions.get(slug) {
            Some(Side::Remote) => self.take_remote(slug, mapping, token, sides),
            Some(Side::Local) if mapping.origin == Origin::Export => {
                self.push(slug, mapping, issue, sides, &sides.remote_now);
            }
            // An imported brief kept as it is here: the issue's edit is recorded as seen, and
            // the brief stays changed, so a later edit there conflicts again rather than
            // replacing what was kept.
            Some(Side::Local) => self.synced(slug, mapping, &mapping.local, &sides.remote_now),
            None => {
                if mapping.flag.is_none() {
                    let seen: String = sides.remote_now.chars().take(12).collect();
                    self.note(
                        slug,
                        format!("github:conflict:{}:{seen}", issue.id),
                        format!(
                            "GitHub issue #{} ({}) and this task's side of it both changed \
                             since they were last the same. Neither was overwritten: keep one \
                             with `symphony-github resolve {slug} local` or `remote`, then \
                             preview again.",
                            issue.number, issue.url
                        ),
                        Some("@orchestrator"),
                    );
                }
                self.flag(slug, mapping, Some(Flag::Conflict));
            }
        }
    }

    fn mapped(&mut self, slug: &str, mapping: &Mapping, issue: &Issue) {
        if let Some(Flag::Suspended(why)) = &mapping.flag {
            self.notices
                .push(format!("{slug}: suspended, not exchanged: {why}"));
            return;
        }
        let Some((task, token)) = self.still(slug, mapping) else {
            return;
        };
        self.news(slug, mapping, issue);
        let config = self.seen.config;
        let sides = Sides::of(mapping, task, self.seen.state.drafts.get(slug), issue);
        let local_changed = sides.local_now != mapping.local;
        let remote_changed = sides.remote_now != mapping.remote;
        match (local_changed, remote_changed) {
            (false, false) => self.flag(slug, mapping, None),
            (false, true) if config.issues.imports() => {
                self.take_remote(slug, mapping, &token, &sides);
            }
            (false, true) => self.notices.push(format!(
                "{slug}: issue #{} was edited; not taken, as issues is {}",
                issue.number,
                config.issues.word()
            )),
            // An imported task's own brief is never published: only curated text is.
            (true, false) if mapping.origin == Origin::Import => {}
            (true, false) if config.issues.exports() => {
                self.push(slug, mapping, issue, &sides, &mapping.remote);
            }
            (true, false) => self.notices.push(format!(
                "{slug}: its curated text changed; not published, as issues is {}",
                config.issues.word()
            )),
            (true, true) if sides.agree() => {
                self.synced(slug, mapping, &sides.local_now, &sides.remote_now);
            }
            (true, true) => self.diverged(slug, mapping, issue, &token, &sides),
        }
    }

    /// A mapping for `issue` and the task `slug` found without one: the state was lost, or an
    /// answer was. The digests kept are the sides' own when they agree, and otherwise ones
    /// neither side has, so the next preview reads a conflict and overwrites neither.
    fn adopt(&mut self, slug: &str, task: &Task, issue: &Issue, origin: Origin) {
        let token = match self.seen.tokens.token(slug) {
            Ok(token) => token,
            Err(why) => {
                self.notices.push(format!(
                    "{slug}: not mapped again: the board gives no token: {why}"
                ));
                return;
            }
        };
        let kept = self.seen.state.drafts.get(slug).cloned();
        let (local, remote, draft) = match origin {
            Origin::Import => {
                let agree = task.title == issue.title && task.body == render_import(issue);
                if agree {
                    (
                        content(&task.title, &task.body),
                        content(&issue.title, &issue.body),
                        None,
                    )
                } else {
                    ("unknown".into(), "unknown".into(), None)
                }
            }
            Origin::Export => {
                let body = strip_marker(&issue.body);
                let digest = content(&issue.title, &body);
                let draft = kept.is_none().then(|| Draft {
                    title: issue.title.clone(),
                    body,
                });
                (digest.clone(), digest, draft)
            }
        };
        self.adopted.insert(slug.into());
        self.actions.push(Action::Adopt {
            slug: slug.into(),
            mapping: Mapping {
                origin,
                task_id: task.id,
                generation: token.generation().into(),
                issue_number: issue.number,
                issue_id: issue.id,
                issue_node: issue.node_id.clone(),
                url: issue.url.clone(),
                local,
                remote,
                remote_state: issue.state.clone(),
                comments_since: None,
                comments_seen: Vec::new(),
                flag: None,
            },
            draft,
        });
    }

    fn unmapped(&mut self, issue: &Issue, marked: &BTreeMap<&str, usize>) {
        let seen = self.seen;
        let free = |slug: &str| !seen.state.mappings.contains_key(slug);
        if let Some(marker) = find_marker(&issue.body) {
            let published = seen
                .tasks
                .iter()
                .find(|task| free(&task.slug) && marker_for(seen.repo.id, &task.slug) == marker);
            if let Some(task) = published {
                if marked.get(marker).copied().unwrap_or(0) > 1 {
                    self.adopted.insert(task.slug.clone());
                    self.notices.push(format!(
                        "{}: more than one issue carries its marker (#{} among them): none is \
                         mapped and none is published again; close the duplicates, then preview",
                        task.slug, issue.number
                    ));
                } else {
                    self.adopt(&task.slug, task, issue, Origin::Export);
                }
                return;
            }
        }
        let slug = import_slug(seen.repo.id, issue.number);
        if let Some(task) = self.tasks.get(slug.as_str()).copied() {
            if !free(&slug) {
                self.notices.push(format!(
                    "{slug}: mapped to another issue than #{}: not touched",
                    issue.number
                ));
            } else if task.body.lines().any(|line| line == node_line(issue)) {
                self.adopt(&slug, task, issue, Origin::Import);
            } else {
                self.notices.push(format!(
                    "{slug}: a task of that name is not issue #{}'s proposal: not adopted and \
                     not imported",
                    issue.number
                ));
            }
            return;
        }
        let selected = issue
            .labels
            .iter()
            .any(|label| seen.config.import_labels.contains(label));
        if seen.config.issues.imports() && selected && issue.state == "open" {
            self.actions.push(Action::Import {
                slug,
                issue: issue.clone(),
                title: issue.title.clone(),
                body: render_import(issue),
            });
        }
    }

    fn drafts(&mut self) {
        let seen = self.seen;
        for (slug, draft) in &seen.state.drafts {
            if seen.state.mappings.contains_key(slug) || self.adopted.contains(slug) {
                continue;
            }
            if !seen.config.issues.exports() {
                self.notices.push(format!(
                    "{slug}: its curated text waits; not published, as issues is {}",
                    seen.config.issues.word()
                ));
            } else if !self.tasks.contains_key(slug.as_str()) {
                self.notices
                    .push(format!("{slug}: no such task on the board: not published"));
            } else {
                let marker = marker_for(seen.repo.id, slug);
                self.actions.push(Action::Publish {
                    slug: slug.clone(),
                    title: draft.title.clone(),
                    body: with_marker(&draft.body, &marker),
                    labels: seen.config.export_label.iter().cloned().collect(),
                    marker,
                });
            }
        }
    }
}

/// The actions and the notices of what was seen.
#[must_use]
pub fn plan(seen: &Seen<'_>) -> (Vec<Action>, Vec<String>) {
    let mut planner = Planner {
        seen,
        tasks: seen
            .tasks
            .iter()
            .map(|task| (task.slug.as_str(), task))
            .collect(),
        actions: Vec::new(),
        notices: Vec::new(),
        adopted: BTreeSet::new(),
    };
    let found = Binding {
        repository_id: seen.repo.id,
        node_id: seen.repo.node_id.clone(),
        full_name: seen.repo.full_name.clone(),
    };
    match &seen.state.binding {
        Some(bound) if bound.repository_id != found.repository_id => {
            // Another repository: its issues are not this one's, so every mapping waits for
            // its operator, kept and neither deleted nor published again.
            planner.actions.push(Action::Bind { binding: found });
            for (slug, mapping) in &seen.state.mappings {
                planner.flag(
                    slug,
                    mapping,
                    Some(Flag::Suspended(format!(
                        "kept for the repository {} (id {})",
                        bound.full_name, bound.repository_id
                    ))),
                );
            }
            return (planner.actions, planner.notices);
        }
        Some(bound) if *bound == found => {}
        _ => planner.actions.push(Action::Bind { binding: found }),
    }

    let by_issue: BTreeMap<i64, &str> = seen
        .state
        .mappings
        .iter()
        .map(|(slug, mapping)| (mapping.issue_id, slug.as_str()))
        .collect();
    let mut marked: BTreeMap<&str, usize> = BTreeMap::new();
    for issue in seen.issues {
        if !by_issue.contains_key(&issue.id)
            && let Some(marker) = find_marker(&issue.body)
        {
            *marked.entry(marker).or_default() += 1;
        }
    }
    for issue in seen.issues {
        match by_issue.get(&issue.id) {
            Some(slug) => planner.mapped(slug, &seen.state.mappings[*slug], issue),
            None => planner.unmapped(issue, &marked),
        }
    }
    for (slug, mapping) in &seen.state.mappings {
        if seen.missing.contains(&mapping.issue_number) {
            planner.flag(slug, mapping, Some(Flag::Inaccessible));
        }
    }
    planner.drafts();
    (planner.actions, planner.notices)
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
