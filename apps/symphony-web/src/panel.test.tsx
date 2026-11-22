import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  AppearancePanel,
  applyTheme,
  closeAppearance,
  restoreDefault,
  ThemeMenu,
} from "./appearance";
import { ActivityList, PanelApp, PendingBar, reachOf, ToastList } from "./panel";
import type { Toasts } from "./panel-model";
import { HOME } from "./route";
import { elementOf, type Found, fire, memoryStorage } from "./testing";

let storage: Storage;

/** The window the panel shares with the shell and the frame, holding `values`, at `pathname`. */
function page(values: Record<string, unknown> = {}, pathname = "/docs/a.md") {
  const window = Object.assign(new EventTarget(), values);
  vi.stubGlobal("window", window);
  vi.stubGlobal("dispatchEvent", window.dispatchEvent.bind(window));
  vi.stubGlobal("document", { getElementById: () => null });
  vi.stubGlobal("location", { pathname, hash: "", href: `http://localhost${pathname}` });
  vi.stubGlobal("innerHeight", 900);
  return window as EventTarget & Record<string, unknown>;
}

beforeEach(() => {
  storage = memoryStorage();
  vi.stubGlobal("localStorage", storage);
});

afterEach(() => vi.unstubAllGlobals());

const item = (id: string, fields: Record<string, unknown> = {}) => ({
  id,
  kind: "new card",
  slug: id,
  path: "docs/DECISIONS.md",
  href: `/docs/DECISIONS.md#${id}`,
  ts: "2026-11-15 05:13",
  text: `About ${id}.`,
  answered: false,
  ...fields,
});

describe("the panel's media layer", () => {
  it("answers for the chat's pictures on every page, and for every view on a page with no layer", () => {
    const chat = "#quick-sheet .quick-message img";
    const own = [
      HOME,
      { ...HOME, scope: "wt" },
      { ...HOME, path: "docs/a.md", document: true },
      { ...HOME, path: "docs", document: true, folder: true },
      { ...HOME, path: "task/x" },
    ];
    for (const route of own) {
      expect(reachOf(route), route.path).toEqual({ pictures: chat, views: false });
    }
    for (const path of ["tasks", "agents", "queue", "reports", "runs", "settings"]) {
      expect(reachOf({ ...HOME, path }), path).toEqual({
        pictures: `main img, ${chat}`,
        views: true,
      });
    }
  });
});

describe("the panel", () => {
  it("draws the toasts' region and the closed sheet, and nothing waiting", () => {
    page();
    expect(renderToStaticMarkup(<PanelApp />)).toMatch(
      /^<div class="app-frame"><div id="toasts" aria-live="polite"><\/div><aside id="quick-sheet" hidden=""/,
    );
  });

  it("draws the open activity list from the frame's last read, the unread marked", () => {
    storage.setItem("docs-review-seen", JSON.stringify(["b"]));
    page({
      reviewActivityOpen: true,
      reviewActivitySnapshot: { items: [item("a"), item("b"), item("c", { answered: true })] },
      reviewPendingSnapshot: { items: [], docs: [] },
    });
    const markup = renderToStaticMarkup(<PanelApp />);
    expect(markup).toContain(
      '<aside id="activity-panel" aria-label="Recent activity"><div class="panel-heading"><h2>Recent activity</h2>' +
        '<button class="dx-button" data-style="outline" data-size="default" type="button" aria-label="Close activity">×</button></div><ul>' +
        '<li class="unread"><a href="/docs/DECISIONS.md#a">new card · a</a><div class="meta">docs/DECISIONS.md · 2026-11-15 05:13</div><div class="excerpt">About a.</div></li>' +
        '<li class=""><a href="/docs/DECISIONS.md#b">new card · b</a>',
    );
    expect(markup).toContain('<li class=""><a href="/docs/DECISIONS.md#c">');
    expect(markup).not.toContain("No recent activity.");
  });

  it("draws every item unread while what this device kept as seen does not read", () => {
    storage.setItem("docs-review-seen", "{");
    page({
      reviewActivityOpen: true,
      reviewActivitySnapshot: { items: [item("a"), item("b")] },
      reviewPendingSnapshot: { items: [], docs: [] },
    });
    const markup = renderToStaticMarkup(<PanelApp />);
    expect(markup.match(/<li class="unread">/g)).toHaveLength(2);
  });

  it("says an empty list, and a read that failed", () => {
    page({
      reviewActivityOpen: true,
      reviewActivitySnapshot: { items: [] },
      reviewPendingSnapshot: { items: [], docs: [] },
      reviewFrameError: "offline",
    });
    const markup = renderToStaticMarkup(<PanelApp />);
    expect(markup).toContain(
      '<p role="status">Activity could not be refreshed: offline</p><p class="muted">No recent activity.</p><ul></ul>',
    );
  });

  it("draws the pending bar while cards wait, its steps as far as the page allows", () => {
    page({
      reviewActivitySnapshot: { items: [] },
      reviewPendingSnapshot: {
        items: [{ slug: "c1", path: "docs/a.md", href: "/docs/a.md#c1", title: "C1" }],
        docs: [],
      },
    });
    const markup = renderToStaticMarkup(<PanelApp />);
    expect(markup).toContain(
      '<nav class="pending-nav frame-pending" aria-label="Cards waiting on you">' +
        '<button class="dx-button" data-style="outline" data-size="default" type="button" data-pending-action="prev" disabled="" title="Previous pending decision ([)">← prev</button>' +
        '<button class="dx-button" data-style="outline" data-size="default" type="button" data-pending-action="next"',
    );
    expect(markup).toContain('<a href="#top">↑ top</a></nav>');
  });
});

const labelled = (label: string) => (element: Found) => element.props["aria-label"] === label;

