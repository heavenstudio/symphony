//! The two halves of a manual reconcile. [`preview`] reads both sides and keeps what it would
//! do, named by a digest; [`apply`] does exactly the preview it is handed by that digest, one
//! operation at a time, each journaled before it is dispatched and acknowledged before anything
//! kept moves on by it.
//!
//! An apply is safe to run twice and to interrupt. A second run of a preview already applied
//! dispatches nothing. A run cut short is run again with the same digest: an operation
//! acknowledged is not made again, and one whose result is not known is looked for before it is
//! tried (a created issue by its marker, an edit by reading the side it wrote). The board's
//! brief is rewritten only by its token, and a token the board refuses ends the run and drops
//! the preview: the next preview reads the task again.

use crate::board::{Board, BoardError, Bound, Token};
use crate::config::{Config, Loaded, Repository, Source};
use crate::credential::Secret;
use crate::github::{ApiError, Client, Comment, Issue};
use crate::mint;
use crate::plan::{self, Action, Change, Preview, Seen, Tokens};
use crate::state::{Entry, Event, Flag, Mapping, Origin, State, Store};
use crate::transport::Transport;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The pages of issues one preview reads at most; what is past them waits for the next.
const MAX_PAGES: usize = 20;
/// The pages a search for a lost publication reads at most: all of the repository's issues, or
/// the search is not trusted.
const SEARCH_PAGES: usize = 1000;
/// How long nothing is sent after a rate limit that named no time.
const RATE_PAUSE: i64 = 60;

/// What a run is given.
pub struct Context<'a> {
    pub source: &'a dyn Source,
    pub board: &'a dyn Board,
    /// What makes the requests that carry this token.
    pub connect: &'a dyn Fn(Secret) -> Result<Box<dyn Transport>, String>,
    /// Seconds since 1970.
    pub now: i64,
    /// The repository's folders, which the token file must be outside.
    pub repository: Vec<PathBuf>,
    /// The program that signs a GitHub App's JSON Web Token ([`crate::mint`]): `openssl`.
    pub openssl: &'a str,
    /// The gh CLI that reads the rulesets as who is signed in to it ([`crate::rulesets`]): `gh`.
    pub gh: &'a str,
    /// What a delivery rests by between two reads of the checks it wrote, which GitHub lists a
    /// little after it takes them: [`crate::deliver::rest`], or a test's, which waits on nothing.
    pub rest: &'a dyn Fn(Duration),
}

/// How a command ended: its exit code, having said why on `out`.
pub type Outcome = Result<i32, String>;

struct BoardTokens<'a> {
    board: &'a dyn Board,
    read: RefCell<BTreeMap<String, Result<Bound, String>>>,
}

impl Tokens for BoardTokens<'_> {
    fn bound(&self, slug: &str) -> Result<Bound, String> {
        self.read
            .borrow_mut()
            .entry(slug.into())
            .or_insert_with(|| {
                self.board
                    .bound(slug)
                    .map_err(|error| error.said().to_string())
            })
            .clone()
    }
}

fn board(error: &BoardError) -> String {
    format!("the board: {}", error.said())
}

/// The settings, or why nothing may be done: they do not read, or the adapter is turned off.
fn enabled(ctx: &Context<'_>) -> Result<Loaded, String> {
    let loaded = ctx
        .source
        .load()
        .map_err(|why| format!("turned off: its settings do not read: {why}"))?;
    if loaded.config.enabled {
        Ok(loaded)
    } else {
        Err("turned off: the committed settings do not enable it".into())
    }
}

fn connect(ctx: &Context<'_>, loaded: &Loaded) -> Result<Box<dyn Transport>, String> {
    let token = mint::token(ctx, &loaded.config).map_err(mint::Stop::said)?;
    (ctx.connect)(token)
}

/// What an API failure does to the state: a refused credential or a repository not found
/// pauses, a rate limit sets the time nothing is sent before. The line to say.
fn failed(state: &mut State, error: &ApiError, now: i64, doing: &str) -> String {
    match error {
        ApiError::Unauthorized => {
            state.paused = Some(
                "GitHub refused the credential: nothing is sent until a preview succeeds".into(),
            );
        }
        ApiError::NotFound | ApiError::Moved if doing == "reading the repository" => {
            state.paused = Some(
                "the repository the settings name is missing, renamed or not accessible: \
                 nothing is sent until the committed settings name it and a preview succeeds"
                    .into(),
            );
        }
        ApiError::RateLimited(until) => {
            state.rate_limited_until = Some(until.unwrap_or(now + RATE_PAUSE));
        }
        _ => {}
    }
    format!("{doing}: {}", error.said())
}

fn limited(state: &State, now: i64) -> Result<(), String> {
    match state.rate_limited_until {
        Some(until) if until > now => Err(format!(
            "GitHub's rate limit holds until {until} (seconds since 1970, {} from now): \
             nothing was sent",
            until - now
        )),
        _ => Ok(()),
    }
}

fn show(out: &mut dyn Write, label: &str, title: &str, body: &str) {
    let _ = writeln!(out, "    {label} title: {title}");
    for line in body.lines() {
        let _ = writeln!(out, "    | {line}");
    }
}

