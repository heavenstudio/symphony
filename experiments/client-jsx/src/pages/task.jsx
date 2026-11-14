// One task, `/task/SLUG` (`task_detail.rs`, `task_live.rs`, `task_save.rs`): `/api/board` for the
// feed's version, then the task's page (`/api/task-page`: its head and the brief's first part, a
// button reading the rest a chunk at a time from `/api/task-brief`), then its thread's newest page
// (`/api/task-history`) once the thread nears the screen, earlier pages by Load earlier messages,
// and the feed applied as it comes: a message for the task joins its thread once (by its id), a
// change of its fields merges into its head, and a change of its record, body or status reads the
// page again, the events that came while that read was out replayed over its answer. A reconnect
// reads the page again and catches a loaded thread up from its newest message. The message box
// posts `/api/task-message` (`slug`, `text`, `to`), keeps the draft on this device while it is
// sent, clears it once the answer comes (if it was not edited meanwhile) and says a refusal, or
// no answer within 15 s, under it. The thread and the box wait hidden under their ghost until the
// thread on the first screen is in (`viewport::Settle`, 3 s at most).
//
// Left out (named in the comparison): the gates section (`task_gates.rs`), the timeline's box and
// its module (`timeline_slot.rs`), the landing offer (`/api/task-offer`) and its button, the
// owner's task actions (the priority select and Cancel task are drawn, inert; no Later), the
// subtasks beyond their heading and count (no list, Load subtasks or Load more subtasks; a feed
// event for one reads the page again, as the Dioxus page does), the agents' Go to session controls and the `/agents` view, the machine's
// build and test phase chips (the fixture's machine reports none), attachments (the composer's
// Attach control and its sheet), the recipient select's popup (drawn, inert; the post names the
// recipient this device saved, by default everyone), the device cache and its checks and notice,
// the ghosts' kept heights (a task's ghost is its head, brief and thread at their default shapes,
// with no gates or timeline outline), `viewport::hold_place` on live updates, the board's Retry
// when its first read fails (it is tried again on its own), and the gates' nudges.
import { useEffect, useRef, useState } from "react";
import { encode, get, getFound } from "../api.js";
import { keepLive } from "../feed.js";
import { PageNotices } from "../frame.jsx";
import { Skeleton } from "../near.jsx";
import { mergeFields } from "../tasks.js";
import { GhostTask, TaskDetail, Trouble, failed } from "./task-detail.jsx";

const initial = {
  /** The board's version was read: the page draws from here on. */
  board: false,
  boardError: "",
  detail: null,
  /** The slug the server has no task for. */
  missing: null,
  error: "",
  entries: [],
  loaded: false,
  requested: false,
  /** The id the thread's earlier page ends before, while there is one. */
  before: null,
};

/** `Data::start`: another task is shown, nothing of the thread of the one before. */
const start = (data) => ({ ...data, entries: [], loaded: false, requested: false, before: null });

const byId = (a, b) => a.message.id - b.message.id;

/** `entries` and those of `more` not among them, in id order. */
function merged(entries, more) {
  const known = new Set(entries.map((entry) => entry.message.id));
  const added = more.filter((entry) => !known.has(entry.message.id));
  return added.length ? [...entries, ...added].sort(byId) : entries;
}

/** `words::BoardStatus::notice`. */
function boardStatus({ available, slow }) {
  if (available !== false) return "";
  return slow === true ? "The task board is slow to answer. Retrying…" : "The task board is unavailable. Reconnecting…";
}

function upsert(tasks, task) {
  const at = tasks.findIndex((old) => old.slug === task.slug);
  if (at < 0) return [...tasks, task];
  const next = tasks.slice();
  next[at] = task;
  return next;
}

/** `task_live::apply_event`: one feed event merged into the page, and whether to read it again. */
function applyEvent(before, value, slug) {
  let next = before;
  let reloadDetail = false;
  let reloadPage = false;
  for (const { region, data } of value.changes ?? []) {
    switch (region) {
      case "task":
      case "task-summary": {
        const page = next.detail;
        if (page) {
          const existed = page.children.some((task) => task.slug === data.key);
          const belongs = data.value?.parent === page.task.slug;
          if (existed || belongs) {
            const children = data.removed || !belongs ? page.children.filter((task) => task.slug !== data.key) : upsert(page.children, data.value);
            next = { ...next, detail: { ...page, children } };
            reloadDetail ||= !existed || !!data.removed || !belongs;
          }
        }
        if (next.detail?.task.slug === data.key) {
          if (data.removed) next = { ...next, detail: null, missing: data.key, error: "This task was removed" };
          else if (region === "task") reloadDetail = true;
        }
        // A task filed under the slug the page could not find: read it now.
        reloadPage ||= !data.removed && next.missing === data.key && slug === data.key;
        break;
      }
      case "task-fields":
        if (next.detail?.task.slug === data.key) {
          next = { ...next, detail: { ...next.detail, task: mergeFields(next.detail.task, data.fields) } };
          if (data.fields.body != null || data.fields.status != null) reloadDetail = true;
        }
        break;
      case "message":
        if (next.detail?.task.slug === data.slug && !next.entries.some((entry) => entry.message.id === data.id)) {
          const { stamp, blocks, ...message } = data;
          next = { ...next, entries: [...next.entries, { message, stamp: stamp ?? "", blocks: blocks ?? [] }] };
        }
        break;
      case "machine-work":
        if (next.detail) next = { ...next, detail: { ...next.detail, machine_work: data } };
        break;
      case "tasks-status":
        reloadPage ||= data.available === true;
        next = { ...next, error: boardStatus(data) };
        break;
      default:
    }
  }
  return { next, refresh: reloadPage || (reloadDetail && before.detail?.task.slug === slug) };
}

