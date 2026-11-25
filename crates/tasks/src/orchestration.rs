//! One orchestrator per task: whose a task is, which tasks still hold more than one, and who the
//! board gives a task nobody orchestrates.
//!
//! A task is its orchestrator's when that session holds the role on it, or on the nearest task
//! above it that has one, and is live, stale or of unknown liveness ([`crate::Health::holds`]).
//! The board gives the role wherever a task would be left without ([`Pool`], [`unowned_tops`],
//! [`orphaned`]; docs/workflow/TASKS.md, "Every open task has an orchestrator"), to an orchestrator holding
//! fewer open tasks than the limit (`[limits] orchestrator_tasks`, [`counted`], [`at_limit`]) but
//! for a red. A task with none on it or above it is unowned for the moment before the board gives
//! it, or while no orchestrator that stays is under the limit, when it waits for the first to come
//! or to go under it ([`WAITING`]): a well state, which every orchestrator hears and any under the
//! limit takes with `role`. An orchestrator hears its own tasks and the unowned ones only
//! ([`Owners::reaches`]).

use crate::health::{FINISHED, Health, LIVE_SECONDS};
use crate::maps::Collect as _;
use crate::rows::SessionRow;
use crate::{Board, Task};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// The board's tasks by slug, read for whose each one is.
pub struct Owners<'t> {
    tasks: HashMap<&'t str, &'t Task>,
}

impl<'t> Owners<'t> {
    pub fn new(tasks: &'t [Task]) -> Owners<'t> {
        Owners {
            tasks: tasks.iter().map(|t| (t.slug.as_str(), t)).collect(),
        }
    }

    /// The holding orchestrators of the nearest task at or above `slug` that has any, nearest
    /// first; empty when the task is unclaimed or the board does not hold it. More than one only
    /// where a task still holds two ([`doubled`]).
    pub fn of(&self, slug: &str) -> Vec<&'t str> {
        let mut seen = HashSet::new();
        let mut at = self.tasks.get(slug).copied();
        while let Some(task) = at.filter(|t| seen.insert(t.slug.as_str())) {
            let held: Vec<&'t str> = task
                .roles
                .iter()
                .filter(|r| r.role == "orchestrator" && r.health.holds())
                .map(|r| r.handle.as_str())
                .collect();
            if !held.is_empty() {
                return held;
            }
            at = task
                .parent
                .as_deref()
                .and_then(|p| self.tasks.get(p).copied());
        }
        Vec::new()
    }

    /// Whether orchestrator `handle` hears `slug`: the task is its own, or nobody's.
    pub fn reaches(&self, slug: &str, handle: &str) -> bool {
        let owners = self.of(slug);
        owners.is_empty() || owners.contains(&handle)
    }

    /// Whether `slug` is some other orchestrator's, and not `handle`'s too.
    pub fn elsewhere(&self, slug: &str, handle: &str) -> bool {
        !self.reaches(slug, handle)
    }
}

/// The tasks somebody answers for ([`owed`]) that nobody orchestrates ([`Owners::of`] empty), until
/// the board gives them ([`unowned_tops`]): every orchestrator follows them meanwhile, and the
/// owner's word on one reaches every orchestrator (`Store::inbox`). The board's health counts them
/// as a defect (`crate::board_health::Report::unowned`).
pub fn unowned<'t>(owners: &Owners, tasks: &'t [Task]) -> impl Iterator<Item = &'t Task> {
    (tasks.iter())
        .filter(|t| owed(t))
        .filter(|t| owners.of(&t.slug).is_empty())
}

/// Whether session `child` reports to `handle` when it goes stale or lost: the orchestrator of a
/// task it holds claimable work on, or, for work nobody orchestrates and a session holding none,
/// its parent. `work`: the tasks it holds claimable work on ([`claimable_work`]).
pub fn reports_to(owners: &Owners, work: &[&Task], parent: Option<&str>, handle: &str) -> bool {
    if work.is_empty() {
        return parent == Some(handle);
    }
    work.iter()
        .any(|task| hears(owners, &task.slug, parent, handle))
}

/// Whether `handle` hears of a quiet or waiting session's work on `slug`: that task's orchestrator,
/// or, for a task nobody orchestrates, the session's `parent`.
pub fn hears(owners: &Owners, slug: &str, parent: Option<&str>, handle: &str) -> bool {
    let owners = owners.of(slug);
    if owners.is_empty() {
        parent == Some(handle)
    } else {
        owners.contains(&handle)
    }
}

/// Whether `handle` is told when `session` goes quiet or waits in its pane: its parent is; an
/// orchestrator ([`claimable_work`] given as `work`) only by [`reports_to`].
pub fn told_of(
    owners: &Owners,
    work: Option<&HashMap<&str, Vec<&Task>>>,
    session: &SessionRow,
    handle: &str,
) -> bool {
    let parent = session.parent.as_deref();
    work.map_or(parent == Some(handle), |work| {
        let held = work
            .get(session.handle.as_str())
            .map_or(&[][..], Vec::as_slice);
        reports_to(owners, held, parent, handle)
    })
}

/// Of `held`, quiet session `child`'s claimable work, what `handle` is told of: an orchestrator
/// hears its own tasks', and those nobody orchestrates when it is `child`'s parent; any other
/// session, all of it, as a parent always has.
pub fn told(board: &Board, child: &str, handle: &str, held: Vec<String>) -> Vec<String> {
    let row = |h: &str| board.sessions.iter().find(|s| s.row.handle == h);
    if row(handle).is_none_or(|s| s.row.kind != "orchestrator") {
        return held;
    }
    let parent = row(child).and_then(|s| s.row.parent.as_deref());
    let owners = Owners::new(&board.tasks);
    held.into_iter()
        .filter(|slug| hears(&owners, slug, parent, handle))
        .collect()
}

/// The status of each unfinished task on the board, by slug: the map an orchestrator's callback
/// state keeps, in the meaning dispatchers built before one orchestrator per task read it.
pub fn open_statuses(board: &Board) -> BTreeMap<String, String> {
    (board.tasks.iter())
        .filter(|t| !crate::health::FINISHED.contains(&t.status.as_str()))
        .map(|t| (t.slug.clone(), t.status.clone()))
        .collect_map()
}

/// Of `statuses`, those of the tasks orchestrator `handle` hears: what schedules its PULL digest,
/// so another orchestrator's tasks moving schedule nothing for it.
pub fn heard_statuses<'statuses>(
    owners: &Owners,
    statuses: &'statuses BTreeMap<String, String>,
    handle: &str,
) -> BTreeMap<&'statuses str, &'statuses str> {
    let mut heard = BTreeMap::new();
    heard.extend(
        (statuses.iter())
            .filter(|(slug, _)| owners.reaches(slug, handle))
            .map(|(slug, status)| (slug.as_str(), status.as_str())),
    );
    heard
}

