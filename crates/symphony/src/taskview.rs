//! The task pages' read API, directly over the Rust board. Failures remain visible to the HTTP
//! layer: a missing task is a 404, an unreachable board a 503.

use std::collections::BTreeMap;
use std::path::Path;

use sha2::{Digest, Sha256};
use symphony_api::{
    ActivityItem, Message, Role, Session, Task, TaskAsks, TaskCounts, TaskVerdict, Tasks,
};

use crate::{clock, dashboard, render::Render};

mod connect;
pub(crate) use connect::served_journal;
pub use connect::{connect, connect_in, connect_running, read_failure, serve};
pub mod waiting;
pub use waiting::Stood;

pub fn snapshot(store: &symphony_board::Store) -> Result<(Tasks, i64), symphony_board::Error> {
    let board = store.board()?;
    let mut stood = Stood::default();
    stood.read(store, &board)?;
    Ok(snapshot_board(&board, &stood, &BTreeMap::new()))
}

/// Project one shared board read into the wire snapshot, preserving its message cursor: each open
/// task with what it waits on, since when `stood` read it took its status ([`waiting::fill`]), each
/// in the owner's sign-off `lane` waiting on the owner's decision ([`waiting::decided`]), and the
/// board's orchestrators and ready tasks waiting to start.
pub fn snapshot_board(
    board: &symphony_board::Board,
    stood: &Stood,
    lane: &BTreeMap<String, Vec<String>>,
) -> (Tasks, i64) {
    snapshot_board_with_busy(board, stood, lane, &[])
}

/// [`snapshot_board`] with the sessions observed running tests, just as PULL counts them present
/// even when their tool call's heartbeat has expired.
pub fn snapshot_board_with_busy(
    board: &symphony_board::Board,
    stood: &Stood,
    lane: &BTreeMap<String, Vec<String>>,
    busy: &[String],
) -> (Tasks, i64) {
    let look = symphony_board::waits_on::Look::new(&board.tasks, busy);
    let mut tasks: Vec<Task> = (board.tasks.iter())
        .map(|t| {
            let mut wire = task(t);
            waiting::fill(&mut wire, t, &look, stood);
            wire
        })
        .collect();
    waiting::decided(&mut tasks, lane);
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
            orchestrators: waiting::orchestrators(board),
            unstarted: look.unstarted(&board.tasks),
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
        in_progress: dashboard::count(symphony_api::tasks_in_progress(&board.tasks).count()),
        in_review: dashboard::count(symphony_api::tasks_in_review(&board.tasks).count()),
        agents_live: dashboard::count(board.sessions.iter().filter(|s| s.health == "live").count()),
    }
}

/// The people the board's questions and verdicts are for ([`symphony_api::TaskAsks`]): the owner
/// alone until Symphony has users. One added is a name in those maps, never a field.
pub const PEOPLE: [&str; 1] = ["owner"];

/// The slugs of `tasks` that are not done or cancelled: those whose questions can still wait.
pub fn open(tasks: &[Task]) -> Vec<String> {
    tasks
        .iter()
        .filter(|task| !task.is_finished())
        .map(|task| task.slug.clone())
        .collect()
}

/// The questions still waiting on each of [`PEOPLE`] on the tasks `open` names
/// ([`symphony_board::Store::questions_waiting`]), by person, a person with none at 0.
pub fn questions(
    store: &symphony_board::Store,
    open: &[String],
) -> Result<BTreeMap<String, u32>, symphony_board::Error> {
    let mut questions = BTreeMap::new();
    for person in PEOPLE {
        let waiting = store.questions_waiting(person, open)?;
        questions.insert(person.to_string(), dashboard::count(waiting.len()));
    }
    Ok(questions)
}

/// `message` on the wire, when it is a reviewer's verdict ([`symphony_board::reviewer_verdict`]).
pub fn verdict(message: &symphony_board::Message) -> Option<TaskVerdict> {
    Some(TaskVerdict {
        verdict: symphony_board::reviewer_verdict(message)?.to_string(),
        id: message.id,
        task: message.slug.clone(),
        reviewer: message.sender.clone(),
        at: message.ts,
    })
}

