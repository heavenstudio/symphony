// A task's page, live (`/task/SLUG`): its parts, each a read of its own in the page's store
// (parts.ts): its page (head, facts, brief), its thread's newest page, its gates, the landing it
// offers and its timeline. Each is asked as the page mounts, behind nothing: the page and the
// thread are drawn at once from what this device kept of them (`taskItem`), marked stale, and
// checked with their copy's tag; every part is asked again until it answers, and none waits for
// another's read (docs/analysis/components-draw-when-their-data-arrives.md).
//
// The board's feed (live.tsx's `keepBoard`, which reads the board, follows its feed and starts
// again when the stream is lost) is heard one event at a time, with the page as it stood when it
// came: the store gives it to each part it is newer than, which applies it (the page, the thread)
// or reads itself again for what it names without carrying (the gates, the offer, the timeline),
// and drops it for a part whose answer already holds it. A reconnect reads again only the parts
// answered at another version than the one the feed opens from. What stays the page's own is what
// no key names: the thread's earlier pages and its catch-up after a gap, and the subtasks, read
// into the page as their section nears the screen.

import { flushSync } from "react-dom";
import { type Freshness, PAGE, shown, THREAD, taskItem, visitLater } from "./cache";
import type { FrameRefresh } from "./frame";
import { readTaskGates, WORDS, workOf } from "./gates-model";
import { type BoardRun, keepBoard, type PageBoard, Retry } from "./live";
import { type Heard, type Part, Parts, type Shown } from "./parts";
import { scoped } from "./project";
import { getJson, said } from "./request";
import { field, holdPlace, list } from "./shared";
import { holderHealth, merge, readItemChange, readListing, readTaskChange } from "./task";
import { heardEntry, readHistory, readOffer, readTaskPage } from "./task-read";
import {
  type ItemChange,
  type MachineWork,
  TASK_GATES_PATH,
  TASK_TIMELINE_PATH,
  TASK_TOKENS_PATH,
  type Task,
  type TaskChange,
  type TaskEntry,
  type TaskGates,
  type TaskOffer,
  type TaskPage,
} from "./wire";
import { readWork } from "./work";

/** A live update about to be drawn keeps the messages, the subtasks and the cards in place. */
const HELD_ROWS = "[id^=message-], [id^=child-], [data-slug]";

/** What the page made of a task's changed fields (`pageFields`). */
export interface PageFields {
  /** The page is read again: its task's brief or status changed, or a subtask came or went, so its
   * count of subtasks did. */
  reload: boolean;
  /** A task came under the page's task: its subtasks are read again with the page. */
  childCame: boolean;
}

/**
 * The page `page` with a task's changed fields, which the feed sends alone, never the task's record
 * again: into its task and into a subtask it lists. A subtask that left its task goes from the list,
 * and one that came is read with the list again.
 */
export function pageFields(page: TaskPage, change: TaskChange): [TaskPage, PageFields] {
  const done: PageFields = { reload: false, childCame: false };
  const { fields } = change;
  let next = page;
  if (page.task.slug === change.key) {
    next = { ...next, task: merge(next.task, fields) };
    done.reload = (fields.body ?? null) !== null || (fields.status ?? null) !== null;
  }
  const child = next.children.findIndex((task) => task.slug === change.key);
  if (child >= 0) {
    next = {
      ...next,
      children: next.children.map((task, at) => (at === child ? merge(task, fields) : task)),
    };
  }
  const parent = fields.parent ?? null;
  if (parent !== null && (parent === next.task.slug) !== child >= 0) {
    if (child >= 0) next = { ...next, children: next.children.filter((_, at) => at !== child) };
    else done.childCame = true;
    done.reload = true;
  }
  return [next, done];
}

/**
 * Whether the card the feed's `pending` region names by `key` (`/docs/X.md#card`) is in a document
 * the task links (`links`, `docs/X.md#part`): one its owner gate may count, so the page's gates read
 * again when it comes, changes or goes.
 */
export function linksCard(links: readonly string[], key: string): boolean {
  const path = key.replace(/^\/+/, "").split("#")[0] ?? "";
  return path !== "" && links.some((link) => link.split("#")[0] === path);
}

/**
 * The branch whose new tip changes the gates of the task `slug`: the branch they were last answered
 * for (`answered`, with the task they were answered for) while that is this task, else the slug,
 * the branch the board names a task's work by.
 */
