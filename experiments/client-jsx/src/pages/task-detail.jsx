// A task's page as `task_detail.rs` draws it: its head, its brief, its thread and its message box,
// the ghosts while they read, and the block a page shows for what it could not read
// (`trouble.rs`). What is left out is named in `task.jsx`.
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { encode, get, post, sleep } from "../api.js";
import { Blocks } from "../blocks.jsx";
import { Button, SelectTrigger, Textarea } from "../controls.jsx";
import { mark } from "../frame.jsx";
import { Near, Skeleton } from "../near.jsx";
import { href, stored } from "../project.js";
import { statusWords } from "../tasks.js";

const CHECKING = "Checking…";
/** `task_save::NO_ANSWER` and `ANSWER_MS`: a post the server did not answer in time. */
const NO_ANSWER = "✖ no answer from the server; check before sending again";
const ANSWER_MS = 15000;
/** `viewport::SETTLE_MS`: the longest the thread stays under its ghost. */
const SETTLE_MS = 3000;
const PRIORITIES = [0, 1, 2, 3, 4].map((value) => [String(value), `p${value}`]);
const EMPTY = [];

/** `trouble::failed`: the sentence for a read that did not finish. */
export function failed(why) {
  const reason = why.trim().replace(/\.+$/, "");
  if (!reason) return "The read did not finish. Try again.";
  return `${reason}.`;
}

export function Trouble({ title, text, back, onRetry }) {
  return (
    <section className="trouble card" role="status">
      <h1>{title}</h1>
      <p>{text}</p>
      <p className="trouble-actions">
        {onRetry ? <Button onClick={() => onRetry()}>Retry</Button> : null}
        {back ? (
          <a className="trouble-back" href={href(back[0])}>
            <span aria-hidden="true">← </span>
            {back[1]}
          </a>
        ) : null}
      </p>
    </section>
  );
}

function load(key) {
  try {
    return localStorage.getItem(stored(key)) ?? "";
  } catch {
    return "";
  }
}

function save(key, value) {
  try {
    localStorage.setItem(stored(key), value);
  } catch {}
}

export function TaskDetail({ page, entries, older, loaded, failed, stale, onHistory }) {
  const task = page.task;
  const [settled, setSettled] = useState(false);
  const [composing, setComposing] = useState(false);
  // Usable: the thread's newest page is drawn, the region under its ghost shows, and the box is in.
  useEffect(() => {
    if (settled && loaded && composing) mark("task-ready");
  }, [settled, loaded, composing]);
  const enterThread = useCallback(() => onHistory(false), [onHistory]);
  const settle = useCallback(() => setSettled(true), []);
  const compose = useCallback(() => setComposing(true), []);
  return (
    <article className="task" data-slug={task.slug}>
      <TaskHead task={task} stale={stale} />
      {page.children_count > 0 ? (
        <Near id={`subtasks-${task.slug}`} height={60}>
          <h2>
            Subtasks <span className="count">{page.children_count}</span>
          </h2>
        </Near>
      ) : null}
      <Brief slug={task.slug} revision={page.brief_revision ?? ""} blocks={page.blocks} omitted={page.brief_omitted ?? 0} next={page.brief_next ?? 0} />
      <h2>Thread</h2>
      {/* The head and the brief are in with the page; the thread and the box under it wait under
          their ghost until the thread on the first screen is in. */}
      <Settle failed={failed} cover={<GhostThread />} onSettled={settle}>
        <Near id={`thread-region-${task.slug}`} height={200} onEnter={enterThread}>
          <div id="task-thread" data-pending={String(!loaded)}>
            {older ? <Button onClick={() => onHistory(true)}>Load earlier messages</Button> : null}
            {!loaded ? <Button onClick={() => onHistory(false)}>Load messages</Button> : null}
            <ul className="history">
              {entries.map((entry) => (
                <ThreadEntry key={entry.message.id} entry={entry} />
              ))}
            </ul>
            {!loaded ? (
              <div className="ghost-history">
                <GhostEntry />
                <GhostEntry />
              </div>
            ) : null}
          </div>
        </Near>
        <Near id={`composer-${task.slug}`} height={200}>
          <Composer key={task.slug} slug={task.slug} roles={task.roles} stale={stale} onMounted={compose} />
        </Near>
      </Settle>
    </article>
  );
}

