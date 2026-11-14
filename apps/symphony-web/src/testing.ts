// What the unit tests share: the cases crates/symphony-api writes (wire.cases.json, `just
// client-wire`), and a browser's storage held in memory. The tests run in Node, without a DOM:
// markup is drawn with react-dom/server, and the page's globals are stubbed where a test needs one.

import type { ProjectView } from "./wire";
import cases from "./wire.cases.json";

export interface ViewCase {
  view: ProjectView;
  /** Each route, whether it is a document page, and whether that page is a folder's. */
  routes: [string, boolean, boolean][];
  ownPages: [string, string][];
  readme: string;
}

export const CASES = cases as unknown as {
  here: [string, string, string][];
  strip: [string, string, string][];
  views: ViewCase[];
  queueCategory: [string, string][];
  samples: { queue: unknown; emptyQueue: unknown; board: unknown };
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
