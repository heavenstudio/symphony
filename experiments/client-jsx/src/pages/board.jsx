// The task board, `/tasks` (`board_live.rs`, `tasks.rs`): the counts first, then each column's
// first page once the column nears the screen, Load more after it, and the feed's task events
// applied as they come; an event that changes the totals reads the counts again 750 ms on, and
// the columns in view read their first page again. A read that was out while events came has
// them replayed over its answer.
//
// Left out (named in the comparison): the device cache and its checks, the ghosts' kept shapes,
// search and the Status and Finished filters (drawn, inert), the task groups tree (its button
// drawn, closed), the machine's build and test context and phase chips (the fixture's machine
// reports none, so the Dioxus page draws the unreported rail, as this one does).
import { memo, useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";
import { get, cursorQuery, sleep } from "../api.js";
import { keepLive } from "../feed.js";
import { mark, PageNotices } from "../frame.jsx";
import { Near } from "../near.jsx";
import { Button, Input, SelectTrigger } from "../controls.jsx";
import { href } from "../project.js";
import { byPriority, byRecent, mergeFields, OFFLINE, PIPELINE, statusWords, taskStage } from "../tasks.js";

/** The rows of a column's page, as the server pages it, and a waiting row's reserve. */
const PAGE = 30;
const ROW = 139;

const initial = {
  tasks: [],
  work: { machines: [] },
  counts: [],
  lifecycle: [],
  statuses: [],
  signoff: [],
  connected: true,
  ready: false,
  error: "",
  generation: 0,
};

function upsert(tasks, key, value, removed) {
  const at = tasks.findIndex((task) => task.slug === key);
  if (removed) return at < 0 ? tasks : tasks.filter((task) => task.slug !== key);
  if (!value) return tasks;
  if (at < 0) return [...tasks, value];
  const next = tasks.slice();
  next[at] = value;
  return next;
}

/** Whether a feed event changes the totals, so the counts and the columns in view read again. */
function changesTotals(value) {
  return (value.changes ?? []).some(
    ({ region, data }) =>
      region === "machine-work" ||
      region === "task-counts" ||
      region === "task-recent-counts" ||
      (region === "task-fields" && (data.fields.status != null || data.fields.parent != null)) ||
      (region === "tasks-status" && data.available === true),
  );
}

/** One feed event merged into the board. */
function merge(state, value, days) {
  let next = state;
  for (const change of value.changes ?? []) {
    const data = change.data;
    switch (change.region) {
      case "machine-work":
        next = { ...next, work: data };
        break;
      case "task":
      case "task-summary": {
        let removed = !!data.removed;
        const task = data.value;
        if (task && (task.status === "done" || task.status === "cancelled") && task.updated < Date.now() / 1000 - days * 86400) removed = true;
        next = { ...next, tasks: upsert(next.tasks, data.key, task, removed) };
        break;
      }
      case "task-fields": {
        const at = next.tasks.findIndex((task) => task.slug === data.key);
        if (at >= 0) {
          const tasks = next.tasks.slice();
          tasks[at] = mergeFields(tasks[at], data.fields);
          next = { ...next, tasks };
        }
        break;
      }
      case "task-signoff":
        next = { ...next, signoff: data };
        break;
      case "tasks-status":
        next = { ...next, error: data.available === false ? "The task board is not reachable." : data.slow ? "The task board is slow to answer." : "" };
        break;
      default:
    }
  }
  if (changesTotals(value)) next = { ...next, generation: next.generation + 1 };
  return next;
}

function reduce(state, action) {
  switch (action.type) {
    case "counts":
      return { ...state, counts: action.list.counts, lifecycle: action.list.lifecycle_counts, statuses: action.list.statuses, signoff: action.list.signoff, ready: true, error: "" };
    case "column": {
      const { status, more, list } = action;
      let tasks = more ? state.tasks : state.tasks.filter((task) => taskStage(state.work, task, state.connected) !== status);
      for (const task of list.tasks) tasks = upsert(tasks, task.slug, task, false);
      return { ...state, tasks, counts: list.counts, lifecycle: list.lifecycle_counts, signoff: list.signoff };
    }
    case "work":
      return { ...state, work: action.work };
    case "event":
      return merge(state, action.value, action.days);
    case "live":
      return action.live
        ? { ...state, connected: true }
        : { ...state, connected: false, generation: state.generation + 1, counts: state.lifecycle.length ? state.lifecycle : state.counts, error: action.error ?? state.error };
    case "error":
      return { ...state, error: action.error };
    default:
      return state;
  }
}

export default function TasksPage() {
  const [state, dispatch] = useReducer(reduce, initial);
  const [stale, setStale] = useState(false);
  const [refreshed, setRefreshed] = useState(0);
  const current = useRef(state);
  current.current = state;
  // Events applied while a read was out, replayed over its answer (`board_live::Journal`).
  const journal = useRef({ seq: 0, events: [], readers: 0 });
  const cursors = useRef(new Map());
  const requests = useRef(new Map());
  const days = 1;

  const reading = useCallback(async (path) => {
    const log = journal.current;
    const since = log.seq;
    log.readers++;
    try {
      const list = await get(path);
      return { list, since };
    } finally {
      log.readers--;
    }
  }, []);

  const replay = useCallback((since) => {
    const log = journal.current;
    for (const { seq, value } of log.events) {
      if (seq > since) dispatch({ type: "event", value, days });
    }
    if (log.readers === 0) log.events = [];
  }, []);

  const counts = useCallback(async () => {
    const activity = current.current.connected ? 1 : 0;
    const { list, since } = await reading(`/api/task-board?counts=1&q=&days=${days}&activity=${activity}&work=0`);
    dispatch({ type: "counts", list });
    replay(since);
  }, [reading, replay]);

  const column = useCallback(
    async (status, more) => {
      const cursor = cursors.current.get(status);
      if (more && !cursor) return;
      const version = (requests.current.get(status) ?? 0) + 1;
      requests.current.set(status, version);
      const generation = current.current.generation;
      await sleep(250);
      if (requests.current.get(status) !== version) return;
      const activity = current.current.connected ? 1 : 0;
      try {
        const { list, since } = await reading(`/api/task-board?q=&status=${status}&days=${days}${cursorQuery(more ? cursor : null)}&activity=${activity}&work=0`);
        if (requests.current.get(status) !== version || current.current.generation !== generation) return;
        if (list.next) cursors.current.set(status, list.next);
        else cursors.current.delete(status);
        dispatch({ type: "column", status, more, list });
        replay(since);
      } catch (error) {
        dispatch({ type: "error", error: error.message });
      }
    },
    [reading, replay],
  );

  useEffect(() => {
    let pending = false;
    return keepLive({
      feed: (version) => `/api/feed?since=${version}&document=`,
      read: async () => {
        cursors.current.clear();
        const work = get("/api/machine-work").then((work) => dispatch({ type: "work", work }));
        await Promise.all([counts(), work]);
        setRefreshed((n) => n + 1);
      },
      delta: (value) => {
        const log = journal.current;
        log.seq++;
        if (log.readers > 0) log.events.push({ seq: log.seq, value });
        dispatch({ type: "event", value, days });
        if (changesTotals(value) && !pending) {
          pending = true;
          (async () => {
            for (;;) {
              await sleep(750);
              const generation = current.current.generation;
              try {
                await counts();
              } catch {}
              if (generation === current.current.generation) {
                cursors.current.clear();
                setRefreshed((n) => n + 1);
                pending = false;
                break;
              }
            }
          })();
        }
      },
      state: (live, error) => {
        setStale(!live);
        dispatch({ type: "live", live, error });
        if (!live) setRefreshed((n) => n + 1);
      },
    });
  }, [counts]);

  return (
    <main data-ready="board" data-feed={stale ? "stale" : "live"} aria-busy={String(!state.ready)}>
      {!state.ready ? (
        <span className="status-text" role="status">
          Loading tasks…
        </span>
      ) : null}
      <div className="page-head">
        <h1>Board</h1>
        <a href={href("/agents")}>Who is connected</a>
      </div>
      {state.ready ? <TaskBoard state={state} connected={!stale} refreshed={refreshed} onLoad={column} /> : <GhostBoard />}
      <PageNotices reconnecting={stale} error={state.error} />
    </main>
  );
}

function GhostBoard() {
  return (
    <div className="kanban" aria-hidden="true">
      <div className="board-scroll">
        <div className="pipeline">
          {["concept", "design", "ready", "in_progress"].map((status) => (
            <section key={status} className="column" data-status={status}>
              <h2>{statusWords(status)}</h2>
            </section>
          ))}
        </div>
      </div>
    </div>
  );
}

function TaskBoard({ state, connected, refreshed, onLoad }) {
  const { tasks, work, counts, signoff } = state;
  const unreported = (work?.machines ?? []).length === 0;
  const statuses = useMemo(() => {
    let list = state.statuses.length ? state.statuses.slice() : counts.map(([status]) => status);
    if (unreported) list = list.filter((s) => s !== "build" && s !== "test");
    else for (const stage of ["build", "test"]) if (!list.includes(stage)) list.push(stage);
    return list;
  }, [state.statuses, counts, unreported]);
  // Each column's rows, by stage, in the page's order.
  const byStage = useMemo(() => {
    const columns = new Map();
    const sorted = tasks.filter((task) => !task.parent).sort(byPriority);
    for (const task of sorted) {
      const stage = signoff.includes(task.slug) ? "signoff" : taskStage(work, task, connected);
      if (!columns.has(stage)) columns.set(stage, []);
      columns.get(stage).push(task);
    }
    return columns;
  }, [tasks, work, connected, signoff]);
  const count = (status) => counts.find(([s]) => s === status)?.[1] ?? 0;
  const column = (status) => (
    <TaskColumn key={status} status={status} tasks={byStage.get(status) ?? EMPTY} count={count(status)} refreshed={refreshed} onLoad={onLoad} />
  );
  return (
    <>
      <div className="row filters">
        <label>
          Search tasks and messages <Input type="search" placeholder="Slug or words" defaultValue="" />
        </label>
        <label>
          Status <SelectTrigger label="Status" value="" options={[["", "All statuses"]]} />
        </label>
        <label>
          Finished tasks <SelectTrigger label="Finished tasks" value="1" options={[["1", "Last 24 hours"], ["7", "Last week"]]} />
        </label>
      </div>
      <div className="kanban">
        {signoff.length ? <TaskColumn status="signoff" tasks={byStage.get("signoff") ?? EMPTY} count={signoff.length} refreshed={refreshed} onLoad={onLoad} /> : null}
        <p id="board-scroll-hint" className="board-scroll-hint muted">
          Columns continue right → · Swipe, scroll, or focus the board and use arrow keys.
        </p>
        <div className="board-scroll" tabIndex={0} role="region" aria-label="Task pipeline columns" aria-describedby="board-scroll-hint">
          <div className="pipeline">
            {PIPELINE.map((stage) => (
              <Stage key={stage} stage={stage} unreported={unreported} connected={connected} statuses={statuses} column={column} />
            ))}
          </div>
        </div>
        <div className="offline">{statuses.filter((s) => OFFLINE.includes(s)).map(column)}</div>
      </div>
      <section className="task-groups" aria-labelledby="task-groups-title">
        <h2 id="task-groups-title">Task groups</h2>
        <Button aria-expanded="false" aria-controls="task-groups">
          Show task groups
        </Button>
      </section>
    </>
  );
}

const EMPTY = [];

function Stage({ stage, unreported, connected, statuses, column }) {
  return (
    <>
      {unreported && stage === "build" ? (
        <section className="column rail" data-status="build-test" aria-label="Build and test">
          <h2>build · test</h2>
          <p className="muted work-empty">{connected ? "No machine reports this project's builds or tests." : "Disconnected · build and test activity unknown"}</p>
          <a href={href("/agents#machine-context")}>Machine details</a>
        </section>
      ) : null}
      {statuses.includes(stage) ? column(stage) : null}
    </>
  );
}

const TaskColumn = memo(function TaskColumn({ status, tasks, count, refreshed, onLoad }) {
  const [visible, setVisible] = useState(false);
  const needsPage = count > 0 || tasks.length > 0;
  useEffect(() => {
    if (visible && needsPage) onLoad(status, false);
  }, [visible, needsPage, refreshed, status, onLoad]);
  const rows = useMemo(() => (status === "done" || status === "cancelled" ? tasks.slice().sort(byRecent) : tasks), [tasks, status]);
  const reading = count > 0 && tasks.length === 0;
  const reserved = Math.min(count, PAGE) * ROW;
  const enter = useCallback(() => setVisible(true), []);
  return (
    <section className={status === "signoff" ? "column signoff" : "column"} data-status={status} aria-busy={String(reading)}>
      <h2>
        {status === "signoff" ? "Waiting for your sign-off" : statusWords(status)} <span className="count">{count}</span>
      </h2>
      {count === 0 && (status === "build" || status === "test") ? <p className="muted work-empty">No tasks with fresh {status} activity.</p> : null}
      <Near id={`column-${status}`} height={reserved} onEnter={enter}>
        {reading ? <div style={{ minHeight: `${reserved}px` }} data-pending="true" /> : null}
        {rows.map((task) => (
          <Near key={task.slug} id={`row-${task.slug}`} height={ROW}>
            <TaskRow task={task} />
          </Near>
        ))}
        {tasks.length > 0 && count > tasks.length ? <Button onClick={() => onLoad(status, true)}>Load more tasks</Button> : null}
      </Near>
    </section>
  );
});

const TaskRow = memo(function TaskRow({ task }) {
  useEffect(() => mark("tasks-ready"), []);
  return (
    <div className={`taskcard p${task.priority}`} data-slug={task.slug}>
      <code className="taskcard-slug">{task.slug}</code> <span className="prio">p{task.priority}</span>
      <span className={`task-status st-${task.status}`}>{statusWords(task.status)}</span>
      <a className="taskcard-title" href={href(`/task/${task.slug}`)}>
        {task.title}
      </a>
      {task.parent ? (
        <div className="subtask-parent">
          Subtask of <a href={href(`/task/${task.parent}`)}>{task.parent}</a>
        </div>
      ) : null}
      <div className="chips">
        {task.links.map((link) => (
          <a key={link} className="docchip" href={href(`/${link}`)}>
            {link.split("/").pop()}
          </a>
        ))}
      </div>
      <Roles roles={task.roles} />
    </div>
  );
});

export function Roles({ roles }) {
  return (
    <span className="chips">
      {roles.map((role) => (
        <span key={`${role.role}:${role.handle}`} className={`chip health-${role.health}`} title={`${role.role} is ${role.health}`}>
          {role.role} {role.handle}
        </span>
      ))}
    </span>
  );
}
