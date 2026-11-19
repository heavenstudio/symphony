import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import SERVED_MARK from "../../symphony-server/src/mark/mark.svg?raw";
import { Frame, openActivity, readMetadata } from "./frame";
import { MetadataReads } from "./state";
import { memoryStorage, turn } from "./testing";

/** The window the frame shares with the shell and the panel: its values, events and storage. */
function page(values: Record<string, unknown> = {}) {
  const window = Object.assign(new EventTarget(), values);
  vi.stubGlobal("window", window);
  vi.stubGlobal("dispatchEvent", window.dispatchEvent.bind(window));
  vi.stubGlobal("localStorage", memoryStorage());
  return window as EventTarget & Record<string, unknown>;
}

const draw = (path: string, scope = "") =>
  renderToStaticMarkup(
    <Frame route={{ path, scope, document: path.startsWith("docs/"), folder: false }}>
      <p>page</p>
    </Frame>,
  );

/** The Symphony's mark, the fermata, as the header draws it. */
const MARK =
  '<svg class="app-mark" viewBox="4.5 9.5 23 14.4" aria-hidden="true">' +
  '<path d="M5 21A11 11 0 0 1 27 21H24.6A8.6 7.2 0 0 0 7.4 21Z"></path>' +
  '<circle cx="16" cy="19.6" r="2.9"></circle></svg>';

beforeEach(() => vi.resetModules());
afterEach(() => vi.unstubAllGlobals());

describe("the frame", () => {
  it("draws the header with the page lit, the page, the dock and the + button", () => {
    page();
    const markup = draw("queue");
    expect(markup).toMatch(/^<div class="app-frame"><header class="app-header" id="top">/);
    expect(markup).toContain('<a href="/queue" class="active" aria-current="page">Queue</a>');
    expect(markup).toContain('<a href="/tasks" class="">Board</a>');
    expect(markup).not.toContain("project-switch");
    expect(markup).not.toContain('class="crumbs"');
    expect(markup).toContain('<div class="frame-page"><p>page</p></div>');
    expect(markup).toContain('<div class="frame-dock" aria-hidden="true"></div>');
    expect(markup).toContain('id="quick-open" type="button" aria-label="Chat or add a task"');
    expect(markup).toContain('aria-expanded="false"');
    expect(markup).not.toContain("Unread activity");
    expect(markup).not.toContain("pending-nav");
  });

  it("opens the header with the Symphony's mark and wordmark, which are no link", () => {
    page();
    expect(draw("queue")).toContain(
      `<header class="app-header" id="top"><span class="app-brand">${MARK}` +
        '<span class="app-wordmark">Symphony</span></span><nav class="app-pages scroll-row"',
    );
  });

  // The pages are a row that scrolls across on a phone (scrollrow.ts), which finds the page shown
  // by its `aria-current`; the bell's name is an element of its own, which a narrow phone hides
  // from the eye alone, so the button keeps its name.
  it("draws the pages as a scrolling row and the bell's name as text a sheet can hide", () => {
    page();
    const markup = draw("queue");
    expect(markup).toContain('<nav class="app-pages scroll-row" aria-label="Pages">');
    expect(markup.match(/aria-current="page"/g)).toHaveLength(1);
    expect(markup).toContain('</svg><span class="bell-label"> Activity</span></button>');
  });

  // The server's own pages draw its file inline (apps/symphony-server/src/mark.rs), so the two
  // headers open alike: the same shapes in the same box, as markup and as React draws it.
  it("draws the mark the server's own pages draw, from the server's file", () => {
    const served = SERVED_MARK.trim()
      .replace(' xmlns="http://www.w3.org/2000/svg"', "")
      .replace(/<(\w+)([^>]*)\/>/g, "<$1$2></$1>");
    expect(served).toBe(MARK);
  });

  it.each([
    [
      "its theme's icon and name in their place",
      { wordmark: "Lantern <Works>", mark: "look/icon.svg" },
      '<img class="app-mark" src="/lantern/look/icon.svg" alt=""/>' +
        '<span class="app-name">Lantern &lt;Works&gt;</span>',
    ],
    [
      "the Symphony's mark beside a theme's name alone",
      { wordmark: "Lantern" },
      `${MARK}<span class="app-name">Lantern</span>`,
    ],
  ])("opens a project's header with %s, under its base", async (_, brand, drawn) => {
    page();
    const view = { name: "Lantern", register: "", base: "/lantern", ...brand };
    vi.stubGlobal("document", { getElementById: () => ({ textContent: JSON.stringify(view) }) });
    const { Frame } = await import("./frame");
    const markup = renderToStaticMarkup(
      <Frame route={{ path: "queue", scope: "", document: false, folder: false }}>
        <p>page</p>
      </Frame>,
    );
    expect(markup).toContain(
      `<header class="app-header" id="top"><span class="app-brand">${drawn}</span>` +
        '<a href="/" data-native="true" class="project-switch">Projects</a>',
    );
    expect(markup).not.toContain("app-wordmark");
  });

  it("keeps the Symphony's own pages global under a checkout, its documents the checkout's", () => {
    page();
    const markup = draw("docs/a/B.md", "proposal~x");
    expect(markup).toContain('<a href="/queue" class="">Queue</a>');
    expect(markup).toContain(
      '<a href="/@proposal~x/docs/" class="active" aria-current="page">Docs</a>',
    );
    expect(markup).toContain(
      '<nav class="crumbs" aria-label="Breadcrumbs"><a href="/@proposal~x/docs/">docs</a> / ' +
        '<a href="/@proposal~x/docs/a/">a</a> / <span>B.md</span></nav>',
    );
  });

  it("marks unread activity, the open sheet and the pending steps", () => {
    page({
      reviewActivitySnapshot: { items: [{ id: "a", answered: false }] },
      reviewPendingSnapshot: { items: [{}] },
      reviewQuickDraft: { open: true },
      reviewActivityOpen: true,
    });
    const markup = draw("");
    expect(markup).toMatch(/^<div class="app-frame quick-expanded">/);
    expect(markup).toContain(
      '<span class="dot ok" role="img" aria-label="Unread activity"></span>',
    );
    expect(markup).toContain('id="bell" type="button" aria-expanded="true"');
    expect(markup).toContain(
      '<nav class="pending-nav frame-pending" aria-label="Cards waiting on you">',
    );
  });

  it("marks nothing unread that this device has seen or that was answered", () => {
    const window = page({
      reviewActivitySnapshot: {
        items: [
          { id: "a", answered: false },
          { id: "b", answered: true },
        ],
      },
      reviewPanelReady: true,
      reviewPendingSnapshot: { items: [{}] },
    });
    localStorage.setItem("docs-review-seen", '["a"]');
    const markup = draw("");
    expect(markup).not.toContain("Unread activity");
    expect(markup).not.toContain("pending-nav");
    expect(window.reviewPanelReady).toBe(true);
  });
});

