// The task board, live (`/tasks`): it reads summaries and counts, never a task's brief, thread or
// documents. What this device kept of the default view is drawn first; the board's version is read
// (live.tsx's `keepBoard`, which also follows the feed and starts again when the stream is lost),
// then the counts, the machines' work and each column drawn from the device are checked, and each
// column reads its first page once it comes near the screen.

import { useEffect, useReducer, useRef, useState } from "react";
import { flushSync } from "react-dom";
import {
  boardShape,
  GhostBoard,
  keepShape,
  type OnLoad,
  PAGE,
  recallGhost,
  SHAPE,
  TaskBoard,
  UnreadBoard,
} from "./board";
import { readerRows } from "./boardplace";
import { waitingStage } from "./boardwait";
import {
  answered,
  BOARD,
  boardItem,
  COUNTS,
  column as columnPart,
  drawn,
  FRESH,
  type Freshness,
  failed,
  type Item,
  keepLater,
  columns as keptColumns,
  keptView,
  left,
  readKept,
  shown,
  visitLater,
  WORK,
} from "./cache";
import { type FrameRefresh, useFrame } from "./frame";
import { type Journal, ReadCursor } from "./journal";
import { type BoardRun, keepBoard, type PageBoard, PageNotices, Retry } from "./live";
import { scoped } from "./project";
import { getIf, getJson, said } from "./request";
import { field, holdPlace, list as items } from "./shared";
import { tryAgain } from "./state";
import { load, save } from "./storage";
import {
  holderHealth,
  isTask,
  merge,
  readItemChange,
  readListing,
  readSlugs,
  readTaskChange,
} from "./task";
import { markTasksReady } from "./taskcard";
import { Debounce } from "./timing";
import { failed as failedRead } from "./trouble";
import type {
  ItemChange,
  MachineWork,
  Orchestrator,
  Task,
  TaskChange,
  TaskListing,
  WaitingBoard,
} from "./wire";
import { PAGE_NAMES } from "./wire";
import { statusNotice } from "./words";
import { drawsAlike, NO_WORK, placesAlike, readWork } from "./work";

/** How long a search, a window or a column waits for the reader's next key before it reads. */
const TYPING_MS = 250;
/** How long a refresh a feed event asked for waits before it reads the counts again. */
const REFRESH_MS = 750;

/** A read's copy: the tag the server answered it with, and the listing. */
interface Copy {
  etag: string;
  list: TaskListing;
}

/** What the page shows of the board, and where each of its reads stands. */
export interface BoardData {
  tasks: Task[];
  work: MachineWork;
  counts: [string, number][];
  lifecycleCounts: [string, number][];
  connected: boolean;
  statuses: string[];
  signoff: string[];
  query: string;
  wait: string;
  projection: WaitingBoard | null;
  orchestrators: Orchestrator[];
  unstarted: number;
  days: number;
  ready: boolean;
  error: string;
  cursors: Map<string, string>;
  pendingMore: string[];
  requests: Map<string, number>;
  countRequest: number;
  /**
   * Moves with every change the page reads again for: a new view, or a feed event that asks for a
   * refresh. A refresh whose counts answered at the generation it started at reads the columns;
   * one that did not reads the counts again.
   */
  generation: number;
  /**
   * Which view the page reads: its search, its window, the connection that places its stages and
   * the board's version its feed follows. An answer for another view is dropped, as is one a newer
   * read of the same counts or column replaced. One a feed event came during is drawn, the events
   * since its read began replayed onto it, and the refresh the event asked for reads it again.
   */
  view: number;
  journal: Journal;
  /** The copy of each of the default view's reads the page holds, by its part of the board's item
   * (`counts`, `column:ready`): the next read of it names the copy's tag, and a `304` is that copy. */
  copies: Map<string, Copy>;
  /** The last answer of each column's long read (`rows`, `readColumn`), by its part: the next long
   * read of the column names its tag, and a `304` is that answer. Held on the page alone, never on
   * the device. */
  long: Map<string, Copy>;
  /** The columns drawn from this device's cache the server has not answered yet. */
  drawn: string[];
  /** Each column checked once the board's version was read, by the generation it was checked at:
   * its own read at that generation would ask again. */
  checked: Map<string, number>;
  /** The board's version is not read yet: no column is read before it, as the feed replays only
   * what changed after it. */
  waiting: boolean;
  /** The tasks drawn from this device no fresh answer has listed yet: once every column drawn from
   * the device has answered, those left are gone from the server's board. */
  fromDevice: string[];
  /** Which check of the page's reads is current (`check`): a check's reads stop asking again once
   * another has started. */
  round: number;
  /** How many times the counts were answered, and the machines' work (by its read or by the feed's
   * `machine-work` event): a read that failed is asked again until one is. */
  counted: number;
  worked: number;
}

