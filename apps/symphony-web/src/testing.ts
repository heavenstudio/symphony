// What the unit tests share: the cases crates/symphony-api writes (wire.cases.json, `just
// client-wire`), a browser's storage held in memory, a server whose answers the test gives, and the
// one wait for a condition (`until`).
// The tests run in Node, without a DOM: markup is drawn with react-dom/server, and the page's
// globals are stubbed where a test needs one.

import { isValidElement, type ReactElement, type ReactNode } from "react";
import { expect, vi } from "vitest";
import { BACKSTOP_MS } from "./unit-run";
import type {
  Agent,
  Block,
  MachineObservation,
  MachineWork,
  ProjectView,
  Role,
  SessionGo,
  Task,
  View,
  Viewer,
  WorkKind,
  WorkOperation,
  WorkState,
} from "./wire";
import cases from "./wire.cases.json";

export interface ViewCase {
  view: ProjectView;
  /** Each route, whether it is a document page, and whether that page is a folder's. */
  routes: [string, boolean, boolean][];
  ownPages: [string, string][];
  readme: string;
}

/**
 * The documents' reads, each as the server writes it: with every field, then (`bare…`, `noAsks`)
 * with only those it always sends.
 */
export type DocumentSample =
  | "outline"
  | "bareOutline"
  | "sections"
  | "change"
  | "bareChange"
  | "folder"
  | "bareFolder"
  | "versions"
  | "diff"
  | "asks"
  | "noAsks";

export const CASES = cases as unknown as {
  here: [string, string, string][];
  strip: [string, string, string][];
  views: ViewCase[];
  queueCategory: [string, string][];
  /** A task's slug, and whether it is one the Symphony filed for the owner's verdicts. */
  answersVerdicts: [string, boolean][];
  isHtml: [string, boolean][];
  /** Every pair of slugs, and how the first orders against the second: -1, 0 or 1. */
  slugOrder: [string, string, number][];
  stages: {
    works: Record<string, MachineWork>;
    tasks: Task[];
    /** The work by its name, the task by its place, whether the feed is open, and its stage. */
    placed: [string, number, boolean, string][];
    /** Two works by their names, and whether they place every task alike. */
    alike: [string, string, boolean][];
  };
  /** A word, the grain it names, and that grain's default and longest windows in days. */
  grains: [string, string | null, number | null, number | null][];
  fileAddress: [string, string][];
  /** An address, and whether a page may link it as a session's. */
  webAddress: [string, boolean][];
  /**
   * A way into a session, whether the page is on the server's machine, the control it shows
   * (`{focus|link|screen: target}`, null for none), and whether it goes through another session
   * than `amber`'s.
   */
  sessionControl: [SessionGo, boolean, Record<string, string> | null, boolean][];
  /** A viewer, a file, its caption and poster, and the element's markup (null for none). */
  elementHtml: [Viewer, string, string, string | null, string | null][];
  encoded: [View, string][];
  /** A view's attribute value, and the pairs it holds. */
  decode: [string, [string, string][]][];
  /** `blocks` holds every kind of block, span and media, and every word they name. */
  samples: {
    queue: unknown;
    emptyQueue: unknown;
    board: unknown;
    listing: unknown;
    emptyListing: unknown;
    work: unknown;
    emptyWork: unknown;
    taskChange: unknown;
    bareTaskChange: unknown;
    taskAdded: unknown;
    taskRemoved: unknown;
    agentPage: unknown;
    emptyAgentPage: unknown;
    agentChange: unknown;
    bareAgent: unknown;
    agentProbes: unknown;
    agentFocus: unknown;
    agentScreen: unknown;
    reports: unknown;
    emptyReports: unknown;
    taskTokens: unknown;
    emptyTaskTokens: unknown;
    taskCosts: unknown;
    /** A board's custom workflow read, two reviews and rework in it; a board's with none. */
    workflow: unknown;
    noWorkflow: unknown;
    /** The owner's next actions on a task in review, one unavailable; an apply's answer. */
    nextActions: unknown;
    applied: unknown;
    /** A waiting listing of tasks the workflow holds, one blocked in Build. */
    heldListing: unknown;
    taskGuards: unknown;
    guardRefusal: unknown;
    /** A task's attempts: one stopped, its resume running, one of a provider not held; a view. */
    attempts: unknown;
    attemptLink: unknown;
    blocks: Block[];
  } & Record<DocumentSample, unknown>;
  /** The Rust each case's function carries, whole: no test reads it, `just client-lint` does. */
  carried: { rust: string; typescript: string; source: string[] }[];
};

/** A task as the server lists one: ready, of priority 2, linking and held by nothing; `fields`
 * say otherwise. */
export function aTask(slug: string, fields: Partial<Task> = {}): Task {
  return {
    slug,
    title: slug,
    body: "",
    status: "ready",
    priority: 2,
    parent: null,
    created_by: "owner",
    created: 0,
    updated: 0,
    links: [],
    depends: [],
    roles: [],
    instruction: false,
    ...fields,
  };
}

/** `handle`, an idle worker holding nothing, with nothing known of its session; `fields` say
 * otherwise. */