/// What `preview` prints of the preview: each action, with the exact text of every write.
pub fn print(out: &mut dyn Write, preview: &Preview) {
    for (index, action) in preview.actions.iter().enumerate() {
        let n = index + 1;
        match action {
            Action::Bind { binding } => {
                let _ = writeln!(
                    out,
                    "{n}. bind this state to the repository {} (id {})",
                    binding.full_name, binding.repository_id
                );
            }
            Action::Adopt { slug, mapping, .. } => {
                let _ = writeln!(
                    out,
                    "{n}. map {slug} to issue #{} again; nothing is written to either",
                    mapping.issue_number
                );
            }
            Action::Import {
                slug,
                issue,
                title,
                body,
            } => {
                let _ = writeln!(
                    out,
                    "{n}. import issue #{} as the backlog proposal {slug}",
                    issue.number
                );
                show(out, "local", title, body);
            }
            Action::Pull {
                slug, title, body, ..
            } => {
                let _ = writeln!(out, "{n}. rewrite {slug} from its issue's edit");
                show(out, "local", title, body);
            }
            Action::Publish {
                slug,
                title,
                body,
                labels,
                ..
            } => {
                let _ = writeln!(
                    out,
                    "{n}. publish {slug} as a new public issue, labels {labels:?}"
                );
                show(out, "public", title, body);
            }
            Action::Push {
                slug,
                issue_number,
                title,
                body,
                ..
            } => {
                let _ = writeln!(
                    out,
                    "{n}. rewrite issue #{issue_number} from {slug}'s curated text"
                );
                show(out, "public", title, body);
            }
            Action::Note { slug, text, to, .. } => {
                let to = to.as_deref().unwrap_or("its thread");
                let _ = writeln!(out, "{n}. note on {slug} to {to}");
                show(out, "note", "", text);
            }
            Action::Local { slug, change } => {
                let what = match change {
                    Change::Draft { .. } => "its curated text takes the issue's edit".to_string(),
                    Change::Synced { .. } => "both sides are recorded as synchronized".into(),
                    Change::Flag(Some(flag)) => flag.said(),
                    Change::Flag(None) => "exchanged again".into(),
                    Change::RemoteState(state) => format!("its issue is recorded as {state}"),
                    Change::CommentsSince { since, .. } => {
                        format!("its comments are read up to {since}")
                    }
                };
                let _ = writeln!(out, "{n}. keep of {slug}: {what}");
            }
        }
    }
    for notice in &preview.notices {
        let _ = writeln!(out, "note: {notice}");
    }
}

/// What a preview read of the repository's issues.
struct Gathered {
    /// The issues listed from the cursor, then each mapped one that had to be read by itself.
    issues: Vec<Issue>,
    /// The mapped issues answered 404, by number.
    missing: BTreeSet<i64>,
    /// The comments not read before, by issue number.
    comments: BTreeMap<i64, Vec<Comment>>,
    stable: bool,
    all: bool,
}

/// The issues changed since the cursor, each mapped issue whose task's side needs it looked at
/// (its curated text changed, an operator kept a side, or it is flagged), and the new comments
/// of the mapped ones where the settings ask for them.
fn gather(
    client: &Client<'_>,
    state: &State,
    config: &Config,
    cursor: Option<&str>,
) -> Result<Gathered, (ApiError, &'static str)> {
    let listing = client
        .issues(cursor, MAX_PAGES)
        .map_err(|error| (error, "listing the issues"))?;
    let mut issues = listing.issues;
    let mut missing = BTreeSet::new();
    let listed: BTreeSet<i64> = issues.iter().map(|issue| issue.id).collect();
    for (slug, mapping) in &state.mappings {
        let draft_changed = mapping.origin == Origin::Export
            && state
                .drafts
                .get(slug)
                .is_some_and(|draft| plan::content(&draft.title, &draft.body) != mapping.local);
        let look = draft_changed
            || state.resolutions.contains_key(slug)
            || matches!(mapping.flag, Some(Flag::Conflict | Flag::Inaccessible));
        if !look || listed.contains(&mapping.issue_id) {
            continue;
        }
        match client.issue(mapping.issue_number) {
            Ok(issue) => issues.push(issue),
            Err(ApiError::NotFound) => {
                missing.insert(mapping.issue_number);
            }
            Err(error) => return Err((error, "reading an issue")),
        }
    }
    let mut comments = BTreeMap::new();
    if config.import_comments && config.issues.imports() {
        for issue in &issues {
            let Some(mapping) = state.mappings.values().find(|m| m.issue_id == issue.id) else {
                continue;
            };
            let new: Vec<Comment> = client
                .comments(issue.number, mapping.comments_since.as_deref())
                .map_err(|error| (error, "reading comments"))?
                .into_iter()
                .filter(|comment| {
                    mapping.comments_since.as_deref() != Some(&comment.updated_at)
                        || !mapping.comments_seen.contains(&comment.id)
                })
                .collect();
            if !new.is_empty() {
                comments.insert(issue.number, new);
            }
        }
    }
    Ok(Gathered {
        issues,
        missing,
        comments,
        stable: listing.stable,
        all: listing.all,
    })
}

