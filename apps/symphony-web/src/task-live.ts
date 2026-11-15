// A task's page, live (`/task/SLUG`): its page, its thread and its subtasks, read from the server's
// JSON API and kept current by the board's feed, one event at a time (live.tsx's `keepBoard`, which
// reads the board, follows its feed and starts again when the stream is lost). Only a route change
// and a reconnect read the page whole; an ordinary event merges one record.
//
// A task page keeps its page and its thread's newest page on this device (`taskItem`): opened again,
// it draws them at once (`opened`) and, once the board's version is read, asks whether each changed
// (`load`), a `304` confirming the copy and a `200` replacing it on the page and on the device
// (docs/analysis/client-cache.md).
//
// The feed opens beside those reads, not after them: its cursor is the board's version, read before
// any of them is asked, so each answer is at that version or after it, and an event heard before an
// answer is applied again onto it (`replay`).

import {
  answered,
  drawn,
  FRESH,
  type Freshness,
  failed,
  forget,
  type Item,
  keepLater,
  left,
  PAGE,
  readKept,
  shown,
  THREAD,
  taskItem,
  visitLater,
  waits,
} from "./cache";
import type { FrameRefresh } from "./frame";
import { type BoardRun, keepBoard, type PageBoard } from "./live";
import { scoped } from "./project";
import { Failure, getIf, getJson, said } from "./request";
import { field, holdPlace, list } from "./shared";
import { tryAgain } from "./state";
import { holderHealth, merge, readItemChange, readListing, readTaskChange } from "./task";
import { heardEntry, readHistory, readOffer, readTaskPage } from "./task-read";
import type {
  ItemChange,
  MachineWork,
  Task,
  TaskChange,
  TaskEntry,
  TaskHistory,
  TaskOffer,
  TaskPage,
} from "./wire";
import { readWork } from "./work";

/** How many of the feed's events a page keeps, to replay onto an answer read while they came. */
const KEPT_EVENTS = 128;
/** A live update about to be drawn keeps the messages, the subtasks and the cards in place. */
const HELD_ROWS = "[id^=message-], [id^=child-], [data-slug]";

/** What a task's page shows, and where each of its reads stands. */
export interface TaskData {
  /** The page the server (or this device) last gave, which is drawn while it is the route's task. */
  page: TaskPage | null;
  /** The slug the server has no task for, as its task route last answered. */
  missing: string | null;
  error: string;
  entries: TaskEntry[];
  threadLoaded: boolean;
  threadRequested: boolean;
  childRequested: boolean;
  /** A task came under the shown one since its subtasks were read: they are read again with the
   * page (`refresh`), as the feed sent its changed fields alone. */
  childCame: boolean;
  before: number | null;
  /** The landing the task offers, with the revision of the task it was offered for. */
  approval: [TaskOffer, number] | null;
  childCursor: string | null;
  /** Counts the feed's events: each kept with its count, for the reads still out (`replay`). */
  generation: number;
  events: [number, unknown][];
  /** How many board events since the page started may have changed the task's gates other than
   * through its own record: a verdict on its thread, a new tip of the branch its gates answer for
   * (`gatesBranch`) and a card in a document it links (`linksCard`). Its gates read again when it
   * grows. */
  gatesNudged: number;
  /** The task and the branch its gates were last answered for (`gatesAnswered`). */
  gatesBranch: [string, string] | null;
  /** The task's page as the server last answered it, with the answer's tag: the page's next read
   * names the tag, and a `304` shows this copy again. */
  pageCopy: [string, TaskPage] | null;
  /** The tag of the thread's newest page this device drew, which its check names. */
  threadTag: string;
  /** The messages drawn from this device, by id, until the server answers the thread. */
  drawnEntries: number[];
  /** Earlier messages were asked for while the thread showed this device's copy: read once the
   * thread is answered, from the cursor its answer gives. */
  olderWaiting: boolean;
  /** Counts the checks of what the page drew (`load`): an older check's retries stop. */
  round: number;
}

/** A task's page before anything is read. */
export function emptyTask(): TaskData {
  return {
    page: null,
    missing: null,
    error: "",
    entries: [],
    threadLoaded: false,
    threadRequested: false,
    childRequested: false,
    childCame: false,
    before: null,
    approval: null,
    childCursor: null,
    generation: 0,
    events: [],
    gatesNudged: 0,
    gatesBranch: null,
    pageCopy: null,
    threadTag: "",
    drawnEntries: [],
    olderWaiting: false,
    round: 0,
  };
}

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

