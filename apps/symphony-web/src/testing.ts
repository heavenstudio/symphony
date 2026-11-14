// What the unit tests share: the cases crates/symphony-api writes (wire.cases.json, `just
// client-wire`), a browser's storage held in memory, and a server whose answers the test gives.
// The tests run in Node, without a DOM: markup is drawn with react-dom/server, and the page's
// globals are stubbed where a test needs one.

import { expect, vi } from "vitest";
import type { Block, ProjectView, View, Viewer } from "./wire";
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
  fileAddress: [string, string][];
  /** A viewer, a file, its caption and poster, and the element's markup (null for none). */
  elementHtml: [Viewer, string, string, string | null, string | null][];
  encoded: [View, string][];
  /** `blocks` holds every kind of block, span and media, and every word they name. */
  samples: { queue: unknown; emptyQueue: unknown; board: unknown; blocks: Block[] } & Record<
    DocumentSample,
    unknown
  >;
  /** The Rust each case's function carries, whole: no test reads it, `just client-lint` does. */
  carried: { rust: string; typescript: string; source: string[] }[];
};

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

/** One request the stubbed server holds: its address, its body as JSON, and how to answer it. */
export interface Asked {
  url: string;
  body: unknown;
  answer: (response: Response) => void;
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
        asked.push({ url, body, answer });
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