/// Each session's claimable work ([`crate::health::claimable`]), by handle.
pub fn claimable_work(board: &Board) -> HashMap<&str, Vec<&Task>> {
    let mut work: HashMap<&str, Vec<&Task>> = HashMap::new();
    for t in &board.tasks {
        for r in &t.roles {
            if crate::health::claimable(&r.role, &t.status) {
                work.entry(r.handle.as_str()).or_default().push(t);
            }
        }
    }
    work
}

/// The tasks holding two or more orchestrator roles, whatever their holders' health: what the
/// orchestrators settle by hand (`just tasks handover`, or `just tasks role` over a lost or left one).
pub fn doubled(tasks: &[Task]) -> impl Iterator<Item = &Task> {
    tasks
        .iter()
        .filter(|t| t.roles.iter().filter(|r| r.role == "orchestrator").count() > 1)
}

/// The line `just tasks handover` writes on the thread.
pub fn handed_line(from: &str, to: &str) -> String {
    format!("{from} handed orchestrator over to {to}; address {to} or @orchestrator")
}

/// The line taking the role over a lost or left holder writes on the thread; `from` are the
/// holders it dropped, each with its health.
pub fn took_line(to: &str, from: &[(String, &str)]) -> String {
    let from: Vec<String> = from
        .iter()
        .map(|(handle, health)| format!("{handle} ({health})"))
        .collect();
    format!("{to} took orchestrator over from {}", from.join(", "))
}

/// Whether a move of the orchestrator role from `from` to `to` is said to `handle`: each session it
/// left, and the one it reached.
pub fn moved_for(from: &[&str], to: &str, handle: &str) -> bool {
    to == handle || from.contains(&handle)
}

/// A move of the orchestrator role, read back from its thread line: the handles it left, and the
/// one it reached. A hand-over, a take-over, or the board's give ([`given`]).
pub fn moved(text: &str) -> Option<(Vec<&str>, &str)> {
    if let Some((from, rest)) = text.split_once(" handed orchestrator over to ") {
        let to = rest.split_once(';').map_or(rest, |(to, _)| to);
        return Some((vec![from], to));
    }
    if let Some((to, from)) = text.split_once(" took orchestrator over from ") {
        return Some((holders(from), to));
    }
    given(text)
}

/// The handles of a line's `amber (lost), teal (left)`, each without its health.
fn holders(named: &str) -> Vec<&str> {
    (named.split(", "))
        .map(|one| one.split_once(" (").map_or(one, |(h, _)| h))
        .collect()
}

/// What the board's give says after the session it gave the role to ([`given_line`]).
pub const GIVEN: &str = " was given orchestrator";

/// The line the board writes on a task's thread as it gives the orchestrator role to `to`: from
/// `from`, the holders it dropped, each with its health (`B was given orchestrator from A (left)`);
/// holding none, with `why` (`B was given orchestrator (filed by w)`).
pub fn given_line(to: &str, from: &[(String, Health)], why: &str) -> String {
    if from.is_empty() {
        return format!("{to}{GIVEN} ({why})");
    }
    let from: Vec<String> = (from.iter())
        .map(|(handle, health)| format!("{handle} ({})", health.as_str()))
        .collect();
    format!("{to}{GIVEN} from {}", from.join(", "))
}

/// A give of the board's, read back from its line ([`given_line`]): the holders it dropped, and the
/// session it gave the role to.
pub fn given(text: &str) -> Option<(Vec<&str>, &str)> {
    let (to, rest) = text.split_once(GIVEN)?;
    if to.is_empty() || to.contains(' ') {
        return None;
    }
    match rest.strip_prefix(" from ") {
        Some(from) => Some((holders(from), to)),
        None => rest.starts_with(" (").then(|| (Vec::new(), to)),
    }
}

/// How many slugs a GIVEN line names before it counts the rest.
const NAMED: usize = 10;

/// The one line a session hears for the tasks `slugs` the board gave it in one pass or between two
/// looks, naming the first few (`GIVEN 2 open tasks are yours to orchestrate: tools/a, tools/b`);
/// none for none. Each thread says from whom and why ([`given_line`]).
pub fn given_event(slugs: &[&str]) -> Option<String> {
    let count = slugs.len();
    if count == 0 {
        return None;
    }
    let tasks = if count == 1 {
        "open task is"
    } else {
        "open tasks are"
    };
    let mut named = slugs[..count.min(NAMED)].join(", ");
    if count > NAMED {
        named.push_str(&format!(", and {} more", count - NAMED));
    }
    Some(format!(
        "GIVEN {count} {tasks} yours to orchestrate: {named}"
    ))
}

/// The board field marking a task waiting for an orchestrator, set when the board found none live
/// to give it to: when it began waiting. The next orchestrator to register or beat is given it.
pub const WAITING: &str = "waiting";

/// When a task's `fields` say it began waiting for an orchestrator ([`WAITING`]).
pub fn waiting(fields: &BTreeMap<String, BTreeMap<String, Value>>) -> Option<f64> {
    (fields.get(crate::red::FIELDS))
        .and_then(|board| board.get(WAITING))
        .and_then(Value::as_f64)
}

/// The board field marking a task its reviewer approved and no landing has closed: when the approve
/// was given. The approve writes it with the task's move to done, and the landing's line, or a move
/// back to review, lifts it (`Store::verdict`, `Store::landed`, `Store::move_to`), so a reader of
/// the board tells such a task from a landed one without its thread.
pub const UNLANDED: &str = "unlanded";

/// When `task` was approved, while it is done and no landing has closed it ([`UNLANDED`]): work
/// still somebody's to land, though it reads done.
pub fn unlanded(task: &Task) -> Option<f64> {
    marked_unlanded(&task.status, &task.fields)
}

/// [`unlanded`], of a task in `status` whose fields are `fields`.
pub fn marked_unlanded(
    status: &str,
    fields: &BTreeMap<String, BTreeMap<String, Value>>,
) -> Option<f64> {
    (fields.get(crate::red::FIELDS))
        .and_then(|board| board.get(UNLANDED))
        .and_then(Value::as_f64)
        .filter(|_| status == "done")
}

