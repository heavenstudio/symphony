import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppearancePanel, ThemeMenu } from "./appearance";
import { PanelApp, reachOf } from "./panel";
import { HOME } from "./route";
import { memoryStorage } from "./testing";

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
  it("answers for the chat's pictures on every page, and for the page's on the board's", () => {
    const chat = "#quick-sheet .quick-message img";
    for (const path of ["", "docs/a.md", "task/x", "queue", "reports"]) {
      expect(reachOf({ ...HOME, path }), path).toEqual({ pictures: chat, views: false });
    }
    for (const path of ["tasks", "agents"]) {
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

describe("the appearance panel", () => {
  it("is drawn only while open, under its button", () => {
    page();
    expect(renderToStaticMarkup(<AppearancePanel />)).toBe("");
    page({ reviewAppearanceOpen: true });
    expect(renderToStaticMarkup(<AppearancePanel />)).toMatch(
      /^<aside id="appearance-panel" role="dialog" aria-label="Appearance" style="--appearance-top:60px"><div class="panel-heading"><h2>Appearance<\/h2>/,
    );
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