/**
 * The thread once the server answered its newest page with `page`: the entries this device drew
 * (`drawn`, by id) give way to the page's, and every other one shown stays (one the feed brought
 * meanwhile), each message once, in id order.
 */
export function newest(
  entries: readonly TaskEntry[],
  drawnIds: readonly number[],
  page: readonly TaskEntry[],
): TaskEntry[] {
  const kept = entries.filter((entry) => !drawnIds.includes(entry.message.id));
  for (const entry of page) {
    const at = kept.findIndex((shown) => shown.message.id === entry.message.id);
    if (at >= 0) kept[at] = entry;
    else kept.push(entry);
  }
  return kept.sort((a, b) => a.message.id - b.message.id);
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

/** What the live task changes on the page that draws it. */
export interface TaskHost {
  /** The data changed: draw it again. */
  redraw(): void;
  /** What the page shows from this device changed. */
  freshness(f: Freshness): void;
  /** Whether the feed is lost. */
  stale(stale: boolean): void;
}

/** A read's fresh answer, to keep on the device: its tag and its body. */
interface Fresh {
  etag: string;
  value: unknown;
}

/**
 * A task's page as it is kept live: its data, what it shows from this device, and every read and
 * feed event that changes them, for the route's task (`slug`), which a move within the kind changes
 * (`moved`). Nothing it does goes on once `signal` ends.
 */
export class LiveTask {
  readonly d = emptyTask();
  fresh: Freshness = FRESH;
  /** Whether the board was read: the page is drawn under it from then on, as it is once a copy
   * from this device is. */
  boardShown = false;
  /** What this device kept of the first task, drawn: the board's first read waits for it. */
  kept: Promise<void> = Promise.resolve();
  /** How many times the reader pressed Retry, which the board's loop waits on. */
  readonly retry = { current: 0 };
  /** A refresh a feed event asked for is under way, and whether another was asked for meanwhile. */
  private refreshing = false;
  private again = false;
  /** Counts the page's reads as they are asked, and the last of them the server answered: an
   * answer to one asked before it is dropped (`refresh`). */
  private pageAsked = 0;
  private pageAnswered = 0;

  constructor(
    readonly scope: string,
    /** The route's task. */
    public slug: string,
    private readonly host: TaskHost,
    readonly signal: AbortSignal,
  ) {}

  /** The cache item of the task `slug`, as the page's checkout shows it. */
  item(slug: string): Item {
    return taskItem(this.scope, slug);
  }

  /** `path` under the page's checkout. */
  url(path: string): string {
    return scoped(this.scope, path);
  }

  /** Change what the page shows from this device. */
  freshen(change: (f: Freshness) => Freshness): void {
    const next = change(this.fresh);
    if (next === this.fresh) return;
    this.fresh = next;
    this.host.freshness(next);
  }

  /** Whether the page has its task's page to draw: until it has, a failed read is said by the page's
   * own trouble block, not by the notices. */
  drawn(): boolean {
    return this.d.page?.task.slug === this.slug;
  }

  /** Whether `selected` is still the task shown, the page not stopped. */
  private current(selected: string): boolean {
    return !this.signal.aborted && this.slug === selected;
  }

  /** Draw what this device kept of the task, and keep the board live. */
  start(frame: FrameRefresh): Promise<void> {
    this.kept = this.opened();
    return keepBoard(this.scope, null, this.run(frame), this.signal);
  }

  /** What `keepBoard` changes on this page. */
  run(frame: FrameRefresh): BoardRun {
    const board: { current: PageBoard | null } = { current: null };
    return {
      board,
      show: (read) => {
        board.current = read;
        // As the copy this device kept is drawn first: the board's answer waits for it.
        void this.kept.then(() => {
          if (this.signal.aborted || this.boardShown) return;
          this.boardShown = true;
          this.host.redraw();
        });
      },
      setStale: (stale) => this.host.stale(stale),
      setTrouble: (why) => {
        void this.kept.then(() => {
          if (this.signal.aborted) return;
          this.trouble = why;
          this.host.redraw();
        });
      },
      retry: this.retry,
      views: {
        freshen: (change) => this.freshen(change),
        // Nothing for the board's loop to wait for: the feed opens at the board's answer, beside
        // the page's and the thread's reads, whose answers take what it said meanwhile (`replay`).
        read: () => {
          void this.read();
        },
        heard: (value) => this.heard(value),
        frame,
      },
    };
  }

  /** Why the board's last read failed, while nothing is drawn: said in place of the page. */
  trouble: string | null = null;

  /** The board's version was read: what the page drew is checked, and the rest read, while the
   * board's loop opens the feed. */
  async read(): Promise<void> {
    await this.kept;
    if (this.signal.aborted) return;
    await this.load();
  }

  /** The route moved to the task `slug`: drawn from this device, when it kept it, then read. */
  async moved(slug: string): Promise<void> {
    if (slug === this.slug) return;
    this.slug = slug;
    // The task before is not drawn under the new route meanwhile: its ghost is.
    this.host.redraw();
    await this.opened();
    if (!this.current(slug)) return;
    await this.load();
  }

  /** Another task is shown: nothing of the thread, the offer or the subtasks of the one before. */
  private begin(): void {
    const { d } = this;
    d.entries = [];
    d.threadLoaded = false;
    d.approval = null;
    d.threadRequested = false;
    d.childRequested = false;
    d.childCame = false;
    d.childCursor = null;
    d.before = null;
    d.pageCopy = null;
    d.threadTag = "";
    d.drawnEntries = [];
    d.olderWaiting = false;
  }

  /** The route the page left showed what this device kept, unanswered: none of it is shown again
   * before it is drawn or read anew. */
  private leave(): boolean {
    const { d } = this;
    let changed = false;
    if (waits(this.fresh, PAGE)) {
      d.page = null;
      d.pageCopy = null;
      changed = true;
    }
    if (waits(this.fresh, THREAD)) {
      d.entries = [];
      d.threadLoaded = false;
      d.threadRequested = false;
      d.before = null;
      d.drawnEntries = [];
      d.olderWaiting = false;
      changed = true;
    }
    this.freshen((f) => left(f, [PAGE, THREAD]));
    return changed;
  }

  /**
   * Open the route's task: one more visit to it, and what this device kept of it drawn at once
   * unless the page shows the task already: its page and, kept with it, its thread's newest page.
   * `load` checks each once the board's version is read. What this device drew for the route before
   * goes, unless answered.
   */
  async opened(): Promise<void> {
    const selected = this.slug;
    if (this.leave()) this.host.redraw();
    const item = this.item(selected);
    visitLater(item);
    const page = await readKept(item, PAGE, readTaskPage);
    // Stopped or moved on while the store answered: nothing more is read of it, nor forgotten.
    if (page === null || !this.current(selected)) return;
    const thread = await readKept(item, THREAD, readHistory);
    if (!this.current(selected) || this.drawn() || page.value.task.slug !== selected) return;
    const { d } = this;
    this.begin();
    d.pageCopy = [page.etag, page.value];
    d.page = page.value;
    d.missing = null;
    d.error = "";
    this.freshen((f) => drawn(f, PAGE, page.saved));
    if (thread !== null) {
      d.drawnEntries = thread.value.entries.map((entry) => entry.message.id);
      d.entries = thread.value.entries;
      d.before = thread.value.before;
      d.threadLoaded = true;
      d.threadRequested = true;
      d.threadTag = thread.etag;
      this.freshen((f) => drawn(f, THREAD, thread.saved));
    }
    this.host.redraw();
  }

  /**
   * Read with `read` and, while the page still waits for `part` (`unanswered`), again, waiting
   * longer after each failure (`tryAgain`), until it is answered, the route changes or a newer
   * check starts (`round`). Settles once the first read is done.
   */
  private async answer(part: string, round: number, read: () => Promise<void>): Promise<void> {
    const selected = this.slug;
    const current = () => this.current(selected) && this.d.round === round && this.unanswered(part);
    await read();
    void tryAgain(current, read, this.signal);
  }

  /**
   * Whether the page still waits for `part`'s answer: it shows this device's copy unanswered, or,
   * for the page, it shows none and the server did not say it has no such task (its read failed).
   */
  private unanswered(part: string): boolean {
    if (waits(this.fresh, part)) return true;
    return part === PAGE && !this.drawn() && this.d.missing !== this.slug;
  }

  /** Keep the branch the gates of `slug` were answered for: the moved branches the feed names are
   * matched against it. */
  gatesAnswered(slug: string, branch: string): void {
    const held = this.d.gatesBranch;
    if (held === null || held[0] !== slug || held[1] !== branch)
      this.d.gatesBranch = [slug, branch];
  }

  /**
   * Read the route's task: its page, naming the copy the page holds, and the thread and the
   * subtasks the page asked for. The page and the thread this device drew are checked at once, each
   * asked again until it answers; a thread read before is caught up from its newest message once
   * the page is.
   */
  async load(): Promise<void> {
    const selected = this.slug;
    const { d } = this;
    const same = d.page?.task.slug === selected;
    const catchUp = d.threadLoaded && same;
    // The newest message shown before any read: the feed, open beside the reads, brings newer
    // ones meanwhile, and those between are the catch-up's to read.
    const newestShown = d.entries.reduce((most, entry) => Math.max(most, entry.message.id), 0);
    // What a page drawn from this device was asked to read waits for its page's answer (`refresh`).
    const waiting = waits(this.fresh, PAGE);
    const requested = d.threadRequested && same && !waiting;
    const childrenRequested = d.childRequested && same && !waiting;
    d.round += 1;
    const round = d.round;
    if (same && waits(this.fresh, THREAD)) {
      await Promise.all([
        this.answer(PAGE, round, () => this.refresh()),
        this.answer(THREAD, round, () => this.checkThread()),
      ]);
    } else {
      await this.answer(PAGE, round, () => this.refresh());
      if (!this.current(selected)) return;
      if (catchUp) await this.catchUp(newestShown);
      else if (requested) await this.history(false);
    }
    if (!this.current(selected)) return;
    const page = d.page;
    if (
      childrenRequested &&
      page?.task.slug === selected &&
      page.children.length === 0 &&
      page.children_count > 0
    ) {
      await this.children(false);
    }
  }

  /** One feed event: kept for the reads still out, applied, and the page read again when it asks
   * for it. False for one the page cannot read, which loses the stream. */
  heard(value: unknown): boolean {
    holdPlace(HELD_ROWS);
    const { d } = this;
    d.generation += 1;
    d.events.push([d.generation, value]);
    if (d.events.length > KEPT_EVENTS) d.events.shift();
    const reload = this.apply(value);
    if (reload === null) return false;
    if (reload) void this.refreshAfter();
    return true;
  }

  /** Read the page again, then again for each event that asked for it while it was read. */
  private async refreshAfter(): Promise<void> {
    if (this.refreshing) {
      this.again = true;
      return;
    }
    this.refreshing = true;
    try {
      do {
        this.again = false;
        await this.refresh();
      } while (this.again && !this.signal.aborted);
    } finally {
      this.refreshing = false;
    }
  }

  /** The events heard since `since`, applied again to what a read answered; the page read again
   * when one asks for it, and read whole when one could not be read or was dropped. */
  private replay(since: number): void {
    const { d } = this;
    const first = d.events[0];
    let lost = first !== undefined && since + 1 < first[0];
    let needed = false;
    for (const [generation, value] of d.events) {
      if (generation <= since) continue;
      const reload = this.apply(value);
      if (reload === null) lost = true;
      else needed ||= reload;
    }
    if (lost) void this.load();
    // After the refresh under way, as one the feed asked for: the events it replays were heard
    // while it was read, and each asked for one already.
    else if (needed) void this.refreshAfter();
  }

  /**
   * Apply feed event `value` to what the page shows: whether it asks the page to be read again;
   * null for one the page cannot read, which changes nothing.
   */
  apply(value: unknown): boolean | null {
    const changes = readChanges(value);
    if (changes === null) return null;
    const { d } = this;
    const before = d.page;
    let changed = false;
    let reloadDetail = false;
    let reloadPage = false;
    for (const change of changes) {
      const page = d.page;
      switch (change.region) {
        case "task": {
          const item = change.change;
          const task = item.value ?? null;
          const belongs = task !== null && page !== null && task.parent === page.task.slug;
          const existed = page?.children.some((child) => child.slug === item.key) ?? false;
          if (page !== null && (existed || belongs)) {
            d.page = {
              ...page,
              children:
                item.removed || !belongs
                  ? page.children.filter((child) => child.slug !== item.key)
                  : upsert(page.children, item),
            };
            changed = true;
            reloadDetail ||= !existed || item.removed || !belongs;
          }
          if (d.page?.task.slug === item.key) {
            if (item.removed) {
              d.page = null;
              changed = true;
              if (this.slug === item.key) {
                d.missing = item.key;
                d.error = "This task was removed";
              }
            } else reloadDetail = true;
          }
          // A task filed under the slug the page could not find: read it now.
          reloadPage ||= !item.removed && d.missing === item.key && this.slug === item.key;
          break;
        }
        // The feed sends a task it changed as these fields alone.
        case "task-fields": {
          if (page === null) break;
          const [next, done] = pageFields(page, change.change);
          if (next !== page) {
            d.page = next;
            changed = true;
          }
          reloadDetail ||= done.reload;
          d.childCame ||= done.childCame;
          break;
        }
        // A holder's health: its roles on the page's task and its subtasks say it.
        case "session": {
          if (page === null) break;
          const [moved, tasks] = holderHealth(
            [page.task, ...page.children],
            change.key,
            change.health,
          );
          if (moved) {
            const [task, ...children] = tasks as [Task, ...Task[]];
            d.page = { ...page, task, children };
            changed = true;
          }
          break;
        }
        case "message": {
          const { entry } = change;
          if (
            page?.task.slug === entry.message.slug &&
            !d.entries.some((shown) => shown.message.id === entry.message.id)
          ) {
            if (entry.message.kind === "verdict") d.gatesNudged += 1;
            d.entries = [...d.entries, entry];
            changed = true;
          }
          break;
        }
        // A card that came, changed or went: the owner gate of a task that links its document.
        case "pending":
          if (change.key !== null && page !== null && linksCard(page.task.links, change.key)) {
            d.gatesNudged += 1;
            changed = true;
          }
          break;
        case "branches":
          if (page !== null && change.moved.includes(gatesBranch(page.task.slug, d.gatesBranch))) {
            d.gatesNudged += 1;
            changed = true;
          }
          break;
        case "machine-work":
          if (page !== null) {
            d.page = { ...page, machine_work: change.work };
            changed = true;
          }
          break;
        case "tasks-status": {
          reloadPage ||= change.available === true;
          const notice = boardNotice(change.available, change.slow);
          if (notice !== d.error) {
            d.error = notice;
            changed = true;
          }
          break;
        }
      }
    }
    if (changed) this.host.redraw();
    return reloadPage || (reloadDetail && before?.task.slug === this.slug);
  }

  /**
   * The page of `slug`, naming the `copy` the page holds (with its tag) when it holds one: none when
   * the server has no such task, or answers another's; else the page and, when the server sent one
   * rather than a `304`,
   * the tag and body to keep.
   */
  async readPage(
    slug: string,
    copy: [string, TaskPage] | null,
  ): Promise<[TaskPage, Fresh | null] | null> {
    let answer: Awaited<ReturnType<typeof getIf>>;
    try {
      answer = await getIf(
        this.url(`/api/task-page?slug=${encodeURIComponent(slug)}`),
        copy?.[0] ?? "",
        this.signal,
      );
    } catch (error) {
      if (error instanceof Failure && error.kind === "missing") return null;
      throw error;
    }
    if (answer.same) {
      if (copy === null) throw new Error("The server answered 304 to a plain read");
      return [copy[1], null];
    }
    const page = readTaskPage(answer.value);
    if (page === null) throw new Error("The task's page is not readable");
    // A page answered under another name is not this task's: the server has none by this one, and
    // the page neither draws it nor asks again.
    if (page.task.slug !== slug) return null;
    return [page, { etag: answer.etag, value: answer.value }];
  }

  /**
   * Read the route's page again, naming the copy the page holds, so that a `304` shows it again and
   * a `200` replaces it, on the page and on this device. Two reads may be out together (a feed lost
   * while one is out starts the next round's): an answer, or a failure, that comes after the answer
   * to a read asked later changes nothing.
   */
  async refresh(): Promise<void> {
    const selected = this.slug;
    const { d } = this;
    const since = d.generation;
    const copy = d.pageCopy !== null && d.pageCopy[1].task.slug === selected ? d.pageCopy : null;
    this.pageAsked += 1;
    const asked = this.pageAsked;
    const superseded = () => asked < this.pageAnswered;
    let read: [TaskPage, Fresh | null] | null;
    try {
      read = await this.readPage(selected, copy);
    } catch (error) {
      if (!this.current(selected) || superseded()) return;
      this.freshen((f) => failed(f, PAGE));
      // A page drawn from this device says the server is not reachable instead.
      if (shown(this.fresh) === null) d.error = said(error);
      this.host.redraw();
      return;
    }
    if (!this.current(selected) || superseded()) return;
    this.pageAnswered = asked;
    if (read === null) {
      d.missing = selected;
      d.error = "";
      // The task is gone, and so is what this device drew and kept of it.
      if (d.page?.task.slug === selected) {
        d.page = null;
        this.begin();
      }
      this.freshen((f) => answered(answered(f, PAGE), THREAD));
      void forget(this.item(selected));
      this.replay(since);
      this.host.redraw();
      return;
    }
    const [answeredPage, fresh] = read;
    let page = answeredPage;
    const fromDevice = waits(this.fresh, PAGE);
    if (d.page?.task.slug !== page.task.slug) this.begin();
    else page = { ...page, children: d.page.children };
    if (fresh !== null) {
      keepLater(this.item(selected), PAGE, fresh.etag, fresh.value);
      d.pageCopy = [fresh.etag, page];
    }
    d.page = page;
    d.missing = null;
    d.error = "";
    this.freshen((f) => answered(f, PAGE));
    // What a page drawn from this device was asked to read meanwhile, it reads now.
    const thread = fromDevice && d.threadRequested && !d.threadLoaded;
    const came = d.childCame;
    d.childCame = false;
    const subtasks =
      d.childRequested &&
      (came || (fromDevice && page.children.length === 0 && page.children_count > 0));
    this.replay(since);
    this.host.redraw();
    if (thread) void this.history(false);
    if (subtasks) void this.children(false);
  }

  /** The thread's newest page, `path`, read fresh: the page and the tag and body to keep. */
  private async readNewest(path: string): Promise<[TaskHistory, Fresh]> {
    const answer = await getIf(this.url(path), "", this.signal);
    if (answer.same) throw new Error("The server answered 304 to a plain read");
    const page = readHistory(answer.value);
    if (page === null) throw new Error("The task's thread is not readable");
    return [page, { etag: answer.etag, value: answer.value }];
  }

  /**
   * Ask whether the thread's newest page this device drew changed, naming its tag: a `304` confirms
   * it, and a `200` replaces what the device drew (`newest`), on the page and on the device.
   * Earlier messages asked for meanwhile are read then.
   */
  async checkThread(): Promise<void> {
    const selected = this.slug;
    const { d } = this;
    let read: [TaskHistory, Fresh] | null;
    try {
      const answer = await getIf(
        this.url(`/api/task-history?slug=${encodeURIComponent(selected)}`),
        d.threadTag,
        this.signal,
      );
      if (answer.same) read = null;
      else {
        const page = readHistory(answer.value);
        if (page === null) throw new Error("The task's thread is not readable");
        read = [page, { etag: answer.etag, value: answer.value }];
      }
    } catch (error) {
      if (!this.current(selected) || !waits(this.fresh, THREAD)) return;
      this.freshen((f) => failed(f, THREAD));
      if (shown(this.fresh) === null) d.error = said(error);
      this.host.redraw();
      return;
    }
    if (!this.current(selected) || !waits(this.fresh, THREAD)) return;
    const drawnIds = d.drawnEntries;
    d.drawnEntries = [];
    if (read !== null) {
      const [page, fresh] = read;
      d.entries = newest(d.entries, drawnIds, page.entries);
      d.before = page.before;
      keepLater(this.item(selected), THREAD, fresh.etag, fresh.value);
      d.threadTag = fresh.etag;
    }
    this.freshen((f) => answered(f, THREAD));
    const older = d.olderWaiting;
    d.olderWaiting = false;
    this.host.redraw();
    if (older) void this.history(true);
  }

  /**
   * Read the thread: its newest page (kept on this device), or the page before the earliest shown
   * (`older`, never kept). The newest is read once; an earlier one waits while the thread shows this
   * device's copy, as its cursor may be another.
   */
  async history(older: boolean): Promise<void> {
    const selected = this.slug;
    const { d } = this;
    if (!older && d.threadLoaded) return;
    if (older && waits(this.fresh, THREAD)) {
      d.olderWaiting = true;
      return;
    }
    d.threadRequested = true;
    // A page drawn from this device reads nothing before the board's version is read and its own
    // check answered: the thread is read then (`refresh`), and the feed replays what follows.
    if (!older && waits(this.fresh, PAGE)) return;
    const before = older ? d.before : null;
    if (older && before === null) return;
    const cursor = before === null ? "" : `&before=${before}`;
    const path = `/api/task-history?slug=${encodeURIComponent(selected)}${cursor}`;
    let read: [TaskHistory, Fresh | null];
    try {
      if (older) {
        const page = readHistory(await getJson(this.url(path), this.signal));
        if (page === null) throw new Error("The task's thread is not readable");
        read = [page, null];
      } else read = await this.readNewest(path);
    } catch (error) {
      if (!this.current(selected)) return;
      d.error = said(error);
      this.host.redraw();
      return;
    }
    if (!this.current(selected)) return;
    const [page, fresh] = read;
    if (fresh !== null) {
      keepLater(this.item(selected), THREAD, fresh.etag, fresh.value);
      d.threadTag = fresh.etag;
    }
    d.entries = withEntries(d.entries, page.entries);
    d.before = page.before;
    d.threadLoaded = true;
    this.host.redraw();
  }

  /** The messages after message `after`, a page at a time, until the server has no more. */
  private async catchUp(after: number): Promise<void> {
    const selected = this.slug;
    const { d } = this;
    for (;;) {
      let page: TaskHistory | null;
      try {
        const path = `/api/task-history?slug=${encodeURIComponent(selected)}&after=${after}`;
        page = readHistory(await getJson(this.url(path), this.signal));
        if (page === null) throw new Error("The task's thread is not readable");
      } catch (error) {
        if (this.current(selected)) {
          d.error = said(error);
          this.host.redraw();
        }
        return;
      }
      if (!this.current(selected)) return;
      d.entries = withEntries(d.entries, page.entries);
      this.host.redraw();
      if (page.after === null || page.after <= after) return;
      after = page.after;
    }
  }

  /** Read the task's subtasks: their first page, or the next (`more`). As the thread, read once the
   * page drawn from this device is answered (`refresh`). */
  async children(more: boolean): Promise<void> {
    const selected = this.slug;
    const { d } = this;
    d.childRequested = true;
    if (waits(this.fresh, PAGE)) return;
    const cursor = more ? d.childCursor : null;
    const after = cursor === null ? "" : `&cursor=${encodeURIComponent(cursor)}`;
    const path = `/api/task-board?parent=${encodeURIComponent(selected)}&older=1${after}`;
    let listed: ReturnType<typeof readListing>;
    try {
      listed = readListing(await getJson(this.url(path), this.signal));
      if (listed === null) throw new Error("The task board's answer is not a listing");
    } catch (error) {
      if (!this.current(selected)) return;
      d.error = said(error);
      this.host.redraw();
      return;
    }
    if (!this.current(selected)) return;
    d.childCursor = listed.next ?? null;
    const page = d.page;
    if (page?.task.slug === selected) {
      let children = more ? page.children : [];
      for (const task of listed.tasks) {
        children = upsert(children, { key: task.slug, value: task, removed: false });
      }
      const count = listed.counts.reduce((sum, [, n]) => sum + n, 0);
      d.page = { ...page, children, children_count: count };
    }
    this.host.redraw();
  }

  /** Read the landing the task offers, kept for the revision of the task it was asked at, and only
   * when it has a payload to post. */
  async offer(): Promise<void> {
    const selected = this.slug;
    const { d } = this;
    const asked = d.page;
    if (asked?.task.slug !== selected) return;
    const revision = asked.task.updated;
    let offer: TaskOffer | null;
    try {
      const path = `/api/task-offer?slug=${encodeURIComponent(selected)}`;
      offer = readOffer(await getJson(this.url(path), this.signal));
      if (offer === null) throw new Error("The task's offer is not readable");
    } catch (error) {
      if (!this.current(selected)) return;
      d.error = said(error);
      this.host.redraw();
      return;
    }
    if (!this.current(selected) || d.page?.task.updated !== revision) return;
    d.approval = offer.payload !== null ? [offer, revision] : null;
    this.host.redraw();
  }
}