/// What the board asks of each of [`PEOPLE`]: the `questions` waiting on them, and the newest
/// `verdict`, news for each.
pub fn asks(questions: BTreeMap<String, u32>, verdict: Option<&TaskVerdict>) -> TaskAsks {
    TaskAsks {
        questions,
        verdicts: verdict
            .map(|verdict| {
                PEOPLE
                    .iter()
                    .map(|person| ((*person).to_string(), verdict.clone()))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// The tasks of the owner's sign-off lane (docs/workflow/TASKS.md): open, executed, and held by their owner
/// gate. The Symphony's is held while a card the task posted waits on the owner
/// ([`crate::gates::owner`]); a gate named `owner` that a plugin of the project at `root` answered
/// for the task as it is now replaces it ([`crate::gates::merged`]), read from what `kept` holds and
/// never asked ([`crate::gates::kept_theirs`]), so the lane starts no plugin's work. Only a known
/// gate kept for the task does: with nothing kept for it as it is now, an error kept, or the gate
/// unknown, the card rule decides, erring toward showing the owner what may wait on them. Each is
/// its slug and the cards it posted that wait on the owner: none where a plugin's gate holds it.
pub fn signoff(
    store: &symphony_board::Store,
    tasks: &[Task],
    pending: &symphony_api::Pending,
    root: &Path,
    kept: &crate::gates::Kept,
) -> Result<Vec<(String, Vec<String>)>, String> {
    let answerers = crate::gates::answerers(&crate::project::Project::of(root), root);
    let mut result = Vec::new();
    for task in tasks.iter().filter(|t| {
        !["done", "blocked", "cancelled"].contains(&t.status.as_str())
            && t.roles.iter().any(|r| r.role == "executor")
    }) {
        let cards = crate::gates::linked_cards(&task.links, pending);
        if cards.is_empty() && answerers.is_empty() {
            continue;
        }
        // The owner gate's rule (`gates::posted_cards`), the thread read only when the body names
        // none of the cards.
        let mut posted = crate::gates::posted_cards(&cards, std::iter::once(task.body.as_str()));
        if posted.is_empty() && !cards.is_empty() {
            let thread = store.thread(&task.slug).map_err(|e| e.to_string())?;
            posted = crate::gates::posted_cards(&cards, thread.iter().map(|m| m.text.as_str()));
        }
        // Kept beside what a plugin answers: whether the hold is still the Symphony's own.
        let own: Vec<symphony_api::Gate> = crate::gates::owner(&posted).into_iter().collect();
        let gates = if answerers.is_empty() {
            own.clone()
        } else {
            let branch = branch_of(store, &task.slug, &task.body).map_err(|e| e.to_string())?;
            let asked = crate::gates::Asked {
                slug: task.slug.clone(),
                status: task.status.clone(),
                tip: crate::gates::tip(root, &branch),
                branch,
            };
            let theirs = crate::gates::kept_theirs(kept, root, &answerers, &asked);
            crate::gates::merged(own.clone(), theirs)
        };
        if gates.iter().any(is_held) {
            // The cards, where the Symphony's own gate holds it and no plugin's replaced it.
            let ours = (gates.iter()).any(|g| is_held(g) && own.contains(g));
            let cards = (posted.iter().filter(|_| ours))
                .map(|card| card.slug.clone())
                .collect();
            result.push((task.slug.clone(), cards));
        }
    }
    Ok(result)
}

/// Whether `gate` is the owner's, holding.
fn is_held(gate: &symphony_api::Gate) -> bool {
    gate.name == symphony_api::gates::OWNER && gate.state == symphony_api::GateState::Held
}

fn role(r: &symphony_board::Role) -> Role {
    Role {
        handle: r.handle.clone(),
        role: r.role.clone(),
        since: r.since,
        expires: r.expires,
        health: r.health.as_str().into(),
    }
}

fn task(t: &symphony_board::Task) -> Task {
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
        waits_on: None,
        review_rounds: None,
        from: None,
        workflow: place(&t.fields),
    }
}

/// Where a custom workflow holds the task whose board fields are `stored`, as a page shows it: none
/// for a task the board's own writers keep, and none for a position this build cannot read (a
/// newer board's), which the page then draws as the board's own.
pub fn place(
    stored: &BTreeMap<String, BTreeMap<String, serde_json::Value>>,
) -> Option<symphony_api::workflow::TaskPlace> {
    let held = stored.get("_board")?.get("workflow")?;
    let position: symphony_board::workflow::TaskPosition =
        serde_json::from_value(held.clone()).ok()?;
    Some(symphony_api::workflow::TaskPlace {
        phase: position.phase,
        state: position.state,
        resume: position.resume,
        revision: position.revision,
        manual: position.manual.filter(|exit| exit.manual).map(|exit| {
            symphony_api::workflow::ManualRecovery {
                trigger: exit.trigger.as_str().into(),
                holder: exit.holder,
                role: exit.role,
            }
        }),
    })
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
    let (fragment, next) =
        render.typed_reply_part(body, &task_doc(render), from, limit, BRIEF_LIMIT);
    (typed_blocks(fragment, render), next)
}

/// Whether the subtask `slug`, with its instruction flag and status, is listed under its parent: on
/// the task page's Subtasks, in their count and in the board's Task groups
/// (`apps/symphony-server/src/api/task_board.rs`). Slices and errands are, and so is every open
/// subtask. A finished instruction to do or to review the parent itself (`PARENT/work`,
/// `PARENT/review`, the staffing pass's `PARENT/staff-review-N`) is not: the guidance filed those
/// before a task's executor and reviewer worked on the task itself, its verdicts stand on the
/// parent's thread, and the parent is the row. Open, it shows where its agent is at work.
pub fn listed_under_parent(slug: &str, instruction: bool, status: &str) -> bool {
    let name = slug.rsplit('/').next().unwrap_or(slug);
    let staffs_parent = name == "work"
        || name == "review"
        || name
            .strip_prefix("staff-review-")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    !(instruction && staffs_parent && ["done", "cancelled"].contains(&status))
}

/// The page of `record`, already read by the caller. Children are counted from the rows right under
/// it, those [`listed_under_parent`]: reading every descendant would load the whole board for a
/// number.
pub fn page(
    store: &symphony_board::Store,
    record: &symphony_board::Task,
    root: &Path,
) -> Result<symphony_api::TaskPage, String> {
    let (blocks, brief_next) = brief_part(&Render::new(root), &record.body, 0, BRIEF_LIMIT);
    let mut task = task(record);
    task.body = record.body.get(..brief_next).unwrap_or_default().to_owned();
    Ok(symphony_api::TaskPage {
        machine_work: symphony_api::MachineWork::default(),
        task,
        blocks,
        children: Vec::new(),
        children_count: store
            .children(&record.slug)
            .map_err(|e| e.to_string())?
            .iter()
            .filter(|child| listed_under_parent(&child.slug, child.instruction, &child.status))
            .count(),
        brief_omitted: record.body.len() - brief_next,
        brief_next,
        brief_revision: digest(record.body.as_bytes()),
        sessions: Default::default(),
        local: false,
        gates: None,
        attempts: store
            .attempt_count(&record.slug)
            .map_err(|e| e.to_string())?,
    })
}

/// What the model calls of the task `slug` took, its subtasks' with them, and their estimated
/// cost at the prices this build embeds: `tasks show --json`'s `tokens`, its rows with them, from
/// the same two reads, so the page's figures are that command's.
pub fn tokens(
    store: &symphony_board::Store,
    slug: &str,
) -> Result<symphony_api::TaskTokens, symphony_board::Error> {
    let children = store.children(slug)?;
    let slugs: Vec<&str> = std::iter::once(slug)
        .chain(children.iter().map(|child| child.slug.as_str()))
        .collect();
    let spent = store.spent_on(&slugs)?;
    let prices = symphony_board::usage::Prices::embedded();
    let tally = symphony_board::usage::Tally::at(&spent, prices);
    serde_json::from_value(tally.json(&spent, prices))
        .map_err(|e| symphony_board::Error::Db(format!("a task's tokens do not read: {e}")))
}

/// What each of `slugs` cost, its subtasks' with it, as its page's figure all told
/// ([`tokens`]), at the prices this build embeds: one read of the board however many it names
/// ([`symphony_board::Store::spent_under`]). One that took none is left out.
pub fn costs(
    store: &symphony_board::Store,
    slugs: &[String],
) -> Result<symphony_api::TaskCosts, symphony_board::Error> {
    let asked: Vec<&str> = slugs.iter().map(String::as_str).collect();
    let prices = symphony_board::usage::Prices::embedded();
    let costs = (store.spent_under(&asked)?.into_iter())
        .map(|(slug, spend)| symphony_api::TaskCost {
            dollars: spend.cost.json(&prices.read)["dollars"]
                .as_f64()
                .unwrap_or_default(),
            unpriced: !spend.cost.unpriced.is_empty(),
            slug,
        })
        .collect();
    Ok(symphony_api::TaskCosts {
        prices: prices.read.clone(),
        costs,
    })
}

/// The next part of the brief of `record` from byte `from`, for the owner who asks for what the
/// page left out.
pub fn brief(record: &symphony_board::Task, from: usize, root: &Path) -> symphony_api::TaskBrief {
    let (blocks, next) = brief_part(&Render::new(root), &record.body, from, BRIEF_CHUNK);
    symphony_api::TaskBrief {
        blocks,
        next,
        omitted: record.body.len() - next,
        revision: digest(record.body.as_bytes()),
    }
}

/// The first offer of `plugins`' board actions on `slug` that has a payload, whatever the task's
/// status: each action decides which work it offers on; with none, the owner's approve of the tip
/// while the task needs it (`gates::stamps::owner_offer`). Requested only when the owner reaches
/// the message/approval controls: preparing an offer must not delay the task's metadata and brief.
pub fn owner_offer(
    store: &symphony_board::Store,
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
    if let Some(offer) = crate::gates::stamps::owner_offer(store, &task, root, &branch) {
        return Ok(offer);
    }
    Ok(symphony_api::TaskOffer {
        branch,
        ..symphony_api::TaskOffer::default()
    })
}

/// A task slug can differ from its branch when an ancestor already names a branch. Review
/// requests name their target in their brief; ordinary work uses its recorded executor branch.
pub(crate) fn approval_branch(
    store: &symphony_board::Store,
    task: &symphony_board::Task,
) -> Result<String, symphony_board::Error> {
    branch_of(store, &task.slug, &task.body)
}

/// [`approval_branch`] of the task `slug`, whose brief is `body`.
fn branch_of(
    store: &symphony_board::Store,
    slug: &str,
    body: &str,
) -> Result<String, symphony_board::Error> {
    if let Some(branch) = body
        .trim_start()
        .strip_prefix("Review `")
        .and_then(|text| text.split_once("` before it lands"))
        .map(|(branch, _)| branch)
    {
        return Ok(branch.to_string());
    }
    if let Some(branch) = slug.strip_prefix("review/") {
        return Ok(branch.to_string());
    }
    Ok(store
        .task_sessions(slug)?
        .into_iter()
        .rev()
        .find(|s| s.role == "executor" && !s.branch.is_empty())
        .map_or_else(|| slug.to_string(), |s| s.branch))
}

pub fn history(
    store: &symphony_board::Store,
    slug: &str,
    before: Option<i64>,
    after: Option<i64>,
    root: &Path,
) -> Result<symphony_api::TaskHistory, symphony_board::Error> {
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
            typed_blocks(render.typed_owner(&message.text, &task_doc(render)), render)
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

/// Task text links and pictures resolve as on the board's guide (`[board] guide`).
fn task_doc(render: &Render) -> String {
    crate::project::Project::of(render.root())
        .board
        .guide()
        .to_string()
}

fn reply_blocks(render: &Render, text: &str) -> Vec<symphony_api::Block> {
    typed_blocks(render.typed_reply(text, &task_doc(render)), render)
}

fn typed_blocks(
    mut fragment: crate::render::Fragment,
    render: &Render,
) -> Vec<symphony_api::Block> {
    let path = &task_doc(render);
    let mut media = std::mem::take(&mut fragment.media);
    crate::docview::prepare_fragment(&mut fragment.html, &mut media, render.root(), path, "");
    crate::blocks::parse(&fragment.html, &media, path)
}

fn session(s: &symphony_board::Session) -> Session {
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

pub fn message(m: &symphony_board::Message) -> Message {
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

pub fn activity(store: &symphony_board::Store) -> Result<Vec<ActivityItem>, symphony_board::Error> {
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
            text: crate::dashboard::excerpt(&format!("{}: {}", m.sender, m.text)),
            href: format!("/task/{}", m.slug),
            answered: threads
                .get(&m.slug)
                .is_some_and(|thread| thread.iter().any(|o| o.sender == "owner" && o.id > m.id)),
        })
        .collect())
}

/// A quick-task outcome read must identify the owner's intended task before confirming it.
pub(crate) fn is_quick_task(task: &symphony_board::Task, receipt: &str) -> bool {
    task.created_by == "owner"
        && task
            .fields
            .get("_symphony")
            .and_then(|fields| fields.get("owner_save"))
            .and_then(serde_json::Value::as_str)
            == Some(receipt)
}

/// The agents view, its machine probes run without this process's session
/// ([`crate::children::board_env`]): hc's session names would scope the census to the starter's
/// core.
pub fn agents(store: &symphony_board::Store, root: &Path) -> Result<symphony_api::Agents, String> {
    let mut env = crate::children::board_env();
    env.cwd = root.to_path_buf();
    agents_probed(store, &symphony_board::cli::probes::machine(&env), None)
}

/// A clock tick supplies its single trusted native observation; HTTP consumers reuse this view.
pub fn agents_in_observed(
    store: &symphony_board::Store,
    root: &Path,
    common: Option<&Path>,
    board: &symphony_board::Board,
    evidence: symphony_board::cli::machine_work::Evidence,
) -> Result<symphony_api::Agents, String> {
    let mut env = crate::children::board_env();
    env.cwd = root.to_path_buf();
    // The server views its agents every tick: the process listing is kept by its store a while.
    let probes = match common {
        Some(common) => symphony_board::cli::probes::machine_in(&env, common),
        None => symphony_board::cli::probes::machine(&env),
    }
    .kept_by(store);
    let view = symphony_board::cli::agents::describe_board_observed(
        store,
        &probes,
        store.now(),
        false,
        board,
        evidence,
    )
    .map_err(|error| error.to_string())?;
    let mut agents = wire_agents(&view)?;
    place_under(&mut agents.machine_work, &board.tasks);
    Ok(agents)
}

/// Says on each operation which tasks its task is under on the board, so a subtask's run is its
/// parent's card's work too ([`symphony_api::MachineWork::place_under`]).
fn place_under(work: &mut symphony_api::MachineWork, tasks: &[symphony_board::Task]) {
    let parents: std::collections::HashMap<&str, &str> = tasks
        .iter()
        .filter_map(|task| Some((task.slug.as_str(), task.parent.as_deref()?)))
        .collect();
    work.place_under(|slug| parents.get(slug).copied());
}

fn agents_probed(
    store: &symphony_board::Store,
    probes: &symphony_board::cli::agents::Probes<'_>,
    board: Option<&symphony_board::Board>,
) -> Result<symphony_api::Agents, String> {
    let view = match board {
        Some(board) => {
            symphony_board::cli::agents::describe_board(store, probes, store.now(), false, board)
        }
        None => symphony_board::cli::agents::describe(store, probes, store.now(), false),
    }
    .map_err(|e| e.to_string())?;
    wire_agents(&view)
}

/// The board's agents view on the Symphony's wire. The board's own JSON names the panes probe
/// `herdr`, which [`symphony_api::AgentProbes`] reads and sends on as `panes` and `herdr` both.
fn wire_agents(view: &symphony_board::cli::agents::View) -> Result<symphony_api::Agents, String> {
    serde_json::from_value(view.json()).map_err(|e| e.to_string())
}

/// Set the board field `name` on the task `slug`: the first of `plugins` declaring a field of that
/// name (or `plugin.name`, naming the plugin) validates `value` before the board stores what it
/// returns. Returns the plugin's name and the stored value.
pub fn set_field(
    store: &symphony_board::Store,
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
    let mut plugins = BTreeMap::new();
    for (plugin, fields) in stored {
        let mut values = BTreeMap::new();
        for (name, value) in fields {
            if let Ok(value) = serde_json::from_value(value.clone()) {
                values.insert(name.clone(), value);
            }
        }
        plugins.insert(plugin.clone(), values);
    }
    plugins
}

#[cfg(test)]
#[path = "taskview/page_tests.rs"]
pub(crate) mod page_tests;

#[cfg(test)]
#[path = "taskview/agents_tests.rs"]
mod agents_tests;

#[cfg(test)]
mod tests;