/** The board before anything is read. */
export function emptyBoard(): BoardData {
  return {
    tasks: [],
    work: NO_WORK,
    counts: [],
    lifecycleCounts: [],
    connected: true,
    statuses: [],
    signoff: [],
    query: "",
    wait: "",
    projection: null,
    orchestrators: [],
    unstarted: 0,
    days: 1,
    ready: false,
    error: "",
    cursors: new Map(),
    pendingMore: [],
    requests: new Map(),
    countRequest: 0,
    generation: 0,
    view: 0,
    journal: { version: 0, events: [], readers: new Map() },
    copies: new Map(),
    long: new Map(),
    drawn: [],
    checked: new Map(),
    waiting: true,
    fromDevice: [],
    round: 0,
    counted: 0,
    worked: 0,
  };
}

/** One change of a feed event, as the board reads it. */
type Change =
  | { region: "machine-work"; work: MachineWork }
  | { region: "task"; change: ItemChange<Task> }
  | { region: "task-fields"; change: TaskChange }
  | { region: "session"; key: string; health: string | null }
  | { region: "message" }
  | { region: "task-signoff"; slugs: string[] | null }
  | { region: "counts" }
  | { region: "tasks-status"; available: boolean | undefined; slow: boolean };

/**
 * The changes of feed event `value` the board follows, read whole before any is applied; none when
 * one cannot be read, which loses the stream. While `searching`, the owner's sign-off is read
 * again rather than taken from the event.
 */