export function gatesBranch(slug: string, answered: readonly [string, string] | null): string {
  return answered !== null && answered[0] === slug ? answered[1] : slug;
}

/** `entries` with each of `page` it lacks, in id order. */
export function withEntries(
  entries: readonly TaskEntry[],
  page: readonly TaskEntry[],
): TaskEntry[] {
  const shown = new Set(entries.map((entry) => entry.message.id));
  const added = page.filter((entry) => !shown.has(entry.message.id));
  return [...entries, ...added].sort((a, b) => a.message.id - b.message.id);
}

/** `tasks` with `change` made: a task removed, replaced or added last. */
function upsert(tasks: readonly Task[], change: ItemChange<Task>): Task[] {
  if (change.removed) return tasks.filter((task) => task.slug !== change.key);
  const task = change.value;
  if (task === null || task === undefined) return [...tasks];
  const at = tasks.findIndex((one) => one.slug === change.key);
  return at < 0 ? [...tasks, task] : tasks.map((one, index) => (index === at ? task : one));
}

/** What the board says of its own state: nothing unless it is unavailable; that it is slow when it
 * is, else that it is unavailable. */
export function boardNotice(available: boolean | undefined, slow: boolean): string {
  if (available !== false) return "";
  return slow
    ? "The task board is slow to answer. Retrying…"
    : "The task board is unavailable. Reconnecting…";
}

/** One change of a feed event, as a task's page reads it. */
export type Change =
  | { region: "task"; change: ItemChange<Task> }
  | { region: "task-fields"; change: TaskChange }
  | { region: "session"; key: string; health: string | null }
  | { region: "message"; entry: TaskEntry }
  | { region: "pending"; key: string | null }
  | { region: "branches"; moved: string[] }
  | { region: "machine-work"; work: MachineWork }
  | { region: "tasks-status"; available: boolean | undefined; slow: boolean };

/**
 * The changes of feed event `value` a task's page follows, read whole before any is applied; none
 * when one cannot be read, which loses the stream.
 */
export function readChanges(value: unknown): Change[] | null {
  const read: Change[] = [];
  for (const change of list(field(value, "changes"))) {
    const data = field(change, "data");
    const region = field(change, "region");
    if (region === "task") {
      const item = readItemChange(data);
      if (item === null) return null;
      read.push({ region, change: item });
    } else if (region === "task-fields") {
      const fields = readTaskChange(data);
      if (fields === null) return null;
      read.push({ region, change: fields });
    } else if (region === "session") {
      const key = field(data, "key");
      const health = field(field(data, "value"), "health");
      read.push({
        region,
        key: typeof key === "string" ? key : "",
        health: typeof health === "string" ? health : null,
      });
    } else if (region === "message") {
      const entry = heardEntry(data);
      if (entry === null) return null;
      read.push({ region, entry });
    } else if (region === "pending") {
      const key = field(data, "key");
      read.push({ region, key: typeof key === "string" ? key : null });
    } else if (region === "branches") {
      read.push({
        region,
        moved: list(data).filter((moved): moved is string => typeof moved === "string"),
      });
    } else if (region === "machine-work") {
      const work = readWork(data);
      if (work === null) return null;
      read.push({ region, work });
    } else if (region === "tasks-status") {
      const available = field(data, "available");
      read.push({
        region,
        available: typeof available === "boolean" ? available : undefined,
        slow: field(data, "slow") === true,
      });
    }
  }
  return read;
}

/**
 * A feed event as a task's page hears it: its changes, read whole, and the task's page as the page
 * drew it when the event came (none before it had one), which says what of them names the gates.
 * The store keeps it so, and judges it the same way on an answer read while it came.
 */
export interface TaskEvent {
  changes: Change[];
  page: TaskPage | null;
}

/** `slug` as a read's query names it. */
const named = (slug: string) => encodeURIComponent(slug);

/**
 * What the feed's `changes` leave of the page of `slug`: a change to its task or to a subtask it
 * lists merged in, the page read again where the change names what it does not carry (a new brief
 * or status, a subtask come or gone, its own record, the board back), and gone once its task is
 * removed. A page not drawn yet is read again for its own record (a task filed under a slug the
 * page could not find).
 */
