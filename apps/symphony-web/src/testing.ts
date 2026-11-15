// What the unit tests share: the cases crates/symphony-api writes (wire.cases.json, `just
// client-wire`), a browser's storage held in memory, and a server whose answers the test gives.
// The tests run in Node, without a DOM: markup is drawn with react-dom/server, and the page's
// globals are stubbed where a test needs one.

import { isValidElement, type ReactNode } from "react";
import { expect, vi } from "vitest";
import type {
  Block,
  MachineObservation,
  MachineWork,
  ProjectView,
  Role,
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
    reports: unknown;
    emptyReports: unknown;
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
 * One request the stubbed server holds: its address, its body as JSON, how to answer it, and the
 * signal that gives it up.
 */
export interface Asked {
  url: string;
  body: unknown;
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
        asked.push({ url, body, answer, signal: init?.signal ?? null });
      }),
  );
  return asked;
}

/** A JSON reply of `status`, tagged `etag` when one is given. */
export const reply = (status: number, body: unknown, etag?: string) =>
  new Response(JSON.stringify(body), {
    status,
    headers: etag === undefined ? {} : { ETag: etag },
  });

/** Lets the replies given so far reach the code that awaits them, until `asked` holds `count`. */
export async function asking(asked: unknown[], count: number): Promise<void> {
  for (let turn = 0; turn < 50 && asked.length < count; turn += 1) {
    await new Promise((done) => setTimeout(done, 0));
  }
  expect(asked).toHaveLength(count);
}

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