/// Whether somebody answers for `task`: it is unfinished, or approved and not landed
/// ([`unlanded`]), whose land is still to make. What the board keeps an orchestrator on; the limit
/// on the open tasks an orchestrator holds counts the unfinished alone ([`counted`]).
pub fn owed(task: &Task) -> bool {
    unfinished(task) || unlanded(task).is_some()
}

/// The live orchestrators of `board` past their first live line, but `except`: those that may show
/// they stay ([`Pool::of`]), whose listening the board read does not hold (a channel, a `tasks
/// wait`) and a store reads for them alone.
pub fn candidates(board: &Board, except: &[&str], now: f64) -> Vec<String> {
    (board.sessions.iter())
        .filter(|s| s.row.kind == "orchestrator" && s.health == Health::Live)
        .filter(|s| now - s.row.connected >= LIVE_SECONDS)
        .filter(|s| !except.contains(&s.row.handle.as_str()))
        .map(|s| s.row.handle.clone())
        .collect()
}

/// Whether `task` takes a place under the limit on the open tasks an orchestrator holds the role on
/// (`[limits] orchestrator_tasks`, [`crate::limits`]): open, done and cancelled ones being
/// finished, and no red ([`crate::red`]), which is never held back for want of an orchestrator, as
/// red work never waits for a slot. A task in review or blocked counts: the limit bounds what one
/// orchestrator answers for, not what is being built ([`crate::slots`]).
pub fn counted(task: &Task) -> bool {
    unfinished(task) && !task.red()
}

/// How many tasks of `tasks` `handle` holds the orchestrator role on itself that take a place under
/// the limit ([`counted`]); the tasks under them, its by the tree, take none.
pub fn orchestrated(tasks: &[Task], handle: &str) -> usize {
    (tasks.iter())
        .filter(|t| counted(t))
        .filter(|t| (t.roles.iter()).any(|r| r.role == "orchestrator" && r.handle == handle))
        .count()
}

/// What the limit says of `handle` orchestrating `held` open tasks against `limit`: none while it
/// may gain another, else why it gains none, which a refusal and the board's give say alike.
pub fn at_limit(handle: &str, held: usize, limit: usize) -> Option<String> {
    (held >= limit).then(|| {
        format!(
            "{handle} orchestrates {held} open tasks, the limit of {limit} an orchestrator holds \
             ([limits] orchestrator_tasks): it gains none until one of them is done or cancelled \
             or it lets one go"
        )
    })
}

/// The orchestrators the board gives a task to, each with the open tasks it holds the role on
/// ([`counted`]): rule 4 of docs/workflow/TASKS.md, "Every open task has an orchestrator", among those that
/// show they stay and hold fewer than the limit ([`Pool::within`]). Each pick is the one holding
/// fewest, ties by handle, and counts the task it gave, so a pass of several spreads them and fills
/// none past the limit.
#[derive(Debug, Default)]
pub struct Pool {
    load: BTreeMap<String, usize>,
    /// The open tasks at which an orchestrator is given no more but a red; none gives at any count.
    limit: Option<usize>,
}

impl Pool {
    /// The sessions of kind orchestrator on `board`, but `except`, that show they stay: live, live
    /// past their first live line (connected [`LIVE_SECONDS`] or more before `now`), and listening
    /// (a channel or a `tasks wait`, as `listening` names them, or a monitor's process) or already
    /// orchestrating an unfinished task. A session that only registered is none of them, so the
    /// board never hands work to one that may be gone in a minute.
    pub fn of(board: &Board, except: &[&str], listening: &BTreeSet<String>, now: f64) -> Pool {
        let mut held: BTreeMap<&str, usize> = BTreeMap::new();
        let mut orchestrating: BTreeSet<&str> = BTreeSet::new();
        for task in board.tasks.iter().filter(|t| unfinished(t)) {
            for role in task.roles.iter().filter(|r| r.role == "orchestrator") {
                orchestrating.insert(role.handle.as_str());
                if counted(task) {
                    *held.entry(role.handle.as_str()).or_default() += 1;
                }
            }
        }
        let candidates = candidates(board, except, now);
        let load = (board.sessions.iter())
            .filter(|s| candidates.contains(&s.row.handle))
            .filter_map(|s| {
                let handle = s.row.handle.as_str();
                let holds = held.get(handle).copied().unwrap_or(0);
                let monitor = s.row.pid.is_some_and(|pid| pid > 0);
                let listens = monitor || listening.contains(handle);
                (orchestrating.contains(handle) || listens).then(|| (handle.to_owned(), holds))
            })
            .collect_map();
        Pool { load, limit: None }
    }

    /// This pool, giving no orchestrator a task past `limit` open ones but a red
    /// ([`Pool::pick_for`]).
    pub fn within(mut self, limit: usize) -> Pool {
        self.limit = Some(limit);
        self
    }

    /// Whether `handle` is in it: an orchestrator that stays, not left out.
    pub fn holds(&self, handle: &str) -> bool {
        self.load.contains_key(handle)
    }

    /// Whether any orchestrator in it may be given a task that is no red: one under the limit.
    pub fn any(&self) -> bool {
        self.load.values().any(|held| self.under(*held))
    }

    /// Whether one in it may be given `task`: any that stays for a red, else one under the limit.
    pub fn takes(&self, task: &Task) -> bool {
        if task.red() {
            !self.load.is_empty()
        } else {
            self.any()
        }
    }

    fn under(&self, held: usize) -> bool {
        self.limit.is_none_or(|limit| held < limit)
    }

    /// The orchestrator under the limit holding fewest, ties by handle, its count raised by the one
    /// it is given; none when none that stays is under it.
    pub fn pick(&mut self) -> Option<String> {
        self.choose(false)
    }

    /// [`Pool::pick`] for `task`: a red goes to the one holding fewest whatever it holds, and is
    /// not counted, as no red waits for an orchestrator under the limit or takes a place under it.
    pub fn pick_for(&mut self, task: &Task) -> Option<String> {
        self.choose(task.red())
    }

    /// The one holding fewest, ties by handle: of all of them for a red, which it is given uncounted,
    /// else of those under the limit, its count raised by the task.
    fn choose(&mut self, red: bool) -> Option<String> {
        let (handle, _) = (self.load.iter())
            .filter(|(_, held)| red || self.under(**held))
            .min_by_key(|(handle, held)| (**held, *handle))?;
        let handle = handle.clone();
        if !red {
            self.give(&handle);
        }
        Some(handle)
    }

    /// Count a task given to `handle`.
    pub fn give(&mut self, handle: &str) {
        if let Some(held) = self.load.get_mut(handle) {
            *held += 1;
        }
    }
}