/// A preview while turned off: nothing remote is read, what waited is not kept to be replayed,
/// and each curated text not yet published is shown as it would be. Where nothing was kept, as
/// in a project that never turned it on, nothing is made: no folder and no lock.
fn turned_off(folder: &Path, why: &str, out: &mut dyn Write) -> Outcome {
    let _ = writeln!(out, "{why}. Nothing was read from GitHub.");
    let Some(store) = Store::existing(folder) else {
        return Ok(0);
    };
    let _lock = store.lock()?;
    store.drop_preview()?;
    let state = store.state()?.unwrap_or_default();
    for (slug, draft) in &state.drafts {
        if !state.mappings.contains_key(slug) {
            let _ = writeln!(out, "{slug} would be published as a new public issue:");
            show(out, "public", &draft.title, &draft.body);
        }
    }
    Ok(0)
}

/// The cursor, which holds only under the settings it was read under: an issue passed by
/// while it was not selected is listed again once a landing selects it.
fn cursor_under(state: &State, revision: &str, out: &mut dyn Write) -> Option<String> {
    let cursor = state
        .cursor
        .clone()
        .filter(|_| state.cursor_revision.as_deref() == Some(revision));
    if state.cursor.is_some() && cursor.is_none() {
        let _ = writeln!(
            out,
            "The committed settings changed since the issues were last listed: every issue is \
             listed again."
        );
    }
    cursor
}

/// The tasks whose publication the journal holds a word of.
fn journaled(store: &Store) -> Result<BTreeSet<String>, String> {
    Ok(store
        .entries()?
        .into_iter()
        .filter(|entry| entry.kind == "publish")
        .map(|entry| entry.target)
        .collect())
}

/// `symphony-github preview`: read both sides and keep what a reconcile would do.
///
/// # Errors
///
/// Why nothing was previewed.
pub fn preview(ctx: &Context<'_>, rebind: bool, out: &mut dyn Write) -> Outcome {
    let folder = ctx.board.state_folder().map_err(|e| board(&e))?;
    let loaded = match enabled(ctx) {
        Ok(loaded) => loaded,
        Err(why) => return turned_off(&folder, &why, out),
    };
    let store = Store::open(&folder)?;
    let _lock = store.lock()?;
    let mut state = store.state()?.unwrap_or_default();
    let config = &loaded.config;
    let repository = config.repository.as_ref().ok_or("no repository is named")?;
    limited(&state, ctx.now)?;
    let transport = connect(ctx, &loaded)?;
    let client = Client::new(transport.as_ref(), repository, ctx.now);
    // A failure is kept (a pause, a rate limit's time) before it is said.
    let keep = |state: &mut State, error: &ApiError, doing: &str| -> String {
        let why = failed(state, error, ctx.now, doing);
        let kept = store.save(state).and_then(|()| store.drop_preview());
        kept.err().unwrap_or(why)
    };

    let repo = match client.repository() {
        Ok(repo) => repo,
        Err(error) => return Err(keep(&mut state, &error, "reading the repository")),
    };
    if let Some(bound) = &state.binding
        && bound.repository_id != repo.id
        && !rebind
    {
        state.paused = Some(format!(
            "{} is now the repository id {}, not the id {} this state is bound to ({}): \
             nothing is sent until `symphony-github preview --rebind` is applied",
            repo.full_name, repo.id, bound.repository_id, bound.full_name
        ));
        store.save(&state)?;
        store.drop_preview()?;
        return Err(state.paused.clone().unwrap_or_default());
    }
    let cursor = cursor_under(&state, &loaded.revision, out);
    let published = journaled(&store)?;
    let read = match gather(&client, &state, config, cursor.as_deref()) {
        Ok(read) => read,
        Err((error, doing)) => return Err(keep(&mut state, &error, doing)),
    };
    let Gathered {
        issues,
        missing,
        comments,
        stable,
        all,
    } = read;
    let tasks = ctx.board.tasks().map_err(|e| board(&e))?;
    let tokens = BoardTokens {
        board: ctx.board,
        read: RefCell::default(),
    };
    let (actions, mut notices) = plan::plan(&Seen {
        config,
        state: &state,
        repo: &repo,
        issues: &issues,
        missing: &missing,
        tasks: &tasks,
        comments: &comments,
        tokens: &tokens,
        published: &published,
    });
    if config.pull_requests == crate::config::PullRequests::Track {
        notices.push("pull_requests is track: this build tracks no pull request".into());
    }
    let newest = issues.iter().map(|issue| &issue.updated_at).max().cloned();
    let next_cursor = if stable {
        newest.filter(|newest| cursor.as_ref() != Some(newest))
    } else {
        notices.push(
            "issues moved between pages while they were listed: the cursor stays, and the \
             next preview lists from it again"
                .into(),
        );
        None
    };
    if !all {
        notices.push(format!(
            "more than {MAX_PAGES} pages of issues changed: the rest wait for the next preview"
        ));
    }
    // The repository answered: what paused the exchange is over.
    state.paused = None;
    state.rate_limited_until = None;
    let preview = Preview::new(&loaded.revision, actions, next_cursor, notices);
    kept(&store, state, &preview, &repo.full_name, out)
}