export function hearPage(
  slug: string,
  page: TaskPage | undefined,
  changes: readonly Change[],
): Heard<TaskPage> | null {
  let next = page;
  let again = false;
  let gone = false;
  for (const change of changes) {
    switch (change.region) {
      case "task": {
        const item = change.change;
        const task = item.value ?? null;
        const belongs = task !== null && task.parent === slug;
        const existed = next?.children.some((child) => child.slug === item.key) ?? false;
        if (next !== undefined && (existed || belongs)) {
          next = {
            ...next,
            children:
              item.removed || !belongs
                ? next.children.filter((child) => child.slug !== item.key)
                : upsert(next.children, item),
          };
          again ||= !existed || item.removed || !belongs;
        }
        if (item.key === slug) {
          if (item.removed) gone = true;
          else again = true;
        }
        break;
      }
      // The feed sends a task it changed as these fields alone.
      case "task-fields": {
        if (next === undefined) break;
        const [after, done] = pageFields(next, change.change);
        next = after;
        again ||= done.reload;
        break;
      }
      // A holder's health: its roles on the page's task and its subtasks say it.
      case "session": {
        if (next === undefined) break;
        const [moved, tasks] = holderHealth(
          [next.task, ...next.children],
          change.key,
          change.health,
        );
        if (moved) {
          const [task, ...children] = tasks as [Task, ...Task[]];
          next = { ...next, task, children };
        }
        break;
      }
      case "machine-work":
        if (next !== undefined) next = { ...next, machine_work: change.work };
        break;
      // The board is back: what it said meanwhile is read.
      case "tasks-status":
        again ||= change.available === true;
        break;
    }
  }
  if (gone) return { value: undefined, gone };
  return next === page && !again ? null : { value: next, again };
}

/** The page of the task `slug` under the checkout `scope` names: its head, its facts, its brief. */
export function pagePart(scope: string, slug: string): Part<TaskPage, TaskEvent> {
  return {
    key: `${PAGE} ${slug}`,
    url: scoped(scope, `/api/task-page?slug=${named(slug)}`),
    kept: { item: taskItem(scope, slug), part: PAGE },
    read: readTaskPage,
    unreadable: "The task's page is not readable",
    // A page answered under another name is not this task's: the server has none by this one.
    absent: (page) => page.task.slug !== slug,
    // The subtasks the page read are its own (`LiveTask.children`), which no answer lists.
    merge: (answer, held) =>
      held !== undefined && held.children.length > 0
        ? { ...answer, children: held.children }
        : answer,
    hear: (page, event) => hearPage(slug, page, event.changes),
  };
}

/** A task's thread as its part holds it. */
export interface Thread {
  entries: TaskEntry[];
  /** The cursor of the page before the earliest shown; none at the thread's start. */
  before: number | null;
  /** The newest message shown before an answer that may not reach back to it: the messages
   * between are read from it (`LiveTask.catchUp`). */
  gap: number | null;
}

const ids = (entries: readonly TaskEntry[]) => entries.map((entry) => entry.message.id);

/**
 * The thread once the server answers its newest page again with `answer`, over what it showed
 * (`held`): every message shown stays (earlier pages, the feed's), the answer's are added, and
 * where the answer starts after the newest shown, and is not the whole thread, the messages
 * between may be missing (`gap`).
 */
export function rejoined(answer: Thread, held: Thread): Thread {
  const newest = Math.max(0, ...ids(held.entries));
  const oldest = Math.min(...ids(answer.entries));
  const apart = held.entries.length > 0 && answer.entries.length > 0 && oldest > newest;
  return {
    entries: withEntries(held.entries, answer.entries),
    before: held.entries.length > 0 ? held.before : answer.before,
    gap: apart && answer.before !== null ? newest : held.gap,
  };
}

/** What the feed's `changes` leave of the thread of `slug`: each of its messages, once. */
export function hearThread(
  slug: string,
  thread: Thread | undefined,
  changes: readonly Change[],
): Heard<Thread> | null {
  if (thread === undefined) return null;
  const held = new Set(ids(thread.entries));
  const came = changes.flatMap((change) =>
    change.region === "message" &&
    change.entry.message.slug === slug &&
    !held.has(change.entry.message.id)
      ? [change.entry]
      : [],
  );
  if (came.length === 0) return null;
  return { value: { ...thread, entries: withEntries(thread.entries, came) } };
}

