//! The owner's save operations, independent of the HTTP transport. One lock covers all register
//! reads, edits and writes, including checkout synchronization by the verdict committer.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};

use serde_json::{Value, json};

use crate::{Who, asks, branches, clock, commit, edit, register, taskview};

mod failure;
mod quick;
mod quick_task;
pub use failure::Failure;

#[cfg(test)]
mod close_tests;

pub const ROUTES: [&str; 13] = [
    "/api/postpone",
    "/api/decision",
    "/api/comment",
    "/api/message",
    "/api/task-message",
    "/api/task-priority",
    "/api/task-field",
    "/api/task-cancel",
    "/api/quick-chat",
    "/api/quick-task",
    "/api/ask",
    "/api/ask-reply",
    symphony_api::TASK_ACTION_ROUTE,
];

/// The routes that write to one task, named by the payload's `slug`.
const TASK_ROUTES: [&str; 5] = [
    "/api/task-message",
    "/api/task-priority",
    "/api/task-field",
    "/api/task-cancel",
    symphony_api::TASK_ACTION_ROUTE,
];

fn string<'a>(payload: &'a Value, key: &str) -> &'a str {
    payload[key].as_str().unwrap_or("")
}

pub struct Saves {
    root: PathBuf,
    writes: Arc<Mutex<()>>,
    taps: Mutex<VecDeque<String>>,
    unpublished: Mutex<BTreeMap<String, (String, Value)>>,
    committer: OnceLock<commit::Committer>,
    closed: AtomicBool,
    filer: Mutex<Option<mpsc::SyncSender<()>>>,
    filing_worker: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Saves {
    pub fn close(&self) {
        self.close_saying(|_| {});
    }

    /// Close, naming each wait through `stage` as it begins: the save in flight, the queued
    /// verdict publications, then the verdict filing. A close that never ends names its wait.
    pub fn close_saying(&self, stage: impl Fn(&str)) {
        stage("the save in flight");
        let committer = {
            let _held = self.writes.lock().unwrap_or_else(|e| e.into_inner());
            self.closed.store(true, Ordering::Release);
            self.committer.get()
        };
        if let Some(committer) = committer {
            stage("the queued verdict publications");
            committer.close();
        }
        self.filer.lock().unwrap_or_else(|e| e.into_inner()).take();
        // The worker's lock is held through its join, so a concurrent close waits here for the
        // same worker to end instead of returning while it still files.
        stage("the verdict filing");
        let mut filing = self.filing_worker.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(worker) = filing.take()
            && worker.join().is_err()
        {
            let _ = std::io::Write::write_all(
                &mut std::io::stderr(),
                b"Symphony: verdict filing worker panicked while closing\n",
            );
        }
        drop(filing);
    }
    pub fn start(root: &Path) -> Self {
        let writes = Arc::new(Mutex::new(()));
        let (filer, receive) = mpsc::sync_channel(1);
        let home = root.to_path_buf();
        let filing_worker = std::thread::spawn(move || {
            while receive.recv().is_ok() {
                while receive.try_recv().is_ok() {}
                let mut out = Vec::new();
                let mut err = Vec::new();
                let code = crate::cli::verdict_tasks_main(
                    &[
                        "--root".into(),
                        home.to_string_lossy().into_owned(),
                        "--file".into(),
                        "--app".into(),
                    ],
                    &mut out,
                    &mut err,
                );
                if code != 0 {
                    let _ = std::io::Write::write_all(&mut std::io::stderr(), &err);
                }
            }
        });
        Self {
            root: root.to_path_buf(),
            writes,
            taps: Mutex::new(VecDeque::new()),
            unpublished: Mutex::new(BTreeMap::new()),
            committer: OnceLock::new(),
            closed: AtomicBool::new(false),
            filer: Mutex::new(Some(filer)),
            filing_worker: Mutex::new(Some(filing_worker)),
        }
    }

    fn store(&self) -> Result<symphony_board::Store, Failure> {
        taskview::connect(&self.root).map_err(|e| Failure {
            status: 503,
            error: format!("the task board cannot be reached: {e}"),
        })
    }

    fn scope(&self, scope: &str, rel: &str) -> Result<(PathBuf, String), Failure> {
        let (scope, rel) = if let Some(path) = rel.trim_start_matches('/').strip_prefix('@') {
            path.split_once('/')
                .ok_or_else(|| crate::project::Project::of(&self.root).not_a_document())?
        } else {
            (scope, rel.trim_start_matches('/'))
        };
        let root = if scope.is_empty() {
            self.root.clone()
        } else {
            branches::trees(&self.root)
                .into_iter()
                .find(|t| t.id() == scope)
                .map(|t| t.path)
                .ok_or_else(|| Failure {
                    status: 404,
                    error: "no such worktree".into(),
                })?
        };
        Ok((root, rel.to_string()))
    }

    /// The file the document name `rel` (`docs/…`) stands for in its entrypoint's folder, as the
    /// document view reads it ([`crate::docview::target`]).
    fn target(root: &Path, rel: &str) -> Result<PathBuf, Failure> {
        crate::docview::target(root, rel, true)
            .map_err(|_| Failure::from(crate::project::Project::of(root).not_a_document()))
    }

    fn page(&self, root: &Path, rel: &str, anchor: &str) -> Result<String, Failure> {
        let prefix = if root == self.root {
            String::new()
        } else {
            let tree = branches::trees(&self.root)
                .into_iter()
                .find(|t| t.path == root)
                .ok_or("no such worktree".to_string())?;
            format!("@{}/", tree.id())
        };
        Ok(format!(
            "/{prefix}{rel}{}",
            if anchor.is_empty() {
                String::new()
            } else {
                format!("#{anchor}")
            }
        ))
    }

    pub fn post(&self, scope: &str, route: &str, payload: &Value) -> Result<Value, Failure> {
        self.post_inner(scope, route, payload, None)
    }

    /// Apply an app request against the same decision metadata its reader was shown.
    pub fn post_with_order(
        &self,
        scope: &str,
        route: &str,
        payload: &Value,
        order: &crate::postpone::View,
    ) -> Result<Value, Failure> {
        self.post_inner(scope, route, payload, Some(order))
    }

    fn post_inner(
        &self,
        scope: &str,
        route: &str,
        payload: &Value,
        order: Option<&crate::postpone::View>,
    ) -> Result<Value, Failure> {
        if !payload.is_object() {
            return Err("bad JSON: body must be a JSON object".to_string().into());
        }
        // A task write names its task before it reaches the board, which reads an empty slug as no
        // valid record id at all.
        if TASK_ROUTES.contains(&route) && string(payload, "slug").trim().is_empty() {
            return Err("slug is required".to_string().into());
        }
        let request = json!([scope, route, payload]).to_string();
        let mut unpublished = self.unpublished.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((text, value)) = unpublished.get(&request).cloned() {
            crate::callbacks::publish(&self.root, &text).map_err(Failure::from)?;
            unpublished.remove(&request);
            return Ok(value);
        }
        drop(unpublished);
        let result = match route {
            "/api/postpone" => {
                let (root, rel) = self.scope(scope, string(payload, "path"))?;
                Self::target(&root, &rel)?;
                let action = string(payload, "action");
                let path = self.page(&root, &rel, "")?;
                let fallback;
                let order = match order {
                    Some(order) => Some(order),
                    None if action == "later" => {
                        fallback = crate::dashboard::snapshot(
                            &self.root,
                            &branches::branch_cards(&self.root),
                            "",
                        )
                        .map_err(|e| Failure::from(e.to_string()))?;
                        Some(fallback.order.as_ref())
                    }
                    None => None,
                };
                let path = order
                    .and_then(|order| {
                        order.path_for(path.trim_start_matches('/'), string(payload, "slug"))
                    })
                    .unwrap_or(path.trim_start_matches('/'));
                let changed = if let Some(order) = order {
                    order.act(
                        &self.root,
                        crate::postpone::OWNER,
                        path.trim_start_matches('/'),
                        string(payload, "slug"),
                        action,
                        crate::postpone::now(),
                    )
                } else {
                    crate::postpone::act(
                        &self.root,
                        crate::postpone::OWNER,
                        path.trim_start_matches('/'),
                        string(payload, "slug"),
                        action,
                        crate::postpone::now(),
                    )
                }
                .map_err(|e| Failure::from(e.to_string()))?;
                let next = if action == "later" {
                    order
                        .expect("Later has a decision snapshot")
                        .waiting(&self.root, crate::postpone::OWNER, crate::postpone::now())
                        .map_err(|e| Failure::from(e.to_string()))?
                        .iter()
                        .find(|card| {
                            card.doc != path.trim_start_matches('/')
                                || card.slug != string(payload, "slug")
                        })
                        .map(|card| card.href())
                } else {
                    None
                };
                Ok(json!({"ok":true, "changed":changed, "next":next}))
            }
            "/api/decision" | "/api/comment" | "/api/message" => {
                self.document(scope, route, payload)
            }
            "/api/task-message" => {
                let to = string(payload, "to");
                self.store()?.say(
                    string(payload, "slug"),
                    "owner",
                    string(payload, "text"),
                    (!to.is_empty()).then_some(to),
                )?;
                Ok(json!({"ok":true}))
            }
            "/api/task-priority" => {
                let priority = payload["priority"]
                    .as_i64()
                    .or_else(|| payload["priority"].as_str().and_then(|s| s.parse().ok()))
                    .ok_or("priority must be an integer from 0 to 4".to_string())?;
                self.store()?
                    .set_priority(string(payload, "slug"), priority, "owner")?;
                Ok(json!({"ok":true}))
            }
            "/api/task-field" => {
                let actor = Some(string(payload, "actor"))
                    .filter(|actor| !actor.is_empty())
                    .unwrap_or("owner");
                let (plugin, value) = taskview::set_field(
                    &self.store()?,
                    &crate::plugins::Plugins::of(&self.root),
                    string(payload, "slug"),
                    string(payload, "name"),
                    &payload["value"],
                    actor,
                )?;
                Ok(json!({"ok": true, "plugin": plugin, "value": value}))
            }
            "/api/task-cancel" => {
                self.store()?
                    .move_to(string(payload, "slug"), "cancelled", "owner")?;
                Ok(json!({"ok":true}))
            }
            symphony_api::TASK_ACTION_ROUTE => {
                let slug = string(payload, "slug");
                let store = self.store()?;
                let task = store.get(slug)?;
                let branch = crate::taskview::approval_branch(&store, &task)?;
                let plugins = crate::plugins::Plugins::of(&self.root);
                let actions = plugins.board_actions();
                // The board action that made the offer, by the name the offer gave.
                let named = string(payload, "action");
                let Some(action) = actions.iter().find(|action| action.name() == named) else {
                    return Err(Failure {
                        status: 404,
                        error: "no such board action".into(),
                    });
                };
                let done = action.submit(
                    &self.root,
                    &symphony_plugin::TaskRef {
                        slug,
                        status: &task.status,
                        branch: &branch,
                    },
                    &symphony_plugin::Submission {
                        payload: string(payload, "payload").into(),
                        note: string(payload, "note").into(),
                    },
                )?;
                store.say(slug, "owner", &done.said, None)?;
                // The work it releases goes back to review. A move that fails here is made by the
                // next landing or stamp edit, which re-reads them.
                for held in &done.released {
                    let _ = store.move_to(held, "review", "owner");
                }
                Ok(json!({"ok":true, "said": done.said}))
            }
            "/api/ask" | "/api/ask-reply" => self.ask(scope, route, payload),
            "/api/quick-chat" | "/api/quick-task" => self.quick(scope, route, payload),
            _ => Err(Failure {
                status: 404,
                error: "no such route".into(),
            }),
        };
        if let Ok(value) = &result {
            let notify = matches!(
                route,
                "/api/ask" | "/api/ask-reply" | "/api/quick-chat" | "/api/message" | "/api/comment"
            ) || ((route == "/api/decision"
                || route == symphony_api::TASK_ACTION_ROUTE)
                && !string(payload, "note").trim().is_empty());
            if notify && value["repeat"] != true {
                let id = value["id"].as_i64();
                let db = asks::default_path(&self.root, std::env::var("ASKS_DB").ok().as_deref());
                let db = format!("'{}'", db.to_string_lossy().replace('\'', "'\\''"));
                let reply = id.map_or_else(|| format!("Reply in the document/card at {}#{} using the chat-doc skill.", string(payload, "path"), string(payload, "slug")), |id| format!("Read: just symphony-ask --db {db} show {id}. Answer: just symphony-ask --db {db} answer {id} TEXT --by HANDLE."));
                let text = format!(
                    "OWNER {route} scope={scope} {}\n{reply}{}",
                    payload,
                    attached(&self.root, payload)
                );
                // A saved document can already count as a repeated tap when journaling fails.
                // Retain both the callback and response so its HTTP retry publishes that same
                // save rather than silently skipping it (or creating another ask/comment).
                let mut unpublished = self.unpublished.lock().unwrap_or_else(|e| e.into_inner());
                unpublished.insert(request.clone(), (text.clone(), value.clone()));
                crate::callbacks::publish(&self.root, &text).map_err(Failure::from)?;
                unpublished.remove(&request);
            }
        }
        result
    }

    fn document(&self, scope: &str, route: &str, payload: &Value) -> Result<Value, Failure> {
        let (root, rel) = self.scope(scope, string(payload, "path"))?;
        let target = Self::target(&root, &rel)?;
        // A branch's file, by its path in the worktree: git is asked of it.
        let file = (root != self.root).then(|| crate::project::Project::of(&root).file(&rel));
        let tap: String = if route == "/api/decision" {
            string(payload, "tap").chars().take(64).collect()
        } else {
            String::new()
        };
        let _held = self.writes.lock().unwrap_or_else(|e| e.into_inner());
        let _file = commit::write_lock(&root)?;
        if self.closed.load(Ordering::Acquire) {
            return Err(Failure {
                status: 503,
                error: "document saves are closing".into(),
            });
        }
        let mut taps = self.taps.lock().unwrap_or_else(|e| e.into_inner());
        if !tap.is_empty() && taps.contains(&tap) {
            return Ok(json!({"ok":true,"repeat":true}));
        }
        let original = std::fs::read_to_string(&target).map_err(|e| e.to_string())?;
        if route == "/api/decision"
            && let Some(file) = &file
        {
            closes_only_posted(
                &root,
                file,
                &original,
                string(payload, "slug"),
                string(payload, "state"),
            )?;
        }
        // A merge stopped on the branch's file: the page showed its last committed copy, and an
        // answer on a card goes on the branch's side of the conflicts.
        let conflict = file
            .as_deref()
            .and_then(|file| crate::merging::conflict(&root, file, &original));
        if conflict.is_some() && route == "/api/comment" {
            return Err(Failure {
                status: 409,
                error: format!(
                    "{rel} is being merged in its worktree: comment once the merge is done (an answer on a card is saved now)"
                ),
            });
        }
        let original = conflict
            .as_ref()
            .map_or(original, crate::merging::Conflict::ours);
        let slug = string(payload, "slug");
        let now = clock::now_stamp();
        let text = match route {
            "/api/message" => edit::add_message(
                &original,
                slug,
                string(payload, "text"),
                &now,
                Who::Owner,
                None,
            )?,
            "/api/comment" => {
                let (title, body) = if payload.get("text").is_some() {
                    edit::split_message(string(payload, "text"))?
                } else {
                    (
                        string(payload, "title").to_string(),
                        string(payload, "body").to_string(),
                    )
                };
                let line = payload["after_line"]
                    .as_i64()
                    .or_else(|| payload["after_line"].as_str().and_then(|s| s.parse().ok()))
                    .unwrap_or(0);
                edit::insert_comment(&original, line, string(payload, "kind"), &title, &body)?
            }
            _ => {
                if edit::is_repeat_decision(
                    &original,
                    slug,
                    string(payload, "state"),
                    string(payload, "note"),
                    &now,
                )? {
                    return Ok(json!({"ok":true,"repeat":true}));
                }
                edit::set_decision(
                    &original,
                    slug,
                    string(payload, "state"),
                    string(payload, "note"),
                    &now,
                    &crate::plugins::Plugins::of(&self.root),
                )?
            }
        };
        let (text, _) = register::supersede(&text, &now);
        edit::check_nothing_lost(&original, &text, slug)?;
        let text = match &conflict {
            Some(conflict) => conflict.with_card(&text, slug).ok_or_else(|| Failure {
                status: 409,
                error: format!(
                    "{rel} is being merged in its worktree and the card {slug} sits across a conflict there: answer once the merge is done"
                ),
            })?,
            None => text,
        };
        std::fs::write(&target, &text).map_err(|e| e.to_string())?;
        if route != "/api/comment" {
            let cleared = self
                .page(&root, &rel, "")
                .map_err(|e| e.error)
                .and_then(|path| {
                    crate::postpone::answered(
                        &self.root,
                        crate::postpone::OWNER,
                        &crate::postpone::key(path.trim_start_matches('/'), slug),
                    )
                    .map_err(|e| e.to_string())
                });
            if let Err(error) = cleared {
                // View preferences must not interrupt committing or delivering an owner's answer.
                eprintln!("· could not clear postponed card {slug}: {error}");
            }
        }
        if root == self.root && commit::on_main(&root) {
            self.committer
                .get_or_init(|| commit::Committer::start(&self.root, self.writes.clone()))
                .submit(commit::Edit {
                    rel: crate::project::Project::of(&root).file(&rel),
                    base: original,
                    new: text,
                })?;
        }
        if !tap.is_empty() {
            taps.push_back(tap);
            while taps.len() > 512 {
                taps.pop_front();
            }
        }
        drop(taps);
        // Enqueue before releasing the save lock so close cannot disconnect the filer
        // between accepting this document and recording its pending board work.
        if route != "/api/comment"
            && let Some(filer) = self
                .filer
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
        {
            let _ = filer.try_send(());
        }
        drop(_held);
        Ok(json!({"ok":true}))
    }

    fn ask(&self, scope: &str, route: &str, payload: &Value) -> Result<Value, Failure> {
        let store = asks::AskStore::open(&asks::default_path(
            &self.root,
            std::env::var("ASKS_DB").ok().as_deref(),
        ))?;
        if route == "/api/ask" {
            let (root, rel) = self.scope(scope, string(payload, "path"))?;
            Self::target(&root, &rel)?;
            // Asks use docs-relative paths throughout the existing readers. A branch question
            // keeps that contract and carries the actual proposal URL in the owner's message.
            let mut question = string(payload, "question").trim().to_string();
            if root != self.root && !question.is_empty() {
                let page = self.page(&root, &rel, string(payload, "anchor"))?;
                question.push_str(&format!("\n\n(sent from {page})"));
            }
            let id = store.ask(
                &rel,
                &question,
                string(payload, "anchor"),
                string(payload, "quote"),
            )?;
            Ok(json!({"id":id}))
        } else {
            let id = payload["id"]
                .as_i64()
                .or_else(|| payload["id"].as_str().and_then(|s| s.parse().ok()))
                .ok_or("id must be a number".to_string())?;
            store.reply(id, string(payload, "text"))?;
            Ok(json!({"ok":true,"id":id}))
        }
    }
}

/// The verdicts that close a card: once one is given, nothing more is asked of the owner.
const CLOSING: [&str; 4] = ["approved", "dismissed", "resolved", "auto-approved"];

/// Refuses a verdict that closes the card `slug` (`state`, [`CLOSING`]) unless the copy of it in
/// `text`, the file `file` of the worktree at `root`, is the one its branch posted. A card the
/// branch's HEAD commit does not hold is a draft; one whose worktree copy says anything HEAD's
/// does not besides the owner's lines (another text, an agent's line) is being changed by its
/// agent ([`branches::changed_since`]). A verdict never covers what the owner has not seen; a
/// question, a rejection or a message is saved and counts.
fn closes_only_posted(
    root: &Path,
    file: &str,
    text: &str,
    slug: &str,
    state: &str,
) -> Result<(), Failure> {
    if !CLOSING.contains(&state) {
        return Ok(());
    }
    let refused = |error: String| Err(Failure { status: 409, error });
    let head = crate::merging::head_text(root, file).unwrap_or_default();
    let head = register::parse_decisions(&crate::text::splitlines(&head));
    let Some(posted) = head.iter().find(|card| card.slug == slug) else {
        return refused(format!(
            "{slug} is a draft, not posted yet: its branch has not committed it and its agent may still change it, so it cannot be {state} now. Ask about it or reject it; close it once it is posted"
        ));
    };
    let here = branches::file_cards(root, file, text);
    match here.iter().find(|card| card.slug == slug) {
        Some(here) if branches::changed_since(here, posted) => refused(format!(
            "{slug} has changes its branch has not posted: its agent is changing it in its worktree, so it cannot be {state} now. Ask about it or reject it; close it once its agent commits"
        )),
        _ => Ok(()),
    }
}

/// The files the owner attached to a post, as paths the receiving agent opens: an upload line
/// names `/artifacts/uploads/…` of this server's checkout, which an agent in another worktree
/// would otherwise look for in its own. Empty when nothing is attached.
fn attached(root: &Path, payload: &Value) -> String {
    let files: Vec<String> = ["text", "question", "note", "body"]
        .iter()
        .flat_map(|key| crate::uploads::named_files(root, string(payload, key)))
        .map(|path| path.display().to_string())
        .collect();
    if files.is_empty() {
        String::new()
    } else {
        format!("\nAttached files: {}", files.join(", "))
    }
}
