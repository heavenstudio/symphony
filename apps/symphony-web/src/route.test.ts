import { afterEach, describe, expect, it, vi } from "vitest";
import { PLAIN } from "./project";
import { anchor, currentRoute, engaged, HOME, parseRoute, scrollAnchor, whenFree } from "./route";
import { observers } from "./testing";

const NAV = [{ label: "▤ shelf", title: "Shelf", href: "/shelf", document: "docs/SHELF.md" }];

describe("a route", () => {
  it.each([
    ["/", "", ""],
    ["/queue", "queue", ""],
    ["/app/queue", "queue", ""],
    ["//queue", "queue", ""],
    ["/tasks", "tasks", ""],
    ["/task", "tasks", ""],
    ["/task/app/x", "task/app/x", ""],
    ["/reports", "reports", ""],
    ["/settings", "settings", ""],
    ["/runs", "runs", ""],
    ["/agents", "agents", ""],
    ["/@proposal~x/queue", "queue", "proposal~x"],
    ["/@proposal~x/app/queue", "queue", "proposal~x"],
    ["/@proposal~x/", "", "proposal~x"],
  ])("of %j is %j under the checkout %j", (pathname, path, scope) => {
    expect(parseRoute(PLAIN, pathname)).toEqual({ path, scope, document: false, folder: false });
  });

  it("is a document or folder page of an entrypoint", () => {
    expect(parseRoute(PLAIN, "/docs/A.md")).toEqual({
      path: "docs/A.md",
      scope: "",
      document: true,
      folder: false,
    });
    expect(parseRoute(PLAIN, "/doc/docs/a/")).toEqual({
      path: "docs/a",
      scope: "",
      document: true,
      folder: true,
    });
    expect(parseRoute(PLAIN, "/docs/%41.md")?.path).toBe("docs/A.md");
  });

  it("is a plugin's page by the document it shows", () => {
    expect(parseRoute({ ...PLAIN, nav: NAV }, "/shelf/")).toEqual({
      path: "docs/SHELF.md",
      scope: "",
      document: true,
      folder: false,
    });
  });

  it("is under the project's base", () => {
    expect(parseRoute({ ...PLAIN, base: "/alpha" }, "/alpha/queue")?.path).toBe("queue");
  });

  it.each([["/x.png"], ["/notes"], ["/@scope"], ["/%E0%A4%A"], ["/api/queue"]])(
    "is none that a page answers for %j",
    (pathname) => {
      expect(parseRoute(PLAIN, pathname)).toBeNull();
    },
  );
});

describe("the page's route and its address", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("is the browser's, or the home's where no page answers", () => {
    vi.stubGlobal("location", { pathname: "/queue", hash: "" });
    expect(currentRoute().path).toBe("queue");
    vi.stubGlobal("location", { pathname: "/nowhere", hash: "" });
    expect(currentRoute()).toBe(HOME);
  });

  it("names its fragment decoded, none for one that does not decode", () => {
    vi.stubGlobal("location", { hash: "#a%20b" });
    expect(anchor()).toBe("a b");
    vi.stubGlobal("location", { hash: "#%E0%A4%A" });
    expect(anchor()).toBe("");
  });

  it("brings the element its fragment names into view, opening what holds it", () => {
    class Details {
      open = false;
      parentElement = null;
    }
    const details = new Details();
    const element = { parentElement: details, scrollIntoView: vi.fn() };
    vi.stubGlobal("HTMLDetailsElement", Details);
    vi.stubGlobal("document", { getElementById: (id: string) => (id === "you" ? element : null) });
    vi.stubGlobal("location", { hash: "#you" });
    scrollAnchor();
    expect(details.open).toBe(true);
    expect(element.scrollIntoView).toHaveBeenCalledTimes(1);
    vi.stubGlobal("location", { hash: "#gone" });
    scrollAnchor();
    vi.stubGlobal("location", { hash: "" });
    scrollAnchor();
    expect(element.scrollIntoView).toHaveBeenCalledTimes(1);
  });

  it("has the shell land the element its fragment names, and nothing it does not", () => {
    class Details {
      open = false;
      parentElement = null;
    }
    const details = new Details();
    const element = { parentElement: details, scrollIntoView: vi.fn() };
    const landed: unknown[] = [];
    vi.stubGlobal("HTMLDetailsElement", Details);
    vi.stubGlobal("reviewLand", (target: unknown) => landed.push(target));
    vi.stubGlobal("document", { getElementById: (id: string) => (id === "you" ? element : null) });
    vi.stubGlobal("location", { hash: "#you" });
    scrollAnchor();
    expect(landed).toEqual([element]);
    // The shell opens and scrolls, or leaves the page where the reader put it.
    expect(details.open).toBe(false);
    expect(element.scrollIntoView).not.toHaveBeenCalled();
    for (const hash of ["#gone", ""]) {
      vi.stubGlobal("location", { hash });
      scrollAnchor();
    }
    expect(landed).toEqual([element]);
  });

  it("scrolls on its own where no shell holds the landing", () => {
    const element = { parentElement: null, scrollIntoView: vi.fn() };
    vi.stubGlobal("HTMLDetailsElement", class {});
    vi.stubGlobal("document", { getElementById: () => element });
    vi.stubGlobal("location", { hash: "#you" });
    expect(() => scrollAnchor()).not.toThrow();
    expect(element.scrollIntoView).toHaveBeenCalledTimes(1);
  });

  it("says the reader is engaged by a busy region or a field with the focus", () => {
    const page = (busy: boolean, field: boolean) => ({
      querySelector: () => (busy ? {} : null),
      activeElement: { matches: () => field },
    });
    vi.stubGlobal("document", page(true, false));
    expect(engaged()).toBe(true);
    vi.stubGlobal("document", page(false, true));
    expect(engaged()).toBe(true);
    vi.stubGlobal("document", page(false, false));
    expect(engaged()).toBe(false);
    vi.stubGlobal("document", { querySelector: () => null, activeElement: null });
    expect(engaged()).toBe(false);
  });
});

