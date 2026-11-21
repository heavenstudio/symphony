// Who is connected, and what they are working on (`/agents`): the agents page's reads, guarded as
// serde reads them; its rules (who an agent's line names, what it holds, follows and finished, which
// change brings a task in); and the page kept live, a model a test runs without a DOM. The board is
// read first (live.tsx's `keepBoard`, which follows the feed and starts again when it is lost), then
// the page's first page (`/api/agent-page`); each feed event is applied where it lands, and one that
// brings a task in, or the board back, reads the first page again, merged into what is drawn.

import { flushSync } from "react-dom";
import type { FrameRefresh } from "./frame";
import { type Journal, ReadCursor } from "./journal";
import { type BoardRun, keepBoard, type PageBoard, Retry } from "./live";
import { scoped } from "./project";
import { getJson, said } from "./request";
import { whiteSpace } from "./session";
import { field, holdPlace, list as items } from "./shared";
import { amount, calls, costSaid, estimate, isCost, isTokens, total, unpricedSaid } from "./spend";
import { tryAgain } from "./state";
import {
  byPriority,
  holderHealth,
  merge,
  readItemChange,
  readListing,
  readTask,
  readTaskChange,
  textOrder,
} from "./task";
import type {
  Agent,
  AgentRun,
  AgentSaid,
  AgentSlots,
  HeldTask,
  ItemChange,
  MachineObservation,
  MachineWork,
  Role,
  SessionGo,
  Task,
  TaskChange,
  TaskFields,
} from "./wire";
import { plural, statusNotice } from "./words";
import { freshnessWord, NO_WORK, readWork } from "./work";

/** Where the page reads its agents, a page of them at a time. */
export const AGENT_PAGE_PATH = "/api/agent-page";
/** What a read says when its answer is not the page's. */
const UNREADABLE = "The agents page's answer is not readable";

const isText = (value: unknown): value is string => typeof value === "string";
const isNumber = (value: unknown): value is number => typeof value === "number";
/**
 * A number as serde reads one into a `usize`, as far as a double holds it: blocks-read.ts's
 * `isCount`, which this page's chunks do not carry.
 */
const isCount = (value: unknown): value is number =>
  Number.isSafeInteger(value) && (value as number) >= 0;
/** As serde reads an `Option`: absent and `null` are none. */
const absent = (value: unknown) => value === undefined || value === null;
const optionalText = (value: unknown) => absent(value) || isText(value);
const optionalFlag = (value: unknown) => value === undefined || typeof value === "boolean";
/** An `Option` read by `read`, or `undefined` when present and unreadable. */
function optional<T>(value: unknown, read: (value: unknown) => T | null): T | null | undefined {
  if (absent(value)) return null;
  return read(value) ?? undefined;
}

/** The machine probes as the page reads them: the panes probe by either of its names, `panes` first. */
export interface Probes {
  panes: string;
  slots: string;
}

/** The agents as the page holds them. */
export interface AgentsRead {
  work: MachineWork;
  generated: number;
  probes: Probes;
  agents: Agent[];
}

/** A page of the agents (`/api/agent-page`) as the page reads it. */
export interface AgentPageRead {
  view: AgentsRead;
  tasks: Task[];
  total: number;
  next: string | null;
  local: boolean;
}

/** The probes, or none: a server from before the rename names the panes probe `herdr` alone. */
export function readProbes(value: unknown): Probes | null {
  const panes = field(value, "panes");
  const herdr = field(value, "herdr");
  const slots = field(value, "slots");
  if (!optionalText(panes) || !optionalText(herdr) || !isText(slots)) return null;
  const named = isText(panes) ? panes : isText(herdr) ? herdr : null;
  return named === null ? null : { panes: named, slots };
}

function readHeld(value: unknown): HeldTask | null {
  const [slug, role, status] = ["slug", "role", "status"].map((key) => field(value, key));
  return isText(slug) && isText(role) && isText(status) ? { slug, role, status } : null;
}

