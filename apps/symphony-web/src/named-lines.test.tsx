// The person at the machine drawn by the name they gave themselves, on every page that draws a line
// of theirs or names who reads it: a task's thread, a card's history, the header and the Settings
// page's link, each from the project view the server inlined (`ProjectView.you`, `.machine`). The
// rest of their tests, with no name given, draw `owner` as before.

import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Block, ProjectView, TaskEntry } from "./wire";

const view = vi.hoisted(() => ({ current: { name: "", register: "", nav: [], entrypoints: [] } }));
vi.mock(import("./project"), async (original) => {
  const real = await original();
  return {
    ...real,
    project: () => view.current as ProjectView,
    writer: (name: string) => real.drawnName(name, (view.current as ProjectView).machine),
  };
});
// What waits to come near the screen is drawn as its content alone.
vi.mock(import("./viewport"), async (original) => {
  const real = await original();
  const { createElement, Fragment } = await import("react");
  return {
    ...real,
    NearViewport: (props: { children?: unknown }) =>
      createElement(Fragment, null, props.children as never),
  };
});

const { BlocksView } = await import("./blocks");
const { You } = await import("./frame");
const { TaskMessage } = await import("./task-page");

const stefano = { name: "stefano", shown: "Stefano" };
const NAMED = { name: "", register: "", nav: [], entrypoints: [], you: stefano, machine: stefano };
const UNNAMED = {
  name: "",
  register: "",
  nav: [],
  entrypoints: [],
  you: { name: "owner", shown: "owner" },
};

const paragraph = (text: string): Block => ({ type: "paragraph", spans: [{ type: "text", text }] });

function message(sender: string, person: boolean, kind = "message"): TaskEntry {
  return {
    message: { id: 1, slug: "a/b", sender, recipient: null, kind, text: "Said", ts: 1, person },
    stamp: "2026-11-24",
    blocks: [],
  };
}

beforeEach(() => {
  vi.stubGlobal("document", { getElementById: () => null });
});

afterEach(() => {
  vi.unstubAllGlobals();
  view.current = { name: "", register: "", nav: [], entrypoints: [] };
});

describe("a task's thread", () => {
  it("draws the person at the machine's lines by their shown name, under owner and their own", () => {
    view.current = NAMED;
    expect(renderToStaticMarkup(<TaskMessage entry={message("owner", false)} />)).toContain(
      '<span class="from" title="owner">Stefano</span>',
    );
    expect(renderToStaticMarkup(<TaskMessage entry={message("stefano", true)} />)).toContain(
      '<span class="from" title="stefano">Stefano</span>',
    );
    // A person a board named, and an agent, as written.
    expect(renderToStaticMarkup(<TaskMessage entry={message("ana", true)} />)).toContain(
      '<span class="from">ana</span>',
    );
    expect(renderToStaticMarkup(<TaskMessage entry={message("amber", false)} />)).toContain(
      '<span class="from">amber</span>',
    );
  });

  it("draws owner as before while the person at the machine gave no name", () => {
    view.current = UNNAMED;
    expect(renderToStaticMarkup(<TaskMessage entry={message("owner", false)} />)).toContain(
      '<span class="from">owner</span>',
    );
  });
});

describe("a card's history", () => {
  const owners = {
    who: "owner" as const,
    timestamp: "2026-11-24 09:00",
    verdict: "approved",
    blocks: [paragraph("Warm.")],
  };

  it("names the owner's line, written before a name was given, as the person at the machine", () => {
    view.current = NAMED;
    expect(
      renderToStaticMarkup(<BlocksView blocks={[{ type: "thread", entries: [owners] }]} />),
    ).toContain(
      '<time>2026-11-24 09:00</time> <span class="from" title="owner">Stefano</span><b class="entry-verdict">approved</b>',
    );
  });

  it("names no one on the owner's line while no name is given", () => {
    view.current = UNNAMED;
    expect(
      renderToStaticMarkup(<BlocksView blocks={[{ type: "thread", entries: [owners] }]} />),
    ).toContain('<time>2026-11-24 09:00</time> <b class="entry-verdict">approved</b>');
  });
});

describe("the header", () => {
  it("names who reads the page and links to where they write their profile", () => {
    expect(renderToStaticMarkup(<You view={NAMED} />)).toBe(
      '<a class="app-you" href="/people#you" title="stefano" aria-label="Stefano, your profile">Stefano</a>',
    );
    expect(renderToStaticMarkup(<You view={UNNAMED} />)).toBe(
      '<a class="app-you" href="/people#you" aria-label="owner, your profile">owner</a>',
    );
    const ana = { ...NAMED, you: { name: "ana", shown: "Ana Lima" } };
    expect(renderToStaticMarkup(<You view={ana} />)).toContain('href="/people#person-ana"');
    expect(renderToStaticMarkup(<You view={{ ...NAMED, you: null }} />)).toBe("");
  });
});

describe("every other writer a page draws from a stored line or row", () => {
  it("is the person at the machine by their shown name once named, and as written before", async () => {
    const { byline } = await import("./gates-model");
    const { logLine } = await import("./people");
    const { changeLine } = await import("./settings");
    const { address } = await import("./timeline-history");
    const gate = { name: "owner", state: "met" as const, by: "owner", detail: "" };
    const line = { n: 1, at: 1, by: "owner", did: "add", name: "ana", powers: ["read"] };
    const change = { at: "2026-11-24 09:00", by: "stefano", before: "1", after: "2" };
    const sent = { sender: "owner", recipient: "owner" } as Parameters<typeof address>[0];
    const drawn = () => [
      byline(gate, 0)[0],
      logLine(line),
      changeLine(change),
      address(sent),
      address({ ...sent, recipient: null }),
    ];
    view.current = UNNAMED;
    expect(drawn()).toEqual([
      "owner",
      "owner put ana on the board, holding read",
      "2026-11-24 09:00 · stefano: 1 → 2",
      "owner → owner",
      "owner → Everyone on task",
    ]);
    view.current = NAMED;
    expect(drawn()).toEqual([
      "Stefano",
      "Stefano put ana on the board, holding read",
      "2026-11-24 09:00 · Stefano: 1 → 2",
      "Stefano → Stefano",
      "Stefano → Everyone on task",
    ]);
    // An agent and a person a board named are drawn as written.
    expect(byline({ ...gate, by: "amber" }, 0)[0]).toBe("amber");
    expect(logLine({ ...line, by: "ana" })).toBe("ana put ana on the board, holding read");
  });

  it("names who filed a task and to whom a line went", async () => {
    const { aTask, aTaskPage } = await import("./testing");
    const { TaskHead } = await import("./task-page");
    const head = (by: string) =>
      renderToStaticMarkup(
        <TaskHead page={aTaskPage(aTask("a/b", { created_by: by }))} scope="" connected />,
      );
    view.current = UNNAMED;
    expect(head("owner")).toContain('<span class="by">filed by owner</span>');
    view.current = NAMED;
    expect(head("owner")).toContain('<span class="by" title="owner">filed by Stefano</span>');
    expect(head("stefano")).toContain('<span class="by" title="stefano">filed by Stefano</span>');
    expect(head("ana")).toContain('<span class="by">filed by ana</span>');
    const addressed = message("amber", false);
    addressed.message.recipient = "owner";
    expect(renderToStaticMarkup(<TaskMessage entry={addressed} />)).toContain(
      '<span class="from">amber → Stefano</span>',
    );
  });
});
