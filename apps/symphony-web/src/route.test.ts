import { afterEach, describe, expect, it, vi } from "vitest";
import { PLAIN } from "./project";
import { anchor, currentRoute, engaged, HOME, parseRoute, scrollAnchor } from "./route";

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