/** The newest page of the thread of the task `slug`: the part of the page that matters most. */
export function threadPart(scope: string, slug: string): Part<Thread, TaskEvent> {
  return {
    key: `${THREAD} ${slug}`,
    url: scoped(scope, `/api/task-history?slug=${named(slug)}`),
    kept: { item: taskItem(scope, slug), part: THREAD },
    first: true,
    unreadable: "The task's thread is not readable",
    read: (value) => {
      const page = readHistory(value);
      return page === null ? null : { entries: page.entries, before: page.before, gap: null };
    },
    // Over this device's copy the answer stands alone: the store applies the feed's messages
    // since to it again.
    merge: (answer, held, fresh) => (fresh && held !== undefined ? rejoined(answer, held) : answer),
    hear: (thread, event) => hearThread(slug, thread, event.changes),
  };
}

/**
 * Whether `event` may change the gates of the task `slug`, answered as `gates` (none before an
 * answer): the task itself changed, a verdict on it came, a card came, changed or went in a
 * document it links, the branch they were answered for (`gatesBranch`) has a new tip, or one of
 * its builds or tests started, moved on or ended. What it links and the machine work it had are
 * the page's as it stood when the event came (`TaskEvent.page`).
 */
export function namesGates(slug: string, gates: TaskGates | undefined, event: TaskEvent): boolean {
  const { page } = event;
  // Its part's answer is the task's, whatever slug it names.
  const branch = gatesBranch(
    slug,
    gates === undefined || gates.branch === "" ? null : [slug, gates.branch],
  );
  return event.changes.some((change) => {
    switch (change.region) {
      case "task":
      case "task-fields":
        return change.change.key === slug;
      case "message":
        return change.entry.message.slug === slug && change.entry.message.kind === "verdict";
      case "pending":
        return change.key !== null && page !== null && linksCard(page.task.links, change.key);
      case "branches":
        return change.moved.includes(branch);
      case "machine-work":
        return page !== null && workOf(page.machine_work, slug) !== workOf(change.work, slug);
      default:
        return false;
    }
  });
}

/**
 * The gates of the task `slug` (gates.tsx draws them); the server's 404 is a task nothing gates.
 * An event that may change them (`namesGates`) reads them again, the answer shown meanwhile.
 */
export function gatesPart(scope: string, slug: string): Part<TaskGates, TaskEvent> {
  return {
    key: `gates ${slug}`,
    url: scoped(scope, `${TASK_GATES_PATH}?slug=${named(slug)}`),
    read: readTaskGates,
    unreadable: WORDS.unreadable,
    hear: (gates, event) => (namesGates(slug, gates, event) ? { value: gates, again: true } : null),
  };
}

/** Whether `changes` change the task `slug` itself: its record, or fields of it. */
export function changesTask(slug: string, changes: readonly Change[]): boolean {
  return changes.some(
    (change) =>
      (change.region === "task" || change.region === "task-fields") && change.change.key === slug,
  );
}

/**
 * The landing the task `slug` offers. An offer is the task's as it was asked: once the task
 * changes, none stands (its payload goes) until it is read again, for an answer asked before the
 * change that lands after it too, which the store applies the change to.
 */
export function offerPart(scope: string, slug: string): Part<TaskOffer, TaskEvent> {
  return {
    key: `offer ${slug}`,
    url: scoped(scope, `/api/task-offer?slug=${named(slug)}`),
    read: readOffer,
    unreadable: "The task's offer is not readable",
    hear: (offer, event) =>
      changesTask(slug, event.changes)
        ? {
            value:
              offer === undefined || offer.payload === null ? offer : { ...offer, payload: null },
            again: true,
          }
        : null,
  };
}

/**
 * The timeline of the task `slug`, as the server sent it: its chunk (timeline.tsx), which a first
 * visit does not carry, reads it as it draws it.
 */
export function timelinePart(scope: string, slug: string): Part<unknown, TaskEvent> {
  return {
    key: `timeline ${slug}`,
    url: scoped(scope, `${TASK_TIMELINE_PATH}?slug=${named(slug)}`),
    read: (value) => value ?? null,
    // The task's own change moves its timeline: read again, the one shown left meanwhile.
    hear: (value, event) => (changesTask(slug, event.changes) ? { value, again: true } : null),
  };
}

