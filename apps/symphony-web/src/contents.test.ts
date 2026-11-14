import { afterEach, describe, expect, it, vi } from "vitest";
import { chosen, cramped, foldContents, watchContents } from "./contents";

afterEach(() => {
  vi.unstubAllGlobals();
});

/** An element as these read it: its attributes, its children by selector and its ancestors. */
class Fake extends EventTarget {
  readonly attributes = new Map<string, string>();
  isConnected = true;
  constructor(
    readonly found: Record<string, Fake[]> = {},
    readonly ancestors: Record<string, Fake | null> = {},
    readonly clientWidth = 0,
    readonly scrollWidth = 0,
  ) {
    super();
  }
  get parentElement(): Fake | null {
    return this.ancestors.parent ?? null;
  }
  querySelectorAll(selector: string): Fake[] {
    const all = this.found[selector] ?? [];
    return selector.includes(":not([data-toc-observed])")
      ? all.filter((one) => !one.attributes.has("data-toc-observed"))
      : all;
  }
  closest(selector: string): Fake | null {
    return this.ancestors[selector] ?? null;
  }
  setAttribute(name: string, value: string) {
    this.attributes.set(name, value);
  }
  removeAttribute(name: string) {
    this.attributes.delete(name);
  }
}

/** A table `width` px wide in a box `box` px wide. */
const table = (box: number, width: number) => new Fake({}, {}, box, width);

/** A document's layout holding `tables`, its contents open (or chosen already). */
function layoutOf(tables: Fake[], chosenAs?: string) {
  const layout = new Fake({ ".doc-body table": tables });
  const toc = new Fake({}, { ".doc-layout": layout });
  toc.setAttribute("open", "");
  if (chosenAs !== undefined) toc.setAttribute("data-toc-chosen", chosenAs);
  return { layout, toc };
}

describe("a document's contents", () => {
  it("are cramped once a drawn table is wider than its box", () => {
    expect(cramped(layoutOf([table(600, 602)]).layout as unknown as Element)).toBe(false);
    expect(cramped(layoutOf([table(600, 603)]).layout as unknown as Element)).toBe(true);
    // A table not drawn yet has no width to need.
    expect(cramped(layoutOf([table(0, 900)]).layout as unknown as Element)).toBe(false);
    expect(cramped(layoutOf([]).layout as unknown as Element)).toBe(false);
  });

  it("fold on their own for a cramped table, unless the reader chose", () => {
    const wide = layoutOf([table(600, 640)]);
    const narrow = layoutOf([table(600, 600)]);
    const loose = new Fake({}, {});
    loose.setAttribute("open", "");
    const root = new Fake({
      ".doc-toc details:not([data-toc-chosen])": [wide.toc, narrow.toc, loose],
    });
    foldContents(root as unknown as ParentNode);
    expect(wide.toc.attributes.get("data-toc-chosen")).toBe("automatic");
    expect(wide.toc.attributes.has("open")).toBe(false);
    expect(narrow.toc.attributes.has("open")).toBe(true);
    expect(narrow.toc.attributes.has("data-toc-chosen")).toBe(false);
    expect(loose.attributes.has("open")).toBe(true);
  });

  it("are the reader's once a summary is clicked", () => {
    const details = new Fake();
    const summary = new Fake({}, { parent: details });
    const inSummary = new Fake({}, { ".doc-toc summary": summary });
    chosen({ target: inSummary } as unknown as Event);
    expect(details.attributes.get("data-toc-chosen")).toBe("manual");
    const elsewhere = new Fake();
    chosen({ target: elsewhere } as unknown as Event);
    chosen({ target: null } as unknown as Event);
    chosen({ target: {} } as unknown as Event);
    expect(elsewhere.attributes.size).toBe(0);
  });
});