export function anAgent(handle: string, fields: Partial<Agent> = {}): Agent {
  return {
    handle,
    kind: "worker",
    model: "opus",
    parent: null,
    task: null,
    role: null,
    tasks: [],
    following: 0,
    worktree: "",
    branch: "",
    agent: null,
    session_id: null,
    pane: null,
    callback: null,
    chat: false,
    state: "idle",
    state_detail: null,
    health: "live",
    last_tool_call: null,
    last_message: null,
    slots: null,
    session_url: null,
    in_process: false,
    go: null,
    tokens: null,
    cost: null,
    ...fields,
  };
}

/** A role `handle` holds, live unless `health` says otherwise. */
export function aRole(role: string, handle: string, health = "live"): Role {
  return { handle, role, since: 0, expires: null, health };
}

/** An operation `machine` reports as `id`, for the task `sim/a` unless `fields` say otherwise. */
export function anOperation(
  machine: string | null,
  id: string | null,
  kind: WorkKind,
  state: WorkState,
  fields: Partial<WorkOperation> = {},
): WorkOperation {
  return {
    machine_id: machine,
    operation_id: id,
    parent_id: null,
    handle: "worker-a",
    task: "sim/a",
    checkout: "/opaque/checkout",
    recipe: "fixture",
    kind,
    state,
    resource: "run_slot",
    reason: "",
    phase_since: null,
    ...fields,
  };
}

/** A fresh machine `id` with a complete inventory of `operations`. */
export function aMachine(
  id: string,
  operations: WorkOperation[],
  fields: Partial<MachineObservation> = {},
): MachineObservation {
  return {
    machine_id: id,
    label: id,
    producer_id: "fixture",
    sequence: 1,
    age_ms: 0,
    freshness: "fresh",
    complete: true,
    reason: "",
    counts: { running_builds: 0, queued_builds: 0, running_tests: 0, queued_tests: 0 },
    operations,
    ...fields,
  };
}

/** The work of `machines`, and of nothing no machine vouches for unless `fields` say so. */
export function aWork(
  machines: MachineObservation[],
  fields: Partial<MachineWork> = {},
): MachineWork {
  return { freshness: "fresh", machines, unassigned: [], guidance: {}, ...fields };
}

/** A `Storage` held in memory; `refuse` makes each of its calls throw, as a private window's may. */
export function memoryStorage(refuse = false): Storage {
  const kept = new Map<string, string>();
  const guard = () => {
    if (refuse) throw new Error("The storage refused");
  };
  return {
    get length() {
      return kept.size;
    },
    clear: () => kept.clear(),
    getItem: (key) => {
      guard();
      return kept.get(key) ?? null;
    },
    key: (at) => [...kept.keys()][at] ?? null,
    removeItem: (key) => {
      kept.delete(key);
    },
    setItem: (key, value) => {
      guard();
      kept.set(key, value);
    },
  };
}

/**
 * One request the stubbed server holds: its address, its body as JSON, the tag it names
 * (`If-None-Match`) when it names one, how to answer it, and the signal that gives it up.
 */
export interface Asked {
  url: string;
  body: unknown;
  named: string | null;
  answer: (response: Response) => void;
  signal: AbortSignal | null;
}

/**
 * The server, its answers held until a test gives them: each fetch, in order. The document is one
 * with no project element, so the page's project is the plain one.
 */
export function server(): Asked[] {
  const asked: Asked[] = [];
  vi.stubGlobal("document", { getElementById: () => null });
  vi.stubGlobal(
    "fetch",
    (url: string, init?: RequestInit) =>
      new Promise<Response>((answer) => {
        const body = typeof init?.body === "string" ? JSON.parse(init.body) : undefined;
        const headers = init?.headers as Record<string, string> | undefined;
        const named = headers?.["If-None-Match"] ?? null;
        asked.push({ url, body, named, answer, signal: init?.signal ?? null });
      }),
  );
  return asked;
}

/** One stream of the feed a page opened: where, how often it was closed, and its socket. */
export interface Stream {
  url: string;
  closed: number;
  source: EventTarget;
}

/**
 * The page's WebSocket, on a page at `href`: each stream the feed opens, kept in order, a socket
 * the test makes speak (`say`, and its `open` and `error` events).
 */
export function feeds(href: string): Stream[] {
  const opened: Stream[] = [];
  vi.stubGlobal("location", { href });
  vi.stubGlobal(
    "WebSocket",
    class extends EventTarget {
      private readonly stream: Stream;
      constructor(url: string) {
        super();
        this.stream = { url, closed: 0, source: this };
        opened.push(this.stream);
      }
      close() {
        this.stream.closed += 1;
      }
    },
  );
  return opened;
}

/** The feed's `source` says the event `name` with `data`, as the server's socket does: one message. */
export function say(source: EventTarget | undefined, name: string, data: string): void {
  source?.dispatchEvent(new MessageEvent("message", { data: `${name}\n${data}` }));
}

/** A JSON reply of `status`, tagged `etag` when one is given. */
export const reply = (status: number, body: unknown, etag?: string) =>
  new Response(JSON.stringify(body), {
    status,
    headers: etag === undefined ? {} : { ETag: etag },
  });