/// A preview kept to be applied and shown; one with nothing to do is not kept, and its cursor
/// is taken at once.
fn kept(
    store: &Store,
    mut state: State,
    preview: &Preview,
    repository: &str,
    out: &mut dyn Write,
) -> Outcome {
    print(out, preview);
    if preview.actions.is_empty() {
        if let Some(cursor) = &preview.next_cursor {
            state.cursor = Some(cursor.clone());
            state.cursor_revision = Some(preview.config.clone());
        }
        store.save(&state)?;
        store.drop_preview()?;
        let _ = writeln!(out, "Nothing to reconcile with {repository}.");
        return Ok(0);
    }
    store.save(&state)?;
    store.keep_preview(preview)?;
    let _ = writeln!(
        out,
        "Nothing was written. Apply exactly this with:\n  symphony-github reconcile --preview {} \
         --as HANDLE",
        preview.digest
    );
    Ok(0)
}

/// How one operation ended short of its acknowledgment.
enum Stop {
    /// It is not to be made: the state holds what it found.
    Skip(String),
    /// The run ends; the event the journal takes, and whether the preview is dropped with it.
    End {
        why: String,
        event: Event,
        drop: bool,
    },
}

fn end(why: String, event: Event) -> Stop {
    Stop::End {
        why,
        event,
        drop: false,
    }
}

struct Run<'a> {
    ctx: &'a Context<'a>,
    loaded: &'a Loaded,
    by: &'a str,
    transport: Option<Box<dyn Transport>>,
}

fn issue_result(issue: &Issue, token: &Token, digest: &str) -> Value {
    json!({
        "issue_number": issue.number, "issue_id": issue.id, "issue_node": issue.node_id,
        "url": issue.url, "state": issue.state, "task_id": token.id(),
        "generation": token.generation(), "local": digest, "remote": digest,
    })
}

