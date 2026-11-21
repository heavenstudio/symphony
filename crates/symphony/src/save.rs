//! The owner's save operations, independent of the HTTP transport. One lock covers all register
//! reads, edits and writes, including checkout synchronization by the verdict committer.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{Value, json};
use symphony_board::NewAsk;

use crate::{Who, asks, branches, clock, commit, edit, register, taskview};

mod attest;
mod failure;
mod quick;
mod quick_task;
pub use failure::Failure;

#[cfg(test)]
mod close_tests;

pub const ROUTES: [&str; 14] = [
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
    WORKFLOW_APPLY,
];

/// The owner's operation on a task a custom workflow holds, through the board's one admission
/// (`symphony_board::Store::apply_attested`): `{"slug": SLUG, "request": OperationRequest}`, with
/// `"attestations": [Signed]` for a guarded edge, or with none, each guard's asked of the project
/// authority a selected plugin answers for its adapter (`attest.rs`); the principal the local
/// owner, never one the payload names ([`symphony_api::workflow::WORKFLOW_APPLY_ROUTE`], which
/// the pages post to).
pub const WORKFLOW_APPLY: &str = symphony_api::workflow::WORKFLOW_APPLY_ROUTE;

/// The routes that write to one task, named by the payload's `slug`.
const TASK_ROUTES: [&str; 6] = [
    "/api/task-message",
    "/api/task-priority",
    "/api/task-field",
    "/api/task-cancel",
    symphony_api::TASK_ACTION_ROUTE,
    WORKFLOW_APPLY,
];

fn string<'a>(payload: &'a Value, key: &str) -> &'a str {
    payload[key].as_str().unwrap_or("")
}

/// Now, in seconds since the epoch: when the owner said what an ask records.
fn epoch_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |since| since.as_secs_f64())
}

/// A save's words for the chat and the ask it opens or joins, kept until the journal holds them.
type Unpublished = (String, Value, Option<NewAsk>);

pub struct Saves {
    root: PathBuf,
    writes: Arc<Mutex<()>>,
    /// The decision taps written lately, each with the verdict and note it wrote: a repeat of one
    /// answers with them, which may not be the words the repeat says.
    taps: Mutex<VecDeque<(String, String, String)>>,
    unpublished: Mutex<BTreeMap<String, Unpublished>>,
    committer: OnceLock<commit::Committer>,
    closed: AtomicBool,
}

impl Saves {
    pub fn close(&self) {
        self.close_saying(|_| {});
    }

