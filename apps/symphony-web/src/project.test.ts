import { afterEach, describe, expect, it, vi } from "vitest";
import {
  crumbs,
  documentsHref,
  entrypoints,
  fileAddress,
  headerLinks,
  here,
  isDocumentRoute,
  isFolderRoute,
  isHtml,
  navForDocument,
  navPage,
  ownPages,
  PLAIN,
  readmeRoute,
  readView,
  scoped,
  stored,
  strip,
} from "./project";
import { CASES } from "./testing";
import type { ProjectView } from "./wire";

const NAV = [
  { label: "▤ shelf", title: "Shelf", href: "/shelf", document: "docs/SHELF.md" },
  { label: "◇ maps", title: "Maps", href: "/maps/", document: "docs/MAPS.md" },
];
const WITH_NAV: ProjectView = { ...PLAIN, nav: NAV };

describe("what crates/symphony-api's ProjectView answers", () => {
  it.each(CASES.here)("puts %j's base before %j", (base, path, want) => {
    expect(here({ ...PLAIN, base }, path)).toBe(want);
  });

  it.each(CASES.strip)("takes %j's base from %j", (base, path, want) => {
    expect(strip({ ...PLAIN, base }, path)).toBe(want);
  });

  it.each(CASES.views.flatMap(({ view, routes }) => routes.map((route) => [view, ...route])))(
    "reads %j's route %j as a document %j, a folder %j",
    (view, path, document, folder) => {
      expect(isDocumentRoute(view as ProjectView, path as string)).toBe(document);
      expect(isFolderRoute(view as ProjectView, path as string)).toBe(folder);
    },
  );

  it.each(CASES.isHtml)("reads %j as an HTML page: %j", (name, html) => {
    expect(isHtml(name)).toBe(html);
  });

  it.each(CASES.fileAddress)("asks for %j's file at %j", (src, address) => {
    expect(fileAddress(src)).toBe(address);
  });

  it.each(CASES.views)(
    "names $view's own pages and its README",
    ({ view, ownPages: own, readme }) => {
      expect(ownPages(view)).toEqual(own);
      expect(readmeRoute(view)).toBe(readme);
    },
  );
});

describe("the entrypoints", () => {
  it("are the convention's one when the project names none", () => {
    expect(entrypoints(PLAIN)).toEqual(["docs"]);
    expect(documentsHref(PLAIN)).toBe("/docs/");
  });

  it("are the project's, the root's folder page being the site's", () => {
    const root = { ...PLAIN, entrypoints: [""] };
    expect(documentsHref(root)).toBe("/");
    expect(readmeRoute(root)).toBe("README.md");
    expect(documentsHref({ ...PLAIN, entrypoints: ["design", "docs"] })).toBe("/design/");
  });
});

describe("a plugin's page", () => {
  it("is the item whose link the path is, slashes aside", () => {
    expect(navPage(NAV, "/shelf")?.document).toBe("docs/SHELF.md");
    expect(navPage(NAV, "maps")?.document).toBe("docs/MAPS.md");
    expect(navPage(NAV, "")).toBeUndefined();
    expect(navPage(NAV, "other")).toBeUndefined();
    expect(navForDocument(NAV, "docs/MAPS.md")?.href).toBe("/maps/");
    expect(navForDocument(NAV, "docs/A.md")).toBeUndefined();
  });
});

describe("the header", () => {
  const lit = (path: string, document: boolean, view = WITH_NAV) =>
    headerLinks(view, path, document)
      .filter((link) => link.active)
      .map((link) => link.label);

  it("lights the page shown, the board for a task's page", () => {
    expect(lit("", false)).toEqual(["Home"]);
    expect(lit("queue", false)).toEqual(["Queue"]);
    expect(lit("docs/A.md", true)).toEqual(["Docs"]);
    expect(lit("tasks", false)).toEqual(["Board"]);
    expect(lit("task/app/x", false)).toEqual(["Board"]);
    expect(lit("reports", false)).toEqual(["Reports"]);
    expect(lit("settings", false)).toEqual(["Settings"]);
    expect(lit("people", false)).toEqual(["People"]);
    expect(lit("runs", false)).toEqual(["Runs"]);
    expect(lit("agents", false)).toEqual(["Agents"]);
  });

  it("lights a plugin's page, not the documents', on its document", () => {
    expect(lit("docs/SHELF.md", true)).toEqual(["▤ shelf"]);
  });

  it("lists the Symphony's pages, then the plugins'", () => {
    expect(headerLinks(WITH_NAV, "", false).map((link) => link.href)).toEqual([
      "/",
      "/queue",
      "/docs/",
      "/tasks",
      "/reports",
      "/settings",
      "/people",
      "/runs",
      "/agents",
      "/shelf",
      "/maps/",
    ]);
  });
});