describe("the contents' watch", () => {
  /** The page's observers, each kept with what it observes and the callback it calls. */
  function observers() {
    const resizes: { observed: Set<unknown>; call: () => void; disconnected: boolean }[] = [];
    const mutations: { call: () => void; disconnected: boolean }[] = [];
    vi.stubGlobal(
      "ResizeObserver",
      class {
        readonly entry: (typeof resizes)[number];
        constructor(call: () => void) {
          this.entry = { observed: new Set(), call, disconnected: false };
          resizes.push(this.entry);
        }
        observe(target: unknown) {
          this.entry.observed.add(target);
        }
        unobserve(target: unknown) {
          this.entry.observed.delete(target);
        }
        disconnect() {
          this.entry.disconnected = true;
        }
      },
    );
    vi.stubGlobal(
      "MutationObserver",
      class {
        readonly entry: (typeof mutations)[number];
        constructor(call: () => void) {
          this.entry = { call, disconnected: false };
          mutations.push(this.entry);
        }
        observe() {}
        disconnect() {
          this.entry.disconnected = true;
        }
      },
    );
    return { resizes, mutations };
  }

  it("observes each table drawn, folds as they change, and ends with its signal", () => {
    const { resizes, mutations } = observers();
    const first = table(600, 600);
    const { toc, layout } = layoutOf([first]);
    const root = new Fake({
      ".doc-body table:not([data-toc-observed])": [first],
      ".doc-toc details:not([data-toc-chosen])": [toc],
    });
    const page = Object.assign(new EventTarget(), { documentElement: root });
    vi.stubGlobal("document", page);
    const left = new AbortController();
    watchContents(left.signal);
    expect(resizes[0]?.observed).toEqual(new Set([first]));
    expect(first.attributes.get("data-toc-observed")).toBe("true");
    expect(toc.attributes.has("open")).toBe(true);
    // A wider table drawn, the first gone from the page.
    const second = table(600, 700);
    first.isConnected = false;
    root.found[".doc-body table:not([data-toc-observed])"] = [second];
    layout.found[".doc-body table"] = [second];
    mutations[0]?.call();
    expect(resizes[0]?.observed).toEqual(new Set([second]));
    expect(toc.attributes.get("data-toc-chosen")).toBe("automatic");
    expect(toc.attributes.has("open")).toBe(false);
    // A click on a summary is heard while it watches.
    const details = new Fake();
    const summary = new Fake({}, { parent: details });
    const click = new Event("click");
    Object.defineProperty(click, "target", {
      value: new Fake({}, { ".doc-toc summary": summary }),
    });
    page.dispatchEvent(click);
    expect(details.attributes.get("data-toc-chosen")).toBe("manual");
    left.abort();
    expect(resizes[0]?.disconnected).toBe(true);
    expect(mutations[0]?.disconnected).toBe(true);
    expect(second.attributes.has("data-toc-observed")).toBe(false);
    const after = new Fake();
    const later = new Event("click");
    Object.defineProperty(later, "target", {
      value: new Fake({}, { ".doc-toc summary": new Fake({}, { parent: after }) }),
    });
    page.dispatchEvent(later);
    expect(after.attributes.size).toBe(0);
  });

  it("folds as the page scrolls", () => {
    const { resizes } = observers();
    const wide = table(600, 600);
    const { toc, layout } = layoutOf([wide]);
    const root = new Fake({ ".doc-toc details:not([data-toc-chosen])": [toc] });
    const page = Object.assign(new EventTarget(), { documentElement: root });
    vi.stubGlobal("document", page);
    watchContents(new AbortController().signal);
    expect(resizes[0]?.observed.size).toBe(0);
    layout.found[".doc-body table"] = [table(600, 900)];
    page.dispatchEvent(new Event("scroll"));
    expect(toc.attributes.has("open")).toBe(false);
  });

  it("watches nothing once its page left, or where the browser cannot observe sizes", () => {
    const { resizes } = observers();
    const left = new AbortController();
    left.abort();
    watchContents(left.signal);
    vi.stubGlobal("ResizeObserver", undefined);
    watchContents(new AbortController().signal);
    expect(resizes).toHaveLength(0);
  });
});