function readRun(value: unknown): AgentRun | null {
  const [recipe, phase, since] = ["recipe", "phase", "since"].map((key) => field(value, key));
  return isText(recipe) && isText(phase) && (absent(since) || isNumber(since))
    ? { recipe, phase, since: (since as number | undefined) ?? null }
    : null;
}

function readSaid(value: unknown): AgentSaid | null {
  const [slug, ts, text] = ["slug", "ts", "text"].map((key) => field(value, key));
  return isText(slug) && isNumber(ts) && isText(text) ? { slug, ts, text } : null;
}

function readSlots(value: unknown): AgentSlots | null {
  const [open, cap] = ["open", "cap"].map((key) => field(value, key));
  return isCount(open) && isCount(cap) ? { open, cap } : null;
}

function readGo(value: unknown): SessionGo | null {
  const [handle, url, pane] = ["handle", "url", "pane"].map((key) => field(value, key));
  return isText(handle) && optionalText(url) && optionalText(pane)
    ? {
        handle,
        url: (url as string | undefined) ?? null,
        pane: (pane as string | undefined) ?? null,
      }
    : null;
}

const TEXTS = ["handle", "kind", "model", "worktree", "branch", "state", "health"] as const;
const OPTIONAL_TEXTS = [
  "parent",
  "task",
  "role",
  "agent",
  "session_id",
  "pane",
  "callback",
  "session_url",
] as const;

/**
 * An agent as the page reads it, or none for a value serde would refuse: what a server from before
 * a field's time leaves out reads as serde's default (following nothing, no callback, no chat, no
 * slots, no address, not in process, no way in).
 */
export function readAgent(value: unknown): Agent | null {
  if (!TEXTS.every((key) => isText(field(value, key)))) return null;
  if (!OPTIONAL_TEXTS.every((key) => optionalText(field(value, key)))) return null;
  const tasks = field(value, "tasks");
  if (!Array.isArray(tasks)) return null;
  const held = tasks.map(readHeld);
  const lastTool = field(value, "last_tool_call");
  const following = field(value, "following");
  const [chat, inProcess] = [field(value, "chat"), field(value, "in_process")];
  if (held.some((one) => one === null) || !(absent(lastTool) || isNumber(lastTool))) return null;
  if (!(following === undefined || isCount(following))) return null;
  if (!optionalFlag(chat) || !optionalFlag(inProcess)) return null;
  const run = optional(field(value, "state_detail"), readRun);
  const message = optional(field(value, "last_message"), readSaid);
  const slots = optional(field(value, "slots"), readSlots);
  const go = optional(field(value, "go"), readGo);
  if (run === undefined || message === undefined || slots === undefined || go === undefined) {
    return null;
  }
  // A server from before the board kept tokens leaves both out: none taken, none priced.
  const tokens = optional(field(value, "tokens"), (one) => (isTokens(one) ? one : null));
  const cost = optional(field(value, "cost"), (one) => (isCost(one) ? one : null));
  if (tokens === undefined || cost === undefined) return null;
  const text = (key: (typeof OPTIONAL_TEXTS)[number]) =>
    (field(value, key) as string | undefined) ?? null;
  return {
    ...(value as Agent),
    parent: text("parent"),
    task: text("task"),
    role: text("role"),
    agent: text("agent"),
    session_id: text("session_id"),
    pane: text("pane"),
    callback: text("callback"),
    session_url: text("session_url"),
    tasks: held as HeldTask[],
    following: (following as number | undefined) ?? 0,
    chat: chat === true,
    in_process: inProcess === true,
    state_detail: run,
    last_tool_call: (lastTool as number | undefined) ?? null,
    last_message: message,
    slots,
    go,
    tokens,
    cost,
  };
}

/** The agents of a page, or none: the machines' work a server leaves out reads as none known. */
export function readAgents(value: unknown): AgentsRead | null {
  const workValue = field(value, "machine_work");
  const work = workValue === undefined ? NO_WORK : readWork(workValue);
  const generated = field(value, "generated");
  const probes = readProbes(field(value, "probes"));
  const agents = field(value, "agents");
  if (work === null || !isNumber(generated) || probes === null || !Array.isArray(agents)) {
    return null;
  }
  const read = agents.map(readAgent);
  return read.some((agent) => agent === null)
    ? null
    : { work, generated, probes, agents: read as Agent[] };
}