impl Run<'_> {
    fn remote(&mut self, state: &mut State, error: &ApiError, doing: &str) -> Stop {
        let why = failed(state, error, self.ctx.now, doing);
        match error {
            ApiError::Unknown(_) => end(why, Event::Unknown),
            ApiError::Unauthorized => Stop::End {
                why,
                event: Event::Failed,
                drop: true,
            },
            ApiError::RateLimited(_) | ApiError::NotSent(_) => end(why, Event::Failed),
            ApiError::NotFound | ApiError::Moved | ApiError::Unexpected(_) => Stop::Skip(why),
        }
    }

    /// As [`Run::remote`] for a read made to learn what an earlier dispatch came to: whatever
    /// stops it, that result is still not known, so the operation is never skipped by it.
    fn looking(&mut self, state: &mut State, error: &ApiError, doing: &str) -> Stop {
        match self.remote(state, error, doing) {
            Stop::Skip(why) => end(why, Event::Unknown),
            Stop::End { why, drop, .. } => Stop::End {
                why,
                event: Event::Unknown,
                drop,
            },
        }
    }

    fn transport(&mut self) -> Result<&dyn Transport, Stop> {
        if self.transport.is_none() {
            let made = connect(self.ctx, self.loaded).map_err(|why| end(why, Event::Failed))?;
            self.transport = Some(made);
        }
        self.transport
            .as_deref()
            .ok_or_else(|| end("no transport".into(), Event::Failed))
    }

    fn repository(&self) -> Result<Repository, Stop> {
        let named = self.loaded.config.repository.clone();
        named.ok_or_else(|| end("the settings name no repository".into(), Event::Failed))
    }

    /// An issue becomes a proposal, and the proposal is read back for its token.
    fn import(&self, slug: &str, issue: &Issue, title: &str, body: &str) -> Result<Value, Stop> {
        let board = self.ctx.board;
        match board.propose(slug, title, body, self.by) {
            Ok(_) => {}
            Err(BoardError::Refused(why)) => {
                // Refused: the slug is taken. By this issue's own proposal, made by a run
                // whose answer was lost, or by something else, which is left alone.
                let tasks = board
                    .tasks()
                    .map_err(|e| end(board_said(&e), Event::Failed))?;
                let node = format!("GitHub issue node {}", issue.node_id);
                let ours = tasks
                    .iter()
                    .any(|task| task.slug == slug && task.body.lines().any(|l| l == node));
                if !ours {
                    return Err(Stop::Skip(format!("{slug}: {why}")));
                }
            }
            Err(error) => return Err(end(board_said(&error), Event::Unknown)),
        }
        let bound = board
            .bound(slug)
            .map_err(|e| end(format!("{slug}: {}", e.said()), Event::Failed))?;
        Ok(json!({
            "task_id": bound.token.id(), "generation": bound.token.generation(),
            "local": plan::content(&bound.title, &bound.body),
        }))
    }

    /// What the task holds before a pull rewrites it, read before the pull's intent is
    /// journaled: the board keeps no earlier words of a brief, so the intent does, and an edit
    /// cut short can be put back from it. Where the board gives none, nothing is written.
    fn prior(&self, action: &Action) -> Result<Option<Bound>, Stop> {
        let Action::Pull { slug, .. } = action else {
            return Ok(None);
        };
        match self.ctx.board.bound(slug) {
            Ok(bound) => Ok(Some(bound)),
            Err(BoardError::Refused(why)) => Err(Stop::End {
                why: format!(
                    "{slug}: the board gives no token, so nothing was written: {why}. Preview \
                     again."
                ),
                event: Event::Failed,
                drop: true,
            }),
            Err(error) => Err(end(board_said(&error), Event::Failed)),
        }
    }

    /// A task's title and brief rewritten by the token the preview read, or not at all. `before`
    /// is what it held when the intent was journaled.
    fn pull(
        &self,
        (slug, token): (&str, &Token),
        (title, body): (&str, &str),
        before: &Bound,
        unknown: bool,
    ) -> Result<Value, Stop> {
        let board = self.ctx.board;
        let done = |token: &Token| {
            json!({
                "task_id": token.id(), "generation": token.generation(),
                "local": plan::content(title, body),
            })
        };
        if unknown && before.title == title && before.body == body {
            return Ok(done(&before.token));
        }
        match board.edit(slug, token, title, body, self.by) {
            Ok(written) => Ok(done(&written.token)),
            // The task is not the one, or not at the version, the preview read. The run ends
            // here and the preview with it: nothing is tried on what the board holds now.
            Err(BoardError::Refused(why)) => Err(Stop::End {
                why: format!(
                    "{slug}: the board refused the edit, so nothing was written: {why}. Preview \
                     again."
                ),
                event: Event::Failed,
                drop: true,
            }),
            Err(error) => Err(end(board_said(&error), Event::Unknown)),
        }
    }

    /// A task's curated text becomes an issue, once: an earlier dispatch whose result is not
    /// known is looked for by its marker among every issue before anything is created.
    fn publish(
        &mut self,
        state: &mut State,
        (slug, marker): (&str, &str),
        (title, body): (&str, &str),
        labels: &[String],
        unknown: bool,
    ) -> Result<Value, Stop> {
        // The board's token first: a board that gives none is found before anything is
        // published.
        let (board, by, now) = (self.ctx.board, self.by, self.ctx.now);
        let token = board.bound(slug).map(|bound| bound.token).map_err(|e| {
            end(
                format!("{slug}: the board gives no token: {}", e.said()),
                Event::Failed,
            )
        })?;
        let repository = self.repository()?;
        let digest = plan::content(title, &plan::strip_marker(body));
        if unknown {
            let listing =
                Client::new(self.transport()?, &repository, now).issues(None, SEARCH_PAGES);
            let listing = match listing {
                Ok(listing) if listing.stable && listing.all => listing,
                // A listing that is not all of them is not trusted to say none was made.
                Ok(_) => {
                    return Err(end(
                        format!("{slug}: the issues could not all be listed to look for it"),
                        Event::Unknown,
                    ));
                }
                Err(error) => return Err(self.looking(state, &error, "looking for a publication")),
            };
            let found: Vec<&Issue> = (listing.issues.iter())
                .filter(|issue| plan::find_marker(&issue.body) == Some(marker))
                .filter(|issue| !state.dropped_from(slug, issue.id))
                .collect();
            match found.as_slice() {
                [] => {}
                [one] => {
                    published(board, slug, marker, one, by);
                    // Mapped at the text the issue holds: where the curated text moved on
                    // since that dispatch, the next preview reads its edit and pushes it.
                    let holds = plan::content(&one.title, &plan::strip_marker(&one.body));
                    return Ok(issue_result(one, &token, &holds));
                }
                many => {
                    return Err(Stop::Skip(format!(
                        "{slug}: {} issues carry its marker: none is mapped and none is \
                         published again; close the duplicates, then preview",
                        many.len()
                    )));
                }
            }
        }
        let made = Client::new(self.transport()?, &repository, now).create(title, body, labels);
        match made {
            Ok(issue) => {
                published(board, slug, marker, &issue, by);
                Ok(issue_result(&issue, &token, &digest))
            }
            Err(error) => Err(self.remote(state, &error, "publishing an issue")),
        }
    }

    /// A curated text's edit reaches its issue, which is read again first and must still
    /// hold `base`.
    fn push(
        &mut self,
        state: &mut State,
        (slug, number): (&str, i64),
        base: &str,
        (title, body): (&str, &str),
        unknown: bool,
    ) -> Result<Value, Stop> {
        let repository = self.repository()?;
        let now = self.ctx.now;
        let intended = plan::content(title, &plan::strip_marker(body));
        let flag = |state: &mut State, flag: Flag| {
            if let Some(mapping) = state.mappings.get_mut(slug) {
                mapping.flag = Some(flag);
            }
        };
        let read = Client::new(self.transport()?, &repository, now).issue(number);
        let current = match read {
            Ok(issue) => issue,
            Err(ApiError::NotFound) => {
                flag(state, Flag::Inaccessible);
                return Err(Stop::Skip(format!(
                    "{slug}: issue #{number} is missing or not accessible: nothing was deleted \
                     or published again"
                )));
            }
            Err(error) if unknown => return Err(self.looking(state, &error, "reading an issue")),
            Err(error) => return Err(self.remote(state, &error, "reading an issue")),
        };
        let digest_of =
            |issue: &Issue| plan::content(&issue.title, &plan::strip_marker(&issue.body));
        let result = json!({"local": intended, "remote": intended});
        if digest_of(&current) == intended {
            // Already written: an earlier run's lost answer, or the same edit made there.
            return Ok(result);
        }
        if digest_of(&current) != base {
            flag(state, Flag::Conflict);
            return Err(Stop::Skip(format!(
                "{slug}: issue #{number} changed since the preview: it was not overwritten"
            )));
        }
        let written = Client::new(self.transport()?, &repository, now).update(number, title, body);
        match written {
            Ok(issue) if digest_of(&issue) == intended => Ok(result),
            Ok(_) => {
                flag(state, Flag::Conflict);
                Err(Stop::Skip(format!(
                    "{slug}: issue #{number} reads otherwise than it was written"
                )))
            }
            Err(error) => Err(self.remote(state, &error, "rewriting an issue")),
        }
    }

    /// A note on the thread of the task a mapping names, and of no other: a note carries a
    /// slug, and a board restored or rebound since the preview may hold another task under it.
    fn note(
        &self,
        state: &State,
        slug: &str,
        (key, text): (&str, &str),
        to: Option<&str>,
    ) -> Result<Value, Stop> {
        let board = self.ctx.board;
        let Some(mapping) = state.mappings.get(slug) else {
            return Err(Stop::Skip(format!(
                "{slug}: mapped to no issue now: its note was not written"
            )));
        };
        match board.bound(slug) {
            Ok(now)
                if now.token.id() == mapping.task_id
                    && now.token.generation() == mapping.generation => {}
            Ok(_) => {
                return Err(Stop::Skip(format!(
                    "{slug}: the board holds another task or generation than the one mapped: \
                     its note was not written; preview again"
                )));
            }
            Err(BoardError::Refused(why)) => return Err(Stop::Skip(format!("{slug}: {why}"))),
            Err(error) => return Err(end(board_said(&error), Event::Failed)),
        }
        match board.note(slug, text, to, key, self.by) {
            Ok(()) => Ok(json!({})),
            Err(BoardError::Refused(why)) => Err(Stop::Skip(format!("{slug}: {why}"))),
            Err(error) => Err(end(board_said(&error), Event::Unknown)),
        }
    }

    /// One action done: its result. `unknown` says an earlier run dispatched it and never
    /// learned what came of it; `prior` is what [`Run::prior`] read before its intent.
    fn act(
        &mut self,
        state: &mut State,
        action: &Action,
        unknown: bool,
        prior: Option<&Bound>,
    ) -> Result<Value, Stop> {
        match action {
            Action::Bind { .. } | Action::Adopt { .. } | Action::Local { .. } => Ok(json!({})),
            Action::Note {
                slug,
                key,
                text,
                to,
            } => self.note(state, slug, (key, text), to.as_deref()),
            Action::Import {
                slug,
                issue,
                title,
                body,
            } => self.import(slug, issue, title, body),
            Action::Pull {
                slug,
                token,
                title,
                body,
                ..
            } => {
                let before = prior.ok_or_else(|| {
                    end(
                        format!("{slug}: its brief was not read before its intent"),
                        Event::Failed,
                    )
                })?;
                self.pull((slug, token), (title, body), before, unknown)
            }
            Action::Publish {
                slug,
                marker,
                title,
                body,
                labels,
            } => self.publish(state, (slug, marker), (title, body), labels, unknown),
            Action::Push {
                slug,
                issue_number,
                base,
                title,
                body,
            } => self.push(state, (slug, *issue_number), base, (title, body), unknown),
        }
    }
}