/**
 * The tokens of the task `slug` and what they cost, as the server sent them: the chunk of the
 * page's disclosure of them (task-tokens.tsx), which a first visit does not carry, reads them as it
 * draws them. Asked once the disclosure opens.
 */
export function tokensPart(scope: string, slug: string): Part<unknown, TaskEvent> {
  return {
    key: `tokens ${slug}`,
    url: scoped(scope, `${TASK_TOKENS_PATH}?slug=${named(slug)}`),
    read: (value) => value ?? null,
    // The task's own change (a move, a verdict's status, a role) is when its figures are worth
    // reading again: a heartbeat says nothing on the feed.
    hear: (value, event) => (changesTask(slug, event.changes) ? { value, again: true } : null),
  };
}

/** What the live task changes on the page that draws it. */
export interface TaskHost {
  /** The data changed: draw it again. */
  redraw(): void;
  /** What the page shows from this device changed. */
  freshness(f: Freshness): void;
  /** Whether the feed is lost. */
  stale(stale: boolean): void;
}

/**
 * A task's page as it is kept live: the store of its parts, the feed heard into it, and the reads
 * that are the page's own, for the route's task (`slug`), which a move within the kind changes
 * (`moved`). Nothing it does goes on once `signal` ends.
 */
export class LiveTask {
  readonly parts: Parts<TaskEvent>;
  /** Whether the board was read. */
  boardShown = false;
  /** How many times the reader pressed Retry, which the board's loop waits on. */
  readonly retry = new Retry();
  /** Why the board's last read failed, while nothing is drawn: said in place of the page. */
  trouble: string | null = null;
  /** What the board says of its own state (`boardNotice`). */
  private notice = "";
  /** Why a read the reader asked for failed (earlier messages, subtasks), until the page answers. */
  private failed = "";
  /** The subtasks were asked for, and where their next page starts. */
  private childRequested = false;
  private childCursor: string | null = null;
  /** A task came under the shown one since its subtasks were read: read again with the page. */
  private childCame = false;
  /** Earlier messages were asked for while the thread showed this device's copy: read once the
   * thread is answered, from the cursor its answer gives. */
  private olderWaiting = false;
  private catching = false;

  constructor(
    readonly scope: string,
    /** The route's task. */
    public slug: string,
    private readonly host: TaskHost,
    readonly signal: AbortSignal,
  ) {
    this.parts = new Parts<TaskEvent>(signal, {
      freshness: (f) => host.freshness(f),
      answered: (key) => this.answered(key),
    });
    this.parts.subscribe(() => host.redraw());
  }

  /** `path` under the page's checkout. */
  url(path: string): string {
    return scoped(this.scope, path);
  }

  /** The route's page, as the store keys it. */
  get pagePart(): Part<TaskPage, TaskEvent> {
    return pagePart(this.scope, this.slug);
  }

  /** The route's thread, as the store keys it. */
  get threadPart(): Part<Thread, TaskEvent> {
    return threadPart(this.scope, this.slug);
  }

  /** The route's page, once the server or this device gave it. */
  page(): TaskPage | null {
    return this.parts.shown(this.pagePart).value ?? null;
  }

  /** Whether the server has no task by the route's slug. */
  missing(): boolean {
    return this.parts.shown(this.pagePart).missing;
  }

  /** The route's thread as its part stands. */
  thread(): Shown<Thread> {
    return this.parts.shown(this.threadPart);
  }

  /** Whether the page has its task's page to draw: until it has, a failed read is said in the
   * head's place, not by the notices. */
  drawn(): boolean {
    return this.page() !== null;
  }

  /**
   * What the page says went wrong: the board's own state, else why a read of the page, the
   * thread or the owner's offer fails (the gates and the timeline say theirs in their place; a
   * page drawn from this device says the server is not reachable instead), else why a read the
   * reader asked for failed.
   */
  error(): string {
    if (this.notice !== "") return this.notice;
    if (shown(this.parts.fresh) === null) {
      const noticed = [this.pagePart, this.threadPart, offerPart(this.scope, this.slug)];
      for (const part of noticed as Part<unknown, TaskEvent>[]) {
        const { error } = this.parts.shown(part);
        if (error !== null) return said(error);
      }
    }
    return this.failed;
  }