fn unfinished(task: &Task) -> bool {
    !FINISHED.contains(&task.status.as_str())
}

/// Of the tasks nobody orchestrates ([`unowned`]), the topmost of each tree they make:
/// the nearest such task to the top of each one's line of parents, which gives the role to
/// every unowned task below it ([`Owners::of`] walks up). In the board's order.
pub fn unowned_tops(tasks: &[Task]) -> Vec<&Task> {
    let owners = Owners::new(tasks);
    let unowned: HashSet<&str> = unowned(&owners, tasks).map(|t| t.slug.as_str()).collect();
    tops(tasks, &unowned)
}

/// `tops` with the tasks that wait on nothing first, each group in the order it came: a task whose
/// every dependency on `tasks` is done ([`crate::waits_on::Look::free`], as the digest's
/// `ready_unstaffed` reads it) can be worked on now, so where the limit on the open tasks an
/// orchestrator holds leaves places for only some, they go to those before a task that still waits
/// on another, which nobody can start whoever orchestrates it. A partition, not a sort by the
/// key: a stable sort links some 33 KB of code into each binary for a two-valued key.
pub fn first_free<'t>(tasks: &[Task], tops: Vec<&'t Task>) -> Vec<&'t Task> {
    let look = crate::waits_on::Look::new(tasks, &[]);
    let (mut free, waiting): (Vec<&Task>, Vec<&Task>) =
        tops.into_iter().partition(|top| look.free(top));
    free.extend(waiting);
    free
}

/// What a read of the whole board says of the board's give, before anything else is read: none to
/// make (`None`: every open task has an orchestrator, and no waiting mark is left to lift), or the
/// work there is, which `pool` then decides ([`give_due`]).
pub fn may_give(tasks: &[Task]) -> Option<Vec<&Task>> {
    let tops = unowned_tops(tasks);
    let marked = tasks.iter().any(|t| waiting(&t.fields).is_some());
    (!tops.is_empty() || marked).then_some(tops)
}

/// Whether the board's give has work on a board read whose topmost unowned tasks are `tops` and
/// whose orchestrators that stay are `pool`: a task one of them could take (under the limit, or a
/// red at any count), a task not yet marked waiting, or a waiting mark on a task that waits no
/// more. With none of those a pass would write nothing, so a reader that sees the same board again
/// reads no more for it: bounded, and each give made once.
pub fn give_due(tasks: &[Task], tops: &[&Task], pool: &Pool) -> bool {
    let waits: HashSet<&str> = tops.iter().map(|t| t.slug.as_str()).collect();
    tops.iter().any(|t| pool.takes(t))
        || tops.iter().any(|t| waiting(&t.fields).is_none())
        || (tasks.iter()).any(|t| waiting(&t.fields).is_some() && !waits.contains(t.slug.as_str()))
}

/// The tasks somebody answers for ([`owed`]) that are somebody's on `tasks` and would be nobody's
/// were each role `gone` says dropped: the topmost of each tree they make, in the board's order.
/// What a leave gives ([`crate::Store::leave`]), those that wait on nothing first ([`first_free`]),
/// and what a release of the role is refused for.
pub fn orphaned(tasks: &[Task], gone: impl Fn(&Task, &crate::Role) -> bool) -> Vec<&Task> {
    tops(tasks, &orphans(tasks, gone))
}

/// Every task of `tasks` somebody answers for ([`owed`]) that is somebody's and would be nobody's
/// were each role `gone` says dropped, not only the top of each tree they make ([`orphaned`]).
pub fn orphans(tasks: &[Task], gone: impl Fn(&Task, &crate::Role) -> bool) -> HashSet<&str> {
    let before = Owners::new(tasks);
    let after: Vec<Task> = (tasks.iter())
        .map(|t| {
            let mut kept = t.clone();
            kept.roles.retain(|r| !gone(t, r));
            kept
        })
        .collect();
    let after_owners = Owners::new(&after);
    (tasks.iter())
        .filter(|t| owed(t) && after_owners.of(&t.slug).is_empty())
        .filter(|t| !before.of(&t.slug).is_empty())
        .map(|t| t.slug.as_str())
        .collect()
}

