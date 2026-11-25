//! The owner's save operations, independent of the HTTP transport. One lock covers all register
//! reads, edits and writes, including checkout synchronization by the verdict committer.
//!
//! Every save takes the actor who asked for it ([`symphony_plugin::Actor`], [`Saves::post_as`]),
//! which its host supplies: its line on a card, its post and moves on the board, the author of its
//! ask, its settings change and what a board action's plugin is told carry the actor's short name,
//! and a workflow edge is taken as its principal. The local host's actor is the person at the
//! machine ([`symphony_board::workflow::local_actor`]), whose every line reads `owner` as it always
//! has; [`Saves::post`] is that actor's. What only shares the word is left as it is: whose queue
//! a Later is ([`crate::postpone::OWNER`]), the stamp's kind and the words the chat reads.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{Value, json};
use symphony_board::NewAsk;
use symphony_board::lock::Recovered as _;

use symphony_plugin::Actor;

use crate::{asks, branches, clock, commit, edit, register, taskview};

mod attest;
mod failure;
mod held;
mod quick;
mod quick_task;
mod settings;
pub use failure::{Failure, Refused};

#[cfg(test)]
mod actor_tests;
#[cfg(test)]
mod close_tests;
#[cfg(test)]
mod hand_tests;
#[cfg(test)]
mod lock_tests;

pub const ROUTES: [&str; 19] = [
    "/api/postpone",
    "/api/decision",
    "/api/comment",
    "/api/message",
    "/api/task-message",
    symphony_api::TASK_PASS_ROUTE,
    symphony_api::DOCUMENT_PASS_ROUTE,
    "/api/task-priority",
    "/api/task-field",
    "/api/task-cancel",
    "/api/quick-chat",
    "/api/quick-task",
    "/api/ask",
    "/api/ask-reply",
    symphony_api::TASK_ACTION_ROUTE,
    WORKFLOW_APPLY,
    symphony_api::SETTING_WRITE_PATH,
    symphony_api::TASK_HAND_ROUTE,
    symphony_api::CARD_HAND_ROUTE,
];

/// The owner's operation on a task a custom workflow holds, through the board's one admission
/// (`symphony_board::Store::apply_attested`): `{"slug": SLUG, "request": OperationRequest}`, with
/// `"attestations": [Signed]` for a guarded edge, or with none, each guard's asked of the project
/// authority a selected plugin answers for its adapter (`attest.rs`); the principal the local
/// owner, never one the payload names ([`symphony_api::workflow::WORKFLOW_APPLY_ROUTE`], which
/// the pages post to).
pub const WORKFLOW_APPLY: &str = symphony_api::workflow::WORKFLOW_APPLY_ROUTE;

/// The routes that write to one task, named by the payload's `slug`.
const TASK_ROUTES: [&str; 8] = [
    "/api/task-message",
    symphony_api::TASK_PASS_ROUTE,
    "/api/task-priority",
    "/api/task-field",
    "/api/task-cancel",
    symphony_api::TASK_ACTION_ROUTE,
    WORKFLOW_APPLY,
    symphony_api::TASK_HAND_ROUTE,
];

/// The task routes that decide the task: its move, priority, field, cancel and a board action (the
/// approve that meets its `owner` stamp among them), which only those of its owner make
/// ([`crate::owners::refused_task`]). A message speaks and a hand-over is its own rule's.
const DECIDING: [&str; 4] = [
    "/api/task-priority",
    "/api/task-field",
    "/api/task-cancel",
    symphony_api::TASK_ACTION_ROUTE,
];