/** A page of the agents, or none for a value without what the page draws. */
export function readAgentPage(value: unknown): AgentPageRead | null {
  const view = readAgents(field(value, "view"));
  const tasks = field(value, "tasks");
  const total = field(value, "total");
  const next = field(value, "next");
  const local = field(value, "local");
  if (view === null || !Array.isArray(tasks) || !isCount(total) || !optionalText(next)) {
    return null;
  }
  if (!optionalFlag(local)) return null;
  const read = tasks.map(readTask);
  if (read.some((task) => task === null)) return null;
  return {
    view,
    tasks: read as Task[],
    total,
    next: (next as string | undefined) ?? null,
    local: local === true,
  };
}

/** An agent the feed changed (`agent`), or none for one it cannot read. */
export function readAgentChange(value: unknown): ItemChange<Agent> | null {
  const key = field(value, "key");
  const removed = field(value, "removed");
  if (!isText(key) || !optionalFlag(removed)) return null;
  const agent = field(value, "value");
  if (absent(agent)) return { key, removed: removed === true };
  const read = readAgent(agent);
  return read === null ? null : { key, value: read, removed: removed === true };
}

/** `items` with `change` made: one removed, replaced in its place or added last. */
export function put<T>(items: T[], change: ItemChange<T>, key: (item: T) => string): T[] {
  if (change.removed === true) return items.filter((item) => key(item) !== change.key);
  const value = change.value;
  if (value === null || value === undefined) return items;
  const at = items.findIndex((item) => key(item) === change.key);
  if (at < 0) return [...items, value];
  const next = [...items];
  next[at] = value;
  return next;
}

/** `text` without the spaces Rust's `str::trim` takes from its ends (`whiteSpace`). */
export function trimmed(text: string): string {
  let start = 0;
  let end = text.length;
  while (start < end && whiteSpace(text.charCodeAt(start))) start += 1;
  while (end > start && whiteSpace(text.charCodeAt(end - 1))) end -= 1;
  return text.slice(start, end);
}

/** A line of the parts known, a dot between two, so an empty one leaves no gap. */
export function meta(parts: string[]): string {
  return parts
    .map(trimmed)
    .filter((part) => part !== "")
    .join(" · ");
}

/** An agent's line: its kind, model, state, branch and its worktree's last folder. */
export function agentLine(agent: Agent): string {
  const worktree = agent.worktree.split("/").pop() ?? agent.worktree;
  return meta([agent.kind, agent.model, agent.state, agent.branch, worktree]);
}

/**
 * What an agent's own sessions took, in a line: its tokens and calls, and their cost estimated at
 * API list prices with the day those were read, never money spent; what had no price named. None
 * for an agent that took none.
 */
export function agentSpent(agent: Agent): string | null {
  const { tokens, cost } = agent;
  if (tokens === null || (tokens.calls === 0 && total(tokens) === 0)) return null;
  const taken = `Tokens: ${amount(total(tokens))} in ${calls(tokens.calls)}`;
  if (cost === null) return taken;
  const priced = `${taken} · ${costSaid(cost)}, ${estimate(cost.prices)}`;
  return cost.unpriced.length > 0 ? `${priced}; no price for ${unpricedSaid(cost)}` : priced;
}

const finishedStatus = (status: string) => status === "done" || status === "cancelled";

/** The disclosure of the open tasks an agent orchestrates, counted in words. */
export function followingLabel(count: number): string {
  return `Following ${count} open ${plural(count, "task")}`;
}

/**
 * The open tasks `handle` orchestrates, as its Following list shows them: the rows the server paged
 * (`rows`), each as the page now knows it, and any other the page shows it orchestrating, in the
 * board's one order (`byPriority`). The server pages them in that order, so the rows Load more
 * brings follow the ones already shown.
 */