describe("the activity list's controls", () => {
  it("close the list, and open an item by its id", () => {
    page();
    const close = vi.fn();
    const opened = vi.fn();
    const list = ActivityList({
      activity: [item("a"), item("b")],
      seen: null,
      trouble: null,
      close,
      opened,
    });
    fire(list, labelled("Close activity"), "onClick");
    expect(close).toHaveBeenCalledOnce();
    fire(list, (element) => element.props.href === "/docs/DECISIONS.md#b", "onClick");
    expect(opened).toHaveBeenCalledWith("b");
  });
});

describe("the toasts", () => {
  it("draw each item up as a link and its dismiss button", () => {
    page();
    const toasts = { shown: [[1, item("a")]] as Toasts["shown"], batches: 1 };
    expect(
      renderToStaticMarkup(<ToastList toasts={toasts} opened={() => {}} dismissed={() => {}} />),
    ).toBe(
      '<div id="toasts" aria-live="polite"><div class="pop"><a href="/docs/DECISIONS.md#a">new card · a</a>' +
        '<button class="dx-button" data-style="outline" data-size="default" type="button" aria-label="Dismiss notification">×</button></div></div>',
    );
  });

  it("open an item by its link, and take one down by its ×", () => {
    page();
    const opened = vi.fn();
    const dismissed = vi.fn();
    const toasts = { shown: [[1, item("a")]] as Toasts["shown"], batches: 1 };
    const list = ToastList({ toasts, opened, dismissed });
    fire(list, (element) => element.type === "a", "onClick");
    expect(opened).toHaveBeenCalledWith("a");
    fire(list, labelled("Dismiss notification"), "onClick");
    expect(dismissed).toHaveBeenCalledWith("a");
  });
});

describe("the pending bar", () => {
  const card = (slug: string) => ({ path: "docs/a.md", slug, href: `/docs/a.md#${slug}` });
  const step = (action: string) => (element: Found) =>
    element.props["data-pending-action"] === action;

  it("goes to the card before and the one after, and nowhere from a step disabled", () => {
    page();
    const go = vi.fn();
    const both = PendingBar({ targets: [card("c1"), card("c3")], go });
    fire(both, step("prev"), "onClick");
    fire(both, step("next"), "onClick");
    expect(go.mock.calls).toEqual([[card("c1")], [card("c3")]]);
    const none = PendingBar({ targets: [null, null], go });
    expect(elementOf(none, step("prev"))?.props.disabled).toBe(true);
    fire(none, step("prev"), "onClick");
    fire(none, step("next"), "onClick");
    expect(go).toHaveBeenCalledTimes(2);
  });
});

describe("the appearance panel", () => {
  it("is drawn only while open, under its button", () => {
    page();
    expect(renderToStaticMarkup(<AppearancePanel />)).toBe("");
    page({ reviewAppearanceOpen: true });
    expect(renderToStaticMarkup(<AppearancePanel />)).toMatch(
      /^<aside id="appearance-panel" role="dialog" aria-label="Appearance" style="--appearance-top:60px"><div class="panel-heading"><h2>Appearance<\/h2>/,
    );
  });

  /** A page whose theme element `install` writes (one that refuses it, `broken`), and whose
   * Appearance button takes the focus. */
  function themed(broken = false) {
    const style = { id: "studio-theme", textContent: "unset" };
    const focused: string[] = [];
    vi.stubGlobal("document", {
      getElementById: (id: string) => {
        if (broken) throw new Error("The page refused its theme");
        return id === "studio-theme" ? style : null;
      },
      documentElement: { setAttribute: () => {} },
      querySelector: (css: string) => ({ focus: () => focused.push(css) }),
    });
    return { style, focused };
  }

  it("applies a theme file's tokens, and says why a file makes none", () => {
    page();
    const { style } = themed();
    const theme = JSON.stringify({ cssVars: { light: { background: "white" } } });
    expect(applyTheme(theme)).toEqual({ applied: true, status: "Theme applied." });
    expect(style.textContent).toContain("--background:white");
    expect(applyTheme("{")).toEqual({ applied: false, status: "The theme is not valid JSON." });
    expect(applyTheme("{}")).toEqual({
      applied: false,
      status: "The theme needs cssVars with theme, light or dark tokens.",
    });
    expect(style.textContent).toContain("--background:white");
  });

  it("puts the default look back, kept on this device, and says why the page refused it", () => {
    page();
    const { style } = themed();
    expect(restoreDefault()).toBe("Default theme restored.");
    expect(style.textContent).toBe("");
    expect(storage.getItem("studio.cssVars")).toBe("");
    themed(true);
    expect(restoreDefault()).toBe("The page refused its theme");
  });

  it("closes, says so to the frame, and gives the focus back to its button", () => {
    const window = page({ reviewAppearanceOpen: true });
    const { focused } = themed();
    const heard = vi.fn();
    window.addEventListener("review:appearance", heard);
    closeAppearance();
    expect(window.reviewAppearanceOpen).toBe(false);
    expect(heard).toHaveBeenCalledOnce();
    expect(focused).toEqual(["#appearance"]);
  });

  it("offers a theme's import and the default theme, and says nothing yet", () => {
    page();
    expect(renderToStaticMarkup(<ThemeMenu />)).toBe(
      '<div class="theme-menu"><p class="muted">This project&#x27;s look, on this device.</p><div class="theme-actions">' +
        '<button class="dx-button" data-style="outline" data-size="default" type="button" data-theme-action="import">Import shadcn theme…</button>' +
        '<button class="dx-button" data-style="outline" data-size="default" type="button" data-theme-action="default">Default theme</button>' +
        '</div><p role="status"></p></div>',
    );
  });
});