/// The thread's note of a publication, written once for its marker: a courtesy the mapping
/// does not wait on.
fn published(board: &dyn Board, slug: &str, marker: &str, issue: &Issue, by: &str) {
    let said = format!(
        "Published as GitHub issue #{} ({}) from this task's curated public text.",
        issue.number, issue.url
    );
    let _ = board.note(slug, &said, None, &format!("github:published:{marker}"), by);
}

fn board_said(error: &BoardError) -> String {
    board(error)
}

fn text(value: &Value, field: &str) -> String {
    value[field].as_str().unwrap_or_default().to_string()
}

/// What an acknowledged action leaves in the state. Run again on the same state it changes
/// nothing, so a result journaled before a crash is settled by the next run.
pub fn settle(state: &mut State, action: &Action, result: &Value) {
    let number = |field: &str| result[field].as_i64().unwrap_or_default();
    match action {
        Action::Bind { binding } => state.binding = Some(binding.clone()),
        Action::Adopt {
            slug,
            mapping,
            draft,
        } => {
            state
                .mappings
                .entry(slug.clone())
                .or_insert_with(|| mapping.clone());
            if let Some(draft) = draft {
                state
                    .drafts
                    .entry(slug.clone())
                    .or_insert_with(|| draft.clone());
            }
        }
        Action::Import { slug, issue, .. } => {
            state.mappings.insert(
                slug.clone(),
                Mapping {
                    origin: Origin::Import,
                    task_id: number("task_id"),
                    generation: text(result, "generation"),
                    issue_number: issue.number,
                    issue_id: issue.id,
                    issue_node: issue.node_id.clone(),
                    url: issue.url.clone(),
                    local: text(result, "local"),
                    remote: plan::content(&issue.title, &issue.body),
                    remote_state: issue.state.clone(),
                    comments_since: None,
                    comments_seen: Vec::new(),
                    flag: None,
                },
            );
        }
        Action::Pull { slug, remote, .. } => {
            if let Some(mapping) = state.mappings.get_mut(slug) {
                mapping.local = text(result, "local");
                mapping.remote.clone_from(remote);
                mapping.generation = text(result, "generation");
            }
            state.resolutions.remove(slug);
        }
        Action::Publish { slug, .. } => {
            state.mappings.insert(
                slug.clone(),
                Mapping {
                    origin: Origin::Export,
                    task_id: number("task_id"),
                    generation: text(result, "generation"),
                    issue_number: number("issue_number"),
                    issue_id: number("issue_id"),
                    issue_node: text(result, "issue_node"),
                    url: text(result, "url"),
                    local: text(result, "local"),
                    remote: text(result, "remote"),
                    remote_state: text(result, "state"),
                    comments_since: None,
                    comments_seen: Vec::new(),
                    flag: None,
                },
            );
        }
        Action::Push { slug, .. } => {
            if let Some(mapping) = state.mappings.get_mut(slug) {
                mapping.local = text(result, "local");
                mapping.remote = text(result, "remote");
            }
            state.resolutions.remove(slug);
        }
        Action::Note { .. } => {}
        Action::Local { slug, change } => {
            let Some(mapping) = state.mappings.get_mut(slug) else {
                return;
            };
            match change {
                Change::Draft { draft, digest } => {
                    mapping.local.clone_from(digest);
                    mapping.remote.clone_from(digest);
                    state.drafts.insert(slug.clone(), draft.clone());
                    state.resolutions.remove(slug);
                }
                Change::Synced { local, remote } => {
                    mapping.local.clone_from(local);
                    mapping.remote.clone_from(remote);
                    state.resolutions.remove(slug);
                }
                Change::Flag(flag) => mapping.flag.clone_from(flag),
                Change::RemoteState(remote) => mapping.remote_state.clone_from(remote),
                Change::CommentsSince { since, seen } => {
                    mapping.comments_since = Some(since.clone());
                    mapping.comments_seen.clone_from(seen);
                }
            }
        }
    }
}