const TaskHead = memo(function TaskHead({ task, stale }) {
  return (
    <div className="task-head card">
      <p className="eyebrow">
        <code>{task.slug}</code>
      </p>
      <h1>{task.title}</h1>
      {/* The state, who filed it, then the owner's controls apart from them. */}
      <div className="task-state row">
        <span className={`state pill st-${task.status}`}>{statusWords(task.status)}</span>
        <span className="by">filed by {task.created_by}</span>
        <OwnerControls priority={task.priority} status={task.status} stale={stale} />
      </div>
      <ul className="facts">
        {task.parent ? (
          <li>
            part of <a href={href(`/task/${task.parent}`)}>{task.parent}</a>
          </li>
        ) : null}
        {task.depends.map((dependency) => (
          <li key={`after ${dependency}`}>
            after <a href={href(`/task/${dependency}`)}>{dependency}</a>
          </li>
        ))}
        {task.links.map((link) => (
          <li key={`link ${link}`}>
            implements <a href={href(`/${link}`)}>{link}</a>
          </li>
        ))}
      </ul>
      <span className="chips">
        {task.roles.map((role) => (
          <span key={`${role.role}:${role.handle}`} className={`chip health-${role.health}`} title={`${role.role} is ${role.health}`}>
            {role.role} {role.handle}
          </span>
        ))}
      </span>
    </div>
  );
});

/** The owner's priority and Cancel task, drawn as the Dioxus page draws them, inert here. */
function OwnerControls({ priority, status, stale }) {
  const terminal = status === "done" || status === "cancelled";
  return (
    <div className="task-controls row">
      <label className="inline-control" htmlFor="task-priority">
        Priority <SelectTrigger id="task-priority" label="Priority" value={String(priority)} options={PRIORITIES} disabled={stale} />
      </label>
      {!terminal ? <Button disabled={stale}>{stale ? CHECKING : "Cancel task"}</Button> : null}
    </div>
  );
}

/** `BriefRead::add`: a chunk of the brief at `revision` added after what was read of it. */
function addChunk(held, revision, next, omitted, answer) {
  const read = held?.revision === revision ? held : { revision, blocks: EMPTY, next, omitted, failed: "" };
  if (answer.error) return { ...read, failed: answer.error };
  if (answer.chunk.revision !== revision) return { ...read, failed: "The brief changed since this page was sent; the page shows the new one when it reloads." };
  return { revision, blocks: [...read.blocks, ...answer.chunk.blocks], next: answer.chunk.next, omitted: answer.chunk.omitted, failed: "" };
}

/** The brief, which the page sends cut when it is very long; a button reads the rest a chunk at a time. */
function Brief({ slug, revision, blocks, omitted, next }) {
  const [read, setRead] = useState(null);
  const [busy, setBusy] = useState(false);
  const held = read?.revision === revision ? read : null;
  const more = held ? held.blocks : EMPTY;
  const from = held ? held.next : next;
  const remaining = held ? held.omitted : omitted;
  const shown = useMemo(() => (more.length ? [...blocks, ...more] : blocks), [blocks, more]);
  const readMore = async () => {
    if (busy) return;
    setBusy(true);
    let answer;
    try {
      answer = { chunk: await get(`/api/task-brief?slug=${encode(slug)}&from=${from}`) };
    } catch (error) {
      answer = { error: error.message };
    }
    setRead((old) => addChunk(old, revision, next, omitted, answer));
    setBusy(false);
  };
  return (
    <div className="task-body card">
      <Blocks blocks={shown} />
      {remaining > 0 ? (
        <>
          <p className="brief-cut">
            This brief is long; {remaining} bytes of it are not shown yet.{" "}
            <Button id="brief-more" disabled={busy} onClick={readMore}>
              Show more of the brief
            </Button>
          </p>
          {held?.failed ? <p role="alert">{held.failed}</p> : null}
        </>
      ) : null}
    </div>
  );
}