  /** Whether `selected` is still the task shown, the page not stopped. */
  private current(selected: string): boolean {
    return !this.signal.aborted && this.slug === selected;
  }

  /** Open the route's task, and keep the board live. */
  start(frame: FrameRefresh): Promise<void> {
    this.open();
    return keepBoard(this.scope, null, this.run(frame), this.signal);
  }

  /** One more visit to the route's task: its page and its thread opened, each drawn from this
   * device and read. The parts its components name open as they mount. */
  open(): void {
    visitLater(taskItem(this.scope, this.slug));
    this.parts.open(this.pagePart);
    this.parts.open(this.threadPart);
  }

  /** What `keepBoard` changes on this page. */
  run(frame: FrameRefresh): BoardRun {
    const board: { current: PageBoard | null } = { current: null };
    return {
      board,
      show: (read) => {
        board.current = read;
        if (this.signal.aborted || this.boardShown) return;
        this.boardShown = true;
        this.host.redraw();
      },
      setStale: (stale) => this.host.stale(stale),
      setTrouble: (why) => {
        if (this.signal.aborted) return;
        this.trouble = why;
        this.host.redraw();
      },
      retry: this.retry,
      views: {
        freshen: (change) => this.parts.freshen(change),
        // Nothing for the board's loop to wait for: the feed opens at the board's answer, beside
        // the parts' reads, each judged against the version it opens from.
        read: () => {
          if (board.current !== null) this.parts.since(board.current.version);
        },
        heard: (value) => this.heard(value),
        frame,
      },
    };
  }

  /** The route moved to the task `slug`: nothing of the one before is shown or read any more. */
  moved(slug: string): void {
    if (slug === this.slug) return;
    const before = ` ${this.slug}`;
    this.slug = slug;
    this.failed = "";
    this.childRequested = false;
    this.childCursor = null;
    this.childCame = false;
    this.olderWaiting = false;
    this.parts.drop((key) => key.endsWith(before));
    this.open();
    this.host.redraw();
  }

  /** Read the route's page again at once (the reader's Retry). */
  again(): void {
    void this.parts.read(this.pagePart);
  }

  /**
   * One feed event: held for the reads still out and applied to each part it is newer than by the
   * store, each part reading itself again for what it names without carrying (its `hear`). False
   * for one the page cannot read, which loses the stream.
   */
  heard(value: unknown): boolean {
    const changes = readChanges(value);
    const version = field(value, "version");
    if (changes === null || typeof version !== "number") return false;
    holdPlace(HELD_ROWS);
    const { slug } = this;
    const page = this.page();
    let notice = this.notice;
    for (const change of changes) {
      if (change.region === "task-fields") {
        const { key, fields } = change.change;
        const listed = page?.children.some((child) => child.slug === key) ?? false;
        this.childCame ||= (fields.parent ?? null) === slug && !listed;
      } else if (change.region === "tasks-status") {
        notice = boardNotice(change.available, change.slow);
      }
    }
    // Drawn at once, within the event, so the hold starts from the change in the page: left to
    // React's scheduler (the page's redraw, a later task a loaded renderer runs after its frames),
    // the hold's four quiet frames could pass before it.
    flushSync(() => {
      // With the page as it was drawn when the event came: what it links and its machine work
      // before the event say whether the gates change (`namesGates`).
      this.parts.heard(version, { changes, page });
      if (notice !== this.notice) {
        this.notice = notice;
        this.host.redraw();
      }
    });
    return true;
  }

  /** The server answered the part `key`: what waited for it is read. */
  private answered(key: string): void {
    if (key === this.pagePart.key) {
      this.failed = "";
      const page = this.page();
      const came = this.childCame;
      this.childCame = false;
      if (
        page !== null &&
        this.childRequested &&
        (came || (page.children.length === 0 && page.children_count > 0))
      ) {
        void this.children(false);
      }
    } else if (key === this.threadPart.key) {
      const thread = this.thread().value;
      if (this.olderWaiting) {
        this.olderWaiting = false;
        void this.history(true);
      }
      if (thread !== undefined && thread.gap !== null) void this.catchUp(thread.gap);
    }
  }