describe("the frame's reads", () => {
  /** The signals the server's reads were given, by path. */
  const given = new Map<string, AbortSignal | null>();
  /** The server, holding each read until the test answers it by its path. */
  const server = () => {
    const held = new Map<string, (response: Response) => void>();
    given.clear();
    vi.stubGlobal("document", { getElementById: () => null });
    vi.stubGlobal(
      "fetch",
      (url: string, init?: RequestInit) =>
        new Promise<Response>((answer) => {
          held.set(url, answer);
          given.set(url, init?.signal ?? null);
        }),
    );
    const answer = async (activity: unknown, pending: unknown) => {
      for (let turns = 0; turns < 50 && held.size < 2; turns += 1) {
        await turn();
      }
      held.get("/api/activity")?.(new Response(JSON.stringify(activity)));
      held.get("/api/pending")?.(new Response(JSON.stringify(pending)));
    };
    return answer;
  };

  it("leave the activity and the pending on the window for the panel, saying so", async () => {
    const window = page();
    const answer = server();
    const said: string[] = [];
    window.addEventListener("review:frame", () => said.push("review:frame"));
    const read = readMetadata(new MetadataReads(), new AbortController().signal);
    await answer({ items: [{ id: "a" }] }, { items: [] });
    expect(await read).toBe(false);
    expect(window.reviewActivitySnapshot).toEqual({ items: [{ id: "a" }] });
    expect(window.reviewPendingSnapshot).toEqual({ items: [] });
    expect(window.reviewKnownIds).toEqual(["a"]);
    expect(said).toEqual(["review:frame"]);
  });

  it("given up once the page is gone, write nothing over the next page's", async () => {
    const window = page({ reviewActivitySnapshot: "the next page's" });
    const answer = server();
    const said: string[] = [];
    window.addEventListener("review:frame", () => said.push("review:frame"));
    const mount = new AbortController();
    const read = readMetadata(new MetadataReads(), mount.signal);
    expect([...given.values()].map((signal) => signal?.aborted)).toEqual([false, false]);
    // The shell stops the kind before the reads answer.
    mount.abort();
    expect([...given.keys()]).toEqual(["/api/activity", "/api/pending"]);
    expect([...given.values()].map((signal) => signal?.aborted)).toEqual([true, true]);
    await answer({ items: [{ id: "late" }] }, { items: [{}] });
    expect(await read).toBe(false);
    expect(window.reviewActivitySnapshot).toBe("the next page's");
    expect(window.reviewPendingSnapshot).toBeUndefined();
    expect(said).toEqual([]);
  });
});

describe("opening the activity panel", () => {
  it("closes Appearance, opens activity and asks the panel to take the focus", () => {
    const window = page({ reviewAppearanceOpen: true });
    vi.stubGlobal("document", { activeElement: "the bell" });
    const said: string[] = [];
    for (const event of ["review:appearance", "review:activity"]) {
      window.addEventListener(event, () => said.push(event));
    }
    openActivity();
    expect(window.reviewAppearanceOpen).toBe(false);
    expect(window.reviewActivityOpen).toBe(true);
    expect(window.reviewPanelFocus).toBe("activity");
    expect(window.reviewPanelOpener).toBe("the bell");
    expect(said).toEqual(["review:appearance", "review:activity"]);
  });
});