/**
 * The task page's reads and the feed's events (`task_live.rs`), over the page's data as `read()`
 * gives it now and `write(change)` changes it: a signal's `peek` and `write`.
 */
function taskLive(slug, read, write) {
  const encoded = encode(slug);
  let stopped = false;
  let generation = 0;
  let events = [];
  let refreshes = Promise.resolve();
  const fail = (error) => !stopped && write((data) => ({ ...data, error: error.message }));

  function applyOne(value) {
    let refresh = false;
    write((data) => {
      const out = applyEvent(data, value, slug);
      refresh = out.refresh;
      return out.next;
    });
    return refresh;
  }

  /** The events that came after `since`, applied again over a read's answer. */
  function replay(since) {
    const lost = events.length > 0 && since + 1 < events[0].generation;
    let needed = false;
    for (const event of events) if (event.generation > since) needed = applyOne(event.value) || needed;
    if (lost) load();
    else if (needed) refresh();
  }

  async function refresh() {
    const since = generation;
    let page;
    try {
      page = await getFound(`/api/task-page?slug=${encoded}`);
    } catch (error) {
      return fail(error);
    }
    if (stopped) return;
    write((data) => {
      if (page === null) {
        const next = { ...data, missing: slug, error: "" };
        return data.detail?.task.slug === slug ? start({ ...next, detail: null }) : next;
      }
      const same = data.detail?.task.slug === page.task.slug;
      const shown = same ? { ...page, children: data.detail.children } : page;
      return { ...(same ? data : start(data)), detail: shown, missing: null, error: "" };
    });
    replay(since);
  }

  async function history(older) {
    const data = read();
    if (!older && data.loaded) return;
    if (!data.requested) write((now) => ({ ...now, requested: true }));
    const before = older ? data.before : null;
    if (older && before == null) return;
    try {
      const page = await get(`/api/task-history?slug=${encoded}${before != null ? `&before=${before}` : ""}`);
      if (stopped) return;
      write((now) => ({ ...now, entries: merged(now.entries, page.entries), before: page.before, loaded: true }));
    } catch (error) {
      fail(error);
    }
  }

  /** A thread read before a reconnect, caught up from its newest message. */
  async function catchUp() {
    let after = read().entries.reduce((most, entry) => Math.max(most, entry.message.id), 0);
    for (;;) {
      let page;
      try {
        page = await get(`/api/task-history?slug=${encoded}&after=${after}`);
      } catch (error) {
        return fail(error);
      }
      if (stopped) return;
      write((now) => ({ ...now, entries: merged(now.entries, page.entries) }));
      if (page.after != null && page.after > after) after = page.after;
      else return;
    }
  }

  /** The page read at the board's version, and what of its thread was read before, again. */
  async function load() {
    const data = read();
    const same = data.detail?.task.slug === slug;
    const caughtUp = data.loaded && same;
    const requested = data.requested && same;
    await refresh();
    if (caughtUp) await catchUp();
    else if (requested) await history(false);
  }

  function delta(value) {
    generation++;
    events.push({ generation, value });
    if (events.length > 128) events.shift();
    // One read of the page at a time, as the Dioxus feed awaits each before the next event.
    if (applyOne(value)) refreshes = refreshes.then(refresh);
  }

  return {
    load,
    refresh,
    history,
    delta,
    stop: () => {
      stopped = true;
    },
  };
}

export default function TaskPage({ route }) {
  const slug = route.slug;
  const [data, setData] = useState(initial);
  const current = useRef(initial);
  const [stale, setStale] = useState(false);
  const [live] = useState(() =>
    taskLive(
      slug,
      () => current.current,
      (change) => {
        const next = change(current.current);
        if (next !== current.current) {
          current.current = next;
          setData(next);
        }
      },
    ),
  );

  useEffect(() => {
    const stop = keepLive({
      feed: (version) => `/api/feed?since=${version}&document=`,
      read: async () => {
        if (!current.current.board) {
          current.current = { ...current.current, board: true, boardError: "" };
          setData(current.current);
        }
        await live.load();
      },
      delta: live.delta,
      state: (on, error) => {
        setStale(!on);
        if (!on && error && !current.current.board) {
          current.current = { ...current.current, boardError: error };
          setData(current.current);
        }
      },
    });
    return () => {
      stop();
      live.stop();
    };
  }, [live]);

  const drawn = data.detail?.task.slug === slug;
  const shown = data.board || drawn;
  let content;
  if (shown) {
    const ready = drawn ? "task" : data.missing === slug ? "missing" : data.error ? "failed" : "loading";
    content = (
      <div className="task-page" data-ready={ready}>
        {drawn ? (
          <TaskDetail key={slug} page={data.detail} entries={data.entries} older={data.before != null} loaded={data.loaded} failed={!!data.error} stale={stale} onHistory={live.history} />
        ) : data.missing === slug ? (
          <Trouble title={`No task ${slug}`} text="The board has no task by this name: it may have been removed, or the link names it wrongly." back={["/tasks", "All tasks"]} onRetry={live.refresh} />
        ) : data.error ? (
          <Trouble title="The task could not be loaded" text={failed(data.error)} onRetry={live.refresh} />
        ) : (
          <GhostTask />
        )}
      </div>
    );
  } else if (data.boardError) {
    content = <Trouble title="The board could not be loaded" text={`${failed(data.boardError)} It is also tried again on its own.`} />;
  } else {
    content = <Skeleton lines={8} label="Loading the board…" />;
  }
  return (
    <main data-ready={shown ? "board" : data.boardError ? "failed" : "loading"} data-feed={stale ? "stale" : "live"}>
      {content}
      <PageNotices reconnecting={stale && data.board} error={drawn ? data.error : ""} />
    </main>
  );
}