/// Refused, with nothing written, where `refused` says why its asker may not decide what they
/// write to ([`Failure::forbidden`]).
fn deciding(refused: Result<Option<String>, symphony_board::Error>) -> Result<(), Failure> {
    match refused? {
        Some(why) => Err(Failure::forbidden(&why)),
        None => Ok(()),
    }
}

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
            let _held = self.writes.lock().recover();
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

    /// The board, for the writes `actor` asked for: these saves vouch for the person their host
    /// admitted, by the short name their lines carry ([`symphony_board::Store::for_actor`]): the
    /// person at the machine, under `owner` or the name they gave themselves, or a person a board
    /// named.
    fn store_for(&self, actor: &Actor) -> Result<symphony_board::Store, Failure> {
        Ok(self.store()?.for_actor(actor))
    }

    /// The board, for the writes `by` asked for, a record kept from such a write naming them
    /// (a quick task's): the person at the machine where `machine`
    /// ([`symphony_board::Store::for_machine`]), else a person a board named.
    fn store_as(&self, by: &str, machine: bool) -> Result<symphony_board::Store, Failure> {
        let store = self.store()?;
        Ok(if machine {
            store.for_machine(by)
        } else {
            store.for_person(by)
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

    /// The owner's workflow operation ([`WORKFLOW_APPLY`]), as [`Self::post`] takes it, a
    /// refusal answered with the guard that refused where one did ([`Refused`]): what a page
    /// shows beside the action, the refusal's words unchanged.
    pub fn apply_workflow(&self, payload: &Value) -> Result<Value, Refused> {
        self.apply_workflow_as(&symphony_board::workflow::local_actor(), payload)
    }

    /// [`Self::apply_workflow`] asked by `actor`, whose principal takes the edge and is handed to
    /// any approval authority a guard asks.
    pub fn apply_workflow_as(&self, actor: &Actor, payload: &Value) -> Result<Value, Refused> {
        if !payload.is_object() {
            return Err(Failure::from("bad JSON: body must be a JSON object".to_string()).into());
        }
        if string(payload, "slug").trim().is_empty() {
            return Err(Failure::from("slug is required".to_string()).into());
        }
        self.workflow_apply(actor, payload)
    }

    fn workflow_apply(&self, actor: &Actor, payload: &Value) -> Result<Value, Refused> {
        let request: symphony_api::workflow::OperationRequest =
            serde_json::from_value(payload["request"].clone())
                .map_err(|e| Failure::from(format!("request: not a workflow operation: {e}")))?;
        if request.task != string(payload, "slug") {
            return Err(Failure::from(format!(
                "request: its task is {}, not the slug {}",
                request.task,
                string(payload, "slug")
            ))
            .into());
        }
        // A guarded edge's attestations, each the project authority's signed word: the payload's
        // as given, or where it gives none, asked of the authorities the project's plugins answer
        // for (attest.rs).
        let given: Option<Vec<symphony_api::workflow::Signed>> =
            match payload.get("attestations") {
                None | Some(Value::Null) => None,
                Some(given) => Some(serde_json::from_value(given.clone()).map_err(|e| {
                    Failure::from(format!("attestations: not signed statements: {e}"))
                })?),
            };
        let (store, owner) = (self.store_for(actor)?, &actor.principal);
        // A person moves a task by its workflow only while they are of its owner, refused here in
        // the board's words before any authority is asked; its host grants them its edges as well.
        if crate::owners::person(actor).is_some() {
            let task = store.get(&request.task).map_err(Failure::from)?;
            deciding(crate::owners::refused_task(&store, &task, actor))?;
        }
        let attestations = match given {
            Some(given) => given,
            None => attest::fetched(
                &store,
                &crate::plugins::Plugins::of(&self.root),
                &self.root,
                &request,
                owner,
            )?,
        };
        let host = store.host_on(owner, &request.task).map_err(|e| Refused {
            failure: Failure::from(e),
            guard: None,
        })?;
        match store.apply_attested(owner, &host, &request, &attestations) {
            Ok(result) => Ok(json!({"ok": true, "result": result})),
            Err(refused) => {
                // Which guard it was, read after the refusal: none where the board answers none,
                // or cannot be read again. Judged on the board's error, never on the words a
                // failure makes of it.
                let guard = (attest::guards(&refused))
                    .then(|| store.guard_refusal(owner, &request, &attestations))
                    .and_then(|read| read.ok().flatten())
                    .map(Box::new);
                Err(Refused {
                    failure: Failure::from(refused),
                    guard,
                })
            }
        }
    }

    /// The save `route` of the local owner ([`symphony_board::workflow::local_actor`]).
    pub fn post(&self, scope: &str, route: &str, payload: &Value) -> Result<Value, Failure> {
        self.post_as(
            &symphony_board::workflow::local_actor(),
            scope,
            route,
            payload,
        )
    }

    /// The save `route` asked by `actor`, as its host authenticated them, never as a payload names
    /// them.
    pub fn post_as(
        &self,
        actor: &Actor,
        scope: &str,
        route: &str,
        payload: &Value,
    ) -> Result<Value, Failure> {
        self.post_inner(actor, scope, route, payload, None)
    }

    /// Apply an app request against the same decision metadata its reader was shown.
    pub fn post_with_order(
        &self,
        scope: &str,
        route: &str,
        payload: &Value,
        order: &crate::postpone::View,
    ) -> Result<Value, Failure> {
        let actor = symphony_board::workflow::local_actor();
        self.post_with_order_as(&actor, scope, route, payload, order)
    }

    /// [`Self::post_with_order`] asked by `actor` ([`Self::post_as`]).
    pub fn post_with_order_as(
        &self,
        actor: &Actor,
        scope: &str,
        route: &str,
        payload: &Value,
        order: &crate::postpone::View,
    ) -> Result<Value, Failure> {
        self.post_inner(actor, scope, route, payload, Some(order))
    }

    fn post_inner(
        &self,
        actor: &Actor,
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
        let mut unpublished = self.unpublished.lock().recover();
        if let Some((text, value, ask)) = unpublished.get(&request).cloned() {
            crate::callbacks::publish_asking(&self.root, &text, ask.as_ref())
                .map_err(Failure::from)?;
            unpublished.remove(&request);
            return Ok(value);
        }
        drop(unpublished);
        // The owner's line on a card main holds is main's, whichever branch's page it was given on.
        let moved = matches!(
            route,
            "/api/decision" | "/api/message" | symphony_api::CARD_HAND_ROUTE
        )
        .then(|| self.mains(scope, payload))
        .flatten();
        let (scope, payload) = match &moved {
            Some(moved) => ("", moved),
            None => (scope, payload),
        };
        // The ask this save opens or joins, if it waits on an agent's answer.
        let mut asking: Option<NewAsk> = None;
        // What the chat is told was said, where it is not this save's own route and payload: a
        // held comment's pass tells the comment it wrote, its author's words.
        let mut told: Option<Told> = None;
        let result = match route {
            "/api/postpone" => self.postpone(actor, scope, payload, order),
            "/api/decision" | "/api/comment" | "/api/message" | symphony_api::CARD_HAND_ROUTE => {
                self.document(actor, scope, route, payload, None)
                    .map(|(value, ask)| {
                        asking = ask;
                        value
                    })
            }
            symphony_api::DOCUMENT_PASS_ROUTE => {
                self.pass_held(actor, payload).map(|(value, ask, said)| {
                    asking = ask;
                    told = Some(said);
                    value
                })
            }
            "/api/task-message"
            | symphony_api::TASK_PASS_ROUTE
            | "/api/task-priority"
            | "/api/task-field"
            | "/api/task-cancel"
            | symphony_api::TASK_HAND_ROUTE
            | symphony_api::TASK_ACTION_ROUTE => on_task(
                &self.store_for(actor)?,
                || crate::plugins::Plugins::of(&self.root),
                &self.root,
                actor,
                route,
                payload,
            ),
            WORKFLOW_APPLY => {
                (self.workflow_apply(actor, payload)).map_err(|refused| refused.failure)
            }
            "/api/ask" | "/api/ask-reply" => self.ask(actor, scope, route, payload),
            symphony_api::SETTING_WRITE_PATH => self
                .settings_refused(actor)
                .and_then(|()| self.setting(scope, payload, actor)),
            "/api/quick-chat" | "/api/quick-task" => self.quick(scope, route, payload, actor),
            _ => Err(Failure {
                status: 404,
                error: "no such route".into(),
            }),
        };
        if let Ok(value) = &result {
            if matches!(route, "/api/ask" | "/api/ask-reply" | "/api/quick-chat") {
                asking = self.chat_ask(scope, route, payload, value);
            }
            let (route, scope, payload, by) = match &told {
                Some(told) => (told.route, told.scope.as_str(), &told.payload, &*told.by),
                None => (route, scope, payload, actor.name()),
            };
            let notify = matches!(
                route,
                "/api/ask" | "/api/ask-reply" | "/api/quick-chat" | "/api/message" | "/api/comment"
            ) || ((route == "/api/decision"
                || route == symphony_api::TASK_ACTION_ROUTE)
                && !string(payload, "note").trim().is_empty());
            if value.get("held").is_some() {
                // Held for people: no ask, and nothing for the chat.
            } else if value["repeat"] == true {
                // A repeated tap was journalled by the tap it repeats.
            } else if notify {
                let id = value["id"].as_i64();
                let db = asks::default_path(&self.root, std::env::var("ASKS_DB").ok().as_deref());
                let db = format!("'{}'", db.to_string_lossy().replace('\'', "'\\''"));
                let reply = id.map_or_else(|| format!("Reply in the document/card at {}#{} using the chat-doc skill.", string(payload, "path"), string(payload, "slug")), |id| format!("Read: just symphony-ask --db {db} show {id}. Answer: just symphony-ask --db {db} answer {id} TEXT --as HANDLE."));
                // When the owner said it, so a line delivered late (nobody heard chat then) is
                // never read as new; a retry publishes this same text. A person a board named is
                // said by name (`by=ana`); the person at the machine's line reads as it always has,
                // as `owner` is no one's short name.
                let by = if symphony_docs::syntax::is_short_name(by) {
                    format!(" by={by}")
                } else {
                    String::new()
                };
                let text = format!(
                    "OWNER {route} at {} scope={scope}{by} {}\n{reply}{}",
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

    /// Refused, with nothing written, where `actor`, a person a board named, is not of the Settings
    /// page's owner ([`crate::owners::refused_settings`]): nothing is read for the person at the
    /// machine.
    fn settings_refused(&self, actor: &Actor) -> Result<(), Failure> {
        if crate::owners::person(actor).is_none() {
            return Ok(());
        }
        deciding(crate::owners::refused_settings(
            &self.store_for(actor)?,
            actor,
        ))
    }

    /// Why the person `actor` may not move the card `slug` of the document `rel` (its file
    /// `target` in the checkout at `root`) in the queue: the refusal of its verdict, read from the
    /// document as its page shows it. None for a card the document does not hold, which the order
    /// answers in its own words.
    fn postpone_refused(
        &self,
        actor: &Actor,
        root: &Path,
        rel: &str,
        target: &Path,
        slug: &str,
    ) -> Result<(), Failure> {
        let text = std::fs::read_to_string(target).map_err(|e| e.to_string())?;
        let text = if root == self.root && commit::on_main(root) && commit::delivers(root) {
            let file = crate::project::Project::of(root).file(rel);
            commit::pending::shown(root, &file, text)
        } else {
            text
        };
        let cards = register::parse_decisions(&crate::text::splitlines(&text));
        let Some(card) = cards.iter().find(|card| card.slug == slug) else {
            return Ok(());
        };
        let store = self.store_for(actor)?;
        deciding(crate::owners::refused_card(&store, rel, card, actor))
    }

    /// A card postponed, brought back or visited for the owner: `payload`'s `action` on the card
    /// `slug` of its page, by the decisions its reader was shown (`order`). A Later is judged
    /// against the waiting cards, read now where its reader was shown none, and answers where the
    /// next waiting card is; an Undo or a visit reads no document. The order is the board's one,
    /// so each of the three moves the card in the queue of every one who decides it: a person a
    /// board named who is not of its owner is refused it as its verdict is
    /// ([`crate::owners::refused_card`]), with nothing written.
    fn postpone(
        &self,
        actor: &Actor,
        scope: &str,
        payload: &Value,
        order: Option<&crate::postpone::View>,
    ) -> Result<Value, Failure> {
        use crate::postpone::{OWNER, now};
        let failed = |e: crate::ReviewError| Failure::from(e.to_string());
        let (root, rel) = self.scope(scope, string(payload, "path"))?;
        let target = Self::target(&root, &rel)?;
        let (action, slug) = (string(payload, "action"), string(payload, "slug"));
        if crate::owners::person(actor).is_some() {
            self.postpone_refused(actor, &root, &rel, &target, slug)?;
        }
        let page = self.page(&root, &rel, "")?;
        let page = page.trim_start_matches('/');
        if action != "later" {
            let changed = match order {
                Some(order) => {
                    let path = order.path_for(page, slug).unwrap_or(page);
                    order.act(
                        &self.root,
                        OWNER,
                        path.trim_start_matches('/'),
                        slug,
                        action,
                        now(),
                    )
                }
                None => crate::postpone::act(&self.root, OWNER, page, slug, action, now()),
            }
            .map_err(failed)?;
            return Ok(json!({"ok":true, "changed":changed, "next":null}));
        }
        let fallback;
        let order = match order {
            Some(order) => order,
            None => {
                fallback =
                    crate::dashboard::snapshot(&self.root, &branches::branch_cards(&self.root), "")
                        .map_err(|e| Failure::from(e.to_string()))?;
                fallback.order.as_ref()
            }
        };
        let path = order
            .path_for(page, slug)
            .unwrap_or(page)
            .trim_start_matches('/');
        let changed = order
            .act(&self.root, OWNER, path, slug, action, now())
            .map_err(failed)?;
        let next = order
            .waiting(&self.root, OWNER, now())
            .map_err(failed)?
            .iter()
            .find(|card| card.doc != path || card.slug != slug)
            .map(|card| card.href());
        Ok(json!({"ok":true, "changed":changed, "next":next}))
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
        let mut unpublished = self.unpublished.lock().recover();
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

    /// A card's verdict, its message, a comment or a card's hand-over on the document `payload`
    /// names, saved for `actor`. A comment or a card's message of a person a board named who may
    /// not direct agents is held for people (`held.rs`): checked as it would be written, kept as
    /// the board's row alone, and answered `"held"`, its number, with no ask. `pass`, a held comment
    /// `actor` passes on, is written as its author's, where it was left, and marked passed.
    fn document(
        &self,
        actor: &Actor,
        scope: &str,
        route: &str,
        payload: &Value,
        pass: Option<&symphony_board::held::OnDocument>,
    ) -> Result<(Value, Option<NewAsk>), Failure> {
        let (root, rel) = self.scope(scope, string(payload, "path"))?;
        let target = Self::target(&root, &rel)?;
        // Whose words the file is given: a passed comment's are its author's.
        let name = pass.map_or(actor.name(), |held| held.author.as_str());
        let holding = match crate::owners::person(actor) {
            Some(person) if pass.is_none() && matches!(route, "/api/comment" | "/api/message") => {
                let store = self.store_by(person)?;
                held::holds(&store, person)?.then_some(store)
            }
            _ => None,
        };
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
        let _held = self.writes.lock().recover();
        let _file = held_write_lock(&root)?;
        if self.closed.load(Ordering::Acquire) {
            return Err(Failure {
                status: 503,
                error: "document saves are closing".into(),
            });
        }
        // A comment passed on once: another pass that came first is refused here, under the lock.
        let passing = match pass {
            Some(passed) => {
                let store = self.store_by(actor.name())?;
                let now = store.held_comment(passed.id)?;
                if let Some(refused) = held::not_held(passed.id, now.as_ref()) {
                    return Err(refused);
                }
                Some((store, passed))
            }
            None => None,
        };
        let mut taps = self.taps.lock().recover();
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
        // A verdict is a person's only on a card they are of the owner of, read as the page shows
        // the card; a card the document lacks is refused by the edit below, in its own words.
        if route == "/api/decision" && crate::owners::person(actor).is_some() {
            let lines = crate::text::splitlines(&original);
            let cards = register::parse_decisions(&lines);
            if let Some(card) = cards.iter().find(|card| card.slug == slug) {
                let store = self.store_for(actor)?;
                deciding(crate::owners::refused_card(&store, &rel, card, actor))?;
            }
        }
        // A card handed on: the board records who did once the file's edit is known to be whole,
        // just before it is written.
        let mut handing: Option<Handing> = None;
        let text = match route {
            symphony_api::CARD_HAND_ROUTE => {
                let (text, hand) = handed(&rel, &original, slug, payload)?;
                handing = Some(hand);
                text
            }
            "/api/message" => {
                edit::add_message_by(&original, slug, string(payload, "text"), &now, name, None)?
            }
            "/api/comment" => {
                let (title, body) = comment.as_ref().expect("a comment's words were read");
                let line = match pass.and_then(|held| held.document.block.as_ref()) {
                    Some(block) => held::found_again(&original, block)?,
                    None => after_line(payload),
                };
                let kind = string(payload, "kind");
                edit::insert_comment_by(&original, line, kind, title, body, name)?
            }
            _ => {
                if edit::is_repeat_decision_by(
                    &original,
                    slug,
                    string(payload, "state"),
                    string(payload, "note"),
                    &now,
                    actor.name(),
                )? {
                    return Ok((
                        json!({"ok":true,"repeat":true,
                            "state":string(payload, "state"),"note":string(payload, "note")}),
                        None,
                    ));
                }
                edit::set_decision_by(
                    &original,
                    slug,
                    string(payload, "state"),
                    string(payload, "note"),
                    &now,
                    &crate::plugins::Plugins::of(&self.root),
                    actor.name(),
                )?
            }
        };
        // Held: what was checked against the document is kept by the board, and nothing else.
        if let Some(store) = holding {
            let (named, _) = self.named(scope, string(payload, "path"))?;
            let place = held::place(route, named, &rel, &original, payload, after_line(payload))?;
            let held = store.hold_on_document(name, &held::words(route, payload), &place)?;
            return Ok((json!({"ok": true, "held": held.id}), None));
        }
        let (superseded, _) = register::supersede(&text, &now);
        // A hand-over rewrites its card's first line, and that alone.
        let whole = if handing.is_some() { &text } else { &original };
        edit::check_nothing_lost(whole, &superseded, slug)?;
        let text = superseded;
        let text = match &conflict {
            Some(conflict) => conflict.with_card(&text, slug).ok_or_else(|| Failure {
                status: 409,
                error: format!(
                    "{rel} is being merged in its worktree and the card {slug} sits across a conflict there: answer once the merge is done"
                ),
            })?,
            None => text,
        };
        let handed = match &handing {
            Some(hand) => {
                let store = self.store_for(actor)?;
                let to = hand.to.as_str();
                let written = hand.written.as_deref();
                let Some(line) = store.hand_card(&hand.thing, written, to, actor.name())? else {
                    return Ok((json!({"ok":true,"repeat":true}), None));
                };
                Some((store, line))
            }
            None => None,
        };
        // A hand-over's line names a `*For:*` the card's text holds: where that text is kept
        // nowhere, the line is taken back, so the board never says a card was handed that was not.
        let take_back = |error: String| -> String {
            if let Some((store, line)) = &handed
                && let Err(lost) = store.take_back(line)
            {
                eprintln!("· could not take back hand-over line {}: {lost}", line.n);
            }
            error
        };
        if !pending {
            std::fs::write(&target, &text).map_err(|e| take_back(e.to_string()))?;
        }
        let ask = self.document_ask(route, payload, &root, &rel, &text, comment.as_ref());
        if !matches!(route, "/api/comment" | symphony_api::CARD_HAND_ROUTE) {
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
                .submit_as(
                    commit::Edit {
                        rel: crate::project::Project::of(&root).file(&rel),
                        base: original,
                        new: text,
                    },
                    actor,
                )
                // Kept for delivery, a save is written nowhere else: one not kept keeps no line.
                .map_err(|error| if pending { take_back(error) } else { error })?;
        }
        // Its words are in the file, or kept for its delivery: the board marks it passed.
        if let Some((store, passed)) = &passing {
            store.pass_on_document(passed.id, actor.name())?;
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

    /// The held comment `payload`'s `id` passed on by `actor` ([`symphony_api::DOCUMENT_PASS_ROUTE`]):
    /// written into its document as its author's own save would write it, their name on it and
    /// the passer's first in its words, where it was left (`held.rs`), then marked passed. Its ask
    /// and the chat's callback are its author's comment's ([`Told`]). Refused, with nothing
    /// written: no comment held by that number, one passed already, and its place not found again.
    fn pass_held(
        &self,
        actor: &Actor,
        payload: &Value,
    ) -> Result<(Value, Option<NewAsk>, Told), Failure> {
        let id =
            (payload["id"].as_i64()).ok_or("id must be a held comment's number".to_string())?;
        let found = self.store_by(actor.name())?.held_comment(id)?;
        if let Some(refused) = held::not_held(id, found.as_ref()) {
            return Err(refused);
        }
        let found = found.expect("a comment not refused is held");
        let (route, said) = held::passed_save(&found, actor.name());
        let scope = found.document.scope.clone().unwrap_or_default();
        let (value, ask) = self.document(actor, &scope, route, &said, Some(&found))?;
        let told = Told {
            route,
            scope,
            payload: said,
            by: found.author.clone(),
        };
        Ok((value, ask, told))
    }

    fn ask(
        &self,
        actor: &Actor,
        scope: &str,
        route: &str,
        payload: &Value,
    ) -> Result<Value, Failure> {
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
            let id = store.ask_by(
                &rel,
                &question,
                string(payload, "anchor"),
                string(payload, "quote"),
                actor.name(),
            )?;
            Ok(json!({"id":id}))
        } else {
            let id = payload["id"]
                .as_i64()
                .or_else(|| payload["id"].as_str().and_then(|s| s.parse().ok()))
                .ok_or("id must be a number".to_string())?;
            store.reply_by(id, string(payload, "text"), actor.name())?;
            Ok(json!({"ok":true,"id":id}))
        }
    }
}

/// What the chat is told a save said, where it is not the save's own route and payload: the route
/// and payload of the save it made, the checkout's scope and whose words they are.
struct Told {
    route: &'static str,
    scope: String,
    payload: Value,
    by: String,
}

/// The line a comment's `payload` goes after, `after_line`, a number or its text: 0 where none.
fn after_line(payload: &Value) -> i64 {
    payload["after_line"]
        .as_i64()
        .or_else(|| payload["after_line"].as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0)
}

/// A card a save hands on: the card as the board's log names it, the name written on it before,
/// and the name it is handed to.
struct Handing {
    thing: symphony_board::people::ownership::Thing,
    written: Option<String>,
    to: String,
}

/// The document `original` (of route `rel`) with its card `slug` written for `payload`'s `to`
/// ([`symphony_docs::syntax::card_line_for`]), and the hand-over the board is then to record and
/// may refuse ([`symphony_board::Store::hand_card`]). Refused for a card missing, one that waits on
/// no answer, and a `to` that is no short name's form.
fn handed(
    rel: &str,
    original: &str,
    slug: &str,
    payload: &Value,
) -> Result<(String, Handing), Failure> {
    let to = string(payload, "to").trim();
    let lines = crate::text::splitlines(original);
    let cards = register::parse_decisions(&lines);
    let card = (cards.iter().find(|card| card.slug == slug))
        .ok_or_else(|| format!("no decision with slug {}", crate::text::repr(slug)))?;
    if !crate::owners::waits(card.state) {
        return Err(format!(
            "the card {slug} is {}: only a card that waits on an answer is handed on",
            card.state.as_str()
        )
        .into());
    }
    let at = card.line - 1;
    let line = symphony_docs::syntax::card_line_for(lines[at], to).ok_or_else(|| {
        format!("a card is handed to a group's or a person's short name, or owner, not {to:?}")
    })?;
    let mut out: Vec<String> = lines.iter().map(|line| line.to_string()).collect();
    out[at] = line;
    let hand = Handing {
        thing: crate::owners::thing(rel, card),
        written: symphony_docs::syntax::card_for(&card.text).map(str::to_string),
        to: to.to_string(),
    };
    Ok((out.join("\n") + "\n", hand))
}

/// A write to one task on the board `store` (a message, a held message passed on, its priority, a
/// field, its cancel, a board action), asked by `actor`, `plugins` the project's at `root`, read
/// only by a field or an action: the actor's short name is the sender and the mover the board
/// records, as it always was `owner` for the person at the machine. A message of a person who may
/// not direct agents is held for people (`symphony_board::held`), and answers `"held"`: its id. A
/// write that decides the task ([`DECIDING`]) is refused, with nothing written, to a person not of
/// its owner.
fn on_task(
    store: &symphony_board::Store,
    plugins: impl FnOnce() -> crate::plugins::Plugins,
    root: &Path,
    actor: &Actor,
    route: &str,
    payload: &Value,
) -> Result<Value, Failure> {
    let slug = string(payload, "slug");
    if DECIDING.contains(&route) && crate::owners::person(actor).is_some() {
        deciding(crate::owners::refused_task(store, &store.get(slug)?, actor))?;
    }
    match route {
        "/api/task-message" => {
            // The board opens the task's ask in the message's own write, or holds it.
            let to = string(payload, "to");
            let said = store.say_or_hold_as(
                slug,
                actor.name(),
                string(payload, "text"),
                (!to.is_empty()).then_some(to),
            )?;
            Ok(match said {
                symphony_board::held::Said::Line(_) => json!({"ok":true}),
                symphony_board::held::Said::Held(held) => json!({"ok":true, "held": held.id}),
            })
        }
        symphony_api::TASK_PASS_ROUTE => {
            let id =
                (payload["id"].as_i64()).ok_or("id must be a held message's number".to_string())?;
            store.pass(slug, id, actor.name())?;
            Ok(json!({"ok":true}))
        }
        symphony_api::TASK_HAND_ROUTE => {
            let handed = store.hand_task(slug, string(payload, "to").trim(), actor.name())?;
            Ok(json!({"ok": true, "repeat": handed.is_none()}))
        }
        "/api/task-priority" => {
            let priority = payload["priority"]
                .as_i64()
                .or_else(|| payload["priority"].as_str().and_then(|s| s.parse().ok()))
                .ok_or("priority must be an integer from 0 to 4".to_string())?;
            store.set_priority(slug, priority, actor.name())?;
            Ok(json!({"ok":true}))
        }
        "/api/task-field" => {
            // An agent's own command (`tasks field … --as HANDLE`) names its session; a page's
            // write is who asked.
            let by = Some(string(payload, "actor"))
                .filter(|by| !by.is_empty())
                .unwrap_or(actor.name());
            let (plugin, value) = taskview::set_field(
                store,
                &plugins(),
                slug,
                string(payload, "name"),
                &payload["value"],
                by,
            )?;
            Ok(json!({"ok": true, "plugin": plugin, "value": value}))
        }
        "/api/task-cancel" => {
            store.move_to(slug, "cancelled", actor.name())?;
            Ok(json!({"ok":true}))
        }
        _ => task_action(store, &plugins(), root, actor, payload),
    }
}

/// The owner's board action on a task ([`symphony_api::TASK_ACTION_ROUTE`]): `payload`'s `action`,
/// submitted to the project's board action of that name with its `payload` and `note` and who sent
/// it ([`symphony_plugin::BoardAction::submit_from`]), or where none has it, the Symphony's own
/// owner approve. The line it says goes on the task `slug`'s thread as `actor`'s, and the work it
/// releases back to review.
fn task_action(
    store: &symphony_board::Store,
    plugins: &crate::plugins::Plugins,
    root: &Path,
    actor: &Actor,
    payload: &Value,
) -> Result<Value, Failure> {
    let slug = string(payload, "slug");
    let task = store.get(slug)?;
    let branch = crate::taskview::approval_branch(store, &task)?;
    let actions = plugins.board_actions();
    // The board action that made the offer, by the name the offer gave.
    let named = string(payload, "action");
    let done = match actions.iter().find(|action| action.name() == named) {
        Some(action) => action.submit_from(
            root,
            &symphony_plugin::TaskRef {
                slug,
                status: &task.status,
                branch: &branch,
            },
            &symphony_plugin::Submission {
                payload: string(payload, "payload").into(),
                note: string(payload, "note").into(),
            },
            &actor.sender,
        )?,
        // The Symphony's own, where no plugin's has its name: the owner's approve of the tip its
        // offer showed, the line the `owner` stamp reads.
        None if named == crate::gates::stamps::OWNER_APPROVE => symphony_plugin::Submitted {
            said: crate::gates::stamps::approve_line(string(payload, "payload"))
                .map_err(|error| Failure { status: 400, error })?,
            released: Vec::new(),
        },
        None => {
            return Err(Failure {
                status: 404,
                error: "no such board action".into(),
            });
        }
    };
    // With a note the owner's line waits on an agent: it opens the task's ask.
    if string(payload, "note").trim().is_empty() {
        store.say(slug, actor.name(), &done.said, None)?;
    } else {
        store.say_asking_as(slug, actor.name(), &done.said, None)?;
    }
    // The work it releases goes back to review. A move that fails here is made by the next landing
    // or stamp edit, which re-reads them.
    for held in &done.released {
        let _ = store.move_to(held, "review", actor.name());
    }
    Ok(json!({"ok":true, "said": done.said}))
}

/// The write lock a save shares with the lander ([`commit::write_lock`]), unlocked as it drops,
/// as every lock the committer takes is ([`commit::Held`]): closed alone, it would stay held by a
/// copy of its descriptor a child another thread spawned keeps until its exec, past the save.
fn held_write_lock(root: &Path) -> Result<Option<commit::Held>, String> {
    Ok(commit::write_lock(root)?.map(commit::Held))
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