export function followedTasks(rows: Task[], tasks: Task[], handle: string): Task[] {
  const known = (row: Task) => tasks.find((task) => task.slug === row.slug) ?? row;
  const followed = rows.map(known).filter((task) => !finishedStatus(task.status));
  for (const task of tasks) {
    const orchestrated = task.roles.some(
      (role) => role.handle === handle && role.role === "orchestrator",
    );
    if (
      !finishedStatus(task.status) &&
      orchestrated &&
      !followed.some((one) => one.slug === task.slug)
    ) {
      followed.push(task);
    }
  }
  return followed.sort(byPriority);
}

/**
 * The machines' work as the page shows it while its feed is lost (`connected` false): every machine
 * disconnected and its counts unknown, its operations as last observed. A broken feed cannot keep
 * a claim of current capacity.
 */
export function connection(work: MachineWork, connected: boolean): MachineWork {
  if (connected) return work;
  return {
    ...work,
    machines: work.machines.map((machine) => ({
      ...machine,
      freshness: "disconnected",
      counts: null,
    })),
  };
}

/**
 * What a machine's row says of its occupancy: its counts, only from a fresh and complete report
 * that has them; else that it is unknown, and how sure its report is.
 */
export function occupancy(machine: MachineObservation): string {
  const counts = machine.counts;
  if (
    machine.complete &&
    machine.freshness === "fresh" &&
    counts !== null &&
    counts !== undefined
  ) {
    return `Builds: ${counts.running_builds} running, ${counts.queued_builds} queued · Tests: ${counts.running_tests} running, ${counts.queued_tests} queued`;
  }
  const observed =
    !machine.complete && machine.freshness === "fresh" ? "incomplete" : machine.freshness;
  return `Occupancy unknown · ${freshnessWord(observed)}`;
}

/** The page's tasks `handle` holds a role on, each with the first role it holds there. */
export function held(tasks: Task[], handle: string): [Task, Role][] {
  const found: [Task, Role][] = [];
  for (const task of tasks) {
    const role = task.roles.find((one) => one.handle === handle);
    if (role !== undefined) found.push([task, role]);
  }
  return found;
}

/** Of what an agent holds, the open tasks it works on, not those it orchestrates. */
export function workingOn(holding: [Task, Role][]): [Task, Role][] {
  return holding.filter(
    ([task, role]) => !finishedStatus(task.status) && role.role !== "orchestrator",
  );
}

/** Of what an agent holds, the finished tasks, the most recently updated first. */
export function finished(holding: [Task, Role][]): Task[] {
  return holding
    .filter(([task]) => finishedStatus(task.status))
    .map(([task]) => task)
    .sort((a, b) => Math.sign(b.updated - a.updated));
}

/**
 * Whether the agents page, which lists no record of a task, shows it once `fields` changed it, by
 * the server's own rule: an agent it shows (`shown`) came to hold it in a role other than
 * orchestrator, or it finished, for the Finished line of each agent that held it.
 */
export function agentsPageTakes(fields: TaskFields, shown: (handle: string) => boolean): boolean {
  const status = fields.status;
  return (
    (status !== undefined && status !== null && finishedStatus(status)) ||
    (fields.roles ?? []).some((role) => role.role !== "orchestrator" && shown(role.handle))
  );
}

/** The agents the Connected list shows, by handle as Rust orders text; and the lost ones. */
export function connectedAgents(agents: Agent[]): Agent[] {
  return agents
    .filter((agent) => agent.health !== "lost" && agent.health !== "left")
    .sort((a, b) => textOrder(a.handle, b.handle));
}

export function lostAgents(agents: Agent[]): Agent[] {
  return agents
    .filter((agent) => agent.health === "lost")
    .sort((a, b) => textOrder(a.handle, b.handle));
}

/** The page's tasks happening now: in progress or in review. */
export function happening(tasks: Task[]): Task[] {
  return tasks.filter((task) => task.status === "in_progress" || task.status === "review");
}