/**
 * Waits until `condition` holds, its assertions passing, as `vi.waitFor` does: it looks again every
 * 50 ms of real time, moving Vitest's fake timers on as far between looks where they are on. Its one
 * time is the unit run's backstop (`BACKSTOP_MS`), never `vi.waitFor`'s own second, which a loaded
 * machine reaches and which fails as the condition's assertion would. The test's own backstop, which
 * started first, runs out first, and the run says it as a stall (src/unit-run.ts). Every test waits
 * so: unit-run.test.ts fails on a `vi.waitFor` or `vi.waitUntil` anywhere else.
 */
export function until<T>(condition: () => T | Promise<T>): Promise<T> {
  return vi.waitFor(condition, { timeout: BACKSTOP_MS });
}

/**
 * One turn of the event loop: what the code under test queued before it (a reply, a promise's
 * callbacks) has run. The tests' one wait of this kind, by no length of time.
 */
export function turn(): Promise<void> {
  // biome-ignore lint/style/noRestrictedGlobals: the named helper of the unit tests' turns: no length of time, the next task
  return new Promise((done) => setTimeout(done, 0));
}

/** A change a page's MutationObserver can be told of: an attribute's, or elements added or
 * removed. */
export type Mutation = { attribute: string } | { childList: true };

/**
 * The page's MutationObserver and ResizeObserver, stubbed. A mutation (`changed`) reaches only the
 * observers whose options cover it, as the browser's does, and a resize (`resized`) only those
 * that observe what changed size. `watched` holds each observing mutation observer's options and
 * `sized` what each resize observer observes.
 */
export function observers() {
  const watched = new Map<() => void, MutationObserverInit>();
  const sized = new Map<() => void, unknown[]>();
  vi.stubGlobal(
    "MutationObserver",
    class {
      constructor(private readonly seen: () => void) {}
      observe(_: unknown, options: MutationObserverInit) {
        watched.set(this.seen, options);
      }
      disconnect() {
        watched.delete(this.seen);
      }
    },
  );
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(private readonly seen: () => void) {}
      observe(target: unknown) {
        sized.set(this.seen, [...(sized.get(this.seen) ?? []), target]);
      }
      disconnect() {
        sized.delete(this.seen);
      }
    },
  );
  const covers = (options: MutationObserverInit, change: Mutation) =>
    "childList" in change
      ? options.childList === true
      : options.attributes === true ||
        (options.attributeFilter?.includes(change.attribute) ?? false);
  const changed = (change: Mutation) => {
    for (const [seen, options] of [...watched]) if (covers(options, change)) seen();
  };
  const resized = (target: unknown) => {
    for (const [seen, targets] of [...sized]) if (targets.includes(target)) seen();
  };
  return { watched, sized, changed, resized };
}

/** Lets the replies given so far reach the code that awaits them, until `asked` holds `count`. */
export async function asking(asked: unknown[], count: number): Promise<void> {
  for (let turns = 0; turns < 50 && asked.length < count; turns += 1) {
    await turn();
  }
  expect(asked).toHaveLength(count);
}

/** An element as a test finds it: its type, and its props to read or call. */
export type Found = ReactElement<Record<string, unknown>>;

/**
 * The first element of `node` that `test` takes, itself or one it writes (a component in it is not
 * drawn, but its element is found), depth first: a handler a test calls as the browser would.
 */
export function elementOf(node: ReactNode, test: (element: Found) => boolean): Found | undefined {
  if (Array.isArray(node)) {
    for (const each of node) {
      const found = elementOf(each, test);
      if (found !== undefined) return found;
    }
    return undefined;
  }
  if (!isValidElement<Record<string, unknown>>(node)) return undefined;
  if (test(node)) return node;
  return elementOf(node.props.children as ReactNode, test);
}

/** Calls handler `name` of the first element of `node` that `test` takes, as the browser would;
 * a test whose element or handler is not there fails. */
export function fire(
  node: ReactNode,
  test: (element: Found) => boolean,
  name: string,
  ...event: unknown[]
): void {
  const handler = elementOf(node, test)?.props[name];
  if (typeof handler !== "function") throw new Error(`No element with ${name} was drawn`);
  handler(...event);
}

/** Whether `element` reads `text` and nothing more. */
export const reading = (text: string) => (element: Found) => element.props.children === text;

/**
 * The children of the first element of `node` whose class or tag is `name`, searched through the
 * elements `node` writes itself (a component in it is not drawn): one string where the element
 * holds one text. Markup drawn by react-dom/server cannot tell one text from several side by side,
 * which the browser sets apart.
 */
export function childrenOf(node: ReactNode, name: string): ReactNode {
  if (Array.isArray(node)) {
    for (const each of node) {
      const found = childrenOf(each, name);
      if (found !== undefined) return found;
    }
    return undefined;
  }
  if (!isValidElement<{ className?: string; children?: ReactNode }>(node)) return undefined;
  if (node.props.className === name || node.type === name) return node.props.children;
  return childrenOf(node.props.children, name);
}