const ThreadEntry = memo(function ThreadEntry({ entry }) {
  return (
    <Near id={`message-${entry.message.id}`} listItem height={100} ghost={<GhostEntry />}>
      <TaskMessage entry={entry} />
    </Near>
  );
});

function TaskMessage({ entry }) {
  const { message, stamp, blocks } = entry;
  const who = message.sender === "owner" ? "owner" : message.kind === "system" ? "system" : "agent";
  return (
    <div className={`entry ${who}`}>
      <time>{stamp}</time>{" "}
      <span className="from">
        {message.sender}
        {message.recipient ? ` → ${message.recipient}` : null}
      </span>
      {message.kind === "verdict" ? <b>verdict </b> : null}
      {/* An owner message is literal text, unless it names an upload: the server then sends its blocks. */}
      {who === "agent" || (who === "owner" && blocks.length > 0) ? (
        <div className="entry-text">
          <Blocks blocks={blocks} />
        </div>
      ) : (
        <div className="entry-text literal">{message.text}</div>
      )}
    </div>
  );
}

/** `task_save::in_time`: `answer`, or the failure a post no answer came for within `ms`. */
function inTime(answer, ms) {
  return Promise.race([answer, sleep(ms).then(() => Promise.reject(new Error(NO_ANSWER)))]);
}

/**
 * The message box. Its draft is kept on this device as it is typed; a sent draft is cleared only
 * once the server answers, and only if the owner has not edited it while the post ran.
 */
function Composer({ slug, roles, stale, onMounted }) {
  const draftKey = `task-draft:${slug}`;
  const toKey = `task-to:${slug}`;
  const [draft, setDraft] = useState(() => load(draftKey));
  const [recipient] = useState(() => load(toKey));
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState("");
  const typed = useRef(draft);
  const sending = useRef(false);
  useEffect(() => onMounted?.(), [onMounted]);
  const options = useMemo(
    () => [["", "everyone on it"], ...["@executor", "@reviewer", "@orchestrator"].map((role) => [role, role]), ...roles.map((role) => [role.handle, role.handle])],
    [roles],
  );
  const send = async () => {
    const text = typed.current;
    if (sending.current || stale || !text.trim()) return;
    sending.current = true;
    setBusy(true);
    try {
      await inTime(post("/api/task-message", { slug, text, to: recipient }), ANSWER_MS);
      if (typed.current === text) {
        save(draftKey, "");
        typed.current = "";
        setDraft("");
        setResult("Sent");
      } else {
        setResult("");
      }
    } catch (error) {
      setResult(error.message);
    }
    sending.current = false;
    setBusy(false);
  };
  const input = (event) => {
    const text = event.target.value;
    typed.current = text;
    save(draftKey, text);
    setDraft(text);
    if (result === "Sent") setResult("");
  };
  const key = (event) => {
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      send();
    }
  };
  return (
    <form
      id="task-send"
      className="verdict"
      data-posting={String(busy)}
      onSubmit={(event) => {
        event.preventDefault();
        send();
      }}
    >
      <label htmlFor="task-message">Message</label>
      <Textarea id="task-message" value={draft} placeholder="Message (⌘/Ctrl+Enter sends). Agents on this task hear it wherever they run." onChange={input} onKeyDown={key} />
      <div className="row">
        <label className="inline-control">
          to <SelectTrigger label="Message recipient" value={recipient} options={options} />
        </label>
        <Button type="submit" disabled={busy || stale}>
          {stale ? CHECKING : "Send"}
        </Button>
      </div>
      <p role="status">{result}</p>
    </form>
  );
}

let settles = 0;

/** `viewport::on_screen`: whether a box shows on the part of the viewport its page covers. */
function onScreen(box, [top, bottom, left, right]) {
  return box.bottom > box.top && box.right > box.left && box.bottom > top && box.top < bottom && box.right > left && box.left < right;
}