describe("the wait for the reader to be free", () => {
  afterEach(() => vi.unstubAllGlobals());

  /** A page whose focus moves (`focusout`) and whose elements change (`changed`). */
  function page() {
    const { watched, changed } = observers();
    const doc = Object.assign(new EventTarget(), { documentElement: {} });
    vi.stubGlobal("document", doc);
    return { doc, watched, changed };
  }

  /** Whether `waiting` ended within the turns its events take. */
  async function ended(waiting: Promise<void>): Promise<boolean> {
    let over = false;
    void waiting.then(() => {
      over = true;
    });
    await Promise.resolve();
    await Promise.resolve();
    return over;
  }

  it.each(["data-busy", "data-state", "open"])(
    "ends at a change of the %s attribute anywhere in the page, and of no other",
    async (attribute) => {
      const { watched, changed } = page();
      const waiting = whenFree();
      expect(watched.size).toBe(1);
      changed({ attribute: "class" });
      expect(await ended(waiting)).toBe(false);
      changed({ attribute });
      expect(await ended(waiting)).toBe(true);
      expect(watched.size).toBe(0);
    },
  );

  it("ends as an element is added or removed: a busy region or the focused field gone", async () => {
    const { watched, changed } = page();
    const waiting = whenFree();
    expect([...watched.values()]).toEqual([
      expect.objectContaining({ subtree: true, childList: true }),
    ]);
    changed({ childList: true });
    expect(await ended(waiting)).toBe(true);
    expect(watched.size).toBe(0);
  });

  it("ends as the focus leaves the fields, not as it moves from one to another", async () => {
    const { doc, watched } = page();
    const waiting = whenFree();
    const field = { matches: (selector: string) => selector.includes("textarea") };
    doc.dispatchEvent(Object.assign(new Event("focusout"), { relatedTarget: field }));
    expect(await ended(waiting)).toBe(false);
    const link = { matches: () => false };
    doc.dispatchEvent(Object.assign(new Event("focusout"), { relatedTarget: link }));
    expect(await ended(waiting)).toBe(true);
    expect(watched.size).toBe(0);
    // Into nothing, the page itself.
    const again = whenFree();
    doc.dispatchEvent(new Event("focusout"));
    expect(await ended(again)).toBe(true);
  });

  it("ends once its signal ends, or at once when it had, watching nothing after", async () => {
    const { doc, watched } = page();
    const leaving = new AbortController();
    const waiting = whenFree(leaving.signal);
    leaving.abort();
    expect(await ended(waiting)).toBe(true);
    expect(watched.size).toBe(0);
    const listened = vi.spyOn(doc, "addEventListener");
    expect(await ended(whenFree(leaving.signal))).toBe(true);
    expect(listened).not.toHaveBeenCalled();
    expect(watched.size).toBe(0);
  });
});
