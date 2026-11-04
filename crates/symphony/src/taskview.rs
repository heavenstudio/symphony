//! The task pages' read API, directly over the Rust board. Failures remain visible to the HTTP
//! layer: a missing task is a 404, an unreachable board a 503.

use std::collections::BTreeMap;
use std::path::Path;

use sha2::{Digest, Sha256};
use symphony_api::{ActivityItem, Message, Role, Session, Task, TaskCounts, Tasks};

use crate::{clock, dashboard, render::Render};

pub fn snapshot(store: &tasks::Store) -> Result<(Tasks, i64), tasks::Error> {
    let board = store.board()?;
    Ok(snapshot_board(&board))
}

/// Project one shared board read into the wire snapshot, preserving its message cursor.
pub fn snapshot_board(board: &tasks::Board) -> (Tasks, i64) {
    let tasks: Vec<Task> = board.tasks.iter().map(task).collect();
    let sessions: Vec<Session> = board.sessions.iter().map(session).collect();
    // A version is opaque. Include every field the API returns, plus the last message, so a body,
    // title, role or thread edit moves it too.
    let bytes = serde_json::to_vec(&(&tasks, &sessions, board.latest_message))
        .expect("serializable board records");
    let version = digest(&bytes);
    (
        Tasks {
            version,
            tasks,
            sessions,
        },
        board.latest_message,
    )
}

/// An opaque revision of `bytes`: the first 8 bytes of their SHA-256, in hex.
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn counts(board: &Tasks) -> TaskCounts {
    TaskCounts {
        in_progress: dashboard::count(
            board
                .tasks
                .iter()
                .filter(|t| t.status == "in_progress")
                .count(),
        ),
        in_review: dashboard::count(board.tasks.iter().filter(|t| t.status == "review").count()),
        agents_live: dashboard::count(board.sessions.iter().filter(|s| s.health == "live").count()),
    }
}

pub fn signoff(
    store: &tasks::Store,
    tasks: &[Task],
    pending: &symphony_api::Pending,
) -> Result<Vec<String>, String> {
    let mut result = Vec::new();
    for task in tasks.iter().filter(|t| {
        !["done", "blocked", "cancelled"].contains(&t.status.as_str())
            && t.roles.iter().any(|r| r.role == "executor")
    }) {
        let cards: Vec<_> = pending
            .items
            .iter()
            .filter(|card| {
                task.links
                    .iter()
                    .any(|link| link.split('#').next() == Some(card.path.as_str()))
            })
            .collect();
        if cards.is_empty() {
            continue;
        }
        let names = |text: &str| {
            cards.iter().any(|card| {
                text.match_indices(&card.slug).any(|(at, slug)| {
                    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
                    !text[..at].chars().next_back().is_some_and(word)
                        && !text[at + slug.len()..].chars().next().is_some_and(word)
                })
            })
        };
        if names(&task.body)
            || store
                .thread(&task.slug)
                .map_err(|e| e.to_string())?
                .iter()
                .any(|m| names(&m.text))
        {
            result.push(task.slug.clone());
        }
    }
    Ok(result)
}

fn role(r: &tasks::Role) -> Role {
    Role {
        handle: r.handle.clone(),
        role: r.role.clone(),
        since: r.since,
        expires: r.expires,
        health: r.health.as_str().into(),
    }
}

fn task(t: &tasks::Task) -> Task {
    Task {
        slug: t.slug.clone(),
        title: t.title.clone(),
        body: t.body.clone(),
        status: t.status.clone(),
        priority: t.priority,
        parent: t.parent.clone(),
        created_by: t.created_by.clone(),
        created: t.created,
        updated: t.updated,
        links: t.links.clone(),
        depends: t.depends.clone(),
        roles: t.roles.iter().map(role).collect(),
        instruction: t.instruction,
        fields: fields(&t.fields),
    }
}

/// The most of a brief the task page renders and sends. A longer brief's page carries the blocks
/// its start holds and the byte the rest begins at; the owner reads the rest on demand from
/// `brief`, a `BRIEF_CHUNK` at a time, so the page costs the same however long its brief grows. A
/// block is cut between its lines, never inside one: a line longer than the limit is sent whole.
const BRIEF_LIMIT: usize = 24_000;
/// The most of a brief one `brief` call renders and sends.
const BRIEF_CHUNK: usize = 100_000;