/** Whether `root` has not drawn its page yet, or a region of it on the screen still waits for its read. */
function waiting(root) {
  if (root.childElementCount < 2) return true;
  const page = root.getBoundingClientRect();
  const screen = [0, window.innerHeight, Math.max(page.left, 0), Math.min(page.right, window.innerWidth)];
  return [...root.querySelectorAll("[data-pending=true]")].some((region) => onScreen(region.getBoundingClientRect(), screen));
}

/**
 * `viewport::Settle`: `children` hidden under `cover` until no region they reserve on the first
 * screen waits for its read, until `failed`, or for 3 s at most, checked every 16 ms.
 */
function Settle({ failed, cover, onSettled, children }) {
  const [id] = useState(() => `settle-${++settles}`);
  const root = useRef(null);
  const [settled, setSettled] = useState(false);
  useEffect(() => {
    if (failed) setSettled(true);
  }, [failed]);
  useEffect(() => {
    if (settled) {
      onSettled?.();
      return;
    }
    let drawn = 0;
    let timer = 0;
    const check = () => {
      if (root.current) {
        drawn ||= Date.now();
        if (!waiting(root.current) || Date.now() - drawn > SETTLE_MS) return setSettled(true);
      }
      timer = setTimeout(check, 16);
    };
    timer = setTimeout(check, 16);
    return () => clearTimeout(timer);
  }, [settled, onSettled]);
  return (
    <div className="settle" data-settle={id} data-settled={String(settled)} aria-busy={String(!settled)} ref={root}>
      {!settled ? <div className="settle-cover">{cover}</div> : null}
      {children}
    </div>
  );
}

/** A message of a task's thread while it loads: a bar for its time and sender, and two lines. */
function GhostEntry() {
  return (
    <div className="ghost-entry">
      <span className="bone" />
      <Skeleton lines={2} />
    </div>
  );
}

/** A task's thread and message box while the thread loads. */
function GhostThread({ announce = true }) {
  return (
    <>
      {announce ? (
        <span className="status-text" role="status">
          Loading the thread…
        </span>
      ) : null}
      <div className="ghost-history">
        <GhostEntry />
        <GhostEntry />
        <GhostEntry />
      </div>
      <div className="ghost-composer">
        <div>
          <span className="bone" style={{ "--w": "7ch" }} />
        </div>
        <div className="ghost-field ghost-textarea" />
        <div>
          <span className="ghost-field" style={{ width: "6rem" }} />
          <span className="bone" style={{ "--w": "22ch" }} />
        </div>
        <div>
          <span className="bone" style={{ "--w": "2ch" }} />
          <span className="ghost-field" style={{ width: "9rem" }} />
          <span className="ghost-field" style={{ width: "4.5rem" }} />
        </div>
      </div>
    </>
  );
}

/** A task's page before its read, at the shapes a task with nothing kept takes: its head, brief and thread. */
export function GhostTask() {
  const bone = (width) => <span className="bone" style={{ "--w": width }} />;
  return (
    <div aria-busy="true">
      <span className="status-text" role="status">
        Loading task…
      </span>
      <div className="card ghost-head">
        <div>{bone("18ch")}</div>
        <div className="ghost-title">
          {bone("72%")}
          {bone("45%")}
        </div>
        <div className="row ghost-meta">
          <span className="ghost-pill">{bone("7ch")}</span>
          {bone("13ch")}
          <div className="row ghost-controls">
            {bone("6ch")}
            <span className="ghost-field" style={{ width: "4rem" }} />
            <span className="ghost-field" style={{ width: "7rem" }} />
          </div>
        </div>
        <div className="ghost-meta">{bone("40%")}</div>
        <span className="ghost-pill">{bone("14ch")}</span>
      </div>
      <div className="card ghost-brief">
        <Skeleton lines={4} />
        <Skeleton lines={3} />
      </div>
      <div className="ghost-heading">{bone("5ch")}</div>
      <GhostThread announce={false} />
    </div>
  );
}