    /// Close, naming each wait through `stage` as it begins: the save in flight, then the queued
    /// verdict publications. A close that never ends names its wait.
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
    }

    /// Take up again the saves kept for delivery by pull request (`commit::pending`), with no
    /// new save: asked by the server as it starts and each time it reads main's pages again,
    /// which a land's fast-forward of the checkout makes it do. It starts the committer for
    /// that alone where such saves are kept, and does nothing where none is.
    pub fn settle(&self) {
        if self.closed.load(Ordering::Acquire) || !commit::pending::kept(&self.root) {
            return;
        }
        self.committer
            .get_or_init(|| commit::Committer::start(&self.root, self.writes.clone()))
            .wake();
    }

    /// The saves of the checkout `root`. A verdict waiting on an agent is an ask on the board
    /// ([`symphony_board::Ask`]), opened by the save's journal record ([`crate::callbacks`]): no
    /// save files a task for it.
    pub fn start(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            writes: Arc::new(Mutex::new(())),
            taps: Mutex::new(VecDeque::new()),
            unpublished: Mutex::new(BTreeMap::new()),
            committer: OnceLock::new(),
            closed: AtomicBool::new(false),
        }
    }

    /// The branch of the worktree `root` is, if it is one of this checkout's other worktrees.
    fn tree_of(&self, root: &Path) -> Option<String> {
        (root != self.root)
            .then(|| {
                branches::trees(&self.root)
                    .into_iter()
                    .find(|t| t.path == root)
                    .map(|t| t.branch)
            })
            .flatten()
            .filter(|branch| !branch.is_empty())
    }

    /// The chat ask a question or a follow-up opens or joins, in the ask store's thread `value`
    /// names, about the page it was asked on when it names one.
    fn chat_ask(&self, scope: &str, route: &str, payload: &Value, value: &Value) -> Option<NewAsk> {
        let thread = value["id"].as_i64()?;
        let (root, rel) = match route {
            "/api/ask" => self.scope(scope, string(payload, "path")).ok()?,
            _ => (self.root.clone(), String::new()),
        };
        let words = match route {
            "/api/ask" => string(payload, "question"),
            _ => string(payload, "text"),
        };
        Some(NewAsk {
            kind: "chat".into(),
            path: Some(rel).filter(|rel| !rel.is_empty()),
            tree: self.tree_of(&root),
            thread: Some(thread.to_string()),
            words: words.into(),
            at: Some(epoch_now()),
            ..NewAsk::default()
        })
    }

    fn store(&self) -> Result<symphony_board::Store, Failure> {
        taskview::connect(&self.root).map_err(|e| Failure {
            status: 503,
            error: format!("the task board cannot be reached: {e}"),
        })
    }

    /// The scope and document name a save names: the page's `scope` and `rel`, or those a document
    /// named `@<tree>/…` carries.
    fn named<'a>(&self, scope: &'a str, rel: &'a str) -> Result<(&'a str, &'a str), Failure> {
        Ok(
            if let Some(path) = rel.trim_start_matches('/').strip_prefix('@') {
                path.split_once('/')
                    .ok_or_else(|| crate::project::Project::of(&self.root).not_a_document())?
            } else {
                (scope, rel.trim_start_matches('/'))
            },
        )
    }

    /// A card's save from a branch's page, as main's own when the card is main's
    /// ([`branches::mains_card`]: main's copy of the document holds it, and the branch's page
    /// draws main's card): its payload with its path the document's name, saved under main's
    /// scope. None when the save stays the page's. So the owner's line on a card main holds is
    /// main's, committed there, never left in a worktree a land does not carry. A branch whose
    /// worktree is gone is read at its branch, and with neither the card is main's. A worktree
    /// listing that fails ([`branches::listed`]) says nothing of the page's worktree, so the save
    /// stays the page's, and its own failure is the owner's to see: never a worktree read as gone
    /// and its card answered on main's copy.
    fn mains(&self, scope: &str, payload: &Value) -> Option<Value> {
        let (scope, rel) = self.named(scope, string(payload, "path")).ok()?;
        if scope.is_empty() {
            return None;
        }
        let tree = branches::listed(&self.root)?
            .into_iter()
            .find(|t| t.id() == scope);
        let branch = scope.replace('~', "/");
        let page = match &tree {
            Some(tree) => branches::Page::Tree(&tree.path),
            None => branches::Page::Gone(&branch),
        };
        branches::mains_card(&self.root, page, rel, string(payload, "slug")).then(|| {
            let mut payload = payload.clone();
            payload["path"] = json!(rel);
            payload
        })
    }

    fn scope(&self, scope: &str, rel: &str) -> Result<(PathBuf, String), Failure> {
        let (scope, rel) = self.named(scope, rel)?;
        let root = if scope.is_empty() {
            self.root.clone()
        } else {
            branches::listed(&self.root)
                .ok_or_else(|| Failure {
                    status: 503,
                    error: "the worktrees could not be listed".into(),
                })?
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
        if let Some((text, value, ask)) = unpublished.get(&request).cloned() {
            crate::callbacks::publish_asking(&self.root, &text, ask.as_ref())
                .map_err(Failure::from)?;
            unpublished.remove(&request);
            return Ok(value);
        }
        drop(unpublished);
        // The owner's line on a card main holds is main's, whichever branch's page it was given on.
        let moved = matches!(route, "/api/decision" | "/api/message")
            .then(|| self.mains(scope, payload))
            .flatten();
        let (scope, payload) = match &moved {
            Some(moved) => ("", moved),
            None => (scope, payload),
        };
        // The ask this save opens or joins, if it waits on an agent's answer.
        let mut asking: Option<NewAsk> = None;
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
                self.document(scope, route, payload).map(|(value, ask)| {
                    asking = ask;
                    value
                })
            }
            "/api/task-message" => {
                // The board opens the task's ask in the message's own write.
                let to = string(payload, "to");
                self.store()?.say_asking(
                    string(payload, "slug"),
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
            WORKFLOW_APPLY => {
                let request: symphony_api::workflow::OperationRequest =
                    serde_json::from_value(payload["request"].clone())
                        .map_err(|e| format!("request: not a workflow operation: {e}"))?;
                if request.task != string(payload, "slug") {
                    return Err(format!(
                        "request: its task is {}, not the slug {}",
                        request.task,
                        string(payload, "slug")
                    )
                    .into());
                }
                // A guarded edge's attestations, each the project authority's signed word: the
                // payload's as given, or where it gives none, asked of the authorities the
                // project's plugins answer for (attest.rs).
                let given: Option<Vec<symphony_api::workflow::Signed>> =
                    match payload.get("attestations") {
                        None | Some(Value::Null) => None,
                        Some(given) => Some(
                            serde_json::from_value(given.clone())
                                .map_err(|e| format!("attestations: not signed statements: {e}"))?,
                        ),
                    };
                let (store, owner) = (self.store()?, symphony_board::workflow::local_owner());
                let attestations = match given {
                    Some(given) => given,
                    None => attest::fetched(
                        &store,
                        &crate::plugins::Plugins::of(&self.root),
                        &self.root,
                        &request,
                        &owner,
                    )?,
                };
                let result = store.apply_attested(
                    &owner,
                    &symphony_board::workflow::Host::local(),
                    &request,
                    &attestations,
                )?;
                Ok(json!({"ok": true, "result": result}))
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
                let done = match actions.iter().find(|action| action.name() == named) {
                    Some(action) => action.submit(
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
                    )?,
                    // The Symphony's own, where no plugin's has its name: the owner's approve of
                    // the tip its offer showed, the line the `owner` stamp reads.
                    None if named == crate::gates::stamps::OWNER_APPROVE => {
                        symphony_plugin::Submitted {
                            said: crate::gates::stamps::approve_line(string(payload, "payload"))
                                .map_err(|error| Failure { status: 400, error })?,
                            released: Vec::new(),
                        }
                    }
                    None => {
                        return Err(Failure {
                            status: 404,
                            error: "no such board action".into(),
                        });
                    }
                };
                // With a note the owner's line waits on an agent: it opens the task's ask.
                if string(payload, "note").trim().is_empty() {
                    store.say(slug, "owner", &done.said, None)?;
                } else {
                    store.say_asking(slug, &done.said, None)?;
                }
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
            if matches!(route, "/api/ask" | "/api/ask-reply" | "/api/quick-chat") {
                asking = self.chat_ask(scope, route, payload, value);
            }
            let notify = matches!(
                route,
                "/api/ask" | "/api/ask-reply" | "/api/quick-chat" | "/api/message" | "/api/comment"
            ) || ((route == "/api/decision"
                || route == symphony_api::TASK_ACTION_ROUTE)
                && !string(payload, "note").trim().is_empty());
            if value["repeat"] == true {
                // A repeated tap was journalled by the tap it repeats.
            } else if notify {
                let id = value["id"].as_i64();
                let db = asks::default_path(&self.root, std::env::var("ASKS_DB").ok().as_deref());
                let db = format!("'{}'", db.to_string_lossy().replace('\'', "'\\''"));
                let reply = id.map_or_else(|| format!("Reply in the document/card at {}#{} using the chat-doc skill.", string(payload, "path"), string(payload, "slug")), |id| format!("Read: just symphony-ask --db {db} show {id}. Answer: just symphony-ask --db {db} answer {id} TEXT --as HANDLE."));
                // When the owner said it, so a line delivered late (nobody heard chat then) is
                // never read as new; a retry publishes this same text.
                let text = format!(
                    "OWNER {route} at {} scope={scope} {}\n{reply}{}",
                    clock::now_stamp(),
                    payload,
                    attached(&self.root, payload)
                );
                // A saved document can already count as a repeated tap when journaling fails.
                // Retain both the callback and response so its HTTP retry publishes that same
                // save rather than silently skipping it (or creating another ask/comment).
                self.journal(&request, text, value, asking)?;
            } else if asking.is_some() {
                // Waiting on an agent with no words for the chat (a rejection with no note): the
                // journal opens its ask alone.
                self.journal(&request, String::new(), value, asking)?;
            }
        }
        result
    }

    /// The save `request`'s chat `text` and ask put in the journal for the shared server to hand to
    /// the board, kept until then so an HTTP retry of the save journals the same.
    fn journal(
        &self,
        request: &str,
        text: String,
        value: &Value,
        ask: Option<NewAsk>,
    ) -> Result<(), Failure> {
        let mut unpublished = self.unpublished.lock().unwrap_or_else(|e| e.into_inner());
        unpublished.insert(
            request.to_string(),
            (text.clone(), value.clone(), ask.clone()),
        );
        crate::callbacks::publish_asking(&self.root, &text, ask.as_ref()).map_err(Failure::from)?;
        unpublished.remove(request);
        Ok(())
    }

    /// The ask a saved card or comment opens or joins: a card the save leaves waiting on an agent
    /// ([`register::Decision::open`]: a rejection, a note, a message, an approval the project's
    /// card kinds say an agent applies), keyed by its card; a comment, keyed by its title, the block's
    /// first line. `text` is the document as saved.
    fn document_ask(
        &self,
        route: &str,
        payload: &Value,
        root: &Path,
        rel: &str,
        text: &str,
        comment: Option<&(String, String)>,
    ) -> Option<NewAsk> {
        let mut ask = NewAsk {
            path: Some(rel.to_string()),
            tree: self.tree_of(root),
            at: Some(epoch_now()),
            ..NewAsk::default()
        };
        if let Some((title, body)) = comment {
            ask.kind = "comment".into();
            ask.block = Some(title.clone());
            ask.words = format!("{title} {body}");
            return Some(ask);
        }
        let slug = string(payload, "slug");
        let lines: Vec<&str> = text.lines().collect();
        let plugins = crate::plugins::Plugins::of(&self.root);
        register::parse_decisions(&lines)
            .iter()
            .find(|card| card.slug == slug)
            .filter(|card| card.open(&plugins))?;
        let said = match route {
            "/api/message" => string(payload, "text"),
            _ => string(payload, "note"),
        };
        ask.kind = if route == "/api/message" {
            "message"
        } else {
            "verdict"
        }
        .into();
        ask.card = Some(slug.to_string());
        ask.words = if said.trim().is_empty() {
            string(payload, "state").to_string()
        } else {
            said.to_string()
        };
        Some(ask)
    }

    fn document(
        &self,
        scope: &str,
        route: &str,
        payload: &Value,
    ) -> Result<(Value, Option<NewAsk>), Failure> {
        let (root, rel) = self.scope(scope, string(payload, "path"))?;
        let target = Self::target(&root, &rel)?;
        // On the default branch of a project that delivers by pull request the save writes no file
        // and commits nothing here: it is kept and delivered by a pull request of its own
        // (`commit::pending`), and answered as pending.
        let main = root == self.root && commit::on_main(&root);
        let pending = main && commit::delivers(&root);
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
        if !tap.is_empty()
            && let Some((_, state, note)) = taps.iter().find(|(held, ..)| *held == tap)
        {
            return Ok((
                json!({"ok":true,"repeat":true,"state":state,"note":note}),
                None,
            ));
        }
        let original = std::fs::read_to_string(&target).map_err(|e| e.to_string())?;
        // Written against the document as the saves still waiting leave it, as its page shows it.
        let original = if pending {
            let file = crate::project::Project::of(&root).file(&rel);
            commit::pending::shown(&root, &file, original)
        } else {
            original
        };
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
        let comment = if route == "/api/comment" {
            Some(if payload.get("text").is_some() {
                edit::split_message(string(payload, "text"))?
            } else {
                (
                    string(payload, "title").to_string(),
                    string(payload, "body").to_string(),
                )
            })
        } else {
            None
        };
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
                let (title, body) = comment.as_ref().expect("a comment's words were read");
                let line = payload["after_line"]
                    .as_i64()
                    .or_else(|| payload["after_line"].as_str().and_then(|s| s.parse().ok()))
                    .unwrap_or(0);
                edit::insert_comment(&original, line, string(payload, "kind"), title, body)?
            }
            _ => {
                if edit::is_repeat_decision(
                    &original,
                    slug,
                    string(payload, "state"),
                    string(payload, "note"),
                    &now,
                )? {
                    return Ok((
                        json!({"ok":true,"repeat":true,
                            "state":string(payload, "state"),"note":string(payload, "note")}),
                        None,
                    ));
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
        if !pending {
            std::fs::write(&target, &text).map_err(|e| e.to_string())?;
        }
        let ask = self.document_ask(route, payload, &root, &rel, &text, comment.as_ref());
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
        if main {
            self.committer
                .get_or_init(|| commit::Committer::start(&self.root, self.writes.clone()))
                .submit(commit::Edit {
                    rel: crate::project::Project::of(&root).file(&rel),
                    base: original,
                    new: text,
                })?;
        }
        if !tap.is_empty() {
            taps.push_back((
                tap,
                string(payload, "state").into(),
                string(payload, "note").into(),
            ));
            while taps.len() > 512 {
                taps.pop_front();
            }
        }
        drop(taps);
        drop(_held);
        if pending {
            // Kept, not committed: the page says so until the remote holds it.
            return Ok((json!({"ok":true,"delivery":"pending"}), ask));
        }
        Ok((json!({"ok":true}), ask))
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