/// The blocks of the brief `body` from the one holding byte `from`, as many as `limit` bytes hold,
/// and the byte the next part begins at: the brief's length when the rest fits. Every part renders
/// from one parse of the whole brief, so it means there what it means in the whole. The page's part
/// and every `brief` call cut a block longer than `BRIEF_LIMIT` at the same places, so a part read
/// from where another ended starts with the item or line after it.
fn brief_part(
    render: &Render,
    body: &str,
    from: usize,
    limit: usize,
) -> (Vec<symphony_api::Block>, usize) {
    let (fragment, next) = render.typed_reply_part(body, task_doc(), from, limit, BRIEF_LIMIT);
    (typed_blocks(fragment, render), next)
}

/// The page of `record`, already read by the caller. Children are counted from the rows right under
/// it: reading every descendant would load the whole board for a number.
pub fn page(
    store: &tasks::Store,
    record: &tasks::Task,
    root: &Path,
) -> Result<symphony_api::TaskPage, String> {
    let (blocks, brief_next) = brief_part(&Render::new(root), &record.body, 0, BRIEF_LIMIT);
    let mut task = task(record);
    task.body = record.body.get(..brief_next).unwrap_or_default().to_owned();
    Ok(symphony_api::TaskPage {
        task,
        blocks,
        children: Vec::new(),
        children_count: store
            .children(&record.slug)
            .map_err(|e| e.to_string())?
            .len(),
        brief_omitted: record.body.len() - brief_next,
        brief_next,
        brief_revision: digest(record.body.as_bytes()),
    })
}

/// The next part of the brief of `record` from byte `from`, for the owner who asks for what the
/// page left out.
pub fn brief(record: &tasks::Task, from: usize, root: &Path) -> symphony_api::TaskBrief {
    let (blocks, next) = brief_part(&Render::new(root), &record.body, from, BRIEF_CHUNK);
    symphony_api::TaskBrief {
        blocks,
        next,
        omitted: record.body.len() - next,
        revision: digest(record.body.as_bytes()),
    }
}

/// The first offer of `plugins`' board actions on `slug` that has a payload, whatever the task's
/// status: each action decides which work it offers on. Requested only when the owner reaches the
/// message/approval controls: preparing an offer must not delay the task's metadata and brief.
pub fn owner_offer(
    store: &tasks::Store,
    plugins: &crate::plugins::Plugins,
    slug: &str,
    root: &Path,
) -> Result<symphony_api::TaskOffer, String> {
    let task = store.get(slug).map_err(|e| e.to_string())?;
    let branch = approval_branch(store, &task).map_err(|e| e.to_string())?;
    let found = symphony_plugin::TaskRef {
        slug: &task.slug,
        status: &task.status,
        branch: &branch,
    };
    for action in plugins.board_actions() {
        let offer = action.offer(root, &found)?;
        if offer.payload.is_some() {
            return Ok(offer);
        }
    }
    Ok(symphony_api::TaskOffer {
        branch,
        ..symphony_api::TaskOffer::default()
    })
}

/// A task slug can differ from its branch when an ancestor already names a branch. Review
/// requests name their target in their brief; ordinary work uses its recorded executor branch.
pub(crate) fn approval_branch(
    store: &tasks::Store,
    task: &tasks::Task,
) -> Result<String, tasks::Error> {
    if let Some(branch) = task
        .body
        .trim_start()
        .strip_prefix("Review `")
        .and_then(|text| text.split_once("` before it lands"))
        .map(|(branch, _)| branch)
    {
        return Ok(branch.to_string());
    }
    if let Some(branch) = task.slug.strip_prefix("review/") {
        return Ok(branch.to_string());
    }
    Ok(store
        .task_sessions(&task.slug)?
        .into_iter()
        .rev()
        .find(|s| s.role == "executor" && !s.branch.is_empty())
        .map_or_else(|| task.slug.clone(), |s| s.branch))
}

pub fn history(
    store: &tasks::Store,
    slug: &str,
    before: Option<i64>,
    after: Option<i64>,
    root: &Path,
) -> Result<symphony_api::TaskHistory, tasks::Error> {
    store.get(slug)?;
    let all = store.thread(slug)?;
    let selected: Vec<_> = all
        .iter()
        .filter(|m| before.is_none_or(|id| m.id < id) && after.is_none_or(|id| m.id > id))
        .collect();
    let start = if after.is_some() {
        0
    } else {
        selected.len().saturating_sub(30)
    };
    let end = if after.is_some() {
        selected.len().min(30)
    } else {
        selected.len()
    };
    let render = Render::new(root);
    let entries: Vec<_> = selected[start..end]
        .iter()
        .map(|record| entry(&message(record), &render))
        .collect();
    Ok(symphony_api::TaskHistory {
        before: if start > 0 {
            entries.first().map(|e| e.message.id)
        } else {
            None
        },
        after: if end < selected.len() {
            entries.last().map(|e| e.message.id)
        } else {
            None
        },
        entries,
    })
}