export function readChanges(value: unknown, searching: boolean): Change[] | null {
  const read: Change[] = [];
  for (const change of items(field(value, "changes"))) {
    const data = field(change, "data");
    const region = field(change, "region");
    if (region === "machine-work") {
      const work = readWork(data);
      if (work === null) return null;
      read.push({ region, work });
    } else if (region === "task") {
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
      read.push({ region });
    } else if (region === "task-signoff") {
      const slugs = searching ? null : readSlugs(data);
      if (!searching && slugs === null) return null;
      read.push({ region, slugs });
    } else if (["task-counts", "task-recent-counts", "workflow"].includes(region as string)) {
      // A workflow activated: the counts say its generation, by which the board reads it.
      read.push({ region: "counts" });
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

/** `tasks` with `change` made: a task removed, replaced or added last. A task read again as it
 * stands keeps `tasks` as they are, so nothing is drawn again. */
export function upsert(tasks: Task[], change: ItemChange<Task>): Task[] {
  if (change.removed) return tasks.filter((task) => task.slug !== change.key);
  const task = change.value;
  if (task === null || task === undefined) return tasks;
  const at = tasks.findIndex((one) => one.slug === change.key);
  if (at < 0) return [...tasks, task];
  const before = tasks[at] as Task;
  if (sameTask(before, task)) return tasks;
  const next = [...tasks];
  next[at] = task;
  return next;
}

/** Whether two reads of a task say the same. */
export function sameTask(a: Task, b: Task): boolean {
  return a === b || JSON.stringify(a) === JSON.stringify(b);
}

/**
 * `tasks` with each of `listed` in its place, or last when new. A task read as `before` held it
 * keeps that object, so its card is not drawn again.
 */
export function withListed(
  tasks: Task[],
  listed: Task[],
  before: ReadonlyMap<string, Task>,
): Task[] {
  const next = [...tasks];
  const at = new Map(next.map((task, index) => [task.slug, index]));
  for (const task of listed) {
    const old = before.get(task.slug);
    const kept = old !== undefined && sameTask(old, task) ? old : task;
    const index = at.get(task.slug);
    if (index === undefined) {
      at.set(task.slug, next.length);
      next.push(kept);
    } else next[index] = kept;
  }
  return next;
}

/** What the live board changes on the page that draws it. */
export interface BoardHost {
  /** The data changed: draw it again. */
  redraw(): void;
  /** The board was read again: each column shown reads its first page again, as long as the rows
   * it shows (`readColumn`). */
  refresh(): void;
  /** What the page shows from this device changed. */
  freshness(f: Freshness): void;
  /** Whether the feed is lost. */
  stale(stale: boolean): void;
}

/**
 * The board as the page keeps it live: its data, what it shows from this device, and every read
 * and feed event that changes them. Nothing it does goes on once `signal` ends.
 */
export class LiveBoard {
  readonly d = emptyBoard();
  fresh: Freshness = FRESH;
  readonly item: Item;
  /** What this device kept, drawn: the board's first read waits for it. */
  kept: Promise<void> = Promise.resolve();
  /** A refresh a feed event asked for is under way. */
  private pending = false;
  /** What a failed read last had the page say (`d.error`): a column's answer takes that away, and
   * never the board's own notice of its state. */
  private said = "";
  /** How many times the counts were answered (`d.counted`) when the last check began: while they
   * still are, the check asks for them again until one answers, whoever's read failed. */
  private uncounted: number | null = null;
  /** How many times the reader pressed Retry, which the board's loop waits on. */
  readonly retry = new Retry();
  /** A column's read, one function for the page's life: a column drawn again for it would be. */
  readonly load: OnLoad = (status, more, query, days) => {
    void this.column(status, more, query, days);
  };
  /** A search or window chosen, one function for the page's life as `load`. */
  readonly search = (query: string, days: number, wait = ""): void => {
    void this.filter(query, days, wait);
  };

  /** The rests between a search's keys: the last reads its counts. */
  private readonly typing: Debounce;
  /** Each column's rests between the asks for its pages: the last reads. */
  private readonly columns = new Map<string, Debounce>();
  /** The rest a refresh a feed event asked for takes before it reads. */
  private readonly refreshes: Debounce;

  constructor(
    readonly scope: string,
    private readonly host: BoardHost,
    readonly signal: AbortSignal,
    private readonly now: () => number = Date.now,
  ) {
    this.item = { ...boardItem(scope), id: `${boardItem(scope).id}waiting` };
    this.typing = new Debounce(TYPING_MS, signal);
    this.refreshes = new Debounce(REFRESH_MS, signal);
  }

  /** Change what the page shows from this device. */
  freshen(change: (f: Freshness) => Freshness): void {
    this.fresh = change(this.fresh);
    this.host.freshness(this.fresh);
  }

  /** Visit the board's item, draw what this device kept, and keep the board live. */
  start(frame: FrameRefresh): Promise<void> {
    visitLater(this.item);
    this.kept = this.drawKept();
    return keepBoard(this.scope, null, this.run(frame), this.signal);
  }

  /** What `keepBoard` changes on this page. */
  run(frame: FrameRefresh): BoardRun {
    const board: { current: PageBoard | null } = { current: null };
    return {
      board,
      show: (read) => {
        board.current = read;
      },
      setStale: (stale) => this.setStale(stale),
      setTrouble: (why) => {
        void this.kept.then(() => {
          if (!this.signal.aborted) this.unanswered(BOARD, why);
        });
      },
      retry: this.retry,
      views: {
        freshen: (change) => this.freshen(change),
        read: () => this.read(),
        heard: (value) => this.heard(value),
        frame,
      },
    };
  }

  /**
   * Draw what this device kept of the default view before the server answers: the counts, the
   * machines' work that placed them and each column's first page. A column reads nothing before
   * the board's version is read (`waiting`), and its copy is checked then.
   */
  async drawKept(): Promise<void> {
    const counts = await readKept(this.item, COUNTS, readListing);
    if (counts === null || this.signal.aborted) return;
    const work = await readKept(this.item, WORK, readWork);
    const read: [string, { value: TaskListing; etag: string; saved: number }][] = [];
    for (const status of keptColumns(counts.value.statuses, counts.value.counts)) {
      const copy = await readKept(this.item, columnPart(status), readListing);
      if (copy !== null) read.push([status, copy]);
    }
    if (this.signal.aborted) return;
    const { d } = this;
    if (work !== null) {
      d.work = work.value;
      this.freshen((f) => drawn(f, WORK, work.saved));
    }
    d.copies.set(COUNTS, { etag: counts.etag, list: counts.value });
    this.showCounts(counts.value);
    this.freshen((f) => drawn(f, COUNTS, counts.saved));
    for (const [status, copy] of read) {
      const part = columnPart(status);
      d.fromDevice.push(...copy.value.tasks.map((task) => task.slug));
      d.copies.set(part, { etag: copy.etag, list: copy.value });
      this.showColumn(status, false, copy.value);
      this.freshen((f) => drawn(f, part, copy.saved));
      d.drawn.push(status);
    }
    if (d.drawn.length > 0) markTasksReady();
    this.host.redraw();
  }

  /** A listing's complete totals: its stages' counts, the statuses and the owner's sign-off. */
  showCounts(list: TaskListing): void {
    const { d } = this;
    d.counts = list.counts;
    d.lifecycleCounts = list.lifecycle_counts;
    d.statuses = list.statuses;
    d.signoff = list.signoff;
    d.projection = list.waiting ?? null;
    d.orchestrators = list.orchestrators;
    d.unstarted = list.unstarted;
    d.ready = true;
  }

  /** A page of `status`'s column, which replaces the column's tasks when it is the first (`more`
   * false). Whether the column has another. */
  showColumn(status: string, more: boolean, list: TaskListing): boolean {
    const { d } = this;
    const next = list.next ?? null;
    if (next !== null) d.cursors.set(status, next);
    else d.cursors.delete(status);
    const before = new Map(d.tasks.map((task) => [task.slug, task]));
    if (!more)
      d.tasks =
        status === "waiting"
          ? []
          : d.tasks.filter((task) => waitingStage(d.work, task, d.connected) !== status);
    d.tasks = withListed(d.tasks, list.tasks, before);
    d.counts = list.counts;
    d.lifecycleCounts = list.lifecycle_counts;
    d.signoff = list.signoff;
    d.projection = list.waiting ?? null;
    d.orchestrators = list.orchestrators;
    d.unstarted = list.unstarted;
    return next !== null;
  }

  /**
   * Read a listing at `path` under the page's project. With the `copy` the page holds, the read
   * names its tag and a `304` is that copy; otherwise the server's body, with the tag and value to
   * keep.
   */
  async readListing(
    path: string,
    copy: Copy | undefined,
  ): Promise<[TaskListing, { etag: string; value: unknown } | null]> {
    const answer = await getIf(scoped(this.scope, path), copy?.etag ?? "", this.signal);
    if (answer.same) {
      if (copy === undefined) throw new Error("The server answered 304 to a plain read");
      return [copy.list, null];
    }
    const list = readListing(answer.value);
    if (list === null) throw new Error("The task board's answer is not a listing");
    return [list, { etag: answer.etag, value: answer.value }];
  }

  /** The read of `part` failed: a page drawn from this device says whether the server is
   * reachable, any other why. */
  unanswered(part: string, why: string): void {
    this.freshen((f) => failed(f, part));
    if (shown(this.fresh) === null) {
      this.d.error = why;
      this.said = why;
      this.host.redraw();
    }
  }

  /**
   * The read of `part` was answered: what a failed read said goes once none fails any more, never
   * the board's own notice of its state, which no read said. Whether it went, to be drawn.
   */
  answeredRead(part: string): boolean {
    this.freshen((f) => answered(f, part));
    const { d } = this;
    if (this.fresh.failing.length > 0 || d.error === "" || d.error !== this.said) return false;
    d.error = "";
    return true;
  }

  /** Keep a fresh answer of the default view as `part`'s copy, on the page and on this device. */
  keep(part: string, list: TaskListing, fresh: { etag: string; value: unknown } | null): void {
    if (fresh === null) return;
    this.d.copies.set(part, { etag: fresh.etag, list });
    keepLater(this.item, part, fresh.etag, fresh.value);
  }

  /**
   * The board's listings leave the machines' work out (`work=0`): every recorder's heartbeat moves
   * it, so a listing with it would never be answered `304`. The page reads it here once the
   * board's version is read, and follows the feed's `machine-work` events after; an event applied
   * while the read was out is newer than its answer. Only the check reads it, which tries a failed
   * read again until the work is answered, so a failure says it tries again; one after the feed's
   * event answered for it says nothing, as nothing tries it again. Once the work is answered, by
   * the next try or by the feed's event (`heard`), what the failure said goes (`answeredRead`).
   */
  async machineWork(): Promise<void> {
    const { d } = this;
    const worked = d.worked;
    let value: unknown;
    let work: MachineWork | null = null;
    let why = "";
    try {
      value = await getJson(scoped(this.scope, "/api/machine-work"), this.signal);
      work = readWork(value);
      if (work === null) why = "The machines' work is not readable";
    } catch (error) {
      why = said(error);
    }
    if (this.signal.aborted) return;
    if (work === null) {
      if (d.worked === worked) this.unanswered(WORK, failedRead(why, true));
      return;
    }
    if (d.worked === worked) {
      d.work = work;
      d.worked += 1;
      this.rememberShape();
      keepLater(this.item, WORK, "", value);
      this.host.redraw();
    }
    if (this.answeredRead(WORK)) this.host.redraw();
  }

  /**
   * Keep the default view's columns and counts as the board draws them, for the next first visit's
   * ghost (SHAPE): once both the counts and the machines' work that places its build and test
   * columns are in, whichever comes last, and again at each answer of the counts or of a column's
   * first page, whose complete counts may have changed since the counts read.
   */
  rememberShape(): void {
    const { d } = this;
    if (
      d.counted === 0 ||
      d.worked === 0 ||
      d.wait !== "" ||
      !keptView(d.query, d.days, d.connected ? 1 : 0)
    ) {
      return;
    }
    const shape = boardShape(d.counts);
    save(SHAPE, keepShape(shape, Boolean(d.projection?.workflow)));
  }

  /**
   * Read the counts of the page's view; an answer a newer view or count read replaced is dropped.
   * A failure says why, and that the counts are tried again while they are: until the last check
   * has its counts, it asks again (`uncounted`), whichever read failed (its own, a refresh's, a
   * search's, the feed's open); after, nothing does until the next event or check.
   */
  async counts(): Promise<void> {
    const { d } = this;
    d.countRequest += 1;
    const request = d.countRequest;
    const { query, days, view } = d;
    const activity = d.connected ? 1 : 0;
    const kept = d.wait === "" && keptView(query, days, activity);
    const since = new ReadCursor(d.journal);
    try {
      const path = `/api/task-board?view=waiting${d.wait ? `&wait=${d.wait}` : ""}&counts=1&q=${encodeURIComponent(query)}&days=${days}&activity=${activity}&work=0`;
      let read: Awaited<ReturnType<LiveBoard["readListing"]>> | null = null;
      let why = "";
      try {
        read = await this.readListing(path, kept ? d.copies.get(COUNTS) : undefined);
      } catch (error) {
        why = said(error);
      }
      if (this.signal.aborted || d.view !== view || d.countRequest !== request) return;
      if (read === null) {
        this.unanswered(COUNTS, this.uncounted === d.counted ? failedRead(why, true) : why);
        return;
      }
      const [list, fresh] = read;
      if (kept) this.keep(COUNTS, list, fresh);
      this.showCounts(list);
      d.error = "";
      d.counted += 1;
      this.replay(since);
      this.rememberShape();
      this.freshen((f) => answered(f, COUNTS));
      this.host.redraw();
    } finally {
      since.release();
    }
  }

  /**
   * A new search or window: what the page showed goes, each column shown reads its first page under
   * it, and after a pause for the next key, its counts are read. A -> B -> A is three searches,
   * even though the first and last are alike, and the window chosen again is a new one: the page
   * reads it again rather than leaving its columns empty, as the Dioxus page did.
   */
  async filter(query: string, days: number, wait = ""): Promise<void> {
    const { d } = this;
    d.query = query;
    d.wait = wait;
    d.days = days;
    d.generation += 1;
    d.view += 1;
    d.tasks = [];
    d.fromDevice = [];
    d.cursors.clear();
    d.pendingMore = [];
    // The columns this device drew went with their tasks.
    const drawnParts = d.drawn.map(columnPart);
    this.freshen((f) => left(f, drawnParts));
    d.drawn = [];
    const view = d.view;
    this.host.redraw();
    // A column reads again when its search or window changes, and when the board is refreshed: the
    // same window chosen again changes neither, and its tasks are gone.
    this.host.refresh();
    // Typing on: a later key's search reads in this one's place. The new view has no rows yet,
    // so its counts retry just like its first column pages, even after the initial check ended.
    if ((await this.typing.rest()) && d.view === view) {
      const counted = d.counted;
      this.uncounted = counted;
      await this.untilAnswered(
        () => d.view === view && d.counted === counted,
        async () => {
          if (d.counted !== counted) return true;
          await this.counts();
          return d.counted !== counted;
        },
      );
    }
  }

  /** A column asks for a page: its first, or the next (`more`). */
  async column(status: string, more: boolean, query: string, days: number): Promise<void> {
    const { d } = this;
    // A live refresh resets the first page's cursor, and a column drawn from this device has only
    // the device's until it is checked. Keep a Load more click behind that read instead of
    // cancelling it and asking for the first page again.
    if (more && (!d.cursors.has(status) || d.drawn.includes(status))) {
      if (!d.pendingMore.includes(status)) d.pendingMore.push(status);
      return;
    }
    // Before the board's version, nothing is read; once checked at this generation, the column is
    // read already.
    if (d.waiting || (!more && d.checked.get(status) === d.generation)) return;
    const view = d.view;
    const version = (d.requests.get(status) ?? 0) + 1;
    d.requests.set(status, version);
    let resting = this.columns.get(status);
    if (resting === undefined) {
      resting = new Debounce(TYPING_MS, this.signal);
      this.columns.set(status, resting);
    }
    // A later ask of the column reads in this one's place.
    if (!(await resting.rest())) return;
    const read = () => this.readColumn(status, more, query, days, view, version);
    // A page a click asked for is read once. A first page that failed is read again by itself
    // until it answers, or a newer read of the column or another view replaces it.
    if (more) await read();
    else {
      await this.untilAnswered(() => d.view === view && d.requests.get(status) === version, read);
    }
  }

  /**
   * Check a column drawn from this device once the board's version is read: at once, not after the
   * 250 ms a column waits, and marked so that its own read at this generation does not ask again.
   * Unanswered, the column reads itself when it is next asked to, and `check` asks again.
   */
  async checkColumn(status: string): Promise<void> {
    const { d } = this;
    d.checked.set(status, d.generation);
    const view = d.view;
    const version = (d.requests.get(status) ?? 0) + 1;
    d.requests.set(status, version);
    if (!(await this.readColumn(status, false, d.query, d.days, view, version))) {
      d.checked.delete(status);
    }
  }

  /** The rows the board draws in `status`'s column: its tasks the stages place there. */
  shownRows(status: string): number {
    const { d } = this;
    return d.tasks.filter(
      (task) =>
        status === "waiting" ||
        ((isTask(task) || d.query !== "") && waitingStage(d.work, task, d.connected) === status),
    ).length;
  }

  /**
   * Read a page of `status`'s column, unless a later read of it or another view supersedes it
   * (`view`, `version`: the read's). Whether the page shows the server's answer. A first page read
   * again is as long as the rows the column shows, when they are more than a page (the server's
   * `rows`): it replaces them with the same rows as they stand now, so the pages Load more brought
   * stay, the reader's place among them with them, and the next page starts after them. Such a read
   * is no first page this device keeps: it names the tag of the column's last long answer, which
   * the page holds (`long`), and a `304` is that answer, so a refresh that changed nothing in the
   * column transfers no body. While a search is active the feed's new tasks are not added to the
   * column (`merge`'s `task` case), so the refresh reads as many rows as it showed before a filing
   * that matches the search: the column stands one row short of its count until Load more.
   */
  async readColumn(
    status: string,
    more: boolean,
    query: string,
    days: number,
    view: number,
    version: number,
  ): Promise<boolean> {
    const { d } = this;
    const current = () =>
      !this.signal.aborted && d.view === view && d.requests.get(status) === version;
    if (!current()) return false;
    const part = columnPart(status);
    const activity = d.connected ? 1 : 0;
    const rows = more ? 0 : this.shownRows(status);
    const long = rows > PAGE;
    const kept = !more && !long && d.wait === "" && keptView(query, days, activity);
    const copy = kept ? d.copies.get(part) : long ? d.long.get(part) : undefined;
    const cursor = more ? (d.cursors.get(status) ?? null) : null;
    const since = new ReadCursor(d.journal);
    try {
      const after = cursor === null ? "" : `&cursor=${encodeURIComponent(cursor)}`;
      const length = long ? `&rows=${rows}` : "";
      const path = `/api/task-board?view=waiting${d.wait ? `&wait=${d.wait}` : ""}&q=${encodeURIComponent(query)}${status === "waiting" ? "" : `&status=${status}`}&days=${days}${after}${length}&activity=${activity}&work=0`;
      let read: Awaited<ReturnType<LiveBoard["readListing"]>> | null = null;
      let why = "";
      try {
        read = await this.readListing(path, copy);
      } catch (error) {
        why = said(error);
      }
      if (!current()) return false;
      if (read === null) {
        // A first page is tried again by itself (`column`, `check`), and says so.
        this.unanswered(part, more ? why : failedRead(why, true));
        return false;
      }
      const [list, fresh] = read;
      if (kept) this.keep(part, list, fresh);
      else if (long && fresh !== null) d.long.set(part, { etag: fresh.etag, list });
      d.fromDevice = d.fromDevice.filter((slug) => !list.tasks.some((task) => task.slug === slug));
      holdPlace(readerRows());
      const hasNext = this.showColumn(status, more, list);
      this.replay(since);
      markTasksReady();
      if (!more) {
        d.drawn = d.drawn.filter((one) => one !== status);
        // Every column drawn from the device has answered: a task it drew that none listed is
        // gone from the board, whichever stage the machines' work places it in now.
        if (d.drawn.length === 0) {
          const left = d.fromDevice;
          d.fromDevice = [];
          d.tasks = d.tasks.filter((task) => !left.includes(task.slug));
        }
        this.rememberShape();
      }
      // What a failed read said goes once none fails any more; drawn below either way.
      this.answeredRead(part);
      const queued = !more && hasNext && d.pendingMore.includes(status);
      d.pendingMore = d.pendingMore.filter((one) => one !== status);
      this.host.redraw();
      if (queued) void this.column(status, true, query, days);
      return true;
    } finally {
      since.release();
    }
  }

  /**
   * Once the board's version is read: the counts, the machines' work and each column still drawn
   * from this device, all at once. Settles when each has answered or failed once; one that failed
   * is asked again until it answers, by its read or (the machines' work) by the feed, or the next
   * check starts.
   */
  async check(): Promise<void> {
    const { d } = this;
    d.round += 1;
    const round = d.round;
    const current = () => !this.signal.aborted && d.round === round;
    const { worked, counted } = d;
    this.uncounted = counted;
    const firsts = d.drawn.map((status) =>
      this.untilAnswered(current, async () => {
        const answeredNow = () => !d.drawn.includes(status);
        if (answeredNow()) return true;
        await this.checkColumn(status);
        return answeredNow();
      }),
    );
    firsts.push(
      this.untilAnswered(current, async () => {
        if (d.worked !== worked) return true;
        await this.machineWork();
        return d.worked !== worked;
      }),
      this.untilAnswered(current, async () => {
        if (d.counted !== counted) return true;
        await this.counts();
        return d.counted !== counted;
      }),
    );
    await Promise.all(firsts);
  }

  /** Try `read` until it says it was answered, waiting longer after each failure (`tryAgain`),
   * while `current`; settles once the first try is over. */
  async untilAnswered(current: () => boolean, read: () => Promise<boolean>): Promise<void> {
    let answeredYet = await read();
    const again = async () => {
      answeredYet = await read();
    };
    void tryAgain(() => !answeredYet && current(), again, this.signal);
  }

  /** The board's version was read: each column may read, and what the page drew is checked; then
   * each column shown reads its first page again. */
  async read(): Promise<void> {
    await this.kept;
    if (this.signal.aborted) return;
    const { d } = this;
    d.generation += 1;
    d.view += 1;
    d.waiting = false;
    await this.check();
    if (this.signal.aborted) return;
    this.host.refresh();
  }

  /** The feed opened (`stale` false) or was lost. */
  setStale(stale: boolean): void {
    const { d } = this;
    this.host.stale(stale);
    if (stale) {
      d.connected = false;
      d.generation += 1;
      d.view += 1;
      d.cursors.clear();
      d.pendingMore = [];
      if (d.lifecycleCounts.length > 0) d.counts = d.lifecycleCounts;
      this.host.refresh();
      this.host.redraw();
      return;
    }
    if (d.connected) return;
    // The stages are placed by the machines' activity again: the counts read again first.
    d.connected = true;
    d.generation += 1;
    d.view += 1;
    d.cursors.clear();
    d.pendingMore = [];
    this.host.redraw();
    void this.counts().then(() => {
      if (!this.signal.aborted) this.host.refresh();
    });
  }

  /** One feed event: applied, and a refresh asked for when it moves a count or a column. False
   * for one the page cannot read. */
  heard(value: unknown): boolean {
    const { d } = this;
    const worked = d.worked;
    const refresh = this.apply(value);
    if (refresh === null) return false;
    // The event carries the machines' whole work: it answers for the work's read, and what that
    // read's failure said goes with it, as nothing tries it again.
    if (d.worked !== worked && this.answeredRead(WORK)) this.host.redraw();
    if (refresh && !this.pending) {
      this.pending = true;
      void this.refreshing();
    }
    return true;
  }

  /** Read the counts again after a pause, until no event asked for another read meanwhile; then
   * each column shown reads its first page again. */
  private async refreshing(): Promise<void> {
    const { d } = this;
    for (;;) {
      if (!(await this.refreshes.rest())) return;
      const generation = d.generation;
      await this.counts();
      if (this.signal.aborted) return;
      if (generation === d.generation) {
        this.host.refresh();
        this.pending = false;
        return;
      }
    }
  }

  /**
   * Apply feed event `value`, kept for the reads still out. Whether it asks for a refresh; null
   * for one the page cannot read, which changes nothing.
   */
  apply(value: unknown): boolean | null {
    const { d } = this;
    const changes = readChanges(value, d.query !== "");
    if (changes === null) return null;
    const { journal } = d;
    journal.version += 1;
    if (journal.readers.size > 0) journal.events.push([journal.version, value]);
    const [refresh, changed] = this.merge(changes);
    if (refresh) {
      // Every change affecting a query's complete totals invalidates older pages and counts. The
      // pending refresh retries if another such event arrives while its read is held. Only the
      // live event does: its replay onto an answer read meanwhile asks for nothing.
      d.generation += 1;
      d.cursors.clear();
      d.pendingMore = [];
    }
    if (changed || refresh) {
      holdPlace(readerRows());
      // Drawn at once, within the event, so the hold starts from the change in the page. React
      // would leave the draw to its scheduler, a later task, and a slow or loaded renderer runs
      // frames before such a task: the hold's four quiet frames could pass first, and a row filed
      // above the reader land with nothing holding their place.
      flushSync(() => this.host.redraw());
    }
    return refresh;
  }

  /** The events heard since `cursor`'s read began, applied again to what it answered. */
  replay(cursor: ReadCursor): void {
    for (const value of cursor.events()) {
      const changes = readChanges(value, this.d.query !== "");
      if (changes !== null) this.merge(changes);
    }
  }

  /** `changes` made to the data: whether they ask for a refresh, and whether any changed it. */
  merge(changes: Change[]): [boolean, boolean] {
    const { d } = this;
    const searching = d.query !== "" || d.wait !== "";
    let refresh = false;
    let changed = false;
    for (const change of changes) {
      switch (change.region) {
        case "machine-work":
          // The listings leave the work out (`work=0`): only work that puts a task in another
          // stage changes a count or a column. The machine sends its heartbeat every second,
          // which moves none.
          refresh ||= !placesAlike(d.work, change.work);
          // Nor does it change what the page draws: the board keeps the work it drew, and draws
          // nothing again.
          if (!drawsAlike(d.work, change.work)) {
            d.work = change.work;
            changed = true;
          }
          d.worked += 1;
          // The feed's first word of the machines' work, when it beats the read.
          if (d.worked === 1) this.rememberShape();
          break;
        case "task": {
          const item = { ...change.change };
          d.fromDevice = d.fromDevice.filter((slug) => slug !== item.key);
          const task = item.value;
          if (
            task !== null &&
            task !== undefined &&
            !searching &&
            (task.status === "done" || task.status === "cancelled") &&
            task.updated < this.now() / 1000 - d.days * 86_400
          ) {
            item.removed = true;
          }
          if (searching) refresh = true;
          else {
            d.tasks = upsert(d.tasks, item);
            changed = true;
          }
          break;
        }
        case "task-fields": {
          const { key, fields } = change.change;
          const at = d.tasks.findIndex((task) => task.slug === key);
          const task = d.tasks[at];
          if (task !== undefined) {
            d.tasks = d.tasks.map((one, index) => (index === at ? merge(task, fields) : one));
            changed = true;
          }
          refresh ||=
            searching ||
            (fields.status !== undefined && fields.status !== null) ||
            (fields.parent !== undefined && fields.parent !== null) ||
            fields.waits_on != null ||
            fields.from != null;
          break;
        }
        case "session": {
          refresh = true;
          // A holder's health: its roles on every card say it.
          const [moved, tasks] = holderHealth(d.tasks, change.key, change.health);
          if (moved) {
            d.tasks = tasks;
            changed = true;
          }
          break;
        }
        case "message":
          refresh ||= searching;
          break;
        case "task-signoff":
          if (change.slugs === null) refresh = true;
          else {
            d.signoff = change.slugs;
            changed = true;
          }
          break;
        case "counts":
          // Legacy feed totals describe lifecycle. Ask for the complete stage projection under
          // this client's search and window, never infer totals from loaded pages.
          refresh = true;
          break;
        case "tasks-status": {
          refresh ||= change.available === true;
          const notice = statusNotice(change.available, change.slow);
          if (notice !== d.error) {
            d.error = notice;
            changed = true;
          }
          break;
        }
      }
    }
    return [refresh, changed];
  }
}

/** The task board's page: its heading, then the board, its ghost before the first read, or why
 * that read failed, under its notices. */
export function TasksPage({ scope }: { scope: string }) {
  const frame = useFrame();
  const [, redraw] = useReducer((count: number) => count + 1, 0);
  const [refreshed, setRefreshed] = useState(0);
  const [stale, setStale] = useState(false);
  const [freshness, setFreshness] = useState<Freshness>(FRESH);
  const live = useRef<LiveBoard | null>(null);
  useEffect(() => {
    const controller = new AbortController();
    const board = new LiveBoard(
      scope,
      {
        redraw,
        refresh: () => setRefreshed((count) => count + 1),
        freshness: setFreshness,
        stale: setStale,
      },
      controller.signal,
    );
    live.current = board;
    setFreshness(FRESH);
    redraw();
    void board.start(frame);
    return () => controller.abort();
  }, [scope, frame]);
  return (
    <TasksView
      scope={scope}
      board={live.current}
      stale={stale}
      freshness={freshness}
      refreshed={refreshed}
    />
  );
}

export interface TasksViewProps {
  scope: string;
  /** The board kept live, once the page started it. */
  board: LiveBoard | null;
  stale: boolean;
  freshness: Freshness;
  refreshed: number;
}

/** The task board's page as `board` stands. */
export function TasksView({ scope, board, stale, freshness, refreshed }: TasksViewProps) {
  const d = board?.d ?? null;
  const device = shown(freshness);
  const ready = d?.ready ?? false;
  const error = d?.error ?? "";
  // A first read that failed, nothing drawn yet (a page drawn from this device is ready), says why
  // in place of the ghost, once: the notices leave it out while it stands.
  const unread = !ready && error !== "";
  return (
    <main
      data-ready="board"
      data-feed={stale || device ? "stale" : "live"}
      data-cache={device ? "device" : undefined}
      aria-busy={!ready && !unread}
    >
      {!ready && !unread && (
        <span className="status-text" role="status">
          Loading tasks…
        </span>
      )}
      <div className="page-head">
        <h1>{PAGE_NAMES.BOARD}</h1>
      </div>
      {board !== null && d !== null && unread ? (
        <UnreadBoard
          why={error}
          retry={() => {
            board.retry.current += 1;
            void board.counts();
          }}
        />
      ) : board !== null && d !== null && ready ? (
        // Each column reserves its own rows while they load: the board's controls show at once.
        <TaskBoard
          scope={scope}
          tasks={d.tasks}
          counts={d.counts}
          work={d.work}
          connected={!stale}
          waiting={d.projection}
          orchestrators={d.orchestrators}
          unstarted={d.unstarted}
          refreshed={refreshed}
          onFilter={board.search}
          onLoad={board.load}
        />
      ) : (
        <GhostBoard {...recallGhost(load(SHAPE))} />
      )}
      <PageNotices reconnecting={stale} error={unread ? "" : error} device={device} />
    </main>
  );
}