/// What an operation expects to find, journaled with its intent: for a brief's edit the token
/// with the title and brief that the task holds before it, which the board does not keep.
fn base(action: &Action, prior: Option<&Bound>) -> Value {
    match action {
        Action::Pull { token, .. } => json!({
            "token": token.0,
            "prior_title": prior.map(|prior| prior.title.as_str()),
            "prior_body": prior.map(|prior| prior.body.as_str()),
        }),
        Action::Push { base, .. } => json!(base),
        _ => Value::Null,
    }
}

fn written(action: &Action) -> String {
    match action {
        Action::Import { title, body, .. }
        | Action::Pull { title, body, .. }
        | Action::Publish { title, body, .. }
        | Action::Push { title, body, .. } => plan::content(title, body),
        Action::Note { text, .. } => crate::digest(&[text]),
        _ => String::new(),
    }
}

/// The markers of the publications that may have made an issue no mapping holds, under
/// whichever preview they were dispatched. Only an acknowledgment, which maps the issue,
/// settles one. An answer that never arrived and an intent with no word after it (a run
/// killed) may each have made the issue. A skip leaves issues that carry the marker unmapped,
/// or made none. A failure says only that its own attempt sent nothing: it closes that
/// attempt's intent and clears nothing said before it. So whatever follows, the next run looks
/// among every issue before it creates anything.
fn unsettled_publications(entries: &[Entry]) -> BTreeSet<&str> {
    let mut pending = BTreeSet::new();
    let mut open = BTreeSet::new();
    for entry in entries.iter().filter(|entry| entry.kind == "publish") {
        let op = entry.op.as_str();
        match entry.event {
            // An earlier intent that got no word is not known unmade: it stays pending
            // whatever this newer attempt comes to.
            Event::Intent => {
                if !open.insert(op) {
                    pending.insert(op);
                }
            }
            Event::Unknown | Event::Skipped => {
                open.remove(op);
                pending.insert(op);
            }
            Event::Failed => {
                open.remove(op);
            }
            Event::Ack => {
                open.remove(op);
                pending.remove(op);
            }
            Event::Applied => {}
        }
    }
    pending.extend(open);
    pending
}