/// A message's display data. The client owns its markup; owner/system words remain literal. An
/// owner message that attached files has blocks: its literal lines and the files they name.
pub fn entry(message: &Message, render: &Render) -> symphony_api::TaskEntry {
    let blocks = if message.kind == "system" {
        Vec::new()
    } else if message.sender == "owner" {
        if crate::uploads::has_reference(&message.text) {
            typed_blocks(render.typed_owner(&message.text, task_doc()), render)
        } else {
            Vec::new()
        }
    } else {
        reply_blocks(render, &message.text)
    };
    symphony_api::TaskEntry {
        message: message.clone(),
        stamp: stamp(message.ts),
        blocks,
    }
}

/// Task text links and pictures resolve as on the board's own page.
fn task_doc() -> &'static str {
    docs_index::doc::WORKFLOW_TASKS.path()
}

fn reply_blocks(render: &Render, text: &str) -> Vec<symphony_api::Block> {
    typed_blocks(render.typed_reply(text, task_doc()), render)
}

fn typed_blocks(
    mut fragment: crate::render::Fragment,
    render: &Render,
) -> Vec<symphony_api::Block> {
    let path = task_doc();
    let mut media = std::mem::take(&mut fragment.media);
    crate::docview::prepare_fragment(&mut fragment.html, &mut media, render.root(), path, "");
    crate::blocks::parse(&fragment.html, &media, path)
}

fn session(s: &tasks::Session) -> Session {
    let r = &s.row;
    Session {
        handle: r.handle.clone(),
        kind: r.kind.clone(),
        parent: r.parent.clone(),
        worktree: r.worktree.clone(),
        branch: r.branch.clone(),
        model: r.model.clone(),
        host: r.host.clone(),
        affinity: r
            .affinity
            .split(',')
            .filter(|a| !a.is_empty())
            .map(str::to_string)
            .collect(),
        connected: r.connected,
        last_seen: r.last_seen,
        last_tool: r.last_tool,
        left_at: r.left_at,
        health: s.health.as_str().into(),
    }
}

pub fn message(m: &tasks::Message) -> Message {
    Message {
        id: m.id,
        slug: m.slug.clone(),
        sender: m.sender.clone(),
        recipient: m.recipient.clone(),
        kind: m.kind.clone(),
        text: m.text.clone(),
        ts: m.ts,
    }
}

// A board timestamp is a local minute, like the register's timestamps.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the board stores Unix seconds as f64, formatted to whole minutes"
)]
fn stamp(seconds: f64) -> String {
    clock::stamp_at(seconds as libc::time_t)
}