describe("the crumbs", () => {
  it("are each folder of a document's route, the last its page", () => {
    expect(crumbs(PLAIN, "docs/a/B.md")).toEqual(["docs", "a", "B.md"]);
    expect(crumbs(PLAIN, "docs/a/")).toEqual(["docs", "a"]);
  });

  it("are a task's slug alone", () => {
    expect(crumbs(PLAIN, "task/app/x")).toEqual(["app/x"]);
    // No crumb names a part of the slug as a task of its own, a trailing slash none either.
    expect(crumbs(PLAIN, "task/product/polish-sweep/client-source")).toEqual([
      "product/polish-sweep/client-source",
    ]);
    expect(crumbs(PLAIN, "task/solo/")).toEqual(["solo"]);
  });

  it("make a slug only under the task route's own prefix", () => {
    expect(crumbs(PLAIN, "taskforce/notes.md")).toEqual(["taskforce", "notes.md"]);
    expect(crumbs(PLAIN, "docs/workflow/REVIEW.md")).toEqual(["docs", "workflow", "REVIEW.md"]);
  });

  it("are none on a header link's own page or a plugin's", () => {
    expect(crumbs(PLAIN, "")).toEqual([]);
    expect(crumbs(PLAIN, "queue")).toEqual([]);
    expect(crumbs(PLAIN, "docs/")).toEqual([]);
    expect(crumbs(WITH_NAV, "docs/SHELF.md")).toEqual([]);
  });
});

describe("a key kept in the browser", () => {
  it("takes the project's id on the machine's one server", () => {
    expect(stored({ ...PLAIN, base: "/alpha" }, "k")).toBe("alpha:k");
    expect(stored(PLAIN, "k")).toBe("k");
  });
});

describe("a route under a checkout", () => {
  it("names the checkout first", () => {
    expect(scoped("", "/api/queue")).toBe("/api/queue");
    expect(scoped("proposal~x", "/api/queue")).toBe("/@proposal~x/api/queue");
  });
});

describe("the view the server inlined", () => {
  it("is read as it is, its left-out fields their defaults", () => {
    const view = {
      name: "n",
      register: "r",
      nav: NAV,
      entrypoints: ["docs"],
      base: "/a",
      wordmark: "n",
      mark: "look/icon.svg",
    };
    expect(readView(view)).toEqual(view);
    expect(readView({ name: "n", register: "r" })).toEqual({
      name: "n",
      register: "r",
      nav: [],
      entrypoints: [],
    });
  });

  it.each([
    ["nothing", undefined],
    ["text", "x"],
    ["no name", { register: "r" }],
    ["a name of another type", { name: 1, register: "r" }],
    [
      "an item without its document",
      { name: "", register: "", nav: [{ label: "", title: "", href: "" }] },
    ],
    ["a list of other things", { name: "", register: "", entrypoints: [1] }],
    ["a base of another type", { name: "", register: "", base: 1 }],
    ["a guide of another type", { name: "", register: "", guide: false }],
    ["a wordmark of another type", { name: "", register: "", wordmark: 1 }],
    ["a mark of another type", { name: "", register: "", mark: {} }],
  ])("is the plain project for %s", (_, value) => {
    expect(readView(value)).toBe(PLAIN);
  });
});

describe("the page's project", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetModules();
  });

  it("is read once from its element, and its links take its base", async () => {
    const read = vi.fn(() => ({
      textContent: JSON.stringify({ name: "n", register: "", base: "/alpha" }),
    }));
    vi.stubGlobal("document", { getElementById: read });
    const { href, project } = await import("./project");
    expect(href("/queue")).toBe("/alpha/queue");
    expect(project().name).toBe("n");
    expect(read).toHaveBeenCalledTimes(1);
  });

  it("is the plain project where the element does not parse", async () => {
    vi.stubGlobal("document", { getElementById: () => ({ textContent: "{" }) });
    const { project } = await import("./project");
    expect(project()).toEqual(PLAIN);
  });
});