/// `symphony-github reconcile --preview DIGEST --as BY`: apply the preview of that digest, at
/// most `limit` operations of it.
///
/// # Errors
///
/// Why nothing, or nothing more, was applied.
#[expect(
    clippy::too_many_lines,
    reason = "one run in the order it happens: its refusals, then each operation journaled, done and settled"
)]
pub fn apply(
    ctx: &Context<'_>,
    digest: &str,
    by: &str,
    limit: usize,
    out: &mut dyn Write,
) -> Outcome {
    let folder = ctx.board.state_folder().map_err(|e| board(&e))?;
    let loaded = match enabled(ctx) {
        Ok(loaded) => loaded,
        Err(why) => {
            // What waited is not kept to be replayed; where nothing was kept, nothing is made.
            if let Some(store) = Store::existing(&folder) {
                let _lock = store.lock()?;
                store.drop_preview()?;
            }
            return Err(format!("{why}: nothing was dispatched"));
        }
    };
    let store = Store::open(&folder)?;
    let _lock = store.lock()?;
    let entries = store.entries()?;
    let preview = match store.preview()? {
        Some(preview) if preview.digest == digest => preview,
        _ => {
            let applied = entries
                .iter()
                .any(|entry| entry.event == Event::Applied && entry.preview == digest);
            if applied {
                let _ = writeln!(out, "Preview {digest} is already applied: nothing to do.");
                return Ok(0);
            }
            return Err(format!(
                "no preview {digest} waits to be applied: run `symphony-github preview` and \
                 apply the digest it prints"
            ));
        }
    };
    if preview.computed() != preview.digest {
        store.drop_preview()?;
        return Err("the kept preview is not the one its digest names: preview again".into());
    }
    if preview.config != loaded.revision {
        store.drop_preview()?;
        return Err("the committed settings changed since the preview: preview again".into());
    }
    let mut state = store.state()?.unwrap_or_default();
    if let Some(why) = &state.paused {
        return Err(format!("paused: {why}"));
    }
    limited(&state, ctx.now)?;

    // What earlier runs of this preview left of each operation: its last word.
    let mut last: BTreeMap<&str, &Entry> = BTreeMap::new();
    for entry in entries.iter().filter(|entry| entry.preview == digest) {
        last.insert(&entry.op, entry);
    }
    let pending = unsettled_publications(&entries);
    let mut run = Run {
        ctx,
        loaded: &loaded,
        by,
        transport: None,
    };
    let (mut done, mut skipped, mut remaining) = (0, 0, 0);
    for (index, action) in preview.actions.iter().enumerate() {
        let op = preview.op(index);
        let before = last
            .get(op.as_str())
            .map(|entry| (entry.event, &entry.result));
        let unknown = match before {
            Some((Event::Ack, result)) => {
                settle(&mut state, action, result);
                continue;
            }
            Some((Event::Skipped, _)) => continue,
            Some((Event::Intent | Event::Unknown, _)) => true,
            _ => false,
        } || pending.contains(op.as_str());
        if done >= limit {
            remaining += 1;
            continue;
        }
        if action.dispatches() {
            // Turned off, or changed, since the run began: this dispatch and the rest are not
            // made.
            let now = enabled(ctx);
            if now.as_ref().map(|now| &now.revision) != Ok(&loaded.revision) {
                store.save(&state)?;
                store.drop_preview()?;
                return Err(format!(
                    "{}: nothing more was dispatched",
                    now.err()
                        .unwrap_or_else(|| "the committed settings changed during the run".into())
                ));
            }
        }
        let prior = run.prior(action);
        let expected = base(action, prior.as_ref().ok().and_then(Option::as_ref));
        let entry = |event: Event, result: Value| Entry {
            at: ctx.now,
            event,
            preview: digest.into(),
            op: op.clone(),
            kind: action.kind().into(),
            target: action.target().into(),
            digest: written(action),
            base: expected.clone(),
            config: loaded.revision.clone(),
            by: by.into(),
            result,
        };
        store.journal(&entry(Event::Intent, Value::Null))?;
        let acted = prior.and_then(|prior| run.act(&mut state, action, unknown, prior.as_ref()));
        match acted {
            Ok(result) => {
                store.journal(&entry(Event::Ack, result.clone()))?;
                settle(&mut state, action, &result);
                store.save(&state)?;
                done += 1;
            }
            Err(Stop::Skip(why)) => {
                store.journal(&entry(Event::Skipped, json!(why)))?;
                store.save(&state)?;
                let _ = writeln!(out, "skipped: {why}");
                skipped += 1;
            }
            Err(Stop::End { why, event, drop }) => {
                store.journal(&entry(event, json!(why)))?;
                store.save(&state)?;
                if drop {
                    store.drop_preview()?;
                }
                return Err(format!("{why}\n{done} applied before the run ended."));
            }
        }
    }
    if remaining > 0 {
        store.save(&state)?;
        let _ = writeln!(
            out,
            "{done} applied, {skipped} skipped, {remaining} wait: run the same command again \
             for the next {limit}."
        );
        return Ok(0);
    }
    store.journal(&Entry {
        at: ctx.now,
        event: Event::Applied,
        preview: digest.into(),
        op: String::new(),
        kind: "applied".into(),
        target: String::new(),
        digest: String::new(),
        base: Value::Null,
        config: loaded.revision.clone(),
        by: by.into(),
        result: Value::Null,
    })?;
    if let Some(cursor) = &preview.next_cursor {
        state.cursor = Some(cursor.clone());
        state.cursor_revision = Some(preview.config.clone());
    }
    store.save(&state)?;
    store.drop_preview()?;
    let _ = writeln!(
        out,
        "{done} applied, {skipped} skipped. Preview {digest} is applied."
    );
    Ok(0)
}