pub fn activity(store: &tasks::Store) -> Result<Vec<ActivityItem>, tasks::Error> {
    let said = store.to_owner(40)?;
    let slugs = said
        .iter()
        .map(|m| m.slug.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let threads = store.threads(&slugs)?;
    Ok(said
        .iter()
        .map(|m| ActivityItem {
            id: format!("task#{}", m.id),
            kind: "task message".into(),
            path: format!("task {}", m.slug),
            slug: m.slug.clone(),
            ts: stamp(m.ts),
            text: format!(
                "{}: {}",
                m.sender,
                m.text.chars().take(150).collect::<String>()
            ),
            href: format!("/task/{}", m.slug),
            answered: threads
                .get(&m.slug)
                .is_some_and(|thread| thread.iter().any(|o| o.sender == "owner" && o.id > m.id)),
        })
        .collect())
}

pub fn connect(root: &Path) -> Result<tasks::Store, String> {
    let mut env = tasks::cli::Env::of_process();
    env.cwd = root.to_path_buf();
    // A read-only web request is never a board session or a launcher.
    env.vars.remove("TASKS_SESSION");
    let client = tasks::cli::board_client(&env)?.waiting(std::time::Duration::from_secs(3));
    if let Some(text) = env.vars.get("TASKS_NOW") {
        let now = text
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .ok_or_else(|| format!("TASKS_NOW={text} is not a time in seconds"))?;
        tasks::Store::with(
            client,
            Box::new(move || now),
            Box::new(tasks::sys::pid_alive),
            &tasks::sys::hostname(),
        )
        .map_err(|e| e.to_string())
    } else {
        tasks::Store::open(client).map_err(|e| e.to_string())
    }
}

pub fn agents(store: &tasks::Store, root: &Path) -> Result<symphony_api::Agents, String> {
    let mut env = tasks::cli::Env::of_process();
    env.cwd = root.to_path_buf();
    agents_probed(store, &tasks::cli::probes::machine(&env), None)
}

pub fn agents_in(
    store: &tasks::Store,
    root: &Path,
    common: Option<&Path>,
    board: &tasks::Board,
) -> Result<symphony_api::Agents, String> {
    let mut env = tasks::cli::Env::of_process();
    env.cwd = root.to_path_buf();
    let probes = match common {
        Some(common) => tasks::cli::probes::machine_in(&env, common),
        None => tasks::cli::probes::machine(&env),
    };
    agents_probed(store, &probes, Some(board))
}

fn agents_probed(
    store: &tasks::Store,
    probes: &tasks::cli::agents::Probes<'_>,
    board: Option<&tasks::Board>,
) -> Result<symphony_api::Agents, String> {
    let view = match board {
        Some(board) => tasks::cli::agents::describe_board(store, probes, store.now(), false, board),
        None => tasks::cli::agents::describe(store, probes, store.now(), false),
    }
    .map_err(|e| e.to_string())?;
    serde_json::from_str(&tasks::pyjson::dumps_line(&view.json())).map_err(|e| e.to_string())
}

/// Set the board field `name` on the task `slug`: the first of `plugins` declaring a field of that
/// name (or `plugin.name`, naming the plugin) validates `value` before the board stores what it
/// returns. Returns the plugin's name and the stored value.
pub fn set_field(
    store: &tasks::Store,
    plugins: &crate::plugins::Plugins,
    slug: &str,
    name: &str,
    value: &serde_json::Value,
    actor: &str,
) -> Result<(String, symphony_api::FieldValue), String> {
    let (wanted_plugin, field_name) = name
        .split_once('.')
        .map_or((None, name), |(plugin, field)| (Some(plugin), field));
    let (plugin, field) = plugins
        .list()
        .iter()
        .filter(|plugin| wanted_plugin.is_none_or(|wanted| plugin.name() == wanted))
        .find_map(|plugin| {
            plugins
                .board_field(plugin.name(), field_name)
                .map(|field| (plugin.name().to_string(), field))
        })
        .ok_or_else(|| format!("no plugin of this project declares a board field {name:?}"))?;
    let given = match value {
        serde_json::Value::String(text) => symphony_api::FieldValue::Text(text.clone()),
        serde_json::Value::Number(number) => symphony_api::FieldValue::Number(
            number
                .as_f64()
                .ok_or_else(|| format!("{number} is not a number"))?,
        ),
        other => return Err(format!("a field value is a number or text, not {other}")),
    };
    let stored = field.validate(&given)?;
    let json = serde_json::to_value(&stored).map_err(|e| e.to_string())?;
    store
        .set_field(slug, &plugin, field_name, &json, actor)
        .map_err(|e| e.to_string())?;
    Ok((plugin, stored))
}

/// A task's board fields as the wire carries them: a value that is neither a number nor text (a
/// write the board never makes) is left out.
fn fields(
    stored: &BTreeMap<String, BTreeMap<String, serde_json::Value>>,
) -> BTreeMap<String, BTreeMap<String, symphony_api::FieldValue>> {
    stored
        .iter()
        .map(|(plugin, fields)| {
            let values = fields
                .iter()
                .filter_map(|(name, value)| {
                    serde_json::from_value(value.clone())
                        .ok()
                        .map(|value| (name.clone(), value))
                })
                .collect();
            (plugin.clone(), values)
        })
        .collect()
}

#[cfg(test)]
#[path = "taskview/page_tests.rs"]
mod page_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_tree::Tree;

    #[test]
    fn a_brief_within_the_limit_is_its_whole_reply() {
        let tree = Tree::new();
        let render = Render::new(&tree.root);
        for body in [
            "One paragraph.",
            "# A heading\n\nA paragraph, and:\n\n- a list\n",
        ] {
            assert_eq!(
                brief_part(&render, body, 0, BRIEF_LIMIT),
                (reply_blocks(&render, body), body.len())
            );
        }
    }
}