  /** Say why a read the reader asked for failed. */
  private fail(selected: string, error: unknown): void {
    if (!this.current(selected)) return;
    this.failed = said(error);
    this.host.redraw();
  }

  private earlierRead: { slug: string; promise: Promise<string | null> } | null = null;

  /**
   * Read the thread: its newest page again at once (a reader's ask; the part reads it by itself),
   * or the page before the earliest shown (`older`, never kept). An earlier one waits while the
   * thread shows this device's copy, or none, as its cursor may be another.
   */
  history(older: boolean): Promise<string | null> {
    if (!older)
      return this.parts.read(this.threadPart).then(() => {
        const error = this.thread().error;
        return error === null ? null : said(error);
      });
    if (this.earlierRead?.slug === this.slug) return this.earlierRead.promise;
    const reading = this.readEarlier();
    this.earlierRead = { slug: this.slug, promise: reading };
    void reading.finally(() => {
      if (this.earlierRead?.promise === reading) this.earlierRead = null;
    });
    return reading;
  }

  private async readEarlier(): Promise<string | null> {
    const selected = this.slug;
    const part = this.threadPart;
    const held = this.thread();
    if (!held.answered || held.stale) {
      this.olderWaiting = true;
      return null;
    }
    const before = held.value?.before ?? null;
    if (before === null) return null;
    let earlier: NonNullable<ReturnType<typeof readHistory>>;
    try {
      const path = `/api/task-history?slug=${named(selected)}&before=${before}`;
      const page = readHistory(await getJson(this.url(path), this.signal));
      if (page === null) throw new Error("The task's thread is not readable");
      earlier = page;
    } catch (error) {
      this.fail(selected, error);
      return said(error);
    }
    if (!this.current(selected)) return null;
    if (
      earlier.before === before &&
      earlier.entries.every((entry) =>
        held.value?.entries.some((e) => e.message.id === entry.message.id),
      )
    ) {
      return "The thread read made no progress. Retry to load earlier messages.";
    }
    this.parts.change(part, (thread) => ({
      ...thread,
      entries: withEntries(thread.entries, earlier.entries),
      before: earlier.before,
    }));
    return null;
  }

  /** The messages after message `after`, a page at a time, until the server has no more. */
  private async catchUp(after: number): Promise<void> {
    if (this.catching) return;
    this.catching = true;
    const selected = this.slug;
    const part = this.threadPart;
    try {
      for (;;) {
        const path = `/api/task-history?slug=${named(selected)}&after=${after}`;
        const page = readHistory(await getJson(this.url(path), this.signal));
        if (page === null) throw new Error("The task's thread is not readable");
        if (!this.current(selected)) return;
        const more = page.after !== null && page.after > after;
        this.parts.change(part, (thread) => ({
          ...thread,
          entries: withEntries(thread.entries, page.entries),
          gap: more ? thread.gap : null,
        }));
        if (page.after === null || !more) return;
        after = page.after;
      }
    } catch (error) {
      this.fail(selected, error);
    } finally {
      this.catching = false;
    }
  }

  /** Read the task's subtasks into its page: their first page, or the next (`more`). A page drawn
   * from this device, or not drawn yet, reads them once it is answered. */
  async children(more: boolean): Promise<void> {
    const selected = this.slug;
    const part = this.pagePart;
    this.childRequested = true;
    const held = this.parts.shown(part);
    if (!held.answered || held.stale) return;
    const cursor = more ? this.childCursor : null;
    const after = cursor === null ? "" : `&cursor=${encodeURIComponent(cursor)}`;
    const path = `/api/task-board?parent=${named(selected)}&older=1${after}`;
    let listed: NonNullable<ReturnType<typeof readListing>>;
    try {
      const read = readListing(await getJson(this.url(path), this.signal));
      if (read === null) throw new Error("The task board's answer is not a listing");
      listed = read;
    } catch (error) {
      return this.fail(selected, error);
    }
    if (!this.current(selected)) return;
    this.childCursor = listed.next ?? null;
    const { tasks, counts } = listed;
    this.parts.change(part, (page) => {
      let children = more ? page.children : [];
      for (const task of tasks) {
        children = upsert(children, { key: task.slug, value: task, removed: false });
      }
      return { ...page, children, children_count: counts.reduce((sum, [, n]) => sum + n, 0) };
    });
  }
}