/** One change of a feed event, as the agents page reads it. */
export type Change =
  | { region: "task"; change: ItemChange<Task> }
  | { region: "task-fields"; change: TaskChange }
  | { region: "session"; key: string; health: string | null }
  | { region: "agent"; change: ItemChange<Agent> }
  | { region: "agent-probes"; probes: Probes }
  | { region: "machine-work"; work: MachineWork }
  | { region: "tasks-status"; available: boolean | undefined; slow: boolean };

/**
 * The changes of feed event `value` the page follows, read whole before any is applied; none when
 * one cannot be read, which loses the stream. The probes are read only while the page shows them
 * (`shown`), as the Dioxus page read them.
 */
export function readChanges(value: unknown, shown: boolean): Change[] | null {
  const read: Change[] = [];
  for (const change of items(field(value, "changes"))) {
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
        key: isText(key) ? key : "",
        health: isText(health) ? health : null,
      });
    } else if (region === "agent") {
      const agent = readAgentChange(data);
      if (agent === null) return null;
      read.push({ region, change: agent });
    } else if (region === "agent-probes" && shown) {
      const probes = readProbes(data);
      if (probes === null) return null;
      read.push({ region, probes });
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

/** What the page shows of the agents, and where its reads stand. */
export interface AgentsData {
  /** The agents, once a first page answered. */
  view: AgentsRead | null;
  /** The page was asked from the server's own machine. */
  local: boolean;
  /** The tasks the page's agents hold, work on or finished. */
  tasks: Task[];
  /** How many agents but the lost there are. */
  total: number;
  /** Where the next page of the connected agents starts, and of the lost ones. */
  agentCursor: string | null;
  lostCursor: string | null;
  /** The lost agents were read once. */
  lostLoaded: boolean;
  error: string;
  journal: Journal;
}

/** The page before anything is read. */
export function emptyAgents(): AgentsData {
  return {
    view: null,
    local: false,
    tasks: [],
    total: 0,
    agentCursor: null,
    lostCursor: null,
    lostLoaded: false,
    error: "",
    journal: { version: 0, events: [], readers: new Map() },
  };
}

/** What the live agents change on the page that draws them. */
export interface AgentsHost {
  /** The data changed: draw it again. */
  redraw(): void;
  /** The board was read: the page under it is drawn. */
  board(): void;
  /** Whether the feed is lost. */
  stale(stale: boolean): void;
  /** Why the board could not be read. */
  trouble(why: string): void;
}

/**
 * The agents page as it is kept live: its data, and every read and feed event that changes it.
 * Nothing it does goes on once `signal` ends.
 */
export class LiveAgents {
  readonly d = emptyAgents();
  /** How many times the reader pressed Retry, which the board's loop waits on. */
  readonly retry = new Retry();
  /** A read of the first page again is under way, and another was asked for meanwhile. */
  private reading = false;
  private again = false;
  /** The board reads so far: each starts the first page's reads anew (`first`). */
  private round = 0;
  /** The next page of the connected agents, or of the lost ones: one function for the page's life. */
  readonly more = (lost: boolean): void => {
    void this.next(lost);
  };

  constructor(
    readonly scope: string,
    private readonly host: AgentsHost,
    readonly signal: AbortSignal,
  ) {}

  /** Keep the board live, and the page under it. */
  start(frame: FrameRefresh): Promise<void> {
    return keepBoard(this.scope, null, this.run(frame), this.signal);
  }

  /** What `keepBoard` changes on this page. */
  run(frame: FrameRefresh): BoardRun {
    const board: { current: PageBoard | null } = { current: null };
    return {
      board,
      show: (read) => {
        board.current = read;
        this.host.board();
      },
      setStale: (stale) => this.host.stale(stale),
      setTrouble: (why) => this.host.trouble(why),
      retry: this.retry,
      views: {
        // The page keeps nothing on this device.
        freshen: () => {},
        read: () => this.first(),
        heard: (value) => this.heard(value),
        frame,
      },
    };
  }

  /** A page of the agents at `query`, or what failed. */
  private async read(query: string): Promise<AgentPageRead | string> {
    try {
      const value = await getJson(scoped(this.scope, `${AGENT_PAGE_PATH}${query}`), this.signal);
      return readAgentPage(value) ?? UNREADABLE;
    } catch (failure) {
      return said(failure);
    }
  }

  /**
   * The board was read: read the first page afresh and, while the page has no agents to show (the
   * read failed), again, waiting longer after each failure (`tryAgain`), until it has them, the
   * next board read starts again or the page stops. Settles once the first read is done.
   */
  async first(): Promise<void> {
    this.round += 1;
    const round = this.round;
    await this.load();
    const unanswered = () => this.round === round && this.d.view === null;
    void tryAgain(unanswered, () => this.load(), this.signal);
  }

  /** Read the first page afresh: the agents, the tasks, the counts and both cursors as it says. */
  async load(): Promise<void> {
    const { d } = this;
    const since = new ReadCursor(d.journal);
    try {
      const page = await this.read("");
      if (this.signal.aborted) return;
      if (typeof page === "string") {
        d.error = page;
      } else {
        d.view = page.view;
        d.local = page.local;
        d.tasks = page.tasks;
        d.total = page.total;
        d.agentCursor = page.next;
        d.lostCursor = null;
        d.lostLoaded = false;
        d.error = "";
        this.replay(since);
      }
      this.host.redraw();
    } finally {
      since.release();
    }
  }

  /**
   * Read the page at `query` and merge its answer into the page drawn, its agents and tasks put in
   * or replaced: the next page of the lost agents or of the others (`paged`), whose cursor moves
   * on, or else (`null`) its first page again for what the feed changed, which keeps both cursors
   * and the lost agents opened.
   */
  async merge(query: string, paged: boolean | null): Promise<void> {
    const { d } = this;
    const since = new ReadCursor(d.journal);
    try {
      const page = await this.read(query);
      if (this.signal.aborted) return;
      if (typeof page === "string") {
        d.error = page;
        this.host.redraw();
        return;
      }
      d.local = page.local;
      if (paged === true) {
        d.lostCursor = page.next;
        d.lostLoaded = true;
      } else {
        d.total = page.total;
        if (paged === false) d.agentCursor = page.next;
        else d.error = "";
      }
      const { view } = d;
      if (view !== null) {
        let agents = view.agents;
        for (const agent of page.view.agents) {
          agents = put(agents, { key: agent.handle, value: agent }, (one) => one.handle);
        }
        d.view = { ...view, agents };
        for (const task of page.tasks) {
          d.tasks = put(d.tasks, { key: task.slug, value: task }, (one) => one.slug);
        }
      }
      this.replay(since);
      this.host.redraw();
    } finally {
      since.release();
    }
  }

  /** The next page of the lost agents (the first, the first time), or of the connected ones. */
  async next(lost: boolean): Promise<void> {
    const { d } = this;
    if (lost ? d.lostLoaded && d.lostCursor === null : d.agentCursor === null) return;
    const cursor = lost ? d.lostCursor : d.agentCursor;
    const health = lost ? "&health=lost" : "";
    const after = cursor === null ? "" : `&cursor=${encodeURIComponent(cursor)}`;
    await this.merge(`?${`${health}${after}`.replace(/^&+/, "")}`, lost);
  }

  /** What the feed changed: on a page drawn, its first page merged; else read afresh. */
  refresh(): Promise<void> {
    return this.d.view !== null ? this.merge("", null) : this.load();
  }

  /** Read the first page again, once at a time: one asked for meanwhile runs after it. */
  async reread(): Promise<void> {
    if (this.reading) {
      this.again = true;
      return;
    }
    this.reading = true;
    try {
      do {
        this.again = false;
        await this.refresh();
      } while (this.again && !this.signal.aborted);
    } finally {
      this.reading = false;
    }
  }

  /** One feed event: applied, and the first page read again when it asks. False for one the page
   * cannot read, which changes nothing. */
  heard(value: unknown): boolean {
    const { d } = this;
    const changes = readChanges(value, d.view !== null);
    if (changes === null) return false;
    const { journal } = d;
    journal.version += 1;
    if (journal.readers.size > 0) journal.events.push([journal.version, value]);
    const [refresh, changed] = this.apply(changes);
    if (changed) {
      holdPlace("[id^=message-], [id^=child-], [data-slug]");
      // Drawn at once, within the event, so the hold starts from the change in the page: left to
      // React's scheduler, a later task a loaded renderer runs after its frames, the hold's four
      // quiet frames could pass before it.
      flushSync(() => this.host.redraw());
    }
    if (refresh) void this.reread();
    return true;
  }

  /** The events heard since `cursor`'s read began, applied again to what it answered; the reads
   * they asked for are the live events' own. */
  replay(cursor: ReadCursor): void {
    for (const value of cursor.events()) {
      const changes = readChanges(value, this.d.view !== null);
      if (changes !== null) this.apply(changes);
    }
  }

  /** `changes` made to the data: whether they ask for the first page again, and whether any
   * changed what the page shows. */
  apply(changes: Change[]): [boolean, boolean] {
    const { d } = this;
    let refresh = false;
    let changed = false;
    for (const change of changes) {
      const view = d.view;
      switch (change.region) {
        case "task":
          if (view !== null) {
            d.tasks = put(d.tasks, change.change, (task) => task.slug);
            changed = true;
          }
          break;
        case "task-fields": {
          const { key, fields } = change.change;
          const at = d.tasks.findIndex((task) => task.slug === key);
          const task = d.tasks[at];
          if (task !== undefined) {
            d.tasks = d.tasks.map((one, index) => (index === at ? merge(task, fields) : one));
            changed = true;
          } else if (view !== null) {
            const shown = (handle: string) => view.agents.some((agent) => agent.handle === handle);
            refresh ||= agentsPageTakes(fields, shown);
          }
          break;
        }
        case "session": {
          // A holder's health: its roles on the page's tasks say it.
          const [moved, tasks] = holderHealth(d.tasks, change.key, change.health);
          if (moved) {
            d.tasks = tasks;
            changed = true;
          }
          break;
        }
        case "agent":
          if (view !== null) {
            d.view = { ...view, agents: put(view.agents, change.change, (agent) => agent.handle) };
            changed = true;
          }
          break;
        case "agent-probes":
          if (view !== null) {
            d.view = { ...view, probes: change.probes };
            changed = true;
          }
          break;
        case "machine-work":
          if (view !== null) {
            d.view = { ...view, work: change.work };
            changed = true;
          }
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

/** The open tasks an agent orchestrates, as its Following list reads them, a page at a time. */
export class Followed {
  /** The list is open. */
  open = false;
  rows: Task[] = [];
  /** Where the server's next page starts. */
  cursor: string | null = null;
  /** Why the last read failed; kept until another fails. */
  trouble = "";

  constructor(
    readonly handle: string,
    readonly scope: string,
    private readonly signal: AbortSignal,
    private readonly redraw: () => void,
  ) {}

  /** The list opened (`open`), its first page read as it does; or closed. */
  toggle(open: boolean): void {
    if (open === this.open) return;
    if (open) void this.load(false);
    this.open = open;
    this.redraw();
  }

  /** Read the first page, or the next (`more`); nothing more once the page is gone. */
  async load(more: boolean): Promise<void> {
    const cursor = more ? this.cursor : null;
    const after = cursor === null ? "" : `&cursor=${encodeURIComponent(cursor)}`;
    const path = `/api/task-board?holder=${encodeURIComponent(this.handle)}&role=orchestrator&open=1&older=1${after}`;
    let answer: unknown;
    let why = "";
    try {
      answer = await getJson(scoped(this.scope, path), this.signal);
    } catch (failure) {
      why = said(failure);
    }
    if (this.signal.aborted) return;
    const page = why === "" ? readListing(answer) : null;
    if (page === null) {
      this.trouble = why === "" ? "The task board's answer is not a listing" : why;
    } else {
      this.cursor = page.next ?? null;
      const rows = more ? [...this.rows] : [];
      for (const task of page.tasks) {
        if (!rows.some((row) => row.slug === task.slug)) rows.push(task);
      }
      this.rows = rows;
    }
    this.redraw();
  }
}