/// Of `among`, each one's topmost task in `among` up its line of parents, once each, in `tasks`'
/// order.
fn tops<'t>(tasks: &'t [Task], among: &HashSet<&'t str>) -> Vec<&'t Task> {
    let by_slug: HashMap<&str, &Task> = tasks.iter().map(|t| (t.slug.as_str(), t)).collect();
    let mut found: HashSet<&str> = HashSet::new();
    for slug in among {
        let mut top = *slug;
        let mut seen = HashSet::from([top]);
        let mut at = by_slug.get(top).and_then(|t| t.parent.as_deref());
        while let Some(parent) = at.filter(|p| seen.insert(p)) {
            if among.contains(parent) {
                top = parent;
            }
            at = by_slug.get(parent).and_then(|t| t.parent.as_deref());
        }
        found.insert(top);
    }
    (tasks.iter())
        .filter(|t| found.contains(t.slug.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Health, Role, Session};

    /// The clock the pool's tests read: every session connected at 0, long past its live line.
    const NOW: f64 = 1_000.0;

    /// A role `handle` holds as `role`, its holder of `health`.
    fn held(handle: &str, role: &str, health: Health) -> Role {
        Role {
            handle: handle.to_string(),
            role: role.to_string(),
            since: 0.0,
            expires: None,
            health,
        }
    }

    fn task(slug: &str, parent: Option<&str>, roles: Vec<Role>) -> Task {
        Task {
            branch: None,
            id: 0,
            slug: slug.into(),
            title: String::new(),
            body: String::new(),
            status: "in_progress".into(),
            priority: 2,
            parent: parent.map(str::to_string),
            created_by: String::new(),
            created: 0.0,
            updated: 0.0,
            links: Vec::new(),
            depends: Vec::new(),
            roles,
            instruction: false,
            fields: Default::default(),
        }
    }

    fn board(tasks: Vec<Task>) -> Board {
        Board {
            tasks,
            sessions: Vec::<Session>::new(),
            latest_message: 0,
            workflow_generation: None,
        }
    }

    #[test]
    fn a_task_is_its_nearest_holding_orchestrators_and_unclaimed_without_one() {
        let tasks = vec![
            task("a", None, vec![held("amber", "orchestrator", Health::Live)]),
            task("a/b", Some("a"), Vec::new()),
            task(
                "a/c",
                Some("a"),
                vec![held("cobalt", "orchestrator", Health::Stale)],
            ),
            task(
                "a/c/d",
                Some("a/c"),
                vec![held("w", "executor", Health::Live)],
            ),
            task(
                "gone",
                None,
                vec![held("ghost", "orchestrator", Health::Left)],
            ),
            task(
                "gone/x",
                Some("gone"),
                vec![held("lost", "orchestrator", Health::Lost)],
            ),
            task("free", None, Vec::new()),
        ];
        let owners = Owners::new(&tasks);
        let nobody: [&str; 0] = [];
        assert_eq!(owners.of("a/b"), ["amber"]);
        assert_eq!(owners.of("a/c/d"), ["cobalt"]);
        assert_eq!(owners.of("gone/x"), nobody);
        assert_eq!(owners.of("free"), nobody);
        assert_eq!(owners.of("not-on-the-board"), nobody);
        let reach = |slug: &str| ["amber", "cobalt"].map(|handle| owners.reaches(slug, handle));
        assert_eq!(reach("a/b"), [true, false]);
        assert_eq!(reach("free"), [true, true]);
        let elsewhere = ["amber", "cobalt"].map(|handle| owners.elsewhere("a/c/d", handle));
        assert_eq!(elsewhere, [true, false]);
    }

    #[test]
    fn the_unfinished_tasks_with_no_holding_orchestrator_on_or_above_them_are_unowned() {
        let mut done = task("free-done", None, Vec::new());
        done.status = "done".into();
        let mut cancelled = task("free-cancelled", None, Vec::new());
        cancelled.status = "cancelled".into();
        let mut ready = task("free-ready", None, Vec::new());
        ready.status = "ready".into();
        let tasks = vec![
            task("a", None, vec![held("amber", "orchestrator", Health::Live)]),
            task("a/b", Some("a"), Vec::new()),
            task(
                "gone",
                None,
                vec![held("ghost", "orchestrator", Health::Lost)],
            ),
            task(
                "gone/x",
                Some("gone"),
                vec![held("w", "executor", Health::Live)],
            ),
            task("orphan", Some("not-on-the-board"), Vec::new()),
            done,
            cancelled,
            ready,
        ];
        let owners = Owners::new(&tasks);
        let found: Vec<&str> = unowned(&owners, &tasks).map(|t| t.slug.as_str()).collect();
        assert_eq!(found, ["gone", "gone/x", "orphan", "free-ready"]);
        assert_eq!(unowned(&owners, &[]).count(), 0);
    }

    #[test]
    fn a_loop_in_the_tree_ends_the_walk() {
        let tasks = vec![
            task("x", Some("y"), Vec::new()),
            task("y", Some("x"), Vec::new()),
        ];
        assert_eq!(Owners::new(&tasks).of("x"), [""; 0]);
    }

    #[test]
    fn a_double_belongs_to_both_until_it_is_settled() {
        let roles = vec![
            held("amber", "orchestrator", Health::Live),
            held("cobalt", "orchestrator", Health::Live),
            held("teal", "orchestrator", Health::Left),
        ];
        let tasks = vec![task("a", None, roles)];
        let owners = Owners::new(&tasks);
        assert_eq!(owners.of("a"), ["amber", "cobalt"]);
        let found: Vec<&str> = doubled(&tasks).map(|t| t.slug.as_str()).collect();
        assert_eq!(found, ["a"]);
        let single = vec![task(
            "b",
            None,
            vec![held("amber", "orchestrator", Health::Left)],
        )];
        assert_eq!(doubled(&single).count(), 0);
    }

    #[test]
    fn a_quiet_session_reports_to_the_orchestrator_of_its_work_else_its_parent() {
        let mut ready = task("free", None, vec![held("w2", "executor", Health::Lost)]);
        ready.status = "ready".into();
        let mut reviewed = task("done-one", None, vec![held("w3", "executor", Health::Lost)]);
        reviewed.status = "review".into();
        let amber = vec![
            held("amber", "orchestrator", Health::Live),
            held("w", "executor", Health::Lost),
        ];
        let b = board(vec![task("a", None, amber), ready, reviewed]);
        let owners = Owners::new(&b.tasks);
        let work = claimable_work(&b);
        let of = |h: &str| work.get(h).cloned().unwrap_or_default();
        assert_eq!(of("w3").len(), 0, "work in review is nobody's to take");
        let told = |child: &str, parent: Option<&str>| {
            ["amber", "cobalt"].map(|handle| reports_to(&owners, &of(child), parent, handle))
        };
        // Launched by cobalt, working amber's task: amber hears it.
        assert_eq!(told("w", Some("cobalt")), [true, false]);
        // Work nobody orchestrates, or nothing claimable: the parent.
        assert_eq!(told("w2", Some("cobalt")), [false, true]);
        assert_eq!(told("w3", Some("cobalt")), [false, true]);
        assert_eq!(told("w3", None), [false, false]);
    }

    #[test]
    fn each_move_of_the_role_reads_back_from_its_line() {
        let handed = handed_line("amber", "cobalt");
        assert_eq!(
            handed,
            "amber handed orchestrator over to cobalt; address cobalt or @orchestrator"
        );
        assert_eq!(moved(&handed), Some((vec!["amber"], "cobalt")));
        let took = took_line(
            "cobalt",
            &[("amber".into(), "lost"), ("teal".into(), "left")],
        );
        assert_eq!(
            took,
            "cobalt took orchestrator over from amber (lost), teal (left)"
        );
        assert_eq!(moved(&took), Some((vec!["amber", "teal"], "cobalt")));
        assert_eq!(moved("amber joined as orchestrator"), None);
        let from = [
            ("amber".into(), Health::Left),
            ("teal".into(), Health::Lost),
        ];
        let gave = given_line("cobalt", &from, "unused");
        assert_eq!(
            gave,
            "cobalt was given orchestrator from amber (left), teal (lost)"
        );
        assert_eq!(moved(&gave), Some((vec!["amber", "teal"], "cobalt")));
        let filed = given_line("cobalt", &[], "filed by w");
        assert_eq!(filed, "cobalt was given orchestrator (filed by w)");
        assert_eq!(moved(&filed), Some((vec![], "cobalt")));
        assert_eq!(given(&filed), Some((vec![], "cobalt")));
        assert_eq!(given(&took), None, "a take-over is no give");
    }

    #[test]
    fn a_line_only_looking_like_a_give_is_none() {
        for text in [
            " was given orchestrator (no one)",
            "the owner said amber was given orchestrator (twice)",
            "amber was given orchestrator",
            "amber was given orchestrator, then more",
        ] {
            assert_eq!(given(text), None, "{text}");
        }
    }

    #[test]
    fn the_given_line_counts_every_task_and_names_the_first_ten() {
        assert_eq!(given_event(&[]), None);
        assert_eq!(
            given_event(&["sim/a"]).as_deref(),
            Some("GIVEN 1 open task is yours to orchestrate: sim/a")
        );
        assert_eq!(
            given_event(&["sim/a", "sim/b"]).as_deref(),
            Some("GIVEN 2 open tasks are yours to orchestrate: sim/a, sim/b")
        );
        let slugs: Vec<String> = (1..=12).map(|n| format!("t/{n}")).collect();
        let slugs: Vec<&str> = slugs.iter().map(String::as_str).collect();
        assert_eq!(
            given_event(&slugs[..10]).as_deref(),
            Some(
                "GIVEN 10 open tasks are yours to orchestrate: \
                 t/1, t/2, t/3, t/4, t/5, t/6, t/7, t/8, t/9, t/10"
            )
        );
        assert_eq!(
            given_event(&slugs).as_deref(),
            Some(
                "GIVEN 12 open tasks are yours to orchestrate: \
                 t/1, t/2, t/3, t/4, t/5, t/6, t/7, t/8, t/9, t/10, and 2 more"
            )
        );
    }

    /// A session of `kind`, of `health`.
    fn session(handle: &str, kind: &str, health: Health) -> Session {
        Session {
            row: SessionRow {
                handle: handle.into(),
                kind: kind.into(),
                ..Default::default()
            },
            health,
        }
    }

    #[test]
    fn the_pool_gives_to_the_live_orchestrator_holding_fewest_ties_by_handle_spread() {
        let mut done = task(
            "old",
            None,
            vec![held("amber", "orchestrator", Health::Live)],
        );
        done.status = "done".into();
        let tasks = vec![
            task("a", None, vec![held("amber", "orchestrator", Health::Live)]),
            task("b", None, vec![held("amber", "orchestrator", Health::Live)]),
            task(
                "c",
                None,
                vec![held("cobalt", "orchestrator", Health::Live)],
            ),
            done,
        ];
        let mut b = board(tasks);
        let mut monitored = session("hazel", "orchestrator", Health::Live);
        monitored.row.pid = Some(4242);
        let mut fresh = session("iris", "orchestrator", Health::Live);
        fresh.row.connected = NOW - LIVE_SECONDS + 1.0;
        b.sessions = vec![
            session("amber", "orchestrator", Health::Live),
            session("cobalt", "orchestrator", Health::Live),
            session("dune", "orchestrator", Health::Live),
            session("ebony", "orchestrator", Health::Stale),
            session("fern", "worker", Health::Live),
            session("gale", "orchestrator", Health::Left),
            monitored,
            fresh,
            session("jet", "orchestrator", Health::Live),
        ];
        // dune listens (a channel or a wait) and holds none, hazel has a monitor, cobalt holds one,
        // amber two (its done task counts for nothing). iris registered under a live line ago and
        // jet only registered: neither shows it stays.
        let listening = BTreeSet::from(["dune".to_string(), "iris".to_string()]);
        let mut pool = Pool::of(&b, &[], &listening, NOW);
        let picks: Vec<String> = (0..6).filter_map(|_| pool.pick()).collect();
        assert_eq!(picks, ["dune", "hazel", "cobalt", "dune", "hazel", "amber"]);
        for out in ["ebony", "fern", "gale", "iris", "jet"] {
            assert!(!pool.holds(out), "{out}");
        }
        let mut without = Pool::of(&b, &["dune", "cobalt", "hazel"], &listening, NOW);
        assert_eq!(without.pick().as_deref(), Some("amber"));
        assert!(!without.holds("jet"));
        let mut none = Pool::of(&board(Vec::new()), &[], &listening, NOW);
        assert_eq!(none.pick(), None);
        assert!(!none.any());
        assert_eq!(
            candidates(&b, &["amber"], NOW),
            ["cobalt", "dune", "hazel", "jet"]
        );
    }

    /// `t`, marked red.
    fn red(mut t: Task) -> Task {
        let board = t.fields.entry(crate::red::FIELDS.into()).or_default();
        board.insert(crate::red::MARK.into(), Value::from("main"));
        t
    }

    /// `t`, in `status`.
    fn at(mut t: Task, status: &str) -> Task {
        t.status = status.into();
        t
    }

    #[test]
    fn an_orchestrator_counts_the_unfinished_tasks_it_holds_itself_but_a_red() {
        let amber = || vec![held("amber", "orchestrator", Health::Live)];
        let tasks = vec![
            task("a", None, amber()),
            at(task("review", None, amber()), "review"),
            at(task("blocked", None, amber()), "blocked"),
            at(task("ready", None, amber()), "ready"),
            at(task("done", None, amber()), "done"),
            at(task("cancelled", None, amber()), "cancelled"),
            red(task("tests/main-red-1", None, amber())),
            task("a/part", Some("a"), Vec::new()),
            task(
                "executed",
                None,
                vec![held("amber", "executor", Health::Live)],
            ),
            task(
                "c",
                None,
                vec![held("cobalt", "orchestrator", Health::Live)],
            ),
        ];
        assert_eq!(orchestrated(&tasks, "amber"), 4);
        assert_eq!(orchestrated(&tasks, "cobalt"), 1);
        assert_eq!(orchestrated(&tasks, "nobody"), 0);
        assert!(!counted(&tasks[4]) && !counted(&tasks[5]) && !counted(&tasks[6]));
        assert!(counted(&tasks[1]) && counted(&tasks[2]));
        // A red finished is no more counted than one open.
        assert!(!counted(&at(red(task("r", None, amber())), "done")));
    }

    #[test]
    fn the_limit_says_nothing_under_it_and_names_the_setting_at_it_and_past_it() {
        assert_eq!(at_limit("amber", 0, 1), None);
        assert_eq!(at_limit("amber", 15, 16), None);
        let at = at_limit("amber", 16, 16).unwrap();
        assert_eq!(
            at,
            "amber orchestrates 16 open tasks, the limit of 16 an orchestrator holds ([limits] \
             orchestrator_tasks): it gains none until one of them is done or cancelled or it lets \
             one go"
        );
        assert!(
            at_limit("amber", 66, 16)
                .unwrap()
                .starts_with("amber orchestrates 66 ")
        );
    }

    #[test]
    fn a_pool_within_the_limit_fills_none_past_it_and_gives_a_red_to_any_uncounted() {
        let one =
            |slug: &str, who: &str| task(slug, None, vec![held(who, "orchestrator", Health::Live)]);
        let mut b = board(vec![
            one("a1", "amber"),
            one("a2", "amber"),
            one("c1", "cobalt"),
            // A red amber holds takes no place: amber is at 2, not 3.
            red(one("r1", "amber")),
        ]);
        b.sessions = vec![
            session("amber", "orchestrator", Health::Live),
            session("cobalt", "orchestrator", Health::Live),
        ];
        let listening = BTreeSet::new();
        let free = task("free", None, Vec::new());
        let alarm = red(task("tests/main-red-2", None, Vec::new()));
        // No limit set, as a pool opens: any count takes one.
        let mut open = Pool::of(&b, &[], &listening, NOW);
        assert_eq!(open.pick().as_deref(), Some("cobalt"));
        assert_eq!(open.pick().as_deref(), Some("amber"));
        assert_eq!(open.pick().as_deref(), Some("cobalt"));
        // Within 2: amber is full, cobalt has one place, then nobody is under it.
        let mut pool = Pool::of(&b, &[], &listening, NOW).within(2);
        assert!(pool.any() && pool.takes(&free) && pool.takes(&alarm));
        assert_eq!(pool.pick_for(&free).as_deref(), Some("cobalt"));
        assert!(!pool.any());
        assert!(!pool.takes(&free));
        assert_eq!(pool.pick(), None);
        assert_eq!(pool.pick_for(&free), None);
        // A red still goes, to the one holding fewest, ties by handle, and is not counted.
        assert!(pool.takes(&alarm));
        assert_eq!(pool.pick_for(&alarm).as_deref(), Some("amber"));
        assert_eq!(pool.pick_for(&alarm).as_deref(), Some("amber"));
        // An orchestrator holding only a red stays, and holds none toward the limit.
        let mut reds = board(vec![red(one("r1", "dune"))]);
        reds.sessions = vec![session("dune", "orchestrator", Health::Live)];
        let mut pool = Pool::of(&reds, &[], &listening, NOW).within(1);
        assert!(pool.holds("dune"));
        assert_eq!(pool.pick_for(&free).as_deref(), Some("dune"));
        assert!(!pool.any());
        // With nobody that stays, not even a red is taken.
        let nobody = Pool::of(&board(Vec::new()), &[], &listening, NOW).within(1);
        assert!(!nobody.takes(&alarm));
    }

    #[test]
    fn a_give_is_due_only_while_one_under_the_limit_could_take_a_task_or_a_red_waits() {
        let waiting_since = |mut t: Task| {
            let board = BTreeMap::from([(WAITING.to_string(), Value::from(NOW))]);
            t.fields.insert("_board".into(), board);
            t
        };
        let mut b = board(vec![
            task("a", None, vec![held("amber", "orchestrator", Health::Live)]),
            waiting_since(task("free", None, Vec::new())),
        ]);
        b.sessions = vec![session("amber", "orchestrator", Health::Live)];
        let listening = BTreeSet::new();
        let tops = may_give(&b.tasks).unwrap();
        // amber at its limit of 1 and the waiting task marked: a pass would write nothing.
        let full = Pool::of(&b, &[], &listening, NOW).within(1);
        assert!(!give_due(&b.tasks, &tops, &full));
        // Under a limit of 2 it takes it.
        let room = Pool::of(&b, &[], &listening, NOW).within(2);
        assert!(give_due(&b.tasks, &tops, &room));
        // A red waiting goes to amber full or not.
        b.tasks[1] = red(waiting_since(task("tests/main-red-3", None, Vec::new())));
        let tops = may_give(&b.tasks).unwrap();
        assert!(give_due(&b.tasks, &tops, &full));
    }

    #[test]
    fn a_give_is_due_while_one_that_stays_could_take_a_task_or_a_mark_is_out_of_date() {
        let waiting_since = |mut t: Task| {
            let board = BTreeMap::from([(WAITING.to_string(), Value::from(NOW))]);
            t.fields.insert("_board".into(), board);
            t
        };
        let owned = task("a", None, vec![held("amber", "orchestrator", Health::Live)]);
        let mut b = board(vec![owned.clone()]);
        b.sessions = vec![session("amber", "orchestrator", Health::Live)];
        let listening = BTreeSet::new();
        // Every open task somebody's and no mark: nothing to read for.
        assert!(may_give(&b.tasks).is_none());
        // One nobody's, not marked: due, to give it or mark it.
        b.tasks.push(task("free", None, Vec::new()));
        let tops = may_give(&b.tasks).unwrap();
        let pool = Pool::of(&b, &[], &listening, NOW);
        assert!(give_due(&b.tasks, &tops, &pool));
        // Marked waiting, and amber the only one that stays: due, to give it to amber.
        b.tasks[1] = waiting_since(task("free", None, Vec::new()));
        let tops = may_give(&b.tasks).unwrap();
        assert!(give_due(&b.tasks, &tops, &pool));
        // Marked, and nobody stays: a pass would write nothing, so nothing is due.
        let gone = Pool::of(&b, &["amber"], &listening, NOW);
        assert!(!give_due(&b.tasks, &tops, &gone));
        // A mark left on a task somebody holds now: due, to lift it.
        let mut held_now = waiting_since(owned);
        held_now.slug = "b".into();
        let b = board(vec![held_now]);
        let tops = may_give(&b.tasks).unwrap();
        assert!(tops.is_empty());
        assert!(give_due(&b.tasks, &tops, &gone));
    }

    #[test]
    fn the_tops_of_the_unowned_trees_are_their_nearest_unfinished_tasks_to_the_top() {
        let mut done = task("gone/done", Some("gone"), Vec::new());
        done.status = "done".into();
        let tasks = vec![
            task("a", None, vec![held("amber", "orchestrator", Health::Live)]),
            task("a/b", Some("a"), Vec::new()),
            task(
                "gone",
                None,
                vec![held("ghost", "orchestrator", Health::Lost)],
            ),
            task("gone/x", Some("gone"), Vec::new()),
            done,
            task("gone/done/y", Some("gone/done"), Vec::new()),
            task("free", None, Vec::new()),
            task("orphan", Some("not-on-the-board"), Vec::new()),
            task("loop-x", Some("loop-y"), Vec::new()),
            task("loop-y", Some("loop-x"), Vec::new()),
        ];
        let found: Vec<&str> = unowned_tops(&tasks)
            .iter()
            .map(|t| t.slug.as_str())
            .collect();
        assert_eq!(found, ["gone", "free", "orphan", "loop-x", "loop-y"]);
        let mut finished = task("finished", None, Vec::new());
        finished.status = "cancelled".into();
        let below = task("finished/open", Some("finished"), Vec::new());
        let found: Vec<String> = (unowned_tops(&[finished, below]).iter())
            .map(|t| t.slug.clone())
            .collect();
        assert_eq!(found, ["finished/open"], "a finished task is given nothing");
    }

    #[test]
    fn the_give_takes_first_the_tops_that_wait_on_nothing_each_group_in_its_order() {
        let on = |slug: &str, depends: &[&str]| Task {
            depends: depends.iter().map(|d| d.to_string()).collect(),
            ..task(slug, None, Vec::new())
        };
        let tasks = vec![
            at(task("landed", None, Vec::new()), "done"),
            at(task("dropped", None, Vec::new()), "cancelled"),
            task("open", None, Vec::new()),
            on("on-open", &["open"]),
            on("on-landed", &["landed"]),
            on("on-dropped", &["dropped"]),
            on("on-elsewhere", &["not-on-the-board"]),
            on("on-landed-and-open", &["landed", "open"]),
            task("on-none", None, Vec::new()),
        ];
        let slugs =
            |found: Vec<&Task>| -> Vec<String> { found.iter().map(|t| t.slug.clone()).collect() };
        let tops: Vec<&Task> = tasks.iter().skip(2).collect();
        // A cancelled dependency is never met, as no open one is; one the board does not hold
        // holds nothing up.
        assert_eq!(
            slugs(first_free(&tasks, tops)),
            [
                "open",
                "on-landed",
                "on-elsewhere",
                "on-none",
                "on-open",
                "on-dropped",
                "on-landed-and-open",
            ]
        );
        assert!(first_free(&tasks, Vec::new()).is_empty());
    }

    #[test]
    fn what_a_leave_orphans_is_the_top_of_the_work_only_it_held() {
        let tasks = vec![
            task("a", None, vec![held("amber", "orchestrator", Health::Live)]),
            task("a/b", Some("a"), Vec::new()),
            task(
                "a/c",
                Some("a"),
                vec![held("cobalt", "orchestrator", Health::Live)],
            ),
            task("a/c/d", Some("a/c"), Vec::new()),
            task(
                "e",
                None,
                vec![
                    held("amber", "orchestrator", Health::Live),
                    held("cobalt", "orchestrator", Health::Live),
                ],
            ),
            task("free", None, Vec::new()),
        ];
        let amber = |_: &Task, r: &crate::Role| r.handle == "amber";
        let found: Vec<&str> = orphaned(&tasks, amber)
            .iter()
            .map(|t| t.slug.as_str())
            .collect();
        // a/c is cobalt's, e is cobalt's too, and free was nobody's before.
        assert_eq!(found, ["a"]);
        let only_b = |t: &Task, r: &crate::Role| t.slug == "a/c" && r.handle == "cobalt";
        let found: Vec<&str> = (orphaned(&tasks, only_b).iter())
            .map(|t| t.slug.as_str())
            .collect();
        assert!(found.is_empty(), "a/c is amber's by the tree: {found:?}");
        let everyone = |_: &Task, r: &crate::Role| r.role == "orchestrator";
        let found: Vec<&str> = (orphaned(&tasks, everyone).iter())
            .map(|t| t.slug.as_str())
            .collect();
        assert_eq!(found, ["a", "e"]);
    }

    /// `t`, done and marked approved at `at` with no landing since.
    fn approved(mut t: Task, at: Value) -> Task {
        t.status = "done".into();
        let board = t.fields.entry(crate::red::FIELDS.into()).or_default();
        board.insert(UNLANDED.into(), at);
        t
    }

    #[test]
    fn an_approved_task_not_landed_is_done_with_the_boards_mark_and_a_time() {
        let plain = task("a", None, Vec::new());
        assert_eq!((unlanded(&plain), owed(&plain)), (None, true));
        let marked = approved(plain.clone(), Value::from(12.5));
        assert_eq!((unlanded(&marked), owed(&marked)), (Some(12.5), true));
        assert!(!counted(&marked), "it takes no place under the limit");
        // The mark says nothing off a done task, and nothing that is no time is a mark.
        for status in ["review", "in_progress", "cancelled"] {
            assert_eq!(unlanded(&at(marked.clone(), status)), None, "{status}");
        }
        assert!(!owed(&at(marked.clone(), "cancelled")));
        for no_time in [Value::Null, Value::from("yes"), Value::from(true)] {
            let odd = approved(plain.clone(), no_time.clone());
            assert_eq!((unlanded(&odd), owed(&odd)), (None, false), "{no_time}");
        }
        let landed = at(plain, "done");
        assert_eq!((unlanded(&landed), owed(&landed)), (None, false));
    }

    #[test]
    fn an_approved_task_not_landed_is_given_and_orphaned_as_open_work_is() {
        let ghost = || vec![held("ghost", "orchestrator", Health::Lost)];
        let amber = || vec![held("amber", "orchestrator", Health::Live)];
        let mut landed = task("landed", None, ghost());
        landed.status = "done".into();
        let tasks = vec![
            approved(task("lost", None, ghost()), Value::from(1.0)),
            approved(task("held", None, amber()), Value::from(1.0)),
            approved(task("under", Some("parent"), Vec::new()), Value::from(1.0)),
            task("parent", None, amber()),
            landed,
        ];
        let slugs =
            |found: Vec<&Task>| -> Vec<String> { found.iter().map(|t| t.slug.clone()).collect() };
        assert_eq!(slugs(unowned_tops(&tasks)), ["lost"]);
        assert!(may_give(&tasks).is_some());
        let leaves = |_: &Task, r: &crate::Role| r.handle == "amber";
        assert_eq!(slugs(orphaned(&tasks, leaves)), ["held", "parent"]);
        assert_eq!(
            orchestrated(&tasks, "amber"),
            1,
            "the open parent alone is counted"
        );
    }

    #[test]
    fn a_waiting_mark_is_the_boards_own_field_with_a_time() {
        let mut fields = BTreeMap::new();
        assert_eq!(waiting(&fields), None);
        fields.insert(
            crate::red::FIELDS.to_string(),
            BTreeMap::from([(WAITING.to_string(), serde_json::json!(12.5))]),
        );
        assert_eq!(waiting(&fields), Some(12.5));
        fields.insert(
            "plugin".into(),
            BTreeMap::from([(WAITING.to_string(), serde_json::json!(1.0))]),
        );
        fields.remove(crate::red::FIELDS);
        assert_eq!(waiting(&fields), None, "a plugin's field of the name");
    }
}
